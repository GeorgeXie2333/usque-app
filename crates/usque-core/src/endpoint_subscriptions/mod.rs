//! Bounded, non-secret endpoint subscription catalog and parser.
//!
//! Subscriptions store external MASQUE endpoint lists bound to a single WARP
//! account tier (`Free`, `Plus`, or `ZeroTrust`) in `endpoint_subscriptions.json`.

use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{self, BufReader, BufWriter, Write};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tempfile::NamedTempFile;
use thiserror::Error;
use uuid::Uuid;

use crate::EndpointPresetTier;
use crate::storage::StoreError;

pub mod fetch;
pub mod parser;
#[cfg(test)]
mod tests;

pub use fetch::{SubscriptionFetchError, fetch_subscription_direct};
pub use parser::{
    ParsedSubscription, canonical_public_domain, is_usable_public_endpoint_ip,
    parse_subscription_body, redacted_url_summary, resolve_subscription_redirect_url,
    validate_subscription_name, validate_subscription_url,
};

pub const ENDPOINT_SUBSCRIPTIONS_FILENAME: &str = "endpoint_subscriptions.json";
pub const CURRENT_SUBSCRIPTION_CATALOG_VERSION: u32 = 1;
pub const MAX_ENDPOINT_SUBSCRIPTIONS: usize = 32;
pub const MAX_ENDPOINTS_PER_SUBSCRIPTION: usize = 512;
pub const MAX_SUBSCRIPTION_URL_BYTES: usize = 2048;
pub const MAX_SUBSCRIPTION_BODY_BYTES: usize = 512 * 1024;
pub const MAX_SUBSCRIPTION_NOTE_CHARS: usize = 128;
pub const MAX_SUBSCRIPTION_NAME_CHARS: usize = 64;
pub const MAX_SUBSCRIPTION_COMMAND_BYTES: usize = 256 * 1024;
pub const MAX_SUBSCRIPTION_PROBE_TARGETS: usize = 512;
pub const SUBSCRIPTION_AUTO_REFRESH_RETRY_MS: u64 = 60 * 60 * 1000;

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SubscriptionRefreshInterval {
    #[default]
    Off,
    Daily,
    Weekly,
}

