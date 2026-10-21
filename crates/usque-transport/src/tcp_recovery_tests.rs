use super::*;
use crate::netstack::{RuntimeHealth, RuntimePath};
use std::time::Duration;

fn connected(epoch: u32) -> RuntimeHealth {
    RuntimeHealth::Connected {
        path: RuntimePath {
            transport: usque_core::Transport::Http3,
            endpoint_family: usque_core::AddressFamily::Ipv4,
            ipv4_available: true,
            ipv6_available: true,
        },
        reconnect_count: epoch,
    }
}

fn recovering() -> RuntimeHealth {
    RuntimeHealth::Connecting {
        requested: connected(0).path(),
        attempt: 1,
    }
}

#[tokio::test(start_paused = true)]
async fn requests_wait_thirty_minutes_then_continue_once() {
    let (sender, health) = tokio::sync::watch::channel(recovering());
    let cancel = CancellationToken::new();
    let task = tokio::spawn(async move {
        with_ready_budget(
            &health,
            &cancel,
            Duration::from_secs(10),
            |_| async { Ok::<_, DialError>(7) },
            |error| error,
        )
        .await
    });
    tokio::task::yield_now().await;
    tokio::time::advance(Duration::from_secs(1_801)).await;
    assert!(!task.is_finished());
    sender.send_replace(connected(1));
    assert_eq!(task.await.unwrap(), Ok(7));
}

#[tokio::test(start_paused = true)]
async fn recovery_pauses_the_remaining_budget_without_resetting_it() {
    let (sender, health) = tokio::sync::watch::channel(connected(0));
    let attempts = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let observed = attempts.clone();
    let task = tokio::spawn(async move {
        with_ready_budget(
            &health,
            &CancellationToken::new(),
            Duration::from_secs(10),
            |_| {
                attempts.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                async {
                    tokio::time::sleep(Duration::from_secs(8)).await;
                    Ok::<_, DialError>(())
                }
            },
            |error| error,
        )
        .await
    });
    tokio::task::yield_now().await;
    tokio::time::advance(Duration::from_secs(4)).await;
    sender.send_replace(recovering());
    tokio::task::yield_now().await;
    tokio::time::advance(Duration::from_secs(7_200)).await;
    assert!(!task.is_finished());
    sender.send_replace(connected(1));
    let resumed = Instant::now();
    assert_eq!(task.await.unwrap(), Err(DialError::Timeout));
    assert_eq!(resumed.elapsed(), Duration::from_secs(6));
    assert_eq!(observed.load(std::sync::atomic::Ordering::Relaxed), 2);
}

#[tokio::test(start_paused = true)]
async fn stop_wins_over_a_simultaneously_ready_connection() {
    let (sender, health) = tokio::sync::watch::channel(recovering());
    let cancellation = CancellationToken::new();
    cancellation.cancel();
    sender.send_replace(connected(0));
    let result = with_ready_budget(
        &health,
        &cancellation,
        Duration::from_secs(10),
        |_| async { Ok::<(), DialError>(()) },
        |error| error,
    )
    .await;
    assert_eq!(result, Err(DialError::Cancelled));
}

#[tokio::test(start_paused = true)]
async fn many_waiters_release_counts_on_client_cancellation() {
    let (_sender, health) = tokio::sync::watch::channel(recovering());
    let telemetry = crate::ConnectionTelemetry::default();
    let cancel = CancellationToken::new();
    let mut tasks = tokio::task::JoinSet::new();
    for _ in 0..128 {
        let health = health.clone();
        let cancel = cancel.clone();
        let telemetry = telemetry.clone();
        tasks.spawn(async move {
            with_ready_budget_tracked(
                &health,
                &cancel,
                Duration::from_secs(10),
                Some(&telemetry),
                |_| async { Ok::<(), DialError>(()) },
                |error| error,
            )
            .await
        });
    }
    for _ in 0..10 {
        tokio::task::yield_now().await;
    }
    assert_eq!(telemetry.snapshot().metrics.waiting_requests, 128);
    tokio::time::advance(Duration::from_secs(1_800)).await;
    cancel.cancel();
    while let Some(result) = tasks.join_next().await {
        assert_eq!(result.unwrap(), Err(DialError::Cancelled));
    }
    assert_eq!(telemetry.snapshot().metrics.waiting_requests, 0);
    assert_eq!(telemetry.snapshot().metrics.peak_waiting_requests, 128);
}
