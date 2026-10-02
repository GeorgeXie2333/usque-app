//! Address races use memory DNS and proxy peers; no physical socket or OS TUN.
use super::*;
use crate::dns::Resolver;
use crate::dns_stream::StreamDns;
use crate::netstack::{RuntimeHealth, RuntimePath};
use crate::socket::{SocketHandle, SocketProtector};
use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tokio::io::DuplexStream;
use tokio::sync::Notify;
use usque_core::chain_exit::{ChainSource, ProxyExitConfiguration};
use usque_core::{
    AddressFamily, ProxyDnsMode, Transport, TransportFailure, TransportFailureCode, TransportStage,
};

#[derive(Clone, Copy)]
enum Outcome {
    Refused,
    Blackhole,
    Connected,
}
#[derive(Default)]
struct Observations {
    attempted: Mutex<Vec<(SocketAddr, Instant)>>,
    active_attempts: AtomicUsize,
    cancelled_blackholes: AtomicUsize,
    uncancelled_blackholes: AtomicUsize,
    streams: AtomicUsize,
    started: Notify,
}
struct AttemptResource {
    observations: Arc<Observations>,
    cancellation: CancellationToken,
    blackhole: bool,
}
impl Drop for AttemptResource {
    fn drop(&mut self) {
        self.observations
            .active_attempts
            .fetch_sub(1, Ordering::SeqCst);
        if self.blackhole {
            if self.cancellation.is_cancelled() {
                self.observations
                    .cancelled_blackholes
                    .fetch_add(1, Ordering::SeqCst);
            } else {
                self.observations
                    .uncancelled_blackholes
                    .fetch_add(1, Ordering::SeqCst);
            }
        }
    }
}
struct MemoryStream {
    io: DuplexStream,
    cancellation: CancellationToken,
    observations: Option<Arc<Observations>>,
}
impl Drop for MemoryStream {
    fn drop(&mut self) {
        if let Some(observations) = &self.observations {
            observations.streams.fetch_sub(1, Ordering::SeqCst);
        }
    }
}
impl TcpIo for MemoryStream {
    fn local_addr(&self) -> io::Result<SocketAddr> {
        Ok("192.0.2.1:41000".parse().unwrap())
    }
}
impl AsyncRead for MemoryStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        if self.cancellation.is_cancelled() {
            return Poll::Ready(Err(io::ErrorKind::ConnectionAborted.into()));
        }
        Pin::new(&mut self.io).poll_read(cx, buf)
    }
}
impl AsyncWrite for MemoryStream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        if self.cancellation.is_cancelled() {
            return Poll::Ready(Err(io::ErrorKind::ConnectionAborted.into()));
        }
        Pin::new(&mut self.io).poll_write(cx, buf)
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.io).poll_flush(cx)
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.io).poll_shutdown(cx)
    }
}
struct ServerDialer {
    outcomes: HashMap<SocketAddr, Outcome>,
    observations: Arc<Observations>,
}
#[async_trait]
impl TcpDialer for ServerDialer {
    async fn connect(
        &self,
        target: TcpTarget,
        _: Instant,
        cancellation: &CancellationToken,
        class: FlowClass,
    ) -> Result<TcpStream, DialError> {
        assert_eq!(class, FlowClass::Business);
        let remote = target
            .socket_address()
            .expect("proxy endpoints are resolved through the private DNS fixture");
        let outcome = *self
            .outcomes
            .get(&remote)
            .expect("only supplied DNS candidates may be dialed");
        self.observations
            .attempted
            .lock()
            .unwrap()
            .push((remote, Instant::now()));
        self.observations
            .active_attempts
            .fetch_add(1, Ordering::SeqCst);
        self.observations.started.notify_one();
        let _resource = AttemptResource {
            observations: self.observations.clone(),
            cancellation: cancellation.clone(),
            blackhole: matches!(outcome, Outcome::Blackhole),
        };
        match outcome {
            Outcome::Refused => Err(DialError::Refused),
            Outcome::Blackhole => {
                cancellation.cancelled().await;
                Err(DialError::Cancelled)
            }
            Outcome::Connected => {
                let (client, mut peer) = tokio::io::duplex(4096);
                let cancel = cancellation.clone();
                tokio::spawn(async move {
                    let work = async {
                        let mut buffer = [0; 256];
                        while let Ok(length) = peer.read(&mut buffer).await {
                            if length == 0 || peer.write_all(&buffer[..length]).await.is_err() {
                                break;
                            }
                        }
                    };
                    tokio::select! { _ = cancel.cancelled() => {}, _ = work => {} }
                });
                self.observations.streams.fetch_add(1, Ordering::SeqCst);
                Ok(Box::new(MemoryStream {
                    io: client,
                    cancellation: cancellation.clone(),
                    observations: Some(self.observations.clone()),
                }))
            }
        }
    }
}
struct NoPhysicalNetwork;
impl SocketProtector for NoPhysicalNetwork {
    fn protect(&self, _: SocketHandle) -> Result<(), String> {
        panic!("private proxy resolution must never use physical sockets")
    }
    fn resolve(&self, _: &str, _: u16) -> Result<Vec<SocketAddr>, String> {
        panic!("private proxy resolution must never use the system resolver")
    }
}
struct DnsDialer {
    addresses: Vec<IpAddr>,
    queries: Arc<AtomicUsize>,
}
fn dns_answer(query: &[u8], addresses: &[IpAddr]) -> Vec<u8> {
    let qtype = u16::from_be_bytes(query[query.len() - 4..query.len() - 2].try_into().unwrap());
    let matching: Vec<_> = addresses
        .iter()
        .filter(|ip| ip.is_ipv4() == (qtype == 1))
        .collect();
    let mut response = query.to_vec();
    response[2..4].copy_from_slice(&[0x81, 0x80]);
    response[6..8].copy_from_slice(&(matching.len() as u16).to_be_bytes());
    for ip in matching {
        let bytes = match ip {
            IpAddr::V4(ip) => ip.octets().to_vec(),
            IpAddr::V6(ip) => ip.octets().to_vec(),
        };
        response.extend_from_slice(&[0xc0, 12]);
        response.extend_from_slice(&qtype.to_be_bytes());
        response.extend_from_slice(&[0, 1, 0, 0, 0, 60]);
        response.extend_from_slice(&(bytes.len() as u16).to_be_bytes());
        response.extend_from_slice(&bytes);
    }
    response
}
#[async_trait]
impl TcpDialer for DnsDialer {
    async fn connect(
        &self,
        target: TcpTarget,
        _: Instant,
        cancellation: &CancellationToken,
        class: FlowClass,
    ) -> Result<TcpStream, DialError> {
        assert_eq!(class, FlowClass::Dns);
        assert_eq!(
            target.socket_address(),
            Some("203.0.113.53:53".parse().unwrap())
        );
        let (client, mut peer) = tokio::io::duplex(4096);
        let addresses = self.addresses.clone();
        let queries = self.queries.clone();
        let cancel = cancellation.clone();
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
                    queries.fetch_add(1, Ordering::SeqCst);
                    let answer = dns_answer(&query, &addresses);
                    if peer.write_u16(answer.len() as u16).await.is_err()
                        || peer.write_all(&answer).await.is_err()
                    {
                        return;
                    }
                }
            };
            tokio::select! { _ = cancel.cancelled() => {}, _ = work => {} }
        });
        Ok(Box::new(MemoryStream {
            io: client,
            cancellation: cancellation.clone(),
            observations: None,
        }))
    }
}
struct Fixture {
    network: crate::InternalNetwork,
    prepared: PreparedProfile,
    cancellation: CancellationToken,
    dns: Arc<StreamDns>,
    dns_queries: Arc<AtomicUsize>,
    observations: Arc<Observations>,
    health: watch::Sender<RuntimeHealth>,
}
impl Fixture {
    fn new(host: &str, candidates: &[(SocketAddr, Outcome)]) -> Self {
        let cancellation = CancellationToken::new();
        let observations = Arc::new(Observations::default());
        let dns_queries = Arc::new(AtomicUsize::new(0));
        let protector = Arc::new(NoPhysicalNetwork);
        let dns = Arc::new(StreamDns::new(
            Arc::new(DnsDialer {
                addresses: candidates.iter().map(|(a, _)| a.ip()).collect(),
                queries: dns_queries.clone(),
            }),
            protector.clone(),
            cancellation.clone(),
            Arc::default(),
        ));
        let resolver = Resolver::for_streams(
            dns.clone(),
            vec!["203.0.113.53".parse().unwrap()],
            ProxyDnsMode::Remote,
            protector,
        );
        let (health, receiver) = watch::channel(RuntimeHealth::Connected {
            path: path(),
            reconnect_count: 0,
        });
        let network = crate::InternalNetwork::for_streams(
            Arc::new(ServerDialer {
                outcomes: candidates.iter().copied().collect(),
                observations: observations.clone(),
            }),
            receiver,
            cancellation.clone(),
        )
        .with_resolver(resolver);
        let mut secrets = ImportSecrets::default();
        secrets.proxy = Some(ProxyExitConfiguration {
            host: host.into(),
            port: 8080,
            auth_mode: ProxyAuthMode::None,
            dns_servers: vec![],
        });
        let parsed =
            usque_core::chain_exit::ValidatedProfile::parse(ChainSource::HttpProxy, &secrets)
                .unwrap();
        let summary = parsed
            .summary("Memory proxy", uuid::Uuid::new_v4(), uuid::Uuid::new_v4())
            .unwrap();
        let prepared = PreparedProfile::imported(summary, parsed, secrets);
        Self {
            network,
            prepared,
            cancellation,
            dns,
            dns_queries,
            observations,
            health,
        }
    }
    async fn start(&self, deadline: Instant) -> Result<Arc<ProxyDialer>, DialError> {
        let budget = Arc::new(crate::l4::BufferBudget::new(
            16 << 20,
            Arc::default(),
            Arc::new(Notify::new()),
        ));
        ProxyDialer::start(
            &self.prepared,
            self.network.clone(),
            None,
            self.cancellation.clone(),
            budget,
            Arc::default(),
            deadline,
        )
        .await
    }
    fn attempts(&self) -> Vec<SocketAddr> {
        self.observations
            .attempted
            .lock()
            .unwrap()
            .iter()
            .map(|(a, _)| *a)
            .collect()
    }
    fn stopped(&self, blackholes: usize) {
        assert_eq!(self.observations.active_attempts.load(Ordering::SeqCst), 0);
        assert_eq!(
            self.observations
                .cancelled_blackholes
                .load(Ordering::SeqCst),
            blackholes
        );
        assert_eq!(
            self.observations
                .uncancelled_blackholes
                .load(Ordering::SeqCst),
            0
        );
        assert_eq!(self.observations.streams.load(Ordering::SeqCst), 0);
    }
    fn shutdown(self) {
        self.cancellation.cancel();
        self.dns.clear();
    }
}
fn path() -> RuntimePath {
    RuntimePath {
        transport: Transport::Http3,
        endpoint_family: AddressFamily::Ipv4,
        ipv4_available: true,
        ipv6_available: true,
    }
}
fn v4() -> SocketAddr {
    "203.0.113.10:8080".parse().unwrap()
}
fn v6() -> SocketAddr {
    "[2001:db8::10]:8080".parse().unwrap()
}

