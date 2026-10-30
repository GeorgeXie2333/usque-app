use super::*;

fn context(config: &AppConfig) -> ScenarioContext {
    ScenarioContext {
        source_epoch: Uuid::new_v4(),
        session_id: Some("1".into()),
        applied_profile: config.active_profile(),
        apply_status: ApplyStatus::Applied,
        dependency_fingerprint: "verified-library-1".into(),
    }
}

fn save(
    config: &mut AppConfig,
    settings: ScenarioSettings,
    bound: Option<Uuid>,
) -> ConnectionScenario {
    command(
        config,
        ScenarioCommand::Save {
            id: None,
            name: "Work".into(),
            bound_account_id: bound,
            settings: Box::new(settings),
            expected_revision: None,
        },
        &ScenarioContext::default(),
    )
    .unwrap();
    config.scenarios.last().unwrap().clone()
}

fn activate(
    config: &mut AppConfig,
    scenario: &ConnectionScenario,
    context: &ScenarioContext,
) -> ScenarioCommandResult {
    let preview = command(
        config,
        ScenarioCommand::Preview {
            id: scenario.id,
            expected_revision: scenario.revision,
        },
        context,
    )
    .unwrap();
    command(
        config,
        ScenarioCommand::Activate {
            id: scenario.id,
            expected_revision: scenario.revision,
            preview_token: preview.response["preview"]["preview_token"]
                .as_str()
                .unwrap()
                .into(),
            operation_id: Uuid::new_v4(),
        },
        context,
    )
    .unwrap()
}

#[test]
fn projection_preserves_global_settings_and_excludes_credentials() {
    let mut config = AppConfig::default();
    config.network.kill_switch = false;
    config.network.auto_connect = true;
    config.network.mtu = 1400;
    config.network.proxy.auth_username = Some("local-user".into());
    config.network.proxy.auth_password =
        Some(zeroize::Zeroizing::new(b"private-password".to_vec()));
    config.network.proxy.socks5_listeners = vec!["127.0.0.1:2000".parse().unwrap()];
    let original = config.network.clone();
    let mut settings = ScenarioSettings::default();
    settings.frontends.http = false;
    let ctx = context(&config);
    let scenario = save(&mut config, settings, None);
    let result = activate(&mut config, &scenario, &ctx);
    assert!(!config.network.frontends.http);
    assert_eq!(config.network.kill_switch, original.kill_switch);
    assert_eq!(config.network.auto_connect, original.auto_connect);
    assert_eq!(config.network.mtu, original.mtu);
    assert_eq!(
        config.network.proxy.auth_password,
        original.proxy.auth_password
    );
    assert_eq!(
        config.network.proxy.socks5_listeners,
        original.proxy.socks5_listeners
    );
    let json = serde_json::to_string(&result.response).unwrap();
    assert!(!json.contains("private-password"));
    assert!(!json.contains("local-user"));
    assert!(!json.contains("persisted"));
}

#[test]
fn save_is_not_activation_and_requires_expected_revision() {
    let mut config = AppConfig::default();
    let before = config.network.clone();
    let scenario = save(&mut config, ScenarioSettings::default(), None);
    assert_eq!(config.network, before);
    assert!(config.scenario_selection.is_none());
    let original = config.clone();
    let result = command(
        &mut config,
        ScenarioCommand::Save {
            id: Some(scenario.id),
            name: "Changed".into(),
            bound_account_id: None,
            settings: Box::new(scenario.settings.clone()),
            expected_revision: Some(Uuid::new_v4()),
        },
        &ScenarioContext::default(),
    );
    assert_eq!(result.unwrap_err(), ScenarioError::StaleRevision);
    assert_eq!(config, original);
}

#[test]
fn save_response_identifies_its_record_in_a_previously_unseen_catalogue() {
    let mut config = AppConfig::default();
    let first = save(&mut config, ScenarioSettings::default(), None);
    let saved = command(
        &mut config,
        ScenarioCommand::Save {
            id: None,
            name: "Second".into(),
            bound_account_id: None,
            settings: Box::default(),
            expected_revision: None,
        },
        &ScenarioContext::default(),
    )
    .unwrap();
    let second = config.scenarios.last().unwrap().clone();
    assert_ne!(first.id, second.id);
    assert_eq!(saved.response["saved_scenario_id"], json!(second.id));
    let renamed = command(
        &mut config,
        ScenarioCommand::Save {
            id: Some(first.id),
            name: "Renamed".into(),
            bound_account_id: None,
            settings: Box::new(first.settings),
            expected_revision: Some(first.revision),
        },
        &ScenarioContext::default(),
    )
    .unwrap();
    assert_eq!(renamed.response["saved_scenario_id"], json!(first.id));
}

