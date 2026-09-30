//! Finite automatic endpoint cycles and bounded, cancellation-aware races.
use std::collections::HashSet;
use std::future::Future;
use std::net::{Ipv6Addr, SocketAddr};
use std::time::Duration;

use async_trait::async_trait;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
use usque_core::{AutomaticEndpointPolicy, IpPolicy};

use crate::h2::TransportError;
use crate::recovery_policy::RecoveryDecision;

pub(crate) const FAMILY_DELAY: Duration = Duration::from_millis(250);

/// Whether an automatic ingress would overlap a configured DNS destination.
pub fn excludes_dns_server(profile: &usque_core::Profile, endpoint: SocketAddr) -> bool {
    profile.dns_servers.contains(&endpoint.ip())
        || profile.proxy.dns_mode == usque_core::ProxyDnsMode::LocalConfigured
            && profile.proxy.dns_servers.contains(&endpoint.ip())
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct RaceTarget {
    pub(crate) endpoint: SocketAddr,
    pub(crate) delay: Duration,
}

#[async_trait]
pub(crate) trait RaceConnection: Send + 'static {
    async fn shutdown(self);
    fn is_alive(&self) -> bool;
}

struct CancelCandidates(Vec<Option<CancellationToken>>);

fn recovery_priority(error: &TransportError) -> u8 {
    match RecoveryDecision::for_failure(&error.failure(None, None)) {
        RecoveryDecision::Stop => 2,
        RecoveryDecision::RefreshPin => 1,
        RecoveryDecision::Retry => 0,
    }
}

impl Drop for CancelCandidates {
    fn drop(&mut self) {
        for token in self.0.iter().flatten() {
            token.cancel();
        }
    }
}

/// Dropped callers cancel the owning task, which keeps polling candidate
/// cleanup rather than aborting handshake wrappers before child drivers join.
struct RaceOwner<T: RaceConnection> {
    cancellation: CancellationToken,
    task: Option<tokio::task::JoinHandle<Result<T, TransportError>>>,
}

impl<T: RaceConnection> Drop for RaceOwner<T> {
    fn drop(&mut self) {
        if let Some(task) = self.task.take() {
            self.cancellation.cancel();
            if let Ok(runtime) = tokio::runtime::Handle::try_current() {
                runtime.spawn(async move {
                    if let Ok(Ok(connection)) = task.await {
                        connection.shutdown().await;
                    }
                });
            } else {
                task.abort();
            }
        }
    }
}

/// Candidate tasks cooperate with cancellation so child protocol drivers and
/// exact socket leases have stopped before another batch begins.
pub(crate) async fn race_batch<T, F, Fut>(
    targets: Vec<RaceTarget>,
    deadline: Option<Duration>,
    connect: F,
) -> Result<T, TransportError>
where
    T: RaceConnection,
    F: Fn(SocketAddr, CancellationToken) -> Fut + Clone + Send + 'static,
    Fut: Future<Output = Result<T, TransportError>> + Send + 'static,
{
    let cancellation = CancellationToken::new();
    let mut owner = RaceOwner {
        task: Some(tokio::spawn(race_batch_inner(
            targets,
            deadline,
            connect,
            cancellation.clone(),
        ))),
        cancellation,
    };
    let result = owner
        .task
        .as_mut()
        .expect("race owner task is present")
        .await
        .map_err(|error| TransportError::Driver(error.to_string()))?;
    owner.task.take();
    result
}

