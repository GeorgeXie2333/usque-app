//! Authenticated Linux privileged-Agent client. The Engine remains unprivileged.

use std::{
    fs, io,
    os::{
        fd::{AsRawFd, OwnedFd},
        unix::{
            fs::{FileTypeExt, MetadataExt},
            net::UnixStream,
        },
    },
    path::Path,
    sync::{Arc, Mutex},
    time::Duration,
};
use usque_ipc::{
    agent_v1::{self, AgentRequest, AgentResponse, agent_request, agent_response},
    decode_frame, encode_frame, unix_fd,
};
use uuid::Uuid;

const SOCKET_PATH: &str = "/run/usque/agent.sock";
const PROTOCOL_VERSION: u32 = 3;

struct Connection {
    socket: UnixStream,
    peer: unix_fd::PeerCredentials,
}
impl Connection {
    fn connect() -> io::Result<Self> {
        for path in [Path::new("/run"), Path::new("/run/usque")] {
            let metadata = fs::symlink_metadata(path)?;
            if !metadata.is_dir() || metadata.uid() != 0 || metadata.mode() & 0o022 != 0 {
                return Err(denied("Agent runtime directory is not trusted"));
            }
        }
        let metadata = fs::symlink_metadata(SOCKET_PATH)?;
        if !metadata.file_type().is_socket() || metadata.uid() != 0 {
            return Err(denied("Agent socket is not root owned"));
        }
        let socket = UnixStream::connect(SOCKET_PATH)?;
        socket.set_read_timeout(Some(Duration::from_secs(30)))?;
        socket.set_write_timeout(Some(Duration::from_secs(5)))?;
        unix_fd::enable_credentials(&socket)?;
        let peer = unix_fd::peer_credentials(&socket)?;
        if peer.uid != 0 {
            return Err(denied("Agent peer is not root"));
        }
        Ok(Self { socket, peer })
    }

    fn exchange(
        &self,
        request: &AgentRequest,
        frame: &[u8],
        descriptor: Option<&OwnedFd>,
    ) -> io::Result<(AgentResponse, Option<OwnedFd>)> {
        unix_fd::send_frame(&self.socket, frame, descriptor)?;
        let response = unix_fd::receive_frame(&self.socket, self.peer)?;
        let message: AgentResponse =
            decode_frame(response.bytes.into()).map_err(io::Error::other)?;
        if message.request_id != request.request_id {
            return Err(denied("Agent response identity mismatch"));
        }
        if message.error.is_some() {
            if message.payload.is_some() || response.descriptor.is_some() {
                return Err(denied("Agent error response has unexpected payload"));
            }
        } else {
            let payload = message
                .payload
                .as_ref()
                .ok_or_else(|| denied("Agent response payload missing"))?;
            let expects_fd = matches!(payload, agent_response::Payload::LinuxTunSession(_));
            if expects_fd != response.descriptor.is_some() {
                return Err(denied("Agent response descriptor count mismatch"));
            }
        }
        Ok((message, response.descriptor))
    }
}
#[derive(Clone)]
pub struct LinuxAgentClient {
    connection: Arc<Mutex<Option<Connection>>>,
    requests: Arc<tokio::sync::Semaphore>,
    connector: Arc<dyn Fn() -> io::Result<Connection> + Send + Sync>,
}

impl LinuxAgentClient {
    #[cfg(test)]
    pub(crate) fn fake_pair() -> (Self, UnixStream) {
        let (client, mut servers) = Self::fake_connections(1);
        (client, servers.remove(0))
    }

    #[cfg(test)]
    pub(crate) fn fake_connections(count: usize) -> (Self, Vec<UnixStream>) {
        let mut connections = std::collections::VecDeque::new();
        let mut servers = Vec::new();
        for _ in 0..count {
            let (client, server) = UnixStream::pair().unwrap();
            unix_fd::enable_credentials(&client).unwrap();
            unix_fd::enable_credentials(&server).unwrap();
            client
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            server
                .set_read_timeout(Some(Duration::from_secs(10)))
                .unwrap();
            let peer = unix_fd::peer_credentials(&client).unwrap();
            connections.push_back(Connection {
                socket: client,
                peer,
            });
            servers.push(server);
        }
        let first = connections.pop_front().unwrap();
        let connections = Mutex::new(connections);
        (
            Self {
                connection: Arc::new(Mutex::new(Some(first))),
                requests: Arc::new(tokio::sync::Semaphore::new(1)),
                connector: Arc::new(move || {
                    connections.lock().unwrap().pop_front().ok_or_else(|| {
                        io::Error::new(io::ErrorKind::NotConnected, "no test Agent connection")
                    })
                }),
            },
            servers,
        )
    }

