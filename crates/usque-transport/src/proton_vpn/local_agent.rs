//! Independent implementation of the Proton in-tunnel agent wire contract.
//! Protocol reference: ProtonVPN/local-agent-rs 74ec6f4f093805d766f11f9ef522bf1120591058.
//! No upstream implementation is included. TCP uses a dedicated in-memory stack,
//! never an OS socket. Its single flow is demultiplexed before business admission.
//! vpn_ca.pem contains public certificate data only. The DER SHA256 hashes are
//! 474792c86ca0e72f6c04ea87cbfd499da38c58f4466e7b258cdf67da34656e31 and
//! a3705cd0790c8a665ae7e8e4269974c2ee318aed03cf0c7e860c2228cf17e4ab,
//! matching the pinned agent_connector.rs and Android Constants.kt trust anchors.

use crate::chain_session::Input;
use crate::packet_pipe::PacketSender;
use base64::{Engine, engine::general_purpose::STANDARD};
use bytes::Bytes;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, ServerName};
use serde_json::{Value, json};
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::watch;
use tokio::time::{Instant, timeout_at};
use tokio_rustls::TlsConnector;
use tokio_util::sync::CancellationToken;
use tokio_util::task::AbortOnDropHandle;
use ts_netstack_smoltcp::netcore::{Config, HasChannel, NetstackControl};
use usque_core::proton_vpn::TunnelCredentials;
use usque_core::vpngate::GateFailure;

const CLIENT: Ipv4Addr = Ipv4Addr::new(10, 2, 0, 2);
const SERVER: Ipv4Addr = Ipv4Addr::new(10, 2, 0, 1);
const SERVER_PORT: u16 = 65432;
const MAX_FRAME: usize = 32 * 1024;
const IO_BUDGET: Duration = Duration::from_secs(10);
const STATUS_INTERVAL: Duration = Duration::from_secs(30);
const ROOT_CERTIFICATES: &str = include_str!("vpn_ca.pem");

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AgentState {
    Connecting,
    Connected,
    Failed(GateFailure),
}

/// This capability describes exactly one flow; it cannot dial arbitrary targets.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ControlTuple {
    local_port: u16,
}

impl ControlTuple {
    fn packet_endpoints(packet: &[u8]) -> Option<(Ipv4Addr, u16, Ipv4Addr, u16)> {
        if packet.len() < 40 || packet[0] >> 4 != 4 || packet[9] != 6 {
            return None;
        }
        let header = usize::from(packet[0] & 15) * 4;
        let total = usize::from(u16::from_be_bytes([packet[2], packet[3]]));
        let fragments = u16::from_be_bytes([packet[6], packet[7]]);
        if header < 20 || total != packet.len() || header + 20 > total || fragments & 0xbfff != 0 {
            return None;
        }
        let tcp_header = usize::from(packet[header + 12] >> 4) * 4;
        if tcp_header < 20 || header + tcp_header > total {
            return None;
        }
        Some((
            Ipv4Addr::new(packet[12], packet[13], packet[14], packet[15]),
            u16::from_be_bytes([packet[header], packet[header + 1]]),
            Ipv4Addr::new(packet[16], packet[17], packet[18], packet[19]),
            u16::from_be_bytes([packet[header + 2], packet[header + 3]]),
        ))
    }

    pub(crate) fn incoming(self, packet: &[u8]) -> bool {
        Self::packet_endpoints(packet) == Some((SERVER, SERVER_PORT, CLIENT, self.local_port))
    }

    fn outgoing(self, packet: &[u8]) -> bool {
        Self::packet_endpoints(packet) == Some((CLIENT, self.local_port, SERVER, SERVER_PORT))
    }

    /// Business packets can never create a second connection to the agent.
    pub(crate) fn reserved_outgoing(packet: &[u8]) -> bool {
        Self::reserved(packet, false)
    }

    pub(crate) fn reserved_incoming(packet: &[u8]) -> bool {
        Self::reserved(packet, true)
    }

