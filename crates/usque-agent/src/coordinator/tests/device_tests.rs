use super::*;
use crate::journal::DeviceState;

async fn prepare_device(coordinator: &AgentCoordinator<MockBackend>, key: DeviceLeaseKey) -> Uuid {
    let operation = Uuid::new_v4();
    let generation = coordinator.state().await.generation;
    coordinator
        .prepare_managed(operation, plan(), caller(), key, generation)
        .await
        .unwrap();
    operation
}

#[tokio::test]
async fn hundred_connections_keep_one_device_and_close_it_only_on_release() {
    let backend = Arc::new(MockBackend::default());
    let (_dir, coordinator) = coordinator(Arc::clone(&backend));
    let key = coordinator.acquire_device_lease(&caller()).await.unwrap();
    assert!(coordinator.state().await.device.is_none());
    for _ in 0..100 {
        let op = prepare_device(&coordinator, key).await;
        coordinator
            .open_packet_session(op, MIN_PACKET_RING_CAPACITY, &caller())
            .await
            .unwrap();
        coordinator.commit(op, &caller()).await.unwrap();
        coordinator.rollback(op, &caller()).await.unwrap();
        let state = coordinator.state().await;
        assert_eq!(state.phase, RecoveryPhase::Clean);
        assert_eq!(state.device.unwrap().state, DeviceState::Idle);
        assert!(state.steps.is_empty());
    }
    let count = |kinds: &[MutationKind], kind| kinds.iter().filter(|value| **value == kind).count();
    assert_eq!(
        count(&backend.applied.lock().await, MutationKind::WintunAdapter),
        1
    );
    assert_eq!(
        count(&backend.restored.lock().await, MutationKind::WintunAdapter),
        0
    );
    assert_eq!(
        count(&backend.restored.lock().await, MutationKind::PacketSession),
        100
    );
    assert_eq!(
        coordinator
            .release_device_lease(key, &caller())
            .await
            .unwrap(),
        DeviceRetirement::Complete
    );
    assert_eq!(
        count(&backend.restored.lock().await, MutationKind::WintunAdapter),
        1
    );
    assert!(coordinator.state().await.is_fully_clean());
}

#[tokio::test]
async fn stale_lease_generation_and_parallel_owner_cannot_bind_or_retire() {
    let (_dir, coordinator) = coordinator(Arc::new(MockBackend::default()));
    let old = coordinator.acquire_device_lease(&caller()).await.unwrap();
    let mut other = caller();
    other.process_id += 1;
    assert!(coordinator.acquire_device_lease(&other).await.is_err());
    let op = prepare_device(&coordinator, old).await;
    coordinator.rollback(op, &caller()).await.unwrap();
    assert!(coordinator.detach_device_lease(old, &caller()));
    let current = coordinator.acquire_device_lease(&other).await.unwrap();
    assert_ne!(current, old);
    assert!(!coordinator.retire_orphaned_device(old).await.unwrap());
    assert!(
        coordinator
            .prepare_managed(
                Uuid::new_v4(),
                plan(),
                other.clone(),
                old,
                coordinator.state().await.generation
            )
            .await
            .is_err()
    );
    assert!(
        coordinator
            .prepare_managed(Uuid::new_v4(), plan(), other.clone(), current, 0)
            .await
            .is_err()
    );
    coordinator
        .release_device_lease(current, &other)
        .await
        .unwrap();
}

#[tokio::test]
async fn lease_eof_prevents_late_session_start_and_another_users_takeover() {
    let backend = Arc::new(MockBackend::default());
    let (_dir, coordinator) = coordinator(Arc::clone(&backend));
    let key = coordinator.acquire_device_lease(&caller()).await.unwrap();
    let op = prepare_device(&coordinator, key).await;
    assert!(coordinator.detach_device_lease(key, &caller()));
    assert!(
        coordinator
            .open_packet_session(op, MIN_PACKET_RING_CAPACITY, &caller())
            .await
            .is_err()
    );
    let mut stranger = caller();
    stranger.user_sid = "S-1-5-21-2000".into();
    assert!(coordinator.acquire_device_lease(&stranger).await.is_err());
    assert!(coordinator.retire_orphaned_device(key).await.unwrap());
    assert!(coordinator.state().await.is_fully_clean());
    assert!(
        !backend
            .applied
            .lock()
            .await
            .contains(&MutationKind::PacketSession)
    );
}

