//! macOS proxy-only parity uses the same Engine handlers and encrypted stores
//! as the other desktops. These fixtures never use Keychain or a remote peer.
use crate::*;
use usque_core::chain_exit::ChainProfileResponse;

fn open_service(directory: &std::path::Path, vault: Arc<tests::MemoryVault>) -> ControlService {
    ControlService::open_with_vault(ConfigStore::new(directory.join("config.json")), vault).unwrap()
}

fn request(payload: control_request::Payload) -> ControlRequest {
    ControlRequest {
        request_id: "macos-parity".into(),
        payload: Some(payload),
    }
}

#[tokio::test]
async fn advertised_chain_import_reaches_encrypted_storage_and_connection_preparation() {
    let directory = tempfile::tempdir().unwrap();
    let vault = Arc::new(tests::MemoryVault::default());
    let service = open_service(directory.path(), vault.clone());
    let caps = current_capabilities();
    assert!(
        caps.chain_profile_import && caps.chain_openvpn_udp && caps.chain_openvpn_multi_endpoint
    );
    assert!(caps.chain_http_proxy && caps.chain_socks5_proxy && caps.chain_proxy_encrypted_dns);
    assert_eq!(caps.chain_wireguard, cfg!(feature = "wireguard"));
    assert!(!caps.vpn && !caps.l4_tun_tcp && caps.system_proxy && !caps.platform_lockdown);

    let cases = [
        (
            "openvpn_custom",
            "client\ndev tun\nproto udp\nremote 192.0.2.5 1194\nauth-user-pass\n<ca>\nTEST\n</ca>\n",
            None,
        ),
        (
            "openvpn_custom",
            "client\ndev tun\nproto tcp-client\nremote 192.0.2.6 443\nauth-user-pass\n<ca>\nTEST\n</ca>\n",
            None,
        ),
        (
            "wireguard_custom",
            "[Interface]\nPrivateKey = AQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQE=\nAddress = 10.8.0.2/32\n[Peer]\nPublicKey = AgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgI=\nEndpoint = 192.0.2.7:51820\nAllowedIPs = 0.0.0.0/0\n",
            None,
        ),
        (
            "warp_wireguard",
            "[Interface]\nPrivateKey = AQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQE=\nAddress = 10.8.0.2/32\n[Peer]\nPublicKey = AgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgI=\nEndpoint = 192.0.2.7:51820\nAllowedIPs = 0.0.0.0/0\n",
            None,
        ),
        ("http_proxy", "", Some("192.0.2.8")),
        ("socks5_proxy", "", Some("192.0.2.9")),
    ];
    let mut selections = Vec::new();
    for (source, configuration, proxy_host) in cases {
        if matches!(source, "wireguard_custom" | "warp_wireguard") && !cfg!(feature = "wireguard") {
            continue;
        }
        let reply = service
            .handle(request(control_request::Payload::ChainProfile(Box::new(
                v1::ChainProfileRequest {
                    action: "import".into(),
                    source: source.into(),
                    name: format!("Synthetic {source}"),
                    configuration: configuration.as_bytes().to_vec(),
                    username: "synthetic-user".into(),
                    password: "synthetic-password".into(),
                    proxy: proxy_host.map(|host| v1::ProxyExitConfiguration {
                        host: host.into(),
                        port: 8080,
                        auth_mode: "username_password".into(),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
            ))))
            .await;
        assert!(reply.error.is_none(), "{source}: {:?}", reply.error);
        let Some(control_response::Payload::ChainProfiles(reply)) = reply.payload else {
            panic!("chain response");
        };
        let result: ChainProfileResponse = serde_json::from_str(&reply.metadata_json).unwrap();
        assert!(result.error.is_none(), "{source}: {:?}", result.error);
        let imported = result.preview.unwrap();
        let sealed = std::fs::read(
            directory
                .path()
                .join("chain-profiles")
                .join(format!("{}.profile", imported.id)),
        )
        .unwrap();
        assert!(sealed.starts_with(b"USQUE-MACOS-CHAIN\0\x01"));
        assert!(
            !sealed
                .windows(b"synthetic-password".len())
                .any(|part| part == b"synthetic-password")
        );
        let profile = Profile {
            chain_exit: Some(imported.selection()),
            ..Profile::default()
        };
        let prepared = service.prepare_gate_selection(&profile).unwrap().unwrap();
        assert_eq!(prepared.1.summary.as_ref().unwrap().id, imported.id);
        assert_eq!(prepared.1.summary.as_ref().unwrap().source, imported.source);
        let dto = usque_macos_runtime::credentials::ChainCredentials::from_prepared(
            &prepared.0,
            &prepared.1,
        )
        .unwrap();
        let encoded = zeroize::Zeroizing::new(serde_json::to_vec(&dto).unwrap());
        let decoded: usque_macos_runtime::credentials::ChainCredentials =
            serde_json::from_slice(&encoded).unwrap();
        let key = usque_macos_runtime::SessionKey {
            session_id: Uuid::new_v4(),
            config_revision: 1,
            network_generation: 7,
        };
        let recovered = decoded
            .prepare(
                &key,
                &usque_macos_runtime::credentials::UnavailableCredentialBroker,
            )
            .unwrap();
        assert_eq!(recovered.1.summary, prepared.1.summary);
        assert_eq!(recovered.1.credentials.password, "synthetic-password");
        assert!(!format!("{decoded:?}").contains("synthetic-password"));
        selections.push(profile);
    }
    assert!(
        vault
            .get(desktop_chain::KEY_ID, SecretRecord::ChainEncryptionKey)
            .await
            .unwrap()
            .is_some()
    );
    drop(service);
    let reopened = open_service(directory.path(), vault);
    for profile in selections {
        assert!(reopened.prepare_gate_selection(&profile).unwrap().is_some());
    }
}

#[cfg(feature = "wireguard")]
#[tokio::test]
async fn warp_wireguard_effective_endpoint_survives_provider_secret_handoff() {
    use usque_core::chain_exit::{ChainSource, Endpoint, ImportSecrets, store::ChainProfileStore};
    use usque_macos_runtime::credentials::{ChainCredentials, UnavailableCredentialBroker};
    let directory = tempfile::tempdir().unwrap();
    let service = open_service(directory.path(), Arc::new(tests::MemoryVault::default()));
    let cipher = service.chain_cipher.as_ref().unwrap();
    let original = ChainProfileStore::new(&service.profile_dir, cipher.as_ref())
        .import(
            ChainSource::WarpWireguard,
            "Synthetic WARP WireGuard",
            ImportSecrets::new("[Interface]\nPrivateKey = AQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQE=\nAddress = 10.8.0.2/32\n[Peer]\nPublicKey = AgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgI=\nEndpoint = 192.0.2.7:51820\nAllowedIPs = 0.0.0.0/0\n".into()),
        )
        .unwrap();
    for host in ["198.51.100.25", "2001:db8::25"] {
        let mut selection = original.selection();
        selection.endpoint_override = Some(Endpoint::parse(host, "2408", 0).unwrap());
        let profile = Profile {
            chain_exit: Some(selection),
            ..Profile::default()
        };
        let (server, prepared) = service.prepare_gate_selection(&profile).unwrap().unwrap();
        let encoded = zeroize::Zeroizing::new(
            serde_json::to_vec(&ChainCredentials::from_prepared(&server, &prepared).unwrap())
                .unwrap(),
        );
        let decoded: ChainCredentials = serde_json::from_slice(&encoded).unwrap();
        let (_, recovered) = decoded
            .prepare(
                &usque_macos_runtime::SessionKey {
                    session_id: Uuid::new_v4(),
                    config_revision: 3,
                    network_generation: 7,
                },
                &UnavailableCredentialBroker,
            )
            .expect("effective WARP endpoint must survive handoff");
        assert_eq!(recovered.summary, prepared.summary);
        let Some(usque_core::chain_exit::ValidatedProfile::WireGuard(config)) =
            recovered.custom.as_deref()
        else {
            panic!("WireGuard profile");
        };
        assert_eq!(config.endpoint.host, host);
        assert_eq!(config.endpoint.port, 2408);
        let stored = ChainProfileStore::new(&service.profile_dir, cipher.as_ref())
            .load(&original.selection())
            .unwrap();
        assert_eq!(
            stored.0.endpoint, original.endpoint,
            "the saved profile is immutable"
        );
    }
}

#[cfg(feature = "wireguard")]
#[tokio::test]
async fn provider_handoff_rejects_unapproved_or_malformed_endpoint_changes() {
    use usque_core::chain_exit::{ChainSource, Endpoint, ImportSecrets, store::ChainProfileStore};
    use usque_macos_runtime::credentials::{ChainCredentials, UnavailableCredentialBroker};
    let directory = tempfile::tempdir().unwrap();
    let service = open_service(directory.path(), Arc::new(tests::MemoryVault::default()));
    for (source, endpoint, expected) in [
        (
            ChainSource::WireguardCustom,
            Endpoint {
                host: "198.51.100.25".into(),
                port: 2408,
            },
            "chain_summary_mismatch",
        ),
        (
            ChainSource::WarpWireguard,
            Endpoint {
                host: "unverified.example".into(),
                port: 2408,
            },
            "invalid_warp_endpoint_override",
        ),
        (
            ChainSource::WarpWireguard,
            Endpoint {
                host: "198.51.100.25".into(),
                port: 0,
            },
            "invalid_warp_endpoint_override",
        ),
    ] {
        let imported = ChainProfileStore::new(&service.profile_dir, service.chain_cipher.as_ref().unwrap().as_ref())
            .import(source, "Synthetic peer", ImportSecrets::new("[Interface]\nPrivateKey = AQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQE=\nAddress = 10.8.0.2/32\n[Peer]\nPublicKey = AgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgI=\nEndpoint = 192.0.2.7:51820\nAllowedIPs = 0.0.0.0/0\n".into())).unwrap();
        let (server, prepared) = service
            .prepare_gate_selection(&Profile {
                chain_exit: Some(imported.selection()),
                ..Profile::default()
            })
            .unwrap()
            .unwrap();
        let mut dto = ChainCredentials::from_prepared(&server, &prepared).unwrap();
        let ChainCredentials::Imported { summary, .. } = &mut dto else {
            panic!("imported profile");
        };
        summary.endpoint = endpoint;
        let error = dto
            .prepare(
                &usque_macos_runtime::SessionKey {
                    session_id: Uuid::new_v4(),
                    config_revision: 1,
                    network_generation: 7,
                },
                &UnavailableCredentialBroker,
            )
            .unwrap_err();
        assert_eq!(error, expected);
    }
}

#[tokio::test]
async fn vpn_gate_download_requires_reusable_warp_without_releasing_retained_protection() {
    for retained in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let service = open_service(directory.path(), Arc::new(tests::MemoryVault::default()));
        let reservation = retained.then(|| usque_macos_runtime::SessionKey {
            session_id: Uuid::new_v4(),
            config_revision: 3,
            network_generation: 7,
        });
        *service.macos_reserved.lock().await = reservation.clone();
        for payload in [
            control_request::Payload::RefreshVpnGate(v1::RefreshVpnGateRequest { cancel: false }),
            control_request::Payload::VpnGateNode(v1::VpnGateNodeRequest {
                operation_id: Uuid::new_v4().to_string(),
                action: "prepare".into(),
                server_id: format!("v1:{}", "a".repeat(64)),
                config_sha256: "b".repeat(64),
                ..Default::default()
            }),
        ] {
            let reply = service.handle(request(payload)).await;
            let error = reply
                .error
                .expect("unverified physical bootstrap is refused");
            assert_eq!(error.code, "MACOS_WARP_CONNECTION_REQUIRED");
            assert!(error.retryable);
            assert!(service.gate_fetch_task.lock().await.is_none());
            assert!(service.data_plane.lock().await.is_none());
            assert_eq!(*service.macos_reserved.lock().await, reservation);
            assert_eq!(
                service.gate_directory.progress().stage,
                usque_core::vpngate::DownloadStage::Idle
            );
        }
        assert!(
            service
                .list_vpn_gate(v1::ListVpnGateRequest::default())
                .await
                .is_ok(),
            "cached browsing remains local"
        );
    }
}

#[test]
fn vpn_gate_retry_uses_connected_base_warp_when_the_final_exit_is_reconnecting() {
    use usque_transport::{RuntimeHealth, RuntimePath};
    let path = RuntimePath {
        transport: usque_core::Transport::Http3,
        endpoint_family: usque_core::AddressFamily::Ipv4,
        ipv4_available: true,
        ipv6_available: true,
    };
    let connected = RuntimeHealth::Connected {
        path,
        reconnect_count: 0,
    };
    let reconnecting = RuntimeHealth::Reconnecting {
        last_path: path,
        attempt: 1,
        reconnect_count: 1,
        reason: "synthetic exit recovery".into(),
        failure: usque_core::TransportFailure::new(
            usque_core::TransportFailureCode::H3ConnectionClosed,
            usque_core::TransportStage::SocketConnect,
        ),
    };
    assert!(
        vpngate::require_catalogue_warp(Some((reconnecting.clone(), connected.clone()))).is_ok(),
        "a blocked/reconnecting final exit retains its explicit base-WARP control path"
    );
    assert!(
        vpngate::require_catalogue_warp(Some((connected, reconnecting))).is_err(),
        "a final-exit observation cannot authorize a disconnected WARP underlay"
    );
    assert!(vpngate::require_catalogue_warp(None).is_err());
}

#[tokio::test]
async fn unsigned_system_vpn_is_refused_before_starting_any_chain_runtime() {
    use usque_core::chain_exit::{ChainExitSettings, ChainSource};
    for source in [
        ChainSource::OpenvpnCustom,
        ChainSource::WireguardCustom,
        ChainSource::WarpWireguard,
        ChainSource::ProtonVpn,
        ChainSource::VpnGate,
        ChainSource::HttpProxy,
        ChainSource::Socks5Proxy,
    ] {
        let directory = tempfile::tempdir().unwrap();
        let service = open_service(directory.path(), Arc::new(tests::MemoryVault::default()));
        let mut profile = service.config_snapshot().await.active_profile().unwrap();
        profile.frontends = usque_core::FrontendSettings {
            tunnel: true,
            http: true,
            socks5: true,
        };
        profile.proxy.system_proxy = false;
        profile.canonicalize_mode();
        service.upsert_profile(profile.clone()).await.unwrap();
        // A signed-provider prerequisite must win even over a selected family
        // whose credentials have not been provisioned, before any DNS/socket.
        service.config.write().await.network.chain_exit = Some(ChainExitSettings {
            source,
            enabled: true,
            profile_id: (source != ChainSource::VpnGate).then(Uuid::new_v4),
            revision: (source != ChainSource::VpnGate).then(Uuid::new_v4),
            ..Default::default()
        });
        let reply = service
            .handle(request(control_request::Payload::Connect(
                v1::ConnectRequest {
                    profile_id: profile.id.to_string(),
                    continuous: false,
                },
            )))
            .await;
        assert_eq!(
            reply.error.unwrap().code,
            "OPERATING_MODE_UNAVAILABLE",
            "{source:?}"
        );
        assert!(service.data_plane.lock().await.is_none());
        assert!(service.macos_reserved.lock().await.is_none());
        assert!(!current_capabilities().vpn);
    }
}

#[tokio::test]
async fn cached_vpn_gate_profile_prepares_and_handoffs_without_a_download() {
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    use sha2::{Digest, Sha256};
    use usque_core::vpngate::{Catalogue, CatalogueStore, ListQuery, Selection};
    use usque_macos_runtime::credentials::{ChainCredentials, UnavailableCredentialBroker};
    let directory = tempfile::tempdir().unwrap();
    let service = open_service(directory.path(), Arc::new(tests::MemoryVault::default()));
    // VPN Gate intentionally rejects documentation/private endpoints. This
    // public literal is parsed only; the fixture never creates a dialer.
    let configuration = "client\ndev tun\nproto tcp\nremote 8.8.4.4 443\ncipher AES-128-CBC\nauth SHA1\n<ca>\nfixture\n</ca>\n";
    let hash = |text: &str| {
        Sha256::digest(text)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    };
    let catalogue = Catalogue::parse(&serde_json::to_vec(&serde_json::json!({
        "schema_version": 1, "source_csv_sha256": "0".repeat(64), "server_count": 1,
        "servers": [{
            "id": format!("v1:{}", hash("vpngate-node-v1\0fixture\x008.8.4.4")),
            "hostname": "fixture", "ip": "8.8.4.4", "country_code": "JP", "country_name": "Japan",
            "score": 1, "ping_ms": null, "speed_bps": null, "num_vpn_sessions": 1,
            "openvpn_config_base64": STANDARD.encode(configuration),
            "openvpn_config_sha256": hash(configuration), "openvpn_config_bytes": configuration.len(),
        }],
    })).unwrap()).unwrap();
    let store = CatalogueStore::new(&service.cache_dir);
    store
        .save(&catalogue, 1, usque_core::vpngate::RAW_URL)
        .unwrap();
    let node = catalogue.list(&ListQuery::default()).servers.remove(0);
    let selection = Selection {
        server_id: node.id,
        config_sha256: node.config_sha256,
    };
    store.pin(&selection).unwrap();
    let profile = Profile {
        vpn_gate: usque_core::vpngate::VpnGateSettings {
            enabled: true,
            selection: Some(selection),
        },
        ..Default::default()
    };
    let (server, prepared) = service.prepare_gate_selection(&profile).unwrap().unwrap();
    let encoded = zeroize::Zeroizing::new(
        serde_json::to_vec(&ChainCredentials::from_prepared(&server, &prepared).unwrap()).unwrap(),
    );
    let decoded: ChainCredentials = serde_json::from_slice(&encoded).unwrap();
    let recovered = decoded
        .prepare(
            &usque_macos_runtime::SessionKey {
                session_id: Uuid::new_v4(),
                config_revision: 1,
                network_generation: 7,
            },
            &UnavailableCredentialBroker,
        )
        .unwrap();
    assert_eq!(recovered.0.id, server.id);
    assert_eq!(recovered.1.content(), prepared.content());
    assert!(service.gate_fetch_task.lock().await.is_none());
    for action in ["prepare", "favorite"] {
        let reply = service
            .handle(request(control_request::Payload::VpnGateNode(
                v1::VpnGateNodeRequest {
                    operation_id: Uuid::new_v4().to_string(),
                    action: action.into(),
                    server_id: server.id.clone(),
                    config_sha256: server.config_sha256.clone(),
                    ..Default::default()
                },
            )))
            .await;
        assert!(
            reply.error.is_none(),
            "cached {action} must not require a new WARP connection: {:?}",
            reply.error
        );
        tokio::time::timeout(std::time::Duration::from_secs(3), async {
            loop {
                let progress = service.gate_directory.node_progress();
                match progress.stage.as_str() {
                    "complete" => return,
                    "failed" => panic!("cached {action}: {:?}", progress.error),
                    _ => tokio::time::sleep(std::time::Duration::from_millis(5)).await,
                }
            }
        })
        .await
        .unwrap();
        assert!(service.data_plane.lock().await.is_none());
        assert!(service.macos_reserved.lock().await.is_none());
    }
}

#[cfg(feature = "wireguard")]
#[tokio::test]
async fn managed_proton_profile_uses_encrypted_account_and_dedicated_provider_credentials() {
    use usque_core::{
        chain_exit::{ChainSource, ImportSecrets, store::ChainProfileStore},
        proton_vpn::{
            ApiIdentity, Node, Record, SelectedNode, SessionSecrets, store::SessionStore,
        },
    };
    use usque_macos_runtime::credentials::{ChainCredentials, CredentialBroker, PersistenceAck};
    struct Broker {
        session: usque_macos_runtime::SessionKey,
        persistence: Arc<dyn usque_core::proton_vpn::LeasePersistence>,
    }
    #[async_trait::async_trait]
    impl CredentialBroker for Broker {
        async fn persist_pin(
            &self,
            _: &usque_macos_runtime::SessionKey,
            _: Uuid,
            _: &WarpIdentity,
            _: &WarpIdentity,
        ) -> Result<PersistenceAck, String> {
            Err("unused_pin_update".into())
        }
        fn proton_persistence(
            &self,
            session: &usque_macos_runtime::SessionKey,
            profile_id: Uuid,
            record: &Record,
        ) -> Result<Arc<dyn usque_core::proton_vpn::LeasePersistence>, String> {
            if session != &self.session || profile_id != record.id {
                return Err("stale_test_broker".into());
            }
            Ok(self.persistence.clone())
        }
    }
    let directory = tempfile::tempdir().unwrap();
    let service = open_service(directory.path(), Arc::new(tests::MemoryVault::default()));
    let cipher = service.chain_cipher.as_ref().unwrap();
    // Fixed synthetic seed [7; 32], with its matching clamped WireGuard key.
    let imported = ChainProfileStore::new(&service.profile_dir, cipher.as_ref()).import(
        ChainSource::ProtonVpn, "Synthetic Proton guest",
        ImportSecrets::new("[Interface]\nPrivateKey = KK05/v1/o+IAqcYm7vWZ5hotBVxIqCiKTn5MS8o5KHg=\nAddress = 10.8.0.2/32\n[Peer]\nPublicKey = AgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgI=\nEndpoint = 192.0.2.7:51820\nAllowedIPs = 0.0.0.0/0\n".into()),
    ).unwrap();
    let node = Node {
        id: "logical-fixture".into(),
        name: "Synthetic server".into(),
        country: "IS".into(),
        city: "Fixture".into(),
        load: 0,
    };
    let record = Record {
        version: 2,
        id: imported.id,
        revision: imported.revision,
        name: imported.name.clone(),
        identity: "guest".into(),
        api_identity: ApiIdentity::default(),
        session: SessionSecrets {
            uid: "synthetic-uid".into(),
            access_token: "synthetic-proton-access".into(),
            refresh_token: "synthetic-proton-refresh".into(),
            user_id: "synthetic-user".into(),
        },
        ed25519_seed: zeroize::Zeroizing::new(vec![7; 32]),
        certificate_chain: zeroize::Zeroizing::new(String::new()),
        expires_at: 0,
        refresh_at: 0,
        max_tier: 0,
        node: SelectedNode {
            state: None,
            metadata: node.clone(),
            domain: "agent.example.invalid".into(),
            endpoint: imported.endpoint.clone(),
            public_key: "AgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgI=".into(),
            label: None,
        },
        rotation_pending: false,
        server_id: Some(node.id.clone()),
        server_catalog: vec![node],
        retired_sessions: Vec::new(),
        catalog_revision: Uuid::new_v4(),
        tunnel_seed: None,
        key_recovery_at: 0,
        connection_cache: None,
    };
    SessionStore::new(&service.profile_dir, cipher.as_ref())
        .save(&record)
        .unwrap();
    let mut selection = imported.selection();
    selection.proton_server_id = Some("logical-fixture".into());
    let profile = Profile {
        chain_exit: Some(selection),
        ..Profile::default()
    };
    let (server, prepared) = service.prepare_gate_selection(&profile).unwrap().unwrap();
    assert!(prepared.proton.is_some());
    let encoded = zeroize::Zeroizing::new(
        serde_json::to_vec(&ChainCredentials::from_prepared(&server, &prepared).unwrap()).unwrap(),
    );
    let decoded: ChainCredentials = serde_json::from_slice(&encoded).unwrap();
    assert!(!format!("{decoded:?}").contains("synthetic-proton-access"));
    let key = usque_macos_runtime::SessionKey {
        session_id: Uuid::new_v4(),
        config_revision: 1,
        network_generation: 7,
    };
    let broker = Broker {
        session: key.clone(),
        persistence: prepared.proton_persistence.clone().unwrap(),
    };
    let (_, recovered) = decoded.prepare(&key, &broker).unwrap();
    assert_eq!(
        recovered.proton_server_id.as_deref(),
        Some("logical-fixture")
    );
    assert!(recovered.proton.as_ref().unwrap().private_key_pkcs8.len() > 32);
    assert_eq!(
        recovered.proton_persistence.unwrap().load().unwrap().id,
        imported.id
    );
    let mut stale = key.clone();
    stale.network_generation += 1;
    assert!(decoded.prepare(&stale, &broker).is_err());
    let sidecar = std::fs::read(
        service
            .profile_dir
            .join("proton-vpn")
            .join(format!("{}.sealed", imported.id)),
    )
    .unwrap();
    assert!(
        !sidecar
            .windows(b"synthetic-proton-access".len())
            .any(|bytes| bytes == b"synthetic-proton-access")
    );
}

#[cfg(feature = "wireguard")]
#[tokio::test]
async fn advertised_account_commands_reach_managers_without_starting_network_jobs() {
    let directory = tempfile::tempdir().unwrap();
    let service = open_service(directory.path(), Arc::new(tests::MemoryVault::default()));
    assert!(current_capabilities().chain_warp_wireguard);
    assert!(current_capabilities().chain_proton_vpn);
    let proton = service
        .handle(request(control_request::Payload::ProtonVpn(
            v1::ProtonVpnRequest {
                command_json: r#"{"action":"list"}"#.into(),
                ..Default::default()
            },
        )))
        .await;
    assert!(proton.error.is_none(), "{:?}", proton.error);
    let Some(control_response::Payload::ProtonVpn(proton)) = proton.payload else {
        panic!("Proton response");
    };
    let proton: serde_json::Value = serde_json::from_str(&proton.metadata_json).unwrap();
    assert!(proton.get("error").is_none_or(serde_json::Value::is_null));
    let warp = service
        .handle(request(control_request::Payload::WarpWireguard(
            v1::WarpWireguardRequest {
                command_json: r#"{"action":"get"}"#.into(),
            },
        )))
        .await;
    assert!(warp.error.is_none(), "{:?}", warp.error);
    assert!(matches!(
        warp.payload,
        Some(control_response::Payload::WarpWireguard(_))
    ));
    service.cancel_warp_jobs().await.unwrap();
}

#[test]
fn proxy_only_policy_retains_distinct_same_ip_domain_actions() {
    use usque_core::{RoutingAction, RoutingMatch, RoutingRule, RoutingSettings};
    use usque_transport::GeoRoute;
    assert!(
        current_capabilities().custom_bypass
            && current_capabilities().routing_rules
            && current_capabilities().live_ads_rules
    );
    let profile = Profile {
        routing: RoutingSettings {
            rules: [
                ("direct.example", RoutingAction::Direct),
                ("proxy.example", RoutingAction::Proxy),
                ("reject.example", RoutingAction::Reject),
            ]
            .into_iter()
            .map(|(target, action)| RoutingRule {
                id: Uuid::new_v4(),
                kind: RoutingMatch::Domain,
                target: target.into(),
                action,
            })
            .collect(),
            ..Default::default()
        },
        ..Profile::default()
    };
    let policy = GeoDirectPolicy::disabled()
        .with_custom_rules(&profile)
        .unwrap();
    let same_ip = "192.0.2.42".parse().unwrap();
    assert_eq!(
        policy.resolved_route("direct.example", same_ip),
        GeoRoute::Direct
    );
    assert_eq!(
        policy.resolved_route("proxy.example", same_ip),
        GeoRoute::Tunnel
    );
    assert_eq!(
        policy.resolved_route("reject.example", same_ip),
        GeoRoute::Reject
    );
}
