//! Real, pinned loopback QUIC peers behind a socket-local test relay.
use super::client_tests::{ServerCommand, identities, server_with_control};
use super::*;
use crate::socket::{DirectEgressLease, DirectProtocol, SocketHandle, SocketProtector};
use crate::tcp::{FlowClass, TcpDialer, TcpStream, TcpTarget};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UdpSocket;
use tokio::sync::mpsc;
use tokio::task::JoinSet;
use tokio::time::{Instant, timeout};
use tokio_util::sync::CancellationToken;
use tokio_util::task::AbortOnDropHandle;
use usque_core::{DataPlaneMode, IdentityProvider, IpPolicy, Profile};

struct Peer {
    commands: mpsc::UnboundedSender<ServerCommand>,
    targets: Arc<Mutex<Vec<String>>>,
}

struct Fixture {
    endpoint: SocketAddr,
    peers: mpsc::UnboundedReceiver<Peer>,
    _task: AbortOnDropHandle<()>,
}

impl Fixture {
    async fn start(identity: crate::MasqueTlsIdentity, hold_after: Option<usize>) -> Self {
        let socket = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
        let endpoint = socket.local_addr().unwrap();
        let (peers, received) = mpsc::unbounded_channel();
        let task = tokio::spawn(async move {
            let mut routes = HashMap::<SocketAddr, mpsc::Sender<Vec<u8>>>::new();
            let mut children = JoinSet::new();
            let mut buffer = vec![0; 65536];
            loop {
                let (n, from) = socket.recv_from(&mut buffer).await.unwrap();
                let established = routes.len();
                if let std::collections::hash_map::Entry::Vacant(entry) = routes.entry(from) {
                    let peer_socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
                    let peer_address = peer_socket.local_addr().unwrap();
                    let relay = UdpSocket::bind("127.0.0.1:0").await.unwrap();
                    let (sender, mut requests) = mpsc::channel::<Vec<u8>>(32);
                    let (commands, receiver) = mpsc::unbounded_channel();
                    let targets = Arc::new(Mutex::new(Vec::new()));
                    let held = hold_after
                        .filter(|after| established >= *after)
                        .map(|_| Arc::new(tokio::sync::Notify::new()));
                    children.spawn(server_with_control(
                        peer_socket,
                        identity.clone(),
                        usque_core::CONSUMER_L4_SNI,
                        targets.clone(),
                        held,
                        Some(receiver),
                    ));
                    let output = socket.clone();
                    children.spawn(async move {
                        let mut bytes = vec![0; 65536];
                        loop {
                            tokio::select! {
                                request = requests.recv() => match request {
                                    Some(request) => { relay.send_to(&request, peer_address).await.unwrap(); },
                                    None => break,
                                },
                                result = relay.recv_from(&mut bytes) => {
                                    match result {
                                        Ok((n, _)) => { output.send_to(&bytes[..n], from).await.unwrap(); },
                                        Err(error) if error.kind() == std::io::ErrorKind::ConnectionReset => {},
                                        Err(error) => panic!("loopback relay: {error}"),
                                    }
                                },
                            }
                        }
                    });
                    entry.insert(sender);
                    peers
                        .send(Peer { commands, targets })
                        .unwrap_or_else(|_| panic!("fixture owner dropped"));
                }
                routes[&from].send(buffer[..n].to_vec()).await.unwrap();
            }
        });
        Self {
            endpoint,
            peers: received,
            _task: AbortOnDropHandle::new(task),
        }
    }
    async fn peer(&mut self) -> Peer {
        timeout(Duration::from_secs(3), self.peers.recv())
            .await
            .unwrap()
            .unwrap()
    }
}

struct Lease(Arc<AtomicUsize>);
impl Drop for Lease {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

#[derive(Default)]
struct Protector(Arc<AtomicUsize>);
#[async_trait::async_trait]
impl SocketProtector for Protector {
    fn protect(&self, _: SocketHandle) -> Result<(), String> {
        Ok(())
    }
    async fn protect_for_target(
        &self,
        _: SocketHandle,
        _: SocketAddr,
        _: DirectProtocol,
    ) -> Result<DirectEgressLease, String> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(DirectEgressLease::hold(Lease(self.0.clone())))
    }
}

fn profile(endpoint: SocketAddr, count: u8) -> Profile {
    let mut profile = Profile {
        data_plane: DataPlaneMode::L4Proxy,
        masque_connections: count,
        ip_policy: IpPolicy::Ipv4Only,
        ..Profile::default()
    };
    profile.endpoint.selection = usque_core::EndpointSelection::Custom;
    profile.endpoint.ipv4 = "127.0.0.1".parse().unwrap();
    profile.endpoint.port = endpoint.port();
    profile
}

