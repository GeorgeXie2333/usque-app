//! Versioned shared-memory SPSC packet-ring layout.
//!
//! One ring carries Engine → Agent packets and the other carries Agent →
//! Engine packets. Each direction has exactly one producer and one consumer.
//! All indices are monotonic wrapping counters; Release/Acquire publication
//! keeps packet bytes ordered across processes.

use std::{
    mem,
    ptr::{self, NonNull},
    sync::atomic::{AtomicU32, AtomicU64, Ordering},
};

use thiserror::Error;

pub const PACKET_RING_LAYOUT_VERSION: u32 = 1;
pub const MIN_RING_CAPACITY: u32 = 128 * 1024;
pub const MAX_RING_CAPACITY: u32 = 64 * 1024 * 1024;
pub const MAX_PACKET_BYTES: usize = 0xffff;
const MAGIC: [u8; 8] = *b"USQRING1";
const RECORD_HEADER_BYTES: usize = mem::size_of::<u32>();

#[repr(C, align(64))]
struct RingHeader {
    magic: [u8; 8],
    layout_version: u32,
    capacity: u32,
    engine_to_agent_head: AtomicU32,
    engine_to_agent_tail: AtomicU32,
    agent_to_engine_head: AtomicU32,
    agent_to_engine_tail: AtomicU32,
    engine_to_agent_dropped: AtomicU64,
    agent_to_engine_dropped: AtomicU64,
    reserved: [u8; 16],
}

const _: () = assert!(mem::size_of::<RingHeader>() == 64);
const _: () = assert!(mem::align_of::<RingHeader>() == 64);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PacketDirection {
    EngineToAgent,
    AgentToEngine,
}

#[derive(Clone, Copy)]
pub struct SharedPacketRing {
    header: NonNull<RingHeader>,
    engine_to_agent: NonNull<u8>,
    agent_to_engine: NonNull<u8>,
    capacity: u32,
}

// SAFETY: construction validates alignment/layout and callers guarantee the
// mapped memory outlives every copy. All cross-thread state is accessed
// atomically; each data direction is SPSC by protocol contract.
unsafe impl Send for SharedPacketRing {}
// SAFETY: `&SharedPacketRing` is safe to share: head/tail/dropped counters are
// atomics, NonNull bases are immutable after attach, and SPSC ownership of each
// direction is an external protocol invariant (not interior &mut sharing).
unsafe impl Sync for SharedPacketRing {}

impl SharedPacketRing {
    pub fn mapped_bytes(capacity: u32) -> Result<usize, PacketRingError> {
        validate_capacity(capacity)?;
        mem::size_of::<RingHeader>()
            .checked_add(capacity as usize * 2)
            .ok_or(PacketRingError::SizeOverflow)
    }

    /// Initializes a newly created, page-aligned shared-memory mapping.
    ///
    /// # Safety
    ///
    /// `base` must be valid and writable for `length` bytes, aligned to at
    /// least 64 bytes, zero-aliasing during initialization, and remain mapped
    /// until every resulting [`SharedPacketRing`] is dropped.
    pub unsafe fn initialize(
        base: NonNull<u8>,
        length: usize,
        capacity: u32,
    ) -> Result<Self, PacketRingError> {
        let required = Self::mapped_bytes(capacity)?;
        validate_mapping(base, length, required)?;
        // SAFETY: guaranteed by the caller and validated bounds/alignment.
        unsafe {
            ptr::write_bytes(base.as_ptr(), 0, required);
            ptr::write(
                base.cast::<RingHeader>().as_ptr(),
                RingHeader {
                    magic: MAGIC,
                    layout_version: PACKET_RING_LAYOUT_VERSION,
                    capacity,
                    engine_to_agent_head: AtomicU32::new(0),
                    engine_to_agent_tail: AtomicU32::new(0),
                    agent_to_engine_head: AtomicU32::new(0),
                    agent_to_engine_tail: AtomicU32::new(0),
                    engine_to_agent_dropped: AtomicU64::new(0),
                    agent_to_engine_dropped: AtomicU64::new(0),
                    reserved: [0; 16],
                },
            );
        }
        // SAFETY: this mapping was just initialized above.
        unsafe { Self::attach(base, length) }
    }

