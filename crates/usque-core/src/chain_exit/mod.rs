//! User-owned chained exits. Secrets never belong in settings or status.
mod openvpn;
pub mod store;
#[cfg(test)]
mod tests;
mod wireguard;

use serde::{Deserialize, Serialize};
use std::fmt;
use std::net::{IpAddr, SocketAddr};
use uuid::Uuid;
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

pub use wireguard::WireGuardProfile;
pub const MAX_CONFIG_BYTES: usize = 128 * 1024;
pub const MAX_IMPORTED_PROFILES: usize = 128;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChainSource {
    #[default]
    OpenvpnCustom,
    WireguardCustom,
    WarpWireguard,
    VpnGate,
}
impl ChainSource {
    pub const DISPLAY_ORDER: [Self; 4] = [
        Self::OpenvpnCustom,
        Self::WireguardCustom,
        Self::WarpWireguard,
        Self::VpnGate,
    ];
    pub const fn is_wireguard(self) -> bool {
        matches!(self, Self::WireguardCustom | Self::WarpWireguard)
    }
    pub const fn label(self) -> &'static str {
        match self {
            Self::OpenvpnCustom => "OpenVPN",
            Self::WireguardCustom => "WireGuard",
            Self::WarpWireguard => "WARP via WireGuard",
            Self::VpnGate => "VPN Gate",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChainExitSettings {
    pub enabled: bool,
    pub source: ChainSource,
    pub profile_id: Option<Uuid>,
    pub revision: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub endpoint_override: Option<Endpoint>,
}
impl ChainExitSettings {
    pub fn validate(&self) -> Result<(), ImportError> {
        if let Some(endpoint) = &self.endpoint_override {
            if self.source != ChainSource::WarpWireguard
                || self.profile_id.is_none()
                || endpoint.address().is_none()
            {
                return Err(ImportError::new(0, "endpoint_override", "invalid_endpoint"));
            }
            Endpoint::parse(&endpoint.host, &endpoint.port.to_string(), 0)?;
        }
        if self.profile_id.is_some() != self.revision.is_some()
            || self.enabled && self.source != ChainSource::VpnGate && self.profile_id.is_none()
            || self.source == ChainSource::VpnGate && self.profile_id.is_some()
        {
            return Err(ImportError::new(0, "selection", "invalid_selection"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChainProtocol {
    OpenvpnTcp,
    OpenvpnUdp,
    Wireguard,
}
impl ChainProtocol {
    pub const fn requires_udp(self) -> bool {
        !matches!(self, Self::OpenvpnTcp)
    }
    pub const fn source(self) -> ChainSource {
        match self {
            Self::OpenvpnTcp | Self::OpenvpnUdp => ChainSource::OpenvpnCustom,
            Self::Wireguard => ChainSource::WireguardCustom,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Endpoint {
    pub host: String,
    pub port: u16,
}
impl Endpoint {
    pub fn parse(host: &str, port: &str, line: usize) -> Result<Self, ImportError> {
        let port = port
            .parse::<u16>()
            .ok()
            .filter(|p| *p != 0)
            .ok_or_else(|| ImportError::new(line, "endpoint", "invalid_endpoint"))?;
        let host = host.trim_start_matches('[').trim_end_matches(']');
        let valid = if let Ok(ip) = host.parse::<IpAddr>() {
            !ip.is_unspecified()
                && !ip.is_loopback()
                && !ip.is_multicast()
                && match ip {
                    IpAddr::V4(ip) => !ip.is_link_local() && ip.octets() != [255; 4],
                    IpAddr::V6(ip) => !ip.is_unicast_link_local(),
                }
        } else {
            !host.is_empty()
                && host.len() <= 253
                && host.is_ascii()
                && host.split('.').all(|label| {
                    !label.is_empty()
                        && label.len() <= 63
                        && !label.starts_with('-')
                        && !label.ends_with('-')
                        && label
                            .bytes()
                            .all(|c| c.is_ascii_alphanumeric() || c == b'-')
                })
        };
        if !valid {
            return Err(ImportError::new(line, "endpoint", "invalid_endpoint"));
        }
        Ok(Self {
            host: host
                .parse::<IpAddr>()
                .map_or_else(|_| host.to_ascii_lowercase(), |ip| ip.to_string()),
            port,
        })
    }
    pub fn address(&self) -> Option<SocketAddr> {
        self.host
            .parse()
            .ok()
            .map(|ip| SocketAddr::new(ip, self.port))
    }
}

/// Persist only after platform encryption; never include in diagnostics/Debug.
#[derive(Clone, Default, Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
pub struct ImportSecrets {
    #[serde(default)]
    pub configuration: String,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub password: String,
    #[serde(default)]
    pub private_key_password: String,
}
impl fmt::Debug for ImportSecrets {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ImportSecrets([redacted])")
    }
}
impl ImportSecrets {
    pub fn new(configuration: String) -> Self {
        Self {
            configuration,
            username: String::new(),
            password: String::new(),
            private_key_password: String::new(),
        }
    }
    pub fn validate(&self) -> Result<(), ImportError> {
        if self.configuration.is_empty()
            || self.configuration.len() > MAX_CONFIG_BYTES
            || self
                .configuration
                .chars()
                .any(|c| c.is_control() && !matches!(c, '\r' | '\n' | '\t'))
        {
            return Err(ImportError::new(
                0,
                "configuration",
                "invalid_size_or_encoding",
            ));
        }
        for (key, value) in [
            ("username", &self.username),
            ("password", &self.password),
            ("private_key_password", &self.private_key_password),
        ] {
            if value.len() > 2048 || value.chars().any(|c| matches!(c, '\0' | '\r' | '\n')) {
                return Err(ImportError::new(0, key, "invalid_credential"));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChainProfileSummary {
    pub id: Uuid,
    pub revision: Uuid,
    /// Changes on metadata/credential writes without changing the selected
    /// immutable protocol configuration or reconnecting a live session.
    pub edit_revision: Uuid,
    pub name: String,
    pub protocol: ChainProtocol,
    #[serde(default)]
    pub source: ChainSource,
    pub endpoint: Endpoint,
    #[serde(default)]
    pub candidates: Vec<OpenVpnEndpoint>,
    #[serde(default)]
    pub remote_random: bool,
    pub address_family: String,
    pub addresses: Vec<String>,
    pub dns_servers: Vec<IpAddr>,
    pub allowed_ips: Vec<String>,
    pub mtu: Option<u16>,
    pub requires_auth: bool,
    pub requires_key_password: bool,
}
impl ChainProfileSummary {
    pub fn selection(&self) -> ChainExitSettings {
        ChainExitSettings {
            enabled: true,
            source: self.source,
            profile_id: Some(self.id),
            revision: Some(self.revision),
            endpoint_override: None,
        }
    }
    pub fn reference_hash(&self) -> String {
        use sha2::{Digest, Sha256};
        let mut digest = Sha256::new();
        digest.update(self.revision.as_bytes());
        if self.source == ChainSource::WarpWireguard {
            digest.update(self.endpoint.host.as_bytes());
            digest.update(self.endpoint.port.to_be_bytes());
        }
        digest
            .finalize()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    }
    pub fn server_summary(&self) -> crate::vpngate::ServerSummary {
        crate::vpngate::ServerSummary {
            id: self.id.to_string(),
            hostname: self.name.clone(),
            ip: self
                .endpoint
                .address()
                .map_or(IpAddr::V4(std::net::Ipv4Addr::UNSPECIFIED), |a| a.ip()),
            country_code: None,
            country_name: None,
            score: None,
            ping_ms: None,
            speed_bps: None,
            num_vpn_sessions: None,
            config_sha256: self.reference_hash(),
            unsupported_reason: None,
            pool: None,
            favorite: None,
        }
    }
}

#[derive(Clone)]
pub struct OpenVpnProfile {
    pub endpoint: Endpoint,
    pub endpoint_ipv6: Option<bool>,
    pub protocol: ChainProtocol,
    pub content: Zeroizing<String>,
    pub requires_auth: bool,
    pub requires_key_password: bool,
    pub candidates: Vec<OpenVpnEndpoint>,
    pub remote_random: bool,
    pub client_certificate: ClientCertificateMode,
    pub mss: MssPolicy,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenVpnEndpoint {
    pub endpoint: Endpoint,
    pub ipv6: Option<bool>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientCertificateMode {
    Required,
    Disabled,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MssPolicy {
    Default,
    Disabled,
    Value { value: u16, modifier: MssModifier },
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MssModifier {
    None,
    Mtu,
    Fixed,
}
#[derive(Clone)]
pub enum ValidatedProfile {
    OpenVpn(OpenVpnProfile),
    WireGuard(WireGuardProfile),
}
impl fmt::Debug for ValidatedProfile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ValidatedProfile([redacted])")
    }
}
impl ValidatedProfile {
    pub fn parse(source: ChainSource, secrets: &ImportSecrets) -> Result<Self, ImportError> {
        secrets.validate()?;
        match source {
            ChainSource::OpenvpnCustom => openvpn::parse(&secrets.configuration).map(Self::OpenVpn),
            ChainSource::WireguardCustom | ChainSource::WarpWireguard => {
                wireguard::parse(&secrets.configuration).map(Self::WireGuard)
            }
            ChainSource::VpnGate => Err(ImportError::new(0, "source", "directory_only")),
        }
    }
    pub fn summary(
        &self,
        name: &str,
        id: Uuid,
        revision: Uuid,
    ) -> Result<ChainProfileSummary, ImportError> {
        let name = name.trim();
        if name.is_empty() || name.chars().count() > 64 || name.chars().any(char::is_control) {
            return Err(ImportError::new(0, "name", "invalid_name"));
        }
        let mut result = ChainProfileSummary {
            id,
            revision,
            edit_revision: revision,
            name: name.into(),
            protocol: ChainProtocol::OpenvpnTcp,
            source: ChainSource::OpenvpnCustom,
            endpoint: Endpoint {
                host: String::new(),
                port: 1,
            },
            address_family: "IPv4/IPv6".into(),
            addresses: vec![],
            dns_servers: vec![],
            allowed_ips: vec![],
            mtu: None,
            requires_auth: false,
            requires_key_password: false,
            candidates: vec![],
            remote_random: false,
        };
        match self {
            Self::OpenVpn(p) => {
                result.protocol = p.protocol;
                result.endpoint = p.endpoint.clone();
                result.candidates = p.candidates.clone();
                result.remote_random = p.remote_random;
                result.requires_auth = p.requires_auth;
                result.requires_key_password = p.requires_key_password;
                result.address_family = match p.endpoint_ipv6 {
                    Some(true) => "IPv6",
                    Some(false) => "IPv4",
                    None => "IPv4/IPv6",
                }
                .into();
            }
            Self::WireGuard(p) => {
                result.protocol = ChainProtocol::Wireguard;
                result.source = ChainSource::WireguardCustom;
                result.endpoint = p.endpoint.clone();
                result.addresses = p.addresses.iter().map(ToString::to_string).collect();
                result.allowed_ips = p.allowed_ips.iter().map(ToString::to_string).collect();
                result.dns_servers = p.dns_servers.clone();
                result.mtu = Some(p.mtu);
            }
        }
        if let Some(address) = result.endpoint.address() {
            result.address_family = if address.is_ipv6() { "IPv6" } else { "IPv4" }.into();
        }
        Ok(result)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[error("{reason} ({field}, line {line})")]
pub struct ImportError {
    pub line: usize,
    pub field: String,
    pub reason: String,
}
impl ImportError {
    pub fn new(line: usize, field: &'static str, reason: &'static str) -> Self {
        Self {
            line,
            field: field.into(),
            reason: reason.into(),
        }
    }
}

/// Generalized status keeps the existing lifecycle vocabulary and wire adapter.
pub type ChainExitStatus = crate::vpngate::GateStatus;

#[derive(Deserialize)]
pub struct ChainProfileRequest {
    pub action: String,
    #[serde(default)]
    pub source: ChainSource,
    #[serde(default)]
    pub name: String,
    pub profile_id: Option<Uuid>,
    pub revision: Option<Uuid>,
    #[serde(flatten)]
    pub secrets: ImportSecrets,
}
#[derive(Default, Serialize, Deserialize)]
pub struct ChainProfileResponse {
    pub profiles: Vec<ChainProfileSummary>,
    pub preview: Option<ChainProfileSummary>,
    pub error: Option<ImportError>,
}
pub fn profile_command(
    parent: &std::path::Path,
    cipher: &dyn store::ProfileCipher,
    request: ChainProfileRequest,
    retained: &[Uuid],
) -> ChainProfileResponse {
    let run = || -> Result<ChainProfileResponse, ImportError> {
        let store = store::ChainProfileStore::new(parent, cipher);
        let reference = || -> Result<(Uuid, Uuid), ImportError> {
            Ok((
                request
                    .profile_id
                    .ok_or_else(|| ImportError::new(0, "profile_id", "missing_profile"))?,
                request
                    .revision
                    .ok_or_else(|| ImportError::new(0, "revision", "missing_revision"))?,
            ))
        };
        let preview = match request.action.as_str() {
            "list" => None,
            "preview" => {
                let mut summary = ValidatedProfile::parse(request.source, &request.secrets)?
                    .summary(&request.name, Uuid::nil(), Uuid::nil())?;
                summary.source = request.source;
                Some(summary)
            }
            "import" => Some(store.import(request.source, &request.name, request.secrets)?),
            "rename" => {
                let (id, rev) = reference()?;
                Some(store.rename(id, rev, &request.name)?)
            }
            "remove" => {
                let (id, rev) = reference()?;
                store.remove(id, rev, retained)?;
                None
            }
            "credentials" => {
                let (id, rev) = reference()?;
                Some(store.update_credentials(id, rev, request.secrets)?)
            }
            _ => return Err(ImportError::new(0, "action", "unsupported_action")),
        };
        Ok(ChainProfileResponse {
            profiles: store.list()?,
            preview,
            error: None,
        })
    };
    run().unwrap_or_else(|error| ChainProfileResponse {
        error: Some(error),
        ..Default::default()
    })
}

pub fn prepare_selection(
    parent: &std::path::Path,
    profile: &crate::Profile,
    cipher: &dyn store::ProfileCipher,
) -> Result<
    Option<(
        crate::vpngate::ServerSummary,
        crate::vpngate::PreparedProfile,
    )>,
    ImportError,
> {
    if !profile.chain_enabled() {
        return Ok(None);
    }
    if let Some(chain) = profile.custom_chain() {
        let (mut summary, mut parsed, secrets) =
            store::ChainProfileStore::new(parent, cipher).load(chain)?;
        if let Some(endpoint) = &chain.endpoint_override {
            let ValidatedProfile::WireGuard(wireguard) = &mut parsed else {
                return Err(ImportError::new(0, "endpoint_override", "invalid_endpoint"));
            };
            wireguard.endpoint = endpoint.clone();
            summary.endpoint = endpoint.clone();
            summary.address_family = if endpoint.address().is_some_and(|a| a.is_ipv6()) {
                "IPv6"
            } else {
                "IPv4"
            }
            .into();
        }
        if profile.data_plane == crate::DataPlaneMode::L4Proxy && summary.protocol.requires_udp() {
            return Err(ImportError::new(0, "data_plane", "connect_ip_required"));
        }
        let server = summary.server_summary();
        return Ok(Some((
            server,
            crate::vpngate::PreparedProfile::imported(summary, parsed, secrets),
        )));
    }
    let selection = profile
        .vpn_gate
        .selection
        .as_ref()
        .ok_or_else(|| ImportError::new(0, "selection", "missing_profile"))?;
    crate::vpngate::CatalogueStore::new(parent)
        .load_selection(selection)
        .map(Some)
        .map_err(|_| ImportError::new(0, "selection", "invalid_vpn_gate_selection"))
}
