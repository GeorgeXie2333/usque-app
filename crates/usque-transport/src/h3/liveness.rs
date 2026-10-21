//! ACK/PTO liveness independent of the idle keepalive cadence.
use std::net::SocketAddr;
use std::time::Duration;

use tokio::time::Instant;

#[derive(Default)]
pub(super) struct Liveness {
    sample: Option<Sample>,
    progress_at: Option<Instant>,
    resume_until: Option<Instant>,
}

struct Sample {
    path: (SocketAddr, SocketAddr),
    acked: u64,
    pto: usize,
    observed_at: Instant,
}

impl Liveness {
    pub(super) fn stalled(
        &mut self,
        path: (SocketAddr, SocketAddr),
        acked: u64,
        pto: usize,
        rtt: Duration,
        now: Instant,
    ) -> bool {
        let Some(previous) = self.sample.as_mut() else {
            self.sample = Some(Sample {
                path,
                acked,
                pto,
                observed_at: now,
            });
            self.progress_at = None;
            return false;
        };
        let resumed = now.saturating_duration_since(previous.observed_at) > Duration::from_secs(5);
        let changed = previous.path != path || acked != previous.acked || pto < previous.pto;
        previous.observed_at = now;
        if changed || resumed {
            *previous = Sample {
                path,
                acked,
                pto,
                observed_at: now,
            };
            self.progress_at = None;
            if resumed {
                self.resume_until = Some(now + Duration::from_secs(5));
            }
            return false;
        }
        // Start the loss window with an outstanding probe, so business-idle
        // time cannot consume the new probe's ACK deadline.
        if pto == previous.pto {
            return false;
        }
        let progress_at = *self.progress_at.get_or_insert(now);
        pto.saturating_sub(previous.pto) >= 2
            && self.resume_until.is_none_or(|until| now >= until)
            && now.saturating_duration_since(progress_at)
                >= usque_core::recovery::liveness_timeout(Some(rtt))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path() -> (SocketAddr, SocketAddr) {
        (
            "127.0.0.1:1000".parse().unwrap(),
            "127.0.0.1:443".parse().unwrap(),
        )
    }

    #[tokio::test(start_paused = true)]
    async fn idle_is_healthy_but_two_ptos_without_ack_progress_trigger_recovery() {
        let mut monitor = Liveness::default();
        let rtt = Duration::from_millis(50);
        for _ in 0..600 {
            assert!(!monitor.stalled(path(), 10, 0, rtt, Instant::now()));
            tokio::time::advance(Duration::from_secs(1)).await;
        }
        assert!(!monitor.stalled(path(), 10, 1, rtt, Instant::now()));
        tokio::time::advance(Duration::from_secs(1)).await;
        assert!(!monitor.stalled(path(), 10, 2, rtt, Instant::now()));
        tokio::time::advance(Duration::from_secs(2)).await;
        assert!(monitor.stalled(path(), 10, 2, rtt, Instant::now()));
        assert!(!monitor.stalled(path(), 20, 2, rtt, Instant::now()));
    }

    #[tokio::test(start_paused = true)]
    async fn resume_and_new_path_reset_stale_loss_history() {
        let mut monitor = Liveness::default();
        let rtt = Duration::from_secs(1);
        assert!(!monitor.stalled(path(), 1, 0, rtt, Instant::now()));
        tokio::time::advance(Duration::from_secs(60)).await;
        assert!(!monitor.stalled(path(), 1, 10, rtt, Instant::now()));
        for _ in 0..4 {
            tokio::time::advance(Duration::from_secs(1)).await;
            assert!(!monitor.stalled(path(), 1, 12, rtt, Instant::now()));
        }
        tokio::time::advance(Duration::from_secs(1)).await;
        assert!(monitor.stalled(path(), 1, 12, rtt, Instant::now()));
        let mut next = path();
        next.0.set_port(1001);
        assert!(!monitor.stalled(next, 1, 12, rtt, Instant::now()));
    }
}
