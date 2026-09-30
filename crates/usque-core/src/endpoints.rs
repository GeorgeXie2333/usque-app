//! Canonical Consumer MASQUE endpoint policy. Prefixes here validate targets;
//! they never authorize a prefix route or a wildcard firewall exception.
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::config::EndpointSelection;
use crate::{ConsumerEntitlement, DataPlaneMode, IpPolicy, Profile, Transport, TransportPolicy};

pub const AUTOMATIC_H2_BATCH_SIZE: usize = 10;
pub const AUTOMATIC_H2_BATCH_TIMEOUT: Duration = Duration::from_secs(2);
pub const AUTOMATIC_H2_IPV6_PER_PREFIX: usize = 256;
pub const AUTOMATIC_MAX_CANDIDATES: usize = 1024;
pub const AUTOMATIC_PIN_REFRESH_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EndpointPool {
    Free,
    WarpPlus,
}

impl EndpointPool {
    /// Unknown legacy entitlement uses the endpoints shared with Plus.
    pub const fn from_entitlement(entitlement: Option<ConsumerEntitlement>) -> Self {
        match entitlement {
            Some(ConsumerEntitlement::Free) => Self::Free,
            Some(ConsumerEntitlement::WarpPlus) | None => Self::WarpPlus,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct AutomaticEndpointPolicy {
    pub pool: EndpointPool,
    pub port: u16,
    pub ipv4: bool,
    pub ipv6: bool,
    pub tcp: bool,
    pub udp: bool,
}

impl AutomaticEndpointPolicy {
    pub fn for_profile(profile: &Profile, pool: EndpointPool) -> Self {
        Self {
            pool,
            port: profile.endpoint.port,
            ipv4: profile.ip_policy != IpPolicy::Ipv6Only,
            ipv6: profile.ip_policy != IpPolicy::Ipv4Only,
            tcp: profile.data_plane == DataPlaneMode::ConnectIp
                && profile.transport != TransportPolicy::Http3,
            udp: profile.data_plane == DataPlaneMode::L4Proxy
                || profile.transport != TransportPolicy::Http2,
        }
    }

    pub fn permits(self, endpoint: SocketAddr, transport: Transport) -> bool {
        if self.port == 0 || endpoint.port() != self.port {
            return false;
        }
        let third = match endpoint.ip() {
            IpAddr::V4(ip) if self.ipv4 => {
                let octets = ip.octets();
                if octets[..2] != [162, 159] {
                    return false;
                }
                if transport == Transport::Http3 && !matches!(octets[3], 1 | 2) {
                    return false;
                }
                u16::from(octets[2])
            }
            IpAddr::V6(ip) if self.ipv6 => {
                let segments = ip.segments();
                if segments[..2] != [0x2606, 0x4700] {
                    return false;
                }
                if transport == Transport::Http3
                    && (segments[3..7] != [0, 0, 0, 0] || !matches!(segments[7], 1 | 2))
                {
                    return false;
                }
                match segments[2] {
                    0x103 => 198,
                    0x104 => 199,
                    _ => return false,
                }
            }
            _ => return false,
        };
        let pool_matches = third == 199 || self.pool == EndpointPool::Free && third == 198;
        pool_matches
            && match transport {
                Transport::Http2 => self.tcp,
                Transport::Http3 => self.udp,
            }
    }

    /// Stable, shared Plus-compatible targets for physical-network observation.
    /// These are representatives, not the connection race's selected winner.
    pub fn representative_pair(self) -> (SocketAddr, SocketAddr) {
        (
            SocketAddr::new(Ipv4Addr::new(162, 159, 199, 2).into(), self.port),
            SocketAddr::new(
                Ipv6Addr::new(0x2606, 0x4700, 0x104, 0, 0, 0, 0, 2).into(),
                self.port,
            ),
        )
    }

    pub fn h3_candidates(self) -> Vec<SocketAddr> {
        let mut candidates = Vec::with_capacity(8);
        for third in self.prefixes() {
            for last in [1, 2] {
                let v4 = SocketAddr::new(Ipv4Addr::new(162, 159, third, last).into(), self.port);
                let v6 = SocketAddr::new(
                    Ipv6Addr::new(
                        0x2606,
                        0x4700,
                        if third == 198 { 0x103 } else { 0x104 },
                        0,
                        0,
                        0,
                        0,
                        u16::from(last),
                    )
                    .into(),
                    self.port,
                );
                for endpoint in [v4, v6] {
                    if self.permits(endpoint, Transport::Http3) {
                        candidates.push(endpoint);
                    }
                }
            }
        }
        candidates
    }

    pub fn h2_ipv4_candidates(self) -> Vec<SocketAddr> {
        if !self.tcp || !self.ipv4 {
            return Vec::new();
        }
        self.prefixes()
            .into_iter()
            .flat_map(|third| {
                (0..=255).map(move |last| {
                    SocketAddr::new(Ipv4Addr::new(162, 159, third, last).into(), self.port)
                })
            })
            .collect()
    }

    pub fn ipv6_prefixes(self) -> Vec<Ipv6Addr> {
        if !self.tcp || !self.ipv6 {
            return Vec::new();
        }
        self.prefixes()
            .into_iter()
            .map(|third| {
                Ipv6Addr::new(
                    0x2606,
                    0x4700,
                    if third == 198 { 0x103 } else { 0x104 },
                    0,
                    0,
                    0,
                    0,
                    0,
                )
            })
            .collect()
    }

    fn prefixes(self) -> Vec<u8> {
        match self.pool {
            EndpointPool::Free => vec![198, 199],
            EndpointPool::WarpPlus => vec![199],
        }
    }
}

/// Conservative Free/dual-stack budget: 103 two-second H2 batches plus an
/// eight-second H3 attempt and its one interoperability retry; one complete
/// pin retry, a globally bounded 60-second refresh and setup/cleanup margin.
pub fn endpoint_underlay_budget(profile: &Profile) -> Duration {
    if profile.endpoint.selection == EndpointSelection::Custom {
        return Duration::from_secs(180);
    }
    let dual_stack = !matches!(profile.ip_policy, IpPolicy::Ipv4Only | IpPolicy::Ipv6Only);
    let h2 = profile.data_plane == DataPlaneMode::ConnectIp
        && profile.transport != TransportPolicy::Http3;
    let h3 =
        profile.data_plane == DataPlaneMode::L4Proxy || profile.transport != TransportPolicy::Http2;
    let h2_cycle_ms = if h2 {
        if dual_stack { 206_000_u64 } else { 104_000 }
    } else {
        0
    };
    let h3_cycle_ms = if h3 { 16_250 } else { 0 };
    let milliseconds = 2 * (h2_cycle_ms + h3_cycle_ms) + 60_000 + 15_000;
    Duration::from_secs(milliseconds.div_ceil(1_000).max(180))
}

pub fn endpoint_connection_budget(profile: &Profile) -> Duration {
    let underlay = endpoint_underlay_budget(profile);
    if profile.endpoint.selection == EndpointSelection::Automatic && profile.chain_enabled() {
        underlay + Duration::from_secs(180)
    } else {
        underlay
    }
}

pub fn endpoint_report_budget(profile: &Profile) -> Duration {
    endpoint_connection_budget(profile) + Duration::from_secs(15)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy(pool: EndpointPool) -> AutomaticEndpointPolicy {
        AutomaticEndpointPolicy {
            pool,
            port: 443,
            ipv4: true,
            ipv6: true,
            tcp: true,
            udp: true,
        }
    }

    #[test]
    fn subscription_protocol_and_port_boundaries_are_exact() {
        let free = policy(EndpointPool::Free);
        let plus = policy(EndpointPool::WarpPlus);
        for target in ["162.159.198.1:443", "[2606:4700:103::2]:443"] {
            let target = target.parse().unwrap();
            assert!(free.permits(target, Transport::Http3));
            assert!(!plus.permits(target, Transport::Http2));
        }
        for target in [
            "162.159.199.0:443",
            "162.159.199.255:443",
            "[2606:4700:104:ffff::ffff]:443",
        ] {
            let target = target.parse().unwrap();
            assert!(plus.permits(target, Transport::Http2));
            assert!(!plus.permits(target, Transport::Http3));
        }
        for target in [
            "162.159.199.1:444",
            "[2600:4700:104::1]:443",
            "[2606:4700:105::1]:443",
        ] {
            assert!(!free.permits(target.parse().unwrap(), Transport::Http2));
        }
        assert_eq!(free.h3_candidates().len(), 8);
        assert_eq!(plus.h3_candidates().len(), 4);
        assert_eq!(free.h2_ipv4_candidates().len(), 512);
        assert_eq!(EndpointPool::from_entitlement(None), EndpointPool::WarpPlus);
    }

    #[test]
    fn complete_cycle_budgets_cover_pin_retry_and_preserve_custom_budget() {
        let mut profile = Profile::default();
        assert_eq!(endpoint_underlay_budget(&profile), Duration::from_secs(520));
        profile.transport = TransportPolicy::Http2;
        assert_eq!(endpoint_underlay_budget(&profile), Duration::from_secs(487));
        profile.ip_policy = IpPolicy::Ipv4Only;
        assert_eq!(endpoint_underlay_budget(&profile), Duration::from_secs(283));
        profile.transport = TransportPolicy::Auto;
        assert_eq!(endpoint_underlay_budget(&profile), Duration::from_secs(316));
        profile.transport = TransportPolicy::Http3;
        assert_eq!(endpoint_underlay_budget(&profile), Duration::from_secs(180));
        profile.endpoint.selection = EndpointSelection::Custom;
        profile.transport = TransportPolicy::Auto;
        assert_eq!(endpoint_report_budget(&profile), Duration::from_secs(195));
    }
}
