//! Explicit Deep Doctor probes; no CONNECT-IP or OS mutation.
use std::{future::Future, net::SocketAddr, sync::Arc, time::Duration};

use bytes::Bytes;
use tokio::time::{Instant, timeout_at};
use tokio_util::sync::CancellationToken;
use usque_core::{DirectDnsMode, DirectDnsSettings, IpPolicy, Profile};

use crate::{
    DirectDnsError, DirectDnsQueryContext, DirectDnsResolver, MasqueTlsIdentity,
    NetworkQualityTelemetry, SocketProtector, TransportError,
};

pub const PROBE_IO_TIMEOUT: Duration = Duration::from_millis(3_800);

/// Allowlisted results only, never remote errors or identifying data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkProbeResult {
    Passed { milliseconds: u64 },
    NotApplicable,
    Failed,
    TimedOut,
    Cancelled,
    NetworkChanged,
}

/// Retry eligibility is internal to the probe runner. The public diagnostic
/// contract intentionally keeps the same compact, allowlisted result codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum H3ProbeTerminal {
    PinMismatch,
    Identity,
    Protection,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum H3ProbeOutcome {
    Complete(NetworkProbeResult),
    Terminal(H3ProbeTerminal),
}

impl H3ProbeOutcome {
    pub(crate) fn public_result(self) -> NetworkProbeResult {
        match self {
            Self::Complete(result) => result,
            Self::Terminal(_) => NetworkProbeResult::Failed,
        }
    }
}

impl From<NetworkProbeResult> for H3ProbeOutcome {
    fn from(result: NetworkProbeResult) -> Self {
        Self::Complete(result)
    }
}

pub async fn probe_encrypted_dns(
    settings: &DirectDnsSettings,
    protector: Arc<dyn SocketProtector>,
    cancellation: CancellationToken,
) -> NetworkProbeResult {
    if settings.mode == DirectDnsMode::PhysicalSystem {
        return NetworkProbeResult::NotApplicable;
    }
    if cancellation.is_cancelled() {
        return NetworkProbeResult::Cancelled;
    }
    let started = Instant::now();
    let lifetime = cancellation.child_token();
    let _cancel_on_drop = lifetime.clone().drop_guard();
    let resolver = match DirectDnsResolver::new(
        settings,
        Arc::clone(&protector),
        NetworkQualityTelemetry::default(),
        &lifetime,
    ) {
        Ok(resolver) => resolver,
        Err(_) => return NetworkProbeResult::Failed,
    };
    run_dns_probe(resolver, protector, cancellation, lifetime, started).await
}

pub(crate) async fn run_dns_probe(
    resolver: Arc<DirectDnsResolver>,
    protector: Arc<dyn SocketProtector>,
    cancellation: CancellationToken,
    lifetime: CancellationToken,
    started: Instant,
) -> NetworkProbeResult {
    let _cancel_on_drop = lifetime.clone().drop_guard();
    let generation = protector.network_generation().unwrap_or_default();
    // A reserved, fixed name, never a user's query. NXDOMAIN is a valid reply.
    let query = Bytes::from_static(b"\x12\x34\x01\x00\x00\x01\x00\x00\x00\x00\x00\x00\x07example\x07invalid\x00\x00\x01\x00\x01");
    let deadline = started + PROBE_IO_TIMEOUT;
    let outcome = tokio::select! {
        biased;
        _ = cancellation.cancelled() => Err(DirectDnsError::Cancelled),
        reply = timeout_at(deadline, resolver.query(query, DirectDnsQueryContext { network_generation: generation, deadline })) =>
            reply.unwrap_or(Err(DirectDnsError::Timeout)),
    };
    lifetime.cancel();
    // A dedicated pool avoids changing or clearing any business DNS pool.
    // Wait for actual I/O destruction, not only for a driver abort request.
    let cleaned = resolver.close_probe_pool().await;
    if cancellation.is_cancelled() {
        return NetworkProbeResult::Cancelled;
    }
    if protector.network_generation().unwrap_or_default() != generation {
        return NetworkProbeResult::NetworkChanged;
    }
    if !cleaned {
        return NetworkProbeResult::Failed;
    }
    match outcome {
        Ok(_) => NetworkProbeResult::Passed {
            milliseconds: elapsed_ms(started),
        },
        Err(DirectDnsError::Timeout | DirectDnsError::Busy) => NetworkProbeResult::TimedOut,
        Err(DirectDnsError::Cancelled) => NetworkProbeResult::Cancelled,
        Err(DirectDnsError::NetworkChanged) => NetworkProbeResult::NetworkChanged,
        Err(_) => NetworkProbeResult::Failed,
    }
}

