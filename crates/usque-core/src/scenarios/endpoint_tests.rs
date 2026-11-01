use super::*;

fn manual() -> ScenarioSettings {
    let mut settings = ScenarioSettings::default();
    let endpoint = &mut settings.endpoint;
    endpoint.selection = crate::EndpointSelection::Custom;
    endpoint.ipv4 = "192.0.2.10".parse().unwrap();
    endpoint.ipv6 = "2001:db8::10".parse().unwrap();
    endpoint.port = 8443;
    endpoint.sni = "edge.example.com".into();
    settings
}

#[test]
fn endpoints_preview_commit_and_restore_through_the_same_transaction() {
    let mut config = AppConfig::default();
    let before = config.network.clone();
    let scene = save(&mut config, manual(), None);
    let ctx = context(&config);
    let preview = command(
        &mut config,
        ScenarioCommand::Preview {
            id: scene.id,
            expected_revision: scene.revision,
        },
        &ctx,
    )
    .unwrap()
    .response;
    assert_eq!(config.network, before);
    assert_eq!(preview["preview"]["needs_reconnect"], true);
    assert_eq!(
        preview["preview"]["target_settings"]["endpoint"]["port"],
        8443
    );
    assert!(
        preview["preview"]["changed_fields"]
            .as_array()
            .unwrap()
            .contains(&json!("endpoint.ipv4"))
    );
    let activated = activate(&mut config, &scene, &ctx);
    assert_eq!(config.network.endpoint, manual().endpoint);
    assert_eq!(activated.response["state"]["modified"], false);
    assert_eq!(activated.response["state"]["applied_matches"], false);
    let ctx = context(&config);
    assert_eq!(catalogue(&config, &ctx)["state"]["applied_matches"], true);
    let restore = command(&mut config, ScenarioCommand::PreviewRestore, &ctx).unwrap();
    command(
        &mut config,
        ScenarioCommand::Restore {
            preview_token: restore.response["preview"]["preview_token"]
                .as_str()
                .unwrap()
                .into(),
            operation_id: Uuid::new_v4(),
        },
        &ctx,
    )
    .unwrap();
    assert_eq!(config.network, before);
}

#[test]
fn endpoint_edits_invalidate_preview_restore_and_selection_status() {
    let mut config = AppConfig::default();
    let scene = save(&mut config, manual(), None);
    let ctx = context(&config);
    let preview = command(
        &mut config,
        ScenarioCommand::Preview {
            id: scene.id,
            expected_revision: scene.revision,
        },
        &ctx,
    )
    .unwrap();
    config.network.endpoint.port = 444;
    let before = config.clone();
    assert_eq!(
        command(
            &mut config,
            ScenarioCommand::Activate {
                id: scene.id,
                expected_revision: scene.revision,
                preview_token: preview.response["preview"]["preview_token"]
                    .as_str()
                    .unwrap()
                    .into(),
                operation_id: Uuid::new_v4(),
            },
            &ctx
        )
        .unwrap_err(),
        ScenarioError::PreviewStale
    );
    assert_eq!(config, before);
    activate(&mut config, &scene, &ctx);
    config.network.endpoint.port = 445;
    assert!(!can_restore(&config));
    assert_eq!(catalogue(&config, &ctx)["state"]["modified"], true);
}

#[test]
fn scene_endpoints_are_required_in_templates_and_recovery_snapshots() {
    let mut config = AppConfig::default();
    let first = save(&mut config, ScenarioSettings::default(), None);
    let ctx = context(&config);
    activate(&mut config, &first, &ctx);
    let second = save(&mut config, manual(), None);
    activate(&mut config, &second, &ctx);
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("config.json");
    let store = crate::storage::ConfigStore::new(path.clone());
    for pointer in [
        "/scenarios/0/settings",
        "/scenario_selection/settings",
        "/last_scenario_switch/source/settings",
        "/last_scenario_switch/target/settings",
        "/last_scenario_switch/source/selection/settings",
        "/last_scenario_switch/target/selection/settings",
    ] {
        for null in [false, true] {
            let mut unsupported = serde_json::to_value(&config).unwrap();
            let settings = unsupported
                .pointer_mut(pointer)
                .unwrap()
                .as_object_mut()
                .unwrap();
            if null {
                settings.insert("endpoint".into(), Value::Null);
            } else {
                settings.remove("endpoint");
            }
            let bytes = serde_json::to_vec(&unsupported).unwrap();
            std::fs::write(&path, &bytes).unwrap();
            assert!(store.load().is_err(), "{pointer}");
            assert_eq!(std::fs::read(&path).unwrap(), bytes);
            assert!(!store.backup_path().exists());
        }
    }
}

