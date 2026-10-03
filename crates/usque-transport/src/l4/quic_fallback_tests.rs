//! A real QUIC Initial and explicit test-client fallback over the memory TUN.
//! This checks the engine path, not Chrome's fallback timer or Android lifecycle.
use super::tun::TunBridge;
use super::tun_wire::ip_packet;
use super::{BufferBudget, L4Metrics, Limits};
use crate::dns::Resolver;
use crate::dns_stream::StreamDns;
use crate::netstack::{RuntimeHealth, RuntimePath};
use crate::proxy_exit::ProxyDialer;
use crate::socket::{SocketHandle, SocketProtector};
use crate::tcp::{DialError, FlowClass, ProxyServices, TcpDialer, TcpIo, TcpStream, TcpTarget};
use async_trait::async_trait;
use bytes::Bytes;
use std::io;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, DuplexStream, ReadBuf};
use tokio::sync::{Notify, watch};
use tokio::time::{Instant, timeout};
use tokio_util::sync::CancellationToken;
use tokio_util::task::AbortOnDropHandle;
use ts_netstack_smoltcp::CreateSocket;
use ts_netstack_smoltcp::netcore::smoltcp::wire::{
    Icmpv4DstUnreachable, Icmpv4Message, Icmpv4Packet, Icmpv6DstUnreachable, Icmpv6Message,
    Icmpv6Packet, IpProtocol, Ipv4Packet, Ipv6Packet,
};
use ts_netstack_smoltcp::netcore::{Config, HasChannel, NetstackControl};
use usque_core::chain_exit::{
    ChainSource, ImportSecrets, ProxyDnsTransport, ProxyExitConfiguration, ValidatedProfile,
};
use usque_core::{AddressFamily, DataPlaneMode, Profile, ProxyDnsMode, Transport};

struct MemoryStream(DuplexStream);