    fn reserved(packet: &[u8], incoming: bool) -> bool {
        if packet.len() < 20 || packet[0] >> 4 != 4 || packet[9] != 6 {
            return false;
        }
        let address = if incoming { 12 } else { 16 };
        if packet[address..address + 4] != SERVER.octets() {
            return false;
        }
        // A noninitial fragment has no ports. Reject every TCP fragment to or
        // from the reserved address so reassembly cannot bypass the port gate.
        let fragments = u16::from_be_bytes([packet[6], packet[7]]);
        let header = usize::from(packet[0] & 15) * 4;
        let total = usize::from(u16::from_be_bytes([packet[2], packet[3]]));
        if fragments & 0xbfff != 0 || header < 20 || total != packet.len() || header + 20 > total {
            return true;
        }
        let port = header + if incoming { 0 } else { 2 };
        u16::from_be_bytes([packet[port], packet[port + 1]]) == SERVER_PORT
    }
}

#[derive(Clone)]
pub(crate) struct AgentIngress {
    tuple: ControlTuple,
    sender: PacketSender,
}

impl AgentIngress {
    /// True means consumed by the reserved flow; it must not reach business TCP.
    pub(crate) async fn route(&self, packet: &Bytes) -> Result<bool, GateFailure> {
        if !self.tuple.incoming(packet) {
            return Ok(false);
        }
        if !self.sender.send_owned_checked(packet.clone()).await {
            return Err(GateFailure::Transport);
        }
        Ok(true)
    }
}

pub(crate) struct AgentControl {
    pub(crate) status: watch::Receiver<AgentState>,
    pub(crate) ingress: AgentIngress,
    gate: AgentGate,
    cancellation: CancellationToken,
    _task: AbortOnDropHandle<()>,
}

pub(crate) struct Parameters {
    pub(crate) local: Ipv4Addr,
    pub(crate) mtu: u16,
    pub(crate) generation: u64,
    pub(crate) startup_deadline: Instant,
}

struct Lifetime<'a> {
    publisher: &'a watch::Sender<AgentState>,
    cancellation: &'a CancellationToken,
    startup_deadline: Instant,
    expires_at_gate: &'a AtomicI64,
}

#[derive(Clone)]
pub(crate) struct AgentGate {
    status: watch::Receiver<AgentState>,
    expires_at: Arc<AtomicI64>,
}
impl AgentGate {
    pub(crate) fn authorized(&self) -> bool {
        self.status.has_changed().is_ok()
            && *self.status.borrow() == AgentState::Connected
            && unix_now().is_ok_and(|now| now < self.expires_at.load(Ordering::Acquire))
    }
}

impl Drop for AgentControl {
    fn drop(&mut self) {
        self.cancellation.cancel();
    }
}

