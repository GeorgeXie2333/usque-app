//! Final proxy startup uses memory transports and loopback DNS, never an OS TUN.
use super::*;
use crate::socket::{DirectEgressLease, DirectProtocol, SocketHandle};
use crate::tcp::{DialError, FlowClass, TcpDialer, TcpIo, TcpStream, TcpTarget};
use async_trait::async_trait;
use std::io;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::{Context, Poll};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, DuplexStream, ReadBuf};
use tokio::sync::watch;
use tokio::time::{Duration, Instant, timeout};
use tokio_util::task::AbortOnDropHandle;
use usque_core::chain_exit::{ChainSource, ImportSecrets, ProxyAuthMode, ProxyExitConfiguration};
use usque_core::{AddressFamily, DataPlaneMode, DirectDnsMode, DirectDnsSettings, Transport};

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
struct ProxyPeer(ChainSource);
#[async_trait]
impl TcpDialer for ProxyPeer {
    async fn connect(
        &self,
        _: TcpTarget,
        _: Instant,
        _: &CancellationToken,
        _: FlowClass,
    ) -> Result<TcpStream, DialError> {
        let (client, mut peer) = tokio::io::duplex(4096);
        if self.0 == ChainSource::Socks5Proxy {
            tokio::spawn(async move {
                let mut greeting = [0; 3];
                if peer.read_exact(&mut greeting).await.is_ok() {
                    assert_eq!(greeting, [5, 1, 0]);
                    let _ = peer.write_all(&[5, 0]).await;
                }
            });
        }
        Ok(Box::new(MemoryStream(client)))
    }
}
struct DnsProtector {
    physical: SocketAddr,
    encrypted_attempts: AtomicUsize,
}
#[async_trait]
impl SocketProtector for DnsProtector {
    fn protect(&self, _: SocketHandle) -> Result<(), String> {
        Ok(())
    }
    async fn protect_for_target(
        &self,
        _: SocketHandle,
        remote: SocketAddr,
        _: DirectProtocol,
    ) -> Result<DirectEgressLease, String> {
        if remote == self.physical {
            return Ok(DirectEgressLease::default());
        }
        // The encrypted resolver is deliberately unavailable. Deny its socket
        // before connecting or emitting any packet to the documentation IP.
        self.encrypted_attempts.fetch_add(1, Ordering::SeqCst);
        Err("fixture encrypted resolver unavailable".into())
    }
    fn physical_dns_servers(&self) -> Vec<SocketAddr> {
        vec![self.physical]
    }
    fn tun_direct_available(&self) -> bool {
        true
    }
}
fn prepared(source: ChainSource) -> usque_core::vpngate::PreparedProfile {
    let mut secrets = ImportSecrets::default();
    secrets.proxy = Some(ProxyExitConfiguration {
        host: "198.51.100.10".into(),
        port: 1080,
        auth_mode: ProxyAuthMode::None,
        dns_servers: vec![],
    });
    let parsed = usque_core::chain_exit::ValidatedProfile::parse(source, &secrets).unwrap();
    let summary = parsed
        .summary("Test proxy", uuid::Uuid::new_v4(), uuid::Uuid::new_v4())
        .unwrap();
    usque_core::vpngate::PreparedProfile::imported(summary, parsed, secrets)
}
fn query() -> Vec<u8> {
    let mut query = vec![0x12, 0x34, 1, 0, 0, 1, 0, 0, 0, 0, 0, 0];
    query.extend_from_slice(b"\x07example\x04test\0\0\x01\0\x01");
    query
}
fn dns_packet(query: &[u8]) -> bytes::Bytes {
    let mut udp = vec![0; 8];
    udp[..2].copy_from_slice(&42000_u16.to_be_bytes());
    udp[2..4].copy_from_slice(&53_u16.to_be_bytes());
    udp[4..6].copy_from_slice(&((query.len() + 8) as u16).to_be_bytes());
    udp.extend_from_slice(query);
    super::super::tun_wire::ip_packet(
        "192.0.2.44".parse().unwrap(),
        crate::SPLIT_DNS_IPV4.into(),
        17,
        udp,
    )
}

