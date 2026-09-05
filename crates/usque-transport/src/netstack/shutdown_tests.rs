//! Exercise real pump/recovery code with deterministic actor-exit ordering.
use std::future::poll_fn;
use std::task::Poll;

use tokio::sync::oneshot;

use super::*;
use crate::h3::regression_tests::{ClosedTestChannel, tunnel_with_closed_channel};
use crate::recovery_policy::AutoRecoveryPolicy;

const CHANNELS: [ClosedTestChannel; 4] = [
    ClosedTestChannel::Send,
    ClosedTestChannel::Receive,
    ClosedTestChannel::Control,
    ClosedTestChannel::All,
];

async fn pump_fixture(tunnel: MasqueTunnel, cancellation: CancellationToken) -> ActiveOutcome {
    let key = usque_core::MasqueKeyPair::generate();
    let identity = Arc::new(
        MasqueTlsIdentity::new(
            key.private_sec1_der().unwrap(),
            &key.public_spki_der().unwrap(),
            "172.16.0.2".parse().unwrap(),
            "2001:db8::2".parse().unwrap(),
        )
        .unwrap(),
    );
    let (outgoing_tx, outgoing) = super::tests::test_packet_channel(QueueKind::H3DatagramSend, 1);
    let (incoming, _incoming_rx) = super::tests::test_batch_channel(QueueKind::H3WireSend, 1);
    outgoing_tx
        .send(super::tests::test_ipv4_packet(1), 20)
        .await
        .unwrap();
    let mut packet_io = PacketIo::Channel {
        outgoing,
        incoming,
        buffered_outgoing: None,
    };
    let (control_tx, _control_rx) = watch::channel(PeerNetworkState::default());
    let path = runtime_path(Transport::Http3, AddressFamily::Ipv4);
    let (health_tx, _health_rx) = watch::channel(RuntimeHealth::Connected {
        path,
        reconnect_count: 0,
    });
    pump_active_tunnel(
        tunnel,
        path,
        0,
        &mut packet_io,
        &Profile::default(),
        identity,
        crate::socket::noop_socket_protector(),
        &cancellation,
        Arc::new(TrafficCounters::default()),
        &control_tx,
        &health_tx,
        &ConnectionTelemetry::default(),
        0,
        None,
    )
    .await
}

fn reconnect_failure(outcome: ActiveOutcome) -> TransportFailure {
    match outcome {
        ActiveOutcome::Reconnect(failure) => failure,
        _ => panic!("expected a reconnect outcome"),
    }
}

#[tokio::test(start_paused = true)]
async fn h3_shutdown_preserves_typed_failure_before_and_after_driver_completion() {
    for channel in CHANNELS {
        for already_finished in [false, true] {
            for error in [
                TransportError::PmtuRevalidationExhausted,
                TransportError::Http3("peer protocol error".to_owned()),
                TransportError::TunnelClosed,
                TransportError::Http3ConnectRejected(403),
                TransportError::EndpointPinMismatch,
                TransportError::InvalidIdentity,
                TransportError::SocketProtection("refused".to_owned()),
                TransportError::EndpointAssignmentChanged,
                TransportError::MalformedIpPacket,
                TransportError::UnderlyingNetworkChanged,
            ] {
                let expected = error.failure(Some(Transport::Http3), Some(AddressFamily::Ipv4));
                let (finish_tx, finish_rx) = oneshot::channel();
                let (closed_tx, closed_rx) = oneshot::channel();
                let tunnel = tunnel_with_closed_channel(channel, finish_rx, closed_tx);
                closed_rx.await.unwrap();
                let pump = pump_fixture(MasqueTunnel::Http3(tunnel), CancellationToken::new());
                tokio::pin!(pump);
                if already_finished {
                    finish_tx.send(Err(error)).unwrap();
                    tokio::task::yield_now().await;
                } else {
                    for _ in 0..4 {
                        assert!(
                            poll_fn(|cx| Poll::Ready(pump.as_mut().poll(cx)))
                                .await
                                .is_pending(),
                            "{channel:?} closure must wait for the authoritative driver result"
                        );
                    }
                    finish_tx.send(Err(error)).unwrap();
                }
                let failure = reconnect_failure(pump.await);
                assert_eq!(
                    failure, expected,
                    "channel={channel:?}, finished={already_finished}"
                );
                let mut policy = AutoRecoveryPolicy::default();
                let now = Instant::now();
                let immediate = policy.record_failure(&failure, None, Duration::from_secs(1), now);
                assert_eq!(
                    immediate,
                    expected.code == TransportFailureCode::PmtuRevalidationExhausted
                );
                assert_eq!(
                    policy.reconnect_transport(TransportPolicy::Auto, None, now),
                    if immediate {
                        TransportPolicy::Http2
                    } else {
                        TransportPolicy::Auto
                    }
                );
                if expected.fallback_allowed {
                    assert!(policy.record_failure(&failure, None, Duration::from_secs(1), now));
                    assert_eq!(
                        policy.reconnect_transport(TransportPolicy::Auto, None, now),
                        TransportPolicy::Http2
                    );
                }
                if !expected.fallback_allowed {
                    for _ in 0..3 {
                        assert!(!policy.record_failure(&failure, None, Duration::ZERO, now));
                    }
                    assert_eq!(
                        policy.reconnect_transport(TransportPolicy::Auto, None, now),
                        TransportPolicy::Auto
                    );
                }
            }
        }
    }
}

