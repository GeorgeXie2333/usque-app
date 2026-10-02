//! Synthetic DNS and UDP routing over an in-memory TUN bridge; sockets are loopback only.
use crate::direct_gateway::NatPacket;
use crate::dns::Resolver;
use crate::dns_stream::StreamDns;
use crate::geo_direct::GeoDirectPolicy;
use crate::netstack::{RuntimeHealth, RuntimePath};
use crate::socket::{DirectEgressLease, DirectProtocol, SocketHandle, SocketProtector};
use crate::tcp::{DialError, FlowClass, ProxyServices, TcpDialer, TcpIo, TcpStream, TcpTarget};
use async_trait::async_trait;
use bytes::Bytes;
use std::io;
use std::net::{Ipv4Addr, SocketAddr};
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, DuplexStream, ReadBuf};
use tokio::sync::{Notify, mpsc, watch};
use tokio::time::{Instant, timeout};
use tokio_util::sync::CancellationToken;
use tokio_util::task::AbortOnDropHandle;
use usque_core::{AddressFamily, DataPlaneMode, Profile, ProxyDnsMode, Transport};

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
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.0).poll_read(cx, buf)
    }
}
impl AsyncWrite for MemoryStream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.0).poll_write(cx, buf)
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.0).poll_flush(cx)
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.0).poll_shutdown(cx)
    }
}
fn query(name: &str) -> Vec<u8> {
    let mut query = vec![0x12, 0x34, 1, 0, 0, 1, 0, 0, 0, 0, 0, 0];
    for label in name.split('.') {
        query.push(label.len() as u8);
        query.extend_from_slice(label.as_bytes());
    }
    query.extend_from_slice(&[0, 0, 1, 0, 1]);
    query
}
fn answer(query: &[u8], ttl: u32) -> Vec<u8> {
    let mut answer = query.to_vec();
    answer[2..4].copy_from_slice(&[0x81, 0x80]);
    answer[6..8].copy_from_slice(&[0, 1]);
    answer.extend_from_slice(&[0xc0, 12, 0, 1, 0, 1]);
    answer.extend_from_slice(&ttl.to_be_bytes());
    answer.extend_from_slice(&[0, 4, 127, 0, 0, 1]);
    answer
}
struct TunnelDns;
#[async_trait]
impl TcpDialer for TunnelDns {
    async fn connect(
        &self,
        _: TcpTarget,
        _: Instant,
        cancel: &CancellationToken,
        class: FlowClass,
    ) -> Result<TcpStream, DialError> {
        assert_eq!(class, FlowClass::Dns);
        let (client, mut peer) = tokio::io::duplex(4096);
        let cancel = cancel.clone();
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
                    let response = answer(&query, 60);
                    if peer.write_u16(response.len() as u16).await.is_err()
                        || peer.write_all(&response).await.is_err()
                    {
                        return;
                    }
                }
            };
            tokio::select! { _ = cancel.cancelled() => {}, _ = work => {} }
        });
        Ok(Box::new(MemoryStream(client)))
    }
}
struct Lease(Arc<AtomicUsize>);
impl Drop for Lease {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}
struct Protector {
    dns: SocketAddr,
    generation: AtomicU64,
    direct_attempts: AtomicUsize,
    fail_direct: AtomicBool,
    leases: Arc<AtomicUsize>,
}
#[async_trait]
impl SocketProtector for Protector {
    fn protect(&self, _: SocketHandle) -> Result<(), String> {
        Ok(())
    }
    async fn protect_for_target(
        &self,
        _: SocketHandle,
        remote: SocketAddr,
        _: DirectProtocol,
    ) -> Result<DirectEgressLease, String> {
        assert!(
            remote.ip().is_loopback(),
            "fixtures may only open loopback sockets"
        );
        if remote == self.dns {
            return Ok(DirectEgressLease::default());
        }
        self.direct_attempts.fetch_add(1, Ordering::SeqCst);
        if self.fail_direct.load(Ordering::SeqCst) {
            return Err("fixture direct egress denied".into());
        }
        self.leases.fetch_add(1, Ordering::SeqCst);
        Ok(DirectEgressLease::hold(Lease(self.leases.clone())))
    }
    fn physical_dns_servers(&self) -> Vec<SocketAddr> {
        vec![self.dns]
    }
    fn network_generation(&self) -> Option<u64> {
        Some(self.generation.load(Ordering::SeqCst))
    }
}
#[derive(Default)]
struct Associations {
    opened: AtomicUsize,
    sent: Mutex<Vec<TcpTarget>>,
    sent_signal: Notify,
    fail_open: AtomicBool,
    fail_send: AtomicBool,
    silent: AtomicBool,
    pause_open: AtomicBool,
    open_started: Notify,
    resume_open: Notify,
    closed: CancellationToken,
}
struct Factory(Arc<Associations>);
struct Association {
    observations: Arc<Associations>,
    sender: mpsc::Sender<(TcpTarget, Bytes)>,
    receiver: tokio::sync::Mutex<mpsc::Receiver<(TcpTarget, Bytes)>>,
}
#[async_trait]
impl crate::proxy_udp::UdpFactory for Factory {
    async fn open(
        &self,
        _: &CancellationToken,
        _: Instant,
    ) -> Result<Arc<dyn crate::proxy_udp::UdpAssociation>, DialError> {
        self.0.opened.fetch_add(1, Ordering::SeqCst);
        self.0.open_started.notify_one();
        if self.0.pause_open.load(Ordering::SeqCst) {
            self.0.resume_open.notified().await;
        }
        if self.0.fail_open.load(Ordering::SeqCst) {
            return Err(DialError::Rejected(7));
        }
        let (sender, receiver) = mpsc::channel(16);
        Ok(Arc::new(Association {
            observations: self.0.clone(),
            sender,
            receiver: tokio::sync::Mutex::new(receiver),
        }))
    }
}
#[async_trait]
impl crate::proxy_udp::UdpAssociation for Association {
    async fn send(&self, target: &TcpTarget, payload: &[u8]) -> Result<(), DialError> {
        self.observations.sent.lock().unwrap().push(target.clone());
        self.observations.sent_signal.notify_one();
        if self.observations.fail_send.load(Ordering::SeqCst) {
            return Err(DialError::Closed);
        }
        if self.observations.silent.load(Ordering::SeqCst) {
            return Ok(());
        }
        self.sender
            .send((target.clone(), Bytes::copy_from_slice(payload)))
            .await
            .map_err(|_| DialError::Closed)
    }
    async fn recv(&self) -> Result<(TcpTarget, Bytes), DialError> {
        tokio::select! {
            _ = self.observations.closed.cancelled() => Err(DialError::Closed),
            result = async { self.receiver.lock().await.recv().await } => result.ok_or(DialError::Closed),
        }
    }
}
fn packet(source: SocketAddr, remote: SocketAddr, payload: &[u8]) -> Bytes {
    let mut udp = vec![0; 8];
    udp[..2].copy_from_slice(&source.port().to_be_bytes());
    udp[2..4].copy_from_slice(&remote.port().to_be_bytes());
    udp[4..6].copy_from_slice(&((payload.len() + 8) as u16).to_be_bytes());
    udp.extend_from_slice(payload);
    super::super::tun_wire::ip_packet(source.ip(), remote.ip(), 17, udp)
}
struct Fixture {
    bridge: super::super::tun::TunBridge,
    io: super::super::L4TunIo,
    protector: Arc<Protector>,
    associations: Arc<Associations>,
    direct_packets: Arc<AtomicUsize>,
    remote: SocketAddr,
    peers: Vec<AbortOnDropHandle<()>>,
    budget: Arc<super::super::BufferBudget>,
    metrics: Arc<super::super::L4Metrics>,
    services: ProxyServices,
    _health: watch::Sender<RuntimeHealth>,
}
impl Fixture {
    async fn new(final_udp: bool, ttl: u32) -> Self {
        Self::with_quic_port(final_udp, ttl, false).await
    }
    async fn with_quic_port(final_udp: bool, ttl: u32, loopback_quic: bool) -> Self {
        let dns = tokio::net::UdpSocket::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let echo = tokio::net::UdpSocket::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let remote = echo.local_addr().unwrap();
        let protector = Arc::new(Protector {
            dns: dns.local_addr().unwrap(),
            generation: AtomicU64::new(9),
            direct_attempts: AtomicUsize::new(0),
            fail_direct: AtomicBool::new(false),
            leases: Arc::new(AtomicUsize::new(0)),
        });
        let dns_peer = AbortOnDropHandle::new(tokio::spawn(async move {
            let mut buffer = [0; 4096];
            loop {
                let Ok((length, source)) = dns.recv_from(&mut buffer).await else {
                    break;
                };
                if dns
                    .send_to(&answer(&buffer[..length], ttl), source)
                    .await
                    .is_err()
                {
                    break;
                }
            }
        }));
        let direct_packets = Arc::new(AtomicUsize::new(0));
        let observed = direct_packets.clone();
        let echo_peer = AbortOnDropHandle::new(tokio::spawn(async move {
            let mut buffer = [0; 1280];
            loop {
                let Ok((length, source)) = echo.recv_from(&mut buffer).await else {
                    break;
                };
                observed.fetch_add(1, Ordering::SeqCst);
                if echo.send_to(&buffer[..length], source).await.is_err() {
                    break;
                }
            }
        }));
        let mut profile = Profile {
            data_plane: DataPlaneMode::ConnectIp,
            mtu: 1280,
            bypass_domains: vec!["direct.test".into()],
            ..Default::default()
        };
        profile.frontends.tunnel = true;
        let policy = Arc::new(
            GeoDirectPolicy::disabled()
                .with_custom_rules(&profile)
                .unwrap(),
        );
        let cancellation = CancellationToken::new();
        let metrics = Arc::new(super::super::L4Metrics::default());
        let budget = Arc::new(super::super::BufferBudget::new(
            super::super::Limits::platform().buffers,
            metrics.clone(),
            Arc::new(Notify::new()),
        ));
        let dialer = Arc::new(TunnelDns);
        let stream_dns = Arc::new(StreamDns::new(
            dialer.clone(),
            protector.clone(),
            cancellation.clone(),
            metrics.clone(),
        ));
        let (health_sender, health) = watch::channel(RuntimeHealth::Connected {
            path: RuntimePath {
                transport: Transport::Http3,
                endpoint_family: AddressFamily::Ipv4,
                ipv4_available: true,
                ipv6_available: true,
            },
            reconnect_count: 0,
        });
        let associations = Arc::new(Associations::default());
        let services = ProxyServices {
            traffic_policy: Arc::new(if loopback_quic {
                crate::application_traffic::ApplicationTrafficPolicy::for_loopback_quic(
                    remote.port(),
                )
            } else {
                crate::application_traffic::ApplicationTrafficPolicy::default()
            }),
            admission: None,
            dialer,
            udp: final_udp.then(|| {
                Arc::new(Factory(associations.clone())) as Arc<dyn crate::proxy_udp::UdpFactory>
            }),
            resolver: Resolver::for_streams(
                stream_dns.clone(),
                profile.dns_servers.clone(),
                ProxyDnsMode::Remote,
                protector.clone(),
            ),
            protector: protector.clone(),
            geo_policy: policy,
            counters: Arc::default(),
            cancellation,
            health,
        };
        let mut bridge = super::super::tun::TunBridge::start(
            &profile,
            services.clone(),
            stream_dns,
            budget.clone(),
            metrics.clone(),
            crate::NetworkQualityTelemetry::default(),
        )
        .await
        .unwrap();
        let io = bridge.attach().unwrap();
        Self {
            bridge,
            io,
            protector,
            associations,
            direct_packets,
            remote,
            peers: vec![dns_peer, echo_peer],
            budget,
            metrics,
            services,
            _health: health_sender,
        }
    }
    async fn dns(&mut self, name: &str) {
        let request = query(name);
        self.io
            .send_owned_packet(packet(
                "192.0.2.44:42000".parse().unwrap(),
                SocketAddr::new(crate::SPLIT_DNS_IPV4.into(), 53),
                &request,
            ))
            .await
            .unwrap();
        let response = timeout(Duration::from_secs(2), self.io.receive_packet())
            .await
            .unwrap()
            .unwrap();
        let meta = NatPacket::parse(&response).unwrap();
        crate::split_dns::validate_response_bytes(&request, &response[meta.transport_offset + 8..])
            .unwrap();
    }
    async fn udp(&mut self) {
        self.io
            .send_owned_packet(packet(
                "192.0.2.44:42001".parse().unwrap(),
                self.remote,
                b"payload",
            ))
            .await
            .unwrap();
        let response = timeout(Duration::from_secs(2), self.io.receive_packet())
            .await
            .unwrap()
            .unwrap();
        let meta = NatPacket::parse(&response).unwrap();
        assert_eq!(&response[meta.transport_offset + 8..], b"payload");
    }
    async fn shutdown(mut self) {
        self.bridge.shutdown().await;
        assert_eq!(self.protector.leases.load(Ordering::SeqCst), 0);
        drop(self.io);
        drop(self.bridge);
        assert_eq!(
            self.budget.available(),
            super::super::Limits::platform().buffers
        );
        for peer in self.peers {
            peer.abort();
            let _ = peer.await;
        }
    }
}

