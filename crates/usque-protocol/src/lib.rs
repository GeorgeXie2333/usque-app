//! Wire-level primitives shared by the HTTP/3 and HTTP/2 CONNECT-IP transports.
//!
//! RFC 9484 carries each IP packet in a QUIC-style datagram prefixed by a
//! variable-length context ID. Cloudflare's consumer endpoint currently uses
//! context ID zero, but the implementation accepts the full QUIC varint range
//! so the parser stays protocol-correct.

use bytes::{Buf, BufMut, Bytes, BytesMut};
use thiserror::Error;

mod capsule;

pub use capsule::{
    ADDRESS_ASSIGN_CAPSULE_TYPE, ADDRESS_REQUEST_CAPSULE_TYPE, AddressAssign, AddressRequest,
    CapsuleEffect, ConnectIpCapsule, IpAddressRange, IpPrefix, MAX_CAPSULE_ENTRIES,
    MAX_CAPSULE_PAYLOAD, PeerNetworkState, ROUTE_ADVERTISEMENT_CAPSULE_TYPE, RouteAdvertisement,
};

/// The context ID used for CONNECT-IP payloads negotiated without additional
/// context registrations.
pub const DEFAULT_CONTEXT_ID: u64 = 0;

/// Largest integer representable by a QUIC variable-length integer.
pub const MAX_VARINT: u64 = (1_u64 << 62) - 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IpDatagram {
    pub context_id: u64,
    pub packet: Bytes,
}

impl IpDatagram {
    pub fn new(packet: impl Into<Bytes>) -> Self {
        Self {
            context_id: DEFAULT_CONTEXT_ID,
            packet: packet.into(),
        }
    }

    pub fn encode(&self) -> Result<Bytes, ProtocolError> {
        let mut output = BytesMut::with_capacity(varint_len(self.context_id)? + self.packet.len());
        encode_varint(self.context_id, &mut output)?;
        output.extend_from_slice(&self.packet);
        Ok(output.freeze())
    }

    pub fn decode(mut input: Bytes) -> Result<Self, ProtocolError> {
        let context_id = decode_varint(&mut input)?;
        if input.is_empty() {
            return Err(ProtocolError::EmptyIpPacket);
        }
        Ok(Self {
            context_id,
            packet: input,
        })
    }
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ProtocolError {
    #[error("QUIC variable-length integer exceeds 62 bits")]
    VarintTooLarge,
    #[error("truncated QUIC variable-length integer")]
    TruncatedVarint,
    #[error("CONNECT-IP datagram does not contain an IP packet")]
    EmptyIpPacket,
    #[error("CONNECT-IP capsule payload is truncated")]
    TruncatedCapsule,
    #[error("CONNECT-IP capsule entry is truncated")]
    TruncatedCapsuleEntry,
    #[error("CONNECT-IP capsule payload is {0} bytes, exceeding the configured limit")]
    CapsuleTooLarge(u64),
    #[error("invalid CONNECT-IP address family value {0}")]
    InvalidIpVersion(u8),
    #[error("prefix length {prefix_len} is invalid for IPv{ip_version}")]
    InvalidPrefixLength { ip_version: u8, prefix_len: u8 },
    #[error("CONNECT-IP prefix has non-zero host bits")]
    NonCanonicalPrefix,
    #[error("ADDRESS_REQUEST must contain at least one address")]
    EmptyAddressRequest,
    #[error("ADDRESS_REQUEST uses the reserved request ID zero")]
    ZeroRequestId,
    #[error("ADDRESS_REQUEST repeats request ID {0}")]
    DuplicateRequestId(u64),
    #[error("route range start is greater than its end")]
    InvalidRouteRange,
    #[error("route advertisements are not in RFC 9484 order or overlap")]
    UnorderedOrOverlappingRoutes,
    #[error("an all-protocol route overlaps a protocol-specific route")]
    OverlappingAllProtocolRoutes,
    #[error("CONNECT-IP capsule contains too many entries")]
    TooManyCapsuleEntries,
}

pub fn varint_len(value: u64) -> Result<usize, ProtocolError> {
    match value {
        0..=63 => Ok(1),
        64..=16_383 => Ok(2),
        16_384..=1_073_741_823 => Ok(4),
        1_073_741_824..=MAX_VARINT => Ok(8),
        _ => Err(ProtocolError::VarintTooLarge),
    }
}

pub fn encode_varint(value: u64, output: &mut BytesMut) -> Result<(), ProtocolError> {
    match varint_len(value)? {
        1 => output.put_u8(value as u8),
        2 => output.put_u16((value as u16) | 0x4000),
        4 => output.put_u32((value as u32) | 0x8000_0000),
        8 => output.put_u64(value | 0xC000_0000_0000_0000),
        _ => unreachable!("varint_len returns only valid QUIC widths"),
    }
    Ok(())
}

pub fn decode_varint(input: &mut Bytes) -> Result<u64, ProtocolError> {
    let first = *input.first().ok_or(ProtocolError::TruncatedVarint)?;
    let width = 1_usize << (first >> 6);
    if input.len() < width {
        return Err(ProtocolError::TruncatedVarint);
    }

    let value = match width {
        1 => input.get_u8() as u64 & 0x3f,
        2 => input.get_u16() as u64 & 0x3fff,
        4 => input.get_u32() as u64 & 0x3fff_ffff,
        8 => input.get_u64() & MAX_VARINT,
        _ => unreachable!("the two-bit QUIC prefix only encodes four widths"),
    };
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn default_context_is_one_zero_byte() {
        let frame = IpDatagram::new(Bytes::from_static(&[0x45, 0, 0, 20]));
        let encoded = frame.encode().expect("encode");
        assert_eq!(&encoded[..], &[0, 0x45, 0, 0, 20]);
        assert_eq!(IpDatagram::decode(encoded).expect("decode"), frame);
    }

    #[test]
    fn rejects_empty_packet() {
        let input = Bytes::from_static(&[0]);
        assert_eq!(IpDatagram::decode(input), Err(ProtocolError::EmptyIpPacket));
    }

    proptest! {
        #[test]
        fn varints_round_trip(value in 0_u64..=MAX_VARINT) {
            let mut encoded = BytesMut::new();
            encode_varint(value, &mut encoded).expect("encode");
            let mut encoded = encoded.freeze();
            let decoded = decode_varint(&mut encoded).expect("decode");
            prop_assert_eq!(decoded, value);
            prop_assert!(encoded.is_empty());
        }
    }
}
