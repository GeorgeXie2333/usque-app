//! Native receipts are independent of Windows/Linux resource identifiers.
use super::{journal::Evidence, transaction::Fence};
use ipnet::IpNet;
use serde::{Deserialize, Serialize};
use std::{io, net::IpAddr};
use uuid::Uuid;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct DnsReceipt {
    pub service: Uuid,
    pub before: Option<Vec<u8>>,
    pub applied: Vec<u8>,
    pub restored: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RouteReceipt {
    pub destination: IpNet,
    pub interface: String,
    pub interface_index: u32,
    pub restored: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PfReceipt {
    pub removal_intended: bool,
    pub anchor: String,
    pub enable_intended: bool,
    pub enable_token: Option<u64>,
    pub reference: Option<super::pf_reference::Reference>,
    /// Both accepted states during a crash between rule replacement and save.
    pub previous_rules: Option<String>,
    pub intended_rules: String,
    pub restored: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct InterfaceReceipt {
    pub name: String,
    pub index: u32,
    pub addresses: Vec<IpNet>,
    pub restored: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct NativeReceipts {
    pub boot: Uuid,
    #[serde(default)]
    pub control_api_candidates: Vec<std::net::SocketAddr>,
    pub fence: Fence,
    pub pf: Option<PfReceipt>,
    pub interface: Option<InterfaceReceipt>,
    pub dns: Vec<DnsReceipt>,
    pub routes: Vec<RouteReceipt>,
}
impl Evidence for NativeReceipts {
    fn validate(&self) -> io::Result<()> {
        let invalid = || {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid macOS resource receipts",
            )
        };
        if self.boot.is_nil()
            || self.fence.session.is_nil()
            || self.fence.configuration == 0
            || self.fence.network == 0
            || self.control_api_candidates.len() > 16
            || self
                .control_api_candidates
                .iter()
                .any(|a| a.port() != 443 || !usable_address(a.ip()))
            || self.dns.len() > 64
            // 8 IPv4 and 16 IPv6 capture prefixes, 2 subnets per physical
            // alias (16 per family), and up to 8 host routes for DNS.
            || self.routes.len() > 96
        {
            return Err(invalid());
        }
        if let Some(pf) = &self.pf
            && (pf.anchor != format!("com.apple/zzzz.usque.{}", self.fence.session.simple())
                || (pf.intended_rules.trim().is_empty() != pf.removal_intended)
                || pf.intended_rules.len() > 128 * 1024
                || pf
                    .previous_rules
                    .as_ref()
                    .is_some_and(|s| s.len() > 128 * 1024)
                || pf.enable_token == Some(0)
                || pf.reference.as_ref().is_some_and(|r| {
                    r.pid == 0 || r.created_min > r.created_max || Some(r.token) != pf.enable_token
                })
                || (pf.enable_token.is_some() != pf.reference.is_some())
                || (pf.enable_token.is_some() && !pf.enable_intended))
        {
            return Err(invalid());
        }
        if let Some(interface) = &self.interface
            && (!interface_name(&interface.name)
                || !interface
                    .name
                    .strip_prefix("utun")
                    .is_some_and(|s| s.parse::<u32>().is_ok_and(|n| n.to_string() == s))
                || interface.index == 0
                || interface.addresses.len() > 4
                || interface
                    .addresses
                    .iter()
                    .any(|a| !usable_address(a.addr())))
        {
            return Err(invalid());
        }
        let mut services = std::collections::HashSet::new();
        for dns in &self.dns {
            if dns.service.is_nil()
                || !services.insert(dns.service)
                || dns.applied.is_empty()
                || dns.applied.len() > 32 * 1024
                || dns.before.as_ref().is_some_and(|b| b.len() > 32 * 1024)
            {
                return Err(invalid());
            }
        }
        let mut destinations = std::collections::HashSet::new();
        for route in &self.routes {
            if !destinations.insert(route.destination)
                || self
                    .interface
                    .as_ref()
                    .is_none_or(|i| i.name != route.interface || i.index != route.interface_index)
            {
                return Err(invalid());
            }
        }
        Ok(())
    }
}
pub fn interface_name(name: &str) -> bool {
    !name.is_empty() && name.len() < 16 && name.bytes().all(|b| b.is_ascii_alphanumeric())
}
pub fn usable_address(ip: IpAddr) -> bool {
    !ip.is_unspecified() && !ip.is_multicast() && !ip.is_loopback()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> NativeReceipts {
        NativeReceipts {
            boot: Uuid::from_u128(42),
            control_api_candidates: vec![],
            fence: Fence {
                session: Uuid::new_v4(),
                configuration: 1,
                network: 1,
            },
            pf: None,
            interface: None,
            dns: vec![],
            routes: vec![],
        }
    }
    #[test]
    fn receipts_reject_unbounded_payloads_and_unowned_anchor_names() {
        let valid = fixture();
        valid.validate().unwrap();
        let mut bad = valid.clone();
        bad.fence.network = 0;
        assert!(bad.validate().is_err());
        let mut bad = valid.clone();
        bad.dns = vec![DnsReceipt {
            service: Uuid::nil(),
            before: None,
            applied: vec![],
            restored: false,
        }];
        assert!(bad.validate().is_err());
        let mut bad = valid.clone();
        bad.pf = Some(PfReceipt {
            removal_intended: false,
            anchor: "com.apple".into(),
            enable_intended: false,
            enable_token: None,
            reference: None,
            previous_rules: None,
            intended_rules: "block all".into(),
            restored: false,
        });
        assert!(bad.validate().is_err());
        let mut good = valid;
        good.pf = Some(PfReceipt {
            removal_intended: false,
            anchor: format!("com.apple/zzzz.usque.{}", good.fence.session.simple()),
            enable_intended: false,
            enable_token: None,
            reference: None,
            previous_rules: None,
            intended_rules: "block drop out quick all\n".into(),
            restored: false,
        });
        good.validate().unwrap();
        good.pf.as_mut().unwrap().intended_rules = "x".repeat(128 * 1024 + 1);
        assert!(good.validate().is_err());
    }
    #[test]
    fn route_receipts_cannot_target_unrelated_interfaces_or_overlap_identities() {
        let mut value = fixture();
        value.interface = Some(InterfaceReceipt {
            name: "utun7".into(),
            index: 44,
            addresses: vec!["10.0.0.1/32".parse().unwrap()],
            restored: false,
        });
        value.routes.push(RouteReceipt {
            destination: "0.0.0.0/1".parse().unwrap(),
            interface: "utun7".into(),
            interface_index: 44,
            restored: false,
        });
        value.validate().unwrap();
        value.routes[0].interface = "en0".into();
        assert!(value.validate().is_err());
        value.routes[0].interface = "utun7".into();
        value.routes.push(value.routes[0].clone());
        assert!(value.validate().is_err());
    }
}