#[tokio::test]
async fn domain_dns_hint_routes_tun_udp_direct_without_opening_final_association() {
    for final_udp in [false, true] {
        let mut fixture = Fixture::new(final_udp, 60).await;
        fixture.dns("direct.test").await;
        fixture.udp().await;
        assert_eq!(fixture.direct_packets.load(Ordering::SeqCst), 1);
        assert_eq!(fixture.protector.direct_attempts.load(Ordering::SeqCst), 1);
        assert_eq!(fixture.associations.opened.load(Ordering::SeqCst), 0);
        fixture.shutdown().await;
    }
}

#[tokio::test]
async fn conflicting_domain_hint_returns_existing_udp_worker_to_final_exit() {
    let mut fixture = Fixture::new(true, 60).await;
    fixture.dns("direct.test").await;
    fixture.udp().await;
    fixture.dns("tunnel.test").await;
    fixture.udp().await;
    assert_eq!(fixture.direct_packets.load(Ordering::SeqCst), 1);
    assert_eq!(
        *fixture.associations.sent.lock().unwrap(),
        vec![TcpTarget::address(fixture.remote)]
    );
    fixture.shutdown().await;
}

#[tokio::test]
async fn changed_generation_invalidates_domain_udp_hint_before_sending() {
    let mut fixture = Fixture::new(true, 60).await;
    fixture.dns("direct.test").await;
    fixture.protector.generation.store(10, Ordering::SeqCst);
    fixture.udp().await;
    assert_eq!(fixture.protector.direct_attempts.load(Ordering::SeqCst), 0);
    assert_eq!(
        *fixture.associations.sent.lock().unwrap(),
        vec![TcpTarget::address(fixture.remote)]
    );
    fixture.shutdown().await;
}

