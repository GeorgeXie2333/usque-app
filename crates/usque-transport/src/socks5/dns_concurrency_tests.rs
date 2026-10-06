//! Loopback frontend and memory-only DNS/UDP peers; no system networking changes.
use super::*;
use crate::dns_stream::StreamDns;
use crate::proxy_udp::{UdpAssociation, UdpFactory};
use crate::tcp::{DialError, FlowClass, TcpDialer, TcpIo, TcpTarget};
use async_trait::async_trait;
use bytes::Bytes;
use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::io::{AsyncRead, AsyncWrite, DuplexStream, ReadBuf};
use tokio::sync::oneshot;

const BUDGET: usize = 4 << 20;

struct MemoryStream(DuplexStream);
impl TcpIo for MemoryStream {
    fn local_addr(&self) -> io::Result<SocketAddr> {
        Ok("192.0.2.1:41000".parse().unwrap())
    }
}
impl AsyncRead for MemoryStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.0).poll_read(cx, buffer)
    }
}
impl AsyncWrite for MemoryStream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.0).poll_write(cx, bytes)
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.0).poll_flush(cx)
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.0).poll_shutdown(cx)
    }
}

#[derive(Debug)]
struct StartedQuery {
    id: u16,
    release: Option<oneshot::Sender<()>>,
    closed: oneshot::Receiver<()>,
}

struct DnsPeer {
    started: mpsc::UnboundedSender<StartedQuery>,
    cancellation: CancellationToken,
}

#[async_trait]
impl TcpDialer for DnsPeer {
    async fn connect(
        &self,
        target: TcpTarget,
        _: Instant,
        _: &CancellationToken,
        _: FlowClass,
    ) -> Result<crate::tcp::TcpStream, DialError> {
        assert_eq!(target.host_port().1, 53);
        let slow = target.host_port().0.starts_with("slow");
        let (client, mut peer) = tokio::io::duplex(4096);
        let started = self.started.clone();
        let cancellation = self.cancellation.clone();
        tokio::spawn(async move {
            let work = async {
                loop {
                    let Ok(length) = peer.read_u16().await else {
                        return;
                    };
                    let mut query = vec![0; usize::from(length)];
                    if peer.read_exact(&mut query).await.is_err() {
                        return;
                    }
                    crate::split_dns::validate_query_bytes(&query).unwrap();
                    let id = u16::from_be_bytes([query[0], query[1]]);
                    let (release_tx, release_rx) = oneshot::channel();
                    let (closed_tx, closed_rx) = oneshot::channel();
                    started
                        .send(StartedQuery {
                            id,
                            release: slow.then_some(release_tx),
                            closed: closed_rx,
                        })
                        .unwrap();
                    if slow {
                        tokio::select! {
                            released = release_rx => if released.is_err() { return; },
                            closed = peer.read_u8() => {
                                assert!(closed.is_err(), "pending query must close on cancellation");
                                let _ = closed_tx.send(());
                                return;
                            }
                        }
                    }
                    let response = answer(&query);
                    if peer.write_u16(response.len() as u16).await.is_err()
                        || peer.write_all(&response).await.is_err()
                    {
                        return;
                    }
                }
            };
            tokio::select! { _ = cancellation.cancelled() => {}, _ = work => {} }
        });
        Ok(Box::new(MemoryStream(client)))
    }
}

struct EchoUdp {
    replies: mpsc::Sender<(TcpTarget, Bytes)>,
    received: Mutex<mpsc::Receiver<(TcpTarget, Bytes)>>,
}
#[async_trait]
impl UdpAssociation for EchoUdp {
    async fn send(&self, target: &TcpTarget, payload: &[u8]) -> Result<(), DialError> {
        self.replies
            .send((target.clone(), Bytes::copy_from_slice(payload)))
            .await
            .map_err(|_| DialError::Closed)
    }
    async fn recv(&self) -> Result<(TcpTarget, Bytes), DialError> {
        self.received
            .lock()
            .await
            .recv()
            .await
            .ok_or(DialError::Closed)
    }
}
struct EchoFactory;
#[async_trait]
impl UdpFactory for EchoFactory {
    async fn open(
        &self,
        _: &CancellationToken,
        _: Instant,
    ) -> Result<Arc<dyn UdpAssociation>, DialError> {
        let (replies, received) = mpsc::channel(8);
        Ok(Arc::new(EchoUdp {
            replies,
            received: Mutex::new(received),
        }))
    }
}

