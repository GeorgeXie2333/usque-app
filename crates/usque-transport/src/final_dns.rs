//! Bounded candidate racing for the final chained exit only.
use std::future::Future;
use std::time::Duration;
use tokio::time::{Instant, sleep_until, timeout_at};

pub(crate) async fn query<S: Copy, T, F, Fut>(
    servers: &[S],
    deadline: Instant,
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
                let candidate_deadline = deadline.min(Instant::now() + Duration::from_secs(1));
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