#[tokio::test]
async fn device_and_session_eof_in_either_order_allow_guarded_engine_reattachment() {
    for device_first in [true, false] {
        let backend = Arc::new(MockBackend::default());
        let (_dir, coordinator) = coordinator(Arc::clone(&backend));
        let old = coordinator.acquire_device_lease(&caller()).await.unwrap();
        let op = prepare_device(&coordinator, old).await;
        let profile = coordinator.state().await.plan.unwrap().profile_id;
        coordinator
            .open_packet_session(op, MIN_PACKET_RING_CAPACITY, &caller())
            .await
            .unwrap();
        coordinator.commit(op, &caller()).await.unwrap();
        coordinator
            .acquire_tunnel_lease(op, &caller())
            .await
            .unwrap();
        if device_first {
            assert!(coordinator.detach_device_lease(old, &caller()));
        }
        let epoch = coordinator
            .release_tunnel_lease(op, &caller())
            .await
            .unwrap()
            .unwrap();
        if !device_first {
            assert!(coordinator.detach_device_lease(old, &caller()));
        }
        let mut next = caller();
        next.process_id += 1;
        let current = coordinator.acquire_device_lease(&next).await.unwrap();
        coordinator.resume_tunnel(op, profile, &next).await.unwrap();
        coordinator.acquire_tunnel_lease(op, &next).await.unwrap();
        assert!(!coordinator.retire_orphaned_device(old).await.unwrap());
        assert!(
            !coordinator
                .recover_orphaned_tunnel(op, epoch)
                .await
                .unwrap()
        );
        coordinator.rollback(op, &next).await.unwrap();
        assert!(
            !backend
                .restored
                .lock()
                .await
                .contains(&MutationKind::WintunAdapter)
        );
        coordinator
            .release_device_lease(current, &next)
            .await
            .unwrap();
    }
}

#[tokio::test]
async fn network_restore_failure_blocks_new_session_and_deferred_exit() {
    for kind in [
        MutationKind::InterfaceConfiguration,
        MutationKind::Dns,
        MutationKind::DefaultRoutes,
        MutationKind::KillSwitch,
    ] {
        let backend = Arc::new(MockBackend::default());
        let (_dir, coordinator) = coordinator(Arc::clone(&backend));
        let key = coordinator.acquire_device_lease(&caller()).await.unwrap();
        let op = prepare_device(&coordinator, key).await;
        coordinator
            .open_packet_session(op, MIN_PACKET_RING_CAPACITY, &caller())
            .await
            .unwrap();
        coordinator.commit(op, &caller()).await.unwrap();
        backend.fail_restore.lock().await.insert(kind);
        assert!(coordinator.rollback(op, &caller()).await.is_err());
        assert!(
            coordinator
                .prepare_managed(
                    Uuid::new_v4(),
                    plan(),
                    caller(),
                    key,
                    coordinator.state().await.generation
                )
                .await
                .is_err()
        );
        assert!(
            coordinator
                .release_device_lease(key, &caller())
                .await
                .is_err()
        );
        assert!(!coordinator.device_retirement_deferred());
        assert!(
            !backend
                .restored
                .lock()
                .await
                .contains(&MutationKind::WintunAdapter)
        );
    }
}