#[test]
fn bound_zero_trust_manual_endpoints_preserve_consumer_and_other_accounts() {
    let mut config = AppConfig::default();
    let consumer = config.active_profile_id.unwrap();
    let zt = Uuid::new_v4();
    config.profiles.push(crate::Account::new(zt, "ZT"));
    config.identity_bindings.insert(
        zt,
        crate::identity::IdentityProvider::ZeroTrust {
            organization: "test".into(),
        },
    );
    let registered = crate::ManagedEndpointIps {
        ipv4: "162.159.197.2".parse().unwrap(),
        ipv6: "2606:4700:102::2".parse().unwrap(),
    };
    config
        .set_managed_endpoint_ips(zt, registered.clone())
        .unwrap();
    config.active_profile_id = Some(consumer);
    let shared = config.network.endpoint.clone();
    let scene = save(&mut config, manual(), Some(zt));
    let ctx = context(&config);
    let preview = command(
        &mut config,
        ScenarioCommand::Preview {
            id: scene.id,
            expected_revision: scene.revision,
        },
        &ctx,
    )
    .unwrap();
    assert_eq!(
        preview.response["preview"]["zero_trust_endpoint_warning"],
        true
    );
    assert!(
        config
            .account(zt)
            .unwrap()
            .zero_trust_endpoint_override
            .is_none()
    );
    activate(&mut config, &scene, &ctx);
    assert_eq!(config.active_profile_id, Some(zt));
    assert_eq!(config.active_profile().unwrap().endpoint, manual().endpoint);
    assert_eq!(
        config.account(zt).unwrap().managed_endpoint_ips,
        Some(registered)
    );
    assert_eq!(config.network.endpoint.ipv4, shared.ipv4);
    assert_eq!(config.network.endpoint.ipv6, shared.ipv6);
    assert_eq!(config.network.endpoint.selection, shared.selection);
    assert_eq!(
        catalogue(&config, &context(&config))["state"]["modified"],
        false
    );
    config.account_mut(zt).unwrap().zero_trust_endpoint_override = None;
    assert!(!can_restore(&config));
    assert_eq!(
        catalogue(&config, &context(&config))["state"]["modified"],
        true
    );
}

#[test]
fn automatic_and_presets_use_zero_trust_account_addresses_without_overriding_them() {
    let mut config = AppConfig::default();
    let id = config.active_profile_id.unwrap();
    config.identity_bindings.insert(
        id,
        crate::identity::IdentityProvider::ZeroTrust {
            organization: "test".into(),
        },
    );
    let registered = crate::ManagedEndpointIps {
        ipv4: "162.159.197.2".parse().unwrap(),
        ipv6: "2606:4700:102::2".parse().unwrap(),
    };
    config
        .set_managed_endpoint_ips(id, registered.clone())
        .unwrap();
    for preset in [None, Some(crate::EndpointPresetRegion::De)] {
        let mut settings = ScenarioSettings::default();
        if preset.is_some() {
            settings.endpoint.selection = crate::EndpointSelection::Custom;
            settings.endpoint.preset_region = preset;
            settings.ip_policy = crate::IpPolicy::Ipv6Only;
        }
        let scene = save(&mut config, settings, None);
        let ctx = context(&config);
        activate(&mut config, &scene, &ctx);
        let current = config.active_profile().unwrap();
        assert_eq!(current.endpoint.ipv4, registered.ipv4);
        assert_eq!(current.endpoint.preset_region, preset);
        assert!(
            config
                .account(id)
                .unwrap()
                .zero_trust_endpoint_override
                .is_none()
        );
        assert_eq!(
            catalogue(&config, &context(&config))["state"]["modified"],
            false
        );
    }
}