    /// Attaches to a previously initialized mapping.
    ///
    /// # Safety
    ///
    /// `base` must refer to a live shared mapping initialized by this exact
    /// layout version and outlive every returned view. Each direction must
    /// still have only one producer and one consumer across all processes.
    pub unsafe fn attach(base: NonNull<u8>, length: usize) -> Result<Self, PacketRingError> {
        validate_mapping(base, length, mem::size_of::<RingHeader>())?;
        // SAFETY: base is aligned/readable for a complete header.
        let header = unsafe { base.cast::<RingHeader>().as_ref() };
        if header.magic != MAGIC {
            return Err(PacketRingError::Magic);
        }
        if header.layout_version != PACKET_RING_LAYOUT_VERSION {
            return Err(PacketRingError::Version(header.layout_version));
        }
        validate_capacity(header.capacity)?;
        let required = Self::mapped_bytes(header.capacity)?;
        validate_mapping(base, length, required)?;
        // SAFETY: required size includes the header followed by two complete
        // capacity-byte regions.
        let engine_to_agent =
            unsafe { NonNull::new_unchecked(base.as_ptr().add(mem::size_of::<RingHeader>())) };
        // SAFETY: second region starts at capacity bytes after the first and
        // remains inside the validated mapping.
        let agent_to_engine = unsafe {
            NonNull::new_unchecked(engine_to_agent.as_ptr().add(header.capacity as usize))
        };
        Ok(Self {
            header: base.cast(),
            engine_to_agent,
            agent_to_engine,
            capacity: header.capacity,
        })
    }

    pub const fn capacity(&self) -> u32 {
        self.capacity
    }

    pub fn try_push(
        &self,
        direction: PacketDirection,
        packet: &[u8],
    ) -> Result<bool, PacketRingError> {
        if packet.is_empty() || packet.len() > MAX_PACKET_BYTES {
            return Err(PacketRingError::PacketSize(packet.len()));
        }
        let required = aligned_record_bytes(packet.len())?;
        let (head, tail, dropped, buffer) = self.producer_fields(direction);
        let head_value = head.load(Ordering::Relaxed);
        let tail_value = tail.load(Ordering::Acquire);
        let used = head_value.wrapping_sub(tail_value);
        if used > self.capacity {
            return Err(PacketRingError::CorruptIndices {
                head: head_value,
                tail: tail_value,
                capacity: self.capacity,
            });
        }
        if required as u32 > self.capacity - used {
            dropped.fetch_add(1, Ordering::Relaxed);
            return Ok(false);
        }

        let length = u32::try_from(packet.len()).expect("packet bound fits u32");
        // SAFETY: this is the sole producer for this direction; free capacity
        // was checked using the consumer's Acquire-published tail.
        unsafe {
            write_wrapped(buffer, self.capacity, head_value, &length.to_le_bytes());
            write_wrapped(
                buffer,
                self.capacity,
                head_value.wrapping_add(RECORD_HEADER_BYTES as u32),
                packet,
            );
            let padding = required - RECORD_HEADER_BYTES - packet.len();
            if padding != 0 {
                write_wrapped(
                    buffer,
                    self.capacity,
                    head_value
                        .wrapping_add(RECORD_HEADER_BYTES as u32)
                        .wrapping_add(length),
                    &[0_u8; 3][..padding],
                );
            }
        }
        head.store(head_value.wrapping_add(required as u32), Ordering::Release);
        Ok(true)
    }

    pub fn try_pop(&self, direction: PacketDirection) -> Result<Option<Vec<u8>>, PacketRingError> {
        let (head, tail, buffer) = self.consumer_fields(direction);
        let tail_value = tail.load(Ordering::Relaxed);
        let head_value = head.load(Ordering::Acquire);
        let used = head_value.wrapping_sub(tail_value);
        if used > self.capacity {
            return Err(PacketRingError::CorruptIndices {
                head: head_value,
                tail: tail_value,
                capacity: self.capacity,
            });
        }
        if used == 0 {
            return Ok(None);
        }
        if used < RECORD_HEADER_BYTES as u32 {
            return Err(PacketRingError::TruncatedRecord);
        }
        let mut length = [0_u8; RECORD_HEADER_BYTES];
        // SAFETY: Acquire observed at least a complete record header published
        // by the sole producer.
        unsafe {
            read_wrapped(buffer, self.capacity, tail_value, &mut length);
        }
        let length = u32::from_le_bytes(length) as usize;
        if length == 0 || length > MAX_PACKET_BYTES {
            return Err(PacketRingError::PacketSize(length));
        }
        let required = aligned_record_bytes(length)?;
        if required as u32 > used || required as u32 > self.capacity {
            return Err(PacketRingError::TruncatedRecord);
        }
        let mut packet = vec![0_u8; length];
        // SAFETY: the complete aligned record is within the producer-published
        // used range and this is the sole consumer.
        unsafe {
            read_wrapped(
                buffer,
                self.capacity,
                tail_value.wrapping_add(RECORD_HEADER_BYTES as u32),
                &mut packet,
            );
        }
        tail.store(tail_value.wrapping_add(required as u32), Ordering::Release);
        Ok(Some(packet))
    }

