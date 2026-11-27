use std::net::IpAddr;

use ipnet::IpNet;
use serde::{Deserialize, Serialize};

use super::{
    Account, CongestionControlAlgorithm, DataPlaneMode, DirectDnsSettings, DnsMode,
    EndpointPresetTier, EndpointSettings, FrontendSettings, IpPolicy, Profile, ProxySettings,
    TlsFingerprint, TransportPolicy, WarpDnsSettings,
};
use uuid::Uuid;

/// Active endpoint subscription per Consumer entitlement tier (`Free` vs `WARP+`).
/// Zero Trust accounts store their subscription selection directly on [`Account`].
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConsumerTierSubscriptions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub free: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plus: Option<Uuid>,
}

impl ConsumerTierSubscriptions {
    pub const fn is_empty(&self) -> bool {
        self.free.is_none() && self.plus.is_none()
    }

    pub const fn for_tier(&self, tier: EndpointPresetTier) -> Option<Uuid> {
        match tier {
            EndpointPresetTier::Free => self.free,
            EndpointPresetTier::Plus => self.plus,
            EndpointPresetTier::ZeroTrust => None,
        }
    }

    pub fn set_for_tier(&mut self, tier: EndpointPresetTier, id: Option<Uuid>) {
        match tier {
            EndpointPresetTier::Free => self.free = id,
            EndpointPresetTier::Plus => self.plus = id,
            EndpointPresetTier::ZeroTrust => {}
        }
    }
}

/// Device-wide MASQUE, DNS, proxy, and output settings. A Zero Trust account
/// overlays its registered or explicitly overridden IPv4/IPv6 pair.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SharedNetworkSettings {
    pub frontends: FrontendSettings,
    pub transport: TransportPolicy,
    #[serde(default)]
    pub tls_fingerprint: TlsFingerprint,
    #[serde(default)]
    pub data_plane: DataPlaneMode,
    #[serde(default)]
    pub congestion_control: CongestionControlAlgorithm,
    #[serde(default = "super::default_masque_connections")]
    pub masque_connections: u8,
    pub endpoint: EndpointSettings,
    #[serde(default, skip_serializing_if = "ConsumerTierSubscriptions::is_empty")]
    pub consumer_tier_subscriptions: ConsumerTierSubscriptions,
    pub ip_policy: IpPolicy,
    pub mtu: u16,
    pub dns_mode: DnsMode,
    pub dns_servers: Vec<IpAddr>,
    #[serde(default)]
    pub warp_dns: WarpDnsSettings,
    pub allow_lan: bool,
    #[serde(default)]
    pub disable_quic: bool,
    pub split_exclusions: Vec<IpNet>,
    pub kill_switch: bool,
    pub auto_connect: bool,
    pub proxy: ProxySettings,
    #[serde(default)]
    pub geo_direct_countries: Vec<String>,
    #[serde(default)]
    pub bypass_domains: Vec<String>,
    #[serde(default)]
    pub routing: super::RoutingSettings,
    #[serde(default)]
    pub direct_dns: DirectDnsSettings,
    #[serde(default)]
    pub vpn_gate: crate::vpngate::VpnGateSettings,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chain_exit: Option<crate::chain_exit::ChainExitSettings>,
}

impl Default for SharedNetworkSettings {
    fn default() -> Self {
        Self::from_profile(&Profile::default())
    }
}

