//! Read-only, bounded native timeline. No endpoint, raw failure detail or key
//! material is serialized, and the full timeline never rides the 1 Hz event bus.

use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, UNIX_EPOCH};

use serde_json::{Value, json};
use usque_core::{AddressFamily, Transport};
use usque_transport::{ConnectionEventType, ConnectionTimelineSnapshot};

const MAX_EVENTS: usize = 256;
const MAX_JSON_BYTES: usize = 192 * 1024;
static LATEST: OnceLock<Mutex<Arc<ConnectionTimelineSnapshot>>> = OnceLock::new();

#[cfg(any(test, target_os = "android"))]
pub(super) fn publish(snapshot: ConnectionTimelineSnapshot) {
    let slot = LATEST.get_or_init(|| Mutex::new(Arc::default()));
    if let Ok(mut current) = slot.lock() {
        *current = Arc::new(snapshot);
    }
}

pub(super) fn json_snapshot() -> String {
    let snapshot = LATEST
        .get()
        .and_then(|slot| slot.lock().ok())
        .map(|slot| Arc::clone(&slot))
        .unwrap_or_default();
    let result = to_value(&snapshot).to_string();
    if snapshot.events.is_empty() {
        return "{}".to_owned();
    }
    // Hard Binder byte budget. Never truncate UTF-8 or return partial JSON.
    if result.len() <= MAX_JSON_BYTES {
        result
    } else {
        "{}".to_owned()
    }
}

fn millis(value: Duration) -> u64 {
    value.as_millis().min(i64::MAX as u128) as u64
}

fn to_value(snapshot: &ConnectionTimelineSnapshot) -> Value {
    let skipped = snapshot.events.len().saturating_sub(MAX_EVENTS);
    let events: Vec<_> = snapshot.events.iter().skip(skipped).map(|event| {
        let failure = event.failure.as_ref().map(|failure| json!({
            "code": failure.code.as_str(), "stage": failure.stage.as_str(),
            "retryable": failure.retryable, "fallback_allowed": failure.fallback_allowed,
            "severity": failure.severity,
        }));
        json!({
            "sequence": event.sequence.min(i64::MAX as u64),
            "timestamp_unix_milliseconds": millis(event.timestamp.duration_since(UNIX_EPOCH).unwrap_or_default()),
            "elapsed_from_attempt_start_milliseconds": millis(event.elapsed_from_attempt_start),
            "event_type": event_type(event.event_type),
            "stage": event.stage.map(|stage| stage.as_str()),
            "transport": event.transport.map(|transport| match transport { Transport::Http2 => "http2", Transport::Http3 => "http3" }),
            "address_family": event.address_family.map(|family| match family { AddressFamily::Ipv4 => "ipv4", AddressFamily::Ipv6 => "ipv6" }),
            "duration_milliseconds": event.duration.map(millis), "failure": failure,
        })
    }).collect();
    let metrics = &snapshot.metrics;
    json!({
        "schema_version": 1, "events": events,
        "dropped_event_count": snapshot.dropped_event_count.saturating_add(skipped as u64).min(i64::MAX as u64),
        "metrics": {
            "last_connect_duration_milliseconds": metrics.last_connect_duration.map(millis),
            "last_h3_handshake_duration_milliseconds": metrics.last_h3_handshake_duration.map(millis),
            "last_h2_handshake_duration_milliseconds": metrics.last_h2_handshake_duration.map(millis),
            "current_smoothed_rtt_milliseconds": metrics.current_smoothed_rtt.map(millis),
            "current_smoothed_rtt_known": metrics.current_smoothed_rtt.is_some(),
            "reconnect_count": metrics.reconnect_count, "fallback_count": metrics.fallback_count,
            "network_change_count": metrics.network_change_count,
            "send_queue_high_watermark": metrics.send_queue_high_watermark.min(i64::MAX as u64),
            "send_queue_drop_count": metrics.send_queue_drop_count.min(i64::MAX as u64),
            "last_failure_code": metrics.last_failure_code.map(|code| code.as_str()),
            "last_reconnect_code": metrics.last_reconnect_code.map(|code| code.as_str()),
        }
    })
}

