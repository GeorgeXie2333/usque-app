use super::*;
use proptest::prelude::*;
use serde_json::{Value, json};

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]
    #[test]
    fn arbitrary_directory_and_configuration_input_is_bounded(
        bytes in proptest::collection::vec(any::<u8>(), 0..8192),
        directive in ".{0,2048}",
    ) {
        if let Ok(catalogue) = Catalogue::parse(&bytes) {
            let list = catalogue.list(&ListQuery::default());
            prop_assert!(list.source_server_count <= MAX_SERVERS);
            prop_assert!(list.servers.len() <= 100);
        }
        if let Ok(profile) = prepare_profile(&directive, "8.8.8.8".parse().unwrap()) {
            prop_assert_eq!(profile.remote.ip(), "8.8.8.8".parse::<IpAddr>().unwrap());
            prop_assert!(profile.content().len() <= MAX_CONFIG_BYTES);
            prop_assert!(profile.content().contains("tls-version-min"));
        }
    }
    #[test]
    fn mutated_valid_catalogue_cannot_change_a_selected_remote_without_validation(
        offset in 0_usize..8192,
        replacement in any::<u8>(),
        truncate in any::<bool>(),
    ) {
        let mut data = fixture();
        let index = offset % data.len();
        if truncate { data.truncate(index); } else { data[index] = replacement; }
        if let Ok(catalogue) = Catalogue::parse(&data) {
            for server in catalogue.list(&ListQuery::default()).servers {
                let selected = catalogue.selected(&Selection { server_id: server.id, config_sha256: server.config_sha256 }).unwrap();
                let (_, text) = validate_server(selected).unwrap();
                let profile = prepare_profile(&text, server.ip).unwrap();
                prop_assert_eq!(profile.remote.ip(), server.ip);
            }
        }
    }
}

// Deliberately not a real certificate. Directory parsing only validates the
// envelope/allowlist; the native core independently validates certificate data.
pub(super) fn fixture() -> Vec<u8> {
    let server = |host: &str, ip: &str, country: Option<&str>, score: u64, proto: &str| {
        let profile = format!(
            "client\ndev tun\nproto {proto}\nremote {ip} 443\ncipher AES-128-CBC\ndata-ciphers AES-128-CBC\nauth SHA1\n<ca>\nfixture\n</ca>\n"
        );
        json!({
            "id": format!("v1:{}", hash(format!("vpngate-node-v1\0{host}\0{ip}").as_bytes())),
            "hostname": host, "ip": ip, "country_code": country, "country_name": country,
            "score": score, "ping_ms": null, "speed_bps": 0, "num_vpn_sessions": 0,
            "openvpn_config_base64": STANDARD.encode(profile.as_bytes()),
            "openvpn_config_sha256": hash(profile.as_bytes()), "openvpn_config_bytes": profile.len()
        })
    };
    serde_json::to_vec(&json!({
        "schema_version": 1, "source_csv_sha256": "0".repeat(64), "server_count": 3,
        "servers": [server("first", "8.8.8.8", Some("JP"), 7, "tcp"),
                    server("second", "1.1.1.1", None, 9, "tcp"),
                    server("third", "9.9.9.9", Some("US"), 10, "udp")]
    }))
    .unwrap()
}

#[test]
fn compatible_filter_score_country_counts_and_nullable_metrics_share_one_snapshot() {
    let catalogue = Catalogue::parse(&fixture()).unwrap();
    let list = catalogue.list(&ListQuery::default());
    assert_eq!((list.total, list.source_server_count), (2, 3));
    assert_eq!(list.servers[0].hostname, "second");
    assert_eq!(list.servers[0].ping_ms, None);
    assert_eq!(list.servers[0].speed_bps, Some(0));
    assert_eq!(
        list.countries.iter().map(|c| c.server_count).sum::<usize>(),
        2
    );
    let japan = catalogue.list(&ListQuery {
        country_code: Some("JP".into()),
        ..Default::default()
    });
    assert_eq!(japan.servers.len(), 1);
    assert_eq!(japan.countries, list.countries);
    let unknown = catalogue.list(&ListQuery {
        unknown_country: true,
        ..Default::default()
    });
    assert_eq!(unknown.servers[0].hostname, "second");
    let all = catalogue.list(&ListQuery {
        include_unsupported: true,
        limit: 1,
        ..Default::default()
    });
    assert_eq!(all.total, 3);
    assert_eq!(
        all.servers[0].unsupported_reason,
        Some(UnsupportedReason::UdpTransport)
    );
    assert!(
        catalogue
            .list(&ListQuery {
                offset: 999,
                ..Default::default()
            })
            .servers
            .is_empty()
    );
}