#[tokio::test(start_paused = true)]
async fn proxy_server_refused_a_uses_reachable_aaaa_and_retains_winner_token() {
    let fixture = Fixture::new(
        "proxy.example",
        &[(v4(), Outcome::Refused), (v6(), Outcome::Connected)],
    );
    let started = Instant::now();
    let proxy = fixture
        .start(started + Duration::from_secs(2))
        .await
        .unwrap();
    assert_eq!(fixture.attempts(), vec![v4(), v6()]);
    assert!(started.elapsed() < Duration::from_millis(250));
    assert_eq!(proxy.status.borrow().active_endpoint, Some(v6()));
    let (mut stream, winner) = proxy
        .server(
            Instant::now() + Duration::from_secs(2),
            &fixture.cancellation,
        )
        .await
        .unwrap();
    assert_eq!(winner, v6());
    stream.write_all(b"winner").await.unwrap();
    let mut echo = [0; 6];
    stream.read_exact(&mut echo).await.unwrap();
    assert_eq!(&echo, b"winner");
    drop(stream);
    fixture.stopped(0);
    fixture.shutdown();
}

#[tokio::test(start_paused = true)]
async fn proxy_server_blackholed_a_staggers_aaaa_and_cancels_loser_resources() {
    let fixture = Fixture::new(
        "proxy.example",
        &[(v4(), Outcome::Blackhole), (v6(), Outcome::Connected)],
    );
    let started = Instant::now();
    let proxy = fixture
        .start(started + Duration::from_secs(2))
        .await
        .unwrap();
    assert_eq!(fixture.attempts(), vec![v4(), v6()]);
    assert_eq!(started.elapsed(), Duration::from_millis(250));
    assert_eq!(proxy.status.borrow().active_endpoint, Some(v6()));
    fixture.stopped(1);
    fixture.shutdown();
}

