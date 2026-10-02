//! Exercise the production actor's select policy with continuously ready work.
use super::*;

#[derive(Debug, Default)]
struct WorkCounts {
    received: usize,
    sent: usize,
    timers: usize,
    cancelled_at: Option<usize>,
}

impl WorkCounts {
    fn total(&self) -> usize {
        self.received + self.sent + self.timers
    }
}

async fn drive_ready_work(
    cancellation: &CancellationToken,
    counts: &mut WorkCounts,
    turns: usize,
    cancel_after_receive: bool,
) -> Result<(), TransportError> {
    let (receive_tx, mut receive_rx) = mpsc::channel(1);
    receive_tx.try_send(()).unwrap();
    for _ in 0..turns {
        // Refill receive capacity immediately, so it competes on every turn
        // with due wire work and an expired timer. No OS socket or QUIC state
        // is needed to exercise the actor's actual selection policy.
        select_h3_actor_work! { cancellation;
            Some(()) = receive_rx.recv() => {
                counts.received += 1;
                receive_tx.try_send(()).unwrap();
                if cancel_after_receive {
                    counts.cancelled_at = Some(counts.total());
                    cancellation.cancel();
                }
            }
            _ = std::future::ready(()) => counts.sent += 1,
            _ = sleep_until(Instant::now() - Duration::from_millis(1)) => counts.timers += 1,
        }
    }
    Ok(())
}

#[tokio::test]
async fn sustained_receive_readiness_does_not_starve_due_wire_or_timer_work() {
    let cancellation = CancellationToken::new();
    let mut counts = WorkCounts::default();
    drive_ready_work(&cancellation, &mut counts, 1024, false)
        .await
        .unwrap();
    assert_eq!(counts.total(), 1024);
    assert!(counts.received > 0, "{counts:?}");
    assert!(counts.sent > 0, "{counts:?}");
    assert!(counts.timers > 0, "{counts:?}");
}

#[tokio::test]
async fn cancellation_precedes_already_ready_actor_work() {
    let cancellation = CancellationToken::new();
    cancellation.cancel();
    let mut counts = WorkCounts::default();
    assert!(matches!(
        drive_ready_work(&cancellation, &mut counts, 1024, false).await,
        Err(TransportError::TunnelClosed)
    ));
    assert_eq!(counts.total(), 0);
}

#[tokio::test]
async fn cancellation_after_receive_stops_before_another_ready_branch() {
    let cancellation = CancellationToken::new();
    let mut counts = WorkCounts::default();
    assert!(matches!(
        drive_ready_work(&cancellation, &mut counts, 1024, true).await,
        Err(TransportError::TunnelClosed)
    ));
    assert_eq!(counts.received, 1);
    assert_eq!(counts.cancelled_at, Some(counts.total()));
}

#[tokio::test]
async fn cancellation_wakes_an_actor_waiting_for_work() {
    let cancellation = CancellationToken::new();
    let task_cancellation = cancellation.clone();
    let task = tokio::spawn(async move {
        select_h3_actor_work! { &task_cancellation;
            _ = std::future::pending::<()>() => {},
        }
        Ok::<(), TransportError>(())
    });
    tokio::task::yield_now().await;
    cancellation.cancel();
    assert!(matches!(
        timeout(Duration::from_secs(1), task)
            .await
            .unwrap()
            .unwrap(),
        Err(TransportError::TunnelClosed)
    ));
}
