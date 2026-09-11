use crate::direct_gateway::NatPacket;
use bytes::Bytes;
use std::net::{IpAddr, SocketAddr};

#[derive(Clone)]
pub(crate) struct TcpReset {
    source: SocketAddr,
    destination: SocketAddr,
    sequence: u32,
    acknowledgment: u32,
    flags: u8,
    payload_length: usize,
}

impl TcpReset {
    pub(crate) fn from_packet(packet: &[u8], meta: &NatPacket) -> Option<Self> {
        if meta.protocol != 6 {
            return None;
        }
        let at = meta.transport_offset;
        let header = usize::from(*packet.get(at + 12)? >> 4) * 4;
        if header < 20 || at + header > packet.len() {
            return None;
        }
        Some(Self {
            source: SocketAddr::new(meta.source, meta.source_port),
            destination: SocketAddr::new(meta.destination, meta.destination_port),
            sequence: u32::from_be_bytes(packet.get(at + 4..at + 8)?.try_into().ok()?),
            acknowledgment: u32::from_be_bytes(packet.get(at + 8..at + 12)?.try_into().ok()?),
            flags: packet[at + 13],
            payload_length: packet.len() - at - header,
        })
    }
    pub(crate) fn syn(&self) -> bool {
        self.flags & 0x17 == 0x02
    }
    pub(crate) fn response(&self) -> Option<Bytes> {
        if self.flags & 4 != 0 {
            return None;
        }
        let mut tcp = vec![0u8; 20];
        tcp[0..2].copy_from_slice(&self.destination.port().to_be_bytes());
        tcp[2..4].copy_from_slice(&self.source.port().to_be_bytes());
        tcp[12] = 5 << 4;
        if self.flags & 16 != 0 {
            tcp[4..8].copy_from_slice(&self.acknowledgment.to_be_bytes());
            tcp[13] = 4;
        } else {
            let n = (self.payload_length as u32)
                .wrapping_add(u32::from(self.flags & 2 != 0))
                .wrapping_add(u32::from(self.flags & 1 != 0));
            tcp[8..12].copy_from_slice(&self.sequence.wrapping_add(n).to_be_bytes());
            tcp[13] = 0x14;
        }
        Some(ip_packet(self.destination.ip(), self.source.ip(), 6, tcp))
    }
}

pub(crate) fn valid_transport(packet: &[u8], meta: &NatPacket) -> bool {
    if meta.version == 4 && checksum(&packet[..20]) != 0 {
        return false;
    }
    let body = &packet[meta.transport_offset..];
    if meta.protocol == 17 {
        if body.len() < 8 || usize::from(u16::from_be_bytes([body[4], body[5]])) != body.len() {
            return false;
        }
        if meta.version == 4 && body[6..8] == [0, 0] {
            return true;
        }
    }
    pseudo_checksum(meta.source, meta.destination, meta.protocol, body) == 0
}

pub(crate) fn udp_response(meta: &NatPacket, body: &[u8]) -> Bytes {
    let mut udp = vec![0u8; 8];
    udp[0..2].copy_from_slice(&meta.destination_port.to_be_bytes());
    udp[2..4].copy_from_slice(&meta.source_port.to_be_bytes());
    udp[4..6].copy_from_slice(&((8 + body.len()) as u16).to_be_bytes());
    udp.extend_from_slice(body);
    ip_packet(meta.destination, meta.source, 17, udp)
}

pub(crate) fn udp_unreachable(packet: &[u8], meta: &NatPacket) -> Option<Bytes> {
    if !reply_allowed(meta) {
        return None;
    }
    let mut icmp = vec![0u8; 8];
    let protocol = if meta.version == 4 {
        icmp[0] = 3;
        icmp[1] = 3;
        1
    } else {
        icmp[0] = 1;
        icmp[1] = 4;
        58
    };
    let quote = if meta.version == 4 { 28 } else { 1232 };
    icmp.extend_from_slice(&packet[..packet.len().min(quote)]);
    Some(ip_packet(meta.destination, meta.source, protocol, icmp))
}