#[tokio::test]
async fn only_device_removal_failure_is_durable_and_allows_exit() {
    let backend = Arc::new(MockBackend::default());
    let (dir, coordinator) = coordinator(Arc::clone(&backend));
    let key = coordinator.acquire_device_lease(&caller()).await.unwrap();
    let op = prepare_device(&coordinator, key).await;
    coordinator.rollback(op, &caller()).await.unwrap();
    backend
        .fail_restore
        .lock()
        .await
        .insert(MutationKind::WintunAdapter);
    assert_eq!(
        coordinator
            .release_device_lease(key, &caller())
            .await
            .unwrap(),
        DeviceRetirement::Deferred
    );
    assert!(coordinator.device_retirement_deferred());
    let state = coordinator.state().await;
    assert_eq!(state.phase, RecoveryPhase::Clean);
    assert_eq!(state.device.unwrap().state, DeviceState::RecoveryRequired);
    let restarted = AgentCoordinator::open(
        JournalStore::new(dir.path().join("recovery.json")),
        Arc::clone(&backend),
    )
    .unwrap();
    assert!(restarted.acquire_device_lease(&caller()).await.is_err());
    backend.fail_restore.lock().await.clear();
    restarted.retire_device().await.unwrap();
    assert!(restarted.state().await.is_fully_clean());
}

#[tokio::test]
async fn persisted_idle_device_never_proves_creator_survived_restart() {
    let backend = Arc::new(MockBackend::default());
    let (dir, coordinator) = coordinator(Arc::clone(&backend));
    let key = coordinator.acquire_device_lease(&caller()).await.unwrap();
    let op = prepare_device(&coordinator, key).await;
    coordinator.rollback(op, &caller()).await.unwrap();
    let restarted =
        AgentCoordinator::open(JournalStore::new(dir.path().join("recovery.json")), backend)
            .unwrap();
    assert!(restarted.acquire_device_lease(&caller()).await.is_err());
}

#[tokio::test(start_paused = true)]
async fn retirement_timeout_retains_native_ownership_and_never_allows_another_device() {
    let backend = Arc::new(MockBackend::default());
    let (_dir, coordinator) = coordinator(Arc::clone(&backend));
    let key = coordinator.acquire_device_lease(&caller()).await.unwrap();
    let op = prepare_device(&coordinator, key).await;
    coordinator.rollback(op, &caller()).await.unwrap();
    backend.block_restore.store(true, Ordering::Release);
    assert_eq!(
        coordinator
            .release_device_lease(key, &caller())
            .await
            .unwrap(),
        DeviceRetirement::Deferred
    );
    assert!(coordinator.may_exit_idle().await);
    assert_eq!(
        coordinator
            .store
            .load_or_clean()
            .unwrap()
            .device
            .unwrap()
            .state,
        DeviceState::RecoveryRequired
    );
    assert!(coordinator.acquire_device_lease(&caller()).await.is_err());
    assert_eq!(
        coordinator.retire_device().await.unwrap(),
        DeviceRetirement::Deferred
    );
    assert_eq!(
        backend
            .restored
            .lock()
            .await
            .iter()
            .filter(|kind| **kind == MutationKind::WintunAdapter)
            .count(),
        1
    );
    backend.restore_release.notify_one();
    tokio::task::yield_now().await;
}

#[tokio::test(start_paused = true)]
async fn idle_device_outlives_the_ten_second_service_idle_timeout() {
    let backend = Arc::new(MockBackend::default());
    let (_dir, coordinator) = coordinator(Arc::clone(&backend));
    let key = coordinator.acquire_device_lease(&caller()).await.unwrap();
    let op = prepare_device(&coordinator, key).await;
    coordinator.rollback(op, &caller()).await.unwrap();
    tokio::time::advance(Duration::from_secs(60)).await;
    assert!(!coordinator.may_exit_idle().await);
    let op = prepare_device(&coordinator, key).await;
    coordinator.rollback(op, &caller()).await.unwrap();
    assert_eq!(
        backend
            .applied
            .lock()
            .await
            .iter()
            .filter(|kind| **kind == MutationKind::WintunAdapter)
            .count(),
        1
    );
}