/// Caller must hold its connection-lifecycle exclusion while disconnected.
/// This completes only the authenticated QUIC handshake, with no HTTP/3 or
/// CONNECT-IP stream ever constructed, and closes the socket before its lease.
pub async fn probe_h3_handshake(
    endpoint: SocketAddr,
    sni: &str,
    identity: &MasqueTlsIdentity,
    protector: Arc<dyn SocketProtector>,
    cancellation: CancellationToken,
) -> NetworkProbeResult {
    crate::h3::diagnostic::handshake(endpoint, sni, identity, protector, cancellation)
        .await
        .public_result()
}

/// The same family preference and forced-family policy as normal connection
/// selection. No resolver lookup or endpoint discovery is performed.
pub fn h3_probe_endpoints(profile: &Profile) -> Vec<SocketAddr> {
    if profile.endpoint.subscription_id.is_some() {
        return profile
            .subscription_endpoint
            .as_ref()
            .and_then(|sub_ep| sub_ep.socket_addr(profile.endpoint.port))
            .into_iter()
            .collect();
    }
    // Preset discovery requires authenticated tier and physical DNS. This
    // synchronous diagnostic must never probe the dormant manual IPv6 field.
    if profile.endpoint.preset_region.is_some() {
        return Vec::new();
    }
    let (v4, v6) = if profile.endpoint.selection == usque_core::EndpointSelection::Automatic {
        // Diagnostics stay bounded to one common endpoint per family. This
        // pair is eligible for both Free and Plus authenticated identities.
        usque_core::AutomaticEndpointPolicy::for_profile(
            profile,
            usque_core::EndpointPool::WarpPlus,
        )
        .representative_pair()
    } else {
        (
            profile.endpoint.ipv4_socket(),
            profile.endpoint.ipv6_socket(),
        )
    };
    match profile.ip_policy {
        IpPolicy::Ipv4Only => vec![v4],
        IpPolicy::Ipv6Only => vec![v6],
        IpPolicy::PreferIpv4 => vec![v4, v6],
        IpPolicy::Auto | IpPolicy::PreferIpv6 => vec![v6, v4],
    }
}

/// Preset-aware discovery with the same authenticated tier and physical DNS
/// authorization as a connection. No tunnel is constructed.
pub struct ResolvedH3ProbeEndpoints {
    endpoints: Vec<SocketAddr>,
    generation: Option<u64>,
}

impl ResolvedH3ProbeEndpoints {
    /// Uses the discovery generation through socket admission and every packet.
    pub async fn probe(
        &self,
        sni: &str,
        identity: &MasqueTlsIdentity,
        protector: Arc<dyn SocketProtector>,
        cancellation: CancellationToken,
    ) -> NetworkProbeResult {
        probe_h3_candidates_for_generation(
            &self.endpoints,
            sni,
            identity,
            protector,
            cancellation,
            self.generation,
        )
        .await
    }
}

