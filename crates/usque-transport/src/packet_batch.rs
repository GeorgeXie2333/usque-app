use std::collections::VecDeque;

use bytes::Bytes;

pub(crate) const MAX_PACKET_BATCH_PACKETS: usize = 64;
pub(crate) const MAX_PACKET_BATCH_BYTES: usize = 256 * 1024;
pub(crate) const PACKET_BATCH_CHANNEL_CAPACITY: usize = 1_024 / MAX_PACKET_BATCH_PACKETS;

#[derive(Debug)]
pub(crate) struct PacketBatch {
    packets: VecDeque<Bytes>,
    bytes: usize,
}

impl Default for PacketBatch {
    fn default() -> Self {
        Self::new()
    }
}

impl PacketBatch {
    pub(crate) fn new() -> Self {
        Self {
            packets: VecDeque::with_capacity(MAX_PACKET_BATCH_PACKETS),
            bytes: 0,
        }
    }

    pub(crate) fn single(packet: Bytes) -> Self {
        let mut batch = Self::new();
        batch
            .push_back(packet)
            .expect("one valid IP packet fits the batch byte bound");
        batch
    }

    pub(crate) fn push_back(&mut self, packet: Bytes) -> Result<(), Bytes> {
        if !self.can_accept(packet.len()) {
            return Err(packet);
        }
        self.bytes += packet.len();
        self.packets.push_back(packet);
        Ok(())
    }

    pub(crate) fn pop_front(&mut self) -> Option<Bytes> {
        let packet = self.packets.pop_front()?;
        self.bytes = self.bytes.saturating_sub(packet.len());
        Some(packet)
    }

    pub(crate) fn front(&self) -> Option<&Bytes> {
        self.packets.front()
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = &Bytes> {
        self.packets.iter()
    }

    pub(crate) fn can_accept(&self, packet_bytes: usize) -> bool {
        self.packets.len() < MAX_PACKET_BATCH_PACKETS
            && self.bytes.saturating_add(packet_bytes) <= MAX_PACKET_BATCH_BYTES
    }

    pub(crate) fn len(&self) -> usize {
        self.packets.len()
    }

    pub(crate) fn bytes(&self) -> usize {
        self.bytes
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.packets.is_empty()
    }
}

#[derive(Debug, Default)]
pub(crate) struct PacketBatchResult {
    pub(crate) accepted_bytes: usize,
    pub(crate) oversized: Vec<(Bytes, usize)>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn batch_preserves_order_and_packet_limit() {
        let mut batch = PacketBatch::new();
        for value in 0..MAX_PACKET_BATCH_PACKETS {
            batch
                .push_back(Bytes::from(vec![value as u8]))
                .expect("packet fits");
        }
        assert_eq!(batch.len(), MAX_PACKET_BATCH_PACKETS);
        assert!(batch.push_back(Bytes::from_static(b"overflow")).is_err());
        for value in 0..MAX_PACKET_BATCH_PACKETS {
            assert_eq!(batch.pop_front().unwrap().as_ref(), &[value as u8]);
        }
        assert!(batch.is_empty());
        assert_eq!(batch.bytes(), 0);
    }

    #[test]
    fn batch_enforces_byte_limit_without_consuming_overflow() {
        let mut batch = PacketBatch::new();
        let first = Bytes::from(vec![0; MAX_PACKET_BATCH_BYTES]);
        batch.push_back(first).expect("exact limit fits");
        let overflow = Bytes::from_static(b"overflow");
        let returned = batch.push_back(overflow.clone()).unwrap_err();
        assert_eq!(returned, overflow);
        assert_eq!(batch.bytes(), MAX_PACKET_BATCH_BYTES);
    }
}