    pub fn dropped(&self, direction: PacketDirection) -> u64 {
        let header = self.header();
        match direction {
            PacketDirection::EngineToAgent => {
                header.engine_to_agent_dropped.load(Ordering::Relaxed)
            }
            PacketDirection::AgentToEngine => {
                header.agent_to_engine_dropped.load(Ordering::Relaxed)
            }
        }
    }

    fn header(&self) -> &RingHeader {
        // SAFETY: construction guarantees a live aligned header for the
        // lifetime of this view.
        unsafe { self.header.as_ref() }
    }

    fn producer_fields(
        &self,
        direction: PacketDirection,
    ) -> (&AtomicU32, &AtomicU32, &AtomicU64, NonNull<u8>) {
        let header = self.header();
        match direction {
            PacketDirection::EngineToAgent => (
                &header.engine_to_agent_head,
                &header.engine_to_agent_tail,
                &header.engine_to_agent_dropped,
                self.engine_to_agent,
            ),
            PacketDirection::AgentToEngine => (
                &header.agent_to_engine_head,
                &header.agent_to_engine_tail,
                &header.agent_to_engine_dropped,
                self.agent_to_engine,
            ),
        }
    }

    fn consumer_fields(&self, direction: PacketDirection) -> (&AtomicU32, &AtomicU32, NonNull<u8>) {
        let header = self.header();
        match direction {
            PacketDirection::EngineToAgent => (
                &header.engine_to_agent_head,
                &header.engine_to_agent_tail,
                self.engine_to_agent,
            ),
            PacketDirection::AgentToEngine => (
                &header.agent_to_engine_head,
                &header.agent_to_engine_tail,
                self.agent_to_engine,
            ),
        }
    }
}

fn validate_capacity(capacity: u32) -> Result<(), PacketRingError> {
    if !(MIN_RING_CAPACITY..=MAX_RING_CAPACITY).contains(&capacity) || !capacity.is_power_of_two() {
        Err(PacketRingError::Capacity(capacity))
    } else {
        Ok(())
    }
}

fn validate_mapping(
    base: NonNull<u8>,
    length: usize,
    required: usize,
) -> Result<(), PacketRingError> {
    if !(base.as_ptr() as usize).is_multiple_of(mem::align_of::<RingHeader>()) {
        return Err(PacketRingError::Alignment);
    }
    if length < required {
        return Err(PacketRingError::MappingSize { required, length });
    }
    Ok(())
}

fn aligned_record_bytes(packet_bytes: usize) -> Result<usize, PacketRingError> {
    RECORD_HEADER_BYTES
        .checked_add(packet_bytes)
        .and_then(|bytes| bytes.checked_add(3))
        .map(|bytes| bytes & !3)
        .ok_or(PacketRingError::SizeOverflow)
}

unsafe fn write_wrapped(buffer: NonNull<u8>, capacity: u32, offset: u32, source: &[u8]) {
    let start = (offset & (capacity - 1)) as usize;
    let first = source.len().min(capacity as usize - start);
    // SAFETY: caller validated ring free space and SPSC ownership.
    unsafe {
        ptr::copy_nonoverlapping(source.as_ptr(), buffer.as_ptr().add(start), first);
        if first < source.len() {
            ptr::copy_nonoverlapping(
                source.as_ptr().add(first),
                buffer.as_ptr(),
                source.len() - first,
            );
        }
    }
}

