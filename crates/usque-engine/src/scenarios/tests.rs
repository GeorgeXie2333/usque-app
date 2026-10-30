//! Inert scene transaction and acknowledgement tests; no native networking.
use super::*;
use usque_core::scenarios::{ScenarioError, ScenarioSettings};
use usque_core::{Account, IdentityProvider, storage::ConfigStore};
use uuid::Uuid;

async fn fixture() -> (tempfile::TempDir, ControlService) {
    let directory = tempfile::tempdir().unwrap();
    let service = ControlService::open_with_vault(
        ConfigStore::new(directory.path().join("config.json")),
        Arc::new(crate::tests::MemoryVault::default()),
    )
    .unwrap();
    let id = service.config_snapshot().await.active_profile_id.unwrap();
    service
        .persist_identity(
            id,
            &crate::tests::test_identity(IdentityProvider::Consumer, None),
            None,
        )
        .await
        .unwrap();
    (directory, service)
}

async fn request(service: &ControlService, command: Value) -> Result<Value, ControlServiceError> {
    let response = service
        .scenario_command(v1::ScenarioRequest {
            command_json: command.to_string(),
        })
        .await?;
    Ok(serde_json::from_str(&response.metadata_json).unwrap())
}

async fn save_scene(
    service: &ControlService,
    settings: ScenarioSettings,
    bound: Option<Uuid>,
) -> Value {
    let response = request(
        service,
        json!({"action":"save","name":"Work","settings":settings,"bound_account_id":bound}),
    )
    .await
    .unwrap();
    assert_eq!(response["persisted"], true);
    response["scenarios"]
        .as_array()
        .unwrap()
        .last()
        .unwrap()
        .clone()
}

async fn preview(service: &ControlService, scene: &Value) -> Value {
    request(
        service,
        json!({"action":"preview","id":scene["id"],"expected_revision":scene["revision"]}),
    )
    .await
    .unwrap()
}

fn activate_request(scene: &Value, preview: &Value, operation: Uuid) -> Value {
    json!({
        "action":"activate","id":scene["id"],"expected_revision":scene["revision"],
        "preview_token":preview["preview"]["preview_token"],"operation_id":operation
    })
}

#[tokio::test]
async fn template_crud_and_preview_do_not_change_selected_network_or_connect() {
    let (_directory, service) = fixture().await;
    let before = service.config_snapshot().await;
    let mut settings = ScenarioSettings::from_network(&before.network);
    settings.frontends.http = false;
    let scene = save_scene(&service, settings, None).await;
    let after_save = service.config_snapshot().await;
    assert_eq!(after_save.network, before.network);
    assert_eq!(after_save.active_profile_id, before.active_profile_id);
    let saved_bytes = std::fs::read(service.store.path()).unwrap();
    let response = preview(&service, &scene).await;
    assert_eq!(response["preview"]["apply_kind"], "select");
    assert_eq!(std::fs::read(service.store.path()).unwrap(), saved_bytes);
    assert!(service.data_plane.lock().await.is_none());
    request(
        &service,
        json!({"action":"delete","id":scene["id"],"expected_revision":scene["revision"]}),
    )
    .await
    .unwrap();
    let after = service.config_snapshot().await;
    assert!(after.scenarios.is_empty());
    assert_eq!(after.network, before.network);
}

