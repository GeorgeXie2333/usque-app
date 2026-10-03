use super::*;
use crate::journal::ReplacementPhase;

async fn source(coordinator: &AgentCoordinator<MockBackend>) -> (Uuid, DeviceLeaseKey) {
    let key = coordinator.acquire_device_lease(&caller()).await.unwrap();
    let operation = Uuid::new_v4();
    let generation = coordinator.state().await.generation;
    coordinator
        .prepare_managed(operation, plan(), caller(), key, generation)
        .await
        .unwrap();
    coordinator
        .open_packet_session(operation, MIN_PACKET_RING_CAPACITY, &caller())
        .await
        .unwrap();
    coordinator.commit(operation, &caller()).await.unwrap();
    (operation, key)
}

async fn replace(
    coordinator: &AgentCoordinator<MockBackend>,
    source: Uuid,
    target: Uuid,
    plan: ValidatedTunnelPlan,
    key: DeviceLeaseKey,
) -> Result<RecoveryJournal, CoordinatorError> {
    coordinator
        .replace_tunnel(
            source,
            coordinator.state().await.generation,
            target,
            plan,
            caller(),
            key,
            async {},
        )
        .await
}

#[tokio::test]
async fn replacement_installs_guard_before_cleanup_and_commit_alone_releases_it() {
    for kill_switch in [true, false] {
        let backend = Arc::new(MockBackend::default());
        let (_dir, coordinator) = coordinator(Arc::clone(&backend));
        let (old, key) = source(&coordinator).await;
        let new = Uuid::new_v4();
        let mut target = plan();
        target.kill_switch = kill_switch;
        let prepared = replace(&coordinator, old, new, target.clone(), key)
            .await
            .unwrap();
        assert_eq!(prepared.phase, RecoveryPhase::Prepared);
        assert!(prepared.replacement_pending());
        assert!(
            coordinator
                .replacement_status(&prepared)
                .await
                .unwrap()
                .guard_active
        );
        assert!(matches!(
            coordinator.rollback(new, &caller()).await,
            Err(CoordinatorError::ReplacementPending)
        ));
        assert!(coordinator.rollback(old, &caller()).await.is_err());
        assert!(matches!(
            coordinator.recover_stale().await,
            Err(CoordinatorError::ReplacementPending)
        ));
        coordinator
            .open_packet_session(new, MIN_PACKET_RING_CAPACITY, &caller())
            .await
            .unwrap();
        let committed = coordinator.commit(new, &caller()).await.unwrap();
        assert_eq!(committed.phase, RecoveryPhase::Active);
        assert_eq!(committed.plan, Some(target));
        assert_eq!(
            committed.replacement.unwrap().phase,
            ReplacementPhase::Complete
        );
        assert!(!backend.replacement_guard.load(Ordering::Acquire));
        assert_eq!(
            *backend.replacement_events.lock().await,
            ["guard_installed", "normal_guard_removed", "guard_removed"]
        );
    }
}

#[tokio::test]
async fn failed_guard_installation_or_readback_never_cleans_source_protection() {
    for hide in [false, true] {
        let backend = Arc::new(MockBackend::default());
        let (_dir, coordinator) = coordinator(Arc::clone(&backend));
        let (old, key) = source(&coordinator).await;
        backend
            .fail_replacement_apply
            .store(!hide, Ordering::Release);
        backend
            .hide_replacement_guard
            .store(hide, Ordering::Release);
        let new = Uuid::new_v4();
        let target = plan();
        assert!(
            replace(&coordinator, old, new, target.clone(), key)
                .await
                .is_err()
        );
        assert!(backend.restored.lock().await.is_empty());
        let state = coordinator.state().await;
        assert!(
            !coordinator
                .replacement_status(&state)
                .await
                .unwrap()
                .guard_active
        );
        backend
            .fail_replacement_apply
            .store(false, Ordering::Release);
        backend
            .hide_replacement_guard
            .store(false, Ordering::Release);
        replace(&coordinator, old, new, target, key).await.unwrap();
        assert!(backend.replacement_guard.load(Ordering::Acquire));
    }
}