/// Discovery and its exact network generation are an inseparable probe input.
pub async fn h3_probe_endpoints_resolved(
    profile: &Profile,
    identity: &MasqueTlsIdentity,
    protector: Arc<dyn SocketProtector>,
    cancellation: CancellationToken,
) -> Result<ResolvedH3ProbeEndpoints, NetworkProbeResult> {
    let generation = protector.network_generation();
    if profile.endpoint.subscription_id.is_some() {
        let Some(sub_ep) = profile.subscription_endpoint.as_ref() else {
            return Ok(ResolvedH3ProbeEndpoints {
                endpoints: Vec::new(),
                generation,
            });
        };
        let port = sub_ep.effective_port(profile.endpoint.port);
        match &sub_ep.host {
            usque_core::endpoint_subscriptions::SubscriptionEndpointHost::Ipv4(ip) => {
                return Ok(ResolvedH3ProbeEndpoints {
                    endpoints: vec![SocketAddr::new(std::net::IpAddr::V4(*ip), port)],
                    generation,
                });
            }
            usque_core::endpoint_subscriptions::SubscriptionEndpointHost::Ipv6(ip) => {
                return Ok(ResolvedH3ProbeEndpoints {
                    endpoints: vec![SocketAddr::new(std::net::IpAddr::V6(*ip), port)],
                    generation,
                });
            }
            usque_core::endpoint_subscriptions::SubscriptionEndpointHost::Domain(host) => {
                let expected = generation.unwrap_or_default();
                let resolved = tokio::select! {
                    biased;
                    _ = cancellation.cancelled() => return Err(NetworkProbeResult::Cancelled),
                    res = tokio::time::timeout(
                        PROBE_IO_TIMEOUT,
                        protector.resolve_endpoint_hostname(host, port, profile.ip_policy, expected),
                    ) => match res {
                        Ok(Ok(addrs)) => addrs,
                        Ok(Err(_)) => {
                            return Err(if protector.network_generation() != generation {
                                NetworkProbeResult::NetworkChanged
                            } else {
                                NetworkProbeResult::Failed
                            });
                        }
                        Err(_) => return Err(NetworkProbeResult::TimedOut),
                    }
                };
                if protector.network_generation() != generation {
                    return Err(NetworkProbeResult::NetworkChanged);
                }
                return Ok(ResolvedH3ProbeEndpoints {
                    endpoints: crate::endpoint_subscription::canonical_subscription_candidates(
                        resolved,
                        port,
                        profile.ip_policy,
                    ),
                    generation,
                });
            }
        }
    }
    if profile.endpoint.preset_region.is_none() {
        return Ok(ResolvedH3ProbeEndpoints {
            endpoints: h3_probe_endpoints(profile),
            generation,
        });
    }
    let resolution = tokio::select! {
        biased;
        _ = cancellation.cancelled() => return Err(NetworkProbeResult::Cancelled),
        result = crate::endpoint_preset::resolve(profile, identity, &protector) => result,
    };
    if protector.network_generation() != generation {
        return Err(NetworkProbeResult::NetworkChanged);
    }
    let mut endpoints = match resolution {
        Ok(resolved) => resolved.candidates.into_iter().take(1).collect::<Vec<_>>(),
        Err(error) if crate::endpoint_preset::may_fallback(profile, &error) => Vec::new(),
        Err(TransportError::UnderlyingNetworkChanged) => {
            return Err(NetworkProbeResult::NetworkChanged);
        }
        Err(_) => return Err(NetworkProbeResult::Failed),
    };
    if profile.ip_policy == IpPolicy::PreferIpv6 {
        endpoints.push(profile.endpoint.ipv4_socket());
    }
    Ok(ResolvedH3ProbeEndpoints {
        endpoints,
        generation,
    })
}

/// Serial, at-most-two-family handshake checks within one 3.8 second deadline.
/// The first socket/lease is destroyed before an alternate can be opened.
pub async fn probe_h3_handshake_candidates(
    endpoints: &[SocketAddr],
    sni: &str,
    identity: &MasqueTlsIdentity,
    protector: Arc<dyn SocketProtector>,
    cancellation: CancellationToken,
) -> NetworkProbeResult {
    let generation = protector.network_generation();
    probe_h3_candidates_for_generation(
        endpoints,
        sni,
        identity,
        protector,
        cancellation,
        generation,
    )
    .await
}

async fn probe_h3_candidates_for_generation(
    endpoints: &[SocketAddr],
    sni: &str,
    identity: &MasqueTlsIdentity,
    protector: Arc<dyn SocketProtector>,
    cancellation: CancellationToken,
    generation: Option<u64>,
) -> NetworkProbeResult {
    if cancellation.is_cancelled() {
        return NetworkProbeResult::Cancelled;
    }
    if protector.network_generation() != generation {
        return NetworkProbeResult::NetworkChanged;
    }
    // The QUIC handshake also snapshots its protector. Fence admission so a
    // change between that snapshot and socket setup cannot rebind old AAAA.
    let protector = match generation {
        Some(generation) => crate::endpoint_preset::for_generation(protector, generation),
        None => protector,
    };
    let candidates = endpoints
        .iter()
        .copied()
        // Skip an unavailable family before bounding the probe count, so a
        // single-stack network still probes its own family's candidates.
        .filter(|endpoint| protector.endpoint_family_available(*endpoint) != Some(false))
        .take(2)
        .collect::<Vec<_>>();
    if candidates.is_empty() {
        return if cancellation.is_cancelled() {
            NetworkProbeResult::Cancelled
        } else {
            NetworkProbeResult::Failed
        };
    }
    run_h3_candidates(&candidates, cancellation.clone(), |endpoint| {
        let protector = Arc::clone(&protector);
        let cancellation = cancellation.clone();
        async move {
            if protector.network_generation() != generation {
                return NetworkProbeResult::NetworkChanged.into();
            }
            let result = crate::h3::diagnostic::handshake(
                endpoint,
                sni,
                identity,
                protector.clone(),
                cancellation,
            )
            .await;
            if protector.network_generation() != generation {
                NetworkProbeResult::NetworkChanged.into()
            } else {
                result
            }
        }
    })
    .await
}

