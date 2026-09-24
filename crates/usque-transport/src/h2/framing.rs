//! HTTP/2 DATA ownership is separate from the shared CONNECT-IP control plane.
use super::{ConnectIpCapsule, MAX_CAPSULE_PAYLOAD, TransportError, decode_varint};
use bytes::{Buf, Bytes, BytesMut};

#[derive(Default)]
pub(super) struct H2CapsuleFramer {
    pub(super) data: Bytes,
    pub(super) partial: BytesMut,
    copied: u64,
}

impl H2CapsuleFramer {
    pub(super) fn feed(&mut self, data: Bytes) {
        assert!(self.data.is_empty(), "consume the current DATA tail first");
        self.data = data;
    }

    pub(super) fn take_copied_bytes(&mut self) -> u64 {
        std::mem::take(&mut self.copied)
    }

    pub(super) fn next(&mut self) -> Result<Option<ConnectIpCapsule>, TransportError> {
        loop {
            if self.partial.is_empty() {
                if self.data.is_empty() {
                    return Ok(None);
                }
                let (required, framed) = required_bytes(&self.data)?;
                if framed && self.data.len() >= required {
                    return decode_frame(self.data.split_to(required)).map(Some);
                }
                // The entire DATA is part of this incomplete capsule. Complete
                // later capsules in the next DATA will keep that DATA's storage.
                self.copy_prefix(self.data.len());
            }
            let (required, framed) = required_bytes(&self.partial)?;
            if framed && self.partial.len() >= required {
                return decode_frame(self.partial.split_to(required).freeze()).map(Some);
            }
            if self.data.is_empty() {
                return Ok(None);
            }
            if framed {
                let (capsule_type, type_len) =
                    decode_varint(&self.partial)?.expect("complete capsule header");
                let (_, length_len) =
                    decode_varint(&self.partial[type_len..])?.expect("complete capsule length");
                // Peers can write the envelope and the IP packet as separate
                // DATA frames. Keep the entire IP packet in the latter DATA's
                // storage, even when the envelope itself was fragmented. Only
                // DATAGRAM is opaque here; control payloads retain their shared
                // validation and ordering through decode_frame.
                if capsule_type == super::DATAGRAM_CAPSULE_TYPE
                    && self.partial.len() == type_len + length_len
                    && self.data.len() >= required - self.partial.len()
                {
                    let payload = self.data.split_to(required - self.partial.len());
                    // Retain the small allocation for the next split envelope.
                    self.partial.clear();
                    return Ok(Some(ConnectIpCapsule::Unknown {
                        capsule_type,
                        payload,
                    }));
                }
            }
            self.copy_prefix((required - self.partial.len()).min(self.data.len()));
        }
    }

    fn copy_prefix(&mut self, length: usize) {
        debug_assert!(self.partial.len() + length <= MAX_CAPSULE_PAYLOAD + 16);
        self.partial.extend_from_slice(&self.data[..length]);
        self.data.advance(length);
        self.copied = self.copied.saturating_add(length as u64);
    }
}

// Return the next framing milestone, without speculative copying. Even an
// eight-byte type and length require at most sixteen header bytes.
fn required_bytes(bytes: &[u8]) -> Result<(usize, bool), TransportError> {
    let Some(&first) = bytes.first() else {
        return Ok((1, false));
    };
    let type_len = 1usize << (first >> 6);
    if bytes.len() <= type_len {
        return Ok((type_len + 1, false));
    }
    let length_len = 1usize << (bytes[type_len] >> 6);
    let header = type_len + length_len;
    if bytes.len() < header {
        return Ok((header, false));
    }
    let (payload, _) = decode_varint(&bytes[type_len..])?.expect("complete length varint");
    if payload > MAX_CAPSULE_PAYLOAD as u64 {
        return Err(TransportError::CapsuleTooLarge);
    }
    Ok((header + payload as usize, true))
}

