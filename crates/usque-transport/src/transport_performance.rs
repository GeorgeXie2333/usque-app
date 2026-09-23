//! Bounded, payload-free counters. Durations are monotonic microseconds.
use std::sync::atomic::{AtomicU64, Ordering};

pub(crate) fn add(counter: &AtomicU64, value: u64) {
    let _ = counter.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| {
        Some(n.saturating_add(value))
    });
}

macro_rules! counters {
    ($storage:ident, $snapshot:ident, $($field:ident),+ $(,)?) => {
        #[derive(Debug, Default)]
        pub(crate) struct $storage { $(pub(crate) $field: AtomicU64,)+ }
        #[derive(Debug, Clone, Default, PartialEq, Eq)]
        pub struct $snapshot { $(pub $field: u64,)+ }
        impl $storage {
            pub(crate) fn snapshot(&self) -> $snapshot {
                $snapshot { $($field: self.$field.load(Ordering::Relaxed),)+ }
            }
        }
        impl $snapshot {
            pub fn to_json(&self) -> serde_json::Value {
                serde_json::json!({ $(stringify!($field): self.$field,)+ })
            }
        }
    };
}

counters!(
    H2Counters,
    H2ReceivePerformance,
    data_frames,
    data_bytes,
    assembly_copy_bytes,
    batches,
    packets,
    packet_bytes,
);
counters!(
    H3Counters,
    H3SendPerformance,
    application_batches,
    application_packets,
    application_bytes,
    encode_pool_exhausted,
    datagram_queue_full,
    pmtu_deferred,
    wire_queue_full,
    quantum_limited,
    quic_no_progress_with_backlog,
    udp_would_block,
    udp_partial_sends,
    udp_message_too_large,
);

#[derive(Debug, Default)]
pub(crate) struct PerformanceCounters {
    pub(crate) h2: H2Counters,
    pub(crate) h3: H3Counters,
    pub(crate) incoming_copy_bytes: AtomicU64,
    pub(crate) send_timeouts: AtomicU64,
    pub(crate) h2_batch_sizes: [AtomicU64; 7],
    pub(crate) h3_batch_sizes: [AtomicU64; 7],
}

/// Batch buckets: 1, 2–3, 4–7, 8–15, 16–31, 32–63, 64 packets.
pub(crate) fn record_batch(buckets: &[AtomicU64; 7], packets: usize) {
    if packets != 0 {
        add(&buckets[(packets.ilog2() as usize).min(6)], 1);
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TransportPerformanceSnapshot {
    pub h2: Option<H2ReceivePerformance>,
    pub h3: Option<H3SendPerformance>,
    pub incoming_copy_bytes: u64,
    pub send_timeouts: u64,
    pub h2_batch_sizes: Vec<u64>,
    pub h3_batch_sizes: Vec<u64>,
}

impl PerformanceCounters {
    pub(crate) fn snapshot(
        &self,
        transport: Option<usque_core::Transport>,
    ) -> TransportPerformanceSnapshot {
        use usque_core::Transport;
        let h2 = transport == Some(Transport::Http2);
        let h3 = transport == Some(Transport::Http3);
        TransportPerformanceSnapshot {
            h2: h2.then(|| self.h2.snapshot()),
            h3: h3.then(|| self.h3.snapshot()),
            incoming_copy_bytes: self.incoming_copy_bytes.load(Ordering::Relaxed),
            send_timeouts: self.send_timeouts.load(Ordering::Relaxed),
            h2_batch_sizes: if h2 {
                self.h2_batch_sizes
                    .iter()
                    .map(|n| n.load(Ordering::Relaxed))
                    .collect()
            } else {
                Vec::new()
            },
            h3_batch_sizes: if h3 {
                self.h3_batch_sizes
                    .iter()
                    .map(|n| n.load(Ordering::Relaxed))
                    .collect()
            } else {
                Vec::new()
            },
        }
    }
}

impl TransportPerformanceSnapshot {
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "h2": self.h2.as_ref().map(H2ReceivePerformance::to_json),
            "h3": self.h3.as_ref().map(H3SendPerformance::to_json),
            "incoming_copy_bytes": self.incoming_copy_bytes,
            "send_timeouts": self.send_timeouts,
            "h2_batch_sizes": self.h2_batch_sizes,
            "h3_batch_sizes": self.h3_batch_sizes,
        })
    }
}

#[derive(Debug, Default)]
pub(crate) struct BackpressureCounters {
    pub(crate) waits: AtomicU64,
    pub(crate) active: AtomicU64,
    pub(crate) completed: AtomicU64,
    pub(crate) cancelled: AtomicU64,
    pub(crate) closed: AtomicU64,
    pub(crate) errors: AtomicU64,
    pub(crate) total_us: AtomicU64,
    pub(crate) max_us: AtomicU64,
    pub(crate) buckets: [AtomicU64; 32],
}

/// One histogram sample per completed/cancelled/failed *actual* capacity wait.
/// Bucket 0 is zero microseconds, bucket n is [2^(n-1), 2^n), last saturates.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct QueueBackpressureSnapshot {
    pub waits: u64,
    pub active: u64,
    pub completed: u64,
    pub cancelled: u64,
    pub closed: u64,
    pub errors: u64,
    pub total_us: u64,
    pub max_us: u64,
    pub buckets: Vec<u64>,
}

impl BackpressureCounters {
    pub(crate) fn snapshot(&self) -> QueueBackpressureSnapshot {
        QueueBackpressureSnapshot {
            waits: self.waits.load(Ordering::Relaxed),
            active: self.active.load(Ordering::Relaxed),
            completed: self.completed.load(Ordering::Relaxed),
            cancelled: self.cancelled.load(Ordering::Relaxed),
            closed: self.closed.load(Ordering::Relaxed),
            errors: self.errors.load(Ordering::Relaxed),
            total_us: self.total_us.load(Ordering::Relaxed),
            max_us: self.max_us.load(Ordering::Relaxed),
            buckets: self
                .buckets
                .iter()
                .map(|n| n.load(Ordering::Relaxed))
                .collect(),
        }
    }
}

impl QueueBackpressureSnapshot {
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "waits": self.waits, "active": self.active, "completed": self.completed,
            "cancelled": self.cancelled, "closed": self.closed, "errors": self.errors,
            "total_us": self.total_us, "max_us": self.max_us, "buckets": self.buckets,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn batch_buckets_are_bounded_and_transport_absence_is_unknown() {
        let counters = PerformanceCounters::default();
        for n in [1, 2, 3, 32, 63, 64] {
            record_batch(&counters.h2_batch_sizes, n);
        }
        assert_eq!(
            counters
                .snapshot(Some(usque_core::Transport::Http2))
                .h2_batch_sizes,
            [1, 2, 0, 0, 0, 2, 1]
        );
        let unavailable = counters.snapshot(None);
        assert!(unavailable.h2.is_none() && unavailable.h3.is_none());
        assert!(unavailable.to_json()["h2"].is_null());
    }
}
