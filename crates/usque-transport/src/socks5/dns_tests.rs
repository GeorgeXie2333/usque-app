//! Loopback SOCKS clients with memory-only final proxy peers; no TUN or routes.
use super::*;
use crate::dns_stream::StreamDns;
use crate::proxy_exit::ProxyDialer;
use crate::proxy_udp::{SocksFactory, UdpFactory};
use crate::tcp::{DialError, FlowClass, TcpDialer, TcpIo, TcpTarget};
use async_trait::async_trait;
use std::io;
use std::pin::Pin;
use std::sync::Mutex as StdMutex;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering};
use std::task::{Context, Poll};
use tokio::io::{AsyncRead, AsyncWrite, DuplexStream, ReadBuf};
use tokio::sync::Notify;
use tokio_util::task::AbortOnDropHandle;
use usque_core::chain_exit::{ChainSource, ImportSecrets, ProxyAuthMode, ProxyExitConfiguration};
use usque_core::{AddressFamily, DataPlaneMode, ProxyDnsMode, Transport};

struct MemoryStream(DuplexStream);
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
impl TcpIo for MemoryStream {
    fn local_addr(&self) -> io::Result<SocketAddr> {
        Ok("192.0.2.1:49152".parse().unwrap())
    }
}

struct ProxyPeer {
    source: ChainSource,
    server_connections: StdMutex<Vec<TcpTarget>>,
    dns_targets: StdMutex<Vec<TcpTarget>>,
    associate_commands: AtomicUsize,
    udp_reply: AtomicU8,
    stall_associate: AtomicBool,
    associate_closed: Notify,
    dns_queries: AtomicUsize,
    stalled_queries: AtomicUsize,
    stall_dns: AtomicBool,
    refuse_dns: AtomicBool,
    refuse_server: AtomicBool,
    auth_reply: AtomicU8,
    query_started: Notify,
    query_closed: Notify,
    cancellation: CancellationToken,
}

impl ProxyPeer {
    async fn serve(self: Arc<Self>, mut stream: DuplexStream) -> io::Result<()> {
        let target = if self.source == ChainSource::HttpProxy {
            let mut header = Vec::new();
            loop {
                match stream.read_u8().await {
                    Ok(byte) => header.push(byte),
                    Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => return Ok(()),
                    Err(error) => return Err(error),
                }
                if header.ends_with(b"\r\n\r\n") {
                    break;
                }
                assert!(header.len() <= 1024);
            }
            let header = String::from_utf8(header).unwrap();
            let authority = header.lines().next().unwrap().split(' ').nth(1).unwrap();
            let (host, port) = authority.rsplit_once(':').unwrap();
            let target =
                TcpTarget::new(host.trim_matches(['[', ']']), port.parse().unwrap()).unwrap();
            if self.refuse_dns.load(Ordering::SeqCst) {
                self.dns_targets.lock().unwrap().push(target);
                stream.write_all(b"HTTP/1.1 403 Forbidden\r\n\r\n").await?;
                return Ok(());
            }
            stream
                .write_all(b"HTTP/1.1 200 Established\r\n\r\n")
                .await?;
            target
        } else {
            let mut greeting = [0; 3];
            stream.read_exact(&mut greeting).await?;
            assert_eq!(greeting, [5, 1, 0]);
            let method = self.auth_reply.load(Ordering::SeqCst);
            stream.write_all(&[5, method]).await?;
            if method != 0 {
                return Ok(());
            }
            let mut header = [0; 4];
            match stream.read_exact(&mut header).await {
                Ok(_) => {}
                Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => return Ok(()),
                Err(error) => return Err(error),
            }
            let target = read_target(&mut stream, header[3]).await?;
            if header[1] == COMMAND_UDP_ASSOCIATE {
                self.associate_commands.fetch_add(1, Ordering::SeqCst);
                if self.stall_associate.load(Ordering::SeqCst) {
                    let closed = stream.read_u8().await;
                    assert!(
                        closed.is_err(),
                        "timed-out association must close its control stream"
                    );
                    self.associate_closed.notify_one();
                    return Ok(());
                }
                let unsupported = self.udp_reply.load(Ordering::SeqCst);
                stream
                    .write_all(&[5, unsupported, 0, 1, 0, 0, 0, 0, 0, 0])
                    .await?;
                return Ok(());
            }
            assert_eq!(header[1], COMMAND_CONNECT);
            stream
                .write_all(&[5, 0, 0, 1, 192, 0, 2, 1, 0xc0, 0x00])
                .await?;
            target
        };
        assert_eq!(target.host_port().1, 53, "ordinary UDP must never open TCP");
        self.dns_targets.lock().unwrap().push(target);
        loop {
            let length = match stream.read_u16().await {
                Ok(length) => length,
                Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => return Ok(()),
                Err(error) => return Err(error),
            };
            let mut request = vec![0; usize::from(length)];
            stream.read_exact(&mut request).await?;
            crate::split_dns::validate_query_bytes(&request).unwrap();
            self.dns_queries.fetch_add(1, Ordering::SeqCst);
            if self.stall_dns.load(Ordering::SeqCst) {
                self.stalled_queries.fetch_add(1, Ordering::SeqCst);
                self.query_started.notify_one();
                let result = stream.read_u8().await;
                self.stalled_queries.fetch_sub(1, Ordering::SeqCst);
                self.query_closed.notify_one();
                assert!(result.is_err(), "cancelled DNS query stream must close");
                return Ok(());
            }
            let response = answer(&request);
            stream.write_u16(response.len() as u16).await?;
            stream.write_all(&response).await?;
        }
    }
}