async fn race_batch_inner<T, F, Fut>(
    targets: Vec<RaceTarget>,
    deadline: Option<Duration>,
    connect: F,
    cancellation: CancellationToken,
) -> Result<T, TransportError>
where
    T: RaceConnection,
    F: Fn(SocketAddr, CancellationToken) -> Fut + Clone + Send + 'static,
    Fut: Future<Output = Result<T, TransportError>> + Send + 'static,
{
    let mut tasks = JoinSet::new();
    let mut cancellations = CancelCandidates(Vec::with_capacity(targets.len()));
    let alternate_start = CancellationToken::new();
    let mut preferred_remaining = targets
        .iter()
        .filter(|target| target.delay.is_zero())
        .count();
    let preferred_indices = targets
        .iter()
        .map(|target| target.delay.is_zero())
        .collect::<Vec<_>>();
    if preferred_remaining == 0 {
        alternate_start.cancel();
    }
    for (index, target) in targets.into_iter().enumerate() {
        let cancellation = cancellation.child_token();
        cancellations.0.push(Some(cancellation.clone()));
        let connect = connect.clone();
        let alternate_start = alternate_start.clone();
        tasks.spawn(async move {
            if !target.delay.is_zero() {
                tokio::select! {
                    biased;
                    _ = cancellation.cancelled() => return (index, Err(TransportError::TunnelClosed)),
                    _ = alternate_start.cancelled() => {},
                    _ = tokio::time::sleep(target.delay) => {}
                }
            }
            (index, connect(target.endpoint, cancellation).await)
        });
    }
    let expires = deadline.map(|duration| tokio::time::Instant::now() + duration);
    let mut failure = None;
    let mut winner = None;
    loop {
        let joined = tokio::select! {
            biased;
            _ = cancellation.cancelled() => { failure = Some(TransportError::TunnelClosed); break; },
            _ = async {
                match expires {
                    Some(at) => tokio::time::sleep_until(at).await,
                    None => std::future::pending().await,
                }
            } => break,
            joined = tasks.join_next() => joined,
        };
        match joined {
            Some(Ok((index, Ok(connection)))) => {
                cancellations.0[index].take();
                winner = Some(connection);
                break;
            }
            Some(Ok((index, Err(error)))) => {
                if preferred_indices[index] {
                    preferred_remaining = preferred_remaining.saturating_sub(1);
                    if preferred_remaining == 0 {
                        alternate_start.cancel();
                    }
                }
                let decision = RecoveryDecision::for_failure(&error.failure(None, None));
                if decision != RecoveryDecision::Retry {
                    failure = Some(error);
                    break;
                }
                failure = Some(error);
            }
            Some(Err(error)) => {
                failure = Some(TransportError::Driver(error.to_string()));
                break;
            }
            None => break,
        }
    }
    for token in cancellations.0.iter().flatten() {
        token.cancel();
    }
    let mut terminal_failure = None;
    while let Some(joined) = tasks.join_next().await {
        match joined {
            Ok((_, Ok(connection))) => connection.shutdown().await,
            Ok((_, Err(error)))
                if !matches!(error, TransportError::TunnelClosed)
                    && RecoveryDecision::for_failure(&error.failure(None, None))
                        != RecoveryDecision::Retry
                    && terminal_failure.as_ref().is_none_or(|previous| {
                        recovery_priority(previous) < recovery_priority(&error)
                    }) =>
            {
                terminal_failure = Some(error);
            }
            _ => {}
        }
    }
    if let Some(connection) = winner {
        if let Some(error) = terminal_failure {
            connection.shutdown().await;
            return Err(error);
        }
        if connection.is_alive() {
            return Ok(connection);
        }
        connection.shutdown().await;
        return Err(TransportError::TunnelClosed);
    }
    if let Some(error) = terminal_failure
        && failure
            .as_ref()
            .is_none_or(|previous| recovery_priority(previous) < recovery_priority(&error))
    {
        return Err(error);
    }
    Err(failure.unwrap_or_else(|| {
        TransportError::AllEndpointsFailed("automatic endpoint batch timed out".to_owned())
    }))
}

pub(crate) fn h3_targets(policy: AutomaticEndpointPolicy, family: IpPolicy) -> Vec<RaceTarget> {
    let prefer_v4 = matches!(family, IpPolicy::PreferIpv4 | IpPolicy::Ipv4Only);
    policy
        .h3_candidates()
        .into_iter()
        .map(|endpoint| RaceTarget {
            endpoint,
            delay: if endpoint.is_ipv4() == prefer_v4 {
                Duration::ZERO
            } else {
                FAMILY_DELAY
            },
        })
        .collect()
}

