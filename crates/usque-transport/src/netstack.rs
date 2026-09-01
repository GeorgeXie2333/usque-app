use std::collections::VecDeque;
use std::future::Future;
use std::net::IpAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use bytes::Bytes;
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;
use tokio::time::{Instant, sleep, timeout};
use tokio_util::sync::CancellationToken;
use ts_netstack_smoltcp::netcore::{
    Channel, Config, HasChannel, NetstackControl, TcpBufferMetrics, TcpBufferPolicy, TcpBufferTier,
};
use ts_netstack_smoltcp::{
    Netstack, WakingPipe, WakingPipeDev, WakingPipeReceiver, WakingPipeSender,
};
use usque_core::{
    AddressFamily, IpPolicy, Profile, Transport, TransportFailure, TransportFailureCode,
    TransportPolicy, TransportStage,
};
use usque_protocol::{IpAddressRange, IpPrefix, PeerNetworkState};

use crate::geo_direct::GeoDirectPolicy;
use crate::h2::{MasqueTlsIdentity, TransportError, connect_h2_with_protector};
use crate::h3::connect_h3_with_protector;
use crate::network_quality::{NetworkQualitySnapshot, spawn_network_quality_sampler};
use crate::packet_batch::{
    MAX_PACKET_BATCH_BYTES, PACKET_BATCH_CHANNEL_CAPACITY, PacketBatch, PacketBatchResult,
};
use crate::pin_refresh::EndpointPinRefresher;
use crate::queue_metrics::{
    QueueKind, TrackedReceiver, TrackedSendErrorKind, TrackedSender, tracked_channel,
};
use crate::socket::SocketProtector;
use crate::telemetry::{
    ConnectionAttemptTelemetry, ConnectionEventPath, ConnectionEventType, ConnectionTelemetry,
    ConnectionTimelineSnapshot,
};
use crate::tunnel::{BatchSendFuture, MasqueTunnel};