#[async_trait]
impl TcpDialer for Arc<ProxyPeer> {
    async fn connect(
        &self,
        target: TcpTarget,
        _deadline: Instant,
        _cancel: &CancellationToken,
        _class: FlowClass,
    ) -> Result<crate::tcp::TcpStream, DialError> {
        self.server_connections.lock().unwrap().push(target);
        if self.refuse_server.load(Ordering::SeqCst) {
            return Err(DialError::Refused);
        }
        let (client, peer) = tokio::io::duplex(8192);
        let server = self.clone();
        let cancellation = self.cancellation.clone();
        tokio::spawn(async move {
            tokio::select! {
                _ = cancellation.cancelled() => {},
                result = server.serve(peer) => { result.unwrap(); }
            }
        });
        Ok(Box::new(MemoryStream(client)))
    }
}

async fn read_target(stream: &mut DuplexStream, atyp: u8) -> io::Result<TcpTarget> {
    let host = match atyp {
        ADDRESS_IPV4 => {
            let mut bytes = [0; 4];
            stream.read_exact(&mut bytes).await?;
            Ipv4Addr::from(bytes).to_string()
        }
        ADDRESS_IPV6 => {
            let mut bytes = [0; 16];
            stream.read_exact(&mut bytes).await?;
            Ipv6Addr::from(bytes).to_string()
        }
        ADDRESS_DOMAIN => {
            let length = stream.read_u8().await?;
            let mut bytes = vec![0; usize::from(length)];
            stream.read_exact(&mut bytes).await?;
            String::from_utf8(bytes).unwrap()
        }
        _ => panic!("unexpected target address type"),
    };
    let port = stream.read_u16().await?;
    // UDP ASSOCIATE uses an unspecified zero-port target.
    Ok(if port == 0 {
        TcpTarget::address(SocketAddr::new(host.parse().unwrap(), 0))
    } else {
        TcpTarget::new(&host, port).unwrap()
    })
}

struct Fixture {
    frontend: Socks5Frontend,
    peer: Arc<ProxyPeer>,
    proxy: Arc<ProxyDialer>,
    dns: Arc<StreamDns>,
    cancellation: CancellationToken,
    physical: Arc<RejectedPhysicalNetwork>,
    _stack: Option<AbortOnDropHandle<()>>,
}

