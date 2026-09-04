//! Pure snapshot checks. This module has no socket, platform or mutation API.
use std::time::Duration;

use usque_core::{
    DiagnosticCheckStatus as Status, DiagnosticFinding, DirectDnsMode, FailureSeverity, Transport,
};
use usque_transport::{
    DirectDnsPhase, MetricAvailability, MetricValue, MigrationReasonCode, PmtuPhase,
};

use super::checks::{DiagnosticCheck, DiagnosticContext, PassiveCheckKind as Kind};

pub(super) fn finding(
    check: &dyn DiagnosticCheck,
    status: Status,
    summary: &'static str,
    remediation: &'static str,
    evidence: Vec<String>,
) -> DiagnosticFinding {
    let mut result = DiagnosticFinding::pending(check.id(), check.category());
    result.status = status;
    result.severity = match status {
        Status::Failed => FailureSeverity::Error,
        Status::Warning | Status::Cancelled => FailureSeverity::Warning,
        _ => FailureSeverity::Info,
    };
    result.summary_key = summary.to_owned();
    result.remediation_key = remediation.to_owned();
    result.sanitized_evidence = evidence;
    result
}

pub(super) fn evaluate(
    check: &dyn DiagnosticCheck,
    kind: Kind,
    context: &DiagnosticContext,
) -> DiagnosticFinding {
    let q = &context.quality;
    let result = |status, summary, remediation, evidence| {
        finding(check, status, summary, remediation, evidence)
    };
    let unavailable = || result(Status::Skipped, "nq_finding_unavailable", "none", vec![]);
    if matches!(kind, Kind::EncryptedDnsConfiguration) {
        return if context.direct_dns.validate().is_err() {
            result(
                Status::Failed,
                "nq_finding_invalid_configuration",
                "nq_profile",
                vec![],
            )
        } else if context.direct_dns.mode == DirectDnsMode::PhysicalSystem {
            result(Status::Skipped, "nq_finding_dns_system", "none", vec![])
        } else if !usque_transport::ENCRYPTED_DIRECT_DNS_ENABLED {
            result(
                Status::Failed,
                "nq_finding_unsupported",
                "nq_profile",
                vec![],
            )
        } else {
            result(
                Status::Passed,
                "nq_finding_dns_custom_valid",
                "none",
                vec!["plaintext_fallback=0".to_owned()],
            )
        };
    }
    if q.connection_id.is_none() {
        return unavailable();
    }
    if q.sampled_at.elapsed() > Duration::from_secs(3) {
        return result(Status::Warning, "nq_finding_stale", "nq_retry", vec![]);
    }
    match kind {
        Kind::QualityRtt => match available(&q.rtt.smoothed) {
            Some(rtt) => {
                let high = *rtt >= Duration::from_millis(150);
                result(
                    if high {
                        Status::Warning
                    } else {
                        Status::Passed
                    },
                    if high {
                        "nq_finding_rtt_high"
                    } else {
                        "nq_finding_healthy"
                    },
                    if high { "nq_network" } else { "none" },
                    vec![format!(
                        "rtt_ms={}",
                        rtt.as_millis().min(u128::from(u64::MAX))
                    )],
                )
            }
            None => unavailable(),
        },
        Kind::QualityLoss if q.transport != Some(Transport::Http3) => unavailable(),
        Kind::QualityLoss => match available(&q.loss.interval_basis_points) {
            Some(loss) => result(
                if *loss >= 200 {
                    Status::Warning
                } else {
                    Status::Passed
                },
                if *loss >= 200 {
                    "nq_finding_loss_high"
                } else {
                    "nq_finding_healthy"
                },
                if *loss >= 200 { "nq_network" } else { "none" },
                vec![format!("loss_basis_points={loss}")],
            ),
            None => unavailable(),
        },
        Kind::QualityQueues => {
            let mut peak = None;
            let mut drops = 0u64;
            for queue in &q.queues {
                if queue.availability != MetricAvailability::Available {
                    continue;
                }
                for (current, capacity) in [
                    (queue.current_items, queue.item_capacity),
                    (queue.current_bytes, queue.byte_capacity),
                ] {
                    if let Some(percent) = current.saturating_mul(100).checked_div(capacity) {
                        peak = Some(peak.unwrap_or(0).max(percent.min(100)));
                    }
                }
                drops = drops.saturating_add(queue.drop_items);
            }
            let Some(peak) = peak else {
                return unavailable();
            };
            let pressure = peak >= 50 || drops > 0;
            result(
                if pressure {
                    Status::Warning
                } else {
                    Status::Passed
                },
                if pressure {
                    "nq_finding_queue_pressure"
                } else {
                    "nq_finding_healthy"
                },
                if pressure { "nq_network" } else { "none" },
                vec![
                    format!("queue_percent={peak}"),
                    format!("queue_drops={drops}"),
                ],
            )
        }
        Kind::QualityPmtu if q.transport != Some(Transport::Http3) => unavailable(),
        Kind::QualityPmtu => {
            let Some(bytes) = available(&q.pmtu.current_bytes) else {
                return unavailable();
            };
            let degraded = q.pmtu.phase == PmtuPhase::Degraded;
            result(
                if degraded {
                    Status::Warning
                } else {
                    Status::Passed
                },
                if degraded {
                    "nq_finding_pmtu_degraded"
                } else {
                    "nq_finding_healthy"
                },
                if degraded { "nq_network" } else { "none" },
                vec![
                    format!("pmtu_bytes={bytes}"),
                    format!("pmtu_failures={}", q.pmtu.revalidation_failure_count),
                ],
            )
        }
        Kind::MigrationCapability if q.transport != Some(Transport::Http3) => unavailable(),
        Kind::MigrationCapability => {
            let blocked = !usque_transport::PRODUCTION_NETWORK_FEATURES.quic_migration
                || matches!(
                    q.migration.last_reason,
                    Some(
                        MigrationReasonCode::Unsupported
                            | MigrationReasonCode::PeerCidUnavailable
                            | MigrationReasonCode::LocalCidUnavailable
                    )
                );
            result(
                if blocked {
                    Status::Warning
                } else {
                    Status::Passed
                },
                if blocked {
                    "nq_finding_migration_reconnect"
                } else {
                    "nq_finding_healthy"
                },
                "none",
                vec![format!("migration_failures={}", q.migration.failures)],
            )
        }
        Kind::EncryptedDnsRuntime if context.direct_dns.mode == DirectDnsMode::PhysicalSystem => {
            result(Status::Skipped, "nq_finding_dns_system", "none", vec![])
        }
        Kind::EncryptedDnsRuntime => {
            let expected = match context.direct_dns.mode {
                DirectDnsMode::Doh => usque_transport::DirectDnsMode::Doh,
                DirectDnsMode::Dot => usque_transport::DirectDnsMode::Dot,
                DirectDnsMode::PhysicalSystem => unreachable!(),
            };
            if q.direct_dns.mode != expected {
                return result(
                    Status::Warning,
                    "nq_finding_dns_changed",
                    "nq_reconnect",
                    vec![],
                );
            }
            let (status, summary) = match q.direct_dns.phase {
                DirectDnsPhase::Ready => (Status::Passed, "nq_finding_dns_runtime"),
                DirectDnsPhase::Degraded | DirectDnsPhase::Disabled => {
                    (Status::Warning, "nq_finding_dns_degraded")
                }
                _ => (Status::Skipped, "nq_finding_unavailable"),
            };
            result(
                status,
                summary,
                if status == Status::Warning {
                    "nq_profile"
                } else {
                    "none"
                },
                vec![
                    format!("dns_successes={}", q.direct_dns.successes),
                    format!("dns_failures={}", q.direct_dns.failures),
                    format!("dns_timeouts={}", q.direct_dns.timeouts),
                ],
            )
        }
        _ => unavailable(),
    }
}

fn available<T>(metric: &MetricValue<T>) -> Option<&T> {
    (metric.availability == MetricAvailability::Available)
        .then_some(metric.value.as_ref())
        .flatten()
}
