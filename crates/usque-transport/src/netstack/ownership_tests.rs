use super::*;
use crate::android_tun_read_slab::TunReadSlab;
use crate::masque_runtime::MasqueRuntime;
use crate::packet_mux::{PacketMuxTable, PacketOrigin};

fn udp_packet(ipv6: bool, sequence: u16) -> Vec<u8> {
    let offset = if ipv6 { 40 } else { 20 };
    let mut packet = vec![0; offset + 8];
    if ipv6 {
        packet[0] = 0x60;
        packet[4..6].copy_from_slice(&8_u16.to_be_bytes());
        packet[6] = 17;
        packet[7] = 64;
        packet[8..24].copy_from_slice(
            &"2001:db8::2"
                .parse::<std::net::Ipv6Addr>()
                .unwrap()
                .octets(),
        );
        packet[24..40].copy_from_slice(
            &"2001:db8::1"
                .parse::<std::net::Ipv6Addr>()
                .unwrap()
                .octets(),
        );
    } else {
        packet[0] = 0x45;
        packet[2..4].copy_from_slice(&28_u16.to_be_bytes());
        packet[8] = 64;
        packet[9] = 17;
        packet[12..16].copy_from_slice(&[172, 16, 0, 2]);
        packet[16..20].copy_from_slice(&[198, 51, 100, 1]);
        let checksum = ipv4_header_checksum(&packet[..20]);
        packet[10..12].copy_from_slice(&checksum.to_be_bytes());
    }
    packet[offset..offset + 2].copy_from_slice(&(50000 + sequence).to_be_bytes());
    packet[offset + 2..offset + 4].copy_from_slice(&443_u16.to_be_bytes());
    packet[offset + 4..offset + 6].copy_from_slice(&8_u16.to_be_bytes());
    if ipv6 {
        let checksum = udp_v6_checksum(&packet);
        packet[offset + 6..offset + 8].copy_from_slice(&checksum.to_be_bytes());
    }
    packet
}

fn udp_v6_checksum(packet: &[u8]) -> u16 {
    let mut pseudo = packet[8..40].to_vec();
    pseudo.extend_from_slice(&8_u32.to_be_bytes());
    pseudo.extend_from_slice(&[0, 0, 0, 17]);
    pseudo.extend_from_slice(&packet[40..]);
    ipv4_header_checksum(&pseudo)
}

fn slab_packet(slab: &mut TunReadSlab, mtu: usize, packet: &[u8]) -> BytesMut {
    slab.prepare(mtu).unwrap();
    slab.read_buffer()[..packet.len()].copy_from_slice(packet);
    slab.take_packet(packet.len()).unwrap()
}

#[tokio::test]
async fn actual_android_slabs_keep_their_pointers_through_mux_and_forwarding_batches() {
    let (tunnel, outgoing, incoming) = ManagedTunnelRuntime::packet_mux_test_channels(256);
    let profile = Profile {
        frontends: usque_core::FrontendSettings {
            tunnel: true,
            socks5: false,
            http: false,
        },
        ..Profile::default()
    };
    let mut runtime = MasqueRuntime::start_over_tunnel(
        &profile,
        tunnel,
        (
            "172.16.0.2".parse().unwrap(),
            "2001:db8::2".parse().unwrap(),
        ),
        crate::socket::noop_socket_protector(),
        Arc::new(GeoDirectPolicy::disabled()),
    )
    .await
    .unwrap();
    let tun = runtime.attach_tun().unwrap();
    let mut slab = TunReadSlab::new();
    let mut pointers = Vec::new();
    for sequence in 0..130 {
        let mtu = if sequence < 65 { 1280 } else { 1500 };
        let packet = slab_packet(&mut slab, mtu, &udp_packet(sequence % 2 != 0, sequence));
        pointers.push(packet.as_ptr() as usize);
        tun.start_send_mut_packet(packet).await.unwrap();
    }
    let mut io = PacketIo::Channel {
        outgoing,
        incoming,
        buffered_outgoing: None,
    };
    let mut received = Vec::new();
    while received.len() < pointers.len() {
        let mut batch = timeout(Duration::from_secs(2), io.receive_outgoing_batch())
            .await
            .unwrap()
            .unwrap();
        while let Some(packet) = batch.pop_front() {
            let index = received.len();
            assert_eq!(
                packet.as_ptr() as usize,
                pointers[index],
                "whole-packet copy at {index}"
            );
            let ipv6 = index % 2 != 0;
            let offset = if ipv6 { 40 } else { 20 };
            assert_eq!(
                packet[if ipv6 { 7 } else { 8 }],
                63,
                "exactly one forwarding decrement"
            );
            assert_eq!(
                u16::from_be_bytes([packet[offset], packet[offset + 1]]),
                50000 + index as u16
            );
            assert_eq!(
                if ipv6 {
                    udp_v6_checksum(&packet)
                } else {
                    ipv4_header_checksum(&packet[..20])
                },
                0
            );
            received.push(packet); // Keep sibling views alive through every freeze.
        }
    }
    // Both MTU-sized slabs and all received siblings are still live here.
    assert_eq!(slab.read_buffer().len(), 1500);
    drop(tun);
    runtime.shutdown().await;
}

#[tokio::test]
async fn mutable_slab_nat_and_forwarding_edit_only_the_owned_view() {
    for ipv6 in [false, true] {
        let original = udp_packet(ipv6, 0);
        let mut slab = TunReadSlab::new();
        let first = slab_packet(&mut slab, 1280, &original);
        let sibling = slab_packet(&mut slab, 1280, &original);
        let pointer = first.as_ptr() as usize;
        let mut table = PacketMuxTable::default();
        assert!(table.route_outgoing(PacketOrigin::Proxy, &mut original.clone()));
        let mut first = OutboundPacket::Mutable(first).into_mut();
        assert!(table.route_outgoing(PacketOrigin::Tunnel, &mut first));
        let offset = if ipv6 { 40 } else { 20 };
        assert_ne!(&first[offset..offset + 2], &original[offset..offset + 2]);
        let (outgoing_tx, outgoing) =
            tests::test_packet_channel(QueueKind::TransportOutgoingPackets, 1);
        let (incoming, _incoming_rx) = tests::test_batch_channel(QueueKind::TransportToTun, 1);
        let length = first.len();
        outgoing_tx
            .try_send(OutboundPacket::Mutable(first), length)
            .unwrap();
        let mut io = PacketIo::Channel {
            outgoing,
            incoming,
            buffered_outgoing: None,
        };
        let packet = io.receive_outgoing().await.unwrap();
        assert_eq!(packet.as_ptr() as usize, pointer);
        assert_eq!(sibling.as_ref(), &original);
        assert_eq!(packet[if ipv6 { 7 } else { 8 }], 63);
        assert_eq!(
            if ipv6 {
                udp_v6_checksum(&packet)
            } else {
                ipv4_header_checksum(&packet[..20])
            },
            0
        );
    }
}

#[tokio::test]
async fn shared_bytes_compatibility_copies_only_when_another_owner_is_live() {
    let shared = Bytes::from(udp_packet(false, 0));
    let witness = shared.clone();
    let (sender, mut receiver, _) = tests::test_managed_sender(1);
    sender.send_owned_packet(shared).await.unwrap();
    let mut packet = receiver.recv().await.unwrap().into_mut();
    prepare_forwarded_packet(&mut packet).unwrap();
    assert_ne!(packet.as_ptr(), witness.as_ptr());
    assert_eq!(packet[8], 63);
    assert_eq!(witness[8], 64);
}
