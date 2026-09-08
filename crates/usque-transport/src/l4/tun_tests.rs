//! In-memory IP/TCP tests: no TUN device, routes, UDP socket or network mutations.
use super::tun::TunBridge;
use super::tun_wire::{ip_packet, valid_transport};
use super::{BufferBudget, L4Metrics, Limits};
use crate::direct_gateway::NatPacket;
use crate::dns::Resolver;
use crate::dns_stream::StreamDns;
use crate::netstack::{RuntimeHealth, RuntimePath};
use crate::socket::NoopSocketProtector;
use crate::tcp::{DialError, FlowClass, ProxyServices, TcpDialer, TcpIo, TcpStream, TcpTarget};
use async_trait::async_trait;
use std::io;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, DuplexStream, ReadBuf};
use tokio::sync::{Notify, watch};
use tokio::time::{Instant, timeout};
use tokio_util::sync::CancellationToken;
use ts_netstack_smoltcp::CreateSocket;
use ts_netstack_smoltcp::netcore::{Config, HasChannel, NetstackControl};
use usque_core::{AddressFamily, DataPlaneMode, Profile, ProxyDnsMode, Transport};

struct MemoryStream(DuplexStream);
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
        Ok("0.0.0.0:0".parse().unwrap())
    }
}

#[derive(Default)]
struct MemoryDialer {
    targets: Mutex<Vec<String>>,
}
#[async_trait]
impl TcpDialer for MemoryDialer {
    async fn connect(
        &self,
        target: TcpTarget,
        _: Instant,
        cancel: &CancellationToken,
        class: FlowClass,
    ) -> Result<TcpStream, DialError> {
        self.targets
            .lock()
            .unwrap()
            .push(target.authority().to_owned());
        let (client, mut peer) = tokio::io::duplex(8192);
        let cancellation = cancel.clone();
        tokio::spawn(async move {
            let result = async {
                if class == FlowClass::Dns {
                    loop {
                        let n = peer.read_u16().await?;
                        let mut query = vec![0; usize::from(n)];
                        peer.read_exact(&mut query).await?;
                        let response = answer(&query);
                        peer.write_u16(response.len() as u16).await?;
                        peer.write_all(&response).await?;
                    }
                } else {
                    let mut bytes = [0; 4096];
                    loop {
                        let n = peer.read(&mut bytes).await?;
                        if n == 0 {
                            return peer.shutdown().await;
                        }
                        peer.write_all(&bytes[..n]).await?;
                    }
                }
            };
            tokio::select! { _ = cancellation.cancelled() => {}, _ = result => {} }
        });
        Ok(Box::new(MemoryStream(client)))
    }
}

