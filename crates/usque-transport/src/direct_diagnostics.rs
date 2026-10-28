//! Fixed-cardinality DIRECT data-path diagnostics without destination data.

use std::sync::{
    Mutex,
    atomic::{AtomicU64, Ordering},
};
use std::time::{Duration, Instant};

use crate::transport_performance::add;

const LOG_INTERVAL: Duration = Duration::from_secs(15);
const FLOW_FAILURE_SLOT: usize = 6;
const RETURN_DROP_SLOT: usize = 7;

#[derive(Clone, Copy, Debug)]
pub(crate) enum DirectFallbackReason {
    Unavailable,
    WorkerFailed,
    FlowCapacity,
    SetupFailed,
    UnsupportedTarget,
    RewriteFailed,
}

impl DirectFallbackReason {
    fn index(self) -> usize {
        match self {
            Self::Unavailable => 0,
            Self::WorkerFailed => 1,
            Self::FlowCapacity => 2,
            Self::SetupFailed => 3,
            Self::UnsupportedTarget => 4,
            Self::RewriteFailed => 5,
        }
    }

    fn code(self) -> &'static str {
        match self {
            Self::Unavailable => "direct_unavailable",
            Self::WorkerFailed => "direct_worker_failed",
            Self::FlowCapacity => "direct_flow_capacity",
            Self::SetupFailed => "direct_setup_failed",
            Self::UnsupportedTarget => "direct_unsupported_target",
            Self::RewriteFailed => "direct_rewrite_failed",
        }
    }
}

/// Session-local counters for DIRECT data traffic, separate from DNS metrics.
/// Only fixed numeric keys are exported; no destinations or error text exist.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DirectGatewaySnapshot {
    pub fallback_total: u64,
    pub fallback_unavailable: u64,
    pub fallback_worker_failed: u64,
    pub fallback_flow_capacity: u64,
    pub fallback_setup_failed: u64,
    pub fallback_unsupported_target: u64,
    pub fallback_rewrite_failed: u64,
    pub flow_failures: u64,
    pub return_queue_drop_packets: u64,
    pub return_queue_drop_bytes: u64,
}

impl DirectGatewaySnapshot {
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "fallback_total": self.fallback_total,
            "fallback_unavailable": self.fallback_unavailable,
            "fallback_worker_failed": self.fallback_worker_failed,
            "fallback_flow_capacity": self.fallback_flow_capacity,
            "fallback_setup_failed": self.fallback_setup_failed,
            "fallback_unsupported_target": self.fallback_unsupported_target,
            "fallback_rewrite_failed": self.fallback_rewrite_failed,
            "flow_failures": self.flow_failures,
            "return_queue_drop_packets": self.return_queue_drop_packets,
            "return_queue_drop_bytes": self.return_queue_drop_bytes,
        })
    }
}

#[derive(Debug, Default)]
pub(crate) struct DirectGatewayDiagnostics {
    fallback_total: AtomicU64,
    fallbacks: [AtomicU64; 6],
    flow_failures: AtomicU64,
    return_queue_drop_packets: AtomicU64,
    return_queue_drop_bytes: AtomicU64,
    log_limits: Mutex<[LogLimit; 8]>,
}

#[derive(Debug, Clone, Copy, Default)]
struct LogLimit {
    last_logged: Option<Instant>,
    suppressed: u64,
}

impl LogLimit {
    /// Returns the number suppressed since the previous emitted event.
    fn admit(&mut self, now: Instant) -> Option<u64> {
        if self
            .last_logged
            .is_some_and(|last| now.saturating_duration_since(last) < LOG_INTERVAL)
        {
            self.suppressed = self.suppressed.saturating_add(1);
            return None;
        }
        self.last_logged = Some(now);
        Some(std::mem::take(&mut self.suppressed))
    }
}

impl DirectGatewayDiagnostics {
    pub(crate) fn record_fallback(&self, reason: DirectFallbackReason) {
        add(&self.fallback_total, 1);
        add(&self.fallbacks[reason.index()], 1);
        if let Some(suppressed_count) = self.admit_log(reason.index(), Instant::now()) {
            tracing::warn!(
                reason_code = reason.code(),
                fallback_count = self.fallbacks[reason.index()].load(Ordering::Relaxed),
                suppressed_count,
                "DIRECT traffic fell back to the tunnel"
            );
        }
    }

    pub(crate) fn record_flow_failure(&self) {
        add(&self.flow_failures, 1);
        if let Some(suppressed_count) = self.admit_log(FLOW_FAILURE_SLOT, Instant::now()) {
            tracing::warn!(
                reason_code = "direct_flow_failed",
                flow_failures = self.flow_failures.load(Ordering::Relaxed),
                suppressed_count,
                "a DIRECT flow worker failed"
            );
        }
    }

    pub(crate) fn record_return_queue_drop(&self, bytes: usize) {
        add(&self.return_queue_drop_packets, 1);
        add(&self.return_queue_drop_bytes, bytes as u64);
        if let Some(suppressed_count) = self.admit_log(RETURN_DROP_SLOT, Instant::now()) {
            tracing::warn!(
                reason_code = "direct_return_queue_full",
                drop_items = self.return_queue_drop_packets.load(Ordering::Relaxed),
                drop_bytes = self.return_queue_drop_bytes.load(Ordering::Relaxed),
                suppressed_count,
                "DIRECT return packets were dropped because the bounded queue is full"
            );
        }
    }