pub(crate) fn h2_candidates(
    policy: AutomaticEndpointPolicy,
) -> Result<(Vec<SocketAddr>, Vec<SocketAddr>), TransportError> {
    let mut ipv4 = policy.h2_ipv4_candidates();
    let mut ipv6 = Vec::new();
    for prefix in policy.ipv6_prefixes() {
        let mut addresses = HashSet::with_capacity(256);
        addresses.insert(Ipv6Addr::from(u128::from(prefix) | 1));
        addresses.insert(Ipv6Addr::from(u128::from(prefix) | 2));
        for _ in 0..2048 {
            if addresses.len() == usque_core::endpoints::AUTOMATIC_H2_IPV6_PER_PREFIX {
                break;
            }
            let mut random = [0_u8; 16];
            boring::rand::rand_bytes(&mut random)?;
            let suffix = u128::from_be_bytes(random) & ((1_u128 << 80) - 1);
            if suffix != 0 {
                addresses.insert(Ipv6Addr::from(u128::from(prefix) | suffix));
            }
        }
        if addresses.len() != usque_core::endpoints::AUTOMATIC_H2_IPV6_PER_PREFIX {
            return Err(TransportError::AllEndpointsFailed(
                "automatic IPv6 candidate generation exhausted".to_owned(),
            ));
        }
        ipv6.extend(
            addresses
                .into_iter()
                .map(|address| SocketAddr::new(address.into(), policy.port)),
        );
    }
    shuffle(&mut ipv4)?;
    shuffle(&mut ipv6)?;
    let seeds = AutomaticEndpointPolicy {
        udp: true,
        ..policy
    }
    .h3_candidates();
    for candidates in [&mut ipv4, &mut ipv6] {
        let known = seeds
            .iter()
            .copied()
            .filter(|endpoint| candidates.contains(endpoint))
            .collect::<Vec<_>>();
        candidates.retain(|endpoint| !known.contains(endpoint));
        candidates.extend(known.into_iter().rev());
    }
    Ok((ipv4, ipv6))
}

fn shuffle(addresses: &mut [SocketAddr]) -> Result<(), TransportError> {
    for index in (1..addresses.len()).rev() {
        let mut random = [0_u8; 8];
        boring::rand::rand_bytes(&mut random)?;
        let choice = (u64::from_be_bytes(random) % (index as u64 + 1)) as usize;
        addresses.swap(index, choice);
    }
    Ok(())
}

