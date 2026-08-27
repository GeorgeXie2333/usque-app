use std::{
    net::{IpAddr, SocketAddr},
    str::FromStr,
};

use ipnet::IpNet;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use usque_ipc::agent_v1;
use uuid::Uuid;

pub const MAX_DNS_SERVERS: usize = 8;
pub const MAX_SPLIT_EXCLUSIONS: usize = 128;
pub const MAX_ENDPOINT_CANDIDATES: usize = 2;
pub const MAX_CONTROL_API_CANDIDATES: usize = 16;
pub const MIN_MTU: u16 = 1280;
pub const MAX_MTU: u16 = 9000;
const SPLIT_DNS_IPV4: IpAddr = IpAddr::V4(std::net::Ipv4Addr::new(198, 18, 0, 1));
const SPLIT_DNS_IPV6: IpAddr = IpAddr::V6(std::net::Ipv6Addr::new(0xfd00, 0, 0, 0, 0, 0, 0, 1));

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ValidatedTunnelPlan {
    pub profile_id: Uuid,
    pub endpoint: SocketAddr,
    pub endpoint_candidates: Vec<SocketAddr>,
    #[serde(default)]
    pub control_api_candidates: Vec<SocketAddr>,
    pub mtu: u16,
    pub dns_servers: Vec<IpAddr>,
    pub split_exclusions: Vec<IpNet>,
    pub allow_lan: bool,
    pub kill_switch: bool,
    #[serde(default)]
    pub split_dns: bool,
    pub assigned_ipv4: Option<IpNet>,
    pub assigned_ipv6: Option<IpNet>,
}

impl TryFrom<agent_v1::TunnelPlan> for ValidatedTunnelPlan {
    type Error = PlanError;

    fn try_from(value: agent_v1::TunnelPlan) -> Result<Self, Self::Error> {
        let profile_id =
            Uuid::parse_str(value.profile_id.trim()).map_err(|_| PlanError::ProfileId)?;
        let endpoint =
            SocketAddr::from_str(value.endpoint.trim()).map_err(|_| PlanError::Endpoint)?;
        let mut endpoint_candidates = if value.endpoint_candidates.is_empty() {
            vec![endpoint]
        } else {
            value
                .endpoint_candidates
                .iter()
                .map(|value| {
                    SocketAddr::from_str(value.trim())
                        .map_err(|_| PlanError::EndpointCandidate(value.clone()))
                })
                .collect::<Result<Vec<_>, _>>()?
        };
        endpoint_candidates.sort();
        endpoint_candidates.dedup();
        validate_endpoint_candidates(endpoint, &endpoint_candidates)?;
        let mut control_api_candidates = value
            .control_api_candidates
            .iter()
            .map(|value| {
                SocketAddr::from_str(value.trim())
                    .map_err(|_| PlanError::ControlApiCandidate(value.clone()))
            })
            .collect::<Result<Vec<_>, _>>()?;
        control_api_candidates.sort();
        control_api_candidates.dedup();
        if control_api_candidates.is_empty() {
            return Err(PlanError::ControlApiCandidateCount(0));
        }
        validate_control_api_candidates(&control_api_candidates)?;
        let mtu = u16::try_from(value.mtu).map_err(|_| PlanError::Mtu(value.mtu))?;
        if !(MIN_MTU..=MAX_MTU).contains(&mtu) {
            return Err(PlanError::Mtu(value.mtu));
        }
        if value.dns_servers.is_empty() || value.dns_servers.len() > MAX_DNS_SERVERS {
            return Err(PlanError::DnsCount(value.dns_servers.len()));
        }
        let dns_servers = value
            .dns_servers
            .iter()
            .map(|value| {
                value
                    .trim()
                    .parse::<IpAddr>()
                    .map_err(|_| PlanError::Dns(value.clone()))
            })
            .collect::<Result<Vec<_>, _>>()?;
        if value.split_exclusions.len() > MAX_SPLIT_EXCLUSIONS {
            return Err(PlanError::ExclusionCount(value.split_exclusions.len()));
        }
        let mut split_exclusions = value
            .split_exclusions
            .iter()
            .map(|value| {
                value
                    .trim()
                    .parse::<IpNet>()
                    .map_err(|_| PlanError::Exclusion(value.clone()))
            })
            .collect::<Result<Vec<_>, _>>()?;
        if split_exclusions
            .iter()
            .any(|network| network.prefix_len() == 0)
        {
            return Err(PlanError::DefaultRouteExclusion);
        }
        split_exclusions.sort();
        split_exclusions.dedup();

        let assigned_ipv4 = parse_assignment(&value.assigned_ipv4, false)?;
        let assigned_ipv6 = parse_assignment(&value.assigned_ipv6, true)?;
        if assigned_ipv4.is_none() && assigned_ipv6.is_none() {
            return Err(PlanError::MissingAssignment);
        }
        if dns_servers.iter().all(IpAddr::is_ipv4) && assigned_ipv4.is_none()
            || dns_servers.iter().all(IpAddr::is_ipv6) && assigned_ipv6.is_none()
        {
            return Err(PlanError::DnsFamilyUnavailable);
        }
        validate_split_dns(value.split_dns, &dns_servers, assigned_ipv4, assigned_ipv6)?;

        let plan = Self {
            profile_id,
            endpoint,
            endpoint_candidates,
            control_api_candidates,
            mtu,
            dns_servers,
            split_exclusions,
            allow_lan: value.allow_lan,
            kill_switch: value.kill_switch,
            split_dns: value.split_dns,
            assigned_ipv4,
            assigned_ipv6,
        };
        plan.validate()?;
        Ok(plan)
    }
}

