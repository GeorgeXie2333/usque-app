//! The control reply acknowledges intent; the owned worker prepares the runtime.
use super::*;
use tokio_util::sync::CancellationToken;

pub(super) struct ConnectionIntent {
    enabled: bool,
    profile_id: Uuid,
    cancellation: CancellationToken,
    wake: Arc<tokio::sync::Notify>,
    #[cfg(target_os = "macos")]
    macos_agent_dial: std::sync::Weak<Profile>,
    task: AbortOnDropHandle<()>,
}

impl ConnectionIntent {
    pub(super) fn is_active(&self) -> bool {
        self.enabled && !self.cancellation.is_cancelled()
    }
}

impl ControlService {
    #[cfg(target_os = "macos")]
    pub(super) async fn track_macos_agent_dial(
        &self,
        request: Option<(&tokio::sync::Notify, Profile)>,
    ) -> Option<Arc<Profile>> {
        let (wake, profile) = request?;
        let mut owner = self.continuous_connection.lock().await;
        let intent = owner
            .as_mut()
            .filter(|intent| intent.is_active() && std::ptr::eq(wake, intent.wake.as_ref()))?;
        let profile = Arc::new(profile);
        intent.macos_agent_dial = Arc::downgrade(&profile);
        Some(profile)
    }

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
        // The state owns the join across caller cancellation. Taking an
        // AbortOnDropHandle into an RPC waiter would abort unpublished startup
        // when that waiter timed out, losing its chance to register cleanup.
        let mut retirement = self.continuous_retirement.lock().await;
        if let Some(task) = retirement.as_mut() {
            let _ = task.await;
            retirement.take();
        }
        let retiring = self.continuous_connection.lock().await.take();
        if let Some(intent) = retiring {
            intent.cancellation.cancel();
            *retirement = Some(tokio::spawn(async move {
                let _ = intent.task.await;
            }));
            if let Some(task) = retirement.as_mut() {
                let _ = task.await;
            }
            retirement.take();
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
        self.accept_continuous(profile_id, None, false).await
    }

    pub(super) async fn retry_continuously(
        &self,
        profile_id: Uuid,
    ) -> Result<ConnectionSnapshot, ControlServiceError> {
        self.accept_continuous(profile_id, None, true).await
    }

    pub(super) async fn restart_continuously(
        &self,
        profile_id: Uuid,
        cancellation: CancellationToken,
    ) -> Result<ConnectionSnapshot, ControlServiceError> {
        self.accept_continuous(profile_id, Some(cancellation), false)
            .await
    }