    pub async fn connect() -> io::Result<Self> {
        let connection = tokio::task::spawn_blocking(Connection::connect)
            .await
            .map_err(|_| io::Error::other("Agent connection worker failed"))??;
        Ok(Self {
            connection: Arc::new(Mutex::new(Some(connection))),
            requests: Arc::new(tokio::sync::Semaphore::new(1)),
            connector: Arc::new(Connection::connect),
        })
    }

    pub async fn reconnect(&self) -> io::Result<()> {
        let permit = Arc::clone(&self.requests)
            .acquire_owned()
            .await
            .map_err(|_| io::Error::other("Agent request queue closed"))?;
        let connection = Arc::clone(&self.connection);
        let connector = Arc::clone(&self.connector);
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            // Retain any existing authenticated connection until its replacement
            // is ready. Requests and reconnects share the same serialization.
            let fresh = connector()?;
            let mut connection = connection
                .lock()
                .map_err(|_| io::Error::other("Agent connection unavailable"))?;
            if let Some(previous) = connection.replace(fresh) {
                let _ = previous.socket.shutdown(std::net::Shutdown::Both);
            }
            Ok(())
        })
        .await
        .map_err(|_| io::Error::other("Agent reconnect worker failed"))?
    }
    pub async fn resume(&self, id: Uuid, profile: &str, descriptor: OwnedFd) -> io::Result<()> {
        match self
            .request(
                agent_request::Payload::ResumeTunnel(agent_v1::ResumeTunnelRequest {
                    operation_id: id.to_string(),
                    profile_id: profile.into(),
                }),
                Some(descriptor),
            )
            .await?
            .0
        {
            agent_response::Payload::State(state)
                if state.operation_id == id.to_string()
                    && state.profile_id == profile
                    && state.phase == agent_v1::AgentPhase::Active as i32 =>
            {
                Ok(())
            }
            _ => Err(denied("Agent resumed an unexpected operation")),
        }
    }
    pub async fn request(
        &self,
        payload: agent_request::Payload,
        descriptor: Option<OwnedFd>,
    ) -> io::Result<(agent_response::Payload, Option<OwnedFd>)> {
        let permit = Arc::clone(&self.requests)
            .acquire_owned()
            .await
            .map_err(|_| io::Error::other("Agent request queue closed"))?;
        let connection = Arc::clone(&self.connection);
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            let mut connection = connection
                .lock()
                .map_err(|_| io::Error::other("Agent connection unavailable"))?;
            let request = AgentRequest {
                request_id: Uuid::new_v4().to_string(),
                protocol_version: PROTOCOL_VERSION,
                payload: Some(payload),
            };
            let frame = encode_frame(&request).map_err(io::Error::other)?;
            let current = connection.as_ref().ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::NotConnected,
                    "Agent connection requires reconnect",
                )
            })?;
            let (message, descriptor) =
                match current.exchange(&request, &frame, descriptor.as_ref()) {
                    Ok(response) => response,
                    Err(error) => {
                        // An EOF, timeout, partial frame or mismatched response may
                        // follow a committed mutation. Never replay it or read a
                        // late acknowledgement as the next request's response.
                        if let Some(previous) = connection.take() {
                            let _ = previous.socket.shutdown(std::net::Shutdown::Both);
                        }
                        return Err(error);
                    }
                };
            if let Some(error) = message.error {
                return Err(io::Error::other(format!(
                    "Linux Agent {}: {}",
                    error.code, error.message
                )));
            }
            let payload = message
                .payload
                .ok_or_else(|| denied("Agent response payload missing"))?;
            Ok((payload, descriptor))
        })
        .await
        .map_err(|_| io::Error::other("Agent request worker failed"))?
    }

    pub async fn state(&self) -> io::Result<agent_v1::AgentState> {
        match self
            .request(
                agent_request::Payload::GetState(agent_v1::GetStateRequest {}),
                None,
            )
            .await?
            .0
        {
            agent_response::Payload::State(state) => Ok(state),
            _ => Err(denied("Agent state response mismatch")),
        }
    }

    pub(crate) async fn recovery_state(&self) -> io::Result<agent_v1::AgentState> {
        match self.state().await {
            Ok(state) => Ok(state),
            Err(_) => {
                // Recovery owns no data traffic. Reconnect using the same root
                // installation and peer checks, then inspect the journal rather
                // than replaying the mutation whose outcome may be unknown.
                self.reconnect().await?;
                self.state().await
            }
        }
    }
    pub async fn capabilities(&self) -> io::Result<agent_v1::AgentCapabilities> {
        match self
            .request(
                agent_request::Payload::GetCapabilities(agent_v1::GetCapabilitiesRequest {}),
                None,
            )
            .await?
            .0
        {
            agent_response::Payload::Capabilities(capabilities)
                if capabilities.protocol_version == PROTOCOL_VERSION
                    && capabilities.operating_system == "linux"
                    && capabilities.architecture == std::env::consts::ARCH =>
            {
                Ok(capabilities)
            }
            _ => Err(denied("Agent protocol or architecture is incompatible")),
        }
    }
    pub async fn physical_network(
        &self,
        operation_id: Uuid,
    ) -> io::Result<agent_v1::PhysicalNetworkInfo> {
        match self
            .request(
                agent_request::Payload::GetPhysicalNetworkInfo(
                    agent_v1::GetPhysicalNetworkInfoRequest {
                        operation_id: operation_id.to_string(),
                    },
                ),
                None,
            )
            .await?
            .0
        {
            agent_response::Payload::PhysicalNetworkInfo(network) => Ok(network),
            _ => Err(denied("Agent physical-network response mismatch")),
        }
    }
    pub async fn prepare(
        &self,
        id: Uuid,
        plan: agent_v1::TunnelPlan,
        generation: u64,
    ) -> io::Result<()> {
        self.empty(agent_request::Payload::PrepareTunnel(
            agent_v1::PrepareTunnelRequest {
                operation_id: id.to_string(),
                plan: Some(plan),
                expected_journal_generation: generation,
                ..Default::default()
            },
        ))
        .await
    }
    pub async fn commit(&self, id: Uuid) -> io::Result<()> {
        self.empty(agent_request::Payload::CommitTunnel(
            agent_v1::CommitTunnelRequest {
                operation_id: id.to_string(),
            },
        ))
        .await
    }
    pub async fn rollback(&self, id: Uuid) -> io::Result<()> {
        self.empty(agent_request::Payload::RollbackTunnel(
            agent_v1::RollbackTunnelRequest {
                operation_id: id.to_string(),
                ..Default::default()
            },
        ))
        .await
    }
    pub async fn replace(
        &self,
        source: Uuid,
        target: Uuid,
        plan: agent_v1::TunnelPlan,
        generation: u64,
    ) -> io::Result<()> {
        self.empty(agent_request::Payload::ReplaceTunnel(
            agent_v1::ReplaceTunnelRequest {
                source_operation_id: source.to_string(),
                operation_id: target.to_string(),
                expected_journal_generation: generation,
                plan: Some(plan),
                ..Default::default()
            },
        ))
        .await
    }
    pub async fn abort_replacement(&self, id: Uuid, generation: u64) -> io::Result<()> {
        self.empty(agent_request::Payload::AbortReplacement(
            agent_v1::AbortReplacementRequest {
                operation_id: id.to_string(),
                expected_journal_generation: generation,
            },
        ))
        .await
    }
    pub async fn finalize(&self, id: Uuid, plan: agent_v1::TunnelPlan) -> io::Result<()> {
        self.empty(agent_request::Payload::FinalizeTunnel(
            agent_v1::FinalizeTunnelRequest {
                operation_id: id.to_string(),
                plan: Some(plan),
            },
        ))
        .await
    }
    pub async fn begin_chain_transition(&self, id: Uuid) -> io::Result<()> {
        self.empty(agent_request::Payload::BeginChainTransition(
            agent_v1::BeginChainTransitionRequest {
                operation_id: id.to_string(),
                retain_startup_lease: true,
            },
        ))
        .await
    }
    pub async fn recover(&self, state: &agent_v1::AgentState) -> io::Result<()> {
        self.empty(agent_request::Payload::RecoverOrphaned(
            agent_v1::RecoverOrphanedRequest {
                operation_id: state.operation_id.clone(),
                expected_journal_generation: state.journal_generation,
            },
        ))
        .await
    }
    async fn empty(&self, payload: agent_request::Payload) -> io::Result<()> {
        match self.request(payload, None).await?.0 {
            agent_response::Payload::Empty(_) => Ok(()),
            _ => Err(denied("Agent acknowledgement mismatch")),
        }
    }
    pub async fn packet_session(
        &self,
        id: Uuid,
    ) -> io::Result<(agent_v1::LinuxTunSession, OwnedFd)> {
        match self
            .request(
                agent_request::Payload::OpenPacketSession(agent_v1::OpenPacketSessionRequest {
                    operation_id: id.to_string(),
                    ..Default::default()
                }),
                None,
            )
            .await?
        {
            (agent_response::Payload::LinuxTunSession(session), Some(descriptor))
                if session.operation_id == id.to_string() =>
            {
                validate_tun_descriptor(&descriptor, session.interface_index)?;
                Ok((session, descriptor))
            }
            _ => Err(denied("Agent packet-session response mismatch")),
        }
    }
}