async fn run_h3_candidates<F, Fut>(
    endpoints: &[SocketAddr],
    cancellation: CancellationToken,
    mut probe: F,
) -> NetworkProbeResult
where
    F: FnMut(SocketAddr) -> Fut,
    Fut: Future<Output = H3ProbeOutcome>,
{
    let started = Instant::now();
    let deadline = started + PROBE_IO_TIMEOUT;
    let mut timed_out = false;
    for (index, endpoint) in endpoints.iter().copied().enumerate() {
        if cancellation.is_cancelled() {
            return NetworkProbeResult::Cancelled;
        }
        let now = Instant::now();
        let remaining = deadline.saturating_duration_since(now);
        if remaining.is_zero() {
            return NetworkProbeResult::TimedOut;
        }
        let attempt_deadline = now + remaining / (endpoints.len() - index) as u32;
        let outcome = tokio::select! {
            biased;
            _ = cancellation.cancelled() => NetworkProbeResult::Cancelled.into(),
            result = timeout_at(attempt_deadline, probe(endpoint)) => result.unwrap_or(NetworkProbeResult::TimedOut.into()),
        };
        let outcome = match outcome {
            H3ProbeOutcome::Complete(outcome) => outcome,
            H3ProbeOutcome::Terminal(_) => return NetworkProbeResult::Failed,
        };
        match outcome {
            NetworkProbeResult::Passed { .. } => {
                return NetworkProbeResult::Passed {
                    milliseconds: elapsed_ms(started),
                };
            }
            NetworkProbeResult::Failed => {}
            NetworkProbeResult::TimedOut => timed_out = true,
            other => return other,
        }
    }
    if timed_out {
        NetworkProbeResult::TimedOut
    } else {
        NetworkProbeResult::Failed
    }
}

/// Result of probing one subscription MASQUE endpoint using a real TLS/QUIC
/// handshake and P-256 SPKI verification (no CONNECT-IP session opened).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubscriptionProbeOutcome {
    Passed {
        milliseconds: u64,
        transport: usque_core::Transport,
    },
    PinMismatch,
    TimedOut,
    Cancelled,
    NetworkChanged,
    Failed,
}

impl From<NetworkProbeResult> for SubscriptionProbeOutcome {
    fn from(value: NetworkProbeResult) -> Self {
        match value {
            NetworkProbeResult::Passed { milliseconds } => Self::Passed {
                milliseconds,
                transport: usque_core::Transport::Http3,
            },
            NetworkProbeResult::TimedOut => Self::TimedOut,
            NetworkProbeResult::Cancelled => Self::Cancelled,
            NetworkProbeResult::NetworkChanged => Self::NetworkChanged,
            NetworkProbeResult::Failed | NetworkProbeResult::NotApplicable => Self::Failed,
        }
    }
}

/// Probes one concrete MASQUE socket target using real H3 (QUIC+Pin) and/or H2
/// (TCP+TLS+ALPN+Pin) handshakes without creating any CONNECT-IP stream.
#[allow(clippy::too_many_arguments)]
pub async fn probe_resolved_subscription_target(
    target: SocketAddr,
    sni: &str,
    transport: usque_core::TransportPolicy,
    data_plane: usque_core::DataPlaneMode,
    identity: &MasqueTlsIdentity,
    protector: Arc<dyn SocketProtector>,
    cancellation: CancellationToken,
    io_timeout: Duration,
) -> SubscriptionProbeOutcome {
    let generation = protector.network_generation();
    probe_resolved_subscription_target_at(
        target,
        sni,
        transport,
        data_plane,
        identity,
        protector,
        cancellation,
        io_timeout,
        generation,
    )
    .await
}

