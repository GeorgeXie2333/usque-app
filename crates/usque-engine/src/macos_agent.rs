//! Root-owned Darwin Agent connection. Credentials never cross this boundary.
use std::{
    fs, io,
    net::Shutdown,
    os::{
        fd::{AsRawFd, OwnedFd},
        unix::{
            fs::{FileTypeExt, MetadataExt, OpenOptionsExt},
            net::UnixStream,
        },
    },
    path::Path,
    sync::{Arc, Mutex},
    time::Duration,
};
use usque_ipc::{
    agent_v1::{
        self as p, agent_request, agent_response, macos_agent_request::Operation,
        macos_agent_response::Payload,
    },
    darwin_fd, decode_frame, encode_frame,
};
use uuid::Uuid;

pub const SOCKET_PATH: &str = "/Library/PrivilegedHelperTools/com.usque.agent/control.sock";
pub const ENGINE_PATH: &str = "/Library/PrivilegedHelperTools/com.usque.agent/usque-engine";
pub const PROTOCOL_VERSION: u32 = 1;
pub const RELEASE_APPROVED: bool = false;

struct Connection {
    socket: UnixStream,
    peer: darwin_fd::PeerIdentity,
    sequence: u64,
}
#[derive(Clone)]
pub struct MacAgentClient {
    connection: Arc<Mutex<Option<Connection>>>,
}
fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        "macOS Agent protocol or identity rejected",
    )
}

unsafe extern "C" {
    fn acl_get_fd_np(fd: libc::c_int, kind: libc::c_int) -> *mut std::ffi::c_void;
    fn acl_free(value: *mut std::ffi::c_void) -> libc::c_int;
}
fn no_acl(file: &fs::File) -> io::Result<()> {
    // SAFETY: live FD and Darwin ACL_TYPE_EXTENDED.
    let acl = unsafe { acl_get_fd_np(file.as_raw_fd(), 0x100) };
    if acl.is_null() {
        let error = io::Error::last_os_error();
        return if error.raw_os_error() == Some(libc::ENOENT) {
            Ok(())
        } else {
            Err(error)
        };
    }
    // SAFETY: releases the returned ACL exactly once.
    unsafe {
        acl_free(acl);
    }
    Err(invalid())
}
fn trusted_socket() -> io::Result<()> {
    for parent in Path::new(SOCKET_PATH).ancestors().skip(1) {
        let directory = fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(parent)?;
        no_acl(&directory)?;
        let metadata = directory.metadata()?;
        if !metadata.is_dir() || metadata.uid() != 0 || metadata.mode() & 0o022 != 0 {
            return Err(invalid());
        }
    }
    let metadata = fs::symlink_metadata(SOCKET_PATH)?;
    if !metadata.file_type().is_socket() || metadata.uid() != 0 {
        return Err(invalid());
    }
    Ok(())
}
impl MacAgentClient {
    pub async fn connect() -> io::Result<Self> {
        tokio::task::spawn_blocking(|| {
            trusted_socket()?;
            let socket = UnixStream::connect(SOCKET_PATH)?;
            socket.set_read_timeout(Some(Duration::from_secs(10)))?;
            socket.set_write_timeout(Some(Duration::from_secs(5)))?;
            let peer = darwin_fd::peer_identity(&socket)?;
            if peer.audit_token[1] != 0 || peer.audit_token[3] != 0 {
                return Err(invalid());
            }
            Ok(Self {
                connection: Arc::new(Mutex::new(Some(Connection {
                    socket,
                    peer,
                    sequence: 0,
                }))),
            })
        })
        .await
        .map_err(io::Error::other)?
    }
    pub async fn request(&self, operation: Operation) -> io::Result<(Payload, Option<OwnedFd>)> {
        let connection = self.connection.clone();
        tokio::task::spawn_blocking(move || {
            let mut guard = connection.lock().map_err(|_| invalid())?;
            let result = (|| {
                let connection = guard.as_mut().ok_or_else(invalid)?;
                connection.sequence = connection.sequence.checked_add(1).ok_or_else(invalid)?;
                let request = p::AgentRequest {
                    request_id: Uuid::new_v4().to_string(),
                    protocol_version: 3,
                    payload: Some(agent_request::Payload::Macos(p::MacosAgentRequest {
                        sequence: connection.sequence,
                        operation: Some(operation),
                    })),
                };
                darwin_fd::send_frame(
                    &connection.socket,
                    &encode_frame(&request).map_err(io::Error::other)?,
                    None::<&OwnedFd>,
                )?;
                let frame = darwin_fd::receive_frame(&connection.socket, connection.peer)?;
                let response: p::AgentResponse =
                    decode_frame(frame.bytes.into()).map_err(io::Error::other)?;
                if response.request_id != request.request_id || response.error.is_some() {
                    return Err(invalid());
                }
                let Some(agent_response::Payload::Macos(response)) = response.payload else {
                    return Err(invalid());
                };
                if response.failure != 0 {
                    if response.payload.is_some() || frame.descriptor.is_some() {
                        return Err(invalid());
                    }
                    let failure =
                        p::MacosFailure::try_from(response.failure).map_err(|_| invalid())?;
                    return Err(io::Error::other(format!(
                        "macOS Agent: {}",
                        failure.as_str_name()
                    )));
                }
                let payload = response.payload.ok_or_else(invalid)?;
                if matches!(payload, Payload::Descriptor(_)) != frame.descriptor.is_some() {
                    return Err(invalid());
                }
                Ok((payload, frame.descriptor))
            })();
            if result.is_err()
                && let Some(old) = guard.take()
            {
                let _ = old.socket.shutdown(Shutdown::Both);
            }
            result
        })
        .await
        .map_err(io::Error::other)?
    }
    pub async fn status(&self) -> io::Result<p::MacosPlatformStatus> {
        let (payload, _) = self.request(Operation::Status(p::Empty {})).await?;
        match payload {
            Payload::Status(status)
                if status.protocol_version == PROTOCOL_VERSION
                    && status.agent_version == env!("CARGO_PKG_VERSION")
                    && status.architecture == std::env::consts::ARCH =>
            {
                Ok(status)
            }
            _ => Err(invalid()),
        }
    }
    pub async fn empty(&self, operation: Operation) -> io::Result<()> {
        match self.request(operation).await? {
            (Payload::Empty(_), None) => Ok(()),
            _ => Err(invalid()),
        }
    }
    pub async fn descriptor(
        &self,
        operation: Operation,
        fence: &p::MacosFence,
    ) -> io::Result<(p::MacosDescriptor, OwnedFd)> {
        match self.request(operation).await? {
            (Payload::Descriptor(info), Some(fd))
                if info.fence.as_ref() == Some(fence) && info.interface_index != 0 =>
            {
                Ok((info, fd))
            }
            _ => Err(invalid()),
        }
    }
    /// Closing ownership retains protection. Only an explicit Disconnect RPC
    /// asks the Agent to restore the user's system network.
    pub async fn abandon(&self) {
        let connection = self.connection.clone();
        let _ = tokio::task::spawn_blocking(move || {
            if let Ok(mut guard) = connection.lock()
                && let Some(old) = guard.take()
            {
                let _ = old.socket.shutdown(Shutdown::Both);
            }
        })
        .await;
    }
}

