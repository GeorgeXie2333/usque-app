use super::*;
use base64::{Engine, engine::general_purpose::STANDARD};
use std::sync::{
    Mutex as StdMutex,
    atomic::{AtomicU64, AtomicUsize, Ordering},
};
use usque_core::{chain_exit::Endpoint, proton_vpn::Node};

fn peer(id: &str, country: &str, state: &str, city: &str, load: u32) -> SelectedNode {
    SelectedNode {
        metadata: Node {
            id: id.into(),
            name: id.into(),
            country: country.into(),
            city: city.into(),
            load,
        },
        state: Some(state.into()),
        domain: "agent.example.invalid".into(),
        endpoint: Endpoint::parse("192.0.2.1", "51820", 0).unwrap(),
        public_key: STANDARD.encode([8u8; 32]),
        label: None,
    }
}
fn scope() -> Scope {
    Scope {
        account: Uuid::from_u128(1),
        catalogue: Uuid::from_u128(2),
    }
}
#[derive(Default)]
struct MockProbe {
    replies: StdMutex<BTreeMap<String, VecDeque<Option<u64>>>>,
    calls: StdMutex<Vec<String>>,
    active: Arc<AtomicUsize>,
    peak: AtomicUsize,
    network: Arc<AtomicU64>,
    path: Option<InternalNetwork>,
}
struct Active(Arc<AtomicUsize>);
impl Drop for Active {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}
impl MockProbe {
    fn replies(&self, id: &str, replies: &[Option<u64>]) {
        self.replies
            .lock()
            .unwrap()
            .insert(id.into(), replies.iter().copied().collect());
    }
    fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }
}
#[async_trait]
impl Probe for MockProbe {
    fn generation(&self) -> Option<Generation> {
        if let Some(path) = &self.path {
            return path.probe_generation();
        }
        Some((1, Some(self.network.load(Ordering::SeqCst))))
    }
    async fn measure(
        &self,
        peer: &SelectedNode,
        _: &CancellationToken,
    ) -> Result<Option<Duration>, Failure> {
        let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
        self.peak.fetch_max(active, Ordering::SeqCst);
        let _active = Active(self.active.clone());
        self.calls.lock().unwrap().push(peer.metadata.id.clone());
        let reply = self
            .replies
            .lock()
            .unwrap()
            .get_mut(&peer.metadata.id)
            .and_then(VecDeque::pop_front)
            .unwrap_or(Some(10));
        match reply {
            Some(ms) => {
                tokio::time::sleep(Duration::from_millis(ms)).await;
                Ok(Some(Duration::from_millis(ms)))
            }
            None => std::future::pending().await,
        }
    }
}

fn tracked_probe() -> (Arc<MockProbe>, crate::netstack::ActiveNetworkPath) {
    struct Physical(Arc<AtomicU64>);
    impl crate::socket::SocketProtector for Physical {
        fn protect(&self, _: crate::socket::SocketHandle) -> Result<(), String> {
            panic!("selection tests must not open system sockets")
        }
        fn network_generation(&self) -> Option<u64> {
            Some(self.0.load(Ordering::SeqCst))
        }
    }
    let physical = Arc::new(AtomicU64::new(1));
    let path = crate::netstack::ActiveNetworkPath::default();
    path.activate(Some(1));
    let (_, health) = tokio::sync::watch::channel(crate::netstack::RuntimeHealth::Connected {
        path: crate::netstack::RuntimePath {
            transport: usque_core::Transport::Http3,
            endpoint_family: usque_core::AddressFamily::Ipv4,
            ipv4_available: true,
            ipv6_available: false,
        },
        reconnect_count: 0,
    });
    let network = InternalNetwork::for_streams(Arc::new(NoTcp), health, CancellationToken::new())
        .with_test_probe_path(Arc::new(Physical(physical.clone())), path.clone());
    (
        Arc::new(MockProbe {
            network: physical,
            path: Some(network),
            ..Default::default()
        }),
        path,
    )
}