pub(crate) fn reply_allowed(meta: &NatPacket) -> bool {
    fn safe(ip: IpAddr) -> bool {
        !ip.is_unspecified()
            && !ip.is_multicast()
            && !matches!(ip, IpAddr::V4(v4) if v4.is_broadcast())
    }
    safe(meta.source) && safe(meta.destination)
}

pub(super) fn ip_packet(
    source: IpAddr,
    destination: IpAddr,
    protocol: u8,
    mut body: Vec<u8>,
) -> Bytes {
    let checksum_offset = match protocol {
        6 => 16,
        17 => 6,
        _ => 2,
    };
    let value = if protocol == 1 {
        checksum(&body)
    } else {
        pseudo_checksum(source, destination, protocol, &body)
    };
    let value = if protocol == 17 && value == 0 {
        u16::MAX
    } else {
        value
    };
    body[checksum_offset..checksum_offset + 2].copy_from_slice(&value.to_be_bytes());
    let mut packet;
    match (source, destination) {
        (IpAddr::V4(source), IpAddr::V4(destination)) => {
            packet = vec![0u8; 20];
            packet[0] = 0x45;
            packet[8] = 64;
            packet[9] = protocol;
            packet[2..4].copy_from_slice(&((20 + body.len()) as u16).to_be_bytes());
            packet[6] = 0x40;
            packet[12..16].copy_from_slice(&source.octets());
            packet[16..20].copy_from_slice(&destination.octets());
            let value = checksum(&packet);
            packet[10..12].copy_from_slice(&value.to_be_bytes());
        }
        (IpAddr::V6(source), IpAddr::V6(destination)) => {
            packet = vec![0u8; 40];
            packet[0] = 0x60;
            packet[6] = protocol;
            packet[7] = 64;
            packet[4..6].copy_from_slice(&(body.len() as u16).to_be_bytes());
            packet[8..24].copy_from_slice(&source.octets());
            packet[24..40].copy_from_slice(&destination.octets());
        }
        _ => return Bytes::new(),
    }
    packet.extend_from_slice(&body);
    Bytes::from(packet)
}

fn sum(bytes: &[u8]) -> u32 {
    bytes
        .chunks(2)
        .map(|b| u32::from(b[0]) * 256 + u32::from(*b.get(1).unwrap_or(&0)))
        .sum()
}
fn finish(mut value: u32) -> u16 {
    while value > 0xffff {
        value = (value & 0xffff) + (value >> 16);
    }
    !(value as u16)
}
fn checksum(bytes: &[u8]) -> u16 {
    finish(sum(bytes))
}
fn pseudo_checksum(source: IpAddr, destination: IpAddr, protocol: u8, body: &[u8]) -> u16 {
    let addresses = match (source, destination) {
        (IpAddr::V4(a), IpAddr::V4(b)) => sum(&a.octets()) + sum(&b.octets()),
        (IpAddr::V6(a), IpAddr::V6(b)) => sum(&a.octets()) + sum(&b.octets()),
        _ => return 1,
    };
    finish(addresses + u32::from(protocol) + body.len() as u32 + sum(body))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dns_response_checksums_and_udp_rejection_are_valid_for_both_families() {
        for (a, b) in [("10.0.0.1", "1.1.1.1"), ("fd00::2", "2606:4700:4700::1111")] {
            let mut udp = vec![0u8; 8];
            udp[0..2].copy_from_slice(&10000u16.to_be_bytes());
            udp[2..4].copy_from_slice(&53u16.to_be_bytes());
            udp[4..6].copy_from_slice(&8u16.to_be_bytes());
            let request = ip_packet(a.parse().unwrap(), b.parse().unwrap(), 17, udp);
            let meta = NatPacket::parse(&request).unwrap();
            assert!(valid_transport(&request, &meta));
            let response = udp_response(&meta, b"answer");
            let response_meta = NatPacket::parse(&response).unwrap();
            assert!(valid_transport(&response, &response_meta));
            assert_eq!(response_meta.source, meta.destination);
            let rejected = udp_unreachable(&request, &meta).unwrap();
            assert!(rejected.len() <= 1280);
        }
    }
}