pub fn capability(status: &p::MacosPlatformStatus) -> bool {
    RELEASE_APPROVED
        && status.release_approved
        && status.protocol_version == PROTOCOL_VERSION
        && status.agent_version == env!("CARGO_PKG_VERSION")
        && status.architecture == std::env::consts::ARCH
        && status.preflight_passed
        && !status.recovery_pending
        && status.failure == 0
}

/// A retained transaction has a distinct guarded-resume path. It never makes
/// the ordinary fresh-connect capability true while recovery is outstanding.
pub fn resumable(status: &p::MacosPlatformStatus) -> bool {
    RELEASE_APPROVED
        && status.release_approved
        && status.resumable
        && status.protection_retained
        && status.fence.is_some()
        && status.physical_network.is_some()
        && !status.control_api_candidates.is_empty()
        && status.protocol_version == PROTOCOL_VERSION
        && status.agent_version == env!("CARGO_PKG_VERSION")
        && status.architecture == std::env::consts::ARCH
}
pub async fn connection_available() -> bool {
    match MacAgentClient::connect().await {
        Ok(client) => client
            .status()
            .await
            .is_ok_and(|s| capability(&s) || resumable(&s)),
        Err(_) => false,
    }
}
#[async_trait::async_trait]
trait RecoveryClient: Sync {
    async fn recovery_status(&self) -> io::Result<p::MacosPlatformStatus>;
    async fn disconnect_fence(&self, fence: p::MacosFence) -> io::Result<()>;
}
#[async_trait::async_trait]
impl RecoveryClient for MacAgentClient {
    async fn recovery_status(&self) -> io::Result<p::MacosPlatformStatus> {
        self.status().await
    }
    async fn disconnect_fence(&self, fence: p::MacosFence) -> io::Result<()> {
        self.empty(Operation::Disconnect(fence)).await
    }
}
async fn recover_explicit(
    client: &impl RecoveryClient,
    expected: Option<&p::MacosFence>,
) -> io::Result<()> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    loop {
        let status = client.recovery_status().await?;
        if status.phase == p::AgentPhase::Clean as i32
            && !status.recovery_pending
            && status.failure == 0
            && status.fence.is_none()
        {
            return Ok(());
        }
        let fence = status.fence.as_ref().ok_or_else(invalid)?;
        // An interrupted Resume can commit its successor before the runtime
        // is published. Only the same session's monotonic successor is ours.
        if expected.is_some_and(|old| {
            fence.session_id != old.session_id
                || fence.configuration < old.configuration
                || fence.network < old.network
        }) {
            return Err(invalid());
        }
        if status.recovery_pending
            && matches!(
                p::AgentPhase::try_from(status.phase),
                Ok(p::AgentPhase::RecoveryRequired | p::AgentPhase::Recovering)
            )
        {
            // Agent authenticates this Engine and enforces the retained UID;
            // neither a new PID nor an observed fence permits another user.
            return client.disconnect_fence(fence.clone()).await;
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(invalid());
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}
fn retained_guard(status: &p::MacosPlatformStatus, expected: &p::MacosFence) -> bool {
    status.protection_retained
        && status.recovery_pending
        && status.resumable
        && status.fence.as_ref().is_some_and(|f| {
            f.session_id == expected.session_id
                && f.configuration >= expected.configuration
                && f.network >= expected.network
        })
}
pub async fn confirm_retained_protection(expected: &p::MacosFence) -> io::Result<()> {
    let client = MacAgentClient::connect().await?;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    loop {
        if retained_guard(&client.status().await?, expected) {
            return Ok(());
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(io::Error::other(
                "network protection could not be confirmed",
            ));
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}
pub async fn disconnect_retained(fence: p::MacosFence) -> io::Result<()> {
    recover_explicit(&MacAgentClient::connect().await?, Some(&fence)).await
}
/// Called only by explicit Disconnect/Quit, including failed startup with no
/// in-memory runtime. Startup, error handling and Engine loss never call this.
pub async fn recover_on_explicit_disconnect() -> io::Result<()> {
    match fs::symlink_metadata(ENGINE_PATH) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
        Ok(_) => (),
    }
    recover_explicit(&MacAgentClient::connect().await?, None).await
}

pub fn verify_utun(fd: &OwnedFd, info: &p::MacosDescriptor) -> io::Result<()> {
    if info.lease_id != 0
        || info.protocol != 0
        || !info.local_endpoint.is_empty()
        || !info.remote_endpoint.is_empty()
    {
        return Err(invalid());
    }
    let mut name = [0u8; libc::IFNAMSIZ];
    let mut length = name.len() as libc::socklen_t;
    // SAFETY: live owned control FD and bounded output buffer.
    if unsafe {
        libc::getsockopt(
            fd.as_raw_fd(),
            libc::SYSPROTO_CONTROL,
            2,
            name.as_mut_ptr().cast(),
            &mut length,
        )
    } != 0
    {
        return Err(invalid());
    }
    let bytes = name
        .get(..length as usize)
        .and_then(|s| s.strip_suffix(&[0]))
        .ok_or_else(invalid)?;
    if bytes != info.interface_name.as_bytes()
        || !info
            .interface_name
            .strip_prefix("utun")
            .is_some_and(|s| s.parse::<u32>().is_ok_and(|n| n.to_string() == s))
    {
        return Err(invalid());
    }
    let cname = std::ffi::CString::new(bytes).map_err(|_| invalid())?;
    // SAFETY: terminated kernel name and live descriptor; only read-only queries.
    let index = unsafe { libc::if_nametoindex(cname.as_ptr()) };
    // SAFETY: fcntl only reads flags on this live descriptor.
    let flags = unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_GETFL) };
    if index != info.interface_index
        || info.interface_index == 0
        || flags < 0
        || flags & libc::O_NONBLOCK == 0
    {
        return Err(invalid());
    }
    Ok(())
}