#[tokio::test(start_paused = true)]
async fn migration_blocks_cached_and_fresh_selection_then_discards_winner_and_city_history() {
    let selector = Selector::default();
    let (probe, path) = tracked_probe();
    let nodes = vec![
        peer("a", "US", "", "City", 0),
        peer("b", "US", "", "City", 1),
    ];
    probe.replies("a", &[None]);
    let winner = selector
        .select(&nodes, scope(), probe.clone(), &CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(winner.metadata.id, "b");
    assert_eq!(probe.calls(), ["a", "b", "b", "b"]);

    probe.network.store(2, Ordering::SeqCst);
    for selector in [&selector, &Selector::default()] {
        assert_eq!(
            selector
                .select(&nodes, scope(), probe.clone(), &CancellationToken::new())
                .await
                .err()
                .unwrap()
                .reason,
            "underlay_changed"
        );
    }
    assert_eq!(probe.calls().len(), 4);
    path.activate(Some(2));
    let winner = selector
        .select(&nodes, scope(), probe.clone(), &CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(winner.metadata.id, "a");
    // Neither the 30-second winner nor the five-minute representative survives.
    assert_eq!(&probe.calls()[4..], ["a", "a", "a"]);
}

#[tokio::test(start_paused = true)]
async fn migration_during_scan_cancels_and_joins_old_path_samples() {
    let selector = Arc::new(Selector::default());
    let (probe, path) = tracked_probe();
    let nodes: Vec<_> = (0..20)
        .map(|i| peer(&i.to_string(), "US", "", &i.to_string(), 0))
        .collect();
    for node in &nodes {
        probe.replies(&node.metadata.id, &[None]);
    }
    let task = tokio::spawn({
        let selector = selector.clone();
        let probe = probe.clone();
        async move {
            selector
                .select(&nodes, scope(), probe, &CancellationToken::new())
                .await
        }
    });
    while probe.calls().len() < 8 {
        tokio::task::yield_now().await;
    }
    probe.network.store(2, Ordering::SeqCst);
    // Even promotion before the next poll cannot hide the path change.
    path.activate(Some(2));
    assert_eq!(
        task.await.unwrap().err().unwrap().reason,
        "underlay_changed"
    );
    assert_eq!(probe.active.load(Ordering::SeqCst), 0);
    let cache = selector.cache.lock().await;
    assert!(cache.winner.is_none());
    assert!(cache.observations.is_empty());
}

#[tokio::test(start_paused = true)]
async fn small_catalogue_still_covers_cities_instead_of_probing_every_node() {
    let probe = Arc::new(MockProbe::default());
    let nodes = vec![
        peer("a", "US", "California", " Los   Angeles ", 5),
        peer("same-city", "US", " california ", "los angeles", 10),
        peer("different-state", "US", "Other", "Los Angeles", 5),
        peer("different-country", "CA", "California", "Los Angeles", 5),
        peer("unknown-1", "US", "", "", 5),
        peer("unknown-2", "US", "", "  ", 5),
    ];
    Selector::default()
        .select(&nodes, scope(), probe.clone(), &CancellationToken::new())
        .await
        .unwrap();
    let calls = probe.calls();
    for id in [
        "a",
        "different-state",
        "different-country",
        "unknown-1",
        "unknown-2",
    ] {
        assert_eq!(
            calls.iter().filter(|value| value.as_str() == id).count(),
            3,
            "{id}"
        );
    }
    assert!(!calls.iter().any(|value| value == "same-city"));
    assert_eq!(nodes.len(), 6);
}

#[tokio::test(start_paused = true)]
async fn every_city_is_covered_and_parallelism_is_bounded() {
    for (configured, expected) in [(8, 8), (100, 16)] {
        let probe = Arc::new(MockProbe::default());
        let nodes: Vec<_> = (0..140)
            .map(|i| peer(&format!("node-{i:03}"), "US", "", &format!("City {i}"), 0))
            .collect();
        let selector = Selector {
            concurrency: configured,
            ..Default::default()
        };
        selector
            .select(&nodes, scope(), probe.clone(), &CancellationToken::new())
            .await
            .unwrap();
        let calls = probe.calls();
        assert!(nodes.iter().all(|node| calls.contains(&node.metadata.id)));
        assert_eq!(calls.len(), 140 + 10);
        assert_eq!(probe.peak.load(Ordering::SeqCst), expected);
        assert_eq!(probe.active.load(Ordering::SeqCst), 0);
    }
}

#[tokio::test(start_paused = true)]
async fn failed_representative_uses_same_city_backup_after_other_cities() {
    let probe = Arc::new(MockProbe::default());
    probe.replies("failed", &[None]);
    probe.replies("backup", &[Some(5); 3]);
    let nodes = vec![
        peer("failed", "US", "", "A", 1),
        peer("backup", "US", "", "A", 2),
        peer("not-needed", "US", "", "A", 3),
        peer("other-city", "US", "", "B", 1),
    ];
    let winner = Selector::default()
        .select(&nodes, scope(), probe.clone(), &CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(winner.metadata.id, "backup");
    let calls = probe.calls();
    assert!(
        calls.iter().position(|id| id == "other-city") < calls.iter().position(|id| id == "backup")
    );
    assert!(!calls.iter().any(|id| id == "not-needed"));
}

#[tokio::test(start_paused = true)]
async fn median_confirmation_rejects_a_lucky_fast_sample() {
    let probe = Arc::new(MockProbe::default());
    probe.replies("unstable", &[Some(1), Some(200), Some(200)]);
    probe.replies("stable", &[Some(30); 3]);
    let nodes = vec![
        peer("unstable", "US", "", "A", 0),
        peer("stable", "US", "", "B", 99),
    ];
    let winner = Selector::default()
        .select(&nodes, scope(), probe, &CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(winner.metadata.id, "stable");
}

#[tokio::test(start_paused = true)]
async fn an_unconfirmed_sixth_candidate_cannot_win_against_confirmed_medians() {
    let probe = Arc::new(MockProbe::default());
    let nodes: Vec<_> = (1..=6)
        .map(|i| peer(&i.to_string(), "US", "", &i.to_string(), 0))
        .collect();
    for i in 1..=6 {
        probe.replies(&i.to_string(), &[Some(i), Some(20 + i), Some(20 + i)]);
    }
    let winner = Selector::default()
        .select(&nodes, scope(), probe.clone(), &CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(winner.metadata.id, "1");
    assert_eq!(
        probe.calls().iter().filter(|id| id.as_str() == "6").count(),
        1
    );
}

#[tokio::test(start_paused = true)]
async fn timeouts_do_not_become_zero_latency_or_a_cached_success() {
    let selector = Selector::default();
    let probe = Arc::new(MockProbe::default());
    let nodes: Vec<_> = (0..4)
        .map(|i| peer(&format!("node-{i}"), "US", "", "Same", i))
        .collect();
    for node in &nodes {
        probe.replies(&node.metadata.id, &[None, None]);
    }
    for _ in 0..2 {
        let winner = selector
            .select(&nodes, scope(), probe.clone(), &CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(winner.metadata.id, "node-0");
        assert!(selector.cache.lock().await.winner.is_none());
    }
    assert_eq!(
        probe.calls(),
        ["node-0", "node-1", "node-2", "node-0", "node-1", "node-2"]
    );
}

#[tokio::test(start_paused = true)]
async fn winner_expires_but_recent_city_history_prefers_a_working_backup() {
    let selector = Selector::default();
    let probe = Arc::new(MockProbe::default());
    probe.replies("lowest-load", &[None]);
    let nodes = vec![
        peer("lowest-load", "US", "", "A", 1),
        peer("working", "US", "", "A", 2),
    ];
    selector
        .select(&nodes, scope(), probe.clone(), &CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(probe.calls().len(), 4);
    let winner = selector
        .select(&nodes, scope(), probe.clone(), &CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(winner.metadata.id, "working");
    assert_eq!(probe.calls().len(), 4);
    tokio::time::advance(Duration::from_secs(30)).await;
    selector
        .select(&nodes, scope(), probe.clone(), &CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(&probe.calls()[4..], ["working", "working", "working"]);
}

#[tokio::test(start_paused = true)]
async fn cache_is_invalidated_by_network_account_catalogue_or_endpoint_changes() {
    let selector = Selector::default();
    let probe = Arc::new(MockProbe::default());
    let mut nodes = vec![peer("node", "US", "", "A", 1)];
    let mut context = scope();
    for step in 0..5 {
        match step {
            1 => {
                probe.network.fetch_add(1, Ordering::SeqCst);
            }
            2 => context.account = Uuid::from_u128(3),
            3 => context.catalogue = Uuid::from_u128(4),
            4 => nodes[0].endpoint.port += 1,
            _ => (),
        }
        selector
            .select(&nodes, context, probe.clone(), &CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(probe.calls().len(), (step + 1) * 3);
    }
}

#[tokio::test(start_paused = true)]
async fn cancellation_and_network_change_join_probes_without_publishing_partial_results() {
    for network_change in [false, true] {
        let selector = Arc::new(Selector::default());
        let probe = Arc::new(MockProbe::default());
        let nodes: Vec<_> = (0..20)
            .map(|i| peer(&i.to_string(), "US", "", &i.to_string(), 0))
            .collect();
        for node in &nodes {
            probe.replies(&node.metadata.id, &[None]);
        }
        let cancel = CancellationToken::new();
        let task = tokio::spawn({
            let selector = selector.clone();
            let probe = probe.clone();
            let cancel = cancel.clone();
            async move { selector.select(&nodes, scope(), probe, &cancel).await }
        });
        while probe.calls().len() < 8 {
            tokio::task::yield_now().await;
        }
        if network_change {
            probe.network.fetch_add(1, Ordering::SeqCst);
        } else {
            cancel.cancel();
        }
        let result = task.await.unwrap();
        assert_eq!(
            result.err().unwrap().reason,
            if network_change {
                "underlay_changed"
            } else {
                "cancelled"
            }
        );
        assert_eq!(probe.active.load(Ordering::SeqCst), 0);
        let cache = selector.cache.lock().await;
        assert!(cache.winner.is_none());
        assert!(cache.observations.is_empty());
    }
}

#[test]
fn udp_packet_matches_an_independent_hmac_vector() {
    let key: Vec<u8> = (0..32).collect();
    let packet = ping::packet(&STANDARD.encode(key), 1_700_000_000).unwrap();
    let hex: String = packet.iter().map(|byte| format!("{byte:02x}")).collect();
    // Python stdlib hmac/hashlib vector over public-key bytes + LE timestamp.
    assert_eq!(
        hex,
        "fe0100f15365550931ae2f45a9e9aea79e93fac32bddf12fc0b41562423906a779a63b5412ab"
    );
    assert!(ping::packet("malformed", 0).is_err());
    assert!(ping::packet(&STANDARD.encode([0u8; 31]), 0).is_err());
}

struct NoTcp;
#[async_trait]
impl crate::tcp::TcpDialer for NoTcp {
    async fn connect(
        &self,
        _: crate::tcp::TcpTarget,
        _: Instant,
        _: &CancellationToken,
        _: crate::tcp::FlowClass,
    ) -> Result<crate::tcp::TcpStream, crate::tcp::DialError> {
        panic!("UDP probes must never use TCP or a system-network fallback")
    }
}

#[tokio::test]
async fn warp_ping_uses_memory_udp_checks_source_and_reply_and_isolates_samples() {
    use crate::{
        dns::QuerySocket,
        netstack::{RuntimeHealth, RuntimePath},
    };
    use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};
    use tokio::sync::{oneshot, watch};
    use tokio_util::task::AbortOnDropHandle;
    use ts_netstack_smoltcp::{HasChannel, netcore::NetstackControl};

    let client_ip: Ipv4Addr = "192.0.2.2".parse().unwrap();
    let server: SocketAddr = "192.0.2.1:51820".parse().unwrap();
    let profile = usque_core::Profile::default();
    let (config, _) = crate::netstack::proxy_netstack_config(&profile);
    let (left, mut left_pipe) = crate::netstack::bounded_piped(config);
    let (config, _) = crate::netstack::proxy_netstack_config(&profile);
    let (right, mut right_pipe) = crate::netstack::bounded_piped(config);
    let left_channel = left.command_channel();
    let right_channel = right.command_channel();
    let _left = AbortOnDropHandle::new(left.spawn_tokio());
    let _right = AbortOnDropHandle::new(right.spawn_tokio());
    left_channel.set_ips([client_ip.into()]).await.unwrap();
    right_channel.set_ips([server.ip()]).await.unwrap();
    let _bridge = AbortOnDropHandle::new(tokio::spawn(async move {
        loop {
            tokio::select! {
                Some(packet) = left_pipe.rx.recv_async() => { right_pipe.tx.send_owned_async(packet).await; }
                Some(packet) = right_pipe.rx.recv_async() => { left_pipe.tx.send_owned_async(packet).await; }
                else => break,
            }
        }
    }));
    let (_health, receiver) = watch::channel(RuntimeHealth::Connected {
        path: RuntimePath {
            transport: usque_core::Transport::Http3,
            endpoint_family: usque_core::AddressFamily::Ipv4,
            ipv4_available: true,
            ipv6_available: false,
        },
        reconnect_count: 1,
    });
    let network = InternalNetwork::for_streams(Arc::new(NoTcp), receiver, CancellationToken::new())
        .with_test_packets(left_channel, client_ip, Ipv6Addr::UNSPECIFIED);
    let probe = Arc::new(WarpProbe::new(network));
    let socket = QuerySocket::bind(right_channel.clone(), server)
        .await
        .unwrap();
    let wrong_source = QuerySocket::bind(right_channel, "192.0.2.1:51821".parse().unwrap())
        .await
        .unwrap();
    let node = peer("node", "US", "", "A", 0);
    let mut previous = None;
    for _ in 0..2 {
        let (release, released) = oneshot::channel();
        let (sent, received) = oneshot::channel();
        let task = tokio::spawn({
            let probe = probe.clone();
            let node = node.clone();
            async move { probe.measure(&node, &CancellationToken::new()).await }
        });
        let remote = tokio::time::timeout(Duration::from_secs(2), async {
            let (remote, request) = socket.recv_from_bytes().await.unwrap();
            assert_eq!(request.len(), 38);
            let timestamp = u32::from_le_bytes(request[2..6].try_into().unwrap());
            assert_eq!(
                request.as_ref(),
                ping::packet(&node.public_key, timestamp).unwrap()
            );
            if let Some(old) = previous {
                assert_ne!(remote, old);
                socket.send_to(old, &[0xfe, 1, 1]).await.unwrap();
            }
            wrong_source.send_to(remote, &[0xfe, 1, 1]).await.unwrap();
            for malformed in [&[0xfe, 1][..], &[0xfe, 1, 1, 0], &[0xfe, 1, 0]] {
                socket.send_to(remote, malformed).await.unwrap();
            }
            sent.send(remote).unwrap();
            released.await.unwrap();
            socket.send_to(remote, &[0xfe, 1, 1]).await.unwrap();
            remote
        });
        let verify = async {
            let remote = received.await.unwrap();
            tokio::time::sleep(Duration::from_millis(30)).await;
            assert!(
                !task.is_finished(),
                "wrong source, old sample or malformed reply accepted"
            );
            release.send(()).unwrap();
            assert!(task.await.unwrap().unwrap().is_some());
            remote
        };
        let (remote, checked) = tokio::join!(remote, verify);
        assert_eq!(remote.unwrap(), checked);
        previous = Some(checked);
    }
}