fn query() -> Vec<u8> {
    let mut bytes = vec![0x12, 0x34, 1, 0, 0, 1, 0, 0, 0, 0, 0, 0];
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

async fn bridge() -> (TunBridge, Arc<MemoryDialer>, Arc<L4Metrics>) {
    let profile = Profile {
        data_plane: DataPlaneMode::L4Proxy,
        ..Profile::default()
    };
    let dialer = Arc::new(MemoryDialer::default());
    let cancellation = CancellationToken::new();
    let protector = Arc::new(NoopSocketProtector);
    let metrics = Arc::new(L4Metrics::default());
    let budget = Arc::new(BufferBudget::new(
        Limits::platform().buffers,
        metrics.clone(),
        Arc::new(Notify::new()),
    ));
    let dns = Arc::new(StreamDns::new(
        dialer.clone(),
        protector.clone(),
        cancellation.clone(),
        metrics.clone(),
    ));
    let (_, health) = watch::channel(RuntimeHealth::Connected {
        path: RuntimePath {
            transport: Transport::Http3,
            endpoint_family: AddressFamily::Ipv4,
            ipv4_available: true,
            ipv6_available: true,
        },
        reconnect_count: 0,
    });
    let services = ProxyServices {
        admission: None,
        dialer: dialer.clone(),
        udp: None,
        resolver: Resolver::for_streams(
            dns.clone(),
            profile.dns_servers.clone(),
            ProxyDnsMode::Remote,
            protector.clone(),
        ),
        protector,
        geo_policy: Arc::default(),
        counters: Arc::default(),
        ipv4: "192.0.2.2".parse().unwrap(),
        ipv6: "fd00::2".parse().unwrap(),
        cancellation,
        health,
    };
    let bridge = TunBridge::start(
        &profile,
        services,
        dns,
        budget,
        metrics.clone(),
        crate::NetworkQualityTelemetry::default(),
    )
    .await
    .unwrap();
    (bridge, dialer, metrics)
}

#[tokio::test]
async fn ipv4_and_ipv6_tcp_round_trip_restores_original_remote() {
    for (local, remote) in [
        ("192.0.2.2:40000", "203.0.113.7:8080"),
        ("[fd00::2]:40000", "[2001:db8::7]:8080"),
    ] {
        let (mut bridge, dialer, metrics) = bridge().await;
        let mut io = bridge.attach().unwrap();
        let (stack, mut pipe) = crate::netstack::bounded_piped_with_capacity(
            Config {
                mtu: 1280,
                tcp_nagle_enabled: false,
                ..Config::default()
            },
            64,
        );
        let channel = stack.command_channel();
        let task = tokio_util::task::AbortOnDropHandle::new(stack.spawn_tokio());
        let local: SocketAddr = local.parse().unwrap();
        let remote: SocketAddr = remote.parse().unwrap();
        channel.set_ips([local.ip()]).await.unwrap();
        let wire = tokio_util::task::AbortOnDropHandle::new(tokio::spawn(async move {
            loop {
                tokio::select! {
                    packet = pipe.rx.recv_async() => {
                        let Some(packet) = packet else { break; };
                        if io.send_owned_packet(packet).await.is_err() { break; }
                    }
                    packet = io.receive_packet() => {
                        let Ok(packet) = packet else { break; };
                        let meta = NatPacket::parse(&packet).unwrap();
                        assert_eq!(meta.source, remote.ip());
                        assert_eq!(meta.source_port, remote.port());
                        assert!(valid_transport(&packet, &meta));
                        pipe.tx.send_async(&packet).await;
                    }
                }
            }
        }));
        timeout(Duration::from_secs(5), async {
            let mut stream = channel.tcp_connect(local, remote).await.unwrap();
            stream.write_all(b"TUN TCP round trip").await.unwrap();
            let mut bytes = [0; 18];
            stream.read_exact(&mut bytes).await.unwrap();
            assert_eq!(&bytes, b"TUN TCP round trip");
        })
        .await
        .unwrap();
        assert_eq!(*dialer.targets.lock().unwrap(), [remote.to_string()]);
        bridge.shutdown().await;
        drop(wire);
        drop(task);
        drop(bridge);
        assert_eq!(metrics.snapshot().tun_flows, 0);
        assert_eq!(metrics.snapshot().half_open_flows, 0);
    }
}

fn udp(source: SocketAddr, destination: SocketAddr, payload: &[u8]) -> bytes::Bytes {
    let mut body = vec![0; 8];
    body[..2].copy_from_slice(&source.port().to_be_bytes());
    body[2..4].copy_from_slice(&destination.port().to_be_bytes());
    body[4..6].copy_from_slice(&((payload.len() + 8) as u16).to_be_bytes());
    body.extend_from_slice(payload);
    ip_packet(source.ip(), destination.ip(), 17, body)
}

#[tokio::test]
async fn udp_dns_preserves_resolver_and_other_udp_never_dials() {
    let (mut bridge, dialer, metrics) = bridge().await;
    let mut io = bridge.attach().unwrap();
    for (source, destination) in [
        ("192.0.2.2:50000", "198.51.100.53:53"),
        ("[fd00::2]:50000", "[2001:db8::53]:53"),
    ] {
        let source: SocketAddr = source.parse().unwrap();
        let destination: SocketAddr = destination.parse().unwrap();
        io.send_owned_packet(udp(source, destination, &query()))
            .await
            .unwrap();
        let response = timeout(Duration::from_secs(1), io.receive_packet())
            .await
            .unwrap()
            .unwrap();
        let meta = NatPacket::parse(&response).unwrap();
        assert_eq!(meta.source, destination.ip());
        assert_eq!(meta.destination, source.ip());
        assert!(valid_transport(&response, &meta));
        crate::split_dns::validate_response_bytes(&query(), &response[meta.transport_offset + 8..])
            .unwrap();
    }
    assert_eq!(
        *dialer.targets.lock().unwrap(),
        ["198.51.100.53:53", "[2001:db8::53]:53"]
    );
    for port in [53, 443, 853, 4444] {
        io.send_owned_packet(udp(
            "192.0.2.2:50000".parse().unwrap(),
            SocketAddr::new("198.51.100.1".parse().unwrap(), port),
            b"not a DNS query",
        ))
        .await
        .unwrap();
        let response = timeout(Duration::from_secs(1), io.receive_packet())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(response[9], 1); // ICMP, never an upstream UDP exchange.
    }
    assert_eq!(dialer.targets.lock().unwrap().len(), 2);
    assert_eq!(metrics.snapshot().udp_rejected, 4);
    bridge.shutdown().await;
}

#[test]
fn dns_large_udp_response_sets_tc_without_fragmenting_and_rejects_malformed_wire() {
    let query = query();
    let mut response = answer(&query);
    response.resize(2000, 0);
    let limited = crate::split_dns::limit_udp_response(&query, response, 1232);
    assert!(limited.len() <= 512);
    assert_ne!(limited[2] & 2, 0);
    let mut malformed = query.clone();
    malformed[4..6].copy_from_slice(&[0xff, 0xff]);
    assert!(crate::split_dns::validate_query_bytes(&malformed).is_err());
    let mut wrong = answer(&query);
    wrong[0] ^= 1;
    assert!(crate::split_dns::validate_response_bytes(&query, &wrong).is_err());
    for size in 0..query.len() {
        assert!(crate::split_dns::validate_query_bytes(&query[..size]).is_err());
    }
}

#[tokio::test]
async fn malformed_and_fragmented_packets_do_not_stop_other_tun_flows() {
    let (mut bridge, dialer, metrics) = bridge().await;
    let io = bridge.attach().unwrap();
    io.send_owned_packet(bytes::Bytes::from_static(&[0]))
        .await
        .unwrap();
    let mut fragment = udp(
        "192.0.2.2:50000".parse().unwrap(),
        "192.0.2.53:53".parse().unwrap(),
        &query(),
    )
    .to_vec();
    fragment[6] = 0x20; // MF: deliberately unsupported, before transport parsing.
    io.send_owned_packet(fragment.into()).await.unwrap();
    timeout(Duration::from_secs(1), async {
        while metrics.snapshot().unsupported_packets != 2 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert!(dialer.targets.lock().unwrap().is_empty());
    assert_eq!(metrics.snapshot().sessions, 0); // no hidden packet tunnel
    bridge.shutdown().await;
}
