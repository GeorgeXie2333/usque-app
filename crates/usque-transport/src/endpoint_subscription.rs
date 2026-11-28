//! Runtime endpoint subscription dialing and generation-scoped hostname resolution.
//!
//! Numeric subscription entries are dialed strictly single-stack using only
//! their explicit `IP:Port`, never falling back to dormant manual addresses.
//! Domain subscription entries resolve fresh public unicast candidates on the
//! physical network per connection/reconnect and authorize them on the exact
//! physical network generation.

use std::collections::HashSet;
use std::future::Future;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use tokio::time::timeout;
use tokio_util::sync::CancellationToken;
use usque_core::endpoint_subscriptions::{canonical_public_domain, is_usable_public_endpoint_ip};
use usque_core::{IpPolicy, Profile};

use crate::endpoint_race::RaceConnection;
use crate::h2::TransportError;
use crate::socket::{STALE_GENERATION_REASON, SocketProtector};

pub(crate) const RESOLUTION_TIMEOUT: Duration = Duration::from_secs(4);
pub(crate) const MAX_CANDIDATES_PER_FAMILY: usize = 16;
pub(crate) const BATCH_SIZE: usize = 8;

/// Filters candidate sockets to public unicast addresses matching `port`, caps
/// per family at `MAX_CANDIDATES_PER_FAMILY`, and orders them by `ip_policy`.
pub(crate) fn canonical_subscription_candidates(
    addresses: Vec<SocketAddr>,
    port: u16,
    ip_policy: IpPolicy,
) -> Vec<SocketAddr> {
    if port == 0 {
        return Vec::new();
    }
    let mut v4 = Vec::new();
    let mut v6 = Vec::new();
    let mut seen = HashSet::new();
    for addr in addresses {
        if addr.port() != port || !is_usable_public_endpoint_ip(addr.ip()) {
            continue;
        }
        if !seen.insert(addr) {
            continue;
        }
        match addr {
            SocketAddr::V4(_) if v4.len() < MAX_CANDIDATES_PER_FAMILY => v4.push(addr),
            SocketAddr::V6(_) if v6.len() < MAX_CANDIDATES_PER_FAMILY => v6.push(addr),
            _ => {}
        }
    }
    match ip_policy {
        IpPolicy::Ipv4Only => v4,
        IpPolicy::Ipv6Only => v6,
        IpPolicy::PreferIpv4 => {
            v4.extend(v6);
            v4
        }
        IpPolicy::Auto | IpPolicy::PreferIpv6 => {
            v6.extend(v4);
            v6
        }
    }
}

pub async fn resolve_physical_endpoint_hostname<S: SocketProtector + ?Sized>(
    protector: &S,
    host: &str,
    port: u16,
    ip_policy: IpPolicy,
    expected_generation: u64,
) -> Result<Vec<SocketAddr>, String> {
    crate::split_dns::resolve_endpoint_hostname_ips(
        protector,
        host,
        port,
        ip_policy,
        expected_generation,
    )
    .await
}

pub async fn resolve_system_endpoint_hostname(
    host: &str,
    port: u16,
    ip_policy: IpPolicy,
) -> Result<Vec<SocketAddr>, String> {
    let host = host.to_owned();
    tokio::task::spawn_blocking(move || system_addresses(&host, port, ip_policy))
        .await
        .map_err(|_| "endpoint subscription resolver failed".to_owned())?
}

fn protection_error(reason: String) -> TransportError {
    let token = reason
        .strip_prefix("preset_dns_protection:")
        .unwrap_or(&reason);
    if token == STALE_GENERATION_REASON {
        TransportError::UnderlyingNetworkChanged
    } else {
        TransportError::SocketProtection(reason)
    }
}

pub(crate) struct ResolvedDomain {
    pub(crate) candidates: Vec<SocketAddr>,
    pub(crate) generation: u64,
}

