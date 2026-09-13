//! Version-pinned cumulative pool envelopes. Configuration bytes are fetched lazily.
use super::*;

pub const MAX_INDEX_BYTES: usize = 64 * 1024;
pub const MAX_CONFIG_JSON_BYTES: usize = 256 * 1024;
pub(super) const SERVERS_PATH: &str = "pool/servers.json";
pub(super) const COUNTRIES_PATH: &str = "pool/countries.json";
const SOURCE: &str = "https://www.vpngate.net/api/iphone/";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PoolMetadata {
    pub first_seen_at: String,
    pub last_seen_at: String,
    pub present_in_latest_source: bool,
    pub tcp_status: String,
    pub tcp_checked_at: Option<String>,
    pub tcp_connect_ms: Option<u64>,
    pub in_pool: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct FileDescriptor {
    pub bytes: usize,
    pub sha256: String,
}
impl FileDescriptor {
    pub fn validate(&self, limit: usize) -> Result<(), DirectoryError> {
        if self.bytes == 0 || self.bytes > limit {
            return Err(DirectoryError::SizeLimit);
        }
        if !valid_hash(&self.sha256) {
            return Err(DirectoryError::InvalidDirectory);
        }
        Ok(())
    }
    pub fn verify(&self, bytes: &[u8], limit: usize) -> Result<(), DirectoryError> {
        self.validate(limit)?;
        if bytes.len() != self.bytes || hash(bytes) != self.sha256 {
            return Err(DirectoryError::InvalidDirectory);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolIndex {
    kind: String,
    schema_version: u32,
    pub data_commit: String,
    pub source_fetched_at: String,
    generated_at: String,
    pub index_generated_at: String,
    source_url: String,
    server_count: usize,
    pub(super) files: BTreeMap<String, FileDescriptor>,
}
pub(super) fn timestamp(value: &str) -> Result<i64, DirectoryError> {
    if value.len() != 24 {
        return Err(DirectoryError::InvalidDirectory);
    }
    let time = chrono::DateTime::parse_from_rfc3339(value)
        .map_err(|_| DirectoryError::InvalidDirectory)?;
    if time.to_rfc3339_opts(chrono::SecondsFormat::Millis, true) != value {
        return Err(DirectoryError::InvalidDirectory);
    }
    Ok(time.timestamp_millis())
}
pub(super) fn valid_commit(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}
impl PoolIndex {
    pub fn parse(bytes: &[u8]) -> Result<Self, DirectoryError> {
        if bytes.len() > MAX_INDEX_BYTES {
            return Err(DirectoryError::SizeLimit);
        }
        let index: Self =
            serde_json::from_slice(bytes).map_err(|_| DirectoryError::InvalidDirectory)?;
        index.validate()?;
        Ok(index)
    }
    fn validate(&self) -> Result<(), DirectoryError> {
        if self.schema_version != 1 {
            return Err(DirectoryError::UnsupportedVersion);
        }
        if self.kind != "vpngate-pool"
            || self.source_url != SOURCE
            || !valid_commit(&self.data_commit)
            || self.server_count > MAX_SERVERS
        {
            return Err(DirectoryError::InvalidDirectory);
        }
        let source = timestamp(&self.source_fetched_at)?;
        let generated = timestamp(&self.generated_at)?;
        if generated < source || timestamp(&self.index_generated_at)? < generated {
            return Err(DirectoryError::InvalidDirectory);
        }
        for path in [SERVERS_PATH, COUNTRIES_PATH] {
            self.files
                .get(path)
                .ok_or(DirectoryError::InvalidDirectory)?
                .validate(MAX_DIRECTORY_BYTES)?;
        }
        Ok(())
    }
    pub fn not_older_than(&self, previous: &Self) -> Result<(), DirectoryError> {
        if timestamp(&self.source_fetched_at)? < timestamp(&previous.source_fetched_at)?
            || timestamp(&self.index_generated_at)? < timestamp(&previous.index_generated_at)?
        {
            return Err(DirectoryError::InvalidDirectory);
        }
        Ok(())
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct PoolServer {
    #[serde(flatten)]
    pub wire: WireServer,
    config: ConfigDescriptor,
    first_seen_at: String,
    last_seen_at: String,
    present_in_latest_source: bool,
    probe_reason: String,
    probe_targets: Vec<Endpoint>,
    tcp_probe: TcpProbe,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Endpoint {
    ip: String,
    port: u16,
}
#[derive(Clone, Serialize, Deserialize)]
struct ConfigDescriptor {
    path: String,
    #[serde(flatten)]
    file: FileDescriptor,
}
#[derive(Clone, Serialize, Deserialize)]
struct TcpProbe {
    status: String,
    probe_source: String,
    #[serde(deserialize_with = "required_nullable")]
    checked_at: Option<String>,
    #[serde(deserialize_with = "required_nullable")]
    last_success_at: Option<String>,
    #[serde(deserialize_with = "required_nullable")]
    connect_ms: Option<u64>,
    #[serde(deserialize_with = "required_nullable")]
    connected_endpoint: Option<Endpoint>,
    #[serde(deserialize_with = "required_nullable")]
    round: Option<u64>,
    #[serde(deserialize_with = "required_nullable")]
    worker_id: Option<u8>,
    #[serde(deserialize_with = "required_nullable")]
    last_failure_round: Option<u64>,
    consecutive_failures: u64,
}
impl PoolServer {
    pub fn config_path(&self) -> &str {
        &self.config.path
    }
    pub fn summary(&self) -> Result<ServerSummary, DirectoryError> {
        let ip = validate_server_metadata(&self.wire)?;
        if !self.wire.openvpn_config_base64.is_empty()
            || self.config.path != format!("pool/configs/{}.json", self.wire.openvpn_config_sha256)
            || timestamp(&self.first_seen_at)? > timestamp(&self.last_seen_at)?
            || self.probe_targets.len() > 8
            || self
                .probe_targets
                .iter()
                .any(|e| e.ip != self.wire.ip || e.port == 0)
            || !matches!(
                self.probe_reason.as_str(),
                "tcp"
                    | "udp"
                    | "non_public_ip"
                    | "complex_configuration"
                    | "ambiguous_endpoint"
                    | "ambiguous_protocol"
            )
            || !matches!(
                self.tcp_probe.status.as_str(),
                "reachable" | "unreachable" | "unknown" | "not_applicable"
            )
            || self.tcp_probe.probe_source != "cloudflare_workers"
            || self.tcp_probe.consecutive_failures > 10_000
            || self.tcp_probe.round.is_some_and(|n| n > 1_000_000_000)
            || self
                .tcp_probe
                .last_failure_round
                .is_some_and(|n| n > 1_000_000_000)
            || self.tcp_probe.worker_id.is_some_and(|n| n > 1)
            || self.tcp_probe.connect_ms.is_some_and(|n| n > 3000)
            || self
                .tcp_probe
                .connected_endpoint
                .as_ref()
                .is_some_and(|e| e.ip != self.wire.ip || e.port == 0)
        {
            return Err(DirectoryError::InvalidDirectory);
        }
        self.config.file.validate(MAX_CONFIG_JSON_BYTES)?;
        for time in [&self.tcp_probe.checked_at, &self.tcp_probe.last_success_at]
            .into_iter()
            .flatten()
        {
            timestamp(time)?;
        }
        let unsupported = match self.probe_reason.as_str() {
            "tcp" if !self.probe_targets.is_empty() => None,
            "udp" => Some(UnsupportedReason::UdpTransport),
            _ => Some(UnsupportedReason::InvalidEndpoint),
        };
        let mut summary = server_summary(&self.wire, ip, unsupported);
        summary.pool = Some(PoolMetadata {
            first_seen_at: self.first_seen_at.clone(),
            last_seen_at: self.last_seen_at.clone(),
            present_in_latest_source: self.present_in_latest_source,
            tcp_status: self.tcp_probe.status.clone(),
            tcp_checked_at: self.tcp_probe.checked_at.clone(),
            tcp_connect_ms: self.tcp_probe.connect_ms,
            in_pool: true,
        });
        Ok(summary)
    }
    pub fn configuration(
        &self,
        bytes: &[u8],
    ) -> Result<(ServerSummary, WireServer), DirectoryError> {
        self.config.file.verify(bytes, MAX_CONFIG_JSON_BYTES)?;
        let config: ConfigDocument =
            serde_json::from_slice(bytes).map_err(|_| DirectoryError::InvalidDirectory)?;
        if config.schema_version != 1 {
            return Err(DirectoryError::UnsupportedVersion);
        }
        if config.kind != "vpngate-pool-config"
            || config.openvpn_config_sha256 != self.wire.openvpn_config_sha256
            || config.openvpn_config_bytes != self.wire.openvpn_config_bytes
        {
            return Err(DirectoryError::InvalidDirectory);
        }
        let mut wire = self.wire.clone();
        wire.openvpn_config_base64 = config.openvpn_config_base64;
        let (validated, _) = validate_server(&wire)?;
        if validated.unsupported_reason.is_some() {
            return Err(DirectoryError::UnsupportedProfile);
        }
        let mut summary = self.summary()?;
        summary.unsupported_reason = None;
        Ok((summary, wire))
    }
}
#[derive(Serialize, Deserialize)]
struct ConfigDocument {
    kind: String,
    schema_version: u32,
    openvpn_config_base64: String,
    openvpn_config_sha256: String,
    openvpn_config_bytes: usize,
}
#[derive(Deserialize)]
struct ServersDocument {
    kind: String,
    schema_version: u32,
    source_fetched_at: String,
    server_count: usize,
    servers: Vec<PoolServer>,
}
#[derive(Deserialize)]
struct CountriesDocument {
    kind: String,
    schema_version: u32,
    source_fetched_at: String,
    server_count: usize,
    countries: Vec<PoolCountry>,
}
#[derive(Deserialize)]
struct PoolCountry {
    #[serde(deserialize_with = "required_nullable")]
    code: Option<String>,
    names: Vec<String>,
    server_count: usize,
}

#[derive(Clone)]
pub struct PoolCatalogue {
    pub index: PoolIndex,
    pub(super) nodes: Vec<PoolServer>,
    servers_json: String,
    countries_json: String,
}
impl PoolCatalogue {
    pub fn parse(
        index: PoolIndex,
        servers: &[u8],
        countries: &[u8],
    ) -> Result<Self, DirectoryError> {
        index.validate()?;
        index.files[SERVERS_PATH].verify(servers, MAX_DIRECTORY_BYTES)?;
        index.files[COUNTRIES_PATH].verify(countries, MAX_DIRECTORY_BYTES)?;
        let nodes: ServersDocument =
            serde_json::from_slice(servers).map_err(|_| DirectoryError::InvalidDirectory)?;
        let regions: CountriesDocument =
            serde_json::from_slice(countries).map_err(|_| DirectoryError::InvalidDirectory)?;
        if nodes.schema_version != 1 || regions.schema_version != 1 {
            return Err(DirectoryError::UnsupportedVersion);
        }
        if nodes.kind != "vpngate-pool"
            || regions.kind != "vpngate-pool"
            || nodes.source_fetched_at != index.source_fetched_at
            || regions.source_fetched_at != index.source_fetched_at
            || nodes.server_count != index.server_count
            || regions.server_count != index.server_count
            || nodes.servers.len() != index.server_count
            || regions.countries.len() > MAX_SERVERS
        {
            return Err(DirectoryError::InvalidDirectory);
        }
        let mut ids = HashSet::new();
        let mut counts =
            BTreeMap::<Option<String>, (usize, std::collections::BTreeSet<String>)>::new();
        for node in &nodes.servers {
            let summary = node.summary()?;
            if !ids.insert(&node.wire.id)
                || timestamp(&node.last_seen_at)? > timestamp(&index.source_fetched_at)?
            {
                return Err(DirectoryError::InvalidDirectory);
            }
            let entry = counts.entry(summary.country_code).or_default();
            entry.0 += 1;
            if let Some(name) = summary.country_name {
                entry.1.insert(name);
            }
        }
        for region in &regions.countries {
            let names = region
                .names
                .iter()
                .cloned()
                .collect::<std::collections::BTreeSet<_>>();
            if names.len() != region.names.len()
                || counts.remove(&region.code) != Some((region.server_count, names))
            {
                return Err(DirectoryError::InvalidDirectory);
            }
        }
        if !counts.is_empty() {
            return Err(DirectoryError::InvalidDirectory);
        }
        Ok(Self {
            index,
            nodes: nodes.servers,
            servers_json: String::from_utf8(servers.to_vec())
                .map_err(|_| DirectoryError::InvalidDirectory)?,
            countries_json: String::from_utf8(countries.to_vec())
                .map_err(|_| DirectoryError::InvalidDirectory)?,
        })
    }
    pub(super) fn selected(&self, selection: &Selection) -> Result<&PoolServer, DirectoryError> {
        self.nodes
            .iter()
            .find(|s| {
                s.wire.id == selection.server_id
                    && s.wire.openvpn_config_sha256 == selection.config_sha256
            })
            .ok_or(DirectoryError::StaleSelection)
    }
}
#[derive(Serialize, Deserialize)]
struct PoolCache {
    version: u32,
    index: PoolIndex,
    servers_json: String,
    countries_json: String,
    source_url: String,
    fetched_at_unix_ms: u64,
    skip_raw: bool,
}
impl CatalogueStore {
    pub(super) fn load_pool(
        &self,
    ) -> Result<Option<(PoolCatalogue, u64, String, bool)>, DirectoryError> {
        let path = self.directory.join("pool-cache.json");
        let bytes = match bounded_read(&path, MAX_DIRECTORY_BYTES * 4 + MAX_INDEX_BYTES * 2) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err(DirectoryError::CacheIo),
        };
        let cache: PoolCache =
            serde_json::from_slice(&bytes).map_err(|_| DirectoryError::InvalidDirectory)?;
        if cache.version != 2 || !approved_url(&cache.source_url) {
            return Err(DirectoryError::InvalidDirectory);
        }
        Ok(Some((
            PoolCatalogue::parse(
                cache.index,
                cache.servers_json.as_bytes(),
                cache.countries_json.as_bytes(),
            )?,
            cache.fetched_at_unix_ms,
            cache.source_url,
            cache.skip_raw,
        )))
    }
    pub(super) fn save_pool(
        &self,
        pool: &PoolCatalogue,
        time: u64,
        source: &str,
        skip_raw: bool,
    ) -> Result<(), DirectoryError> {
        if !approved_url(source) {
            return Err(DirectoryError::InvalidDirectory);
        }
        if let Ok(Some((previous, ..))) = self.load_pool() {
            pool.index.not_older_than(&previous.index)?;
        }
        atomic_json(
            &self.directory.join("pool-cache.json"),
            &PoolCache {
                version: 2,
                index: pool.index.clone(),
                servers_json: pool.servers_json.clone(),
                countries_json: pool.countries_json.clone(),
                source_url: source.into(),
                fetched_at_unix_ms: time,
                skip_raw,
            },
        )
    }
}
