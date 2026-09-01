use std::io;

use bytes::{Bytes, BytesMut};

const ANDROID_TUN_SLAB_PACKETS: usize = 8;

pub(crate) struct TunReadSlab {
    storage: BytesMut,
    slot_size: usize,
    #[cfg(test)]
    fresh_allocations: u64,
}

impl TunReadSlab {
    pub(crate) fn new() -> Self {
        Self {
            storage: BytesMut::new(),
            slot_size: 0,
            #[cfg(test)]
            fresh_allocations: 0,
        }
    }

    pub(crate) fn prepare(&mut self, slot_size: usize) -> io::Result<bool> {
        let slab_bytes = slot_size
            .checked_mul(ANDROID_TUN_SLAB_PACKETS)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "TUN MTU is too large"))?;
        if slot_size == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "TUN MTU must be non-zero",
            ));
        }
        let allocated = self.slot_size != slot_size || self.storage.len() < slot_size;
        if allocated {
            let mut storage = BytesMut::with_capacity(slab_bytes);
            storage.resize(slab_bytes, 0);
            self.storage = storage;
            self.slot_size = slot_size;
            #[cfg(test)]
            {
                self.fresh_allocations = self.fresh_allocations.saturating_add(1);
            }
        }
        Ok(allocated)
    }

    pub(crate) fn read_buffer(&mut self) -> &mut [u8] {
        &mut self.storage[..self.slot_size]
    }

    pub(crate) fn take_packet(&mut self, length: usize) -> io::Result<Bytes> {
        if length == 0 || length > self.slot_size || self.storage.len() < self.slot_size {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Android TUN returned an invalid packet length",
            ));
        }
        let slot = self.storage.split_to(self.slot_size).freeze();
        Ok(slot.slice(..length))
    }

    #[cfg(test)]
    fn fresh_allocations(&self) -> u64 {
        self.fresh_allocations
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eight_packets_share_each_owned_slab_without_mutable_aliasing() {
        let mut slab = TunReadSlab::new();
        let mut packets = Vec::new();
        for marker in 0_u8..16 {
            slab.prepare(1_280).unwrap();
            let buffer = slab.read_buffer();
            buffer[..20].fill(marker);
            packets.push(slab.take_packet(20).unwrap());
        }

        assert_eq!(slab.fresh_allocations(), 2);
        assert!(slab.fresh_allocations() * 4 <= packets.len() as u64);
        for (marker, packet) in (0_u8..16).zip(&packets) {
            assert_eq!(packet.as_ref(), &[marker; 20]);
        }
        let clone = packets[0].clone();
        assert!(packets.remove(0).try_into_mut().is_err());
        assert_eq!(clone.as_ref(), &[0; 20]);
    }

    #[test]
    fn mtu_change_uses_a_new_slab_and_keeps_old_packets_immutable() {
        let mut slab = TunReadSlab::new();
        slab.prepare(1_280).unwrap();
        slab.read_buffer()[..20].fill(1);
        let first = slab.take_packet(20).unwrap();
        slab.prepare(1_500).unwrap();
        slab.read_buffer()[..20].fill(2);
        let second = slab.take_packet(20).unwrap();

        assert_eq!(slab.fresh_allocations(), 2);
        assert_eq!(first.as_ref(), &[1; 20]);
        assert_eq!(second.as_ref(), &[2; 20]);
    }

    #[test]
    fn invalid_slot_or_packet_lengths_fail_closed() {
        let mut slab = TunReadSlab::new();
        assert!(slab.prepare(0).is_err());
        slab.prepare(1_280).unwrap();
        assert!(slab.take_packet(0).is_err());
        assert!(slab.take_packet(1_281).is_err());
    }
}
