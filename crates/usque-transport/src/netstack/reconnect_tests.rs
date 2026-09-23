use super::*;
use crate::socket::{
    PhysicalNetworkAvailability as Availability, PhysicalNetworkSnapshot, SocketHandle,
};

struct Network(watch::Sender<PhysicalNetworkSnapshot>);
impl SocketProtector for Network {
    fn protect(&self, _: SocketHandle) -> Result<(), String> {
        Ok(())
    }
    fn subscribe_physical_network(&self) -> Option<watch::Receiver<PhysicalNetworkSnapshot>> {
        Some(self.0.subscribe())
    }
}

struct DeniedNetwork {
    network: Network,
    calls: std::sync::atomic::AtomicUsize,
}

impl SocketProtector for DeniedNetwork {
    fn protect(&self, _: SocketHandle) -> Result<(), String> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        Err("test binding denial".into())
    }
    fn subscribe_physical_network(&self) -> Option<watch::Receiver<PhysicalNetworkSnapshot>> {
        self.network.subscribe_physical_network()
    }
}

async fn supervisor(
    error: TransportError,
    protector: Arc<dyn SocketProtector>,
    cancel: CancellationToken,
) -> (
    tokio::task::JoinHandle<()>,
    watch::Receiver<RuntimeHealth>,
    TrackedSender<OutboundPacket>,
) {
    supervisor_with_refresh(error, protector, cancel, None, false).await
}

async fn supervisor_with_refresh(
    error: TransportError,
    protector: Arc<dyn SocketProtector>,
    cancel: CancellationToken,
    pin_refresher: Option<Arc<dyn EndpointPinRefresher>>,
    pin_refresh_attempted: bool,
) -> (
    tokio::task::JoinHandle<()>,
    watch::Receiver<RuntimeHealth>,
    TrackedSender<OutboundPacket>,
) {
    use crate::h3::regression_tests::{ClosedTestChannel, tunnel_with_closed_channel};
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
    let (finish, finished) = tokio::sync::oneshot::channel();
    let (closed, closed_rx) = tokio::sync::oneshot::channel();
    let tunnel = tunnel_with_closed_channel(ClosedTestChannel::All, finished, closed);
    closed_rx.await.unwrap();
    finish.send(Err(error)).unwrap();
    let mut profile = Profile {
        transport: TransportPolicy::Http3,
        ip_policy: IpPolicy::Ipv4Only,
        ..Profile::default()
    };
    profile.endpoint.ipv4 = std::net::Ipv4Addr::LOCALHOST;
    let path = runtime_path(Transport::Http3, AddressFamily::Ipv4);
    let (health_tx, health) = watch::channel(RuntimeHealth::Connected {
        path,
        reconnect_count: 0,
    });
    let (packets, packet_io) = packets();
    let context = SupervisorContext {
        profile,
        identity,
        protector,
        pin_refresher,
        pin_refresh_attempted,
        cancellation: cancel,
        failure_tx: watch::channel(None).0,
        health_tx,
        control_tx: watch::channel(PeerNetworkState::default()).0,
        counters: Arc::new(TrafficCounters::default()),
        telemetry: ConnectionTelemetry::default(),
    };
    (
        tokio::spawn(run_transport_supervisor(
            MasqueTunnel::Http3(tunnel),
            AddressFamily::Ipv4,
            packet_io,
            context,
        )),
        health,
        packets,
    )
}

#[tokio::test(start_paused = true)]
async fn actual_supervisor_stops_on_established_authentication_and_replacement_protection_failure()
{
    for (error, expected, calls) in [
        (
            TransportError::Http3ConnectRejected(403),
            TransportFailureCode::AuthenticationFailed,
            0,
        ),
        (
            TransportError::TunnelClosed,
            TransportFailureCode::SocketProtectionFailed,
            1,
        ),
    ] {
        let protector = Arc::new(DeniedNetwork {
            network: Network(watch::channel(snapshot(1, Availability::Unknown)).0),
            calls: 0.into(),
        });
        let (task, health, _packets) =
            supervisor(error, protector.clone(), CancellationToken::new()).await;
        timeout(Duration::from_secs(5), task)
            .await
            .unwrap()
            .unwrap();
        assert!(
            matches!(&*health.borrow(), RuntimeHealth::Failed { failure, reconnect_count, .. }
            if failure.code == expected && !failure.retryable && *reconnect_count == calls)
        );
        tokio::time::advance(Duration::from_secs(300)).await;
        assert_eq!(protector.calls.load(Ordering::Relaxed), calls as usize);
    }
}