pub(crate) async fn resolve_domain(
    profile: &Profile,
    host: &str,
    port: u16,
    protector: &Arc<dyn SocketProtector>,
) -> Result<ResolvedDomain, TransportError> {
    let canonical = canonical_public_domain(host).ok_or(TransportError::InvalidIdentity)?;
    if port == 0 {
        return Err(TransportError::InvalidIdentity);
    }
    let generation = protector.network_generation().unwrap_or_default();
    protector
        .authorize_endpoint_hostname_candidates(&canonical, &[], generation)
        .await
        .map_err(protection_error)?;

    let resolved = timeout(
        RESOLUTION_TIMEOUT,
        protector.resolve_endpoint_hostname(&canonical, port, profile.ip_policy, generation),
    )
    .await
    .map_err(|_| TransportError::Dns("endpoint subscription DNS lookup timed out".to_owned()))?
    .map_err(|reason| {
        if reason.starts_with("preset_dns_protection:") || reason == STALE_GENERATION_REASON {
            protection_error(reason)
        } else {
            TransportError::Dns(reason)
        }
    })?;

    if protector.network_generation().unwrap_or_default() != generation {
        return Err(TransportError::UnderlyingNetworkChanged);
    }

    let mut candidates = canonical_subscription_candidates(resolved, port, profile.ip_policy);
    // Agents reject a batch that names a tunnel DNS server; such an address
    // can never be a MASQUE endpoint for this session anyway.
    candidates.retain(|addr| {
        protector.endpoint_family_available(*addr) != Some(false)
            && !profile.dns_servers.contains(&addr.ip())
    });
    if candidates.is_empty() {
        return Err(TransportError::Dns(
            "endpoint subscription DNS returned no usable public endpoint".to_owned(),
        ));
    }

    protector
        .authorize_endpoint_hostname_candidates(&canonical, &candidates, generation)
        .await
        .map_err(protection_error)?;

    Ok(ResolvedDomain {
        candidates,
        generation,
    })
}

/// Connects a domain subscription endpoint by resolving fresh public unicast
/// candidates on the current physical network generation and racing them in
/// batches without ever falling back to dormant manual addresses.
pub(crate) async fn connect_domain_cancellable<T, F, Fut>(
    profile: &Profile,
    host: &str,
    port: u16,
    protector: &Arc<dyn SocketProtector>,
    cancellation: &CancellationToken,
    mut attempt: F,
) -> Result<T, TransportError>
where
    T: RaceConnection,
    F: FnMut(Vec<SocketAddr>, u64) -> Fut,
    Fut: Future<Output = Result<T, TransportError>>,
{
    let canonical = canonical_public_domain(host).ok_or(TransportError::InvalidIdentity)?;
    for round in 0..4 {
        if cancellation.is_cancelled() {
            return Err(TransportError::TunnelClosed);
        }
        let resolution = tokio::select! {
            biased;
            _ = cancellation.cancelled() => {
                // A cancelled lookup may already have authorized candidates.
                let generation = protector.network_generation().unwrap_or_default();
                let _ = protector
                    .authorize_endpoint_hostname_candidates(&canonical, &[], generation)
                    .await;
                return Err(TransportError::TunnelClosed);
            }
            result = resolve_domain(profile, &canonical, port, protector) => result,
        };
        let resolved = match resolution {
            Ok(resolved) => resolved,
            Err(TransportError::UnderlyingNetworkChanged) if round < 3 => continue,
            Err(error) => return Err(error),
        };
        let generation = resolved.generation;
        let mut last_error = None;
        for chunk in resolved.candidates.chunks(BATCH_SIZE) {
            if cancellation.is_cancelled() {
                let _ = protector
                    .authorize_endpoint_hostname_candidates(&canonical, &[], generation)
                    .await;
                return Err(TransportError::TunnelClosed);
            }
            if protector.network_generation().unwrap_or_default() != generation {
                last_error = Some(TransportError::UnderlyingNetworkChanged);
                break;
            }
            match attempt(chunk.to_vec(), generation).await {
                // A connection completed on a superseded network generation is
                // not trusted; the next round resolves on the new network.
                Ok(connection)
                    if protector.network_generation().unwrap_or_default() != generation =>
                {
                    connection.shutdown().await;
                    last_error = Some(TransportError::UnderlyingNetworkChanged);
                    break;
                }
                Ok(connection) => return Ok(connection),
                Err(error) => {
                    let stale = matches!(error, TransportError::UnderlyingNetworkChanged)
                        || protector.network_generation().unwrap_or_default() != generation;
                    last_error = Some(error);
                    if stale {
                        break;
                    }
                    if !crate::recovery_policy::RecoveryDecision::may_try_another_candidate(
                        last_error.as_ref().expect("recorded error"),
                    ) {
                        let _ = protector
                            .authorize_endpoint_hostname_candidates(&canonical, &[], generation)
                            .await;
                        return Err(last_error.expect("recorded error"));
                    }
                }
            }
        }
        let _ = protector
            .authorize_endpoint_hostname_candidates(&canonical, &[], generation)
            .await;
        let error = last_error.unwrap_or_else(|| {
            TransportError::AllEndpointsFailed("endpoint subscription cycle exhausted".to_owned())
        });
        if matches!(error, TransportError::UnderlyingNetworkChanged) && round < 3 {
            continue;
        }
        return Err(error);
    }
    Err(TransportError::UnderlyingNetworkChanged)
}