#[tokio::test]
async fn disconnected_activation_commits_bound_account_and_settings_without_starting_runtime() {
    let (_directory, service) = fixture().await;
    let bound = Uuid::new_v4();
    service
        .update_config(move |config| {
            config.profiles.push(Account::new(bound, "Bound account"));
            Ok(())
        })
        .await
        .unwrap();
    service
        .persist_identity(
            bound,
            &crate::tests::test_identity(IdentityProvider::Consumer, None),
            None,
        )
        .await
        .unwrap();
    let before = service.config_snapshot().await;
    let mut settings = ScenarioSettings::from_network(&before.network);
    settings.frontends.http = false;
    let scene = save_scene(&service, settings, Some(bound)).await;
    let plan = preview(&service, &scene).await;
    assert_eq!(plan["preview"]["account_changed"], true);
    let operation = Uuid::new_v4();
    let response = request(&service, activate_request(&scene, &plan, operation))
        .await
        .unwrap();
    assert_eq!(response["persisted"], true);
    assert_eq!(response["persisted_operation_id"], operation.to_string());
    assert_eq!(response["state"]["apply_status"], "deferred");
    assert!(response["state"]["applied_matches"].is_null());
    let saved = service.store.load().unwrap();
    assert_eq!(saved.active_profile_id, Some(bound));
    assert!(!saved.network.frontends.http);
    assert_eq!(saved.network.kill_switch, before.network.kill_switch);
    assert_eq!(saved.last_scenario_switch.unwrap().operation_id, operation);
    assert!(service.data_plane.lock().await.is_none());
    assert_eq!(
        service.state.lock().await.snapshot().phase,
        ConnectionPhase::Disconnected
    );
}

#[tokio::test]
async fn stale_preview_rejects_before_mutating_storage() {
    let (_directory, service) = fixture().await;
    let scene = save_scene(&service, ScenarioSettings::default(), None).await;
    let plan = preview(&service, &scene).await;
    service
        .update_config(|config| {
            config.network.auto_connect = !config.network.auto_connect;
            Ok(())
        })
        .await
        .unwrap();
    let before = service.store.load().unwrap();
    let error = request(&service, activate_request(&scene, &plan, Uuid::new_v4()))
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        ControlServiceError::Scenario(ScenarioError::PreviewStale)
    ));
    assert_eq!(service.store.load().unwrap(), before);
    assert!(service.data_plane.lock().await.is_none());
}

#[tokio::test]
async fn missing_bound_account_remains_editable_but_cannot_be_used() {
    let (_directory, service) = fixture().await;
    let missing = Uuid::new_v4();
    let error = request(
        &service,
        json!({"action":"save","name":"Work","settings":ScenarioSettings::default(),"bound_account_id":missing}),
    )
    .await
    .unwrap_err();
    assert!(matches!(
        error,
        ControlServiceError::Scenario(ScenarioError::AccountUnavailable)
    ));
    let saved = save_scene(&service, ScenarioSettings::default(), None).await;
    // Model an account deleted after the scene was bound to it.
    service
        .update_config(move |config| {
            config.scenarios[0].bound_account_id = Some(missing);
            Ok(())
        })
        .await
        .unwrap();
    let mut scene = saved;
    scene["bound_account_id"] = json!(missing);
    let before = service.store.load().unwrap();
    let error = request(
        &service,
        json!({"action":"preview","id":scene["id"],"expected_revision":scene["revision"]}),
    )
    .await
    .unwrap_err();
    assert!(matches!(
        error,
        ControlServiceError::Scenario(ScenarioError::AccountUnavailable)
    ));
    assert_eq!(service.store.load().unwrap(), before);
    let list = request(&service, json!({"action":"list"})).await.unwrap();
    assert_eq!(list["scenarios"].as_array().unwrap().len(), 1);
    request(
        &service,
        json!({"action":"delete","id":scene["id"],"expected_revision":scene["revision"]}),
    )
    .await
    .unwrap();
    assert!(service.store.load().unwrap().scenarios.is_empty());
}

