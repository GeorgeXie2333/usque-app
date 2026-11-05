use super::*;
use crate::{ConfigStore, active_runtime::HarnessRuntime, tests::MemoryVault};
use std::sync::{Arc, atomic::Ordering};
use tokio_util::sync::CancellationToken;

fn service() -> (tempfile::TempDir, ControlService) {
    let directory = tempfile::tempdir().unwrap();
    let service = ControlService::open_with_vault(
        ConfigStore::new(directory.path().join("config.json")),
        Arc::new(MemoryVault::default()),
    )
    .unwrap();
    (directory, service)
}

#[cfg(target_os = "macos")]
#[tokio::test]
async fn macos_disconnect_rpc_waits_for_cleanup_and_returns_failure_until_owner_is_recovered() {
    use usque_ipc::v1::{self, control_request, control_response};

    let (_directory, service) = service();
    let mut profile = service.config_snapshot().await.active_profile().unwrap();
    profile.frontends.tunnel = false;
    profile.canonicalize_mode();
    service
        .install_test_session(profile, false, 0)
        .await
        .unwrap();
    let release = CancellationToken::new();
    let attempts = {
        let mut active = service.data_plane.lock().await;
        let ActiveRuntime::Harness(runtime) = &mut active.as_mut().unwrap().runtime else {
            unreachable!()
        };
        // Both the first background attempt and the explicit authorization
        // retry fail. The RPC must return an error and retain the same owner.
        runtime.shutdown_failures = 2;
        runtime.shutdown_release = Some(release.clone());
        runtime.shutdown_attempts.clone()
    };
    let disconnect = || v1::ControlRequest {
        request_id: "quit-cleanup-proof".into(),
        payload: Some(control_request::Payload::Disconnect(
            v1::DisconnectRequest {},
        )),
    };
    let request = {
        let service = service.clone();
        let request = disconnect();
        tokio::spawn(async move { service.handle(request).await })
    };
    tokio::time::timeout(std::time::Duration::from_secs(1), async {
        while attempts.load(Ordering::SeqCst) == 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert!(
        !request.is_finished(),
        "accepting Stop is not cleanup proof"
    );
    release.cancel();
    let response = request.await.unwrap();
    assert_eq!(response.error.unwrap().code, "DISCONNECT_CLEANUP_FAILED");
    assert!(response.payload.is_none());
    assert_eq!(attempts.load(Ordering::SeqCst), 2);
    assert_eq!(service.disconnect_owners.lock().await.len(), 1);
    assert!(service.await_disconnect_cleanup().await.is_err());

    let response = service.handle(disconnect()).await;
    assert!(response.error.is_none());
    let Some(control_response::Payload::Status(snapshot)) = response.payload else {
        panic!("cleanup-completed status acknowledgement");
    };
    assert_eq!(snapshot.phase, v1::ConnectionPhase::Disconnected as i32);
    assert!(snapshot.error.is_none());
    assert_eq!(attempts.load(Ordering::SeqCst), 3);
    assert!(service.disconnect_owners.lock().await.is_empty());
}

#[cfg(target_os = "macos")]
#[tokio::test]
async fn cancelled_macos_disconnect_waiter_keeps_cleanup_owned_for_the_next_quit() {
    let (_directory, service) = service();
    let mut profile = service.config_snapshot().await.active_profile().unwrap();
    profile.frontends.tunnel = false;
    profile.canonicalize_mode();
    service
        .install_test_session(profile, false, 0)
        .await
        .unwrap();
    let release = CancellationToken::new();
    let attempts = {
        let mut active = service.data_plane.lock().await;
        let ActiveRuntime::Harness(runtime) = &mut active.as_mut().unwrap().runtime else {
            unreachable!()
        };
        runtime.shutdown_release = Some(release.clone());
        runtime.shutdown_attempts.clone()
    };
    let request = {
        let service = service.clone();
        tokio::spawn(async move { service.disconnect_confirmed().await })
    };
    tokio::time::timeout(std::time::Duration::from_secs(1), async {
        while attempts.load(Ordering::SeqCst) == 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    request.abort();
    assert!(request.await.unwrap_err().is_cancelled());
    assert!(service.disconnect_cleanup.lock().await.is_some());
    assert_eq!(attempts.load(Ordering::SeqCst), 1);
    release.cancel();
    service.disconnect_confirmed().await.unwrap();
    assert_eq!(attempts.load(Ordering::SeqCst), 1);
    assert!(service.disconnect_owners.lock().await.is_empty());
}

#[cfg(target_os = "macos")]
#[tokio::test]
async fn macos_quit_retries_a_new_startup_cleanup_owner_before_confirming() {
    let (_directory, service) = service();
    let profile = service.config_snapshot().await.active_profile().unwrap();
    let mut runtime = HarnessRuntime::from_profile(&profile, false, 0);
    runtime.shutdown_failures = 1;
    let attempts = runtime.shutdown_attempts.clone();
    let stopped = runtime.stopped.clone();
    service
        .queue_runtime_shutdown(ActiveRuntime::Harness(Box::new(runtime)), None)
        .await;
    assert!(service.data_plane.lock().await.is_none());
    service.disconnect_confirmed().await.unwrap();
    assert_eq!(attempts.load(Ordering::SeqCst), 2);
    assert!(stopped.is_cancelled());
    assert!(service.disconnect_owners.lock().await.is_empty());
}

#[tokio::test]
async fn repeated_disconnect_retries_exact_failed_owner_and_connect_cannot_consume_the_failure() {
    for vpn in [false, true] {
        let (_directory, service) = service();
        let mut profile = service.config_snapshot().await.active_profile().unwrap();
        profile.frontends.tunnel = vpn;
        profile.frontends.http = true;
        profile.proxy.system_proxy = true;
        profile.canonicalize_mode();
        service
            .install_test_session(profile.clone(), vpn, 0)
            .await
            .unwrap();
        let (attempts, stopped) = {
            let mut active = service.data_plane.lock().await;
            let ActiveRuntime::Harness(runtime) = &mut active.as_mut().unwrap().runtime else {
                unreachable!()
            };
            runtime.shutdown_failures = 2;
            (runtime.shutdown_attempts.clone(), runtime.stopped.clone())
        };
        for attempt in 1..=2 {
            service.disconnect().await.unwrap();
            assert!(service.await_disconnect_cleanup().await.is_err());
            assert!(service.data_plane.lock().await.is_none());
            {
                let owners = service.disconnect_owners.lock().await;
                assert_eq!(owners.len(), 1);
                let ActiveRuntime::Harness(runtime) = &owners.front().unwrap().runtime else {
                    unreachable!()
                };
                assert_eq!(runtime.vpn, vpn);
                assert!(Arc::ptr_eq(&runtime.shutdown_attempts, &attempts));
                assert_eq!(runtime.shutdown_failures, 2 - attempt);
                assert!(owners.front().unwrap().last_error.is_some());
            }
            assert_eq!(attempts.load(Ordering::SeqCst), attempt as usize);
            assert!(!stopped.is_cancelled());
            assert!(service.disconnect_cleanup_failed.load(Ordering::Acquire));
            let snapshot = service.state.lock().await.snapshot().clone();
            assert_eq!(snapshot.phase, ConnectionPhase::Error);
            assert!(snapshot.error.is_some());
            assert_eq!(
                snapshot.kill_switch_state,
                if vpn {
                    KillSwitchState::Error
                } else {
                    KillSwitchState::NotApplicable
                }
            );
            // Reading or attempting Connect cannot consume the failed owner
            // or silently treat a previous restore attempt as confirmation.
            assert!(service.await_disconnect_cleanup().await.is_err());
            assert!(matches!(
                service.connect(profile.id).await,
                Err(ControlServiceError::DisconnectCleanup(_))
            ));
            assert_eq!(attempts.load(Ordering::SeqCst), attempt as usize);
            assert_eq!(service.state.lock().await.snapshot().error, snapshot.error);
            assert_eq!(service.disconnect_owners.lock().await.len(), 1);
            assert!(service.disconnect_cleanup_failed.load(Ordering::Acquire));
        }
        service.disconnect().await.unwrap();
        service.await_disconnect_cleanup().await.unwrap();
        assert_eq!(attempts.load(Ordering::SeqCst), 3);
        assert!(stopped.is_cancelled());
        assert!(service.disconnect_owners.lock().await.is_empty());
        assert!(!service.disconnect_cleanup_failed.load(Ordering::Acquire));
        let snapshot = service.state.lock().await.snapshot().clone();
        assert_eq!(snapshot.phase, ConnectionPhase::Disconnected);
        assert!(snapshot.error.is_none());
    }
}

#[tokio::test]
async fn duplicate_disconnect_and_cancelled_wait_do_not_spawn_parallel_shutdown_or_touch_successor()
{
    let (_directory, service) = service();
    let mut profile = service.config_snapshot().await.active_profile().unwrap();
    profile.frontends.tunnel = false;
    profile.canonicalize_mode();
    service
        .install_test_session(profile.clone(), false, 0)
        .await
        .unwrap();
    let release = CancellationToken::new();
    let attempts = {
        let mut active = service.data_plane.lock().await;
        let ActiveRuntime::Harness(runtime) = &mut active.as_mut().unwrap().runtime else {
            unreachable!()
        };
        runtime.shutdown_release = Some(release.clone());
        runtime.shutdown_attempts.clone()
    };
    service.disconnect().await.unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(1), async {
        while attempts.load(Ordering::SeqCst) == 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    tokio::time::timeout(std::time::Duration::from_millis(100), service.disconnect())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(attempts.load(Ordering::SeqCst), 1);
    assert!(
        tokio::time::timeout(
            std::time::Duration::from_millis(10),
            service.await_disconnect_cleanup()
        )
        .await
        .is_err()
    );
    assert!(service.disconnect_cleanup.lock().await.is_some());
    release.cancel();
    service.await_disconnect_cleanup().await.unwrap();
    service
        .install_test_session(profile, false, 0)
        .await
        .unwrap();
    // No detached retry timer or cancelled waiter remains to retire session B.
    service.await_disconnect_cleanup().await.unwrap();
    let successor_stopped = {
        let active = service.data_plane.lock().await;
        let active = active.as_ref().unwrap();
        assert!(!active.profile.frontends.tunnel);
        let ActiveRuntime::Harness(runtime) = &active.runtime else {
            unreachable!()
        };
        assert!(!runtime.stop_requested.is_cancelled());
        assert_eq!(runtime.shutdown_attempts.load(Ordering::SeqCst), 0);
        runtime.stopped.clone()
    };
    service.disconnect().await.unwrap();
    service.await_disconnect_cleanup().await.unwrap();
    assert!(successor_stopped.is_cancelled());
}

#[tokio::test]
async fn failed_startup_runtime_uses_the_same_retained_shutdown_owner_and_reports_cleanup_error() {
    let (_directory, service) = service();
    let mut profile = service.config_snapshot().await.active_profile().unwrap();
    profile.frontends.tunnel = false;
    profile.canonicalize_mode();
    service.upsert_profile(profile.clone()).await.unwrap();
    let mut runtime = HarnessRuntime::from_profile(&profile, false, 0);
    runtime.gate_status.stage = usque_core::vpngate::GateStage::Error;
    runtime.gate_status.failure = Some(usque_core::vpngate::GateFailure::Transport);
    runtime.shutdown_failures = 1;
    let attempts = runtime.shutdown_attempts.clone();
    let stopped = runtime.stopped.clone();
    assert!(
        service
            .accept_gate_runtime(ActiveRuntime::Harness(Box::new(runtime)), &profile)
            .await
            .is_err()
    );
    assert!(service.await_disconnect_cleanup().await.is_err());
    assert_eq!(service.disconnect_owners.lock().await.len(), 1);
    assert!(
        service
            .state
            .lock()
            .await
            .snapshot()
            .error
            .as_ref()
            .unwrap()
            .message
            .contains("injected shutdown failure")
    );
    service.disconnect().await.unwrap();
    service.await_disconnect_cleanup().await.unwrap();
    assert_eq!(attempts.load(Ordering::SeqCst), 2);
    assert!(stopped.is_cancelled());
    assert!(service.disconnect_owners.lock().await.is_empty());
}

#[tokio::test]
async fn cancelled_shutdown_worker_keeps_the_runtime_owned_until_explicit_retry() {
    let (_directory, service) = service();
    let mut profile = service.config_snapshot().await.active_profile().unwrap();
    profile.frontends.tunnel = true;
    profile.canonicalize_mode();
    service
        .install_test_session(profile, true, 0)
        .await
        .unwrap();
    let release = CancellationToken::new();
    let (attempts, stopped) = {
        let mut active = service.data_plane.lock().await;
        let ActiveRuntime::Harness(runtime) = &mut active.as_mut().unwrap().runtime else {
            unreachable!()
        };
        runtime.shutdown_release = Some(release.clone());
        (runtime.shutdown_attempts.clone(), runtime.stopped.clone())
    };
    service.disconnect().await.unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(1), async {
        while attempts.load(Ordering::SeqCst) == 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    // Unlike cancelling an awaiter, aborting the worker drops its in-flight
    // shutdown future. The exact runtime must still reside in the owner queue.
    service
        .disconnect_cleanup
        .lock()
        .await
        .as_ref()
        .unwrap()
        .abort();
    assert!(matches!(
        service.await_disconnect_cleanup().await,
        Err(ControlServiceError::DisconnectCleanup(_))
    ));
    assert!(service.disconnect_cleanup.lock().await.is_none());
    assert_eq!(service.disconnect_owners.lock().await.len(), 1);
    assert_eq!(attempts.load(Ordering::SeqCst), 1);
    assert!(!stopped.is_cancelled());
    assert_eq!(
        service.state.lock().await.snapshot().kill_switch_state,
        KillSwitchState::Error
    );
    assert!(service.await_disconnect_cleanup().await.is_err());
    assert_eq!(attempts.load(Ordering::SeqCst), 1);
    let cleanup_error = service.state.lock().await.snapshot().error.clone();
    let retrying = service.disconnect().await.unwrap();
    assert_eq!(retrying.phase, ConnectionPhase::Error);
    assert_eq!(retrying.error, cleanup_error);
    assert_eq!(retrying.kill_switch_state, KillSwitchState::Error);
    tokio::time::timeout(std::time::Duration::from_secs(1), async {
        while attempts.load(Ordering::SeqCst) != 2 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    // An explicit retry is not yet cleanup confirmation. While its shutdown
    // future is blocked, the prior uncertainty must remain visible.
    assert_eq!(service.state.lock().await.snapshot().error, cleanup_error);
    assert_eq!(
        service.state.lock().await.snapshot().kill_switch_state,
        KillSwitchState::Error
    );
    assert!(!stopped.is_cancelled());
    release.cancel();
    service.await_disconnect_cleanup().await.unwrap();
    assert_eq!(attempts.load(Ordering::SeqCst), 2);
    assert!(stopped.is_cancelled());
    assert!(service.disconnect_owners.lock().await.is_empty());
    assert_eq!(
        service.state.lock().await.snapshot().phase,
        ConnectionPhase::Disconnected
    );
}

#[tokio::test]
async fn disconnect_of_a_fresh_queued_startup_owner_finishes_the_starting_or_error_phase() {
    for phase in [
        ConnectionPhase::Preparing,
        ConnectionPhase::ConnectingHttp3,
        ConnectionPhase::Error,
    ] {
        let (_directory, service) = service();
        let mut profile = service.config_snapshot().await.active_profile().unwrap();
        profile.frontends.tunnel = false;
        profile.canonicalize_mode();
        service.upsert_profile(profile.clone()).await.unwrap();
        service
            .state
            .lock()
            .await
            .transition(ConnectionPhase::Preparing)
            .unwrap();
        let mut runtime = HarnessRuntime::from_profile(&profile, false, 0);
        if phase == ConnectionPhase::ConnectingHttp3 {
            service.state.lock().await.transition(phase).unwrap();
        } else if phase == ConnectionPhase::Error {
            runtime.gate_status.stage = usque_core::vpngate::GateStage::Error;
            runtime.gate_status.failure = Some(usque_core::vpngate::GateFailure::Transport);
            service
                .mark_connection_error(&ControlServiceError::Transport(
                    usque_transport::TransportError::VpnGate(
                        usque_core::vpngate::GateFailure::Transport,
                    ),
                ))
                .await;
        }
        let stopped = runtime.stopped.clone();
        let attempts = runtime.shutdown_attempts.clone();
        service
            .queue_runtime_shutdown(ActiveRuntime::Harness(Box::new(runtime)), None)
            .await;
        assert_eq!(service.state.lock().await.snapshot().phase, phase);
        assert!(service.data_plane.lock().await.is_none());
        assert_eq!(attempts.load(Ordering::SeqCst), 0);
        // This owner is new, not the retained result of a failed cleanup. An
        // explicit Disconnect must retire startup even before shutdown runs.
        let disconnected = service.disconnect().await.unwrap();
        assert_eq!(disconnected.phase, ConnectionPhase::Disconnected);
        service.await_disconnect_cleanup().await.unwrap();
        assert!(stopped.is_cancelled());
        assert_eq!(attempts.load(Ordering::SeqCst), 1);
        assert!(service.disconnect_owners.lock().await.is_empty());
        assert!(service.disconnect_cleanup.lock().await.is_none());
        assert_eq!(
            service.state.lock().await.snapshot().phase,
            ConnectionPhase::Disconnected
        );
        assert!(service.state.lock().await.snapshot().error.is_none());
    }
}