impl AgentControl {
    pub(crate) fn gate(&self) -> AgentGate {
        self.gate.clone()
    }
    pub(crate) async fn start(
        credentials: watch::Receiver<Arc<TunnelCredentials>>,
        parameters: Parameters,
        input: Input,
        parent: &CancellationToken,
    ) -> Result<Self, GateFailure> {
        let Parameters {
            local,
            mtu,
            generation,
            startup_deadline,
        } = parameters;
        if local != CLIENT || !(576..=1500).contains(&mtu) {
            return Err(GateFailure::Configuration);
        }
        let cancellation = parent.child_token();
        let tuple = ControlTuple {
            local_port: crate::port_allocator::next_tcp_port(),
        };
        let config = Config {
            command_channel_capacity: Some(8),
            mtu: usize::from(mtu),
            tcp_buffer_size: 32 * 1024,
            tcp_nagle_enabled: false,
            ..Config::default()
        };
        let (stack, pipe) = crate::netstack::bounded_piped_with_capacity(config, 16);
        let channel = stack.command_channel();
        let stack_task = AbortOnDropHandle::new(stack.spawn_tokio());
        tokio::select! {
            biased;
            _ = cancellation.cancelled() => return Err(GateFailure::Transport),
            result = timeout_at(startup_deadline, channel.set_ips([CLIENT.into()])) => {
                result.map_err(|_| GateFailure::Transport)?.map_err(|_| GateFailure::Transport)?;
            }
        }
        let ingress = AgentIngress {
            tuple,
            sender: pipe.tx,
        };
        let mut outgoing = pipe.rx;
        let (publisher, status) = watch::channel(AgentState::Connecting);
        let expires_at = Arc::new(AtomicI64::new(0));
        let gate = AgentGate {
            status: status.clone(),
            expires_at: expires_at.clone(),
        };
        let task_cancel = cancellation.clone();
        let task = tokio::spawn(async move {
            let _stack_task = stack_task;
            let egress_cancel = task_cancel.clone();
            let egress_status = publisher.clone();
            let _egress = AbortOnDropHandle::new(tokio::spawn(async move {
                let forward = async {
                    while let Some(packet) = outgoing.recv_async().await {
                        if !tuple.outgoing(&packet) {
                            return;
                        }
                        if input.send_ip_owned(generation, packet).await.is_err() {
                            return;
                        }
                    }
                };
                tokio::select! {
                    biased;
                    _ = egress_cancel.cancelled() => {},
                    _ = forward => {
                        egress_status.send_replace(AgentState::Failed(GateFailure::Transport));
                        egress_cancel.cancel();
                    }
                }
            }));
            let work = credential_loop(
                credentials,
                channel,
                tuple,
                Lifetime {
                    publisher: &publisher,
                    cancellation: &task_cancel,
                    startup_deadline,
                    expires_at_gate: &expires_at,
                },
            );
            let result = tokio::select! {
                biased;
                _ = task_cancel.cancelled() => Err(GateFailure::Transport),
                result = work => result,
            };
            publisher.send_replace(AgentState::Failed(
                result.err().unwrap_or(GateFailure::Transport),
            ));
            task_cancel.cancel();
        });
        Ok(Self {
            status,
            ingress,
            gate,
            cancellation,
            _task: AbortOnDropHandle::new(task),
        })
    }
}

fn unix_now() -> Result<i64, GateFailure> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|d| i64::try_from(d.as_secs()).ok())
        .ok_or(GateFailure::Certificate)
}

fn certificates(pem: &str) -> Result<Vec<CertificateDer<'static>>, GateFailure> {
    if pem.len() > usque_core::proton_vpn::MAX_CERTIFICATE_BYTES || !pem.is_ascii() {
        return Err(GateFailure::Certificate);
    }
    let mut rest = pem.trim();
    let mut result = Vec::new();
    while !rest.is_empty() && result.len() < 8 {
        let content = rest
            .strip_prefix("-----BEGIN CERTIFICATE-----")
            .ok_or(GateFailure::Certificate)?;
        let (body, tail) = content
            .split_once("-----END CERTIFICATE-----")
            .ok_or(GateFailure::Certificate)?;
        let body: String = body.chars().filter(|c| !c.is_ascii_whitespace()).collect();
        let der = STANDARD
            .decode(&body)
            .map_err(|_| GateFailure::Certificate)?;
        if der.is_empty()
            || STANDARD.encode(&der) != body
            || result
                .iter()
                .any(|existing: &CertificateDer<'_>| existing.as_ref() == der)
        {
            return Err(GateFailure::Certificate);
        }
        result.push(CertificateDer::from(der));
        rest = tail.trim();
    }
    if result.is_empty() || !rest.is_empty() {
        return Err(GateFailure::Certificate);
    }
    Ok(result)
}

fn tls_config(
    credentials: &TunnelCredentials,
    roots: &str,
) -> Result<Arc<rustls::ClientConfig>, GateFailure> {
    if credentials.expires_at <= unix_now()? || credentials.private_key_pkcs8.len() > 4096 {
        return Err(GateFailure::Certificate);
    }
    let mut store = rustls::RootCertStore::empty();
    for root in certificates(roots)? {
        store.add(root).map_err(|_| GateFailure::Certificate)?;
    }
    let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(
        credentials.private_key_pkcs8.to_vec(),
    ));
    let config = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .map_err(|_| GateFailure::Certificate)?
    .with_root_certificates(store)
    .with_client_auth_cert(certificates(&credentials.certificate_chain)?, key)
    .map_err(|_| GateFailure::Certificate)?;
    Ok(Arc::new(config))
}

