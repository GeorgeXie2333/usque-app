//! OpenVPN's transport is supplied exclusively by a WARP internal network.
//! Decrypted packets use the same bounded mux as CONNECT-IP frontends.
use crate::h2::TransportError;
use crate::internal_network::InternalNetwork;
use crate::netstack::{ExternalPacketChannels, ManagedTunnelRuntime, RuntimeHealth, RuntimePath};
use crate::packet_batch::PacketBatch;
use bytes::Bytes;
use std::net::{IpAddr, SocketAddr};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::{mpsc, oneshot, watch};
use tokio::task::JoinHandle;
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;
use tokio_util::task::AbortOnDropHandle;
use usque_core::vpngate::{
    FinalNetworkParameters, GateFailure, GateStage, GateStatus, PreparedProfile,
};
use usque_openvpn::{Event, Input, NetworkConfig, Session};

static NEXT_GENERATION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

// Cancellation covers the whole operation, including awaits inside a selected
// event branch and the final native failure notification.
async fn until_cancelled<F: std::future::Future>(
    cancel: &CancellationToken,
    work: F,
) -> Option<F::Output> {
    tokio::select! {
        biased;
        _ = cancel.cancelled() => None,
        result = work => Some(result),
    }
}

/// Shared by the desktop and Android platform owners. A reconnect replaces
/// the whole final stack and platform assignment, using only the saved node.
#[derive(Default)]
pub struct VpnGateRetry {
    attempt: u32,
    deadline: Option<Instant>,
    stable_since: Option<Instant>,
}
impl VpnGateRetry {
    pub fn due(&mut self, status: &GateStatus, warp_ready: bool, now: Instant) -> bool {
        if status.stage == GateStage::Connected {
            let stable = *self.stable_since.get_or_insert(now);
            if now.duration_since(stable) >= Duration::from_secs(60) {
                self.attempt = 0;
            }
            self.deadline = None;
            return false;
        }
        self.stable_since = None;
        if status.stage != GateStage::Error || !status.failure.is_some_and(GateFailure::retryable) {
            self.deadline = None;
            return false;
        }
        let deadline = *self.deadline.get_or_insert_with(|| {
            now + crate::netstack::reconnect_delay(self.attempt.saturating_add(1))
        });
        if !warp_ready || now < deadline {
            return false;
        }
        self.deadline = None;
        self.attempt = self.attempt.saturating_add(1);
        true
    }
}

pub(crate) struct GateDriver {
    pub(crate) status: watch::Receiver<GateStatus>,
    status_tx: watch::Sender<GateStatus>,
    admission: watch::Sender<bool>,
    cancellation: CancellationToken,
    native_input: Input,
    diagnostic_id: u64,
    task: Option<JoinHandle<()>>,
}

