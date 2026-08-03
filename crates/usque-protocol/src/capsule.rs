//! RFC 9484 CONNECT-IP control capsules.
//!
//! Capsule payloads are deliberately bounded before allocation or parsing.
//! Unknown capsule types are retained so callers can ignore them while keeping
//! stream framing synchronized, as required by the Capsule Protocol.

use std::{
    collections::HashSet,
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
};

use bytes::{Buf, BufMut, Bytes, BytesMut};

use crate::{ProtocolError, decode_varint, encode_varint, varint_len};

pub const ADDRESS_ASSIGN_CAPSULE_TYPE: u64 = 0x01;
pub const ADDRESS_REQUEST_CAPSULE_TYPE: u64 = 0x02;
pub const ROUTE_ADVERTISEMENT_CAPSULE_TYPE: u64 = 0x03;

/// Defensive limit for one control capsule. The RFC has no application-level
/// upper bound, so Usque rejects unreasonably large peer-controlled payloads.
pub const MAX_CAPSULE_PAYLOAD: usize = 64 * 1024;
/// Defensive limit for decoded entries in one peer-controlled capsule.
pub const MAX_CAPSULE_ENTRIES: usize = 4_096;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IpPrefix {
    pub request_id: u64,
    pub address: IpAddr,
    pub prefix_len: u8,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AddressAssign {
    pub addresses: Vec<IpPrefix>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddressRequest {
    pub addresses: Vec<IpPrefix>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IpAddressRange {
    pub start: IpAddr,
    pub end: IpAddr,
    /// IANA IP protocol number. Zero means all protocols.
    pub protocol: u8,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RouteAdvertisement {
    pub ranges: Vec<IpAddressRange>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectIpCapsule {
    AddressAssign(AddressAssign),
    AddressRequest(AddressRequest),
    RouteAdvertisement(RouteAdvertisement),
    Unknown { capsule_type: u64, payload: Bytes },
}

/// Network information most recently advertised by the peer.
///
/// RFC 9484 defines both ADDRESS_ASSIGN and ROUTE_ADVERTISEMENT as complete
/// replacement lists. Keeping that rule in this shared type prevents a
/// transport from accidentally retaining withdrawn addresses or routes.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PeerNetworkState {
    /// Distinguishes "no ADDRESS_ASSIGN received" (out-of-band assignment is
    /// still authoritative) from an explicit empty replacement that withdraws
    /// every address.
    pub assignments_advertised: bool,
    pub assigned_addresses: Vec<IpPrefix>,
    /// Distinguishes "no ROUTE_ADVERTISEMENT received" (local policy applies)
    /// from an explicit empty replacement that withdraws every route.
    pub routes_advertised: bool,
    pub available_routes: Vec<IpAddressRange>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CapsuleEffect {
    AssignmentsReplaced,
    RoutesReplaced,
    AddressRequested(AddressRequest),
    UnknownIgnored(u64),
}

impl PeerNetworkState {
    pub fn apply(&mut self, capsule: &ConnectIpCapsule) -> CapsuleEffect {
        match capsule {
            ConnectIpCapsule::AddressAssign(assign) => {
                self.assignments_advertised = true;
                self.assigned_addresses.clone_from(&assign.addresses);
                CapsuleEffect::AssignmentsReplaced
            }
            ConnectIpCapsule::RouteAdvertisement(advertisement) => {
                self.routes_advertised = true;
                self.available_routes.clone_from(&advertisement.ranges);
                CapsuleEffect::RoutesReplaced
            }
            ConnectIpCapsule::AddressRequest(request) => {
                CapsuleEffect::AddressRequested(request.clone())
            }
            ConnectIpCapsule::Unknown { capsule_type, .. } => {
                CapsuleEffect::UnknownIgnored(*capsule_type)
            }
        }
    }
}

impl ConnectIpCapsule {
    /// Encodes one complete Capsule Protocol frame (type, length, payload).
    pub fn encode(&self) -> Result<Bytes, ProtocolError> {
        let (capsule_type, payload) = match self {
            Self::AddressAssign(assign) => (
                ADDRESS_ASSIGN_CAPSULE_TYPE,
                encode_prefixes(&assign.addresses, false)?,
            ),
            Self::AddressRequest(request) => {
                validate_address_request(&request.addresses)?;
                (
                    ADDRESS_REQUEST_CAPSULE_TYPE,
                    encode_prefixes(&request.addresses, true)?,
                )
            }
            Self::RouteAdvertisement(advertisement) => {
                validate_routes(&advertisement.ranges)?;
                (
                    ROUTE_ADVERTISEMENT_CAPSULE_TYPE,
                    encode_routes(&advertisement.ranges)?,
                )
            }
            Self::Unknown {
                capsule_type,
                payload,
            } => {
                ensure_payload_size(payload.len() as u64)?;
                (*capsule_type, payload.clone())
            }
        };

        ensure_payload_size(payload.len() as u64)?;
        let mut output = BytesMut::with_capacity(
            varint_len(capsule_type)? + varint_len(payload.len() as u64)? + payload.len(),
        );
        encode_varint(capsule_type, &mut output)?;
        encode_varint(payload.len() as u64, &mut output)?;
        output.extend_from_slice(&payload);
        Ok(output.freeze())
    }

    /// Decodes exactly one capsule and advances `input` only after the entire
    /// frame has been received and validated.
    pub fn decode(input: &mut Bytes) -> Result<Self, ProtocolError> {
        let initial_len = input.len();
        let mut cursor = input.clone();
        let capsule_type = decode_varint(&mut cursor)?;
        let payload_len = decode_varint(&mut cursor)?;
        ensure_payload_size(payload_len)?;

        let payload_len =
            usize::try_from(payload_len).map_err(|_| ProtocolError::CapsuleTooLarge(u64::MAX))?;
        if cursor.len() < payload_len {
            return Err(ProtocolError::TruncatedCapsule);
        }

        let payload = cursor.split_to(payload_len);
        let capsule = match capsule_type {
            ADDRESS_ASSIGN_CAPSULE_TYPE => Self::AddressAssign(AddressAssign {
                addresses: decode_prefixes(payload, false)?,
            }),
            ADDRESS_REQUEST_CAPSULE_TYPE => {
                let addresses = decode_prefixes(payload, true)?;
                validate_address_request(&addresses)?;
                Self::AddressRequest(AddressRequest { addresses })
            }
            ROUTE_ADVERTISEMENT_CAPSULE_TYPE => {
                let ranges = decode_routes(payload)?;
                validate_routes(&ranges)?;
                Self::RouteAdvertisement(RouteAdvertisement { ranges })
            }
            _ => Self::Unknown {
                capsule_type,
                payload,
            },
        };

        input.advance(initial_len - cursor.len());
        Ok(capsule)
    }

    /// Decodes one capsule from a streaming buffer.
    ///
    /// `Ok(None)` means that the frame header or declared payload is not fully
    /// buffered yet. Once the complete frame is present, malformed entries are
    /// returned as errors and are never confused with transport fragmentation.
    pub fn decode_if_complete(input: &mut Bytes) -> Result<Option<Self>, ProtocolError> {
        let mut header = input.clone();
        let _capsule_type = match decode_varint(&mut header) {
            Ok(value) => value,
            Err(ProtocolError::TruncatedVarint) => return Ok(None),
            Err(error) => return Err(error),
        };
        let payload_len = match decode_varint(&mut header) {
            Ok(value) => value,
            Err(ProtocolError::TruncatedVarint) => return Ok(None),
            Err(error) => return Err(error),
        };
        ensure_payload_size(payload_len)?;
        let payload_len =
            usize::try_from(payload_len).map_err(|_| ProtocolError::CapsuleTooLarge(u64::MAX))?;
        let header_len = input.len() - header.len();
        let frame_len = header_len
            .checked_add(payload_len)
            .ok_or(ProtocolError::CapsuleTooLarge(u64::MAX))?;
        if input.len() < frame_len {
            return Ok(None);
        }

        let mut frame = input.slice(..frame_len);
        let capsule = Self::decode(&mut frame)?;
        debug_assert!(frame.is_empty());
        input.advance(frame_len);
        Ok(Some(capsule))
    }
}

fn ensure_payload_size(payload_len: u64) -> Result<(), ProtocolError> {
    if payload_len > MAX_CAPSULE_PAYLOAD as u64 {
        return Err(ProtocolError::CapsuleTooLarge(payload_len));
    }
    Ok(())
}

fn encode_prefixes(
    prefixes: &[IpPrefix],
    validate_request_ids: bool,
) -> Result<Bytes, ProtocolError> {
    if validate_request_ids {
        validate_address_request(prefixes)?;
    }

    let mut payload = BytesMut::new();
    for (index, prefix) in prefixes.iter().enumerate() {
        ensure_entry_capacity(index)?;
        validate_prefix(prefix)?;
        encode_varint(prefix.request_id, &mut payload)?;
        match prefix.address {
            IpAddr::V4(address) => {
                payload.put_u8(4);
                payload.extend_from_slice(&address.octets());
            }
            IpAddr::V6(address) => {
                payload.put_u8(6);
                payload.extend_from_slice(&address.octets());
            }
        }
        payload.put_u8(prefix.prefix_len);
        ensure_payload_size(payload.len() as u64)?;
    }
    Ok(payload.freeze())
}

fn decode_prefixes(
    mut payload: Bytes,
    validate_request_ids: bool,
) -> Result<Vec<IpPrefix>, ProtocolError> {
    let mut prefixes = Vec::new();
    while payload.has_remaining() {
        ensure_entry_capacity(prefixes.len())?;
        let request_id = decode_varint(&mut payload)?;
        let ip_version = take_u8(&mut payload)?;
        let address = decode_address(&mut payload, ip_version)?;
        let prefix_len = take_u8(&mut payload)?;
        let prefix = IpPrefix {
            request_id,
            address,
            prefix_len,
        };
        validate_prefix(&prefix)?;
        prefixes.push(prefix);
    }

    if validate_request_ids {
        validate_address_request(&prefixes)?;
    }
    Ok(prefixes)
}

fn validate_address_request(prefixes: &[IpPrefix]) -> Result<(), ProtocolError> {
    if prefixes.is_empty() {
        return Err(ProtocolError::EmptyAddressRequest);
    }

    let mut request_ids = HashSet::with_capacity(prefixes.len());
    for prefix in prefixes {
        if prefix.request_id == 0 {
            return Err(ProtocolError::ZeroRequestId);
        }
        if !request_ids.insert(prefix.request_id) {
            return Err(ProtocolError::DuplicateRequestId(prefix.request_id));
        }
    }
    Ok(())
}

fn validate_prefix(prefix: &IpPrefix) -> Result<(), ProtocolError> {
    match prefix.address {
        IpAddr::V4(address) => {
            if prefix.prefix_len > 32 {
                return Err(ProtocolError::InvalidPrefixLength {
                    ip_version: 4,
                    prefix_len: prefix.prefix_len,
                });
            }
            let host_mask = if prefix.prefix_len == 32 {
                0
            } else {
                u32::MAX >> prefix.prefix_len
            };
            if u32::from(address) & host_mask != 0 {
                return Err(ProtocolError::NonCanonicalPrefix);
            }
        }
        IpAddr::V6(address) => {
            if prefix.prefix_len > 128 {
                return Err(ProtocolError::InvalidPrefixLength {
                    ip_version: 6,
                    prefix_len: prefix.prefix_len,
                });
            }
            let host_mask = if prefix.prefix_len == 128 {
                0
            } else {
                u128::MAX >> prefix.prefix_len
            };
            if u128::from(address) & host_mask != 0 {
                return Err(ProtocolError::NonCanonicalPrefix);
            }
        }
    }
    Ok(())
}

fn encode_routes(ranges: &[IpAddressRange]) -> Result<Bytes, ProtocolError> {
    let mut payload = BytesMut::new();
    for (index, range) in ranges.iter().enumerate() {
        ensure_entry_capacity(index)?;
        match (range.start, range.end) {
            (IpAddr::V4(start), IpAddr::V4(end)) => {
                payload.put_u8(4);
                payload.extend_from_slice(&start.octets());
                payload.extend_from_slice(&end.octets());
            }
            (IpAddr::V6(start), IpAddr::V6(end)) => {
                payload.put_u8(6);
                payload.extend_from_slice(&start.octets());
                payload.extend_from_slice(&end.octets());
            }
            _ => return Err(ProtocolError::InvalidRouteRange),
        }
        payload.put_u8(range.protocol);
        ensure_payload_size(payload.len() as u64)?;
    }
    Ok(payload.freeze())
}

fn decode_routes(mut payload: Bytes) -> Result<Vec<IpAddressRange>, ProtocolError> {
    let mut ranges = Vec::new();
    while payload.has_remaining() {
        ensure_entry_capacity(ranges.len())?;
        let ip_version = take_u8(&mut payload)?;
        let start = decode_address(&mut payload, ip_version)?;
        let end = decode_address(&mut payload, ip_version)?;
        let protocol = take_u8(&mut payload)?;
        ranges.push(IpAddressRange {
            start,
            end,
            protocol,
        });
    }
    Ok(ranges)
}

fn ensure_entry_capacity(current_len: usize) -> Result<(), ProtocolError> {
    if current_len >= MAX_CAPSULE_ENTRIES {
        return Err(ProtocolError::TooManyCapsuleEntries);
    }
    Ok(())
}

fn validate_routes(ranges: &[IpAddressRange]) -> Result<(), ProtocolError> {
    for range in ranges {
        if address_family(range.start) != address_family(range.end)
            || address_number(range.start) > address_number(range.end)
        {
            return Err(ProtocolError::InvalidRouteRange);
        }
    }

    for pair in ranges.windows(2) {
        let previous = &pair[0];
        let current = &pair[1];
        let previous_family = address_family(previous.start);
        let current_family = address_family(current.start);

        if previous_family > current_family
            || (previous_family == current_family && previous.protocol > current.protocol)
            || (previous_family == current_family
                && previous.protocol == current.protocol
                && address_number(previous.end) >= address_number(current.start))
        {
            return Err(ProtocolError::UnorderedOrOverlappingRoutes);
        }
    }

    for (index, left) in ranges.iter().enumerate() {
        for right in &ranges[index + 1..] {
            if address_family(left.start) != address_family(right.start)
                || left.protocol == right.protocol
                || (left.protocol != 0 && right.protocol != 0)
            {
                continue;
            }
            if ranges_overlap(left, right) {
                return Err(ProtocolError::OverlappingAllProtocolRoutes);
            }
        }
    }

    Ok(())
}

fn ranges_overlap(left: &IpAddressRange, right: &IpAddressRange) -> bool {
    address_number(left.start) <= address_number(right.end)
        && address_number(right.start) <= address_number(left.end)
}

fn address_family(address: IpAddr) -> u8 {
    match address {
        IpAddr::V4(_) => 4,
        IpAddr::V6(_) => 6,
    }
}

fn address_number(address: IpAddr) -> u128 {
    match address {
        IpAddr::V4(address) => u32::from(address) as u128,
        IpAddr::V6(address) => u128::from(address),
    }
}

fn decode_address(payload: &mut Bytes, ip_version: u8) -> Result<IpAddr, ProtocolError> {
    match ip_version {
        4 => {
            if payload.remaining() < 4 {
                return Err(ProtocolError::TruncatedCapsuleEntry);
            }
            let mut octets = [0_u8; 4];
            payload.copy_to_slice(&mut octets);
            Ok(IpAddr::V4(Ipv4Addr::from(octets)))
        }
        6 => {
            if payload.remaining() < 16 {
                return Err(ProtocolError::TruncatedCapsuleEntry);
            }
            let mut octets = [0_u8; 16];
            payload.copy_to_slice(&mut octets);
            Ok(IpAddr::V6(Ipv6Addr::from(octets)))
        }
        other => Err(ProtocolError::InvalidIpVersion(other)),
    }
}

fn take_u8(payload: &mut Bytes) -> Result<u8, ProtocolError> {
    if !payload.has_remaining() {
        return Err(ProtocolError::TruncatedCapsuleEntry);
    }
    Ok(payload.get_u8())
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

    use bytes::{Bytes, BytesMut};
    use serde::Deserialize;

    use super::*;
    use crate::encode_varint;

    #[test]
    fn address_assign_round_trips_ipv4_and_ipv6() {
        let capsule = ConnectIpCapsule::AddressAssign(AddressAssign {
            addresses: vec![
                IpPrefix {
                    request_id: 0,
                    address: IpAddr::V4(Ipv4Addr::new(172, 16, 0, 0)),
                    prefix_len: 24,
                },
                IpPrefix {
                    request_id: 7,
                    address: "fd00:1234::".parse().expect("IPv6"),
                    prefix_len: 64,
                },
            ],
        });

        let mut encoded = capsule.encode().expect("encode");
        assert_eq!(
            ConnectIpCapsule::decode(&mut encoded).expect("decode"),
            capsule
        );
        assert!(encoded.is_empty());
    }

    #[test]
    fn empty_address_assign_withdraws_all_assignments() {
        let capsule = ConnectIpCapsule::AddressAssign(AddressAssign::default());
        let encoded = capsule.encode().expect("encode");
        assert_eq!(&encoded[..], &[ADDRESS_ASSIGN_CAPSULE_TYPE as u8, 0]);
    }

    #[test]
    fn address_request_rejects_empty_zero_and_duplicate_ids() {
        let empty = ConnectIpCapsule::AddressRequest(AddressRequest {
            addresses: Vec::new(),
        });
        assert_eq!(empty.encode(), Err(ProtocolError::EmptyAddressRequest));

        let zero = ConnectIpCapsule::AddressRequest(AddressRequest {
            addresses: vec![v4_prefix(0, [192, 0, 2, 0], 24)],
        });
        assert_eq!(zero.encode(), Err(ProtocolError::ZeroRequestId));

        let duplicate = ConnectIpCapsule::AddressRequest(AddressRequest {
            addresses: vec![
                v4_prefix(4, [192, 0, 2, 0], 24),
                v4_prefix(4, [198, 51, 100, 0], 24),
            ],
        });
        assert_eq!(
            duplicate.encode(),
            Err(ProtocolError::DuplicateRequestId(4))
        );
    }

    #[test]
    fn prefix_must_be_canonical_and_fit_its_family() {
        let host_bits = ConnectIpCapsule::AddressAssign(AddressAssign {
            addresses: vec![v4_prefix(0, [192, 0, 2, 1], 24)],
        });
        assert_eq!(host_bits.encode(), Err(ProtocolError::NonCanonicalPrefix));

        let invalid_length = ConnectIpCapsule::AddressAssign(AddressAssign {
            addresses: vec![v4_prefix(0, [192, 0, 2, 0], 33)],
        });
        assert_eq!(
            invalid_length.encode(),
            Err(ProtocolError::InvalidPrefixLength {
                ip_version: 4,
                prefix_len: 33,
            })
        );
    }

    #[test]
    fn route_advertisement_round_trips_ordered_ranges() {
        let capsule = ConnectIpCapsule::RouteAdvertisement(RouteAdvertisement {
            ranges: vec![
                v4_range([10, 0, 0, 0], [10, 0, 0, 255], 0),
                v4_range([192, 0, 2, 0], [192, 0, 2, 255], 6),
                IpAddressRange {
                    start: "2001:db8::".parse().expect("start"),
                    end: "2001:db8::ffff".parse().expect("end"),
                    protocol: 17,
                },
            ],
        });

        let mut encoded = capsule.encode().expect("encode");
        assert_eq!(
            ConnectIpCapsule::decode(&mut encoded).expect("decode"),
            capsule
        );
    }

    #[test]
    fn routes_reject_bad_order_and_wildcard_overlap() {
        let bad_order = ConnectIpCapsule::RouteAdvertisement(RouteAdvertisement {
            ranges: vec![
                v4_range([192, 0, 2, 0], [192, 0, 2, 255], 6),
                v4_range([10, 0, 0, 0], [10, 0, 0, 255], 6),
            ],
        });
        assert_eq!(
            bad_order.encode(),
            Err(ProtocolError::UnorderedOrOverlappingRoutes)
        );

        let wildcard_overlap = ConnectIpCapsule::RouteAdvertisement(RouteAdvertisement {
            ranges: vec![
                v4_range([10, 0, 0, 0], [10, 0, 0, 255], 0),
                v4_range([10, 0, 0, 128], [10, 0, 1, 0], 6),
            ],
        });
        assert_eq!(
            wildcard_overlap.encode(),
            Err(ProtocolError::OverlappingAllProtocolRoutes)
        );
    }

    #[test]
    fn unknown_capsule_is_preserved() {
        let capsule = ConnectIpCapsule::Unknown {
            capsule_type: 42,
            payload: Bytes::from_static(b"future"),
        };
        let mut encoded = capsule.encode().expect("encode");
        assert_eq!(
            ConnectIpCapsule::decode(&mut encoded).expect("decode"),
            capsule
        );
    }

    #[test]
    fn decoder_consumes_one_frame_at_a_time() {
        let first = ConnectIpCapsule::AddressAssign(AddressAssign::default());
        let second = ConnectIpCapsule::RouteAdvertisement(RouteAdvertisement::default());
        let mut stream = BytesMut::new();
        stream.extend_from_slice(&first.encode().expect("first"));
        stream.extend_from_slice(&second.encode().expect("second"));
        let mut stream = stream.freeze();

        assert_eq!(ConnectIpCapsule::decode(&mut stream).expect("first"), first);
        assert_eq!(
            ConnectIpCapsule::decode(&mut stream).expect("second"),
            second
        );
        assert!(stream.is_empty());
    }

    #[test]
    fn incomplete_frame_does_not_consume_stream() {
        let complete = ConnectIpCapsule::Unknown {
            capsule_type: 42,
            payload: Bytes::from_static(b"future"),
        }
        .encode()
        .expect("encode");
        let mut truncated = complete.slice(..complete.len() - 1);
        let original = truncated.clone();

        assert_eq!(
            ConnectIpCapsule::decode(&mut truncated),
            Err(ProtocolError::TruncatedCapsule)
        );
        assert_eq!(truncated, original);
    }

    #[test]
    fn streaming_decoder_waits_for_fragmented_frame() {
        let complete = ConnectIpCapsule::AddressAssign(AddressAssign {
            addresses: vec![v4_prefix(0, [192, 0, 2, 0], 24)],
        })
        .encode()
        .expect("encode");

        for split in 0..complete.len() {
            let mut fragment = complete.slice(..split);
            let original = fragment.clone();
            assert_eq!(
                ConnectIpCapsule::decode_if_complete(&mut fragment).expect("partial decode"),
                None
            );
            assert_eq!(fragment, original);
        }

        let mut complete = complete;
        assert!(matches!(
            ConnectIpCapsule::decode_if_complete(&mut complete).expect("complete decode"),
            Some(ConnectIpCapsule::AddressAssign(_))
        ));
        assert!(complete.is_empty());
    }

    #[test]
    fn streaming_decoder_treats_complete_bad_entry_as_fatal() {
        let mut malformed = Bytes::from_static(&[ADDRESS_ASSIGN_CAPSULE_TYPE as u8, 3, 0, 4, 192]);
        assert_eq!(
            ConnectIpCapsule::decode_if_complete(&mut malformed),
            Err(ProtocolError::TruncatedCapsuleEntry)
        );
    }

    #[test]
    fn peer_state_replaces_withdrawn_values() {
        let mut state = PeerNetworkState::default();
        let first = ConnectIpCapsule::AddressAssign(AddressAssign {
            addresses: vec![v4_prefix(0, [192, 0, 2, 1], 32)],
        });
        assert_eq!(state.apply(&first), CapsuleEffect::AssignmentsReplaced);
        assert_eq!(state.assigned_addresses.len(), 1);

        assert_eq!(
            state.apply(&ConnectIpCapsule::AddressAssign(AddressAssign::default())),
            CapsuleEffect::AssignmentsReplaced
        );
        assert!(state.assignments_advertised);
        assert!(state.assigned_addresses.is_empty());

        state.apply(&ConnectIpCapsule::RouteAdvertisement(RouteAdvertisement {
            ranges: vec![v4_range([0, 0, 0, 0], [255, 255, 255, 255], 0)],
        }));
        assert_eq!(state.available_routes.len(), 1);
        state.apply(&ConnectIpCapsule::RouteAdvertisement(
            RouteAdvertisement::default(),
        ));
        assert!(state.routes_advertised);
        assert!(state.available_routes.is_empty());
    }

    #[test]
    fn oversized_declared_payload_is_rejected_before_waiting_for_bytes() {
        let mut frame = BytesMut::new();
        encode_varint(ADDRESS_ASSIGN_CAPSULE_TYPE, &mut frame).expect("type");
        encode_varint(MAX_CAPSULE_PAYLOAD as u64 + 1, &mut frame).expect("length");
        let mut frame = frame.freeze();

        assert_eq!(
            ConnectIpCapsule::decode(&mut frame),
            Err(ProtocolError::CapsuleTooLarge(
                MAX_CAPSULE_PAYLOAD as u64 + 1
            ))
        );
    }

    #[test]
    fn malformed_entry_does_not_consume_stream() {
        let mut malformed = Bytes::from_static(&[ADDRESS_ASSIGN_CAPSULE_TYPE as u8, 3, 0, 4, 192]);
        let original = malformed.clone();

        assert_eq!(
            ConnectIpCapsule::decode(&mut malformed),
            Err(ProtocolError::TruncatedCapsuleEntry)
        );
        assert_eq!(malformed, original);
    }

    fn v4_prefix(request_id: u64, address: [u8; 4], prefix_len: u8) -> IpPrefix {
        IpPrefix {
            request_id,
            address: IpAddr::V4(Ipv4Addr::from(address)),
            prefix_len,
        }
    }

    fn v4_range(start: [u8; 4], end: [u8; 4], protocol: u8) -> IpAddressRange {
        IpAddressRange {
            start: IpAddr::V4(Ipv4Addr::from(start)),
            end: IpAddr::V4(Ipv4Addr::from(end)),
            protocol,
        }
    }

    #[test]
    fn ipv6_zero_prefix_is_canonical_only_at_unspecified_address() {
        let valid = IpPrefix {
            request_id: 0,
            address: IpAddr::V6(Ipv6Addr::UNSPECIFIED),
            prefix_len: 0,
        };
        assert!(validate_prefix(&valid).is_ok());

        let invalid = IpPrefix {
            request_id: 0,
            address: IpAddr::V6(Ipv6Addr::LOCALHOST),
            prefix_len: 0,
        };
        assert_eq!(
            validate_prefix(&invalid),
            Err(ProtocolError::NonCanonicalPrefix)
        );
    }

    #[derive(Deserialize)]
    struct OracleCapsuleDocument {
        schema_version: u32,
        fixtures: Vec<OracleCapsuleFixture>,
    }

    #[derive(Deserialize)]
    struct OracleCapsuleFixture {
        name: String,
        hex: String,
        expected_error: Option<String>,
    }

    #[test]
    fn sanitized_go_oracle_capsules_stay_wire_compatible() {
        let document: OracleCapsuleDocument =
            serde_json::from_str(include_str!("../../../oracle/fixtures/capsules.json"))
                .expect("parse sanitized oracle capsule fixtures");
        assert_eq!(document.schema_version, 1);
        assert_eq!(document.fixtures.len(), 3);

        for fixture in document.fixtures {
            let bytes = decode_fixture_hex(&fixture.hex);
            let mut input = Bytes::from(bytes.clone());
            match fixture.expected_error.as_deref() {
                Some("truncated-capsule-entry") => {
                    assert_eq!(
                        ConnectIpCapsule::decode(&mut input),
                        Err(ProtocolError::TruncatedCapsuleEntry),
                        "{}",
                        fixture.name
                    );
                    assert_eq!(&input[..], &bytes, "{}", fixture.name);
                }
                Some(other) => panic!("unknown oracle error contract {other}"),
                None => {
                    let decoded =
                        ConnectIpCapsule::decode(&mut input).expect("decode oracle capsule");
                    assert!(input.is_empty(), "{}", fixture.name);
                    assert_eq!(
                        &decoded.encode().expect("re-encode oracle capsule")[..],
                        &bytes,
                        "{}",
                        fixture.name
                    );
                }
            }
        }
    }

    fn decode_fixture_hex(value: &str) -> Vec<u8> {
        assert_eq!(value.len() % 2, 0, "fixture hex must be byte aligned");
        value
            .as_bytes()
            .chunks_exact(2)
            .map(|pair| {
                let text = std::str::from_utf8(pair).expect("fixture hex is ASCII");
                u8::from_str_radix(text, 16).expect("fixture hex byte")
            })
            .collect()
    }
}