/// Five per family while both remain, then fill from the remaining family.
pub(crate) fn next_h2_batch(
    preferred: &mut Vec<SocketAddr>,
    alternate: &mut Vec<SocketAddr>,
) -> Vec<RaceTarget> {
    let mut targets = Vec::with_capacity(10);
    let preferred_count = if alternate.is_empty() { 10 } else { 5 };
    for _ in 0..preferred_count {
        let Some(endpoint) = preferred.pop() else {
            break;
        };
        targets.push(RaceTarget {
            endpoint,
            delay: Duration::ZERO,
        });
    }
    let alternate_delay = if targets.is_empty() {
        Duration::ZERO
    } else {
        FAMILY_DELAY
    };
    while targets.len() < 10 {
        let Some(endpoint) = alternate.pop() else {
            break;
        };
        targets.push(RaceTarget {
            endpoint,
            delay: alternate_delay,
        });
    }
    while targets.len() < 10 {
        let Some(endpoint) = preferred.pop() else {
            break;
        };
        targets.push(RaceTarget {
            endpoint,
            delay: Duration::ZERO,
        });
    }
    targets
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct FakeConnection(Arc<AtomicUsize>);
    #[async_trait]
    impl RaceConnection for FakeConnection {
        async fn shutdown(self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
        fn is_alive(&self) -> bool {
            true
        }
    }

    fn policy() -> AutomaticEndpointPolicy {
        AutomaticEndpointPolicy {
            pool: usque_core::EndpointPool::Free,
            port: 443,
            ipv4: true,
            ipv6: true,
            tcp: true,
            udp: true,
        }
    }

    #[tokio::test]
    async fn native_h3_blackhole_race_times_out_and_preserves_h2_fallback() {
        struct Lease(Arc<AtomicUsize>);
        impl Drop for Lease {
            fn drop(&mut self) {
                self.0.fetch_sub(1, Ordering::SeqCst);
            }
        }
        struct Protector(Arc<AtomicUsize>);
        #[async_trait]
        impl crate::socket::SocketProtector for Protector {
            fn protect(&self, _: crate::socket::SocketHandle) -> Result<(), String> {
                Ok(())
            }
            async fn protect_for_target(
                &self,
                _: crate::socket::SocketHandle,
                endpoint: SocketAddr,
                protocol: crate::socket::DirectProtocol,
            ) -> Result<crate::socket::DirectEgressLease, String> {
                assert!(endpoint.ip().is_loopback());
                assert_eq!(protocol, crate::socket::DirectProtocol::Udp);
                self.0.fetch_add(1, Ordering::SeqCst);
                Ok(crate::socket::DirectEgressLease::hold(Lease(
                    self.0.clone(),
                )))
            }
        }
        // Bound, silent loopback peers model dropped UDP without public traffic.
        let mut peers = Vec::new();
        let mut targets = Vec::new();
        for _ in 0..4 {
            let peer = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
            targets.push(RaceTarget {
                endpoint: peer.local_addr().unwrap(),
                delay: Duration::ZERO,
            });
            peers.push(peer);
        }
        let key = usque_core::MasqueKeyPair::generate();
        let identity = crate::h2::MasqueTlsIdentity::new(
            key.private_sec1_der().unwrap(),
            &key.public_spki_der().unwrap(),
            "172.16.0.2".parse().unwrap(),
            "2001:db8::2".parse().unwrap(),
        )
        .unwrap();
        let leases = Arc::new(AtomicUsize::new(0));
        let protector: Arc<dyn crate::socket::SocketProtector> =
            Arc::new(Protector(leases.clone()));
        let result = tokio::time::timeout(
            Duration::from_secs(10),
            race_batch(targets, None, move |endpoint, cancellation| {
                let identity = identity.clone();
                let protector = protector.clone();
                async move {
                    crate::h3::connect_h3_with_cancellation(
                        endpoint,
                        "blackhole.test",
                        &identity,
                        crate::h3::H3ConnectSettings {
                            inner_mtu: usize::from(usque_core::config::DEFAULT_MTU),
                            congestion_control: Default::default(),
                        },
                        protector,
                        None,
                        cancellation,
                    )
                    .await
                    .map(|tunnel| crate::tunnel::MasqueTunnel::Http3(Box::new(tunnel)))
                }
            }),
        )
        .await
        .expect("native H3 timeout must finish loser cleanup before fallback");
        let error = match result {
            Ok(_) => panic!("silent peers cannot complete QUIC"),
            Err(error) => error,
        };
        assert!(
            error
                .failure(Some(usque_core::Transport::Http3), None)
                .fallback_allowed,
            "{error}"
        );
        assert_eq!(leases.load(Ordering::SeqCst), 0);
        drop(peers);
    }

    #[test]
    fn h2_cycle_is_finite_unique_and_contains_fixed_h3_hosts() {
        let policy = policy();
        let (mut ipv4, mut ipv6) = h2_candidates(policy).unwrap();
        assert_eq!(ipv4.len() + ipv6.len(), 1024);
        let all = ipv4.iter().chain(&ipv6).copied().collect::<HashSet<_>>();
        assert_eq!(all.len(), 1024);
        assert!(
            policy
                .h3_candidates()
                .iter()
                .all(|address| all.contains(address))
        );
        let mut visited = HashSet::new();
        while !ipv4.is_empty() || !ipv6.is_empty() {
            let batch = next_h2_batch(&mut ipv4, &mut ipv6);
            assert!(batch.len() <= 10);
            for target in batch {
                assert!(visited.insert(target.endpoint));
            }
        }
        assert_eq!(visited, all);
        assert_ne!(
            h2_candidates(policy).unwrap().1,
            h2_candidates(policy).unwrap().1
        );
    }

    #[test]
    fn dual_stack_batches_prioritize_known_hosts_and_never_exceed_ten() {
        let (mut ipv4, mut ipv6) = h2_candidates(policy()).unwrap();
        let batch = next_h2_batch(&mut ipv6, &mut ipv4);
        assert_eq!(batch.len(), 10);
        assert_eq!(
            batch
                .iter()
                .filter(|target| target.endpoint.is_ipv6())
                .count(),
            5
        );
        assert_eq!(batch[0].endpoint, "[2606:4700:103::1]:443".parse().unwrap());
        assert_eq!(batch[5].endpoint, "162.159.198.1:443".parse().unwrap());
        assert!(batch[..5].iter().all(|target| target.delay.is_zero()));
        assert!(batch[5..].iter().all(|target| target.delay == FAMILY_DELAY));
    }

    #[tokio::test(start_paused = true)]
    async fn final_h2_batch_succeeds_after_complete_traversal_and_reuses_winner() {
        struct Winner {
            identity: Arc<()>,
            closed: Arc<AtomicUsize>,
        }
        #[async_trait]
        impl RaceConnection for Winner {
            async fn shutdown(self) {
                self.closed.fetch_add(1, Ordering::SeqCst);
            }
            fn is_alive(&self) -> bool {
                true
            }
        }
        let policy = policy();
        let mut ipv4 = policy.h2_ipv4_candidates();
        let mut ipv6 = policy
            .ipv6_prefixes()
            .into_iter()
            .flat_map(|prefix| {
                (1..=256_u128).map(move |suffix| {
                    SocketAddr::new(
                        Ipv6Addr::from(u128::from(prefix) | suffix).into(),
                        policy.port,
                    )
                })
            })
            .collect::<Vec<_>>();
        let expected = ipv4.iter().chain(&ipv6).copied().collect::<HashSet<_>>();
        let last_endpoint = ipv4[0];
        let identity = Arc::new(());
        let closed = Arc::new(AtomicUsize::new(0));
        let visited = Arc::new(std::sync::Mutex::new(Vec::new()));
        let mut batch_count = 0;
        let winner = loop {
            let targets = next_h2_batch(&mut ipv6, &mut ipv4);
            assert!(!targets.is_empty());
            batch_count += 1;
            let result = race_batch(
                targets,
                Some(usque_core::endpoints::AUTOMATIC_H2_BATCH_TIMEOUT),
                {
                    let identity = identity.clone();
                    let closed = closed.clone();
                    let visited = visited.clone();
                    move |endpoint, _| {
                        let identity = identity.clone();
                        let closed = closed.clone();
                        let visited = visited.clone();
                        async move {
                            visited.lock().unwrap().push(endpoint);
                            if endpoint == last_endpoint {
                                Ok(Winner { identity, closed })
                            } else {
                                Err(TransportError::EndpointTimeout(endpoint))
                            }
                        }
                    }
                },
            )
            .await;
            match result {
                Ok(winner) => break winner,
                Err(error) => assert_eq!(
                    RecoveryDecision::for_failure(&error.failure(None, None)),
                    RecoveryDecision::Retry
                ),
            }
        };
        assert_eq!(batch_count, 103);
        assert!(ipv4.is_empty() && ipv6.is_empty());
        {
            let visited = visited.lock().unwrap();
            assert_eq!(visited.len(), 1024);
            assert_eq!(visited.iter().copied().collect::<HashSet<_>>(), expected);
        }
        assert!(Arc::ptr_eq(&winner.identity, &identity));
        assert_eq!(closed.load(Ordering::SeqCst), 0);
        winner.shutdown().await;
        assert_eq!(closed.load(Ordering::SeqCst), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn two_second_batch_deadline_joins_all_ten_before_returning() {
        let stopped = Arc::new(AtomicUsize::new(0));
        let started = tokio::time::Instant::now();
        let targets = (0..10)
            .map(|last| RaceTarget {
                endpoint: format!("162.159.199.{last}:443").parse().unwrap(),
                delay: Duration::ZERO,
            })
            .collect();
        let result = race_batch(targets, Some(Duration::from_secs(2)), {
            let stopped = stopped.clone();
            move |_, cancellation: CancellationToken| {
                let stopped = stopped.clone();
                async move {
                    cancellation.cancelled().await;
                    stopped.fetch_add(1, Ordering::SeqCst);
                    Err::<FakeConnection, _>(TransportError::TunnelClosed)
                }
            }
        })
        .await;
        assert!(result.is_err());
        assert_eq!(started.elapsed(), Duration::from_secs(2));
        assert_eq!(stopped.load(Ordering::SeqCst), 10);
    }

    #[tokio::test(start_paused = true)]
    async fn failed_preferred_family_starts_alternate_immediately() {
        let closed = Arc::new(AtomicUsize::new(0));
        let started = tokio::time::Instant::now();
        let winner = race_batch(
            vec![
                RaceTarget {
                    endpoint: "[2606:4700:104::1]:443".parse().unwrap(),
                    delay: Duration::ZERO,
                },
                RaceTarget {
                    endpoint: "162.159.199.1:443".parse().unwrap(),
                    delay: FAMILY_DELAY,
                },
            ],
            None,
            move |endpoint, _| {
                let closed = closed.clone();
                async move {
                    if endpoint.is_ipv6() {
                        Err(TransportError::EndpointTimeout(endpoint))
                    } else {
                        Ok(FakeConnection(closed))
                    }
                }
            },
        )
        .await
        .unwrap();
        assert!(started.elapsed() < FAMILY_DELAY);
        winner.shutdown().await;
    }

    #[tokio::test]
    async fn dropping_outer_race_still_polls_candidate_cleanup_to_completion() {
        let started = Arc::new(tokio::sync::Notify::new());
        let stopped = Arc::new(tokio::sync::Notify::new());
        let race = race_batch(
            vec![RaceTarget {
                endpoint: "162.159.199.1:443".parse().unwrap(),
                delay: Duration::ZERO,
            }],
            None,
            {
                let started = started.clone();
                let stopped = stopped.clone();
                move |_, cancellation: CancellationToken| {
                    let started = started.clone();
                    let stopped = stopped.clone();
                    async move {
                        started.notify_one();
                        cancellation.cancelled().await;
                        tokio::task::yield_now().await;
                        stopped.notify_one();
                        Err::<FakeConnection, _>(TransportError::TunnelClosed)
                    }
                }
            },
        );
        let task = tokio::spawn(race);
        started.notified().await;
        task.abort();
        let _ = task.await;
        tokio::time::timeout(Duration::from_secs(1), stopped.notified())
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn protection_failure_outranks_pin_refresh_during_cleanup() {
        let targets = (1..=2)
            .map(|last| RaceTarget {
                endpoint: format!("162.159.199.{last}:443").parse().unwrap(),
                delay: Duration::ZERO,
            })
            .collect();
        let result = race_batch(
            targets,
            None,
            |endpoint: SocketAddr, cancellation: CancellationToken| async move {
                if endpoint.ip().to_string().ends_with(".1") {
                    Err::<FakeConnection, _>(TransportError::EndpointPinMismatch)
                } else {
                    cancellation.cancelled().await;
                    Err(TransportError::SocketProtection("denied".to_owned()))
                }
            },
        )
        .await;
        assert!(matches!(result, Err(TransportError::SocketProtection(_))));
    }

    #[test]
    fn automatic_candidates_exclude_active_dns_targets() {
        let mut profile = usque_core::Profile::default();
        let endpoint: SocketAddr = "162.159.199.1:443".parse().unwrap();
        profile.dns_servers = vec![endpoint.ip()];
        assert!(excludes_dns_server(&profile, endpoint));
        profile.dns_servers.clear();
        assert!(!excludes_dns_server(&profile, endpoint));
    }

    #[tokio::test]
    async fn winner_cancels_and_joins_losing_requests() {
        let stopped = Arc::new(AtomicUsize::new(0));
        let closed = Arc::new(AtomicUsize::new(0));
        let target = "162.159.199.1:443".parse().unwrap();
        let connect = {
            let stopped = stopped.clone();
            let closed = closed.clone();
            move |endpoint: SocketAddr, cancellation: CancellationToken| {
                let stopped = stopped.clone();
                let closed = closed.clone();
                async move {
                    if endpoint.ip().to_string() == "162.159.199.1" {
                        tokio::time::sleep(Duration::from_millis(5)).await;
                        Ok(FakeConnection(closed))
                    } else {
                        cancellation.cancelled().await;
                        stopped.fetch_add(1, Ordering::SeqCst);
                        Err(TransportError::TunnelClosed)
                    }
                }
            }
        };
        let winner = race_batch(
            vec![
                RaceTarget {
                    endpoint: target,
                    delay: Duration::ZERO,
                },
                RaceTarget {
                    endpoint: "162.159.199.2:443".parse().unwrap(),
                    delay: Duration::ZERO,
                },
            ],
            Some(Duration::from_secs(2)),
            connect,
        )
        .await
        .unwrap();
        assert_eq!(stopped.load(Ordering::SeqCst), 1);
        assert_eq!(closed.load(Ordering::SeqCst), 0);
        winner.shutdown().await;
        assert_eq!(closed.load(Ordering::SeqCst), 1);
    }
}
