use super::*;
use crate::socket::{DirectEgressLease, DirectProtocol, SocketHandle};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

#[derive(Default)]
struct Resolver {
    generation: AtomicU64,
    calls: Mutex<Vec<String>>,
    authorizations: Mutex<Vec<(u64, Vec<SocketAddr>)>>,
    addresses: Vec<SocketAddr>,
    failure: Option<&'static str>,
    pending: bool,
    change_during_lookup: bool,
    deny_authorization: bool,
    authorization_failure: Option<(usize, &'static str)>,
}

#[async_trait::async_trait]
impl SocketProtector for Resolver {
    fn protect(&self, _: SocketHandle) -> Result<(), String> {
        panic!("fixture opens no data socket")
    }
    fn network_generation(&self) -> Option<u64> {
        Some(self.generation.load(Ordering::SeqCst))
    }
    async fn resolve_endpoint_preset(
        &self,
        host: &str,
        _: u16,
        expected: u64,
    ) -> Result<Vec<SocketAddr>, String> {
        assert_eq!(self.network_generation(), Some(expected));
        self.calls.lock().unwrap().push(host.to_owned());
        if self.pending {
            std::future::pending::<()>().await;
        }
        if self.change_during_lookup {
            self.generation.fetch_add(1, Ordering::SeqCst);
        }
        match self.failure {
            Some(reason) => Err(reason.into()),
            None => Ok(self.addresses.clone()),
        }
    }
    async fn authorize_endpoint_preset_candidates(
        &self,
        _: &str,
        candidates: &[SocketAddr],
        expected: u64,
    ) -> Result<(), String> {
        let call = {
            let mut authorizations = self.authorizations.lock().unwrap();
            authorizations.push((expected, candidates.to_vec()));
            authorizations.len()
        };
        if let Some((failure_call, reason)) = self.authorization_failure
            && call == failure_call
        {
            return Err(reason.into());
        }
        if self.deny_authorization {
            Err("fixture denial".into())
        } else {
            Ok(())
        }
    }
}

fn identity() -> MasqueTlsIdentity {
    let key = usque_core::MasqueKeyPair::generate();
    let mut identity = MasqueTlsIdentity::new(
        key.private_sec1_der().unwrap(),
        &key.public_spki_der().unwrap(),
        "172.16.0.2".parse().unwrap(),
        "2001:db8::2".parse().unwrap(),
    )
    .unwrap();
    identity.provider = Some(IdentityProvider::Consumer);
    identity.entitlement = Some(usque_core::ConsumerEntitlement::Free);
    identity
}

fn profile(policy: IpPolicy) -> Profile {
    let mut profile = Profile {
        ip_policy: policy,
        ..Profile::default()
    };
    profile.endpoint.selection = usque_core::EndpointSelection::Custom;
    profile.endpoint.preset_region = Some(usque_core::EndpointPresetRegion::Us);
    profile.endpoint.port = 443;
    profile
}

fn resolver() -> Arc<Resolver> {
    Arc::new(Resolver {
        addresses: vec!["[2001:db8::8]:443".parse().unwrap()],
        ..Default::default()
    })
}

#[test]
fn authenticated_tier_never_guesses_legacy_or_profile_account_type() {
    let mut identity = identity();
    assert_eq!(tier(&identity).unwrap(), EndpointPresetTier::Free);
    identity.entitlement = Some(usque_core::ConsumerEntitlement::WarpPlus);
    assert_eq!(tier(&identity).unwrap(), EndpointPresetTier::Plus);
    identity.entitlement = None;
    assert!(matches!(
        tier(&identity),
        Err(TransportError::InvalidIdentity)
    ));
    identity.provider = Some(IdentityProvider::ZeroTrust {
        organization: "fixture".into(),
    });
    assert_eq!(tier(&identity).unwrap(), EndpointPresetTier::ZeroTrust);
    identity.provider = None;
    assert!(matches!(
        tier(&identity),
        Err(TransportError::InvalidIdentity)
    ));
}

#[test]
fn numeric_host_accepts_scopes_and_requires_a_terminated_ipv6_literal() {
    let link_local = "fe80::1".parse().unwrap();
    assert_eq!(parse_numeric_ipv6(b"fe80::1%en0\0"), Some(link_local));
    assert_eq!(parse_numeric_ipv6(b"fe80::1"), None);
    assert_eq!(parse_numeric_ipv6(b"192.0.2.1\0"), None);
}

#[test]
fn system_resolver_keeps_only_usable_numeric_ipv6_and_preserves_the_requested_port() {
    let port = 8443;
    assert_eq!(
        system_ipv6("2001:db8::123", port).unwrap(),
        vec!["[2001:db8::123]:8443".parse::<SocketAddr>().unwrap()]
    );
    assert!(system_ipv6("::1", port).unwrap().is_empty());
    assert!(system_ipv6("::ffff:192.0.2.1", port).unwrap().is_empty());
}

#[test]
fn candidates_are_finite_ipv6_unique_and_exact_port() {
    let mut addresses = (1..=30)
        .map(|n| format!("[2001:db8::{n:x}]:443").parse().unwrap())
        .collect::<Vec<_>>();
    addresses.extend(addresses.clone());
    for invalid in [
        "127.0.0.1:443",
        "[::]:443",
        "[::1]:443",
        "[ff02::1]:443",
        "[fe80::1]:443",
        "[fc00::1]:443",
        "[::ffff:192.0.2.1]:443",
        "[2001:db8::1]:8443",
    ] {
        addresses.push(invalid.parse().unwrap());
    }
    let candidates = canonical_candidates(addresses, 443);
    assert_eq!(candidates.len(), 16);
    assert!(
        candidates
            .iter()
            .all(|address| usable_ipv6(address) && address.port() == 443)
    );
}

#[tokio::test]
async fn resolves_new_records_each_attempt_and_clears_old_authorization_first() {
    let resolver = resolver();
    let protector: Arc<dyn SocketProtector> = resolver.clone();
    let profile = profile(IpPolicy::Ipv6Only);
    let mut identity = identity();
    resolve(&profile, &identity, &protector).await.unwrap();
    resolver.generation.store(2, Ordering::SeqCst);
    identity.entitlement = Some(usque_core::ConsumerEntitlement::WarpPlus);
    resolve(&profile, &identity, &protector).await.unwrap();
    assert_eq!(
        *resolver.calls.lock().unwrap(),
        ["us.v6.free.orangesky.dev", "us.v6.plus.orangesky.dev"]
    );
    let authorizations = resolver.authorizations.lock().unwrap();
    assert_eq!(authorizations.len(), 4);
    assert!(authorizations[0].1.is_empty() && authorizations[2].1.is_empty());
    assert_eq!(authorizations[3].0, 2);
}

#[tokio::test(start_paused = true)]
async fn empty_timeout_network_change_and_authorization_fail_closed() {
    for fixture in [
        Resolver::default(),
        Resolver {
            pending: true,
            ..Default::default()
        },
        Resolver {
            change_during_lookup: true,
            ..Default::default()
        },
        Resolver {
            deny_authorization: true,
            ..Default::default()
        },
        Resolver {
            failure: Some("preset_dns_protection:denied"),
            ..Default::default()
        },
    ] {
        let changed = fixture.change_during_lookup;
        let denied = fixture.deny_authorization || fixture.failure.is_some();
        let protector: Arc<dyn SocketProtector> = Arc::new(fixture);
        let result = resolve(&profile(IpPolicy::Ipv6Only), &identity(), &protector).await;
        if changed {
            assert!(matches!(
                result,
                Err(TransportError::UnderlyingNetworkChanged)
            ));
        } else if denied {
            assert!(matches!(result, Err(TransportError::SocketProtection(_))));
        } else {
            assert!(matches!(result, Err(TransportError::Dns(_))));
        }
    }
}

struct Connection(Arc<AtomicUsize>);
#[async_trait::async_trait]
impl crate::endpoint_race::RaceConnection for Connection {
    async fn shutdown(self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
    fn is_alive(&self) -> bool {
        true
    }
}

#[tokio::test]
async fn agent_stale_generation_stops_the_round_without_waiting_for_the_local_watch() {
    for (authorization_failure, dns_failure, expected_authorizations, expected_queries) in [
        (Some((1, "stale_generation")), None, 1, 0),
        (Some((2, "stale_generation")), None, 2, 1),
        (None, Some("stale_generation"), 1, 1),
        (None, Some("preset_dns_protection:stale_generation"), 1, 1),
    ] {
        let resolver = Arc::new(Resolver {
            addresses: vec!["[2001:db8::8]:443".parse().unwrap()],
            authorization_failure,
            failure: dns_failure,
            ..Default::default()
        });
        let protector: Arc<dyn SocketProtector> = resolver.clone();
        let profile = profile(IpPolicy::PreferIpv6);
        let attempts = AtomicUsize::new(0);
        let result = connect::<Connection, _, _>(
            &profile,
            &identity(),
            &protector,
            &Default::default(),
            |_, _| {
                attempts.fetch_add(1, Ordering::SeqCst);
                std::future::ready(Err(TransportError::SocketProtection(
                    "unexpected socket".into(),
                )))
            },
        )
        .await;
        let error = match result {
            Err(error) => error,
            Ok(_) => panic!("stale Agent generation must cancel discovery"),
        };
        assert!(matches!(error, TransportError::UnderlyingNetworkChanged));
        let failure = error.failure(None, None);
        assert_eq!(
            failure.code,
            usque_core::TransportFailureCode::PhysicalNetworkChanged
        );
        assert!(failure.retryable);
        assert!(!may_fallback(&profile, &error));
        assert_eq!(
            attempts.load(Ordering::SeqCst),
            0,
            "neither IPv6 nor IPv4 may use stale authorization"
        );
        assert_eq!(
            resolver.network_generation(),
            Some(0),
            "local watch deliberately remains stale"
        );
        assert_eq!(
            resolver.authorizations.lock().unwrap().len(),
            expected_authorizations
        );
        assert_eq!(resolver.calls.lock().unwrap().len(), expected_queries);
    }
}

#[tokio::test]
async fn ipv4_is_admitted_only_after_all_preset_attempts_fail_retryably() {
    let protector: Arc<dyn SocketProtector> = resolver();
    let profile = profile(IpPolicy::PreferIpv6);
    let calls = Arc::new(Mutex::new(Vec::new()));
    let closed = Arc::new(AtomicUsize::new(0));
    let connected = connect(
        &profile,
        &identity(),
        &protector,
        &Default::default(),
        |addresses, _| {
            calls.lock().unwrap().push(addresses.clone());
            let closed = closed.clone();
            async move {
                if addresses[0].is_ipv6() {
                    Err(TransportError::EndpointTimeout(addresses[0]))
                } else {
                    Ok(Connection(closed))
                }
            }
        },
    )
    .await;
    assert!(connected.is_ok());
    let calls = calls.lock().unwrap();
    assert_eq!(calls.len(), 2);
    assert!(calls[0].iter().all(SocketAddr::is_ipv6));
    assert_eq!(calls[1], [profile.endpoint.ipv4_socket()]);
    assert!(
        !calls
            .iter()
            .flatten()
            .any(|address| *address == profile.endpoint.ipv6_socket())
    );
}

#[tokio::test]
async fn pin_identity_authorization_and_forced_ipv6_never_fall_back() {
    for (policy, error) in [
        (IpPolicy::PreferIpv6, TransportError::EndpointPinMismatch),
        (IpPolicy::PreferIpv6, TransportError::InvalidIdentity),
        (
            IpPolicy::PreferIpv6,
            TransportError::SocketProtection("denied".into()),
        ),
        (
            IpPolicy::PreferIpv6,
            TransportError::Http3ConnectRejected(403),
        ),
        (IpPolicy::Ipv6Only, TransportError::Dns("empty".into())),
    ] {
        let protector: Arc<dyn SocketProtector> = resolver();
        let calls = AtomicUsize::new(0);
        let mut error = Some(error);
        let result = connect::<Connection, _, _>(
            &profile(policy),
            &identity(),
            &protector,
            &Default::default(),
            |_, _| {
                calls.fetch_add(1, Ordering::SeqCst);
                std::future::ready(Err(error.take().expect("must not admit IPv4")))
            },
        )
        .await;
        assert!(result.is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
}

#[tokio::test]
async fn changed_generation_closes_a_late_success_instead_of_using_or_falling_back() {
    let resolver = resolver();
    let protector: Arc<dyn SocketProtector> = resolver.clone();
    let closed = Arc::new(AtomicUsize::new(0));
    let result = connect(
        &profile(IpPolicy::PreferIpv6),
        &identity(),
        &protector,
        &Default::default(),
        |_, _| {
            resolver.generation.fetch_add(1, Ordering::SeqCst);
            std::future::ready(Ok(Connection(closed.clone())))
        },
    )
    .await;
    assert!(matches!(
        result,
        Err(TransportError::UnderlyingNetworkChanged)
    ));
    assert_eq!(closed.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn fixed_generation_cannot_authorize_a_stale_record_on_a_new_network() {
    let resolver = resolver();
    let protected = for_generation(resolver.clone(), 0);
    resolver.generation.store(1, Ordering::SeqCst);
    let socket = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    let result: Result<DirectEgressLease, _> = protected
        .protect_masque_endpoint_generation(
            crate::socket::socket_handle(&socket),
            "[2001:db8::8]:443".parse().unwrap(),
            DirectProtocol::Udp,
            1,
        )
        .await;
    assert_eq!(result.unwrap_err(), crate::socket::STALE_GENERATION_REASON);
}