#[derive(Default)]
struct RejectedPhysicalNetwork(AtomicUsize);
impl crate::SocketProtector for RejectedPhysicalNetwork {
    fn protect(&self, _: crate::SocketHandle) -> Result<(), String> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Err("fixture forbids physical egress".into())
    }
    fn resolve(&self, _: &str, _: u16) -> Result<Vec<SocketAddr>, String> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Err("fixture forbids physical DNS".into())
    }
}

impl Fixture {
    async fn new(source: ChainSource) -> Self {
        Self::with_mode(source, ProxyDnsMode::EdgeResolved).await
    }

    async fn with_mode(source: ChainSource, mode: ProxyDnsMode) -> Self {
        let cancellation = CancellationToken::new();
        let peer = Arc::new(ProxyPeer {
            source,
            server_connections: StdMutex::default(),
            dns_targets: StdMutex::default(),
            associate_commands: AtomicUsize::new(0),
            udp_reply: AtomicU8::new(
                serde_json::from_str::<serde_json::Value>(include_str!(
                    "../../tests/fixtures/proxy-exit/contract.json"
                ))
                .unwrap()["socks5_udp_unsupported"]
                    .as_u64()
                    .unwrap() as u8,
            ),
            stall_associate: AtomicBool::new(false),
            associate_closed: Notify::new(),
            dns_queries: AtomicUsize::new(0),
            stalled_queries: AtomicUsize::new(0),
            stall_dns: AtomicBool::new(false),
            refuse_dns: AtomicBool::new(false),
            refuse_server: AtomicBool::new(false),
            auth_reply: AtomicU8::new(0),
            query_started: Notify::new(),
            query_closed: Notify::new(),
            cancellation: CancellationToken::new(),
        });
        let (_, health) = watch::channel(RuntimeHealth::Connected {
            path: RuntimePath {
                transport: Transport::Http3,
                endpoint_family: AddressFamily::Ipv4,
                ipv4_available: true,
                ipv6_available: true,
            },
            reconnect_count: 0,
        });
        let mut network = crate::InternalNetwork::for_streams(
            Arc::new(peer.clone()),
            health.clone(),
            cancellation.clone(),
        );
        let mut stack_task = None;
        if source == ChainSource::Socks5Proxy {
            use ts_netstack_smoltcp::netcore::HasChannel;
            let (config, _) = crate::netstack::proxy_netstack_config(&Profile::default());
            let (stack, _pipe) = crate::netstack::bounded_piped(config);
            network = network.with_test_packets(
                stack.command_channel(),
                "192.0.2.1".parse().unwrap(),
                Ipv6Addr::UNSPECIFIED,
            );
            stack_task = Some(AbortOnDropHandle::new(stack.spawn_tokio()));
        }
        let server_port = if source == ChainSource::HttpProxy {
            8080
        } else {
            1080
        };
        let mut secrets = ImportSecrets::default();
        secrets.proxy = Some(ProxyExitConfiguration {
            host: "198.51.100.10".into(),
            port: server_port,
            auth_mode: ProxyAuthMode::None,
            dns_servers: vec![],
        });
        let parsed = usque_core::chain_exit::ValidatedProfile::parse(source, &secrets).unwrap();
        let summary = parsed
            .summary("DNS test", uuid::Uuid::new_v4(), uuid::Uuid::new_v4())
            .unwrap();
        let prepared = usque_core::vpngate::PreparedProfile::imported(summary, parsed, secrets);
        let metrics = Arc::new(crate::l4::L4Metrics::default());
        let budget = Arc::new(crate::l4::BufferBudget::new(
            16 << 20,
            metrics.clone(),
            Arc::new(Notify::new()),
        ));
        let proxy = ProxyDialer::start(
            &prepared,
            network,
            None,
            cancellation.clone(),
            budget,
            Arc::default(),
            Instant::now() + REMOTE_CONNECT_TIMEOUT,
        )
        .await
        .unwrap();
        proxy.admitted.store(true, Ordering::Release);
        let protector = Arc::new(RejectedPhysicalNetwork::default());
        let dns = Arc::new(StreamDns::new(
            proxy.clone(),
            protector.clone(),
            cancellation.clone(),
            metrics,
        ));
        let profile = Profile {
            data_plane: DataPlaneMode::L4Proxy,
            ..Default::default()
        };
        let mut profile = profile;
        profile.proxy.dns_mode = mode;
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let services = crate::tcp::ProxyServices {
            traffic_policy: Arc::default(),
            admission: Some(Arc::new(crate::tcp::FrontendAdmission::new(
                proxy.budget.clone(),
                crate::l4::Limits::platform().active + crate::l4::Limits::platform().pending,
            ))),
            dialer: proxy.clone(),
            udp: (source == ChainSource::Socks5Proxy)
                .then(|| Arc::new(SocksFactory(proxy.clone())) as Arc<dyn UdpFactory>),
            resolver: Resolver::for_streams(
                dns.clone(),
                vec!["198.51.100.53".parse().unwrap()],
                mode,
                protector.clone(),
            ),
            protector: protector.clone(),
            geo_policy: Arc::default(),
            counters: Arc::default(),
            cancellation: cancellation.clone(),
            health,
        };
        let frontend =
            Socks5Frontend::activate_services(&profile, services, vec![listener]).unwrap();
        Self {
            frontend,
            peer,
            proxy,
            dns,
            cancellation,
            physical: protector,
            _stack: stack_task,
        }
    }

