//! Proxy quality regressions use in-memory peers, with no sockets or OS TUN.
use super::*;
use crate::network_quality::H3MetricsSample;
use crate::tcp::{DialError, FlowClass, TcpIo, TcpStream, TcpTarget};
use crate::{MetricAvailability, MetricValue, NetworkQualitySnapshot, NetworkQualityTelemetry};
use async_trait::async_trait;
use std::io;
use std::pin::Pin;
use std::sync::atomic::Ordering;
use std::task::{Context, Poll};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, DuplexStream, ReadBuf};
use tokio::sync::watch;
use tokio::time::{Duration, Instant, advance, timeout};
use usque_core::chain_exit::{ChainSource, ImportSecrets, ProxyAuthMode, ProxyExitConfiguration};
use usque_core::{AddressFamily, DataPlaneMode, Transport};

struct MemoryStream {
    stream: DuplexStream,
    wire: Arc<TrafficCounters>,
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
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let before = buffer.filled().len();
        let result = Pin::new(&mut self.stream).poll_read(cx, buffer);
        self.wire.record_received(buffer.filled().len() - before);
        result
    }
}

impl AsyncWrite for MemoryStream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buffer: &[u8],
    ) -> Poll<io::Result<usize>> {
        let result = Pin::new(&mut self.stream).poll_write(cx, buffer);
        if let Poll::Ready(Ok(bytes)) = result {
            self.wire.record_sent(bytes);
        }
        result
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.stream).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.stream).poll_shutdown(cx)
    }
}

struct ProxyPeer {
    source: ChainSource,
    wire: Arc<TrafficCounters>,
}

#[async_trait]
impl TcpDialer for ProxyPeer {
    async fn connect(
        &self,
        target: TcpTarget,
        _: Instant,
        _: &CancellationToken,
        _: FlowClass,
    ) -> Result<TcpStream, DialError> {
        assert_eq!(target.authority(), "198.51.100.10:1080");
        let (client, peer) = tokio::io::duplex(4096);
        let source = self.source;
        tokio::spawn(async move {
            // Startup deliberately closes after reachability/authentication;
            // business connections continue through CONNECT to the echo peer.
            let _ = proxy_session(peer, source).await;
        });
        Ok(Box::new(MemoryStream {
            stream: client,
            wire: self.wire.clone(),
        }))
    }
}

async fn proxy_session(mut peer: DuplexStream, source: ChainSource) -> io::Result<()> {
    if source == ChainSource::HttpProxy {
        let mut request = Vec::new();
        while !request.ends_with(b"\r\n\r\n") {
            request.push(peer.read_u8().await?);
        }
        assert!(request.starts_with(b"CONNECT 203.0.113.20:443 HTTP/1.1\r\n"));
        peer.write_all(b"HTTP/1.1 200 Connection established\r\n\r\n")
            .await?;
    } else {
        let mut greeting = [0; 3];
        peer.read_exact(&mut greeting).await?;
        assert_eq!(greeting, [5, 1, 0]);
        peer.write_all(&[5, 0]).await?;
        let mut command = [0; 10];
        peer.read_exact(&mut command).await?;
        assert_eq!(command, [5, 1, 0, 1, 203, 0, 113, 20, 1, 187]);
        peer.write_all(&[5, 0, 0, 1, 192, 0, 2, 1, 0xa0, 0x28])
            .await?;
    }
    let mut buffer = [0; 256];
    loop {
        let count = peer.read(&mut buffer).await?;
        if count == 0 {
            return Ok(());
        }
        peer.write_all(&buffer[..count]).await?;
    }
}

struct Underlay {
    quality: NetworkQualityTelemetry,
    attempt: NetworkQualityTelemetry,
    network: crate::InternalNetwork,
    wire: Arc<TrafficCounters>,
    cancellation: CancellationToken,
    _health: watch::Sender<RuntimeHealth>,
}