fn assert_port_unreachable(request: &[u8], response: &[u8], local: SocketAddr, remote: SocketAddr) {
    // Decode independently of the engine's ICMP writer, including the quoted
    // datagram that an application needs to associate the error with its socket.
    let (quoted, header_len) = match (local, remote) {
        (SocketAddr::V4(local), SocketAddr::V4(remote)) => {
            let ip = Ipv4Packet::new_checked(response).unwrap();
            assert!(ip.verify_checksum());
            assert_eq!(ip.src_addr(), *remote.ip());
            assert_eq!(ip.dst_addr(), *local.ip());
            assert_eq!(ip.next_header(), IpProtocol::Icmp);
            let icmp = Icmpv4Packet::new_checked(ip.payload()).unwrap();
            assert!(icmp.verify_checksum());
            assert_eq!(icmp.msg_type(), Icmpv4Message::DstUnreachable);
            assert_eq!(
                icmp.msg_code(),
                u8::from(Icmpv4DstUnreachable::PortUnreachable)
            );
            (icmp.data(), 20)
        }
        (SocketAddr::V6(local), SocketAddr::V6(remote)) => {
            let ip = Ipv6Packet::new_checked(response).unwrap();
            assert_eq!(ip.src_addr(), *remote.ip());
            assert_eq!(ip.dst_addr(), *local.ip());
            assert_eq!(ip.next_header(), IpProtocol::Icmpv6);
            let icmp = Icmpv6Packet::new_checked(ip.payload()).unwrap();
            assert!(icmp.verify_checksum(&ip.src_addr(), &ip.dst_addr()));
            assert_eq!(icmp.msg_type(), Icmpv6Message::DstUnreachable);
            assert_eq!(
                icmp.msg_code(),
                u8::from(Icmpv6DstUnreachable::PortUnreachable)
            );
            (icmp.payload(), 40)
        }
        _ => unreachable!(),
    };
    assert!(quoted.len() >= header_len + 8);
    assert_eq!(quoted, &request[..quoted.len()]);
    assert_eq!(
        &quoted[header_len..header_len + 2],
        &local.port().to_be_bytes()
    );
    assert_eq!(
        &quoted[header_len + 2..header_len + 4],
        &remote.port().to_be_bytes()
    );
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
impl TcpIo for MemoryStream {
    fn local_addr(&self) -> io::Result<SocketAddr> {
        Ok("192.0.2.1:42000".parse().unwrap())
    }
}
struct DenyPhysical;
impl SocketProtector for DenyPhysical {
    fn protect(&self, _: SocketHandle) -> Result<(), String> {
        panic!("fallback must stay on the final exit")
    }
    fn resolve(&self, _: &str, _: u16) -> Result<Vec<SocketAddr>, String> {
        panic!("no physical DNS")
    }
}
struct WebProxyPeer {
    source: ChainSource,
    tls: Arc<rustls::ServerConfig>,
    remote: SocketAddr,
    targets: Arc<Mutex<Vec<TcpTarget>>>,
    udp_commands: Arc<AtomicUsize>,
}
#[async_trait]
impl TcpDialer for WebProxyPeer {
    async fn connect(
        &self,
        target: TcpTarget,
        _: Instant,
        cancel: &CancellationToken,
        _: FlowClass,
    ) -> Result<TcpStream, DialError> {
        assert_eq!(
            target.socket_address().unwrap().ip(),
            "198.51.100.10".parse::<std::net::IpAddr>().unwrap()
        );
        let (client, mut peer) = tokio::io::duplex(32 * 1024);
        let source = self.source;
        let config = self.tls.clone();
        let remote = self.remote;
        let targets = self.targets.clone();
        let udp_commands = self.udp_commands.clone();
        let cancel = cancel.clone();
        tokio::spawn(async move {
            let work = async {
                if source == ChainSource::HttpProxy {
                    let mut header = Vec::new();
                    while !header.ends_with(b"\r\n\r\n") {
                        header.push(peer.read_u8().await?);
                        assert!(header.len() <= 4096);
                    }
                    assert!(
                        header.starts_with(format!("CONNECT {remote} HTTP/1.1\r\n").as_bytes())
                    );
                    peer.write_all(b"HTTP/1.1 200 Connection established\r\n\r\n")
                        .await?;
                } else {
                    let mut greeting = [0; 3];
                    peer.read_exact(&mut greeting).await?;
                    assert_eq!(greeting, [5, 1, 0]);
                    peer.write_all(&[5, 0]).await?;
                    let mut command = [0; 4];
                    peer.read_exact(&mut command).await?;
                    if command[1] == 3 {
                        udp_commands.fetch_add(1, Ordering::AcqRel);
                        peer.write_all(&[5, 7, 0, 1, 0, 0, 0, 0, 0, 0]).await?;
                        return Ok(());
                    }
                    assert_eq!(command[..3], [5, 1, 0]);
                    let address = match command[3] {
                        1 => {
                            let mut bytes = [0; 4];
                            peer.read_exact(&mut bytes).await?;
                            std::net::IpAddr::V4(bytes.into())
                        }
                        4 => {
                            let mut bytes = [0; 16];
                            peer.read_exact(&mut bytes).await?;
                            std::net::IpAddr::V6(bytes.into())
                        }
                        _ => panic!("numeric final target expected"),
                    };
                    assert_eq!(SocketAddr::new(address, peer.read_u16().await?), remote);
                    // UDP ASSOCIATE would be rejected by this TCP-only proxy.
                    assert_eq!(udp_commands.load(Ordering::Acquire), 0);
                    peer.write_all(&[5, 0, 0, 1, 0, 0, 0, 0, 0, 0]).await?;
                }
                targets.lock().unwrap().push(TcpTarget::address(remote));
                let tls = tokio_rustls::TlsAcceptor::from(config).accept(peer).await?;
                assert_eq!(tls.get_ref().1.alpn_protocol(), Some(b"h2".as_slice()));
                let mut connection = h2::server::handshake(tls).await.map_err(io::Error::other)?;
                while let Some(request) = connection.accept().await {
                    let (request, mut respond) = request.map_err(io::Error::other)?;
                    assert_eq!(request.method(), http::Method::GET);
                    let response = http::Response::builder()
                        .status(200)
                        .header("alt-svc", "h3=\":443\"")
                        .body(())
                        .unwrap();
                    respond
                        .send_response(response, false)
                        .map_err(io::Error::other)?
                        .send_data(Bytes::from_static(b"HTTP/2 fallback succeeded"), true)
                        .map_err(io::Error::other)?;
                }
                Ok::<_, io::Error>(())
            };
            tokio::select! { _ = cancel.cancelled() => {}, _ = work => {} }
        });
        Ok(Box::new(MemoryStream(client)))
    }
}

#[tokio::test]
async fn real_quic_rejection_allows_tls_http2_through_http_and_tcp_only_socks_exits() {
    for source in [ChainSource::HttpProxy, ChainSource::Socks5Proxy] {
        for (local, remote) in [
            ("192.0.2.2:42000", "203.0.113.7:443"),
            ("[fd00::2]:42000", "[2001:db8::7]:443"),
        ] {
            let local: SocketAddr = local.parse().unwrap();
            let remote: SocketAddr = remote.parse().unwrap();
            let (server_tls, client_tls) = crate::encrypted_dns::tests::test_certificates(
                usque_core::DirectDnsMode::Doh,
                "site.test",
                false,
            );
            let peer = Arc::new(WebProxyPeer {
                source,
                tls: Arc::new(server_tls),
                remote,
                targets: Arc::default(),
                udp_commands: Arc::default(),
            });
            let cancel = CancellationToken::new();
            let (_health, health) = watch::channel(RuntimeHealth::Connected {
                path: RuntimePath {
                    transport: Transport::Http3,
                    endpoint_family: AddressFamily::Ipv4,
                    ipv4_available: true,
                    ipv6_available: true,
                },
                reconnect_count: 0,
            });
            let metrics = Arc::new(L4Metrics::default());
            let budget = Arc::new(BufferBudget::new(
                Limits::platform().buffers,
                metrics.clone(),
                Arc::new(Notify::new()),
            ));
            let mut secrets = ImportSecrets::default();
            secrets.proxy = Some(ProxyExitConfiguration {
                host: "198.51.100.10".into(),
                port: 1080,
                auth_mode: Default::default(),
                dns_servers: vec![],
                dns_transport: ProxyDnsTransport::Tcp,
            });
            let parsed = ValidatedProfile::parse(source, &secrets).unwrap();
            let summary = parsed
                .summary("Memory exit", uuid::Uuid::new_v4(), uuid::Uuid::new_v4())
                .unwrap();
            let prepared = usque_core::vpngate::PreparedProfile::imported(summary, parsed, secrets);
            let network =
                crate::InternalNetwork::for_streams(peer.clone(), health.clone(), cancel.clone());
            let proxy = ProxyDialer::start(
                &prepared,
                network,
                None,
                cancel.clone(),
                budget.clone(),
                Arc::default(),
                Instant::now() + Duration::from_secs(4),
            )
            .await
            .unwrap();
            proxy.admitted.store(true, Ordering::Release);
            let protector = Arc::new(DenyPhysical);
            let dns = Arc::new(StreamDns::new(
                proxy.clone(),
                protector.clone(),
                cancel.clone(),
                metrics.clone(),
            ));
            let services = ProxyServices {
                traffic_policy: Arc::new(
                    crate::application_traffic::ApplicationTrafficPolicy::new(true),
                ),
                admission: None,
                dialer: proxy.clone(),
                udp: (source == ChainSource::Socks5Proxy).then(|| {
                    Arc::new(crate::proxy_udp::SocksFactory(proxy.clone()))
                        as Arc<dyn crate::proxy_udp::UdpFactory>
                }),
                resolver: Resolver::for_streams(
                    dns.clone(),
                    vec![],
                    ProxyDnsMode::Remote,
                    protector.clone(),
                ),
                protector,
                geo_policy: Arc::default(),
                counters: Arc::default(),
                cancellation: cancel.clone(),
                health,
            };
            let profile = Profile {
                data_plane: DataPlaneMode::ConnectIp,
                mtu: 1280,
                ..Default::default()
            };
            let mut bridge = TunBridge::start(
                &profile,
                services,
                dns,
                budget,
                metrics.clone(),
                crate::NetworkQualityTelemetry::default(),
            )
            .await
            .unwrap();
            let mut io = bridge.attach().unwrap();
            let mut config = quiche::Config::new(quiche::PROTOCOL_VERSION).unwrap();
            config
                .set_application_protos(quiche::h3::APPLICATION_PROTOCOL)
                .unwrap();
            config.set_max_send_udp_payload_size(1200);
            let mut quic = quiche::connect(
                Some("site.test"),
                &quiche::ConnectionId::from_ref(&[7; 16]),
                local,
                remote,
                &mut config,
            )
            .unwrap();
            let mut initial = [0; 1280];
            let (length, _) = quic.send(&mut initial).unwrap();
            assert!(length >= 1200);
            let mut udp = vec![0; 8];
            udp[..2].copy_from_slice(&local.port().to_be_bytes());
            udp[2..4].copy_from_slice(&remote.port().to_be_bytes());
            udp[4..6].copy_from_slice(&((8 + length) as u16).to_be_bytes());
            udp.extend_from_slice(&initial[..length]);
            let request = ip_packet(local.ip(), remote.ip(), 17, udp);
            io.send_owned_packet(request.clone()).await.unwrap();
            let rejected = timeout(Duration::from_secs(1), io.receive_packet())
                .await
                .unwrap()
                .unwrap();
            assert_port_unreachable(&request, &rejected, local, remote);
            assert_eq!(metrics.snapshot().udp_rejected, 1);
            assert_eq!(peer.udp_commands.load(Ordering::Acquire), 0);
            assert!(peer.targets.lock().unwrap().is_empty());
            // Explicit test-client fallback, using the same memory TUN for TCP.
            let (stack, mut pipe) = crate::netstack::bounded_piped_with_capacity(
                Config {
                    mtu: 1280,
                    tcp_nagle_enabled: false,
                    ..Default::default()
                },
                64,
            );
            let channel = stack.command_channel();
            let stack_task = AbortOnDropHandle::new(stack.spawn_tokio());
            channel.set_ips([local.ip()]).await.unwrap();
            let wire = AbortOnDropHandle::new(tokio::spawn(async move {
                loop {
                    tokio::select! {
                        packet = pipe.rx.recv_async() => { let Some(packet) = packet else { break; }; if io.send_owned_packet(packet).await.is_err() { break; } },
                        packet = io.receive_packet() => { let Ok(packet) = packet else { break; }; pipe.tx.send_async(&packet).await; },
                    }
                }
            }));
            timeout(Duration::from_secs(5), async {
                let stream = channel.tcp_connect(local, remote).await.unwrap();
                let tls = tokio_rustls::TlsConnector::from(Arc::new(client_tls))
                    .connect(
                        rustls::pki_types::ServerName::try_from("site.test").unwrap(),
                        stream,
                    )
                    .await
                    .unwrap();
                assert_eq!(tls.get_ref().1.alpn_protocol(), Some(b"h2".as_slice()));
                let (mut sender, connection) = h2::client::handshake(tls).await.unwrap();
                let driver = AbortOnDropHandle::new(tokio::spawn(connection));
                for _ in 0..2 {
                    let request = http::Request::builder()
                        .uri("https://site.test/")
                        .body(())
                        .unwrap();
                    let (response, _) = sender.send_request(request, true).unwrap();
                    let response = response.await.unwrap();
                    assert_eq!(response.status(), 200);
                    let mut body = response.into_body();
                    let mut bytes = Vec::new();
                    while let Some(data) = body.data().await {
                        let data = data.unwrap();
                        bytes.extend_from_slice(&data);
                        body.flow_control().release_capacity(data.len()).unwrap();
                    }
                    assert_eq!(bytes, b"HTTP/2 fallback succeeded");
                }
                drop(driver);
            })
            .await
            .unwrap();
            assert_eq!(*peer.targets.lock().unwrap(), [TcpTarget::address(remote)]);
            cancel.cancel();
            wire.abort();
            stack_task.abort();
            bridge.shutdown().await;
        }
    }
}
