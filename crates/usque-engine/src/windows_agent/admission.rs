//! Bounded, read-only admission across an Agent service retirement boundary.
use super::*;
use std::future::Future;

impl WindowsAgentClient {
    pub(super) async fn admission_preflight(
        &self,
        restart_exhausted: bool,
    ) -> Result<Option<AutomaticRecoveryObservation>, WindowsVpnError> {
        timeout(AGENT_RECOVERY_TIMEOUT, async {
            let capabilities = self.get_capabilities().await?;
            if capabilities.protocol_version != AGENT_PROTOCOL_VERSION {
                return Err(WindowsVpnError::ProtocolVersion(
                    capabilities.protocol_version,
                ));
            }
            self.automatic_recovery_preflight(&capabilities, restart_exhausted)
                .await
        })
        .await
        .map_err(|_| WindowsVpnError::RecoveryTimeout)?
    }

    async fn admission_read<T, Read, Reading>(
        &self,
        budget: Duration,
        mut read: Read,
    ) -> Result<T, WindowsVpnError>
    where
        Read: FnMut(bool) -> Reading,
        Reading: Future<Output = Result<T, WindowsVpnError>>,
    {
        timeout(budget, async {
            let mut retrying = false;
            loop {
                match read(retrying).await {
                    Ok(value) => return Ok(value),
                    Err(error) => {
                        if !self.admission_read_can_retry(&error, retrying).await {
                            return Err(error);
                        }
                        // Each read owns and drops its pipe before this delay.
                        // An exiting Agent must be allowed to reach clean idle.
                        sleep(AGENT_RECOVERY_POLL_INTERVAL).await;
                        retrying = true;
                    }
                }
            }
        })
        .await
        .map_err(|_| WindowsVpnError::RecoveryTimeout)?
    }

    async fn admission_read_can_retry(&self, error: &WindowsVpnError, retrying: bool) -> bool {
        match error {
            WindowsVpnError::Remote { code, .. } => {
                matches!(
                    code.as_str(),
                    "AGENT_SHUTTING_DOWN" | "AGENT_DEVICE_RETIRING"
                )
            }
            WindowsVpnError::Io(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::BrokenPipe
                        | io::ErrorKind::UnexpectedEof
                        | io::ErrorKind::ConnectionReset
                        | io::ErrorKind::ConnectionAborted
                        | io::ErrorKind::NotConnected
                ) =>
            {
                retrying
                    || self.service_controller.status().await.is_ok_and(|status| {
                        matches!(
                            status.state,
                            SERVICE_STOP_PENDING | SERVICE_STOPPED | SERVICE_START_PENDING
                        )
                    })
            }
            // Invalid framing, caller/owner mismatches and persistent recovery
            // faults are terminal. No privileged mutation is ever replayed here.
            _ => false,
        }
    }

    pub(super) async fn get_admission_capabilities(
        &self,
    ) -> Result<AgentCapabilities, WindowsVpnError> {
        self.admission_read(AGENT_RECOVERY_TIMEOUT, |_| self.get_capabilities_once())
            .await
    }

    pub(super) async fn get_admission_state(
        &self,
        expected: &AgentCapabilities,
    ) -> Result<AgentState, WindowsVpnError> {
        self.get_admission_state_with_budget(expected, AGENT_RECOVERY_TIMEOUT)
            .await
    }

    pub(super) async fn get_admission_state_with_budget(
        &self,
        expected: &AgentCapabilities,
        budget: Duration,
    ) -> Result<AgentState, WindowsVpnError> {
        self.admission_read(budget, |retrying| async move {
            if retrying {
                let current = self.get_capabilities_once().await?;
                if current.protocol_version != AGENT_PROTOCOL_VERSION {
                    return Err(WindowsVpnError::ProtocolVersion(current.protocol_version));
                }
                // A replacement service cannot silently change the negotiated
                // protection contract. A later explicit Connect negotiates anew.
                if &current != expected {
                    return Err(WindowsVpnError::MissingCapabilities(
                        "Agent capabilities changed during startup".into(),
                    ));
                }
            }
            self.get_state().await
        })
        .await
    }
}