#[tokio::test(start_paused = true)]
async fn h3_clean_driver_exit_is_a_connection_close_not_a_control_rejection() {
    for channel in CHANNELS {
        let (finish_tx, finish_rx) = oneshot::channel();
        let (closed_tx, closed_rx) = oneshot::channel();
        let tunnel = tunnel_with_closed_channel(channel, finish_rx, closed_tx);
        closed_rx.await.unwrap();
        let pump = pump_fixture(MasqueTunnel::Http3(tunnel), CancellationToken::new());
        tokio::pin!(pump);
        assert!(
            poll_fn(|cx| Poll::Ready(pump.as_mut().poll(cx)))
                .await
                .is_pending()
        );
        finish_tx.send(Ok(())).unwrap();
        let failure = reconnect_failure(pump.await);
        assert_eq!(failure.code, TransportFailureCode::H3ConnectionClosed);
        assert!(failure.fallback_allowed);
    }
    let h2_path = runtime_path(Transport::Http2, AddressFamily::Ipv6);
    let failure = reconnect_failure(driver_shutdown_outcome(Ok(()), h2_path));
    assert_eq!(failure.code, TransportFailureCode::H2StreamClosed);
    assert_eq!(failure.transport, Some(Transport::Http2));
    assert_eq!(failure.address_family, Some(AddressFamily::Ipv6));
}

#[tokio::test(start_paused = true)]
async fn h3_shutdown_wait_is_cancellable_and_aborts_the_driver() {
    for channel in CHANNELS {
        let (mut finish_tx, finish_rx) = oneshot::channel();
        let (closed_tx, closed_rx) = oneshot::channel();
        let tunnel = tunnel_with_closed_channel(channel, finish_rx, closed_tx);
        closed_rx.await.unwrap();
        let cancellation = CancellationToken::new();
        let pump = pump_fixture(MasqueTunnel::Http3(tunnel), cancellation.clone());
        tokio::pin!(pump);
        assert!(
            poll_fn(|cx| Poll::Ready(pump.as_mut().poll(cx)))
                .await
                .is_pending()
        );
        cancellation.cancel();
        assert!(matches!(pump.await, ActiveOutcome::Shutdown));
        timeout(Duration::from_secs(1), finish_tx.closed())
            .await
            .unwrap();
    }
}

#[tokio::test(start_paused = true)]
async fn h3_shutdown_wait_has_a_non_fallback_timeout() {
    for channel in CHANNELS {
        let (mut finish_tx, finish_rx) = oneshot::channel();
        let (closed_tx, closed_rx) = oneshot::channel();
        let tunnel = tunnel_with_closed_channel(channel, finish_rx, closed_tx);
        closed_rx.await.unwrap();
        let pump = pump_fixture(MasqueTunnel::Http3(tunnel), CancellationToken::new());
        tokio::pin!(pump);
        for _ in 0..4 {
            assert!(
                poll_fn(|cx| Poll::Ready(pump.as_mut().poll(cx)))
                    .await
                    .is_pending()
            );
        }
        let before = Instant::now();
        tokio::time::advance(PACKET_SEND_TIMEOUT).await;
        let failure = reconnect_failure(pump.await);
        assert_eq!(before.elapsed(), PACKET_SEND_TIMEOUT);
        assert_eq!(failure.code, TransportFailureCode::PacketReceiveStalled);
        assert!(!failure.fallback_allowed);
        timeout(Duration::from_secs(1), finish_tx.closed())
            .await
            .unwrap();
    }
}