#[test]
fn invalid_endpoint_port_sni_and_preset_policy_never_save_a_template() {
    for field in ["port", "sni", "preset_region"] {
        let mut config = AppConfig::default();
        let before = config.clone();
        let mut settings = manual();
        let endpoint = &mut settings.endpoint;
        match field {
            "port" => endpoint.port = 0,
            "sni" => endpoint.sni = "https://example.com".into(),
            _ => endpoint.preset_region = Some(crate::EndpointPresetRegion::Us),
        }
        assert!(
            command(
                &mut config,
                ScenarioCommand::Save {
                    id: None,
                    name: "Invalid".into(),
                    bound_account_id: None,
                    settings: Box::new(settings),
                    expected_revision: None,
                },
                &ScenarioContext::default()
            )
            .is_err()
        );
        assert_eq!(config, before);
    }
}

#[test]
fn restore_freezes_zero_trust_preset_fallback_and_restores_override_ownership() {
    for original_override in [None, Some("192.0.2.20")] {
        let mut config = AppConfig::default();
        let id = config.active_profile_id.unwrap();
        config.identity_bindings.insert(
            id,
            crate::identity::IdentityProvider::ZeroTrust {
                organization: "test".into(),
            },
        );
        config
            .set_managed_endpoint_ips(
                id,
                crate::ManagedEndpointIps {
                    ipv4: "162.159.197.2".parse().unwrap(),
                    ipv6: "2606:4700:102::2".parse().unwrap(),
                },
            )
            .unwrap();
        if let Some(ipv4) = original_override {
            config.account_mut(id).unwrap().zero_trust_endpoint_override =
                Some(crate::ManagedEndpointIps {
                    ipv4: ipv4.parse().unwrap(),
                    ipv6: "2001:db8::20".parse().unwrap(),
                });
        }
        config.network.endpoint.selection = crate::EndpointSelection::Custom;
        config.network.endpoint.preset_region = Some(crate::EndpointPresetRegion::De);
        config.network.ip_policy = IpPolicy::PreferIpv6;
        let original = config.active_profile().unwrap().endpoint;
        let original_account = config.account(id).unwrap().clone();
        let scene = save(&mut config, manual(), None);
        let ctx = context(&config);
        activate(&mut config, &scene, &ctx);
        assert_ne!(
            config.active_profile().unwrap().endpoint.ipv4,
            original.ipv4
        );
        let preview_request = ScenarioCommand::PreviewRestore;
        let candidate = candidate_profile(&config, &preview_request)
            .unwrap()
            .unwrap();
        assert_eq!(candidate.endpoint, original);
        let preview = command(&mut config, preview_request, &ctx).unwrap();
        assert_eq!(
            preview.response["preview"]["target_settings"]["endpoint"],
            json!(original)
        );
        let result = command(
            &mut config,
            ScenarioCommand::Restore {
                operation_id: Uuid::new_v4(),
                preview_token: preview.response["preview"]["preview_token"]
                    .as_str()
                    .unwrap()
                    .into(),
            },
            &ctx,
        )
        .unwrap();
        assert_eq!(result.target.unwrap().endpoint, original);
        assert_eq!(config.active_profile().unwrap().endpoint, original);
        assert_eq!(config.account(id).unwrap(), &original_account);
    }
}

fn stored_switch() -> (
    tempfile::TempDir,
    crate::storage::ConfigStore,
    ConnectionScenario,
) {
    let directory = tempfile::tempdir().unwrap();
    let store = crate::storage::ConfigStore::new(directory.path().join("profiles.json"));
    let mut config = AppConfig::default();
    let ctx = context(&config);
    let scene = save(&mut config, ScenarioSettings::default(), None);
    activate(&mut config, &scene, &ctx);
    store.save(&config).unwrap();
    (directory, store, scene)
}

fn write(store: &crate::storage::ConfigStore, edit: impl FnOnce(&mut AppConfig)) -> AppConfig {
    store
        .update(|config| {
            edit(config);
            Ok::<_, crate::storage::StoreError>(())
        })
        .unwrap()
        .0
}