#[test]
fn tunnel_only_scene_preview_uses_native_platform_reconnect_policy() {
    let mut config = AppConfig::default();
    config.network.endpoint.selection = crate::EndpointSelection::Custom;
    let ctx = context(&config);
    let mut settings = ScenarioSettings::from_network(&config.network);
    settings.frontends.tunnel = false;
    let scene = save(&mut config, settings, None);
    let target = target_for(&config, &scene.settings, config.active_profile_id.unwrap()).unwrap();
    assert_eq!(
        crate::classify_reconfigure(ctx.applied_profile.as_ref().unwrap(), &target),
        crate::ReconfigureClass::HotTunnelAttach
    );
    let result = command(
        &mut config,
        ScenarioCommand::Preview {
            id: scene.id,
            expected_revision: scene.revision,
        },
        &ctx,
    )
    .unwrap();
    assert_eq!(
        result.response["preview"]["needs_reconnect"],
        cfg!(target_os = "linux")
    );
    assert_eq!(
        result.response["preview"]["apply_kind"],
        if cfg!(target_os = "linux") {
            "cold_reconnect"
        } else {
            "hot_tunnel_attach"
        }
    );
}

#[test]
fn rule_order_and_rule_ids_do_not_make_a_scenario_modified() {
    let mut settings = ScenarioSettings::default();
    settings.routing.rules = vec![
        crate::RoutingRule {
            id: Uuid::new_v4(),
            kind: crate::RoutingMatch::Domain,
            target: "EXAMPLE.com.".into(),
            action: crate::RoutingAction::Proxy,
        },
        crate::RoutingRule {
            id: Uuid::new_v4(),
            kind: crate::RoutingMatch::Cidr,
            target: "192.0.2.5/24".into(),
            action: crate::RoutingAction::Reject,
        },
    ];
    let mut other = settings.normalized().unwrap();
    other.routing.rules.reverse();
    for rule in &mut other.routing.rules {
        rule.id = Uuid::new_v4();
    }
    assert!(settings.equivalent(&other));
    other.routing.rules[0].action = crate::RoutingAction::Direct;
    assert!(!settings.equivalent(&other));
}

#[test]
fn bound_account_and_settings_commit_together_and_manual_account_changes_are_modified() {
    let mut config = AppConfig::default();
    let first = config.active_profile_id.unwrap();
    let second = Uuid::new_v4();
    config.profiles.push(crate::Account::new(second, "Second"));
    let mut settings = ScenarioSettings::default();
    settings.frontends.http = false;
    let scenario = save(&mut config, settings, Some(second));
    let ctx = context(&config);
    let result = activate(&mut config, &scenario, &ctx);
    assert_eq!(config.active_profile_id, Some(second));
    assert_eq!(result.target.unwrap().id, second);
    assert!(!config.network.frontends.http);
    config.active_profile_id = Some(first);
    assert_eq!(catalogue(&config, &ctx)["state"]["modified"], true);
    assert_eq!(config.scenarios[0].bound_account_id, Some(second));
}

#[test]
fn missing_binding_does_not_break_storage_but_blocks_activation() {
    let mut config = AppConfig::default();
    let missing = Uuid::new_v4();
    config
        .profiles
        .push(crate::Account::new(missing, "Deleted"));
    let scenario = save(&mut config, ScenarioSettings::default(), Some(missing));
    config.profiles.retain(|account| account.id != missing);
    config.validate().unwrap();
    // Renaming keeps the dangling binding the user still has to repair.
    let renamed = command(
        &mut config.clone(),
        ScenarioCommand::Save {
            id: Some(scenario.id),
            name: "Renamed".into(),
            bound_account_id: Some(missing),
            settings: Box::new(scenario.settings.clone()),
            expected_revision: Some(scenario.revision),
        },
        &ScenarioContext::default(),
    );
    assert!(renamed.is_ok());
    let error = command(
        &mut config,
        ScenarioCommand::Preview {
            id: scenario.id,
            expected_revision: scenario.revision,
        },
        &ScenarioContext::default(),
    )
    .unwrap_err();
    assert_eq!(error, ScenarioError::AccountUnavailable);
}