#[tokio::test]
async fn queries_and_template_edits_remain_available_while_lifecycle_owner_is_busy() {
    let (_directory, service) = fixture().await;
    let scene = save_scene(&service, ScenarioSettings::default(), None).await;
    let owner = service.mutation_lock.lock().await;
    let response = request(&service, json!({"action":"list"})).await.unwrap();
    assert_eq!(response["scenarios"].as_array().unwrap().len(), 1);
    let plan = preview(&service, &scene).await;
    let second = save_scene(&service, ScenarioSettings::default(), None).await;
    request(
        &service,
        json!({"action":"delete","id":second["id"],"expected_revision":second["revision"]}),
    )
    .await
    .unwrap();
    let before = service.store.load().unwrap();
    let error = request(&service, activate_request(&scene, &plan, Uuid::new_v4()))
        .await
        .unwrap_err();
    assert_eq!(service.store.load().unwrap(), before);
    drop(owner);
    assert!(matches!(
        error,
        ControlServiceError::ScenarioUnavailable("SCENARIO_BUSY")
    ));
}

#[tokio::test]
async fn disconnect_rpc_cancels_scene_owner_before_acquiring_its_provider_lease() {
    let (_directory, service) = fixture().await;
    let mut profile = service.config_snapshot().await.active_profile().unwrap();
    profile.frontends.tunnel = false;
    service
        .install_test_session(profile, false, 0)
        .await
        .unwrap();
    let lifecycle = service.mutation_lock.clone().lock_owned().await;
    let provider = usque_core::proton_vpn::lifecycle_lease(&service.profile_dir).unwrap();
    let cancellation = service.gate_connection_request().await;
    let before = service.store.load().unwrap();
    // Model a slow scene worker's exact ownership, without starting any native
    // network. It cannot release either lease until Disconnect reaches it.
    let owner = tokio::spawn(async move {
        let _lifecycle = lifecycle;
        let _provider = provider;
        cancellation.cancelled().await;
    });
    let response = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        service.handle(usque_ipc::v1::ControlRequest {
            request_id: "disconnect-scene".into(),
            payload: Some(usque_ipc::v1::control_request::Payload::Disconnect(
                v1::DisconnectRequest {},
            )),
        }),
    )
    .await
    .unwrap();
    assert!(response.error.is_none(), "{:?}", response.error);
    owner.await.unwrap();
    assert!(service.data_plane.lock().await.is_none());
    assert_eq!(
        service.state.lock().await.snapshot().phase,
        ConnectionPhase::Disconnected
    );
    assert_eq!(service.store.load().unwrap(), before);
    assert!(usque_core::proton_vpn::lifecycle_lease(&service.profile_dir).is_ok());
}

#[tokio::test]
async fn unknown_running_owner_is_rejected_before_scene_commit() {
    let (_directory, service) = fixture().await;
    let mut profile = service.config_snapshot().await.active_profile().unwrap();
    profile.frontends.tunnel = false;
    service
        .install_test_session(profile.clone(), false, 0)
        .await
        .unwrap();
    {
        let mut state = service.settings.lock().await;
        state.applied_profile = None;
        state.apply_status = ApplyStatus::Unknown;
    }
    let scene = save_scene(&service, ScenarioSettings::from_profile(&profile), None).await;
    let plan = preview(&service, &scene).await;
    assert_eq!(plan["preview"]["apply_kind"], "unconfirmed");
    let before = service.store.load().unwrap();
    let error = request(&service, activate_request(&scene, &plan, Uuid::new_v4()))
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        ControlServiceError::ScenarioUnavailable("SCENARIO_RUNTIME_UNCONFIRMED")
    ));
    assert_eq!(service.store.load().unwrap(), before);
}

#[tokio::test]
async fn restore_is_a_new_explicit_commit_and_preserves_new_global_preferences() {
    let (_directory, service) = fixture().await;
    let mut settings = ScenarioSettings::default();
    settings.frontends.http = false;
    let scene = save_scene(&service, settings, None).await;
    let plan = preview(&service, &scene).await;
    request(&service, activate_request(&scene, &plan, Uuid::new_v4()))
        .await
        .unwrap();
    service
        .update_config(|config| {
            config.network.auto_connect = true;
            Ok(())
        })
        .await
        .unwrap();
    let restore = request(&service, json!({"action":"preview_restore"}))
        .await
        .unwrap();
    let operation = Uuid::new_v4();
    request(&service,json!({"action":"restore","preview_token":restore["preview"]["preview_token"],"operation_id":operation})).await.unwrap();
    let saved = service.store.load().unwrap();
    assert!(saved.network.frontends.http);
    assert!(saved.network.auto_connect);
    assert!(saved.last_scenario_switch.unwrap().restored);
    assert!(service.data_plane.lock().await.is_none());
}

