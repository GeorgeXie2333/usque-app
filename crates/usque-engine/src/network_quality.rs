use std::time::{Duration, SystemTime, UNIX_EPOCH};

use usque_ipc::v1;
use usque_transport::{
    DirectDnsMode, DirectDnsPhase, DirectDnsReasonCode, MetricAvailability, MetricValue,
    MigrationPhase, MigrationReasonCode, NetworkQualityLevel, NetworkQualitySampler,
    NetworkQualitySnapshot, NetworkQualityTelemetry, PmtuPhase, QueueKind,
};

pub(crate) fn disconnected_snapshot() -> NetworkQualitySnapshot {
    NetworkQualitySampler::new(NetworkQualityTelemetry::default()).sample()
}

pub(crate) fn snapshot_to_proto(snapshot: &NetworkQualitySnapshot) -> v1::NetworkQualitySnapshot {
    let sampled_at = SystemTime::now()
        .checked_sub(snapshot.sampled_at.elapsed())
        .unwrap_or(UNIX_EPOCH)
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let queues = snapshot
        .queues
        .iter()
        .map(|queue| {
            let (oldest_age_ms, oldest_age_known) = duration_metric(&queue.oldest_age);
            v1::QueueQuality {
                kind: queue_kind(queue.kind) as i32,
                current_items: queue.current_items,
                capacity_items: queue.item_capacity,
                current_bytes: queue.current_bytes,
                capacity_bytes: queue.byte_capacity,
                high_water_items: queue.items_high_water,
                high_water_bytes: queue.bytes_high_water,
                drop_items: queue.drop_items,
                drop_bytes: queue.drop_bytes,
                oldest_age_ms,
                oldest_age_known,
                availability: availability(queue.availability) as i32,
                enqueue_count: queue.enqueue_count,
                dequeue_count: queue.dequeue_count,
                closed: queue.closed,
                cancelled: queue.cancelled,
            }
        })
        .collect::<Vec<_>>();

    let (smoothed_rtt, smoothed_rtt_known) = duration_metric(&snapshot.rtt.smoothed);
    let (minimum_rtt, minimum_rtt_known) = duration_metric(&snapshot.rtt.minimum);
    let (rtt_variance, rtt_variance_known) = duration_metric(&snapshot.rtt.variance);
    let (interval_loss, interval_loss_known) = u32_metric(&snapshot.loss.interval_basis_points);
    let (congestion_window, congestion_window_known) =
        u64_metric(&snapshot.congestion.congestion_window_bytes);
    let (bytes_in_flight, bytes_in_flight_known) = u64_metric(&snapshot.congestion.bytes_in_flight);
    let (send_rate, send_rate_known) = u64_metric(&snapshot.congestion.send_rate_bps);
    let (current_pmtu, current_pmtu_known) = u32_metric(&snapshot.pmtu.current_bytes);
    let (last_migration_duration, last_migration_duration_known) =
        duration_metric(&snapshot.migration.last_duration);
    let (direct_dns_last_rtt, direct_dns_last_rtt_known) =
        duration_metric(&snapshot.direct_dns.last_rtt);
    let queue_oldest_age = snapshot
        .queues
        .iter()
        .filter_map(|queue| available_duration(&queue.oldest_age))
        .max();
    let tun_sink_drop_count = queue_drop_count(snapshot, QueueKind::TransportToTun);
    let quic_datagram_drop_count = queue_drop_count(snapshot, QueueKind::H3DatagramSend)
        .saturating_add(metric_raw_u64(&snapshot.loss.datagram_receive_drops));

    v1::NetworkQualitySnapshot {
        sampled_at_unix_ms: duration_milliseconds(sampled_at),
        connection_instance_id: snapshot
            .connection_id
            .map(|connection| connection.0.to_string())
            .unwrap_or_default(),
        level: quality_level(snapshot.level) as i32,
        metrics: Some(v1::ConnectionMetrics {
            current_smoothed_rtt_milliseconds: smoothed_rtt,
            current_smoothed_rtt_known: smoothed_rtt_known,
            min_rtt_ms: minimum_rtt,
            min_rtt_known: minimum_rtt_known,
            rtt_variance_ms: rtt_variance,
            rtt_variance_known,
            interval_loss_basis_points: interval_loss,
            interval_loss_known,
            congestion_window_bytes: congestion_window,
            congestion_window_known,
            bytes_in_flight,
            bytes_in_flight_known,
            send_rate_bps: send_rate,
            send_rate_known,
            packets_lost: metric_raw_u64(&snapshot.loss.lost_packets),
            bytes_lost: metric_raw_u64(&snapshot.loss.lost_bytes),
            tun_sink_drop_count,
            quic_datagram_drop_count,
            queue_oldest_age_ms: queue_oldest_age.map_or(0, duration_milliseconds),
            queue_oldest_age_known: queue_oldest_age.is_some(),
            current_pmtu_bytes: current_pmtu,
            current_pmtu_known,
            migration_attempt_count: snapshot.migration.attempts,
            migration_success_count: snapshot.migration.successes,
            migration_failure_count: snapshot.migration.failures,
            last_migration_duration_ms: last_migration_duration,
            last_migration_duration_known,
            udp_send_syscall_count: snapshot.udp_io.send_syscalls,
            udp_recv_syscall_count: snapshot.udp_io.recv_syscalls,
            udp_datagram_sent_count: snapshot.udp_io.sent_datagrams,
            udp_datagram_received_count: snapshot.udp_io.received_datagrams,
            packet_buffer_pool_hit_count: snapshot.allocations.packet_buffer_pool_hits,
            packet_buffer_pool_miss_count: snapshot.allocations.packet_buffer_pool_misses,
            h2_flow_control_stall_count: snapshot.h2_flow_control.capacity_stall_count,
            h2_flow_control_stall_total_ms: duration_milliseconds(
                snapshot.h2_flow_control.capacity_stall_total,
            ),
            h2_flow_control_stall_max_ms: duration_milliseconds(
                snapshot.h2_flow_control.capacity_stall_max,
            ),
            h2_stream_receive_window_bytes: snapshot.h2_flow_control.stream_receive_window_bytes,
            h2_connection_receive_window_bytes: snapshot
                .h2_flow_control
                .connection_receive_window_bytes,
            direct_dns_success_count: snapshot.direct_dns.successes,
            direct_dns_failure_count: snapshot.direct_dns.failures,
            direct_dns_timeout_count: snapshot.direct_dns.timeouts,
            direct_dns_last_rtt_ms: direct_dns_last_rtt,
            direct_dns_last_rtt_known,
            pmtu_change_count: snapshot.pmtu.change_count,
            pmtu_revalidation_failure_count: snapshot.pmtu.revalidation_failure_count,
            smoothed_rtt_availability: availability(snapshot.rtt.smoothed.availability) as i32,
            min_rtt_availability: availability(snapshot.rtt.minimum.availability) as i32,
            rtt_variance_availability: availability(snapshot.rtt.variance.availability) as i32,
            interval_loss_availability: availability(
                snapshot.loss.interval_basis_points.availability,
            ) as i32,
            congestion_window_availability: availability(
                snapshot.congestion.congestion_window_bytes.availability,
            ) as i32,
            bytes_in_flight_availability: availability(
                snapshot.congestion.bytes_in_flight.availability,
            ) as i32,
            send_rate_availability: availability(snapshot.congestion.send_rate_bps.availability)
                as i32,
            ..v1::ConnectionMetrics::default()
        }),
        queues,
        pmtu: Some(v1::PmtuQuality {
            availability: availability(snapshot.pmtu.current_bytes.availability) as i32,
            outer_pmtu_bytes: snapshot.pmtu.current_bytes.value.unwrap_or_default(),
            effective_connect_ip_payload_bytes: 0,
            phase_code: pmtu_phase(snapshot.pmtu.phase).to_owned(),
            change_count: snapshot.pmtu.change_count,
            revalidation_failure_count: snapshot.pmtu.revalidation_failure_count,
            effective_payload_availability: v1::MetricAvailability::NotReady as i32,
        }),
        migration: Some(v1::MigrationQuality {
            phase_code: migration_phase(snapshot.migration.phase).to_owned(),
            attempt_count: snapshot.migration.attempts,
            success_count: snapshot.migration.successes,
            failure_count: snapshot.migration.failures,
            last_duration_ms: last_migration_duration,
            last_duration_known: last_migration_duration_known,
            last_reason_code: snapshot
                .migration
                .last_reason
                .map(migration_reason)
                .unwrap_or_default()
                .to_owned(),
        }),
        direct_dns: Some(v1::DirectDnsQuality {
            mode: direct_dns_mode(snapshot.direct_dns.mode) as i32,
            phase_code: direct_dns_phase(snapshot.direct_dns.phase).to_owned(),
            success_count: snapshot.direct_dns.successes,
            failure_count: snapshot.direct_dns.failures,
            timeout_count: snapshot.direct_dns.timeouts,
            last_rtt_ms: direct_dns_last_rtt,
            last_rtt_known: direct_dns_last_rtt_known,
            last_reason_code: snapshot
                .direct_dns
                .last_reason
                .map(direct_dns_reason)
                .unwrap_or_default()
                .to_owned(),
        }),
    }
}

