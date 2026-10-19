//! Independent implementation of the Proton in-tunnel agent wire contract.
//! Protocol reference: ProtonVPN/local-agent-rs 74ec6f4f093805d766f11f9ef522bf1120591058.
//! No upstream implementation is included. TCP uses a dedicated in-memory stack,
//! never an OS socket. Its single flow is demultiplexed before business admission.
//! vpn_ca.pem contains public certificate data only. The DER SHA256 hashes are
//! 474792c86ca0e72f6c04ea87cbfd499da38c58f4466e7b258cdf67da34656e31 and
//! a3705cd0790c8a665ae7e8e4269974c2ee318aed03cf0c7e860c2228cf17e4ab,
//! matching the pinned agent_connector.rs and Android Constants.kt trust anchors.

use super::renew::RecoveryRequest;
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
use tokio::sync::{mpsc, oneshot, watch};
use tokio::time::{Instant, timeout_at};
use tokio_rustls::TlsConnector;
use tokio_util::sync::CancellationToken;
use tokio_util::task::AbortOnDropHandle;
use ts_netstack_smoltcp::netcore::{Config, HasChannel, NetstackControl};
use usque_core::proton_vpn::{ConnectionFailure, FailureKind, TunnelCredentials};
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
    recovery: Option<mpsc::Sender<RecoveryRequest>>,
    roots: &'static str,
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
        recovery: Option<mpsc::Sender<RecoveryRequest>>,
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
        tracing::debug!(gate_event = "PROTON_AGENT_STACK_READY");
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
                    recovery,
                    roots: ROOT_CERTIFICATES,
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

async fn recover_credentials(
    recovery: &Option<mpsc::Sender<RecoveryRequest>>,
    code: u32,
    reason: GateFailure,
    cancel: &CancellationToken,
) -> Result<Arc<TunnelCredentials>, GateFailure> {
    let Some(recovery) = recovery else {
        return Err(reason);
    };
    let (reply, response) = oneshot::channel();
    let recover = async {
        recovery
            .send(RecoveryRequest { code, reply })
            .await
            .map_err(|_| reason)?;
        response.await.map_err(|_| reason)?
    };
    tokio::select! {
        biased;
        _ = cancel.cancelled() => Err(GateFailure::Transport),
        result = timeout_at(Instant::now() + Duration::from_secs(35), recover) => result.map_err(|_| reason)?,
    }
}

fn client_certificate_alert(error: &std::io::Error) -> bool {
    matches!(
        error
            .get_ref()
            .and_then(|error| error.downcast_ref::<rustls::Error>()),
        Some(rustls::Error::AlertReceived(
            rustls::AlertDescription::CertificateExpired | rustls::AlertDescription::UnknownCA
        ))
    )
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
        recovery,
        roots,
    } = lifetime;
    let mut first = true;
    let mut certificate_recoveries = 0;
    loop {
        deauthorize(publisher);
        let current = credentials.borrow_and_update().clone();
        expires_at_gate.store(current.expires_at, Ordering::Release);
        let connect_deadline = if first {
            startup_deadline.min(Instant::now() + IO_BUDGET)
        } else {
            Instant::now() + IO_BUDGET
        };
        first = false;
        let config = tls_config(&current, roots)?;
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
            .map_err(|_| (GateFailure::Transport, false))?;
            tracing::debug!(gate_event = "PROTON_AGENT_TCP_READY");
            TlsConnector::from(config)
                .connect(name, stream)
                .await
                .map_err(|error| (GateFailure::Certificate, client_certificate_alert(&error)))
        };
        let stream = tokio::select! {
            biased;
            _ = cancellation.cancelled() => return Err(GateFailure::Transport),
            result = timeout_at(connect_deadline, establish) => result.map_err(|_| GateFailure::Transport)?,
        };
        let mut stream = match stream {
            Ok(stream) => stream,
            Err((reason, true)) if certificate_recoveries == 0 => {
                certificate_recoveries += 1;
                // This is a server TLS alert about our client certificate, not
                // a failure validating the remote server's trust or hostname.
                recover_credentials(&recovery, 86101, reason, cancellation).await?;
                continue;
            }
            Err((reason, _)) => return Err(reason),
        };
        tracing::debug!(gate_event = "PROTON_AGENT_TLS_READY");
        let until_expiry = Duration::from_secs(
            u64::try_from(current.expires_at - unix_now()?)
                .map_err(|_| GateFailure::Certificate)?,
        );
        let expires = Instant::now()
            .checked_add(until_expiry)
            .ok_or(GateFailure::Certificate)?;
        let protocol_deadline = if startup_deadline > Instant::now() {
            startup_deadline
        } else {
            Instant::now() + Duration::from_secs(35)
        };
        let result = tokio::select! {
            biased;
            _ = cancellation.cancelled() => return Err(GateFailure::Transport),
            _ = tokio::time::sleep_until(expires) => return Err(GateFailure::Proton(ConnectionFailure::new(FailureKind::Certificate))),
            result = credentials.changed() => {
                result.map_err(|_| GateFailure::Certificate)?;
                if credentials.borrow().private_key_pkcs8 != current.private_key_pkcs8
                    || credentials.borrow().server_name != current.server_name {
                    return Err(GateFailure::Proton(ConnectionFailure::new(FailureKind::Reconnect)));
                }
                Ok(())
            }
            result = protocol_loop_with_label(&mut stream, publisher, protocol_deadline, current.expires_at, current.label.as_deref()) => result,
        };
        drop(stream);
        if let Err(reason) = result {
            let code = match reason {
                GateFailure::Proton(ConnectionFailure {
                    code: Some(code @ (86101 | 86102 | 86105)),
                    ..
                }) => code,
                _ => return Err(reason),
            };
            if certificate_recoveries >= 1 {
                return Err(reason);
            }
            certificate_recoveries += 1;
            deauthorize(publisher);
            let updated = recover_credentials(&recovery, code, reason, cancellation).await?;
            if updated.private_key_pkcs8 != current.private_key_pkcs8 {
                return Err(GateFailure::Proton(ConnectionFailure::new(
                    FailureKind::Reconnect,
                )));
            }
        }
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

