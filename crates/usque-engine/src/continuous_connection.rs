//! The control reply acknowledges intent; the owned worker prepares the runtime.
use super::*;
use tokio_util::sync::CancellationToken;

pub(super) struct ConnectionIntent {
    enabled: bool,
    profile_id: Uuid,
    cancellation: CancellationToken,
    wake: Arc<tokio::sync::Notify>,
    task: AbortOnDropHandle<()>,
}

impl ConnectionIntent {
    pub(super) fn is_active(&self) -> bool {
        self.enabled && !self.cancellation.is_cancelled()
    }
}

impl ControlService {
    pub(super) async fn continuous_enabled(&self) -> bool {
        self.continuous_connection
            .lock()
            .await
            .as_ref()
            .is_some_and(|intent| intent.enabled)
    }

    pub(super) async fn disable_continuous(&self) {
        if let Some(intent) = self.continuous_connection.lock().await.as_mut() {
            intent.enabled = false;
            intent.cancellation.cancel();
        }
    }

    /// Startup may own an unpublished platform guard while the mutation lock is
    /// released. Join it before teardown acquires that lock or deletes state.
    pub(super) async fn retire_continuous_startup(&self) {
        let retiring = self.continuous_connection.lock().await.take();
        if let Some(intent) = retiring {
            intent.cancellation.cancel();
            let _ = intent.task.await;
        }
    }
    pub(super) async fn continuous_requested(&self, profile_id: Uuid) -> bool {
        self.continuous_connection
            .lock()
            .await
            .as_ref()
            .is_some_and(|intent| {
                intent.enabled
                    && intent.profile_id == profile_id
                    && !intent.cancellation.is_cancelled()
            })
    }

    pub(super) async fn connect_continuously(
        &self,
        profile_id: Uuid,
    ) -> Result<ConnectionSnapshot, ControlServiceError> {
        self.accept_continuous(profile_id, None).await
    }

    pub(super) async fn restart_continuously(
        &self,
        profile_id: Uuid,
        cancellation: CancellationToken,
    ) -> Result<ConnectionSnapshot, ControlServiceError> {
        self.accept_continuous(profile_id, Some(cancellation)).await
    }

