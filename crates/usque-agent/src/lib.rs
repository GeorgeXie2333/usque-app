//! Least-privilege platform service for Windows.
//!
//! This crate deliberately has no dependency on the WARP registration or
//! MASQUE transport crates. The Agent receives only validated interface,
//! route, DNS, firewall, packet-ring, and system-proxy operations.

pub mod coordinator;
pub mod journal;
pub mod plan;

#[cfg(windows)]
pub mod windows;

pub const AGENT_PROTOCOL_VERSION: u32 = 2;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthenticatedCaller {
    pub process_id: u32,
    pub user_sid: String,
    pub executable_path: std::path::PathBuf,
    /// Ephemeral process HANDLE value owned by the authenticated pipe
    /// connection. It is never serialized or persisted.
    pub process_handle: Option<usize>,
}
