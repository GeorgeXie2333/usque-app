//! Control-service lifecycle tests with a memory runtime. The temporary proxy
//! library exercises normal encrypted-reference validation; no socket, Agent,
//! TUN, route, DNS or system-proxy mutation is performed.
#![cfg(windows)]

use super::*;
use std::time::Duration;
use tokio_util::sync::CancellationToken;
use usque_core::chain_exit::{
    ChainSource, ImportSecrets, ProxyAuthMode, ProxyExitConfiguration,
    store::{ChainProfileStore, WindowsProfileCipher},
};
use usque_core::vpngate::{GateFailure, GateStage};

#[test]
fn older_agent_handoff_error_uses_the_existing_component_update_message() {
    assert!(matches!(
        map_windows_vpn_error(windows_agent::WindowsVpnError::MissingCapabilities(
            "protected_tunnel_replacement/deferred_network_configuration".into(),
        )),
        ControlServiceError::PlatformRecovery {
            code: "WINDOWS_RECOVERY_UNSUPPORTED",
            retryable: false,
            ..
        }
    ));
}

async fn fixture(
    source: ChainSource,
    vpn: bool,
    kill_switch: bool,
    install: bool,
) -> (tempfile::TempDir, ControlService, Profile) {
    let directory = tempfile::tempdir().unwrap();
    let service = ControlService::open_with_vault(
        ConfigStore::new(directory.path().join("config.json")),
        Arc::new(crate::tests::MemoryVault::default()),
    )
    .unwrap();
    let secrets = ImportSecrets {
        proxy: Some(ProxyExitConfiguration {
            host: "proxy.example".into(),
            port: 1080,
            auth_mode: ProxyAuthMode::None,
            dns_servers: vec!["1.1.1.1".parse().unwrap()],
        }),
        username: String::new(),
        password: String::new(),
        private_key_password: String::new(),
        configuration: String::new(),
    };
    let imported = ChainProfileStore::new(&service.cache_dir, &WindowsProfileCipher)
        .import(source, "Protected-chain fixture", secrets)
        .unwrap();
    let mut profile = service.config_snapshot().await.active_profile().unwrap();
    profile.chain_exit = Some(imported.selection());
    profile.frontends = FrontendSettings {
        tunnel: vpn,
        socks5: true,
        http: true,
    };
    profile.proxy.system_proxy = false;
    profile.kill_switch = kill_switch;
    profile.canonicalize_mode();
    if install {
        service
            .install_test_session(profile.clone(), vpn, 0)
            .await
            .unwrap();
        let mut active = service.data_plane.lock().await;
        let ActiveRuntime::Harness(runtime) = &mut active.as_mut().unwrap().runtime else {
            panic!("fixture must remain an in-memory runtime")
        };
        runtime.gate_status.stage = GateStage::Connected;
    } else {
        service.upsert_profile(profile.clone()).await.unwrap();
    }
    (directory, service, profile)
}

async fn stopped_token(service: &ControlService) -> CancellationToken {
    let active = service.data_plane.lock().await;
    let ActiveRuntime::Harness(runtime) = &active.as_ref().unwrap().runtime else {
        panic!("fixture must remain an in-memory runtime")
    };
    runtime.stopped.clone()
}

/// Account insertion preserves the global network configuration. Save a real
/// masked network edit against the currently selected account while preventing
/// a settings worker from applying it to the memory session under test.
async fn save_network_for_next_connection(
    service: &ControlService,
    desired: &Profile,
    fields: &[&str],
) {
    let selected = service.config_snapshot().await.active_profile().unwrap();
    let mut values = desired.clone();
    values.id = selected.id;
    values.name = selected.name;
    let before = {
        let active = service.data_plane.lock().await;
        let active = active.as_ref().unwrap();
        (
            active.session_generation,
            active.profile.id,
            active.profile.kill_switch,
            active.profile.mtu,
            active.profile.frontends,
        )
    };
    let _lifecycle = service.mutation_lock.lock().await;
    let saved = service
        .save_network_settings(v1::SaveNetworkSettingsRequest {
            operation_id: Uuid::new_v4().to_string(),
            account_id: selected.id.to_string(),
            values: Some(profile_to_proto(&values)),
            changed_fields: fields.iter().map(|field| (*field).into()).collect(),
        })
        .await
        .unwrap();
    assert_eq!(saved.persisted, Some(true));
    assert_eq!(saved.apply_status, 4, "the test edit must remain deferred");
    let active = service.data_plane.lock().await;
    let active = active.as_ref().unwrap();
    assert_eq!(
        (
            active.session_generation,
            active.profile.id,
            active.profile.kill_switch,
            active.profile.mtu,
            active.profile.frontends,
        ),
        before,
        "saving shared settings must preserve the old applied session"
    );
}

