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
