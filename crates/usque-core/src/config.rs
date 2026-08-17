use std::collections::{BTreeMap, HashSet};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

use ipnet::IpNet;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

use crate::identity::IdentityProvider;

pub const CURRENT_SCHEMA_VERSION: u32 = 6;
pub const DEFAULT_ENDPOINT_V4: Ipv4Addr = Ipv4Addr::new(162, 159, 198, 2);
pub const DEFAULT_ENDPOINT_V6: Ipv6Addr = Ipv6Addr::new(0x2606, 0x4700, 0x0103, 0, 0, 0, 0, 2);
pub const DEFAULT_PORT: u16 = 443;
pub const DEFAULT_SNI: &str = "speed.cloudflare.com";
pub const LEGACY_DEFAULT_SNI: &str = "www.visa.cn";
pub const DEFAULT_MTU: u16 = 1280;
pub const DEFAULT_DNS_V4: Ipv4Addr = Ipv4Addr::new(1, 1, 1, 1);
pub const DEFAULT_DNS_V6: Ipv6Addr = Ipv6Addr::new(0x2606, 0x4700, 0x4700, 0, 0, 0, 0, 0x1111);
pub const DEFAULT_PROFILE_ID: Uuid = Uuid::from_u128(0x8c30_b771_9ebd_457a_b67b_bbc7_4a1d_dba6);
pub const MAX_PROFILES: usize = 128;
pub const MAX_DNS_SERVERS: usize = 8;
pub const MAX_SPLIT_EXCLUSIONS: usize = 256;
pub const MAX_PROXY_LISTENERS_PER_PROTOCOL: usize = 16;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AppConfig {
    pub schema_version: u32,
    pub active_profile_id: Option<Uuid>,
    pub profiles: Vec<Profile>,
    pub preferences: AppPreferences,
    /// Non-secret provider boundary used to classify a profile even if its
    /// secure IdentityMetadata record is missing or corrupted.
    #[serde(default)]
    pub identity_bindings: BTreeMap<Uuid, IdentityProvider>,
    #[serde(default)]
    pub pending_identity_deletions: Vec<Uuid>,
    /// Profile identities durably staged before the non-secret profile is
    /// committed. Startup recovery deletes these orphaned records.
    #[serde(default)]
    pub pending_identity_creations: Vec<Uuid>,
}

impl Default for AppConfig {
    fn default() -> Self {
        let profile = Profile::default();
        Self {
            schema_version: CURRENT_SCHEMA_VERSION,
            active_profile_id: Some(profile.id),
            profiles: vec![profile],
            preferences: AppPreferences::default(),
            identity_bindings: BTreeMap::new(),
            pending_identity_deletions: Vec::new(),
            pending_identity_creations: Vec::new(),
        }
    }
}

impl AppConfig {
    pub fn active_profile(&self) -> Option<&Profile> {
        let id = self.active_profile_id?;
        self.profiles.iter().find(|profile| profile.id == id)
    }

    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.schema_version > CURRENT_SCHEMA_VERSION {
            return Err(ConfigError::NewerSchema {
                found: self.schema_version,
                supported: CURRENT_SCHEMA_VERSION,
            });
        }

        if self.profiles.is_empty() {
            return Err(ConfigError::NoProfiles);
        }
        if self.profiles.len() > MAX_PROFILES {
            return Err(ConfigError::TooManyProfiles(self.profiles.len()));
        }

        let mut ids = HashSet::new();
        for profile in &self.profiles {
            if !ids.insert(profile.id) {
                return Err(ConfigError::DuplicateProfileId(profile.id));
            }
            profile.validate()?;
        }

        if self.identity_bindings.len() > MAX_PROFILES {
            return Err(ConfigError::TooManyIdentityBindings(
                self.identity_bindings.len(),
            ));
        }
        for (profile_id, provider) in &self.identity_bindings {
            if !ids.contains(profile_id) {
                return Err(ConfigError::IdentityBindingWithoutProfile(*profile_id));
            }
            if let IdentityProvider::ZeroTrust { organization } = provider
                && IdentityProvider::zero_trust(organization.clone()).is_err()
            {
                return Err(ConfigError::InvalidIdentityBinding(*profile_id));
            }
        }