impl SharedNetworkSettings {
    pub fn chain_enabled(&self) -> bool {
        self.chain_exit
            .as_ref()
            .map_or(self.vpn_gate.enabled, |s| s.enabled)
    }
    /// Copy device-wide settings from a runtime profile. Zero Trust endpoint
    /// addresses are restored from the account after the shared copy is made.
    pub fn from_profile(profile: &Profile) -> Self {
        Self {
            frontends: profile.frontends,
            transport: profile.transport,
            tls_fingerprint: profile.tls_fingerprint,
            data_plane: profile.data_plane,
            congestion_control: profile.congestion_control,
            masque_connections: profile.masque_connections,
            endpoint: profile.endpoint.clone(),
            consumer_tier_subscriptions: ConsumerTierSubscriptions::default(),
            ip_policy: profile.ip_policy,
            mtu: profile.mtu,
            dns_mode: profile.dns_mode,
            dns_servers: profile.dns_servers.clone(),
            warp_dns: profile.warp_dns.clone(),
            allow_lan: profile.allow_lan,
            disable_quic: profile.disable_quic,
            split_exclusions: profile.split_exclusions.clone(),
            kill_switch: profile.kill_switch,
            auto_connect: profile.auto_connect,
            proxy: profile.proxy.clone(),
            geo_direct_countries: profile.geo_direct_countries.clone(),
            bypass_domains: profile.bypass_domains.clone(),
            routing: profile.routing.clone(),
            direct_dns: profile.direct_dns.clone(),
            vpn_gate: profile.vpn_gate.clone(),
            chain_exit: profile.chain_exit.clone(),
        }
    }

    pub fn hydrate(&self, account: &Account) -> Profile {
        let mut endpoint = self.endpoint.clone();
        if let Some(managed) = &account.managed_endpoint_ips {
            let managed = account
                .zero_trust_endpoint_override
                .as_ref()
                .unwrap_or(managed);
            endpoint.ipv4 = managed.ipv4;
            endpoint.ipv6 = managed.ipv6;
            endpoint.selection = super::EndpointSelection::Custom;
            if let Some(sub_id) = account.zero_trust_subscription_id {
                endpoint.subscription_id = Some(sub_id);
                endpoint.preset_region = None;
            }
        }
        let mut profile = Profile {
            id: account.id,
            name: account.name.clone(),
            mode: super::OperatingMode::Vpn,
            frontends: self.frontends,
            transport: self.transport,
            tls_fingerprint: self.tls_fingerprint,
            data_plane: self.data_plane,
            congestion_control: self.congestion_control,
            masque_connections: self.masque_connections,
            endpoint,
            subscription_endpoint: None,
            ip_policy: self.ip_policy,
            mtu: self.mtu,
            dns_mode: self.dns_mode,
            dns_servers: self.dns_servers.clone(),
            warp_dns: self.warp_dns.clone(),
            allow_lan: self.allow_lan,
            disable_quic: self.disable_quic,
            split_exclusions: self.split_exclusions.clone(),
            kill_switch: self.kill_switch,
            auto_connect: self.auto_connect,
            proxy: self.proxy.clone(),
            geo_direct_countries: self.geo_direct_countries.clone(),
            bypass_domains: self.bypass_domains.clone(),
            routing: self.routing.clone(),
            direct_dns: self.direct_dns.clone(),
            vpn_gate: self.vpn_gate.clone(),
            chain_exit: self.chain_exit.clone(),
        };
        profile.canonicalize_mode();
        profile.proxy.normalize_auth();
        let _ = profile.canonicalize_geo_direct();
        profile.canonicalize_direct_dns();
        profile.canonicalize_warp_dns();
        profile
    }

    pub fn hydrate_for_tier(&self, account: &Account, tier: Option<EndpointPresetTier>) -> Profile {
        let mut profile = self.hydrate(account);
        let sub_id = match tier {
            Some(EndpointPresetTier::ZeroTrust) => account.zero_trust_subscription_id,
            Some(EndpointPresetTier::Free) => self.consumer_tier_subscriptions.free,
            Some(EndpointPresetTier::Plus) => self.consumer_tier_subscriptions.plus,
            None => {
                if account.managed_endpoint_ips.is_some() {
                    account.zero_trust_subscription_id
                } else {
                    None
                }
            }
        };
        profile.endpoint.subscription_id = sub_id;
        if sub_id.is_some() {
            profile.endpoint.selection = super::EndpointSelection::Custom;
            profile.endpoint.preset_region = None;
        }
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