const HAPPY_EYEBALLS_DELAY: Duration = Duration::from_millis(250);
const STACK_COMMAND_CAPACITY: usize = 256;
const PACKET_SEND_TIMEOUT: Duration = Duration::from_secs(10);
const STABLE_CONNECTION_RESET: Duration = Duration::from_secs(60);
const H3_PROBE_INTERVAL: Duration = Duration::from_secs(10 * 60);
const H3_PROBE_JITTER_PERCENT: u64 = 20;
const RAW_PACKET_CHANNEL_CAPACITY: usize = 1_024;
const RAW_PACKET_CHANNEL_BYTE_CAPACITY: usize = RAW_PACKET_CHANNEL_CAPACITY * u16::MAX as usize;
const PROXY_PACKET_PIPE_CAPACITY: usize = 1_024;
const PREFERRED_TCP_RECEIVE_BUFFER: usize = 4 * 1024 * 1024;
const PREFERRED_TCP_TRANSMIT_BUFFER: usize = 1024 * 1024;
const FALLBACK_TCP_RECEIVE_BUFFER: usize = 1024 * 1024;
const FALLBACK_TCP_TRANSMIT_BUFFER: usize = 256 * 1024;
const RECONNECT_DELAYS: [Duration; 6] = [
    Duration::from_secs(1),
    Duration::from_secs(2),
    Duration::from_secs(4),
    Duration::from_secs(8),
    Duration::from_secs(15),
    Duration::from_secs(30),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuntimePath {
    pub transport: Transport,
    /// Physical address family of the one active MASQUE connection.
    pub endpoint_family: AddressFamily,
    /// CONNECT-IP payload families, independent from `endpoint_family`.
    pub ipv4_available: bool,
    pub ipv6_available: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeHealth {
    Connected {
        path: RuntimePath,
        reconnect_count: u32,
    },
    Reconnecting {
        last_path: RuntimePath,
        attempt: u32,
        reconnect_count: u32,
        reason: String,
        failure: TransportFailure,
    },
    Failed {
        last_path: RuntimePath,
        reconnect_count: u32,
        message: String,
        failure: TransportFailure,
    },
}

impl RuntimeHealth {
    pub fn path(&self) -> RuntimePath {
        match self {
            Self::Connected { path, .. } => *path,
            Self::Reconnecting { last_path, .. } | Self::Failed { last_path, .. } => *last_path,
        }
    }

    pub fn reconnect_count(&self) -> u32 {
        match self {
            Self::Connected {
                reconnect_count, ..
            }
            | Self::Reconnecting {
                reconnect_count, ..
            }
            | Self::Failed {
                reconnect_count, ..
            } => *reconnect_count,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TrafficSnapshot {
    pub bytes_sent: u64,
    pub bytes_received: u64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ProxyPerformanceSnapshot {
    pub preferred_tcp_buffer_bytes: usize,
    pub total_tcp_buffer_bytes: usize,
    pub preferred_tcp_sockets: usize,
    pub fallback_tcp_sockets: usize,
    pub rejected_tcp_sockets: usize,
    pub http_pool_hits: u64,
    pub http_pool_misses: u64,
    pub http_stale_retries: u64,
    pub http_busy_rejections: u64,
    pub send_queue_high_watermark: u64,
    pub send_queue_drop_count: u64,
    pub fallback_count: u32,
    pub network_change_count: u32,
}

#[derive(Debug, Default)]
pub(crate) struct TrafficCounters {
    sent: AtomicU64,
    received: AtomicU64,
}

impl TrafficCounters {
    pub(crate) fn snapshot(&self) -> TrafficSnapshot {
        TrafficSnapshot {
            bytes_sent: self.sent.load(Ordering::Relaxed),
            bytes_received: self.received.load(Ordering::Relaxed),
        }
    }

    pub(crate) fn record_sent(&self, bytes: usize) {
        self.sent.fetch_add(bytes as u64, Ordering::Relaxed);
    }

    pub(crate) fn record_received(&self, bytes: usize) {
        self.received.fetch_add(bytes as u64, Ordering::Relaxed);
    }
}

pub(crate) struct PacketStack {
    pub(crate) channel: Channel,
    pub(crate) protector: Arc<dyn SocketProtector>,
    pub(crate) geo_policy: Arc<GeoDirectPolicy>,
    pub(crate) cancellation: CancellationToken,
    pub(crate) failure: watch::Receiver<Option<String>>,
    pub(crate) counters: Arc<TrafficCounters>,
    tcp_buffer_metrics: TcpBufferMetrics,
    health: watch::Receiver<RuntimeHealth>,
    telemetry: ConnectionTelemetry,
    quality: watch::Receiver<NetworkQualitySnapshot>,
    // Retain a receiver so the supervisor can publish control state even
    // though proxy modes do not currently expose route diagnostics.
    _control: watch::Receiver<PeerNetworkState>,
    tasks: Vec<JoinHandle<()>>,
}

impl PacketStack {
    /// Starts only the local smoltcp side of a shared MASQUE runtime.
    ///
    /// The returned pipe must be driven by the runtime's packet multiplexer;
    /// this stack deliberately does not create a second remote tunnel.
    pub(crate) async fn start_detached(
        profile: &Profile,
        assigned_addresses: (std::net::Ipv4Addr, std::net::Ipv6Addr),
        monitor: &ManagedTunnelMonitor,
        parent_cancellation: &CancellationToken,
        protector: Arc<dyn SocketProtector>,
        geo_policy: Arc<GeoDirectPolicy>,
    ) -> Result<(Self, WakingPipe), TransportError> {
        let (assigned_ipv4, assigned_ipv6) = assigned_addresses;
        let (config, tcp_buffer_metrics) = proxy_netstack_config(profile);
        let (stack, pipe) = bounded_piped(config);
        let channel = stack.command_channel();
        let stack_task = stack.spawn_tokio();
        channel
            .set_ips([IpAddr::V4(assigned_ipv4), IpAddr::V6(assigned_ipv6)])
            .await
            .map_err(|error| TransportError::Netstack(error.to_string()))?;

        Ok((
            Self {
                channel,
                protector,
                geo_policy,
                cancellation: parent_cancellation.child_token(),
                failure: monitor.failure.clone(),
                counters: Arc::clone(&monitor.counters),
                tcp_buffer_metrics,
                health: monitor.health.clone(),
                telemetry: monitor.telemetry.clone(),
                quality: monitor.quality.clone(),
                _control: monitor.control.clone(),
                tasks: vec![stack_task],
            },
            pipe,
        ))
    }

    pub(crate) async fn start_with_refresh(
        profile: &Profile,
        identity: Arc<MasqueTlsIdentity>,
        protector: Arc<dyn SocketProtector>,
        pin_refresher: Option<Arc<dyn EndpointPinRefresher>>,
    ) -> Result<Self, TransportError> {
        let telemetry = ConnectionTelemetry::default();
        telemetry.reset_attempt();
        let (tunnel, endpoint_family, identity, pin_refresh_attempted) =
            connect_initial_with_refresh(
                profile,
                identity,
                Arc::clone(&protector),
                pin_refresher.as_ref(),
                &telemetry,
            )
            .await?;
        let transport = tunnel.transport();
        let initial_path = runtime_path(transport, endpoint_family);
        let (config, tcp_buffer_metrics) = proxy_netstack_config(profile);
        let (stack, pipe) = bounded_piped(config);
        let channel = stack.command_channel();
        let stack_task = stack.spawn_tokio();
        channel
            .set_ips([
                IpAddr::V4(identity.assigned_ipv4),
                IpAddr::V6(identity.assigned_ipv6),
            ])
            .await
            .map_err(|error| TransportError::Netstack(error.to_string()))?;

        let cancellation = CancellationToken::new();
        let (failure_tx, failure) = watch::channel(None);
        let (health_tx, health) = watch::channel(RuntimeHealth::Connected {
            path: initial_path,
            reconnect_count: 0,
        });
        let (control_tx, control) = watch::channel(PeerNetworkState::default());
        let counters = Arc::new(TrafficCounters::default());
        let (quality, quality_task) =
            spawn_network_quality_sampler(telemetry.network_quality(), cancellation.child_token());
        let mut tasks = vec![stack_task, quality_task];
        tasks.push(tokio::spawn(run_transport_supervisor(
            tunnel,
            endpoint_family,
            PacketIo::from_pipe(pipe),
            SupervisorContext {
                profile: profile.clone(),
                identity,
                protector: Arc::clone(&protector),
                pin_refresher,
                pin_refresh_attempted,
                cancellation: cancellation.clone(),
                failure_tx: failure_tx.clone(),
                health_tx,
                control_tx,
                counters: Arc::clone(&counters),
                telemetry: telemetry.clone(),
            },
        )));

        let watcher_cancel = cancellation.clone();
        let mut terminal_failure = failure_tx.subscribe();
        tasks.push(tokio::spawn(async move {
            loop {
                if terminal_failure.borrow().is_some() {
                    watcher_cancel.cancel();
                    break;
                }
                if terminal_failure.changed().await.is_err() {
                    break;
                }
            }
        }));

        Ok(Self {
            channel,
            protector,
            geo_policy: Arc::new(GeoDirectPolicy::disabled()),
            cancellation,
            failure,
            counters,
            tcp_buffer_metrics,
            health,
            telemetry,
            quality,
            _control: control,
            tasks,
        })
    }

    pub(crate) fn path(&self) -> RuntimePath {
        self.health.borrow().path()
    }

    pub(crate) fn health(&self) -> RuntimeHealth {
        self.health.borrow().clone()
    }

    pub(crate) fn subscribe_health(&self) -> watch::Receiver<RuntimeHealth> {
        self.health.clone()
    }

    pub(crate) fn performance(&self) -> ProxyPerformanceSnapshot {
        let snapshot = self.tcp_buffer_metrics.snapshot();
        let telemetry = self.telemetry.snapshot();
        ProxyPerformanceSnapshot {
            preferred_tcp_buffer_bytes: snapshot.preferred_bytes,
            total_tcp_buffer_bytes: snapshot.total_bytes,
            preferred_tcp_sockets: snapshot.preferred_sockets,
            fallback_tcp_sockets: snapshot.fallback_sockets,
            rejected_tcp_sockets: snapshot.rejected_sockets,
            http_pool_hits: 0,
            http_pool_misses: 0,
            http_stale_retries: 0,
            http_busy_rejections: 0,
            send_queue_high_watermark: telemetry.metrics.send_queue_high_watermark,
            send_queue_drop_count: telemetry.metrics.send_queue_drop_count,
            fallback_count: telemetry.metrics.fallback_count,
            network_change_count: telemetry.metrics.network_change_count,
        }
    }

    pub(crate) fn network_quality(&self) -> NetworkQualitySnapshot {
        self.quality.borrow().clone()
    }

    pub(crate) fn cancel_immediately(&mut self) {
        self.cancellation.cancel();
        for task in &self.tasks {
            task.abort();
        }
    }

    pub(crate) async fn shutdown(&mut self) {
        self.cancel_immediately();
        for task in self.tasks.drain(..) {
            let _ = task.await;
        }
    }
}

pub(crate) fn proxy_netstack_config(profile: &Profile) -> (Config, TcpBufferMetrics) {
    let (preferred_budget, total_budget) = tcp_buffer_budgets();
    let metrics = TcpBufferMetrics::default();
    let config = Config {
        command_channel_capacity: Some(STACK_COMMAND_CAPACITY),
        mtu: usize::from(profile.mtu),
        // Retained as a conservative default for the upstream listener path.
        // Outbound proxy connections use the bounded asymmetric policy below.
        tcp_buffer_size: FALLBACK_TCP_RECEIVE_BUFFER,
        tcp_buffer_policy: Some(TcpBufferPolicy {
            preferred: TcpBufferTier {
                receive: PREFERRED_TCP_RECEIVE_BUFFER,
                transmit: PREFERRED_TCP_TRANSMIT_BUFFER,
            },
            fallback: TcpBufferTier {
                receive: FALLBACK_TCP_RECEIVE_BUFFER,
                transmit: FALLBACK_TCP_TRANSMIT_BUFFER,
            },
            preferred_budget,
            total_budget,
        }),
        tcp_buffer_metrics: Some(metrics.clone()),
        tcp_nagle_enabled: false,
        udp_buffer_size: 64 * 1024,
        udp_message_count: 128,
        ..Config::default()
    };
    (config, metrics)
}

pub(crate) fn bounded_piped(config: Config) -> (Netstack<WakingPipeDev>, WakingPipe) {
    let (stack_pipe, remote_pipe) = WakingPipe::bounded(PROXY_PACKET_PIPE_CAPACITY);
    let device = WakingPipeDev {
        pipe: stack_pipe,
        mtu: config.mtu,
        medium: ts_netstack_smoltcp::netcore::smoltcp::phy::Medium::Ip,
    };
    (Netstack::new(device, config), remote_pipe)
}

const fn tcp_buffer_budgets() -> (usize, usize) {
    #[cfg(all(target_os = "android", target_pointer_width = "32"))]
    {
        (32 * 1024 * 1024, 48 * 1024 * 1024)
    }
    #[cfg(all(target_os = "android", target_pointer_width = "64"))]
    {
        (96 * 1024 * 1024, 128 * 1024 * 1024)
    }
    #[cfg(not(target_os = "android"))]
    {
        (192 * 1024 * 1024, 256 * 1024 * 1024)
    }
}

impl Drop for PacketStack {
    fn drop(&mut self) {
        self.cancel_immediately();
    }
}

/// A reconnecting, single-channel MASQUE runtime for platform TUN adapters.
///
/// Unlike [`PacketStack`], this boundary does not run smoltcp. Packets supplied
/// here originate from the platform TUN; the transport supervisor validates
/// them and decrements TTL/hop-limit immediately before encapsulation.
pub struct ManagedTunnelRuntime {
    outgoing: Option<TrackedSender<Bytes>>,
    incoming: TrackedReceiver<PacketBatch>,
    pending_incoming: PacketBatch,
    cancellation: CancellationToken,
    failure: watch::Receiver<Option<String>>,
    health: watch::Receiver<RuntimeHealth>,
    control: watch::Receiver<PeerNetworkState>,
    counters: Arc<TrafficCounters>,
    telemetry: ConnectionTelemetry,
    quality: watch::Receiver<NetworkQualitySnapshot>,
    tasks: Vec<JoinHandle<()>>,
}

/// Read-only, cloneable view of a managed tunnel's live state.
///
/// Platform packet pumps own the mutable [`ManagedTunnelRuntime`] so they can
/// receive packets continuously. The Engine control plane retains this
/// monitor to publish health and traffic without sharing mutable tunnel I/O.
#[derive(Clone)]
pub struct ManagedTunnelMonitor {
    failure: watch::Receiver<Option<String>>,
    health: watch::Receiver<RuntimeHealth>,
    control: watch::Receiver<PeerNetworkState>,
    counters: Arc<TrafficCounters>,
    telemetry: ConnectionTelemetry,
    quality: watch::Receiver<NetworkQualitySnapshot>,
}

#[derive(Clone)]
pub struct ManagedTunnelSender {
    outgoing: TrackedSender<Bytes>,
    telemetry: ConnectionTelemetry,
}

impl ManagedTunnelSender {
    pub async fn send_packet(&self, packet: &[u8]) -> Result<(), TransportError> {
        crate::h2::validate_ip_packet(packet)?;
        self.telemetry
            .network_quality()
            .record_borrowed_to_owned_copy(packet.len());
        self.send_owned_packet(Bytes::copy_from_slice(packet)).await
    }

    pub(crate) async fn send_owned_packet(&self, packet: Bytes) -> Result<(), TransportError> {
        crate::h2::validate_ip_packet(&packet)?;
        let packet_bytes = packet.len();
        let queued = self
            .outgoing
            .max_capacity()
            .saturating_sub(self.outgoing.capacity());
        self.telemetry.observe_queue_depth(queued);
        if self.outgoing.capacity() == 0 {
            self.telemetry.record_queue_saturated(queued);
        }
        self.outgoing
            .send(packet, packet_bytes)
            .await
            .map_err(|error| match error.kind {
                TrackedSendErrorKind::Closed
                | TrackedSendErrorKind::Cancelled
                | TrackedSendErrorKind::Full
                | TrackedSendErrorKind::ByteLimit => TransportError::TunnelClosed,
            })
    }
}

impl ManagedTunnelMonitor {
    #[cfg(test)]
    pub(crate) fn stub() -> Self {
        let path = RuntimePath {
            transport: Transport::Http2,
            endpoint_family: AddressFamily::Ipv4,
            ipv4_available: true,
            ipv6_available: true,
        };
        let (_failure_tx, failure) = watch::channel(None);
        let (_health_tx, health) = watch::channel(RuntimeHealth::Connected {
            path,
            reconnect_count: 0,
        });
        let (_control_tx, control) = watch::channel(PeerNetworkState::default());
        let telemetry = ConnectionTelemetry::default();
        let quality = initial_quality_receiver(&telemetry);
        Self {
            failure,
            health,
            control,
            counters: Arc::new(TrafficCounters::default()),
            telemetry,
            quality,
        }
    }

    pub fn path(&self) -> RuntimePath {
        self.health.borrow().path()
    }

    pub fn health(&self) -> RuntimeHealth {
        self.health.borrow().clone()
    }

    pub fn statistics(&self) -> TrafficSnapshot {
        self.counters.snapshot()
    }

    pub fn failure(&self) -> Option<String> {
        self.failure.borrow().clone()
    }

    pub fn control_state(&self) -> PeerNetworkState {
        self.control.borrow().clone()
    }

    pub fn connection_timeline(&self) -> ConnectionTimelineSnapshot {
        self.telemetry.snapshot()
    }

    pub fn network_quality(&self) -> NetworkQualitySnapshot {
        self.quality.borrow().clone()
    }

    pub fn subscribe_network_quality(&self) -> watch::Receiver<NetworkQualitySnapshot> {
        self.quality.clone()
    }

    pub(crate) fn network_quality_telemetry(&self) -> crate::NetworkQualityTelemetry {
        self.telemetry.network_quality()
    }
}

#[cfg(test)]
fn initial_quality_receiver(
    telemetry: &ConnectionTelemetry,
) -> watch::Receiver<NetworkQualitySnapshot> {
    let mut sampler = crate::NetworkQualitySampler::new(telemetry.network_quality());
    watch::channel(sampler.sample()).1
}

impl ManagedTunnelRuntime {
    #[cfg(test)]
    pub(crate) fn packet_mux_test_channels(
        outgoing_capacity: usize,
    ) -> (Self, TrackedReceiver<Bytes>, TrackedSender<PacketBatch>) {
        let telemetry = ConnectionTelemetry::default();
        let quality_snapshot = initial_quality_receiver(&telemetry);
        let quality = telemetry.network_quality();
        let outgoing_metrics = quality.register_queue(
            QueueKind::TransportOutgoingPackets,
            outgoing_capacity,
            outgoing_capacity * u16::MAX as usize,
        );
        let incoming_metrics = quality.register_queue(
            QueueKind::TransportToTun,
            PACKET_BATCH_CHANNEL_CAPACITY,
            PACKET_BATCH_CHANNEL_CAPACITY * MAX_PACKET_BATCH_BYTES,
        );
        let (outgoing, outgoing_rx) = tracked_channel(outgoing_metrics);
        let (incoming_tx, incoming) = tracked_channel(incoming_metrics);
        let (_failure_tx, failure) = watch::channel(None);
        let path = runtime_path(Transport::Http3, AddressFamily::Ipv4);
        let (_health_tx, health) = watch::channel(RuntimeHealth::Connected {
            path,
            reconnect_count: 0,
        });
        let (_control_tx, control) = watch::channel(PeerNetworkState::default());
        (
            Self {
                outgoing: Some(outgoing),
                incoming,
                pending_incoming: PacketBatch::new(),
                cancellation: CancellationToken::new(),
                failure,
                health,
                control,
                counters: Arc::new(TrafficCounters::default()),
                telemetry,
                quality: quality_snapshot,
                tasks: Vec::new(),
            },
            outgoing_rx,
            incoming_tx,
        )
    }

    pub async fn start(
        profile: &Profile,
        identity: MasqueTlsIdentity,
    ) -> Result<Self, TransportError> {
        Self::start_with_protector(profile, identity, crate::socket::noop_socket_protector()).await
    }

    pub async fn start_with_protector(
        profile: &Profile,
        identity: MasqueTlsIdentity,
        protector: Arc<dyn SocketProtector>,
    ) -> Result<Self, TransportError> {
        Self::start_with_refresh(profile, identity, protector, None).await
    }

    pub async fn start_with_refresh(
        profile: &Profile,
        identity: MasqueTlsIdentity,
        protector: Arc<dyn SocketProtector>,
        pin_refresher: Option<Arc<dyn EndpointPinRefresher>>,
    ) -> Result<Self, TransportError> {
        let identity = Arc::new(identity);
        let telemetry = ConnectionTelemetry::default();
        telemetry.reset_attempt();
        let (tunnel, endpoint_family, identity, pin_refresh_attempted) =
            connect_initial_with_refresh(
                profile,
                identity,
                Arc::clone(&protector),
                pin_refresher.as_ref(),
                &telemetry,
            )
            .await?;
        let path = runtime_path(tunnel.transport(), endpoint_family);
        let quality = telemetry.network_quality();
        let outgoing_metrics = quality.register_queue(
            QueueKind::TransportOutgoingPackets,
            RAW_PACKET_CHANNEL_CAPACITY,
            RAW_PACKET_CHANNEL_BYTE_CAPACITY,
        );
        let incoming_metrics = quality.register_queue(
            QueueKind::TransportToTun,
            PACKET_BATCH_CHANNEL_CAPACITY,
            PACKET_BATCH_CHANNEL_CAPACITY * MAX_PACKET_BATCH_BYTES,
        );
        let (outgoing, outgoing_rx) = tracked_channel(outgoing_metrics);
        let (incoming_tx, incoming) = tracked_channel(incoming_metrics);
        let cancellation = CancellationToken::new();
        let (failure_tx, failure) = watch::channel(None);
        let (health_tx, health) = watch::channel(RuntimeHealth::Connected {
            path,
            reconnect_count: 0,
        });
        let (control_tx, control) = watch::channel(PeerNetworkState::default());
        let counters = Arc::new(TrafficCounters::default());
        let (quality_updates, quality_task) =
            spawn_network_quality_sampler(quality, cancellation.child_token());
        let mut tasks = vec![
            quality_task,
            tokio::spawn(run_transport_supervisor(
                tunnel,
                endpoint_family,
                PacketIo::Channel {
                    outgoing: outgoing_rx,
                    incoming: incoming_tx,
                    buffered_outgoing: None,
                },
                SupervisorContext {
                    profile: profile.clone(),
                    identity,
                    protector,
                    pin_refresher,
                    pin_refresh_attempted,
                    cancellation: cancellation.clone(),
                    failure_tx: failure_tx.clone(),
                    health_tx,
                    control_tx,
                    counters: Arc::clone(&counters),
                    telemetry: telemetry.clone(),
                },
            )),
        ];
        let watcher_cancel = cancellation.clone();
        let mut terminal_failure = failure_tx.subscribe();
        tasks.push(tokio::spawn(async move {
            loop {
                if terminal_failure.borrow().is_some() {
                    watcher_cancel.cancel();
                    break;
                }
                if terminal_failure.changed().await.is_err() {
                    break;
                }
            }
        }));

        Ok(Self {
            outgoing: Some(outgoing),
            incoming,
            pending_incoming: PacketBatch::new(),
            cancellation,
            failure,
            health,
            control,
            counters,
            telemetry,
            quality: quality_updates,
            tasks,
        })
    }

    pub async fn send_packet(&self, packet: &[u8]) -> Result<(), TransportError> {
        self.packet_sender()?.send_packet(packet).await
    }

    pub fn packet_sender(&self) -> Result<ManagedTunnelSender, TransportError> {
        Ok(ManagedTunnelSender {
            outgoing: self
                .outgoing
                .as_ref()
                .ok_or(TransportError::TunnelClosed)?
                .clone(),
            telemetry: self.telemetry.clone(),
        })
    }

    pub async fn receive_packet(&mut self) -> Result<Bytes, TransportError> {
        loop {
            if let Some(packet) = self.pending_incoming.pop_front() {
                return Ok(packet);
            }
            self.pending_incoming = self
                .incoming
                .recv()
                .await
                .ok_or(TransportError::TunnelClosed)?;
        }
    }

    pub(crate) async fn receive_batch(&mut self) -> Result<PacketBatch, TransportError> {
        if !self.pending_incoming.is_empty() {
            return Ok(std::mem::take(&mut self.pending_incoming));
        }
        self.incoming
            .recv()
            .await
            .ok_or(TransportError::TunnelClosed)
    }

    pub fn path(&self) -> RuntimePath {
        self.health.borrow().path()
    }

    pub fn health(&self) -> RuntimeHealth {
        self.health.borrow().clone()
    }

    pub fn statistics(&self) -> TrafficSnapshot {
        self.counters.snapshot()
    }

    pub fn failure(&self) -> Option<String> {
        self.failure.borrow().clone()
    }

    pub fn control_state(&self) -> PeerNetworkState {
        self.control.borrow().clone()
    }

    pub fn connection_timeline(&self) -> ConnectionTimelineSnapshot {
        self.telemetry.snapshot()
    }

    pub fn monitor(&self) -> ManagedTunnelMonitor {
        ManagedTunnelMonitor {
            failure: self.failure.clone(),
            health: self.health.clone(),
            control: self.control.clone(),
            counters: Arc::clone(&self.counters),
            telemetry: self.telemetry.clone(),
            quality: self.quality.clone(),
        }
    }

    pub async fn shutdown(&mut self) {
        self.cancel_immediately();
        for task in self.tasks.drain(..) {
            let _ = task.await;
        }
    }

    pub fn cancel_immediately(&mut self) {
        self.outgoing.take();
        self.cancellation.cancel();
        for task in &self.tasks {
            task.abort();
        }
    }
}

impl Drop for ManagedTunnelRuntime {
    fn drop(&mut self) {
        self.cancel_immediately();
    }
}

async fn connect_initial_with_refresh(
    profile: &Profile,
    identity: Arc<MasqueTlsIdentity>,
    protector: Arc<dyn SocketProtector>,
    pin_refresher: Option<&Arc<dyn EndpointPinRefresher>>,
    telemetry: &ConnectionTelemetry,
) -> Result<(MasqueTunnel, AddressFamily, Arc<MasqueTlsIdentity>, bool), TransportError> {
    match connect_with_policy(
        profile,
        identity.as_ref(),
        Arc::clone(&protector),
        telemetry,
    )
    .await
    {
        Ok((tunnel, family)) => Ok((tunnel, family, identity, false)),
        Err(TransportError::EndpointPinMismatch) => {
            let Some(pin_refresher) = pin_refresher else {
                return Err(TransportError::EndpointPinMismatch);
            };
            let refreshed = pin_refresher.refresh(Arc::clone(&protector)).await?;
            ensure_assignments_unchanged(identity.as_ref(), &refreshed)?;
            let refreshed = Arc::new(refreshed);
            telemetry.reset_attempt();
            match connect_with_policy(profile, refreshed.as_ref(), protector, telemetry).await {
                Ok((tunnel, family)) => Ok((tunnel, family, refreshed, true)),
                Err(error) => Err(TransportError::EndpointPinRefresh(format!(
                    "the single retry with the refreshed enrollment failed: {error}"
                ))),
            }
        }
        Err(error) => Err(error),
    }
}

fn ensure_assignments_unchanged(
    current: &MasqueTlsIdentity,
    refreshed: &MasqueTlsIdentity,
) -> Result<(), TransportError> {
    if current.assigned_ipv4 == refreshed.assigned_ipv4
        && current.assigned_ipv6 == refreshed.assigned_ipv6
    {
        Ok(())
    } else {
        Err(TransportError::EndpointAssignmentChanged)
    }
}

async fn connect_with_policy(
    profile: &Profile,
    identity: &MasqueTlsIdentity,
    protector: Arc<dyn SocketProtector>,
    telemetry: &ConnectionTelemetry,
) -> Result<(MasqueTunnel, AddressFamily), TransportError> {
    match profile.transport {
        TransportPolicy::Http3 => {
            connect_happy_eyeballs(profile, identity, Transport::Http3, protector, telemetry).await
        }
        TransportPolicy::Http2 => {
            connect_happy_eyeballs(profile, identity, Transport::Http2, protector, telemetry).await
        }
        TransportPolicy::Auto => {
            let h3_result = connect_happy_eyeballs(
                profile,
                identity,
                Transport::Http3,
                Arc::clone(&protector),
                telemetry,
            )
            .await;
            match h3_result {
                Ok(connected) => Ok(connected),
                Err(h3_error) => {
                    let h3_failure = h3_error.failure(Some(Transport::Http3), None);
                    if !h3_failure.fallback_allowed {
                        return Err(h3_error);
                    }
                    telemetry.increment_fallback();
                    telemetry.record(
                        ConnectionEventType::FallbackStarted,
                        Some(h3_failure.stage),
                        ConnectionEventPath::new(Some(Transport::Http3), h3_failure.address_family),
                        None,
                        Some(h3_failure.clone()),
                    );
                    match connect_happy_eyeballs(
                        profile,
                        identity,
                        Transport::Http2,
                        protector,
                        telemetry,
                    )
                    .await
                    {
                        Ok(connected) => Ok(connected),
                        Err(TransportError::EndpointPinMismatch) => {
                            Err(TransportError::EndpointPinMismatch)
                        }
                        Err(h2_error) => Err(TransportError::AllTransportsFailed {
                            h3: Box::new(h3_failure),
                            h2: Box::new(h2_error.failure(Some(Transport::Http2), None)),
                        }),
                    }
                }
            }
        }
    }
}

async fn connect_happy_eyeballs(
    profile: &Profile,
    identity: &MasqueTlsIdentity,
    transport: Transport,
    protector: Arc<dyn SocketProtector>,
    telemetry: &ConnectionTelemetry,
) -> Result<(MasqueTunnel, AddressFamily), TransportError> {
    let (preferred, preferred_family, alternate, alternate_family) = match profile.ip_policy {
        IpPolicy::Auto | IpPolicy::PreferIpv6 | IpPolicy::Ipv6Only => (
            profile.endpoint.ipv6_socket(),
            AddressFamily::Ipv6,
            profile.endpoint.ipv4_socket(),
            AddressFamily::Ipv4,
        ),
        IpPolicy::PreferIpv4 | IpPolicy::Ipv4Only => (
            profile.endpoint.ipv4_socket(),
            AddressFamily::Ipv4,
            profile.endpoint.ipv6_socket(),
            AddressFamily::Ipv6,
        ),
    };

    let single_family = matches!(profile.ip_policy, IpPolicy::Ipv4Only | IpPolicy::Ipv6Only);
    let preferred_available = protector.endpoint_family_available(preferred);
    let alternate_available = protector.endpoint_family_available(alternate);
    if single_family && preferred_available == Some(false) {
        return Err(TransportError::EndpointFamilyUnavailable(preferred_family));
    }
    if !single_family && preferred_available == Some(false) {
        if alternate_available == Some(false) {
            return Err(TransportError::AllEndpointsFailed(format!(
                "{} and {} are unavailable on the selected physical network",
                preferred_family_label(preferred_family),
                preferred_family_label(alternate_family),
            )));
        }
        return connect_endpoint(
            transport,
            EndpointCandidate::new(alternate, alternate_family),
            &profile.endpoint.sni,
            identity,
            protector,
            telemetry,
        )
        .await
        .map(|tunnel| (tunnel, alternate_family));
    }

    let preferred_connect = connect_endpoint(
        transport,
        EndpointCandidate::new(preferred, preferred_family),
        &profile.endpoint.sni,
        identity,
        Arc::clone(&protector),
        telemetry,
    );
    tokio::pin!(preferred_connect);

    if single_family {
        return preferred_connect
            .await
            .map(|tunnel| (tunnel, preferred_family));
    }

    if alternate_available == Some(false) {
        return preferred_connect
            .await
            .map(|tunnel| (tunnel, preferred_family));
    }

    let alternate_connect = connect_endpoint(
        transport,
        EndpointCandidate::new(alternate, alternate_family),
        &profile.endpoint.sni,
        identity,
        protector,
        telemetry,
    );
    match race_candidates(preferred_connect, alternate_connect, HAPPY_EYEBALLS_DELAY).await {
        Ok((tunnel, false)) => Ok((tunnel, preferred_family)),
        Ok((tunnel, true)) => Ok((tunnel, alternate_family)),
        Err((preferred_error, alternate_error)) => Err(combine_endpoint_errors(
            preferred,
            preferred_error,
            alternate,
            alternate_error,
        )),
    }
}

async fn race_candidates<P, A, T, E>(
    preferred: P,
    alternate: A,
    delay: Duration,
) -> Result<(T, bool), (E, E)>
where
    P: Future<Output = Result<T, E>>,
    A: Future<Output = Result<T, E>>,
{
    tokio::pin!(preferred);
    tokio::pin!(alternate);
    match timeout(delay, &mut preferred).await {
        Ok(Ok(value)) => return Ok((value, false)),
        Ok(Err(preferred_error)) => {
            return alternate
                .await
                .map(|value| (value, true))
                .map_err(|alternate_error| (preferred_error, alternate_error));
        }
        Err(_) => {}
    }

    tokio::select! {
        result = &mut preferred => match result {
            Ok(value) => Ok((value, false)),
            Err(preferred_error) => alternate
                .await
                .map(|value| (value, true))
                .map_err(|alternate_error| (preferred_error, alternate_error)),
        },
        result = &mut alternate => match result {
            Ok(value) => Ok((value, true)),
            Err(alternate_error) => preferred
                .await
                .map(|value| (value, false))
                .map_err(|preferred_error| (preferred_error, alternate_error)),
        },
    }
}

const fn preferred_family_label(family: AddressFamily) -> &'static str {
    match family {
        AddressFamily::Ipv4 => "IPv4",
        AddressFamily::Ipv6 => "IPv6",
    }
}

#[derive(Clone, Copy)]
struct EndpointCandidate {
    socket: std::net::SocketAddr,
    family: AddressFamily,
}

impl EndpointCandidate {
    const fn new(socket: std::net::SocketAddr, family: AddressFamily) -> Self {
        Self { socket, family }
    }
}

async fn connect_endpoint(
    transport: Transport,
    target: EndpointCandidate,
    sni: &str,
    identity: &MasqueTlsIdentity,
    protector: Arc<dyn SocketProtector>,
    telemetry: &ConnectionTelemetry,
) -> Result<MasqueTunnel, TransportError> {
    let EndpointCandidate {
        socket: endpoint,
        family,
    } = target;
    telemetry.record_attempt(transport, family);
    let attempt = ConnectionAttemptTelemetry::new(telemetry.clone(), transport, family);
    attempt.record(
        ConnectionEventType::EndpointResolved,
        TransportStage::EndpointResolution,
    );
    let started = Instant::now();
    for _ in 0..8 {
        let network_generation = protector.network_generation();
        let connecting = async {
            match transport {
                Transport::Http3 => connect_h3_with_protector(
                    endpoint,
                    sni,
                    identity,
                    protector.as_ref(),
                    Some(&attempt),
                )
                .await
                .map(MasqueTunnel::Http3),
                Transport::Http2 => connect_h2_with_protector(
                    endpoint,
                    sni,
                    identity,
                    protector.as_ref(),
                    Some(&attempt),
                )
                .await
                .map(MasqueTunnel::Http2),
            }
        };
        tokio::pin!(connecting);
        tokio::select! {
            result = &mut connecting => {
                match &result {
                    Ok(tunnel) => {
                        telemetry.record(
                            ConnectionEventType::AddressAssigned,
                            Some(TransportStage::AddressAssignment),
                            ConnectionEventPath::known(transport, family),
                            None,
                            None,
                        );
                        telemetry.record_tunnel_ready(transport, family, started.elapsed());
                        tunnel.activate_network_quality();
                    }
                    Err(error) => {
                        let failure = error.failure(Some(transport), Some(family));
                        telemetry.record(
                            ConnectionEventType::Failed,
                            Some(failure.stage),
                            ConnectionEventPath::known(transport, family),
                            Some(started.elapsed()),
                            Some(failure),
                        );
                    }
                }
                return result;
            },
            _ = wait_for_network_change(&protector, network_generation), if network_generation.is_some() => {
                telemetry.increment_network_change();
                telemetry.record(
                    ConnectionEventType::NetworkChanged,
                    Some(TransportStage::SocketConnect),
                    ConnectionEventPath::known(transport, family),
                    None,
                    Some(TransportFailure::new(
                        TransportFailureCode::PhysicalNetworkChanged,
                        TransportStage::SocketConnect,
                    ).on_path(transport, family)),
                );
                continue;
            }
        }
    }
    let error = TransportError::UnderlyingNetworkChanged;
    let failure = error.failure(Some(transport), Some(family));
    telemetry.record(
        ConnectionEventType::Failed,
        Some(failure.stage),
        ConnectionEventPath::known(transport, family),
        Some(started.elapsed()),
        Some(failure),
    );
    Err(error)
}

fn combine_endpoint_errors(
    preferred: std::net::SocketAddr,
    preferred_error: TransportError,
    alternate: std::net::SocketAddr,
    alternate_error: TransportError,
) -> TransportError {
    if matches!(&preferred_error, TransportError::EndpointPinMismatch)
        || matches!(&alternate_error, TransportError::EndpointPinMismatch)
    {
        return TransportError::EndpointPinMismatch;
    }
    TransportError::AllEndpointsFailed(format!(
        "{preferred}: {preferred_error}; {alternate}: {alternate_error}"
    ))
}

type ProbeFuture = Pin<
    Box<
        dyn Future<Output = Result<(MasqueTunnel, AddressFamily), TransportError>> + Send + 'static,
    >,
>;

enum ActiveOutcome {
    Switch(Box<MasqueTunnel>, AddressFamily),
    Reconnect(TransportFailure),
    PinMismatch,
    Terminal(TransportFailure),
    Shutdown,
}

enum PacketIo {
    Pipe {
        rx: WakingPipeReceiver,
        tx: WakingPipeSender,
        buffered_outgoing: Option<Bytes>,
    },
    Channel {
        outgoing: TrackedReceiver<Bytes>,
        incoming: TrackedSender<PacketBatch>,
        buffered_outgoing: Option<Bytes>,
    },
}

enum TryOutgoingPacket {
    Packet(Bytes),
    Empty,
    Closed,
}

type IncomingDeliveryFuture = Pin<Box<dyn Future<Output = bool> + Send + 'static>>;
type TimedBatchSendResult =
    Result<Result<PacketBatchResult, TransportError>, tokio::time::error::Elapsed>;
type TimedBatchSendFuture = Pin<Box<dyn Future<Output = TimedBatchSendResult> + Send + 'static>>;

fn start_timed_batch_send(future: BatchSendFuture) -> TimedBatchSendFuture {
    Box::pin(timeout(PACKET_SEND_TIMEOUT, future))
}

async fn wait_for_batch_send(pending: &mut Option<TimedBatchSendFuture>) -> TimedBatchSendResult {
    pending
        .as_mut()
        .expect("pending batch send is present while selected")
        .await
}

async fn wait_for_incoming_delivery(pending: &mut Option<IncomingDeliveryFuture>) -> bool {
    pending
        .as_mut()
        .expect("pending incoming delivery is present while selected")
        .await
}

impl PacketIo {
    fn from_pipe(pipe: WakingPipe) -> Self {
        let WakingPipe { rx, tx } = pipe;
        Self::Pipe {
            rx,
            tx,
            buffered_outgoing: None,
        }
    }

    async fn receive_outgoing(&mut self) -> Option<Bytes> {
        loop {
            if let Some(packet) = self.take_buffered_outgoing() {
                return Some(packet);
            }
            let packet = match self {
                Self::Pipe { rx, .. } => rx.recv_async().await.map(|packet| {
                    let mut packet = packet
                        .try_into_mut()
                        .unwrap_or_else(|packet| bytes::BytesMut::from(packet.as_ref()));
                    if let Err(error) = prepare_forwarded_packet(&mut packet) {
                        tracing::warn!(
                            %error,
                            "discarded malformed packet from the userspace stack"
                        );
                        return None;
                    }
                    Some(packet.freeze())
                }),
                Self::Channel { outgoing, .. } => outgoing.recv().await.map(|packet| {
                    let mut packet = packet
                        .try_into_mut()
                        .unwrap_or_else(|packet| bytes::BytesMut::from(packet.as_ref()));
                    if let Err(error) = prepare_forwarded_packet(&mut packet) {
                        tracing::warn!(%error, "discarded malformed packet from the TUN source");
                        return None;
                    }
                    Some(packet.freeze())
                }),
            };
            match packet {
                Some(Some(packet)) => return Some(packet),
                Some(None) => continue,
                None => return None,
            }
        }
    }

    async fn receive_outgoing_batch(&mut self) -> Option<PacketBatch> {
        let first = self.receive_outgoing().await?;
        let mut batch = PacketBatch::single(first);
        while let TryOutgoingPacket::Packet(packet) = self.try_receive_outgoing() {
            if let Err(packet) = batch.push_back(packet) {
                self.store_buffered_outgoing(packet);
                break;
            }
        }
        Some(batch)
    }

    fn try_receive_outgoing(&mut self) -> TryOutgoingPacket {
        loop {
            if let Some(packet) = self.take_buffered_outgoing() {
                return TryOutgoingPacket::Packet(packet);
            }
            let packet = match self {
                Self::Pipe { rx, .. } => {
                    if !rx.rx_ready() {
                        return TryOutgoingPacket::Empty;
                    }
                    match rx.try_recv() {
                        Some(packet) => packet,
                        None => return TryOutgoingPacket::Closed,
                    }
                }
                Self::Channel { outgoing, .. } => match outgoing.try_recv() {
                    Ok(packet) => packet,
                    Err(mpsc::error::TryRecvError::Empty) => return TryOutgoingPacket::Empty,
                    Err(mpsc::error::TryRecvError::Disconnected) => {
                        return TryOutgoingPacket::Closed;
                    }
                },
            };
            let mut packet = packet
                .try_into_mut()
                .unwrap_or_else(|packet| bytes::BytesMut::from(packet.as_ref()));
            if let Err(error) = prepare_forwarded_packet(&mut packet) {
                tracing::warn!(%error, "discarded malformed packet while building a MASQUE batch");
                continue;
            }
            return TryOutgoingPacket::Packet(packet.freeze());
        }
    }

    fn take_buffered_outgoing(&mut self) -> Option<Bytes> {
        match self {
            Self::Pipe {
                buffered_outgoing, ..
            }
            | Self::Channel {
                buffered_outgoing, ..
            } => buffered_outgoing.take(),
        }
    }

    fn store_buffered_outgoing(&mut self, packet: Bytes) {
        let slot = match self {
            Self::Pipe {
                buffered_outgoing, ..
            }
            | Self::Channel {
                buffered_outgoing, ..
            } => buffered_outgoing,
        };
        debug_assert!(slot.is_none());
        *slot = Some(packet);
    }

    fn start_incoming_batch(&self, mut batch: PacketBatch) -> IncomingDeliveryFuture {
        match self {
            Self::Pipe { tx, .. } => {
                let tx = tx.clone();
                Box::pin(async move {
                    while let Some(packet) = batch.pop_front() {
                        tx.send_async(&packet).await;
                    }
                    true
                })
            }
            Self::Channel { incoming, .. } => {
                let incoming = incoming.clone();
                let bytes = batch.bytes();
                Box::pin(async move { incoming.send(batch, bytes).await.is_ok() })
            }
        }
    }
}

struct SupervisorContext {
    profile: Profile,
    identity: Arc<MasqueTlsIdentity>,
    protector: Arc<dyn SocketProtector>,
    pin_refresher: Option<Arc<dyn EndpointPinRefresher>>,
    pin_refresh_attempted: bool,
    cancellation: CancellationToken,
    failure_tx: watch::Sender<Option<String>>,
    health_tx: watch::Sender<RuntimeHealth>,
    control_tx: watch::Sender<PeerNetworkState>,
    counters: Arc<TrafficCounters>,
    telemetry: ConnectionTelemetry,
}

async fn run_transport_supervisor(
    tunnel: MasqueTunnel,
    endpoint_family: AddressFamily,
    mut packet_io: PacketIo,
    context: SupervisorContext,
) {
    let SupervisorContext {
        profile,
        mut identity,
        protector,
        pin_refresher,
        mut pin_refresh_attempted,
        cancellation,
        failure_tx,
        health_tx,
        control_tx,
        counters,
        telemetry,
    } = context;
    let mut active_tunnel = tunnel;
    let mut active_family = endpoint_family;
    let mut reconnect_count = 0u32;
    let mut backoff_index = 0usize;
    let mut probe_generation = 0u32;

    loop {
        let active_transport = active_tunnel.transport();
        let active_path = runtime_path(active_transport, active_family);
        let _ = health_tx.send(RuntimeHealth::Connected {
            path: active_path,
            reconnect_count,
        });
        let stable_since = Instant::now();
        let outcome = pump_active_tunnel(
            active_tunnel,
            active_path,
            reconnect_count,
            &mut packet_io,
            &profile,
            Arc::clone(&identity),
            Arc::clone(&protector),
            &cancellation,
            Arc::clone(&counters),
            &control_tx,
            &health_tx,
            &telemetry,
            probe_generation,
            protector.network_generation(),
        )
        .await;
        probe_generation = probe_generation.wrapping_add(1);

        match outcome {
            ActiveOutcome::Shutdown => {
                telemetry.record(
                    ConnectionEventType::Disconnected,
                    None,
                    ConnectionEventPath::known(active_path.transport, active_path.endpoint_family),
                    None,
                    None,
                );
                return;
            }
            ActiveOutcome::Terminal(failure) => {
                let message = failure.code.to_string();
                let _ = health_tx.send(RuntimeHealth::Failed {
                    last_path: active_path,
                    reconnect_count,
                    message: message.clone(),
                    failure: failure.clone(),
                });
                telemetry.record(
                    ConnectionEventType::Failed,
                    Some(failure.stage),
                    ConnectionEventPath::new(failure.transport, failure.address_family),
                    None,
                    Some(failure),
                );
                report_failure(&cancellation, &failure_tx, message);
                return;
            }
            ActiveOutcome::Switch(tunnel, family) => {
                let transport = tunnel.transport();
                telemetry.record(
                    ConnectionEventType::PathPromoted,
                    Some(TransportStage::TunnelStartup),
                    ConnectionEventPath::known(transport, family),
                    None,
                    None,
                );
                active_tunnel = *tunnel;
                active_family = family;
                continue;
            }
            ActiveOutcome::PinMismatch => {
                control_tx.send_replace(PeerNetworkState::default());
                reconnect_count = reconnect_count.saturating_add(1);
                let failure = TransportError::EndpointPinMismatch.failure(
                    Some(active_path.transport),
                    Some(active_path.endpoint_family),
                );
                let reason = failure.code.to_string();
                telemetry.set_reconnect(reconnect_count, &failure);
                let _ = health_tx.send(RuntimeHealth::Reconnecting {
                    last_path: active_path,
                    attempt: 1,
                    reconnect_count,
                    reason,
                    failure: failure.clone(),
                });
                if pin_refresh_attempted {
                    let message = failure.code.to_string();
                    let _ = health_tx.send(RuntimeHealth::Failed {
                        last_path: active_path,
                        reconnect_count,
                        message: message.clone(),
                        failure: failure.clone(),
                    });
                    telemetry.record(
                        ConnectionEventType::Failed,
                        Some(failure.stage),
                        ConnectionEventPath::new(failure.transport, failure.address_family),
                        None,
                        Some(failure),
                    );
                    report_failure(&cancellation, &failure_tx, message);
                    return;
                }
                pin_refresh_attempted = true;
                match refresh_and_retry_connection(
                    &profile,
                    identity.as_ref(),
                    pin_refresher.as_ref(),
                    Arc::clone(&protector),
                    RefreshRetryContext {
                        packet_io: &mut packet_io,
                        cancellation: &cancellation,
                        telemetry: &telemetry,
                    },
                )
                .await
                {
                    Some(Ok((tunnel, family, refreshed))) => {
                        identity = refreshed;
                        active_tunnel = tunnel;
                        active_family = family;
                        continue;
                    }
                    Some(Err(error)) => {
                        tracing::warn!(%error, "endpoint-pin refresh reconnect failed");
                        let failure = error.failure(
                            Some(active_path.transport),
                            Some(active_path.endpoint_family),
                        );
                        let message = failure.code.to_string();
                        let _ = health_tx.send(RuntimeHealth::Failed {
                            last_path: active_path,
                            reconnect_count,
                            message: message.clone(),
                            failure: failure.clone(),
                        });
                        telemetry.record(
                            ConnectionEventType::Failed,
                            Some(failure.stage),
                            ConnectionEventPath::new(failure.transport, failure.address_family),
                            None,
                            Some(failure),
                        );
                        report_failure(&cancellation, &failure_tx, message);
                        return;
                    }
                    None => return,
                }
            }
            ActiveOutcome::Reconnect(mut failure) => {
                control_tx.send_replace(PeerNetworkState::default());
                if stable_since.elapsed() >= STABLE_CONNECTION_RESET {
                    backoff_index = 0;
                }
                loop {
                    reconnect_count = reconnect_count.saturating_add(1);
                    let attempt = backoff_index as u32 + 1;
                    let delay = jitter_duration(
                        RECONNECT_DELAYS[backoff_index],
                        H3_PROBE_JITTER_PERCENT,
                        reconnect_count,
                    );
                    backoff_index = (backoff_index + 1).min(RECONNECT_DELAYS.len() - 1);
                    let reason = failure.code.to_string();
                    telemetry.set_reconnect(reconnect_count, &failure);
                    telemetry.record(
                        ConnectionEventType::ReconnectScheduled,
                        Some(failure.stage),
                        ConnectionEventPath::new(failure.transport, failure.address_family),
                        Some(delay),
                        Some(failure.clone()),
                    );
                    let _ = health_tx.send(RuntimeHealth::Reconnecting {
                        last_path: active_path,
                        attempt,
                        reconnect_count,
                        reason,
                        failure: failure.clone(),
                    });
                    if !wait_while_dropping_packets(
                        delay,
                        &mut packet_io,
                        &cancellation,
                        Arc::clone(&protector),
                    )
                    .await
                    {
                        return;
                    }

                    match connect_while_dropping_packets(
                        &profile,
                        Arc::clone(&identity),
                        Arc::clone(&protector),
                        &mut packet_io,
                        &cancellation,
                        &telemetry,
                    )
                    .await
                    {
                        Some(Ok((tunnel, family))) => {
                            active_tunnel = tunnel;
                            active_family = family;
                            break;
                        }
                        Some(Err(TransportError::EndpointPinMismatch)) => {
                            failure = TransportError::EndpointPinMismatch.failure(
                                Some(active_path.transport),
                                Some(active_path.endpoint_family),
                            );
                            if pin_refresh_attempted {
                                let message = failure.code.to_string();
                                let _ = health_tx.send(RuntimeHealth::Failed {
                                    last_path: active_path,
                                    reconnect_count,
                                    message: message.clone(),
                                    failure: failure.clone(),
                                });
                                report_failure(&cancellation, &failure_tx, message);
                                return;
                            }
                            pin_refresh_attempted = true;
                            match refresh_and_retry_connection(
                                &profile,
                                identity.as_ref(),
                                pin_refresher.as_ref(),
                                Arc::clone(&protector),
                                RefreshRetryContext {
                                    packet_io: &mut packet_io,
                                    cancellation: &cancellation,
                                    telemetry: &telemetry,
                                },
                            )
                            .await
                            {
                                Some(Ok((tunnel, family, refreshed))) => {
                                    identity = refreshed;
                                    active_tunnel = tunnel;
                                    active_family = family;
                                    break;
                                }
                                Some(Err(error)) => {
                                    tracing::warn!(%error, "endpoint-pin refresh retry failed");
                                    let failure = error.failure(
                                        Some(active_path.transport),
                                        Some(active_path.endpoint_family),
                                    );
                                    let message = failure.code.to_string();
                                    let _ = health_tx.send(RuntimeHealth::Failed {
                                        last_path: active_path,
                                        reconnect_count,
                                        message: message.clone(),
                                        failure,
                                    });
                                    report_failure(&cancellation, &failure_tx, message);
                                    return;
                                }
                                None => return,
                            }
                        }
                        Some(Err(error)) => {
                            tracing::debug!(%error, "bounded reconnect attempt failed");
                            failure = error.failure(None, None);
                        }
                        None => return,
                    }
                }
            }
        }
    }
}

struct RefreshRetryContext<'a> {
    packet_io: &'a mut PacketIo,
    cancellation: &'a CancellationToken,
    telemetry: &'a ConnectionTelemetry,
}

async fn refresh_and_retry_connection(
    profile: &Profile,
    current: &MasqueTlsIdentity,
    pin_refresher: Option<&Arc<dyn EndpointPinRefresher>>,
    protector: Arc<dyn SocketProtector>,
    context: RefreshRetryContext<'_>,
) -> Option<Result<(MasqueTunnel, AddressFamily, Arc<MasqueTlsIdentity>), TransportError>> {
    let pin_refresher = match pin_refresher {
        Some(pin_refresher) => pin_refresher,
        None => return Some(Err(TransportError::EndpointPinMismatch)),
    };
    let refreshed = match pin_refresher.refresh(Arc::clone(&protector)).await {
        Ok(refreshed) => refreshed,
        Err(error) => return Some(Err(error)),
    };
    if let Err(error) = ensure_assignments_unchanged(current, &refreshed) {
        return Some(Err(error));
    }
    let refreshed = Arc::new(refreshed);
    match connect_while_dropping_packets(
        profile,
        Arc::clone(&refreshed),
        protector,
        context.packet_io,
        context.cancellation,
        context.telemetry,
    )
    .await
    {
        Some(Ok((tunnel, family))) => Some(Ok((tunnel, family, refreshed))),
        Some(Err(error)) => Some(Err(TransportError::EndpointPinRefresh(format!(
            "the single retry with the refreshed enrollment failed: {error}"
        )))),
        None => None,
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "tunnel pump must thread path, identity, I/O, cancellation, counters, and control channels as one runtime unit"
)]
async fn pump_active_tunnel(
    tunnel: MasqueTunnel,
    active_path: RuntimePath,
    reconnect_count: u32,
    packet_io: &mut PacketIo,
    profile: &Profile,
    identity: Arc<MasqueTlsIdentity>,
    protector: Arc<dyn SocketProtector>,
    cancellation: &CancellationToken,
    counters: Arc<TrafficCounters>,
    control_tx: &watch::Sender<PeerNetworkState>,
    health_tx: &watch::Sender<RuntimeHealth>,
    telemetry: &ConnectionTelemetry,
    probe_generation: u32,
    network_generation: Option<u64>,
) -> ActiveOutcome {
    let active_transport = tunnel.transport();
    let (mut send, mut receive, driver, mut control) = tunnel.into_parts();
    let mut peer_state = match control.as_ref() {
        Some(control) => {
            let state = control.borrow().clone();
            control_tx.send_replace(state.clone());
            state
        }
        None => {
            let state = PeerNetworkState::default();
            control_tx.send_replace(state.clone());
            state
        }
    };
    let base_path = active_path;
    let mut current_path = apply_peer_network_state(
        base_path,
        &peer_state,
        identity.assigned_ipv4,
        identity.assigned_ipv6,
    );
    if !current_path.ipv4_available && !current_path.ipv6_available {
        return ActiveOutcome::Terminal(
            TransportFailure::new(
                TransportFailureCode::AddressAssignmentInvalid,
                TransportStage::PeerSettings,
            )
            .on_path(active_transport, active_path.endpoint_family),
        );
    }
    let _ = health_tx.send(RuntimeHealth::Connected {
        path: current_path,
        reconnect_count,
    });
    let driver_wait = driver.wait();
    tokio::pin!(driver_wait);
    let mut probe =
        if profile.transport == TransportPolicy::Auto && active_transport == Transport::Http2 {
            Some(schedule_h3_probe(
                profile.clone(),
                Arc::clone(&identity),
                Arc::clone(&protector),
                probe_generation,
                telemetry.clone(),
            ))
        } else {
            None
        };
    let mut pending_send: Option<TimedBatchSendFuture> = None;
    let mut pending_incoming: Option<IncomingDeliveryFuture> = None;
    let mut queued_incoming = VecDeque::new();

    let outcome = loop {
        if cancellation.is_cancelled() {
            break ActiveOutcome::Shutdown;
        }
        if pending_incoming.is_none()
            && let Some(batch) = queued_incoming.pop_front()
        {
            pending_incoming = Some(packet_io.start_incoming_batch(batch));
        }
        tokio::select! {
            _ = cancellation.cancelled() => break ActiveOutcome::Shutdown,
            _ = wait_for_network_change(&protector, network_generation), if network_generation.is_some() => {
                telemetry.increment_network_change();
                let failure = TransportFailure::new(
                    TransportFailureCode::PhysicalNetworkChanged,
                    TransportStage::SocketConnect,
                ).on_path(active_transport, active_path.endpoint_family);
                telemetry.record(
                    ConnectionEventType::NetworkChanged,
                    Some(failure.stage),
                    ConnectionEventPath::new(failure.transport, failure.address_family),
                    None,
                    Some(failure.clone()),
                );
                break ActiveOutcome::Reconnect(failure);
            }
            result = wait_for_batch_send(&mut pending_send), if pending_send.is_some() => {
                pending_send.take();
                match result {
                    Ok(Ok(PacketBatchResult { accepted_bytes, oversized })) => {
                        if accepted_bytes != 0 {
                            counters.sent.fetch_add(accepted_bytes as u64, Ordering::Relaxed);
                            telemetry.record_first_packet_sent(
                                active_transport,
                                active_path.endpoint_family,
                            );
                        }
                        let mut icmp_batch = PacketBatch::new();
                        let mut oversized_outcome = None;
                        for (packet, maximum_packet_size) in oversized {
                            match crate::icmp::packet_too_big(&packet, maximum_packet_size) {
                                Ok(icmp) => {
                                    icmp_batch
                                        .push_back(icmp)
                                        .expect("one ICMP response per bounded packet batch fits");
                                }
                                Err(TransportError::Ipv6MinimumMtuUnavailable(maximum)) => {
                                    let error =
                                        TransportError::Ipv6MinimumMtuUnavailable(maximum);
                                    oversized_outcome = Some(ActiveOutcome::Terminal(
                                        error.failure(
                                            Some(active_transport),
                                            Some(active_path.endpoint_family),
                                        ),
                                    ));
                                    break;
                                }
                                Err(error) => {
                                    tracing::warn!(
                                        %error,
                                        "failed to generate ICMP Packet Too Big"
                                    );
                                }
                            }
                        }
                        if !icmp_batch.is_empty() {
                            queued_incoming.push_back(icmp_batch);
                        }
                        if let Some(outcome) = oversized_outcome {
                            break outcome;
                        }
                    }
                    Ok(Err(error)) => {
                        tracing::debug!(%error, "active MASQUE packet send failed");
                        break ActiveOutcome::Reconnect(error.failure(
                            Some(active_transport),
                            Some(active_path.endpoint_family),
                        ));
                    }
                    Err(_) => {
                        break ActiveOutcome::Reconnect(
                            TransportFailure::new(
                                TransportFailureCode::PacketSendTimeout,
                                TransportStage::PacketSend,
                            )
                            .on_path(active_transport, active_path.endpoint_family),
                        );
                    }
                }
            }
            delivered = wait_for_incoming_delivery(&mut pending_incoming), if pending_incoming.is_some() => {
                pending_incoming.take();
                if !delivered {
                    break ActiveOutcome::Shutdown;
                }
            }
            batch = packet_io.receive_outgoing_batch(), if pending_send.is_none() && queued_incoming.is_empty() => {
                let Some(mut batch) = batch else {
                    break ActiveOutcome::Shutdown;
                };
                let mut allowed = PacketBatch::new();
                while let Some(packet) = batch.pop_front() {
                    if !packet_allowed_by_peer_state(&packet, current_path) {
                        tracing::warn!(
                            family = packet.first().map(|byte| byte >> 4),
                            "discarded a packet for an address family withdrawn by the CONNECT-IP peer"
                        );
                        continue;
                    }
                    allowed
                        .push_back(packet)
                        .expect("filtering cannot grow a bounded packet batch");
                }
                if allowed.is_empty() {
                    continue;
                }
                pending_send = Some(start_timed_batch_send(send.start_owned_batch(allowed)));
            }
            result = receive.receive_batch(), if pending_incoming.is_none() => {
                match result {
                    Ok(mut batch) => {
                        let mut allowed = PacketBatch::new();
                        while let Some(packet) = batch.pop_front() {
                            if !packet_allowed_by_peer_state(&packet, current_path) {
                                tracing::warn!(
                                    family = packet.first().map(|byte| byte >> 4),
                                    "discarded a peer packet for an unavailable address family"
                                );
                                continue;
                            }
                            allowed
                                .push_back(packet)
                                .expect("filtering cannot grow a bounded packet batch");
                        }
                        if allowed.is_empty() {
                            continue;
                        }
                        counters.received.fetch_add(allowed.bytes() as u64, Ordering::Relaxed);
                        telemetry.record_first_packet_received(
                            active_transport,
                            active_path.endpoint_family,
                        );
                        pending_incoming = Some(packet_io.start_incoming_batch(allowed));
                    }
                    Err(error) => {
                        tracing::debug!(%error, "active MASQUE packet receive failed");
                        break ActiveOutcome::Reconnect(error.failure(
                            Some(active_transport),
                            Some(active_path.endpoint_family),
                        ));
                    }
                }
            }
            result = &mut driver_wait => {
                let failure = match result {
                    Ok(()) => TransportFailure::new(
                        match active_transport {
                            Transport::Http3 => TransportFailureCode::H3ConnectionClosed,
                            Transport::Http2 => TransportFailureCode::H2StreamClosed,
                        },
                        TransportStage::PacketReceive,
                    ).on_path(active_transport, active_path.endpoint_family),
                    Err(error) => {
                        tracing::debug!(%error, "MASQUE transport driver stopped");
                        error.failure(
                            Some(active_transport),
                            Some(active_path.endpoint_family),
                        )
                    }
                };
                break ActiveOutcome::Reconnect(failure);
            }
            changed = wait_for_control(&mut control), if control.is_some() => {
                match changed {
                    Ok(state) => {
                        peer_state = state;
                        control_tx.send_replace(peer_state.clone());
                        telemetry.record(
                            ConnectionEventType::PeerSettingsReceived,
                            Some(TransportStage::PeerSettings),
                            ConnectionEventPath::known(
                                active_transport,
                                active_path.endpoint_family,
                            ),
                            None,
                            None,
                        );
                        current_path = apply_peer_network_state(
                            base_path,
                            &peer_state,
                            identity.assigned_ipv4,
                            identity.assigned_ipv6,
                        );
                        if !current_path.ipv4_available && !current_path.ipv6_available {
                            break ActiveOutcome::Terminal(
                                TransportFailure::new(
                                    TransportFailureCode::AddressAssignmentInvalid,
                                    TransportStage::PeerSettings,
                                )
                                .on_path(active_transport, active_path.endpoint_family),
                            );
                        }
                        let _ = health_tx.send(RuntimeHealth::Connected {
                            path: current_path,
                            reconnect_count,
                        });
                    }
                    Err(()) => {
                        break ActiveOutcome::Reconnect(
                            TransportFailure::new(
                                TransportFailureCode::ConnectIpRejected,
                                TransportStage::PeerSettings,
                            )
                            .on_path(active_transport, active_path.endpoint_family),
                        );
                    }
                }
            }
            result = wait_for_probe(&mut probe), if probe.is_some() => {
                match result {
                    Ok((tunnel, family)) => {
                        tracing::info!(
                            from = ?active_transport,
                            to = ?Transport::Http3,
                            endpoint_family = ?family,
                            "switching the single active MASQUE channel after a successful H3 probe"
                        );
                        telemetry.record(
                            ConnectionEventType::RecoveryProbeSucceeded,
                            Some(TransportStage::TunnelStartup),
                            ConnectionEventPath::known(Transport::Http3, family),
                            None,
                            None,
                        );
                        break ActiveOutcome::Switch(Box::new(tunnel), family);
                    }
                    Err(TransportError::EndpointPinMismatch) => {
                        let failure = TransportError::EndpointPinMismatch.failure(
                            Some(Transport::Http3),
                            Some(active_path.endpoint_family),
                        );
                        telemetry.record(
                            ConnectionEventType::RecoveryProbeFailed,
                            Some(failure.stage),
                            ConnectionEventPath::new(
                                failure.transport,
                                failure.address_family,
                            ),
                            None,
                            Some(failure),
                        );
                        break ActiveOutcome::PinMismatch;
                    }
                    Err(error) => {
                        tracing::debug!(%error, "non-bearing H3 recovery probe failed");
                        let failure = error.failure(Some(Transport::Http3), None);
                        telemetry.record(
                            ConnectionEventType::RecoveryProbeFailed,
                            Some(failure.stage),
                            ConnectionEventPath::new(
                                failure.transport,
                                failure.address_family,
                            ),
                            None,
                            Some(failure),
                        );
                        probe = Some(schedule_h3_probe(
                            profile.clone(),
                            Arc::clone(&identity),
                            Arc::clone(&protector),
                            probe_generation.wrapping_add(1),
                            telemetry.clone(),
                        ));
                    }
                }
            }
        }
    };
    send.close();
    outcome
}

async fn wait_for_control(
    control: &mut Option<watch::Receiver<PeerNetworkState>>,
) -> Result<PeerNetworkState, ()> {
    match control {
        Some(control) => {
            control.changed().await.map_err(|_| ())?;
            let state = control.borrow_and_update().clone();
            Ok(state)
        }
        None => std::future::pending().await,
    }
}

fn schedule_h3_probe(
    profile: Profile,
    identity: Arc<MasqueTlsIdentity>,
    protector: Arc<dyn SocketProtector>,
    generation: u32,
    telemetry: ConnectionTelemetry,
) -> ProbeFuture {
    Box::pin(async move {
        sleep(jitter_duration(
            H3_PROBE_INTERVAL,
            H3_PROBE_JITTER_PERCENT,
            generation,
        ))
        .await;
        telemetry.record(
            ConnectionEventType::RecoveryProbeStarted,
            Some(TransportStage::QuicHandshake),
            ConnectionEventPath::new(Some(Transport::Http3), None),
            None,
            None,
        );
        connect_happy_eyeballs(
            &profile,
            identity.as_ref(),
            Transport::Http3,
            protector,
            &telemetry,
        )
        .await
    })
}

async fn wait_for_probe(
    probe: &mut Option<ProbeFuture>,
) -> Result<(MasqueTunnel, AddressFamily), TransportError> {
    match probe {
        Some(probe) => probe.as_mut().await,
        None => std::future::pending().await,
    }
}

async fn wait_while_dropping_packets(
    delay: Duration,
    packet_io: &mut PacketIo,
    cancellation: &CancellationToken,
    protector: Arc<dyn SocketProtector>,
) -> bool {
    let wait = sleep(delay);
    tokio::pin!(wait);
    let network_generation = protector.network_generation();
    loop {
        tokio::select! {
            _ = cancellation.cancelled() => return false,
            _ = &mut wait => return true,
            _ = wait_for_network_change(&protector, network_generation), if network_generation.is_some() => {
                return true;
            }
            batch = packet_io.receive_outgoing_batch() => {
                if batch.is_none() {
                    return false;
                }
            }
        }
    }
}

async fn connect_while_dropping_packets(
    profile: &Profile,
    identity: Arc<MasqueTlsIdentity>,
    protector: Arc<dyn SocketProtector>,
    packet_io: &mut PacketIo,
    cancellation: &CancellationToken,
    telemetry: &ConnectionTelemetry,
) -> Option<Result<(MasqueTunnel, AddressFamily), TransportError>> {
    let network_generation = protector.network_generation();
    telemetry.reset_attempt();
    let connect = connect_with_policy(
        profile,
        identity.as_ref(),
        Arc::clone(&protector),
        telemetry,
    );
    tokio::pin!(connect);
    loop {
        tokio::select! {
            _ = cancellation.cancelled() => return None,
            _ = wait_for_network_change(&protector, network_generation), if network_generation.is_some() => {
                return Some(Err(TransportError::UnderlyingNetworkChanged));
            }
            result = &mut connect => return Some(result),
            batch = packet_io.receive_outgoing_batch() => {
                batch?;
            }
        }
    }
}

async fn wait_for_network_change(protector: &Arc<dyn SocketProtector>, baseline: Option<u64>) {
    let Some(baseline) = baseline else {
        std::future::pending::<()>().await;
        return;
    };
    loop {
        sleep(Duration::from_millis(100)).await;
        if protector.network_generation() != Some(baseline) {
            return;
        }
    }
}

/// Applies peer control capsules as an additional fail-closed policy over the
/// locally configured full tunnel. We deliberately do not mutate a live TUN
/// address or replace its default route with a peer-provided split route:
/// Windows and Android cannot do that atomically without creating a leak
/// window. A withdrawn family is instead blocked in the shared data plane and
/// reflected as degraded health; withdrawing both families terminates the
/// tunnel so the platform can rebuild it safely.
pub(crate) fn apply_peer_network_state(
    mut path: RuntimePath,
    state: &PeerNetworkState,
    assigned_ipv4: std::net::Ipv4Addr,
    assigned_ipv6: std::net::Ipv6Addr,
) -> RuntimePath {
    if path.ipv4_available {
        path.ipv4_available =
            assignment_allows(state, IpAddr::V4(assigned_ipv4)) && routes_cover_family(state, true);
    }
    if path.ipv6_available {
        path.ipv6_available = assignment_allows(state, IpAddr::V6(assigned_ipv6))
            && routes_cover_family(state, false);
    }
    path
}

fn assignment_allows(state: &PeerNetworkState, address: IpAddr) -> bool {
    !state.assignments_advertised
        || state
            .assigned_addresses
            .iter()
            .any(|prefix| prefix_contains(prefix, address))
}

fn prefix_contains(prefix: &IpPrefix, address: IpAddr) -> bool {
    match (prefix.address, address) {
        (IpAddr::V4(network), IpAddr::V4(address)) => {
            let length = u32::from(prefix.prefix_len);
            if length > 32 {
                return false;
            }
            let mask = if length == 0 {
                0
            } else {
                u32::MAX << (32 - length)
            };
            u32::from(network) & mask == u32::from(address) & mask
        }
        (IpAddr::V6(network), IpAddr::V6(address)) => {
            let length = u32::from(prefix.prefix_len);
            if length > 128 {
                return false;
            }
            let mask = if length == 0 {
                0
            } else {
                u128::MAX << (128 - length)
            };
            u128::from(network) & mask == u128::from(address) & mask
        }
        _ => false,
    }
}

fn routes_cover_family(state: &PeerNetworkState, ipv4: bool) -> bool {
    if !state.routes_advertised {
        return true;
    }

    let maximum = if ipv4 {
        u128::from(u32::MAX)
    } else {
        u128::MAX
    };
    let mut next = 0u128;
    for range in state.available_routes.iter().filter(|range| {
        range.protocol == 0
            && matches!(
                (ipv4, range.start, range.end),
                (true, IpAddr::V4(_), IpAddr::V4(_)) | (false, IpAddr::V6(_), IpAddr::V6(_))
            )
    }) {
        let Some((start, end)) = numeric_range(range) else {
            continue;
        };
        if start > next {
            return false;
        }
        if end == maximum {
            return true;
        }
        next = end.saturating_add(1);
    }
    false
}

fn numeric_range(range: &IpAddressRange) -> Option<(u128, u128)> {
    match (range.start, range.end) {
        (IpAddr::V4(start), IpAddr::V4(end)) => {
            Some((u128::from(u32::from(start)), u128::from(u32::from(end))))
        }
        (IpAddr::V6(start), IpAddr::V6(end)) => Some((u128::from(start), u128::from(end))),
        _ => None,
    }
}

fn packet_allowed_by_peer_state(packet: &[u8], path: RuntimePath) -> bool {
    match packet.first().map(|byte| byte >> 4) {
        Some(4) => path.ipv4_available,
        Some(6) => path.ipv6_available,
        _ => false,
    }
}

fn runtime_path(transport: Transport, endpoint_family: AddressFamily) -> RuntimePath {
    RuntimePath {
        transport,
        endpoint_family,
        // The outer endpoint family is only the CONNECT-IP carrier. WARP's
        // out-of-band identity assigns both payload families over that single
        // active channel. Peer capsules may narrow these flags later.
        ipv4_available: true,
        ipv6_available: true,
    }
}

/// Adds bounded, non-cryptographic scheduling jitter for reconnects and probes.
fn jitter_duration(base: Duration, percent: u64, sequence: u32) -> Duration {
    let base_millis = base.as_millis().min(u128::from(u64::MAX)) as u64;
    let span = base_millis.saturating_mul(percent).saturating_div(100);
    if span == 0 {
        return base;
    }
    let entropy = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| u64::from(duration.subsec_nanos()))
        .unwrap_or_default()
        ^ u64::from(sequence).wrapping_mul(0x9e37_79b9);
    let offset = entropy % span.saturating_mul(2).saturating_add(1);
    Duration::from_millis(base_millis.saturating_sub(span).saturating_add(offset))
}

fn report_failure(
    cancellation: &CancellationToken,
    sender: &watch::Sender<Option<String>>,
    message: String,
) {
    if !cancellation.is_cancelled() && sender.borrow().is_none() {
        let _ = sender.send(Some(message));
    }
}

fn prepare_forwarded_packet(packet: &mut [u8]) -> Result<(), TransportError> {
    crate::h2::validate_ip_packet(packet)?;
    match packet.first().map(|byte| byte >> 4) {
        Some(4) => prepare_ipv4(packet),
        Some(6) => {
            if packet.len() < 40 || packet[7] <= 1 {
                return Err(TransportError::MalformedIpPacket);
            }
            packet[7] -= 1;
            Ok(())
        }
        _ => Err(TransportError::MalformedIpPacket),
    }
}

fn prepare_ipv4(packet: &mut [u8]) -> Result<(), TransportError> {
    if packet.len() < 20 {
        return Err(TransportError::MalformedIpPacket);
    }
    let header_length = usize::from(packet[0] & 0x0f) * 4;
    if header_length < 20 || packet.len() < header_length || packet[8] <= 1 {
        return Err(TransportError::MalformedIpPacket);
    }
    let total_length = usize::from(u16::from_be_bytes([packet[2], packet[3]]));
    if total_length < header_length || total_length > packet.len() {
        return Err(TransportError::MalformedIpPacket);
    }
    packet[8] -= 1;
    packet[10] = 0;
    packet[11] = 0;
    let checksum = ipv4_header_checksum(&packet[..header_length]);
    packet[10..12].copy_from_slice(&checksum.to_be_bytes());
    Ok(())
}

fn ipv4_header_checksum(header: &[u8]) -> u16 {
    let mut sum = 0u32;
    for word in header.chunks_exact(2) {
        sum += u32::from(u16::from_be_bytes([word[0], word[1]]));
    }
    if let Some(last) = header.chunks_exact(2).remainder().first() {
        sum += u32::from(*last) << 8;
    }
    while sum > 0xffff {
        sum = (sum & 0xffff) + (sum >> 16);
    }
    !(sum as u16)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Ipv4Addr, Ipv6Addr};
    use tokio::sync::oneshot;
    use ts_netstack_smoltcp::CreateSocket;

    fn test_ipv4_packet(sequence: u16) -> Bytes {
        let mut packet = vec![0u8; 20];
        packet[0] = 0x45;
        packet[2..4].copy_from_slice(&20_u16.to_be_bytes());
        packet[4..6].copy_from_slice(&sequence.to_be_bytes());
        packet[8] = 64;
        packet[9] = 17;
        packet[12..16].copy_from_slice(&[192, 0, 2, 1]);
        packet[16..20].copy_from_slice(&[198, 51, 100, 1]);
        let checksum = ipv4_header_checksum(&packet);
        packet[10..12].copy_from_slice(&checksum.to_be_bytes());
        Bytes::from(packet)
    }

    fn test_packet_channel(
        kind: QueueKind,
        capacity: usize,
    ) -> (TrackedSender<Bytes>, TrackedReceiver<Bytes>) {
        tracked_channel(crate::queue_metrics::QueueMetrics::new(
            kind,
            capacity,
            capacity * u16::MAX as usize,
        ))
    }

    fn test_batch_channel(
        kind: QueueKind,
        capacity: usize,
    ) -> (TrackedSender<PacketBatch>, TrackedReceiver<PacketBatch>) {
        tracked_channel(crate::queue_metrics::QueueMetrics::new(
            kind,
            capacity,
            capacity * MAX_PACKET_BATCH_BYTES,
        ))
    }

    fn test_managed_sender(
        capacity: usize,
    ) -> (
        ManagedTunnelSender,
        TrackedReceiver<Bytes>,
        ConnectionTelemetry,
    ) {
        let telemetry = ConnectionTelemetry::default();
        let metrics = telemetry.network_quality().register_queue(
            QueueKind::TransportOutgoingPackets,
            capacity,
            capacity * u16::MAX as usize,
        );
        let (outgoing, receiver) = tracked_channel(metrics);
        (
            ManagedTunnelSender {
                outgoing,
                telemetry: telemetry.clone(),
            },
            receiver,
            telemetry,
        )
    }

    #[test]
    fn forwarding_decrements_ipv4_ttl_and_repairs_checksum() {
        let mut packet = [
            0x45, 0, 0, 20, 0, 0, 0, 0, 64, 17, 0, 0, 1, 1, 1, 1, 8, 8, 8, 8,
        ];
        prepare_forwarded_packet(&mut packet).unwrap();
        assert_eq!(packet[8], 63);
        assert_eq!(ipv4_header_checksum(&packet), 0);
    }

    #[test]
    fn forwarding_decrements_ipv6_hop_limit() {
        let mut packet = [0u8; 40];
        packet[0] = 0x60;
        packet[7] = 64;
        prepare_forwarded_packet(&mut packet).unwrap();
        assert_eq!(packet[7], 63);
    }

    #[tokio::test]
    async fn managed_sender_backpressures_instead_of_dropping_or_closing() {
        let (sender, mut receiver, telemetry) = test_managed_sender(1);
        let first = test_ipv4_packet(1);
        let second = test_ipv4_packet(2);
        sender.send_owned_packet(first.clone()).await.unwrap();

        let blocked_sender = sender.clone();
        let blocked_packet = second.clone();
        let blocked =
            tokio::spawn(async move { blocked_sender.send_owned_packet(blocked_packet).await });
        tokio::task::yield_now().await;
        assert!(!blocked.is_finished());
        assert_eq!(receiver.recv().await.unwrap(), first);
        timeout(Duration::from_secs(1), blocked)
            .await
            .expect("sender did not resume after capacity returned")
            .unwrap()
            .unwrap();
        assert_eq!(receiver.recv().await.unwrap(), second);

        let metrics = telemetry.snapshot().metrics;
        assert_eq!(metrics.send_queue_drop_count, 0);
        assert_eq!(metrics.send_queue_high_watermark, 1);
    }

    #[tokio::test]
    async fn closing_managed_receiver_releases_a_waiting_sender() {
        let (sender, receiver, _telemetry) = test_managed_sender(1);
        sender.send_owned_packet(test_ipv4_packet(1)).await.unwrap();
        let waiting_sender = sender.clone();
        let waiting =
            tokio::spawn(
                async move { waiting_sender.send_owned_packet(test_ipv4_packet(2)).await },
            );
        tokio::task::yield_now().await;
        assert!(!waiting.is_finished());
        drop(receiver);
        let result = timeout(Duration::from_secs(1), waiting)
            .await
            .expect("sender did not wake when the receiver closed")
            .unwrap();
        assert!(matches!(result, Err(TransportError::TunnelClosed)));
    }

    #[tokio::test]
    async fn packet_io_nonblocking_drain_batches_without_reordering() {
        let (outgoing_tx, outgoing) = test_packet_channel(QueueKind::TransportOutgoingPackets, 130);
        let (incoming, _incoming_rx) = test_batch_channel(QueueKind::TransportToTun, 1);
        for sequence in 0..130_u16 {
            let packet = test_ipv4_packet(sequence);
            let bytes = packet.len();
            outgoing_tx.try_send(packet, bytes).unwrap();
        }
        drop(outgoing_tx);
        let mut packet_io = PacketIo::Channel {
            outgoing,
            incoming,
            buffered_outgoing: None,
        };

        let mut observed = Vec::new();
        let mut batch_sizes = Vec::new();
        while let Some(batch) = packet_io.receive_outgoing_batch().await {
            batch_sizes.push(batch.len());
            for packet in batch.iter() {
                observed.push(u16::from_be_bytes([packet[4], packet[5]]));
                assert_eq!(packet[8], 63);
                assert_eq!(ipv4_header_checksum(packet), 0);
            }
        }
        assert_eq!(batch_sizes, [64, 64, 2]);
        assert_eq!(observed, (0..130_u16).collect::<Vec<_>>());
    }

    #[tokio::test]
    async fn pending_batch_send_does_not_mask_other_events_or_cancellation() {
        let stalled: BatchSendFuture = Box::pin(std::future::pending());
        let mut pending_send = Some(start_timed_batch_send(stalled));
        let (event_tx, event_rx) = oneshot::channel();
        event_tx.send("control").unwrap();
        tokio::select! {
            result = wait_for_batch_send(&mut pending_send) => {
                panic!("stalled batch send completed unexpectedly: {result:?}");
            }
            event = event_rx => assert_eq!(event.unwrap(), "control"),
        }
        assert!(pending_send.is_some());

        let cancellation = CancellationToken::new();
        cancellation.cancel();
        tokio::select! {
            biased;
            _ = cancellation.cancelled() => {}
            result = wait_for_batch_send(&mut pending_send) => {
                panic!("stalled batch send masked cancellation: {result:?}");
            }
        }
    }

    #[tokio::test]
    async fn pending_incoming_delivery_does_not_mask_cancellation() {
        let (_outgoing_tx, outgoing) = test_packet_channel(QueueKind::TransportOutgoingPackets, 1);
        let (incoming, mut incoming_rx) = test_batch_channel(QueueKind::TransportToTun, 1);
        let first_batch = PacketBatch::single(test_ipv4_packet(1));
        let first_bytes = first_batch.bytes();
        incoming.send(first_batch, first_bytes).await.unwrap();
        let packet_io = PacketIo::Channel {
            outgoing,
            incoming,
            buffered_outgoing: None,
        };
        let mut pending_incoming =
            Some(packet_io.start_incoming_batch(PacketBatch::single(test_ipv4_packet(2))));
        tokio::task::yield_now().await;
        let cancellation = CancellationToken::new();
        cancellation.cancel();
        tokio::select! {
            biased;
            _ = cancellation.cancelled() => {}
            delivered = wait_for_incoming_delivery(&mut pending_incoming) => {
                panic!("blocked incoming delivery masked cancellation: {delivered}");
            }
        }

        pending_incoming.take();
        assert_eq!(
            incoming_rx.recv().await.unwrap().pop_front().unwrap(),
            test_ipv4_packet(1)
        );
        assert!(incoming_rx.try_recv().is_err());
    }

    #[test]
    fn reconnect_and_probe_jitter_stays_within_twenty_percent() {
        for (sequence, base) in RECONNECT_DELAYS.into_iter().enumerate() {
            let delay = jitter_duration(base, H3_PROBE_JITTER_PERCENT, sequence as u32);
            assert!(delay >= base.mul_f64(0.8));
            assert!(delay <= base.mul_f64(1.2));
        }
        let probe = jitter_duration(H3_PROBE_INTERVAL, H3_PROBE_JITTER_PERCENT, 42);
        assert!(probe >= Duration::from_secs(8 * 60));
        assert!(probe <= Duration::from_secs(12 * 60));
    }

    #[test]
    fn endpoint_family_does_not_limit_connect_ip_payload_families() {
        let v4 = runtime_path(Transport::Http3, AddressFamily::Ipv4);
        assert!(v4.ipv4_available);
        assert!(v4.ipv6_available);

        let v6 = runtime_path(Transport::Http2, AddressFamily::Ipv6);
        assert!(v6.ipv4_available);
        assert!(v6.ipv6_available);
    }

    #[tokio::test]
    async fn happy_eyeballs_starts_alternate_after_delay_when_preferred_is_blackholed() {
        let result = race_candidates(
            async {
                sleep(Duration::from_secs(5)).await;
                Ok::<_, &'static str>("ipv6")
            },
            async { Ok::<_, &'static str>("ipv4") },
            Duration::from_millis(20),
        )
        .await
        .unwrap();
        assert_eq!(result, ("ipv4", true));
    }

    #[tokio::test]
    async fn happy_eyeballs_starts_alternate_immediately_after_preferred_failure() {
        let started = Instant::now();
        let result = race_candidates(
            async { Err::<&'static str, _>("ipv6 failed") },
            async { Ok::<_, &'static str>("ipv4") },
            Duration::from_secs(1),
        )
        .await
        .unwrap();
        assert_eq!(result, ("ipv4", true));
        assert!(started.elapsed() < Duration::from_millis(250));
    }

    #[tokio::test]
    async fn happy_eyeballs_keeps_preferred_alive_when_alternate_fails() {
        let result = race_candidates(
            async {
                sleep(Duration::from_millis(30)).await;
                Ok::<_, &'static str>("ipv6")
            },
            async { Err::<&'static str, _>("ipv4 failed") },
            Duration::from_millis(10),
        )
        .await
        .unwrap();
        assert_eq!(result, ("ipv6", false));
    }

    #[test]
    fn absent_peer_capsules_preserve_out_of_band_dual_stack_configuration() {
        let base = runtime_path(Transport::Http3, AddressFamily::Ipv4);
        let path = apply_peer_network_state(
            base,
            &PeerNetworkState::default(),
            Ipv4Addr::new(172, 16, 0, 2),
            "2606:4700:110::2".parse().unwrap(),
        );
        assert!(path.ipv4_available);
        assert!(path.ipv6_available);
    }

    #[test]
    fn address_assignment_withdrawal_degrades_or_stops_families_fail_closed() {
        let base = runtime_path(Transport::Http3, AddressFamily::Ipv4);
        let ipv4_only = PeerNetworkState {
            assignments_advertised: true,
            assigned_addresses: vec![IpPrefix {
                request_id: 0,
                address: IpAddr::V4(Ipv4Addr::new(172, 16, 0, 0)),
                prefix_len: 24,
            }],
            ..PeerNetworkState::default()
        };
        let path = apply_peer_network_state(
            base,
            &ipv4_only,
            Ipv4Addr::new(172, 16, 0, 2),
            "2606:4700:110::2".parse().unwrap(),
        );
        assert!(path.ipv4_available);
        assert!(!path.ipv6_available);

        let withdrawn = PeerNetworkState {
            assignments_advertised: true,
            ..PeerNetworkState::default()
        };
        let path = apply_peer_network_state(
            base,
            &withdrawn,
            Ipv4Addr::new(172, 16, 0, 2),
            "2606:4700:110::2".parse().unwrap(),
        );
        assert!(!path.ipv4_available);
        assert!(!path.ipv6_available);
    }

    #[test]
    fn peer_routes_must_cover_a_complete_family_for_full_tunnel_policy() {
        let base = runtime_path(Transport::Http3, AddressFamily::Ipv6);
        let state = PeerNetworkState {
            routes_advertised: true,
            available_routes: vec![
                IpAddressRange {
                    start: IpAddr::V4(Ipv4Addr::UNSPECIFIED),
                    end: IpAddr::V4(Ipv4Addr::new(127, 255, 255, 255)),
                    protocol: 0,
                },
                IpAddressRange {
                    start: IpAddr::V4(Ipv4Addr::new(128, 0, 0, 0)),
                    end: IpAddr::V4(Ipv4Addr::BROADCAST),
                    protocol: 0,
                },
                IpAddressRange {
                    start: IpAddr::V6(Ipv6Addr::UNSPECIFIED),
                    end: IpAddr::V6("ffff:ffff:ffff:ffff::".parse().unwrap()),
                    protocol: 0,
                },
            ],
            ..PeerNetworkState::default()
        };
        let path = apply_peer_network_state(
            base,
            &state,
            Ipv4Addr::new(172, 16, 0, 2),
            "2606:4700:110::2".parse().unwrap(),
        );
        assert!(path.ipv4_available);
        assert!(!path.ipv6_available);

        let ipv4_packet = [0x45];
        let ipv6_packet = [0x60];
        assert!(packet_allowed_by_peer_state(&ipv4_packet, path));
        assert!(!packet_allowed_by_peer_state(&ipv6_packet, path));
    }

    #[tokio::test]
    async fn cancelled_udp_receive_does_not_replay_a_stale_socket_handle() {
        let (stack, _pipe) = bounded_piped(Config::default());
        let channel = stack.command_channel();
        let stack_task = stack.spawn_tokio();
        channel
            .set_ips([IpAddr::V4("10.0.0.2".parse().unwrap())])
            .await
            .unwrap();

        let socket = channel
            .udp_bind("10.0.0.2:49152".parse().unwrap())
            .await
            .unwrap();
        assert!(
            timeout(Duration::from_millis(5), socket.recv_from_bytes())
                .await
                .is_err()
        );
        drop(socket);

        // Processing another command pumps the cancelled WouldBlock receive.
        // The patched core must discard it before the queued Close invalidates
        // the smoltcp handle.
        let second = timeout(
            Duration::from_secs(1),
            channel.udp_bind("10.0.0.2:49153".parse().unwrap()),
        )
        .await
        .expect("netstack task did not survive cancellation")
        .expect("second UDP bind failed");
        drop(second);

        stack_task.abort();
        let _ = stack_task.await;
    }
}
