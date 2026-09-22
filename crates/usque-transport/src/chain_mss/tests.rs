use super::*;
use proptest::prelude::*;
use smoltcp::wire::{IpAddress, Ipv4Packet, Ipv6Packet, TcpPacket};

fn syn(ipv6: bool, options: &[u8], flags: u8, extension: bool, payload: &[u8]) -> Bytes {
    assert!(options.len().is_multiple_of(4));
    let ip_len = if ipv6 {
        40 + usize::from(extension) * 8
    } else {
        20
    };
    let tcp_len = 20 + options.len() + payload.len();
    let mut p = vec![0; ip_len + tcp_len];
    if ipv6 {
        p[0] = 0x60;
        p[4..6].copy_from_slice(&((ip_len + tcp_len - 40) as u16).to_be_bytes());
        p[6] = if extension { 60 } else { 6 };
        p[7] = 64;
        p[8..24].copy_from_slice(
            &"2001:db8::1"
                .parse::<std::net::Ipv6Addr>()
                .unwrap()
                .octets(),
        );
        p[24..40].copy_from_slice(
            &"2001:db8::2"
                .parse::<std::net::Ipv6Addr>()
                .unwrap()
                .octets(),
        );
        if extension {
            p[40] = 6;
        }
    } else {
        p[0] = 0x45;
        p[2..4].copy_from_slice(&((ip_len + tcp_len) as u16).to_be_bytes());
        p[8] = 64;
        p[9] = 6;
        p[12..16].copy_from_slice(&[192, 0, 2, 1]);
        p[16..20].copy_from_slice(&[192, 0, 2, 2]);
        Ipv4Packet::new_unchecked(&mut p).fill_checksum();
    }
    let (src, dst) = addresses(&p);
    let tcp = &mut p[ip_len..];
    tcp[..2].copy_from_slice(&40000u16.to_be_bytes());
    tcp[2..4].copy_from_slice(&443u16.to_be_bytes());
    tcp[12] = ((20 + options.len()) / 4) as u8 * 16;
    tcp[13] = flags;
    tcp[14..16].copy_from_slice(&32768u16.to_be_bytes());
    tcp[20..20 + options.len()].copy_from_slice(options);
    tcp[20 + options.len()..].copy_from_slice(payload);
    TcpPacket::new_unchecked(tcp).fill_checksum(&src, &dst);
    p.into()
}
fn addresses(p: &[u8]) -> (IpAddress, IpAddress) {
    if p[0] >> 4 == 4 {
        let ip = Ipv4Packet::new_unchecked(p);
        (ip.src_addr().into(), ip.dst_addr().into())
    } else {
        let ip = Ipv6Packet::new_unchecked(p);
        (ip.src_addr().into(), ip.dst_addr().into())
    }
}
fn checksum_valid(p: &[u8]) -> bool {
    let (offset, ipv6) = tcp_offset(p).unwrap();
    if !ipv6 {
        assert!(Ipv4Packet::new_unchecked(p).verify_checksum());
    }
    let (src, dst) = addresses(p);
    TcpPacket::new_unchecked(&p[offset..]).verify_checksum(&src, &dst)
}
fn mss(p: &[u8]) -> Option<u16> {
    let (offset, _) = tcp_offset(p).unwrap();
    let tcp = TcpPacket::new_unchecked(&p[offset..]);
    smoltcp::wire::TcpRepr::parse(
        &tcp,
        &addresses(p).0,
        &addresses(p).1,
        &smoltcp::phy::ChecksumCapabilities::default(),
    )
    .unwrap()
    .max_seg_size
}

#[test]
fn budgets_include_outer_headers_crypto_and_wireguard_peer_padding() {
    for v6 in [false, true] {
        let outer = if v6 { 48 } else { 28 };
        let wg = packet_budget(ChainProtocol::Wireguard, v6).unwrap();
        assert_eq!(wg, if v6 { 1200 } else { 1216 });
        assert_eq!(wg % 16, 0);
        assert!(wg + 32 + outer <= WARP_MTU);
        assert!((wg + 1).next_multiple_of(16) + 32 + outer > WARP_MTU);
        assert!(packet_budget(ChainProtocol::OpenvpnUdp, v6).unwrap() + outer + 128 <= WARP_MTU);
        assert_eq!(packet_budget(ChainProtocol::OpenvpnTcp, v6), None);
    }
}