pub(crate) fn is_major_change(
    previous: &v1::NetworkQualitySnapshot,
    next: &v1::NetworkQualitySnapshot,
) -> bool {
    previous.pmtu.as_ref().map(|value| value.change_count)
        != next.pmtu.as_ref().map(|value| value.change_count)
        || previous
            .migration
            .as_ref()
            .map(|value| value.phase_code.as_str())
            != next
                .migration
                .as_ref()
                .map(|value| value.phase_code.as_str())
        || dns_degraded_transition(previous, next)
        || (queue_drop_total(previous) == 0 && queue_drop_total(next) != 0)
}

pub(crate) fn same_content(
    previous: &v1::NetworkQualitySnapshot,
    next: &v1::NetworkQualitySnapshot,
) -> bool {
    let mut previous = previous.clone();
    let mut next = next.clone();
    previous.sampled_at_unix_ms = 0;
    next.sampled_at_unix_ms = 0;
    previous == next
}

fn dns_degraded_transition(
    previous: &v1::NetworkQualitySnapshot,
    next: &v1::NetworkQualitySnapshot,
) -> bool {
    let previous = previous
        .direct_dns
        .as_ref()
        .map(|quality| quality.phase_code.as_str());
    let next = next
        .direct_dns
        .as_ref()
        .map(|quality| quality.phase_code.as_str());
    previous != next && (previous == Some("degraded") || next == Some("degraded"))
}