#[cfg(unix)]
fn system_addresses(host: &str, port: u16, ip_policy: IpPolicy) -> Result<Vec<SocketAddr>, String> {
    use std::net::{Ipv4Addr, Ipv6Addr};
    let host_cstr = std::ffi::CString::new(host).map_err(|_| "invalid endpoint hostname")?;
    let family = match ip_policy {
        IpPolicy::Ipv4Only => libc::AF_INET,
        IpPolicy::Ipv6Only => libc::AF_INET6,
        IpPolicy::Auto | IpPolicy::PreferIpv4 | IpPolicy::PreferIpv6 => libc::AF_UNSPEC,
    };
    // SAFETY: addrinfo is zero-initialized, host_cstr stays alive across getaddrinfo,
    // and result is freed via freeaddrinfo before return.
    unsafe {
        let mut hints: libc::addrinfo = std::mem::zeroed();
        hints.ai_family = family;
        hints.ai_socktype = libc::SOCK_STREAM;
        let mut result = std::ptr::null_mut();
        if libc::getaddrinfo(host_cstr.as_ptr(), std::ptr::null(), &hints, &mut result) != 0 {
            return Err("endpoint subscription DNS lookup failed".to_owned());
        }
        let mut addresses = Vec::new();
        let mut numeric_host = [0u8; 1025];
        let mut entry = result;
        while let Some(value) = entry.as_ref() {
            entry = value.ai_next;
            if value.ai_addr.is_null() {
                continue;
            }
            let status = libc::getnameinfo(
                value.ai_addr,
                value.ai_addrlen,
                numeric_host.as_mut_ptr().cast(),
                numeric_host.len() as _,
                std::ptr::null_mut(),
                0,
                libc::NI_NUMERICHOST,
            );
            if status != 0 {
                continue;
            }
            let Ok(cstr) = std::ffi::CStr::from_bytes_until_nul(&numeric_host) else {
                continue;
            };
            let Ok(raw) = cstr.to_str() else {
                continue;
            };
            let ip_part = raw.split('%').next().unwrap_or(raw);
            if let Ok(ip) = ip_part.parse::<Ipv4Addr>() {
                addresses.push(SocketAddr::new(IpAddr::V4(ip), port));
            } else if let Ok(ip) = ip_part.parse::<Ipv6Addr>() {
                addresses.push(SocketAddr::new(IpAddr::V6(ip), port));
            }
        }
        libc::freeaddrinfo(result);
        Ok(canonical_subscription_candidates(
            addresses, port, ip_policy,
        ))
    }
}