#[test]
fn syn_and_synack_mss_are_lowered_with_valid_checksums_and_unchanged_payload() {
    for v6 in [false, true] {
        for flags in [2, 18, 0xc2] {
            for options in [&[2, 4, 5, 180][..], &[1, 2, 4, 5, 180, 0, 0, 0][..]] {
                for protocol in [ChainProtocol::Wireguard, ChainProtocol::OpenvpnUdp] {
                    let packet = syn(v6, options, flags, v6, b"TCP Fast Open test payload");
                    assert!(checksum_valid(&packet));
                    let budget = packet_budget(protocol, false);
                    let result = clamp(packet.clone(), budget, Transport::Http3);
                    assert_eq!(
                        mss(&result),
                        Some(budget.unwrap() - if v6 { 68 } else { 40 })
                    );
                    assert_eq!(packet.len(), result.len());
                    assert!(result.ends_with(b"TCP Fast Open test payload"));
                    assert!(checksum_valid(&result));
                    assert_eq!(clamp(result.clone(), budget, Transport::Http3), result);
                }
            }
        }
    }
}

#[test]
fn h2_tcp_smaller_mss_and_non_syn_keep_original_bytes_and_allocation() {
    let small = syn(false, &[2, 4, 2, 24], 2, false, &[]);
    let large = syn(true, &[2, 4, 5, 180], 2, false, &[]);
    let data = syn(false, &[2, 4, 5, 180], 16, false, b"unchanged");
    for (p, budget, transport) in [
        (small, Some(1124), Transport::Http3),
        (large.clone(), Some(1124), Transport::Http2),
        (large, None, Transport::Http3),
        (data, Some(1124), Transport::Http3),
    ] {
        let ptr = p.as_ptr();
        let output = clamp(p.clone(), budget, transport);
        assert_eq!(output, p);
        assert_eq!(output.as_ptr(), ptr);
    }
}

#[test]
fn missing_ipv6_mss_is_added_without_changing_payload_or_checksums() {
    for options in [&[][..], &[0, 0, 0, 0][..], &[1, 1, 0, 0][..]] {
        let p = syn(true, options, 2, false, b"payload");
        let result = clamp(p, Some(1200), Transport::Http3);
        assert_eq!(mss(&result), Some(1140));
        assert!(checksum_valid(&result));
        assert!(result.ends_with(b"payload"));
    }
    let p = syn(false, &[], 2, false, &[]);
    assert_eq!(clamp(p.clone(), Some(1200), Transport::Http3), p);
}

#[test]
fn malformed_fragmented_authenticated_and_duplicate_options_are_not_rewritten() {
    for options in [
        &[2, 3, 5, 180][..],
        &[2, 4, 5, 180, 2, 4, 5, 180][..],
        &[2, 4, 5, 180, 19, 4, 0, 0][..],
        &[2, 4, 5, 180, 29, 4, 0, 0][..],
        &[2, 4, 5, 180, 42, 8, 0, 0][..],
    ] {
        let p = syn(false, options, 2, false, &[]);
        assert_eq!(clamp(p.clone(), Some(1200), Transport::Http3), p);
    }
    for (v6, value) in [(false, 0x2000u16), (false, 1), (true, 44), (true, 51)] {
        let mut p = syn(v6, &[2, 4, 5, 180], 2, false, &[]).to_vec();
        if v6 {
            p[6] = value as u8;
        } else {
            p[6..8].copy_from_slice(&value.to_be_bytes());
        }
        let p = Bytes::from(p);
        assert_eq!(clamp(p.clone(), Some(1200), Transport::Http3), p);
    }
    let p = syn(true, &[1; 40], 2, false, &[]);
    assert_eq!(clamp(p.clone(), Some(1200), Transport::Http3), p);
}

#[test]
fn invalid_tcp_checksum_is_not_repaired_by_mss_rewriting() {
    let mut p = syn(false, &[1, 2, 4, 5, 180, 0, 0, 0], 2, false, &[]).to_vec();
    p[36] ^= 1;
    let p = clamp(p.into(), Some(1200), Transport::Http3);
    assert!(!checksum_valid(&p));
}

proptest! {
    #[test]
    fn arbitrary_syn_options_preserve_checksum_and_payload(
        ipv6 in any::<bool>(), words in prop::collection::vec(any::<[u8; 4]>(), 0..11),
        payload in prop::collection::vec(any::<u8>(), 0..64), budget in 900u16..1281,
    ) {
        let options: Vec<u8> = words.into_iter().flatten().collect();
        let packet = syn(ipv6, &options, 2, false, &payload);
        let output = clamp(packet.clone(), Some(budget), Transport::Http3);
        prop_assert!(checksum_valid(&output));
        prop_assert!(output.ends_with(&payload));
        prop_assert!(output.len() <= packet.len() + 4);
        prop_assert_eq!(clamp(output.clone(), Some(budget), Transport::Http3), output);
    }

    #[test]
    fn arbitrary_or_truncated_packets_never_panic(
        bytes in prop::collection::vec(any::<u8>(), 0..2048), budget in 0u16..1281,
    ) {
        let output = clamp(Bytes::from(bytes.clone()), Some(budget), Transport::Http3);
        prop_assert!(output.len() <= bytes.len() + 4);
    }
}