impl SubscriptionRefreshInterval {
    pub const fn interval_ms(self) -> Option<u64> {
        match self {
            Self::Off => None,
            Self::Daily => Some(24 * 60 * 60 * 1000),
            Self::Weekly => Some(7 * 24 * 60 * 60 * 1000),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum SubscriptionEndpointHost {
    Ipv4(Ipv4Addr),
    Ipv6(Ipv6Addr),
    Domain(String),
}

impl SubscriptionEndpointHost {
    pub const fn is_ipv4(&self) -> bool {
        matches!(self, Self::Ipv4(_))
    }

    pub const fn is_ipv6(&self) -> bool {
        matches!(self, Self::Ipv6(_))
    }

    pub const fn is_domain(&self) -> bool {
        matches!(self, Self::Domain(_))
    }

    pub fn display_host(&self) -> String {
        match self {
            Self::Ipv4(ip) => ip.to_string(),
            Self::Ipv6(ip) => ip.to_string(),
            Self::Domain(host) => host.clone(),
        }
    }

    pub fn validate(&self) -> Result<(), SubscriptionError> {
        match self {
            Self::Ipv4(ip) => is_usable_public_endpoint_ip(IpAddr::V4(*ip))
                .then_some(())
                .ok_or(SubscriptionError::InvalidBody),
            Self::Ipv6(ip) => is_usable_public_endpoint_ip(IpAddr::V6(*ip))
                .then_some(())
                .ok_or(SubscriptionError::InvalidBody),
            Self::Domain(host) => canonical_public_domain(host)
                .filter(|canonical| canonical == host)
                .map(|_| ())
                .ok_or(SubscriptionError::InvalidBody),
        }
    }
}

pub fn canonical_endpoint_id(host: &SubscriptionEndpointHost, port: Option<u16>) -> String {
    let port_part = port.map_or_else(|| "default".to_owned(), |p| p.to_string());
    match host {
        SubscriptionEndpointHost::Ipv4(ip) => format!("v4:{ip}:{port_part}"),
        SubscriptionEndpointHost::Ipv6(ip) => format!("v6:{ip}:{port_part}"),
        SubscriptionEndpointHost::Domain(domain) => format!("host:{domain}:{port_part}"),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SubscriptionEndpoint {
    pub id: String,
    pub host: SubscriptionEndpointHost,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
    #[serde(default)]
    pub pinned: bool,
}

/// Whether two optional runtime endpoints dial the same target. Notes, pin
/// state and the catalogue identifier are presentation metadata and never
/// require a reconnect or mark a scene modified.
pub fn same_subscription_target(
    left: Option<&SubscriptionEndpoint>,
    right: Option<&SubscriptionEndpoint>,
) -> bool {
    match (left, right) {
        (None, None) => true,
        (Some(left), Some(right)) => left.same_target(right),
        _ => false,
    }
}

impl SubscriptionEndpoint {
    pub fn same_target(&self, other: &Self) -> bool {
        self.host == other.host && self.port == other.port
    }

    pub const fn effective_port(&self, fallback_port: u16) -> u16 {
        match self.port {
            Some(port) => port,
            None => fallback_port,
        }
    }

    pub fn socket_addr(&self, fallback_port: u16) -> Option<SocketAddr> {
        let port = self.effective_port(fallback_port);
        match &self.host {
            SubscriptionEndpointHost::Ipv4(ip) => Some(SocketAddr::new(IpAddr::V4(*ip), port)),
            SubscriptionEndpointHost::Ipv6(ip) => Some(SocketAddr::new(IpAddr::V6(*ip), port)),
            SubscriptionEndpointHost::Domain(_) => None,
        }
    }

    pub fn validate(&self) -> Result<(), SubscriptionError> {
        if self.port == Some(0) || self.note.chars().count() > MAX_SUBSCRIPTION_NOTE_CHARS {
            return Err(SubscriptionError::InvalidBody);
        }
        self.host.validate()?;
        if self.id != canonical_endpoint_id(&self.host, self.port) {
            return Err(SubscriptionError::InvalidBody);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EndpointSubscription {
    pub id: Uuid,
    pub revision: Uuid,
    pub name: String,
    pub url: String,
    pub account_tier: EndpointPresetTier,
    #[serde(default)]
    pub auto_refresh: SubscriptionRefreshInterval,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_refreshed_at_unix_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_refresh_at_unix_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_refresh_error: Option<String>,
    #[serde(default)]
    pub skipped_line_count: u32,
    #[serde(default)]
    pub endpoints: Vec<SubscriptionEndpoint>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_endpoint_id: Option<String>,
    /// Retained fail-closed snapshot when an upstream refresh removes `selected_endpoint_id`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retained_missing_endpoint: Option<SubscriptionEndpoint>,
}

impl EndpointSubscription {
    pub fn active_endpoint(&self) -> Option<(&SubscriptionEndpoint, bool)> {
        if let Some(selected_id) = &self.selected_endpoint_id {
            if let Some(found) = self.endpoints.iter().find(|ep| &ep.id == selected_id) {
                return Some((found, false));
            }
            if let Some(retained) = self
                .retained_missing_endpoint
                .as_ref()
                .filter(|ep| &ep.id == selected_id)
            {
                return Some((retained, true));
            }
        }
        self.endpoints.first().map(|ep| (ep, false))
    }

    pub fn reconcile_endpoints(&mut self, parsed: ParsedSubscription, now_ms: u64) {
        let mut pinned_by_id: HashMap<String, bool> = self
            .endpoints
            .iter()
            .filter(|ep| ep.pinned)
            .map(|ep| (ep.id.clone(), true))
            .collect();
        if let Some(retained) = &self.retained_missing_endpoint
            && retained.pinned
        {
            pinned_by_id.insert(retained.id.clone(), true);
        }
        let previous_active = self.selected_endpoint_id.as_ref().and_then(|selected_id| {
            self.endpoints
                .iter()
                .find(|ep| &ep.id == selected_id)
                .cloned()
                .or_else(|| {
                    self.retained_missing_endpoint
                        .as_ref()
                        .filter(|ep| &ep.id == selected_id)
                        .cloned()
                })
        });

        let mut next_endpoints = parsed.endpoints;
        for endpoint in &mut next_endpoints {
            if pinned_by_id.get(&endpoint.id) == Some(&true) {
                endpoint.pinned = true;
            }
        }
        self.endpoints = next_endpoints;
        self.skipped_line_count = parsed.skipped_line_count;
        self.last_refreshed_at_unix_ms = Some(now_ms);
        self.next_refresh_at_unix_ms = self
            .auto_refresh
            .interval_ms()
            .map(|interval| now_ms.saturating_add(interval));
        self.last_refresh_error = None;
        self.revision = Uuid::new_v4();

        if let Some(selected_id) = &self.selected_endpoint_id {
            if self.endpoints.iter().any(|ep| &ep.id == selected_id) {
                self.retained_missing_endpoint = None;
            } else if let Some(prev) = previous_active {
                self.retained_missing_endpoint = Some(prev);
            }
        } else {
            self.selected_endpoint_id = self.endpoints.first().map(|ep| ep.id.clone());
            self.retained_missing_endpoint = None;
        }
    }

    pub fn record_refresh_failure(&mut self, error_code: &str, now_ms: u64) {
        self.last_refresh_error = Some(error_code.chars().take(64).collect());
        if self.auto_refresh != SubscriptionRefreshInterval::Off {
            self.next_refresh_at_unix_ms =
                Some(now_ms.saturating_add(SUBSCRIPTION_AUTO_REFRESH_RETRY_MS));
        }
        self.revision = Uuid::new_v4();
    }

    pub fn validate(&self) -> Result<(), SubscriptionError> {
        if self.id.is_nil() || self.revision.is_nil() {
            return Err(SubscriptionError::InvalidRequest);
        }
        if validate_subscription_name(&self.name)? != self.name {
            return Err(SubscriptionError::InvalidName);
        }
        validate_subscription_url(&self.url)?;
        if self.endpoints.len() > MAX_ENDPOINTS_PER_SUBSCRIPTION {
            return Err(SubscriptionError::Limit);
        }
        let mut ids = HashSet::with_capacity(self.endpoints.len());
        for endpoint in &self.endpoints {
            endpoint.validate()?;
            if !ids.insert(endpoint.id.as_str()) {
                return Err(SubscriptionError::InvalidBody);
            }
        }
        if let Some(retained) = &self.retained_missing_endpoint {
            retained.validate()?;
            if self.selected_endpoint_id.as_deref() != Some(retained.id.as_str()) {
                return Err(SubscriptionError::InvalidBody);
            }
        }
        if let Some(err) = &self.last_refresh_error
            && (err.is_empty()
                || err.len() > 64
                || !err
                    .bytes()
                    .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_'))
        {
            return Err(SubscriptionError::InvalidBody);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResolvedSubscriptionTarget {
    pub subscription_id: Uuid,
    pub account_tier: EndpointPresetTier,
    pub endpoint: SubscriptionEndpoint,
    pub removed_upstream: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EndpointSubscriptionCatalogFile {
    pub version: u32,
    #[serde(default)]
    pub subscriptions: Vec<EndpointSubscription>,
}

impl Default for EndpointSubscriptionCatalogFile {
    fn default() -> Self {
        Self {
            version: CURRENT_SUBSCRIPTION_CATALOG_VERSION,
            subscriptions: Vec::new(),
        }
    }
}

impl EndpointSubscriptionCatalogFile {
    pub fn validate(&self) -> Result<(), SubscriptionError> {
        if self.version == 0 || self.version > CURRENT_SUBSCRIPTION_CATALOG_VERSION {
            return Err(SubscriptionError::InvalidBody);
        }
        if self.subscriptions.len() > MAX_ENDPOINT_SUBSCRIPTIONS {
            return Err(SubscriptionError::Limit);
        }
        let mut ids = HashSet::with_capacity(self.subscriptions.len());
        for subscription in &self.subscriptions {
            subscription.validate()?;
            if !ids.insert(subscription.id) {
                return Err(SubscriptionError::InvalidBody);
            }
        }
        Ok(())
    }

    pub fn resolve_target(
        &self,
        subscription_id: Uuid,
        account_tier: Option<EndpointPresetTier>,
    ) -> Result<ResolvedSubscriptionTarget, SubscriptionError> {
        let Some(tier) = account_tier else {
            return Err(SubscriptionError::VerifyAccount);
        };
        let subscription = self
            .subscriptions
            .iter()
            .find(|sub| sub.id == subscription_id)
            .ok_or(SubscriptionError::NotFound)?;
        if subscription.account_tier != tier {
            return Err(SubscriptionError::TierMismatch);
        }
        let (endpoint, removed_upstream) = subscription
            .active_endpoint()
            .ok_or(SubscriptionError::Empty)?;
        Ok(ResolvedSubscriptionTarget {
            subscription_id: subscription.id,
            account_tier: subscription.account_tier,
            endpoint: endpoint.clone(),
            removed_upstream,
        })
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum EndpointSubscriptionCommand {
    List {
        #[serde(default)]
        known_revision: Option<String>,
    },
    Add {
        name: String,
        url: String,
        account_tier: EndpointPresetTier,
        #[serde(default)]
        auto_refresh: SubscriptionRefreshInterval,
    },
    Update {
        id: Uuid,
        name: String,
        url: String,
        account_tier: EndpointPresetTier,
        auto_refresh: SubscriptionRefreshInterval,
        expected_revision: Uuid,
    },
    Delete {
        id: Uuid,
        expected_revision: Uuid,
    },
    Refresh {
        id: Uuid,
    },
    CancelRefresh {
        id: Uuid,
    },
    SelectEndpoint {
        subscription_id: Uuid,
        endpoint_id: String,
        operation_id: Uuid,
        #[serde(default)]
        zero_trust_risk_acknowledged: bool,
    },
    Deactivate {
        operation_id: Uuid,
    },
    SetPinned {
        subscription_id: Uuid,
        endpoint_id: String,
        pinned: bool,
    },
    StartProbe {
        subscription_id: Uuid,
        #[serde(default)]
        endpoint_ids: Option<Vec<String>>,
        probe_id: Uuid,
    },
    CancelProbe {
        probe_id: Uuid,
    },
}

impl EndpointSubscriptionCommand {
    pub fn parse(request: &str) -> Result<Self, SubscriptionError> {
        if request.len() > MAX_SUBSCRIPTION_COMMAND_BYTES {
            return Err(SubscriptionError::Limit);
        }
        let value: Value =
            serde_json::from_str(request).map_err(|_| SubscriptionError::InvalidRequest)?;
        let object = value.as_object().ok_or(SubscriptionError::InvalidRequest)?;
        let allowed: &[&str] = match object.get("action").and_then(Value::as_str) {
            Some("list") => &["action", "known_revision"],
            Some("add") => &["action", "name", "url", "account_tier", "auto_refresh"],
            Some("update") => &[
                "action",
                "id",
                "name",
                "url",
                "account_tier",
                "auto_refresh",
                "expected_revision",
            ],
            Some("delete") => &["action", "id", "expected_revision"],
            Some("refresh" | "cancel_refresh") => &["action", "id"],
            Some("select_endpoint") => &[
                "action",
                "subscription_id",
                "endpoint_id",
                "operation_id",
                "zero_trust_risk_acknowledged",
            ],
            Some("deactivate") => &["action", "operation_id"],
            Some("set_pinned") => &["action", "subscription_id", "endpoint_id", "pinned"],
            Some("start_probe") => &["action", "subscription_id", "endpoint_ids", "probe_id"],
            Some("cancel_probe") => &["action", "probe_id"],
            _ => return Err(SubscriptionError::InvalidRequest),
        };
        if object.keys().any(|key| !allowed.contains(&key.as_str())) {
            return Err(SubscriptionError::InvalidRequest);
        }
        serde_json::from_value(value).map_err(|_| SubscriptionError::InvalidRequest)
    }
}

#[derive(Debug, Clone, Copy, Error, PartialEq, Eq)]
pub enum SubscriptionError {
    #[error("SUBSCRIPTION_INVALID_REQUEST")]
    InvalidRequest,
    #[error("SUBSCRIPTION_INVALID_NAME")]
    InvalidName,
    #[error("SUBSCRIPTION_INVALID_URL")]
    InvalidUrl,
    #[error("SUBSCRIPTION_EMPTY")]
    Empty,
    #[error("SUBSCRIPTION_INVALID_BODY")]
    InvalidBody,
    #[error("SUBSCRIPTION_LIMIT")]
    Limit,
    #[error("SUBSCRIPTION_NOT_FOUND")]
    NotFound,
    #[error("SUBSCRIPTION_ENDPOINT_NOT_FOUND")]
    EndpointNotFound,
    #[error("SUBSCRIPTION_STALE_REVISION")]
    StaleRevision,
    #[error("SUBSCRIPTION_TIER_MISMATCH")]
    TierMismatch,
    #[error("SUBSCRIPTION_VERIFY_ACCOUNT")]
    VerifyAccount,
    #[error("SUBSCRIPTION_ZT_RISK_REQUIRED")]
    ZeroTrustRiskRequired,
    #[error("SUBSCRIPTION_NETWORK")]
    Network,
    #[error("SUBSCRIPTION_TIMEOUT")]
    Timeout,
    #[error("SUBSCRIPTION_TOO_LARGE")]
    TooLarge,
    #[error("SUBSCRIPTION_HTTP_ERROR")]
    HttpError,
    #[error("SUBSCRIPTION_CANCELLED")]
    Cancelled,
    #[error("SUBSCRIPTION_STORAGE_UNAVAILABLE")]
    StorageUnavailable,
    /// A download would leave the active protection (WARP tunnel or Kill
    /// Switch) for the physical network. Downloads fail closed instead.
    #[error("SUBSCRIPTION_PROTECTED_PATH_UNAVAILABLE")]
    ProtectedPathUnavailable,
}

impl SubscriptionError {
    pub const fn code(self) -> &'static str {
        match self {
            Self::InvalidRequest => "SUBSCRIPTION_INVALID_REQUEST",
            Self::InvalidName => "SUBSCRIPTION_INVALID_NAME",
            Self::InvalidUrl => "SUBSCRIPTION_INVALID_URL",
            Self::Empty => "SUBSCRIPTION_EMPTY",
            Self::InvalidBody => "SUBSCRIPTION_INVALID_BODY",
            Self::Limit => "SUBSCRIPTION_LIMIT",
            Self::NotFound => "SUBSCRIPTION_NOT_FOUND",
            Self::EndpointNotFound => "SUBSCRIPTION_ENDPOINT_NOT_FOUND",
            Self::StaleRevision => "SUBSCRIPTION_STALE_REVISION",
            Self::TierMismatch => "SUBSCRIPTION_TIER_MISMATCH",
            Self::VerifyAccount => "SUBSCRIPTION_VERIFY_ACCOUNT",
            Self::ZeroTrustRiskRequired => "SUBSCRIPTION_ZT_RISK_REQUIRED",
            Self::Network => "SUBSCRIPTION_NETWORK",
            Self::Timeout => "SUBSCRIPTION_TIMEOUT",
            Self::TooLarge => "SUBSCRIPTION_TOO_LARGE",
            Self::HttpError => "SUBSCRIPTION_HTTP_ERROR",
            Self::Cancelled => "SUBSCRIPTION_CANCELLED",
            Self::StorageUnavailable => "SUBSCRIPTION_STORAGE_UNAVAILABLE",
            Self::ProtectedPathUnavailable => "SUBSCRIPTION_PROTECTED_PATH_UNAVAILABLE",
        }
    }
}

impl From<StoreError> for SubscriptionError {
    fn from(_: StoreError) -> Self {
        Self::StorageUnavailable
    }
}

#[derive(Debug, Clone)]
pub struct EndpointSubscriptionStore {
    path: PathBuf,
}

impl EndpointSubscriptionStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn in_directory(directory: &Path) -> Self {
        Self::new(directory.join(ENDPOINT_SUBSCRIPTIONS_FILENAME))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn lock_exclusive(&self) -> Result<File, StoreError> {
        let parent = self
            .path
            .parent()
            .ok_or_else(|| StoreError::MissingParent(self.path.clone()))?;
        fs::create_dir_all(parent)?;
        let lock = File::options()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(self.path.with_extension("json.lock"))?;
        crate::storage::lock_exclusive_file(&lock)?;
        Ok(lock)
    }

    pub fn load_or_default(&self) -> Result<EndpointSubscriptionCatalogFile, StoreError> {
        if !self.path.exists() {
            return Ok(EndpointSubscriptionCatalogFile::default());
        }
        let file = File::open(&self.path)?;
        let catalog: EndpointSubscriptionCatalogFile =
            serde_json::from_reader(BufReader::new(file))?;
        catalog
            .validate()
            .map_err(|err| StoreError::NetworkSettings(err.code().to_owned()))?;
        Ok(catalog)
    }

    pub fn save(&self, catalog: &EndpointSubscriptionCatalogFile) -> Result<(), StoreError> {
        catalog
            .validate()
            .map_err(|err| StoreError::NetworkSettings(err.code().to_owned()))?;
        let parent = self
            .path
            .parent()
            .ok_or_else(|| StoreError::MissingParent(self.path.clone()))?;
        fs::create_dir_all(parent)?;
        let mut temporary = NamedTempFile::new_in(parent)?;
        {
            let mut writer = BufWriter::new(temporary.as_file_mut());
            serde_json::to_writer_pretty(&mut writer, catalog)?;
            writer.write_all(b"\n")?;
            writer.flush()?;
        }
        temporary.as_file().sync_all()?;
        crate::storage::replace_file_atomic(temporary.path(), &self.path)?;
        let _ = temporary.keep();
        crate::storage::sync_parent_dir(parent).map_err(StoreError::CommitUncertain)?;
        Ok(())
    }

    pub fn update<T, E>(
        &self,
        change: impl FnOnce(&mut EndpointSubscriptionCatalogFile) -> Result<T, E>,
    ) -> Result<(EndpointSubscriptionCatalogFile, T), E>
    where
        E: From<StoreError>,
    {
        let _lock = self.lock_exclusive()?;
        let mut catalog = self.load_or_default()?;
        let result = change(&mut catalog)?;
        self.save(&catalog)?;
        Ok((catalog, result))
    }

    pub fn clear(&self) -> Result<(), StoreError> {
        let _lock = self.lock_exclusive()?;
        match fs::remove_file(&self.path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        }
    }
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

pub fn catalog_revision(catalog: &EndpointSubscriptionCatalogFile) -> String {
    let bytes = serde_json::to_vec(catalog).unwrap_or_default();
    sha256_hex(&bytes)
}