async fn inject_terminal_failure(service: &ControlService, reason: GateFailure) {
    service
        .data_plane
        .lock()
        .await
        .as_mut()
        .unwrap()
        .runtime
        .fail_gate(reason)
        .await;
    assert_eq!(
        service.status_snapshot().await.phase,
        ConnectionPhase::Error
    );
    service.ensure_gate_supervisor().await;
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let processed = {
                let active = service.data_plane.lock().await;
                active
                    .as_ref()
                    .is_none_or(|active| active.runtime.failure_retained())
            };
            if processed {
                break;
            }
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    })
    .await
    .expect("the ordinary gate supervisor must admit the failure");
    // Join any admitted lifecycle mutation before inspecting the publication.
    let _finished = service.mutation_lock.lock().await;
}

async fn assert_explicit_disconnect_releases(
    service: &ControlService,
    stopped: &CancellationToken,
) {
    assert_eq!(
        service.disconnect().await.unwrap().phase,
        ConnectionPhase::Disconnected
    );
    service.await_disconnect_cleanup().await.unwrap();
    assert!(
        stopped.is_cancelled(),
        "explicit Disconnect must run shutdown"
    );
    assert!(service.data_plane.lock().await.is_none());
    assert!(service.session_profile.lock().await.is_none());
}

#[tokio::test]
async fn automatic_failure_retains_only_applied_proxy_vpn_with_kill_switch() {
    for source in [ChainSource::HttpProxy, ChainSource::Socks5Proxy] {
        for vpn in [false, true] {
            for kill_switch in [false, true] {
                let (_directory, service, profile) = fixture(source, vpn, kill_switch, true).await;
                let stopped = stopped_token(&service).await;
                inject_terminal_failure(&service, GateFailure::Transport).await;
                let should_retain = vpn && kill_switch;
                {
                    let active = service.data_plane.lock().await;
                    assert_eq!(
                        active.is_some(),
                        should_retain,
                        "{source:?}, tunnel={vpn}, Kill Switch={kill_switch}"
                    );
                    if let Some(active) = active.as_ref() {
                        assert_eq!(active.profile_id, profile.id);
                        assert_eq!(active.profile.kill_switch, kill_switch);
                        assert!(active.runtime.failure_retained());
                        assert!(active.runtime.listeners().is_empty());
                        assert!(!stopped.is_cancelled(), "retention must not call shutdown");
                    }
                }
                let snapshot = service.status_snapshot().await;
                assert_eq!(snapshot.phase, ConnectionPhase::Error);
                assert_eq!(
                    service.gate_status.borrow().failure,
                    Some(GateFailure::Transport)
                );
                if should_retain {
                    assert_eq!(snapshot.kill_switch_state, KillSwitchState::Active);
                    assert!(service.session_profile.lock().await.is_some());
                    assert_explicit_disconnect_releases(&service, &stopped).await;
                } else {
                    service.await_disconnect_cleanup().await.unwrap();
                    assert!(stopped.is_cancelled());
                    assert!(service.session_profile.lock().await.is_none());
                }
            }
        }
    }
}

#[tokio::test]
async fn first_failed_startup_transfers_guard_ownership_before_returning_error() {
    for source in [ChainSource::HttpProxy, ChainSource::Socks5Proxy] {
        for vpn in [false, true] {
            for kill_switch in [false, true] {
                let (_directory, service, profile) = fixture(source, vpn, kill_switch, false).await;
                let mut runtime = active_runtime::HarnessRuntime::from_profile(&profile, vpn, 0);
                runtime.gate_status.stage = GateStage::Error;
                runtime.gate_status.failure = Some(GateFailure::Authentication);
                let stopped = runtime.stopped.clone();
                let result = service
                    .accept_gate_runtime(ActiveRuntime::Harness(Box::new(runtime)), &profile)
                    .await;
                assert!(result.is_err());
                assert_eq!(
                    service.status_snapshot().await.phase,
                    ConnectionPhase::Error
                );
                if vpn && kill_switch {
                    let active = service.data_plane.lock().await;
                    let active = active.as_ref().expect("startup guard must have an owner");
                    assert_eq!(active.profile_id, profile.id);
                    assert!(active.runtime.failure_retained());
                    assert!(!stopped.is_cancelled());
                } else {
                    service.await_disconnect_cleanup().await.unwrap();
                    assert!(stopped.is_cancelled());
                    assert!(service.data_plane.lock().await.is_none());
                }
                if vpn && kill_switch {
                    assert_explicit_disconnect_releases(&service, &stopped).await;
                }
            }
        }
    }
}

