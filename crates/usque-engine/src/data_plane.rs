use usque_core::{DataPlaneMode, L4Snapshot};
use usque_ipc::v1;

use crate::ControlServiceError;

pub(crate) fn from_proto(value: i32) -> Result<DataPlaneMode, ControlServiceError> {
    match v1::DataPlaneMode::try_from(value) {
        Ok(v1::DataPlaneMode::Unspecified | v1::DataPlaneMode::ConnectIp) => {
            Ok(DataPlaneMode::ConnectIp)
        }
        Ok(v1::DataPlaneMode::L4Proxy) => Ok(DataPlaneMode::L4Proxy),
        Err(_) => Err(ControlServiceError::InvalidRequest(
            "unknown data plane".to_owned(),
        )),
    }
}

pub(crate) const fn to_proto(mode: DataPlaneMode) -> i32 {
    match mode {
        DataPlaneMode::ConnectIp => v1::DataPlaneMode::ConnectIp as i32,
        DataPlaneMode::L4Proxy => v1::DataPlaneMode::L4Proxy as i32,
    }
}

pub(crate) fn snapshot_to_proto(value: &L4Snapshot) -> v1::L4Snapshot {
    v1::L4Snapshot {
        connect_verified: value.connect_verified,
        sessions: value.sessions,
        draining_sessions: value.draining_sessions,
        active_flows: value.active_flows,
        pending_flows: value.pending_flows,
        connect_successes: value.connect_successes,
        connect_failures: value.connect_failures,
        connect_timeouts: value.connect_timeouts,
        buffer_bytes: value.buffer_bytes,
        budget_rejections: value.budget_rejections,
        send_backpressure: value.send_backpressure,
        receive_backpressure: value.receive_backpressure,
        udp_rejected: value.udp_rejected,
        dns_successes: value.dns_successes,
        dns_failures: value.dns_failures,
        dns_timeouts: value.dns_timeouts,
        migration_preserved_flows: value.migration_preserved_flows,
        reconnect_terminated_flows: value.reconnect_terminated_flows,
        tun_flows: value.tun_flows,
        half_open_flows: value.half_open_flows,
        connect_latency_us: value.connect_latency_us,
        unsupported_packets: value.unsupported_packets,
        performance: value.performance.as_ref().map(performance_to_proto),
    }
}

fn performance_to_proto(value: &usque_core::L4PerformanceSnapshot) -> v1::L4PerformanceSnapshot {
    v1::L4PerformanceSnapshot {
        h3_read_calls: value.h3_read_calls,
        h3_read_bytes: value.h3_read_bytes,
        h3_empty_reads: value.h3_empty_reads,
        receive_pool_allocations: value.receive_pool_allocations,
        receive_pool_hits: value.receive_pool_hits,
        receive_pool_evictions: value.receive_pool_evictions,
        receive_pool_idle_bytes: value.receive_pool_idle_bytes,
        receive_pool_idle_high_watermark: value.receive_pool_idle_high_watermark,
        receive_pool_live_bytes: value.receive_pool_live_bytes,
        receive_pool_live_high_watermark: value.receive_pool_live_high_watermark,
        adapter_copied_bytes: value.adapter_copied_bytes,
        tcp_accepted_bytes: value.tcp_accepted_bytes,
        tcp_write_calls: value.tcp_write_calls,
        tcp_partial_writes: value.tcp_partial_writes,
        actor_wakeups: value.actor_wakeups,
        actor_polls: value.actor_polls,
        actor_no_progress_polls: value.actor_no_progress_polls,
        actor_no_progress_wakeups: value.actor_no_progress_wakeups,
        budget_wakeups: value.budget_wakeups,
        tun_ingress_packets: value.tun_ingress_packets,
        tun_ingress_bytes: value.tun_ingress_bytes,
        tun_egress_packets: value.tun_egress_packets,
        tun_egress_bytes: value.tun_egress_bytes,
        tun_write_calls: value.tun_write_calls,
        tun_write_would_block: value.tun_write_would_block,
        udp_receive_buffer_bytes: value.udp_receive_buffer_bytes,
        udp_send_buffer_bytes: value.udp_send_buffer_bytes,
        udp_buffer_source: value.udp_buffer_source.clone(),
        tun_mtu: value.tun_mtu,
        tun_mtu_source: value.tun_mtu_source.clone(),
        tcp_preferred_sockets: value.tcp_preferred_sockets,
        tcp_fallback_sockets: value.tcp_fallback_sockets,
        tcp_buffer_bytes: value.tcp_buffer_bytes,
        command_wait: Some(wait_to_proto(&value.command_wait)),
        tun_write_wait: Some(wait_to_proto(&value.tun_write_wait)),
        tun_ingress_queue: value.tun_ingress_queue.as_ref().map(queue_to_proto),
        tun_egress_queue: value.tun_egress_queue.as_ref().map(queue_to_proto),
        stack_ingress_queue: value.stack_ingress_queue.as_ref().map(queue_to_proto),
        stack_egress_queue: value.stack_egress_queue.as_ref().map(queue_to_proto),
    }
}
fn wait_to_proto(value: &usque_core::L4WaitSnapshot) -> v1::L4WaitSnapshot {
    v1::L4WaitSnapshot {
        samples: value.samples,
        sum_us: value.sum_us,
        max_us: value.max_us,
        buckets: value.buckets.to_vec(),
    }
}
fn queue_to_proto(value: &usque_core::L4QueueSnapshot) -> v1::L4QueueSnapshot {
    v1::L4QueueSnapshot {
        packets: value.packets,
        bytes: value.bytes,
        high_water_packets: value.high_water_packets,
        high_water_bytes: value.high_water_bytes,
        wait: Some(wait_to_proto(&value.wait)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn performance_is_additive_and_missing_observations_remain_unknown() {
        let mut legacy = L4Snapshot::default();
        assert!(snapshot_to_proto(&legacy).performance.is_none());
        let old = serde_json::to_value(&legacy).unwrap();
        assert!(old.get("performance").is_none());
        assert!(
            serde_json::from_value::<L4Snapshot>(old)
                .unwrap()
                .performance
                .is_none()
        );
        legacy.performance = Some(usque_core::L4PerformanceSnapshot::default());
        let added = snapshot_to_proto(&legacy).performance.unwrap();
        assert_eq!(added.h3_read_bytes, 0);
        assert!(added.udp_receive_buffer_bytes.is_none());
        assert!(added.tun_mtu.is_none());
        assert!(added.tun_write_calls.is_none());
        assert_eq!(added.command_wait.unwrap().buckets.len(), 32);
    }
}
