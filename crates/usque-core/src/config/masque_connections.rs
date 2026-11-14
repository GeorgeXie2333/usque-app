use super::*;

#[test]
fn defaults_and_missing_json_fields_use_three_connections() {
    let profile = Profile::default();
    assert_eq!(profile.masque_connections, 3);
    let mut value = serde_json::to_value(&profile).unwrap();
    value.as_object_mut().unwrap().remove("masque_connections");
    assert_eq!(
        serde_json::from_value::<Profile>(value)
            .unwrap()
            .masque_connections,
        3
    );
    let mut shared = serde_json::to_value(SharedNetworkSettings::default()).unwrap();
    shared.as_object_mut().unwrap().remove("masque_connections");
    assert_eq!(
        serde_json::from_value::<SharedNetworkSettings>(shared)
            .unwrap()
            .masque_connections,
        3
    );
}

#[test]
fn range_shared_settings_and_scenes_preserve_the_count() {
    assert_eq!(MAX_MASQUE_CONNECTIONS, 16);
    for count in 1..=16 {
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
    for count in [0, 17, 255] {
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
fn migration_defaults_old_schemas_and_preserves_explicit_schema_28_counts() {
    for (schema, count, expected) in [
        (27, None, 3),
        (27, Some(1), 3),
        (28, None, 3),
        (28, Some(1), 1),
        (28, Some(16), 16),
    ] {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.json");
        let mut config = serde_json::to_value(AppConfig::default()).unwrap();
        config["schema_version"] = schema.into();
        match count {
            Some(count) => config["network"]["masque_connections"] = count.into(),
            None => {
                config["network"]
                    .as_object_mut()
                    .unwrap()
                    .remove("masque_connections");
            }
        }
        std::fs::write(&path, serde_json::to_vec(&config).unwrap()).unwrap();
        let store = crate::storage::ConfigStore::new(&path);
        let loaded = store.load().unwrap();
        assert_eq!(loaded.schema_version, CURRENT_SCHEMA_VERSION);
        assert_eq!(
            loaded.network.masque_connections, expected,
            "schema {schema}, count {count:?}"
        );
        store.save(&loaded).unwrap();
        assert_eq!(store.load().unwrap().network.masque_connections, expected);
    }
}
