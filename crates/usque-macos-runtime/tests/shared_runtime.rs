use async_trait::async_trait;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};
use tokio_util::sync::CancellationToken;
use usque_core::{
    chain_exit::{
        ChainSource, ImportSecrets, ProxyAuthMode, ProxyDnsTransport, ProxyExitConfiguration,
        ValidatedProfile,
    },
    proton_vpn::{LeasePersistence, Record},
    *,
};
use usque_macos_runtime::{
    backend::{
        BrokerPinRefresher, DataPlaneBackend, PlatformAdapter, PlatformRequest, SessionBackend,
    },
    credentials::{
        ChainCredentials, CredentialBroker, PersistenceAck, SessionCredentials,
        UnavailableCredentialBroker,
    },
    policy::PolicyEvaluator,
    *,
};
use usque_transport::{GeoDirectPolicy, GeoRoute};
use uuid::Uuid;

fn identity() -> WarpIdentity {
    let key = MasqueKeyPair::generate();
    let pin = EndpointPin::from_spki_der(&key.public_spki_der().unwrap()).unwrap();
    WarpIdentity::from_secure_records(
        key,
        pin,
        "device-private".into(),
        "access-token-private".into(),
        None,
        IdentityProvider::Consumer,
        None,
        "172.16.0.2".parse().unwrap(),
        "2606:4700:110:8888::2".parse().unwrap(),
    )
    .unwrap()
}
fn key() -> SessionKey {
    SessionKey {
        session_id: Uuid::new_v4(),
        config_revision: 3,
        network_generation: 4,
    }
}

#[test]
fn dedicated_secret_dto_roundtrips_identity_and_chain_credentials_without_debug_leakage() {
    let identity = identity();
    let mut dto = SessionCredentials::from_identity(&identity).unwrap();
    let mut secrets = ImportSecrets::new(String::new());
    secrets.proxy = Some(ProxyExitConfiguration {
        host: "proxy.example".into(),
        port: 1080,
        auth_mode: ProxyAuthMode::UsernamePassword,
        dns_servers: vec![],
        dns_transport: ProxyDnsTransport::Doh,
    });
    secrets.username = "chain-private-user".into();
    secrets.password = "chain-private-password".into();
    let parsed = ValidatedProfile::parse(ChainSource::Socks5Proxy, &secrets).unwrap();
    let summary = parsed
        .summary("Proxy", Uuid::new_v4(), Uuid::new_v4())
        .unwrap();
    let server = summary.server_summary();
    let prepared = vpngate::PreparedProfile::imported(summary, parsed, secrets);
    dto.chain = Some(ChainCredentials::from_prepared(&server, &prepared).unwrap());
    let encoded = zeroize::Zeroizing::new(serde_json::to_vec(&dto).unwrap());
    let decoded: SessionCredentials = serde_json::from_slice(&encoded).unwrap();
    assert_eq!(
        decoded.identity().unwrap().access_token(),
        identity.access_token()
    );
    let (_, recovered) = decoded
        .chain
        .as_ref()
        .unwrap()
        .prepare(&key(), &UnavailableCredentialBroker)
        .unwrap();
    assert_eq!(recovered.credentials.password, "chain-private-password");
    assert!(!format!("{decoded:?}").contains("chain-private-password"));
    assert!(!format!("{decoded:?}").contains("access-token-private"));
}

struct TestBroker {
    ack_correct: bool,
    saves: AtomicUsize,
}
#[async_trait]
impl CredentialBroker for TestBroker {
    async fn persist_pin(
        &self,
        session: &SessionKey,
        profile_id: Uuid,
        _: &WarpIdentity,
        _: &WarpIdentity,
    ) -> Result<PersistenceAck, String> {
        self.saves.fetch_add(1, Ordering::AcqRel);
        let mut key = session.clone();
        if !self.ack_correct {
            key.network_generation += 1;
        }
        Ok(PersistenceAck {
            session: key,
            profile_id,
            revision: Uuid::new_v4(),
        })
    }
    fn proton_persistence(
        &self,
        _: &SessionKey,
        _: Uuid,
        _: &Record,
    ) -> Result<Arc<dyn LeasePersistence>, String> {
        Err("not_installed".into())
    }
}