fn deauthorize(publisher: &watch::Sender<AgentState>) {
    publisher.send_if_modified(|state| {
        if *state == AgentState::Connecting {
            false
        } else {
            *state = AgentState::Connecting;
            true
        }
    });
}

fn status(value: &Value) -> Result<bool, GateFailure> {
    let object = value.as_object().ok_or(GateFailure::Protocol)?;
    if let Some(error) = object.get("error").filter(|v| !v.is_null()) {
        return reason_status(error, false).map(|_| false);
    }
    let status = object
        .get("status")
        .and_then(Value::as_object)
        .ok_or(GateFailure::Protocol)?;
    if let Some(reason) = status.get("reason").filter(|v| !v.is_null()) {
        return reason_status(reason, true).map(|_| false);
    }
    match status.get("state").and_then(Value::as_str) {
        Some("connected") => Ok(true),
        Some("jailed") => Ok(false),
        Some("hard-jailed") => Err(GateFailure::Proton(ConnectionFailure::agent(0))),
        _ => Err(GateFailure::Protocol),
    }
}

fn reason_status(reason: &Value, has_final: bool) -> Result<(), GateFailure> {
    let object = reason.as_object().ok_or(GateFailure::Protocol)?;
    let code = object
        .get("code")
        .and_then(Value::as_u64)
        .and_then(|v| u32::try_from(v).ok())
        .ok_or(GateFailure::Protocol)?;
    let final_error = match object.get("final") {
        None => false,
        Some(Value::Bool(value)) => *value,
        _ => return Err(GateFailure::Protocol),
    };
    if code == 86104 {
        return Ok(());
    }
    if code == 0
        || matches!(code, 86100..=86106 | 86110..=86115 | 86120..=86122 | 86150..=86154)
        || has_final && final_error
    {
        return Err(GateFailure::Proton(ConnectionFailure::agent(code)));
    }
    Ok(())
}

#[cfg(test)]
fn requested_features() -> Value {
    features(None)
}

fn features(label: Option<&str>) -> Value {
    let mut value = json!({"netshield-level": 0, "randomized-nat": false, "split-tcp": false});
    if let Some(label) = label {
        value["bouncing"] = json!(label);
    }
    value
}

