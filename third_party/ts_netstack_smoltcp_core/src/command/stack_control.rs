//! Commands that mutate network stack configuration.

use alloc::vec::Vec;
use core::net::IpAddr;

use crate::command;

/// Mutate the network stack's configuration.
#[derive(Debug)]
pub enum Command {
    /// Set local TCP send limits for an explicitly opted-in private stack.
    /// Revisions must increase; old commands are ignored. None removes a limit.
    SetTcpPacketBudget {
        /// Monotonic revision owned by one runtime, never a remote packet value.
        revision: u64,
        /// Maximum IPv4 IP packet length (128..=65535).
        ipv4: Option<usize>,
        /// Maximum IPv6 IP packet length (1280..=65535).
        ipv6: Option<usize>,
    },
    /// Set the network interface's IPs.
    SetIps {
        /// IPs to assign to the netstack's interface.
        ///
        /// If the netstack was configured with
        /// [`Config::loopback`](crate::Config::loopback) enabled, the loopback addresses
        /// should not be included here.
        ///
        /// May fail if `smoltcp` was not configured with a sufficient
        /// `iface-max-addr-count-*` (feature flag).
        new_ips: Vec<IpAddr>,
    },
}

impl From<Command> for command::Command {
    fn from(command: Command) -> Self {
        command::Command::StackControl(command)
    }
}
