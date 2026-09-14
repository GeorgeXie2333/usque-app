use super::*;
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

pub(super) const COMMIT: &str = "1234567890123456789012345678901234567890";
pub(super) struct Fixture {
    pub index: Vec<u8>,
    pub servers: Vec<u8>,
    pub countries: Vec<u8>,
    pub configs: BTreeMap<String, Vec<u8>>,
}
impl Fixture {
    pub fn new() -> Self {
        let legacy: Value = serde_json::from_slice(&super::tests::fixture()).unwrap();
        let mut nodes = legacy["servers"].as_array().unwrap().clone();
        let time = "2026-09-13T06:29:46.957Z";
        let mut configs = BTreeMap::new();
        for (i, node) in nodes.iter_mut().enumerate() {
            let config = serde_json::to_vec(&json!({"kind":"vpngate-pool-config", "schema_version":1,
                "openvpn_config_base64":node["openvpn_config_base64"], "openvpn_config_sha256":node["openvpn_config_sha256"], "openvpn_config_bytes":node["openvpn_config_bytes"]})).unwrap();
            let path = format!(
                "pool/configs/{}.json",
                node["openvpn_config_sha256"].as_str().unwrap()
            );
            node["config"] = json!({"path":path,"bytes":config.len(),"sha256":hash(&config)});
            configs.insert(path, config);
            node.as_object_mut()
                .unwrap()
                .remove("openvpn_config_base64");
            node["first_seen_at"] = json!(time);
            node["last_seen_at"] = json!(time);
            node["present_in_latest_source"] = json!(true);
            node["probe_reason"] = json!(if i == 2 { "udp" } else { "tcp" });
            node["probe_targets"] = if i == 2 {
                json!([])
            } else {
                json!([{"ip":node["ip"],"port":443}])
            };
            node["tcp_probe"] = json!({"status":"unknown","probe_source":"cloudflare_workers", "checked_at":null,"last_success_at":null,"connect_ms":null,"connected_endpoint":null,"consecutive_failures":0,"round":null,"worker_id":null,"last_failure_round":null});
        }
        let servers = serde_json::to_vec(&json!({"kind":"vpngate-pool","schema_version":1,"source_fetched_at":time,"server_count":3,"servers":nodes})).unwrap();
        let countries = serde_json::to_vec(&json!({"kind":"vpngate-pool","schema_version":1,"source_fetched_at":time,"server_count":3,
            "countries":[{"code":null,"names":[],"server_count":1},{"code":"JP","names":["JP"],"server_count":1},{"code":"US","names":["US"],"server_count":1}]})).unwrap();
        let mut fixture = Self {
            index: vec![],
            servers,
            countries,
            configs,
        };
        fixture.reindex();
        fixture
    }
    pub fn reindex(&mut self) {
        self.index = serde_json::to_vec(&json!({"kind":"vpngate-pool","schema_version":1,"data_commit":COMMIT,
            "source_url":"https://www.vpngate.net/api/iphone/", "source_fetched_at":"2026-09-13T06:29:46.957Z",
            "generated_at":"2026-09-13T06:29:48.017Z","index_generated_at":"2026-09-13T06:29:56.538Z","server_count":3,
            "files":{"pool/servers.json":{"bytes":self.servers.len(),"sha256":hash(&self.servers)},
                "pool/countries.json":{"bytes":self.countries.len(),"sha256":hash(&self.countries)}}})).unwrap();
    }
    pub fn pool(&self) -> PoolCatalogue {
        PoolCatalogue::parse(
            PoolIndex::parse(&self.index).unwrap(),
            &self.servers,
            &self.countries,
        )
        .unwrap()
    }
}
pub(super) fn node_request(server: &ServerSummary, action: NodeAction) -> NodeRequest {
    NodeRequest {
        operation_id: uuid::Uuid::new_v4().to_string(),
        action,
        server_id: server.id.clone(),
        config_sha256: server.config_sha256.clone(),
        expected_favorite_hash: String::new(),
    }
}

