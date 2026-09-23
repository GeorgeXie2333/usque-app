use super::tests::{mux_udp_packet, test_tun_io};
use super::*;
use crate::packet_pipe::{PacketReceiver, PacketSender};
use std::time::Duration;
use tokio::time::{Instant, timeout};

struct Fixture {
    io: MasqueTunIo,
    inner: TrackedReceiver<OutboundPacket>,
    incoming: TrackedSender<PacketBatch>,
    proxy_tx: PacketSender,
    proxy_rx: PacketReceiver,
    direct_tx: mpsc::Sender<Bytes>,
    quality: NetworkQualityTelemetry,
    cancel: CancellationToken,
    task: JoinHandle<()>,
}

impl Fixture {
    async fn new(proxy_capacity: usize) -> Self {
        let (mut tunnel, inner, incoming) = ManagedTunnelRuntime::packet_mux_test_channels(1);
        let quality = tunnel.monitor().network_quality_telemetry();
        let (io, raw_rx, tun_incoming) = test_tun_io(4, 4);
        let (tun_sink, _) = watch::channel(Some(tun_incoming));
        let (proxy, client) = WakingPipe::bounded(proxy_capacity);
        let cancel = CancellationToken::new();
        let (router, _) = DirectGatewayRouter::start(
            &Profile::default(),
            Arc::new(GeoDirectPolicy::disabled()),
            noop_socket_protector(),
            Arc::new(crate::netstack::TrafficCounters::default()),
            None,
            &cancel,
        )
        .await
        .unwrap();
        let (direct_tx, direct_rx) = mpsc::channel(4);
        let task_cancel = cancel.clone();
        let task_quality = quality.clone();
        let task = tokio::spawn(async move {
            run_packet_mux(
                &mut tunnel,
                proxy,
                raw_rx,
                DirectGatewayMux {
                    router,
                    incoming: direct_rx,
                },
                tun_sink,
                &task_cancel,
                task_quality,
                Arc::default(),
            )
            .await;
        });
        Self {
            io,
            inner,
            incoming,
            proxy_tx: client.tx,
            proxy_rx: client.rx,
            direct_tx,
            quality,
            cancel,
            task,
        }
    }

    async fn send(&self, origin: PacketOrigin, port: u16) {
        let packet = mux_udp_packet(port);
        match origin {
            PacketOrigin::Tunnel => self.io.send_owned_packet(packet).await.unwrap(),
            PacketOrigin::Proxy => self.proxy_tx.send_owned_async(packet).await,
        }
    }

    async fn establish(&mut self, origin: PacketOrigin) -> Bytes {
        self.send(origin, 50_000).await;
        let sent = timeout(Duration::from_secs(2), self.inner.recv())
            .await
            .unwrap()
            .unwrap();
        let mut reply = sent.to_vec();
        for n in 0..4 {
            reply.swap(12 + n, 16 + n);
        }
        for n in 0..2 {
            reply.swap(20 + n, 22 + n);
        }
        // Swapping the addresses leaves the IPv4 checksum unchanged; UDP uses zero.
        Bytes::from(reply)
    }

    async fn inject(&self, packets: &[Bytes]) {
        let mut batch = PacketBatch::new();
        for packet in packets {
            batch.push_back(packet.clone()).unwrap();
        }
        let bytes = batch.bytes();
        self.incoming.send(batch, bytes).await.unwrap();
    }