impl Underlay {
    fn new(source: ChainSource, transport: Transport, data_plane: DataPlaneMode) -> Self {
        let quality = NetworkQualityTelemetry::default();
        if data_plane == DataPlaneMode::L4Proxy {
            quality.use_stream_data_plane();
        }
        let attempt = quality.new_attempt(transport, AddressFamily::Ipv4);
        attempt.observe_h3(h3_sample(100, 0));
        attempt.configure_h2_connection(65535, 1048576, true);
        attempt.observe_h2_rtt(
            Duration::from_millis(20),
            Duration::from_millis(20),
            Duration::from_millis(15),
            Duration::from_millis(2),
        );
        quality.activate_attempt(&attempt);
        let (health_tx, health) = watch::channel(RuntimeHealth::Connected {
            path: crate::RuntimePath {
                transport,
                endpoint_family: AddressFamily::Ipv4,
                ipv4_available: true,
                ipv6_available: true,
            },
            reconnect_count: 0,
        });
        let cancellation = CancellationToken::new();
        let wire = Arc::new(TrafficCounters::default());
        let network = crate::InternalNetwork::for_streams(
            Arc::new(ProxyPeer {
                source,
                wire: wire.clone(),
            }),
            health,
            cancellation.clone(),
        );
        Self {
            quality,
            attempt,
            network,
            wire,
            cancellation,
            _health: health_tx,
        }
    }

    async fn start(&self, source: ChainSource, data_plane: DataPlaneMode) -> L4Runtime {
        let mut secrets = ImportSecrets::default();
        secrets.proxy = Some(ProxyExitConfiguration {
            host: "198.51.100.10".into(),
            port: 1080,
            auth_mode: ProxyAuthMode::None,
            dns_servers: vec![],
        });
        let parsed = usque_core::chain_exit::ValidatedProfile::parse(source, &secrets).unwrap();
        let summary = parsed
            .summary("Quality test", uuid::Uuid::new_v4(), uuid::Uuid::new_v4())
            .unwrap();
        let prepared = usque_core::vpngate::PreparedProfile::imported(summary, parsed, secrets);
        let profile = Profile {
            data_plane,
            frontends: usque_core::FrontendSettings {
                tunnel: false,
                socks5: false,
                http: false,
            },
            ..Default::default()
        };
        let runtime = L4Runtime::start_proxy(
            &profile,
            &prepared,
            self.network.clone(),
            self.quality.clone(),
            ("192.0.2.1".parse().unwrap(), "2001:db8::1".parse().unwrap()),
            Arc::new(crate::NoopSocketProtector),
            Arc::new(GeoDirectPolicy::disabled()),
            None,
            &self.cancellation,
            Instant::now() + Duration::from_secs(5),
        )
        .await
        .unwrap();
        runtime
            .client
            .proxy
            .as_ref()
            .unwrap()
            .admitted
            .store(true, Ordering::Release);
        runtime
    }
}

fn h3_sample(sent: u64, lost: u64) -> H3MetricsSample {
    H3MetricsSample {
        rtt: Duration::from_millis(20),
        min_rtt: Some(Duration::from_millis(15)),
        rtt_variance: Duration::from_millis(2),
        congestion_window_bytes: 64 * 1024,
        send_rate_bytes_per_second: 1_000_000,
        sent_packets: sent,
        received_packets: sent - lost,
        lost_packets: lost,
        sent_bytes: sent * 1_000,
        received_bytes: (sent - lost) * 1_000,
        lost_bytes: lost * 1_000,
        pto_count: 0,
        datagrams_sent: sent,
        datagrams_received: sent - lost,
        datagrams_lost: lost,
        datagram_receive_drops: 0,
    }
}

async fn echo(runtime: &L4Runtime, payload: &[u8]) {
    let mut stream = runtime
        .client
        .connect(
            TcpTarget::address("203.0.113.20:443".parse().unwrap()),
            Instant::now() + Duration::from_secs(2),
            &runtime.cancellation,
            FlowClass::Business,
        )
        .await
        .unwrap();
    stream.write_all(payload).await.unwrap();
    let mut response = vec![0; payload.len()];
    stream.read_exact(&mut response).await.unwrap();
    assert_eq!(response, payload);
}