#[test]
fn pool_metadata_countries_and_two_layers_of_configuration_integrity() {
    let fixture = Fixture::new();
    let pool = fixture.pool();
    let directory = tempfile::tempdir().unwrap();
    let store = CatalogueStore::new(directory.path());
    store.save_pool(&pool, 42, RAW_URL, true).unwrap();
    let list = store.list(&ListQuery::default()).unwrap();
    assert_eq!((list.total, list.source_server_count), (2, 3));
    assert_eq!(list.servers[0].hostname, "second");
    assert_eq!(list.countries.len(), 2);
    for node in &pool.nodes[..2] {
        let bytes = &fixture.configs[node.config_path()];
        let (summary, wire) = node.configuration(bytes).unwrap();
        store
            .stage_configuration(summary.clone(), wire, &CancellationToken::new())
            .unwrap();
        let selection = Selection {
            server_id: summary.id,
            config_sha256: summary.config_sha256,
        };
        store.pin(&selection).unwrap();
        assert_eq!(
            store.load_selection(&selection).unwrap().1.remote.port(),
            443
        );
        let mut changed = bytes.clone();
        changed.push(b' ');
        assert!(node.configuration(&changed).is_err());
    }
    let mut corrupt: Value =
        serde_json::from_slice(&fixture.configs[pool.nodes[0].config_path()]).unwrap();
    corrupt["openvpn_config_base64"] = json!(STANDARD.encode(b"incorrect"));
    let corrupt = serde_json::to_vec(&corrupt).unwrap();
    let mut nodes: Value = serde_json::from_slice(&fixture.servers).unwrap();
    nodes["servers"][0]["config"]["sha256"] = json!(hash(&corrupt));
    nodes["servers"][0]["config"]["bytes"] = json!(corrupt.len());
    let mut fixture = fixture;
    fixture.servers = serde_json::to_vec(&nodes).unwrap();
    fixture.reindex();
    assert!(fixture.pool().nodes[0].configuration(&corrupt).is_err());
}

#[test]
fn pool_hostnames_with_underscores_keep_identity_and_config_validation() {
    for hostname in ["vpn_gate", "VPN_Gate.Example.", "_relay", "relay_"] {
        let mut fixture = Fixture::new();
        let mut document: Value = serde_json::from_slice(&fixture.servers).unwrap();
        let node = &mut document["servers"][0];
        node["hostname"] = json!(hostname);
        let normalized = hostname.trim().trim_end_matches('.').to_ascii_lowercase();
        node["id"] = json!(format!(
            "v1:{}",
            hash(format!("vpngate-node-v1\0{normalized}\08.8.8.8").as_bytes())
        ));
        fixture.servers = serde_json::to_vec(&document).unwrap();
        fixture.reindex();
        let pool = fixture.pool();
        let directory = tempfile::tempdir().unwrap();
        let store = CatalogueStore::new(directory.path());
        store.save_pool(&pool, 42, RAW_URL, false).unwrap();
        let list = store.list(&ListQuery::default()).unwrap();
        assert_eq!((list.total, list.source_server_count), (2, 3));
        assert!(list.servers.iter().any(|s| s.hostname == hostname));
        let node = &pool.nodes[0];
        let (summary, wire) = node
            .configuration(&fixture.configs[node.config_path()])
            .unwrap();
        let selection = node_request(&summary, NodeAction::Prepare).selection();
        store
            .stage_configuration(summary, wire, &CancellationToken::new())
            .unwrap();
        assert_eq!(
            store.load_selection(&selection).unwrap().1.remote.ip(),
            "8.8.8.8".parse::<IpAddr>().unwrap()
        );

        document["servers"][0]["hostname"] = json!("other_host");
        fixture.servers = serde_json::to_vec(&document).unwrap();
        fixture.reindex();
        assert!(
            PoolCatalogue::parse(
                PoolIndex::parse(&fixture.index).unwrap(),
                &fixture.servers,
                &fixture.countries,
            )
            .is_err()
        );
    }
}

#[test]
fn malformed_pool_fields_paths_counts_hashes_and_rollback_are_rejected() {
    let fixture = Fixture::new();
    for (pointer, value) in [
        ("/servers/0/id", json!("v1:invalid")),
        ("/servers/0/config/path", json!("../secret")),
        ("/servers/0/config/bytes", json!(MAX_CONFIG_JSON_BYTES + 1)),
        ("/servers/0/last_seen_at", json!("tomorrow")),
        ("/servers/0/probe_targets/0/ip", json!("127.0.0.1")),
        ("/server_count", json!(4)),
    ] {
        let mut f = Fixture::new();
        let mut document: Value = serde_json::from_slice(&f.servers).unwrap();
        *document.pointer_mut(pointer).unwrap() = value;
        f.servers = serde_json::to_vec(&document).unwrap();
        f.reindex();
        assert!(
            PoolCatalogue::parse(
                PoolIndex::parse(&f.index).unwrap(),
                &f.servers,
                &f.countries
            )
            .is_err(),
            "{pointer}"
        );
    }
    let mut countries: Value = serde_json::from_slice(&fixture.countries).unwrap();
    countries["countries"][0]["server_count"] = json!(2);
    let mut f = Fixture::new();
    f.countries = serde_json::to_vec(&countries).unwrap();
    f.reindex();
    assert!(
        PoolCatalogue::parse(
            PoolIndex::parse(&f.index).unwrap(),
            &f.servers,
            &f.countries
        )
        .is_err()
    );
    let mut previous = PoolIndex::parse(&fixture.index).unwrap();
    previous.index_generated_at = "2026-09-13T07:00:00.000Z".into();
    assert!(
        PoolIndex::parse(&fixture.index)
            .unwrap()
            .not_older_than(&previous)
            .is_err()
    );
    for url in [
        "https://raw.githubusercontent.com/other/repo/main/pool/latest.json",
        "https://cdn.jsdelivr.net/gh/GeorgeXie2333/vpngate-list-mirror@latest/pool/servers.json",
        "https://cdn.jsdelivr.net/gh/GeorgeXie2333/vpngate-list-mirror@123/../../secret",
    ] {
        assert!(!approved_url(url));
    }
}

