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
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
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
        self.sender
            .send((target.clone(), Bytes::copy_from_slice(payload)))
            .await
            .map_err(|_| DialError::Closed)
    }
    async fn recv(&self) -> Result<(TcpTarget, Bytes), DialError> {
        self.receiver
            .lock()
            .await
            .recv()
            .await
            .ok_or(DialError::Closed)
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
    _health: watch::Sender<RuntimeHealth>,
}
impl Fixture {
    async fn new(final_udp: bool, ttl: u32) -> Self {
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
            traffic_policy: Arc::default(),
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
            services,
            stream_dns,
            budget.clone(),
            metrics,
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
