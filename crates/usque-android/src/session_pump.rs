//! Platform-independent arbitration for Android's bounded packet pump.
use std::future::Future;
use std::pin::Pin;

/// Ready-only egress batch; keeps no packet buffers of its own.
pub(super) struct ReadyBatch {
    packets: usize,
    bytes: usize,
    started: std::time::Instant,
}
impl ReadyBatch {
    pub(super) fn new() -> Self {
        Self {
            packets: 0,
            bytes: 0,
            started: std::time::Instant::now(),
        }
    }
    pub(super) fn allows(&self, next: usize) -> bool {
        self.allows_at(next, self.started.elapsed())
    }
    fn allows_at(&self, next: usize, elapsed: std::time::Duration) -> bool {
        self.packets < 16
            && self.bytes + next <= 64 << 10
            && elapsed < std::time::Duration::from_micros(200)
    }
    pub(super) fn completed(&mut self, bytes: usize) {
        self.packets += 1;
        self.bytes += bytes;
    }
}

pub(super) enum SessionDataEvent<Read, Receive, Sent, Written> {
    TunRead(Read),
    TunnelReceive(Receive),
    Sent(Sent),
    Written(Written),
    Tick,
    PreparationError(std::io::Error),
}

pub(super) async fn next_session_data<Read, Receive, Sent, Written>(
    read: impl Future<Output = Read>,
    receive: impl Future<Output = Receive>,
    send: impl Future<Output = Sent>,
    write: impl Future<Output = Written>,
    tick: impl Future,
) -> SessionDataEvent<Read, Receive, Sent, Written> {
    // No data direction can monopolize the pump. The caller gives cancellation
    // and reconfiguration priority in its outer select.
    tokio::select! {
        value = read => SessionDataEvent::TunRead(value),
        value = receive => SessionDataEvent::TunnelReceive(value),
        value = send => SessionDataEvent::Sent(value),
        value = write => SessionDataEvent::Written(value),
        _ = tick => SessionDataEvent::Tick,
    }
}

pub(super) async fn wait_pending<F: Future>(pending: Pin<&mut Option<F>>) -> F::Output {
    match pending.as_pin_mut() {
        Some(send) => send.await,
        None => std::future::pending().await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ready_batches_bound_packets_bytes_and_soft_time_for_every_mtu() {
        use std::time::Duration;
        for mtu in [1280, 1500, 4096, 9000] {
            let mut batch = ReadyBatch::new();
            while batch.allows_at(mtu, Duration::ZERO) {
                batch.completed(mtu);
            }
            assert!(batch.packets <= 16 && batch.bytes <= 64 << 10);
            assert!(!batch.allows_at(1, Duration::from_micros(200)));
        }
        assert!(!ReadyBatch::new().allows(65537));
    }
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    #[test]
    fn preparation_errors_remain_visible_to_the_session_owner() {
        let event: SessionDataEvent<(), (), (), ()> = SessionDataEvent::PreparationError(
            std::io::Error::other("packet allocation unavailable"),
        );
        let SessionDataEvent::PreparationError(error) = event else {
            panic!("preparation failure");
        };
        assert_eq!(error.kind(), std::io::ErrorKind::Other);
    }

    #[tokio::test]
    async fn saturated_send_preserves_one_future_while_receive_and_ticks_continue() {
        let (tx, mut rx) = tokio::sync::mpsc::channel(1);
        tx.send(1).await.unwrap();
        let starts = Arc::new(AtomicUsize::new(0));
        let seen = starts.clone();
        let mut pending = std::pin::pin!(Some(async move {
            seen.fetch_add(1, Ordering::Relaxed);
            tx.send(2).await
        }));
        for _ in 0..3 {
            let event = next_session_data(
                std::future::pending::<()>(),
                std::future::ready(9),
                wait_pending(pending.as_mut()),
                std::future::pending::<()>(),
                std::future::pending::<()>(),
            )
            .await;
            assert!(matches!(event, SessionDataEvent::TunnelReceive(9)));
            let event = next_session_data(
                std::future::pending::<()>(),
                std::future::pending::<()>(),
                wait_pending(pending.as_mut()),
                std::future::pending::<()>(),
                std::future::ready(()),
            )
            .await;
            assert!(matches!(event, SessionDataEvent::Tick));
        }
        assert_eq!(rx.recv().await, Some(1));
        assert!(wait_pending(pending.as_mut()).await.is_ok());
        pending.set(None);
        assert_eq!(starts.load(Ordering::Relaxed), 1);
        assert_eq!(rx.recv().await, Some(2));
        assert_eq!(rx.recv().await, None);
    }

    #[tokio::test]
    async fn blocked_tun_write_does_not_stop_reads_or_control_cancellation() {
        let event = next_session_data(
            std::future::ready(5),
            std::future::pending::<()>(),
            std::future::pending::<()>(),
            std::future::pending::<()>(),
            std::future::pending::<()>(),
        )
        .await;
        assert!(matches!(event, SessionDataEvent::TunRead(5)));
        let cancel = tokio_util::sync::CancellationToken::new();
        cancel.cancel();
        tokio::select! {
            biased;
            _ = cancel.cancelled() => {},
            _ = next_session_data(
                std::future::pending::<()>(), std::future::pending::<()>(),
                std::future::pending::<()>(), std::future::pending::<()>(),
                std::future::pending::<()>(),
            ) => panic!("cancellation must remain responsive"),
        }
    }
}
