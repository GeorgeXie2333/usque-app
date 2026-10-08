//! Rejection at the application packet boundary, before NAT/flow allocation.
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use bytes::Bytes;

/// Read the bounded base IP header, independently of payload validity. Fragments, options and extension
/// headers still carry a destination even when endpoint NAT cannot parse them.
pub(crate) fn destination(packet: &[u8]) -> Option<IpAddr> {
    match packet.first()? >> 4 {
        4 if packet.len() >= 20 => {
            Some(Ipv4Addr::new(packet[16], packet[17], packet[18], packet[19]).into())
        }
        6 if packet.len() >= 40 => {
            Some(Ipv6Addr::from(<[u8; 16]>::try_from(&packet[24..40]).ok()?).into())
        }
        _ => None,
    }
}

pub(crate) struct RoutingRejector {
    window: Mutex<(Instant, u16)>,
}

impl Default for RoutingRejector {
    fn default() -> Self {
        Self {
            window: Mutex::new((Instant::now(), 0)),
        }
    }
}

impl RoutingRejector {
    pub(crate) fn reply(&self, packet: &[u8]) -> Option<Bytes> {
        let valid_length = match packet.first()? >> 4 {
            4 if packet.len() >= 20 => {
                usize::from(u16::from_be_bytes([packet[2], packet[3]])) == packet.len()
            }
            6 if packet.len() >= 40 => {
                usize::from(u16::from_be_bytes([packet[4], packet[5]])) + 40 == packet.len()
            }
            _ => false,
        };
        if !valid_length {
            return None;
        }
        let meta = crate::direct_gateway::NatPacket::parse(packet)?;
        if !crate::l4::tun_wire::reply_allowed(&meta)
            || !crate::l4::tun_wire::valid_transport(packet, &meta)
        {
            return None;
        }
        let mut window = self.window.lock().unwrap_or_else(|e| e.into_inner());
        if window.0.elapsed() >= Duration::from_secs(1) {
            *window = (Instant::now(), 0);
        }
        if window.1 >= 32 {
            return None;
        }
        let response = if meta.protocol == 6 {
            crate::l4::tun_wire::TcpReset::from_packet(packet, &meta)?.response()
        } else {
            crate::l4::tun_wire::udp_unreachable(packet, &meta)
        };
        if response.is_some() {
            window.1 += 1;
        }
        response
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    proptest::proptest! {
        #[test]
        fn arbitrary_packets_never_panic_or_create_oversized_replies(packet in proptest::collection::vec(proptest::prelude::any::<u8>(), 0..1600)) {
            let _ = destination(&packet);
            if let Some(reply) = RoutingRejector::default().reply(&packet) { proptest::prop_assert!(reply.len() <= 1280); }
        }
    }

    #[test]
    fn fragments_options_extensions_and_non_tcp_have_a_destination() {
        let mut v4 = vec![0; 28];
        v4[0] = 0x46;
        v4[2..4].copy_from_slice(&28_u16.to_be_bytes());
        v4[6] = 0x20;
        v4[9] = 1;
        v4[16..20].copy_from_slice(&[192, 0, 2, 1]);
        assert_eq!(destination(&v4), Some("192.0.2.1".parse().unwrap()));
        let mut v6 = vec![0; 48];
        v6[0] = 0x60;
        v6[4..6].copy_from_slice(&8_u16.to_be_bytes());
        v6[6] = 44;
        v6[39] = 1;
        assert_eq!(destination(&v6), Some("::1".parse().unwrap()));
        // A malformed length must not hide an otherwise visible blocked IP.
        v4[2..4].fill(0);
        assert_eq!(destination(&v4), Some("192.0.2.1".parse().unwrap()));
        assert!(destination(&v4[..19]).is_none());
        assert!(RoutingRejector::default().reply(&v4).is_none());
    }
}
