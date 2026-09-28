use super::*;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use usque_core::chain_exit::{ChainSource, ImportSecrets, ValidatedProfile, WireGuardProfile};

fn profile(source: ChainSource) -> PreparedProfile {
    let config = ValidatedProfile::WireGuard(WireGuardProfile {
        private_key: zeroize::Zeroizing::new([1; 32]),
        public_key: boringtun::x25519::PublicKey::from(&boringtun::x25519::StaticSecret::from(
            [2; 32],
        ))
        .to_bytes(),
        preshared_key: None,
        endpoint: usque_core::chain_exit::Endpoint::parse("192.0.2.1", "51820", 0).unwrap(),
        addresses: vec!["10.8.0.2/32".parse().unwrap()],
        dns_servers: vec![],
        allowed_ips: vec!["0.0.0.0/0".parse().unwrap()],
        mtu: 1280,
        keepalive: None,
    });
    let mut summary = config
        .summary("test", uuid::Uuid::new_v4(), uuid::Uuid::new_v4())
        .unwrap();
    summary.source = source;
    PreparedProfile::imported(summary, config, ImportSecrets::default())
}

#[tokio::test(start_paused = true)]
async fn warp_retries_use_each_budget_and_stop_on_success_or_exhaustion() {
    for succeed_at in 0..=6 {
        let calls = AtomicUsize::new(0);
        let started = Instant::now();
        let result = with_startup_retries(
            &CancellationToken::new(),
            started + Duration::from_secs(180),
            StartupAttempt::for_profile(&profile(ChainSource::WarpWireguard)),
            |attempt| {
                let index = calls.fetch_add(1, Ordering::SeqCst);
                async move {
                    assert_eq!(attempt.timeout().as_secs(), [3, 4, 5, 5, 5, 5][index]);
                    if index == succeed_at {
                        return Ok(index);
                    }
                    tokio::time::sleep(attempt.timeout()).await;
                    // Cleanup is awaited before the next attempt's budget starts.
                    tokio::time::sleep(Duration::from_secs(1)).await;
                    Err(TransportError::VpnGate(GateFailure::Transport))
                }
            },
        )
        .await;
        assert_eq!(calls.load(Ordering::SeqCst), (succeed_at + 1).min(6));
        assert_eq!(
            started.elapsed().as_secs(),
            [3, 4, 5, 5, 5, 5][..succeed_at].iter().sum::<u64>() + succeed_at as u64
        );
        if succeed_at < 6 {
            assert_eq!(result.unwrap(), succeed_at);
        } else {
            assert!(matches!(
                result,
                Err(TransportError::VpnGate(GateFailure::Transport))
            ));
        }
    }
}