/// Probes `target` only on the network generation that produced it. A
/// hostname answer must never be dialed after the physical network changed.
#[allow(clippy::too_many_arguments)]
async fn probe_resolved_subscription_target_at(
    target: SocketAddr,
    sni: &str,
    transport: usque_core::TransportPolicy,
    data_plane: usque_core::DataPlaneMode,
    identity: &MasqueTlsIdentity,
    protector: Arc<dyn SocketProtector>,
    cancellation: CancellationToken,
    io_timeout: Duration,
    generation: Option<u64>,
) -> SubscriptionProbeOutcome {
    if cancellation.is_cancelled() {
        return SubscriptionProbeOutcome::Cancelled;
    }
    if protector.network_generation() != generation {
        return SubscriptionProbeOutcome::NetworkChanged;
    }
    if protector.endpoint_family_available(target) == Some(false) {
        return SubscriptionProbeOutcome::Failed;
    }
    let fenced = match generation {
        Some(generation) => crate::endpoint_preset::for_generation(protector, generation),
        None => protector,
    };

    let run_h3 = |budget: Duration| {
        let fenced = Arc::clone(&fenced);
        let cancellation = cancellation.clone();
        async move {
            if fenced.network_generation() != generation {
                return SubscriptionProbeOutcome::NetworkChanged;
            }
            let outcome = tokio::select! {
                biased;
                _ = cancellation.cancelled() => NetworkProbeResult::Cancelled.into(),
                res = tokio::time::timeout(
                    budget,
                    crate::h3::diagnostic::handshake(
                        target,
                        sni,
                        identity,
                        fenced.clone(),
                        cancellation.clone(),
                    ),
                ) => res.unwrap_or_else(|_| NetworkProbeResult::TimedOut.into()),
            };
            if fenced.network_generation() != generation {
                return SubscriptionProbeOutcome::NetworkChanged;
            }
            match outcome {
                H3ProbeOutcome::Complete(res) => SubscriptionProbeOutcome::from(res),
                H3ProbeOutcome::Terminal(H3ProbeTerminal::PinMismatch) => {
                    SubscriptionProbeOutcome::PinMismatch
                }
                H3ProbeOutcome::Terminal(
                    H3ProbeTerminal::Identity | H3ProbeTerminal::Protection,
                ) => SubscriptionProbeOutcome::Failed,
            }
        }
    };

    let run_h2 = |budget: Duration| {
        let fenced = Arc::clone(&fenced);
        let cancellation = cancellation.clone();
        async move {
            crate::h2::diagnostic::handshake(target, sni, identity, fenced, cancellation, budget)
                .await
        }
    };

    if data_plane == usque_core::DataPlaneMode::L4Proxy
        || transport == usque_core::TransportPolicy::Http3
    {
        return run_h3(io_timeout).await;
    }
    if transport == usque_core::TransportPolicy::Http2 {
        return run_h2(io_timeout).await;
    }

    // Auto: prefer H3 within half budget, then fall back to H2 with remaining budget.
    let started = Instant::now();
    let half = io_timeout / 2;
    let h3_outcome = run_h3(half.max(Duration::from_millis(250))).await;
    match h3_outcome {
        SubscriptionProbeOutcome::Passed { .. }
        | SubscriptionProbeOutcome::PinMismatch
        | SubscriptionProbeOutcome::Cancelled
        | SubscriptionProbeOutcome::NetworkChanged => h3_outcome,
        SubscriptionProbeOutcome::Failed | SubscriptionProbeOutcome::TimedOut => {
            let remaining = io_timeout.saturating_sub(started.elapsed());
            if remaining.is_zero() {
                h3_outcome
            } else {
                let h2_outcome = run_h2(remaining).await;
                if matches!(
                    h2_outcome,
                    SubscriptionProbeOutcome::Passed { .. }
                        | SubscriptionProbeOutcome::PinMismatch
                        | SubscriptionProbeOutcome::Cancelled
                        | SubscriptionProbeOutcome::NetworkChanged
                ) {
                    h2_outcome
                } else if h3_outcome == SubscriptionProbeOutcome::TimedOut
                    || h2_outcome == SubscriptionProbeOutcome::TimedOut
                {
                    SubscriptionProbeOutcome::TimedOut
                } else {
                    h2_outcome
                }
            }
        }
    }
}