#[tokio::test]
async fn zero_ttl_domain_answer_does_not_authorize_direct_udp() {
    let mut fixture = Fixture::new(true, 0).await;
    fixture.dns("direct.test").await;
    fixture.udp().await;
    assert_eq!(fixture.protector.direct_attempts.load(Ordering::SeqCst), 0);
    assert_eq!(
        *fixture.associations.sent.lock().unwrap(),
        vec![TcpTarget::address(fixture.remote)]
    );
    fixture.shutdown().await;
}

#[tokio::test]
async fn unavailable_policy_open_and_send_reject_tun_udp_in_both_families() {
    for (source, remote) in [
        ("192.0.2.44:42001", "203.0.113.44:443"),
        ("[2001:db8::44]:42001", "[2001:db8::55]:443"),
    ] {
        for failure in ["missing", "policy", "open", "send"] {
            let mut fixture = Fixture::new(failure != "missing", 60).await;
            fixture
                .services
                .traffic_policy
                .set_disable_quic(failure == "policy");
            fixture
                .associations
                .fail_open
                .store(failure == "open", Ordering::SeqCst);
            fixture
                .associations
                .fail_send
                .store(failure == "send", Ordering::SeqCst);
            let request = packet(source.parse().unwrap(), remote.parse().unwrap(), b"initial");
            let meta = NatPacket::parse(&request).unwrap();
            fixture.io.send_owned_packet(request.clone()).await.unwrap();
            let response = timeout(Duration::from_secs(2), fixture.io.receive_packet())
                .await
                .unwrap()
                .unwrap();
            assert_eq!(
                response,
                super::super::tun_wire::udp_unreachable(&request, &meta).unwrap(),
                "{failure}: {source}"
            );
            assert_eq!(fixture.metrics.snapshot().udp_rejected, 1);
            if matches!(failure, "missing" | "policy") {
                assert_eq!(fixture.associations.opened.load(Ordering::SeqCst), 0);
            }
            fixture.shutdown().await;
        }
    }
}

