//! Preserve disjoint mutable slab views until all packet headers are final.
use bytes::{Bytes, BytesMut};
use std::ops::Deref;

pub(crate) enum OutboundPacket {
    Mutable(BytesMut),
    Shared(Bytes),
}

impl OutboundPacket {
    pub(crate) fn into_mut(self) -> BytesMut {
        match self {
            Self::Mutable(packet) => packet,
            Self::Shared(packet) => packet
                .try_into_mut()
                .unwrap_or_else(|packet| BytesMut::from(packet.as_ref())),
        }
    }

    pub(crate) fn freeze(self) -> Bytes {
        match self {
            Self::Mutable(packet) => packet.freeze(),
            Self::Shared(packet) => packet,
        }
    }
}

impl Deref for OutboundPacket {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        match self {
            Self::Mutable(packet) => packet,
            Self::Shared(packet) => packet,
        }
    }
}

impl AsRef<[u8]> for OutboundPacket {
    fn as_ref(&self) -> &[u8] {
        self
    }
}

impl From<Bytes> for OutboundPacket {
    fn from(packet: Bytes) -> Self {
        Self::Shared(packet)
    }
}

impl std::fmt::Debug for OutboundPacket {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OutboundPacket")
            .field("length", &self.len())
            .finish()
    }
}