struct Fixture {
    frontend: Socks5Frontend,
    started: mpsc::UnboundedReceiver<StartedQuery>,
    budget: Arc<crate::l4::BufferBudget>,
    dns: Arc<StreamDns>,
    cancellation: CancellationToken,
}
impl Fixture {
    async fn new(udp_available: bool) -> Self {
        let cancellation = CancellationToken::new();
        let (started_tx, started) = mpsc::unbounded_channel();
        let dialer = Arc::new(DnsPeer {
            started: started_tx,
            cancellation: cancellation.clone(),
        });
        let metrics = Arc::new(crate::l4::L4Metrics::default());
        let budget = Arc::new(crate::l4::BufferBudget::new(
            BUDGET,
            metrics.clone(),
            Arc::new(tokio::sync::Notify::new()),
        ));
        let protector = noop_socket_protector();
        let dns = Arc::new(StreamDns::new(
            dialer.clone(),
            protector.clone(),
            cancellation.clone(),
            metrics,
        ));
        let (_, health) = watch::channel(RuntimeHealth::Connected {
            path: RuntimePath {
                transport: usque_core::Transport::Http3,
                endpoint_family: usque_core::AddressFamily::Ipv4,
                ipv4_available: true,
                ipv6_available: true,
            },
            reconnect_count: 0,
        });
        let mut profile = Profile::default();
        profile.proxy.dns_mode = usque_core::ProxyDnsMode::EdgeResolved;
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let services = crate::tcp::ProxyServices {
            traffic_policy: Arc::default(),
            admission: Some(Arc::new(crate::tcp::FrontendAdmission::new(
                budget.clone(),
                32,
            ))),
            dialer,
            udp: udp_available.then(|| Arc::new(EchoFactory) as Arc<dyn UdpFactory>),
            resolver: Resolver::for_streams(
                dns.clone(),
                vec![],
                profile.proxy.dns_mode,
                protector.clone(),
            ),
            protector,
            geo_policy: Arc::default(),
            counters: Arc::default(),
            cancellation: cancellation.clone(),
            health,
        };
        let frontend =
            Socks5Frontend::activate_services(&profile, services, vec![listener]).unwrap();
        Self {
            frontend,
            started,
            budget,
            dns,
            cancellation,
        }
    }

    async fn associate(&self) -> (TcpStream, TokioUdpSocket, SocketAddr) {
        let mut control = TcpStream::connect(self.frontend.listeners()[0])
            .await
            .unwrap();
        control.write_all(&[5, 1, 0]).await.unwrap();
        let mut greeting = [0; 2];
        control.read_exact(&mut greeting).await.unwrap();
        assert_eq!(greeting, [5, 0]);
        control
            .write_all(&[5, 3, 0, 1, 0, 0, 0, 0, 0, 0])
            .await
            .unwrap();
        let mut reply = [0; 10];
        timeout(Duration::from_secs(2), control.read_exact(&mut reply))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(&reply[..4], &[5, 0, 0, 1]);
        let relay = SocketAddr::new(
            Ipv4Addr::new(reply[4], reply[5], reply[6], reply[7]).into(),
            u16::from_be_bytes([reply[8], reply[9]]),
        );
        let udp = TokioUdpSocket::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        (control, udp, relay)
    }