pub fn verify_socket(
    fd: &OwnedFd,
    info: &p::MacosDescriptor,
    remote: std::net::SocketAddr,
    protocol: u8,
) -> io::Result<()> {
    if info.lease_id == 0
        || info.remote_endpoint != remote.to_string()
        || info.protocol != u32::from(protocol)
    {
        return Err(invalid());
    }
    let mut kind = 0i32;
    let mut length = std::mem::size_of_val(&kind) as libc::socklen_t;
    // SAFETY: live owned FD and exact writable getsockopt buffer.
    if unsafe {
        libc::getsockopt(
            fd.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_TYPE,
            (&mut kind as *mut i32).cast(),
            &mut length,
        )
    } != 0
        || kind
            != if protocol == 6 {
                libc::SOCK_STREAM
            } else {
                libc::SOCK_DGRAM
            }
    {
        return Err(invalid());
    }
    let mut index = 0u32;
    length = std::mem::size_of_val(&index) as _;
    let (level, option) = if remote.is_ipv4() {
        (libc::IPPROTO_IP, libc::IP_BOUND_IF)
    } else {
        (libc::IPPROTO_IPV6, libc::IPV6_BOUND_IF)
    };
    // SAFETY: reads the interface binding from the received socket.
    if unsafe {
        libc::getsockopt(
            fd.as_raw_fd(),
            level,
            option,
            (&mut index as *mut u32).cast(),
            &mut length,
        )
    } != 0
        || index != info.interface_index
    {
        return Err(invalid());
    }
    // SAFETY: inspecting flags of a live descriptor does not transfer ownership.
    let flags = unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_GETFL) };
    if flags < 0 || flags & libc::O_NONBLOCK == 0 {
        return Err(invalid());
    }
    Ok(())
}