fn decode_frame(mut frame: Bytes) -> Result<ConnectIpCapsule, TransportError> {
    let capsule = ConnectIpCapsule::decode(&mut frame)?;
    debug_assert!(frame.is_empty());
    Ok(capsule)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn varint(bytes: &mut Vec<u8>, value: u64, width: usize) {
        assert!(value < (1_u64 << (width * 8 - 2)));
        let start = bytes.len();
        bytes.extend_from_slice(&value.to_be_bytes()[8 - width..]);
        bytes[start] |= (width.ilog2() as u8) << 6;
    }

    fn unknown(payload: &[u8], type_width: usize, length_width: usize) -> Bytes {
        let mut bytes = Vec::new();
        varint(&mut bytes, 0x21, type_width);
        varint(&mut bytes, payload.len() as u64, length_width);
        bytes.extend_from_slice(payload);
        bytes.into()
    }

    fn payload(capsule: ConnectIpCapsule) -> Bytes {
        let ConnectIpCapsule::Unknown {
            capsule_type: 0x21,
            payload,
        } = capsule
        else {
            panic!("unexpected capsule");
        };
        payload
    }

    #[test]
    fn split_datagram_header_preserves_contiguous_payload_storage() {
        for type_width in [1, 2, 4, 8] {
            for length_width in [1, 2, 4, 8] {
                for payload_len in [20, 1280] {
                    if length_width == 1 && payload_len >= 64 {
                        continue;
                    }
                    let expected = vec![0x45; payload_len];
                    let mut header = Vec::new();
                    varint(&mut header, super::super::DATAGRAM_CAPSULE_TYPE, type_width);
                    varint(&mut header, payload_len as u64, length_width);
                    // Include every header split, including a DATA containing just
                    // the envelope followed by a DATA containing the entire IP packet.
                    for split in 1..=header.len() {
                        let mut framer = H2CapsuleFramer::default();
                        framer.feed(Bytes::copy_from_slice(&header[..split]));
                        assert!(framer.next().unwrap().is_none());
                        let mut tail = header[split..].to_vec();
                        tail.extend_from_slice(&expected);
                        tail.extend_from_slice(&unknown(&[8; 13], 1, 1));
                        let data = Bytes::from(tail);
                        let pointer = data.as_ptr().wrapping_add(header.len() - split);
                        framer.feed(data.clone());
                        let ConnectIpCapsule::Unknown {
                            capsule_type: 0,
                            payload,
                        } = framer.next().unwrap().unwrap()
                        else {
                            panic!("expected DATAGRAM capsule");
                        };
                        assert_eq!(payload.as_ref(), expected.as_slice());
                        assert_eq!(payload.as_ptr(), pointer);
                        assert_eq!(framer.take_copied_bytes(), header.len() as u64);
                        assert_eq!(self::payload(framer.next().unwrap().unwrap()), &[8; 13][..]);
                        assert!(framer.next().unwrap().is_none());
                    }
                }
            }
        }
    }

    #[test]
    fn every_varint_split_and_width_preserves_the_following_data_slice() {
        for type_width in [1, 2, 4, 8] {
            for length_width in [1, 2, 4, 8] {
                let first = unknown(&[9; 17], type_width, length_width);
                let second = unknown(&[8; 13], 1, 1);
                for split in 1..first.len() {
                    let mut framer = H2CapsuleFramer::default();
                    framer.feed(first.slice(..split));
                    assert!(framer.next().unwrap().is_none());
                    let mut joined = first.slice(split..).to_vec();
                    joined.extend_from_slice(&second);
                    let data = Bytes::from(joined);
                    let pointer = data.as_ptr().wrapping_add(first.len() - split + 2);
                    framer.feed(data.clone());
                    assert_eq!(payload(framer.next().unwrap().unwrap()), &[9; 17][..]);
                    let next = payload(framer.next().unwrap().unwrap());
                    assert_eq!(next.as_ptr(), pointer);
                    assert_eq!(next, &[8; 13][..]);
                    assert_eq!(framer.take_copied_bytes(), first.len() as u64);
                    assert!(framer.next().unwrap().is_none());
                }
            }
        }
    }

    proptest! {
        #[test]
        fn random_chunking_preserves_datagrams(
            contents in prop::collection::vec(any::<u8>(), 0..4096),
            sizes in prop::collection::vec(1usize..2048, 1..30),
        ) {
            let mut encoded = Vec::new();
            varint(&mut encoded, super::super::DATAGRAM_CAPSULE_TYPE, 8);
            varint(&mut encoded, contents.len() as u64, 8);
            encoded.extend_from_slice(&contents);
            let bytes = Bytes::from(encoded);
            let mut framer = H2CapsuleFramer::default();
            let mut received = Vec::new();
            let mut offset = 0;
            for size in sizes.iter().cycle() {
                if offset == bytes.len() { break; }
                let end = (offset + size).min(bytes.len());
                framer.feed(bytes.slice(offset..end));
                while let Some(capsule) = framer.next().unwrap() {
                    let ConnectIpCapsule::Unknown { capsule_type: 0, payload } = capsule
                    else { panic!("unexpected capsule"); };
                    received.push(payload);
                }
                prop_assert!(framer.partial.len() <= MAX_CAPSULE_PAYLOAD + 16);
                offset = end;
            }
            prop_assert_eq!(received, vec![Bytes::from(contents)]);
        }

        #[test]
        fn random_chunking_preserves_unknown_capsules(
            contents in prop::collection::vec(any::<u8>(), 0..2048),
            sizes in prop::collection::vec(1usize..128, 1..30),
        ) {
            let bytes = unknown(&contents, 8, 8);
            let mut framer = H2CapsuleFramer::default();
            let mut received = Vec::new();
            let mut offset = 0;
            for size in sizes.iter().cycle() {
                if offset == bytes.len() { break; }
                let end = (offset + size).min(bytes.len());
                framer.feed(bytes.slice(offset..end));
                while let Some(capsule) = framer.next().unwrap() { received.push(payload(capsule)); }
                prop_assert!(framer.partial.len() <= MAX_CAPSULE_PAYLOAD + 16);
                offset = end;
            }
            prop_assert_eq!(received, vec![Bytes::from(contents)]);
        }

        #[test]
        fn arbitrary_bytes_and_chunks_are_bounded(
            bytes in prop::collection::vec(any::<u8>(), 0..4096), chunk in 1usize..128,
        ) {
            let mut framer = H2CapsuleFramer::default();
            'input: for data in bytes.chunks(chunk) {
                framer.feed(Bytes::copy_from_slice(data));
                loop {
                    match framer.next() {
                        Ok(Some(_)) => {},
                        Ok(None) => break,
                        Err(_) => break 'input,
                    }
                }
                prop_assert!(framer.partial.len() <= MAX_CAPSULE_PAYLOAD + 16);
            }
        }
    }
}
