use super::*;

fn manual() -> ScenarioSettings {
    let mut settings = ScenarioSettings::default();
    let endpoint = settings.endpoint.as_mut().unwrap();
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
    assert_eq!(config.network.endpoint, manual().endpoint.unwrap());
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
fn schema_27_scenes_keep_inheriting_endpoints_after_migration_and_use() {
    let mut config = AppConfig::default();
    let settings = ScenarioSettings {
        endpoint: None,
        ..ScenarioSettings::default()
    };
    let scene = save(&mut config, settings, None);
    let ctx = context(&config);
    activate(&mut config, &scene, &ctx);
    let mut legacy = serde_json::to_value(&config).unwrap();
    legacy["schema_version"] = json!(27);
    for snapshot in ["source", "target"] {
        legacy["last_scenario_switch"][snapshot]["settings"]
            .as_object_mut()
            .unwrap()
            .remove("endpoint");
    }
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("config.json");
    std::fs::write(&path, serde_json::to_vec(&legacy).unwrap()).unwrap();
    let mut config = crate::storage::ConfigStore::new(path).load().unwrap();
    assert_eq!(config.schema_version, crate::config::CURRENT_SCHEMA_VERSION);
    assert_eq!(config.scenarios[0], scene);
    config.network.endpoint = manual().endpoint.unwrap();
    let before = config.network.clone();
    activate(&mut config, &scene, &ctx);
    assert_eq!(config.network, before);
    assert_eq!(
        catalogue(&config, &context(&config))["state"]["modified"],
        false
    );
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
    assert_eq!(
        config.active_profile().unwrap().endpoint,
        manual().endpoint.unwrap()
    );
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
            settings.endpoint.as_mut().unwrap().selection = crate::EndpointSelection::Custom;
            settings.endpoint.as_mut().unwrap().preset_region = preset;
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
        let endpoint = settings.endpoint.as_mut().unwrap();
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
