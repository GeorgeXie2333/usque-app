//! TCP SYN sizing for UDP exits nested inside CONNECT-IP H3.
//! Interface MTU and non-TCP datagrams retain their existing semantics.
use bytes::Bytes;
use usque_core::{Transport, chain_exit::ChainProtocol};

pub(crate) const WARP_MTU: u16 = 1280;

/// Bound the complete inner IP packet, including peer-side WireGuard padding.
/// The OpenVPN reserve covers the supported CBC/SHA512 maximum (opcode/peer ID,
/// packet ID, HMAC, IV, block padding and compression framing) as well as AEAD.
/// It is deliberately independent of rekey/cipher negotiation and mssfix 0.
pub(crate) fn packet_budget(protocol: ChainProtocol, endpoint_ipv6: bool) -> Option<u16> {
    let udp_payload = WARP_MTU - if endpoint_ipv6 { 48 } else { 28 };
    match protocol {
        ChainProtocol::Wireguard => Some((udp_payload - 32) / 16 * 16),
        ChainProtocol::OpenvpnUdp => Some(udp_payload - 128),
        ChainProtocol::OpenvpnTcp => None,
    }
}

pub(crate) fn clamp(packet: Bytes, budget: Option<u16>, transport: Transport) -> Bytes {
    let Some(budget) = budget.filter(|_| transport == Transport::Http3) else {
        return packet;
    };
    let Some((tcp, ipv6)) = tcp_offset(&packet) else {
        return packet;
    };
    if packet.len() < tcp + 20 || packet[tcp + 13] & 0x07 != 0x02 {
        return packet;
    }
    let header_len = usize::from(packet[tcp + 12] >> 4) * 4;
    if header_len < 20 || tcp + header_len > packet.len() {
        return packet;
    }
    let Some(max_mss) = usize::from(budget)
        .checked_sub(tcp + 20)
        .filter(|mss| *mss >= 64)
    else {
        return packet;
    };
    let max_mss = max_mss as u16;
    let mut header = packet[tcp..tcp + header_len].to_vec();
    let mut option = 20;
    let mut mss = None;
    let mut eol = header_len;
    while option < header_len {
        match header[option] {
            0 => {
                eol = option;
                break;
            }
            1 => {
                option += 1;
                continue;
            }
            // Rewriting a TCP MD5/AO authenticated SYN would invalidate it.
            19 | 29 => return packet,
            _ => {}
        }
        if option + 2 > header_len {
            return packet;
        }
        let length = usize::from(header[option + 1]);
        if length < 2 || option + length > header_len {
            return packet;
        }
        if header[option] == 2 {
            if length != 4 || mss.is_some() {
                return packet;
            }
            mss = Some(option + 2);
        }
        option += length;
    }
    if let Some(offset) = mss {
        let old = read16(&header, offset);
        if old <= max_mss {
            return packet;
        }
        header[offset..offset + 2].copy_from_slice(&max_mss.to_be_bytes());
    } else {
        // No MSS means 536 for IPv4 or 1220 for IPv6. Only add an option when
        // that implicit value would exceed the nested path's budget.
        if (if ipv6 { 1220 } else { 536 }) <= max_mss {
            return packet;
        }
        if header_len - eol >= 4 {
            header[eol..eol + 4].copy_from_slice(&[2, 4, (max_mss >> 8) as u8, max_mss as u8]);
            header[eol + 4..].fill(0);
        } else {
            if header_len > 56 || packet.len() + 4 > usize::from(budget) {
                return packet;
            }
            header[eol..].fill(1);
            header.extend_from_slice(&[2, 4, (max_mss >> 8) as u8, max_mss as u8]);
            header[12] = (header[12] & 15) | ((header.len() / 4) as u8) << 4;
        }
    }

    // Incremental checksum update preserves the original checksum's validity,
    // supports odd-aligned MSS options, and never scans application payload.
    let mut sum = u32::from(!read16(&packet, tcp + 16));
    for offset in (0..header_len).step_by(2).filter(|o| *o != 16) {
        sum += u32::from(!read16(&packet, tcp + offset));
    }
    for offset in (0..header.len()).step_by(2).filter(|o| *o != 16) {
        sum += u32::from(read16(&header, offset));
    }
    let added = header.len() - header_len;
    if added != 0 {
        sum += u32::from(!((packet.len() - tcp) as u16));
        sum += (packet.len() - tcp + added) as u32;
    }
    header[16..18].copy_from_slice(&finish_checksum(sum).to_be_bytes());
    let mut output = Vec::with_capacity(packet.len() + added);
    output.extend_from_slice(&packet[..tcp]);
    output.extend_from_slice(&header);
    output.extend_from_slice(&packet[tcp + header_len..]);
    if added != 0 {
        let length_offset = if ipv6 { 4 } else { 2 };
        let old = read16(&output, length_offset);
        let new = old + added as u16;
        output[length_offset..length_offset + 2].copy_from_slice(&new.to_be_bytes());
        if !ipv6 {
            let sum = u32::from(!read16(&output, 10)) + u32::from(!old) + u32::from(new);
            output[10..12].copy_from_slice(&finish_checksum(sum).to_be_bytes());
        }
    }
    Bytes::from(output)
}

fn tcp_offset(packet: &[u8]) -> Option<(usize, bool)> {
    match packet.first()? >> 4 {
        4 if packet.len() >= 20 => {
            let length = usize::from(packet[0] & 15) * 4;
            (length >= 20
                && length <= packet.len()
                && packet[9] == 6
                && usize::from(read16(packet, 2)) == packet.len()
                && read16(packet, 6) & 0x3fff == 0)
                .then_some((length, false))
        }
        6 if packet.len() >= 40 && usize::from(read16(packet, 4)) + 40 == packet.len() => {
            let mut next = packet[6];
            let mut offset = 40;
            for _ in 0..8 {
                if next == 6 {
                    return Some((offset, true));
                }
                // Hop-by-hop, routing, destination options. Fragment/AH/ESP
                // packets are never rewritten, including non-initial fragments.
                if !matches!(next, 0 | 43 | 60) || offset + 2 > packet.len() {
                    return None;
                }
                let length = (usize::from(packet[offset + 1]) + 1) * 8;
                next = packet[offset];
                offset += length;
                if offset > packet.len() {
                    return None;
                }
            }
            None
        }
        _ => None,
    }
}

fn read16(bytes: &[u8], offset: usize) -> u16 {
    u16::from_be_bytes([bytes[offset], bytes[offset + 1]])
}
fn finish_checksum(mut sum: u32) -> u16 {
    while sum > 65535 {
        sum = (sum & 65535) + (sum >> 16);
    }
    !(sum as u16)
}

#[cfg(test)]
mod tests;