impl ValidatedTunnelPlan {
    pub fn validate(&self) -> Result<(), PlanError> {
        if !(MIN_MTU..=MAX_MTU).contains(&self.mtu) {
            return Err(PlanError::Mtu(u32::from(self.mtu)));
        }
        validate_endpoint_candidates(self.endpoint, &self.endpoint_candidates)?;
        if !self.control_api_candidates.is_empty() {
            validate_control_api_candidates(&self.control_api_candidates)?;
        }
        if self.dns_servers.is_empty() || self.dns_servers.len() > MAX_DNS_SERVERS {
            return Err(PlanError::DnsCount(self.dns_servers.len()));
        }
        if self.split_exclusions.len() > MAX_SPLIT_EXCLUSIONS {
            return Err(PlanError::ExclusionCount(self.split_exclusions.len()));
        }
        if self
            .split_exclusions
            .iter()
            .any(|network| network.prefix_len() == 0)
        {
            return Err(PlanError::DefaultRouteExclusion);
        }
        if self
            .assigned_ipv4
            .is_some_and(|network| !network.addr().is_ipv4())
            || self
                .assigned_ipv6
                .is_some_and(|network| !network.addr().is_ipv6())
        {
            return Err(PlanError::AssignmentFamily("journal assignment".to_owned()));
        }
        if self.assigned_ipv4.is_none() && self.assigned_ipv6.is_none() {
            return Err(PlanError::MissingAssignment);
        }
        if self.dns_servers.iter().all(IpAddr::is_ipv4) && self.assigned_ipv4.is_none()
            || self.dns_servers.iter().all(IpAddr::is_ipv6) && self.assigned_ipv6.is_none()
        {
            return Err(PlanError::DnsFamilyUnavailable);
        }
        validate_split_dns(
            self.split_dns,
            &self.dns_servers,
            self.assigned_ipv4,
            self.assigned_ipv6,
        )?;
        Ok(())
    }
}

