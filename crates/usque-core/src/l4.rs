//! Export-safe L4 metrics. Counters never contain traffic or destination data.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct L4Snapshot {
    pub connect_verified: bool,
    pub sessions: u32,
    pub draining_sessions: u32,
    pub active_flows: u32,
    pub pending_flows: u32,
    pub connect_successes: u64,
    pub connect_failures: u64,
    pub connect_timeouts: u64,
    pub buffer_bytes: u64,
    pub budget_rejections: u64,
    pub send_backpressure: u64,
    pub receive_backpressure: u64,
    pub udp_rejected: u64,
    pub dns_successes: u64,
    pub dns_failures: u64,
    pub dns_timeouts: u64,
    pub migration_preserved_flows: u64,
    pub reconnect_terminated_flows: u64,
    pub tun_flows: u32,
    pub half_open_flows: u32,
    pub connect_latency_us: u64,
    pub unsupported_packets: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub performance: Option<L4PerformanceSnapshot>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct L4PerformanceSnapshot {
    pub h3_read_calls: u64,
    pub h3_read_bytes: u64,
    pub h3_empty_reads: u64,
    pub receive_pool_allocations: u64,
    pub receive_pool_hits: u64,
    pub receive_pool_evictions: u64,
    pub receive_pool_idle_bytes: u64,
    pub receive_pool_idle_high_watermark: u64,
    pub receive_pool_live_bytes: u64,
    pub receive_pool_live_high_watermark: u64,
    pub adapter_copied_bytes: u64,
    pub tcp_accepted_bytes: u64,
    pub tcp_write_calls: u64,
    pub tcp_partial_writes: u64,
    pub actor_wakeups: u64,
    pub actor_polls: u64,
    pub actor_no_progress_polls: u64,
    pub actor_no_progress_wakeups: u64,
    pub budget_wakeups: u64,
    pub tun_ingress_packets: u64,
    pub tun_ingress_bytes: u64,
    pub tun_egress_packets: u64,
    pub tun_egress_bytes: u64,
    pub tun_write_calls: Option<u64>,
    pub tun_write_would_block: Option<u64>,
    pub udp_receive_buffer_bytes: Option<u64>,
    pub udp_send_buffer_bytes: Option<u64>,
    pub udp_buffer_source: Option<String>,
    pub tun_mtu: Option<u32>,
    pub tun_mtu_source: Option<String>,
    pub tcp_preferred_sockets: Option<u64>,
    pub tcp_fallback_sockets: Option<u64>,
    pub tcp_buffer_bytes: Option<u64>,
    pub command_wait: L4WaitSnapshot,
    pub tun_write_wait: L4WaitSnapshot,
    pub tun_ingress_queue: Option<L4QueueSnapshot>,
    pub tun_egress_queue: Option<L4QueueSnapshot>,
    pub stack_ingress_queue: Option<L4QueueSnapshot>,
    pub stack_egress_queue: Option<L4QueueSnapshot>,
}

/// Logarithmic microsecond buckets; no raw timings or destination identifiers.
/// Bucket 0 is zero, bucket i covers [2^(i-1), 2^i-1], and bucket 31 saturates.
/// Zero samples means that no percentile is available.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct L4WaitSnapshot {
    pub samples: u64,
    pub sum_us: u64,
    pub max_us: u64,
    pub buckets: [u64; 32],
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct L4QueueSnapshot {
    pub packets: u64,
    pub bytes: u64,
    pub high_water_packets: u64,
    pub high_water_bytes: u64,
    pub wait: L4WaitSnapshot,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NativeBuildInfo {
    pub version: String,
    pub architecture: String,
    pub debug_assertions: bool,
}
impl NativeBuildInfo {
    pub fn current() -> Self {
        Self {
            version: env!("CARGO_PKG_VERSION").to_owned(),
            architecture: std::env::consts::ARCH.to_owned(),
            debug_assertions: cfg!(debug_assertions),
        }
    }
}