#[test]
fn favorites_survive_cache_removal_and_restart_and_removal_cannot_resurrect_update() {
    let f = Fixture::new();
    let p = f.pool();
    let directory = tempfile::tempdir().unwrap();
    let store = CatalogueStore::new(directory.path());
    store.save_pool(&p, 42, RAW_URL, false).unwrap();
    let node = &p.nodes[0];
    let (summary, wire) = node.configuration(&f.configs[node.config_path()]).unwrap();
    let request = node_request(&summary, NodeAction::Favorite);
    let cancel = CancellationToken::new();
    store.stage_configuration(summary, wire, &cancel).unwrap();
    store.set_favorite(&request, 0, &cancel).unwrap();
    store.release_prepared(&request.selection());
    let mut remove = request.clone();
    remove.action = NodeAction::RemoveFavorite;
    remove.expected_favorite_hash = request.config_sha256.clone();
    let revision = store.favorite_revision(&remove).unwrap();
    std::fs::remove_file(directory.path().join("vpngate/pool-cache.json")).unwrap();
    let reopened = CatalogueStore::new(directory.path());
    let list = reopened
        .list(&ListQuery {
            favorites_only: true,
            ..Default::default()
        })
        .unwrap();
    assert_eq!((list.total, list.favorite_count), (1, 1));
    assert!(reopened.load_selection(&request.selection()).is_ok());
    reopened.pin(&request.selection()).unwrap();
    reopened
        .remove_favorite(&remove, &[request.selection()])
        .unwrap();
    assert_eq!(
        reopened
            .list(&ListQuery {
                favorites_only: true,
                ..Default::default()
            })
            .unwrap()
            .total,
        0
    );
    assert!(reopened.load_selection(&request.selection()).is_ok());
    assert_eq!(
        reopened.set_favorite(&remove, revision, &cancel),
        Err(DirectoryError::FavoriteChanged)
    );
}

