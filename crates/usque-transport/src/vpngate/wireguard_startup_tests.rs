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
        // A Proton source can never silently become an ordinary WG tunnel when
        // the private session certificate/agent capability is absent.
        (ChainSource::ProtonVpn, 0, None),
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
            let expected = if source == ChainSource::ProtonVpn {
                GateFailure::Configuration
            } else {
                GateFailure::Transport
            };
            assert!(matches!(
                result,
                Err(TransportError::VpnGate(reason)) if reason == expected
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

#[tokio::test]
async fn native_ready_queued_before_warp_loss_keeps_the_final_packet_runtime_alive() {
    let path = RuntimePath {
        transport: usque_core::Transport::Http3,
        endpoint_family: usque_core::AddressFamily::Ipv4,
        ipv4_available: true,
        ipv6_available: false,
    };
    let (mut outer, underlay) =
        ManagedTunnelRuntime::for_external_packets(path, Default::default());
    let profile = profile(ChainSource::WireguardCustom);
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
        Arc::default(),
    )
    .await
    .unwrap();
    let warp = InternalNetwork::for_stack(&outer_profile, &stack, addresses.0, addresses.1);
    let remote = "192.0.2.1:51820".parse().unwrap();
    let mut native = Session::start(&profile, remote).unwrap();
    let (transport_output, ip_output) = native.split_packet_outputs().unwrap();
    let cancellation = CancellationToken::new();
    let (status, _) = watch::channel(GateStatus::default());
    let (_admission, admitted) = watch::channel(true);
    let network = FinalNetworkParameters {
        ipv4: Some("10.8.0.2".parse().unwrap()),
        ipv6: None,
        mtu: 1280,
        dns_servers: vec![],
    };
    let mut actor = Actor {
        diagnostic_id: 0,
        connection_deadline: Instant::now() + Duration::from_secs(35),
        startup_attempt: StartupAttempt::Once,
        native,
        transport_output,
        ip_output,
        packet_tasks: Vec::new(),
        remote,
        udp: true,
        tcp_packet_budget: None,
        underlay_health: warp.health(),
        transport_telemetry: Default::default(),
        warp,
        status,
        ready: None,
        channels: None,
        network: Some(network.clone()),
        generation: 1,
        packet_generation: Arc::new(std::sync::atomic::AtomicU64::new(1)),
        packet_ready: Arc::new(std::sync::atomic::AtomicBool::new(true)),
        reconnect_count: 0,
        connected: true,
        connection: None,
        cancellation: cancellation.clone(),
        admitted,
        proton_updates: None,
        proton_recovery: None,
        agent: None,
        agent_route: Arc::default(),
        pending_runtime: None,
    };
    let (mut final_runtime, mut channels) =
        ManagedTunnelRuntime::for_external_packets(path, Default::default());
    actor.start_packet_tasks(&mut channels, &network).unwrap();
    actor.channels = Some(channels);
    underlay.health.send_replace(RuntimeHealth::Reconnecting {
        last_path: path,
        attempt: 1,
        reconnect_count: 0,
        reason: "test".into(),
        failure: TransportError::UnderlyingNetworkChanged.failure(None, None),
    });
    actor
        .event(Event::State {
            generation: 1,
            name: "CONNECTED".into(),
            error: false,
            fatal: false,
        })
        .await
        .unwrap();
    assert!(!actor.packet_ready.load(Ordering::Acquire));
    assert!(!cancellation.is_cancelled());
    assert!(matches!(
        final_runtime.health(),
        RuntimeHealth::Reconnecting { .. }
    ));
    assert!(actor.channels.as_ref().unwrap().failure.borrow().is_none());
    assert!(actor.packet_tasks.iter().all(|task| !task.is_finished()));
    cancellation.cancel();
    actor.native.shutdown().await.unwrap();
    for task in actor.packet_tasks.drain(..) {
        task.await.unwrap();
    }
    final_runtime.shutdown().await;
    stack.shutdown().await;
    outer.shutdown().await;
}