#[test]
fn previews_bind_configuration_runtime_and_resources() {
    for change in 0..4 {
        let mut config = AppConfig::default();
        let scenario = save(&mut config, ScenarioSettings::default(), None);
        let mut ctx = context(&config);
        let result = command(
            &mut config,
            ScenarioCommand::Preview {
                id: scenario.id,
                expected_revision: scenario.revision,
            },
            &ctx,
        )
        .unwrap();
        let token = result.response["preview"]["preview_token"]
            .as_str()
            .unwrap()
            .into();
        match change {
            0 => config.network.kill_switch = !config.network.kill_switch,
            1 => ctx.source_epoch = Uuid::new_v4(),
            2 => ctx.session_id = Some("2".into()),
            _ => ctx.dependency_fingerprint = "changed".into(),
        }
        let before = config.clone();
        assert_eq!(
            command(
                &mut config,
                ScenarioCommand::Activate {
                    id: scenario.id,
                    expected_revision: scenario.revision,
                    preview_token: token,
                    operation_id: Uuid::new_v4()
                },
                &ctx
            )
            .unwrap_err(),
            ScenarioError::PreviewStale
        );
        assert_eq!(config, before);
    }
}

#[test]
fn activation_includes_changes_pending_in_running_session() {
    let mut config = AppConfig::default();
    let mut ctx = context(&config);
    ctx.applied_profile.as_mut().unwrap().frontends.http = false;
    let scenario = save(&mut config, ScenarioSettings::default(), None);
    let response = command(
        &mut config,
        ScenarioCommand::Preview {
            id: scenario.id,
            expected_revision: scenario.revision,
        },
        &ctx,
    )
    .unwrap()
    .response;
    assert!(
        response["preview"]["changed_fields"]
            .as_array()
            .unwrap()
            .contains(&json!("frontends.http"))
    );
    assert_eq!(response["state"]["applied_matches"], false);
}

#[test]
fn restore_preserves_new_global_values_and_refuses_new_managed_edits() {
    let mut config = AppConfig::default();
    let ctx = context(&config);
    let mut settings = ScenarioSettings::default();
    settings.frontends.http = false;
    let scenario = save(&mut config, settings, None);
    activate(&mut config, &scenario, &ctx);
    assert!(can_restore(&config));
    config.network.auto_connect = true;
    let preview = command(&mut config, ScenarioCommand::PreviewRestore, &ctx).unwrap();
    command(
        &mut config,
        ScenarioCommand::Restore {
            preview_token: preview.response["preview"]["preview_token"]
                .as_str()
                .unwrap()
                .into(),
            operation_id: Uuid::new_v4(),
        },
        &ctx,
    )
    .unwrap();
    assert!(config.network.frontends.http);
    assert!(config.network.auto_connect);
    assert!(!can_restore(&config));
    activate(&mut config, &scenario, &ctx);
    config.network.frontends.socks5 = false;
    assert!(!can_restore(&config));
}

#[test]
fn edited_template_does_not_change_selected_snapshot_and_deleting_does_not_change_network() {
    let mut config = AppConfig::default();
    let scenario = save(&mut config, ScenarioSettings::default(), None);
    let ctx = context(&config);
    activate(&mut config, &scenario, &ctx);
    let before = config.network.clone();
    let mut settings = scenario.settings.clone();
    settings.frontends.http = false;
    command(
        &mut config,
        ScenarioCommand::Save {
            id: Some(scenario.id),
            name: "New template".into(),
            bound_account_id: None,
            settings: Box::new(settings),
            expected_revision: Some(scenario.revision),
        },
        &ctx,
    )
    .unwrap();
    assert_eq!(config.network, before);
    assert_eq!(catalogue(&config, &ctx)["state"]["template_updated"], true);
    let updated = config.scenarios[0].clone();
    command(
        &mut config,
        ScenarioCommand::Delete {
            id: updated.id,
            expected_revision: updated.revision,
        },
        &ctx,
    )
    .unwrap();
    assert_eq!(config.network, before);
    assert!(config.scenario_selection.is_none());
}

#[test]
fn dependency_helpers_retain_inactive_scenarios_and_restore_snapshots() {
    let mut config = AppConfig::default();
    let id = Uuid::new_v4();
    let revision = Uuid::new_v4();
    let settings = ScenarioSettings {
        chain_exit: Some(ChainExitSettings {
            enabled: false,
            source: crate::chain_exit::ChainSource::WireguardCustom,
            profile_id: Some(id),
            revision: Some(revision),
            ..Default::default()
        }),
        ..Default::default()
    };
    let scenario = save(&mut config, settings, None);
    assert_eq!(retained_chain_profiles(&config), vec![id]);
    let ctx = context(&config);
    activate(&mut config, &scenario, &ctx);
    let other = save(&mut config, ScenarioSettings::default(), None);
    activate(&mut config, &other, &ctx);
    command(
        &mut config,
        ScenarioCommand::Delete {
            id: scenario.id,
            expected_revision: scenario.revision,
        },
        &ctx,
    )
    .unwrap();
    assert_eq!(retained_chain_profiles(&config), vec![id]);
}

