//! Device-wide application routing. Order is presentation-only.
use std::collections::{HashMap, HashSet};
use std::net::IpAddr;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{ConfigError, IpNet, canonical_bypass_domain};

pub const MAX_ROUTING_RULES_PER_KIND: usize = 4096;
/// Bound on the canonical targets of all rules together. With the per-kind
/// count it keeps one encoded rule set near 1 MiB, so every message and store
/// that carries the device-wide rules stays within its own size limit.
pub const MAX_ROUTING_TARGET_BYTES: usize = 384 * 1024;

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
        let mut target_bytes = 0_usize;
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
            target_bytes += target.len();
            if *count > MAX_ROUTING_RULES_PER_KIND || target_bytes > MAX_ROUTING_TARGET_BYTES {
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

/// The largest rule set by encoded size: both kinds full and the whole
/// target budget spent. Size tests for each message and store that carries
/// the device-wide rules use it.
#[doc(hidden)]
pub fn largest_routing_settings() -> RoutingSettings {
    // Eight four-digit groups and "/128": the longest canonical CIDR.
    const CIDR_BYTES: usize = 43;
    let per_kind = MAX_ROUTING_RULES_PER_KIND;
    let domain_bytes = (MAX_ROUTING_TARGET_BYTES - per_kind * CIDR_BYTES) / per_kind;
    let rule = |kind, target| RoutingRule {
        id: Uuid::new_v4(),
        kind,
        target,
        action: RoutingAction::Reject,
    };
    let mut rules = Vec::with_capacity(per_kind * 2);
    for index in 0..per_kind {
        let [high, low] = u16::try_from(index).expect("rule index").to_be_bytes();
        let (high, low) = (0x8000 | u16::from(high), 0x8000 | u16::from(low));
        rules.push(rule(
            RoutingMatch::Cidr,
            format!("2001:db8f:aaaa:bbbb:cccc:dddd:{high:04x}:{low:04x}/128"),
        ));
        rules.push(rule(
            RoutingMatch::Domain,
            format!("h{index:04}.{}", "a".repeat(domain_bytes - 6)),
        ));
    }
    RoutingSettings {
        rules,
        ads_enabled: true,
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

    fn bulk(
        kind: RoutingMatch,
        count: usize,
        target: impl Fn(usize) -> String,
    ) -> Vec<RoutingRule> {
        (0..count)
            .map(|index| RoutingRule {
                id: Uuid::new_v4(),
                kind,
                target: target(index),
                action: RoutingAction::Direct,
            })
            .collect()
    }

    fn cidr(index: usize) -> String {
        let [_, _, high, low] = u32::try_from(index).unwrap().to_be_bytes();
        format!("10.{high}.{low}.0/24")
    }

    #[test]
    fn rule_count_limit_is_per_kind() {
        let mut rules = bulk(RoutingMatch::Cidr, MAX_ROUTING_RULES_PER_KIND, cidr);
        rules.extend(bulk(
            RoutingMatch::Domain,
            MAX_ROUTING_RULES_PER_KIND,
            |index| format!("h{index}.example"),
        ));
        let mut settings = RoutingSettings {
            rules,
            ads_enabled: false,
        };
        assert_eq!(
            settings.normalized().unwrap().rules.len(),
            MAX_ROUTING_RULES_PER_KIND * 2
        );
        // A full kind rejects one more target even when the other has room.
        settings.rules.truncate(MAX_ROUTING_RULES_PER_KIND);
        settings
            .rules
            .extend(bulk(RoutingMatch::Cidr, 1, |_| "192.0.2.0/24".into()));
        assert_eq!(settings.normalized(), Err(ConfigError::RoutingLimit));
    }

    #[test]
    fn total_target_length_is_bounded() {
        // 63-octet labels make the longest valid names (253 octets).
        let name = |index: usize| {
            let label = format!("h{index:0>62}");
            format!("{label}.{label}.{label}.{}", "a".repeat(61))
        };
        assert_eq!(name(0).len(), 253);
        let fits = MAX_ROUTING_TARGET_BYTES / 253;
        let mut settings = RoutingSettings {
            rules: bulk(RoutingMatch::Domain, fits, name),
            ads_enabled: false,
        };
        assert_eq!(settings.normalized().unwrap().rules.len(), fits);
        settings
            .rules
            .extend(bulk(RoutingMatch::Domain, 1, |_| name(fits)));
        assert_eq!(settings.normalized(), Err(ConfigError::RoutingLimit));
    }

    #[test]
    fn largest_rule_set_is_valid_and_spends_the_budget() {
        let settings = largest_routing_settings().normalized().unwrap();
        assert_eq!(settings.rules.len(), MAX_ROUTING_RULES_PER_KIND * 2);
        let bytes: usize = settings.rules.iter().map(|rule| rule.target.len()).sum();
        assert!(bytes <= MAX_ROUTING_TARGET_BYTES);
        assert!(bytes + MAX_ROUTING_RULES_PER_KIND * 2 > MAX_ROUTING_TARGET_BYTES);
        // One JSON copy stays near 1 MiB; transports budget for this bound.
        let json = serde_json::to_vec(&settings).unwrap();
        assert!(json.len() < 1_250_000, "{}", json.len());
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