#[tokio::test]
async fn deferred_kill_switch_off_cannot_release_old_applied_guard_but_retry_can_commit_it() {
    for source in [ChainSource::HttpProxy, ChainSource::Socks5Proxy] {
        let (_directory, service, mut saved) = fixture(source, true, true, true).await;
        let stopped = stopped_token(&service).await;
        saved.kill_switch = false;
        let busy = service.mutation_lock.lock().await;
        let response = service
            .save_network_settings(v1::SaveNetworkSettingsRequest {
                operation_id: Uuid::new_v4().to_string(),
                account_id: saved.id.to_string(),
                values: Some(profile_to_proto(&saved)),
                changed_fields: vec!["kill_switch".into()],
            })
            .await
            .unwrap();
        assert_eq!(response.persisted, Some(true));
        assert!(
            !service
                .config_snapshot()
                .await
                .active_profile()
                .unwrap()
                .kill_switch
        );
        assert!(
            service
                .data_plane
                .lock()
                .await
                .as_ref()
                .unwrap()
                .profile
                .kill_switch
        );
        assert!(
            service
                .network_settings_state()
                .await
                .applied_profile
                .unwrap()
                .kill_switch
        );
        drop(busy);
        inject_terminal_failure(&service, GateFailure::Transport).await;
        assert!(!stopped.is_cancelled());
        assert_eq!(
            service.status_snapshot().await.kill_switch_state,
            KillSwitchState::Active
        );

        let retried = service.retry().await.unwrap();
        assert_eq!(retried.phase, ConnectionPhase::Connected);
        assert_eq!(retried.kill_switch_state, KillSwitchState::Inactive);
        assert!(
            !service
                .data_plane
                .lock()
                .await
                .as_ref()
                .unwrap()
                .profile
                .kill_switch
        );
        assert!(
            !stopped.is_cancelled(),
            "successful handoff retains the same owner"
        );
        inject_terminal_failure(&service, GateFailure::Transport).await;
        service.await_disconnect_cleanup().await.unwrap();
        assert!(
            stopped.is_cancelled(),
            "the committed off preference applies to later failures"
        );
        assert!(service.data_plane.lock().await.is_none());
    }
}

#[tokio::test]
async fn account_activation_switches_only_the_existing_protected_vpn_boundary() {
    for source in [ChainSource::HttpProxy, ChainSource::Socks5Proxy] {
        for vpn in [false, true] {
            for kill_switch in [false, true] {
                let (_directory, service, a) = fixture(source, vpn, kill_switch, true).await;
                let stopped = stopped_token(&service).await;
                let generation_a = service
                    .data_plane
                    .lock()
                    .await
                    .as_ref()
                    .unwrap()
                    .session_generation;
                let b = Profile {
                    id: Uuid::new_v4(),
                    name: "Account B".into(),
                    ..a.clone()
                };
                service.upsert_profile(b.clone()).await.unwrap();
                service.set_active_profile(b.id).await.unwrap();
                assert_eq!(
                    service.config_snapshot().await.active_profile_id,
                    Some(b.id)
                );
                {
                    let active = service.data_plane.lock().await;
                    let active = active.as_ref().unwrap();
                    if vpn {
                        assert_eq!(active.profile_id, b.id);
                        assert!(active.session_generation > generation_a);
                    } else {
                        assert_eq!(active.profile_id, a.id);
                        assert_eq!(active.session_generation, generation_a);
                    }
                }
                assert!(!stopped.is_cancelled());
                assert_explicit_disconnect_releases(&service, &stopped).await;
            }
        }
    }
}