    fn accept_continuous(
        &self,
        profile_id: Uuid,
        inherited: Option<CancellationToken>,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = Result<ConnectionSnapshot, ControlServiceError>>
                + Send
                + '_,
        >,
    > {
        Box::pin(async move {
            if self
                .config
                .read()
                .await
                .runtime_profile(profile_id)
                .is_none()
            {
                return Err(ControlServiceError::ProfileNotFound(profile_id));
            }
            let mut owner = self.continuous_connection.lock().await;
            let snapshot = self.state.lock().await.snapshot().clone();
            if inherited
                .as_ref()
                .is_some_and(CancellationToken::is_cancelled)
            {
                return Ok(snapshot);
            }
            if inherited.is_none()
                && let Some(intent) = owner.as_ref()
                && intent.enabled
                && intent.profile_id == profile_id
                && !intent.cancellation.is_cancelled()
                && (!intent.task.is_finished()
                    || !matches!(
                        snapshot.phase,
                        ConnectionPhase::Error | ConnectionPhase::Disconnected
                    ))
            {
                intent.wake.notify_one();
                if let Some(active) = self.data_plane.lock().await.as_ref() {
                    active.runtime.request_recovery();
                }
                return Ok(snapshot);
            }
            if let Some(active) = self.data_plane.lock().await.as_ref()
                && active.profile_id != profile_id
            {
                return Err(ControlServiceError::AlreadyConnected(active.profile_id));
            }
            let mut registered = self.gate_startup_cancel.lock().await;
            if inherited
                .as_ref()
                .is_some_and(CancellationToken::is_cancelled)
            {
                return Ok(snapshot);
            }
            let retiring = owner.take().map(|previous| {
                previous.cancellation.cancel();
                previous.task
            });
            // This explicit intent owns a new token; internal attempts reuse it.
            let cancellation = CancellationToken::new();
            *registered = cancellation.clone();
            drop(registered);
            let wake = Arc::new(tokio::sync::Notify::new());
            let worker_cancel = cancellation.clone();
            let worker_wake = wake.clone();
            let weak = Arc::downgrade(&self.inner);
            let mut new_connection = inherited.is_none();
            let snapshot = {
                let mut state = self.state.lock().await;
                let phase = if matches!(
                    state.snapshot().phase,
                    ConnectionPhase::Connected
                        | ConnectionPhase::Degraded
                        | ConnectionPhase::Reconnecting
                ) {
                    ConnectionPhase::Reconnecting
                } else {
                    ConnectionPhase::Preparing
                };
                if state.snapshot().phase != phase {
                    state.transition(phase)?;
                }
                state.snapshot().clone()
            };
            let task = tokio::spawn(async move {
                if let Some(retiring) = retiring {
                    let _ = retiring.await;
                }
                loop {
                    let Some(inner) = weak.upgrade() else {
                        return;
                    };
                    let service = ControlService { inner };
                    let result = {
                        let mutation = tokio::select! {
                            biased;
                            _ = worker_cancel.cancelled() => return,
                            guard = service.mutation_lock.clone().lock_owned() => guard,
                        };
                        if worker_cancel.is_cancelled() {
                            return;
                        }
                        if new_connection {
                            service.reset_proton_selection();
                            new_connection = false;
                        }
                        match service.stop_failed_gate_locked().await {
                            Ok(()) => {
                                let failed = service.data_plane.lock().await.as_ref().is_some_and(
                                    |active| {
                                        matches!(
                                            active.runtime.health(),
                                            RuntimeHealth::Failed { .. }
                                        )
                                    },
                                );
                                if failed {
                                    Box::pin(
                                        service.retry_connection_locked(
                                            profile_id,
                                            worker_cancel.clone(),
                                        ),
                                    )
                                    .await
                                } else {
                                    #[cfg(windows)]
                                    service.begin_windows_connection_intent(profile_id).await;
                                    service
                                        .connect_with_startup_lock(
                                            profile_id,
                                            worker_cancel.clone(),
                                            false,
                                            Some(mutation),
                                        )
                                        .await
                                }
                            }
                            Err(error) => Err(error),
                        }
                    };
                    if worker_cancel.is_cancelled() {
                        return;
                    }
                    match result {
                        Ok(_) => return,
                        Err(error) if retry_network_error(&error) => {
                            service.mark_connection_error(&error).await;
                        }
                        Err(error) => {
                            service.mark_connection_error(&error).await;
                            return;
                        }
                    }
                    drop(service);
                    tokio::select! {
                        biased;
                        _ = worker_cancel.cancelled() => return,
                        _ = worker_wake.notified() => {},
                        _ = tokio::time::sleep(usque_core::recovery::RETRY_INTERVAL) => {},
                    }
                }
            });
            *owner = Some(ConnectionIntent {
                enabled: true,
                profile_id,
                cancellation,
                wake,
                task: AbortOnDropHandle::new(task),
            });
            Ok(snapshot)
        })
    }
}

pub(super) fn retry_network_error(error: &ControlServiceError) -> bool {
    if let ControlServiceError::Transport(error) = error {
        let failure = error.failure(None, None);
        return failure.retryable
            && failure.action() != usque_core::FailureAction::Stop
            && failure.code != usque_core::TransportFailureCode::EndpointPinMismatch;
    }
    #[cfg(target_os = "linux")]
    if let ControlServiceError::LinuxPlatform {
        detail: Some(detail),
        retryable,
    } = error
    {
        return *retryable && crate::linux_agent::unavailable_physical_network(detail);
    }
    let error = error.as_structured_error();
    error.retryable
        && matches!(
            error.code.as_str(),
            "AGENT_PHYSICAL_NETWORK_OFFLINE"
                | "AGENT_PHYSICAL_NETWORK_UNAVAILABLE"
                | "PHYSICAL_IPV4_UNAVAILABLE"
                | "PHYSICAL_IPV6_UNAVAILABLE"
                | "PHYSICAL_DNS_UNAVAILABLE"
                | "PHYSICAL_NETWORK_CHANGED"
                | "DNS_UNAVAILABLE"
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn automatic_restart_keeps_proton_affinity_and_new_user_intent_resets_it() {
        let directory = tempfile::tempdir().unwrap();
        // An empty vault stops each worker before any network startup.
        let service = ControlService::open_with_vault(
            ConfigStore::new(directory.path().join("config.json")),
            Arc::new(crate::tests::MemoryVault::default()),
        )
        .unwrap();
        let id = service.config_snapshot().await.active_profile_id.unwrap();
        let account = Uuid::new_v4();
        async fn finished(service: &ControlService) {
            tokio::time::timeout(Duration::from_secs(2), async {
                loop {
                    if service
                        .continuous_connection
                        .lock()
                        .await
                        .as_ref()
                        .is_some_and(|intent| intent.task.is_finished())
                    {
                        return;
                    }
                    tokio::task::yield_now().await;
                }
            })
            .await
            .unwrap();
        }
        service.connect_continuously(id).await.unwrap();
        finished(&service).await;
        let connection = service.proton_selection.lock().unwrap().clone();
        connection.remember(account, None, "chosen");
        let token = service.gate_startup_cancel.lock().await.clone();
        service.restart_continuously(id, token).await.unwrap();
        finished(&service).await;
        assert_eq!(
            service
                .proton_selection
                .lock()
                .unwrap()
                .selected(account, None)
                .as_deref(),
            Some("chosen")
        );

        service.connect_continuously(id).await.unwrap();
        finished(&service).await;
        connection.remember(account, None, "late-old-worker");
        assert_eq!(
            service
                .proton_selection
                .lock()
                .unwrap()
                .selected(account, None),
            None
        );
        service.retire_continuous_startup().await;
    }

    #[tokio::test]
    async fn accepted_intent_does_not_wait_for_mutation_lock_and_stop_revokes_it() {
        let directory = tempfile::tempdir().unwrap();
        let service = ControlService::open_with_vault(
            ConfigStore::new(directory.path().join("config.json")),
            Arc::new(crate::tests::MemoryVault::default()),
        )
        .unwrap();
        let id = service.config_snapshot().await.active_profile_id.unwrap();
        let guard = service.mutation_lock.lock().await;
        let accepted =
            tokio::time::timeout(Duration::from_millis(100), service.connect_continuously(id))
                .await
                .unwrap()
                .unwrap();
        assert_eq!(accepted.phase, ConnectionPhase::Preparing);
        assert!(accepted.transport.is_none());
        let token = service.gate_startup_cancel.lock().await.clone();
        service.connect_continuously(id).await.unwrap();
        assert!(!token.is_cancelled());
        token.cancel();
        drop(guard);
        service.disconnect().await.unwrap();
        tokio::task::yield_now().await;
        assert_eq!(
            service.status_snapshot().await.phase,
            ConnectionPhase::Disconnected
        );
        assert!(service.data_plane.lock().await.is_none());
    }

    #[tokio::test]
    async fn stop_cancels_an_intent_registered_while_stop_was_waiting() {
        let directory = tempfile::tempdir().unwrap();
        let service = ControlService::open_with_vault(
            ConfigStore::new(directory.path().join("config.json")),
            Arc::new(crate::tests::MemoryVault::default()),
        )
        .unwrap();
        let id = service.config_snapshot().await.active_profile_id.unwrap();
        let previous = service.gate_startup_cancel.lock().await.clone();
        let mutation = service.mutation_lock.lock().await;
        let publication = service.data_plane.lock().await;
        let request = service.clone();
        let connecting = tokio::spawn(async move { request.connect_continuously(id).await });
        while service.continuous_connection.try_lock().is_ok() {
            tokio::task::yield_now().await;
        }
        let request = service.clone();
        let stopping = tokio::spawn(async move { request.disconnect().await });
        previous.cancelled().await;
        drop(publication);
        connecting.await.unwrap().unwrap();
        let current = service.gate_startup_cancel.lock().await.clone();
        tokio::time::timeout(Duration::from_secs(1), current.cancelled())
            .await
            .unwrap();
        drop(mutation);
        stopping.await.unwrap().unwrap();
        service.retire_continuous_startup().await;
        assert_eq!(
            service.status_snapshot().await.phase,
            ConnectionPhase::Disconnected
        );
        assert!(service.data_plane.lock().await.is_none());
    }

    #[tokio::test]
    async fn shutdown_waits_for_unpublished_startup_cleanup_without_holding_mutation() {
        let directory = tempfile::tempdir().unwrap();
        let service = ControlService::open_with_vault(
            ConfigStore::new(directory.path().join("config.json")),
            Arc::new(crate::tests::MemoryVault::default()),
        )
        .unwrap();
        let profile_id = service.config_snapshot().await.active_profile_id.unwrap();
        let cancellation = service.gate_startup_cancel.lock().await.clone();
        let worker_cancel = cancellation.clone();
        let (entered, observed) = tokio::sync::oneshot::channel();
        let (release, cleanup) = tokio::sync::oneshot::channel();
        let worker = service.clone();
        let task = tokio::spawn(async move {
            worker_cancel.cancelled().await;
            entered.send(()).unwrap();
            cleanup.await.unwrap();
            let _mutation = worker.mutation_lock.lock().await;
        });
        *service.continuous_connection.lock().await = Some(ConnectionIntent {
            enabled: true,
            profile_id,
            cancellation,
            wake: Arc::new(tokio::sync::Notify::new()),
            task: AbortOnDropHandle::new(task),
        });
        let request = service.clone();
        let shutdown = tokio::spawn(async move { request.shutdown().await });
        observed.await.unwrap();
        tokio::task::yield_now().await;
        assert!(!shutdown.is_finished());
        release.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(2), shutdown)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(service.continuous_connection.lock().await.is_none());
    }
}