impl GateDriver {
    pub(crate) async fn start(
        profile: &PreparedProfile,
        warp: InternalNetwork,
        transport_telemetry: crate::NetworkQualityTelemetry,
        status_sink: Option<watch::Sender<GateStatus>>,
        startup_cancel: &CancellationToken,
    ) -> Result<(Self, ManagedTunnelRuntime, FinalNetworkParameters), TransportError> {
        if startup_cancel.is_cancelled() {
            return Err(TransportError::TunnelClosed);
        }
        let native = Session::start(profile.content(), profile.remote)
            .map_err(|_| TransportError::VpnGate(GateFailure::Configuration))?;
        let cancellation = CancellationToken::new();
        let guard = cancellation.clone().drop_guard();
        let status_tx = status_sink.unwrap_or_else(|| watch::channel(GateStatus::default()).0);
        status_tx.send_modify(|s| s.stage = GateStage::ConnectingServer);
        let status = status_tx.subscribe();
        let (ready_tx, ready_rx) = oneshot::channel();
        let (admission, admitted) = watch::channel(false);
        let native_input = native.input();
        let diagnostic_id = NEXT_GENERATION.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let actor = Actor {
            diagnostic_id,
            native,
            remote: profile.remote,
            underlay_health: warp.health(),
            transport_telemetry,
            warp,
            status: status_tx.clone(),
            ready: Some(ready_tx),
            channels: None,
            network: None,
            generation: 0,
            connected: false,
            connection: None,
            cancellation: cancellation.clone(),
            reconnect_count: 0,
            admitted,
        };
        let task = tokio::spawn(actor.run());
        let mut driver = Self {
            status,
            status_tx,
            admission,
            cancellation,
            native_input,
            diagnostic_id,
            task: Some(task),
        };
        let result = tokio::select! {
            biased;
            _ = startup_cancel.cancelled() => {
                tracing::info!(gate_event = "STARTUP_CANCELLED", "VPN Gate startup cancelled");
                driver.shutdown().await;
                return Err(TransportError::TunnelClosed);
            }
            result = tokio::time::timeout(Duration::from_secs(35), ready_rx) => result,
        };
        match result {
            Ok(Ok(Ok((runtime, network)))) => {
                guard.disarm();
                Ok((driver, runtime, network))
            }
            other => {
                if other.is_err() {
                    tracing::warn!(gate_event = "STARTUP_TIMEOUT", "VPN Gate startup failed");
                }
                let reason = match other {
                    Ok(Ok(Err(reason))) => reason,
                    _ => GateFailure::Transport,
                };
                driver.shutdown().await;
                Err(TransportError::VpnGate(reason))
            }
        }
    }
    pub(crate) fn cancel(&self) {
        if !self.cancellation.is_cancelled() {
            tracing::info!(
                gate_event = "STOP_REQUESTED",
                gate_driver_id = self.diagnostic_id,
                "VPN Gate stop requested"
            );
        }
        self.cancellation.cancel();
        // Do not put stop behind a join that may itself be waiting for native
        // input capacity. The worker owns no OS transport or TUN resources.
        self.native_input.stop();
    }
    pub(crate) fn fail(&self, reason: GateFailure) {
        self.cancel();
        self.status_tx.send_modify(|s| {
            s.stage = GateStage::Error;
            s.failure = Some(reason);
            s.network = None;
        });
    }
    pub(crate) fn admit(&self) {
        self.admission.send_replace(true);
    }
    pub(crate) async fn shutdown(&mut self) {
        self.cancel();
        if let Some(task) = self.task.take() {
            let started = Instant::now();
            tracing::info!(
                gate_event = "TASK_JOIN_STARTED",
                gate_driver_id = self.diagnostic_id,
                "Waiting for VPN Gate task shutdown"
            );
            let _ = task.await;
            tracing::info!(
                gate_event = "TASK_JOIN_FINISHED",
                gate_driver_id = self.diagnostic_id,
                elapsed_ms = started.elapsed().as_millis() as u64,
                "VPN Gate task shutdown finished"
            );
        }
    }
}
impl Drop for GateDriver {
    fn drop(&mut self) {
        self.cancel();
    }
}

