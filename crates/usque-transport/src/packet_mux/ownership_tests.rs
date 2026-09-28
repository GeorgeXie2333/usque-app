// Included in packet_mux::tests; packets stay shared to force any real rewrite
// through copy-on-write rather than allowing a coincidental unique allocation.

#[test]
fn inbound_owned_rewrites_only_the_required_identifier_or_quote() {
    let mut table = PacketMuxTable::default();
    let mut tun = udp_packet(50_000, 443, false);
    let mut proxy = tun.clone();
    assert!(table.route_outgoing(PacketOrigin::Tunnel, &mut tun));
    assert!(table.route_outgoing(PacketOrigin::Proxy, &mut proxy));
    let wire_port = read_u16(&proxy, 20).unwrap();
    for (packet, offset) in [
        (udp_packet(443, wire_port, true), 22),
        (icmp_unreachable(&proxy), 48),
    ] {
        let original = bytes::Bytes::from(packet);
        let routed = table.route_owned_incoming(original.clone()).unwrap();
        assert_eq!(routed.origin, PacketOrigin::Proxy);
        assert_eq!(routed.copied_bytes, original.len());
        assert_ne!(routed.packet.as_ptr(), original.as_ptr());
        assert_eq!(read_u16(&routed.packet, offset), Some(50_000));
        assert_eq!(
            read_u16(&original, offset),
            Some(wire_port),
            "shared source is unchanged"
        );
    }
    let unique = bytes::Bytes::from(udp_packet(443, wire_port, true));
    let pointer = unique.as_ptr();
    let routed = table.route_owned_incoming(unique).unwrap();
    assert_eq!(routed.packet.as_ptr(), pointer);
    assert_eq!(routed.copied_bytes, 0);
    assert_eq!(read_u16(&routed.packet, 22), Some(50_000));
}

#[test]
fn unchanged_ipv4_fragment_and_icmp_replies_keep_shared_storage() {
    let mut table = PacketMuxTable::default();
    let mut request = udp_packet(50_000, 443, false);
    assert!(table.route_outgoing(PacketOrigin::Tunnel, &mut request));
    let mut first_fragment = udp_packet(443, 50_000, true);
    fragment(&mut first_fragment, 17, 0, true);
    for packet in [
        udp_packet(443, 50_000, true),
        first_fragment,
        later_udp_fragment(17, true),
        icmp_unreachable(&request),
    ] {
        let original = bytes::Bytes::from(packet);
        let routed = table.route_owned_incoming(original.clone()).unwrap();
        assert_eq!(routed.packet, original);
        assert_eq!(routed.packet.as_ptr(), original.as_ptr());
        assert_eq!(routed.copied_bytes, 0);
    }
    table.traffic_policy.set_disable_quic(true);
    let denied = bytes::Bytes::from(udp_packet(443, 50_000, true));
    assert!(table.route_owned_incoming(denied.clone()).is_none());
    assert_eq!(read_u16(&denied, 22), Some(50_000));
}

#[test]
fn ipv6_owned_reply_retains_storage_until_nat_is_required() {
    fn packet(source: u16, destination: u16, reply: bool) -> Vec<u8> {
        let mut packet = vec![0u8; 48];
        packet[0] = 0x60;
        packet[4..6].copy_from_slice(&8u16.to_be_bytes());
        packet[6] = 17;
        packet[7] = 64;
        packet[8..24].copy_from_slice(&"2001:db8::1".parse::<Ipv6Addr>().unwrap().octets());
        packet[24..40].copy_from_slice(&"2001:db8::2".parse::<Ipv6Addr>().unwrap().octets());
        if reply {
            for n in 0..16 {
                packet.swap(8 + n, 24 + n);
            }
        }
        packet[40..42].copy_from_slice(&source.to_be_bytes());
        packet[42..44].copy_from_slice(&destination.to_be_bytes());
        packet[44..46].copy_from_slice(&8u16.to_be_bytes());
        packet[46..48].copy_from_slice(&0x1234u16.to_be_bytes());
        packet
    }
    let mut table = PacketMuxTable::default();
    let mut request = packet(50_000, 443, false);
    let mut collision = request.clone();
    assert!(table.route_outgoing(PacketOrigin::Tunnel, &mut request));
    assert!(table.route_outgoing(PacketOrigin::Proxy, &mut collision));
    let reply = bytes::Bytes::from(packet(443, 50_000, true));
    let routed = table.route_owned_incoming(reply.clone()).unwrap();
    assert_eq!(routed.packet.as_ptr(), reply.as_ptr());
    assert_eq!(routed.copied_bytes, 0);
    let wire_id = read_u16(&collision, 40).unwrap();
    let reply = bytes::Bytes::from(packet(443, wire_id, true));
    let routed = table.route_owned_incoming(reply.clone()).unwrap();
    assert_eq!(routed.copied_bytes, 48);
    assert_eq!(read_u16(&routed.packet, 42), Some(50_000));
    assert_eq!(read_u16(&reply, 42), Some(wire_id));
    assert_ne!(&routed.packet[46..48], &reply[46..48]);
}
