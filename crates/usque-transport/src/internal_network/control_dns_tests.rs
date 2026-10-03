//! Registration refresh exercises real DNS framing over memory streams only.
use super::*;
use std::{
    io,
    pin::Pin,
    sync::atomic::{AtomicUsize, Ordering},
    task::{Context, Poll},
};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, DuplexStream, ReadBuf};

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
impl crate::tcp::TcpIo for MemoryStream {
    fn local_addr(&self) -> io::Result<SocketAddr> {
        Ok("192.0.2.1:40000".parse().unwrap())
    }
}

struct Peer {
    calls: AtomicUsize,
    fail: bool,
}
#[async_trait::async_trait]
impl TcpDialer for Peer {
    async fn connect(
        &self,
        target: TcpTarget,
        _: Instant,
        cancel: &CancellationToken,
        class: FlowClass,
    ) -> Result<TcpStream, DialError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        assert_eq!(target, TcpTarget::address("1.1.1.1:53".parse().unwrap()));
        assert_eq!(class, FlowClass::Dns);
        if self.fail {
            return Err(DialError::Refused);
        }
        let (client, mut server) = tokio::io::duplex(4096);
        let cancel = cancel.clone();
        tokio::spawn(async move {
            let work = async {
                loop {
                    let Ok(length) = server.read_u16().await else {
                        return;
                    };
                    let mut query = vec![0; usize::from(length)];
                    if server.read_exact(&mut query).await.is_err() {
                        return;
                    }
                    let mut expected = Vec::new();
                    for label in usque_core::REGISTRATION_API_HOST.split('.') {
                        expected.push(label.len() as u8);
                        expected.extend_from_slice(label.as_bytes());
                    }
                    expected.push(0);
                    assert_eq!(&query[12..query.len() - 4], expected);
                    let kind = &query[query.len() - 4..query.len() - 2];
                    let ipv6 = kind == [0, 28];
                    assert!(ipv6 || kind == [0, 1]);
                    let answer = if ipv6 {
                        "2001:db8::10"
                            .parse::<std::net::Ipv6Addr>()
                            .unwrap()
                            .octets()
                            .to_vec()
                    } else {
                        vec![198, 51, 100, 10]
                    };
                    query[2..4].copy_from_slice(&[0x81, 0x80]);
                    query[6..8].copy_from_slice(&[0, 1]);
                    query.extend_from_slice(&[
                        0xc0,
                        0x0c,
                        0,
                        if ipv6 { 28 } else { 1 },
                        0,
                        1,
                        0,
                        0,
                        0,
                        30,
                        0,
                        answer.len() as u8,
                    ]);
                    query.extend_from_slice(&answer);
                    if server.write_u16(query.len() as u16).await.is_err()
                        || server.write_all(&query).await.is_err()
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

fn network(
    peer: Arc<Peer>,
    mode: usque_core::ProxyDnsMode,
) -> (InternalNetwork, watch::Sender<RuntimeHealth>) {
    let cancellation = CancellationToken::new();
    let (sender, health) = watch::channel(RuntimeHealth::Connected {
        path: crate::netstack::RuntimePath {
            transport: usque_core::Transport::Http2,
            endpoint_family: usque_core::AddressFamily::Ipv4,
            ipv4_available: true,
            ipv6_available: true,
        },
        reconnect_count: 0,
    });
    let dns = Arc::new(crate::dns_stream::StreamDns::new(
        peer.clone(),
        Arc::new(crate::NoopSocketProtector),
        cancellation.clone(),
        Arc::default(),
    ));
    let resolver = Resolver::for_streams(
        dns,
        vec!["1.1.1.1".parse().unwrap()],
        mode,
        Arc::new(crate::NoopSocketProtector),
    );
    (
        InternalNetwork::for_streams(peer, health, cancellation).with_resolver(resolver),
        sender,
    )
}

#[tokio::test]
async fn control_refresh_uses_only_private_dns_and_fixed_https_port_in_every_frontend_mode() {
    for mode in [
        usque_core::ProxyDnsMode::Remote,
        usque_core::ProxyDnsMode::System,
        usque_core::ProxyDnsMode::LocalConfigured,
    ] {
        let peer = Arc::new(Peer {
            calls: AtomicUsize::new(0),
            fail: false,
        });
        let (network, _health) = network(peer.clone(), mode);
        let result = network
            .resolve_registration_api(&CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(
            result,
            vec![
                "198.51.100.10:443".parse().unwrap(),
                "[2001:db8::10]:443".parse().unwrap()
            ]
        );
        assert!((1..=2).contains(&peer.calls.load(Ordering::SeqCst)));
        network.cancellation.cancel();
    }
}

#[tokio::test]
async fn failed_or_cancelled_control_dns_never_substitutes_another_resolver() {
    let peer = Arc::new(Peer {
        calls: AtomicUsize::new(0),
        fail: true,
    });
    let (network, _health) = network(peer.clone(), usque_core::ProxyDnsMode::System);
    assert_eq!(
        network
            .resolve_registration_api(&CancellationToken::new())
            .await,
        Err(DirectoryError::Request)
    );
    assert_eq!(peer.calls.load(Ordering::SeqCst), 2);
    let cancel = CancellationToken::new();
    cancel.cancel();
    assert_eq!(
        network.resolve_registration_api(&cancel).await,
        Err(DirectoryError::Cancelled)
    );
    network.cancellation.cancel();
    assert_eq!(
        network
            .resolve_registration_api(&CancellationToken::new())
            .await,
        Err(DirectoryError::Cancelled)
    );
    assert_eq!(peer.calls.load(Ordering::SeqCst), 2);
}