fn queue_drop_total(snapshot: &v1::NetworkQualitySnapshot) -> u64 {
    snapshot
        .queues
        .iter()
        .map(|queue| queue.drop_items)
        .fold(0_u64, u64::saturating_add)
}

fn queue_drop_count(snapshot: &NetworkQualitySnapshot, kind: QueueKind) -> u64 {
    snapshot
        .queues
        .iter()
        .find(|queue| queue.kind == kind)
        .map_or(0, |queue| queue.drop_items)
}

fn duration_metric(metric: &MetricValue<Duration>) -> (u64, bool) {
    (
        metric.value.map_or(0, duration_milliseconds),
        metric.availability == MetricAvailability::Available && metric.value.is_some(),
    )
}

fn available_duration(metric: &MetricValue<Duration>) -> Option<Duration> {
    (metric.availability == MetricAvailability::Available)
        .then_some(metric.value)
        .flatten()
}

fn u64_metric(metric: &MetricValue<u64>) -> (u64, bool) {
    (
        metric.value.unwrap_or_default(),
        metric.availability == MetricAvailability::Available && metric.value.is_some(),
    )
}

fn u32_metric(metric: &MetricValue<u32>) -> (u32, bool) {
    (
        metric.value.unwrap_or_default(),
        metric.availability == MetricAvailability::Available && metric.value.is_some(),
    )
}