        if self.pending_identity_deletions.len() > MAX_PROFILES {
            return Err(ConfigError::TooManyPendingIdentityDeletions(
                self.pending_identity_deletions.len(),
            ));
        }
        let mut pending = HashSet::new();
        for profile_id in &self.pending_identity_deletions {
            if !pending.insert(*profile_id) {
                return Err(ConfigError::DuplicatePendingIdentityDeletion(*profile_id));
            }
            if ids.contains(profile_id) {
                return Err(ConfigError::PendingIdentityStillReferenced(*profile_id));
            }
        }

        if self.pending_identity_creations.len() > MAX_PROFILES {
            return Err(ConfigError::TooManyPendingIdentityCreations(
                self.pending_identity_creations.len(),
            ));
        }
        let mut pending_creations = HashSet::new();
        for profile_id in &self.pending_identity_creations {
            if !pending_creations.insert(*profile_id) {
                return Err(ConfigError::DuplicatePendingIdentityCreation(*profile_id));
            }
            if ids.contains(profile_id) {
                return Err(ConfigError::PendingIdentityCreationAlreadyReferenced(
                    *profile_id,
                ));
            }
            if pending.contains(profile_id) {
                return Err(ConfigError::PendingIdentityCreationAndDeletion(*profile_id));
            }
        }