#[tokio::test]
async fn quic_policy_keeps_direct_udp_dns_and_other_udp_available() {
    let mut fixture = Fixture::with_quic_port(true, 60, true).await;
    fixture.services.traffic_policy.set_disable_quic(true);
    // The test-only port matcher avoids binding privileged UDP 443.
    fixture.dns("direct.test").await;
    fixture.udp().await;
    assert_eq!(fixture.direct_packets.load(Ordering::SeqCst), 1);
    assert_eq!(fixture.associations.opened.load(Ordering::SeqCst), 0);
    fixture.shutdown().await;

    let mut fixture = Fixture::new(true, 60).await;
    fixture.services.traffic_policy.set_disable_quic(true);
    fixture.dns("tunnel.test").await;
    fixture.udp().await;
    assert_eq!(fixture.associations.sent.lock().unwrap().len(), 1);
    assert_eq!(fixture.metrics.snapshot().udp_rejected, 0);
    fixture.shutdown().await;
}

#[tokio::test]
async fn failed_direct_egress_without_a_final_udp_factory_returns_unreachable() {
    let mut fixture = Fixture::new(false, 60).await;
    fixture.dns("direct.test").await;
    fixture.protector.fail_direct.store(true, Ordering::SeqCst);
    let request = packet(
        "192.0.2.44:42001".parse().unwrap(),
        fixture.remote,
        b"direct",
    );
    let meta = NatPacket::parse(&request).unwrap();
    fixture.io.send_owned_packet(request.clone()).await.unwrap();
    let response = timeout(Duration::from_secs(2), fixture.io.receive_packet())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        response,
        super::super::tun_wire::udp_unreachable(&request, &meta).unwrap()
    );
    assert_eq!(fixture.protector.direct_attempts.load(Ordering::SeqCst), 1);
    assert_eq!(fixture.associations.opened.load(Ordering::SeqCst), 0);
    fixture.shutdown().await;
}

