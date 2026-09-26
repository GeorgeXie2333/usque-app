//! Live policy for application traffic after direct routing. Never apply this
//! to transport sockets, internal DNS, or the WARP underlay of a chained VPN.
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Debug, Default)]
pub(crate) struct ApplicationTrafficPolicy {
    disable_quic: AtomicBool,
    #[cfg(test)]
    loopback_quic_port: Option<u16>,
}

impl ApplicationTrafficPolicy {
    pub(crate) fn new(disable_quic: bool) -> Self {
        Self {
            disable_quic: AtomicBool::new(disable_quic),
            #[cfg(test)]
            loopback_quic_port: None,
        }
    }

    /// Exercise the real routing/filter order on an OS-assigned loopback port
    /// without requiring privileged UDP 443 binds. Production stays UDP 443 only.
    #[cfg(test)]
    pub(crate) fn for_loopback_quic(port: u16) -> Self {
        assert!(port >= 1024);
        Self {
            loopback_quic_port: Some(port),
            ..Self::default()
        }
    }

    pub(crate) fn set_disable_quic(&self, value: bool) {
        self.disable_quic.store(value, Ordering::SeqCst);
    }

    pub(crate) fn blocks_udp(&self, remote_port: u16) -> bool {
        let is_quic = remote_port == 443;
        #[cfg(test)]
        let is_quic = is_quic || self.loopback_quic_port == Some(remote_port);
        is_quic && self.disable_quic.load(Ordering::SeqCst)
    }
}

#[cfg(test)]
mod tests;