#[test]
fn inputs_are_bounded_and_secret_fields_rejected() {
    assert_eq!(
        ScenarioCommand::parse(&" ".repeat(MAX_COMMAND_BYTES + 1)).unwrap_err(),
        ScenarioError::Limit
    );
    let mut settings = serde_json::to_value(ScenarioSettings::default()).unwrap();
    settings["auth_password"] = json!("secret");
    assert!(serde_json::from_value::<ScenarioSettings>(settings).is_err());
    let mut config = AppConfig::default();
    for _ in 0..MAX_SCENARIOS {
        save(&mut config, ScenarioSettings::default(), None);
    }
    let before = config.clone();
    let error = command(
        &mut config,
        ScenarioCommand::Save {
            id: None,
            name: "Excess".into(),
            bound_account_id: None,
            settings: Box::default(),
            expected_revision: None,
        },
        &ScenarioContext::default(),
    )
    .unwrap_err();
    assert_eq!(error, ScenarioError::Limit);
    assert_eq!(config, before);
}

#[test]
fn rename_does_not_mark_template_updated_and_global_edits_do_not_mark_modified() {
    let mut config = AppConfig::default();
    let value = save(&mut config, ScenarioSettings::default(), None);
    let ctx = context(&config);
    activate(&mut config, &value, &ctx);
    command(
        &mut config,
        ScenarioCommand::Save {
            id: Some(value.id),
            name: "Renamed".into(),
            bound_account_id: None,
            settings: Box::new(value.settings),
            expected_revision: Some(value.revision),
        },
        &ctx,
    )
    .unwrap();
    config.network.kill_switch = !config.network.kill_switch;
    config.network.mtu = 1400;
    let result = catalogue(&config, &ctx);
    assert_eq!(result["state"]["template_updated"], false);
    assert_eq!(result["state"]["modified"], false);
}

#[test]
fn repeated_operation_cannot_replay_a_runtime_target() {
    let mut config = AppConfig::default();
    let value = save(&mut config, ScenarioSettings::default(), None);
    let ctx = context(&config);
    activate(&mut config, &value, &ctx);
    let operation_id = config.last_scenario_switch.as_ref().unwrap().operation_id;
    let preview = command(
        &mut config,
        ScenarioCommand::Preview {
            id: value.id,
            expected_revision: value.revision,
        },
        &ctx,
    )
    .unwrap();
    let before = config.clone();
    assert_eq!(
        command(
            &mut config,
            ScenarioCommand::Activate {
                id: value.id,
                expected_revision: value.revision,
                operation_id,
                preview_token: preview.response["preview"]["preview_token"]
                    .as_str()
                    .unwrap()
                    .into(),
            },
            &ctx
        )
        .unwrap_err(),
        ScenarioError::OperationReused
    );
    assert_eq!(config, before);
}

#[test]
fn schema_twenty_six_migration_preserves_account_and_network_and_starts_without_scenarios() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("profiles-v2.json");
    let mut config = AppConfig::default();
    config.network.kill_switch = false;
    config.network.frontends.http = false;
    config.profiles[0].name = "Existing account".into();
    let mut legacy = serde_json::to_value(&config).unwrap();
    legacy["schema_version"] = json!(26);
    for key in ["scenarios", "scenario_selection", "last_scenario_switch"] {
        legacy.as_object_mut().unwrap().remove(key);
    }
    std::fs::write(&path, serde_json::to_vec(&legacy).unwrap()).unwrap();
    let store = crate::storage::ConfigStore::new(path);
    let migrated = store.load().unwrap();
    assert_eq!(migrated.schema_version, 27);
    assert_eq!(migrated.network, config.network);
    assert_eq!(migrated.profiles, config.profiles);
    assert_eq!(migrated.active_profile_id, config.active_profile_id);
    assert!(migrated.scenarios.is_empty());
    assert!(migrated.scenario_selection.is_none());
    assert!(migrated.last_scenario_switch.is_none());
    let backup: Value =
        serde_json::from_slice(&std::fs::read(store.backup_path()).unwrap()).unwrap();
    assert_eq!(backup, legacy);
}