unsafe fn read_wrapped(buffer: NonNull<u8>, capacity: u32, offset: u32, destination: &mut [u8]) {
    let start = (offset & (capacity - 1)) as usize;
    let first = destination.len().min(capacity as usize - start);
    // SAFETY: caller Acquire-observed a complete producer-published record.
    unsafe {
        ptr::copy_nonoverlapping(buffer.as_ptr().add(start), destination.as_mut_ptr(), first);
        if first < destination.len() {
            ptr::copy_nonoverlapping(
                buffer.as_ptr(),
                destination.as_mut_ptr().add(first),
                destination.len() - first,
            );
        }
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum PacketRingError {
    #[error("packet ring capacity must be a power of two between 128 KiB and 64 MiB: {0}")]
    Capacity(u32),
    #[error("packet ring size overflow")]
    SizeOverflow,
    #[error("shared mapping is not 64-byte aligned")]
    Alignment,
    #[error("shared mapping is too small: need {required}, got {length}")]
    MappingSize { required: usize, length: usize },
    #[error("shared mapping magic does not match Usque packet-ring v1")]
    Magic,
    #[error("shared mapping layout version {0} is unsupported")]
    Version(u32),
    #[error("packet must contain between 1 and 65535 bytes, got {0}")]
    PacketSize(usize),
    #[error("packet ring indices are corrupt: head={head}, tail={tail}, capacity={capacity}")]
    CorruptIndices { head: u32, tail: u32, capacity: u32 },
    #[error("packet ring contains a truncated record")]
    TruncatedRecord,
}

#[cfg(test)]
mod tests {
    use std::{
        alloc::{Layout, alloc_zeroed, dealloc},
        thread,
    };

    use super::*;

    struct AlignedMemory {
        pointer: NonNull<u8>,
        layout: Layout,
    }

    impl AlignedMemory {
        fn ring(capacity: u32) -> (Self, SharedPacketRing) {
            let bytes = SharedPacketRing::mapped_bytes(capacity).expect("size");
            let layout = Layout::from_size_align(bytes, 64).expect("layout");
            // SAFETY: layout is non-zero and valid.
            let pointer = NonNull::new(unsafe { alloc_zeroed(layout) }).expect("allocate");
            // SAFETY: this allocation is aligned, writable, uniquely owned, and
            // retained by Self for longer than the ring view.
            let ring =
                unsafe { SharedPacketRing::initialize(pointer, bytes, capacity) }.expect("ring");
            (Self { pointer, layout }, ring)
        }
    }

    impl Drop for AlignedMemory {
        fn drop(&mut self) {
            // SAFETY: pointer was allocated with this exact layout.
            unsafe {
                dealloc(self.pointer.as_ptr(), self.layout);
            }
        }
    }

    #[test]
    fn both_directions_round_trip_packets() {
        let (_memory, ring) = AlignedMemory::ring(MIN_RING_CAPACITY);
        for direction in [
            PacketDirection::EngineToAgent,
            PacketDirection::AgentToEngine,
        ] {
            assert!(ring.try_push(direction, b"packet").expect("push"));
            assert_eq!(
                ring.try_pop(direction).expect("pop"),
                Some(b"packet".to_vec())
            );
            assert_eq!(ring.try_pop(direction).expect("empty"), None);
        }
    }

    #[test]
    fn records_wrap_at_the_capacity_boundary() {
        let (_memory, ring) = AlignedMemory::ring(MIN_RING_CAPACITY);
        let header = ring.header();
        let near_end = MIN_RING_CAPACITY - 2;
        header
            .engine_to_agent_head
            .store(near_end, Ordering::Relaxed);
        header
            .engine_to_agent_tail
            .store(near_end, Ordering::Relaxed);
        assert!(
            ring.try_push(PacketDirection::EngineToAgent, b"wrapped")
                .expect("push")
        );
        assert_eq!(
            ring.try_pop(PacketDirection::EngineToAgent)
                .expect("pop")
                .expect("packet"),
            b"wrapped"
        );
    }

    #[test]
    fn full_ring_drops_without_overwriting_unread_data() {
        let (_memory, ring) = AlignedMemory::ring(MIN_RING_CAPACITY);
        let packet = vec![0x45; MAX_PACKET_BYTES];
        while ring
            .try_push(PacketDirection::AgentToEngine, &packet)
            .expect("push")
        {}
        assert_eq!(ring.dropped(PacketDirection::AgentToEngine), 1);
        assert_eq!(
            ring.try_pop(PacketDirection::AgentToEngine)
                .expect("pop")
                .expect("packet"),
            packet
        );
    }

    #[test]
    fn spsc_release_acquire_transfer_is_thread_safe() {
        let (_memory, ring) = AlignedMemory::ring(MIN_RING_CAPACITY);
        thread::scope(|scope| {
            scope.spawn(|| {
                for value in 0_u32..1_000 {
                    let bytes = value.to_le_bytes();
                    while !ring
                        .try_push(PacketDirection::EngineToAgent, &bytes)
                        .expect("push")
                    {
                        thread::yield_now();
                    }
                }
            });
            scope.spawn(|| {
                for expected in 0_u32..1_000 {
                    loop {
                        if let Some(packet) =
                            ring.try_pop(PacketDirection::EngineToAgent).expect("pop")
                        {
                            assert_eq!(
                                u32::from_le_bytes(packet.try_into().expect("u32")),
                                expected
                            );
                            break;
                        }
                        thread::yield_now();
                    }
                }
            });
        });
    }

    #[test]
    fn corrupt_peer_indices_fail_closed() {
        let (_memory, ring) = AlignedMemory::ring(MIN_RING_CAPACITY);
        ring.header()
            .engine_to_agent_head
            .store(MIN_RING_CAPACITY + 1, Ordering::Relaxed);
        assert!(matches!(
            ring.try_pop(PacketDirection::EngineToAgent),
            Err(PacketRingError::CorruptIndices { .. })
        ));
    }
}
