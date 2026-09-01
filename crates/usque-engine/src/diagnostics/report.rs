use std::time::{Duration, SystemTime, UNIX_EPOCH};

use usque_core::{
    AddressFamily, DiagnosticCategory, DiagnosticCheckStatus, DiagnosticFinding, DiagnosticMode,
    DiagnosticSession, DiagnosticSessionState, FailureSeverity, Transport,
};
use usque_ipc::v1;
use usque_transport::{
    ConnectionEvent, ConnectionEventType, ConnectionMetrics, ConnectionTimelineSnapshot,
};

use crate::transport_failure_to_proto;

pub(crate) fn empty_session_to_proto() -> v1::DiagnosticSession {
    v1::DiagnosticSession::default()
}

pub(crate) fn session_to_proto(session: &DiagnosticSession) -> v1::DiagnosticSession {
    v1::DiagnosticSession {
        session_id: session.session_id.to_string(),
        state: match session.state {
            DiagnosticSessionState::Pending => v1::DiagnosticSessionState::Pending as i32,
            DiagnosticSessionState::Running => v1::DiagnosticSessionState::Running as i32,
            DiagnosticSessionState::Cancelling => v1::DiagnosticSessionState::Cancelling as i32,
            DiagnosticSessionState::Completed => v1::DiagnosticSessionState::Completed as i32,
            DiagnosticSessionState::Failed => v1::DiagnosticSessionState::Failed as i32,
            DiagnosticSessionState::Cancelled => v1::DiagnosticSessionState::Cancelled as i32,
        },
        started_at_unix_milliseconds: session.started_at.timestamp_millis(),
        completed_at_unix_milliseconds: session
            .completed_at
            .map_or(0, |completed| completed.timestamp_millis()),
        mode: match session.mode {
            DiagnosticMode::Standard => v1::DiagnosticMode::Standard as i32,
            DiagnosticMode::Deep => v1::DiagnosticMode::Deep as i32,
        },
        current_check: session.current_check.clone().unwrap_or_default(),
        progress_percent: session.progress_percent,
        findings: session.findings.iter().map(finding_to_proto).collect(),
        summary: Some(v1::DiagnosticSummary {
            passed: session.summary.passed,
            warnings: session.summary.warnings,
            failed: session.summary.failed,
            skipped: session.summary.skipped,
            cancelled: session.summary.cancelled,
        }),
    }
}

pub(crate) fn finding_to_proto(finding: &DiagnosticFinding) -> v1::DiagnosticFinding {
    v1::DiagnosticFinding {
        check_id: finding.check_id.clone(),
        category: match finding.category {
            DiagnosticCategory::LocalComponent => v1::DiagnosticCategory::LocalComponent as i32,
            DiagnosticCategory::PhysicalNetwork => v1::DiagnosticCategory::PhysicalNetwork as i32,
            DiagnosticCategory::Transport => v1::DiagnosticCategory::Transport as i32,
            DiagnosticCategory::Tunnel => v1::DiagnosticCategory::Tunnel as i32,
            DiagnosticCategory::Protection => v1::DiagnosticCategory::Protection as i32,
            DiagnosticCategory::Recovery => v1::DiagnosticCategory::Recovery as i32,
        },
        status: match finding.status {
            DiagnosticCheckStatus::Pending => v1::DiagnosticCheckStatus::Pending as i32,
            DiagnosticCheckStatus::Running => v1::DiagnosticCheckStatus::Running as i32,
            DiagnosticCheckStatus::Passed => v1::DiagnosticCheckStatus::Passed as i32,
            DiagnosticCheckStatus::Warning => v1::DiagnosticCheckStatus::Warning as i32,
            DiagnosticCheckStatus::Failed => v1::DiagnosticCheckStatus::Failed as i32,
            DiagnosticCheckStatus::Skipped => v1::DiagnosticCheckStatus::Skipped as i32,
            DiagnosticCheckStatus::Cancelled => v1::DiagnosticCheckStatus::Cancelled as i32,
        },
        failure: finding.failure.as_ref().map(transport_failure_to_proto),
        severity: severity_to_proto(finding.severity),
        summary_key: finding.summary_key.clone(),
        remediation_key: finding.remediation_key.clone(),
        sanitized_evidence: finding.sanitized_evidence.clone(),
        started_at_unix_milliseconds: finding
            .started_at
            .map_or(0, |started| started.timestamp_millis()),
        duration_milliseconds: finding.duration_milliseconds.unwrap_or_default(),
        dependency_reason: finding.dependency_reason.clone().unwrap_or_default(),
    }
}

pub(crate) fn timeline_to_proto(timeline: &ConnectionTimelineSnapshot) -> v1::ConnectionTimeline {
    v1::ConnectionTimeline {
        events: timeline.events.iter().map(event_to_proto).collect(),
        metrics: Some(metrics_to_proto(&timeline.metrics)),
        dropped_event_count: timeline.dropped_event_count,
    }
}

