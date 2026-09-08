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
}
