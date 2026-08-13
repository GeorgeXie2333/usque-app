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
use usque_core::{AddressFamily, IpPolicy, Profile, Transport, TransportPolicy};
use usque_protocol::{IpAddressRange, IpPrefix, PeerNetworkState};

use crate::h2::{MasqueTlsIdentity, TransportError, connect_h2_with_protector};
use crate::h3::connect_h3_with_protector;
use crate::pin_refresh::EndpointPinRefresher;
use crate::socket::SocketProtector;
use crate::tunnel::MasqueTunnel;

const HAPPY_EYEBALLS_DELAY: Duration = Duration::from_millis(250);
const STACK_COMMAND_CAPACITY: usize = 256;
const PACKET_SEND_TIMEOUT: Duration = Duration::from_secs(10);
const STABLE_CONNECTION_RESET: Duration = Duration::from_secs(60);
const H3_PROBE_INTERVAL: Duration = Duration::from_secs(10 * 60);
const H3_PROBE_JITTER_PERCENT: u64 = 20;
const RAW_PACKET_CHANNEL_CAPACITY: usize = 1_024;
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
    },
    Failed {
        last_path: RuntimePath,
        reconnect_count: u32,
        message: String,
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
}

pub(crate) struct PacketStack {
    pub(crate) channel: Channel,
    pub(crate) protector: Arc<dyn SocketProtector>,
    pub(crate) cancellation: CancellationToken,
    pub(crate) failure: watch::Receiver<Option<String>>,
    pub(crate) counters: Arc<TrafficCounters>,
    tcp_buffer_metrics: TcpBufferMetrics,
    health: watch::Receiver<RuntimeHealth>,
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
        assigned_ipv4: std::net::Ipv4Addr,
        assigned_ipv6: std::net::Ipv6Addr,
        monitor: &ManagedTunnelMonitor,
        parent_cancellation: &CancellationToken,
        protector: Arc<dyn SocketProtector>,
    ) -> Result<(Self, WakingPipe), TransportError> {
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
                cancellation: parent_cancellation.child_token(),
                failure: monitor.failure.clone(),
                counters: Arc::clone(&monitor.counters),
                tcp_buffer_metrics,
                health: monitor.health.clone(),
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
        let (tunnel, endpoint_family, identity, pin_refresh_attempted) =
            connect_initial_with_refresh(
                profile,
                identity,
                Arc::clone(&protector),
                pin_refresher.as_ref(),
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
        let mut tasks = vec![stack_task];
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
            cancellation,
            failure,
            counters,
            tcp_buffer_metrics,
            health,
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
        }
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

fn proxy_netstack_config(profile: &Profile) -> (Config, TcpBufferMetrics) {
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

fn bounded_piped(config: Config) -> (Netstack<WakingPipeDev>, WakingPipe) {
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
    outgoing: Option<mpsc::Sender<Bytes>>,
    incoming: mpsc::Receiver<Bytes>,
    cancellation: CancellationToken,
    failure: watch::Receiver<Option<String>>,
    health: watch::Receiver<RuntimeHealth>,
    control: watch::Receiver<PeerNetworkState>,
    counters: Arc<TrafficCounters>,
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
}

#[derive(Clone)]
pub struct ManagedTunnelSender {
    outgoing: mpsc::Sender<Bytes>,
}

impl ManagedTunnelSender {
    pub async fn send_packet(&self, packet: &[u8]) -> Result<(), TransportError> {
        crate::h2::validate_ip_packet(packet)?;
        self.send_owned_packet(Bytes::copy_from_slice(packet)).await
    }

    pub(crate) async fn send_owned_packet(&self, packet: Bytes) -> Result<(), TransportError> {
        crate::h2::validate_ip_packet(&packet)?;
        self.outgoing
            .send(packet)
            .await
            .map_err(|_| TransportError::TunnelClosed)
    }
}

impl ManagedTunnelMonitor {
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
}

impl ManagedTunnelRuntime {
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
        let (tunnel, endpoint_family, identity, pin_refresh_attempted) =
            connect_initial_with_refresh(
                profile,
                identity,
                Arc::clone(&protector),
                pin_refresher.as_ref(),
            )
            .await?;
        let path = runtime_path(tunnel.transport(), endpoint_family);
        let (outgoing, outgoing_rx) = mpsc::channel(RAW_PACKET_CHANNEL_CAPACITY);
        let (incoming_tx, incoming) = mpsc::channel(RAW_PACKET_CHANNEL_CAPACITY);
        let cancellation = CancellationToken::new();
        let (failure_tx, failure) = watch::channel(None);
        let (health_tx, health) = watch::channel(RuntimeHealth::Connected {
            path,
            reconnect_count: 0,
        });
        let (control_tx, control) = watch::channel(PeerNetworkState::default());
        let counters = Arc::new(TrafficCounters::default());
        let mut tasks = vec![tokio::spawn(run_transport_supervisor(
            tunnel,
            endpoint_family,
            PacketIo::Channel {
                outgoing: outgoing_rx,
                incoming: incoming_tx,
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
            },
        ))];
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
            cancellation,
            failure,
            health,
            control,
            counters,
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
        })
    }

    pub async fn receive_packet(&mut self) -> Result<Bytes, TransportError> {
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

    pub fn monitor(&self) -> ManagedTunnelMonitor {
        ManagedTunnelMonitor {
            failure: self.failure.clone(),
            health: self.health.clone(),
            control: self.control.clone(),
            counters: Arc::clone(&self.counters),
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
) -> Result<(MasqueTunnel, AddressFamily, Arc<MasqueTlsIdentity>, bool), TransportError> {
    match connect_with_policy(profile, identity.as_ref(), Arc::clone(&protector)).await {
        Ok((tunnel, family)) => Ok((tunnel, family, identity, false)),
        Err(TransportError::EndpointPinMismatch) => {
            let Some(pin_refresher) = pin_refresher else {
                return Err(TransportError::EndpointPinMismatch);
            };
            let refreshed = pin_refresher.refresh(Arc::clone(&protector)).await?;
            ensure_assignments_unchanged(identity.as_ref(), &refreshed)?;
            let refreshed = Arc::new(refreshed);
            match connect_with_policy(profile, refreshed.as_ref(), protector).await {
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
) -> Result<(MasqueTunnel, AddressFamily), TransportError> {
    match profile.transport {
        TransportPolicy::Http3 => {
            connect_happy_eyeballs(profile, identity, Transport::Http3, protector).await
        }
        TransportPolicy::Http2 => {
            connect_happy_eyeballs(profile, identity, Transport::Http2, protector).await
        }
        TransportPolicy::Auto => {
            let h3_result =
                connect_happy_eyeballs(profile, identity, Transport::Http3, Arc::clone(&protector))
                    .await;
            match h3_result {
                Ok(connected) => Ok(connected),
                Err(TransportError::EndpointPinMismatch) => {
                    Err(TransportError::EndpointPinMismatch)
                }
                Err(h3_error) => {
                    match connect_happy_eyeballs(profile, identity, Transport::Http2, protector)
                        .await
                    {
                        Ok(connected) => Ok(connected),
                        Err(TransportError::EndpointPinMismatch) => {
                            Err(TransportError::EndpointPinMismatch)
                        }
                        Err(h2_error) => Err(TransportError::AllEndpointsFailed(format!(
                            "HTTP/3: {h3_error}; HTTP/2: {h2_error}"
                        ))),
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
            alternate,
            &profile.endpoint.sni,
            identity,
            protector,
        )
        .await
        .map(|tunnel| (tunnel, alternate_family));
    }

    let preferred_connect = connect_endpoint(
        transport,
        preferred,
        &profile.endpoint.sni,
        identity,
        Arc::clone(&protector),
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
        alternate,
        &profile.endpoint.sni,
        identity,
        protector,
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

async fn connect_endpoint(
    transport: Transport,
    endpoint: std::net::SocketAddr,
    sni: &str,
    identity: &MasqueTlsIdentity,
    protector: Arc<dyn SocketProtector>,
) -> Result<MasqueTunnel, TransportError> {
    for _ in 0..8 {
        let network_generation = protector.network_generation();
        let connecting = async {
            match transport {
                Transport::Http3 => {
                    connect_h3_with_protector(endpoint, sni, identity, protector.as_ref())
                        .await
                        .map(MasqueTunnel::Http3)
                }
                Transport::Http2 => {
                    connect_h2_with_protector(endpoint, sni, identity, protector.as_ref())
                        .await
                        .map(MasqueTunnel::Http2)
                }
            }
        };
        tokio::pin!(connecting);
        tokio::select! {
            result = &mut connecting => return result,
            _ = wait_for_network_change(&protector, network_generation), if network_generation.is_some() => {
                continue;
            }
        }
    }
    Err(TransportError::UnderlyingNetworkChanged)
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
    Switch(MasqueTunnel, AddressFamily),
    Reconnect(String),
    PinMismatch,
    Terminal(String),
    Shutdown,
}

enum PacketIo {
    Pipe {
        rx: WakingPipeReceiver,
        tx: WakingPipeSender,
    },
    Channel {
        outgoing: mpsc::Receiver<Bytes>,
        incoming: mpsc::Sender<Bytes>,
    },
}

impl PacketIo {
    fn from_pipe(pipe: WakingPipe) -> Self {
        let WakingPipe { rx, tx } = pipe;
        Self::Pipe { rx, tx }
    }

    async fn receive_outgoing(&mut self) -> Option<Bytes> {
        loop {
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

    async fn send_incoming(&mut self, packet: Bytes) -> bool {
        match self {
            Self::Pipe { tx, .. } => {
                tx.send_async(&packet).await;
                true
            }
            Self::Channel { incoming, .. } => incoming.send(packet).await.is_ok(),
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
            probe_generation,
            protector.network_generation(),
        )
        .await;
        probe_generation = probe_generation.wrapping_add(1);

        match outcome {
            ActiveOutcome::Shutdown => return,
            ActiveOutcome::Terminal(message) => {
                let _ = health_tx.send(RuntimeHealth::Failed {
                    last_path: active_path,
                    reconnect_count,
                    message: message.clone(),
                });
                report_failure(&cancellation, &failure_tx, message);
                return;
            }
            ActiveOutcome::Switch(tunnel, family) => {
                active_tunnel = tunnel;
                active_family = family;
                continue;
            }
            ActiveOutcome::PinMismatch => {
                control_tx.send_replace(PeerNetworkState::default());
                reconnect_count = reconnect_count.saturating_add(1);
                let reason = TransportError::EndpointPinMismatch.to_string();
                let _ = health_tx.send(RuntimeHealth::Reconnecting {
                    last_path: active_path,
                    attempt: 1,
                    reconnect_count,
                    reason,
                });
                if pin_refresh_attempted {
                    let message = TransportError::EndpointPinMismatch.to_string();
                    let _ = health_tx.send(RuntimeHealth::Failed {
                        last_path: active_path,
                        reconnect_count,
                        message: message.clone(),
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
                    &mut packet_io,
                    &cancellation,
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
                        let message = error.to_string();
                        let _ = health_tx.send(RuntimeHealth::Failed {
                            last_path: active_path,
                            reconnect_count,
                            message: message.clone(),
                        });
                        report_failure(&cancellation, &failure_tx, message);
                        return;
                    }
                    None => return,
                }
            }
            ActiveOutcome::Reconnect(mut reason) => {
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
                    let _ = health_tx.send(RuntimeHealth::Reconnecting {
                        last_path: active_path,
                        attempt,
                        reconnect_count,
                        reason: reason.clone(),
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
                    )
                    .await
                    {
                        Some(Ok((tunnel, family))) => {
                            active_tunnel = tunnel;
                            active_family = family;
                            break;
                        }
                        Some(Err(TransportError::EndpointPinMismatch)) => {
                            if pin_refresh_attempted {
                                let message = TransportError::EndpointPinMismatch.to_string();
                                let _ = health_tx.send(RuntimeHealth::Failed {
                                    last_path: active_path,
                                    reconnect_count,
                                    message: message.clone(),
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
                                &mut packet_io,
                                &cancellation,
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
                                    let message = error.to_string();
                                    let _ = health_tx.send(RuntimeHealth::Failed {
                                        last_path: active_path,
                                        reconnect_count,
                                        message: message.clone(),
                                    });
                                    report_failure(&cancellation, &failure_tx, message);
                                    return;
                                }
                                None => return,
                            }
                        }
                        Some(Err(error)) => {
                            reason = error.to_string();
                        }
                        None => return,
                    }
                }
            }
        }
    }
}

async fn refresh_and_retry_connection(
    profile: &Profile,
    current: &MasqueTlsIdentity,
    pin_refresher: Option<&Arc<dyn EndpointPinRefresher>>,
    protector: Arc<dyn SocketProtector>,
    packet_io: &mut PacketIo,
    cancellation: &CancellationToken,
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
        packet_io,
        cancellation,
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
            "the CONNECT-IP peer withdrew every usable address family".to_owned(),
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
            ))
        } else {
            None
        };

    let outcome = loop {
        tokio::select! {
            _ = cancellation.cancelled() => break ActiveOutcome::Shutdown,
            _ = wait_for_network_change(&protector, network_generation), if network_generation.is_some() => {
                break ActiveOutcome::Reconnect(
                    "the selected Android physical network changed".to_owned(),
                );
            }
            packet = packet_io.receive_outgoing() => {
                let Some(packet) = packet else {
                    break ActiveOutcome::Shutdown;
                };
                if !packet_allowed_by_peer_state(&packet, current_path) {
                    tracing::warn!(
                        family = packet.first().map(|byte| byte >> 4),
                        "discarded a packet for an address family withdrawn by the CONNECT-IP peer"
                    );
                    continue;
                }
                let packet_length = packet.len();
                // A cheap reference-counted view is retained only so an H3
                // datagram-size rejection can be reflected as ICMP.
                let retained_packet = packet.clone();
                match timeout(PACKET_SEND_TIMEOUT, send.send_owned_packet(packet)).await {
                    Ok(Ok(())) => {
                        counters.sent.fetch_add(packet_length as u64, Ordering::Relaxed);
                    }
                    Ok(Err(TransportError::Http3DatagramTooLarge {
                        maximum_packet_size,
                    })) => {
                        match crate::icmp::packet_too_big(&retained_packet, maximum_packet_size) {
                            Ok(icmp) => {
                                if !packet_io.send_incoming(icmp).await {
                                    break ActiveOutcome::Shutdown;
                                }
                            }
                            Err(TransportError::Ipv6MinimumMtuUnavailable(maximum)) => {
                                break ActiveOutcome::Terminal(
                                    TransportError::Ipv6MinimumMtuUnavailable(maximum).to_string(),
                                );
                            }
                            Err(error) => {
                                tracing::warn!(
                                    %error,
                                    "failed to generate ICMP Packet Too Big"
                                );
                            }
                        }
                    }
                    Ok(Err(error)) => break ActiveOutcome::Reconnect(error.to_string()),
                    Err(_) => {
                        break ActiveOutcome::Reconnect(
                            "sending a packet through MASQUE timed out".to_owned(),
                        );
                    }
                }
            }
            result = receive.receive_packet() => {
                match result {
                    Ok(packet) => {
                        if !packet_allowed_by_peer_state(&packet, current_path) {
                            tracing::warn!(
                                family = packet.first().map(|byte| byte >> 4),
                                "discarded a peer packet for an unavailable address family"
                            );
                            continue;
                        }
                        counters.received.fetch_add(packet.len() as u64, Ordering::Relaxed);
                        if !packet_io.send_incoming(packet).await {
                            break ActiveOutcome::Shutdown;
                        }
                    }
                    Err(error) => break ActiveOutcome::Reconnect(error.to_string()),
                }
            }
            result = &mut driver_wait => {
                let message = match result {
                    Ok(()) => "the MASQUE connection ended unexpectedly".to_owned(),
                    Err(error) => error.to_string(),
                };
                break ActiveOutcome::Reconnect(message);
            }
            changed = wait_for_control(&mut control), if control.is_some() => {
                match changed {
                    Ok(state) => {
                        peer_state = state;
                        control_tx.send_replace(peer_state.clone());
                        current_path = apply_peer_network_state(
                            base_path,
                            &peer_state,
                            identity.assigned_ipv4,
                            identity.assigned_ipv6,
                        );
                        if !current_path.ipv4_available && !current_path.ipv6_available {
                            break ActiveOutcome::Terminal(
                                "the CONNECT-IP peer withdrew every usable address family"
                                    .to_owned(),
                            );
                        }
                        let _ = health_tx.send(RuntimeHealth::Connected {
                            path: current_path,
                            reconnect_count,
                        });
                    }
                    Err(()) => {
                        break ActiveOutcome::Reconnect(
                            "the CONNECT-IP control stream ended unexpectedly".to_owned(),
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
                        break ActiveOutcome::Switch(tunnel, family);
                    }
                    Err(TransportError::EndpointPinMismatch) => {
                        break ActiveOutcome::PinMismatch;
                    }
                    Err(error) => {
                        tracing::debug!(%error, "non-bearing H3 recovery probe failed");
                        probe = Some(schedule_h3_probe(
                            profile.clone(),
                            Arc::clone(&identity),
                            Arc::clone(&protector),
                            probe_generation.wrapping_add(1),
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
) -> ProbeFuture {
    Box::pin(async move {
        sleep(jitter_duration(
            H3_PROBE_INTERVAL,
            H3_PROBE_JITTER_PERCENT,
            generation,
        ))
        .await;
        connect_happy_eyeballs(&profile, identity.as_ref(), Transport::Http3, protector).await
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
            packet = packet_io.receive_outgoing() => {
                if packet.is_none() {
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
) -> Option<Result<(MasqueTunnel, AddressFamily), TransportError>> {
    let network_generation = protector.network_generation();
    let connect = connect_with_policy(profile, identity.as_ref(), Arc::clone(&protector));
    tokio::pin!(connect);
    loop {
        tokio::select! {
            _ = cancellation.cancelled() => return None,
            _ = wait_for_network_change(&protector, network_generation), if network_generation.is_some() => {
                return Some(Err(TransportError::UnderlyingNetworkChanged));
            }
            result = &mut connect => return Some(result),
            packet = packet_io.receive_outgoing() => {
                packet?;
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
fn apply_peer_network_state(
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
    use ts_netstack_smoltcp::CreateSocket;

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
