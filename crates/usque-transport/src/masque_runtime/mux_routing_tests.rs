use super::tests::test_tun_io;
use super::*;
use std::time::Duration;
use tokio::time::timeout;

fn tcp_packet(flags: u8, port: u16, incoming: bool, sequence: u32, ack: u32) -> Bytes {
    fn checksum(data: &[u8]) -> u16 {
        let mut sum: u32 = data
            .chunks(2)
            .map(|word| u32::from(u16::from_be_bytes([word[0], *word.get(1).unwrap_or(&0)])))
            .sum();
        while sum > 0xffff {
            sum = (sum & 0xffff) + (sum >> 16);
        }
        !(sum as u16)
    }
    let mut packet = vec![0_u8; 40];
    packet[0] = 0x45;
    packet[2..4].copy_from_slice(&40_u16.to_be_bytes());
    packet[8] = 64;
    packet[9] = 6;
    packet[12..16].copy_from_slice(&[172, 16, 0, 2]);
    packet[16..20].copy_from_slice(&[198, 51, 100, 1]);
    packet[20..22].copy_from_slice(&port.to_be_bytes());
    packet[22..24].copy_from_slice(&443_u16.to_be_bytes());
    if incoming {
        for index in 0..4 {
            packet.swap(12 + index, 16 + index);
        }
        for index in 0..2 {
            packet.swap(20 + index, 22 + index);
        }
    }
    packet[24..28].copy_from_slice(&sequence.to_be_bytes());
    packet[28..32].copy_from_slice(&ack.to_be_bytes());
    packet[32] = 0x50;
    packet[33] = flags;
    packet[34..36].copy_from_slice(&u16::MAX.to_be_bytes());
    let ip_sum = checksum(&packet[..20]);
    packet[10..12].copy_from_slice(&ip_sum.to_be_bytes());
    let mut tcp = packet[12..20].to_vec();
    tcp.extend_from_slice(&[0, 6, 0, 20]);
    tcp.extend_from_slice(&packet[20..]);
    packet[36..38].copy_from_slice(&checksum(&tcp).to_be_bytes());
    Bytes::from(packet)
}

#[tokio::test]
async fn hot_reject_preserves_live_tcp_but_blocks_reuse_after_close() {
    // Real mux and routing actors with memory packet queues; no OS TUN or
    // remote connection. Check both same-ISN reuse and a new initial sequence.
    for new_sequence in [10, 1000] {
        let (mut tunnel, mut inner_rx, managed_incoming) =
            ManagedTunnelRuntime::packet_mux_test_channels(8);
        let (mut io, raw_rx, incoming_tx) = test_tun_io(8, 8);
        let (tun_sink, _watch) = watch::channel(Some(incoming_tx));
        let (proxy_pipe, _proxy) = WakingPipe::bounded(4);
        let cancel = CancellationToken::new();
        let geo = Arc::new(GeoDirectPolicy::disabled());
        let (router, incoming) = DirectGatewayRouter::start(
            &Profile::default(),
            geo.clone(),
            crate::socket::noop_socket_protector(),
            Arc::default(),
            None,
            &cancel,
        )
        .await
        .unwrap();
        let task_cancel = cancel.clone();
        let task = tokio_util::task::AbortOnDropHandle::new(tokio::spawn(async move {
            run_packet_mux(
                &mut tunnel,
                proxy_pipe,
                raw_rx,
                DirectGatewayMux { router, incoming },
                tun_sink,
                &task_cancel,
                NetworkQualityTelemetry::default(),
                Arc::default(),
            )
            .await;
        }));
        io.send_owned_packet(tcp_packet(0x02, 50_000, false, 10, 0))
            .await
            .unwrap();
        assert!(
            timeout(Duration::from_secs(1), inner_rx.recv())
                .await
                .unwrap()
                .is_some()
        );
        let next = crate::geo_direct::routing_test_profile(&[(
            "198.51.100.1",
            usque_core::RoutingAction::Reject,
        )]);
        geo.publish_routing_update(geo.prepare_routing_update(&next).unwrap())
            .unwrap();
        // A retransmission, the remainder of the handshake, half close and
        // final ACK still belong to the original admitted connection.
        for (flags, incoming, sequence, ack) in [
            (0x02, false, 10, 0),
            (0x12, true, 20, 11),
            (0x10, false, 11, 21),
            (0x11, false, 11, 21),
            (0x11, true, 21, 12),
            (0x10, false, 12, 22),
        ] {
            let packet = tcp_packet(flags, 50_000, incoming, sequence, ack);
            if incoming {
                let length = packet.len();
                managed_incoming
                    .send(PacketBatch::single(packet), length)
                    .await
                    .unwrap();
                timeout(Duration::from_secs(1), io.receive_packet())
                    .await
                    .unwrap()
                    .unwrap();
            } else {
                io.send_owned_packet(packet).await.unwrap();
                assert!(
                    timeout(Duration::from_secs(1), inner_rx.recv())
                        .await
                        .unwrap()
                        .is_some()
                );
            }
        }
        for port in [50_001, 50_000] {
            io.send_owned_packet(tcp_packet(0x02, port, false, new_sequence, 0))
                .await
                .unwrap();
            let rejection = timeout(Duration::from_secs(1), io.receive_packet())
                .await
                .unwrap()
                .unwrap();
            assert_ne!(rejection[33] & 0x04, 0, "new connections receive a reset");
            assert!(
                inner_rx.try_recv().is_err(),
                "new SYN must never reach the tunnel"
            );
        }
        cancel.cancel();
        timeout(Duration::from_secs(1), task)
            .await
            .unwrap()
            .unwrap();
    }
}