#[tokio::test]
async fn failed_target_retries_without_rollback_and_explicit_abort_is_idempotent() {
    let backend = Arc::new(MockBackend::default());
    let (_dir, coordinator) = coordinator(Arc::clone(&backend));
    let (old, key) = source(&coordinator).await;
    let new = Uuid::new_v4();
    let target = plan();
    backend
        .fail_apply
        .lock()
        .await
        .insert(MutationKind::InterfaceConfiguration);
    assert!(
        replace(&coordinator, old, new, target.clone(), key)
            .await
            .is_err()
    );
    assert!(backend.replacement_guard.load(Ordering::Acquire));
    assert!(coordinator.recover_stale().await.is_err());
    backend.fail_apply.lock().await.clear();
    let prepared = replace(&coordinator, old, new, target, key).await.unwrap();
    backend
        .fail_restore
        .lock()
        .await
        .insert(MutationKind::EndpointBypass);
    assert!(
        coordinator
            .abort_replacement(new, prepared.generation, &caller(), async {})
            .await
            .is_err()
    );
    assert!(backend.replacement_guard.load(Ordering::Acquire));
    backend.fail_restore.lock().await.clear();
    let clean = coordinator
        .abort_replacement(new, prepared.generation, &caller(), async {})
        .await
        .unwrap();
    assert_eq!(clean.phase, RecoveryPhase::Clean);
    assert!(!backend.replacement_guard.load(Ordering::Acquire));
    let again = coordinator
        .abort_replacement(new, prepared.generation, &caller(), async {})
        .await
        .unwrap();
    assert_eq!(again, clean);
}

#[tokio::test]
async fn new_target_keeps_previous_intersection_and_rejects_stale_or_mutated_requests() {
    let backend = Arc::new(MockBackend::default());
    let (_dir, coordinator) = coordinator(Arc::clone(&backend));
    let (old, key) = source(&coordinator).await;
    let first = Uuid::new_v4();
    let mut narrow = plan();
    narrow.allow_lan = false;
    narrow.split_exclusions = vec!["192.168.8.0/24".parse().unwrap()];
    let prepared = replace(&coordinator, old, first, narrow.clone(), key)
        .await
        .unwrap();
    let mut widened = narrow.clone();
    widened.allow_lan = true;
    widened.split_exclusions = vec!["192.168.0.0/16".parse().unwrap()];
    assert!(
        replace(&coordinator, old, first, widened.clone(), key)
            .await
            .is_err()
    );
    let second = Uuid::new_v4();
    assert!(
        coordinator
            .replace_tunnel(
                first,
                prepared.generation - 1,
                second,
                widened.clone(),
                caller(),
                key,
                async {}
            )
            .await
            .is_err()
    );
    let changed = replace(&coordinator, first, second, widened, key)
        .await
        .unwrap();
    assert_eq!(
        changed.replacement.as_ref().unwrap().guard_plan.exclusions,
        narrow.split_exclusions
    );
    assert_eq!(
        changed
            .replacement
            .as_ref()
            .unwrap()
            .original_source_operation_id,
        old
    );
    assert!(
        replace(&coordinator, old, first, narrow, key)
            .await
            .is_err()
    );
    assert!(backend.replacement_guard.load(Ordering::Acquire));
}

#[tokio::test]
async fn commit_removal_failure_can_retry_with_active_packet_session() {
    let backend = Arc::new(MockBackend::default());
    let (_dir, coordinator) = coordinator(Arc::clone(&backend));
    let (old, key) = source(&coordinator).await;
    let new = Uuid::new_v4();
    replace(&coordinator, old, new, plan(), key).await.unwrap();
    coordinator
        .open_packet_session(new, MIN_PACKET_RING_CAPACITY, &caller())
        .await
        .unwrap();
    backend
        .fail_replacement_remove
        .store(true, Ordering::Release);
    assert!(coordinator.commit(new, &caller()).await.is_err());
    assert!(backend.replacement_guard.load(Ordering::Acquire));
    assert_eq!(coordinator.state().await.phase, RecoveryPhase::Active);
    backend
        .fail_replacement_remove
        .store(false, Ordering::Release);
    coordinator.commit(new, &caller()).await.unwrap();
    assert!(!backend.replacement_guard.load(Ordering::Acquire));
}