fn metric_raw_u64(metric: &MetricValue<u64>) -> u64 {
    metric.value.unwrap_or_default()
}

fn duration_milliseconds(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

fn availability(value: MetricAvailability) -> v1::MetricAvailability {
    match value {
        MetricAvailability::Available => v1::MetricAvailability::Available,
        MetricAvailability::Unsupported => v1::MetricAvailability::Unsupported,
        MetricAvailability::NotReady => v1::MetricAvailability::NotReady,
        MetricAvailability::Stale => v1::MetricAvailability::Stale,
    }
}

fn quality_level(value: NetworkQualityLevel) -> v1::NetworkQualityLevel {
    match value {
        NetworkQualityLevel::Good => v1::NetworkQualityLevel::Good,
        NetworkQualityLevel::Fair => v1::NetworkQualityLevel::Fair,
        NetworkQualityLevel::Poor => v1::NetworkQualityLevel::Poor,
        NetworkQualityLevel::LimitedData => v1::NetworkQualityLevel::LimitedData,
        NetworkQualityLevel::Disconnected => v1::NetworkQualityLevel::Disconnected,
    }
}

fn queue_kind(value: QueueKind) -> v1::QueueKind {
    match value {
        QueueKind::TunToTransport => v1::QueueKind::TunToTransport,
        QueueKind::ProxyToTransport => v1::QueueKind::ProxyToTransport,
        QueueKind::TransportOutgoingPackets => v1::QueueKind::TransportOutgoing,
        QueueKind::H3DatagramSend => v1::QueueKind::H3DatagramSend,
        QueueKind::H3WireSend => v1::QueueKind::H3WireSend,
        QueueKind::TransportToTun => v1::QueueKind::TransportToTun,
        QueueKind::TransportToProxy => v1::QueueKind::TransportToProxy,
        QueueKind::DirectDnsRequests => v1::QueueKind::DirectDns,
    }
}

fn pmtu_phase(value: PmtuPhase) -> &'static str {
    match value {
        PmtuPhase::Unsupported => "unsupported",
        PmtuPhase::Unknown => "unknown",
        PmtuPhase::Probing => "probing",
        PmtuPhase::Stable => "stable",
        PmtuPhase::Revalidating => "revalidating",
        PmtuPhase::Degraded => "degraded",
    }
}

fn migration_phase(value: MigrationPhase) -> &'static str {
    match value {
        MigrationPhase::Idle => "idle",
        MigrationPhase::PreparingSocket => "preparing_socket",
        MigrationPhase::Probing => "probing",
        MigrationPhase::Validated => "validated",
        MigrationPhase::Promoting => "promoting",
        MigrationPhase::Stable => "stable",
        MigrationPhase::Aborted => "aborted",
    }
}

fn migration_reason(value: MigrationReasonCode) -> &'static str {
    match value {
        MigrationReasonCode::FamilyUnavailable => "family_unavailable",
        MigrationReasonCode::SocketProtectFailed => "socket_protect_failed",
        MigrationReasonCode::GenerationChangedDuringSetup => "generation_changed_during_setup",
        MigrationReasonCode::PeerCidUnavailable => "peer_cid_unavailable",
        MigrationReasonCode::LocalCidUnavailable => "local_cid_unavailable",
        MigrationReasonCode::PathProbeRejected => "path_probe_rejected",
        MigrationReasonCode::PathValidationTimeout => "path_validation_timeout",
        MigrationReasonCode::Superseded => "superseded",
        MigrationReasonCode::PromotionFailed => "promotion_failed",
        MigrationReasonCode::ConnectionClosed => "connection_closed",
        MigrationReasonCode::Unsupported => "unsupported",
    }
}

