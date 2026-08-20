use std::net::IpAddr;

use ipnet::IpNet;
use serde::{Deserialize, Serialize};

use super::{
    Account, DnsMode, EndpointSettings, FrontendSettings, IpPolicy, Profile, ProxySettings,
    TransportPolicy,
};

/// Device-wide MASQUE, DNS, proxy, and output settings.
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
}

impl Default for SharedNetworkSettings {
    fn default() -> Self {
        Self::from_profile(&Profile::default())
    }
}

impl SharedNetworkSettings {
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
        }
    }

    pub fn from_profile_keeping_endpoint(profile: &Profile, endpoint: EndpointSettings) -> Self {
        let mut network = Self::from_profile(profile);
        network.endpoint = endpoint;
        network
    }

    pub fn hydrate(&self, account: &Account) -> Profile {
        let mut profile = Profile {
            id: account.id,
            name: account.name.clone(),
            mode: super::OperatingMode::Vpn,
            frontends: self.frontends,
            transport: self.transport,
            endpoint: account
                .managed_endpoint
                .clone()
                .unwrap_or_else(|| self.endpoint.clone()),
            ip_policy: self.ip_policy,
            mtu: self.mtu,
            dns_mode: self.dns_mode,
            dns_servers: self.dns_servers.clone(),
            allow_lan: self.allow_lan,
            split_exclusions: self.split_exclusions.clone(),
            kill_switch: self.kill_switch,
            auto_connect: self.auto_connect,
            proxy: self.proxy.clone(),
        };
        profile.canonicalize_mode();
        profile.proxy.normalize_auth();
        profile
    }

    pub fn reset_user_defaults(&mut self) {
        let kill_switch = self.kill_switch;
        let auto_connect = self.auto_connect;
        let auth_username = self.proxy.auth_username.clone();
        *self = Self::default();
        self.kill_switch = kill_switch;
        self.auto_connect = auto_connect;
        self.proxy.auth_username = auth_username;
    }
}