async fn sample_after(
    updates: &mut watch::Receiver<NetworkQualitySnapshot>,
    elapsed: Duration,
) -> NetworkQualitySnapshot {
    tokio::task::yield_now().await;
    advance(elapsed).await;
    timeout(Duration::from_secs(1), updates.changed())
        .await
        .unwrap()
        .unwrap();
    updates.borrow_and_update().clone()
}

#[tokio::test(start_paused = true)]
async fn proxy_quality_samples_final_bytes_with_h3_h2_and_l4_underlay_metrics() {
    for source in [ChainSource::HttpProxy, ChainSource::Socks5Proxy] {
        for (transport, data_plane) in [
            (Transport::Http3, DataPlaneMode::ConnectIp),
            (Transport::Http2, DataPlaneMode::ConnectIp),
            (Transport::Http3, DataPlaneMode::L4Proxy),
        ] {
            let underlay = Underlay::new(source, transport, data_plane);
            let mut runtime = underlay.start(source, data_plane).await;
            let mut updates = runtime.monitor.subscribe_network_quality();
            let first = updates.borrow_and_update().clone();
            assert!(first.connection_id.is_some(), "{source:?}/{data_plane:?}");
            assert_eq!(first.transport, Some(transport));
            assert_eq!(first.endpoint_family, Some(AddressFamily::Ipv4));
            assert_eq!(first.samples.len(), 1);
            assert_eq!(first.samples[0].uploaded_bytes, Some(0));
            assert_eq!(first.samples[0].downloaded_bytes, Some(0));
            assert_eq!(
                first.rtt.smoothed,
                MetricValue::available(Duration::from_millis(20))
            );

            let payload = b"final application traffic";
            echo(&runtime, payload).await;
            underlay.attempt.observe_h3(h3_sample(200, 1));
            let current = sample_after(&mut updates, Duration::from_secs(1)).await;
            assert_eq!(current.connection_id, first.connection_id);
            let point = current.samples.last().unwrap();
            assert_eq!(point.uploaded_bytes, Some(payload.len() as u64));
            assert_eq!(point.downloaded_bytes, Some(payload.len() as u64));
            assert_eq!(point.monotonic_millis, 1000);
            assert_eq!(point.rtt_ms, Some(20));
            assert_eq!(
                point.loss_basis_points,
                (transport == Transport::Http3).then_some(100)
            );
            let totals = runtime.monitor.statistics();
            assert_eq!(totals.bytes_sent, payload.len() as u64);
            assert_eq!(totals.bytes_received, payload.len() as u64);
            let wire = underlay.wire.snapshot();
            assert!(wire.bytes_sent > totals.bytes_sent);
            assert!(wire.bytes_received > totals.bytes_received);
            assert_eq!(
                current.loss.datagrams_sent.availability,
                if transport == Transport::Http3 && data_plane == DataPlaneMode::ConnectIp {
                    MetricAvailability::Available
                } else {
                    MetricAvailability::Unsupported
                }
            );
            if transport == Transport::Http2 {
                assert_eq!(
                    current.loss.interval_basis_points,
                    MetricValue::unsupported()
                );
                assert_eq!(
                    current.congestion.congestion_window_bytes,
                    MetricValue::unsupported()
                );
            }
            runtime.shutdown().await;
            assert!(updates.changed().await.is_err());
            assert!(!underlay.cancellation.is_cancelled());
            assert_eq!(
                underlay.quality.current_smoothed_rtt(),
                Some(Duration::from_millis(20))
            );
        }
    }
}