type Startup = Result<(ManagedTunnelRuntime, FinalNetworkParameters), GateFailure>;
struct Actor {
    diagnostic_id: u64,
    native: Session,
    warp: InternalNetwork,
    remote: SocketAddr,
    underlay_health: watch::Receiver<RuntimeHealth>,
    transport_telemetry: crate::NetworkQualityTelemetry,
    status: watch::Sender<GateStatus>,
    ready: Option<oneshot::Sender<Startup>>,
    channels: Option<ExternalPacketChannels>,
    network: Option<FinalNetworkParameters>,
    generation: u64,
    reconnect_count: u32,
    connected: bool,
    connection: Option<Connection>,
    cancellation: CancellationToken,
    admitted: watch::Receiver<bool>,
}
impl Actor {
    async fn run(mut self) {
        let cancel = self.cancellation.clone();
        let result = until_cancelled(&cancel, self.drive())
            .await
            .unwrap_or(Ok(()));
        self.connected = false;
        if let Err(reason) = result {
            self.status.send_modify(|s| {
                s.stage = GateStage::Error;
                s.failure = Some(reason);
                s.network = None;
            });
            if let Some(ready) = self.ready.take() {
                let _ = ready.send(Err(reason));
            }
            if let Some(channels) = &self.channels {
                let error = TransportError::VpnGate(reason);
                let path = self.path();
                channels.health.send_replace(RuntimeHealth::Failed {
                    last_path: path,
                    reconnect_count: self.reconnect_count,
                    message: error.to_string(),
                    failure: error.failure(Some(path.transport), Some(path.endpoint_family)),
                });
                channels.failure.send_replace(Some(error.to_string()));
            }
        }
        self.cancellation.cancel();
        if let Some(channels) = &self.channels {
            channels.cancellation.cancel();
        }
        self.native.input().stop();
        tracing::info!(
            gate_event = "NATIVE_STOP_REQUESTED",
            gate_driver_id = self.diagnostic_id,
            protocol_generation = self.generation,
            "VPN Gate native stop requested"
        );
        if let Some(connection) = self.connection.take() {
            tracing::info!(
                gate_event = "TRANSPORT_JOIN_STARTED",
                gate_driver_id = self.diagnostic_id,
                "Waiting for VPN Gate transport shutdown"
            );
            connection.shutdown().await;
            tracing::info!(
                gate_event = "TRANSPORT_JOIN_FINISHED",
                gate_driver_id = self.diagnostic_id,
                "VPN Gate transport shutdown finished"
            );
        }
        let started = Instant::now();
        match self.native.shutdown().await {
            Ok(()) => tracing::info!(
                gate_event = "NATIVE_STOP_FINISHED",
                gate_driver_id = self.diagnostic_id,
                elapsed_ms = started.elapsed().as_millis() as u64,
                "VPN Gate native worker exited"
            ),
            Err(error) => {
                tracing::warn!(gate_event = "NATIVE_STOP_FAILED", gate_driver_id = self.diagnostic_id, worker_pending = error == usque_openvpn::Error::ShutdownTimeout, failure_kind = ?error, elapsed_ms = started.elapsed().as_millis() as u64, "VPN Gate native shutdown did not complete successfully")
            }
        }
    }

    async fn drive(&mut self) -> Result<(), GateFailure> {
        loop {
            let channel_cancel = self
                .channels
                .as_ref()
                .map(|channels| channels.cancellation.clone());
            let input = self.native.input();
            tokio::select! {
                biased;
                _ = self.cancellation.cancelled() => return Ok(()),
                _ = async {
                    if let Some(cancel) = &channel_cancel { cancel.cancelled().await }
                    else { std::future::pending().await }
                } => return Ok(()),
                event = self.native.next_event() => self.event(event.map_err(|error| {
                    tracing::warn!(gate_event = "NATIVE_EVENT_FAILED", failure_kind = ?error, "VPN Gate native event could not be decoded");
                    GateFailure::Transport
                })?).await?,
                changed = self.underlay_health.changed() => {
                    if changed.is_err() { return Err(GateFailure::Transport); }
                    if !matches!(*self.underlay_health.borrow(), RuntimeHealth::Connected { .. }) {
                        if self.channels.is_some() { return Err(GateFailure::Transport); }
                        self.reconnecting();
                        if let Some(connection) = self.connection.take() { connection.shutdown().await; }
                        let _ = self.native.input().transport_failed(self.generation).await;
                    }
                }
                changed = self.admitted.changed() => {
                    if changed.is_err() { return Ok(()); }
                    if self.connected { self.publish_connected(); }
                }
                packet = async {
                    if let Some(channels) = &mut self.channels { channels.outgoing.recv().await }
                    else { std::future::pending().await }
                } => {
                    let Some(packet) = packet else { return Ok(()); };
                    // The decision follows DirectGatewayRouter. Unsupported
                    // proxied families are dropped here, never sent to WARP.
                    if self.connected && *self.admitted.borrow() && self.network.as_ref().is_some_and(|n| packet_family_supported(n, &packet)) {
                        tokio::select! {
                            _ = self.cancellation.cancelled() => return Ok(()),
                            result = input.send_ip(self.generation, &packet) => {
                                if result.is_err() { return Err(GateFailure::Transport); }
                                else if let Some(channels) = &self.channels { channels.counters.record_sent(packet.len()); }
                            }
                        }
                    }
                }
            }
        }
    }