#[tokio::test]
async fn association_receive_failure_rejects_pending_datagram_without_cancelling_worker_early() {
    let mut fixture = Fixture::new(true, 60).await;
    fixture.associations.silent.store(true, Ordering::SeqCst);
    let request = packet(
        "192.0.2.44:42001".parse().unwrap(),
        "203.0.113.44:8443".parse().unwrap(),
        b"pending",
    );
    let meta = NatPacket::parse(&request).unwrap();
    fixture.io.send_owned_packet(request.clone()).await.unwrap();
    timeout(
        Duration::from_secs(2),
        fixture.associations.sent_signal.notified(),
    )
    .await
    .unwrap();
    fixture.associations.closed.cancel();
    let response = timeout(Duration::from_secs(2), fixture.io.receive_packet())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        response,
        super::super::tun_wire::udp_unreachable(&request, &meta).unwrap()
    );
    assert_eq!(fixture.metrics.snapshot().udp_rejected, 1);
    fixture.shutdown().await;
}

#[tokio::test]
async fn normal_runtime_cancellation_discards_pending_quotes_without_network_errors() {
    let fixture = Fixture::new(true, 60).await;
    fixture.associations.silent.store(true, Ordering::SeqCst);
    fixture
        .io
        .send_owned_packet(packet(
            "192.0.2.44:42001".parse().unwrap(),
            "203.0.113.44:8443".parse().unwrap(),
            b"pending",
        ))
        .await
        .unwrap();
    timeout(
        Duration::from_secs(2),
        fixture.associations.sent_signal.notified(),
    )
    .await
    .unwrap();
    let metrics = fixture.metrics.clone();
    fixture.services.cancellation.cancel();
    fixture.shutdown().await;
    assert_eq!(metrics.snapshot().udp_rejected, 0);
}

