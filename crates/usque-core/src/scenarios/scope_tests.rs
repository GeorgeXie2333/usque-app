use super::*;

const REMOVED: &[&str] = &[
    "frontends",
    "allow_lan",
    "routing",
    "geo_direct_countries",
    "direct_dns",
    "warp_dns",
    "dns_mode",
    "dns_servers",
    "proxy_system_proxy",
    "proxy_dns_mode",
    "proxy_dns_servers",
];

fn globals(network: &SharedNetworkSettings) -> Value {
    let mut value = serde_json::to_value(network).unwrap();
    for key in [
        "endpoint",
        "transport",
        "ip_policy",
        "chain_exit",
        "vpn_gate",
    ] {
        value.as_object_mut().unwrap().remove(key);
    }
    value
}

#[test]
fn scene_activation_and_restore_preserve_all_removed_globals_and_binding() {
    let mut config = AppConfig::default();
    let source = config.active_profile_id.unwrap();
    let bound = Uuid::new_v4();
    config.profiles.push(crate::Account::new(bound, "Bound"));
    config.network.frontends.http = false;
    config.network.allow_lan = false;
    config.network.routing.ads_enabled = true;
    config.network.geo_direct_countries = vec!["CN".into()];
    config.network.dns_servers = vec!["9.9.9.9".parse().unwrap()];
    config.network.proxy.dns_mode = crate::ProxyDnsMode::LocalConfigured;
    config.network.proxy.dns_servers = vec!["8.8.8.8".parse().unwrap()];
    let original = globals(&config.network);
    let settings = ScenarioSettings {
        transport: TransportPolicy::Http2,
        ..Default::default()
    };
    let scene = save(&mut config, settings, Some(bound));
    let ctx = context(&config);
    activate(&mut config, &scene, &ctx);
    assert_eq!(config.active_profile_id, Some(bound));
    assert_eq!(config.network.transport, TransportPolicy::Http2);
    assert_eq!(globals(&config.network), original);
    let before = config.clone();
    config.network.frontends.http = true;
    config.network.allow_lan = true;
    config.network.routing.ads_enabled = false;
    config.network.geo_direct_countries.clear();
    config.network.dns_servers = vec!["8.8.4.4".parse().unwrap()];
    config.network.proxy.system_proxy = true;
    config.network.proxy.dns_servers = vec!["1.0.0.1".parse().unwrap()];
    retire_restore_after_change(&before, &mut config);
    assert!(can_restore(&config));
    let current = globals(&config.network);
    let observed = catalogue(&config, &context(&config));
    assert_eq!(observed["state"]["modified"], false);
    let preview = command(&mut config, ScenarioCommand::PreviewRestore, &ctx).unwrap();
    command(
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
    assert_eq!(config.active_profile_id, Some(source));
    assert_eq!(config.network.transport, TransportPolicy::Auto);
    assert_eq!(globals(&config.network), current);
}

#[test]
fn new_scene_commands_reject_removed_settings_instead_of_persisting_them() {
    for field in REMOVED {
        let mut settings = serde_json::to_value(ScenarioSettings::default()).unwrap();
        settings[*field] = json!({"removed": true});
        let request = json!({"action":"save", "name":"Unsupported fields", "settings":settings});
        assert_eq!(
            ScenarioCommand::parse(&request.to_string()).unwrap_err(),
            ScenarioError::InvalidRequest
        );
    }
    let config = AppConfig::default();
    let full = catalogue(&config, &ScenarioContext::default());
    assert!(full.get("settings_version").is_none());
    assert!(full.get("endpoint_editing").is_none());
    assert!(
        full.as_object()
            .unwrap()
            .contains_key("observation_session_id")
    );
    let mut config = config;
    let poll = command(
        &mut config,
        ScenarioCommand::List {
            known_revision: full["catalogue_revision"].as_str().map(str::to_owned),
        },
        &ScenarioContext::default(),
    )
    .unwrap()
    .response;
    assert!(poll.get("settings_version").is_none());
    assert!(poll.get("endpoint_editing").is_none());
    assert_eq!(poll["scenarios_unchanged"], true);
}

#[test]
fn pre_scene_configuration_initializes_empty_scenes_without_changing_global_settings() {
    let mut config = AppConfig {
        schema_version: 26,
        ..AppConfig::default()
    };
    config.network.allow_lan = false;
    config.network.frontends.http = false;
    config.network.endpoint.selection = crate::EndpointSelection::Custom;
    config.network.endpoint.preset_region = Some(crate::EndpointPresetRegion::De);
    config.network.ip_policy = IpPolicy::PreferIpv6;
    let mut old = serde_json::to_value(&config).unwrap();
    for key in ["scenarios", "scenario_selection", "last_scenario_switch"] {
        old.as_object_mut().unwrap().remove(key);
    }
    let directory = tempfile::tempdir().unwrap();
    let store = crate::storage::ConfigStore::new(directory.path().join("profiles.json"));
    std::fs::write(store.path(), serde_json::to_vec(&old).unwrap()).unwrap();
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
    assert_eq!(backup, old);
    assert_eq!(store.load().unwrap(), migrated);
}

#[test]
fn unsupported_development_schemas_and_scene_fields_are_not_migrated_or_rewritten() {
    let mut config = AppConfig::default();
    config.network.allow_lan = false;
    config.network.frontends.http = false;
    config.network.dns_servers = vec!["9.9.9.9".parse().unwrap()];
    let first = save(&mut config, ScenarioSettings::default(), None);
    let ctx = context(&config);
    activate(&mut config, &first, &ctx);
    let second = save(
        &mut config,
        ScenarioSettings {
            transport: TransportPolicy::Http2,
            ..Default::default()
        },
        None,
    );
    activate(&mut config, &second, &ctx);
    let operation = config.last_scenario_switch.as_ref().unwrap().operation_id;
    acknowledge_operation(&mut config, operation);
    let directory = tempfile::tempdir().unwrap();
    let store = crate::storage::ConfigStore::new(directory.path().join("profiles.json"));
    let rejected = |value: Value| {
        let bytes = serde_json::to_vec(&value).unwrap();
        std::fs::write(store.path(), &bytes).unwrap();
        assert!(store.load().is_err());
        assert_eq!(std::fs::read(store.path()).unwrap(), bytes);
        assert!(!store.backup_path().exists());
    };
    for version in [28, 29] {
        let mut value = serde_json::to_value(&config).unwrap();
        value["schema_version"] = json!(version);
        rejected(value);
    }
    for pointer in [
        "/scenarios/0/settings",
        "/scenario_selection/settings",
        "/last_scenario_switch/source/settings",
        "/last_scenario_switch/target/settings",
        "/last_scenario_switch/source/selection/settings",
        "/last_scenario_switch/target/selection/settings",
    ] {
        let mut value = serde_json::to_value(&config).unwrap();
        value.pointer_mut(pointer).unwrap()["frontends"] = json!({"tunnel":false});
        rejected(value);
    }
    for key in ["acknowledged", "recovery_retired"] {
        let mut value = serde_json::to_value(&config).unwrap();
        value["last_scenario_switch"]
            .as_object_mut()
            .unwrap()
            .remove(key);
        rejected(value);
    }
    let mut value = serde_json::to_value(&config).unwrap();
    value["last_scenario_switch"]["restored"] = json!(false);
    rejected(value);

    // Current records keep their durable acknowledgement and recovery eligibility.
    store.save(&config).unwrap();
    let loaded = store.load().unwrap();
    assert_eq!(loaded, config);
    assert!(loaded.last_scenario_switch.as_ref().unwrap().acknowledged);
    assert!(can_restore(&loaded));
}

#[test]
fn intrinsic_scene_validation_defers_unowned_dns_and_output_constraints_to_target() {
    let mut config = AppConfig::default();
    let mut settings = ScenarioSettings::default();
    let endpoint = &mut settings.endpoint;
    endpoint.selection = crate::EndpointSelection::Custom;
    endpoint.ipv4 = "1.1.1.1".parse().unwrap();
    assert!(settings.normalized().is_ok());
    let scene = save(&mut config, settings, None);
    let request = || ScenarioCommand::Preview {
        id: scene.id,
        expected_revision: scene.revision,
    };
    let ctx = context(&config);
    assert_eq!(
        command(&mut config, request(), &ctx).unwrap_err(),
        ScenarioError::InvalidSettings
    );
    config.network.dns_servers = vec!["9.9.9.9".parse().unwrap()];
    command(&mut config, request(), &ctx).unwrap();
}

#[test]
fn disabled_chain_keeps_its_reference_and_does_not_change_shared_dns() {
    let mut config = AppConfig::default();
    config.network.chain_exit = Some(ChainExitSettings {
        enabled: true,
        source: crate::chain_exit::ChainSource::WireguardCustom,
        profile_id: Some(Uuid::new_v4()),
        revision: Some(Uuid::new_v4()),
        ..Default::default()
    });
    let before = config.network.chain_exit.clone().unwrap();
    let mut settings = ScenarioSettings::from_network(&config.network);
    settings.chain_exit.as_mut().unwrap().enabled = false;
    let normalized = settings.normalized().unwrap();
    let disabled = normalized.chain_exit.unwrap();
    assert_eq!(disabled.profile_id, before.profile_id);
    assert_eq!(disabled.revision, before.revision);
    let original_globals = globals(&config.network);
    settings.apply_to(&mut config.network).unwrap();
    assert_eq!(globals(&config.network), original_globals);
    assert!(!config.network.chain_enabled());
}