fn direct_dns_mode(value: DirectDnsMode) -> v1::DirectDnsMode {
    match value {
        DirectDnsMode::PhysicalSystem => v1::DirectDnsMode::PhysicalSystem,
        DirectDnsMode::Doh => v1::DirectDnsMode::Doh,
        DirectDnsMode::Dot => v1::DirectDnsMode::Dot,
    }
}

fn direct_dns_phase(value: DirectDnsPhase) -> &'static str {
    match value {
        DirectDnsPhase::System => "system",
        DirectDnsPhase::Connecting => "connecting",
        DirectDnsPhase::Ready => "ready",
        DirectDnsPhase::Degraded => "degraded",
        DirectDnsPhase::Disabled => "disabled",
    }
}

fn direct_dns_reason(value: DirectDnsReasonCode) -> &'static str {
    match value {
        DirectDnsReasonCode::Timeout => "timeout",
        DirectDnsReasonCode::QueryFailed => "query_failed",
        DirectDnsReasonCode::NetworkChanged => "network_changed",
        DirectDnsReasonCode::Unsupported => "unsupported",
    }
}

#[cfg(test)]
mod tests {
    use usque_core::{AddressFamily, Transport};

    use super::*;

    #[test]
    fn disconnected_conversion_has_explicit_availability_and_no_identifiers() {
        let proto = snapshot_to_proto(&disconnected_snapshot());
        assert_eq!(proto.level, v1::NetworkQualityLevel::Disconnected as i32);
        assert!(proto.connection_instance_id.is_empty());
        assert!(proto.queues.iter().all(|queue| {
            queue.availability == v1::MetricAvailability::NotReady as i32
                && queue.capacity_items == 0
                && queue.capacity_bytes == 0
        }));
    }

    #[test]
    fn quality_wire_snapshot_contains_only_sanitized_state() {
        let telemetry = NetworkQualityTelemetry::default();
        let connection = telemetry.begin_connection(Transport::Http2, AddressFamily::Ipv4);
        telemetry.observe_h2_rtt(
            Duration::from_millis(10),
            Duration::from_millis(12),
            Duration::from_millis(8),
            Duration::from_millis(2),
        );
        telemetry.record_direct_dns_failure(DirectDnsReasonCode::Timeout, true);
        let proto = snapshot_to_proto(&NetworkQualitySampler::new(telemetry).sample());
        assert_eq!(proto.connection_instance_id, connection.0.to_string());
        assert_eq!(
            proto.direct_dns.as_ref().unwrap().last_reason_code,
            "timeout"
        );
        let debug = format!("{proto:?}");
        for forbidden in ["example.com", "127.0.0.1", "bootstrap", "scid", "token="] {
            assert!(!debug.contains(forbidden));
        }
    }

    #[test]
    fn major_change_detects_migration_and_dns_degradation() {
        let mut previous = v1::NetworkQualitySnapshot {
            migration: Some(v1::MigrationQuality {
                phase_code: "idle".to_owned(),
                ..v1::MigrationQuality::default()
            }),
            direct_dns: Some(v1::DirectDnsQuality {
                phase_code: "system".to_owned(),
                ..v1::DirectDnsQuality::default()
            }),
            ..v1::NetworkQualitySnapshot::default()
        };
        let mut next = previous.clone();
        next.migration.as_mut().unwrap().phase_code = "probing".to_owned();
        assert!(is_major_change(&previous, &next));

        previous = next.clone();
        next.direct_dns.as_mut().unwrap().phase_code = "degraded".to_owned();
        assert!(is_major_change(&previous, &next));
    }
}