async fn credential_loop(
    mut credentials: watch::Receiver<Arc<TunnelCredentials>>,
    channel: ts_netstack_smoltcp::netcore::Channel,
    tuple: ControlTuple,
    lifetime: Lifetime<'_>,
) -> Result<(), GateFailure> {
    let Lifetime {
        publisher,
        cancellation,
        startup_deadline,
        expires_at_gate,
    } = lifetime;
    let mut first = true;
    loop {
        publisher.send_replace(AgentState::Connecting);
        let current = credentials.borrow_and_update().clone();
        expires_at_gate.store(current.expires_at, Ordering::Release);
        let connect_deadline = if first {
            startup_deadline.min(Instant::now() + IO_BUDGET)
        } else {
            Instant::now() + IO_BUDGET
        };
        first = false;
        let config = tls_config(&current, ROOT_CERTIFICATES)?;
        let name = ServerName::try_from(current.server_name.clone())
            .map_err(|_| GateFailure::Certificate)?;
        if !matches!(name, ServerName::DnsName(_)) {
            return Err(GateFailure::Certificate);
        }
        let establish = async {
            let stream = crate::stack_tcp::StackTcpStream::connect(
                channel.clone(),
                SocketAddr::new(CLIENT.into(), tuple.local_port),
                SocketAddr::new(SERVER.into(), SERVER_PORT),
            )
            .await
            .map_err(|_| GateFailure::Transport)?;
            TlsConnector::from(config)
                .connect(name, stream)
                .await
                .map_err(|_| GateFailure::Certificate)
        };
        let mut stream = tokio::select! {
            biased;
            _ = cancellation.cancelled() => return Err(GateFailure::Transport),
            result = timeout_at(connect_deadline, establish) => result.map_err(|_| GateFailure::Transport)??,
        };
        let until_expiry = Duration::from_secs(
            u64::try_from(current.expires_at - unix_now()?)
                .map_err(|_| GateFailure::Certificate)?,
        );
        let expires = Instant::now()
            .checked_add(until_expiry)
            .ok_or(GateFailure::Certificate)?;
        tokio::select! {
            biased;
            _ = cancellation.cancelled() => return Err(GateFailure::Transport),
            _ = tokio::time::sleep_until(expires) => return Err(GateFailure::Certificate),
            result = credentials.changed() => {
                result.map_err(|_| GateFailure::Certificate)?;
                // A changed WireGuard identity requires a complete new tunnel.
                if credentials.borrow().private_key_pkcs8.as_slice() != current.private_key_pkcs8.as_slice()
                    || credentials.borrow().server_name != current.server_name {
                    return Err(GateFailure::Certificate);
                }
                publisher.send_replace(AgentState::Connecting);
            }
            result = protocol_loop(&mut stream, publisher, connect_deadline, current.expires_at) => return result,
        }
        // Dropping this stream closes the old agent before the next certificate.
    }
}

async fn write_frame<S: AsyncWrite + Unpin>(
    stream: &mut S,
    value: &Value,
) -> Result<(), GateFailure> {
    let encoded = serde_json::to_vec(value).map_err(|_| GateFailure::Protocol)?;
    if encoded.is_empty() || encoded.len() > MAX_FRAME {
        return Err(GateFailure::Protocol);
    }
    stream
        .write_all(&(encoded.len() as u32).to_be_bytes())
        .await
        .map_err(|_| GateFailure::Transport)?;
    stream
        .write_all(&encoded)
        .await
        .map_err(|_| GateFailure::Transport)?;
    stream.flush().await.map_err(|_| GateFailure::Transport)
}

async fn read_frame<S: AsyncRead + Unpin>(stream: &mut S) -> Result<Value, GateFailure> {
    let length = stream
        .read_u32()
        .await
        .map_err(|_| GateFailure::Transport)? as usize;
    if length == 0 || length > MAX_FRAME {
        return Err(GateFailure::Protocol);
    }
    let mut body = vec![0; length];
    stream
        .read_exact(&mut body)
        .await
        .map_err(|_| GateFailure::Transport)?;
    serde_json::from_slice(&body).map_err(|_| GateFailure::Protocol)
}