    async fn event(&mut self, event: Event) -> Result<(), GateFailure> {
        match event {
            Event::Dial { generation } => {
                if self.channels.is_some() {
                    return Err(GateFailure::Transport);
                }
                self.connected = false;
                self.generation = generation;
                if let Some(connection) = self.connection.take() {
                    connection.shutdown().await;
                }
                let status_generation =
                    NEXT_GENERATION.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                self.status.send_modify(|s| {
                    s.generation = status_generation;
                    s.stage = GateStage::ConnectingServer;
                });
                self.connection = Some(Connection::start(
                    self.warp.clone(),
                    self.remote,
                    self.native.input(),
                    generation,
                    &self.cancellation,
                ));
            }
            Event::TransportPacket { generation, packet } if generation == self.generation => {
                if let Some(connection) = &self.connection {
                    tokio::select! {
                        _ = self.cancellation.cancelled() => return Ok(()),
                        result = connection.outgoing.send(packet) => {
                            if result.is_err() { let _ = self.native.input().transport_failed(generation).await; }
                        }
                    }
                }
            }
            Event::Network { generation, config } if generation == self.generation => {
                let network = final_network(config)?;
                if self.network.as_ref().is_some_and(|old| old != &network)
                    && self.channels.is_some()
                {
                    // Address changes require platform handoff and a fresh
                    // stack. Close admission before reporting this condition.
                    return Err(GateFailure::AddressChanged);
                }
                self.status.send_modify(|s| {
                    s.network = Some(network.clone());
                    s.stage = GateStage::ConfiguringNetwork;
                });
                self.network = Some(network);
            }
            Event::State {
                generation,
                name,
                error,
                fatal,
            } if generation == self.generation => {
                // The C ABI supplies library enum labels only, never free-form
                // peer info, certificate subjects, endpoints or configuration.
                tracing::info!(gate_event = %name, error, fatal, "VPN Gate protocol event");
                if name == "CONNECTED" {
                    let network = self.network.clone().ok_or(GateFailure::Configuration)?;
                    if !matches!(
                        *self.underlay_health.borrow(),
                        RuntimeHealth::Connected { .. }
                    ) {
                        return Err(GateFailure::Transport);
                    }
                    self.connected = true;
                    let path = self.path();
                    if let Some(ready) = self.ready.take() {
                        let (runtime, channels) = ManagedTunnelRuntime::for_external_packets(
                            path,
                            self.transport_telemetry.clone(),
                        );
                        self.channels = Some(channels);
                        self.publish_connected();
                        ready
                            .send(Ok((runtime, network)))
                            .map_err(|_| GateFailure::Transport)?;
                    } else {
                        self.publish_connected();
                    }
                } else if name == "RECONNECTING" || name == "DISCONNECTED" {
                    if self.channels.is_some() {
                        return Err(GateFailure::Transport);
                    }
                    self.reconnecting();
                } else if error || fatal {
                    let reason = event_failure(&name);
                    if fatal || !reason.retryable() || self.channels.is_some() {
                        return Err(reason);
                    }
                    self.reconnecting();
                } else if name == "CONNECTING" {
                    self.status
                        .send_modify(|s| s.stage = GateStage::Negotiating);
                }
            }
            Event::IpPacket { generation, packet }
                if generation == self.generation && self.connected =>
            {
                if let Some(channels) = &self.channels {
                    let length = packet.len();
                    // A full frontend queue drops an IP datagram; it cannot
                    // block the core's control channel or transport reader.
                    if channels
                        .incoming
                        .try_send(PacketBatch::single(packet), length)
                        .is_ok()
                    {
                        channels.counters.record_received(length);
                    }
                }
            }
            Event::Stopped => return Err(GateFailure::Transport),
            _ => {}
        }
        Ok(())
    }