async fn echo(stream: &mut TcpStream, value: &[u8]) {
    stream.write_all(value).await.unwrap();
    let mut result = vec![0; value.len()];
    stream.read_exact(&mut result).await.unwrap();
    assert_eq!(result, value);
}

async fn open(client: &L4Client, number: usize) -> TcpStream {
    client
        .connect(
            TcpTarget::new(&format!("flow-{number}.test"), 443).unwrap(),
            Instant::now() + Duration::from_secs(3),
            &CancellationToken::new(),
            FlowClass::Business,
        )
        .await
        .unwrap()
}

#[tokio::test]
async fn every_count_builds_one_pool_and_distributes_pinned_streams() {
    timeout(Duration::from_secs(30), async {
        for count in 1..=6 {
            let (mut identity, peer) = identities();
            identity.provider = Some(IdentityProvider::Consumer);
            let mut fixture = Fixture::start(peer, None).await;
            let protector = Arc::new(Protector::default());
            let client = L4Client::start(
                profile(fixture.endpoint, count),
                identity,
                protector.clone(),
                None,
                crate::ConnectionTelemetry::default(),
                Arc::default(),
                &CancellationToken::new(),
            )
            .await
            .unwrap();
            assert_eq!(protector.0.load(Ordering::SeqCst), usize::from(count));
            assert_eq!(client.session_generation(), Some(1));
            let mut peers = Vec::new();
            for _ in 0..count {
                peers.push(fixture.peer().await);
            }
            let mut flows = Vec::new();
            for number in 0..usize::from(count) * 2 {
                let mut stream = open(&client, number).await;
                assert_eq!(stream.session_generation(), client.session_generation());
                echo(&mut stream, &number.to_be_bytes()).await;
                flows.push(stream);
            }
            assert!(
                peers
                    .iter()
                    .all(|peer| peer.targets.lock().unwrap().len() == 2)
            );
            for (number, stream) in flows.iter_mut().enumerate() {
                echo(stream, &number.to_le_bytes()).await;
            }
            client.shutdown().await;
            assert_eq!(protector.0.load(Ordering::SeqCst), 0);
            assert_eq!(client.snapshot().sessions, 0);
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn member_recovery_and_goaway_keep_healthy_streams_and_pool_generation() {
    timeout(Duration::from_secs(12), async {
        let (mut identity, peer) = identities();
        identity.provider = Some(IdentityProvider::Consumer);
        let mut fixture = Fixture::start(peer, None).await;
        let protector = Arc::new(Protector::default());
        let client = L4Client::start(
            profile(fixture.endpoint, 2),
            identity,
            protector.clone(),
            None,
            crate::ConnectionTelemetry::default(),
            Arc::default(),
            &CancellationToken::new(),
        )
        .await
        .unwrap();
        let first = fixture.peer().await;
        let second = fixture.peer().await;
        let mut broken = open(&client, 0).await;
        let mut healthy = open(&client, 1).await;
        echo(&mut broken, b"first").await;
        echo(&mut healthy, b"second").await;
        first.commands.send(ServerCommand::Close).unwrap();
        let replacement = fixture.peer().await;
        timeout(Duration::from_secs(2), async {
            while client.snapshot().sessions != 2 {
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(client.session_generation(), Some(1));
        echo(&mut healthy, b"kept after failure").await;
        assert!(broken.write_all(b"must fail").await.is_err());
        assert_eq!(second.targets.lock().unwrap().len(), 1);
        assert!(replacement.targets.lock().unwrap().is_empty());
        second.commands.send(ServerCommand::GoAway).unwrap();
        let _replacement = fixture.peer().await;
        echo(&mut healthy, b"kept while draining").await;
        assert_eq!(client.session_generation(), Some(1));
        assert_eq!(protector.0.load(Ordering::SeqCst), 3);
        let mut fresh = open(&client, 2).await;
        echo(&mut fresh, b"new active member").await;
        assert_eq!(second.targets.lock().unwrap().len(), 1);
        client.shutdown().await;
        assert_eq!(protector.0.load(Ordering::SeqCst), 0);
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn partial_startup_waits_for_every_member_and_cancellation_releases_all_leases() {
    let (mut identity, peer) = identities();
    identity.provider = Some(IdentityProvider::Consumer);
    let mut fixture = Fixture::start(peer, Some(1)).await;
    let protector = Arc::new(Protector::default());
    let cancel = CancellationToken::new();
    let mut startup = AbortOnDropHandle::new(tokio::spawn({
        let cancel = cancel.clone();
        let protector = protector.clone();
        let profile = profile(fixture.endpoint, 2);
        async move {
            L4Client::start(
                profile,
                identity,
                protector,
                None,
                crate::ConnectionTelemetry::default(),
                Arc::default(),
                &cancel,
            )
            .await
        }
    }));
    let first = fixture.peer().await;
    let _pending = fixture.peer().await;
    assert!(
        timeout(Duration::from_millis(150), &mut startup)
            .await
            .is_err()
    );
    assert!(first.targets.lock().unwrap().is_empty());
    cancel.cancel();
    assert!(
        timeout(Duration::from_secs(2), startup)
            .await
            .unwrap()
            .unwrap()
            .is_err()
    );
    assert_eq!(protector.0.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn losing_every_member_advances_generation_before_admitting_new_streams() {
    timeout(Duration::from_secs(12), async {
        let (mut identity, peer) = identities();
        identity.provider = Some(IdentityProvider::Consumer);
        let mut fixture = Fixture::start(peer, Some(2)).await;
        let protector = Arc::new(Protector::default());
        let client = L4Client::start(
            profile(fixture.endpoint, 2),
            identity,
            protector.clone(),
            None,
            crate::ConnectionTelemetry::default(),
            Arc::default(),
            &CancellationToken::new(),
        )
        .await
        .unwrap();
        let first = fixture.peer().await;
        let second = fixture.peer().await;
        let mut stream = open(&client, 0).await;
        echo(&mut stream, b"old pool").await;
        first.commands.send(ServerCommand::Close).unwrap();
        second.commands.send(ServerCommand::Close).unwrap();
        let replacement = fixture.peer().await;
        timeout(Duration::from_secs(2), async {
            while !matches!(
                *client.health.borrow(),
                crate::RuntimeHealth::Reconnecting { .. }
            ) {
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
        })
        .await
        .unwrap();
        replacement.commands.send(ServerCommand::Settings).unwrap();
        let last = fixture.peer().await;
        last.commands.send(ServerCommand::Settings).unwrap();
        timeout(Duration::from_secs(2), async {
            while client.snapshot().sessions != 2 {
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(client.session_generation(), Some(2));
        let mut fresh = open(&client, 1).await;
        assert_eq!(fresh.session_generation(), Some(2));
        echo(&mut fresh, b"new pool").await;
        assert!(stream.write_all(b"old pool is closed").await.is_err());
        client.shutdown().await;
        assert_eq!(protector.0.load(Ordering::SeqCst), 0);
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn physical_network_change_joins_every_old_member_before_redial() {
    use crate::socket::PhysicalNetworkSnapshot;
    struct Network {
        state: tokio::sync::watch::Sender<PhysicalNetworkSnapshot>,
        active: Arc<Protector>,
        redial: tokio::sync::Notify,
    }
    #[async_trait::async_trait]
    impl SocketProtector for Network {
        fn protect(&self, _: SocketHandle) -> Result<(), String> {
            Ok(())
        }
        fn network_generation(&self) -> Option<u64> {
            Some(self.state.borrow().generation)
        }
        fn subscribe_physical_network(
            &self,
        ) -> Option<tokio::sync::watch::Receiver<PhysicalNetworkSnapshot>> {
            Some(self.state.subscribe())
        }
        async fn protect_for_target_generation(
            &self,
            _: SocketHandle,
            _: SocketAddr,
            _: DirectProtocol,
            generation: u64,
        ) -> Result<DirectEgressLease, String> {
            if generation != 1 {
                assert_eq!(self.active.0.load(Ordering::SeqCst), 0);
                self.redial.notify_one();
                return std::future::pending().await;
            }
            self.active.0.fetch_add(1, Ordering::SeqCst);
            Ok(DirectEgressLease::hold_for_generation(
                Lease(self.active.0.clone()),
                generation,
            ))
        }
    }
    let (mut identity, peer) = identities();
    identity.provider = Some(IdentityProvider::Consumer);
    let mut fixture = Fixture::start(peer, None).await;
    let network = Arc::new(Network {
        state: tokio::sync::watch::channel(PhysicalNetworkSnapshot {
            generation: 1,
            ..Default::default()
        })
        .0,
        active: Arc::new(Protector::default()),
        redial: tokio::sync::Notify::new(),
    });
    let client = L4Client::start(
        profile(fixture.endpoint, 3),
        identity,
        network.clone(),
        None,
        crate::ConnectionTelemetry::default(),
        Arc::default(),
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    for _ in 0..3 {
        let _ = fixture.peer().await;
    }
    assert_eq!(network.active.0.load(Ordering::SeqCst), 3);
    network.state.send_modify(|state| state.generation = 2);
    timeout(Duration::from_secs(3), network.redial.notified())
        .await
        .unwrap();
    client.shutdown().await;
    assert_eq!(network.active.0.load(Ordering::SeqCst), 0);
}