fn authorization_features(value: &Value, label: Option<&str>) -> Result<bool, GateFailure> {
    let received = match &value["status"]["features"] {
        Value::Null => return Ok(label.is_none()),
        Value::Object(received) => received,
        _ => return Err(GateFailure::Protocol),
    };
    // The Android client treats ordinary feature preferences separately from
    // the authenticated agent state. Missing/default preference echoes do not
    // invalidate connected. Keep explicit jail and selected-exit checks closed.
    match received.get("jail") {
        None | Some(Value::Bool(false)) => (),
        Some(Value::Bool(true)) => return Ok(false),
        _ => return Err(GateFailure::Protocol),
    }
    let Some(expected) = label else {
        return Ok(true);
    };
    match received.get("bouncing") {
        None | Some(Value::Null) => Ok(false),
        Some(Value::String(actual)) if usque_core::proton_vpn::valid_label(actual) => {
            Ok(actual == expected)
        }
        _ => Err(GateFailure::Protocol),
    }
}

#[cfg(test)]
async fn protocol_loop<S: AsyncRead + AsyncWrite + Unpin>(
    stream: &mut S,
    publisher: &watch::Sender<AgentState>,
    startup_deadline: Instant,
    expires_at: i64,
) -> Result<(), GateFailure> {
    protocol_loop_with_label(stream, publisher, startup_deadline, expires_at, None).await
}

async fn protocol_loop_with_label<S: AsyncRead + AsyncWrite + Unpin>(
    stream: &mut S,
    publisher: &watch::Sender<AgentState>,
    startup_deadline: Instant,
    expires_at: i64,
    label: Option<&str>,
) -> Result<(), GateFailure> {
    if label.is_some_and(|label| !usque_core::proton_vpn::valid_label(label)) {
        return Err(GateFailure::Protocol);
    }
    let (mut reader, mut writer) = tokio::io::split(stream);
    let mut authenticated = false;
    let mut authorization_deadline = startup_deadline;
    let mut waiting_failure = GateFailure::Transport;
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
            authorization_deadline
        };
        // Keep one decoder future alive across heartbeat ticks. Cancelling a
        // partially read length/body would lose framing on the TLS byte stream.
        let reading = read_frame(&mut reader);
        tokio::pin!(reading);
        let response = loop {
            tokio::select! {
                biased;
                _ = tokio::time::sleep_until(deadline) => return Err(if authenticated { GateFailure::Transport } else { waiting_failure }),
                response = &mut reading => break response?,
                _ = tokio::time::sleep_until(next_poll) => {
                    timeout_at(Instant::now() + IO_BUDGET,
                        write_frame(&mut writer, &json!({"status-get": {"features-statistics": false}})))
                        .await.map_err(|_| GateFailure::Transport)??;
                    next_poll = Instant::now() + STATUS_INTERVAL;
                    if authenticated { deadline = Instant::now() + IO_BUDGET; }
                }
            }
        };
        if unix_now()? >= expires_at {
            return Err(GateFailure::Certificate);
        }
        let connected = match status(&response) {
            Ok(connected) => connected,
            Err(reason) => {
                deauthorize(publisher);
                return Err(reason);
            }
        };
        let authorized = if connected {
            match authorization_features(&response, label) {
                Ok(authorized) => authorized,
                Err(reason) => {
                    deauthorize(publisher);
                    return Err(reason);
                }
            }
        } else {
            false
        };
        tracing::debug!(gate_event = "PROTON_AGENT_STATUS", connected, authorized,);
        if !connected {
            deauthorize(publisher);
            if let Some(code) = response["status"]["reason"]["code"]
                .as_u64()
                .or_else(|| response["error"]["code"].as_u64())
                .and_then(|code| u32::try_from(code).ok())
            {
                waiting_failure = GateFailure::Proton(ConnectionFailure::agent(code));
            }
        } else if !authorized {
            waiting_failure = GateFailure::Protocol;
        }
        if !features_sent {
            timeout_at(
                startup_deadline,
                write_frame(&mut writer, &json!({"features-set": features(label)})),
            )
            .await
            .map_err(|_| GateFailure::Transport)??;
            features_sent = true;
            // Request the resulting state without making preference echoes an
            // extra authorization handshake. A selected label still must match.
            timeout_at(
                startup_deadline,
                write_frame(
                    &mut writer,
                    &json!({"status-get": {"features-statistics": false}}),
                ),
            )
            .await
            .map_err(|_| GateFailure::Transport)??;
        }
        if authorized {
            authenticated = true;
            publisher.send_replace(AgentState::Connected);
        } else {
            if authenticated {
                authenticated = false;
                authorization_deadline = Instant::now() + Duration::from_secs(35);
            }
            deauthorize(publisher);
        }
    }
}

#[cfg(test)]
#[path = "agent_tests.rs"]
mod tests;