#[tokio::test(start_paused = true)]
async fn proxy_quality_replacement_and_failure_keep_underlay_and_new_history_independent() {
    for source in [ChainSource::HttpProxy, ChainSource::Socks5Proxy] {
        let underlay = Underlay::new(source, Transport::Http3, DataPlaneMode::ConnectIp);
        let mut original = underlay.start(source, DataPlaneMode::ConnectIp).await;
        let mut old_updates = original.monitor.subscribe_network_quality();
        let first = old_updates.borrow_and_update().clone();
        assert!(first.connection_id.is_some());
        echo(&original, b"old final exit").await;
        sample_after(&mut old_updates, Duration::from_secs(1)).await;

        let replacement = underlay.start(source, DataPlaneMode::ConnectIp).await;
        let mut updates = replacement.monitor.subscribe_network_quality();
        let fresh = updates.borrow_and_update().clone();
        assert!(fresh.connection_id.is_some());
        assert_ne!(fresh.connection_id, first.connection_id);
        assert_eq!(fresh.samples.len(), 1);
        assert_eq!(fresh.samples[0].sequence, 1);
        assert_eq!(fresh.samples[0].uploaded_bytes, Some(0));

        original
            .client
            .proxy
            .as_ref()
            .unwrap()
            .fail(usque_core::vpngate::GateFailure::Authentication);
        assert!(old_updates.changed().await.is_err());
        original.shutdown().await;
        echo(&replacement, b"new").await;
        let current = sample_after(&mut updates, Duration::from_secs(1)).await;
        assert_eq!(current.connection_id, fresh.connection_id);
        assert_eq!(current.samples.last().unwrap().uploaded_bytes, Some(3));
        assert_eq!(current.samples.last().unwrap().downloaded_bytes, Some(3));
        drop(replacement);
        assert!(updates.changed().await.is_err());
        assert!(!underlay.cancellation.is_cancelled());
        assert_eq!(
            underlay.quality.current_smoothed_rtt(),
            Some(Duration::from_millis(20))
        );
    }
}

#[tokio::test(start_paused = true)]
async fn proxy_quality_follows_selected_underlay_and_preserves_unavailable_metrics() {
    for source in [ChainSource::HttpProxy, ChainSource::Socks5Proxy] {
        let underlay = Underlay::new(source, Transport::Http3, DataPlaneMode::ConnectIp);
        let mut runtime = underlay.start(source, DataPlaneMode::ConnectIp).await;
        let mut updates = runtime.monitor.subscribe_network_quality();
        let first = updates.borrow_and_update().clone();
        assert!(first.connection_id.is_some());
        let candidate = underlay
            .quality
            .new_attempt(Transport::Http2, AddressFamily::Ipv6);
        candidate.configure_h2_connection(65535, 1048576, false);

        let stale = sample_after(&mut updates, Duration::from_secs(4)).await;
        assert_eq!(stale.connection_id, first.connection_id);
        assert_eq!(stale.transport, Some(Transport::Http3));
        assert_eq!(stale.rtt.smoothed.availability, MetricAvailability::Stale);
        assert_eq!(stale.samples.last().unwrap().rtt_ms, None);

        underlay.quality.activate_attempt(&candidate);
        let promoted = sample_after(&mut updates, Duration::from_secs(1)).await;
        assert!(promoted.connection_id.is_some());
        assert_ne!(promoted.connection_id, first.connection_id);
        assert_eq!(promoted.transport, Some(Transport::Http2));
        assert_eq!(promoted.endpoint_family, Some(AddressFamily::Ipv6));
        assert_eq!(promoted.samples.len(), 1);
        assert_eq!(promoted.samples[0].sequence, 1);
        assert_eq!(promoted.rtt.smoothed, MetricValue::unsupported());
        assert_eq!(
            promoted.loss.interval_basis_points,
            MetricValue::unsupported()
        );

        underlay.quality.end_connection();
        let ended = sample_after(&mut updates, Duration::from_secs(1)).await;
        assert!(ended.connection_id.is_none());
        assert!(ended.samples.is_empty());
        assert_eq!(ended.level, crate::NetworkQualityLevel::Disconnected);
        runtime.shutdown().await;
    }
}
