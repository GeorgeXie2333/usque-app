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
            "wireguard_custom",
            "[Interface]\nPrivateKey = AQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQE=\nAddress = 10.8.0.2/32\n[Peer]\nPublicKey = AgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgI=\nEndpoint = 192.0.2.7:51820\nAllowedIPs = 0.0.0.0/0\n",
            None,
        ),
        ("http_proxy", "", Some("192.0.2.8")),
        ("socks5_proxy", "", Some("192.0.2.9")),
    ];
    let mut selections = Vec::new();
    for (source, configuration, proxy_host) in cases {
        if source == "wireguard_custom" && !cfg!(feature = "wireguard") {
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
