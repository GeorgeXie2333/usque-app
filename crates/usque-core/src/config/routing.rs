//! Device-wide application routing. Order is presentation-only.
use std::collections::{HashMap, HashSet};
use std::net::IpAddr;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{ConfigError, IpNet, canonical_bypass_domain};

pub const MAX_ROUTING_RULES_PER_KIND: usize = 256;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RoutingAction {
    Direct,
    Reject,
    Proxy,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum RoutingMatch {
    Domain,
    Cidr,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RoutingRule {
    pub id: Uuid,
    pub kind: RoutingMatch,
    pub target: String,
    pub action: RoutingAction,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct RoutingSettings {
    #[serde(default)]
    pub rules: Vec<RoutingRule>,
    #[serde(default)]
    pub ads_enabled: bool,
}

impl RoutingSettings {
    pub fn normalized(&self) -> Result<Self, ConfigError> {
        if self.rules.len() > MAX_ROUTING_RULES_PER_KIND * 2 {
            return Err(ConfigError::RoutingLimit);
        }
        let mut rules: Vec<RoutingRule> = Vec::new();
        let mut ids = HashSet::new();
        let mut targets = HashMap::new();
        let mut counts = [0_usize; 2];
        for rule in &self.rules {
            if rule.id.is_nil() || !ids.insert(rule.id) {
                return Err(ConfigError::InvalidRoutingRule(rule.id));
            }
            let target = match rule.kind {
                RoutingMatch::Domain => canonical_bypass_domain(&rule.target),
                RoutingMatch::Cidr => rule
                    .target
                    .trim()
                    .parse::<IpNet>()
                    .ok()
                    .or_else(|| rule.target.trim().parse::<IpAddr>().ok().map(IpNet::from))
                    .map(|net| canonical_network(net).to_string()),
            }
            .ok_or(ConfigError::InvalidRoutingRule(rule.id))?;
            let key = (rule.kind, target.clone());
            if let Some(&index) = targets.get(&key) {
                let existing: &RoutingRule = &rules[index];
                if existing.action != rule.action {
                    return Err(ConfigError::RoutingConflict(existing.id, rule.id));
                }
                continue;
            }
            let count = &mut counts[usize::from(rule.kind == RoutingMatch::Cidr)];
            *count += 1;
            if *count > MAX_ROUTING_RULES_PER_KIND {
                return Err(ConfigError::RoutingLimit);
            }
            targets.insert(key, rules.len());
            rules.push(RoutingRule {
                target,
                ..rule.clone()
            });
        }
        Ok(Self {
            rules,
            ads_enabled: self.ads_enabled,
        })
    }

    /// Used only at the storage/import compatibility boundary, not by patches.
    pub fn migrate_direct(&mut self, networks: &mut Vec<IpNet>, domains: &mut Vec<String>) {
        for (kind, target) in networks
            .drain(..)
            .map(|net| (RoutingMatch::Cidr, net.to_string()))
            .chain(
                domains
                    .drain(..)
                    .map(|domain| (RoutingMatch::Domain, domain)),
            )
        {
            self.rules.push(RoutingRule {
                id: Uuid::new_v4(),
                kind,
                target,
                action: RoutingAction::Direct,
            });
        }
    }

    pub fn domain_action(&self, host: &str) -> Option<RoutingAction> {
        let host = canonical_bypass_domain(host)?;
        self.rules
            .iter()
            .filter(|rule| rule.kind == RoutingMatch::Domain && domain_matches(&host, &rule.target))
            .max_by_key(|rule| rule.target.len())
            .map(|rule| rule.action)
    }

    pub fn ip_action(&self, ip: IpAddr) -> Option<RoutingAction> {
        let ip = ip.to_canonical();
        self.rules
            .iter()
            .filter(|rule| rule.kind == RoutingMatch::Cidr)
            .filter_map(|rule| {
                rule.target
                    .parse::<IpNet>()
                    .ok()
                    .filter(|net| net.contains(&ip))
                    .map(|net| (net.prefix_len(), rule.action))
            })
            .max_by_key(|(prefix, _)| *prefix)
            .map(|(_, action)| action)
    }

    pub fn has_domain_rules(&self) -> bool {
        self.ads_enabled
            || self
                .rules
                .iter()
                .any(|rule| rule.kind == RoutingMatch::Domain)
    }

    pub fn has_direct_domains(&self) -> bool {
        self.rules
            .iter()
            .any(|rule| rule.kind == RoutingMatch::Domain && rule.action == RoutingAction::Direct)
    }

    pub fn has_direct_rules(&self) -> bool {
        self.rules
            .iter()
            .any(|rule| rule.action == RoutingAction::Direct)
    }

    pub fn has_ip_rules(&self) -> bool {
        self.rules
            .iter()
            .any(|rule| rule.kind == RoutingMatch::Cidr)
    }
}

fn canonical_network(net: IpNet) -> IpNet {
    let net = net.trunc();
    if let IpNet::V6(v6) = net
        && v6.prefix_len() >= 96
        && let Some(v4) = v6.addr().to_ipv4_mapped()
    {
        return IpNet::new(IpAddr::V4(v4), v6.prefix_len() - 96).expect("mapped IPv4 prefix");
    }
    net
}

fn domain_matches(host: &str, domain: &str) -> bool {
    host == domain
        || host
            .strip_suffix(domain)
            .is_some_and(|prefix| prefix.ends_with('.'))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(target: &str, action: RoutingAction) -> RoutingRule {
        RoutingRule {
            id: Uuid::new_v4(),
            kind: if target.contains('/') || target.parse::<IpAddr>().is_ok() {
                RoutingMatch::Cidr
            } else {
                RoutingMatch::Domain
            },
            target: target.into(),
            action,
        }
    }

    #[test]
    fn specificity_not_order_and_label_boundaries() {
        let mut settings = RoutingSettings {
            rules: vec![
                rule("Example.COM.", RoutingAction::Direct),
                rule("ads.example.com", RoutingAction::Reject),
                rule("safe.ads.example.com", RoutingAction::Proxy),
                rule("192.0.2.0/24", RoutingAction::Reject),
                rule("192.0.2.7", RoutingAction::Direct),
                rule("2001:db8::/32", RoutingAction::Reject),
                rule("2001:db8::1", RoutingAction::Proxy),
            ],
            ads_enabled: false,
        }
        .normalized()
        .unwrap();
        for _ in 0..2 {
            assert_eq!(
                settings.domain_action("a.ads.example.com"),
                Some(RoutingAction::Reject)
            );
            assert_eq!(
                settings.domain_action("safe.ads.example.com"),
                Some(RoutingAction::Proxy)
            );
            assert_eq!(
                settings.domain_action("www.example.com"),
                Some(RoutingAction::Direct)
            );
            assert_eq!(settings.domain_action("notexample.com"), None);
            assert_eq!(
                settings.ip_action("192.0.2.7".parse().unwrap()),
                Some(RoutingAction::Direct)
            );
            assert_eq!(
                settings.ip_action("192.0.2.8".parse().unwrap()),
                Some(RoutingAction::Reject)
            );
            assert_eq!(
                settings.ip_action("2001:db8::1".parse().unwrap()),
                Some(RoutingAction::Proxy)
            );
            settings.rules.reverse();
        }
    }

    #[test]
    fn canonical_conflicts_are_private_and_duplicates_merge() {
        let first = rule("BÜCHER.example.", RoutingAction::Direct);
        let second = rule("xn--bcher-kva.example", RoutingAction::Reject);
        let mut settings = RoutingSettings {
            rules: vec![first.clone(), second.clone()],
            ads_enabled: true,
        };
        let error = settings.normalized().unwrap_err();
        assert_eq!(error, ConfigError::RoutingConflict(first.id, second.id));
        assert!(!error.to_string().contains("example"));
        settings.rules[1].action = RoutingAction::Direct;
        let normalized = settings.normalized().unwrap();
        assert_eq!(normalized.rules.len(), 1);
        assert_eq!(normalized.rules[0].id, first.id);
        assert!(normalized.ads_enabled);
    }

    #[test]
    fn mapped_ipv4_cannot_escape_address_rejection() {
        let settings = RoutingSettings {
            rules: vec![rule("192.0.2.0/24", RoutingAction::Reject)],
            ..Default::default()
        }
        .normalized()
        .unwrap();
        assert_eq!(
            settings.ip_action("::ffff:192.0.2.7".parse().unwrap()),
            Some(RoutingAction::Reject)
        );
        let mut same = settings.clone();
        same.rules
            .push(rule("::ffff:192.0.2.0/120", RoutingAction::Direct));
        assert!(matches!(
            same.normalized(),
            Err(ConfigError::RoutingConflict(_, _))
        ));
    }

    #[test]
    fn validation_and_roundtrip() {
        for target in ["https://example.com", "*.example.com", "bad name"] {
            assert!(
                RoutingSettings {
                    rules: vec![rule(target, RoutingAction::Reject)],
                    ..Default::default()
                }
                .normalized()
                .is_err()
            );
        }
        let settings = RoutingSettings {
            rules: vec![rule("192.0.2.99/24", RoutingAction::Reject)],
            ads_enabled: true,
        }
        .normalized()
        .unwrap();
        assert_eq!(settings.rules[0].target, "192.0.2.0/24");
        assert_eq!(
            serde_json::from_str::<RoutingSettings>(&serde_json::to_string(&settings).unwrap())
                .unwrap(),
            settings
        );
        assert!(serde_json::from_str::<RoutingAction>("\"unknown\"").is_err());
    }
}
