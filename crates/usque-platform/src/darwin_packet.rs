//! Darwin utun uses a four-byte, network-order address family before each IP
//! packet. AF_INET6 is 30 on Darwin, not Linux's 10.

use std::io;

pub const HEADER_BYTES: usize = 4;
pub const MAX_IP_PACKET: usize = 65_575;

pub fn decode(frame: &[u8]) -> io::Result<&[u8]> {
    let header = frame.get(..HEADER_BYTES).ok_or_else(invalid)?;
    let packet = &frame[HEADER_BYTES..];
    if header != family(packet)?.to_be_bytes() {
        return Err(invalid());
    }
    Ok(packet)
}

pub fn encode(packet: &[u8]) -> io::Result<Vec<u8>> {
    let family = family(packet)?;
    let mut frame = Vec::with_capacity(HEADER_BYTES + packet.len());
    frame.extend_from_slice(&family.to_be_bytes());
    frame.extend_from_slice(packet);
    Ok(frame)
}

fn family(packet: &[u8]) -> io::Result<u32> {
    if packet.len() > MAX_IP_PACKET {
        return Err(invalid());
    }
    match packet.first().map(|byte| byte >> 4) {
        Some(4) if packet.len() >= 20 => {
            let header_length = usize::from(packet[0] & 0x0f) * 4;
            let length = u16::from_be_bytes([packet[2], packet[3]]) as usize;
            if header_length < 20 || header_length > packet.len() || length != packet.len() {
                return Err(invalid());
            }
            Ok(2)
        }
        Some(6) if packet.len() >= 40 => {
            let length = u16::from_be_bytes([packet[4], packet[5]]) as usize + 40;
            if length != packet.len() {
                return Err(invalid());
            }
            Ok(30)
        }
        _ => Err(invalid()),
    }
}

fn invalid() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "invalid Darwin utun IP packet")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn packet(v6: bool) -> Vec<u8> {
        let mut value = vec![0; if v6 { 48 } else { 28 }];
        value[0] = if v6 { 0x60 } else { 0x45 };
        if v6 {
            value[4..6].copy_from_slice(&8_u16.to_be_bytes());
        } else {
            value[2..4].copy_from_slice(&28_u16.to_be_bytes());
        }
        value
    }

    #[test]
    fn round_trips_both_families_in_darwin_network_byte_order() {
        for (v6, family) in [(false, 2_u32), (true, 30_u32)] {
            let packet = packet(v6);
            let frame = encode(&packet).unwrap();
            assert_eq!(&frame[..4], &family.to_be_bytes());
            assert_eq!(decode(&frame).unwrap(), packet);
        }
    }

    #[test]
    fn rejects_family_version_mismatch_and_linux_ipv6_family() {
        for family in [2_u32, 10, 0, u32::MAX] {
            let mut frame = family.to_be_bytes().to_vec();
            frame.extend(packet(true));
            assert_eq!(
                decode(&frame).unwrap_err().kind(),
                io::ErrorKind::InvalidData
            );
        }
    }

    #[test]
    fn rejects_truncated_oversize_and_trailing_bytes() {
        for v6 in [false, true] {
            let packet = packet(v6);
            for cut in 0..packet.len() {
                assert!(encode(&packet[..cut]).is_err());
            }
            let mut padded = packet;
            padded.push(0);
            assert!(encode(&padded).is_err());
        }
        assert!(encode(&vec![0x60; MAX_IP_PACKET + 1]).is_err());
        assert!(decode(&[0, 0, 0]).is_err());
    }

    #[test]
    fn validates_ipv4_options_and_accepts_zero_length_ipv6_payload() {
        let mut v4 = packet(false);
        v4[0] = 0x47;
        assert!(encode(&v4).is_ok());
        v4[0] = 0x48;
        assert!(encode(&v4).is_err());
        v4[0] = 0x44;
        assert!(encode(&v4).is_err());
        let mut v6 = vec![0; 40];
        v6[0] = 0x60;
        assert!(encode(&v6).is_ok());
    }
}
