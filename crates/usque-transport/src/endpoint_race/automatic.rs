//! Sliding candidate admission across carriers. A slow candidate never holds
//! the next candidate behind an entire batch, and waiting consumes no deadline.
use super::*;
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use tokio::time::Instant;
use usque_core::{EndpointPool, Profile, Transport, recovery};

struct Cancellations(HashMap<u64, CancellationToken>);

impl Drop for Cancellations {
    fn drop(&mut self) {
        for token in self.0.values() {
            token.cancel();
        }
    }
}

pub(crate) async fn race_automatic<T, F, Fut>(
    profile: Profile,
    pool: EndpointPool,
    protector: Arc<dyn crate::socket::SocketProtector>,
    slots: Arc<tokio::sync::Semaphore>,
    connect: F,
) -> Result<T, TransportError>
where
    T: RaceConnection,
    F: Fn(Transport, SocketAddr, CancellationToken) -> Fut + Clone + Send + 'static,
    Fut: Future<Output = Result<T, TransportError>> + Send + 'static,
{
    let cancellation = CancellationToken::new();
    let mut owner = RaceOwner {
        task: Some(tokio::spawn(run(
            profile,
            pool,
            protector,
            slots,
            connect,
            cancellation.clone(),
        ))),
        cancellation,
    };
    let result = owner
        .task
        .as_mut()
        .expect("race owner")
        .await
        .map_err(|error| TransportError::Driver(error.to_string()))?;
    owner.task.take();
    result
}

fn targets(
    profile: &Profile,
    policy: AutomaticEndpointPolicy,
    protector: &dyn crate::socket::SocketProtector,
) -> Result<(Vec<RaceTarget>, VecDeque<RaceTarget>), TransportError> {
    let allowed = |target: &RaceTarget| {
        protector.endpoint_family_available(target.endpoint) != Some(false)
            && !excludes_dns_server(profile, target.endpoint)
    };
    let mut h3: Vec<RaceTarget> = if policy.udp {
        h3_targets(policy, profile.ip_policy)
            .into_iter()
            .filter(allowed)
            .collect()
    } else {
        Vec::new()
    };
    if !h3.iter().any(|target| target.delay.is_zero()) {
        for target in &mut h3 {
            target.delay = Duration::ZERO;
        }
    }
    let mut h2 = VecDeque::new();
    if policy.tcp {
        let (v4, v6) = h2_candidates(policy)?;
        let (mut preferred, mut alternate) =
            if matches!(profile.ip_policy, IpPolicy::PreferIpv4 | IpPolicy::Ipv4Only) {
                (v4, v6)
            } else {
                (v6, v4)
            };
        while !preferred.is_empty() || !alternate.is_empty() {
            h2.extend(
                next_h2_batch(&mut preferred, &mut alternate)
                    .into_iter()
                    .filter(allowed),
            );
        }
    }
    Ok((h3, h2))
}

