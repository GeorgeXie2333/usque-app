//! Bounded candidate racing for the final chained exit only.
use std::future::Future;
use std::time::Duration;
use tokio::time::{Instant, sleep_until, timeout_at};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Transport {
    Udp,
    Tcp,
}

/// All servers and protocols share the same two active slots and deadline.
/// Preserve the 250 ms backup-server start. A single resolver gets a TCP
/// hedge after 250 ms; with two UDP requests active, TCP waits for capacity.
pub(crate) async fn query_auto<S: Copy, T, F, Fut>(
    servers: &[S],
    deadline: Instant,
    mut attempt: F,
) -> Result<T, String>
where
    F: FnMut(S, Transport, Instant) -> Fut,
    Fut: Future<Output = Result<T, String>>,
{
    let mut candidates = Vec::with_capacity(servers.len().min(8) * 2);
    for transport in [Transport::Udp, Transport::Tcp] {
        candidates.extend(servers.iter().take(8).map(|server| (*server, transport)));
    }
    // Reserve time for later resolvers and TCP alternatives rather than
    // spending the whole deadline retrying the first few silent servers.
    let budget = deadline
        .saturating_duration_since(Instant::now())
        .min(Duration::from_secs(4));
    let limit = (budget * 2 / candidates.len().max(1) as u32).min(Duration::from_secs(1));
    query_with_limit(
        &candidates,
        deadline,
        limit,
        |(server, transport), deadline| attempt(server, transport, deadline),
    )
    .await
}

pub(crate) async fn query<S: Copy, T, F, Fut>(
    servers: &[S],
    deadline: Instant,
    query: F,
) -> Result<T, String>
where
    F: FnMut(S, Instant) -> Fut,
    Fut: Future<Output = Result<T, String>>,
{
    query_with_limit(servers, deadline, Duration::from_secs(1), query).await
}

