//! Asynchronous shutdown keeps its exact runtime until platform cleanup is
//! confirmed. Only another explicit Disconnect/exit retries a failed attempt.
use crate::{ActiveRuntime, ControlService, ControlServiceError, connection_error_for};
use usque_core::{ConnectionPhase, KillSwitchState, LockdownState};

pub(crate) struct ShutdownOwner {
    runtime: ActiveRuntime,
    evidence_generation: Option<u64>,
    last_error: Option<String>,
}

impl ControlService {
    pub(crate) async fn queue_runtime_shutdown(
        &self,
        mut runtime: ActiveRuntime,
        generation: Option<u64>,
    ) {
        runtime.cancel_immediately();
        #[cfg(target_os = "linux")]
        let operations = runtime.linux_cleanup_operations();
        // There is normally one owner. A queue prevents an exceptional second
        // startup-cleanup path from overwriting the first exact transaction.
        let mut owners = self.disconnect_owners.lock().await;
        owners.push_back(ShutdownOwner {
            runtime,
            evidence_generation: generation,
            last_error: None,
        });
        #[cfg(target_os = "linux")]
        self.linux_platform.lock().await.begin_cleanup(operations);
    }

    pub(crate) async fn start_queued_shutdown(&self) {
        let mut pending = self.disconnect_cleanup.lock().await;
        if pending.is_some() {
            return;
        }
        if self
            .disconnect_owners
            .lock()
            .await
            .front()
            .is_some_and(|owner| owner.last_error.is_none())
        {
            *pending = Some(self.spawn_runtime_shutdown(false));
        }
    }

    pub(crate) async fn retry_disconnect_cleanup(&self) -> bool {
        let mut pending = self.disconnect_cleanup.lock().await;
        if pending.as_ref().is_some_and(|task| !task.is_finished()) {
            return false;
        }
        if let Some(task) = pending.take() {
            let _ = task.await;
        }
        if !self.disconnect_owners.lock().await.is_empty() {
            *pending = Some(self.spawn_runtime_shutdown(true));
            true
        } else {
            false
        }
    }

    fn spawn_runtime_shutdown(
        &self,
        interactive: bool,
    ) -> tokio::task::JoinHandle<Result<(), ControlServiceError>> {
        let service = self.clone();
        tokio::spawn(async move {
            let mut owners = service.disconnect_owners.lock().await;
            while let Some(owner) = owners.front_mut() {
                #[cfg(target_os = "linux")]
                let cleaning = owner.runtime.linux_cleanup_operations();
                // Borrow the retained runtime across the await. Cancellation
                // or panic cannot drop its transaction identity with the task.
                let result = Box::pin(owner.runtime.shutdown_with_authorization(interactive)).await;
                #[cfg(target_os = "linux")]
                if result.is_ok() && owner.runtime.is_vpn() {
                    service.linux_platform.lock().await.retire(cleaning);
                }
                if let Some(generation) = owner.evidence_generation
                    && let Some(evidence) =
                        service.retained_connection_evidence.lock().await.as_mut()
                    && evidence.session_generation == generation
                {
                    evidence.timeline = owner.runtime.connection_timeline();
                    evidence.cleanup_status = if result.is_ok() {
                        "shutdown_returned"
                    } else {
                        "shutdown_failed"
                    };
                }
                if let Err(error) = result {
                    let message = error.as_structured_error().message;
                    owner.last_error = Some(message.clone());
                    let error = ControlServiceError::DisconnectCleanup(message);
                    service
                        .disconnect_cleanup_failed
                        .store(true, std::sync::atomic::Ordering::Release);
                    if owner.runtime.is_vpn() {
                        service.state.lock().await.update_safety_state(
                            KillSwitchState::Error,
                            LockdownState::NotSupported,
                        );
                    }
                    service.mark_connection_error(&error).await;
                    return Err(error);
                }
                let previous_error = owner.last_error.take();
                // Holding the same queue lock means this removes only the
                // owner just confirmed; a late task never clears a successor.
                owners.pop_front();
                if owners.is_empty() {
                    service
                        .disconnect_cleanup_failed
                        .store(false, std::sync::atomic::Ordering::Release);
                }
                if let Some(message) = previous_error {
                    let expected =
                        connection_error_for(&ControlServiceError::DisconnectCleanup(message));
                    let mut state = service.state.lock().await;
                    if state.snapshot().error.as_ref() == Some(&expected) {
                        state.transition(ConnectionPhase::Disconnecting)?;
                        state.transition(ConnectionPhase::Disconnected)?;
                    }
                }
            }
            Ok(())
        })
    }

    pub(crate) async fn await_disconnect_cleanup(&self) -> Result<(), ControlServiceError> {
        let mut pending = self.disconnect_cleanup.lock().await;
        let waited = pending.is_some();
        if let Some(task) = pending.as_mut() {
            // Keep the handle here when a Connect waiter is cancelled.
            let result = task
                .await
                .map_err(|error| ControlServiceError::DisconnectCleanup(error.to_string()));
            pending.take();
            if let Err(error) = result {
                if let Some(owner) = self.disconnect_owners.lock().await.front_mut()
                    && let ControlServiceError::DisconnectCleanup(message) = &error
                {
                    owner.last_error = Some(message.clone());
                    self.disconnect_cleanup_failed
                        .store(true, std::sync::atomic::Ordering::Release);
                }
                if self
                    .disconnect_owners
                    .lock()
                    .await
                    .front()
                    .is_some_and(|owner| owner.runtime.is_vpn())
                {
                    self.state
                        .lock()
                        .await
                        .update_safety_state(KillSwitchState::Error, LockdownState::NotSupported);
                }
                self.mark_connection_error(&error).await;
                return Err(error);
            }
            result??;
        }
        drop(pending);
        if let Some(owner) = self.disconnect_owners.lock().await.front() {
            // Consuming a completed error is never permission for the next
            // Connect/Retry to bypass an unconfirmed, still-owned operation.
            return Err(ControlServiceError::DisconnectCleanup(
                owner.last_error.clone().unwrap_or_else(|| {
                    "Previous runtime cleanup remains unconfirmed; retry Disconnect.".into()
                }),
            ));
        }
        if waited && self.data_plane.lock().await.is_none() {
            self.state
                .lock()
                .await
                .update_safety_state(KillSwitchState::NotApplicable, LockdownState::NotSupported);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