/// Read-only projection for an Engine restarted without an in-memory owner.
/// The installed service remains authoritative until explicit recovery.
pub async fn project_recovery(snapshot: &mut usque_core::ConnectionSnapshot) {
    if fs::symlink_metadata(ENGINE_PATH).is_err() {
        return;
    }
    let status = tokio::time::timeout(Duration::from_secs(2), async {
        MacAgentClient::connect().await?.status().await
    })
    .await
    .map_err(io::Error::other)
    .and_then(|s| s);
    project_observation(snapshot, status.as_ref().ok());
}
fn project_observation(
    snapshot: &mut usque_core::ConnectionSnapshot,
    status: Option<&p::MacosPlatformStatus>,
) {
    if status.is_some_and(|s| {
        s.phase == p::AgentPhase::Clean as i32
            && !s.recovery_pending
            && s.failure == 0
            && s.fence.is_none()
    }) {
        return;
    }
    snapshot.phase = usque_core::ConnectionPhase::Error;
    snapshot.kill_switch_state = if status.is_some_and(|s| s.protection_retained) {
        usque_core::KillSwitchState::Active
    } else {
        usque_core::KillSwitchState::Error
    };
    snapshot.error = Some(usque_core::ConnectionError {
        code: usque_core::ErrorCode::TransportUnavailable,
        message:
            "macOS network recovery is incomplete; disconnect or restore the background service"
                .into(),
        retryable: true,
    });
}