#[tokio::test]
async fn restarted_agent_retains_guard_and_recreates_device_for_authenticated_owner() {
    let backend = Arc::new(MockBackend::default());
    let (directory, first) = coordinator(Arc::clone(&backend));
    let (old, key) = source(&first).await;
    let new = Uuid::new_v4();
    let target = plan();
    replace(&first, old, new, target.clone(), key)
        .await
        .unwrap();
    let old_device = first.state().await.device.unwrap().device_id;
    drop(first);
    let restarted = AgentCoordinator::open(
        JournalStore::new(directory.path().join("recovery.json")),
        Arc::clone(&backend),
    )
    .unwrap();
    assert!(restarted.recover_stale().await.is_err());
    let mut stranger = caller();
    stranger.user_sid = "S-1-5-21-9999".into();
    assert!(restarted.acquire_device_lease(&stranger).await.is_err());
    let mut owner = caller();
    owner.process_id += 1;
    let key = restarted.acquire_device_lease(&owner).await.unwrap();
    let generation = restarted.state().await.generation;
    let prepared = restarted
        .replace_tunnel(old, generation, new, target, owner, key, async {})
        .await
        .unwrap();
    assert_ne!(prepared.device.unwrap().device_id, old_device);
    assert!(backend.replacement_guard.load(Ordering::Acquire));
    assert!(
        !backend
            .replacement_events
            .lock()
            .await
            .contains(&"guard_removed")
    );
}

#[tokio::test]
async fn old_tunnel_and_device_eof_cannot_remove_replacement_guard() {
    let backend = Arc::new(MockBackend::default());
    let (_dir, coordinator) = coordinator(Arc::clone(&backend));
    let (old, key) = source(&coordinator).await;
    coordinator
        .acquire_tunnel_lease(old, &caller())
        .await
        .unwrap();
    let new = Uuid::new_v4();
    replace(&coordinator, old, new, plan(), key).await.unwrap();
    assert!(!coordinator.recover_orphaned_tunnel(old, 0).await.unwrap());
    assert!(coordinator.detach_device_lease(key, &caller()));
    assert!(matches!(
        coordinator.retire_orphaned_device(key).await,
        Err(CoordinatorError::ReplacementPending)
    ));
    assert!(backend.replacement_guard.load(Ordering::Acquire));
}

#[tokio::test]
async fn schema_four_cannot_claim_a_replacement_record() {
    let backend = Arc::new(MockBackend::default());
    let (directory, coordinator) = coordinator(backend);
    let (old, key) = source(&coordinator).await;
    replace(&coordinator, old, Uuid::new_v4(), plan(), key)
        .await
        .unwrap();
    let mut state = coordinator.state().await;
    state.schema_version = 4;
    let path = directory.path().join("recovery.json");
    fs::write(&path, serde_json::to_vec(&state).unwrap()).unwrap();
    assert!(matches!(
        JournalStore::new(path).load_or_clean(),
        Err(JournalError::InvalidReplacement)
    ));
}

