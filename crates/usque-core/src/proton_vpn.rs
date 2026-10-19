//! Proton control-plane contracts. Secrets use a separate authenticated IPC field
//! and an encrypted sidecar; ordinary status never contains an authentication URL.
pub mod store;

use crate::chain_exit::{Endpoint, ImportError};
use serde::{Deserialize, Serialize};
use std::fmt;
use uuid::Uuid;
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

pub const MAX_INPUT_BYTES: usize = 40 * 1024;
pub const MAX_RESPONSE_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_CERTIFICATE_BYTES: usize = 48 * 1024;
pub const API_ORIGIN: &str = "https://vpn-api.proton.me";
pub const APP_VERSION: &str = "external-vpn@0.1.0";

/// Headers are kept with the encrypted session so renewal uses the same client
/// identity. Device challenge data is never persisted with this record.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApiIdentity {
    pub app_version: String,
    pub user_agent: String,
    pub locale: String,
}
impl Default for ApiIdentity {
    fn default() -> Self {
        Self {
            app_version: APP_VERSION.into(),
            user_agent: format!(
                "Usque/{} (independent Proton control-plane client)",
                env!("CARGO_PKG_VERSION")
            ),
            locale: String::new(),
        }
    }
}
impl ApiIdentity {
    pub fn validate(&self) -> Result<(), ImportError> {
        for (value, limit) in [(&self.app_version, 128), (&self.user_agent, 512)] {
            if value.is_empty()
                || value.len() > limit
                || !value.bytes().all(|byte| (32..=126).contains(&byte))
            {
                return Err(error("invalid_client_context"));
            }
        }
        if self.locale.len() > 35
            || !self
                .locale
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        {
            return Err(error("invalid_client_context"));
        }
        Ok(())
    }
}

/// Serialize provider jobs with account/configuration mutations across native
/// processes. Keep this stable sidecar outside directories cleared by logout.
pub fn lifecycle_lease(parent: &std::path::Path) -> Result<std::fs::File, ImportError> {
    crate::storage::ConfigStore::new(parent.join("proton-lifecycle.json"))
        .try_lock_exclusive()
        .map_err(|_| error("secure_storage_failed"))?
        .ok_or_else(|| error("proton_operation_busy"))
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub action: String,
    #[serde(default)]
    pub job_id: Option<Uuid>,
    #[serde(default)]
    pub profile_id: Option<Uuid>,
    #[serde(default)]
    pub challenge_id: Option<Uuid>,
    #[serde(default)]
    pub node_id: Option<String>,
    #[serde(default)]
    pub name: String,
}
impl Request {
    pub fn parse(text: &str) -> Result<Self, ImportError> {
        if text.len() > 4096 {
            return Err(error("invalid_request"));
        }
        let value: Self = serde_json::from_str(text).map_err(|_| error("invalid_request"))?;
        value.validate()?;
        Ok(value)
    }
    pub fn validate(&self) -> Result<(), ImportError> {
        if !matches!(
            self.action.as_str(),
            "guest"
                | "login"
                | "get"
                | "cancel"
                | "totp"
                | "handoff"
                | "verify"
                | "select_node"
                | "refresh"
                | "logout"
                | "forget"
                | "list"
        ) || self.name.chars().count() > 64
            || self.name.chars().any(char::is_control)
            || self.node_id.as_ref().is_some_and(|v| {
                v.is_empty() || v.len() > 256 || !v.is_ascii() || v.chars().any(char::is_control)
            })
            || matches!(
                self.action.as_str(),
                "cancel" | "totp" | "handoff" | "verify" | "select_node"
            ) && self.job_id.is_none()
            || matches!(self.action.as_str(), "handoff" | "verify") && self.challenge_id.is_none()
            || matches!(self.action.as_str(), "refresh" | "logout" | "forget")
                && self.profile_id.is_none()
            || self.action == "select_node" && self.node_id.is_none()
        {
            return Err(error("invalid_request"));
        }
        Ok(())
    }
    pub fn needs_network(&self) -> bool {
        matches!(
            self.action.as_str(),
            "guest" | "login" | "refresh" | "logout"
        )
    }
}
impl fmt::Debug for Request {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProtonRequest")
            .field("action", &self.action)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq)]
