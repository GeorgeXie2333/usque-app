//! Public directory validation and pinned VPN Gate selection. Downloaded
//! configuration bytes remain in the engine; UI responses contain metadata.
mod download;
mod favorites;
mod pool;
mod profile;
mod transfer;
pub use favorites::{FavoriteMetadata, NodeAction, NodeProgress, NodeRequest};
pub use pool::{MAX_CONFIG_JSON_BYTES, MAX_INDEX_BYTES, PoolCatalogue, PoolIndex, PoolMetadata};
#[cfg(test)]
mod pool_tests;
#[cfg(test)]
mod tests;
pub use download::{
    CDN_HOSTS, CDN_PATH, CONNECT_TIMEOUT, CatalogueHttp, DirectCatalogueHttp, DirectoryDownloader,
    DownloadProgress, DownloadStage, RAW_URL, RESPONSE_TIMEOUT, StageFailure, WarpCatalogueSource,
    approved_url, response_limit,
};
pub use profile::{MAX_CONFIG_BYTES, PreparedProfile, UnsupportedReason, prepare_profile};

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashSet};
use std::fmt;
use std::io::{Read, Write};
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use zeroize::Zeroizing;

pub const MAX_DIRECTORY_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_SERVERS: usize = 5000;
const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;

#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum DirectoryError {
    #[error("VPN Gate directory exceeds its size limit")]
    SizeLimit,
    #[error("VPN Gate directory has an invalid schema or checksum")]
    InvalidDirectory,
    #[error("VPN Gate mirror schema version is unsupported")]
    UnsupportedVersion,
    #[error("VPN Gate selection changed; refresh and select the server again")]
    StaleSelection,
    #[error("VPN Gate server configuration is unsupported")]
    UnsupportedProfile,
    #[error("VPN Gate cache could not be read or written")]
    CacheIo,
    #[error("VPN Gate directory request failed")]
    Request,
    #[error("VPN Gate directory request timed out")]
    Timeout,
    #[error("VPN Gate directory refresh was cancelled")]
    Cancelled,
    #[error("VPN Gate directory sources and WARP fallback are unavailable")]
    Unavailable,
    #[error("VPN Gate favorite changed; reload before updating it")]
    FavoriteChanged,
    #[error("VPN Gate favorites storage is full")]
    FavoriteLimit,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct VpnGateSettings {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub selection: Option<Selection>,
}
impl VpnGateSettings {
    pub fn validate(&self) -> Result<(), DirectoryError> {
        if self.enabled && self.selection.is_none() {
            return Err(DirectoryError::StaleSelection);
        }
        if let Some(selection) = &self.selection
            && (!valid_id(&selection.server_id) || !valid_hash(&selection.config_sha256))
        {
            return Err(DirectoryError::StaleSelection);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Selection {
    pub server_id: String,
    pub config_sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FinalNetworkParameters {
    pub ipv4: Option<std::net::Ipv4Addr>,
    pub ipv6: Option<std::net::Ipv6Addr>,
    pub dns_servers: Vec<IpAddr>,
    pub mtu: u16,
}
impl FinalNetworkParameters {
    pub fn supports(&self, ip: IpAddr) -> bool {
        if ip.is_ipv4() {
            self.ipv4.is_some()
        } else {
            self.ipv6.is_some()
        }
    }
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GateStage {
    #[default]
    Disabled,
    ConnectingWarp,
    ConnectingServer,
    Negotiating,
    ConfiguringNetwork,
    Connected,
    Reconnecting,
    Error,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GateFailure {
    Transport,
    Authentication,
    Certificate,
    Configuration,
    AddressChanged,
}
impl GateFailure {
    pub fn retryable(self) -> bool {
        matches!(self, Self::Transport | Self::AddressChanged)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct GateStatus {
    pub stage: GateStage,
    pub generation: u64,
    pub current_server: Option<ServerSummary>,
    pub network: Option<FinalNetworkParameters>,
    pub failure: Option<GateFailure>,
    #[serde(default)]
    pub warp_stage: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ServerSummary {
    pub id: String,
    pub hostname: String,
    pub ip: IpAddr,
    pub country_code: Option<String>,
    pub country_name: Option<String>,
    pub score: Option<u64>,
    pub ping_ms: Option<u64>,
    pub speed_bps: Option<u64>,
    pub num_vpn_sessions: Option<u64>,
    pub config_sha256: String,
    pub unsupported_reason: Option<UnsupportedReason>,
    #[serde(default)]
    pub pool: Option<PoolMetadata>,
    #[serde(default)]
    pub favorite: Option<FavoriteMetadata>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CountrySummary {
    pub country_code: Option<String>,
    pub country_name: Option<String>,
    pub server_count: usize,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ListQuery {
    pub country_code: Option<String>,
    #[serde(default)]
    pub unknown_country: bool,
    #[serde(default)]
    pub offset: usize,
    #[serde(default)]
    pub limit: usize,
    #[serde(default)]
    pub include_unsupported: bool,
    #[serde(default)]
    pub favorites_only: bool,
    #[serde(default)]
    pub status_only: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ServerList {
    pub servers: Vec<ServerSummary>,
    pub countries: Vec<CountrySummary>,
    pub total: usize,
    pub source_server_count: usize,
    pub fetched_at_unix_ms: Option<u64>,
    pub source_url: Option<String>,
    pub source_fetched_at: Option<String>,
    pub favorite_count: usize,
}

#[derive(Clone, Serialize, Deserialize)]
struct WireServer {
    id: String,
    hostname: String,
    ip: String,
    #[serde(deserialize_with = "required_nullable")]
    country_code: Option<String>,
    #[serde(deserialize_with = "required_nullable")]
    country_name: Option<String>,
    #[serde(deserialize_with = "required_nullable")]
    score: Option<u64>,
    #[serde(deserialize_with = "required_nullable")]
    ping_ms: Option<u64>,
    #[serde(deserialize_with = "required_nullable")]
    speed_bps: Option<u64>,
    #[serde(deserialize_with = "required_nullable")]
    num_vpn_sessions: Option<u64>,
    #[serde(default)]
    openvpn_config_base64: String,
    openvpn_config_sha256: String,
    openvpn_config_bytes: usize,
}
// The mirror requires these keys even when their value is null. Plain Option
// deserialization would silently accept a missing required key as None.
fn required_nullable<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer)
}
impl fmt::Debug for WireServer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WireServer")
            .field("id", &self.id)
            .field("configuration", &"[redacted]")
            .finish()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct WireDirectory {
    schema_version: u32,
    source_csv_sha256: String,
    server_count: usize,
    servers: Vec<WireServer>,
}

#[derive(Clone)]
pub struct Catalogue {
    document: WireDirectory,
    servers: Vec<ServerSummary>,
}
impl Catalogue {
    pub fn parse(bytes: &[u8]) -> Result<Self, DirectoryError> {
        if bytes.len() > MAX_DIRECTORY_BYTES {
            return Err(DirectoryError::SizeLimit);
        }
        let document: WireDirectory =
            serde_json::from_slice(bytes).map_err(|_| DirectoryError::InvalidDirectory)?;
        Self::validate(document)
    }
    fn validate(document: WireDirectory) -> Result<Self, DirectoryError> {
        if document.schema_version != 1 {
            return Err(DirectoryError::UnsupportedVersion);
        }
        if document.server_count > MAX_SERVERS {
            return Err(DirectoryError::SizeLimit);
        }
        if document.server_count == 0
            || document.server_count != document.servers.len()
            || !valid_hash(&document.source_csv_sha256)
        {
            return Err(DirectoryError::InvalidDirectory);
        }
        let mut ids = HashSet::new();
        let mut servers = Vec::with_capacity(document.server_count);
        for server in &document.servers {
            if !ids.insert(&server.id) {
                return Err(DirectoryError::InvalidDirectory);
            }
            servers.push(validate_server(server)?.0);
        }
        servers.sort_by(|a, b| b.score.cmp(&a.score).then_with(|| a.id.cmp(&b.id)));
        Ok(Self { document, servers })
    }
    pub fn list(&self, query: &ListQuery) -> ServerList {
        let supported: Vec<_> = self
            .servers
            .iter()
            .filter(|s| query.include_unsupported || s.unsupported_reason.is_none())
            .collect();
        let mut countries = BTreeMap::<Option<String>, CountrySummary>::new();
        for server in &supported {
            let country = countries
                .entry(server.country_code.clone())
                .or_insert_with(|| CountrySummary {
                    country_code: server.country_code.clone(),
                    country_name: server.country_name.clone(),
                    server_count: 0,
                });
            country.server_count += 1;
        }
        let filtered: Vec<_> = supported
            .into_iter()
            .filter(|s| {
                if query.unknown_country {
                    s.country_code.is_none()
                } else {
                    query
                        .country_code
                        .as_ref()
                        .is_none_or(|code| s.country_code.as_ref() == Some(code))
                }
            })
            .collect();
        let limit = if query.limit == 0 {
            100
        } else {
            query.limit.min(100)
        };
        ServerList {
            total: filtered.len(),
            source_server_count: self.document.server_count,
            servers: filtered
                .into_iter()
                .skip(query.offset)
                .take(limit)
                .cloned()
                .collect(),
            countries: countries.into_values().collect(),
            ..Default::default()
        }
    }
    /// Resolves exactly the requested directory version without writing a pin.
    pub fn prepare(
        &self,
        selection: &Selection,
    ) -> Result<(ServerSummary, PreparedProfile), DirectoryError> {
        let (summary, content) = validate_server(self.selected(selection)?)?;
        let profile = prepare_profile(&content, summary.ip)
            .map_err(|_| DirectoryError::UnsupportedProfile)?;
        Ok((summary, profile))
    }
    fn selected(&self, selection: &Selection) -> Result<&WireServer, DirectoryError> {
        let server = self
            .document
            .servers
            .iter()
            .find(|s| {
                s.id == selection.server_id && s.openvpn_config_sha256 == selection.config_sha256
            })
            .ok_or(DirectoryError::StaleSelection)?;
        if validate_server(server)?.0.unsupported_reason.is_some() {
            return Err(DirectoryError::UnsupportedProfile);
        }
        Ok(server)
    }
}

fn validate_server(
    server: &WireServer,
) -> Result<(ServerSummary, Zeroizing<String>), DirectoryError> {
    let ip = validate_server_metadata(server)?;
    let invalid = DirectoryError::InvalidDirectory;
    let decoded = Zeroizing::new(
        STANDARD
            .decode(&server.openvpn_config_base64)
            .map_err(|_| invalid.clone())?,
    );
    if decoded.len() != server.openvpn_config_bytes
        || STANDARD.encode(&decoded) != server.openvpn_config_base64
        || hash(&decoded) != server.openvpn_config_sha256
    {
        return Err(invalid);
    }
    let content = Zeroizing::new(
        std::str::from_utf8(&decoded)
            .map_err(|_| invalid.clone())?
            .to_owned(),
    );
    let unsupported_reason = prepare_profile(&content, ip).err();
    Ok((server_summary(server, ip, unsupported_reason), content))
}

fn validate_server_metadata(server: &WireServer) -> Result<IpAddr, DirectoryError> {
    let invalid = DirectoryError::InvalidDirectory;
    let ip: IpAddr = server.ip.parse().map_err(|_| invalid.clone())?;
    if server.ip != ip.to_string()
        || ip.is_unspecified()
        || ip.is_loopback()
        || ip.is_multicast()
        || server.hostname.is_empty()
        || server.hostname.len() > 253
        || server.hostname.chars().any(char::is_control)
        || !valid_id(&server.id)
        || !valid_hash(&server.openvpn_config_sha256)
        || server.openvpn_config_base64.len() > MAX_CONFIG_BYTES.div_ceil(3) * 4
        || server.openvpn_config_bytes == 0
        || server.openvpn_config_bytes > MAX_CONFIG_BYTES
    {
        return Err(invalid);
    }
    let hostname = server
        .hostname
        .trim()
        .trim_end_matches('.')
        .to_ascii_lowercase();
    if hostname.is_empty()
        || hostname.split('.').any(|label| {
            label.is_empty()
                || label.len() > 63
                || !label
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || c == b'-')
                || label.starts_with('-')
                || label.ends_with('-')
        })
    {
        return Err(invalid);
    }
    let expected_id = format!(
        "v1:{}",
        hash(format!("vpngate-node-v1\0{hostname}\0{ip}").as_bytes())
    );
    if server.id != expected_id {
        return Err(invalid);
    }
    if server.country_code.as_ref().is_some_and(|s| {
        s.len() != 2
            || !s.bytes().all(|c| c.is_ascii_uppercase())
            || matches!(s.as_str(), "XX" | "ZZ")
    }) || server
        .country_name
        .as_ref()
        .is_some_and(|s| s.trim().is_empty() || s.len() > 256 || s.chars().any(char::is_control))
        || [
            server.score,
            server.ping_ms,
            server.speed_bps,
            server.num_vpn_sessions,
        ]
        .into_iter()
        .flatten()
        .any(|v| v > MAX_SAFE_INTEGER)
    {
        return Err(invalid);
    }
    Ok(ip)
}

fn server_summary(
    server: &WireServer,
    ip: IpAddr,
    unsupported_reason: Option<UnsupportedReason>,
) -> ServerSummary {
    ServerSummary {
        id: server.id.clone(),
        hostname: server.hostname.clone(),
        ip,
        country_code: server.country_code.clone(),
        country_name: server.country_name.clone(),
        score: server.score,
        ping_ms: server.ping_ms,
        speed_bps: server.speed_bps,
        num_vpn_sessions: server.num_vpn_sessions,
        config_sha256: server.openvpn_config_sha256.clone(),
        unsupported_reason,
        pool: None,
        favorite: None,
    }
}

pub fn valid_hash(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}
fn valid_id(value: &str) -> bool {
    value.strip_prefix("v1:").is_some_and(valid_hash)
}
fn hash(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[derive(Serialize, Deserialize)]
struct CacheRecord {
    version: u32,
    fetched_at_unix_ms: u64,
    source_url: String,
    document: WireDirectory,
}

#[derive(Clone)]
pub struct CatalogueStore {
    directory: PathBuf,
}
impl CatalogueStore {
    pub fn new(cache_directory: &Path) -> Self {
        Self {
            directory: cache_directory.join("vpngate"),
        }
    }
    pub fn load(&self) -> Result<Option<(Catalogue, u64, String)>, DirectoryError> {
        let path = self.directory.join("directory.json");
        let bytes = match bounded_read(&path, MAX_DIRECTORY_BYTES + 4096) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err(DirectoryError::CacheIo),
        };
        let record: CacheRecord =
            serde_json::from_slice(&bytes).map_err(|_| DirectoryError::InvalidDirectory)?;
        if record.version != 1 {
            return Err(DirectoryError::UnsupportedVersion);
        }
        if !download::legacy_cache_url(&record.source_url) {
            return Err(DirectoryError::InvalidDirectory);
        }
        Ok(Some((
            Catalogue::validate(record.document)?,
            record.fetched_at_unix_ms,
            record.source_url,
        )))
    }
    pub fn list(&self, query: &ListQuery) -> Result<ServerList, DirectoryError> {
        self.list_with_favorites(query)
    }
    pub fn save(
        &self,
        catalogue: &Catalogue,
        fetched_at_unix_ms: u64,
        source_url: &str,
    ) -> Result<(), DirectoryError> {
        if !approved_url(source_url) {
            return Err(DirectoryError::InvalidDirectory);
        }
        let record = CacheRecord {
            version: 1,
            fetched_at_unix_ms,
            source_url: source_url.to_owned(),
            document: catalogue.document.clone(),
        };
        atomic_json(&self.directory.join("directory.json"), &record)
    }
    pub fn pin(&self, selection: &Selection) -> Result<ServerSummary, DirectoryError> {
        let _guard = favorites::STORE_WRITE
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if !valid_id(&selection.server_id) || !valid_hash(&selection.config_sha256) {
            return Err(DirectoryError::StaleSelection);
        }
        if let Ok(node) = self.local_node(selection) {
            let summary = self.validate_node(&node)?.0;
            atomic_json(&self.node_path("selected", selection), &node)?;
            let _ = std::fs::remove_file(self.node_path("prepared", selection));
            return Ok(summary);
        }
        let (catalogue, _, _) = self.load()?.ok_or(DirectoryError::StaleSelection)?;
        let (summary, _) = catalogue.prepare(selection)?;
        let server = catalogue.selected(selection)?;
        atomic_json(&self.selection_path(selection), server)?;
        Ok(summary)
    }
    pub fn load_selection(
        &self,
        selection: &Selection,
    ) -> Result<(ServerSummary, PreparedProfile), DirectoryError> {
        if !valid_id(&selection.server_id) || !valid_hash(&selection.config_sha256) {
            return Err(DirectoryError::StaleSelection);
        }
        if let Ok(node) = self.local_node(selection) {
            return self.validate_node(&node);
        }
        let bytes = bounded_read(&self.selection_path(selection), 256 * 1024)
            .map_err(|_| DirectoryError::CacheIo)?;
        let server: WireServer =
            serde_json::from_slice(&bytes).map_err(|_| DirectoryError::InvalidDirectory)?;
        if server.id != selection.server_id
            || server.openvpn_config_sha256 != selection.config_sha256
        {
            return Err(DirectoryError::StaleSelection);
        }
        let (summary, content) = validate_server(&server)?;
        let profile = prepare_profile(&content, summary.ip)
            .map_err(|_| DirectoryError::UnsupportedProfile)?;
        Ok((summary, profile))
    }
    fn selection_path(&self, selection: &Selection) -> PathBuf {
        self.directory.join("selected").join(format!(
            "{}-{}.json",
            &selection.server_id[3..],
            selection.config_sha256
        ))
    }
}

fn bounded_read(path: &Path, max: usize) -> std::io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(max as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > max {
        return Err(std::io::Error::other("cache size limit"));
    }
    Ok(bytes)
}
fn atomic_json(path: &Path, value: &impl Serialize) -> Result<(), DirectoryError> {
    let parent = path.parent().ok_or(DirectoryError::CacheIo)?;
    std::fs::create_dir_all(parent).map_err(|_| DirectoryError::CacheIo)?;
    let mut file = tempfile::NamedTempFile::new_in(parent).map_err(|_| DirectoryError::CacheIo)?;
    serde_json::to_writer(file.as_file_mut(), value).map_err(|_| DirectoryError::CacheIo)?;
    file.flush()
        .and_then(|_| file.as_file().sync_all())
        .map_err(|_| DirectoryError::CacheIo)?;
    file.persist(path).map_err(|_| DirectoryError::CacheIo)?;
    Ok(())
}