fn validate_split_dns(
    enabled: bool,
    dns_servers: &[IpAddr],
    assigned_ipv4: Option<IpNet>,
    assigned_ipv6: Option<IpNet>,
) -> Result<(), PlanError> {
    if !enabled {
        return Ok(());
    }
    let expected = [
        assigned_ipv4.map(|_| SPLIT_DNS_IPV4),
        assigned_ipv6.map(|_| SPLIT_DNS_IPV6),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>();
    if dns_servers != expected {
        return Err(PlanError::SplitDnsServers);
    }
    Ok(())
}

fn validate_control_api_candidates(candidates: &[SocketAddr]) -> Result<(), PlanError> {
    if candidates.len() > MAX_CONTROL_API_CANDIDATES {
        return Err(PlanError::ControlApiCandidateCount(candidates.len()));
    }
    if candidates.iter().any(|candidate| {
        candidate.port() != 443 || candidate.ip().is_unspecified() || candidate.ip().is_multicast()
    }) {
        return Err(PlanError::UnsafeControlApiCandidate);
    }
    Ok(())
}

fn parse_assignment(value: &str, ipv6: bool) -> Result<Option<IpNet>, PlanError> {
    if value.trim().is_empty() {
        return Ok(None);
    }
    let assignment = value
        .trim()
        .parse::<IpNet>()
        .map_err(|_| PlanError::Assignment(value.to_owned()))?;
    if assignment.addr().is_ipv6() != ipv6 {
        return Err(PlanError::AssignmentFamily(value.to_owned()));
    }
    Ok(Some(assignment))
}

fn validate_endpoint_candidates(
    endpoint: SocketAddr,
    candidates: &[SocketAddr],
) -> Result<(), PlanError> {
    if candidates.is_empty() || candidates.len() > MAX_ENDPOINT_CANDIDATES {
        return Err(PlanError::EndpointCandidateCount(candidates.len()));
    }
    if !candidates.contains(&endpoint) {
        return Err(PlanError::EndpointNotCandidate);
    }
    if candidates
        .iter()
        .any(|candidate| candidate.port() != endpoint.port())
    {
        return Err(PlanError::EndpointPortMismatch);
    }
    let ipv4_count = candidates
        .iter()
        .filter(|candidate| candidate.is_ipv4())
        .count();
    let ipv6_count = candidates
        .iter()
        .filter(|candidate| candidate.is_ipv6())
        .count();
    if ipv4_count > 1 || ipv6_count > 1 {
        return Err(PlanError::DuplicateEndpointFamily);
    }
    Ok(())
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum PlanError {
    #[error("profile_id is not a UUID")]
    ProfileId,
    #[error("endpoint must be a numeric IP socket address")]
    Endpoint,
    #[error("endpoint candidate must be a numeric IP socket address: {0}")]
    EndpointCandidate(String),
    #[error("between 1 and {MAX_ENDPOINT_CANDIDATES} endpoint candidates are required, got {0}")]
    EndpointCandidateCount(usize),
    #[error("the selected endpoint is absent from endpoint_candidates")]
    EndpointNotCandidate,
    #[error("all endpoint candidates must use the selected endpoint port")]
    EndpointPortMismatch,
    #[error("endpoint_candidates may contain at most one address of each family")]
    DuplicateEndpointFamily,
    #[error("control API candidate must be a numeric IP socket address: {0}")]
    ControlApiCandidate(String),
    #[error(
        "between 1 and {MAX_CONTROL_API_CANDIDATES} control API candidates are required, got {0}"
    )]
    ControlApiCandidateCount(usize),
    #[error("control API candidates must be usable unicast addresses on TCP/443")]
    UnsafeControlApiCandidate,
    #[error("MTU must be between {MIN_MTU} and {MAX_MTU}, got {0}")]
    Mtu(u32),
    #[error("between 1 and {MAX_DNS_SERVERS} DNS servers are required, got {0}")]
    DnsCount(usize),
    #[error("DNS server is not an IP address: {0}")]
    Dns(String),
    #[error("at most {MAX_SPLIT_EXCLUSIONS} split exclusions are accepted, got {0}")]
    ExclusionCount(usize),
    #[error("split exclusion is not a CIDR: {0}")]
    Exclusion(String),
    #[error("a /0 split exclusion would bypass the complete VPN")]
    DefaultRouteExclusion,
    #[error("assigned address is not a CIDR: {0}")]
    Assignment(String),
    #[error("assigned address has the wrong IP family: {0}")]
    AssignmentFamily(String),
    #[error("at least one assigned WARP address is required")]
    MissingAssignment,
    #[error("configured DNS has no matching assigned address family")]
    DnsFamilyUnavailable,
    #[error("Split DNS requires only the Engine-owned internal DNS addresses")]
    SplitDnsServers,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_plan() -> agent_v1::TunnelPlan {
        agent_v1::TunnelPlan {
            profile_id: Uuid::new_v4().to_string(),
            endpoint: "162.159.198.2:443".to_owned(),
            endpoint_candidates: vec![
                "162.159.198.2:443".to_owned(),
                "[2606:4700:103::2]:443".to_owned(),
            ],
            control_api_candidates: vec![
                "198.51.100.10:443".to_owned(),
                "[2001:db8::10]:443".to_owned(),
            ],
            mtu: 1280,
            dns_servers: vec!["1.1.1.1".to_owned(), "2606:4700:4700::1111".to_owned()],
            split_exclusions: vec!["192.168.0.0/16".to_owned()],
            allow_lan: true,
            kill_switch: true,
            assigned_ipv4: "172.16.0.2/32".to_owned(),
            assigned_ipv6: "2606:4700:110::2/128".to_owned(),
            split_dns: false,
        }
    }

    #[test]
    fn validates_and_normalizes_a_tunnel_plan() {
        let mut plan = valid_plan();
        plan.split_exclusions.push("192.168.0.0/16".to_owned());
        let decoded = ValidatedTunnelPlan::try_from(plan).expect("valid");
        assert_eq!(decoded.mtu, 1280);
        assert_eq!(decoded.split_exclusions.len(), 1);
        assert!(decoded.kill_switch);
    }

    #[test]
    fn rejects_a_full_physical_network_bypass() {
        let mut plan = valid_plan();
        plan.split_exclusions = vec!["0.0.0.0/0".to_owned()];
        assert_eq!(
            ValidatedTunnelPlan::try_from(plan),
            Err(PlanError::DefaultRouteExclusion)
        );
    }

    #[test]
    fn rejects_dns_without_a_matching_tunnel_family() {
        let mut plan = valid_plan();
        plan.assigned_ipv6.clear();
        plan.dns_servers = vec!["2606:4700:4700::1111".to_owned()];
        assert_eq!(
            ValidatedTunnelPlan::try_from(plan),
            Err(PlanError::DnsFamilyUnavailable)
        );
    }

    #[test]
    fn requires_bounded_tcp_443_control_candidates() {
        let mut missing = valid_plan();
        missing.control_api_candidates.clear();
        assert_eq!(
            ValidatedTunnelPlan::try_from(missing),
            Err(PlanError::ControlApiCandidateCount(0))
        );

        let mut unsafe_port = valid_plan();
        unsafe_port.control_api_candidates = vec!["198.51.100.10:80".to_owned()];
        assert_eq!(
            ValidatedTunnelPlan::try_from(unsafe_port),
            Err(PlanError::UnsafeControlApiCandidate)
        );
    }

    #[test]
    fn legacy_recovery_plan_without_control_candidates_remains_restorable() {
        let validated = ValidatedTunnelPlan::try_from(valid_plan()).unwrap();
        let mut json = serde_json::to_value(validated).unwrap();
        json.as_object_mut()
            .unwrap()
            .remove("control_api_candidates");
        let recovered: ValidatedTunnelPlan = serde_json::from_value(json).unwrap();
        assert!(recovered.control_api_candidates.is_empty());
        recovered.validate().unwrap();
    }

    #[test]
    fn split_dns_accepts_only_engine_internal_addresses() {
        let mut plan = valid_plan();
        plan.split_dns = true;
        plan.dns_servers = vec!["198.18.0.1".to_owned(), "fd00::1".to_owned()];
        assert!(ValidatedTunnelPlan::try_from(plan).is_ok());

        let mut unsafe_plan = valid_plan();
        unsafe_plan.split_dns = true;
        assert_eq!(
            ValidatedTunnelPlan::try_from(unsafe_plan),
            Err(PlanError::SplitDnsServers)
        );
    }
}