#[tokio::test]
async fn failed_relay_open_rejects_the_current_and_sixteen_queued_datagrams() {
    let fixture = Fixture::new(true, 60).await;
    fixture.associations.fail_open.store(true, Ordering::SeqCst);
    fixture
        .associations
        .pause_open
        .store(true, Ordering::SeqCst);
    let (sender, receiver) = mpsc::channel(16);
    let (replies, mut received) = mpsc::channel(32);
    let replies = super::super::performance::MeasuredSender::new(replies, Arc::default());
    let rejector = super::super::tun_reject::UdpRejector::new(
        replies.clone(),
        fixture.metrics.clone(),
        fixture.services.cancellation.clone(),
    );
    let services = fixture.services.clone();
    let budget = fixture.budget.clone();
    let cancellation = services.cancellation.child_token();
    let worker = tokio::spawn(async move {
        super::worker(
            receiver,
            services,
            Arc::new(crate::split_dns::DnsRouteCache::default()),
            replies,
            rejector,
            budget,
            &cancellation,
            Duration::from_secs(60),
            1280,
        )
        .await;
    });
    let request = packet(
        "192.0.2.44:42001".parse().unwrap(),
        "203.0.113.44:8443".parse().unwrap(),
        b"queued",
    );
    let meta = NatPacket::parse(&request).unwrap();
    sender.send((meta, request.clone())).await.unwrap();
    timeout(
        Duration::from_secs(2),
        fixture.associations.open_started.notified(),
    )
    .await
    .unwrap();
    for _ in 0..16 {
        sender.try_send((meta, request.clone())).unwrap();
    }
    fixture.associations.resume_open.notify_one();
    timeout(Duration::from_secs(2), worker)
        .await
        .unwrap()
        .unwrap();
    for _ in 0..17 {
        assert!(received.try_recv().is_ok());
    }
    assert!(received.try_recv().is_err());
    assert!(sender.is_closed());
    assert_eq!(fixture.metrics.snapshot().udp_rejected, 17);
    fixture.shutdown().await;
}

#[tokio::test(start_paused = true)]
async fn separate_tun_udp_workers_share_one_icmp_rate_limit() {
    let mut fixture = Fixture::new(true, 60).await;
    fixture.services.traffic_policy.set_disable_quic(true);
    for port in 42000..42040 {
        fixture
            .io
            .send_owned_packet(packet(
                SocketAddr::new("192.0.2.44".parse().unwrap(), port),
                "203.0.113.44:443".parse().unwrap(),
                b"initial",
            ))
            .await
            .unwrap();
    }
    for _ in 0..256 {
        if fixture.metrics.snapshot().udp_rejected == 40 {
            break;
        }
        tokio::task::yield_now().await;
    }
    assert_eq!(fixture.metrics.snapshot().udp_rejected, 40);
    for _ in 0..32 {
        assert!(fixture.io.try_receive_packet().unwrap().is_some());
    }
    assert!(fixture.io.try_receive_packet().unwrap().is_none());
    assert_eq!(fixture.associations.opened.load(Ordering::SeqCst), 0);
    fixture.shutdown().await;
}