#[tokio::test]
async fn failed_idle_save_or_retirement_intent_never_admits_reuse_or_exit() {
    let backend = Arc::new(MockBackend::default());
    let (_dir, coordinator) = coordinator(Arc::clone(&backend));
    let key = coordinator.acquire_device_lease(&caller()).await.unwrap();
    let op = prepare_device(&coordinator, key).await;
    coordinator.store.fail_next_clean_save();
    assert!(coordinator.rollback(op, &caller()).await.is_err());
    assert!(!coordinator.may_exit_idle().await);
    assert_eq!(
        coordinator.store.load_or_clean().unwrap().phase,
        RecoveryPhase::RecoveryRequired
    );
    assert!(
        coordinator
            .prepare_managed(
                Uuid::new_v4(),
                plan(),
                caller(),
                key,
                coordinator.state().await.generation
            )
            .await
            .is_err()
    );
    coordinator.recover_stale().await.unwrap();
    assert!(coordinator.state().await.device.is_some());
    coordinator.store.fail_next_clean_save();
    assert!(
        coordinator
            .release_device_lease(key, &caller())
            .await
            .is_err()
    );
    assert!(!coordinator.may_exit_idle().await);
    assert!(
        !backend
            .restored
            .lock()
            .await
            .contains(&MutationKind::WintunAdapter)
    );
}

#[tokio::test]
async fn unknown_missing_and_conflicting_device_never_create_a_second_session() {
    let backend = Arc::new(MockBackend::default());
    let (_dir, coordinator) = coordinator(Arc::clone(&backend));
    let key = coordinator.acquire_device_lease(&caller()).await.unwrap();
    let op = prepare_device(&coordinator, key).await;
    coordinator.rollback(op, &caller()).await.unwrap();
    backend.inspection_fails.store(true, Ordering::Release);
    assert!(
        coordinator
            .prepare_managed(
                Uuid::new_v4(),
                plan(),
                caller(),
                key,
                coordinator.state().await.generation
            )
            .await
            .is_err()
    );
    backend.inspection_fails.store(false, Ordering::Release);
    *backend.adapter_guid.lock().await = None;
    assert!(
        coordinator
            .prepare_managed(
                Uuid::new_v4(),
                plan(),
                caller(),
                key,
                coordinator.state().await.generation
            )
            .await
            .is_err()
    );
    *backend.adapter_guid.lock().await = Some(Uuid::new_v4());
    assert!(
        coordinator
            .prepare_managed(
                Uuid::new_v4(),
                plan(),
                caller(),
                key,
                coordinator.state().await.generation
            )
            .await
            .is_err()
    );
    assert!(
        !backend
            .applied
            .lock()
            .await
            .contains(&MutationKind::PacketSession)
    );
}

#[tokio::test]
async fn sidecar_and_standalone_proxy_restore_preserve_the_idle_device() {
    let backend = Arc::new(MockBackend::default());
    let (_dir, coordinator) = coordinator(Arc::clone(&backend));
    let key = coordinator.acquire_device_lease(&caller()).await.unwrap();
    let op = prepare_device(&coordinator, key).await;
    coordinator
        .open_packet_session(op, MIN_PACKET_RING_CAPACITY, &caller())
        .await
        .unwrap();
    coordinator.commit(op, &caller()).await.unwrap();
    let settings = SystemProxySettings {
        proxy_uri: "http://127.0.0.1:8080".into(),
        bypass_hosts: vec!["<local>".into()],
    };
    coordinator
        .apply_system_proxy(op, settings.clone(), caller())
        .await
        .unwrap();
    coordinator.rollback(op, &caller()).await.unwrap();
    let proxy_op = Uuid::new_v4();
    coordinator
        .apply_system_proxy(proxy_op, settings, caller())
        .await
        .unwrap();
    coordinator
        .restore_system_proxy(proxy_op, &caller())
        .await
        .unwrap();
    assert_eq!(
        coordinator.state().await.device.unwrap().state,
        DeviceState::Idle
    );
    assert!(
        !backend
            .restored
            .lock()
            .await
            .contains(&MutationKind::WintunAdapter)
    );
}