    fn accept_continuous(
        &self,
        profile_id: Uuid,
        inherited: Option<CancellationToken>,
        explicit_retry: bool,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = Result<ConnectionSnapshot, ControlServiceError>>
                + Send
                + '_,
        >,
    > {
        Box::pin(async move {
            let mut requested = self
                .config
                .read()
                .await
                .runtime_profile(profile_id)
                .ok_or(ControlServiceError::ProfileNotFound(profile_id))?;
            self.hydrate_profile_subscription_endpoint(&mut requested)
                .await?;
            let mut owner = self.continuous_connection.lock().await;
            let snapshot = self.state.lock().await.snapshot().clone();
            if inherited
                .as_ref()
                .is_some_and(CancellationToken::is_cancelled)
            {
                return Ok(snapshot);
            }
            // A continuous Mac Agent dial already owns the protected transaction.
            // Wake its carrier without retiring the worker that holds the Agent connection.
            #[cfg(target_os = "macos")]
            if explicit_retry
                && let Some(intent) = owner.as_ref()
                && intent.is_active()
                && intent.macos_agent_dial.upgrade().as_deref() == Some(&requested)
            {
                intent.wake.notify_one();
                return Ok(snapshot);
            }
            if inherited.is_none()
                && !explicit_retry
                && let Some(intent) = owner.as_ref()
                && intent.enabled
                && intent.profile_id == requested.id
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
            let allow_protected_handoff = explicit_retry || self.protected_chain_present().await;
            let superseded_generation = match self.data_plane.lock().await.as_ref() {
                Some(active) if active.profile_id != profile_id && !allow_protected_handoff => {
                    return Err(ControlServiceError::AlreadyConnected(active.profile_id));
                }
                Some(active) if explicit_retry || active.profile_id != profile_id => {
                    active.session_generation
                }
                None if explicit_retry => self.session_generation.load(Ordering::Acquire),
                _ => 0,
            };
            let mut registered = self.gate_startup_cancel.lock().await;
            if inherited
                .as_ref()
                .is_some_and(CancellationToken::is_cancelled)
            {
                return Ok(snapshot);
            }
            let snapshot = {
                let mut state = self.state.lock().await;
                let phase = if matches!(
                    state.snapshot().phase,
                    ConnectionPhase::ConnectingHttp3
                        | ConnectionPhase::ConnectingHttp2
                        | ConnectionPhase::Connected
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
                self.superseded_session_generation
                    .store(superseded_generation, Ordering::Release);
                state.snapshot().clone()
            };
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
            let mut restart_exhausted_recovery = inherited.is_none();
            let mut force_retry_once = explicit_retry;
            #[cfg(target_os = "macos")]
            let mut interactive = usque_platform::macos_keychain::interaction_allowed();
            let task = tokio::spawn(async move {
                if let Some(retiring) = retiring {
                    let _ = retiring.await;
                }
                loop {
                    let Some(inner) = weak.upgrade() else {
                        return;
                    };
                    let service = ControlService { inner };
                    let mutation = tokio::select! {
                        biased;
                        _ = worker_cancel.cancelled() => return,
                        guard = service.mutation_lock.clone().lock_owned() => guard,
                    };
                    if worker_cancel.is_cancelled() {
                        return;
                    }
                    let attempt = async {
                        if new_connection {
                            service.reset_proton_selection();
                            new_connection = false;
                        }
                        let restart_exhausted = std::mem::take(&mut restart_exhausted_recovery);
                        let force_retry = std::mem::take(&mut force_retry_once);
                        match service.stop_failed_gate_locked().await {
                            Ok(()) => {
                                let should_retry_session = {
                                    let data_plane = service.data_plane.lock().await;
                                    data_plane.as_ref().is_some_and(|active| {
                                        force_retry
                                            || active.profile_id != profile_id
                                            || matches!(
                                                active.runtime.health(),
                                                RuntimeHealth::Failed { .. }
                                            )
                                    })
                                };
                                if should_retry_session {
                                    Box::pin(service.retry_connection_step_locked(
                                        profile_id,
                                        worker_cancel.clone(),
                                        restart_exhausted,
                                        Some(mutation),
                                        #[cfg(target_os = "macos")]
                                        Some(&worker_wake),
                                    ))
                                    .await
                                } else {
                                    if force_retry {
                                        *service.session_congestion_control.lock().await = None;
                                        *service.session_profile.lock().await = None;
                                    }
                                    #[cfg(windows)]
                                    let intent_generation =
                                        service.begin_windows_connection_intent(profile_id).await;
                                    let result = service
                                        .connect_with_startup_lock(
                                            profile_id,
                                            worker_cancel.clone(),
                                            restart_exhausted,
                                            Some(mutation),
                                            #[cfg(target_os = "macos")]
                                            Some(&worker_wake),
                                        )
                                        .await;
                                    #[cfg(windows)]
                                    if result.is_err() {
                                        service
                                            .clear_windows_connection_intent_if(intent_generation)
                                            .await;
                                    }
                                    result
                                }
                            }
                            Err(error) => Err(error),
                        }
                    };
                    #[cfg(target_os = "macos")]
                    let attempt = usque_platform::macos_keychain::INTERACTIVE
                        .scope(std::mem::take(&mut interactive), attempt);
                    let result = attempt.await;
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
                #[cfg(target_os = "macos")]
                macos_agent_dial: Default::default(),
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
            #[cfg(target_os = "macos")]
            macos_agent_dial: Default::default(),
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

    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn confirmed_macos_disconnect_joins_unpublished_startup_before_acknowledging() {
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
            // A real unpublished startup reacquires this lock before handing
            // its cancelled runtime to retained cleanup. Quit must not hold
            // it while joining startup, or either acknowledge before it ends.
            let _mutation = worker.mutation_lock.lock().await;
            let profile = worker.config_snapshot().await.active_profile().unwrap();
            worker
                .queue_runtime_shutdown(
                    ActiveRuntime::Harness(Box::new(active_runtime::HarnessRuntime::from_profile(
                        &profile, false, 0,
                    ))),
                    None,
                )
                .await;
        });
        *service.continuous_connection.lock().await = Some(ConnectionIntent {
            enabled: true,
            profile_id,
            cancellation,
            wake: Arc::new(tokio::sync::Notify::new()),
            macos_agent_dial: Default::default(),
            task: AbortOnDropHandle::new(task),
        });
        let request = service.clone();
        let stopping = tokio::spawn(async move { request.disconnect_confirmed().await });
        observed.await.unwrap();
        tokio::task::yield_now().await;
        assert!(!stopping.is_finished());
        // A timed-out/cancelled IPC waiter must not abort the unpublished
        // startup. The following Quit still waits for its retained join.
        stopping.abort();
        assert!(stopping.await.unwrap_err().is_cancelled());
        assert!(service.continuous_retirement.lock().await.is_some());
        let request = service.clone();
        let stopping = tokio::spawn(async move { request.disconnect_confirmed().await });
        tokio::task::yield_now().await;
        assert!(!stopping.is_finished());
        release.send(()).unwrap();
        let snapshot = tokio::time::timeout(Duration::from_secs(2), stopping)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(snapshot.phase, ConnectionPhase::Disconnected);
        assert!(service.continuous_connection.lock().await.is_none());
        assert!(service.continuous_retirement.lock().await.is_none());
        assert!(service.disconnect_owners.lock().await.is_empty());
    }

    #[tokio::test]
    async fn retry_during_handshake_transitions_to_reconnecting_without_orphaning_worker() {
        for handshake_phase in [
            ConnectionPhase::ConnectingHttp3,
            ConnectionPhase::ConnectingHttp2,
        ] {
            let directory = tempfile::tempdir().unwrap();
            let service = ControlService::open_with_vault(
                ConfigStore::new(directory.path().join("config.json")),
                Arc::new(crate::tests::MemoryVault::default()),
            )
            .unwrap();
            let profile_id = service.config_snapshot().await.active_profile_id.unwrap();
            let mutation = service.mutation_lock.lock().await;
            service.connect_continuously(profile_id).await.unwrap();
            {
                let mut state = service.state.lock().await;
                state.transition(handshake_phase).unwrap();
            }
            let accepted = service.retry_continuously(profile_id).await.unwrap();
            assert_eq!(accepted.phase, ConnectionPhase::Reconnecting);
            assert!(
                service
                    .continuous_connection
                    .lock()
                    .await
                    .as_ref()
                    .is_some_and(ConnectionIntent::is_active)
            );
            drop(mutation);
            service.disconnect().await.unwrap();
        }
    }

    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn macos_retry_wakes_the_current_agent_dial_until_settings_change() {
        let directory = tempfile::tempdir().unwrap();
        let service = ControlService::open_with_vault(
            ConfigStore::new(directory.path().join("config.json")),
            Arc::new(crate::tests::MemoryVault::default()),
        )
        .unwrap();
        let profile = service.config_snapshot().await.active_profile().unwrap();
        let mutation = service.mutation_lock.lock().await;
        service.connect_continuously(profile.id).await.unwrap();
        let (wake, cancel) = {
            let owner = service.continuous_connection.lock().await;
            let intent = owner.as_ref().unwrap();
            (intent.wake.clone(), intent.cancellation.clone())
        };
        let _dial = service
            .track_macos_agent_dial(Some((&wake, profile.clone())))
            .await
            .unwrap();
        service
            .state
            .lock()
            .await
            .transition(ConnectionPhase::ConnectingHttp2)
            .unwrap();
        let ack = service.retry_continuously(profile.id).await.unwrap();
        assert_eq!(ack.phase, ConnectionPhase::ConnectingHttp2);
        tokio::time::timeout(Duration::from_secs(1), wake.notified())
            .await
            .unwrap();
        assert!(!cancel.is_cancelled());

        service.config.write().await.network.kill_switch = !profile.kill_switch;
        let ack = service.retry_continuously(profile.id).await.unwrap();
        assert_eq!(ack.phase, ConnectionPhase::Reconnecting);
        assert!(cancel.is_cancelled());
        drop(mutation);
        service.disconnect().await.unwrap();
    }

    #[tokio::test]
    async fn rejected_state_transition_does_not_cancel_existing_continuous_worker() {
        let directory = tempfile::tempdir().unwrap();
        let service = ControlService::open_with_vault(
            ConfigStore::new(directory.path().join("config.json")),
            Arc::new(crate::tests::MemoryVault::default()),
        )
        .unwrap();
        let profile_id = service.config_snapshot().await.active_profile_id.unwrap();
        let mutation = service.mutation_lock.lock().await;
        service.connect_continuously(profile_id).await.unwrap();
        {
            let mut state = service.state.lock().await;
            state.transition(ConnectionPhase::Disconnecting).unwrap();
        }
        assert!(service.retry_continuously(profile_id).await.is_err());
        assert!(
            service
                .continuous_connection
                .lock()
                .await
                .as_ref()
                .is_some_and(ConnectionIntent::is_active),
            "rejected transition must not cancel or drop the existing worker"
        );
        drop(mutation);
        service.disconnect().await.unwrap();
    }

    #[tokio::test]
    async fn a_borrowed_vpn_remains_cancellable_and_returns_the_exact_cleanup_owner() {
        let directory = tempfile::tempdir().unwrap();
        let service = ControlService::open_with_vault(
            ConfigStore::new(directory.path().join("config.json")),
            Arc::new(crate::tests::MemoryVault::default()),
        )
        .unwrap();
        let mut profile = service.config_snapshot().await.active_profile().unwrap();
        profile.frontends.tunnel = true;
        profile.kill_switch = true;
        profile.canonicalize_mode();
        service
            .install_test_session(profile.clone(), true, 0)
            .await
            .unwrap();
        let lifecycle = service.mutation_lock.lock().await;
        service.connect_continuously(profile.id).await.unwrap();
        let first = service.gate_startup_cancel.lock().await.clone();
        let mut owner = service
            .data_plane
            .borrow_vpn_for_replacement()
            .await
            .unwrap();
        let generation = owner.active().session_generation;
        let ActiveRuntime::Harness(runtime) = &owner.active().runtime else {
            unreachable!()
        };
        let stopped = runtime.stopped.clone();
        assert!(service.data_plane.lock_for_status().await.1);
        let observed = tokio::time::timeout(Duration::from_secs(1), service.status_snapshot())
            .await
            .unwrap();
        assert_eq!(observed.phase, ConnectionPhase::Reconnecting);
        assert_eq!(observed.kill_switch_state, KillSwitchState::Active);
        tokio::time::timeout(
            Duration::from_secs(1),
            service.retry_continuously(profile.id),
        )
        .await
        .unwrap()
        .unwrap();
        assert!(first.is_cancelled());
        // Dropping a waiting replacement future must return the owner even
        // when another observer currently holds the publication lock.
        let observer = service.data_plane.lock().await;
        drop(owner);
        drop(observer);
        assert_eq!(
            service
                .data_plane
                .lock()
                .await
                .as_ref()
                .unwrap()
                .session_generation,
            generation
        );
        service.gate_startup_cancel.lock().await.cancel();
        drop(lifecycle);
        service.disconnect().await.unwrap();
        service.await_disconnect_cleanup().await.unwrap();
        assert!(stopped.is_cancelled());
    }

    #[tokio::test]
    async fn accepted_retry_stays_reconnecting_while_the_old_runtime_is_healthy() {
        let directory = tempfile::tempdir().unwrap();
        let service = ControlService::open_with_vault(
            ConfigStore::new(directory.path().join("config.json")),
            Arc::new(crate::tests::MemoryVault::default()),
        )
        .unwrap();
        let profile = service.config_snapshot().await.active_profile().unwrap();
        service
            .install_test_session(profile.clone(), false, 0)
            .await
            .unwrap();
        let mutation = service.mutation_lock.lock().await;
        let accepted = service.retry_continuously(profile.id).await.unwrap();
        assert_eq!(accepted.phase, ConnectionPhase::Reconnecting);
        assert_eq!(
            service.status_snapshot().await.phase,
            ConnectionPhase::Reconnecting
        );
        drop(mutation);
        service.disconnect().await.unwrap();
        service.await_disconnect_cleanup().await.unwrap();
    }

    #[tokio::test]
    async fn proxy_retry_teardown_stays_reconnecting_without_flashing_disconnected() {
        let directory = tempfile::tempdir().unwrap();
        let service = ControlService::open_with_vault(
            ConfigStore::new(directory.path().join("config.json")),
            Arc::new(crate::tests::MemoryVault::default()),
        )
        .unwrap();
        let mut profile = service.config_snapshot().await.active_profile().unwrap();
        profile.frontends = FrontendSettings {
            tunnel: false,
            socks5: true,
            http: true,
        };
        profile.canonicalize_mode();
        service
            .install_test_session(profile.clone(), false, 0)
            .await
            .unwrap();
        let _mutation = service.mutation_lock.lock().await;
        let before = service.status_snapshot().await;
        assert!(before.transport.is_some());
        assert!(!before.frontends.is_empty());
        let snapshot = service.teardown_for_reconnect_locked().await.unwrap();
        assert_eq!(snapshot.phase, ConnectionPhase::Reconnecting);
        service.await_disconnect_cleanup().await.unwrap();
        let after = service.status_snapshot().await;
        assert_eq!(after.phase, ConnectionPhase::Reconnecting);
        assert!(after.transport.is_none() && after.address_family.is_none());
        assert!(!after.ipv4_available && !after.ipv6_available);
        assert!(after.active_listeners.is_empty() && after.frontends.is_empty());
        assert_eq!(after.statistics, Statistics::default());
        drop(_mutation);
        service.disconnect().await.unwrap();
    }
}