#[cfg(windows)]
fn system_addresses(host: &str, port: u16, ip_policy: IpPolicy) -> Result<Vec<SocketAddr>, String> {
    use std::net::{Ipv4Addr, Ipv6Addr};
    use windows_sys::Win32::Networking::WinSock::{
        ADDRINFOW, AF_INET, AF_INET6, AF_UNSPEC, FreeAddrInfoW, GetAddrInfoW, NI_NUMERICHOST,
        SOCK_STREAM, getnameinfo,
    };
    let _winsock = std::net::UdpSocket::bind("127.0.0.1:0")
        .map_err(|_| "initialize endpoint resolver".to_owned())?;
    let wide: Vec<u16> = host.encode_utf16().chain(Some(0)).collect();
    let family = match ip_policy {
        IpPolicy::Ipv4Only => AF_INET,
        IpPolicy::Ipv6Only => AF_INET6,
        IpPolicy::Auto | IpPolicy::PreferIpv4 | IpPolicy::PreferIpv6 => AF_UNSPEC,
    };
    // SAFETY: zero-initialized hints and UTF-16 host remain valid across GetAddrInfoW,
    // and result is freed once via FreeAddrInfoW.
    unsafe {
        let mut hints: ADDRINFOW = std::mem::zeroed();
        hints.ai_family = i32::from(family);
        hints.ai_socktype = SOCK_STREAM;
        let mut result = std::ptr::null_mut();
        if GetAddrInfoW(wide.as_ptr(), std::ptr::null(), &hints, &mut result) != 0 {
            return Err("endpoint subscription DNS lookup failed".to_owned());
        }
        let mut addresses = Vec::new();
        let mut numeric_host = [0u8; 1025];
        let mut entry = result;
        while let Some(value) = entry.as_ref() {
            entry = value.ai_next;
            if value.ai_addr.is_null() {
                continue;
            }
            let Ok(len) = value.ai_addrlen.try_into() else {
                continue;
            };
            let status = getnameinfo(
                value.ai_addr,
                len,
                numeric_host.as_mut_ptr(),
                numeric_host.len() as u32,
                std::ptr::null_mut(),
                0,
                NI_NUMERICHOST as i32,
            );
            if status != 0 {
                continue;
            }
            let Ok(cstr) = std::ffi::CStr::from_bytes_until_nul(&numeric_host) else {
                continue;
            };
            let Ok(raw) = cstr.to_str() else {
                continue;
            };
            let ip_part = raw.split('%').next().unwrap_or(raw);
            if let Ok(ip) = ip_part.parse::<Ipv4Addr>() {
                addresses.push(SocketAddr::new(IpAddr::V4(ip), port));
            } else if let Ok(ip) = ip_part.parse::<Ipv6Addr>() {
                addresses.push(SocketAddr::new(IpAddr::V6(ip), port));
            }
        }
        FreeAddrInfoW(result);
        Ok(canonical_subscription_candidates(
            addresses, port, ip_policy,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
    use usque_core::endpoint_subscriptions::{SubscriptionEndpoint, SubscriptionEndpointHost};

    #[test]
    fn canonical_candidates_filter_private_and_order_by_ip_policy() {
        let input = vec![
            "127.0.0.1:443".parse().unwrap(),
            "10.0.0.1:443".parse().unwrap(),
            "100.64.0.1:443".parse().unwrap(),
            "[::1]:443".parse().unwrap(),
            "[fe80::1]:443".parse().unwrap(),
            "[fc00::1]:443".parse().unwrap(),
            "162.159.193.10:8443".parse().unwrap(), // wrong port
            "162.159.193.10:443".parse().unwrap(),
            "[2606:4700:103::2]:443".parse().unwrap(),
            "162.159.193.10:443".parse().unwrap(), // duplicate
        ];
        assert_eq!(
            canonical_subscription_candidates(input.clone(), 443, IpPolicy::Ipv4Only),
            vec!["162.159.193.10:443".parse::<SocketAddr>().unwrap()]
        );
        assert_eq!(
            canonical_subscription_candidates(input.clone(), 443, IpPolicy::Ipv6Only),
            vec!["[2606:4700:103::2]:443".parse::<SocketAddr>().unwrap()]
        );
        assert_eq!(
            canonical_subscription_candidates(input.clone(), 443, IpPolicy::PreferIpv4),
            vec![
                "162.159.193.10:443".parse().unwrap(),
                "[2606:4700:103::2]:443".parse().unwrap(),
            ]
        );
        assert_eq!(
            canonical_subscription_candidates(input, 443, IpPolicy::PreferIpv6),
            vec![
                "[2606:4700:103::2]:443".parse().unwrap(),
                "162.159.193.10:443".parse().unwrap(),
            ]
        );
    }

    #[test]
    fn numeric_subscription_targets_remain_strictly_single_stack() {
        let mut profile = Profile::default();
        profile.endpoint.selection = usque_core::EndpointSelection::Custom;
        profile.endpoint.subscription_id = Some(uuid::Uuid::new_v4());
        profile.endpoint.port = 8443;
        profile.subscription_endpoint = Some(SubscriptionEndpoint {
            id: "v4:162.159.193.10:2408".into(),
            host: SubscriptionEndpointHost::Ipv4("162.159.193.10".parse().unwrap()),
            port: Some(2408),
            note: String::new(),
            pinned: false,
        });
        assert_eq!(
            crate::h3_probe_endpoints(&profile),
            vec!["162.159.193.10:2408".parse::<SocketAddr>().unwrap()]
        );

        profile.subscription_endpoint = Some(SubscriptionEndpoint {
            id: "v6:2606:4700:103::9:default".into(),
            host: SubscriptionEndpointHost::Ipv6("2606:4700:103::9".parse().unwrap()),
            port: None,
            note: String::new(),
            pinned: false,
        });
        assert_eq!(
            crate::h3_probe_endpoints(&profile),
            vec!["[2606:4700:103::9]:8443".parse::<SocketAddr>().unwrap()]
        );
    }

    #[derive(Default)]
    struct MockHostProtector {
        generation: AtomicU64,
        change_during_lookup: AtomicBool,
        authorizations: Mutex<Vec<(String, Vec<SocketAddr>, u64)>>,
        addresses: Vec<SocketAddr>,
    }

    #[async_trait::async_trait]
    impl SocketProtector for MockHostProtector {
        fn protect(&self, _: crate::SocketHandle) -> Result<(), String> {
            Ok(())
        }
        fn network_generation(&self) -> Option<u64> {
            Some(self.generation.load(Ordering::SeqCst))
        }
        async fn resolve_endpoint_hostname(
            &self,
            _: &str,
            _: u16,
            _: IpPolicy,
            expected: u64,
        ) -> Result<Vec<SocketAddr>, String> {
            assert_eq!(self.network_generation(), Some(expected));
            if self.change_during_lookup.swap(false, Ordering::SeqCst) {
                self.generation.fetch_add(1, Ordering::SeqCst);
            }
            Ok(self.addresses.clone())
        }
        async fn authorize_endpoint_hostname_candidates(
            &self,
            host: &str,
            candidates: &[SocketAddr],
            expected: u64,
        ) -> Result<(), String> {
            self.authorizations.lock().unwrap().push((
                host.to_owned(),
                candidates.to_vec(),
                expected,
            ));
            Ok(())
        }
    }

    #[tokio::test]
    async fn generation_fenced_resolution_rejects_other_and_changed_generations() {
        let inner = Arc::new(MockHostProtector {
            generation: AtomicU64::new(4),
            change_during_lookup: AtomicBool::new(false),
            authorizations: Mutex::new(Vec::new()),
            addresses: vec!["162.159.193.10:2408".parse().unwrap()],
        });
        let fenced = crate::endpoint_preset::for_generation(inner.clone(), 4);
        // A caller cannot resolve for any generation but the fenced one.
        assert_eq!(
            fenced
                .resolve_endpoint_hostname("edge.example.com", 2408, IpPolicy::Ipv4Only, 5)
                .await,
            Err(crate::socket::STALE_GENERATION_REASON.to_owned())
        );
        assert!(
            fenced
                .resolve_endpoint_hostname("edge.example.com", 2408, IpPolicy::Ipv4Only, 4)
                .await
                .is_ok()
        );
        // Answers from a network that changed during the lookup are dropped.
        inner.change_during_lookup.store(true, Ordering::SeqCst);
        assert_eq!(
            fenced
                .resolve_endpoint_hostname("edge.example.com", 2408, IpPolicy::Ipv4Only, 4)
                .await,
            Err(crate::socket::STALE_GENERATION_REASON.to_owned())
        );
    }

    #[tokio::test]
    async fn resolved_candidates_never_include_a_tunnel_dns_server() {
        let protector: Arc<dyn SocketProtector> = Arc::new(MockHostProtector {
            generation: AtomicU64::new(1),
            change_during_lookup: AtomicBool::new(false),
            authorizations: Mutex::new(Vec::new()),
            addresses: vec![
                "162.159.36.1:2408".parse().unwrap(),
                "162.159.193.10:2408".parse().unwrap(),
            ],
        });
        let profile = Profile {
            ip_policy: IpPolicy::Ipv4Only,
            dns_servers: vec!["162.159.36.1".parse().unwrap()],
            ..Profile::default()
        };
        let resolved = resolve_domain(&profile, "edge.example.com", 2408, &protector)
            .await
            .unwrap();
        assert_eq!(
            resolved.candidates,
            vec!["162.159.193.10:2408".parse::<SocketAddr>().unwrap()]
        );
    }

    struct DummyConn;
    #[async_trait::async_trait]
    impl RaceConnection for DummyConn {
        async fn shutdown(self) {}
        fn is_alive(&self) -> bool {
            true
        }
    }

    #[tokio::test]
    async fn domain_connect_authorizes_clears_and_retries_stale_generation() {
        let protector_impl = Arc::new(MockHostProtector {
            generation: AtomicU64::new(1),
            change_during_lookup: AtomicBool::new(true),
            authorizations: Mutex::new(Vec::new()),
            addresses: vec!["162.159.193.10:2408".parse().unwrap()],
        });
        let protector: Arc<dyn SocketProtector> = protector_impl.clone();
        let profile = Profile {
            ip_policy: IpPolicy::Ipv4Only,
            ..Profile::default()
        };
        let calls = AtomicUsize::new(0);
        let cancel = CancellationToken::new();
        let result = connect_domain_cancellable(
            &profile,
            "edge.example.com",
            2408,
            &protector,
            &cancel,
            |batch, generation| {
                calls.fetch_add(1, Ordering::SeqCst);
                protector_impl
                    .change_during_lookup
                    .store(false, Ordering::SeqCst);
                assert_eq!(
                    batch,
                    vec!["162.159.193.10:2408".parse::<SocketAddr>().unwrap()]
                );
                assert_eq!(generation, 2);
                async { Ok(DummyConn) }
            },
        )
        .await;
        // First round observed generation flip and cleared before retry.
        assert!(result.is_ok() || matches!(result, Err(TransportError::UnderlyingNetworkChanged)));
        if matches!(result, Err(TransportError::UnderlyingNetworkChanged)) {
            unreachable!("second round has stable generation");
        }
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let auths = protector_impl.authorizations.lock().unwrap();
        assert_eq!(auths[0].1.len(), 0);
        assert_eq!(auths[0].2, 1);
        assert_eq!(auths.last().unwrap().2, 2);
        assert_eq!(auths.last().unwrap().1.len(), 1);
    }
}
