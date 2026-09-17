use super::*;
use crate::packet_mux::{PacketMuxTable, PacketOrigin};
use proptest::prelude::*;
use std::sync::Arc;

fn packet(v6: bool, protocol: u8, source_port: u16, destination_port: u16, reply: bool) -> Vec<u8> {
    let offset = if v6 { 40 } else { 20 };
    let size = if protocol == 6 { 20 } else { 8 };
    let mut bytes = vec![0; offset + size];
    if v6 {
        bytes[0] = 0x60;
        bytes[4..6].copy_from_slice(&(size as u16).to_be_bytes());
        bytes[6] = protocol;
        bytes[7] = 64;
        bytes[8..24].copy_from_slice(&"fd00::1".parse::<std::net::Ipv6Addr>().unwrap().octets());
        bytes[24..40].copy_from_slice(
            &"2001:db8::2"
                .parse::<std::net::Ipv6Addr>()
                .unwrap()
                .octets(),
        );
        if reply {
            for n in 0..16 {
                bytes.swap(8 + n, 24 + n);
            }
        }
    } else {
        bytes[0] = 0x45;
        bytes[2..4].copy_from_slice(&((offset + size) as u16).to_be_bytes());
        bytes[8] = 64;
        bytes[9] = protocol;
        bytes[12..16].copy_from_slice(&[10, 0, 0, 1]);
        bytes[16..20].copy_from_slice(&[192, 0, 2, 2]);
        if reply {
            for n in 0..4 {
                bytes.swap(12 + n, 16 + n);
            }
        }
    }
    bytes[offset..offset + 2].copy_from_slice(&source_port.to_be_bytes());
    bytes[offset + 2..offset + 4].copy_from_slice(&destination_port.to_be_bytes());
    if protocol == 17 {
        bytes[offset + 4..offset + 6].copy_from_slice(&8u16.to_be_bytes());
    }
    bytes
}

fn fragment(v6: bool, mut bytes: Vec<u8>, later: bool, id: u16) -> Vec<u8> {
    if v6 {
        bytes[6] = 44;
        let mut header = [0u8; 8];
        header[0] = 17;
        header[2..4].copy_from_slice(&(if later { 8u16 } else { 1u16 }).to_be_bytes());
        header[4..8].copy_from_slice(&u32::from(id).to_be_bytes());
        bytes.splice(40..40, header);
        bytes[4..6].copy_from_slice(&16u16.to_be_bytes());
    } else {
        bytes[4..6].copy_from_slice(&id.to_be_bytes());
        bytes[6..8].copy_from_slice(&(if later { 1u16 } else { 0x2000u16 }).to_be_bytes());
    }
    bytes
}

#[test]
fn established_tunnel_flows_follow_live_policy_in_both_directions() {
    for v6 in [false, true] {
        let policy = Arc::new(ApplicationTrafficPolicy::default());
        let mut mux = PacketMuxTable::with_traffic_policy(policy.clone());
        for blocked in [false, true, false, true] {
            policy.set_disable_quic(blocked);
            for (protocol, source, remote) in [
                (17, 50000, 443),
                (17, 50001, 8443),
                (17, 443, 53),
                (6, 50002, 443),
            ] {
                let allowed = !(blocked && protocol == 17 && remote == 443);
                assert_eq!(
                    mux.route_outgoing(
                        PacketOrigin::Tunnel,
                        &mut packet(v6, protocol, source, remote, false)
                    ),
                    allowed
                );
                assert_eq!(
                    mux.route_incoming(&mut packet(v6, protocol, remote, source, true)),
                    allowed.then_some(PacketOrigin::Tunnel)
                );
            }
            // Internal stack traffic has its own frontend policy; applying a
            // generic filter here would also affect engine-owned packets.
            assert!(
                mux.route_outgoing(PacketOrigin::Proxy, &mut packet(v6, 17, 51000, 443, false))
            );
            assert_eq!(
                mux.route_incoming(&mut packet(v6, 17, 443, 51000, true)),
                Some(PacketOrigin::Proxy)
            );
        }
    }
}