fn status(value: &Value) -> Result<bool, GateFailure> {
    let object = value.as_object().ok_or(GateFailure::Protocol)?;
    if object.contains_key("error") {
        return Err(GateFailure::Authentication);
    }
    let status = object
        .get("status")
        .and_then(Value::as_object)
        .ok_or(GateFailure::Protocol)?;
    if status.get("reason").is_some_and(|v| !v.is_null()) {
        return Err(GateFailure::Authentication);
    }
    match status.get("state").and_then(Value::as_str) {
        Some("connected") => Ok(true),
        Some("jailed") => Ok(false),
        Some("hard-jailed") => Err(GateFailure::Authentication),
        _ => Err(GateFailure::Protocol),
    }
}

fn requested_features() -> Value {
    json!({"netshield-level": 0, "randomized-nat": false, "split-tcp": false, "jail": false})
}

fn features_match(value: &Value) -> bool {
    let Some(received) = value["status"]["features"].as_object() else {
        return false;
    };
    requested_features()
        .as_object()
        .expect("fixed object")
        .iter()
        .all(|(key, expected)| received.get(key) == Some(expected))
}

async fn protocol_loop<S: AsyncRead + AsyncWrite + Unpin>(
    stream: &mut S,
    publisher: &watch::Sender<AgentState>,
    startup_deadline: Instant,
    expires_at: i64,
) -> Result<(), GateFailure> {
    let (mut reader, mut writer) = tokio::io::split(stream);
    let mut authenticated = false;
    // Both requests are standard agent operations; no privileged features are requested.
    timeout_at(
        startup_deadline,
        write_frame(
            &mut writer,
            &json!({"status-get": {"features-statistics": false}}),
        ),
    )
    .await
    .map_err(|_| GateFailure::Transport)??;
    let mut features_sent = false;
    let mut next_poll = Instant::now() + STATUS_INTERVAL;
    loop {
        let mut deadline = if authenticated {
            next_poll + IO_BUDGET
        } else {
            startup_deadline
        };
        // Keep one decoder future alive across heartbeat ticks. Cancelling a
        // partially read length/body would lose framing on the TLS byte stream.
        let reading = read_frame(&mut reader);
        tokio::pin!(reading);
        let response = loop {
            tokio::select! {
                biased;
                _ = tokio::time::sleep_until(deadline) => return Err(GateFailure::Transport),
                response = &mut reading => break response?,
                _ = tokio::time::sleep_until(next_poll), if authenticated => {
                    timeout_at(Instant::now() + IO_BUDGET,
                        write_frame(&mut writer, &json!({"status-get": {"features-statistics": false}})))
                        .await.map_err(|_| GateFailure::Transport)??;
                    next_poll = Instant::now() + STATUS_INTERVAL;
                    deadline = Instant::now() + IO_BUDGET;
                }
            }
        };
        if unix_now()? >= expires_at {
            return Err(GateFailure::Certificate);
        }
        let connected = status(&response)?;
        if !features_sent {
            timeout_at(
                startup_deadline,
                write_frame(&mut writer, &json!({"features-set": requested_features()})),
            )
            .await
            .map_err(|_| GateFailure::Transport)??;
            features_sent = true;
            // Confirm the post-feature status before admitting business packets.
            timeout_at(
                startup_deadline,
                write_frame(
                    &mut writer,
                    &json!({"status-get": {"features-statistics": false}}),
                ),
            )
            .await
            .map_err(|_| GateFailure::Transport)??;
        } else if connected && features_match(&response) {
            authenticated = true;
            publisher.send_replace(AgentState::Connected);
        } else {
            if authenticated {
                return Err(GateFailure::Authentication);
            }
            publisher.send_replace(AgentState::Connecting);
        }
    }
}

#[cfg(test)]
#[path = "agent_tests.rs"]
mod tests;