#[tokio::test]
async fn rapid_account_activations_and_old_exit_callbacks_cannot_restore_a_prior_session() {
    let (_directory, service, a) = fixture(ChainSource::Socks5Proxy, true, true, true).await;
    let stopped = stopped_token(&service).await;
    let generation_a = service
        .data_plane
        .lock()
        .await
        .as_ref()
        .unwrap()
        .session_generation;
    let b = Profile {
        id: Uuid::new_v4(),
        name: "Account B".into(),
        ..a.clone()
    };
    let c = Profile {
        id: Uuid::new_v4(),
        name: "Account C".into(),
        mtu: 1400,
        ..a.clone()
    };
    service.upsert_profile(b.clone()).await.unwrap();
    service.upsert_profile(c.clone()).await.unwrap();
    save_network_for_next_connection(&service, &c, &["mtu"]).await;
    let lifecycle = service.mutation_lock.lock().await;
    let before_b = service.settings_intent.load(Ordering::SeqCst);
    let activating_b = {
        let service = service.clone();
        tokio::spawn(async move { service.set_active_profile(b.id).await })
    };
    // Wait for B to enter the FIFO lifecycle queue before submitting C.
    tokio::time::timeout(Duration::from_secs(2), async {
        while service.settings_intent.load(Ordering::SeqCst) == before_b {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let activating_c = {
        let service = service.clone();
        let id = c.id;
        tokio::spawn(async move { service.set_active_profile(id).await })
    };
    tokio::task::yield_now().await;
    drop(lifecycle);
    activating_b.await.unwrap().unwrap();
    activating_c.await.unwrap().unwrap();
    assert_eq!(
        service.config_snapshot().await.active_profile_id,
        Some(c.id)
    );
    let generation_c = {
        let active = service.data_plane.lock().await;
        let active = active.as_ref().unwrap();
        assert_eq!(active.profile_id, c.id);
        assert_eq!(active.profile.mtu, 1400);
        assert!(active.session_generation > generation_a);
        active.session_generation
    };
    let stale = ExitInfo {
        ipv4: Some("192.0.2.10".parse().unwrap()),
        ipv6: None,
        ipv4_location: None,
        ipv6_location: None,
        checked_at: chrono::Utc::now(),
    };
    apply_exit_info(
        &service.state,
        &service.data_plane,
        a.id,
        generation_a,
        stale.clone(),
    )
    .await;
    apply_exit_info(
        &service.state,
        &service.data_plane,
        c.id,
        generation_a,
        stale,
    )
    .await;
    assert!(service.state.lock().await.snapshot().exit.is_none());
    let current = ExitInfo {
        ipv4: Some("192.0.2.30".parse().unwrap()),
        ipv6: None,
        ipv4_location: None,
        ipv6_location: None,
        checked_at: chrono::Utc::now(),
    };
    apply_exit_info(
        &service.state,
        &service.data_plane,
        c.id,
        generation_c,
        current.clone(),
    )
    .await;
    assert_eq!(
        service.state.lock().await.snapshot().exit.as_ref(),
        Some(&current)
    );
    assert!(!stopped.is_cancelled());
    assert_explicit_disconnect_releases(&service, &stopped).await;
}

#[tokio::test]
async fn failed_new_account_keeps_old_applied_protection_and_retry_loads_latest_saved_account() {
    for source in [ChainSource::HttpProxy, ChainSource::Socks5Proxy] {
        let (_directory, service, a) = fixture(source, true, true, true).await;
        let stopped = stopped_token(&service).await;
        let b = Profile {
            id: Uuid::new_v4(),
            name: "Account B".into(),
            kill_switch: false,
            ..a.clone()
        };
        service.upsert_profile(b.clone()).await.unwrap();
        save_network_for_next_connection(&service, &b, &["kill_switch"]).await;
        {
            let mut active = service.data_plane.lock().await;
            let ActiveRuntime::Harness(runtime) = &mut active.as_mut().unwrap().runtime else {
                unreachable!()
            };
            runtime.fail_protected_reconnect = true;
        }
        assert!(service.set_active_profile(b.id).await.is_err());
        assert_eq!(
            service.config_snapshot().await.active_profile_id,
            Some(b.id)
        );
        assert!(
            !service
                .config_snapshot()
                .await
                .active_profile()
                .unwrap()
                .kill_switch
        );
        {
            let active = service.data_plane.lock().await;
            let active = active
                .as_ref()
                .expect("failed replacement must retain its old owner");
            assert_eq!(active.profile_id, a.id);
            assert!(active.profile.kill_switch);
            assert!(active.runtime.failure_retained());
        }
        assert!(!stopped.is_cancelled());
        assert_eq!(
            service.status_snapshot().await.kill_switch_state,
            KillSwitchState::Active
        );

        // C is durably selected even though this replacement also fails. Retry
        // must then load C's latest network values rather than resurrect A/B.
        let mut c = Profile {
            id: Uuid::new_v4(),
            name: "Account C".into(),
            ..b.clone()
        };
        service.upsert_profile(c.clone()).await.unwrap();
        assert!(service.set_active_profile(c.id).await.is_err());
        c.mtu = 1380;
        save_network_for_next_connection(&service, &c, &["mtu"]).await;
        {
            let mut active = service.data_plane.lock().await;
            let ActiveRuntime::Harness(runtime) = &mut active.as_mut().unwrap().runtime else {
                unreachable!()
            };
            runtime.fail_protected_reconnect = false;
        }
        let snapshot = service.retry().await.unwrap();
        assert_eq!(snapshot.phase, ConnectionPhase::Connected);
        assert_eq!(snapshot.kill_switch_state, KillSwitchState::Inactive);
        let applied = service.session_profile.lock().await.clone().unwrap();
        assert_eq!(applied.id, c.id);
        assert_eq!(applied.mtu, 1380);
        assert!(!applied.kill_switch);
        assert_eq!(
            service.config_snapshot().await.active_profile_id,
            Some(c.id)
        );
        assert!(!stopped.is_cancelled());
        assert_explicit_disconnect_releases(&service, &stopped).await;
    }
}

#[tokio::test]
async fn disconnect_cancels_a_queued_account_handoff_without_undoing_the_saved_selection() {
    let (_directory, service, a) = fixture(ChainSource::HttpProxy, true, true, true).await;
    let stopped = stopped_token(&service).await;
    let original_generation = service.session_generation.load(Ordering::Relaxed);
    let b = Profile {
        id: Uuid::new_v4(),
        name: "Queued account B".into(),
        ..a
    };
    service.upsert_profile(b.clone()).await.unwrap();
    let lifecycle = service.mutation_lock.lock().await;
    let previous_intent = service.settings_intent.load(Ordering::SeqCst);
    let selecting = {
        let service = service.clone();
        let id = b.id;
        tokio::spawn(async move { service.set_active_profile(id).await })
    };
    tokio::time::timeout(Duration::from_secs(2), async {
        while service.settings_intent.load(Ordering::SeqCst) == previous_intent {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert!(!selecting.is_finished());
    let disconnecting = {
        let service = service.clone();
        tokio::spawn(async move { service.disconnect().await })
    };
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if service.gate_startup_cancel.lock().await.is_cancelled() {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert!(!disconnecting.is_finished());
    assert!(!stopped.is_cancelled());
    drop(lifecycle);
    selecting.await.unwrap().unwrap();
    assert_eq!(
        disconnecting.await.unwrap().unwrap().phase,
        ConnectionPhase::Disconnected
    );
    service.await_disconnect_cleanup().await.unwrap();
    assert_eq!(
        service.config_snapshot().await.active_profile_id,
        Some(b.id)
    );
    assert_eq!(
        service.session_generation.load(Ordering::Relaxed),
        original_generation,
        "the cancelled selection must not create a replacement session before Disconnect"
    );
    assert!(stopped.is_cancelled());
    assert!(service.data_plane.lock().await.is_none());
    assert!(service.session_profile.lock().await.is_none());
}

#[tokio::test]
async fn selecting_proxy_only_finishes_owned_vpn_cleanup_before_starting_the_target() {
    let (_directory, service, a) = fixture(ChainSource::HttpProxy, true, true, true).await;
    let stopped = stopped_token(&service).await;
    let mut b = Profile {
        id: Uuid::new_v4(),
        name: "Proxy-only target".into(),
        ..a
    };
    b.frontends.tunnel = false;
    b.canonicalize_mode();
    service.upsert_profile(b.clone()).await.unwrap();
    save_network_for_next_connection(&service, &b, &["frontends.tunnel"]).await;
    // MemoryVault deliberately has no credentials: startup fails before any
    // socket or native Agent access, after the explicitly removed VPN scope.
    assert!(service.set_active_profile(b.id).await.is_err());
    assert!(stopped.is_cancelled());
    assert!(service.disconnect_cleanup.lock().await.is_none());
    assert!(service.data_plane.lock().await.is_none());
    assert!(service.session_profile.lock().await.is_none());
    assert_eq!(
        service.config_snapshot().await.active_profile_id,
        Some(b.id)
    );
    assert_eq!(
        service.status_snapshot().await.phase,
        ConnectionPhase::Error
    );
}