#[test]
fn persisted_managed_aba_edits_retire_undo_but_keep_pending_receipt_and_snapshots() {
    let (_directory, store, scene) = stored_switch();
    let original = store.load().unwrap();
    let receipt = original.last_scenario_switch.as_ref().unwrap().clone();
    assert!(!receipt.acknowledged);
    write(&store, |config| {
        config.network.ip_policy = IpPolicy::Ipv4Only
    });
    write(&store, |config| config.network.ip_policy = IpPolicy::Auto);
    let mut config = store.load().unwrap();
    assert!(!can_restore(&config));
    let record = config.last_scenario_switch.as_ref().unwrap();
    assert!(record.recovery_retired);
    assert_eq!(record.operation_id, receipt.operation_id);
    assert_eq!(record.source, receipt.source);
    assert_eq!(record.target, receipt.target);
    let ctx = context(&config);
    assert_eq!(
        command(&mut config, ScenarioCommand::PreviewRestore, &ctx).unwrap_err(),
        ScenarioError::RestoreUnavailable
    );
    // A late durable receipt still reconciles the original operation.
    write(&store, |config| {
        assert!(acknowledge_operation(config, receipt.operation_id));
    });
    let acknowledged = store.load().unwrap();
    assert!(
        acknowledged
            .last_scenario_switch
            .as_ref()
            .unwrap()
            .acknowledged
    );
    assert!(!can_restore(&acknowledged));
    // A stale same-operation copy cannot reset retirement.
    write(&store, |config| *config = original);
    let config = store.load().unwrap();
    assert!(!can_restore(&config));
    assert!(config.last_scenario_switch.as_ref().unwrap().acknowledged);
    // A fresh preview and activation get a fresh undo, without replaying the old one.
    let ctx = context(&config);
    write(&store, |config| {
        activate(config, &scene, &ctx);
    });
    let fresh = store.load().unwrap();
    assert!(can_restore(&fresh));
    assert_ne!(
        fresh.last_scenario_switch.as_ref().unwrap().operation_id,
        receipt.operation_id
    );
}

#[test]
fn global_name_template_and_normalized_rule_edits_preserve_undo() {
    let (_directory, store, scene) = stored_switch();
    let mut config = store.load().unwrap();
    config.network.routing.rules = vec![crate::RoutingRule {
        id: Uuid::new_v4(),
        kind: crate::RoutingMatch::Domain,
        target: "example.com".into(),
        action: crate::RoutingAction::Proxy,
    }];
    config.network.routing.rules.push(crate::RoutingRule {
        id: Uuid::new_v4(),
        kind: crate::RoutingMatch::Cidr,
        target: "192.0.2.0/24".into(),
        action: crate::RoutingAction::Direct,
    });
    let settings = ScenarioSettings::from_network(&config.network);
    let with_rule = save(&mut config, settings, None);
    let ctx = context(&config);
    activate(&mut config, &with_rule, &ctx);
    store.save(&config).unwrap();
    write(&store, |config| {
        config.network.auto_connect = !config.network.auto_connect;
        config.network.kill_switch = !config.network.kill_switch;
        config.network.mtu = 1400;
        config.profiles[0].name = "Renamed".into();
        config.scenarios[0].name = "Template renamed".into();
        config.scenarios[0].settings.transport = TransportPolicy::Http2;
        config.network.routing.rules[0].id = Uuid::new_v4();
        config.network.routing.rules[0].target = "EXAMPLE.COM.".into();
        config.network.routing.rules.reverse();
    });
    let config = store.load().unwrap();
    assert!(can_restore(&config));
    assert_eq!(config.scenarios[0].id, scene.id);
    let mut next = config.clone();
    retire_restore_after_change(&config, &mut next);
    assert_eq!(config, next);
}

