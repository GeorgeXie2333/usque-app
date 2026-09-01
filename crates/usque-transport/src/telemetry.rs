use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant, SystemTime};

use usque_core::{
    AddressFamily, Transport, TransportFailure, TransportFailureCode, TransportStage,
};

use crate::network_quality::{H3MetricsSample, NetworkQualityTelemetry};

pub const CONNECTION_TIMELINE_CAPACITY: usize = 512;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionEventType {
    AttemptStarted,
    EndpointResolved,
    SocketConnected,
    TlsReady,
    QuicReady,
    MasqueAccepted,
    PeerSettingsReceived,
    AddressAssigned,
    TunnelReady,
    FirstPacketSent,
    FirstPacketReceived,
    FallbackStarted,
    ReconnectScheduled,
    NetworkChanged,
    RecoveryProbeStarted,
    RecoveryProbeSucceeded,
    RecoveryProbeFailed,
    PathPromoted,
    QueueSaturated,
    Disconnected,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectionEvent {
    pub sequence: u64,
    pub timestamp: SystemTime,
    pub elapsed_from_attempt_start: Duration,
    pub event_type: ConnectionEventType,
    pub stage: Option<TransportStage>,
    pub transport: Option<Transport>,
    pub address_family: Option<AddressFamily>,
    pub duration: Option<Duration>,
    pub failure: Option<TransportFailure>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ConnectionEventPath {
    pub transport: Option<Transport>,
    pub address_family: Option<AddressFamily>,
}

impl ConnectionEventPath {
    pub const fn new(transport: Option<Transport>, address_family: Option<AddressFamily>) -> Self {
        Self {
            transport,
            address_family,
        }
    }

    pub const fn known(transport: Transport, address_family: AddressFamily) -> Self {
        Self::new(Some(transport), Some(address_family))
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConnectionMetrics {
    pub last_connect_duration: Option<Duration>,
    pub last_h3_handshake_duration: Option<Duration>,
    pub last_h2_handshake_duration: Option<Duration>,
    /// Remains `None` unless the underlying transport provides a real RTT.
    pub current_smoothed_rtt: Option<Duration>,
    pub reconnect_count: u32,
    pub fallback_count: u32,
    pub network_change_count: u32,
    pub send_queue_high_watermark: u64,
    pub send_queue_drop_count: u64,
    pub last_failure_code: Option<TransportFailureCode>,
    pub last_reconnect_code: Option<TransportFailureCode>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConnectionTimelineSnapshot {
    pub events: Vec<ConnectionEvent>,
    pub metrics: ConnectionMetrics,
    pub dropped_event_count: u64,
}

#[derive(Clone)]
pub struct ConnectionTelemetry {
    inner: Arc<Mutex<TelemetryState>>,
    sequence: Arc<AtomicU64>,
    first_packet_sent: Arc<AtomicBool>,
    first_packet_received: Arc<AtomicBool>,
    send_queue_high_watermark: Arc<AtomicU64>,
    send_queue_drop_count: Arc<AtomicU64>,
    send_queue_wait_count: Arc<AtomicU64>,
    quality: NetworkQualityTelemetry,
}

/// Carries the path identity for one in-flight transport attempt so lower
/// layers can publish successful stage transitions without learning about the
/// Engine or retaining endpoint details.
#[derive(Clone)]
pub(crate) struct ConnectionAttemptTelemetry {
    telemetry: ConnectionTelemetry,
    transport: Transport,
    family: AddressFamily,
}

impl ConnectionAttemptTelemetry {
    pub(crate) fn new(
        telemetry: ConnectionTelemetry,
        transport: Transport,
        family: AddressFamily,
    ) -> Self {
        Self {
            telemetry,
            transport,
            family,
        }
    }

    pub(crate) fn record(&self, event_type: ConnectionEventType, stage: TransportStage) {
        self.telemetry.record(
            event_type,
            Some(stage),
            ConnectionEventPath::known(self.transport, self.family),
            None,
            None,
        );
    }

    pub(crate) fn quality(&self) -> NetworkQualityTelemetry {
        self.telemetry.network_quality()
    }

    pub(crate) fn observe_h3(&self, sample: H3MetricsSample) {
        self.telemetry.observe_h3(sample);
    }

    pub(crate) fn observe_h2_rtt(
        &self,
        latest: Duration,
        smoothed: Duration,
        minimum: Duration,
        variance: Duration,
    ) {
        self.telemetry
            .observe_h2_rtt(latest, smoothed, minimum, variance);
    }
}

struct TelemetryState {
    capacity: usize,
    attempt_started: Instant,
    events: VecDeque<ConnectionEvent>,
    metrics: ConnectionMetrics,
    dropped_event_count: u64,
}

impl Default for ConnectionTelemetry {
    fn default() -> Self {
        Self::new(CONNECTION_TIMELINE_CAPACITY)
    }
}

impl ConnectionTelemetry {
    pub fn new(capacity: usize) -> Self {
        assert!(
            capacity > 0,
            "connection timeline capacity must be non-zero"
        );
        Self {
            inner: Arc::new(Mutex::new(TelemetryState {
                capacity,
                attempt_started: Instant::now(),
                events: VecDeque::with_capacity(capacity),
                metrics: ConnectionMetrics::default(),
                dropped_event_count: 0,
            })),
            sequence: Arc::new(AtomicU64::new(0)),
            first_packet_sent: Arc::new(AtomicBool::new(false)),
            first_packet_received: Arc::new(AtomicBool::new(false)),
            send_queue_high_watermark: Arc::new(AtomicU64::new(0)),
            send_queue_drop_count: Arc::new(AtomicU64::new(0)),
            send_queue_wait_count: Arc::new(AtomicU64::new(0)),
            quality: NetworkQualityTelemetry::default(),
        }
    }

    pub fn reset_attempt(&self) {
        let mut state = self.state();
        state.attempt_started = Instant::now();
        self.first_packet_sent.store(false, Ordering::Release);
        self.first_packet_received.store(false, Ordering::Release);
    }

    pub fn record(
        &self,
        event_type: ConnectionEventType,
        stage: Option<TransportStage>,
        path: ConnectionEventPath,
        duration: Option<Duration>,
        failure: Option<TransportFailure>,
    ) {
        if matches!(
            event_type,
            ConnectionEventType::Disconnected | ConnectionEventType::Failed
        ) {
            self.quality.end_connection();
        }
        let mut state = self.state();
        let event = ConnectionEvent {
            sequence: self.sequence.fetch_add(1, Ordering::Relaxed) + 1,
            timestamp: SystemTime::now(),
            elapsed_from_attempt_start: state.attempt_started.elapsed(),
            event_type,
            stage,
            transport: path.transport,
            address_family: path.address_family,
            duration,
            failure: failure.clone(),
        };
        if state.events.len() == state.capacity {
            state.events.pop_front();
            state.dropped_event_count = state.dropped_event_count.saturating_add(1);
        }
        if let Some(failure) = failure {
            state.metrics.last_failure_code = Some(failure.code);
        }
        state.events.push_back(event);
    }

    pub fn record_attempt(&self, transport: Transport, family: AddressFamily) {
        self.record(
            ConnectionEventType::AttemptStarted,
            Some(TransportStage::EndpointResolution),
            ConnectionEventPath::known(transport, family),
            None,
            None,
        );
    }

    pub fn record_tunnel_ready(
        &self,
        transport: Transport,
        family: AddressFamily,
        duration: Duration,
    ) {
        self.quality.begin_connection(transport, family);
        {
            let mut state = self.state();
            state.metrics.last_connect_duration = Some(duration);
            match transport {
                Transport::Http3 => state.metrics.last_h3_handshake_duration = Some(duration),
                Transport::Http2 => state.metrics.last_h2_handshake_duration = Some(duration),
            }
        }
        self.record(
            ConnectionEventType::TunnelReady,
            Some(TransportStage::TunnelStartup),
            ConnectionEventPath::known(transport, family),
            Some(duration),
            None,
        );
    }

    pub fn record_first_packet_sent(&self, transport: Transport, family: AddressFamily) {
        if !self.first_packet_sent.load(Ordering::Acquire)
            && self
                .first_packet_sent
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
        {
            self.record(
                ConnectionEventType::FirstPacketSent,
                Some(TransportStage::PacketSend),
                ConnectionEventPath::known(transport, family),
                None,
                None,
            );
        }
    }

    pub fn record_first_packet_received(&self, transport: Transport, family: AddressFamily) {
        if !self.first_packet_received.load(Ordering::Acquire)
            && self
                .first_packet_received
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
        {
            self.record(
                ConnectionEventType::FirstPacketReceived,
                Some(TransportStage::PacketReceive),
                ConnectionEventPath::known(transport, family),
                None,
                None,
            );
        }
    }

    pub fn observe_queue_depth(&self, depth: usize) {
        self.send_queue_high_watermark
            .fetch_max(depth as u64, Ordering::Relaxed);
    }

    pub fn record_queue_drop(&self) {
        let _ = self.send_queue_drop_count.fetch_update(
            Ordering::Relaxed,
            Ordering::Relaxed,
            |count| Some(count.saturating_add(1)),
        );
        let failure = TransportFailure::new(
            TransportFailureCode::SendQueueFull,
            TransportStage::PacketSend,
        );
        self.record(
            ConnectionEventType::QueueSaturated,
            Some(TransportStage::PacketSend),
            ConnectionEventPath::default(),
            None,
            Some(failure),
        );
    }

    pub fn record_queue_saturated(&self, queue_depth: usize) {
        let wait_count = self
            .send_queue_wait_count
            .fetch_add(1, Ordering::Relaxed)
            .saturating_add(1);
        if !wait_count.is_power_of_two() {
            return;
        }
        let failure = TransportFailure::new(
            TransportFailureCode::SendQueueFull,
            TransportStage::PacketSend,
        )
        .with_sanitized_detail(format!("queue depth {queue_depth}"));
        self.record(
            ConnectionEventType::QueueSaturated,
            Some(TransportStage::PacketSend),
            ConnectionEventPath::default(),
            None,
            Some(failure),
        );
    }

    pub fn set_reconnect(&self, count: u32, failure: &TransportFailure) {
        self.quality.end_connection();
        let mut state = self.state();
        state.metrics.reconnect_count = count;
        state.metrics.last_reconnect_code = Some(failure.code);
    }

    pub fn increment_fallback(&self) {
        let mut state = self.state();
        state.metrics.fallback_count = state.metrics.fallback_count.saturating_add(1);
    }

    pub fn increment_network_change(&self) {
        let mut state = self.state();
        state.metrics.network_change_count = state.metrics.network_change_count.saturating_add(1);
    }

    pub fn snapshot(&self) -> ConnectionTimelineSnapshot {
        let state = self.state();
        let mut metrics = state.metrics.clone();
        metrics.send_queue_high_watermark = self.send_queue_high_watermark.load(Ordering::Relaxed);
        metrics.send_queue_drop_count = self.send_queue_drop_count.load(Ordering::Relaxed);
        ConnectionTimelineSnapshot {
            events: state.events.iter().cloned().collect(),
            metrics,
            dropped_event_count: state.dropped_event_count,
        }
    }

    pub fn network_quality(&self) -> NetworkQualityTelemetry {
        self.quality.clone()
    }

    fn observe_h3(&self, sample: H3MetricsSample) {
        self.state().metrics.current_smoothed_rtt = Some(sample.rtt);
        self.quality.observe_h3(sample);
    }

    fn observe_h2_rtt(
        &self,
        latest: Duration,
        smoothed: Duration,
        minimum: Duration,
        variance: Duration,
    ) {
        self.state().metrics.current_smoothed_rtt = Some(smoothed);
        self.quality
            .observe_h2_rtt(latest, smoothed, minimum, variance);
    }

    fn state(&self) -> MutexGuard<'_, TelemetryState> {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inv_bounded_work_timeline_evicts_oldest_records() {
        let telemetry = ConnectionTelemetry::new(2);
        for _ in 0..3 {
            telemetry.record(
                ConnectionEventType::AttemptStarted,
                None,
                ConnectionEventPath::default(),
                None,
                None,
            );
        }
        let snapshot = telemetry.snapshot();
        assert_eq!(snapshot.events.len(), 2);
        assert_eq!(snapshot.dropped_event_count, 1);
        assert_eq!(snapshot.events[0].sequence, 2);
        assert_eq!(snapshot.events[1].sequence, 3);
    }

    #[test]
    fn inv_single_active_tunnel_records_only_first_packet_markers_per_attempt() {
        let telemetry = ConnectionTelemetry::new(8);
        telemetry.record_first_packet_sent(Transport::Http2, AddressFamily::Ipv4);
        telemetry.record_first_packet_sent(Transport::Http2, AddressFamily::Ipv4);
        telemetry.record_first_packet_received(Transport::Http2, AddressFamily::Ipv4);
        telemetry.record_first_packet_received(Transport::Http2, AddressFamily::Ipv4);
        let snapshot = telemetry.snapshot();
        assert_eq!(snapshot.events.len(), 2);
    }

    #[test]
    fn queue_metrics_use_atomic_high_water_and_saturating_counts() {
        let telemetry = ConnectionTelemetry::new(8);
        telemetry.observe_queue_depth(7);
        telemetry.observe_queue_depth(3);
        telemetry.record_queue_drop();
        telemetry.record_queue_drop();
        telemetry.record_queue_saturated(1_024);
        telemetry.record_queue_saturated(1_024);
        telemetry.record_queue_saturated(1_024);

        let snapshot = telemetry.snapshot();
        assert_eq!(snapshot.metrics.send_queue_high_watermark, 7);
        assert_eq!(snapshot.metrics.send_queue_drop_count, 2);
        assert_eq!(
            snapshot
                .events
                .iter()
                .filter(|event| event.event_type == ConnectionEventType::QueueSaturated)
                .count(),
            4
        );
        let saturation_details = snapshot
            .events
            .iter()
            .filter(|event| event.event_type == ConnectionEventType::QueueSaturated)
            .filter_map(|event| event.failure.as_ref())
            .filter_map(|failure| failure.sanitized_detail.as_deref())
            .collect::<Vec<_>>();
        assert_eq!(
            saturation_details,
            vec!["queue depth 1024", "queue depth 1024"]
        );
    }

    #[test]
    fn concurrent_first_packet_updates_record_one_marker() {
        let telemetry = ConnectionTelemetry::new(32);
        std::thread::scope(|scope| {
            for _ in 0..16 {
                let telemetry = telemetry.clone();
                scope.spawn(move || {
                    telemetry.record_first_packet_sent(Transport::Http3, AddressFamily::Ipv4);
                });
            }
        });

        let snapshot = telemetry.snapshot();
        assert_eq!(
            snapshot
                .events
                .iter()
                .filter(|event| event.event_type == ConnectionEventType::FirstPacketSent)
                .count(),
            1
        );
    }

    #[test]
    fn rtt_remains_unknown_without_transport_measurement() {
        assert!(
            ConnectionTelemetry::default()
                .snapshot()
                .metrics
                .current_smoothed_rtt
                .is_none()
        );
    }

    #[test]
    fn h2_ping_updates_both_timeline_and_network_quality_rtt() {
        let telemetry = ConnectionTelemetry::new(8);
        telemetry.record_tunnel_ready(
            Transport::Http2,
            AddressFamily::Ipv4,
            Duration::from_millis(5),
        );
        telemetry
            .network_quality()
            .configure_h2_connection(4 * 1024 * 1024, 8 * 1024 * 1024, true);
        let attempt = ConnectionAttemptTelemetry::new(
            telemetry.clone(),
            Transport::Http2,
            AddressFamily::Ipv4,
        );
        attempt.observe_h2_rtt(
            Duration::from_millis(20),
            Duration::from_millis(18),
            Duration::from_millis(15),
            Duration::from_millis(2),
        );

        assert_eq!(
            telemetry.snapshot().metrics.current_smoothed_rtt,
            Some(Duration::from_millis(18))
        );
        let quality =
            crate::network_quality::NetworkQualitySampler::new(telemetry.network_quality())
                .sample();
        assert_eq!(quality.rtt.smoothed.value, Some(Duration::from_millis(18)));
    }

    #[test]
    fn successful_stage_observer_preserves_path_and_sequence() {
        let telemetry = ConnectionTelemetry::new(8);
        let attempt = ConnectionAttemptTelemetry::new(
            telemetry.clone(),
            Transport::Http3,
            AddressFamily::Ipv6,
        );
        attempt.record(
            ConnectionEventType::EndpointResolved,
            TransportStage::EndpointResolution,
        );
        attempt.record(
            ConnectionEventType::SocketConnected,
            TransportStage::SocketConnect,
        );

        let snapshot = telemetry.snapshot();
        assert_eq!(snapshot.events.len(), 2);
        assert_eq!(snapshot.events[0].sequence, 1);
        assert_eq!(snapshot.events[1].sequence, 2);
        assert!(snapshot.events.iter().all(|event| {
            event.transport == Some(Transport::Http3)
                && event.address_family == Some(AddressFamily::Ipv6)
                && event.failure.is_none()
        }));
    }
}