#[test]
fn special_use_remotes_are_unselectable_even_with_valid_directory_hashes() {
    for address in ["3fff::1", "2001:2::1", "192.88.99.2"] {
        let mut directory: Value = serde_json::from_slice(&fixture()).unwrap();
        let server = &mut directory["servers"][0];
        let profile = String::from_utf8(
            STANDARD
                .decode(server["openvpn_config_base64"].as_str().unwrap())
                .unwrap(),
        )
        .unwrap()
        .replace("8.8.8.8", address);
        server["ip"] = json!(address);
        server["id"] = json!(format!(
            "v1:{}",
            hash(format!("vpngate-node-v1\0first\0{address}").as_bytes())
        ));
        server["openvpn_config_base64"] = json!(STANDARD.encode(profile.as_bytes()));
        server["openvpn_config_sha256"] = json!(hash(profile.as_bytes()));
        server["openvpn_config_bytes"] = json!(profile.len());
        let selection = Selection {
            server_id: server["id"].as_str().unwrap().to_owned(),
            config_sha256: server["openvpn_config_sha256"].as_str().unwrap().to_owned(),
        };
        let catalogue = Catalogue::parse(&serde_json::to_vec(&directory).unwrap()).unwrap();
        let compatible = catalogue.list(&ListQuery::default());
        assert_eq!(compatible.total, 1, "{address}");
        assert_eq!(compatible.servers[0].hostname, "second");
        let all = catalogue.list(&ListQuery {
            include_unsupported: true,
            ..Default::default()
        });
        let excluded = all
            .servers
            .iter()
            .find(|s| s.id == selection.server_id)
            .unwrap();
        assert_eq!(
            excluded.unsupported_reason,
            Some(UnsupportedReason::InvalidEndpoint),
            "{address}"
        );
        assert!(catalogue.prepare(&selection).is_err(), "{address}");
    }
}

#[test]
fn rejects_corrupt_hash_base64_schema_counts_ids_and_oversized_configs() {
    let original: Value = serde_json::from_slice(&fixture()).unwrap();
    for (pointer, value) in [
        ("/schema_version", json!(2)),
        ("/server_count", json!(5001)),
        ("/server_count", json!(2)),
        ("/source_csv_sha256", json!("x")),
        ("/servers/0/openvpn_config_sha256", json!("0".repeat(64))),
        ("/servers/0/openvpn_config_base64", json!("***")),
        (
            "/servers/0/openvpn_config_bytes",
            json!(MAX_CONFIG_BYTES + 1),
        ),
        ("/servers/0/hostname", json!("changed")),
        ("/servers/0/country_code", json!("jp")),
        ("/servers/0/country_name", json!(" ")),
        ("/servers/0/score", json!(MAX_SAFE_INTEGER + 1)),
    ] {
        let mut changed = original.clone();
        *changed.pointer_mut(pointer).unwrap() = value;
        assert!(
            Catalogue::parse(&serde_json::to_vec(&changed).unwrap()).is_err(),
            "{pointer}"
        );
    }
    for key in [
        "country_code",
        "country_name",
        "score",
        "ping_ms",
        "speed_bps",
        "num_vpn_sessions",
    ] {
        let mut changed = original.clone();
        changed["servers"][0].as_object_mut().unwrap().remove(key);
        assert!(
            Catalogue::parse(&serde_json::to_vec(&changed).unwrap()).is_err(),
            "missing {key}"
        );
    }
    for hostname in [" ", "a..b", "-invalid", "invalid-", "host/path", "非ASCII"] {
        let mut changed = original.clone();
        changed["servers"][0]["hostname"] = json!(hostname);
        changed["servers"][0]["id"] = json!(format!(
            "v1:{}",
            hash(
                format!(
                    "vpngate-node-v1\0{}\08.8.8.8",
                    hostname.trim().to_lowercase()
                )
                .as_bytes()
            )
        ));
        assert!(
            Catalogue::parse(&serde_json::to_vec(&changed).unwrap()).is_err(),
            "hostname {hostname}"
        );
    }
    assert!(matches!(
        Catalogue::parse(&vec![b' '; MAX_DIRECTORY_BYTES + 1]),
        Err(DirectoryError::SizeLimit)
    ));
    let mut duplicate = original;
    duplicate["servers"][1] = duplicate["servers"][0].clone();
    assert!(Catalogue::parse(&serde_json::to_vec(&duplicate).unwrap()).is_err());
    duplicate["servers"] = json!([]);
    duplicate["server_count"] = json!(0);
    assert!(Catalogue::parse(&serde_json::to_vec(&duplicate).unwrap()).is_err());
}

