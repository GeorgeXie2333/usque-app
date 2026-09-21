//! Property-based malformed-packet coverage without exposing the private codec.
#[path = "../src/chain_udp.rs"]
mod chain_udp;

use proptest::prelude::*;

proptest! {
    #[test]
    fn arbitrary_fragment_sequences_remain_bounded_and_do_not_panic(
        packets in prop::collection::vec(prop::collection::vec(any::<u8>(), 0..1600), 0..48),
    ) {
        let local = "[2001:db8::1]:40000".parse().unwrap();
        let remote = "[2001:db8::2]:51820".parse().unwrap();
        let mut receiver = chain_udp::Reassembler::default();
        for mut packet in packets {
            // Exercise both raw arbitrary input and input that reaches fragment parsing.
            if let Some(body) = receiver.receive(&packet, local, remote) {
                prop_assert!(body.len() <= 65527);
            }
            if packet.len() >= 48 {
                packet[0] = 0x60;
                let length = (packet.len() - 40) as u16;
                packet[4..6].copy_from_slice(&length.to_be_bytes());
                packet[6] = 44;
                packet[8..24].copy_from_slice(&remote.ip().octets());
                packet[24..40].copy_from_slice(&local.ip().octets());
                packet[40] = 17;
                packet[41] = 0;
                if let Some(body) = receiver.receive(&packet, local, remote) {
                    prop_assert!(body.len() <= 65527);
                }
            }
        }
    }

    #[test]
    fn truncated_or_mutated_fragments_never_emit_a_corrupted_body(
        body in prop::collection::vec(any::<u8>(), 1..4096),
        fragment_index in any::<usize>(),
        byte_index in any::<usize>(),
        bit in 0u8..8,
        truncate in any::<bool>(),
    ) {
        let local = "[2001:db8::1]:40000".parse().unwrap();
        let remote = "[2001:db8::2]:51820".parse().unwrap();
        let mut packets = chain_udp::fragments(remote, local, &body, 42).unwrap();
        let count = packets.len();
        let packet = &mut packets[fragment_index % count];
        let index = byte_index % packet.len();
        if truncate {
            packet.truncate(index);
        } else {
            packet[index] ^= 1 << bit;
        }
        let mut receiver = chain_udp::Reassembler::default();
        for packet in packets.iter().rev() {
            if let Some(received) = receiver.receive(packet, local, remote) {
                prop_assert_eq!(&received[..], &body[..]);
            }
        }
    }
}