pub struct Node {
    pub id: String,
    pub name: String,
    pub country: String,
    pub city: String,
    pub load: u32,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct SelectedNode {
    pub metadata: Node,
    pub domain: String,
    pub endpoint: Endpoint,
    pub public_key: String,
}
impl SelectedNode {
    pub fn validate(&self) -> Result<(), ImportError> {
        let domain = self.domain.as_str();
        if domain.len() > 253
            || domain.is_empty()
            || !domain.is_ascii()
            || domain.split('.').count() < 2
            || domain.split('.').any(|part| {
                part.is_empty()
                    || part.len() > 63
                    || part.starts_with('-')
                    || part.ends_with('-')
                    || !part.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
            })
            || self.endpoint.address().is_none()
            || self.public_key.len() != 44
        {
            return Err(error("invalid_node"));
        }
        Endpoint::parse(&self.endpoint.host, &self.endpoint.port.to_string(), 0)?;
        Ok(())
    }
}

#[derive(Clone, Serialize)]
pub struct Job {
    pub id: Uuid,
    pub state: String,
    pub phase: String,
    pub failure: Option<String>,
    pub api_code: Option<u32>,
    pub http_status: Option<u16>,
    pub challenge_id: Option<Uuid>,
    /// Unix milliseconds, shared with the short-lived native verification UI.
    pub expires_at: Option<i64>,
    pub profile_id: Option<Uuid>,
    pub nodes: Vec<Node>,
}
impl Job {
    pub fn new() -> Self {
        Self {
            id: Uuid::new_v4(),
            state: "running".into(),
            phase: "underlay".into(),
            failure: None,
            api_code: None,
            http_status: None,
            challenge_id: None,
            expires_at: None,
            profile_id: None,
            nodes: Vec::new(),
        }
    }
}
impl Default for Job {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Serialize)]
pub struct AccountSummary {
    pub id: Uuid,
    pub name: String,
    pub identity: String,
    pub node_name: String,
    pub country: String,
    pub expires_at: i64,
    pub needs_login: bool,
}
#[derive(Default, Serialize)]
pub struct Response {
    pub job: Option<Job>,
    pub profiles: Vec<AccountSummary>,
    pub error: Option<String>,
}

#[derive(Clone, Default, Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
pub struct SessionSecrets {
    pub uid: String,
    pub access_token: String,
    pub refresh_token: String,
    pub user_id: String,
}
impl SessionSecrets {
    pub fn validate(&self) -> Result<(), ImportError> {
        for value in [
            &self.uid,
            &self.access_token,
            &self.refresh_token,
            &self.user_id,
        ] {
            if value.is_empty()
                || value.len() > 8192
                || !value.is_ascii()
                || value.bytes().any(|v| !(33..=126).contains(&v))
            {
                return Err(error("invalid_session"));
            }
        }
        Ok(())
    }
}
impl fmt::Debug for SessionSecrets {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SessionSecrets([redacted])")
    }
}

/// Ephemeral material passed directly to the currently selected WireGuard job.
/// It is deliberately excluded from protobuf, profile settings and diagnostics.
#[derive(Clone)]
pub struct TunnelCredentials {
    pub certificate_chain: Zeroizing<String>,
    pub private_key_pkcs8: Zeroizing<Vec<u8>>,
    pub server_name: String,
    pub expires_at: i64,
}
/// An owned capability for one selected account, never a platform vault.
pub trait LeasePersistence: Send + Sync {
    fn load(&self) -> Result<Record, ImportError>;
    fn save(&self, record: &Record) -> Result<(), ImportError>;
    fn lock(&self) -> Result<std::fs::File, ImportError>;
}
impl fmt::Debug for TunnelCredentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("TunnelCredentials([redacted])")
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Record {
    pub version: u32,
    pub id: Uuid,
    pub revision: Uuid,
    pub name: String,
    pub identity: String,
    #[serde(default)]
    pub api_identity: ApiIdentity,
    pub session: SessionSecrets,
    pub ed25519_seed: Zeroizing<Vec<u8>>,
    pub certificate_chain: Zeroizing<String>,
    pub expires_at: i64,
    pub refresh_at: i64,
    pub max_tier: u32,
    pub node: SelectedNode,
    /// Written before a token-rotating request; an ambiguous result requires
    /// explicit reauthentication rather than replaying a stale refresh token.
    pub rotation_pending: bool,
}
impl Record {
    pub fn validate(&self) -> Result<(), ImportError> {
        self.api_identity.validate()?;
        self.session.validate()?;
        self.node.validate()?;
        if self.version != 1
            || self.ed25519_seed.len() != 32
            || !matches!(self.identity.as_str(), "guest" | "login")
            || self.name.is_empty()
            || self.name.chars().count() > 64
            || self.name.chars().any(char::is_control)
            || self.certificate_chain.is_empty()
            || self.certificate_chain.len() > MAX_CERTIFICATE_BYTES
            || self.refresh_at <= 0
            || self.expires_at <= self.refresh_at
        {
            return Err(error("invalid_record"));
        }
        Ok(())
    }
    pub fn summary(&self) -> AccountSummary {
        AccountSummary {
            id: self.id,
            name: self.name.clone(),
            identity: self.identity.clone(),
            node_name: self.node.metadata.name.clone(),
            country: self.node.metadata.country.clone(),
            expires_at: self.expires_at,
            needs_login: self.rotation_pending,
        }
    }
}
impl fmt::Debug for Record {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProtonRecord")
            .field("id", &self.id)
            .field("secrets", &"[redacted]")
            .finish()
    }
}
pub fn error(reason: &str) -> ImportError {
    ImportError {
        line: 0,
        field: "proton_vpn".into(),
        reason: reason.into(),
    }
}

#[cfg(test)]
mod tests;