#[tokio::test]
async fn completion_write_crash_reinstalls_guard_before_retry_or_retarget_cleanup() {
    for retarget in [false, true] {
        let backend = Arc::new(MockBackend::default());
        let (directory, first) = coordinator(Arc::clone(&backend));
        let (old, key) = source(&first).await;
        let new = Uuid::new_v4();
        let target = plan();
        replace(&first, old, new, target.clone(), key)
            .await
            .unwrap();
        first
            .open_packet_session(new, MIN_PACKET_RING_CAPACITY, &caller())
            .await
            .unwrap();
        first.store.fail_next_replacement_completion_save();
        assert!(first.commit(new, &caller()).await.is_err());
        assert!(!backend.replacement_guard.load(Ordering::Acquire));
        // After Agent/device loss the old complete tunnel is not reattachable;
        // rebuilding a blocking guard must not depend on that unrelated fact.
        *backend.inspection.lock().await = Some(TunnelInspection::NeedsRecovery);
        drop(first);
        let restarted = AgentCoordinator::open(
            JournalStore::new(directory.path().join("recovery.json")),
            Arc::clone(&backend),
        )
        .unwrap();
        assert_eq!(
            restarted.state().await.replacement.unwrap().phase,
            ReplacementPhase::Committing
        );
        let mut owner = caller();
        owner.process_id += 1;
        let key = restarted.acquire_device_lease(&owner).await.unwrap();
        let generation = restarted.state().await.generation;
        let event_count = backend.replacement_events.lock().await.len();
        let (source, target_id) = if retarget {
            (new, Uuid::new_v4())
        } else {
            (old, new)
        };
        let prepared = restarted
            .replace_tunnel(source, generation, target_id, target, owner, key, async {})
            .await
            .unwrap();
        assert_eq!(prepared.phase, RecoveryPhase::Prepared);
        assert!(backend.replacement_guard.load(Ordering::Acquire));
        let events = backend.replacement_events.lock().await;
        assert_eq!(
            &events[event_count..],
            ["guard_installed", "normal_guard_removed"]
        );
    }
}

#[tokio::test]
async fn aborted_guard_removal_can_recover_after_final_clean_write_crash() {
    let backend = Arc::new(MockBackend::default());
    let (directory, first) = coordinator(Arc::clone(&backend));
    let (old, key) = source(&first).await;
    let new = Uuid::new_v4();
    let prepared = replace(&first, old, new, plan(), key).await.unwrap();
    first.store.fail_next_clean_save();
    assert!(
        first
            .abort_replacement(new, prepared.generation, &caller(), async {})
            .await
            .is_err()
    );
    assert!(!backend.replacement_guard.load(Ordering::Acquire));
    drop(first);
    let restarted = AgentCoordinator::open(
        JournalStore::new(directory.path().join("recovery.json")),
        backend,
    )
    .unwrap();
    let mut owner = caller();
    owner.process_id += 1;
    let state = restarted.state().await;
    assert_eq!(state.replacement.unwrap().phase, ReplacementPhase::Aborting);
    let clean = restarted
        .abort_replacement(new, state.generation, &owner, async {})
        .await
        .unwrap();
    assert_eq!(clean.phase, RecoveryPhase::Clean);
}

#[tokio::test]
async fn malformed_or_unknown_journal_after_guard_intent_never_mutates_source_policy() {
    for corrupt in [true, false] {
        let backend = Arc::new(MockBackend::default());
        let (directory, first) = coordinator(Arc::clone(&backend));
        let (old, key) = source(&first).await;
        backend
            .fail_replacement_apply
            .store(true, Ordering::Release);
        assert!(
            replace(&first, old, Uuid::new_v4(), plan(), key)
                .await
                .is_err()
        );
        let mut state = first.state().await;
        assert_eq!(
            state.replacement.as_ref().unwrap().phase,
            ReplacementPhase::InstallingGuard
        );
        state.schema_version = crate::journal::JOURNAL_SCHEMA_VERSION + 1;
        let bytes = if corrupt {
            b"{truncated".to_vec()
        } else {
            serde_json::to_vec(&state).unwrap()
        };
        let path = directory.path().join("recovery.json");
        fs::write(&path, &bytes).unwrap();
        drop(first);
        assert!(AgentCoordinator::open(JournalStore::new(&path), Arc::clone(&backend)).is_err());
        assert!(backend.restored.lock().await.is_empty());
        assert!(!backend.replacement_guard.load(Ordering::Acquire));
        assert!(
            backend
                .applied
                .lock()
                .await
                .contains(&MutationKind::KillSwitch)
        );
        assert_eq!(fs::read(path).unwrap(), bytes);
    }
}