    async fn wait_for(&self, predicate: impl Fn() -> bool) {
        timeout(Duration::from_secs(2), async {
            while !predicate() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("mux did not reach the required observable state");
    }

    fn snapshot(&self, kind: QueueKind) -> crate::queue_metrics::QueueMetricsSnapshot {
        self.quality.queue(kind).snapshot(Instant::now())
    }

    async fn saturate(&self, origin: PacketOrigin) {
        self.send(origin, 50_001).await;
        self.send(origin, 50_002).await;
        self.wait_for(|| {
            self.snapshot(QueueKind::TransportOutgoingPackets)
                .backpressure
                .unwrap()
                .active
                == 1
        })
        .await;
        assert_eq!(
            self.snapshot(QueueKind::TransportOutgoingPackets)
                .current_items,
            1
        );
    }

    async fn stop(&mut self) {
        self.cancel.cancel();
        timeout(Duration::from_secs(2), &mut self.task)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            self.snapshot(QueueKind::TransportOutgoingPackets)
                .backpressure
                .unwrap()
                .active,
            0
        );
        assert_eq!(self.snapshot(QueueKind::ProxyToTransport).current_items, 0);
        assert_eq!(self.snapshot(QueueKind::TransportToProxy).current_items, 0);
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

#[test]
fn pending_send_keeps_expired_nat_mapping_until_maintenance_can_resume() {
    for idle in [false, true] {
        let mut flows = PacketMuxTable::default();
        let mut first = mux_udp_packet(50_000).to_vec();
        let mut collision = first.clone();
        assert!(flows.route_outgoing(PacketOrigin::Tunnel, &mut first));
        assert!(flows.route_outgoing(PacketOrigin::Proxy, &mut collision));
        let expiry = std::time::Instant::now() + Duration::from_secs(360);
        maintain_mux_flows(&mut flows, idle, expiry);
        for n in 0..4 {
            collision.swap(12 + n, 16 + n);
        }
        for n in 0..2 {
            collision.swap(20 + n, 22 + n);
        }
        assert_eq!(
            flows.route_incoming(&mut collision),
            (!idle).then_some(PacketOrigin::Proxy)
        );
        if !idle {
            assert_eq!(&collision[22..24], &50_000u16.to_be_bytes());
        }
    }
}

#[tokio::test]
async fn saturated_proxy_admission_resumes_exactly_once_in_order() {
    let mut f = Fixture::new(4).await;
    f.saturate(PacketOrigin::Proxy).await;
    for port in [50_001u16, 50_002] {
        let packet = timeout(Duration::from_secs(2), f.inner.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(&packet[20..22], &port.to_be_bytes());
    }
    f.wait_for(|| f.snapshot(QueueKind::ProxyToTransport).current_items == 0)
        .await;
    let wait = f
        .snapshot(QueueKind::TransportOutgoingPackets)
        .backpressure
        .unwrap();
    assert_eq!((wait.waits, wait.completed, wait.active), (1, 1, 0));
    assert!(f.inner.try_recv().is_err());
    f.stop().await;
}

#[tokio::test]
async fn receiver_close_settles_wait_and_releases_proxy_accounting() {
    let mut f = Fixture::new(4).await;
    f.saturate(PacketOrigin::Proxy).await;
    f.inner.close();
    f.wait_for(|| f.task.is_finished()).await;
    assert_eq!(
        f.snapshot(QueueKind::TransportOutgoingPackets)
            .backpressure
            .unwrap()
            .closed,
        1
    );
    let accepted = f.inner.recv().await.unwrap();
    assert_eq!(&accepted[20..22], &50_001u16.to_be_bytes());
    assert!(f.inner.recv().await.is_none());
    assert_eq!(
        f.snapshot(QueueKind::TransportOutgoingPackets)
            .current_items,
        0
    );
    f.stop().await;
}

#[tokio::test]
async fn global_cancel_drops_the_bounded_proxy_delivery_tail() {
    let mut f = Fixture::new(1).await;
    let reply = f.establish(PacketOrigin::Proxy).await;
    f.inject(std::slice::from_ref(&reply)).await;
    f.wait_for(|| f.proxy_rx.rx_ready()).await;
    f.inject(&[reply.clone(), reply]).await;
    f.wait_for(|| f.snapshot(QueueKind::TransportToProxy).current_items == 2)
        .await;
    f.stop().await;
    assert_eq!(f.snapshot(QueueKind::TransportToProxy).drop_items, 2);
    assert!(f.proxy_rx.recv_async().await.is_some());
    assert!(f.proxy_rx.recv_async().await.is_none());
}

#[tokio::test]
async fn full_uplink_keeps_both_reply_origins_and_direct_replies_moving() {
    for origin in [PacketOrigin::Tunnel, PacketOrigin::Proxy] {
        let mut f = Fixture::new(4).await;
        let tun_reply = f.establish(PacketOrigin::Tunnel).await;
        let proxy_reply = f.establish(PacketOrigin::Proxy).await;
        assert_ne!(
            &tun_reply[22..24],
            &proxy_reply[22..24],
            "colliding flows use NAT"
        );
        f.saturate(origin).await;
        f.inject(&[proxy_reply, tun_reply.clone()]).await;
        let received = timeout(Duration::from_secs(2), f.io.receive_packet())
            .await
            .expect("TUN reply must arrive while uplink stays full")
            .unwrap();
        assert_eq!(received, tun_reply);
        let received = timeout(Duration::from_secs(2), f.proxy_rx.recv_async())
            .await
            .expect("proxy reply must arrive while uplink stays full")
            .unwrap();
        assert_eq!(
            received, tun_reply,
            "restore the original port and checksum"
        );
        f.direct_tx.send(tun_reply.clone()).await.unwrap();
        assert_eq!(
            timeout(Duration::from_secs(2), f.io.receive_packet())
                .await
                .unwrap()
                .unwrap(),
            tun_reply
        );
        assert_eq!(
            f.snapshot(QueueKind::TransportOutgoingPackets)
                .current_items,
            1
        );
        f.stop().await; // Global cancellation must not require releasing capacity.
    }
}

#[tokio::test]
async fn blocked_proxy_downlink_delivers_same_batch_tun_and_allows_uplink() {
    let mut f = Fixture::new(1).await;
    let tun_reply = f.establish(PacketOrigin::Tunnel).await;
    let proxy_reply = f.establish(PacketOrigin::Proxy).await;
    f.inject(std::slice::from_ref(&proxy_reply)).await;
    f.wait_for(|| f.proxy_rx.rx_ready()).await;
    f.inject(&[proxy_reply, tun_reply.clone()]).await;
    assert_eq!(
        timeout(Duration::from_secs(2), f.io.receive_packet())
            .await
            .expect("proxy wait must not hold the TUN part of this batch")
            .unwrap(),
        tun_reply
    );
    f.send(PacketOrigin::Tunnel, 50_003).await;
    let sent = timeout(Duration::from_secs(2), f.inner.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(&sent[20..22], &50_003u16.to_be_bytes());
    for _ in 0..2 {
        assert_eq!(
            timeout(Duration::from_secs(2), f.proxy_rx.recv_async())
                .await
                .unwrap()
                .unwrap(),
            tun_reply
        );
    }
    assert!(f.proxy_rx.try_recv().is_none(), "no duplicate delivery");
    f.stop().await;
}

#[tokio::test]
async fn attachment_cancel_revokes_only_pending_packet_and_preserves_admitted_order() {
    let mut f = Fixture::new(4).await;
    f.saturate(PacketOrigin::Tunnel).await;
    f.io.cancellation.cancel();
    f.wait_for(|| {
        f.snapshot(QueueKind::TransportOutgoingPackets)
            .backpressure
            .unwrap()
            .cancelled
            == 1
    })
    .await;
    let packet = mux_udp_packet(50_004);
    f.io.outgoing
        .send(
            TunOutbound {
                packet: packet.into(),
                attachment: CancellationToken::new(),
            },
            28,
        )
        .await
        .unwrap();
    for port in [50_001u16, 50_004] {
        let packet = timeout(Duration::from_secs(2), f.inner.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(&packet[20..22], &port.to_be_bytes());
    }
    assert!(
        f.inner.try_recv().is_err(),
        "cancelled send was not admitted"
    );
    assert!(
        !f.task.is_finished(),
        "attachment cancellation must not close the session"
    );
    f.stop().await;
}