#[tokio::test(start_paused = true)]
async fn proxy_server_tries_an_alternative_address_in_the_same_family() {
    let alternate: SocketAddr = "203.0.113.11:8080".parse().unwrap();
    let fixture = Fixture::new(
        "proxy.example",
        &[(v4(), Outcome::Refused), (alternate, Outcome::Connected)],
    );
    let proxy = fixture
        .start(Instant::now() + Duration::from_secs(2))
        .await
        .unwrap();
    assert_eq!(fixture.attempts(), vec![v4(), alternate]);
    assert_eq!(proxy.status.borrow().active_endpoint, Some(alternate));
    fixture.stopped(0);
    fixture.shutdown();
}

#[tokio::test(start_paused = true)]
async fn proxy_server_deadline_cancels_attempt_without_dialing_later_candidates() {
    let fixture = Fixture::new(
        "proxy.example",
        &[(v4(), Outcome::Blackhole), (v6(), Outcome::Connected)],
    );
    let started = Instant::now();
    assert!(matches!(
        fixture.start(started + Duration::from_millis(100)).await,
        Err(DialError::Timeout)
    ));
    assert_eq!(started.elapsed(), Duration::from_millis(100));
    tokio::time::advance(Duration::from_secs(1)).await;
    assert_eq!(fixture.attempts(), vec![v4()]);
    fixture.stopped(1);
    fixture.shutdown();
}