    fn admit_log(&self, slot: usize, now: Instant) -> Option<u64> {
        self.log_limits
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())[slot]
            .admit(now)
    }

    pub(crate) fn snapshot(&self) -> DirectGatewaySnapshot {
        DirectGatewaySnapshot {
            fallback_total: self.fallback_total.load(Ordering::Relaxed),
            fallback_unavailable: self.fallbacks[0].load(Ordering::Relaxed),
            fallback_worker_failed: self.fallbacks[1].load(Ordering::Relaxed),
            fallback_flow_capacity: self.fallbacks[2].load(Ordering::Relaxed),
            fallback_setup_failed: self.fallbacks[3].load(Ordering::Relaxed),
            fallback_unsupported_target: self.fallbacks[4].load(Ordering::Relaxed),
            fallback_rewrite_failed: self.fallbacks[5].load(Ordering::Relaxed),
            flow_failures: self.flow_failures.load(Ordering::Relaxed),
            return_queue_drop_packets: self.return_queue_drop_packets.load(Ordering::Relaxed),
            return_queue_drop_bytes: self.return_queue_drop_bytes.load(Ordering::Relaxed),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const REASONS: [DirectFallbackReason; 6] = [
        DirectFallbackReason::Unavailable,
        DirectFallbackReason::WorkerFailed,
        DirectFallbackReason::FlowCapacity,
        DirectFallbackReason::SetupFailed,
        DirectFallbackReason::UnsupportedTarget,
        DirectFallbackReason::RewriteFailed,
    ];

    #[test]
    fn fallback_flow_failures_and_packet_drops_have_distinct_numeric_counters() {
        let diagnostics = DirectGatewayDiagnostics::default();
        for reason in REASONS {
            diagnostics.record_fallback(reason);
        }
        diagnostics.record_flow_failure();
        diagnostics.record_return_queue_drop(1200);
        diagnostics.record_return_queue_drop(45);
        let snapshot = diagnostics.snapshot();
        assert_eq!(snapshot.fallback_total, 6);
        assert_eq!(snapshot.fallback_unavailable, 1);
        assert_eq!(snapshot.fallback_worker_failed, 1);
        assert_eq!(snapshot.fallback_flow_capacity, 1);
        assert_eq!(snapshot.fallback_setup_failed, 1);
        assert_eq!(snapshot.fallback_unsupported_target, 1);
        assert_eq!(snapshot.fallback_rewrite_failed, 1);
        assert_eq!(snapshot.flow_failures, 1);
        assert_eq!(snapshot.return_queue_drop_packets, 2);
        assert_eq!(snapshot.return_queue_drop_bytes, 1245);
        let json = snapshot.to_json();
        let fields = json.as_object().unwrap();
        assert_eq!(fields.len(), 10);
        assert!(fields.values().all(|value| value.as_u64().is_some()));
        let mut keys = fields.keys().map(String::as_str).collect::<Vec<_>>();
        keys.sort_unstable();
        assert_eq!(
            keys,
            [
                "fallback_flow_capacity",
                "fallback_rewrite_failed",
                "fallback_setup_failed",
                "fallback_total",
                "fallback_unavailable",
                "fallback_unsupported_target",
                "fallback_worker_failed",
                "flow_failures",
                "return_queue_drop_bytes",
                "return_queue_drop_packets",
            ]
        );
    }

    #[test]
    fn counters_and_suppressed_counts_saturate_without_wrapping() {
        let diagnostics = DirectGatewayDiagnostics::default();
        diagnostics
            .fallback_total
            .store(u64::MAX, Ordering::Relaxed);
        for counter in &diagnostics.fallbacks {
            counter.store(u64::MAX, Ordering::Relaxed);
        }
        diagnostics.flow_failures.store(u64::MAX, Ordering::Relaxed);
        diagnostics
            .return_queue_drop_packets
            .store(u64::MAX, Ordering::Relaxed);
        diagnostics
            .return_queue_drop_bytes
            .store(u64::MAX - 1, Ordering::Relaxed);
        for reason in REASONS {
            diagnostics.record_fallback(reason);
        }
        diagnostics.record_flow_failure();
        diagnostics.record_return_queue_drop(1200);
        assert!(
            diagnostics
                .snapshot()
                .to_json()
                .as_object()
                .unwrap()
                .values()
                .all(|value| value.as_u64() == Some(u64::MAX))
        );
        let now = Instant::now();
        let mut limit = LogLimit {
            last_logged: Some(now),
            suppressed: u64::MAX,
        };
        assert_eq!(limit.admit(now), None);
        assert_eq!(limit.admit(now + LOG_INTERVAL), Some(u64::MAX));
    }

    #[test]
    fn first_occurrence_logs_and_summaries_are_separate_per_fixed_reason() {
        let diagnostics = DirectGatewayDiagnostics::default();
        let now = Instant::now();
        assert_eq!(diagnostics.admit_log(0, now), Some(0));
        assert_eq!(diagnostics.admit_log(0, now), None);
        assert_eq!(
            diagnostics.admit_log(0, now + LOG_INTERVAL - Duration::from_nanos(1)),
            None
        );
        assert_eq!(diagnostics.admit_log(1, now), Some(0));
        assert_eq!(diagnostics.admit_log(FLOW_FAILURE_SLOT, now), Some(0));
        assert_eq!(diagnostics.admit_log(RETURN_DROP_SLOT, now), Some(0));
        assert_eq!(diagnostics.admit_log(0, now + LOG_INTERVAL), Some(2));
        assert_eq!(diagnostics.admit_log(0, now + LOG_INTERVAL), None);
        assert_eq!(diagnostics.admit_log(0, now + LOG_INTERVAL * 2), Some(1));
        for (reason, code) in REASONS.into_iter().zip([
            "direct_unavailable",
            "direct_worker_failed",
            "direct_flow_capacity",
            "direct_setup_failed",
            "direct_unsupported_target",
            "direct_rewrite_failed",
        ]) {
            assert_eq!(reason.code(), code);
        }
    }
}
