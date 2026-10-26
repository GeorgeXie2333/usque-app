//! Production pump, scripted migration replies, and memory-only packet queues.
use super::*;
use crate::h3::regression_tests::scripted_tunnel_with_generation;

struct Physical(AtomicU64);
impl SocketProtector for Physical {
    fn protect(&self, _: crate::socket::SocketHandle) -> Result<(), String> {
        panic!("scripted migration must not open system sockets")
    }
    fn network_generation(&self) -> Option<u64> {
        Some(self.0.load(Ordering::SeqCst))
    }
}

#[tokio::test(start_paused = true)]
async fn pump_publishes_only_confirmed_migrations_and_blocks_superseded_paths() {
    let (tunnel, mut actor) = scripted_tunnel_with_generation(Some(1));
    let physical = Arc::new(Physical(AtomicU64::new(1)));
    let network_path = ActiveNetworkPath::default();
    let cancel = CancellationToken::new();
    let path = runtime_path(Transport::Http3, AddressFamily::Ipv4);
    let (health_tx, health) = watch::channel(RuntimeHealth::Connected {
        path,
        reconnect_count: 0,
    });
    let (control_tx, _control) = watch::channel(PeerNetworkState::default());
    let (_packets, outgoing) = tests::test_packet_channel(QueueKind::H3DatagramSend, 1);
    let (incoming, _received) = tests::test_batch_channel(QueueKind::H3WireSend, 1);
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
    let task = tokio::spawn({
        let physical = physical.clone();
        let network_path = network_path.clone();
        let cancel = cancel.clone();
        async move {
            pump_active_tunnel(
                MasqueTunnel::Http3(Box::new(tunnel)),
                path,
                0,
                &mut PacketIo::Channel {
                    outgoing,
                    incoming,
                    buffered_outgoing: None,
                },
                &Profile::default(),
                identity,
                physical,
                &cancel,
                Arc::new(TrafficCounters::default()),
                &control_tx,
                &health_tx,
                &ConnectionTelemetry::default(),
                0,
                Some(1),
                None,
                &network_path,
            )
            .await
        }
    });
    tokio::task::yield_now().await;
    let first = network_path.generation(Some(1)).unwrap();
    physical.0.store(2, Ordering::SeqCst);
    assert_eq!(network_path.generation(Some(2)), None);
    let (generation, reply) = actor.migration_request().await;
    assert_eq!(generation, 2);
    assert_eq!(network_path.generation(Some(1)), Some(first));
    // A newer platform notification arrives before the previous promotion ack.
    physical.0.store(3, Ordering::SeqCst);
    reply
        .send(H3MigrationResult::Promoted {
            network_generation: 2,
        })
        .unwrap();
    let (generation, reply) = actor.migration_request().await;
    assert_eq!(generation, 3);
    let second = network_path.generation(Some(2)).unwrap();
    assert_ne!(first, second);
    assert_eq!(network_path.generation(Some(3)), None);
    reply
        .send(H3MigrationResult::Promoted {
            network_generation: 3,
        })
        .unwrap();
    tokio::task::yield_now().await;
    assert_ne!(network_path.generation(Some(3)).unwrap(), second);
    assert_eq!(health.borrow().reconnect_count(), 0);
    cancel.cancel();
    assert!(matches!(task.await.unwrap(), ActiveOutcome::Shutdown));
    assert_eq!(network_path.generation(Some(3)), None);
}