/// Non-secret status used by native Settings without opening a user profile.
pub async fn service_status() -> serde_json::Value {
    let result = async { MacAgentClient::connect().await?.status().await }.await;
    match result {
        Ok(status) => {
            serde_json::json!({"installed": true, "compatible": true, "version": status.agent_version,
            "architecture": status.architecture, "preflightPassed": status.preflight_passed,
            "recoveryPending": status.recovery_pending, "protectionRetained": status.protection_retained,
            "tunAvailable": capability(&status), "releaseApproved": status.release_approved && RELEASE_APPROVED,
            "phase": status.phase, "failure": status.failure})
        }
        Err(_) => {
            serde_json::json!({"installed": std::path::Path::new(ENGINE_PATH).is_file(), "compatible": false,
            "recoveryPending": true, "tunAvailable": false, "failure": "service_unavailable"})
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct FakeRecovery {
        status: p::MacosPlatformStatus,
        disconnected: Mutex<Vec<p::MacosFence>>,
    }
    #[async_trait::async_trait]
    impl RecoveryClient for FakeRecovery {
        async fn recovery_status(&self) -> io::Result<p::MacosPlatformStatus> {
            Ok(self.status.clone())
        }
        async fn disconnect_fence(&self, fence: p::MacosFence) -> io::Result<()> {
            self.disconnected.lock().unwrap().push(fence);
            Ok(())
        }
    }
    #[tokio::test]
    async fn failed_startup_and_failed_resume_disconnect_the_observed_retained_fence() {
        let old = p::MacosFence {
            session_id: Uuid::new_v4().to_string(),
            configuration: 1,
            network: 1,
        };
        let next = p::MacosFence {
            configuration: 2,
            network: 2,
            ..old.clone()
        };
        let fake = FakeRecovery {
            status: p::MacosPlatformStatus {
                fence: Some(next.clone()),
                recovery_pending: true,
                phase: p::AgentPhase::RecoveryRequired as i32,
                ..Default::default()
            },
            disconnected: Mutex::new(vec![]),
        };
        recover_explicit(&fake, Some(&old)).await.unwrap();
        recover_explicit(&fake, None).await.unwrap();
        assert_eq!(*fake.disconnected.lock().unwrap(), [next.clone(), next]);
        let unrelated = p::MacosFence {
            session_id: Uuid::new_v4().to_string(),
            ..old
        };
        assert!(recover_explicit(&fake, Some(&unrelated)).await.is_err());
        assert_eq!(fake.disconnected.lock().unwrap().len(), 2);
    }
    #[tokio::test]
    async fn missing_recovery_evidence_is_never_a_clean_disconnect() {
        let fake = FakeRecovery {
            status: p::MacosPlatformStatus {
                recovery_pending: true,
                phase: p::AgentPhase::RecoveryRequired as i32,
                ..Default::default()
            },
            disconnected: Mutex::new(vec![]),
        };
        assert!(recover_explicit(&fake, None).await.is_err());
        assert!(fake.disconnected.lock().unwrap().is_empty());
    }
    #[tokio::test]
    async fn lost_disconnect_reply_retries_as_authenticated_clean() {
        let old = p::MacosFence {
            session_id: Uuid::new_v4().to_string(),
            configuration: 1,
            network: 1,
        };
        let fake = FakeRecovery {
            status: p::MacosPlatformStatus {
                phase: p::AgentPhase::Clean as i32,
                ..Default::default()
            },
            disconnected: Mutex::new(vec![]),
        };
        recover_explicit(&fake, Some(&old)).await.unwrap();
        assert!(fake.disconnected.lock().unwrap().is_empty());
    }
    #[test]
    fn protection_requires_actual_guard_confirmation_not_just_retained_phase() {
        let fence = p::MacosFence {
            session_id: Uuid::new_v4().to_string(),
            configuration: 1,
            network: 1,
        };
        let mut status = p::MacosPlatformStatus {
            fence: Some(fence.clone()),
            recovery_pending: true,
            resumable: true,
            ..Default::default()
        };
        assert!(!retained_guard(&status, &fence));
        status.protection_retained = true;
        assert!(retained_guard(&status, &fence));
        status.resumable = false;
        assert!(!retained_guard(&status, &fence));
    }
    #[test]
    fn restarted_engine_projects_pending_or_unknown_recovery_as_error() {
        let mut snapshot = usque_core::ConnectionSnapshot::default();
        project_observation(&mut snapshot, None);
        assert_eq!(snapshot.phase, usque_core::ConnectionPhase::Error);
        assert_eq!(
            snapshot.kill_switch_state,
            usque_core::KillSwitchState::Error
        );
        project_observation(
            &mut snapshot,
            Some(&p::MacosPlatformStatus {
                recovery_pending: true,
                protection_retained: true,
                ..Default::default()
            }),
        );
        assert_eq!(
            snapshot.kill_switch_state,
            usque_core::KillSwitchState::Active
        );
    }
    #[test]
    fn an_installed_agent_cannot_override_release_acceptance() {
        let status = p::MacosPlatformStatus {
            release_approved: true,
            preflight_passed: true,
            protocol_version: PROTOCOL_VERSION,
            agent_version: env!("CARGO_PKG_VERSION").into(),
            architecture: std::env::consts::ARCH.into(),
            ..Default::default()
        };
        assert!(!capability(&status));
    }
    #[test]
    fn ordinary_descriptor_cannot_impersonate_utun() {
        let (socket, _peer) = std::os::unix::net::UnixDatagram::pair().unwrap();
        socket.set_nonblocking(true).unwrap();
        let info = p::MacosDescriptor {
            interface_name: "utun4".into(),
            interface_index: 4,
            ..Default::default()
        };
        assert!(verify_utun(&socket.into(), &info).is_err());
    }
    #[test]
    fn ordinary_unbound_socket_cannot_impersonate_an_agent_descriptor() {
        let socket = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        let fd: OwnedFd = socket.into();
        let remote: std::net::SocketAddr = "127.0.0.1:53".parse().unwrap();
        let info = p::MacosDescriptor {
            lease_id: 1,
            interface_index: 1,
            remote_endpoint: remote.to_string(),
            protocol: 17,
            ..Default::default()
        };
        assert!(verify_socket(&fd, &info, remote, 17).is_err());
    }
}