/// Resolves (if domain) and probes a single subscription endpoint on the physical network.
#[allow(clippy::too_many_arguments)]
pub async fn probe_subscription_endpoint(
    host: &usque_core::endpoint_subscriptions::SubscriptionEndpointHost,
    port: u16,
    sni: &str,
    transport: usque_core::TransportPolicy,
    data_plane: usque_core::DataPlaneMode,
    ip_policy: IpPolicy,
    identity: &MasqueTlsIdentity,
    protector: Arc<dyn SocketProtector>,
    cancellation: CancellationToken,
    io_timeout: Duration,
) -> SubscriptionProbeOutcome {
    if cancellation.is_cancelled() {
        return SubscriptionProbeOutcome::Cancelled;
    }
    let generation = protector.network_generation();
    let target = match host {
        usque_core::endpoint_subscriptions::SubscriptionEndpointHost::Ipv4(ip) => {
            SocketAddr::new(std::net::IpAddr::V4(*ip), port)
        }
        usque_core::endpoint_subscriptions::SubscriptionEndpointHost::Ipv6(ip) => {
            SocketAddr::new(std::net::IpAddr::V6(*ip), port)
        }
        usque_core::endpoint_subscriptions::SubscriptionEndpointHost::Domain(domain) => {
            let resolved = tokio::select! {
                biased;
                _ = cancellation.cancelled() => return SubscriptionProbeOutcome::Cancelled,
                res = tokio::time::timeout(
                    io_timeout,
                    protector.resolve_endpoint_hostname(
                        domain,
                        port,
                        ip_policy,
                        generation.unwrap_or_default(),
                    ),
                ) => match res {
                    Ok(Ok(addrs)) => addrs,
                    Ok(Err(_)) => {
                        return if protector.network_generation() != generation {
                            SubscriptionProbeOutcome::NetworkChanged
                        } else {
                            SubscriptionProbeOutcome::Failed
                        };
                    }
                    Err(_) => return SubscriptionProbeOutcome::TimedOut,
                }
            };
            let candidates = crate::endpoint_subscription::canonical_subscription_candidates(
                resolved, port, ip_policy,
            );
            let Some(chosen) = candidates
                .into_iter()
                .find(|addr| protector.endpoint_family_available(*addr) != Some(false))
            else {
                return SubscriptionProbeOutcome::Failed;
            };
            chosen
        }
    };
    probe_resolved_subscription_target_at(
        target,
        sni,
        transport,
        data_plane,
        identity,
        protector,
        cancellation,
        io_timeout,
        generation,
    )
    .await
}