#[tokio::test]
async fn hot_scene_changes_publish_confirmed_profile_and_keep_actual_session_algorithm() {
    for change_rules in [false, true] {
        let (_directory, service) = fixture().await;
        let mut profile = service.config_snapshot().await.active_profile().unwrap();
        profile.frontends.tunnel = false;
        service
            .install_test_session(profile.clone(), false, 0)
            .await
            .unwrap();
        let running_algorithm = profile.congestion_control;
        service
            .update_config(|config| {
                config.network.congestion_control = usque_core::CongestionControlAlgorithm::Bbr;
                Ok(())
            })
            .await
            .unwrap();
        let mut settings = ScenarioSettings::from_profile(&profile);
        if change_rules {
            settings.routing.rules.push(usque_core::RoutingRule {
                id: Uuid::new_v4(),
                kind: usque_core::RoutingMatch::Domain,
                target: "blocked.example".into(),
                action: usque_core::RoutingAction::Reject,
            });
        } else {
            settings.frontends.http = false;
        }
        let expected = settings.clone();
        let scene = save_scene(&service, settings, None).await;
        let plan = preview(&service, &scene).await;
        assert_eq!(
            plan["preview"]["apply_kind"],
            if change_rules {
                "hot_traffic_policy"
            } else {
                "hot_frontends"
            }
        );
        request(&service, activate_request(&scene, &plan, Uuid::new_v4()))
            .await
            .unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            loop {
                if !service.scenario_applying.load(Ordering::SeqCst)
                    && service.settings.lock().await.apply_status != ApplyStatus::Applying
                {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        let observed = request(&service, json!({"action":"list"})).await.unwrap();
        assert_eq!(observed["state"]["applied_matches"], true);
        let active = service.data_plane.lock().await;
        let active = active.as_ref().unwrap();
        assert!(expected.equivalent(&ScenarioSettings::from_profile(&active.profile)));
        assert_eq!(active.profile.congestion_control, running_algorithm);
        let crate::ActiveRuntime::Harness(runtime) = &active.runtime else {
            panic!("fixture must stay inert");
        };
        assert!(runtime.traffic_policy_updates > 0);
        assert_eq!(runtime.reconnect_count, 0);
        assert_eq!(
            service.config_snapshot().await.network.congestion_control,
            usque_core::CongestionControlAlgorithm::Bbr
        );
    }
}

#[tokio::test]
async fn session_only_preferences_do_not_require_a_protected_scene_handoff() {
    for change_rules in [false, true] {
        let (_directory, service) = fixture().await;
        let mut profile = service.config_snapshot().await.active_profile().unwrap();
        profile.frontends.tunnel = true;
        profile.kill_switch = true;
        service
            .install_test_session(profile.clone(), true, 0)
            .await
            .unwrap();
        assert!(!service.protected_chain_present().await);
        service
            .update_config(move |config| {
                config.profiles[0].name = "Renamed account".into();
                config.network.auto_connect = !profile.auto_connect;
                Ok(())
            })
            .await
            .unwrap();
        let mut settings = ScenarioSettings::from_profile(&profile);
        if change_rules {
            settings.routing.rules.push(usque_core::RoutingRule {
                id: Uuid::new_v4(),
                kind: usque_core::RoutingMatch::Domain,
                target: "blocked.example".into(),
                action: usque_core::RoutingAction::Reject,
            });
        }
        let scene = save_scene(&service, settings.clone(), None).await;
        let plan = preview(&service, &scene).await;
        assert_eq!(
            plan["preview"]["apply_kind"],
            if change_rules {
                "hot_traffic_policy"
            } else {
                "not_required"
            }
        );
        assert_eq!(plan["preview"]["needs_reconnect"], false);
        request(&service, activate_request(&scene, &plan, Uuid::new_v4()))
            .await
            .unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            loop {
                if !service.scenario_applying.load(Ordering::SeqCst)
                    && service.settings.lock().await.apply_status != ApplyStatus::Applying
                {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        let observed = request(&service, json!({"action":"list"})).await.unwrap();
        assert_eq!(observed["state"]["applied_matches"], true);
        assert_eq!(observed["state"]["apply_status"], "applied");
        {
            let active = service.data_plane.lock().await;
            let active = active.as_ref().unwrap();
            assert_eq!(active.profile.name, profile.name);
            assert_eq!(active.profile.auto_connect, profile.auto_connect);
            assert!(settings.equivalent(&ScenarioSettings::from_profile(&active.profile)));
            let crate::ActiveRuntime::Harness(runtime) = &active.runtime else {
                panic!("fixture must remain inert");
            };
            assert_eq!(runtime.reconnect_count, 0);
            assert_eq!(runtime.traffic_policy_updates > 0, change_rules);
        }
        let bound = Uuid::new_v4();
        service
            .update_config(move |config| {
                config.profiles.push(Account::new(bound, "Other account"));
                Ok(())
            })
            .await
            .unwrap();
        let bound_scene = save_scene(&service, settings, Some(bound)).await;
        let before = service.store.load().unwrap();
        let error = request(
            &service,
            json!({"action":"preview","id":bound_scene["id"],"expected_revision":bound_scene["revision"]}),
        )
        .await
        .unwrap_err();
        assert!(matches!(
            error,
            ControlServiceError::ScenarioUnavailable("SCENARIO_PROTECTION_UNAVAILABLE")
        ));
        assert_eq!(service.store.load().unwrap(), before);
    }
}

#[tokio::test]
async fn scene_noop_and_failed_hot_rollback_keep_private_listener_authentication() {
    let (_directory, service) = fixture().await;
    let password = b"scene-test-listener-password";
    service
        .vault
        .put(
            usque_core::SHARED_NETWORK_SECRET_ID,
            usque_platform::SecretRecord::ProxyPassword,
            password,
        )
        .await
        .unwrap();
    let mut profile = service.config_snapshot().await.active_profile().unwrap();
    profile.frontends.tunnel = false;
    profile.proxy.auth_username = Some("scene-test-listener-user".into());
    profile.proxy.auth_password = Some(zeroize::Zeroizing::new(password.to_vec()));
    service
        .install_test_session(profile.clone(), false, 0)
        .await
        .unwrap();
    {
        let public = service.settings.lock().await;
        assert!(
            public
                .applied_profile
                .as_ref()
                .unwrap()
                .proxy
                .auth_password
                .is_none()
        );
    }
    {
        let mut active = service.data_plane.lock().await;
        let crate::ActiveRuntime::Harness(runtime) = &mut active.as_mut().unwrap().runtime else {
            panic!("fixture must remain inert");
        };
        // A genuine no-op must never reach this injected publication failure.
        runtime.fail_routing_publish = true;
    }
    for change_output in [false, true] {
        let mut settings = ScenarioSettings::from_profile(&profile);
        if change_output {
            settings.frontends.http = false;
        }
        let scene = save_scene(&service, settings, None).await;
        let plan = preview(&service, &scene).await;
        assert_eq!(
            plan["preview"]["apply_kind"],
            if change_output {
                "hot_frontends"
            } else {
                "not_required"
            }
        );
        let response = request(&service, activate_request(&scene, &plan, Uuid::new_v4()))
            .await
            .unwrap();
        assert!(
            !response
                .to_string()
                .contains(std::str::from_utf8(password).unwrap())
        );
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            loop {
                if !service.scenario_applying.load(Ordering::SeqCst)
                    && service.settings.lock().await.apply_status != ApplyStatus::Applying
                {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        let observed = service.settings.lock().await;
        assert_eq!(
            observed.apply_status,
            if change_output {
                ApplyStatus::Failed
            } else {
                ApplyStatus::Applied
            }
        );
        assert!(
            observed
                .applied_profile
                .as_ref()
                .is_none_or(|p| p.proxy.auth_password.is_none())
        );
        drop(observed);
        let active = service.data_plane.lock().await;
        let active = active.as_ref().unwrap();
        assert!(
            active
                .profile
                .proxy
                .listener_credentials()
                .unwrap()
                .is_some()
        );
        assert_eq!(
            active
                .profile
                .proxy
                .auth_password
                .as_deref()
                .unwrap()
                .as_slice(),
            password
        );
        assert_eq!(
            active.profile.proxy.auth_username,
            profile.proxy.auth_username
        );
        assert_eq!(
            active.frontends, profile.frontends,
            "failed output mutation must restore its old listeners"
        );
        let crate::ActiveRuntime::Harness(runtime) = &active.runtime else {
            panic!("fixture must remain inert");
        };
        assert_eq!(runtime.reconfigure_count, if change_output { 2 } else { 0 });
        assert_eq!(runtime.reconnect_count, 0);
    }
    let saved = service.store.load().unwrap();
    assert!(
        !saved.network.frontends.http,
        "failed application retains the intended stored settings"
    );
    assert!(saved.network.proxy.auth_password.is_none());
    assert_eq!(
        service
            .vault
            .get(
                usque_core::SHARED_NETWORK_SECRET_ID,
                usque_platform::SecretRecord::ProxyPassword
            )
            .await
            .unwrap()
            .unwrap()
            .as_slice(),
        password
    );
}

#[tokio::test]
async fn applying_flags_clear_when_the_owned_worker_unwinds() {
    let (_directory, service) = fixture().await;
    let flags = ApplyingFlags::set(&service);
    assert!(service.scenario_applying.load(Ordering::SeqCst));
    let worker = tokio::spawn(async move {
        let _flags = flags;
        panic!("simulated scene worker failure");
    });
    assert!(worker.await.is_err());
    assert!(!service.scenario_applying.load(Ordering::SeqCst));
    assert!(!service.settings_applying.load(Ordering::SeqCst));
}

#[tokio::test]
async fn uncertain_activation_is_settled_from_storage_without_replay() {
    let (_directory, service) = fixture().await;
    let scene = save_scene(&service, ScenarioSettings::default(), None).await;
    let plan = preview(&service, &scene).await;
    let operation = Uuid::new_v4();
    request(&service, activate_request(&scene, &plan, operation))
        .await
        .unwrap();
    // Model a reply whose durable write was uncertain: the record is on disk
    // without the acknowledgement marker and this owner has no receipt.
    service
        .update_config(|config| {
            config.last_scenario_switch.as_mut().unwrap().acknowledged = false;
            Ok(())
        })
        .await
        .unwrap();
    service
        .settings
        .lock()
        .await
        .persisted_operation_ids
        .clear();
    *service.scenario_unconfirmed_operation.lock().await = Some(operation);
    let list = request(&service, json!({"action":"list"})).await.unwrap();
    assert!(list["unresolved_operation_id"].is_null());
    assert_eq!(list["persisted_operation_id"], operation.to_string());
    assert_eq!(list["last_operation_id"], operation.to_string());
    let stored = service.store.load().unwrap();
    assert!(stored.last_scenario_switch.unwrap().acknowledged);
    assert!(service.data_plane.lock().await.is_none());

    // An operation absent from storage never committed.
    let lost = Uuid::new_v4();
    *service.scenario_unconfirmed_operation.lock().await = Some(lost);
    let list = request(&service, json!({"action":"list"})).await.unwrap();
    assert!(list["unresolved_operation_id"].is_null());
    assert_ne!(list["last_operation_id"], lost.to_string());
}