#[tokio::test(start_paused = true)]
async fn proxy_server_cancellation_releases_attempt_without_dialing_later_candidates() {
    let fixture = Fixture::new(
        "proxy.example",
        &[(v4(), Outcome::Blackhole), (v6(), Outcome::Connected)],
    );
    {
        let start = fixture.start(Instant::now() + Duration::from_secs(2));
        tokio::pin!(start);
        tokio::select! { result = &mut start => panic!("blackhole finished before cancellation: {}", result.is_ok()), _ = fixture.observations.started.notified() => {} }
        fixture.cancellation.cancel();
        assert!(matches!(start.await, Err(DialError::Cancelled)));
    }
    tokio::time::advance(Duration::from_secs(1)).await;
    assert_eq!(fixture.attempts(), vec![v4()]);
    fixture.stopped(1);
    fixture.shutdown();
}

#[tokio::test(start_paused = true)]
async fn proxy_server_numeric_address_does_not_use_dns_or_an_address_race() {
    let fixture = Fixture::new("203.0.113.10", &[(v4(), Outcome::Connected)]);
    let proxy = fixture
        .start(Instant::now() + Duration::from_secs(2))
        .await
        .unwrap();
    assert_eq!(fixture.attempts(), vec![v4()]);
    assert_eq!(fixture.dns_queries.load(Ordering::SeqCst), 0);
    assert_eq!(proxy.status.borrow().active_endpoint, Some(v4()));
    fixture.stopped(0);
    fixture.shutdown();
}

#[tokio::test(start_paused = true)]
async fn proxy_server_failed_underlay_never_dials_a_resolved_candidate() {
    let fixture = Fixture::new(
        "proxy.example",
        &[(v4(), Outcome::Connected), (v6(), Outcome::Connected)],
    );
    fixture.health.send_replace(RuntimeHealth::Failed {
        last_path: path(),
        reconnect_count: 0,
        message: "fixture underlay failed".into(),
        failure: TransportFailure::new(
            TransportFailureCode::SocketProtectionFailed,
            TransportStage::SocketProtection,
        ),
    });
    assert!(matches!(
        fixture.start(Instant::now() + Duration::from_secs(2)).await,
        Err(DialError::Closed)
    ));
    assert!(fixture.attempts().is_empty());
    fixture.stopped(0);
    fixture.shutdown();
}