fn event_type(event: ConnectionEventType) -> &'static str {
    match event {
        ConnectionEventType::AttemptStarted => "attempt_started",
        ConnectionEventType::EndpointResolved => "endpoint_resolved",
        ConnectionEventType::SocketConnected => "socket_connected",
        ConnectionEventType::TlsReady => "tls_ready",
        ConnectionEventType::QuicReady => "quic_ready",
        ConnectionEventType::MasqueAccepted => "masque_accepted",
        ConnectionEventType::PeerSettingsReceived => "peer_settings_received",
        ConnectionEventType::AddressAssigned => "address_assigned",
        ConnectionEventType::TunnelReady => "tunnel_ready",
        ConnectionEventType::FirstPacketSent => "first_packet_sent",
        ConnectionEventType::FirstPacketReceived => "first_packet_received",
        ConnectionEventType::FallbackStarted => "fallback_started",
        ConnectionEventType::ReconnectScheduled => "reconnect_scheduled",
        ConnectionEventType::NetworkChanged => "network_changed",
        ConnectionEventType::RecoveryProbeStarted => "recovery_probe_started",
        ConnectionEventType::RecoveryProbeSucceeded => "recovery_probe_succeeded",
        ConnectionEventType::RecoveryProbeFailed => "recovery_probe_failed",
        ConnectionEventType::PathPromoted => "path_promoted",
        ConnectionEventType::MigrationStarted => "migration_started",
        ConnectionEventType::MigrationPathValidated => "migration_path_validated",
        ConnectionEventType::MigrationPromoted => "migration_promoted",
        ConnectionEventType::MigrationFailed => "migration_failed",
        ConnectionEventType::QueueSaturated => "queue_saturated",
        ConnectionEventType::PmtuChanged => "pmtu_changed",
        ConnectionEventType::PmtuRevalidationStarted => "pmtu_revalidation_started",
        ConnectionEventType::PmtuRevalidationFailed => "pmtu_revalidation_failed",
        ConnectionEventType::Disconnected => "disconnected",
        ConnectionEventType::Failed => "failed",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use usque_core::{TransportFailure, TransportFailureCode, TransportStage};
    use usque_transport::{ConnectionEvent, ConnectionMetrics};

    #[test]
    fn native_timeline_is_numeric_allowlisted_and_bounded() {
        let mut failure = TransportFailure::new(
            TransportFailureCode::PmtuRevalidationExhausted,
            TransportStage::PacketSend,
        );
        failure.sanitized_detail = Some("private.example 192.0.2.1 SSID=private".to_owned());
        let event = ConnectionEvent {
            sequence: 1,
            timestamp: UNIX_EPOCH + Duration::from_secs(1),
            elapsed_from_attempt_start: Duration::from_millis(8),
            event_type: ConnectionEventType::PmtuRevalidationFailed,
            stage: Some(TransportStage::PacketSend),
            transport: Some(Transport::Http3),
            address_family: Some(AddressFamily::Ipv4),
            duration: None,
            failure: Some(failure),
        };
        let snapshot = ConnectionTimelineSnapshot {
            events: vec![event; 512],
            dropped_event_count: 2,
            metrics: ConnectionMetrics {
                fallback_count: 3,
                current_smoothed_rtt: Some(Duration::from_millis(42)),
                ..Default::default()
            },
        };
        let value = to_value(&snapshot);
        assert_eq!(value["events"].as_array().unwrap().len(), MAX_EVENTS);
        assert_eq!(value["dropped_event_count"], 258);
        assert_eq!(value["metrics"]["fallback_count"], 3);
        assert_eq!(value["metrics"]["current_smoothed_rtt_known"], true);
        assert_eq!(value["events"][0]["event_type"], "pmtu_revalidation_failed");
        assert!(value.to_string().len() < MAX_JSON_BYTES);
        for forbidden in ["private", "192.0.2.1", "SSID", "sanitized_detail"] {
            assert!(!value.to_string().contains(forbidden));
        }
        publish(snapshot);
        assert!(serde_json::from_str::<Value>(&json_snapshot()).is_ok());
    }
}