#[test]
fn account_round_trip_cannot_restore_over_later_zero_trust_endpoint_edits() {
    let directory = tempfile::tempdir().unwrap();
    let store = crate::storage::ConfigStore::new(directory.path().join("profiles.json"));
    let mut config = AppConfig::default();
    let source = config.active_profile_id.unwrap();
    let target = Uuid::new_v4();
    config
        .profiles
        .push(crate::Account::new(target, "Consumer"));
    config.identity_bindings.insert(
        source,
        crate::identity::IdentityProvider::ZeroTrust {
            organization: "test".into(),
        },
    );
    config
        .set_managed_endpoint_ips(
            source,
            crate::ManagedEndpointIps {
                ipv4: "162.159.197.2".parse().unwrap(),
                ipv6: "2606:4700:102::2".parse().unwrap(),
            },
        )
        .unwrap();
    let ctx = context(&config);
    let scene = save(&mut config, ScenarioSettings::default(), Some(target));
    activate(&mut config, &scene, &ctx);
    store.save(&config).unwrap();
    // An inactive source can also change during identity reauthentication.
    // Retirement cannot rely solely on detecting an active-account switch.
    write(&store, |config| {
        config
            .set_managed_endpoint_ips(
                source,
                crate::ManagedEndpointIps {
                    ipv4: "162.159.197.9".parse().unwrap(),
                    ipv6: "2606:4700:102::9".parse().unwrap(),
                },
            )
            .unwrap();
    });
    assert!(!can_restore(&store.load().unwrap()));
    store.save(&config).unwrap();
    write(&store, |config| config.active_profile_id = Some(source));
    write(&store, |config| {
        let mut values = config.active_profile().unwrap();
        values.endpoint.ipv4 = "192.0.2.99".parse().unwrap();
        crate::network_settings::merge_patch(
            config,
            &crate::network_settings::NetworkSettingsPatch {
                operation_id: Uuid::new_v4(),
                account_id: source,
                values,
                changed_fields: vec!["endpoint.ipv4".into()],
            },
        )
        .unwrap();
    });
    write(&store, |config| config.active_profile_id = Some(target));
    let mut config = store.load().unwrap();
    assert!(!can_restore(&config));
    assert_eq!(
        config
            .runtime_profile(source)
            .unwrap()
            .endpoint
            .ipv4
            .to_string(),
        "192.0.2.99"
    );
    let ctx = context(&config);
    assert_eq!(
        command(&mut config, ScenarioCommand::PreviewRestore, &ctx).unwrap_err(),
        ScenarioError::RestoreUnavailable
    );
}

#[test]
fn dormant_consumer_endpoint_conflict_does_not_retire_undo_on_ack_or_global_edits() {
    let directory = tempfile::tempdir().unwrap();
    let store = crate::storage::ConfigStore::new(directory.path().join("profiles.json"));
    let mut config = AppConfig::default();
    let id = config.active_profile_id.unwrap();
    config.identity_bindings.insert(
        id,
        crate::identity::IdentityProvider::ZeroTrust {
            organization: "test".into(),
        },
    );
    config
        .set_managed_endpoint_ips(
            id,
            crate::ManagedEndpointIps {
                ipv4: "162.159.197.2".parse().unwrap(),
                ipv6: "2606:4700:102::2".parse().unwrap(),
            },
        )
        .unwrap();
    config.network.endpoint.selection = crate::EndpointSelection::Custom;
    config.network.dns_servers = vec![config.network.endpoint.ipv4.into()];
    config.network.routing.rules.push(crate::RoutingRule {
        id: Uuid::new_v4(),
        kind: crate::RoutingMatch::Domain,
        target: "example.com".into(),
        action: crate::RoutingAction::Proxy,
    });
    config.validate().unwrap();
    assert!(
        ScenarioSettings::from_network(&config.network)
            .normalized()
            .is_ok()
    );
    let mut settings = ScenarioSettings::from_profile(&config.active_profile().unwrap());
    settings.transport = TransportPolicy::Http2;
    store.save(&config).unwrap();
    let ctx = context(&config);
    write(&store, |config| {
        let scene = save(config, settings, None);
        activate(config, &scene, &ctx);
    });
    let selected = store.load().unwrap();
    assert!(can_restore(&selected));
    let operation = selected.last_scenario_switch.as_ref().unwrap().operation_id;
    write(&store, |config| {
        assert!(acknowledge_operation(config, operation));
    });
    let acknowledged = store.load().unwrap();
    assert!(can_restore(&acknowledged));
    assert!(
        acknowledged
            .last_scenario_switch
            .as_ref()
            .unwrap()
            .acknowledged
    );
    write(&store, |config| {
        config.network.auto_connect = true;
        config.network.mtu = 1400;
        config.profiles[0].name = "Renamed ZT account".into();
    });
    assert!(can_restore(&store.load().unwrap()));
    write(&store, |config| {
        config.network.routing.rules[0].id = Uuid::new_v4();
        config.network.routing.rules[0].target = "EXAMPLE.COM.".into();
    });
    assert!(can_restore(&store.load().unwrap()));
    write(&store, |config| {
        config.network.ip_policy = IpPolicy::Ipv4Only
    });
    assert!(!can_restore(&store.load().unwrap()));
}