fn validate_tun_descriptor(descriptor: &OwnedFd, expected_index: u32) -> io::Result<()> {
    // SAFETY: all-zero ifreq has no active pointer fields for TUNGETIFF; the
    // kernel writes only the fixed interface name and interface flag fields.
    let mut request: libc::ifreq = unsafe { std::mem::zeroed() };
    // SAFETY: descriptor is owned and request has the exact kernel ifreq size.
    if unsafe { libc::ioctl(descriptor.as_raw_fd(), libc::TUNGETIFF, &mut request) } < 0 {
        return Err(denied("Agent did not return a Linux TUN descriptor"));
    }
    // SAFETY: TUNGETIFF initialized the flags union member.
    let flags = unsafe { request.ifr_ifru.ifru_flags } as i32;
    if flags & (libc::IFF_TUN | libc::IFF_NO_PI) != (libc::IFF_TUN | libc::IFF_NO_PI)
        || request.ifr_name.last() != Some(&0)
    {
        return Err(denied("Agent TUN descriptor has unexpected flags"));
    }
    // SAFETY: ioctl wrote a bounded NUL-terminated name, checked above.
    let index = unsafe { libc::if_nametoindex(request.ifr_name.as_ptr()) };
    if index == 0 || index != expected_index {
        return Err(denied("Agent TUN interface identity mismatch"));
    }
    // SAFETY: fcntl reads the flags of a live descriptor without transferring ownership.
    let flags = unsafe { libc::fcntl(descriptor.as_raw_fd(), libc::F_GETFL) };
    if flags < 0 || flags & libc::O_NONBLOCK == 0 {
        return Err(denied("Agent TUN descriptor must be nonblocking"));
    }
    Ok(())
}
fn denied(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::PermissionDenied, message)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn mismatched_agent_response_cannot_complete_an_operation() {
        let (client, server) = LinuxAgentClient::fake_pair();
        let worker = std::thread::spawn(move || {
            let peer = unix_fd::peer_credentials(&server).unwrap();
            let _ = unix_fd::receive_frame(&server, peer).unwrap();
            let response = AgentResponse {
                request_id: "wrong-operation".into(),
                error: None,
                payload: Some(agent_response::Payload::State(Default::default())),
            };
            unix_fd::send_frame(&server, &encode_frame(&response).unwrap(), None::<&OwnedFd>)
                .unwrap();
        });
        assert_eq!(
            client.state().await.unwrap_err().kind(),
            io::ErrorKind::PermissionDenied
        );
        worker.join().unwrap();
    }
    #[tokio::test]
    async fn unexpected_fd_is_rejected_even_with_a_valid_response_id() {
        let (client, server) = LinuxAgentClient::fake_pair();
        let worker = std::thread::spawn(move || {
            let peer = unix_fd::peer_credentials(&server).unwrap();
            let frame = unix_fd::receive_frame(&server, peer).unwrap();
            let request: AgentRequest = decode_frame(frame.bytes.into()).unwrap();
            let response = AgentResponse {
                request_id: request.request_id,
                error: None,
                payload: Some(agent_response::Payload::State(Default::default())),
            };
            let unrelated: OwnedFd = fs::File::open("/dev/null").unwrap().into();
            unix_fd::send_frame(&server, &encode_frame(&response).unwrap(), Some(&unrelated))
                .unwrap();
        });
        assert_eq!(
            client.state().await.unwrap_err().kind(),
            io::ErrorKind::PermissionDenied
        );
        worker.join().unwrap();
    }
    #[test]
    fn unrelated_descriptors_never_become_packet_devices() {
        let file = fs::File::open("/dev/null").unwrap();
        assert!(validate_tun_descriptor(&file.into(), 1).is_err());
    }
}
