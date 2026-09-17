//! Live policy for application traffic after direct routing. Never apply this
//! to transport sockets, internal DNS, or the WARP underlay of a chained VPN.
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Debug, Default)]
pub(crate) struct ApplicationTrafficPolicy {
    disable_quic: AtomicBool,
}

impl ApplicationTrafficPolicy {
    pub(crate) fn new(disable_quic: bool) -> Self {
        Self {
            disable_quic: AtomicBool::new(disable_quic),
        }
    }

    pub(crate) fn set_disable_quic(&self, value: bool) {
        self.disable_quic.store(value, Ordering::SeqCst);
    }

    pub(crate) fn blocks_udp(&self, remote_port: u16) -> bool {
        remote_port == 443 && self.disable_quic.load(Ordering::SeqCst)
    }
}

#[cfg(test)]
mod tests;
