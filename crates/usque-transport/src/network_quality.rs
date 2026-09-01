use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard};
use std::time::Duration;

use tokio::sync::watch;
use tokio::task::JoinHandle;
use tokio::time::{Instant, MissedTickBehavior, interval_at};
use tokio_util::sync::CancellationToken;
use usque_core::{AddressFamily, Transport};
use uuid::Uuid;

use crate::queue_metrics::{ALL_QUEUE_KINDS, QueueKind, QueueMetrics, QueueMetricsSnapshot};

const METRIC_STALE_AFTER: Duration = Duration::from_secs(3);
const QUALITY_HISTORY_SAMPLES: usize = 30;
const QUALITY_MINIMUM_SAMPLES: usize = 5;
const SAMPLE_INTERVAL: Duration = Duration::from_secs(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetricAvailability {
    Available,
    Unsupported,
    NotReady,
    Stale,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MetricValue<T> {
    pub availability: MetricAvailability,
    pub value: Option<T>,
}

impl<T> MetricValue<T> {
    pub fn available(value: T) -> Self {
        Self {
            availability: MetricAvailability::Available,
            value: Some(value),
        }
    }

    pub fn unsupported() -> Self {
        Self {
            availability: MetricAvailability::Unsupported,
            value: None,
        }
    }

    pub fn not_ready() -> Self {
        Self {
            availability: MetricAvailability::NotReady,
            value: None,
        }
    }

    pub fn stale(value: Option<T>) -> Self {
        Self {
            availability: MetricAvailability::Stale,
            value,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ConnectionInstanceId(pub Uuid);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkQualityLevel {
    Good,
    Fair,
    Poor,
    LimitedData,
    Disconnected,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RttQuality {
    pub latest: MetricValue<Duration>,
    pub smoothed: MetricValue<Duration>,
    pub minimum: MetricValue<Duration>,
    pub variance: MetricValue<Duration>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LossQuality {
    pub interval_basis_points: MetricValue<u32>,
    pub sent_packets: MetricValue<u64>,
    pub received_packets: MetricValue<u64>,
    pub lost_packets: MetricValue<u64>,
    pub sent_bytes: MetricValue<u64>,
    pub received_bytes: MetricValue<u64>,
    pub lost_bytes: MetricValue<u64>,
    pub pto_count: MetricValue<u64>,
    pub datagrams_sent: MetricValue<u64>,
    pub datagrams_received: MetricValue<u64>,
    pub datagrams_lost: MetricValue<u64>,
    pub datagram_receive_drops: MetricValue<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CongestionQuality {
    pub congestion_window_bytes: MetricValue<u64>,
    pub bytes_in_flight: MetricValue<u64>,
    pub send_rate_bps: MetricValue<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PmtuPhase {
    Unsupported,
    Unknown,
    Probing,
    Stable,
    Revalidating,
    Degraded,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PmtuQuality {
    pub phase: PmtuPhase,
    pub current_bytes: MetricValue<u32>,
    pub effective_connect_ip_payload_bytes: MetricValue<u32>,
    pub change_count: u64,
    pub revalidation_failure_count: u64,
    pub send_too_large_count: u64,
    pub last_change_age: MetricValue<Duration>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueueQuality {
    pub kind: QueueKind,
    pub availability: MetricAvailability,
    pub current_items: u64,
    pub current_bytes: u64,
    pub item_capacity: u64,
    pub byte_capacity: u64,
    pub items_high_water: u64,
    pub bytes_high_water: u64,
    pub enqueue_count: u64,
    pub dequeue_count: u64,
    pub drop_items: u64,
    pub drop_bytes: u64,
    pub oldest_age: MetricValue<Duration>,
    pub closed: bool,
    pub cancelled: bool,
}

impl QueueQuality {
    fn from_snapshot(snapshot: QueueMetricsSnapshot, transport: Option<Transport>) -> Self {
        let h3_only_on_h2 = transport == Some(Transport::Http2)
            && matches!(
                snapshot.kind,
                QueueKind::H3DatagramSend | QueueKind::H3WireSend
            );
        let availability = if h3_only_on_h2 {
            MetricAvailability::Unsupported
        } else if snapshot.registered {
            MetricAvailability::Available
        } else {
            MetricAvailability::NotReady
        };
        let oldest_age = if availability == MetricAvailability::Unsupported {
            MetricValue::unsupported()
        } else if !snapshot.registered {
            MetricValue::not_ready()
        } else if snapshot.current_items == 0 {
            MetricValue::available(Duration::ZERO)
        } else {
            snapshot
                .oldest_age
                .map_or_else(MetricValue::not_ready, MetricValue::available)
        };
        Self {
            kind: snapshot.kind,
            availability,
            current_items: snapshot.current_items,
            current_bytes: snapshot.current_bytes,
            item_capacity: snapshot.item_capacity,
            byte_capacity: snapshot.byte_capacity,
            items_high_water: snapshot.items_high_water,
            bytes_high_water: snapshot.bytes_high_water,
            enqueue_count: snapshot.enqueue_count,
            dequeue_count: snapshot.dequeue_count,
            drop_items: snapshot.drop_items,
            drop_bytes: snapshot.drop_bytes,
            oldest_age,
            closed: snapshot.closed,
            cancelled: snapshot.cancelled,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UdpIoQuality {
    pub send_syscalls: u64,
    pub recv_syscalls: u64,
    pub sent_datagrams: u64,
    pub received_datagrams: u64,
    pub partial_batches: u64,
    pub batch_fallbacks: u64,
    pub receive_truncations: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AllocationQuality {
    pub packet_buffer_pool_hits: u64,
    pub packet_buffer_pool_misses: u64,
    pub fresh_allocations: u64,
    pub encode_buffer_reuses: u64,
    pub borrowed_to_owned_copy_bytes: u64,
    pub datagram_header_copy_bytes: u64,
    pub buffer_recycles: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MigrationPhase {
    Idle,
    PreparingSocket,
    Probing,
    Validated,
    Promoting,
    Stable,
    Aborted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MigrationReasonCode {
    FamilyUnavailable,
    SocketProtectFailed,
    GenerationChangedDuringSetup,
    PeerCidUnavailable,
    LocalCidUnavailable,
    PathProbeRejected,
    PathValidationTimeout,
    Superseded,
    PromotionFailed,
    ConnectionClosed,
    Unsupported,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrationQuality {
    pub phase: MigrationPhase,
    pub attempts: u64,
    pub successes: u64,
    pub failures: u64,
    pub last_duration: MetricValue<Duration>,
    pub last_reason: Option<MigrationReasonCode>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DirectDnsMode {
    PhysicalSystem,
    Doh,
    Dot,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DirectDnsPhase {
    System,
    Connecting,
    Ready,
    Degraded,
    Disabled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DirectDnsReasonCode {
    Timeout,
    QueryFailed,
    NetworkChanged,
    Unsupported,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectDnsQuality {
    pub mode: DirectDnsMode,
    pub phase: DirectDnsPhase,
    pub successes: u64,
    pub failures: u64,
    pub timeouts: u64,
    pub last_rtt: MetricValue<Duration>,
    pub last_reason: Option<DirectDnsReasonCode>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct H2FlowControlQuality {
    pub stream_receive_window_bytes: u32,
    pub connection_receive_window_bytes: u32,
    pub capacity_stall_count: u64,
    pub capacity_stall_total: Duration,
    pub capacity_stall_max: Duration,
    pub capacity_wait_cancelled: u64,
    pub capacity_wait_errors: u64,
    pub ping_timeout_count: u64,
    pub ping_error_count: u64,
    pub ping_consecutive_failures: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetworkQualitySnapshot {
    pub sampled_at: Instant,
    pub connection_id: Option<ConnectionInstanceId>,
    pub transport: Option<Transport>,
    pub endpoint_family: Option<AddressFamily>,
    pub level: NetworkQualityLevel,
    pub rtt: RttQuality,
    pub loss: LossQuality,
    pub congestion: CongestionQuality,
    pub pmtu: PmtuQuality,
    pub queues: Vec<QueueQuality>,
    pub udp_io: UdpIoQuality,
    pub allocations: AllocationQuality,
    pub migration: MigrationQuality,
    pub direct_dns: DirectDnsQuality,
    pub h2_flow_control: H2FlowControlQuality,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct H3MetricsSample {
    pub rtt: Duration,
    pub min_rtt: Option<Duration>,
    pub rtt_variance: Duration,
    pub congestion_window_bytes: u64,
    pub send_rate_bytes_per_second: u64,
    pub sent_packets: u64,
    pub received_packets: u64,
    pub lost_packets: u64,
    pub sent_bytes: u64,
    pub received_bytes: u64,
    pub lost_bytes: u64,
    pub pto_count: u64,
    pub datagrams_sent: u64,
    pub datagrams_received: u64,
    pub datagrams_lost: u64,
    pub datagram_receive_drops: u64,
}

#[derive(Debug, Clone)]
struct TimedH3Sample {
    observed_at: Instant,
    sample: H3MetricsSample,
}

#[derive(Debug, Clone, Default)]
struct H2State {
    ping_supported: Option<bool>,
    rtt_stale: bool,
    observed_at: Option<Instant>,
    latest_rtt: Option<Duration>,
    smoothed_rtt: Option<Duration>,
    minimum_rtt: Option<Duration>,
    variance: Option<Duration>,
    flow_control: H2FlowControlQuality,
}

#[derive(Debug, Clone)]
struct PmtuState {
    phase: PmtuPhase,
    current_bytes: Option<u32>,
    effective_connect_ip_payload_bytes: Option<u32>,
    change_count: u64,
    revalidation_failure_count: u64,
    send_too_large_count: u64,
    last_change: Option<Instant>,
}

impl Default for PmtuState {
    fn default() -> Self {
        Self {
            phase: PmtuPhase::Unsupported,
            current_bytes: None,
            effective_connect_ip_payload_bytes: None,
            change_count: 0,
            revalidation_failure_count: 0,
            send_too_large_count: 0,
            last_change: None,
        }
    }
}

#[derive(Debug, Clone)]
struct MigrationState {
    phase: MigrationPhase,
    attempts: u64,
    successes: u64,
    failures: u64,
    last_duration: Option<Duration>,
    last_reason: Option<MigrationReasonCode>,
}

impl Default for MigrationState {
    fn default() -> Self {
        Self {
            phase: MigrationPhase::Idle,
            attempts: 0,
            successes: 0,
            failures: 0,
            last_duration: None,
            last_reason: None,
        }
    }
}

#[derive(Debug, Clone)]
struct DirectDnsState {
    mode: DirectDnsMode,
    phase: DirectDnsPhase,
    successes: u64,
    failures: u64,
    timeouts: u64,
    last_rtt: Option<Duration>,
    last_reason: Option<DirectDnsReasonCode>,
}

impl Default for DirectDnsState {
    fn default() -> Self {
        Self {
            mode: DirectDnsMode::PhysicalSystem,
            phase: DirectDnsPhase::System,
            successes: 0,
            failures: 0,
            timeouts: 0,
            last_rtt: None,
            last_reason: None,
        }
    }
}

#[derive(Debug, Clone, Default)]
struct QualityState {
    connection_id: Option<ConnectionInstanceId>,
    transport: Option<Transport>,
    endpoint_family: Option<AddressFamily>,
    h3: Option<TimedH3Sample>,
    h2: H2State,
    pmtu: PmtuState,
    migration: MigrationState,
    direct_dns: DirectDnsState,
}

#[derive(Debug, Default)]
struct UdpIoCounters {
    send_syscalls: AtomicU64,
    recv_syscalls: AtomicU64,
    sent_datagrams: AtomicU64,
    received_datagrams: AtomicU64,
    partial_batches: AtomicU64,
    batch_fallbacks: AtomicU64,
    receive_truncations: AtomicU64,
}

#[derive(Debug, Default)]
struct AllocationCounters {
    packet_buffer_pool_hits: AtomicU64,
    packet_buffer_pool_misses: AtomicU64,
    fresh_allocations: AtomicU64,
    encode_buffer_reuses: AtomicU64,
    borrowed_to_owned_copy_bytes: AtomicU64,
    datagram_header_copy_bytes: AtomicU64,
    buffer_recycles: AtomicU64,
}

#[derive(Clone)]
pub struct NetworkQualityTelemetry {
    inner: Arc<NetworkQualityTelemetryInner>,
}

struct NetworkQualityTelemetryInner {
    state: RwLock<QualityState>,
    queues: RwLock<[Arc<QueueMetrics>; 8]>,
    udp_io: UdpIoCounters,
    allocations: AllocationCounters,
    active_h2_ping_tasks: AtomicU64,
}

impl std::fmt::Debug for NetworkQualityTelemetry {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("NetworkQualityTelemetry")
            .finish_non_exhaustive()
    }
}

impl Default for NetworkQualityTelemetry {
    fn default() -> Self {
        let queues =
            std::array::from_fn(|index| QueueMetrics::unregistered(ALL_QUEUE_KINDS[index]));
        Self {
            inner: Arc::new(NetworkQualityTelemetryInner {
                state: RwLock::new(QualityState::default()),
                queues: RwLock::new(queues),
                udp_io: UdpIoCounters::default(),
                allocations: AllocationCounters::default(),
                active_h2_ping_tasks: AtomicU64::new(0),
            }),
        }
    }
}

impl NetworkQualityTelemetry {
    pub fn begin_connection(
        &self,
        transport: Transport,
        endpoint_family: AddressFamily,
    ) -> ConnectionInstanceId {
        let connection_id = ConnectionInstanceId(Uuid::new_v4());
        let mut state = self.state_write();
        state.connection_id = Some(connection_id);
        state.transport = Some(transport);
        state.endpoint_family = Some(endpoint_family);
        state.h3 = None;
        state.h2 = H2State::default();
        state.pmtu = if transport == Transport::Http3 {
            PmtuState {
                phase: PmtuPhase::Unknown,
                ..PmtuState::default()
            }
        } else {
            PmtuState::default()
        };
        state.migration.phase = MigrationPhase::Idle;
        connection_id
    }

    pub fn end_connection(&self) {
        let mut state = self.state_write();
        state.connection_id = None;
        state.transport = None;
        state.endpoint_family = None;
        state.h3 = None;
        state.h2 = H2State::default();
        state.pmtu = PmtuState::default();
        state.migration.phase = MigrationPhase::Idle;
    }

    pub fn register_queue(
        &self,
        kind: QueueKind,
        item_capacity: usize,
        byte_capacity: usize,
    ) -> Arc<QueueMetrics> {
        let metrics = QueueMetrics::new(kind, item_capacity, byte_capacity);
        self.queues_write()[queue_index(kind)] = Arc::clone(&metrics);
        metrics
    }

    pub(crate) fn register_unordered_queue(
        &self,
        kind: QueueKind,
        item_capacity: usize,
        byte_capacity: usize,
    ) -> Arc<QueueMetrics> {
        let metrics = QueueMetrics::new_unordered(kind, item_capacity, byte_capacity);
        self.queues_write()[queue_index(kind)] = Arc::clone(&metrics);
        metrics
    }

    pub fn queue(&self, kind: QueueKind) -> Arc<QueueMetrics> {
        Arc::clone(&self.queues_read()[queue_index(kind)])
    }

    pub(crate) fn observe_h3(&self, sample: H3MetricsSample) {
        let now = Instant::now();
        let mut state = self.state_write();
        if state.transport != Some(Transport::Http3) || state.connection_id.is_none() {
            return;
        }
        state.h3 = Some(TimedH3Sample {
            observed_at: now,
            sample,
        });
    }

    pub fn observe_h2_rtt(
        &self,
        latest: Duration,
        smoothed: Duration,
        minimum: Duration,
        variance: Duration,
    ) {
        let mut state = self.state_write();
        if state.transport != Some(Transport::Http2) || state.connection_id.is_none() {
            return;
        }
        state.h2.observed_at = Some(Instant::now());
        state.h2.ping_supported = Some(true);
        state.h2.rtt_stale = false;
        state.h2.latest_rtt = Some(latest);
        state.h2.smoothed_rtt = Some(smoothed);
        state.h2.minimum_rtt = Some(minimum);
        state.h2.variance = Some(variance);
        state.h2.flow_control.ping_consecutive_failures = 0;
    }

    pub fn configure_h2_connection(
        &self,
        stream_receive_window: u32,
        connection_receive_window: u32,
        ping_supported: bool,
    ) {
        let mut state = self.state_write();
        if state.transport != Some(Transport::Http2) || state.connection_id.is_none() {
            return;
        }
        state.h2.flow_control.stream_receive_window_bytes = stream_receive_window;
        state.h2.flow_control.connection_receive_window_bytes = connection_receive_window;
        state.h2.ping_supported = Some(ping_supported);
    }

    pub fn record_h2_ping_timeout(&self) {
        let mut state = self.state_write();
        if state.transport != Some(Transport::Http2) || state.connection_id.is_none() {
            return;
        }
        state.h2.rtt_stale = true;
        state.h2.flow_control.ping_timeout_count =
            state.h2.flow_control.ping_timeout_count.saturating_add(1);
        state.h2.flow_control.ping_consecutive_failures = state
            .h2
            .flow_control
            .ping_consecutive_failures
            .saturating_add(1);
    }

    pub fn record_h2_ping_error(&self) {
        let mut state = self.state_write();
        if state.transport != Some(Transport::Http2) || state.connection_id.is_none() {
            return;
        }
        state.h2.rtt_stale = true;
        state.h2.flow_control.ping_error_count =
            state.h2.flow_control.ping_error_count.saturating_add(1);
        state.h2.flow_control.ping_consecutive_failures = state
            .h2
            .flow_control
            .ping_consecutive_failures
            .saturating_add(1);
    }

    pub(crate) fn h2_ping_task_started(&self) {
        self.inner
            .active_h2_ping_tasks
            .fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn h2_ping_task_finished(&self) {
        self.inner
            .active_h2_ping_tasks
            .fetch_sub(1, Ordering::Relaxed);
    }

    #[cfg(test)]
    pub(crate) fn active_h2_ping_tasks(&self) -> u64 {
        self.inner.active_h2_ping_tasks.load(Ordering::Relaxed)
    }

    pub fn record_h2_capacity_stall(&self, duration: Duration) {
        let mut state = self.state_write();
        if state.transport != Some(Transport::Http2) || state.connection_id.is_none() {
            return;
        }
        let flow = &mut state.h2.flow_control;
        flow.capacity_stall_count = flow.capacity_stall_count.saturating_add(1);
        flow.capacity_stall_total = flow.capacity_stall_total.saturating_add(duration);
        flow.capacity_stall_max = flow.capacity_stall_max.max(duration);
    }

    pub fn record_h2_capacity_wait_cancelled(&self) {
        let mut state = self.state_write();
        if state.transport != Some(Transport::Http2) || state.connection_id.is_none() {
            return;
        }
        state.h2.flow_control.capacity_wait_cancelled = state
            .h2
            .flow_control
            .capacity_wait_cancelled
            .saturating_add(1);
    }

    pub fn record_h2_capacity_wait_error(&self) {
        let mut state = self.state_write();
        if state.transport != Some(Transport::Http2) || state.connection_id.is_none() {
            return;
        }
        state.h2.flow_control.capacity_wait_errors =
            state.h2.flow_control.capacity_wait_errors.saturating_add(1);
    }

    pub fn set_pmtu_phase(&self, phase: PmtuPhase) {
        self.state_write().pmtu.phase = phase;
    }

    pub fn observe_pmtu(
        &self,
        phase: PmtuPhase,
        current_bytes: Option<u32>,
        effective_connect_ip_payload_bytes: Option<u32>,
    ) {
        let now = Instant::now();
        let mut state = self.state_write();
        if state.transport != Some(Transport::Http3) || state.connection_id.is_none() {
            return;
        }
        if state.pmtu.current_bytes != current_bytes {
            if state.pmtu.current_bytes.is_some() && current_bytes.is_some() {
                state.pmtu.change_count = state.pmtu.change_count.saturating_add(1);
            }
            state.pmtu.current_bytes = current_bytes;
            state.pmtu.last_change = Some(now);
        }
        state.pmtu.phase = phase;
        state.pmtu.effective_connect_ip_payload_bytes = effective_connect_ip_payload_bytes;
    }

    pub fn record_pmtu_send_too_large(&self) {
        let mut state = self.state_write();
        state.pmtu.send_too_large_count = state.pmtu.send_too_large_count.saturating_add(1);
    }

    pub fn record_pmtu_revalidation_failure(&self) {
        let mut state = self.state_write();
        state.pmtu.revalidation_failure_count =
            state.pmtu.revalidation_failure_count.saturating_add(1);
    }

    pub fn set_migration_phase(&self, phase: MigrationPhase) {
        let mut state = self.state_write();
        if phase == MigrationPhase::PreparingSocket {
            state.migration.attempts = state.migration.attempts.saturating_add(1);
        }
        state.migration.phase = phase;
    }

    pub fn record_migration_success(&self, duration: Duration) {
        let mut state = self.state_write();
        state.migration.phase = MigrationPhase::Stable;
        state.migration.successes = state.migration.successes.saturating_add(1);
        state.migration.last_duration = Some(duration);
        state.migration.last_reason = None;
    }

    pub fn record_migration_failure(&self, duration: Duration, reason: MigrationReasonCode) {
        let mut state = self.state_write();
        state.migration.phase = MigrationPhase::Aborted;
        state.migration.failures = state.migration.failures.saturating_add(1);
        state.migration.last_duration = Some(duration);
        state.migration.last_reason = Some(reason);
    }

    pub fn set_migration_availability_reason(&self, reason: Option<MigrationReasonCode>) {
        let mut state = self.state_write();
        if state.migration.phase == MigrationPhase::Idle {
            state.migration.last_reason = reason;
        }
    }

    pub fn set_direct_dns_mode(&self, mode: DirectDnsMode) {
        let mut state = self.state_write();
        state.direct_dns.mode = mode;
        state.direct_dns.phase = match mode {
            DirectDnsMode::PhysicalSystem => DirectDnsPhase::System,
            DirectDnsMode::Doh | DirectDnsMode::Dot => DirectDnsPhase::Connecting,
        };
    }

    pub fn record_direct_dns_success(&self, rtt: Duration) {
        let mut state = self.state_write();
        state.direct_dns.phase = match state.direct_dns.mode {
            DirectDnsMode::PhysicalSystem => DirectDnsPhase::System,
            DirectDnsMode::Doh | DirectDnsMode::Dot => DirectDnsPhase::Ready,
        };
        state.direct_dns.successes = state.direct_dns.successes.saturating_add(1);
        state.direct_dns.last_rtt = Some(rtt);
        state.direct_dns.last_reason = None;
    }

    pub fn record_direct_dns_failure(&self, reason: DirectDnsReasonCode, timed_out: bool) {
        let mut state = self.state_write();
        state.direct_dns.phase = DirectDnsPhase::Degraded;
        state.direct_dns.failures = state.direct_dns.failures.saturating_add(1);
        if timed_out {
            state.direct_dns.timeouts = state.direct_dns.timeouts.saturating_add(1);
        }
        state.direct_dns.last_reason = Some(reason);
    }

    pub fn record_udp_send(&self, datagrams: u64) {
        saturating_add(&self.inner.udp_io.send_syscalls, 1);
        saturating_add(&self.inner.udp_io.sent_datagrams, datagrams);
    }

    pub fn record_udp_recv(&self, datagrams: u64) {
        saturating_add(&self.inner.udp_io.recv_syscalls, 1);
        saturating_add(&self.inner.udp_io.received_datagrams, datagrams);
    }

    pub fn record_udp_partial_batch(&self) {
        saturating_add(&self.inner.udp_io.partial_batches, 1);
    }

    pub fn record_udp_batch_fallback(&self) {
        saturating_add(&self.inner.udp_io.batch_fallbacks, 1);
    }

    pub fn record_udp_receive_truncation(&self) {
        saturating_add(&self.inner.udp_io.receive_truncations, 1);
    }

    pub fn record_packet_buffer_pool_hit(&self) {
        saturating_add(&self.inner.allocations.packet_buffer_pool_hits, 1);
    }

    pub fn record_packet_buffer_pool_miss(&self) {
        saturating_add(&self.inner.allocations.packet_buffer_pool_misses, 1);
    }

    pub fn record_fresh_allocation(&self) {
        saturating_add(&self.inner.allocations.fresh_allocations, 1);
    }

    pub fn record_encode_buffer_reuse(&self) {
        saturating_add(&self.inner.allocations.encode_buffer_reuses, 1);
    }

    pub fn record_borrowed_to_owned_copy(&self, bytes: usize) {
        saturating_add(
            &self.inner.allocations.borrowed_to_owned_copy_bytes,
            bytes as u64,
        );
    }

    pub fn record_datagram_header_copy(&self, bytes: usize) {
        saturating_add(
            &self.inner.allocations.datagram_header_copy_bytes,
            bytes as u64,
        );
    }

    pub fn record_buffer_recycle(&self) {
        saturating_add(&self.inner.allocations.buffer_recycles, 1);
    }

    fn state_read(&self) -> RwLockReadGuard<'_, QualityState> {
        self.inner
            .state
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn state_write(&self) -> RwLockWriteGuard<'_, QualityState> {
        self.inner
            .state
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn queues_read(&self) -> RwLockReadGuard<'_, [Arc<QueueMetrics>; 8]> {
        self.inner
            .queues
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn queues_write(&self) -> RwLockWriteGuard<'_, [Arc<QueueMetrics>; 8]> {
        self.inner
            .queues
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn udp_snapshot(&self) -> UdpIoQuality {
        UdpIoQuality {
            send_syscalls: self.inner.udp_io.send_syscalls.load(Ordering::Relaxed),
            recv_syscalls: self.inner.udp_io.recv_syscalls.load(Ordering::Relaxed),
            sent_datagrams: self.inner.udp_io.sent_datagrams.load(Ordering::Relaxed),
            received_datagrams: self.inner.udp_io.received_datagrams.load(Ordering::Relaxed),
            partial_batches: self.inner.udp_io.partial_batches.load(Ordering::Relaxed),
            batch_fallbacks: self.inner.udp_io.batch_fallbacks.load(Ordering::Relaxed),
            receive_truncations: self
                .inner
                .udp_io
                .receive_truncations
                .load(Ordering::Relaxed),
        }
    }

    fn allocation_snapshot(&self) -> AllocationQuality {
        AllocationQuality {
            packet_buffer_pool_hits: self
                .inner
                .allocations
                .packet_buffer_pool_hits
                .load(Ordering::Relaxed),
            packet_buffer_pool_misses: self
                .inner
                .allocations
                .packet_buffer_pool_misses
                .load(Ordering::Relaxed),
            fresh_allocations: self
                .inner
                .allocations
                .fresh_allocations
                .load(Ordering::Relaxed),
            encode_buffer_reuses: self
                .inner
                .allocations
                .encode_buffer_reuses
                .load(Ordering::Relaxed),
            borrowed_to_owned_copy_bytes: self
                .inner
                .allocations
                .borrowed_to_owned_copy_bytes
                .load(Ordering::Relaxed),
            datagram_header_copy_bytes: self
                .inner
                .allocations
                .datagram_header_copy_bytes
                .load(Ordering::Relaxed),
            buffer_recycles: self
                .inner
                .allocations
                .buffer_recycles
                .load(Ordering::Relaxed),
        }
    }
}

pub struct NetworkQualitySampler {
    telemetry: NetworkQualityTelemetry,
    last_connection: Option<ConnectionInstanceId>,
    previous_h3: Option<H3CounterBaseline>,
    last_interval_loss: Option<u32>,
    history: VecDeque<QualitySignal>,
    previous_queue_drops: u64,
    previous_migration_failures: u64,
}

#[derive(Debug, Clone, Copy)]
struct H3CounterBaseline {
    connection_id: ConnectionInstanceId,
    sent: u64,
    lost: u64,
}

#[derive(Debug, Clone, Copy)]
struct QualitySignal {
    rtt_ms: Option<u128>,
    loss_basis_points: Option<u32>,
    queue_percent: Option<u64>,
    queue_drop: bool,
    pmtu_degraded: bool,
    migration_failed: bool,
}

impl NetworkQualitySampler {
    pub fn new(telemetry: NetworkQualityTelemetry) -> Self {
        Self {
            telemetry,
            last_connection: None,
            previous_h3: None,
            last_interval_loss: None,
            history: VecDeque::with_capacity(QUALITY_HISTORY_SAMPLES),
            previous_queue_drops: 0,
            previous_migration_failures: 0,
        }
    }

    pub fn sample(&mut self) -> NetworkQualitySnapshot {
        let sampled_at = Instant::now();
        let state = self.telemetry.state_read().clone();
        let connection_changed = state.connection_id != self.last_connection;
        if connection_changed {
            self.last_connection = state.connection_id;
            self.previous_h3 = None;
            self.last_interval_loss = None;
            self.history.clear();
            self.previous_migration_failures = state.migration.failures;
        }

        let queues = self
            .telemetry
            .queues_read()
            .iter()
            .map(|metrics| {
                QueueQuality::from_snapshot(metrics.snapshot(sampled_at), state.transport)
            })
            .collect::<Vec<_>>();
        if connection_changed {
            self.previous_queue_drops = queue_drop_total(&queues);
        }
        let (rtt, loss, congestion) = self.transport_quality(&state, sampled_at);
        let pmtu = pmtu_quality(&state, sampled_at);
        let migration = migration_quality(&state);
        let direct_dns = direct_dns_quality(&state);
        let mut snapshot = NetworkQualitySnapshot {
            sampled_at,
            connection_id: state.connection_id,
            transport: state.transport,
            endpoint_family: state.endpoint_family,
            level: NetworkQualityLevel::LimitedData,
            rtt,
            loss,
            congestion,
            pmtu,
            queues,
            udp_io: self.telemetry.udp_snapshot(),
            allocations: self.telemetry.allocation_snapshot(),
            migration,
            direct_dns,
            h2_flow_control: state.h2.flow_control,
        };
        snapshot.level = self.classify(&snapshot);
        snapshot
    }

    fn transport_quality(
        &mut self,
        state: &QualityState,
        sampled_at: Instant,
    ) -> (RttQuality, LossQuality, CongestionQuality) {
        match (state.connection_id, state.transport) {
            (Some(connection_id), Some(Transport::Http3)) => {
                let Some(h3) = &state.h3 else {
                    return (rtt_not_ready(), loss_not_ready(), congestion_not_ready());
                };
                let stale =
                    sampled_at.saturating_duration_since(h3.observed_at) > METRIC_STALE_AFTER;
                let interval = if stale {
                    MetricValue::stale(self.last_interval_loss)
                } else {
                    self.interval_loss(
                        connection_id,
                        h3.sample.sent_packets,
                        h3.sample.lost_packets,
                    )
                };
                (
                    RttQuality {
                        latest: observed_metric(h3.sample.rtt, stale),
                        smoothed: observed_metric(h3.sample.rtt, stale),
                        minimum: h3
                            .sample
                            .min_rtt
                            .map_or_else(MetricValue::not_ready, |value| {
                                observed_metric(value, stale)
                            }),
                        variance: observed_metric(h3.sample.rtt_variance, stale),
                    },
                    LossQuality {
                        interval_basis_points: interval,
                        sent_packets: observed_metric(h3.sample.sent_packets, stale),
                        received_packets: observed_metric(h3.sample.received_packets, stale),
                        lost_packets: observed_metric(h3.sample.lost_packets, stale),
                        sent_bytes: observed_metric(h3.sample.sent_bytes, stale),
                        received_bytes: observed_metric(h3.sample.received_bytes, stale),
                        lost_bytes: observed_metric(h3.sample.lost_bytes, stale),
                        pto_count: observed_metric(h3.sample.pto_count, stale),
                        datagrams_sent: observed_metric(h3.sample.datagrams_sent, stale),
                        datagrams_received: observed_metric(h3.sample.datagrams_received, stale),
                        datagrams_lost: observed_metric(h3.sample.datagrams_lost, stale),
                        datagram_receive_drops: observed_metric(
                            h3.sample.datagram_receive_drops,
                            stale,
                        ),
                    },
                    CongestionQuality {
                        congestion_window_bytes: observed_metric(
                            h3.sample.congestion_window_bytes,
                            stale,
                        ),
                        // quiche 0.29.3 doesn't expose current bytes in flight
                        // through its public PathStats contract.
                        bytes_in_flight: MetricValue::unsupported(),
                        send_rate_bps: observed_metric(
                            h3.sample.send_rate_bytes_per_second.saturating_mul(8),
                            stale,
                        ),
                    },
                )
            }
            (Some(_), Some(Transport::Http2)) => {
                if state.h2.ping_supported == Some(false) {
                    return (
                        rtt_unsupported(),
                        loss_unsupported(),
                        congestion_unsupported(),
                    );
                }
                let stale = state.h2.rtt_stale;
                let h2_metric = |value: Option<Duration>| match (value, stale) {
                    (Some(value), true) => MetricValue::stale(Some(value)),
                    (Some(value), false) => MetricValue::available(value),
                    (None, _) => MetricValue::not_ready(),
                };
                (
                    RttQuality {
                        latest: h2_metric(state.h2.latest_rtt),
                        smoothed: h2_metric(state.h2.smoothed_rtt),
                        minimum: h2_metric(state.h2.minimum_rtt),
                        variance: h2_metric(state.h2.variance),
                    },
                    loss_unsupported(),
                    congestion_unsupported(),
                )
            }
            _ => (rtt_not_ready(), loss_not_ready(), congestion_not_ready()),
        }
    }

    fn interval_loss(
        &mut self,
        connection_id: ConnectionInstanceId,
        sent: u64,
        lost: u64,
    ) -> MetricValue<u32> {
        let previous = self.previous_h3.replace(H3CounterBaseline {
            connection_id,
            sent,
            lost,
        });
        let Some(previous) = previous else {
            return MetricValue::not_ready();
        };
        if previous.connection_id != connection_id
            || sent < previous.sent
            || lost < previous.lost
            || sent == previous.sent
        {
            self.last_interval_loss = None;
            return MetricValue::not_ready();
        }
        let delta_sent = sent - previous.sent;
        let delta_lost = lost - previous.lost;
        let basis_points = u32::try_from(delta_lost.saturating_mul(10_000) / delta_sent.max(1))
            .unwrap_or(u32::MAX);
        self.last_interval_loss = Some(basis_points);
        MetricValue::available(basis_points)
    }

    fn classify(&mut self, snapshot: &NetworkQualitySnapshot) -> NetworkQualityLevel {
        let Some(_) = snapshot.connection_id else {
            self.history.clear();
            return NetworkQualityLevel::Disconnected;
        };
        let queue_drops = queue_drop_total(&snapshot.queues);
        let queue_percent = snapshot
            .queues
            .iter()
            .filter(|queue| {
                queue.availability == MetricAvailability::Available && queue.item_capacity != 0
            })
            .map(|queue| {
                let item_percent = queue.current_items.saturating_mul(100) / queue.item_capacity;
                let byte_percent = queue
                    .current_bytes
                    .saturating_mul(100)
                    .checked_div(queue.byte_capacity)
                    .unwrap_or(0);
                item_percent.max(byte_percent)
            })
            .max();
        let signal = QualitySignal {
            rtt_ms: available_value(&snapshot.rtt.smoothed).map(Duration::as_millis),
            loss_basis_points: available_value(&snapshot.loss.interval_basis_points).copied(),
            queue_percent,
            queue_drop: queue_drops > self.previous_queue_drops,
            pmtu_degraded: snapshot.pmtu.phase == PmtuPhase::Degraded,
            migration_failed: snapshot.migration.failures > self.previous_migration_failures,
        };
        self.previous_queue_drops = queue_drops;
        self.previous_migration_failures = snapshot.migration.failures;
        if self.history.len() == QUALITY_HISTORY_SAMPLES {
            self.history.pop_front();
        }
        self.history.push_back(signal);
        if snapshot.transport == Some(Transport::Http2)
            && snapshot.h2_flow_control.ping_consecutive_failures >= 3
        {
            return NetworkQualityLevel::Poor;
        }
        if self.history.len() < QUALITY_MINIMUM_SAMPLES {
            return NetworkQualityLevel::LimitedData;
        }

        let latest = *self.history.back().expect("quality history is non-empty");
        let Some(rtt_ms) = latest.rtt_ms else {
            return NetworkQualityLevel::LimitedData;
        };
        let loss = match snapshot.transport {
            Some(Transport::Http3) => {
                let Some(loss) = latest.loss_basis_points else {
                    return NetworkQualityLevel::LimitedData;
                };
                Some(loss)
            }
            Some(Transport::Http2) => None,
            None => return NetworkQualityLevel::Disconnected,
        };
        let queue = latest.queue_percent.unwrap_or(0);
        let any_drop = self.history.iter().any(|signal| signal.queue_drop);
        let sustained_drop = self
            .history
            .iter()
            .rev()
            .take(2)
            .all(|signal| signal.queue_drop);
        if latest.pmtu_degraded || latest.migration_failed || sustained_drop {
            return NetworkQualityLevel::Poor;
        }
        if rtt_ms < 75 && loss.is_none_or(|value| value < 50) && queue < 50 && !any_drop {
            NetworkQualityLevel::Good
        } else if rtt_ms < 150 && loss.is_none_or(|value| value < 200) && queue < 80 {
            NetworkQualityLevel::Fair
        } else {
            NetworkQualityLevel::Poor
        }
    }
}

fn queue_drop_total(queues: &[QueueQuality]) -> u64 {
    queues
        .iter()
        .map(|queue| queue.drop_items)
        .fold(0_u64, u64::saturating_add)
}

pub fn spawn_network_quality_sampler(
    telemetry: NetworkQualityTelemetry,
    cancellation: CancellationToken,
) -> (watch::Receiver<NetworkQualitySnapshot>, JoinHandle<()>) {
    let mut sampler = NetworkQualitySampler::new(telemetry);
    let initial = sampler.sample();
    let (sender, receiver) = watch::channel(initial);
    let task = tokio::spawn(async move {
        let mut interval = interval_at(Instant::now() + SAMPLE_INTERVAL, SAMPLE_INTERVAL);
        interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                _ = cancellation.cancelled() => break,
                _ = interval.tick() => {
                    sender.send_replace(sampler.sample());
                }
            }
        }
    });
    (receiver, task)
}

fn pmtu_quality(state: &QualityState, now: Instant) -> PmtuQuality {
    let current_bytes = match (state.transport, state.pmtu.current_bytes) {
        (Some(Transport::Http2), _) => MetricValue::unsupported(),
        (Some(Transport::Http3), Some(value)) => MetricValue::available(value),
        (Some(Transport::Http3), None) => MetricValue::not_ready(),
        _ => MetricValue::not_ready(),
    };
    let effective_connect_ip_payload_bytes = match (
        state.transport,
        state.pmtu.effective_connect_ip_payload_bytes,
    ) {
        (Some(Transport::Http2), _) => MetricValue::unsupported(),
        (Some(Transport::Http3), Some(value)) => MetricValue::available(value),
        (Some(Transport::Http3), None) => MetricValue::not_ready(),
        _ => MetricValue::not_ready(),
    };
    PmtuQuality {
        phase: state.pmtu.phase,
        current_bytes,
        effective_connect_ip_payload_bytes,
        change_count: state.pmtu.change_count,
        revalidation_failure_count: state.pmtu.revalidation_failure_count,
        send_too_large_count: state.pmtu.send_too_large_count,
        last_change_age: state
            .pmtu
            .last_change
            .map_or_else(MetricValue::not_ready, |changed| {
                MetricValue::available(now.saturating_duration_since(changed))
            }),
    }
}

fn migration_quality(state: &QualityState) -> MigrationQuality {
    MigrationQuality {
        phase: state.migration.phase,
        attempts: state.migration.attempts,
        successes: state.migration.successes,
        failures: state.migration.failures,
        last_duration: state
            .migration
            .last_duration
            .map_or_else(MetricValue::not_ready, MetricValue::available),
        last_reason: state.migration.last_reason,
    }
}

fn direct_dns_quality(state: &QualityState) -> DirectDnsQuality {
    DirectDnsQuality {
        mode: state.direct_dns.mode,
        phase: state.direct_dns.phase,
        successes: state.direct_dns.successes,
        failures: state.direct_dns.failures,
        timeouts: state.direct_dns.timeouts,
        last_rtt: state
            .direct_dns
            .last_rtt
            .map_or_else(MetricValue::not_ready, MetricValue::available),
        last_reason: state.direct_dns.last_reason,
    }
}

fn rtt_not_ready() -> RttQuality {
    RttQuality {
        latest: MetricValue::not_ready(),
        smoothed: MetricValue::not_ready(),
        minimum: MetricValue::not_ready(),
        variance: MetricValue::not_ready(),
    }
}

fn rtt_unsupported() -> RttQuality {
    RttQuality {
        latest: MetricValue::unsupported(),
        smoothed: MetricValue::unsupported(),
        minimum: MetricValue::unsupported(),
        variance: MetricValue::unsupported(),
    }
}

fn loss_not_ready() -> LossQuality {
    LossQuality {
        interval_basis_points: MetricValue::not_ready(),
        sent_packets: MetricValue::not_ready(),
        received_packets: MetricValue::not_ready(),
        lost_packets: MetricValue::not_ready(),
        sent_bytes: MetricValue::not_ready(),
        received_bytes: MetricValue::not_ready(),
        lost_bytes: MetricValue::not_ready(),
        pto_count: MetricValue::not_ready(),
        datagrams_sent: MetricValue::not_ready(),
        datagrams_received: MetricValue::not_ready(),
        datagrams_lost: MetricValue::not_ready(),
        datagram_receive_drops: MetricValue::not_ready(),
    }
}

fn loss_unsupported() -> LossQuality {
    LossQuality {
        interval_basis_points: MetricValue::unsupported(),
        sent_packets: MetricValue::unsupported(),
        received_packets: MetricValue::unsupported(),
        lost_packets: MetricValue::unsupported(),
        sent_bytes: MetricValue::unsupported(),
        received_bytes: MetricValue::unsupported(),
        lost_bytes: MetricValue::unsupported(),
        pto_count: MetricValue::unsupported(),
        datagrams_sent: MetricValue::unsupported(),
        datagrams_received: MetricValue::unsupported(),
        datagrams_lost: MetricValue::unsupported(),
        datagram_receive_drops: MetricValue::unsupported(),
    }
}

fn congestion_not_ready() -> CongestionQuality {
    CongestionQuality {
        congestion_window_bytes: MetricValue::not_ready(),
        bytes_in_flight: MetricValue::not_ready(),
        send_rate_bps: MetricValue::not_ready(),
    }
}

fn congestion_unsupported() -> CongestionQuality {
    CongestionQuality {
        congestion_window_bytes: MetricValue::unsupported(),
        bytes_in_flight: MetricValue::unsupported(),
        send_rate_bps: MetricValue::unsupported(),
    }
}

fn queue_index(kind: QueueKind) -> usize {
    match kind {
        QueueKind::TunToTransport => 0,
        QueueKind::ProxyToTransport => 1,
        QueueKind::TransportOutgoingPackets => 2,
        QueueKind::H3DatagramSend => 3,
        QueueKind::H3WireSend => 4,
        QueueKind::TransportToTun => 5,
        QueueKind::TransportToProxy => 6,
        QueueKind::DirectDnsRequests => 7,
    }
}

fn available_value<T>(value: &MetricValue<T>) -> Option<&T> {
    (value.availability == MetricAvailability::Available)
        .then_some(value.value.as_ref())
        .flatten()
}

fn observed_metric<T>(value: T, stale: bool) -> MetricValue<T> {
    if stale {
        MetricValue::stale(Some(value))
    } else {
        MetricValue::available(value)
    }
}

fn saturating_add(counter: &AtomicU64, value: u64) {
    let _ = counter.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
        Some(current.saturating_add(value))
    });
}

#[cfg(test)]
mod tests {
    use tokio::time::advance;

    use super::*;

    fn h3_sample(sent: u64, lost: u64) -> H3MetricsSample {
        H3MetricsSample {
            rtt: Duration::from_millis(20),
            min_rtt: Some(Duration::from_millis(15)),
            rtt_variance: Duration::from_millis(2),
            congestion_window_bytes: 64 * 1024,
            send_rate_bytes_per_second: 1_000_000,
            sent_packets: sent,
            received_packets: sent.saturating_sub(lost),
            lost_packets: lost,
            sent_bytes: sent.saturating_mul(1_000),
            received_bytes: sent.saturating_sub(lost).saturating_mul(1_000),
            lost_bytes: lost.saturating_mul(1_000),
            pto_count: 0,
            datagrams_sent: sent,
            datagrams_received: sent.saturating_sub(lost),
            datagrams_lost: lost,
            datagram_receive_drops: 0,
        }
    }

    #[test]
    fn counter_reset_never_creates_a_huge_interval_loss() {
        let telemetry = NetworkQualityTelemetry::default();
        telemetry.begin_connection(Transport::Http3, AddressFamily::Ipv4);
        let mut sampler = NetworkQualitySampler::new(telemetry.clone());
        telemetry.observe_h3(h3_sample(100, 2));
        assert_eq!(
            sampler.sample().loss.interval_basis_points.availability,
            MetricAvailability::NotReady
        );
        telemetry.observe_h3(h3_sample(200, 3));
        assert_eq!(
            sampler.sample().loss.interval_basis_points,
            MetricValue::available(100)
        );
        telemetry.observe_h3(h3_sample(1, 0));
        assert_eq!(
            sampler.sample().loss.interval_basis_points.availability,
            MetricAvailability::NotReady
        );
    }

    #[tokio::test(start_paused = true)]
    async fn stale_metrics_are_explicit_and_keep_the_last_value() {
        let telemetry = NetworkQualityTelemetry::default();
        telemetry.begin_connection(Transport::Http3, AddressFamily::Ipv6);
        telemetry.observe_h3(h3_sample(10, 0));
        let mut sampler = NetworkQualitySampler::new(telemetry);
        let first = sampler.sample();
        assert_eq!(
            first.rtt.smoothed.availability,
            MetricAvailability::Available
        );
        advance(Duration::from_secs(4)).await;
        let stale = sampler.sample();
        assert_eq!(stale.rtt.smoothed.availability, MetricAvailability::Stale);
        assert_eq!(stale.rtt.smoothed.value, Some(Duration::from_millis(20)));
    }

    #[test]
    fn connection_change_resets_interval_and_short_history() {
        let telemetry = NetworkQualityTelemetry::default();
        let first = telemetry.begin_connection(Transport::Http3, AddressFamily::Ipv4);
        let mut sampler = NetworkQualitySampler::new(telemetry.clone());
        telemetry.observe_h3(h3_sample(10, 0));
        sampler.sample();
        telemetry.observe_h3(h3_sample(20, 1));
        assert_eq!(
            sampler.sample().loss.interval_basis_points.availability,
            MetricAvailability::Available
        );
        let second = telemetry.begin_connection(Transport::Http3, AddressFamily::Ipv4);
        assert_ne!(first, second);
        assert_eq!(first.0.get_version_num(), 4);
        assert_eq!(second.0.get_version_num(), 4);
        telemetry.observe_h3(h3_sample(1, 0));
        let snapshot = sampler.sample();
        assert_eq!(snapshot.connection_id, Some(second));
        assert_eq!(snapshot.level, NetworkQualityLevel::LimitedData);
        assert_eq!(
            snapshot.loss.interval_basis_points.availability,
            MetricAvailability::NotReady
        );
    }

    #[test]
    fn five_healthy_h3_samples_classify_as_good() {
        let telemetry = NetworkQualityTelemetry::default();
        telemetry.begin_connection(Transport::Http3, AddressFamily::Ipv4);
        let mut sampler = NetworkQualitySampler::new(telemetry.clone());
        let mut level = NetworkQualityLevel::LimitedData;
        for index in 1..=6 {
            telemetry.observe_h3(h3_sample(index * 100, 0));
            level = sampler.sample().level;
        }
        assert_eq!(level, NetworkQualityLevel::Good);
    }

    #[test]
    fn historical_queue_drops_are_baselined_for_a_new_connection() {
        let telemetry = NetworkQualityTelemetry::default();
        let queue = telemetry.register_queue(QueueKind::TransportOutgoingPackets, 8, 800);
        queue.record_rejected(100);
        telemetry.begin_connection(Transport::Http3, AddressFamily::Ipv4);
        let mut sampler = NetworkQualitySampler::new(telemetry.clone());

        let mut level = NetworkQualityLevel::LimitedData;
        for index in 1..=6 {
            telemetry.observe_h3(h3_sample(index * 100, 0));
            level = sampler.sample().level;
        }

        assert_eq!(level, NetworkQualityLevel::Good);
    }

    #[test]
    fn quality_classification_includes_queue_byte_occupancy() {
        let telemetry = NetworkQualityTelemetry::default();
        let queue = telemetry.register_queue(QueueKind::TransportOutgoingPackets, 100, 100);
        let _entry = queue.start_entry(90);
        telemetry.begin_connection(Transport::Http3, AddressFamily::Ipv4);
        let mut sampler = NetworkQualitySampler::new(telemetry.clone());

        let mut level = NetworkQualityLevel::LimitedData;
        for index in 1..=6 {
            telemetry.observe_h3(h3_sample(index * 100, 0));
            level = sampler.sample().level;
        }

        assert_eq!(level, NetworkQualityLevel::Poor);
    }

    #[test]
    fn h2_marks_quic_only_metrics_as_unsupported() {
        let telemetry = NetworkQualityTelemetry::default();
        telemetry.begin_connection(Transport::Http2, AddressFamily::Ipv6);
        let mut sampler = NetworkQualitySampler::new(telemetry);
        let snapshot = sampler.sample();

        assert_eq!(
            snapshot.loss.interval_basis_points.availability,
            MetricAvailability::Unsupported
        );
        assert_eq!(
            snapshot.congestion.congestion_window_bytes.availability,
            MetricAvailability::Unsupported
        );
        assert_eq!(
            snapshot.pmtu.current_bytes.availability,
            MetricAvailability::Unsupported
        );
        assert_eq!(
            snapshot
                .pmtu
                .effective_connect_ip_payload_bytes
                .availability,
            MetricAvailability::Unsupported
        );
        assert_eq!(
            snapshot.rtt.smoothed.availability,
            MetricAvailability::NotReady
        );
        assert!(
            snapshot
                .queues
                .iter()
                .filter(|queue| matches!(
                    queue.kind,
                    QueueKind::H3DatagramSend | QueueKind::H3WireSend
                ))
                .all(|queue| {
                    queue.availability == MetricAvailability::Unsupported
                        && queue.oldest_age.availability == MetricAvailability::Unsupported
                })
        );
    }

    #[test]
    fn h3_pmtu_is_not_ready_until_discovery_publishes_a_value() {
        let telemetry = NetworkQualityTelemetry::default();
        telemetry.begin_connection(Transport::Http3, AddressFamily::Ipv4);
        let mut sampler = NetworkQualitySampler::new(telemetry.clone());

        telemetry.observe_pmtu(PmtuPhase::Probing, None, None);
        let probing = sampler.sample();
        assert_eq!(probing.pmtu.phase, PmtuPhase::Probing);
        assert_eq!(
            probing.pmtu.current_bytes.availability,
            MetricAvailability::NotReady
        );
        assert_eq!(
            probing.pmtu.effective_connect_ip_payload_bytes.availability,
            MetricAvailability::NotReady
        );

        telemetry.observe_pmtu(PmtuPhase::Stable, Some(1_472), Some(1_400));
        telemetry.record_pmtu_send_too_large();
        let stable = sampler.sample();
        assert_eq!(stable.pmtu.current_bytes.value, Some(1_472));
        assert_eq!(
            stable.pmtu.effective_connect_ip_payload_bytes.value,
            Some(1_400)
        );
        assert_eq!(stable.pmtu.send_too_large_count, 1);
    }

    #[test]
    fn h2_ping_capability_and_failures_have_explicit_quality_states() {
        let unsupported = NetworkQualityTelemetry::default();
        unsupported.begin_connection(Transport::Http2, AddressFamily::Ipv4);
        unsupported.configure_h2_connection(4 * 1024 * 1024, 8 * 1024 * 1024, false);
        let snapshot = NetworkQualitySampler::new(unsupported).sample();
        assert_eq!(
            snapshot.rtt.smoothed.availability,
            MetricAvailability::Unsupported
        );
        assert_eq!(
            snapshot.h2_flow_control.stream_receive_window_bytes,
            4 * 1024 * 1024
        );
        assert_eq!(
            snapshot.h2_flow_control.connection_receive_window_bytes,
            8 * 1024 * 1024
        );

        let telemetry = NetworkQualityTelemetry::default();
        telemetry.begin_connection(Transport::Http2, AddressFamily::Ipv4);
        telemetry.configure_h2_connection(4 * 1024 * 1024, 8 * 1024 * 1024, true);
        telemetry.observe_h2_rtt(
            Duration::from_millis(20),
            Duration::from_millis(20),
            Duration::from_millis(20),
            Duration::ZERO,
        );
        telemetry.record_h2_ping_timeout();
        telemetry.record_h2_ping_timeout();
        telemetry.record_h2_ping_timeout();
        let snapshot = NetworkQualitySampler::new(telemetry).sample();
        assert_eq!(
            snapshot.rtt.smoothed.availability,
            MetricAvailability::Stale
        );
        assert_eq!(snapshot.rtt.smoothed.value, Some(Duration::from_millis(20)));
        assert_eq!(snapshot.h2_flow_control.ping_timeout_count, 3);
        assert_eq!(snapshot.h2_flow_control.ping_consecutive_failures, 3);
        assert_eq!(snapshot.level, NetworkQualityLevel::Poor);
    }

    #[tokio::test(start_paused = true)]
    async fn one_hertz_sampler_stops_on_cancellation() {
        let telemetry = NetworkQualityTelemetry::default();
        let cancellation = CancellationToken::new();
        let (mut receiver, task) = spawn_network_quality_sampler(telemetry, cancellation.clone());
        advance(SAMPLE_INTERVAL).await;
        receiver.changed().await.unwrap();
        cancellation.cancel();
        advance(SAMPLE_INTERVAL * 2).await;
        task.await.unwrap();
        assert!(receiver.changed().await.is_err());
    }

    #[test]
    fn snapshot_type_has_no_endpoint_or_free_form_text_fields() {
        let telemetry = NetworkQualityTelemetry::default();
        let mut sampler = NetworkQualitySampler::new(telemetry);
        let debug = format!("{:?}", sampler.sample());
        for forbidden in ["127.0.0.1", "example.com", "ssid", "token="] {
            assert!(!debug.contains(forbidden));
        }
    }

    #[test]
    fn direct_dns_and_all_queue_kinds_are_numeric_and_bounded() {
        let telemetry = NetworkQualityTelemetry::default();
        for kind in ALL_QUEUE_KINDS {
            let metrics = telemetry.register_queue(kind, 2, 128);
            let entry = metrics.start_entry(16);
            entry.complete();
        }
        telemetry.set_direct_dns_mode(DirectDnsMode::PhysicalSystem);
        telemetry.record_direct_dns_success(Duration::from_millis(12));
        telemetry.record_direct_dns_failure(DirectDnsReasonCode::Timeout, true);

        let mut sampler = NetworkQualitySampler::new(telemetry);
        let snapshot = sampler.sample();
        assert_eq!(snapshot.queues.len(), ALL_QUEUE_KINDS.len());
        assert!(snapshot.queues.iter().all(|queue| {
            queue.availability == MetricAvailability::Available
                && queue.enqueue_count == 1
                && queue.dequeue_count == 1
                && queue.current_items == 0
                && queue.current_bytes == 0
        }));
        assert_eq!(snapshot.direct_dns.successes, 1);
        assert_eq!(snapshot.direct_dns.failures, 1);
        assert_eq!(snapshot.direct_dns.timeouts, 1);
        assert_eq!(
            snapshot.direct_dns.last_rtt.value,
            Some(Duration::from_millis(12))
        );
        assert_eq!(
            snapshot.direct_dns.last_reason,
            Some(DirectDnsReasonCode::Timeout)
        );
    }

    #[test]
    fn one_million_hot_counter_updates_are_exact() {
        let telemetry = NetworkQualityTelemetry::default();
        for _ in 0..1_000_000 {
            telemetry.record_udp_send(1);
        }
        let snapshot = telemetry.udp_snapshot();
        assert_eq!(snapshot.send_syscalls, 1_000_000);
        assert_eq!(snapshot.sent_datagrams, 1_000_000);
    }
}