#[tokio::test]
async fn same_process_retries_completion_and_abort_writes_before_publishing_success() {
    for abort in [false, true] {
        let backend = Arc::new(MockBackend::default());
        let (_directory, coordinator) = coordinator(backend);
        let (old, key) = source(&coordinator).await;
        let new = Uuid::new_v4();
        let target = plan();
        let prepared = replace(&coordinator, old, new, target.clone(), key)
            .await
            .unwrap();
        if abort {
            coordinator.store.fail_next_clean_save();
            assert!(
                coordinator
                    .abort_replacement(new, prepared.generation, &caller(), async {})
                    .await
                    .is_err()
            );
            assert!(coordinator.state().await.replacement_pending());
            let clean = coordinator
                .abort_replacement(new, prepared.generation, &caller(), async {})
                .await
                .unwrap();
            assert_eq!(clean.phase, RecoveryPhase::Clean);
            assert_eq!(coordinator.store.load_or_clean().unwrap(), clean);
        } else {
            coordinator
                .open_packet_session(new, MIN_PACKET_RING_CAPACITY, &caller())
                .await
                .unwrap();
            coordinator.store.fail_next_replacement_completion_save();
            assert!(coordinator.commit(new, &caller()).await.is_err());
            assert!(coordinator.state().await.replacement_pending());
            let active = replace(&coordinator, old, new, target, key).await.unwrap();
            assert_eq!(
                active.replacement.as_ref().unwrap().phase,
                ReplacementPhase::Complete
            );
            assert_eq!(coordinator.store.load_or_clean().unwrap(), active);
        }
    }
}

#[tokio::test]
async fn failed_intent_write_is_retried_before_native_guard_installation() {
    let backend = Arc::new(MockBackend::default());
    let (directory, coordinator) = coordinator(Arc::clone(&backend));
    let (old, key) = source(&coordinator).await;
    *backend.replacement_journal_path.lock().await = Some(directory.path().join("recovery.json"));
    let new = Uuid::new_v4();
    let target = plan();
    coordinator.store.fail_next_replacement_intent_save();
    assert!(
        replace(&coordinator, old, new, target.clone(), key)
            .await
            .is_err()
    );
    assert!(!backend.replacement_guard.load(Ordering::Acquire));
    assert!(backend.restored.lock().await.is_empty());
    replace(&coordinator, old, new, target, key).await.unwrap();
    assert!(backend.replacement_guard.load(Ordering::Acquire));
}

#[tokio::test]
async fn new_owner_can_disconnect_after_acquiring_its_replacement_device_lease() {
    let backend = Arc::new(MockBackend::default());
    let (directory, coordinator) = coordinator(Arc::clone(&backend));
    let (old, key) = source(&coordinator).await;
    let new = Uuid::new_v4();
    replace(&coordinator, old, new, plan(), key).await.unwrap();
    drop(coordinator);
    let restarted = AgentCoordinator::open(
        JournalStore::new(directory.path().join("recovery.json")),
        backend,
    )
    .unwrap();
    let mut owner = caller();
    owner.process_id += 1;
    let key = restarted.acquire_device_lease(&owner).await.unwrap();
    let state = restarted.state().await;
    let clean = restarted
        .abort_replacement(new, state.generation, &owner, async {})
        .await
        .unwrap();
    assert_eq!(clean.phase, RecoveryPhase::Clean);
    restarted
        .prepare_managed(Uuid::new_v4(), plan(), owner, key, clean.generation)
        .await
        .unwrap();
}

#[tokio::test]
async fn retarget_never_reuses_the_original_or_cleanup_operation() {
    let backend = Arc::new(MockBackend::default());
    let (_directory, coordinator) = coordinator(backend);
    let (old, key) = source(&coordinator).await;
    let first = Uuid::new_v4();
    replace(&coordinator, old, first, plan(), key)
        .await
        .unwrap();
    assert!(
        replace(&coordinator, first, old, plan(), key)
            .await
            .is_err()
    );
    let second = Uuid::new_v4();
    replace(&coordinator, first, second, plan(), key)
        .await
        .unwrap();
    assert!(
        replace(&coordinator, second, first, plan(), key)
            .await
            .is_err()
    );
}
