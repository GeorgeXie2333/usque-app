use super::*;

#[test]
fn defaults_and_legacy_json_keep_one_connection() {
    let profile = Profile::default();
    assert_eq!(profile.masque_connections, 1);
    let mut value = serde_json::to_value(&profile).unwrap();
    value.as_object_mut().unwrap().remove("masque_connections");
    assert_eq!(
        serde_json::from_value::<Profile>(value)
            .unwrap()
            .masque_connections,
        1
    );
    let mut shared = serde_json::to_value(SharedNetworkSettings::default()).unwrap();
    shared.as_object_mut().unwrap().remove("masque_connections");
    assert_eq!(
        serde_json::from_value::<SharedNetworkSettings>(shared)
            .unwrap()
            .masque_connections,
        1
    );
}

#[test]
fn range_shared_settings_and_scenes_preserve_the_count() {
    for count in 1..=6 {
        let profile = Profile {
            masque_connections: count,
            ..Default::default()
        };
        profile.validate().unwrap();
        let shared = SharedNetworkSettings::from_profile(&profile);
        assert_eq!(
            shared
                .hydrate(&Account::default_account())
                .masque_connections,
            count
        );
        let scene = crate::scenarios::ScenarioSettings::from_profile(&profile);
        let round_trip: crate::scenarios::ScenarioSettings =
            serde_json::from_value(serde_json::to_value(&scene).unwrap()).unwrap();
        assert!(scene.equivalent(&round_trip));
        let mut inherited = shared.clone();
        crate::scenarios::ScenarioSettings::default()
            .apply_to(&mut inherited)
            .unwrap();
        assert_eq!(inherited.masque_connections, count);
    }
    for count in [0, 7, 255] {
        let profile = Profile {
            masque_connections: count,
            ..Default::default()
        };
        assert!(matches!(
            profile.validate(),
            Err(ConfigError::InvalidMasqueConnections(_))
        ));
    }
}

#[test]
fn editing_connection_count_requires_a_reconnect() {
    let previous = Profile::default();
    let mut next = previous.clone();
    next.masque_connections = 2;
    assert_eq!(
        crate::classify_reconfigure(&previous, &next),
        crate::ReconfigureClass::ColdReconnect
    );
    assert!(
        crate::network_settings::changed_fields(&previous, &next)
            .contains(&"masque_connections".into())
    );
}

#[test]
fn schema_27_migration_preserves_the_single_connection_behavior() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("config.json");
    let mut config = serde_json::to_value(AppConfig::default()).unwrap();
    config["schema_version"] = 27.into();
    config["network"]
        .as_object_mut()
        .unwrap()
        .remove("masque_connections");
    std::fs::write(&path, serde_json::to_vec(&config).unwrap()).unwrap();
    let loaded = crate::storage::ConfigStore::new(&path).load().unwrap();
    assert_eq!(loaded.schema_version, CURRENT_SCHEMA_VERSION);
    assert_eq!(loaded.network.masque_connections, 1);
}