#[test]
fn manual_update_keeps_the_current_pin_and_collects_an_unreferenced_removed_favorite() {
    let mut f = Fixture::new();
    let p = f.pool();
    let directory = tempfile::tempdir().unwrap();
    let store = CatalogueStore::new(directory.path());
    store.save_pool(&p, 42, RAW_URL, false).unwrap();
    let node = &p.nodes[0];
    let cancel = CancellationToken::new();
    let (summary, wire) = node.configuration(&f.configs[node.config_path()]).unwrap();
    let request = node_request(&summary, NodeAction::Favorite);
    store.stage_configuration(summary, wire, &cancel).unwrap();
    store.set_favorite(&request, 0, &cancel).unwrap();
    store.pin(&request.selection()).unwrap();

    let mut config: Value = serde_json::from_slice(&f.configs[node.config_path()]).unwrap();
    let text = String::from_utf8(
        STANDARD
            .decode(config["openvpn_config_base64"].as_str().unwrap())
            .unwrap(),
    )
    .unwrap()
    .replace(" 443\n", " 444\n");
    config["openvpn_config_base64"] = json!(STANDARD.encode(text.as_bytes()));
    config["openvpn_config_bytes"] = json!(text.len());
    config["openvpn_config_sha256"] = json!(hash(text.as_bytes()));
    let bytes = serde_json::to_vec(&config).unwrap();
    let path = format!("pool/configs/{}.json", hash(text.as_bytes()));
    let mut nodes: Value = serde_json::from_slice(&f.servers).unwrap();
    nodes["servers"][0]["openvpn_config_sha256"] = config["openvpn_config_sha256"].clone();
    nodes["servers"][0]["openvpn_config_bytes"] = config["openvpn_config_bytes"].clone();
    nodes["servers"][0]["config"] = json!({"path":path,"bytes":bytes.len(),"sha256":hash(&bytes)});
    nodes["servers"][0]["probe_targets"][0]["port"] = json!(444);
    f.servers = serde_json::to_vec(&nodes).unwrap();
    f.reindex();
    let new_pool = f.pool();
    store.save_pool(&new_pool, 43, RAW_URL, false).unwrap();
    let list = store
        .list(&ListQuery {
            favorites_only: true,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(list.servers[0].config_sha256, request.config_sha256);
    assert_eq!(
        list.servers[0]
            .favorite
            .as_ref()
            .unwrap()
            .latest_config_sha256
            .as_deref(),
        Some(hash(text.as_bytes()).as_str())
    );
    let (summary, wire) = new_pool.nodes[0].configuration(&bytes).unwrap();
    let mut update = node_request(&summary, NodeAction::UpdateFavorite);
    update.expected_favorite_hash = request.config_sha256.clone();
    store.stage_configuration(summary, wire, &cancel).unwrap();
    let revision = store.favorite_revision(&update).unwrap();
    store.set_favorite(&update, revision, &cancel).unwrap();
    store.release_prepared(&update.selection());
    assert_eq!(
        store
            .load_selection(&request.selection())
            .unwrap()
            .1
            .remote
            .port(),
        443
    );
    assert_eq!(
        store
            .load_selection(&update.selection())
            .unwrap()
            .1
            .remote
            .port(),
        444
    );
    update.action = NodeAction::RemoveFavorite;
    update.expected_favorite_hash = update.config_sha256.clone();
    store
        .remove_favorite(&update, &[request.selection()])
        .unwrap();
    assert!(
        !directory
            .path()
            .join(format!("vpngate/objects/{}.json", update.config_sha256))
            .exists()
    );
    assert!(store.load_selection(&request.selection()).is_ok());
}

#[test]
fn prepared_favorite_survives_removal_until_release_and_unused_pins_are_collected() {
    let f = Fixture::new();
    let p = f.pool();
    let directory = tempfile::tempdir().unwrap();
    let store = CatalogueStore::new(directory.path());
    let node = &p.nodes[0];
    let (summary, wire) = node.configuration(&f.configs[node.config_path()]).unwrap();
    let request = node_request(&summary, NodeAction::Favorite);
    let cancel = CancellationToken::new();
    store.stage_configuration(summary, wire, &cancel).unwrap();
    store.set_favorite(&request, 0, &cancel).unwrap();
    store.release_prepared(&request.selection());
    store
        .prepare_local(&request.selection(), &cancel)
        .unwrap()
        .unwrap();
    let mut remove = request.clone();
    remove.action = NodeAction::RemoveFavorite;
    remove.expected_favorite_hash = request.config_sha256.clone();
    store.remove_favorite(&remove, &[]).unwrap();
    assert!(store.load_selection(&request.selection()).is_ok());
    store.pin(&request.selection()).unwrap();
    store.retain_selections(&[request.selection()]).unwrap();
    assert!(store.load_selection(&request.selection()).is_ok());
    store.retain_selections(&[]).unwrap();
    assert!(store.load_selection(&request.selection()).is_err());
    assert_eq!(
        std::fs::read_dir(directory.path().join("vpngate/objects"))
            .unwrap()
            .count(),
        0
    );
}

#[test]
fn restart_drops_abandoned_preparations_but_keeps_saved_snapshot() {
    let f = Fixture::new();
    let p = f.pool();
    let directory = tempfile::tempdir().unwrap();
    let store = CatalogueStore::new(directory.path());
    let mut selections = Vec::new();
    for node in &p.nodes[..2] {
        let (summary, wire) = node.configuration(&f.configs[node.config_path()]).unwrap();
        let request = node_request(&summary, NodeAction::Prepare);
        store
            .stage_configuration(summary, wire, &CancellationToken::new())
            .unwrap();
        selections.push(request.selection());
    }
    store.pin(&selections[0]).unwrap();
    CatalogueStore::new(directory.path())
        .recover_references(&selections[..1])
        .unwrap();
    assert!(store.load_selection(&selections[0]).is_ok());
    assert!(store.load_selection(&selections[1]).is_err());
}

#[test]
fn job_status_queries_do_not_read_corrupt_directory_or_favorites_files() {
    let directory = tempfile::tempdir().unwrap();
    let store = CatalogueStore::new(directory.path());
    std::fs::create_dir_all(directory.path().join("vpngate")).unwrap();
    for file in ["pool-cache.json", "favorites.json"] {
        std::fs::write(directory.path().join("vpngate").join(file), b"invalid").unwrap();
    }
    assert!(
        store
            .list(&ListQuery {
                status_only: true,
                ..Default::default()
            })
            .unwrap()
            .servers
            .is_empty()
    );
    assert!(store.list(&ListQuery::default()).is_err());
}

proptest::proptest! {
    #[test]
    fn arbitrary_pool_envelopes_do_not_panic(bytes in proptest::collection::vec(proptest::prelude::any::<u8>(), 0..8192)) {
        if let Ok(index) = PoolIndex::parse(&bytes) { let _ = PoolCatalogue::parse(index, &bytes, &bytes); }
    }
}