    async fn associate(&self) -> (TcpStream, TokioUdpSocket, SocketAddr) {
        let mut control = TcpStream::connect(self.frontend.listeners()[0])
            .await
            .unwrap();
        control.write_all(&[5, 1, 0]).await.unwrap();
        let mut authentication = [0; 2];
        control.read_exact(&mut authentication).await.unwrap();
        assert_eq!(authentication, [5, 0]);
        control
            .write_all(&[5, 3, 0, 1, 0, 0, 0, 0, 0, 0])
            .await
            .unwrap();
        let mut header = [0; 4];
        timeout(Duration::from_secs(2), control.read_exact(&mut header))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(header, [5, 0, 0, ADDRESS_IPV4]);
        let mut address = [0; 4];
        control.read_exact(&mut address).await.unwrap();
        let port = control.read_u16().await.unwrap();
        assert_ne!(port, 0);
        let relay = SocketAddr::new(Ipv4Addr::from(address).into(), port);
        assert!(relay.ip().is_loopback());
        let udp = TokioUdpSocket::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        (control, udp, relay)
    }

    async fn shutdown(&mut self) {
        self.frontend.shutdown().await;
        self.cancellation.cancel();
        self.dns.clear();
        self.peer.cancellation.cancel();
        timeout(Duration::from_secs(2), async {
            while self.proxy.active.available_permits() != crate::l4::Limits::platform().active
                || self.proxy.budget.available() != 16 << 20
            {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert_eq!(
            self.proxy.active.available_permits(),
            crate::l4::Limits::platform().active
        );
        assert_eq!(self.proxy.budget.available(), 16 << 20);
        assert_eq!(self.physical.0.load(Ordering::SeqCst), 0);
        let expected = format!(
            "198.51.100.10:{}",
            if self.peer.source == ChainSource::HttpProxy {
                8080
            } else {
                1080
            }
        );
        assert!(
            self.peer
                .server_connections
                .lock()
                .unwrap()
                .iter()
                .all(|t| t.authority() == expected)
        );
    }
}

fn query(id: u16) -> Vec<u8> {
    let mut bytes = vec![0, 0, 1, 0, 0, 1, 0, 0, 0, 0, 0, 0];
    bytes[..2].copy_from_slice(&id.to_be_bytes());
    bytes.extend_from_slice(b"\x07example\x04test\0\0\x01\0\x01");
    bytes
}
fn answer(query: &[u8]) -> Vec<u8> {
    let mut bytes = query.to_vec();
    bytes[2..4].copy_from_slice(&[0x81, 0x80]);
    bytes[6..8].copy_from_slice(&[0, 1]);
    bytes.extend_from_slice(&[0xc0, 12, 0, 1, 0, 1, 0, 0, 0, 60, 0, 4, 203, 0, 113, 7]);
    bytes
}
fn datagram(target: &TcpTarget, payload: &[u8]) -> Vec<u8> {
    let mut packet = vec![0, 0, 0];
    crate::proxy_exit::encode_target(target, &mut packet).unwrap();
    packet.extend_from_slice(payload);
    packet
}
async fn exchange(udp: &TokioUdpSocket, relay: SocketAddr, target: &TcpTarget, id: u16) {
    let request = query(id);
    udp.send_to(&datagram(target, &request), relay)
        .await
        .unwrap();
    let mut reply = [0; 4096];
    let (length, source) = timeout(Duration::from_secs(2), udp.recv_from(&mut reply))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(source, relay);
    let parsed = decode_udp_request(&reply[..length]).unwrap();
    let response_target = match parsed.target {
        Target::Address(ip) => TcpTarget::address(SocketAddr::new(ip, parsed.port)),
        Target::Domain(host) => TcpTarget::new(&host, parsed.port).unwrap(),
    };
    assert_eq!(&response_target, target);
    crate::split_dns::validate_response_bytes(&request, parsed.payload).unwrap();
    assert_eq!(parsed.payload, answer(&request));
}

#[tokio::test]
async fn http_exit_socks_udp_dns_uses_tcp_for_ipv4_ipv6_and_edge_domain() {
    let mut fixture = Fixture::new(ChainSource::HttpProxy).await;
    let (control, udp, relay) = fixture.associate().await;
    let targets = [
        TcpTarget::new("198.51.100.53", 53).unwrap(),
        TcpTarget::new("2001:db8::53", 53).unwrap(),
        TcpTarget::new("resolver.example", 53).unwrap(),
    ];
    // Winsock returns WSAEMSGSIZE for this packet. Reject it without closing
    // the control connection, then accept valid DNS on the same association.
    udp.send_to(&datagram(&targets[0], &vec![0; 20 * 1024]), relay)
        .await
        .unwrap();
    for (index, target) in targets.iter().enumerate() {
        exchange(&udp, relay, target, index as u16).await;
    }
    assert_eq!(*fixture.peer.dns_targets.lock().unwrap(), targets);
    assert_eq!(fixture.peer.associate_commands.load(Ordering::SeqCst), 0);
    for (target, payload) in [
        (TcpTarget::new("198.51.100.53", 443).unwrap(), query(8)),
        (
            TcpTarget::new("198.51.100.53", 53).unwrap(),
            b"malformed DNS".to_vec(),
        ),
    ] {
        udp.send_to(&datagram(&target, &payload), relay)
            .await
            .unwrap();
    }
    let mut invalid_target = vec![0, 0, 0, ADDRESS_DOMAIN, 8];
    invalid_target.extend_from_slice(b"bad/name");
    invalid_target.extend_from_slice(&53_u16.to_be_bytes());
    invalid_target.extend_from_slice(&query(9));
    udp.send_to(&invalid_target, relay).await.unwrap();
    let mut bytes = [0; 64];
    assert!(
        timeout(Duration::from_millis(150), udp.recv_from(&mut bytes))
            .await
            .is_err()
    );
    assert_eq!(fixture.peer.dns_queries.load(Ordering::SeqCst), 3);
    assert_eq!(*fixture.peer.dns_targets.lock().unwrap(), targets);
    drop(control);
    fixture.shutdown().await;
}

#[tokio::test]
async fn every_valid_udp_command_refusal_preserves_tcp_dns_only_associations() {
    for reply in 1..=8 {
        let mut fixture = Fixture::new(ChainSource::Socks5Proxy).await;
        fixture.peer.udp_reply.store(reply, Ordering::SeqCst);
        let target = TcpTarget::new("resolver.example", 53).unwrap();
        let request = query(u16::from(reply));
        let response = fixture
            .dns
            .query_target(
                target.clone(),
                &request,
                Instant::now() + Duration::from_secs(2),
            )
            .await
            .unwrap();
        assert_eq!(response, answer(&request));

        let (control, udp, relay) = fixture.associate().await;
        exchange(&udp, relay, &target, u16::from(reply) + 10).await;
        assert_eq!(fixture.peer.dns_queries.load(Ordering::SeqCst), 2);
        assert_eq!(fixture.peer.associate_commands.load(Ordering::SeqCst), 1);
        assert_eq!(
            fixture.proxy.status.borrow().proxy_udp.as_deref(),
            Some("unavailable")
        );
        // A DNS-only association never enables ordinary tunnel UDP.
        udp.send_to(
            &datagram(
                &TcpTarget::new("ordinary.example", 9000).unwrap(),
                b"payload",
            ),
            relay,
        )
        .await
        .unwrap();
        let mut response = [0; 64];
        assert!(
            timeout(Duration::from_millis(50), udp.recv_from(&mut response))
                .await
                .is_err()
        );
        assert_eq!(fixture.peer.dns_queries.load(Ordering::SeqCst), 2);
        drop(control);
        fixture.shutdown().await;
    }
}

#[tokio::test]
async fn udp_transport_authentication_and_invalid_replies_do_not_become_command_refusals() {
    for error in [
        DialError::Refused,
        DialError::Rejected(407),
        DialError::Protocol,
    ] {
        let mut fixture = Fixture::new(ChainSource::Socks5Proxy).await;
        match error {
            DialError::Refused => fixture.peer.refuse_server.store(true, Ordering::SeqCst),
            DialError::Rejected(407) => fixture.peer.auth_reply.store(255, Ordering::SeqCst),
            DialError::Protocol => fixture.peer.udp_reply.store(255, Ordering::SeqCst),
            _ => unreachable!(),
        }
        let result = SocksFactory(fixture.proxy.clone())
            .open(
                &fixture.cancellation,
                Instant::now() + Duration::from_secs(2),
            )
            .await;
        assert!(matches!(result, Err(actual) if actual == error));
        assert_eq!(
            fixture.proxy.status.borrow().proxy_udp.as_deref(),
            Some("unknown")
        );
        assert_eq!(
            fixture.proxy.cancellation.is_cancelled(),
            error == DialError::Rejected(407)
        );
        fixture.shutdown().await;
    }
}

#[tokio::test]
async fn upstream_socks_rejecting_udp_retains_independent_dns_only_associations() {
    let mut fixture = Fixture::new(ChainSource::Socks5Proxy).await;
    let (control_one, udp_one, relay_one) = fixture.associate().await;
    let (control_two, udp_two, relay_two) = fixture.associate().await;
    assert_ne!(relay_one, relay_two);
    assert_eq!(fixture.peer.associate_commands.load(Ordering::SeqCst), 1);
    assert_eq!(
        fixture.proxy.status.borrow().proxy_udp.as_deref(),
        Some("unavailable")
    );
    let target = TcpTarget::new("resolver.example", 53).unwrap();
    exchange(&udp_one, relay_one, &target, 11).await;
    exchange(&udp_two, relay_two, &target, 12).await;
    drop(control_one);
    exchange(&udp_two, relay_two, &target, 13).await;
    assert_eq!(fixture.peer.dns_queries.load(Ordering::SeqCst), 3);
    drop(control_two);
    fixture.shutdown().await;
}

#[tokio::test]
async fn associate_timeout_keeps_udp_unknown_and_dns_bound_to_the_final_exit() {
    let mut fixture = Fixture::new(ChainSource::Socks5Proxy).await;
    fixture.peer.stall_associate.store(true, Ordering::SeqCst);
    let before = fixture.proxy.budget.available();
    let result = SocksFactory(fixture.proxy.clone())
        .open(
            &fixture.cancellation,
            Instant::now() + Duration::from_millis(100),
        )
        .await;
    assert!(matches!(result, Err(DialError::Timeout)));
    timeout(
        Duration::from_secs(2),
        fixture.peer.associate_closed.notified(),
    )
    .await
    .unwrap();
    assert_eq!(fixture.peer.associate_commands.load(Ordering::SeqCst), 1);
    assert_eq!(
        fixture.proxy.status.borrow().proxy_udp.as_deref(),
        Some("unknown")
    );
    assert!(!fixture.proxy.cancellation.is_cancelled());
    assert_eq!(
        fixture.proxy.active.available_permits(),
        crate::l4::Limits::platform().active
    );
    assert_eq!(fixture.proxy.budget.available(), before);

    let target = TcpTarget::new("resolver.example", 53).unwrap();
    let request = query(91);
    assert_eq!(
        fixture
            .dns
            .query_target(
                target.clone(),
                &request,
                Instant::now() + Duration::from_secs(2)
            )
            .await
            .unwrap(),
        answer(&request)
    );
    assert_eq!(*fixture.peer.dns_targets.lock().unwrap(), vec![target]);
    assert_eq!(fixture.physical.0.load(Ordering::SeqCst), 0);
    assert_eq!(
        fixture.proxy.status.borrow().proxy_udp.as_deref(),
        Some("unknown")
    );
    fixture.shutdown().await;
}

#[tokio::test]
async fn socks_udp_rejection_does_not_resolve_ordinary_remote_domain_datagrams() {
    let mut fixture = Fixture::with_mode(ChainSource::Socks5Proxy, ProxyDnsMode::Remote).await;
    let (control, udp, relay) = fixture.associate().await;
    assert_eq!(fixture.peer.associate_commands.load(Ordering::SeqCst), 1);
    let target = TcpTarget::new("ordinary.example", 9000).unwrap();
    // Even a well-formed DNS payload only receives conversion on port 53.
    udp.send_to(&datagram(&target, &query(41)), relay)
        .await
        .unwrap();
    let mut reply = [0; 64];
    assert!(
        timeout(Duration::from_millis(150), udp.recv_from(&mut reply))
            .await
            .is_err()
    );
    assert!(fixture.peer.dns_targets.lock().unwrap().is_empty());
    assert_eq!(fixture.peer.dns_queries.load(Ordering::SeqCst), 0);
    // Only startup negotiation and the rejected ASSOCIATE reached the final proxy.
    assert_eq!(fixture.peer.server_connections.lock().unwrap().len(), 2);
    drop(control);
    fixture.shutdown().await;
}

#[tokio::test]
async fn dns_only_association_rejects_exhausted_local_buffers_and_releases_admission() {
    let mut fixture = Fixture::new(ChainSource::HttpProxy).await;
    let connection_bytes = 2 * crate::relay::RELAY_BUFFER_SIZE;
    let udp_bytes = 16 * 16 * 1024 * 2;
    let progress_headroom = (16 << 20) / 8;
    // Leave enough margin to admit the control connection, one byte too little
    // for its bounded UDP queues. The external lease models other active work.
    let pressure = fixture
        .proxy
        .budget
        .reserve((16 << 20) - progress_headroom - connection_bytes - udp_bytes + 1)
        .unwrap();
    let mut control = TcpStream::connect(fixture.frontend.listeners()[0])
        .await
        .unwrap();
    control.write_all(&[5, 1, 0]).await.unwrap();
    let mut authentication = [0; 2];
    timeout(
        Duration::from_secs(2),
        control.read_exact(&mut authentication),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(authentication, [5, 0]);
    control
        .write_all(&[5, 3, 0, 1, 0, 0, 0, 0, 0, 0])
        .await
        .unwrap();
    let mut reply = [0; 10];
    timeout(Duration::from_secs(2), control.read_exact(&mut reply))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        reply,
        [5, REPLY_GENERAL_FAILURE, 0, ADDRESS_IPV4, 0, 0, 0, 0, 0, 0]
    );
    assert!(
        timeout(Duration::from_secs(2), control.read_u8())
            .await
            .unwrap()
            .is_err()
    );
    assert_eq!(fixture.peer.server_connections.lock().unwrap().len(), 1);
    assert!(fixture.peer.dns_targets.lock().unwrap().is_empty());
    drop(pressure);
    drop(control);
    fixture.shutdown().await;
}

#[tokio::test]
async fn http_exit_refusing_dns_connect_returns_servfail_without_bypass() {
    let mut fixture = Fixture::new(ChainSource::HttpProxy).await;
    fixture.peer.refuse_dns.store(true, Ordering::SeqCst);
    let (control, udp, relay) = fixture.associate().await;
    let target = TcpTarget::new("198.51.100.53", 53).unwrap();
    let request = query(31);
    udp.send_to(&datagram(&target, &request), relay)
        .await
        .unwrap();
    let mut reply = [0; 4096];
    let (length, source) = timeout(Duration::from_secs(2), udp.recv_from(&mut reply))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(source, relay);
    let parsed = decode_udp_request(&reply[..length]).unwrap();
    assert!(
        matches!(parsed.target, Target::Address(ip) if ip == target.socket_address().unwrap().ip())
    );
    assert_eq!(parsed.port, 53);
    crate::split_dns::validate_response_bytes(&request, parsed.payload).unwrap();
    assert_eq!(parsed.payload[3] & 0x0f, 2); // SERVFAIL, with the original question and transaction ID.
    assert_eq!(
        fixture.peer.dns_targets.lock().unwrap().as_slice(),
        std::slice::from_ref(&target)
    );
    assert_eq!(fixture.peer.dns_queries.load(Ordering::SeqCst), 0);
    assert!(!fixture.proxy.status.borrow().tcp_connect_verified);
    assert!(fixture.proxy.is_ready());
    fixture.peer.refuse_dns.store(false, Ordering::SeqCst);
    exchange(&udp, relay, &target, 32).await;
    assert_eq!(fixture.peer.dns_queries.load(Ordering::SeqCst), 1);
    drop(control);
    fixture.shutdown().await;
}

#[tokio::test]
async fn dns_only_association_control_close_and_cancellation_release_pending_queries() {
    let mut fixture = Fixture::new(ChainSource::HttpProxy).await;
    fixture.peer.stall_dns.store(true, Ordering::SeqCst);
    for cancel_session in [false, true] {
        let (control, udp, relay) = fixture.associate().await;
        let target = TcpTarget::new("198.51.100.53", 53).unwrap();
        udp.send_to(&datagram(&target, &query(21)), relay)
            .await
            .unwrap();
        timeout(
            Duration::from_secs(2),
            fixture.peer.query_started.notified(),
        )
        .await
        .unwrap();
        assert_eq!(fixture.peer.stalled_queries.load(Ordering::SeqCst), 1);
        if cancel_session {
            fixture.cancellation.cancel();
        }
        drop(control);
        timeout(Duration::from_secs(2), fixture.peer.query_closed.notified())
            .await
            .unwrap();
        assert_eq!(fixture.peer.stalled_queries.load(Ordering::SeqCst), 0);
        let mut bytes = [0; 64];
        assert!(
            timeout(Duration::from_millis(100), udp.recv_from(&mut bytes))
                .await
                .is_err()
        );
    }
    fixture.shutdown().await;
}