    fn path(&self) -> RuntimePath {
        let mut path = self.underlay_health.borrow().path();
        path.ipv4_available = self.network.as_ref().is_some_and(|n| n.ipv4.is_some());
        path.ipv6_available = self.network.as_ref().is_some_and(|n| n.ipv6.is_some());
        path
    }
    fn publish_connected(&self) {
        let admitted = *self.admitted.borrow();
        if let Some(channels) = &self.channels {
            let path = self.path();
            let health = if admitted {
                RuntimeHealth::Connected {
                    path,
                    reconnect_count: self.reconnect_count,
                }
            } else {
                RuntimeHealth::Reconnecting {
                    last_path: path,
                    attempt: 0,
                    reconnect_count: self.reconnect_count,
                    reason: "VPN Gate platform configuration pending".into(),
                    failure: TransportError::VpnGate(GateFailure::Transport).failure(None, None),
                }
            };
            channels.health.send_replace(health);
        }
        // Connected is the admission acknowledgement. Readers may immediately
        // start a final-exit probe, which requires the health above to be ready.
        self.status.send_modify(|s| {
            s.stage = if admitted {
                GateStage::Connected
            } else {
                GateStage::ConfiguringNetwork
            };
            s.failure = None;
        });
    }
    fn reconnecting(&mut self) {
        if self.connected {
            self.reconnect_count = self.reconnect_count.saturating_add(1);
        }
        self.connected = false;
        self.status
            .send_modify(|s| s.stage = GateStage::Reconnecting);
        if let Some(channels) = &self.channels {
            let path = self.path();
            let error = TransportError::VpnGate(GateFailure::Transport);
            channels.health.send_replace(RuntimeHealth::Reconnecting {
                last_path: path,
                attempt: self.reconnect_count.max(1),
                reconnect_count: self.reconnect_count,
                reason: error.to_string(),
                failure: error.failure(Some(path.transport), Some(path.endpoint_family)),
            });
        }
    }
}

fn event_failure(name: &str) -> GateFailure {
    match name {
        "AUTH_FAILED" => GateFailure::Authentication,
        "CERT_VERIFY_FAIL" | "TLS_VERSION_MIN" | "TLS_CERT_VERIFY_FAIL" | "TLS_ALERT" => {
            GateFailure::Certificate
        }
        "CLIENT_SETUP" | "TUN_SETUP_FAILED" | "OPTIONS_ERROR" | "UNUSED_OPTIONS" => {
            GateFailure::Configuration
        }
        _ => GateFailure::Transport,
    }
}
fn final_network(config: NetworkConfig) -> Result<FinalNetworkParameters, GateFailure> {
    if !(1280..=9000).contains(&config.mtu)
        || config.ipv4.is_none() && config.ipv6.is_none()
        || config.ipv4.is_some_and(|ip| {
            ip.is_unspecified() || ip.is_loopback() || ip.is_multicast() || ip.is_broadcast()
        })
        || config
            .ipv6
            .is_some_and(|ip| ip.is_unspecified() || ip.is_loopback() || ip.is_multicast())
    {
        return Err(GateFailure::Configuration);
    }
    let mut network = FinalNetworkParameters {
        ipv4: config.ipv4,
        ipv6: config.ipv6,
        mtu: config.mtu,
        dns_servers: Vec::new(),
    };
    for ip in config.dns_servers {
        if network.supports(ip)
            && !ip.is_unspecified()
            && !ip.is_loopback()
            && !ip.is_multicast()
            && !matches!(ip, IpAddr::V4(address) if address.is_broadcast())
            && !network.dns_servers.contains(&ip)
        {
            network.dns_servers.push(ip);
        }
    }
    Ok(network)
}
fn packet_family_supported(network: &FinalNetworkParameters, packet: &[u8]) -> bool {
    match packet.first().map(|v| v >> 4) {
        Some(4) => network.ipv4.is_some(),
        Some(6) => network.ipv6.is_some(),
        _ => false,
    }
}