        match self.active_profile_id {
            Some(active) if !ids.contains(&active) => {
                return Err(ConfigError::MissingActiveProfile(active));
            }
            None => return Err(ConfigError::NoActiveProfile),
            Some(_) => {}
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AppPreferences {
    pub locale: AppLocale,
    pub theme: ThemeMode,
    pub update_check_enabled: bool,
    pub log_level: LogLevel,
    #[serde(default)]
    pub profiles_migrated_from_flutter: bool,
}

impl Default for AppPreferences {
    fn default() -> Self {
        Self {
            locale: AppLocale::System,
            theme: ThemeMode::System,
            update_check_enabled: true,
            log_level: LogLevel::Info,
            profiles_migrated_from_flutter: false,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum AppLocale {
    #[default]
    System,
    English,
    SimplifiedChinese,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ThemeMode {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum LogLevel {
    Error,
    Warn,
    #[default]
    Info,
    Debug,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Profile {
    pub id: Uuid,
    pub name: String,
    pub mode: OperatingMode,
    /// Independently selectable local consumers of the one active MASQUE
    /// channel. `mode` remains serialized only for v1 wire/config migration.
    #[serde(default)]
    pub frontends: FrontendSettings,
    pub transport: TransportPolicy,
    pub endpoint: EndpointSettings,
    /// Selects the physical address family used to reach the MASQUE endpoint.
    /// It never restricts IPv4 or IPv6 payloads carried inside CONNECT-IP.
    pub ip_policy: IpPolicy,
    pub mtu: u16,
    pub dns_mode: DnsMode,
    pub dns_servers: Vec<IpAddr>,
    pub allow_lan: bool,
    pub split_exclusions: Vec<IpNet>,
    pub kill_switch: bool,
    pub auto_connect: bool,
    pub proxy: ProxySettings,
}

impl Default for Profile {
    fn default() -> Self {
        Self {
            id: DEFAULT_PROFILE_ID,
            name: "Default".to_owned(),
            mode: OperatingMode::legacy_platform_default(),
            frontends: FrontendSettings::default(),
            transport: TransportPolicy::Auto,
            endpoint: EndpointSettings::default(),
            ip_policy: IpPolicy::Auto,
            mtu: DEFAULT_MTU,
            dns_mode: DnsMode::Tunnel,
            dns_servers: default_dns_servers(),
            allow_lan: false,
            split_exclusions: Vec::new(),
            kill_switch: true,
            auto_connect: false,
            proxy: ProxySettings::default(),
        }
    }
}

impl Profile {
    pub fn validate(&self) -> Result<(), ConfigError> {
        let trimmed_name = self.name.trim();
        if trimmed_name.is_empty() || trimmed_name.chars().count() > 64 {
            return Err(ConfigError::InvalidProfileName);
        }
        self.endpoint.validate()?;

        if !(1280..=9000).contains(&self.mtu) {
            return Err(ConfigError::InvalidMtu(self.mtu));
        }
        if self.dns_servers.is_empty() {
            return Err(ConfigError::MissingDnsServer);
        }
        if self.dns_servers.len() > MAX_DNS_SERVERS {
            return Err(ConfigError::TooManyDnsServers(self.dns_servers.len()));
        }
        if self.dns_servers.iter().collect::<HashSet<_>>().len() != self.dns_servers.len() {
            return Err(ConfigError::DuplicateDnsServer);
        }
        if self.split_exclusions.len() > MAX_SPLIT_EXCLUSIONS {
            return Err(ConfigError::TooManySplitExclusions(
                self.split_exclusions.len(),
            ));
        }
        if self.split_exclusions.iter().collect::<HashSet<_>>().len() != self.split_exclusions.len()
        {
            return Err(ConfigError::DuplicateSplitExclusion);
        }
        if self.frontends.tunnel {
            if self.dns_mode == DnsMode::System {
                return Err(ConfigError::VpnSystemDnsForbidden);
            }
            if let Some(server) = self
                .dns_servers
                .iter()
                .copied()
                .find(|server| invalid_vpn_dns_address(*server))
            {
                return Err(ConfigError::InvalidVpnDnsServer(server));
            }
            if let Some(server) = self.dns_servers.iter().copied().find(|server| {
                self.split_exclusions
                    .iter()
                    .any(|network| network.contains(server))
                    || *server == IpAddr::V4(self.endpoint.ipv4)
                    || *server == IpAddr::V6(self.endpoint.ipv6)
                    || self.allow_lan && is_lan_bypass_address(*server)
            }) {
                return Err(ConfigError::VpnDnsServerBypassed(server));
            }
        }
        self.proxy.validate()?;
        if self.frontends.socks5 && self.proxy.socks5_listeners.is_empty() {
            return Err(ConfigError::MissingSocks5Listener);
        }
        if self.frontends.http && self.proxy.http_listeners.is_empty() {
            return Err(ConfigError::MissingHttpListener);
        }
        if self.proxy.system_proxy && !self.frontends.http {
            return Err(ConfigError::SystemProxyRequiresHttpMode);
        }
        if self.proxy.system_proxy
            && !self
                .proxy
                .http_listeners
                .iter()
                .any(|listener| listener.ip().is_loopback())
        {
            return Err(ConfigError::SystemProxyRequiresLoopback);
        }
        Ok(())
    }

    pub fn reset_network_defaults(&mut self) {
        self.mode = OperatingMode::legacy_platform_default();
        self.frontends = FrontendSettings::default();
        self.transport = TransportPolicy::Auto;
        self.endpoint = EndpointSettings::default();
        self.ip_policy = IpPolicy::Auto;
        self.mtu = DEFAULT_MTU;
        self.dns_mode = DnsMode::Tunnel;
        self.dns_servers = default_dns_servers();
        self.allow_lan = false;
        self.split_exclusions.clear();
        self.proxy = ProxySettings::default();
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct FrontendSettings {
    pub tunnel: bool,
    pub socks5: bool,
    pub http: bool,
}

impl FrontendSettings {
    pub const fn windows_default() -> Self {
        Self {
            tunnel: true,
            socks5: true,
            http: true,
        }
    }

    pub const fn android_default() -> Self {
        Self {
            tunnel: true,
            socks5: true,
            http: true,
        }
    }

    pub const fn platform_default() -> Self {
        if cfg!(target_os = "android") {
            Self::android_default()
        } else {
            Self::windows_default()
        }
    }

    pub const fn any(self) -> bool {
        self.tunnel || self.socks5 || self.http
    }
}

impl Default for FrontendSettings {
    fn default() -> Self {
        Self::platform_default()
    }
}

fn invalid_vpn_dns_address(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(address) => {
            address.is_unspecified()
                || address.is_loopback()
                || address.is_link_local()
                || address.is_multicast()
                || address.is_broadcast()
        }
        IpAddr::V6(address) => {
            address.is_unspecified()
                || address.is_loopback()
                || address.is_unicast_link_local()
                || address.is_multicast()
        }
    }
}

fn is_lan_bypass_address(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(address) => {
            let [first, second, ..] = address.octets();
            first == 10
                || first == 172 && (16..=31).contains(&second)
                || first == 192 && second == 168
                || first == 169 && second == 254
        }
        IpAddr::V6(address) => {
            let [first, second, ..] = address.octets();
            first & 0xfe == 0xfc || first == 0xfe && second & 0xc0 == 0x80
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum OperatingMode {
    #[default]
    Vpn,
    Socks5,
    HttpProxy,
}

impl OperatingMode {
    pub const fn legacy_platform_default() -> Self {
        if cfg!(target_os = "android") {
            Self::Vpn
        } else {
            Self::Socks5
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum TransportPolicy {
    #[default]
    Auto,
    Http3,
    Http2,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
/// Policy for selecting the outer MASQUE endpoint address family.
///
/// Even the `Only` variants keep IPv4 and IPv6 enabled inside CONNECT-IP.
pub enum IpPolicy {
    #[default]
    Auto,
    PreferIpv4,
    PreferIpv6,
    Ipv4Only,
    Ipv6Only,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum DnsMode {
    #[default]
    Tunnel,
    LocalConfigured,
    System,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EndpointSettings {
    pub ipv4: Ipv4Addr,
    pub ipv6: Ipv6Addr,
    pub port: u16,
    pub sni: String,
}

impl Default for EndpointSettings {
    fn default() -> Self {
        Self {
            ipv4: DEFAULT_ENDPOINT_V4,
            ipv6: DEFAULT_ENDPOINT_V6,
            port: DEFAULT_PORT,
            sni: DEFAULT_SNI.to_owned(),
        }
    }
}

impl EndpointSettings {
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.port == 0 {
            return Err(ConfigError::InvalidPort);
        }
        if !valid_dns_name(&self.sni) {
            return Err(ConfigError::InvalidSni(self.sni.clone()));
        }
        Ok(())
    }

    pub fn ipv4_socket(&self) -> SocketAddr {
        SocketAddr::new(self.ipv4.into(), self.port)
    }

    pub fn ipv6_socket(&self) -> SocketAddr {
        SocketAddr::new(self.ipv6.into(), self.port)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProxySettings {
    pub socks5_listeners: Vec<SocketAddr>,
    pub http_listeners: Vec<SocketAddr>,
    pub system_proxy: bool,
    pub udp_idle_timeout_seconds: u32,
    #[serde(default)]
    pub dns_mode: ProxyDnsMode,
    #[serde(default = "default_dns_servers")]
    pub dns_servers: Vec<IpAddr>,
}

impl Default for ProxySettings {
    fn default() -> Self {
        Self {
            socks5_listeners: vec![
                SocketAddr::from(([127, 0, 0, 1], 1080)),
                SocketAddr::new(Ipv6Addr::LOCALHOST.into(), 1080),
            ],
            http_listeners: vec![
                SocketAddr::from(([127, 0, 0, 1], 8080)),
                SocketAddr::new(Ipv6Addr::LOCALHOST.into(), 8080),
            ],
            system_proxy: false,
            udp_idle_timeout_seconds: 60,
            dns_mode: ProxyDnsMode::Remote,
            dns_servers: default_dns_servers(),
        }
    }
}

impl ProxySettings {
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.socks5_listeners.len() > MAX_PROXY_LISTENERS_PER_PROTOCOL
            || self.http_listeners.len() > MAX_PROXY_LISTENERS_PER_PROTOCOL
        {
            return Err(ConfigError::TooManyProxyListeners);
        }
        if !(1..=3_600).contains(&self.udp_idle_timeout_seconds) {
            return Err(ConfigError::InvalidUdpIdleTimeout(
                self.udp_idle_timeout_seconds,
            ));
        }
        if self.dns_servers.is_empty() {
            return Err(ConfigError::MissingDnsServer);
        }
        if self.dns_servers.len() > MAX_DNS_SERVERS {
            return Err(ConfigError::TooManyDnsServers(self.dns_servers.len()));
        }
        if self.dns_servers.iter().collect::<HashSet<_>>().len() != self.dns_servers.len() {
            return Err(ConfigError::DuplicateDnsServer);
        }

        let mut listeners = HashSet::new();
        for listener in self
            .socks5_listeners
            .iter()
            .chain(self.http_listeners.iter())
        {
            if listener.port() == 0 {
                return Err(ConfigError::InvalidPort);
            }
            if !listeners.insert(*listener) {
                return Err(ConfigError::DuplicateProxyListener(*listener));
            }
        }
        Ok(())
    }

    pub fn exposes_lan(&self, mode: OperatingMode) -> bool {
        let listeners = match mode {
            OperatingMode::Vpn => return false,
            OperatingMode::Socks5 => &self.socks5_listeners,
            OperatingMode::HttpProxy => &self.http_listeners,
        };
        listeners.iter().any(|address| !address.ip().is_loopback())
    }

    pub fn socks5_exposes_lan(&self) -> bool {
        self.socks5_listeners
            .iter()
            .any(|address| !address.ip().is_loopback())
    }

    pub fn http_exposes_lan(&self) -> bool {
        self.http_listeners
            .iter()
            .any(|address| !address.ip().is_loopback())
    }
}

fn default_dns_servers() -> Vec<IpAddr> {
    vec![DEFAULT_DNS_V4.into(), DEFAULT_DNS_V6.into()]
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ProxyDnsMode {
    #[default]
    Remote,
    LocalConfigured,
    System,
}

fn valid_dns_name(value: &str) -> bool {
    if value.is_empty()
        || value.len() > 253
        || value.ends_with('.')
        || value.chars().any(char::is_whitespace)
    {
        return false;
    }
    value.split('.').all(|label| {
        !label.is_empty()
            && label.len() <= 63
            && !label.starts_with('-')
            && !label.ends_with('-')
            && label
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    })
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ConfigError {
    #[error("profile name must contain 1 to 64 visible characters")]
    InvalidProfileName,
    #[error("SNI is not a valid DNS name: {0}")]
    InvalidSni(String),
    #[error("port must be between 1 and 65535")]
    InvalidPort,
    #[error("SOCKS5 UDP idle timeout must be between 1 and 3600 seconds, got {0}")]
    InvalidUdpIdleTimeout(u32),
    #[error("MTU must be between 1280 and 9000, got {0}")]
    InvalidMtu(u16),
    #[error("at least one DNS server is required")]
    MissingDnsServer,
    #[error("no more than {MAX_DNS_SERVERS} DNS servers are allowed, got {0}")]
    TooManyDnsServers(usize),
    #[error("duplicate DNS server")]
    DuplicateDnsServer,
    #[error("VPN mode cannot use the physical system DNS resolver")]
    VpnSystemDnsForbidden,
    #[error("VPN DNS server {0} is not a routable unicast address")]
    InvalidVpnDnsServer(IpAddr),
    #[error("VPN DNS server {0} is covered by a LAN or CIDR bypass")]
    VpnDnsServerBypassed(IpAddr),
    #[error("no more than {MAX_SPLIT_EXCLUSIONS} split exclusions are allowed, got {0}")]
    TooManySplitExclusions(usize),
    #[error("duplicate split exclusion")]
    DuplicateSplitExclusion,
    #[error("at least one SOCKS5 listener is required while SOCKS5 is enabled")]
    MissingSocks5Listener,
    #[error("at least one HTTP listener is required while HTTP is enabled")]
    MissingHttpListener,
    #[error(
        "no more than {MAX_PROXY_LISTENERS_PER_PROTOCOL} listeners per proxy protocol are allowed"
    )]
    TooManyProxyListeners,
    #[error("duplicate proxy listener: {0}")]
    DuplicateProxyListener(SocketAddr),
    #[error("Windows system proxy requires the HTTP frontend")]
    SystemProxyRequiresHttpMode,
    #[error("Windows system proxy requires at least one Loopback HTTP listener")]
    SystemProxyRequiresLoopback,
    #[error("duplicate profile ID: {0}")]
    DuplicateProfileId(Uuid),
    #[error("at least one profile is required")]
    NoProfiles,
    #[error("no more than {MAX_PROFILES} profiles are allowed, got {0}")]
    TooManyProfiles(usize),
    #[error("an active profile is required")]
    NoActiveProfile,
    #[error("active profile does not exist: {0}")]
    MissingActiveProfile(Uuid),
    #[error("no more than {MAX_PROFILES} identity bindings are allowed, got {0}")]
    TooManyIdentityBindings(usize),
    #[error("identity binding references a missing profile: {0}")]
    IdentityBindingWithoutProfile(Uuid),
    #[error("identity binding is invalid for profile: {0}")]
    InvalidIdentityBinding(Uuid),
    #[error("no more than {MAX_PROFILES} pending identity deletions are allowed, got {0}")]
    TooManyPendingIdentityDeletions(usize),
    #[error("duplicate pending identity deletion: {0}")]
    DuplicatePendingIdentityDeletion(Uuid),
    #[error("pending identity deletion is still referenced by a profile: {0}")]
    PendingIdentityStillReferenced(Uuid),
    #[error("no more than {MAX_PROFILES} pending identity creations are allowed, got {0}")]
    TooManyPendingIdentityCreations(usize),
    #[error("duplicate pending identity creation: {0}")]
    DuplicatePendingIdentityCreation(Uuid),
    #[error("pending identity creation is already referenced by a profile: {0}")]
    PendingIdentityCreationAlreadyReferenced(Uuid),
    #[error("identity cannot be pending creation and deletion at the same time: {0}")]
    PendingIdentityCreationAndDeletion(Uuid),
    #[error("configuration schema {found} is newer than supported schema {supported}")]
    NewerSchema { found: u32, supported: u32 },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_the_product_contract() {
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../../../oracle/fixtures/defaults.json"))
                .expect("parse sanitized oracle defaults");
        assert_eq!(fixture["schema_version"], 1);

        let profile = Profile::default();
        assert_eq!(
            profile.endpoint.ipv4.to_string(),
            fixture["endpoint_v4"].as_str().expect("endpoint_v4")
        );
        assert_eq!(
            profile.endpoint.ipv6.to_string(),
            fixture["endpoint_v6"].as_str().expect("endpoint_v6")
        );
        assert_eq!(
            u64::from(profile.endpoint.port),
            fixture["endpoint_port"].as_u64().expect("endpoint_port")
        );
        assert_eq!(profile.endpoint.sni, "speed.cloudflare.com");
        assert_eq!(
            u64::from(profile.mtu),
            fixture["mtu"].as_u64().expect("mtu")
        );
        assert_eq!(
            profile
                .dns_servers
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>(),
            fixture["fallback_dns"]
                .as_array()
                .expect("fallback_dns")
                .iter()
                .map(|value| value.as_str().expect("DNS string").to_owned())
                .collect::<Vec<_>>()
        );
        assert_eq!(
            profile
                .proxy
                .socks5_listeners
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>(),
            fixture["socks5_listeners"]
                .as_array()
                .expect("socks5_listeners")
                .iter()
                .map(|value| value.as_str().expect("SOCKS5 listener").to_owned())
                .collect::<Vec<_>>()
        );
        assert_eq!(
            profile
                .proxy
                .http_listeners
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>(),
            fixture["http_listeners"]
                .as_array()
                .expect("http_listeners")
                .iter()
                .map(|value| value.as_str().expect("HTTP listener").to_owned())
                .collect::<Vec<_>>()
        );
        assert_eq!(profile.mode, OperatingMode::legacy_platform_default());
        assert_eq!(profile.frontends, FrontendSettings::platform_default());
        assert_eq!(profile.transport, TransportPolicy::Auto);
        assert!(profile.kill_switch);
        assert!(!profile.proxy.system_proxy);
        assert_eq!(profile.proxy.dns_servers, profile.dns_servers);
        assert!(!profile.proxy.exposes_lan(OperatingMode::Socks5));
    }

    #[test]
    fn platform_frontend_defaults_enable_all_outputs() {
        assert_eq!(
            FrontendSettings::windows_default(),
            FrontendSettings {
                tunnel: true,
                socks5: true,
                http: true,
            }
        );
        assert_eq!(
            FrontendSettings::android_default(),
            FrontendSettings {
                tunnel: true,
                socks5: true,
                http: true,
            }
        );
    }

    #[test]
    fn exposed_proxy_is_reported_without_adding_auth() {
        let proxy = ProxySettings {
            socks5_listeners: vec!["0.0.0.0:1080".parse().unwrap()],
            ..ProxySettings::default()
        };
        assert!(proxy.exposes_lan(OperatingMode::Socks5));
        assert!(proxy.validate().is_ok());
    }

    #[test]
    fn udp_idle_timeout_is_bounded() {
        let mut proxy = ProxySettings {
            udp_idle_timeout_seconds: 0,
            ..ProxySettings::default()
        };
        assert_eq!(proxy.validate(), Err(ConfigError::InvalidUdpIdleTimeout(0)));
        proxy.udp_idle_timeout_seconds = 3_601;
        assert_eq!(
            proxy.validate(),
            Err(ConfigError::InvalidUdpIdleTimeout(3_601))
        );
    }

    #[test]
    fn active_profile_must_exist() {
        let config = AppConfig {
            active_profile_id: Some(Uuid::nil()),
            ..AppConfig::default()
        };
        assert!(matches!(
            config.validate(),
            Err(ConfigError::MissingActiveProfile(_))
        ));
    }

    #[test]
    fn configuration_collections_are_bounded_and_unique() {
        let mut profile = Profile::default();
        profile.dns_servers.push(DEFAULT_DNS_V4.into());
        assert_eq!(profile.validate(), Err(ConfigError::DuplicateDnsServer));

        profile.dns_servers = default_dns_servers();
        profile.proxy.dns_servers.push(DEFAULT_DNS_V4.into());
        assert_eq!(profile.validate(), Err(ConfigError::DuplicateDnsServer));

        let empty = AppConfig {
            active_profile_id: None,
            profiles: Vec::new(),
            ..AppConfig::default()
        };
        assert_eq!(empty.validate(), Err(ConfigError::NoProfiles));
    }

    #[test]
    fn vpn_dns_cannot_escape_through_system_or_bypass_routes() {
        let mut profile = Profile {
            dns_mode: DnsMode::System,
            frontends: FrontendSettings {
                tunnel: true,
                socks5: false,
                http: false,
            },
            proxy: ProxySettings {
                system_proxy: false,
                ..ProxySettings::default()
            },
            ..Profile::default()
        };
        assert_eq!(profile.validate(), Err(ConfigError::VpnSystemDnsForbidden));

        profile.dns_mode = DnsMode::Tunnel;
        profile.dns_servers = vec!["127.0.0.1".parse().unwrap()];
        assert_eq!(
            profile.validate(),
            Err(ConfigError::InvalidVpnDnsServer(
                "127.0.0.1".parse().unwrap()
            ))
        );

        // Endpoint-only policies do not disable the opposite family inside
        // CONNECT-IP, so an IPv6 tunnel DNS server remains valid over an
        // IPv4-only MASQUE ingress.
        profile.ip_policy = IpPolicy::Ipv4Only;
        profile.dns_servers = vec!["2606:4700:4700::1111".parse().unwrap()];
        assert_eq!(profile.validate(), Ok(()));

        profile.dns_servers = vec!["1.1.1.1".parse().unwrap()];
        profile.split_exclusions = vec!["1.1.1.0/24".parse().unwrap()];
        assert_eq!(
            profile.validate(),
            Err(ConfigError::VpnDnsServerBypassed(
                "1.1.1.1".parse().unwrap()
            ))
        );

        profile.split_exclusions.clear();
        profile.dns_servers = vec![IpAddr::V4(profile.endpoint.ipv4)];
        assert_eq!(
            profile.validate(),
            Err(ConfigError::VpnDnsServerBypassed(IpAddr::V4(
                profile.endpoint.ipv4
            )))
        );

        profile.dns_servers = vec!["192.168.1.1".parse().unwrap()];
        profile.allow_lan = true;
        assert_eq!(
            profile.validate(),
            Err(ConfigError::VpnDnsServerBypassed(
                "192.168.1.1".parse().unwrap()
            ))
        );
    }

    #[test]
    fn locally_configured_vpn_dns_is_still_routed_through_the_tunnel() {
        let profile = Profile {
            dns_mode: DnsMode::LocalConfigured,
            dns_servers: vec!["9.9.9.9".parse().unwrap(), "2620:fe::fe".parse().unwrap()],
            frontends: FrontendSettings {
                tunnel: true,
                socks5: false,
                http: false,
            },
            proxy: ProxySettings {
                system_proxy: false,
                ..ProxySettings::default()
            },
            ..Profile::default()
        };
        assert!(profile.validate().is_ok());
    }

    #[test]
    fn pending_identity_deletions_cannot_reference_live_profiles() {
        let mut config = AppConfig::default();
        let active = config.active_profile_id.unwrap();
        config.pending_identity_deletions.push(active);
        assert_eq!(
            config.validate(),
            Err(ConfigError::PendingIdentityStillReferenced(active))
        );
    }

    #[test]
    fn pending_identity_creations_are_unreferenced_and_unique() {
        let mut config = AppConfig::default();
        let active = config.active_profile_id.unwrap();
        config.pending_identity_creations.push(active);
        assert_eq!(
            config.validate(),
            Err(ConfigError::PendingIdentityCreationAlreadyReferenced(
                active
            ))
        );

        let mut config = AppConfig::default();
        let pending = Uuid::new_v4();
        config.pending_identity_creations = vec![pending, pending];
        assert_eq!(
            config.validate(),
            Err(ConfigError::DuplicatePendingIdentityCreation(pending))
        );
    }

    #[test]
    fn identity_bindings_are_bounded_valid_and_reference_live_profiles() {
        let mut config = AppConfig::default();
        let missing = Uuid::new_v4();
        config
            .identity_bindings
            .insert(missing, IdentityProvider::Consumer);
        assert_eq!(
            config.validate(),
            Err(ConfigError::IdentityBindingWithoutProfile(missing))
        );

        let mut config = AppConfig::default();
        let active = config.active_profile_id.unwrap();
        config.identity_bindings.insert(
            active,
            IdentityProvider::ZeroTrust {
                organization: "Invalid.Team".to_owned(),
            },
        );
        assert_eq!(
            config.validate(),
            Err(ConfigError::InvalidIdentityBinding(active))
        );
    }
}