#[test]
fn pinned_configuration_survives_refresh_removal_and_stale_draft_is_rejected() {
    let directory = tempfile::tempdir().unwrap();
    let store = CatalogueStore::new(directory.path());
    assert_eq!(
        store.list(&ListQuery::default()).unwrap(),
        ServerList::default()
    );
    let catalogue = Catalogue::parse(&fixture()).unwrap();
    store.save(&catalogue, 123, RAW_URL).unwrap();
    let list = store.list(&ListQuery::default()).unwrap();
    assert_eq!(list.fetched_at_unix_ms, Some(123));
    let selected = &list.servers[0];
    let selection = Selection {
        server_id: selected.id.clone(),
        config_sha256: selected.config_sha256.clone(),
    };
    store.pin(&selection).unwrap();
    let before = store.load_selection(&selection).unwrap();
    let mut document: Value = serde_json::from_slice(&fixture()).unwrap();
    document["servers"]
        .as_array_mut()
        .unwrap()
        .retain(|server| server["id"].as_str() != Some(&selection.server_id));
    document["server_count"] = json!(document["servers"].as_array().unwrap().len());
    store
        .save(
            &Catalogue::parse(&serde_json::to_vec(&document).unwrap()).unwrap(),
            456,
            RAW_URL,
        )
        .unwrap();
    assert_eq!(store.pin(&selection), Err(DirectoryError::StaleSelection));
    let after = store.load_selection(&selection).unwrap();
    assert_eq!(before.0, after.0);
    assert_eq!(before.1.content(), after.1.content());
    assert_eq!(after.1.remote.ip(), selected.ip);
    assert!(after.1.content().contains("tls-version-min 1.2"));
    assert!(after.1.content().contains("remote-cert-tls server"));
}

#[test]
fn cache_is_revalidated_and_selection_cannot_escape_cache_directory() {
    let directory = tempfile::tempdir().unwrap();
    let store = CatalogueStore::new(directory.path());
    store
        .save(&Catalogue::parse(&fixture()).unwrap(), 123, RAW_URL)
        .unwrap();
    let path = directory.path().join("vpngate/directory.json");
    std::fs::write(&path, b"{partial").unwrap();
    assert!(store.load().is_err());
    let invalid = Selection {
        server_id: "../outside".into(),
        config_sha256: "../outside".into(),
    };
    assert!(store.load_selection(&invalid).is_err());
    assert!(store.pin(&invalid).is_err());
    assert!(VpnGateSettings::default().validate().is_ok());
    assert!(
        VpnGateSettings {
            enabled: true,
            selection: None
        }
        .validate()
        .is_err()
    );
}