struct Connection {
    outgoing: mpsc::Sender<Bytes>,
    cancellation: CancellationToken,
    task: AbortOnDropHandle<()>,
}
impl Connection {
    fn start(
        warp: InternalNetwork,
        remote: SocketAddr,
        input: Input,
        generation: u64,
        parent: &CancellationToken,
    ) -> Self {
        // At most 64 * 65535 bytes, plus one packet in each I/O operation.
        let (outgoing, mut packets) = mpsc::channel::<Bytes>(64);
        let cancellation = parent.child_token();
        let cancel = cancellation.clone();
        let task = tokio::spawn(async move {
            let work = async {
                let stream = warp
                    .connect_address(remote, &cancel, Instant::now() + Duration::from_secs(15))
                    .await
                    .map_err(|error| {
                        tracing::warn!(gate_event = "TCP_DIAL_FAILED", failure_kind = ?error, "VPN Gate WARP dial failed");
                        std::io::Error::from(error)
                    })?;
                tracing::info!(
                    gate_event = "TCP_CONNECTED",
                    "VPN Gate WARP transport ready"
                );
                input
                    .transport_connected(generation)
                    .await
                    .map_err(|_| std::io::ErrorKind::ConnectionAborted)?;
                let (mut reader, mut writer) = tokio::io::split(stream);
                let sent_frames = std::sync::atomic::AtomicU64::new(0);
                let received_frames = std::sync::atomic::AtomicU64::new(0);
                let receive = async {
                    loop {
                        let packet = read_frame(&mut reader).await.inspect_err(|error| {
                            tracing::warn!(gate_event = "TCP_READ_FAILED", io_error_kind = ?error.kind(), "VPN Gate framed transport failed");
                        })?;
                        received_frames.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        input
                            .receive_transport(generation, &packet)
                            .await
                            .map_err(|_| std::io::ErrorKind::ConnectionAborted)?;
                    }
                    #[allow(unreachable_code)]
                    Ok::<(), std::io::Error>(())
                };
                let send = async {
                    while let Some(packet) = packets.recv().await {
                        write_frame(&mut writer, &packet).await.inspect_err(|error| {
                            tracing::warn!(gate_event = "TCP_WRITE_FAILED", io_error_kind = ?error.kind(), "VPN Gate framed transport failed");
                        })?;
                        sent_frames.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    }
                    Ok::<(), std::io::Error>(())
                };
                let result = tokio::select! { result = receive => result, result = send => result };
                tracing::info!(
                    gate_event = "TCP_CLOSED",
                    sent_frames = sent_frames.load(std::sync::atomic::Ordering::Relaxed),
                    received_frames = received_frames.load(std::sync::atomic::Ordering::Relaxed),
                    "VPN Gate transport ended"
                );
                result
            };
            until_cancelled(&cancel, async {
                let _ = work.await;
                let _ = input.transport_failed(generation).await;
            })
            .await;
        });
        Self {
            outgoing,
            cancellation,
            task: AbortOnDropHandle::new(task),
        }
    }
    async fn shutdown(self) {
        self.cancellation.cancel();
        let _ = self.task.await;
    }
}
async fn read_frame(reader: &mut (impl AsyncRead + Unpin)) -> std::io::Result<Vec<u8>> {
    let length = usize::from(reader.read_u16().await?);
    if length == 0 {
        return Err(std::io::ErrorKind::InvalidData.into());
    }
    let mut packet = vec![0; length];
    reader.read_exact(&mut packet).await?;
    Ok(packet)
}
async fn write_frame(writer: &mut (impl AsyncWrite + Unpin), packet: &[u8]) -> std::io::Result<()> {
    let length = u16::try_from(packet.len())
        .ok()
        .filter(|v| *v != 0)
        .ok_or(std::io::ErrorKind::InvalidData)?;
    writer.write_u16(length).await?;
    writer.write_all(packet).await?;
    writer.flush().await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tcp::{DialError, FlowClass, TcpDialer, TcpIo, TcpStream, TcpTarget};
    use std::sync::Arc;

    struct MemoryDialer {
        connected: bool,
        dialed: tokio::sync::Notify,
        peer: std::sync::Mutex<Option<oneshot::Sender<tokio::io::DuplexStream>>>,
    }
    impl TcpIo for tokio::io::DuplexStream {
        fn local_addr(&self) -> std::io::Result<SocketAddr> {
            Ok("127.0.0.1:12345".parse().unwrap())
        }
    }
    #[async_trait::async_trait]
    impl TcpDialer for MemoryDialer {
        async fn connect(
            &self,
            _: TcpTarget,
            _: Instant,
            _: &CancellationToken,
            _: FlowClass,
        ) -> Result<TcpStream, DialError> {
            self.dialed.notify_one();
            if !self.connected {
                return std::future::pending().await;
            }
            let (client, peer) = tokio::io::duplex(4096);
            self.peer
                .lock()
                .unwrap()
                .take()
                .unwrap()
                .send(peer)
                .unwrap();
            Ok(Box::new(client))
        }
    }

    #[tokio::test]
    async fn cancellation_closes_transport_during_backpressured_failure_notification() {
        let cancellation = CancellationToken::new();
        let cancel = cancellation.clone();
        let (stream, mut peer) = tokio::io::duplex(64);
        let (reporting, reported) = oneshot::channel();
        let task = tokio::spawn(async move {
            until_cancelled(&cancel, async move {
                // Model the selected TCP-failure branch with a full native
                // queue. The stream must close without that queue progressing.
                let _stream = stream;
                reporting.send(()).unwrap();
                std::future::pending::<()>().await;
            })
            .await;
        });
        let (outgoing, _) = mpsc::channel(1);
        let connection = Connection {
            outgoing,
            cancellation,
            task: AbortOnDropHandle::new(task),
        };
        reported.await.unwrap();
        tokio::time::timeout(Duration::from_secs(1), connection.shutdown())
            .await
            .expect("failure notification must remain cancellable");
        assert_eq!(peer.read(&mut [0; 1]).await.unwrap(), 0);
    }

    #[tokio::test]
    async fn cancellation_drops_an_event_before_it_can_publish_a_replacement() {
        let cancel = CancellationToken::new();
        let (entered, entering) = oneshot::channel();
        let (resume, waiting) = oneshot::channel::<()>();
        let operation = until_cancelled(&cancel, async {
            entered.send(()).unwrap();
            waiting.await.unwrap();
            panic!("cancelled event must not install a replacement connection");
        });
        let cancellation = async {
            entering.await.unwrap();
            cancel.cancel();
        };
        let (result, ()) = tokio::join!(operation, cancellation);
        assert!(result.is_none());
        assert!(resume.send(()).is_err());
    }

    #[tokio::test]
    async fn cancellation_joins_worker_during_dial_and_tls_wait() {
        let ca = include_str!("../../usque-openvpn/tests/fixtures/ca.crt");
        let cert = include_str!("../../usque-openvpn/tests/fixtures/client.crt");
        let key = include_str!("../../usque-openvpn/tests/fixtures/client.key");
        let prepared = usque_core::vpngate::prepare_profile(
            &format!("client\ndev tun\nproto tcp\nremote 8.8.8.8 1194\ncipher AES-128-CBC\nauth SHA1\n<ca>\n{ca}</ca>\n<cert>\n{cert}</cert>\n<key>\n{key}</key>\n"),
            "8.8.8.8".parse().unwrap(),
        ).unwrap();
        for connected in [false, true] {
            let (peer_tx, peer_rx) = oneshot::channel();
            let dialer = Arc::new(MemoryDialer {
                connected,
                dialed: tokio::sync::Notify::new(),
                peer: std::sync::Mutex::new(Some(peer_tx)),
            });
            let (health, receiver) = watch::channel(RuntimeHealth::Connected {
                path: RuntimePath {
                    transport: usque_core::Transport::Http3,
                    endpoint_family: usque_core::AddressFamily::Ipv4,
                    ipv4_available: true,
                    ipv6_available: false,
                },
                reconnect_count: 0,
            });
            let warp =
                InternalNetwork::for_streams(dialer.clone(), receiver, CancellationToken::new());
            let cancel = CancellationToken::new();
            let startup = GateDriver::start(
                &prepared,
                warp,
                crate::NetworkQualityTelemetry::default(),
                None,
                &cancel,
            );
            tokio::pin!(startup);
            let stop = async {
                dialer.dialed.notified().await;
                let mut peer = if connected {
                    let mut peer = peer_rx.await.unwrap();
                    assert_eq!(read_frame(&mut peer).await.unwrap().len(), 14);
                    Some(peer)
                } else {
                    None
                };
                cancel.cancel();
                if let Some(peer) = &mut peer {
                    assert_eq!(
                        peer.read(&mut [0; 1]).await.unwrap(),
                        0,
                        "cancel closes the WARP stream"
                    );
                }
            };
            let (result, ()) = tokio::time::timeout(Duration::from_secs(3), async {
                tokio::join!(startup, stop)
            })
            .await
            .expect("cancel must not wait for the 35-second startup deadline");
            assert!(matches!(result, Err(TransportError::TunnelClosed)));
            drop(health);
        }
    }
    #[test]
    fn retry_waits_for_warp_preserves_backoff_and_stops_on_terminal_errors() {
        let mut retry = VpnGateRetry::default();
        let start = Instant::now();
        let mut status = GateStatus {
            stage: GateStage::Error,
            failure: Some(GateFailure::Transport),
            ..Default::default()
        };
        assert!(!retry.due(&status, true, start));
        assert!(!retry.due(&status, false, start + Duration::from_secs(5)));
        assert!(retry.due(&status, true, start + Duration::from_secs(5)));
        assert!(!retry.due(&status, true, start + Duration::from_secs(5)));
        assert_eq!(retry.attempt, 1);
        status.failure = Some(GateFailure::Certificate);
        assert!(!retry.due(&status, true, start + Duration::from_secs(120)));
        status.stage = GateStage::Connected;
        assert!(!retry.due(&status, true, start + Duration::from_secs(121)));
        assert!(!retry.due(&status, true, start + Duration::from_secs(182)));
        assert_eq!(retry.attempt, 0);
    }
    #[tokio::test]
    async fn framing_survives_partial_writes_and_consecutive_frames() {
        let (mut writer, mut reader) = tokio::io::duplex(3);
        let sender = tokio::spawn(async move {
            write_frame(&mut writer, &[7; 100]).await.unwrap();
            write_frame(&mut writer, &[8; 9]).await.unwrap();
        });
        assert_eq!(read_frame(&mut reader).await.unwrap(), vec![7; 100]);
        assert_eq!(read_frame(&mut reader).await.unwrap(), vec![8; 9]);
        sender.await.unwrap();
        assert!(read_frame(&mut reader).await.is_err());
    }
    #[tokio::test]
    async fn rejects_empty_and_truncated_frames() {
        assert!(read_frame(&mut &[0_u8, 0][..]).await.is_err());
        assert!(read_frame(&mut &[0_u8, 5, 1, 2][..]).await.is_err());
        assert!(write_frame(&mut tokio::io::sink(), &[]).await.is_err());
    }
    #[test]
    fn final_addresses_and_address_family_gate_do_not_inherit_warp_addresses() {
        let network = final_network(NetworkConfig {
            ipv4: Some("10.8.0.2".parse().unwrap()),
            ipv6: None,
            mtu: 1500,
            dns_servers: vec![
                "1.1.1.1".parse::<IpAddr>().unwrap(),
                "2606:4700:4700::1111".parse().unwrap(),
            ],
        })
        .unwrap();
        assert_eq!(network.dns_servers.len(), 1);
        assert!(packet_family_supported(&network, &[0x45]));
        assert!(!packet_family_supported(&network, &[0x60]));
        assert!(!event_failure("AUTH_FAILED").retryable());
        assert!(!event_failure("CERT_VERIFY_FAIL").retryable());
    }
    #[test]
    fn broadcast_only_pushed_dns_leaves_the_configured_fallback_available() {
        let network = final_network(NetworkConfig {
            ipv4: Some("10.8.0.2".parse().unwrap()),
            ipv6: None,
            mtu: 1500,
            dns_servers: vec!["255.255.255.255".parse().unwrap()],
        })
        .unwrap();
        // install_gate uses the configured DNS servers when this list is empty.
        assert!(network.dns_servers.is_empty());
    }
    #[test]
    fn filters_broadcast_dns_without_discarding_private_or_public_resolvers() {
        let private: IpAddr = "10.8.0.1".parse().unwrap();
        let public: IpAddr = "1.1.1.1".parse().unwrap();
        let network = final_network(NetworkConfig {
            ipv4: Some("10.8.0.2".parse().unwrap()),
            ipv6: None,
            mtu: 1500,
            dns_servers: vec![
                "255.255.255.255".parse().unwrap(),
                private,
                public,
                private,
                "2606:4700:4700::1111".parse().unwrap(),
            ],
        })
        .unwrap();
        assert_eq!(network.dns_servers, vec![private, public]);
    }
}
