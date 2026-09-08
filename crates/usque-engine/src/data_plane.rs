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
    }
}
