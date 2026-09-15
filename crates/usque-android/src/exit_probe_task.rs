//! Cancellation ownership for the current session's optional exit lookup.
use std::future::Future;
use std::sync::Mutex;
use tokio_util::sync::CancellationToken;

#[derive(Default)]
pub(super) struct ExitProbeTask {
    cancellation: Mutex<Option<CancellationToken>>,
}

impl ExitProbeTask {
    pub(super) fn begin(&self, session: &CancellationToken) -> CancellationToken {
        let mut current = self.cancellation.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(previous) = current.take() {
            previous.cancel();
        }
        let next = session.child_token();
        *current = Some(next.clone());
        next
    }

    pub(super) fn cancel(&self) {
        if let Some(current) = self
            .cancellation
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
        {
            current.cancel();
        }
    }
}

impl Drop for ExitProbeTask {
    fn drop(&mut self) {
        self.cancel();
    }
}

pub(super) async fn run_probe<T>(
    cancellation: &CancellationToken,
    probe: impl Future<Output = T>,
) -> Option<T> {
    tokio::select! {
        biased;
        _ = cancellation.cancelled() => None,
        result = probe => Some(result),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    use std::time::Duration;

    #[tokio::test]
    async fn replacement_cancels_old_probe_without_cancelling_session() {
        let session = CancellationToken::new();
        let task = ExitProbeTask::default();
        let old = task.begin(&session);
        let current = task.begin(&session);
        assert!(run_probe(&old, std::future::ready(1)).await.is_none());
        assert_eq!(run_probe(&current, std::future::ready(2)).await, Some(2));
        assert!(!session.is_cancelled());
        task.cancel();
        assert!(current.is_cancelled());
        assert!(!session.is_cancelled());
    }

    #[tokio::test]
    async fn session_cancellation_and_owner_drop_cancel_pending_probes() {
        let session = CancellationToken::new();
        let task = ExitProbeTask::default();
        let current = task.begin(&session);
        session.cancel();
        assert!(
            run_probe(&current, std::future::pending::<()>())
                .await
                .is_none()
        );

        let other_session = CancellationToken::new();
        let current = task.begin(&other_session);
        drop(task);
        assert!(
            run_probe(&current, std::future::pending::<()>())
                .await
                .is_none()
        );
        assert!(!other_session.is_cancelled());
    }

    #[tokio::test(start_paused = true)]
    async fn cancellation_during_retry_delay_prevents_more_network_requests() {
        let session = CancellationToken::new();
        let owner = ExitProbeTask::default();
        let cancellation = owner.begin(&session);
        let calls = Arc::new(AtomicUsize::new(0));
        let request_calls = Arc::clone(&calls);
        let worker = tokio::spawn(async move {
            run_probe(
                &cancellation,
                usque_core::exit_probe::probe_exit_with_retry(
                    |_| {
                        request_calls.fetch_add(1, Ordering::SeqCst);
                        std::future::ready(None)
                    },
                    |_| std::future::ready(None),
                ),
            )
            .await
        });
        tokio::task::yield_now().await;
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        owner.cancel();
        assert!(worker.await.unwrap().is_none());
        tokio::time::advance(Duration::from_secs(10)).await;
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }
}