#[tokio::test(start_paused = true)]
async fn actual_supervisor_waits_offline_without_attempts_and_resumes_on_a_usable_network() {
    let protector = Arc::new(DeniedNetwork {
        network: Network(watch::channel(snapshot(1, Availability::Offline)).0),
        calls: 0.into(),
    });
    let (mut task, health, _packets) = supervisor(
        TransportError::TunnelClosed,
        protector.clone(),
        CancellationToken::new(),
    )
    .await;
    assert!(timeout(Duration::from_secs(300), &mut task).await.is_err());
    assert_eq!(protector.calls.load(Ordering::Relaxed), 0);
    assert!(matches!(
        &*health.borrow(),
        RuntimeHealth::Reconnecting {
            reconnect_count: 0,
            ..
        }
    ));
    let now = Instant::now();
    protector.network.0.send_replace(snapshot(
        2,
        Availability::Online {
            ipv4: true,
            ipv6: false,
        },
    ));
    timeout(Duration::from_secs(1), task)
        .await
        .unwrap()
        .unwrap();
    assert!(now.elapsed() < Duration::from_secs(1));
    assert_eq!(protector.calls.load(Ordering::Relaxed), 1);
}

fn snapshot(generation: u64, availability: Availability) -> PhysicalNetworkSnapshot {
    PhysicalNetworkSnapshot {
        generation,
        availability,
    }
}

#[tokio::test(start_paused = true)]
async fn actual_supervisor_refreshes_a_pin_only_once_and_never_refreshes_authentication() {
    struct CountingRefresh(std::sync::atomic::AtomicUsize);
    #[async_trait::async_trait]
    impl EndpointPinRefresher for CountingRefresh {
        async fn refresh(
            &self,
            _: Arc<dyn SocketProtector>,
        ) -> Result<MasqueTlsIdentity, TransportError> {
            self.0.fetch_add(1, Ordering::Relaxed);
            Err(TransportError::EndpointPinMismatch)
        }
    }
    for (error, attempted, expected) in [
        (TransportError::EndpointPinMismatch, false, 1),
        (TransportError::EndpointPinMismatch, true, 0),
        (TransportError::Http3ConnectRejected(403), false, 0),
    ] {
        let refresher = Arc::new(CountingRefresh(0.into()));
        let (task, health, _packets) = supervisor_with_refresh(
            error,
            crate::socket::noop_socket_protector(),
            CancellationToken::new(),
            Some(refresher.clone()),
            attempted,
        )
        .await;
        timeout(Duration::from_secs(1), task)
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(&*health.borrow(), RuntimeHealth::Failed { .. }));
        assert_eq!(refresher.0.load(Ordering::Relaxed), expected);
    }
}

fn packets() -> (TrackedSender<OutboundPacket>, PacketIo) {
    let (sender, outgoing) = tests::test_packet_channel(QueueKind::TransportOutgoingPackets, 1);
    let (incoming, _) = tests::test_batch_channel(QueueKind::H3WireSend, 1);
    (
        sender,
        PacketIo::Channel {
            outgoing,
            incoming,
            buffered_outgoing: None,
        },
    )
}

#[tokio::test(start_paused = true)]
async fn offline_wait_does_not_dial_and_online_event_bypasses_long_backoff() {
    let network = Network(watch::channel(snapshot(1, Availability::Offline)).0);
    let mut schedule = reconnect::ReconnectSchedule::new(&network, IpPolicy::Auto);
    let (_packets, mut packets) = packets();
    let cancel = CancellationToken::new();
    let wait = schedule.wait(Duration::from_secs(30), &mut packets, &cancel);
    tokio::pin!(wait);
    assert!(timeout(Duration::from_secs(300), &mut wait).await.is_err());
    network.0.send_replace(snapshot(
        2,
        Availability::Online {
            ipv4: true,
            ipv6: false,
        },
    ));
    let now = Instant::now();
    assert_eq!(wait.await, Some(true));
    assert_eq!(now.elapsed(), Duration::from_millis(250));
}

#[tokio::test(start_paused = true)]
async fn unknown_and_closed_observer_keep_bounded_timed_retries() {
    for initial in [Availability::Unknown, Availability::Offline] {
        let network = Network(watch::channel(snapshot(1, initial)).0);
        let mut schedule = reconnect::ReconnectSchedule::new(&network, IpPolicy::Auto);
        drop(network);
        let (_packets, mut packets) = packets();
        let now = Instant::now();
        assert_eq!(
            schedule
                .wait(
                    Duration::from_secs(2),
                    &mut packets,
                    &CancellationToken::new()
                )
                .await,
            Some(false)
        );
        assert_eq!(now.elapsed(), Duration::from_secs(2));
    }
}

#[tokio::test(start_paused = true)]
async fn cancellation_wins_over_network_and_ready_connection() {
    let network = Network(watch::channel(snapshot(1, Availability::Offline)).0);
    let mut schedule = reconnect::ReconnectSchedule::new(&network, IpPolicy::Auto);
    let (_packets, mut packets) = packets();
    let cancel = CancellationToken::new();
    cancel.cancel();
    network.0.send_replace(snapshot(
        2,
        Availability::Online {
            ipv4: true,
            ipv6: true,
        },
    ));
    assert_eq!(
        schedule.wait(Duration::ZERO, &mut packets, &cancel).await,
        None
    );
    assert!(
        schedule
            .connect(async { Some(Ok(())) }, &cancel)
            .await
            .is_none()
    );
}

