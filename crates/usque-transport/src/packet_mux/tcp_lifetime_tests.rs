// Inert packets exercise mapping ownership; no socket or OS networking is used.
fn tcp_packet(ports: (u16, u16), incoming: bool, ipv6: bool, flags: u8, sequence: u32) -> Vec<u8> {
    let offset = if ipv6 { 40 } else { 20 };
    let mut packet = vec![0; offset + 20];
    if ipv6 {
        packet[0] = 0x60;
        packet[4..6].copy_from_slice(&20_u16.to_be_bytes());
        packet[6] = 6;
        packet[7] = 64;
        packet[8..24].copy_from_slice(&"2001:db8::1".parse::<Ipv6Addr>().unwrap().octets());
        packet[24..40].copy_from_slice(&"2001:db8::2".parse::<Ipv6Addr>().unwrap().octets());
        if incoming {
            for index in 0..16 {
                packet.swap(8 + index, 24 + index);
            }
        }
    } else {
        packet[0] = 0x45;
        packet[2..4].copy_from_slice(&40_u16.to_be_bytes());
        packet[8] = 64;
        packet[9] = 6;
        packet[12..16].copy_from_slice(&[172, 16, 0, 2]);
        packet[16..20].copy_from_slice(&[203, 0, 113, 8]);
        if incoming {
            for index in 0..4 {
                packet.swap(12 + index, 16 + index);
            }
        }
    }
    packet[offset..offset + 2].copy_from_slice(&ports.0.to_be_bytes());
    packet[offset + 2..offset + 4].copy_from_slice(&ports.1.to_be_bytes());
    packet[offset + 4..offset + 8].copy_from_slice(&sequence.to_be_bytes());
    packet[offset + 12] = 0x50;
    packet[offset + 13] = flags;
    packet[offset + 16..offset + 18].copy_from_slice(&0x1234_u16.to_be_bytes());
    packet
}

#[test]
fn tcp_retransmissions_and_half_closed_data_keep_their_mapping() {
    for ipv6 in [false, true] {
        let mut table = PacketMuxTable::default();
        let syn = tcp_packet((50_000, 443), false, ipv6, 0xc2, 100);
        assert!(table.route_outgoing(PacketOrigin::Tunnel, &mut syn.clone()));
        for _ in 0..3 {
            let inspection = table.inspect_outgoing(PacketOrigin::Tunnel, &syn);
            assert!(
                inspection.is_owned(),
                "a retransmitted SYN is the same flow"
            );
            assert!(table.route_inspected_outgoing(&mut syn.clone(), inspection));
        }
        let mut reply = tcp_packet((443, 50_000), true, ipv6, 0x12, 200);
        assert_eq!(table.route_incoming(&mut reply), Some(PacketOrigin::Tunnel));
        let mut fin = tcp_packet((50_000, 443), false, ipv6, 0x11, 101);
        assert!(table.route_outgoing(PacketOrigin::Tunnel, &mut fin));
        assert!(
            table
                .inspect_outgoing(PacketOrigin::Tunnel, &syn)
                .is_owned()
        );
        let mut data = tcp_packet((443, 50_000), true, ipv6, 0x18, 201);
        assert_eq!(table.route_incoming(&mut data), Some(PacketOrigin::Tunnel));
        assert_eq!(table.flow_scan.len(), 1);
    }
}

#[test]
fn closed_tcp_tuple_requires_new_admission_even_when_its_isn_is_reused() {
    for ipv6 in [false, true] {
        for close in ["fin", "local_reset", "remote_reset"] {
            let mut table = PacketMuxTable::default();
            let syn = tcp_packet((50_000, 443), false, ipv6, 0x02, 100);
            assert!(table.route_outgoing(PacketOrigin::Tunnel, &mut syn.clone()));
            if close == "fin" {
                let mut fin = tcp_packet((50_000, 443), false, ipv6, 0x11, 101);
                assert!(table.route_outgoing(PacketOrigin::Tunnel, &mut fin));
            }
            if close == "local_reset" {
                let mut reset = tcp_packet((50_000, 443), false, ipv6, 0x14, 101);
                assert!(table.route_outgoing(PacketOrigin::Tunnel, &mut reset));
            } else {
                let flags = if close == "fin" { 0x11 } else { 0x14 };
                let mut reply = tcp_packet((443, 50_000), true, ipv6, flags, 201);
                assert_eq!(table.route_incoming(&mut reply), Some(PacketOrigin::Tunnel));
            }
            let mut ack = tcp_packet((50_000, 443), false, ipv6, 0x10, 102);
            assert!(
                table
                    .inspect_outgoing(PacketOrigin::Tunnel, &ack)
                    .is_owned()
            );
            assert!(table.route_outgoing(PacketOrigin::Tunnel, &mut ack));
            assert!(
                !table
                    .inspect_outgoing(PacketOrigin::Tunnel, &syn)
                    .is_owned()
            );
            assert!(table.forward.is_empty());
            assert!(table.reverse.is_empty());
            assert!(table.flow_scan.is_empty());
            assert!(
                table
                    .route_incoming(&mut tcp_packet((443, 50_000), true, ipv6, 0x10, 202))
                    .is_none()
            );
        }
    }
}

#[test]
fn a_new_tcp_isn_reclaims_only_its_origin_and_bounds_replacement_indexes() {
    for ipv6 in [false, true] {
        let mut table = PacketMuxTable {
            flow_limit: 2,
            ..Default::default()
        };
        let syn = tcp_packet((50_000, 443), false, ipv6, 0x02, u32::MAX);
        assert!(table.route_outgoing(PacketOrigin::Tunnel, &mut syn.clone()));
        let mut proxy = syn.clone();
        assert!(table.route_outgoing(PacketOrigin::Proxy, &mut proxy));
        let offset = if ipv6 { 40 } else { 20 };
        let proxy_port = read_u16(&proxy, offset).unwrap();
        assert_ne!(proxy_port, 50_000);
        for sequence in 0..1000 {
            let mut fresh = tcp_packet((50_000, 443), false, ipv6, 0x02, sequence);
            assert!(
                !table
                    .inspect_outgoing(PacketOrigin::Tunnel, &fresh)
                    .is_owned()
            );
            assert_eq!(table.forward.len(), 1);
            assert!(table.route_outgoing(PacketOrigin::Tunnel, &mut fresh));
            assert_eq!(
                (
                    table.forward.len(),
                    table.reverse.len(),
                    table.flow_scan.len()
                ),
                (2, 2, 2)
            );
            let mut reply = tcp_packet((443, proxy_port), true, ipv6, 0x12, 200);
            assert_eq!(table.route_incoming(&mut reply), Some(PacketOrigin::Proxy));
            assert_eq!(read_u16(&reply, offset + 2), Some(50_000));
        }
    }
}