fn event_to_proto(event: &ConnectionEvent) -> v1::ConnectionEvent {
    v1::ConnectionEvent {
        sequence: event.sequence,
        timestamp_unix_milliseconds: system_time_milliseconds(event.timestamp),
        elapsed_from_attempt_start_milliseconds: duration_milliseconds(
            event.elapsed_from_attempt_start,
        ),
        event_type: match event.event_type {
            ConnectionEventType::AttemptStarted => v1::ConnectionEventType::AttemptStarted as i32,
            ConnectionEventType::EndpointResolved => {
                v1::ConnectionEventType::EndpointResolved as i32
            }
            ConnectionEventType::SocketConnected => v1::ConnectionEventType::SocketConnected as i32,
            ConnectionEventType::TlsReady => v1::ConnectionEventType::TlsReady as i32,
            ConnectionEventType::QuicReady => v1::ConnectionEventType::QuicReady as i32,
            ConnectionEventType::MasqueAccepted => v1::ConnectionEventType::MasqueAccepted as i32,
            ConnectionEventType::PeerSettingsReceived => {
                v1::ConnectionEventType::PeerSettingsReceived as i32
            }
            ConnectionEventType::AddressAssigned => v1::ConnectionEventType::AddressAssigned as i32,
            ConnectionEventType::TunnelReady => v1::ConnectionEventType::TunnelReady as i32,
            ConnectionEventType::FirstPacketSent => v1::ConnectionEventType::FirstPacketSent as i32,
            ConnectionEventType::FirstPacketReceived => {
                v1::ConnectionEventType::FirstPacketReceived as i32
            }
            ConnectionEventType::FallbackStarted => v1::ConnectionEventType::FallbackStarted as i32,
            ConnectionEventType::ReconnectScheduled => {
                v1::ConnectionEventType::ReconnectScheduled as i32
            }
            ConnectionEventType::NetworkChanged => v1::ConnectionEventType::NetworkChanged as i32,
            ConnectionEventType::RecoveryProbeStarted => {
                v1::ConnectionEventType::RecoveryProbeStarted as i32
            }
            ConnectionEventType::RecoveryProbeSucceeded => {
                v1::ConnectionEventType::RecoveryProbeSucceeded as i32
            }
            ConnectionEventType::RecoveryProbeFailed => {
                v1::ConnectionEventType::RecoveryProbeFailed as i32
            }
            ConnectionEventType::PathPromoted => v1::ConnectionEventType::PathPromoted as i32,
            ConnectionEventType::QueueSaturated => v1::ConnectionEventType::QueueSaturated as i32,
            ConnectionEventType::Disconnected => v1::ConnectionEventType::Disconnected as i32,
            ConnectionEventType::Failed => v1::ConnectionEventType::Failed as i32,
        },
        stage: event
            .stage
            .map(usque_core::TransportStage::as_str)
            .unwrap_or_default()
            .to_owned(),
        transport: event
            .transport
            .map(transport_name)
            .unwrap_or_default()
            .to_owned(),
        address_family: event
            .address_family
            .map(family_name)
            .unwrap_or_default()
            .to_owned(),
        duration_milliseconds: event.duration.map_or(0, duration_milliseconds),
        failure: event.failure.as_ref().map(transport_failure_to_proto),
    }
}

fn metrics_to_proto(metrics: &ConnectionMetrics) -> v1::ConnectionMetrics {
    v1::ConnectionMetrics {
        last_connect_duration_milliseconds: metrics
            .last_connect_duration
            .map_or(0, duration_milliseconds),
        last_h3_handshake_duration_milliseconds: metrics
            .last_h3_handshake_duration
            .map_or(0, duration_milliseconds),
        last_h2_handshake_duration_milliseconds: metrics
            .last_h2_handshake_duration
            .map_or(0, duration_milliseconds),
        current_smoothed_rtt_milliseconds: metrics
            .current_smoothed_rtt
            .map_or(0, duration_milliseconds),
        current_smoothed_rtt_known: metrics.current_smoothed_rtt.is_some(),
        reconnect_count: metrics.reconnect_count,
        fallback_count: metrics.fallback_count,
        network_change_count: metrics.network_change_count,
        send_queue_high_watermark: metrics.send_queue_high_watermark,
        send_queue_drop_count: metrics.send_queue_drop_count,
        last_failure_code: metrics
            .last_failure_code
            .map(|code| code.as_str().to_owned())
            .unwrap_or_default(),
        last_reconnect_code: metrics
            .last_reconnect_code
            .map(|code| code.as_str().to_owned())
            .unwrap_or_default(),
        ..v1::ConnectionMetrics::default()
    }
}

fn severity_to_proto(severity: FailureSeverity) -> i32 {
    match severity {
        FailureSeverity::Info => v1::FailureSeverity::Info as i32,
        FailureSeverity::Warning => v1::FailureSeverity::Warning as i32,
        FailureSeverity::Error => v1::FailureSeverity::Error as i32,
        FailureSeverity::Critical => v1::FailureSeverity::Critical as i32,
    }
}

const fn transport_name(transport: Transport) -> &'static str {
    match transport {
        Transport::Http3 => "h3",
        Transport::Http2 => "h2",
    }
}

const fn family_name(family: AddressFamily) -> &'static str {
    match family {
        AddressFamily::Ipv4 => "ipv4",
        AddressFamily::Ipv6 => "ipv6",
    }
}

fn duration_milliseconds(duration: Duration) -> u64 {
    duration.as_millis().min(u128::from(u64::MAX)) as u64
}

fn system_time_milliseconds(time: SystemTime) -> i64 {
    time.duration_since(UNIX_EPOCH)
        .map(duration_milliseconds)
        .unwrap_or_default()
        .min(i64::MAX as u64) as i64
}