#[tokio::test(start_paused = true)]
async fn wrong_family_waits_and_network_flapping_is_rate_limited() {
    let network = Network(
        watch::channel(snapshot(
            1,
            Availability::Online {
                ipv4: true,
                ipv6: false,
            },
        ))
        .0,
    );
    let mut schedule = reconnect::ReconnectSchedule::new(&network, IpPolicy::Ipv6Only);
    let (_packets, mut packets) = packets();
    let cancel = CancellationToken::new();
    assert!(
        timeout(
            Duration::from_secs(50),
            schedule.wait(Duration::ZERO, &mut packets, &cancel)
        )
        .await
        .is_err()
    );
    network.0.send_replace(snapshot(
        2,
        Availability::Online {
            ipv4: false,
            ipv6: true,
        },
    ));
    assert_eq!(
        schedule
            .wait(Duration::from_secs(30), &mut packets, &cancel)
            .await,
        Some(true)
    );
    let started = Instant::now();
    network.0.send_replace(snapshot(
        3,
        Availability::Online {
            ipv4: false,
            ipv6: true,
        },
    ));
    assert_eq!(
        schedule
            .wait(Duration::from_secs(30), &mut packets, &cancel)
            .await,
        Some(true)
    );
    assert_eq!(started.elapsed(), Duration::from_secs(1));
    network.0.send_replace(snapshot(2, Availability::Offline));
    assert_eq!(
        schedule
            .wait(Duration::from_secs(2), &mut packets, &cancel)
            .await,
        Some(false)
    );
}

#[tokio::test(start_paused = true)]
async fn generation_change_cancels_an_obsolete_ready_handshake() {
    let network = Network(
        watch::channel(snapshot(
            1,
            Availability::Online {
                ipv4: true,
                ipv6: true,
            },
        ))
        .0,
    );
    let mut schedule = reconnect::ReconnectSchedule::new(&network, IpPolicy::Auto);
    network.0.send_replace(snapshot(2, Availability::Offline));
    assert!(matches!(
        schedule
            .connect(async { Some(Ok(())) }, &CancellationToken::new())
            .await,
        Some(Err(TransportError::UnderlyingNetworkChanged))
    ));
}

#[tokio::test(start_paused = true)]
async fn pin_refresh_is_interruptible_and_never_dials_after_cancellation() {
    struct BlockedRefresh;
    #[async_trait::async_trait]
    impl EndpointPinRefresher for BlockedRefresh {
        async fn refresh(
            &self,
            _: Arc<dyn SocketProtector>,
        ) -> Result<MasqueTlsIdentity, TransportError> {
            std::future::pending().await
        }
    }
    let key = usque_core::MasqueKeyPair::generate();
    let identity = MasqueTlsIdentity::new(
        key.private_sec1_der().unwrap(),
        &key.public_spki_der().unwrap(),
        "172.16.0.2".parse().unwrap(),
        "2001:db8::2".parse().unwrap(),
    )
    .unwrap();
    let refresher: Arc<dyn EndpointPinRefresher> = Arc::new(BlockedRefresh);
    let protector = Arc::new(DeniedNetwork {
        network: Network(watch::channel(PhysicalNetworkSnapshot::default()).0),
        calls: 0.into(),
    });
    let cancel = CancellationToken::new();
    let (_packets, mut packets) = packets();
    let profile = Profile::default();
    let telemetry = ConnectionTelemetry::default();
    let refresh = refresh_and_retry_connection(
        &profile,
        &identity,
        Some(&refresher),
        protector.clone(),
        RefreshRetryContext {
            packet_io: &mut packets,
            cancellation: &cancel,
            telemetry: &telemetry,
        },
    );
    tokio::pin!(refresh);
    assert!(timeout(Duration::from_secs(1), &mut refresh).await.is_err());
    cancel.cancel();
    assert!(refresh.await.is_none());
    assert_eq!(protector.calls.load(Ordering::Relaxed), 0);
}

#[tokio::test]
async fn terminal_candidate_cancels_the_other_candidate_without_waiting() {
    let result = race_candidates(
        async { Err::<(), _>(TransportError::Http3ConnectRejected(403)) },
        std::future::pending(),
        Duration::ZERO,
        |error| !error.failure(Some(Transport::Http3), None).retryable,
    )
    .await;
    assert!(matches!(
        result,
        Err(CandidateErrors::Terminal(
            TransportError::Http3ConnectRejected(403)
        ))
    ));
    let combined = combine_endpoint_errors(
        "[::1]:443".parse().unwrap(),
        TransportError::EndpointPinMismatch,
        "127.0.0.1:443".parse().unwrap(),
        TransportError::SocketProtection("denied".into()),
    );
    assert!(matches!(combined, TransportError::SocketProtection(_)));
}