async fn run<T, F, Fut>(
    profile: Profile,
    pool: EndpointPool,
    protector: Arc<dyn crate::socket::SocketProtector>,
    slots: Arc<tokio::sync::Semaphore>,
    connect: F,
    cancellation: CancellationToken,
) -> Result<T, TransportError>
where
    T: RaceConnection,
    F: Fn(Transport, SocketAddr, CancellationToken) -> Fut + Clone + Send + 'static,
    Fut: Future<Output = Result<T, TransportError>> + Send + 'static,
{
    let generation = protector.network_generation();
    let policy = AutomaticEndpointPolicy::for_profile(&profile, pool);
    let (h3_targets, mut h2) = targets(&profile, policy, protector.as_ref())?;
    let mut h3: VecDeque<_> = h3_targets.iter().copied().collect();
    let mut h3_completed = h3.is_empty();
    let mut h3_restart = None;
    let h2_start = Instant::now()
        + if h3.is_empty() {
            Duration::ZERO
        } else {
            recovery::TRANSPORT_RACE_DELAY
        };
    let mut in_flight = [0usize; 2];
    let mut next_carrier = 0usize;
    let mut next_id = 0u64;
    let mut tasks = JoinSet::new();
    let mut cancellations = Cancellations(HashMap::new());
    let mut failures = CandidateFailures::default();
    let mut terminal = None;
    let mut winner = None;

    loop {
        if protector.network_generation() != generation {
            terminal = Some(TransportError::UnderlyingNetworkChanged);
            break;
        }
        if h3_completed && h2.is_empty() && in_flight[1] == 0 {
            break;
        }
        if h3.is_empty() && in_flight[0] == 0 && !h3_targets.is_empty() {
            let restart =
                h3_restart.get_or_insert_with(|| Instant::now() + recovery::RETRY_INTERVAL);
            if Instant::now() >= *restart {
                h3.extend(h3_targets.iter().copied());
                h3_restart = None;
            }
        }
        while tasks.len() < recovery::AUTOMATIC_CANDIDATE_LIMIT {
            let ready = [
                !h3.is_empty() && in_flight[0] < recovery::H3_CANDIDATE_LIMIT,
                !h2.is_empty()
                    && Instant::now() >= h2_start
                    && in_flight[1] < recovery::H2_CANDIDATE_LIMIT,
            ];
            let lane = if ready[next_carrier] {
                next_carrier
            } else if ready[1 - next_carrier] {
                1 - next_carrier
            } else {
                break;
            };
            next_carrier = 1 - lane;
            let target = if lane == 0 {
                h3.pop_front()
            } else {
                h2.pop_front()
            }
            .expect("ready lane");
            let transport = if lane == 0 {
                Transport::Http3
            } else {
                Transport::Http2
            };
            let token = cancellation.child_token();
            let id = next_id;
            next_id = next_id.wrapping_add(1);
            cancellations.0.insert(id, token.clone());
            in_flight[lane] += 1;
            let connect = connect.clone();
            let slots = slots.clone();
            tasks.spawn(async move {
                tokio::select! {
                    biased;
                    _ = token.cancelled() => return (id, lane, Err(TransportError::TunnelClosed)),
                    _ = tokio::time::sleep(target.delay) => {},
                }
                let slot = tokio::select! {
                    biased;
                    _ = token.cancelled() => return (id, lane, Err(TransportError::TunnelClosed)),
                    slot = slots.acquire_owned() => slot.expect("intent slots remain open"),
                };
                let operation = connect(transport, target.endpoint, token.clone());
                tokio::pin!(operation);
                let limit = if lane == 0 { Duration::from_secs(8) } else { usque_core::endpoints::AUTOMATIC_H2_BATCH_TIMEOUT };
                let result = tokio::select! {
                    biased;
                    result = &mut operation => result,
                    _ = token.cancelled() => {
                        match operation.await {
                            Ok(connection) => { connection.shutdown().await; Err(TransportError::TunnelClosed) },
                            Err(error) => Err(error),
                        }
                    },
                    _ = tokio::time::sleep(limit) => {
                        token.cancel();
                        match operation.await {
                            Ok(connection) => { connection.shutdown().await; Err(TransportError::ConnectTimeout) },
                            Err(error) if RacePolicy::Automatic.stops(&error) || matches!(error, TransportError::EndpointPinMismatch) => Err(error),
                            Err(_) => Err(TransportError::ConnectTimeout),
                        }
                    }
                };
                // A completed handshake still owns its socket and endpoint
                // authorization until it is selected or explicitly shut down.
                (id, lane, result.map(|connection| (connection, slot)))
            });
        }
        let wake = [Some(h2_start), h3_restart]
            .into_iter()
            .flatten()
            .filter(|at| *at > Instant::now())
            .min();
        tokio::select! {
            biased;
            _ = cancellation.cancelled() => { terminal = Some(TransportError::TunnelClosed); break; },
            _ = async { match wake { Some(at) => tokio::time::sleep_until(at).await, None => std::future::pending().await } } => {},
            joined = tasks.join_next(), if !tasks.is_empty() => {
                match joined {
                    Some(Ok((id, lane, result))) => {
                        cancellations.0.remove(&id);
                        in_flight[lane] -= 1;
                        if lane == 0 && h3.is_empty() && in_flight[0] == 0 { h3_completed = true; }
                        match result {
                            Ok(connection) => { winner = Some(connection); break; },
                            Err(error) => if let Err(error) = failures.record(error) { terminal = Some(error); break; },
                        }
                    },
                    _ => { terminal = Some(TransportError::Netstack("endpoint candidate task failed".into())); break; },
                }
            },
        }
    }
    drop(cancellations);
    while let Some(joined) = tasks.join_next().await {
        match joined {
            Ok((_, _, Ok((connection, _slot)))) => connection.shutdown().await,
            Ok((_, _, Err(error))) if !matches!(error, TransportError::TunnelClosed) => {
                if let Err(error) = failures.record(error)
                    && terminal.as_ref().is_none_or(|previous| {
                        recovery_priority(previous) < recovery_priority(&error)
                    })
                {
                    terminal = Some(error);
                }
            }
            Err(_) => {
                terminal = Some(TransportError::Netstack(
                    "endpoint candidate task failed".into(),
                ))
            }
            _ => {}
        }
    }
    if cancellation.is_cancelled() {
        terminal = Some(TransportError::TunnelClosed);
    }
    if let Some((connection, _slot)) = winner {
        if terminal.is_none() && connection.is_alive() {
            return finish_generation(Ok(connection), protector.as_ref(), generation).await;
        }
        connection.shutdown().await;
    }
    Err(terminal.unwrap_or_else(|| failures.exhausted()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Connected;
    #[async_trait]
    impl RaceConnection for Connected {
        async fn shutdown(self) {}
        fn is_alive(&self) -> bool {
            true
        }
    }

    fn profile() -> Profile {
        Profile {
            transport: usque_core::TransportPolicy::Auto,
            ip_policy: IpPolicy::Ipv4Only,
            ..Profile::default()
        }
    }

    #[tokio::test(start_paused = true)]
    async fn h2_starts_after_250ms_while_h3_is_blackholed() {
        let started = Instant::now();
        let h3 = Arc::new(AtomicUsize::new(0));
        let seen = h3.clone();
        let result = race_automatic(
            profile(),
            EndpointPool::WarpPlus,
            crate::socket::noop_socket_protector(),
            Arc::new(tokio::sync::Semaphore::new(
                recovery::AUTOMATIC_CANDIDATE_LIMIT,
            )),
            move |transport, _, cancel| {
                let seen = seen.clone();
                async move {
                    if transport == Transport::Http3 {
                        seen.fetch_add(1, Ordering::Relaxed);
                        cancel.cancelled().await;
                        Err(TransportError::TunnelClosed)
                    } else {
                        assert_eq!(started.elapsed(), Duration::from_millis(250));
                        Ok(Connected)
                    }
                }
            },
        )
        .await;
        assert!(result.is_ok());
        assert_eq!(h3.load(Ordering::Relaxed), 2);
        assert_eq!(started.elapsed(), Duration::from_millis(250));
    }

    #[tokio::test(start_paused = true)]
    async fn carrier_limits_share_fourteen_slots_until_cleanup_completes() {
        let active = Arc::new([
            AtomicUsize::new(0),
            AtomicUsize::new(0),
            AtomicUsize::new(0),
        ]);
        let peak = Arc::new(AtomicUsize::new(0));
        let mut policy = profile();
        policy.ip_policy = IpPolicy::Auto;
        let total = active.clone();
        let maximum = peak.clone();
        let result = race_automatic(
            policy,
            EndpointPool::Free,
            crate::socket::noop_socket_protector(),
            Arc::new(tokio::sync::Semaphore::new(
                recovery::AUTOMATIC_CANDIDATE_LIMIT,
            )),
            move |transport, _, cancel| {
                let active = total.clone();
                let peak = maximum.clone();
                async move {
                    let lane = if transport == Transport::Http3 { 0 } else { 1 };
                    let lane_count = active[lane].fetch_add(1, Ordering::SeqCst) + 1;
                    let count = active[2].fetch_add(1, Ordering::SeqCst) + 1;
                    peak.fetch_max(count, Ordering::Relaxed);
                    assert!(count <= 14);
                    assert!(lane_count <= if lane == 0 { 8 } else { 10 });
                    let success = tokio::select! {
                        biased;
                        _ = cancel.cancelled() => false,
                        _ = tokio::time::sleep(Duration::from_millis(500)), if lane == 1 => true,
                    };
                    tokio::time::sleep(Duration::from_millis(50)).await;
                    active[lane].fetch_sub(1, Ordering::SeqCst);
                    active[2].fetch_sub(1, Ordering::SeqCst);
                    if success {
                        Ok(Connected)
                    } else {
                        Err(TransportError::TunnelClosed)
                    }
                }
            },
        )
        .await;
        assert!(result.is_ok());
        assert_eq!(peak.load(Ordering::Relaxed), 14);
        assert_eq!(active[2].load(Ordering::SeqCst), 0);
    }

    #[tokio::test(start_paused = true)]
    async fn successful_losers_retain_their_slot_until_socket_cleanup() {
        struct Authorized {
            slots: Arc<tokio::sync::Semaphore>,
            closed: Arc<AtomicUsize>,
        }
        #[async_trait]
        impl RaceConnection for Authorized {
            async fn shutdown(self) {
                assert!(self.slots.available_permits() < recovery::AUTOMATIC_CANDIDATE_LIMIT);
                self.closed.fetch_add(1, Ordering::SeqCst);
            }
            fn is_alive(&self) -> bool {
                true
            }
        }
        let slots = Arc::new(tokio::sync::Semaphore::new(
            recovery::AUTOMATIC_CANDIDATE_LIMIT,
        ));
        let closed = Arc::new(AtomicUsize::new(0));
        let barrier = Arc::new(tokio::sync::Barrier::new(2));
        let mut profile = profile();
        profile.transport = usque_core::TransportPolicy::Http3;
        let observed_slots = slots.clone();
        let observed_closed = closed.clone();
        let result = race_automatic(
            profile,
            EndpointPool::WarpPlus,
            crate::socket::noop_socket_protector(),
            slots.clone(),
            move |_, _, _| {
                let barrier = barrier.clone();
                let slots = observed_slots.clone();
                let closed = observed_closed.clone();
                async move {
                    barrier.wait().await;
                    Ok(Authorized { slots, closed })
                }
            },
        )
        .await;
        assert!(result.is_ok());
        assert_eq!(closed.load(Ordering::SeqCst), 1);
        assert_eq!(
            slots.available_permits(),
            recovery::AUTOMATIC_CANDIDATE_LIMIT
        );
    }

    #[tokio::test(start_paused = true)]
    async fn h2_traverses_every_eligible_address_without_restarting_the_queue() {
        let mut profile = profile();
        profile.transport = usque_core::TransportPolicy::Http2;
        let addresses = Arc::new(std::sync::Mutex::new(HashSet::new()));
        let seen = addresses.clone();
        let result = race_automatic(
            profile,
            EndpointPool::WarpPlus,
            crate::socket::noop_socket_protector(),
            Arc::new(tokio::sync::Semaphore::new(
                recovery::AUTOMATIC_CANDIDATE_LIMIT,
            )),
            move |transport, endpoint, _| {
                let seen = seen.clone();
                async move {
                    assert_eq!(transport, Transport::Http2);
                    let mut seen = seen.lock().unwrap();
                    assert!(
                        seen.insert(endpoint),
                        "a failed candidate was repeated in the same round"
                    );
                    if seen.len() == 255 {
                        Ok(Connected)
                    } else {
                        Err(TransportError::ConnectTimeout)
                    }
                }
            },
        )
        .await;
        assert!(result.is_ok());
        let addresses = addresses.lock().unwrap();
        assert_eq!(addresses.len(), 255);
        assert!(!addresses.contains(&"162.159.199.1:443".parse().unwrap()));
    }

    #[tokio::test(start_paused = true)]
    async fn candidate_pin_failure_does_not_prevent_an_authenticated_other_carrier() {
        let result = race_automatic(
            profile(),
            EndpointPool::WarpPlus,
            crate::socket::noop_socket_protector(),
            Arc::new(tokio::sync::Semaphore::new(
                recovery::AUTOMATIC_CANDIDATE_LIMIT,
            )),
            |transport, _, _| async move {
                if transport == Transport::Http3 {
                    Err(TransportError::EndpointPinMismatch)
                } else {
                    Ok(Connected)
                }
            },
        )
        .await;
        assert!(result.is_ok());
    }

    #[tokio::test(start_paused = true)]
    async fn terminal_authentication_cancels_the_entire_race() {
        let result = race_automatic(
            profile(),
            EndpointPool::WarpPlus,
            crate::socket::noop_socket_protector(),
            Arc::new(tokio::sync::Semaphore::new(
                recovery::AUTOMATIC_CANDIDATE_LIMIT,
            )),
            |_, _, _| async { Err::<Connected, _>(TransportError::Http3ConnectRejected(403)) },
        )
        .await;
        assert!(matches!(
            result,
            Err(TransportError::Http3ConnectRejected(403))
        ));
    }

    #[tokio::test(start_paused = true)]
    async fn terminal_error_returned_during_loser_cleanup_overrides_a_winner() {
        struct Tracked(Arc<AtomicUsize>);
        #[async_trait]
        impl RaceConnection for Tracked {
            async fn shutdown(self) {
                self.0.fetch_add(1, Ordering::SeqCst);
            }
            fn is_alive(&self) -> bool {
                true
            }
        }
        let started = Arc::new(AtomicUsize::new(0));
        let closed = Arc::new(AtomicUsize::new(0));
        let observed = closed.clone();
        let result = race_automatic(
            profile(),
            EndpointPool::WarpPlus,
            crate::socket::noop_socket_protector(),
            Arc::new(tokio::sync::Semaphore::new(
                recovery::AUTOMATIC_CANDIDATE_LIMIT,
            )),
            move |_, _, cancel| {
                let started = started.clone();
                let closed = observed.clone();
                async move {
                    if started.fetch_add(1, Ordering::SeqCst) == 0 {
                        cancel.cancelled().await;
                        tokio::time::sleep(Duration::from_millis(50)).await;
                        Err(TransportError::Http3ConnectRejected(403))
                    } else {
                        Ok(Tracked(closed))
                    }
                }
            },
        )
        .await;
        assert!(matches!(
            result,
            Err(TransportError::Http3ConnectRejected(403))
        ));
        assert_eq!(closed.load(Ordering::SeqCst), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn mixed_exhaustion_retains_pin_evidence_and_terminal_identity_wins() {
        for terminal in [false, true] {
            let result = race_automatic(
                profile(),
                EndpointPool::WarpPlus,
                crate::socket::noop_socket_protector(),
                Arc::new(tokio::sync::Semaphore::new(
                    recovery::AUTOMATIC_CANDIDATE_LIMIT,
                )),
                move |transport, _, _| async move {
                    Err::<Connected, _>(if transport == Transport::Http3 {
                        TransportError::EndpointPinMismatch
                    } else if terminal {
                        TransportError::InvalidIdentity
                    } else {
                        TransportError::ConnectTimeout
                    })
                },
            )
            .await;
            assert!(matches!(
                (terminal, result),
                (true, Err(TransportError::InvalidIdentity))
                    | (false, Err(TransportError::EndpointPinMismatch))
            ));
        }
    }
}