#[test]
fn hot_filter_covers_fragment_associations_and_unknown_later_fragments() {
    for v6 in [false, true] {
        let policy = Arc::new(ApplicationTrafficPolicy::default());
        let mut mux = PacketMuxTable::with_traffic_policy(policy.clone());
        let outgoing = packet(v6, 17, 50000, 443, false);
        let incoming = packet(v6, 17, 443, 50000, true);
        assert!(!mux.route_outgoing(
            PacketOrigin::Tunnel,
            &mut fragment(v6, outgoing.clone(), true, 7)
        ));
        assert!(mux.route_outgoing(
            PacketOrigin::Tunnel,
            &mut fragment(v6, outgoing.clone(), false, 7)
        ));
        assert_eq!(
            mux.route_incoming(&mut fragment(v6, incoming.clone(), false, 9)),
            Some(PacketOrigin::Tunnel)
        );
        for blocked in [true, false, true] {
            policy.set_disable_quic(blocked);
            for later in [true, false] {
                assert_eq!(
                    mux.route_outgoing(
                        PacketOrigin::Tunnel,
                        &mut fragment(v6, outgoing.clone(), later, 7)
                    ),
                    !blocked
                );
                assert_eq!(
                    mux.route_incoming(&mut fragment(v6, incoming.clone(), later, 9)),
                    (!blocked).then_some(PacketOrigin::Tunnel)
                );
            }
        }
        // Reuse a previously allowed fragment ID for a newly blocked flow.
        policy.set_disable_quic(false);
        assert!(mux.route_outgoing(
            PacketOrigin::Tunnel,
            &mut fragment(v6, packet(v6, 17, 50000, 53, false), false, 11)
        ));
        policy.set_disable_quic(true);
        assert!(!mux.route_outgoing(
            PacketOrigin::Tunnel,
            &mut fragment(v6, outgoing.clone(), false, 11)
        ));
        assert!(!mux.route_outgoing(PacketOrigin::Tunnel, &mut fragment(v6, outgoing, true, 11)));

        // A blocked TUN first fragment replaces an older proxy-origin
        // classification for the same incoming IP fragment identifier.
        assert!(mux.route_outgoing(PacketOrigin::Proxy, &mut packet(v6, 17, 51000, 53, false)));
        assert_eq!(
            mux.route_incoming(&mut fragment(
                v6,
                packet(v6, 17, 53, 51000, true),
                false,
                13
            )),
            Some(PacketOrigin::Proxy)
        );
        assert_eq!(
            mux.route_incoming(&mut fragment(v6, incoming.clone(), false, 13)),
            None
        );
        assert_eq!(
            mux.route_incoming(&mut fragment(v6, incoming, true, 13)),
            None
        );
    }
}

#[test]
fn ip_options_and_extension_headers_cannot_hide_udp_443() {
    for v6 in [false, true] {
        let policy = Arc::new(ApplicationTrafficPolicy::new(true));
        let mut mux = PacketMuxTable::with_traffic_policy(policy.clone());
        let mut bytes = packet(v6, 17, 50000, 443, false);
        if v6 {
            bytes[6] = 0;
            bytes.splice(40..40, [17, 0, 0, 0, 0, 0, 0, 0]);
            bytes[4..6].copy_from_slice(&16u16.to_be_bytes());
        } else {
            bytes[0] = 0x46;
            bytes.splice(20..20, [1, 1, 1, 1]);
            bytes[2..4].copy_from_slice(&32u16.to_be_bytes());
        }
        assert!(!mux.route_outgoing(PacketOrigin::Tunnel, &mut bytes.clone()));
        policy.set_disable_quic(false);
        assert!(mux.route_outgoing(PacketOrigin::Tunnel, &mut bytes));
    }
}

proptest! {
    #[test]
    fn malformed_packets_do_not_panic_or_change_rejected_bytes(bytes in prop::collection::vec(any::<u8>(), 0..512)) {
        let mut mux = PacketMuxTable::with_traffic_policy(Arc::new(ApplicationTrafficPolicy::new(true)));
        let mut outgoing = bytes.clone();
        let inspection = mux.inspect_outgoing(PacketOrigin::Tunnel, &outgoing);
        if !mux.route_inspected_outgoing(&mut outgoing, inspection) { prop_assert_eq!(outgoing, bytes.clone()); }
        let _ = mux.route_incoming(&mut bytes.clone());
    }
}