#[tokio::test]
async fn pin_update_is_committed_only_after_matching_durable_ack() {
    for correct in [false, true] {
        let original = identity();
        let old_pin = original.endpoint_pin.clone();
        let new_key = MasqueKeyPair::generate();
        let new_pin = EndpointPin::from_spki_der(&new_key.public_spki_der().unwrap()).unwrap();
        let broker = Arc::new(TestBroker {
            ack_correct: correct,
            saves: AtomicUsize::new(0),
        });
        let refresher = BrokerPinRefresher {
            key: key(),
            profile_id: Uuid::new_v4(),
            identity: tokio::sync::Mutex::new(original),
            broker: broker.clone(),
        };
        let result = refresher
            .commit(EndpointPinRefresh {
                endpoint_pin: new_pin.clone(),
                assigned_ipv4: "172.16.0.4".parse().unwrap(),
                assigned_ipv6: "2606:4700:110:8888::4".parse().unwrap(),
            })
            .await;
        assert_eq!(result.is_ok(), correct);
        assert_eq!(broker.saves.load(Ordering::Acquire), 1);
        assert_eq!(
            refresher.identity.lock().await.endpoint_pin,
            if correct { new_pin } else { old_pin }
        );
    }
}

#[test]
fn shared_rule_evaluator_keeps_domain_decisions_distinct_and_core_precedence() {
    let mut profile = Profile {
        allow_lan: true,
        bypass_domains: vec!["example.com".into()],
        ..Default::default()
    };
    profile.routing.rules = vec![
        RoutingRule {
            id: Uuid::new_v4(),
            kind: RoutingMatch::Domain,
            target: "direct.example.com".into(),
            action: RoutingAction::Direct,
        },
        RoutingRule {
            id: Uuid::new_v4(),
            kind: RoutingMatch::Domain,
            target: "proxy.example.com".into(),
            action: RoutingAction::Proxy,
        },
        RoutingRule {
            id: Uuid::new_v4(),
            kind: RoutingMatch::Domain,
            target: "deny.example.com".into(),
            action: RoutingAction::Reject,
        },
        RoutingRule {
            id: Uuid::new_v4(),
            kind: RoutingMatch::Cidr,
            target: "192.168.1.0/24".into(),
            action: RoutingAction::Reject,
        },
    ];
    let core = Arc::new(
        GeoDirectPolicy::disabled()
            .with_custom_rules(&profile)
            .unwrap(),
    );
    let evaluator = PolicyEvaluator::new(core.clone());
    assert_eq!(evaluator.route_host("direct.example.com"), GeoRoute::Direct);
    assert_eq!(evaluator.route_host("proxy.example.com"), GeoRoute::Tunnel);
    assert_eq!(evaluator.route_host("deny.example.com"), GeoRoute::Reject);
    assert_eq!(
        evaluator.route_flow(
            Some("proxy.example.com"),
            Some("192.168.1.2".parse().unwrap())
        ),
        GeoRoute::Reject
    );
    assert_eq!(
        evaluator.route_flow(
            Some("direct.example.com"),
            Some("192.168.2.2".parse().unwrap())
        ),
        GeoRoute::Direct
    );
    // The same resolved IP never overwrites a per-flow hostname decision.
    assert_eq!(
        evaluator.route_ip("192.168.1.2".parse().unwrap()),
        GeoRoute::Reject
    );
    assert_eq!(
        evaluator.route_ip("192.168.2.2".parse().unwrap()),
        GeoRoute::Direct
    );
    for host in [
        "direct.example.com",
        "proxy.example.com",
        "deny.example.com",
        "unknown.test",
    ] {
        assert_eq!(evaluator.route_host(host), core.route_host(host));
    }
}

#[test]
fn packet_tunnel_profile_never_copies_native_flow_filter_or_listener_policy() {
    let mut profile = Profile {
        allow_lan: true,
        disable_quic: true,
        geo_direct_countries: vec!["CN".into()],
        bypass_domains: vec!["direct.test".into()],
        ..Default::default()
    };
    profile.routing.ads_enabled = true;
    profile.routing.rules.push(RoutingRule {
        id: Uuid::new_v4(),
        kind: RoutingMatch::Domain,
        target: "deny.test".into(),
        action: RoutingAction::Reject,
    });
    let packet = usque_macos_runtime::backend::packet_tunnel_profile(&profile);
    assert!(packet.frontends.tunnel);
    assert!(!packet.frontends.socks5 && !packet.frontends.http);
    assert!(!packet.proxy.system_proxy && !packet.allow_lan && !packet.disable_quic);
    assert!(packet.geo_direct_countries.is_empty() && packet.bypass_domains.is_empty());
    assert!(
        packet.routing.rules.is_empty()
            && !packet.routing.ads_enabled
            && packet.split_exclusions.is_empty()
    );
    assert_eq!(packet.chain_exit, profile.chain_exit);
    assert_eq!(packet.endpoint, profile.endpoint);
    packet.validate().unwrap();
}