#[tokio::test]
async fn rejected_quic_targets_do_not_exhaust_normal_or_direct_udp_target_slots() {
    let mut fixture = Fixture::new(true, 60).await;
    fixture.services.traffic_policy.set_disable_quic(true);
    let source: SocketAddr = "192.0.2.44:42001".parse().unwrap();
    for last_octet in 0..=255_u8 {
        fixture
            .io
            .send_owned_packet(packet(
                source,
                SocketAddr::from(([203, 0, 113, last_octet], 443)),
                b"blocked",
            ))
            .await
            .unwrap();
        // Wait for each rejection so pump/worker queue overflow cannot stand
        // in for the policy path whose target-table accounting is under test.
        timeout(Duration::from_secs(2), async {
            while fixture.metrics.snapshot().udp_rejected < u64::from(last_octet) + 1 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
    }
    while fixture.io.try_receive_packet().unwrap().is_some() {}
    let remote: SocketAddr = "203.0.113.44:8443".parse().unwrap();
    fixture
        .io
        .send_owned_packet(packet(source, remote, b"allowed"))
        .await
        .unwrap();
    let response = timeout(Duration::from_secs(2), fixture.io.receive_packet())
        .await
        .unwrap()
        .unwrap();
    let meta = NatPacket::parse(&response).expect("non-443 UDP must remain available");
    assert_eq!(&response[meta.transport_offset + 8..], b"allowed");
    fixture.dns("direct.test").await;
    fixture.udp().await;
    assert_eq!(fixture.direct_packets.load(Ordering::SeqCst), 1);
    assert_eq!(
        *fixture.associations.sent.lock().unwrap(),
        vec![TcpTarget::address(remote)]
    );
    assert_eq!(fixture.metrics.snapshot().udp_rejected, 256);
    fixture.shutdown().await;
}

#[tokio::test(start_paused = true)]
async fn blocked_quic_sources_do_not_reserve_tcp_slots_or_worker_buffers() {
    let mut fixture = Fixture::new(true, 60).await;
    fixture.services.traffic_policy.set_disable_quic(true);
    let initial_budget = fixture.budget.available();
    let source_ip = "192.0.2.44".parse().unwrap();
    for index in 0..super::super::Limits::platform().active {
        fixture
            .io
            .send_owned_packet(packet(
                SocketAddr::new(source_ip, u16::try_from(42000 + index).unwrap()),
                "203.0.113.44:443".parse().unwrap(),
                b"blocked",
            ))
            .await
            .unwrap();
        // Keep only one packet in flight: a full ingress queue must not make
        // this test pass without exercising each distinct source's admission.
        for _ in 0..256 {
            if fixture.metrics.snapshot().udp_rejected == (index + 1) as u64 {
                break;
            }
            tokio::task::yield_now().await;
        }
        assert_eq!(fixture.metrics.snapshot().udp_rejected, (index + 1) as u64);
    }
    assert_eq!(fixture.associations.opened.load(Ordering::SeqCst), 0);
    assert_eq!(
        fixture.budget.available(),
        initial_budget,
        "policy rejection must happen before reserving a UDP worker or a shared TCP slot"
    );
    while fixture.io.try_receive_packet().unwrap().is_some() {}
    fixture
        .io
        .send_owned_packet(packet(
            SocketAddr::new(source_ip, 60000),
            "203.0.113.44:8443".parse().unwrap(),
            b"still available",
        ))
        .await
        .unwrap();
    let response = timeout(Duration::from_secs(2), fixture.io.receive_packet())
        .await
        .unwrap()
        .unwrap();
    let meta = NatPacket::parse(&response).expect("new non-443 UDP still has a worker slot");
    assert_eq!(&response[meta.transport_offset + 8..], b"still available");
    assert_eq!(fixture.associations.opened.load(Ordering::SeqCst), 1);
    fixture.shutdown().await;
}
