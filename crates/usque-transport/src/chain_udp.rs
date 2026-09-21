//! Narrow IPv6 UDP fragmentation for the private WARP transport. smoltcp's
//! IPv6 raw socket preserves the fragment header but does not reassemble it.
use bytes::Bytes;
use std::collections::BTreeMap;
use std::net::{Ipv6Addr, SocketAddrV6};
use std::time::{Duration, Instant};

pub(crate) fn fragments(
    local: SocketAddrV6,
    remote: SocketAddrV6,
    body: &[u8],
    id: u32,
) -> Option<Vec<Vec<u8>>> {
    let length = body.len().checked_add(8)?;
    if length > 16 * 1024 - 48 {
        return None;
    }
    let mut udp = vec![0; length];
    udp[0..2].copy_from_slice(&local.port().to_be_bytes());
    udp[2..4].copy_from_slice(&remote.port().to_be_bytes());
    udp[4..6].copy_from_slice(&(length as u16).to_be_bytes());
    udp[8..].copy_from_slice(body);
    let checksum = checksum(*local.ip(), *remote.ip(), &udp);
    udp[6..8].copy_from_slice(&if checksum == 0 { u16::MAX } else { checksum }.to_be_bytes());
    let mut output = Vec::new();
    // 1280-byte IPv6 packets, with 40-byte base + 8-byte fragment headers.
    for (index, chunk) in udp.chunks(1232).enumerate() {
        let offset = index * 1232;
        let more = offset + chunk.len() < length;
        let mut packet = vec![0; 48 + chunk.len()];
        packet[0] = 0x60;
        packet[4..6].copy_from_slice(&((8 + chunk.len()) as u16).to_be_bytes());
        packet[6] = 44;
        packet[7] = 64;
        packet[8..24].copy_from_slice(&local.ip().octets());
        packet[24..40].copy_from_slice(&remote.ip().octets());
        packet[40] = 17;
        packet[42..44].copy_from_slice(&((offset as u16) | u16::from(more)).to_be_bytes());
        packet[44..48].copy_from_slice(&id.to_be_bytes());
        packet[48..].copy_from_slice(chunk);
        output.push(packet);
    }
    Some(output)
}
fn checksum(source: Ipv6Addr, destination: Ipv6Addr, bytes: &[u8]) -> u16 {
    let mut sum = 17u32 + bytes.len() as u32;
    for array in [&source.octets()[..], &destination.octets()[..], bytes] {
        for word in array.chunks(2) {
            sum += u32::from(u16::from_be_bytes([word[0], *word.get(1).unwrap_or(&0)]));
        }
    }
    while sum > 65535 {
        sum = (sum & 65535) + (sum >> 16);
    }
    !(sum as u16)
}
struct Assembly {
    created: Instant,
    chunks: BTreeMap<usize, Bytes>,
    end: Option<usize>,
    rejected: bool,
}
#[derive(Default)]
pub(crate) struct Reassembler {
    pending: BTreeMap<u32, Assembly>,
}
impl Reassembler {
    pub(crate) fn receive(
        &mut self,
        packet: &[u8],
        local: SocketAddrV6,
        remote: SocketAddrV6,
    ) -> Option<Bytes> {
        let now = Instant::now();
        self.pending
            .retain(|_, entry| now.duration_since(entry.created) < Duration::from_secs(10));
        if packet.len() < 49
            || packet[0] >> 4 != 6
            || packet[6] != 44
            || packet[40] != 17
            || packet[41] != 0
            || usize::from(u16::from_be_bytes([packet[4], packet[5]])) + 40 != packet.len()
            || packet[8..24] != remote.ip().octets()
            || packet[24..40] != local.ip().octets()
        {
            return None;
        }
        let flags = u16::from_be_bytes([packet[42], packet[43]]);
        let offset = usize::from(flags & !7);
        let more = flags & 1 != 0;
        let payload = &packet[48..];
        let id = u32::from_be_bytes(packet[44..48].try_into().ok()?);
        if flags & 6 != 0
            || more && !payload.len().is_multiple_of(8)
            || offset + payload.len() > 65535
        {
            self.pending.remove(&id);
            return None;
        }
        if !self.pending.contains_key(&id) && self.pending.len() >= 16 {
            return None;
        }
        let entry = self.pending.entry(id).or_insert_with(|| Assembly {
            created: now,
            chunks: BTreeMap::new(),
            end: None,
            rejected: false,
        });
        if entry.rejected {
            return None;
        }
        if entry.chunks.len() >= 64
            || entry.chunks.iter().any(|(start, bytes)| {
                *start < offset + payload.len() && offset < start + bytes.len()
            })
            || entry.end.is_some_and(|end| {
                offset + payload.len() > end || !more && offset + payload.len() != end
            })
        {
            entry.chunks.clear();
            entry.rejected = true;
            return None;
        }
        if !more {
            entry.end = Some(offset + payload.len());
        }
        entry.chunks.insert(offset, Bytes::copy_from_slice(payload));
        let end = entry.end?;
        let mut cursor = 0;
        for (offset, bytes) in &entry.chunks {
            if *offset != cursor {
                return None;
            }
            cursor += bytes.len();
        }
        if cursor != end {
            return None;
        }
        let entry = self.pending.remove(&id)?;
        let mut data = Vec::with_capacity(end);
        for bytes in entry.chunks.values() {
            data.extend_from_slice(bytes);
        }
        if data.len() < 8
            || u16::from_be_bytes([data[0], data[1]]) != remote.port()
            || u16::from_be_bytes([data[2], data[3]]) != local.port()
            || usize::from(u16::from_be_bytes([data[4], data[5]])) != data.len()
            || data[6..8] == [0, 0]
            || checksum(*remote.ip(), *local.ip(), &data) != 0
        {
            return None;
        }
        Some(Bytes::from(data).slice(8..))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mtu_1280_reorders_fragments_and_validates_endpoint_ports_and_checksum() {
        let a: SocketAddrV6 = "[2001:db8::1]:40000".parse().unwrap();
        let b: SocketAddrV6 = "[2001:db8::2]:51820".parse().unwrap();
        for size in [1, 1232, 1312, 9032] {
            let body = vec![0x37; size];
            let packets = fragments(a, b, &body, size as u32).unwrap();
            assert!(packets.iter().all(|p| p.len() <= 1280));
            let mut reassembly = Reassembler::default();
            let mut result = None;
            for packet in packets.iter().rev() {
                result = reassembly.receive(packet, b, a).or(result);
            }
            assert_eq!(result.unwrap(), body);
            let mut packets = packets;
            let last = packets.last_mut().unwrap();
            let i = last.len() - 1;
            last[i] ^= 1;
            let mut reassembly = Reassembler::default();
            assert!(
                packets
                    .iter()
                    .all(|p| reassembly.receive(p, b, a).is_none())
            );
        }
    }
    #[test]
    fn overlaps_and_foreign_endpoints_cannot_complete_a_datagram() {
        let a = "[2001:db8::1]:40000".parse().unwrap();
        let b = "[2001:db8::2]:51820".parse().unwrap();
        let packets = fragments(a, b, &[42; 2000], 1).unwrap();
        let mut r = Reassembler::default();
        assert!(r.receive(&packets[0], b, a).is_none());
        assert!(r.receive(&packets[0], b, a).is_none());
        assert!(r.receive(&packets[1], b, a).is_none());
        assert!(r.receive(&packets[0], b, a).is_none());
        let other = "[2001:db8::3]:40000".parse().unwrap();
        assert!(r.receive(&packets[0], b, other).is_none());
    }
}