struct RejectPhysicalSocket {
    cancellation: CancellationToken,
    requests: Mutex<Vec<String>>,
}
impl PlatformAdapter for RejectPhysicalSocket {
    fn request(&self, request: &PlatformRequest) -> Result<(), String> {
        let text = serde_json::to_string(request).unwrap();
        self.requests.lock().unwrap().push(text);
        if matches!(request, PlatformRequest::ProtectSocket { .. }) {
            self.cancellation.cancel();
            return Err("physical_egress_unverified".into());
        }
        Ok(())
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn production_backend_reaches_actual_data_plane_socket_boundary_and_fails_closed() {
    let cancellation = CancellationToken::new();
    let adapter = Arc::new(RejectPhysicalSocket {
        cancellation: cancellation.clone(),
        requests: Mutex::new(vec![]),
    });
    let (mut backend, _packets) =
        DataPlaneBackend::new(adapter.clone(), Arc::new(UnavailableCredentialBroker));
    let mut profile = Profile {
        allow_lan: false,
        transport: TransportPolicy::Http2,
        ip_policy: IpPolicy::Ipv4Only,
        ..Default::default()
    };
    profile.endpoint.selection = EndpointSelection::Custom;
    let cache = tempfile::tempdir().unwrap();
    let configuration = SessionConfiguration::snapshot(profile, cache.path().into()).unwrap();
    let credentials = SessionCredentials::from_identity(&identity()).unwrap();
    let session = key();
    backend.protect(&session).await.unwrap();
    backend
        .configure_dns_routes(&session, &configuration, &credentials)
        .await
        .unwrap();
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        backend.connect(&session, configuration, credentials, cancellation),
    )
    .await;
    assert!(
        result
            .expect("protected socket refusal cancels actual shared runtime startup")
            .is_err()
    );
    let requests = adapter.requests.lock().unwrap();
    assert!(
        requests
            .iter()
            .any(|request| request.contains("protect_socket")),
        "concrete backend must invoke DataPlaneRuntime, not a stub"
    );
    assert!(
        !requests
            .iter()
            .any(|request| request.contains("\"type\":\"allow\""))
    );
    assert!(
        !requests
            .iter()
            .any(|request| request.contains("access-token-private"))
    );
}

#[test]
fn rule_artifact_changes_require_an_explicit_new_snapshot() {
    let cache = tempfile::tempdir().unwrap();
    let profile = Profile {
        allow_lan: false,
        geo_direct_countries: vec!["CN".into()],
        ..Default::default()
    };
    std::fs::create_dir_all(cache.path().join("geo/geoip")).unwrap();
    std::fs::create_dir_all(cache.path().join("geo/geosite")).unwrap();
    std::fs::write(
        cache.path().join("geo/geoip/CN.dat"),
        include_bytes!("../../usque-geo/tests/fixtures/geoip-cn.dat"),
    )
    .unwrap();
    let domains = cache.path().join("geo/geosite/CN.txt");
    std::fs::write(&domains, "domain:old.example.cn\n").unwrap();
    let configuration =
        SessionConfiguration::snapshot(profile.clone(), cache.path().into()).unwrap();
    let old = PolicyEvaluator::load(&configuration).unwrap();
    assert_eq!(old.route_host("old.example.cn"), GeoRoute::Direct);
    std::fs::write(&domains, "domain:new.example.cn\n").unwrap();
    assert!(PolicyEvaluator::load(&configuration).is_err());
    assert_eq!(old.route_host("old.example.cn"), GeoRoute::Direct);
    let replacement = SessionConfiguration::snapshot(profile, cache.path().into()).unwrap();
    assert_ne!(configuration.policy_digest, replacement.policy_digest);
    let new = PolicyEvaluator::load(&replacement).unwrap();
    assert_eq!(new.route_host("new.example.cn"), GeoRoute::Direct);
    assert_eq!(new.route_host("old.example.cn"), GeoRoute::Tunnel);
}