#[test]
fn same_meaning_rules_preserve_existing_runtime_ids() {
    let mut config = AppConfig::default();
    let id = Uuid::new_v4();
    config.network.routing.rules.push(crate::RoutingRule {
        id,
        kind: crate::RoutingMatch::Domain,
        target: "example.com".into(),
        action: crate::RoutingAction::Proxy,
    });
    let mut settings = ScenarioSettings::from_network(&config.network);
    settings.routing.rules[0].id = Uuid::new_v4();
    let value = save(&mut config, settings, None);
    let ctx = context(&config);
    let preview = command(
        &mut config,
        ScenarioCommand::Preview {
            id: value.id,
            expected_revision: value.revision,
        },
        &ctx,
    )
    .unwrap();
    assert!(
        preview.response["preview"]["actual_changed_fields"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(preview.response["preview"]["apply_kind"], "not_required");
    activate(&mut config, &value, &ctx);
    assert_eq!(config.network.routing.rules[0].id, id);
}

#[test]
fn owner_epoch_is_required_for_preview() {
    let mut config = AppConfig::default();
    let value = save(&mut config, ScenarioSettings::default(), None);
    assert_eq!(
        command(
            &mut config,
            ScenarioCommand::Preview {
                id: value.id,
                expected_revision: value.revision
            },
            &ScenarioContext::default()
        )
        .unwrap_err(),
        ScenarioError::PreviewStale
    );
}

#[test]
fn durable_acknowledgement_is_separate_and_does_not_invalidate_preview() {
    let mut config = AppConfig::default();
    let scene = save(&mut config, ScenarioSettings::default(), None);
    let ctx = context(&config);
    activate(&mut config, &scene, &ctx);
    let operation = config.last_scenario_switch.as_ref().unwrap().operation_id;
    assert!(!config.last_scenario_switch.as_ref().unwrap().acknowledged);
    let plan = command(&mut config, ScenarioCommand::PreviewRestore, &ctx).unwrap();
    let original_network = config.network.clone();
    assert!(!acknowledge_operation(&mut config, Uuid::new_v4()));
    assert!(acknowledge_operation(&mut config, operation));
    assert!(!acknowledge_operation(&mut config, operation));
    assert_eq!(config.network, original_network);
    command(
        &mut config,
        ScenarioCommand::Restore {
            preview_token: plan.response["preview"]["preview_token"]
                .as_str()
                .unwrap()
                .into(),
            operation_id: Uuid::new_v4(),
        },
        &ctx,
    )
    .unwrap();
    assert!(!config.last_scenario_switch.as_ref().unwrap().acknowledged);
}

#[test]
fn oversized_current_configuration_keeps_catalogue_and_deletion_available() {
    let mut config = AppConfig::default();
    for index in 0..256 {
        config.network.routing.rules.push(crate::RoutingRule {
            id: Uuid::new_v4(),
            kind: crate::RoutingMatch::Domain,
            target: format!(
                "r{index}.{}.{}.{}.example",
                "a".repeat(60),
                "b".repeat(60),
                "c".repeat(60)
            ),
            action: crate::RoutingAction::Proxy,
        });
    }
    config.validate().unwrap();
    assert!(
        encoded_size(&ScenarioSettings::from_network(&config.network)).unwrap()
            > MAX_SCENARIO_BYTES
    );
    let value = save(&mut config, ScenarioSettings::default(), None);
    let response = command(
        &mut config,
        ScenarioCommand::List {
            known_revision: None,
        },
        &ScenarioContext::default(),
    )
    .unwrap()
    .response;
    assert!(response["current_settings"].is_null());
    assert_eq!(response["current_settings_error"], "SCENARIO_LIMIT");
    assert_eq!(response["scenarios"].as_array().unwrap().len(), 1);
    assert!(encoded_size(&response).unwrap() < MAX_RESPONSE_BYTES);
    command(
        &mut config,
        ScenarioCommand::Delete {
            id: value.id,
            expected_revision: value.revision,
        },
        &ScenarioContext::default(),
    )
    .unwrap();
    assert!(config.scenarios.is_empty());
    assert_eq!(config.network.routing.rules.len(), 256);
}

#[test]
fn all_command_envelopes_reject_owner_observations_and_secret_fields() {
    for action in ["list", "preview_restore"] {
        for field in [
            "applied_profile",
            "source_epoch",
            "session_id",
            "dependency_fingerprint",
            "auth_password",
            "target",
        ] {
            let mut request = json!({"action":action});
            request[field] = json!({});
            assert_eq!(
                ScenarioCommand::parse(&request.to_string()).unwrap_err(),
                ScenarioError::InvalidRequest
            );
        }
    }
    assert!(matches!(
        ScenarioCommand::parse(r#"{"action":"list"}"#).unwrap(),
        ScenarioCommand::List {
            known_revision: None,
        }
    ));
    assert!(matches!(
        ScenarioCommand::parse(r#"{"action":"preview_restore"}"#).unwrap(),
        ScenarioCommand::PreviewRestore
    ));
}

#[test]
fn completed_and_invalidated_restore_records_do_not_retain_unused_exits() {
    for restore in [false, true] {
        let mut config = AppConfig::default();
        let id = Uuid::new_v4();
        let settings = ScenarioSettings {
            chain_exit: Some(ChainExitSettings {
                enabled: false,
                source: crate::chain_exit::ChainSource::WireguardCustom,
                profile_id: Some(id),
                revision: Some(Uuid::new_v4()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let with_exit = save(&mut config, settings, None);
        let ctx = context(&config);
        activate(&mut config, &with_exit, &ctx);
        if restore {
            let preview = command(&mut config, ScenarioCommand::PreviewRestore, &ctx).unwrap();
            command(
                &mut config,
                ScenarioCommand::Restore {
                    preview_token: preview.response["preview"]["preview_token"]
                        .as_str()
                        .unwrap()
                        .into(),
                    operation_id: Uuid::new_v4(),
                },
                &ctx,
            )
            .unwrap();
        } else {
            let plain = save(&mut config, ScenarioSettings::default(), None);
            activate(&mut config, &plain, &ctx);
            config.network.frontends.http = false;
        }
        assert!(!can_restore(&config));
        command(
            &mut config,
            ScenarioCommand::Delete {
                id: with_exit.id,
                expected_revision: with_exit.revision,
            },
            &ctx,
        )
        .unwrap();
        assert!(retained_chain_profiles(&config).is_empty());
        assert!(
            config.last_scenario_switch.is_some(),
            "receipt metadata survives dependency release"
        );
    }
}

#[test]
fn preview_preserves_same_account_session_only_globals() {
    let mut config = AppConfig::default();
    let ctx = context(&config);
    config.network.auto_connect = !config.network.auto_connect;
    config.network.congestion_control = crate::CongestionControlAlgorithm::Bbr;
    config.profiles[0].name = "Renamed account".into();
    let scene = save(&mut config, ScenarioSettings::default(), None);
    let response = command(
        &mut config,
        ScenarioCommand::Preview {
            id: scene.id,
            expected_revision: scene.revision,
        },
        &ctx,
    )
    .unwrap()
    .response;
    assert_eq!(response["preview"]["apply_kind"], "not_required");
    assert_eq!(response["preview"]["needs_reconnect"], false);
    assert!(
        response["preview"]["actual_changed_fields"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let stored = config.active_profile().unwrap();
    let same = runtime_target(&stored, ctx.applied_profile.as_ref().unwrap());
    assert_eq!(same.name, ctx.applied_profile.as_ref().unwrap().name);
    assert_eq!(
        same.auto_connect,
        ctx.applied_profile.as_ref().unwrap().auto_connect
    );
    assert_eq!(
        same.congestion_control,
        crate::CongestionControlAlgorithm::Cubic
    );
    let mut other = stored.clone();
    other.id = Uuid::new_v4();
    let new_identity = runtime_target(&other, ctx.applied_profile.as_ref().unwrap());
    assert_eq!(new_identity.auto_connect, stored.auto_connect);
    assert_eq!(new_identity.congestion_control, stored.congestion_control);
}

#[test]
fn scene_status_ignores_only_unmanaged_deferrals_not_failed_or_unknown_outcomes() {
    let mut config = AppConfig::default();
    let mut ctx = context(&config);
    config.network.congestion_control = crate::CongestionControlAlgorithm::Bbr;
    ctx.apply_status = ApplyStatus::Deferred;
    assert_eq!(catalogue(&config, &ctx)["state"]["apply_status"], "applied");
    assert_eq!(ctx.apply_status, ApplyStatus::Deferred);
    for status in [ApplyStatus::Failed, ApplyStatus::Unknown] {
        ctx.apply_status = status;
        assert_eq!(
            catalogue(&config, &ctx)["state"]["apply_status"],
            serde_json::to_value(status).unwrap()
        );
    }
    ctx.apply_status = ApplyStatus::Deferred;
    config.network.frontends.http = false;
    assert_eq!(
        catalogue(&config, &ctx)["state"]["apply_status"],
        "deferred"
    );
}

#[test]
fn applied_scene_uses_frozen_selection_and_current_transition_source() {
    let mut config = AppConfig::default();
    let mut ctx = context(&config);
    let mut first_settings = ScenarioSettings::default();
    first_settings.frontends.http = false;
    let first = save(&mut config, first_settings, None);
    activate(&mut config, &first, &ctx);
    ctx.applied_profile = config.active_profile();
    let running_first = ctx.applied_profile.clone();
    let observed = catalogue(&config, &ctx);
    assert_eq!(observed["applied_scenario_id"], first.id.to_string());
    assert_eq!(observed["applied_scene_confirmed"], true);

    let second = save(&mut config, ScenarioSettings::default(), None);
    activate(&mut config, &second, &ctx);
    ctx.apply_status = ApplyStatus::Applying;
    let pending = catalogue(&config, &ctx);
    assert_eq!(pending["selection"]["scenario_id"], second.id.to_string());
    assert_eq!(pending["applied_scenario_id"], first.id.to_string());
    assert_eq!(pending["state"]["applied_matches"], false);
    ctx.applied_profile = config.active_profile();
    ctx.apply_status = ApplyStatus::Applied;
    assert_eq!(
        catalogue(&config, &ctx)["applied_scenario_id"],
        second.id.to_string()
    );

    // Restoring also has a confirmed old source while its target is pending.
    let plan = command(&mut config, ScenarioCommand::PreviewRestore, &ctx).unwrap();
    command(
        &mut config,
        ScenarioCommand::Restore {
            preview_token: plan.response["preview"]["preview_token"]
                .as_str()
                .unwrap()
                .into(),
            operation_id: Uuid::new_v4(),
        },
        &ctx,
    )
    .unwrap();
    ctx.apply_status = ApplyStatus::Applying;
    assert_eq!(
        catalogue(&config, &ctx)["applied_scenario_id"],
        second.id.to_string()
    );
    ctx.applied_profile = running_first;
    assert_eq!(
        catalogue(&config, &ctx)["applied_scenario_id"],
        first.id.to_string()
    );
    ctx.applied_profile = None;
    ctx.apply_status = ApplyStatus::Unknown;
    let unknown = catalogue(&config, &ctx);
    assert_eq!(unknown["applied_scene_confirmed"], false);
    assert!(unknown["applied_scenario_id"].is_null());
}

#[test]
fn applied_scene_never_guesses_from_catalogue_or_mismatched_binding() {
    let mut config = AppConfig::default();
    let mut ctx = context(&config);
    let account = config.active_profile_id.unwrap();
    let scene = save(&mut config, ScenarioSettings::default(), Some(account));
    let custom = catalogue(&config, &ctx);
    assert_eq!(custom["applied_scene_confirmed"], true);
    assert!(custom["applied_scenario_id"].is_null());
    activate(&mut config, &scene, &ctx);
    let other = Uuid::new_v4();
    config.profiles.push(crate::Account::new(other, "Other"));
    config.active_profile_id = Some(other);
    ctx.applied_profile = config.active_profile();
    let modified = catalogue(&config, &ctx);
    assert_eq!(modified["applied_scene_confirmed"], true);
    assert!(modified["applied_scenario_id"].is_null());
    assert_eq!(modified["state"]["modified"], true);
}

#[test]
fn new_bindings_must_name_an_existing_account() {
    let mut config = AppConfig::default();
    let before = config.clone();
    let error = command(
        &mut config,
        ScenarioCommand::Save {
            id: None,
            name: "Work".into(),
            bound_account_id: Some(Uuid::new_v4()),
            settings: Box::default(),
            expected_revision: None,
        },
        &ScenarioContext::default(),
    )
    .unwrap_err();
    assert_eq!(error, ScenarioError::AccountUnavailable);
    assert_eq!(config, before);
}

#[test]
fn stored_scene_rejected_by_current_rules_loads_and_is_reported_unavailable() {
    let mut config = AppConfig::default();
    let valid = save(&mut config, ScenarioSettings::default(), None);
    let mut invalid = save(&mut config, ScenarioSettings::default(), None);
    // Models a snapshot accepted by an older release and rejected by a
    // tightened rule: duplicate rule ids fail routing normalization.
    let rule = crate::RoutingRule {
        id: Uuid::new_v4(),
        kind: crate::RoutingMatch::Domain,
        target: "example.com".into(),
        action: crate::RoutingAction::Direct,
    };
    invalid.settings.routing.rules = vec![
        rule.clone(),
        crate::RoutingRule {
            target: "example.net".into(),
            ..rule
        },
    ];
    config.scenarios[1] = invalid.clone();
    config.validate().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let store = crate::storage::ConfigStore::new(temp.path().join("profiles-v2.json"));
    std::fs::write(
        temp.path().join("profiles-v2.json"),
        serde_json::to_vec(&config).unwrap(),
    )
    .unwrap();
    let loaded = store.load().unwrap();
    assert_eq!(loaded.scenarios.len(), 2);
    let response = catalogue(&loaded, &ScenarioContext::default());
    assert_eq!(response["invalid_scenario_ids"], json!([invalid.id]));
    let mut working = loaded.clone();
    let error = command(
        &mut working,
        ScenarioCommand::Preview {
            id: invalid.id,
            expected_revision: invalid.revision,
        },
        &ScenarioContext::default(),
    )
    .unwrap_err();
    assert_eq!(error, ScenarioError::InvalidSettings);
    command(
        &mut working,
        ScenarioCommand::Preview {
            id: valid.id,
            expected_revision: valid.revision,
        },
        &context(&loaded),
    )
    .unwrap();
}

#[test]
fn status_poll_omits_unchanged_templates_but_keeps_state() {
    let mut config = AppConfig::default();
    save(&mut config, ScenarioSettings::default(), None);
    let list = |config: &mut AppConfig, known: Option<String>| {
        command(
            config,
            ScenarioCommand::List {
                known_revision: known,
            },
            &ScenarioContext::default(),
        )
        .unwrap()
        .response
    };
    let full = list(&mut config, None);
    let revision = full["catalogue_revision"].as_str().unwrap().to_owned();
    let poll = list(&mut config, Some(revision.clone()));
    assert!(poll.get("scenarios").is_none());
    assert_eq!(poll["scenarios_unchanged"], true);
    assert_eq!(poll["state"], full["state"]);
    save(&mut config, ScenarioSettings::default(), None);
    let changed = list(&mut config, Some(revision));
    assert_eq!(changed["scenarios"].as_array().unwrap().len(), 2);
    assert!(changed.get("scenarios_unchanged").is_none());
}

#[test]
fn preview_warns_when_a_scene_turns_off_vpn_output_under_kill_switch() {
    let mut config = AppConfig::default();
    config.network.frontends.tunnel = true;
    config.network.kill_switch = true;
    let mut settings = ScenarioSettings::from_network(&config.network);
    settings.frontends.tunnel = false;
    settings.frontends.socks5 = true;
    let scenario = save(&mut config, settings, None);
    let current = ScenarioSettings::from_network(&config.network);
    let keep = save(&mut config, current, None);
    let preview = |config: &mut AppConfig, scene: &ConnectionScenario| {
        let ctx = context(config);
        command(
            config,
            ScenarioCommand::Preview {
                id: scene.id,
                expected_revision: scene.revision,
            },
            &ctx,
        )
        .unwrap()
        .response["preview"]
            .clone()
    };
    let removed = preview(&mut config, &scenario);
    assert_eq!(removed["kill_switch_output_removed"], true);
    assert!(removed.get("settings").is_none());
    assert!(removed["target_settings"].is_object());
    assert_eq!(
        preview(&mut config, &keep)["kill_switch_output_removed"],
        false
    );
    config.network.kill_switch = false;
    assert_eq!(
        preview(&mut config, &scenario)["kill_switch_output_removed"],
        false
    );
}

/// A non-default projection shared with the GUI. Rust owns the field set; the
/// Flutter contract test reads the same fixture and checks its round trip.
fn gui_contract_sample() -> ScenarioSettings {
    let mut settings = ScenarioSettings::default();
    settings.frontends.tunnel = true;
    settings.frontends.socks5 = true;
    settings.frontends.http = false;
    settings.transport = crate::TransportPolicy::Http2;
    settings.ip_policy = crate::IpPolicy::PreferIpv6;
    settings.allow_lan = true;
    settings.routing.ads_enabled = true;
    settings.routing.rules = vec![crate::RoutingRule {
        id: Uuid::from_u128(0x5ce0_0000_0000_4000_8000_0000_0000_0001),
        kind: crate::RoutingMatch::Domain,
        target: "example.com".into(),
        action: crate::RoutingAction::Direct,
    }];
    settings.geo_direct_countries = vec!["CN".into()];
    settings.dns_mode = DnsMode::LocalConfigured;
    settings.dns_servers = vec![
        "1.1.1.1".parse().unwrap(),
        "2606:4700:4700::1111".parse().unwrap(),
    ];
    settings.proxy_dns_mode = ProxyDnsMode::LocalConfigured;
    settings.proxy_dns_servers = vec!["9.9.9.9".parse().unwrap()];
    settings.chain_exit = Some(ChainExitSettings {
        enabled: true,
        source: crate::chain_exit::ChainSource::WireguardCustom,
        profile_id: Some(Uuid::from_u128(0x5ce0_0000_0000_4000_8000_0000_0000_0002)),
        revision: Some(Uuid::from_u128(0x5ce0_0000_0000_4000_8000_0000_0000_0003)),
        endpoint_override: None,
        proton_server_id: None,
    });
    settings.normalized().unwrap()
}

#[test]
fn gui_scene_settings_contract_fixture_matches_the_rust_projection() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../apps/usque_gui/test/fixtures/scenario_settings_contract.json");
    let expected = serde_json::to_string_pretty(&json!({
        "default": ScenarioSettings::default().normalized().unwrap(),
        "sample": gui_contract_sample(),
    }))
    .unwrap()
        + "\n";
    if std::env::var_os("USQUE_UPDATE_CONTRACT_FIXTURES").is_some() {
        std::fs::write(&path, &expected).unwrap();
    }
    assert_eq!(
        std::fs::read_to_string(&path)
            .unwrap_or_default()
            .replace("\r\n", "\n"),
        expected,
        "scene settings changed; regenerate with USQUE_UPDATE_CONTRACT_FIXTURES=1 and update the GUI projection",
    );
}