    async fn started(&mut self, id: u16) -> StartedQuery {
        let query = timeout(Duration::from_secs(2), self.started.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(query.id, id);
        query
    }

    async fn full_budget(&self) {
        timeout(Duration::from_secs(2), async {
            while self.budget.available() != BUDGET {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
    }

    async fn shutdown(&mut self) {
        self.frontend.shutdown().await;
        self.cancellation.cancel();
        self.dns.clear();
        self.full_budget().await;
    }
}

fn query(id: u16) -> Vec<u8> {
    let mut query = vec![0, 0, 1, 0, 0, 1, 0, 0, 0, 0, 0, 0];
    query[..2].copy_from_slice(&id.to_be_bytes());
    query.extend_from_slice(b"\x07example\x04test\0\0\x01\0\x01");
    query
}
fn answer(query: &[u8]) -> Vec<u8> {
    let mut response = query.to_vec();
    response[2..4].copy_from_slice(&[0x81, 0x80]);
    response
}
fn datagram(target: &TcpTarget, payload: &[u8]) -> Vec<u8> {
    let mut datagram = vec![0, 0, 0];
    crate::proxy_exit::encode_target(target, &mut datagram);
    datagram.extend_from_slice(payload);
    datagram
}
async fn send_query(udp: &TokioUdpSocket, relay: SocketAddr, host: &str, id: u16) {
    udp.send_to(
        &datagram(&TcpTarget::new(host, 53).unwrap(), &query(id)),
        relay,
    )
    .await
    .unwrap();
}
async fn receive(udp: &TokioUdpSocket, relay: SocketAddr) -> (TcpTarget, Vec<u8>) {
    let mut buffer = [0; 4096];
    let (length, source) = timeout(Duration::from_secs(2), udp.recv_from(&mut buffer))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(source, relay);
    let parsed = decode_udp_request(&buffer[..length]).unwrap();
    let target = match parsed.target {
        Target::Address(ip) => TcpTarget::address(SocketAddr::new(ip, parsed.port)),
        Target::Domain(host) => TcpTarget::new(&host, parsed.port).unwrap(),
    };
    (target, parsed.payload.to_vec())
}
async fn expect_dns(udp: &TokioUdpSocket, relay: SocketAddr, host: &str, id: u16, rcode: u8) {
    let (target, payload) = receive(udp, relay).await;
    assert_eq!(target, TcpTarget::new(host, 53).unwrap());
    crate::split_dns::validate_response_bytes(&query(id), &payload).unwrap();
    assert_eq!(&payload[..2], &id.to_be_bytes());
    assert_eq!(payload[3] & 15, rcode);
}

#[tokio::test]
async fn slow_dns_does_not_block_same_association_dns_or_udp_and_replies_match_ids() {
    for udp_available in [false, true] {
        let mut fixture = Fixture::new(udp_available).await;
        let (control, udp, relay) = fixture.associate().await;
        send_query(&udp, relay, "slow-one.example", 1).await;
        let first = fixture.started(1).await;
        send_query(&udp, relay, "fast.example", 2).await;
        fixture.started(2).await;
        expect_dns(&udp, relay, "fast.example", 2, 0).await;
        if udp_available {
            let target = TcpTarget::address("203.0.113.8:443".parse().unwrap());
            udp.send_to(&datagram(&target, b"ordinary UDP"), relay)
                .await
                .unwrap();
            assert_eq!(
                receive(&udp, relay).await,
                (target, b"ordinary UDP".to_vec())
            );
        }
        send_query(&udp, relay, "slow-two.example", 3).await;
        let second = fixture.started(3).await;
        second.release.unwrap().send(()).unwrap();
        expect_dns(&udp, relay, "slow-two.example", 3, 0).await;
        first.release.unwrap().send(()).unwrap();
        expect_dns(&udp, relay, "slow-one.example", 1, 0).await;
        drop(control);
        fixture.shutdown().await;
    }
}

#[tokio::test]
async fn fifth_dns_query_fails_immediately_and_control_close_joins_all_pending_queries() {
    for udp_available in [false, true] {
        let mut fixture = Fixture::new(udp_available).await;
        let (control, udp, relay) = fixture.associate().await;
        let initial = fixture.budget.available();
        let mut pending = Vec::new();
        for id in 1..=4 {
            send_query(&udp, relay, &format!("slow-{id}.example"), id).await;
            pending.push(fixture.started(id).await);
        }
        assert!(initial - fixture.budget.available() >= 4 * usize::from(u16::MAX));
        send_query(&udp, relay, "fast.example", 5).await;
        expect_dns(&udp, relay, "fast.example", 5, 2).await;
        assert!(fixture.started.try_recv().is_err());
        drop(control);
        for query in pending {
            timeout(Duration::from_secs(2), query.closed)
                .await
                .unwrap()
                .unwrap();
        }
        fixture.full_budget().await;
        fixture.shutdown().await;
    }
}

#[tokio::test]
async fn first_pending_dns_query_pins_client_before_completion() {
    let mut fixture = Fixture::new(false).await;
    let (control, udp, relay) = fixture.associate().await;
    let other = TokioUdpSocket::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    // Malformed packets cannot claim the association.
    other.send_to(&[1, 0, 0, 1], relay).await.unwrap();
    send_query(&udp, relay, "slow.example", 1).await;
    let pending = fixture.started(1).await;
    send_query(&other, relay, "fast.example", 2).await;
    assert!(
        timeout(Duration::from_millis(100), fixture.started.recv())
            .await
            .is_err()
    );
    send_query(&udp, relay, "fast.example", 3).await;
    fixture.started(3).await;
    expect_dns(&udp, relay, "fast.example", 3, 0).await;
    let mut packet = [0; 4096];
    assert!(
        timeout(Duration::from_millis(100), other.recv_from(&mut packet))
            .await
            .is_err()
    );
    drop(control);
    timeout(Duration::from_secs(2), pending.closed)
        .await
        .unwrap()
        .unwrap();
    fixture.shutdown().await;
}

#[tokio::test]
async fn unavailable_dns_memory_budget_returns_servfail_without_starting_work() {
    let mut fixture = Fixture::new(false).await;
    let (control, udp, relay) = fixture.associate().await;
    let held = fixture.budget.reserve(fixture.budget.available()).unwrap();
    send_query(&udp, relay, "fast.example", 7).await;
    expect_dns(&udp, relay, "fast.example", 7, 2).await;
    assert!(fixture.started.try_recv().is_err());
    drop(held);
    drop(control);
    fixture.shutdown().await;
}