pub(crate) fn elapsed_ms(started: Instant) -> u64 {
    started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};

    struct DiscoveryProtector {
        generation: AtomicU64,
        change_on_family_check: AtomicBool,
        generation_reads_until_change: AtomicUsize,
        protections: AtomicUsize,
    }

    #[async_trait::async_trait]
    impl SocketProtector for DiscoveryProtector {
        fn protect(&self, _: crate::SocketHandle) -> Result<(), String> {
            panic!("a stale probe must never open an admitted socket")
        }
        fn network_generation(&self) -> Option<u64> {
            if self.generation_reads_until_change.fetch_update(
                Ordering::SeqCst,
                Ordering::SeqCst,
                |remaining| remaining.checked_sub(1),
            ) == Ok(1)
            {
                self.generation.store(2, Ordering::SeqCst);
            }
            Some(self.generation.load(Ordering::SeqCst))
        }
        fn endpoint_family_available(&self, _: SocketAddr) -> Option<bool> {
            if self.change_on_family_check.swap(false, Ordering::SeqCst) {
                self.generation.store(2, Ordering::SeqCst);
            }
            Some(true)
        }
        async fn resolve_endpoint_preset(
            &self,
            _: &str,
            port: u16,
            expected: u64,
        ) -> Result<Vec<SocketAddr>, String> {
            assert_eq!(self.network_generation(), Some(expected));
            Ok(vec![SocketAddr::new("2001:db8::8".parse().unwrap(), port)])
        }
        async fn protect_masque_endpoint_generation(
            &self,
            _: crate::SocketHandle,
            _: SocketAddr,
            _: crate::DirectProtocol,
            _: u64,
        ) -> Result<crate::DirectEgressLease, String> {
            self.protections.fetch_add(1, Ordering::SeqCst);
            Err("a stale candidate reached socket admission".into())
        }
    }

    #[tokio::test]
    async fn resolved_probe_keeps_its_generation_until_handshake_admission() {
        let key = usque_core::MasqueKeyPair::generate();
        let mut identity = MasqueTlsIdentity::new(
            key.private_sec1_der().unwrap(),
            &key.public_spki_der().unwrap(),
            "172.16.0.2".parse().unwrap(),
            "2001:db8::2".parse().unwrap(),
        )
        .unwrap();
        identity.provider = Some(usque_core::IdentityProvider::Consumer);
        identity.entitlement = Some(usque_core::ConsumerEntitlement::Free);
        for policy in [IpPolicy::Ipv6Only, IpPolicy::PreferIpv6] {
            for change_stage in 0..3 {
                let protector = Arc::new(DiscoveryProtector {
                    generation: AtomicU64::new(1),
                    change_on_family_check: AtomicBool::new(false),
                    generation_reads_until_change: AtomicUsize::new(0),
                    protections: AtomicUsize::new(0),
                });
                let platform: Arc<dyn SocketProtector> = protector.clone();
                let mut profile = Profile {
                    ip_policy: policy,
                    ..Profile::default()
                };
                profile.endpoint.selection = usque_core::EndpointSelection::Custom;
                profile.endpoint.preset_region = Some(usque_core::EndpointPresetRegion::Us);
                let endpoints = h3_probe_endpoints_resolved(
                    &profile,
                    &identity,
                    platform.clone(),
                    CancellationToken::new(),
                )
                .await
                .unwrap();
                match change_stage {
                    0 => protector.generation.store(2, Ordering::SeqCst),
                    1 => protector
                        .change_on_family_check
                        .store(true, Ordering::SeqCst),
                    2 => {
                        // Change at the nested handshake snapshot, after the
                        // runner's admission check. Its new generation must
                        // never authorize the previous discovery's address.
                        protector
                            .generation_reads_until_change
                            .store(3, Ordering::SeqCst);
                    }
                    _ => unreachable!(),
                }
                assert_eq!(
                    endpoints
                        .probe(
                            "example.invalid",
                            &identity,
                            platform,
                            CancellationToken::new()
                        )
                        .await,
                    NetworkProbeResult::NetworkChanged,
                );
                assert_eq!(protector.protections.load(Ordering::SeqCst), 0);
            }
        }
    }

    #[test]
    fn diagnostic_family_order_matches_normal_policy_and_never_expands_forced_families() {
        for (policy, families) in [
            (IpPolicy::Auto, vec![false, true]),
            (IpPolicy::PreferIpv6, vec![false, true]),
            (IpPolicy::PreferIpv4, vec![true, false]),
            (IpPolicy::Ipv4Only, vec![true]),
            (IpPolicy::Ipv6Only, vec![false]),
        ] {
            let profile = Profile {
                ip_policy: policy,
                ..Profile::default()
            };
            assert_eq!(
                h3_probe_endpoints(&profile)
                    .iter()
                    .map(SocketAddr::is_ipv4)
                    .collect::<Vec<_>>(),
                families
            );
        }
    }

    struct ActiveProbe(Arc<AtomicUsize>);
    impl Drop for ActiveProbe {
        fn drop(&mut self) {
            self.0.fetch_sub(1, Ordering::AcqRel);
        }
    }

    #[tokio::test(start_paused = true)]
    async fn alternate_probe_gets_budget_and_first_probe_is_closed_before_fallback() {
        let endpoints = h3_probe_endpoints(&Profile::default());
        let active = Arc::new(AtomicUsize::new(0));
        let result = run_h3_candidates(&endpoints, CancellationToken::new(), |endpoint| {
            let active = active.clone();
            async move {
                assert_eq!(active.fetch_add(1, Ordering::AcqRel), 0);
                let _guard = ActiveProbe(active);
                if endpoint.is_ipv6() {
                    std::future::pending().await
                } else {
                    NetworkProbeResult::Passed { milliseconds: 0 }.into()
                }
            }
        })
        .await;
        assert_eq!(
            result,
            NetworkProbeResult::Passed {
                milliseconds: 1_900
            }
        );
        assert_eq!(active.load(Ordering::Acquire), 0);
    }

    #[tokio::test]
    async fn terminal_ipv6_handshake_never_becomes_a_successful_ipv4_diagnosis() {
        let endpoints = [
            "[2001:db8::8]:443".parse().unwrap(),
            "192.0.2.8:443".parse().unwrap(),
        ];
        for terminal in [
            H3ProbeTerminal::PinMismatch,
            H3ProbeTerminal::Identity,
            H3ProbeTerminal::Protection,
        ] {
            let calls = Arc::new(AtomicUsize::new(0));
            let active = Arc::new(AtomicUsize::new(0));
            let result = run_h3_candidates(&endpoints, CancellationToken::new(), |endpoint| {
                let calls = calls.clone();
                let active = active.clone();
                async move {
                    calls.fetch_add(1, Ordering::SeqCst);
                    assert_eq!(active.fetch_add(1, Ordering::SeqCst), 0);
                    let _lease = ActiveProbe(active);
                    if endpoint.is_ipv6() {
                        H3ProbeOutcome::Terminal(terminal)
                    } else {
                        NetworkProbeResult::Passed { milliseconds: 0 }.into()
                    }
                }
            })
            .await;
            assert_eq!(result, NetworkProbeResult::Failed, "{terminal:?}");
            assert_eq!(
                calls.load(Ordering::SeqCst),
                1,
                "{terminal:?} must stop before IPv4"
            );
            assert_eq!(
                active.load(Ordering::SeqCst),
                0,
                "failed candidate releases its lease"
            );
        }
    }

    #[tokio::test(start_paused = true)]
    async fn candidate_deadline_and_cancellation_are_global_not_per_family() {
        let endpoints = h3_probe_endpoints(&Profile::default());
        let calls = AtomicUsize::new(0);
        let started = Instant::now();
        assert_eq!(
            run_h3_candidates(&endpoints, CancellationToken::new(), |_| {
                calls.fetch_add(1, Ordering::Relaxed);
                std::future::pending()
            })
            .await,
            NetworkProbeResult::TimedOut
        );
        assert_eq!(calls.load(Ordering::Relaxed), 2);
        assert_eq!(started.elapsed(), PROBE_IO_TIMEOUT);
        let stop = CancellationToken::new();
        let cancelled_calls = AtomicUsize::new(0);
        assert_eq!(
            run_h3_candidates(&endpoints, stop.clone(), |_| {
                cancelled_calls.fetch_add(1, Ordering::Relaxed);
                stop.cancel();
                std::future::pending()
            })
            .await,
            NetworkProbeResult::Cancelled
        );
        assert_eq!(cancelled_calls.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn subscription_probe_endpoints_are_single_stack_and_fail_closed() {
        let mut profile = Profile {
            ip_policy: IpPolicy::PreferIpv6,
            ..Profile::default()
        };
        profile.endpoint.selection = usque_core::EndpointSelection::Custom;
        profile.endpoint.subscription_id = Some(uuid::Uuid::new_v4());
        profile.endpoint.port = 8443;

        // Missing resolved subscription_endpoint fails closed (no fallback to profile.endpoint).
        assert!(h3_probe_endpoints(&profile).is_empty());
        let resolved_missing = h3_probe_endpoints_resolved(
            &profile,
            &identity_fixture(),
            Arc::new(DiscoveryProtector {
                generation: AtomicU64::new(1),
                change_on_family_check: AtomicBool::new(false),
                generation_reads_until_change: AtomicUsize::new(0),
                protections: AtomicUsize::new(0),
            }),
            CancellationToken::new(),
        )
        .await
        .unwrap();
        assert!(resolved_missing.endpoints.is_empty());

        // Single-stack numeric IPv4 never adds dormant manual IPv6.
        profile.subscription_endpoint =
            Some(usque_core::endpoint_subscriptions::SubscriptionEndpoint {
                id: "v4:162.159.193.10:2408".into(),
                host: usque_core::endpoint_subscriptions::SubscriptionEndpointHost::Ipv4(
                    "162.159.193.10".parse().unwrap(),
                ),
                port: Some(2408),
                note: String::new(),
                pinned: false,
            });
        assert_eq!(
            h3_probe_endpoints(&profile),
            vec!["162.159.193.10:2408".parse::<SocketAddr>().unwrap()]
        );
    }

    fn identity_fixture() -> MasqueTlsIdentity {
        let key = usque_core::MasqueKeyPair::generate();
        MasqueTlsIdentity::new(
            key.private_sec1_der().unwrap(),
            &key.public_spki_der().unwrap(),
            "172.16.0.2".parse().unwrap(),
            "2001:db8::2".parse().unwrap(),
        )
        .unwrap()
    }
}