#[tokio::test(start_paused = true)]
async fn cancellation_deadline_and_terminal_failures_prevent_further_attempts() {
    for reason in [
        GateFailure::Authentication,
        GateFailure::Certificate,
        GateFailure::Configuration,
        GateFailure::Cleanup,
        GateFailure::Protocol,
    ] {
        let calls = AtomicUsize::new(0);
        let result = with_startup_retries(
            &CancellationToken::new(),
            Instant::now() + Duration::from_secs(180),
            StartupAttempt::WarpWireguard(0),
            |_| {
                calls.fetch_add(1, Ordering::SeqCst);
                std::future::ready(Err::<(), _>(TransportError::VpnGate(reason)))
            },
        )
        .await;
        assert!(matches!(result, Err(TransportError::VpnGate(actual)) if actual == reason));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
    for cancel_after_failure in [false, true] {
        let cancel = CancellationToken::new();
        let calls = AtomicUsize::new(0);
        let result = with_startup_retries(
            &cancel,
            Instant::now() + Duration::from_secs(2),
            StartupAttempt::WarpWireguard(0),
            |_| {
                calls.fetch_add(1, Ordering::SeqCst);
                let cancel = &cancel;
                async move {
                    tokio::time::sleep(Duration::from_secs(2)).await;
                    if cancel_after_failure {
                        cancel.cancel();
                    }
                    Err::<(), _>(TransportError::VpnGate(GateFailure::Transport))
                }
            },
        )
        .await;
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        if cancel_after_failure {
            assert!(matches!(result, Err(TransportError::TunnelClosed)));
        } else {
            assert!(matches!(
                result,
                Err(TransportError::VpnGate(GateFailure::Transport))
            ));
        }
    }
}

#[test]
fn only_retryable_startup_failures_are_hidden() {
    for attempt in 0..6 {
        let (status, observed) = watch::channel(GateStatus {
            stage: GateStage::Negotiating,
            ..Default::default()
        });
        publish_failure(
            &status,
            GateFailure::Transport,
            Some(StartupAttempt::WarpWireguard(attempt)),
        );
        assert_eq!(observed.has_changed().unwrap(), attempt == 5);
    }
    let (status, observed) = watch::channel(GateStatus::default());
    publish_failure(&status, GateFailure::Transport, None);
    assert_eq!(observed.borrow().stage, GateStage::Error);
}

// Exercise the real driver, WireGuard worker, private UDP socket and cleanup.
// The outer MASQUE network is represented only by memory packet queues.
#[tokio::test(start_paused = true)]
async fn real_driver_restarts_the_schedule_and_preserves_custom_wireguard_timeout() {
    let path = RuntimePath {
        transport: usque_core::Transport::Http3,
        endpoint_family: usque_core::AddressFamily::Ipv4,
        ipv4_available: true,
        ipv6_available: false,
    };
    let (mut outer, _channels) =
        ManagedTunnelRuntime::for_external_packets(path, crate::NetworkQualityTelemetry::default());
    let outer_profile = usque_core::Profile::default();
    let addresses = (
        "172.16.0.2".parse().unwrap(),
        std::net::Ipv6Addr::UNSPECIFIED,
    );
    let (mut stack, _pipe) = crate::netstack::PacketStack::start_detached(
        &outer_profile,
        addresses,
        &outer.monitor(),
        &CancellationToken::new(),
        crate::socket::noop_socket_protector(),
        Arc::new(crate::geo_direct::GeoDirectPolicy::disabled()),
    )
    .await
    .unwrap();
    let warp = InternalNetwork::for_stack(&outer_profile, &stack, addresses.0, addresses.1);
    for (source, seconds, cancel_after) in [
        (ChainSource::WarpWireguard, 27, None),
        (ChainSource::WarpWireguard, 27, None), // Fresh connection/recovery resets the sequence.
        (ChainSource::WireguardCustom, 35, None),
        (ChainSource::WarpWireguard, 2, Some(2)),
        (ChainSource::WarpWireguard, 4, Some(4)), // Cancel during a retry.
    ] {
        let cancel = CancellationToken::new();
        let (status, observed) = watch::channel(GateStatus::default());
        let started = Instant::now();
        let prepared = profile(source);
        let connect = GateDriver::start(
            &prepared,
            warp.clone(),
            crate::NetworkQualityTelemetry::default(),
            Some(status),
            &cancel,
            started + Duration::from_secs(180),
        );
        let stop = async {
            if let Some(seconds) = cancel_after {
                tokio::time::sleep(Duration::from_secs(seconds)).await;
                assert_ne!(observed.borrow().stage, GateStage::Error);
                cancel.cancel();
            }
        };
        let (result, ()) = tokio::join!(connect, stop);
        assert_eq!(started.elapsed(), Duration::from_secs(seconds));
        if cancel_after.is_some() {
            assert!(matches!(result, Err(TransportError::TunnelClosed)));
        } else {
            assert!(matches!(
                result,
                Err(TransportError::VpnGate(GateFailure::Transport))
            ));
            assert_eq!(observed.borrow().stage, GateStage::Error);
            assert_eq!(observed.borrow().network, None);
        }
        assert!(matches!(
            warp.health_snapshot(),
            RuntimeHealth::Connected { .. }
        ));
    }
    stack.shutdown().await;
    outer.shutdown().await;
}
