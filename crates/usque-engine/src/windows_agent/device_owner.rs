use super::*;

/// Owned by ControlService, never by WindowsVpnRuntime. Acquiring ownership
/// does not create a device; only a subsequent Prepare may do that.
#[derive(Default)]
pub(crate) struct WindowsDeviceOwner {
    state: tokio::sync::Mutex<DeviceOwnerState>,
}

#[derive(Default)]
struct DeviceOwnerState {
    closed: bool,
    held: Option<HeldDevice>,
}

struct HeldDevice {
    client: WindowsAgentClient,
    pipe: NamedPipeClient,
    grant: agent_v1::DeviceLease,
}

impl WindowsDeviceOwner {
    pub(super) async fn acquire(
        &self,
        client: &WindowsAgentClient,
        capabilities: &AgentCapabilities,
    ) -> Result<agent_v1::DeviceLease, WindowsVpnError> {
        if !capabilities.reusable_tun_device {
            return Err(WindowsVpnError::DeviceReuseUnsupported);
        }
        let mut state = self.state.lock().await;
        if state.closed {
            return Err(TransportError::TunnelClosed.into());
        }
        if let Some(held) = &state.held {
            let mut byte = [0];
            if held.client.pipe_name == client.pipe_name
                && matches!(held.pipe.try_read(&mut byte), Err(error) if error.kind() == io::ErrorKind::WouldBlock)
            {
                return Ok(held.grant.clone());
            }
        }
        // EOF invalidates this Agent's grant. The new Agent must independently
        // restore or validate its journal before granting another device lease.
        state.held = None;
        let mut pipe = client.open_pipe().await?;
        let payload = timeout(
            AGENT_RPC_TIMEOUT,
            client.exchange(
                &mut pipe,
                agent_request::Payload::AcquireDeviceLease(agent_v1::AcquireDeviceLeaseRequest {}),
            ),
        )
        .await
        .map_err(|_| WindowsVpnError::RpcTimeout)??;
        let agent_response::Payload::DeviceLease(grant) = payload else {
            return Err(WindowsVpnError::UnexpectedResponse(payload_name(&payload)));
        };
        if Uuid::parse_str(&grant.lease_id)
            .ok()
            .is_none_or(|id| id.is_nil())
            || grant.lease_generation == 0
        {
            return Err(WindowsVpnError::InvalidDeviceLease);
        }
        state.held = Some(HeldDevice {
            client: client.clone(),
            pipe,
            grant: grant.clone(),
        });
        Ok(grant)
    }

    /// Runs even when connection shutdown failed: the Agent retains network
    /// cleanup ownership and may defer exit only after network effects are gone.
    pub(crate) async fn shutdown(&self) -> Result<(), WindowsVpnError> {
        let mut state = self.state.lock().await;
        state.closed = true;
        let Some(mut held) = state.held.take() else {
            return Ok(());
        };
        let response = timeout(
            AGENT_RPC_TIMEOUT,
            held.client.exchange(
                &mut held.pipe,
                agent_request::Payload::ReleaseDeviceLease(agent_v1::ReleaseDeviceLeaseRequest {
                    lease_id: held.grant.lease_id,
                    lease_generation: held.grant.lease_generation,
                }),
            ),
        )
        .await
        .map_err(|_| WindowsVpnError::RpcTimeout)
        .and_then(|result| result);
        let _ = held.pipe.shutdown().await;
        let payload = response?;
        let agent_response::Payload::State(reply) = payload else {
            return Err(WindowsVpnError::UnexpectedResponse(payload_name(&payload)));
        };
        require_connection_clean(&reply)?;
        match reply.device {
            Some(device)
                if !device.lease_attached
                    && matches!(
                        agent_v1::ManagedDevicePhase::try_from(device.phase),
                        Ok(agent_v1::ManagedDevicePhase::Absent
                            | agent_v1::ManagedDevicePhase::RecoveryRequired)
                    ) =>
            {
                Ok(())
            }
            _ => Err(WindowsVpnError::DeviceRecoveryRequired),
        }
    }
}

fn require_connection_clean(state: &AgentState) -> Result<(), WindowsVpnError> {
    if state.phase == agent_v1::AgentPhase::Clean as i32
        && !state.packet_session_active
        && !state.kill_switch_active
        && !state.system_proxy_active
        && state.operation_id.is_empty()
        && state.profile_id.is_empty()
    {
        Ok(())
    } else {
        Err(WindowsVpnError::RecoveryFailed)
    }
}

pub(super) fn require_idle_device(state: &AgentState) -> Result<(), WindowsVpnError> {
    if state.device.as_ref().is_some_and(|device| {
        !matches!(
            agent_v1::ManagedDevicePhase::try_from(device.phase),
            Ok(agent_v1::ManagedDevicePhase::Absent | agent_v1::ManagedDevicePhase::Idle)
        )
    }) {
        return Err(WindowsVpnError::DeviceRecoveryRequired);
    }
    Ok(())
}
