//! Additive JSON protocol carried inside authenticated XPC messages.
use crate::credentials::SessionCredentials;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use usque_core::Profile;
use uuid::Uuid;

pub const PROTOCOL_VERSION: u32 = 1;
pub const MAX_CONTROL_BYTES: usize = 2 * 1024 * 1024;

#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionKey {
    pub session_id: Uuid,
    pub config_revision: u64,
    pub network_generation: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionConfiguration {
    pub profile: Profile,
    /// Read-only verified catalog; never a vault or credential destination.
    pub geo_cache_dir: PathBuf,
    /// Hash of the profile and all rule artifacts consumed by the providers.
    #[serde(default)]
    pub policy_digest: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    /// Complete protection first; callers must observe protection_active before
    /// stopping the Engine's proxy-only runtime.
    Protect,
    Start {
        configuration: SessionConfiguration,
        credentials: SessionCredentials,
    },
    Replace {
        configuration: SessionConfiguration,
        credentials: SessionCredentials,
    },
    Stop,
    Snapshot,
    Cancel {
        operation_id: Uuid,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Envelope {
    pub version: u32,
    pub operation_id: Uuid,
    pub session: SessionKey,
    pub command: Command,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Stopped,
    Protected,
    Starting,
    Connected,
    Stopping,
    Failed,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    pub phase: Phase,
    pub session: Option<SessionKey>,
    pub protection_active: bool,
    pub production_capable: bool,
    pub error_code: Option<String>,
    pub selected_path: Option<ProviderPath>,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProviderPath {
    pub transport: usque_core::Transport,
    pub endpoint_family: usque_core::AddressFamily,
    pub ipv4_available: bool,
    pub ipv6_available: bool,
}
impl From<usque_transport::RuntimePath> for ProviderPath {
    fn from(value: usque_transport::RuntimePath) -> Self {
        Self {
            transport: value.transport,
            endpoint_family: value.endpoint_family,
            ipv4_available: value.ipv4_available,
            ipv6_available: value.ipv6_available,
        }
    }
}
impl Default for Snapshot {
    fn default() -> Self {
        Self {
            phase: Phase::Stopped,
            session: None,
            protection_active: false,
            production_capable: false,
            error_code: None,
            selected_path: None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Acceptance {
    Accepted,
    Duplicate,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Reply {
    pub operation_id: Uuid,
    pub acceptance: Acceptance,
    pub snapshot: Snapshot,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlError {
    #[error("unsupported_protocol_version")]
    UnsupportedVersion,
    #[error("invalid_request")]
    InvalidRequest,
    #[error("stale_session_or_generation")]
    StaleSession,
    #[error("operation_id_reused_with_different_payload")]
    ReplayMismatch,
    #[error("control_queue_full")]
    Backpressure,
    #[error("control_replay_window_full")]
    ReplayWindowFull,
    #[error("provider_unavailable")]
    Unavailable,
}