async fn query_with_limit<S: Copy, T, F, Fut>(
    servers: &[S],
    deadline: Instant,
    limit: Duration,
    mut query: F,
) -> Result<T, String>
where
    F: FnMut(S, Instant) -> Fut,
    Fut: Future<Output = Result<T, String>>,
{
    let deadline = deadline.min(Instant::now() + Duration::from_secs(4));
    let mut first = None;
    let mut second = None;
    let mut remaining = servers.iter();
    let mut next_start = Instant::now();
    let mut exhausted = false;
    let mut last = "no usable DNS servers".to_owned();
    loop {
        if Instant::now() >= deadline {
            return Err("DNS query timed out".into());
        }
        let active = usize::from(first.is_some()) + usize::from(second.is_some());
        if !exhausted && active < 2 && (active == 0 || Instant::now() >= next_start) {
            if let Some(server) = remaining.next() {
                let candidate_deadline = deadline.min(Instant::now() + limit);
                let future = query(*server, candidate_deadline);
                let future = Box::pin(async move {
                    timeout_at(candidate_deadline, future)
                        .await
                        .unwrap_or_else(|_| Err("DNS candidate timed out".into()))
                });
                if first.is_none() {
                    first = Some(future);
                } else {
                    second = Some(future);
                }
                next_start = Instant::now() + Duration::from_millis(250);
            } else {
                exhausted = true;
            }
        }
        if first.is_none() && second.is_none() && exhausted {
            return Err(last);
        }
        let result = tokio::select! {
            _ = sleep_until(deadline) => return Err("DNS query timed out".into()),
            _ = sleep_until(next_start), if !exhausted && (first.is_none() || second.is_none()) => continue,
            result = wait(&mut first), if first.is_some() => { first.take(); result },
            result = wait(&mut second), if second.is_some() => { second.take(); result },
        };
        match result {
            Ok(value) => return Ok(value),
            Err(error) => {
                last = error;
                next_start = Instant::now();
            }
        }
    }
}
async fn wait<F: Future + Unpin>(slot: &mut Option<F>) -> F::Output {
    match slot {
        Some(future) => future.await,
        None => std::future::pending().await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test(start_paused = true)]
    async fn silent_early_resolvers_do_not_starve_the_fifth_server() {
        let start = Instant::now();
        let result = query_auto(
            &[1, 2, 3, 4, 5, 6, 7, 8],
            start + Duration::from_secs(4),
            |server, transport, _| async move {
                if server == 5 && transport == Transport::Udp {
                    Ok(5)
                } else {
                    std::future::pending().await
                }
            },
        )
        .await
        .unwrap();
        assert_eq!(result, 5);
        assert!(start.elapsed() <= Duration::from_secs(2));
    }
    #[tokio::test(start_paused = true)]
    async fn automatic_tcp_hedge_preserves_udp_and_backup_server_precedence() {
        let start = Instant::now();
        let response = query_auto(
            &[1],
            start + Duration::from_secs(4),
            |_, transport, _| async move {
                match transport {
                    Transport::Udp => std::future::pending().await,
                    Transport::Tcp => Ok(Vec::<u8>::new()), // valid negative DNS answer
                }
            },
        )
        .await
        .unwrap();
        assert!(response.is_empty());
        assert_eq!(start.elapsed(), Duration::from_millis(250));
        let start = Instant::now();
        assert_eq!(
            query_auto(
                &[1, 2],
                start + Duration::from_secs(4),
                |server, transport, _| async move {
                    assert_eq!(transport, Transport::Udp);
                    if server == 1 {
                        std::future::pending().await
                    } else {
                        Ok(2)
                    }
                }
            )
            .await
            .unwrap(),
            2
        );
        assert_eq!(start.elapsed(), Duration::from_millis(250));
        let start = Instant::now();
        assert_eq!(
            query_auto(
                &[1],
                start + Duration::from_secs(4),
                |_, transport, _| async move {
                    match transport {
                        Transport::Udp => {
                            tokio::time::sleep(Duration::from_millis(400)).await;
                            Ok(1)
                        }
                        Transport::Tcp => Err("refused".into()),
                    }
                }
            )
            .await
            .unwrap(),
            1
        );
        assert_eq!(start.elapsed(), Duration::from_millis(400));
    }

    #[tokio::test(start_paused = true)]
    async fn auto_servers_and_protocols_share_two_slots_and_one_deadline() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let active = AtomicUsize::new(0);
        let tcp = AtomicUsize::new(0);
        struct Guard<'a>(&'a AtomicUsize);
        impl Drop for Guard<'_> {
            fn drop(&mut self) {
                self.0.fetch_sub(1, Ordering::SeqCst);
            }
        }
        let start = Instant::now();
        let result: Result<(), _> = query_auto(
            &[0; 8],
            start + Duration::from_secs(4),
            |_, transport, _| {
                let active = &active;
                let tcp = &tcp;
                async move {
                    assert!(active.fetch_add(1, Ordering::SeqCst) < 2);
                    let _guard = Guard(active);
                    if transport == Transport::Tcp {
                        tcp.fetch_add(1, Ordering::SeqCst);
                    }
                    std::future::pending().await
                }
            },
        )
        .await;
        assert!(result.is_err());
        assert_eq!(start.elapsed(), Duration::from_secs(4));
        assert_eq!(active.load(Ordering::SeqCst), 0);
        assert!(tcp.load(Ordering::SeqCst) > 0);
    }
    #[tokio::test(start_paused = true)]
    async fn empty_expired_and_single_candidate_bounds() {
        let start = Instant::now();
        let no_io = |_: u8, _| async {
            panic!("unusable list must not perform I/O");
            #[allow(unreachable_code)]
            Ok::<(), String>(())
        };
        assert!(
            query(&[], start + Duration::from_secs(4), no_io)
                .await
                .is_err()
        );
        assert!(query(&[1], start, no_io).await.is_err());
        assert_eq!(start.elapsed(), Duration::ZERO);
        assert!(
            query::<_, (), _, _>(&[1], start + Duration::from_secs(4), |_, _| {
                std::future::pending()
            })
            .await
            .is_err()
        );
        assert_eq!(start.elapsed(), Duration::from_secs(1));
    }
    #[tokio::test(start_paused = true)]
    async fn failed_candidates_are_replaced_immediately() {
        let start = Instant::now();
        assert_eq!(
            query(&[1, 2], start + Duration::from_secs(4), |n, _| async move {
                if n == 1 { Err("failed".into()) } else { Ok(7) }
            })
            .await
            .unwrap(),
            7
        );
        assert_eq!(start.elapsed(), Duration::ZERO);
    }
    #[tokio::test(start_paused = true)]
    async fn silent_primary_does_not_delay_a_valid_negative_backup() {
        let start = Instant::now();
        let answer = query(
            &[1, 2, 3],
            start + Duration::from_secs(4),
            |server, _| async move {
                if server == 1 {
                    std::future::pending().await
                } else {
                    assert_eq!(server, 2);
                    Ok(Vec::<u8>::new())
                }
            },
        )
        .await
        .unwrap();
        assert!(answer.is_empty());
        assert_eq!(start.elapsed(), Duration::from_millis(250));
    }
    #[tokio::test(start_paused = true)]
    async fn concurrency_deadline_and_loser_cleanup_are_bounded() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let active = AtomicUsize::new(0);
        struct Guard<'a>(&'a AtomicUsize);
        impl Drop for Guard<'_> {
            fn drop(&mut self) {
                self.0.fetch_sub(1, Ordering::SeqCst);
            }
        }
        let start = Instant::now();
        let result: Result<(), _> = query(&[0; 16], start + Duration::from_secs(4), |_, _| async {
            let count = active.fetch_add(1, Ordering::SeqCst) + 1;
            let _guard = Guard(&active);
            assert!(count <= 2);
            std::future::pending().await
        })
        .await;
        assert!(result.is_err());
        assert_eq!(start.elapsed(), Duration::from_secs(4));
        assert_eq!(active.load(Ordering::SeqCst), 0);
    }
}
