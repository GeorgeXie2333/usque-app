use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

#[tokio::test(start_paused = true)]
async fn authentication_retries_can_succeed_without_visible_errors_or_added_backoff() {
    for rejections in 0..=AUTHENTICATION_RETRIES {
        let cancel = CancellationToken::new();
        let calls = AtomicUsize::new(0);
        let (status, observed) = watch::channel(GateStatus {
            stage: GateStage::Negotiating,
            warp_stage: Some("connected".into()),
            ..Default::default()
        });
        let started = Instant::now();
        let result = with_authentication_retries(&cancel, |attempt| {
            let calls = &calls;
            let status = &status;
            let observed = &observed;
            async move {
                assert_eq!(calls.fetch_add(1, Ordering::SeqCst), usize::from(attempt));
                // Model handshake/cleanup time per attempt. A slow rejected
                // attempt must not consume the next attempt's time allowance.
                tokio::time::sleep(Duration::from_secs(20)).await;
                if attempt < rejections {
                    publish_failure(status, GateFailure::Authentication, Some(attempt));
                    assert!(!observed.has_changed().unwrap());
                    assert_eq!(observed.borrow().stage, GateStage::Negotiating);
                    assert_eq!(observed.borrow().failure, None);
                    assert_eq!(observed.borrow().warp_stage.as_deref(), Some("connected"));
                    Err(TransportError::VpnGate(GateFailure::Authentication))
                } else {
                    Ok(attempt)
                }
            }
        })
        .await
        .unwrap();
        assert_eq!(result, rejections);
        assert_eq!(calls.load(Ordering::SeqCst), usize::from(rejections) + 1);
        assert_eq!(
            started.elapsed(),
            Duration::from_secs(20 * (u64::from(rejections) + 1))
        );
    }
}

#[tokio::test]
async fn second_authentication_rejection_is_terminal_and_visible() {
    let calls = AtomicUsize::new(0);
    let (status, observed) = watch::channel(GateStatus {
        stage: GateStage::Negotiating,
        ..Default::default()
    });
    let result = with_authentication_retries(&CancellationToken::new(), |attempt| {
        calls.fetch_add(1, Ordering::SeqCst);
        publish_failure(&status, event_failure("AUTH_FAILED"), Some(attempt));
        if attempt < AUTHENTICATION_RETRIES {
            assert!(!observed.has_changed().unwrap());
        }
        std::future::ready(Err::<(), _>(TransportError::VpnGate(
            GateFailure::Authentication,
        )))
    })
    .await;
    assert!(matches!(
        result,
        Err(TransportError::VpnGate(GateFailure::Authentication))
    ));
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert_eq!(observed.borrow().stage, GateStage::Error);
    assert_eq!(observed.borrow().failure, Some(GateFailure::Authentication));
}

#[tokio::test]
async fn other_failures_do_not_receive_authentication_retries() {
    for reason in [
        GateFailure::Certificate,
        GateFailure::Configuration,
        GateFailure::Transport,
        GateFailure::AddressChanged,
    ] {
        let calls = AtomicUsize::new(0);
        let (status, observed) = watch::channel(GateStatus::default());
        let result = with_authentication_retries(&CancellationToken::new(), |attempt| {
            calls.fetch_add(1, Ordering::SeqCst);
            publish_failure(&status, reason, Some(attempt));
            std::future::ready(Err::<(), _>(TransportError::VpnGate(reason)))
        })
        .await;
        assert!(matches!(result, Err(TransportError::VpnGate(actual)) if actual == reason));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(observed.borrow().stage, GateStage::Error);
        assert_eq!(observed.borrow().failure, Some(reason));
    }
}

#[tokio::test]
async fn cancellation_after_authentication_rejection_prevents_the_next_attempt() {
    for cancel_after in 0..=AUTHENTICATION_RETRIES {
        let cancel = CancellationToken::new();
        let calls = AtomicUsize::new(0);
        if cancel_after == 0 {
            cancel.cancel();
        }
        let result = with_authentication_retries(&cancel, |attempt| {
            calls.fetch_add(1, Ordering::SeqCst);
            if attempt + 1 == cancel_after {
                cancel.cancel();
            }
            std::future::ready(Err::<(), _>(TransportError::VpnGate(
                GateFailure::Authentication,
            )))
        })
        .await;
        assert!(matches!(result, Err(TransportError::TunnelClosed)));
        assert_eq!(calls.load(Ordering::SeqCst), usize::from(cancel_after));
    }
}

#[test]
fn authentication_failure_after_startup_is_always_visible() {
    let (status, observed) = watch::channel(GateStatus {
        stage: GateStage::Connected,
        network: Some(FinalNetworkParameters {
            ipv4: Some("10.8.0.2".parse().unwrap()),
            ipv6: None,
            mtu: 1500,
            dns_servers: vec![],
        }),
        ..Default::default()
    });
    publish_failure(&status, GateFailure::Authentication, None);
    assert_eq!(observed.borrow().stage, GateStage::Error);
    assert_eq!(observed.borrow().failure, Some(GateFailure::Authentication));
    assert_eq!(observed.borrow().network, None);
    // This local setup policy must not enable generic session recovery.
    assert!(!GateFailure::Authentication.retryable());
}