#[tokio::test]
async fn final_proxy_encrypted_direct_dns_fails_closed_without_plaintext_fallback() {
    for source in [ChainSource::HttpProxy, ChainSource::Socks5Proxy] {
        for mode in [DirectDnsMode::Doh, DirectDnsMode::Dot] {
            let physical = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
            let plaintext = Arc::new(AtomicUsize::new(0));
            let observed = plaintext.clone();
            let protector = Arc::new(DnsProtector {
                physical: physical.local_addr().unwrap(),
                encrypted_attempts: AtomicUsize::new(0),
            });
            let peer = AbortOnDropHandle::new(tokio::spawn(async move {
                let mut buffer = [0; 4096];
                loop {
                    let Ok((length, from)) = physical.recv_from(&mut buffer).await else {
                        break;
                    };
                    observed.fetch_add(1, Ordering::SeqCst);
                    let mut response = buffer[..length].to_vec();
                    response[2..4].copy_from_slice(&[0x81, 0x80]);
                    let _ = physical.send_to(&response, from).await;
                }
            }));
            let cancellation = CancellationToken::new();
            let (_health, health) = watch::channel(RuntimeHealth::Connected {
                path: crate::netstack::RuntimePath {
                    transport: Transport::Http3,
                    endpoint_family: AddressFamily::Ipv4,
                    ipv4_available: true,
                    ipv6_available: true,
                },
                reconnect_count: 0,
            });
            let network = crate::InternalNetwork::for_streams(
                Arc::new(ProxyPeer(source)),
                health,
                cancellation.clone(),
            );
            let mut profile = Profile {
                data_plane: DataPlaneMode::ConnectIp,
                mtu: 1280,
                bypass_domains: vec!["example.test".into()],
                ..Default::default()
            };
            profile.frontends.tunnel = true;
            profile.frontends.socks5 = false;
            profile.frontends.http = false;
            profile.direct_dns = DirectDnsSettings {
                mode,
                server_name: "resolver.example".into(),
                doh_path: if mode == DirectDnsMode::Doh {
                    "/dns-query".into()
                } else {
                    String::new()
                },
                bootstrap_ips: vec!["192.0.2.53".parse().unwrap()],
                port: if mode == DirectDnsMode::Doh { 443 } else { 853 },
            };
            let policy = Arc::new(
                GeoDirectPolicy::disabled()
                    .with_custom_rules(&profile)
                    .unwrap(),
            );
            let mut runtime = L4Runtime::start_proxy(
                &profile,
                &prepared(source),
                network,
                crate::NetworkQualityTelemetry::default(),
                ("192.0.2.1".parse().unwrap(), "2001:db8::1".parse().unwrap()),
                protector.clone(),
                policy,
                None,
                &cancellation,
                Instant::now() + Duration::from_secs(5),
            )
            .await
            .unwrap();
            assert!(runtime.services.traffic_policy.blocks_udp(443));
            runtime.update_traffic_policy(false);
            assert!(runtime.services.traffic_policy.blocks_udp(443));
            runtime.update_traffic_policy(true);
            assert!(runtime.services.traffic_policy.blocks_udp(443));
            assert!(!runtime.services.traffic_policy.blocks_udp(53));
            assert!(!runtime.services.traffic_policy.blocks_udp(8443));
            let resolver = runtime
                .diagnostic_dns_context()
                .0
                .direct_dns_resolver()
                .unwrap();
            assert!(resolver.is_encrypted());
            runtime
                .client
                .proxy
                .as_ref()
                .unwrap()
                .admitted
                .store(true, Ordering::Release);
            let mut io = runtime.bridge.as_mut().unwrap().attach().unwrap();
            io.send_owned_packet(dns_packet(&query())).await.unwrap();
            let response = timeout(Duration::from_secs(2), io.receive_packet())
                .await
                .unwrap()
                .unwrap();
            let meta = crate::direct_gateway::NatPacket::parse(&response).unwrap();
            let dns = &response[meta.transport_offset + 8..];
            assert_eq!(
                u16::from_be_bytes([dns[2], dns[3]]) & 15,
                2,
                "unavailable encrypted DNS must return SERVFAIL"
            );
            assert!(protector.encrypted_attempts.load(Ordering::SeqCst) > 0);
            assert_eq!(plaintext.load(Ordering::SeqCst), 0);
            drop(io);
            runtime.shutdown().await;
            let before = protector.encrypted_attempts.load(Ordering::SeqCst);
            assert!(
                resolver
                    .query(
                        bytes::Bytes::from(query()),
                        crate::DirectDnsQueryContext {
                            network_generation: 0,
                            deadline: Instant::now() + Duration::from_secs(1)
                        }
                    )
                    .await
                    .is_err()
            );
            assert_eq!(
                protector.encrypted_attempts.load(Ordering::SeqCst),
                before,
                "final shutdown cancels its resolver"
            );
            peer.abort();
            let _ = peer.await;
            cancellation.cancel();
        }
    }
}
