use std::net::IpAddr;

use ipnet::IpNet;
use serde::{Deserialize, Serialize};

use super::{
    Account, DirectDnsSettings, DnsMode, EndpointSettings, FrontendSettings, IpPolicy, Profile,
    ProxySettings, TransportPolicy,
};

/// Device-wide MASQUE, DNS, proxy, and output settings. A Zero Trust account
/// overlays its registration-owned endpoint IPv4/IPv6 pair during hydration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SharedNetworkSettings {
    pub frontends: FrontendSettings,
    pub transport: TransportPolicy,
    pub endpoint: EndpointSettings,
    pub ip_policy: IpPolicy,
    pub mtu: u16,
    pub dns_mode: DnsMode,
    pub dns_servers: Vec<IpAddr>,
    pub allow_lan: bool,
    pub split_exclusions: Vec<IpNet>,
    pub kill_switch: bool,
    pub auto_connect: bool,
    pub proxy: ProxySettings,
    #[serde(default)]
    pub geo_direct_countries: Vec<String>,
    #[serde(default)]
    pub direct_dns: DirectDnsSettings,
}

impl Default for SharedNetworkSettings {
    fn default() -> Self {
        Self::from_profile(&Profile::default())
    }
}

impl SharedNetworkSettings {
    /// Copy device-wide settings from a runtime profile. Zero Trust endpoint
    /// addresses are restored from the account after the shared copy is made.
    pub fn from_profile(profile: &Profile) -> Self {
        Self {
            frontends: profile.frontends,
            transport: profile.transport,
            endpoint: profile.endpoint.clone(),
            ip_policy: profile.ip_policy,
            mtu: profile.mtu,
            dns_mode: profile.dns_mode,
            dns_servers: profile.dns_servers.clone(),
            allow_lan: profile.allow_lan,
            split_exclusions: profile.split_exclusions.clone(),
            kill_switch: profile.kill_switch,
            auto_connect: profile.auto_connect,
            proxy: profile.proxy.clone(),
            geo_direct_countries: profile.geo_direct_countries.clone(),
            direct_dns: profile.direct_dns.clone(),
        }
    }

    pub fn hydrate(&self, account: &Account) -> Profile {
        let mut endpoint = self.endpoint.clone();
        if let Some(managed) = &account.managed_endpoint_ips {
            endpoint.ipv4 = managed.ipv4;
            endpoint.ipv6 = managed.ipv6;
        }
        let mut profile = Profile {
            id: account.id,
            name: account.name.clone(),
            mode: super::OperatingMode::Vpn,
            frontends: self.frontends,
            transport: self.transport,
            endpoint,
            ip_policy: self.ip_policy,
            mtu: self.mtu,
            dns_mode: self.dns_mode,
            dns_servers: self.dns_servers.clone(),
            allow_lan: self.allow_lan,
            split_exclusions: self.split_exclusions.clone(),
            kill_switch: self.kill_switch,
            auto_connect: self.auto_connect,
            proxy: self.proxy.clone(),
            geo_direct_countries: self.geo_direct_countries.clone(),
            direct_dns: self.direct_dns.clone(),
        };
        profile.canonicalize_mode();
        profile.proxy.normalize_auth();
        let _ = profile.canonicalize_geo_direct();
        profile.canonicalize_direct_dns();
        profile
    }

    pub fn reset_user_defaults(&mut self) {
        let kill_switch = self.kill_switch;
        let auto_connect = self.auto_connect;
        *self = Self::default();
        self.kill_switch = kill_switch;
        self.auto_connect = auto_connect;
    }
}
