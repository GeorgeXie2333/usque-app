//! Session-owned immutable routing revisions. Publication never mutates a reader.
use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{Arc, RwLock};

use usque_core::config::IpNet;
use usque_core::{ConfigError, RoutingAction, RoutingMatch, RoutingSettings};

/// Longest-prefix CIDR lookup: one hash table per prefix length present,
/// probed from the longest prefix down. A lookup costs at most one probe per
/// distinct prefix length, independent of the number of rules.
#[derive(Debug, Default)]
struct PrefixLevels(Vec<(u8, HashMap<u128, RoutingAction>)>);

impl PrefixLevels {
    fn key(bits: u128, width: u8, prefix: u8) -> u128 {
        bits.checked_shr(u32::from(width - prefix)).unwrap_or(0)
    }

    fn insert(&mut self, bits: u128, width: u8, prefix: u8, action: RoutingAction) {
        let index = match self.0.binary_search_by(|(len, _)| prefix.cmp(len)) {
            Ok(index) => index,
            Err(index) => {
                self.0.insert(index, (prefix, HashMap::new()));
                index
            }
        };
        self.0[index]
            .1
            .insert(Self::key(bits, width, prefix), action);
    }

    fn lookup(&self, bits: u128, width: u8) -> Option<RoutingAction> {
        self.0
            .iter()
            .find_map(|(prefix, table)| table.get(&Self::key(bits, width, *prefix)).copied())
    }
}

#[derive(Debug, Default)]
struct CidrIndex {
    v4: PrefixLevels,
    v6: PrefixLevels,
}

impl CidrIndex {
    fn insert(&mut self, net: IpNet, action: RoutingAction) {
        match net {
            IpNet::V4(net) => self.v4.insert(
                u128::from(u32::from(net.network())),
                32,
                net.prefix_len(),
                action,
            ),
            IpNet::V6(net) => {
                self.v6
                    .insert(u128::from(net.network()), 128, net.prefix_len(), action);
            }
        }
    }

    /// Mapped IPv4 addresses match IPv4 rules, as normalization stores them.
    fn lookup(&self, ip: IpAddr) -> Option<RoutingAction> {
        match ip.to_canonical() {
            IpAddr::V4(ip) => self.v4.lookup(u128::from(u32::from(ip)), 32),
            IpAddr::V6(ip) => self.v6.lookup(u128::from(ip), 128),
        }
    }

    fn is_empty(&self) -> bool {
        self.v4.0.is_empty() && self.v6.0.is_empty()
    }
}

#[derive(Debug)]
pub(crate) struct RoutingSnapshot {
    pub(crate) revision: u64,
    pub(crate) settings: RoutingSettings,
    networks: CidrIndex,
    /// Canonical domain -> action. The most specific rule is the longest
    /// matching suffix, found first while stripping labels from the host.
    domains: HashMap<String, RoutingAction>,
    has_direct_rules: bool,
    has_direct_domains: bool,
}

impl RoutingSnapshot {
    pub(crate) fn new(settings: &RoutingSettings, revision: u64) -> Result<Self, ConfigError> {
        let settings = settings.normalized()?;
        let mut networks = CidrIndex::default();
        let mut domains = HashMap::new();
        for rule in &settings.rules {
            match rule.kind {
                RoutingMatch::Cidr => networks.insert(
                    rule.target
                        .parse::<IpNet>()
                        .map_err(|_| ConfigError::InvalidRoutingRule(rule.id))?,
                    rule.action,
                ),
                RoutingMatch::Domain => {
                    domains.insert(rule.target.clone(), rule.action);
                }
            }
        }
        Ok(Self {
            revision,
            has_direct_rules: settings.has_direct_rules(),
            has_direct_domains: settings.has_direct_domains(),
            settings,
            networks,
            domains,
        })
    }

    /// `host` must already be canonical (see `canonical_bypass_domain`).
    pub(crate) fn domain_action(&self, host: &str) -> Option<RoutingAction> {
        if self.domains.is_empty() {
            return None;
        }
        let mut suffix = host;
        loop {
            if let Some(action) = self.domains.get(suffix) {
                return Some(*action);
            }
            suffix = suffix.split_once('.')?.1;
        }
    }

    pub(crate) fn ip_action(&self, ip: IpAddr) -> Option<RoutingAction> {
        self.networks.lookup(ip)
    }

    pub(crate) fn has_ip_rules(&self) -> bool {
        !self.networks.is_empty()
    }

    pub(crate) fn has_direct_rules(&self) -> bool {
        self.has_direct_rules
    }

    pub(crate) fn has_direct_domains(&self) -> bool {
        self.has_direct_domains
    }
}

#[derive(Debug)]
pub(crate) struct LiveRouting {
    current: RwLock<Arc<RoutingSnapshot>>,
}

impl Default for LiveRouting {
    fn default() -> Self {
        Self::new(RoutingSnapshot::new(&RoutingSettings::default(), 1).expect("empty routing"))
    }
}

impl LiveRouting {
    pub(crate) fn new(snapshot: RoutingSnapshot) -> Self {
        Self {
            current: RwLock::new(Arc::new(snapshot)),
        }
    }

    pub(crate) fn snapshot(&self) -> Arc<RoutingSnapshot> {
        Arc::clone(&self.current.read().unwrap_or_else(|e| e.into_inner()))
    }

    pub(crate) fn publish(
        &self,
        base_revision: u64,
        next: Arc<RoutingSnapshot>,
    ) -> Result<(), RoutingUpdateError> {
        let mut current = self.current.write().unwrap_or_else(|e| e.into_inner());
        if current.revision != base_revision {
            return Err(RoutingUpdateError::RevisionChanged);
        }
        // Ads/QUIC use the same engine transaction. Unchanged routing must not
        // invalidate HTTP idle connections or in-flight DNS and cached hints.
        if Arc::ptr_eq(&current, &next) {
            return Ok(());
        }
        let next_revision = base_revision
            .checked_add(1)
            .ok_or(RoutingUpdateError::RevisionExhausted)?;
        if next.revision != next_revision {
            return Err(RoutingUpdateError::RevisionChanged);
        }
        *current = next;
        Ok(())
    }

    // The guard must cover the hint write, not merely an earlier comparison.
    // Callers hold no routing guard over awaits; lock order is routing -> hints.
    pub(crate) fn with_current<T>(
        &self,
        revision: u64,
        operation: impl FnOnce() -> T,
    ) -> Option<T> {
        let current = self.current.read().unwrap_or_else(|e| e.into_inner());
        (current.revision == revision).then(operation)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RoutingUpdateError {
    #[error("routing update belongs to a different runtime")]
    OwnerChanged,
    #[error("routing revision changed before publication")]
    RevisionChanged,
    #[error("routing revision is exhausted")]
    RevisionExhausted,
}

/// Validated rule revision, bound to the exact runtime that prepared it.
#[derive(Debug)]
pub struct PreparedRoutingUpdate {
    pub(crate) owner: Arc<LiveRouting>,
    pub(crate) base_revision: u64,
    pub(crate) next: Arc<RoutingSnapshot>,
    pub(crate) changes_rules: bool,
    pub(crate) direct_routes: bool,
    pub(crate) direct_dns: bool,
}

impl PreparedRoutingUpdate {
    pub fn changes_rules(&self) -> bool {
        self.changes_rules
    }

    pub fn has_direct_routes(&self) -> bool {
        self.direct_routes
    }

    pub fn needs_direct_dns(&self) -> bool {
        self.direct_dns
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use usque_core::RoutingRule;

    const ACTIONS: [RoutingAction; 3] = [
        RoutingAction::Direct,
        RoutingAction::Reject,
        RoutingAction::Proxy,
    ];

    /// Few labels and a narrow address space force nested, overlapping rules.
    fn domain() -> impl Strategy<Value = String> {
        prop::collection::vec(
            prop::sample::select(vec!["a", "b", "ex", "xn--bcher-kva"]),
            1..5,
        )
        .prop_map(|labels| labels.join("."))
    }

    fn ipv4() -> impl Strategy<Value = std::net::Ipv4Addr> {
        (0_u8..2, any::<u8>(), any::<u8>())
            .prop_map(|(third, fourth, noise)| [192, 0, third, fourth ^ (noise & 3)].into())
    }

    fn ipv6() -> impl Strategy<Value = std::net::Ipv6Addr> {
        (0_u16..3, any::<u16>()).prop_map(|(group, tail)| {
            std::net::Ipv6Addr::new(0x2001, 0xdb8, group, 0, 0, 0, 0, tail)
        })
    }

    fn address() -> impl Strategy<Value = IpAddr> {
        prop_oneof![
            ipv4().prop_map(IpAddr::V4),
            ipv6().prop_map(IpAddr::V6),
            ipv4().prop_map(|ip| IpAddr::V6(ip.to_ipv6_mapped())),
        ]
    }

    fn target() -> impl Strategy<Value = String> {
        prop_oneof![
            domain(),
            (ipv4(), 0_u8..=32).prop_map(|(ip, prefix)| format!("{ip}/{prefix}")),
            (ipv6(), 0_u8..=128).prop_map(|(ip, prefix)| format!("{ip}/{prefix}")),
            (ipv4(), 96_u8..=128)
                .prop_map(|(ip, prefix)| format!("{}/{prefix}", ip.to_ipv6_mapped())),
        ]
    }

    fn settings(entries: Vec<(String, usize)>) -> RoutingSettings {
        let mut seen = std::collections::HashSet::new();
        let rules = entries
            .into_iter()
            .filter_map(|(target, action)| {
                let kind = if target.contains('/') {
                    RoutingMatch::Cidr
                } else {
                    RoutingMatch::Domain
                };
                let rule = RoutingRule {
                    id: uuid::Uuid::new_v4(),
                    kind,
                    target,
                    action: ACTIONS[action],
                };
                // Keep the first action per canonical target; conflicts are
                // rejected by normalization and are not what this test is about.
                let canonical = RoutingSettings {
                    rules: vec![rule.clone()],
                    ads_enabled: false,
                }
                .normalized()
                .ok()?
                .rules
                .remove(0)
                .target;
                seen.insert((kind, canonical)).then_some(rule)
            })
            .collect();
        RoutingSettings {
            rules,
            ads_enabled: false,
        }
    }

    proptest! {
        #[test]
        fn index_matches_the_reference_specificity(
            entries in prop::collection::vec((target(), 0_usize..3), 0..48),
            hosts in prop::collection::vec(domain(), 1..16),
            addresses in prop::collection::vec(address(), 1..16),
        ) {
            let settings = settings(entries).normalized().unwrap();
            let snapshot = RoutingSnapshot::new(&settings, 1).unwrap();
            for host in &hosts {
                let canonical = usque_core::config::canonical_bypass_domain(host).unwrap();
                prop_assert_eq!(snapshot.domain_action(&canonical), settings.domain_action(host));
            }
            for ip in &addresses {
                prop_assert_eq!(snapshot.ip_action(*ip), settings.ip_action(*ip));
            }
            prop_assert_eq!(snapshot.has_ip_rules(), settings.has_ip_rules());
            prop_assert_eq!(snapshot.has_direct_rules(), settings.has_direct_rules());
            prop_assert_eq!(snapshot.has_direct_domains(), settings.has_direct_domains());
        }
    }

    #[test]
    fn full_rule_sets_build_and_match_without_scanning() {
        let kind_limit = usque_core::config::MAX_ROUTING_RULES_PER_KIND;
        let mut rules = Vec::with_capacity(kind_limit * 2);
        for index in 0..kind_limit {
            let octets = u32::try_from(index).unwrap().to_be_bytes();
            rules.push(RoutingRule {
                id: uuid::Uuid::new_v4(),
                kind: RoutingMatch::Cidr,
                target: format!("10.{}.{}.0/24", octets[2], octets[3]),
                action: RoutingAction::Direct,
            });
            rules.push(RoutingRule {
                id: uuid::Uuid::new_v4(),
                kind: RoutingMatch::Domain,
                target: format!("host{index}.example"),
                action: RoutingAction::Reject,
            });
        }
        let settings = RoutingSettings {
            rules,
            ads_enabled: false,
        };
        let started = std::time::Instant::now();
        let snapshot = RoutingSnapshot::new(&settings, 1).unwrap();
        for index in 0..20_000_u32 {
            let rule = index as usize % kind_limit;
            let octets = u32::try_from(rule).unwrap().to_be_bytes();
            let ip = IpAddr::from([10, octets[2], octets[3], 7]);
            assert_eq!(snapshot.ip_action(ip), Some(RoutingAction::Direct));
            assert_eq!(
                snapshot.domain_action(&format!("a.b.host{rule}.example")),
                Some(RoutingAction::Reject)
            );
            assert_eq!(snapshot.domain_action("unmatched.test"), None);
        }
        // Generous even for unoptimized builds; a linear scan per lookup at
        // this size takes far longer.
        assert!(started.elapsed() < std::time::Duration::from_secs(10));
    }

    #[test]
    fn publication_waits_for_revision_scoped_hint_commit() {
        let live = Arc::new(LiveRouting::default());
        let next = Arc::new(RoutingSnapshot::new(&RoutingSettings::default(), 2).unwrap());
        let (started, attempted) = std::sync::mpsc::channel();
        let (done, completed) = std::sync::mpsc::channel();
        let writer = live.clone();
        let thread = live
            .with_current(1, || {
                let thread = std::thread::spawn(move || {
                    started.send(()).unwrap();
                    let result = writer.publish(1, next);
                    done.send(result).unwrap();
                });
                attempted
                    .recv_timeout(std::time::Duration::from_secs(2))
                    .unwrap();
                assert_eq!(
                    completed.try_recv(),
                    Err(std::sync::mpsc::TryRecvError::Empty)
                );
                thread
            })
            .unwrap();
        assert_eq!(
            completed
                .recv_timeout(std::time::Duration::from_secs(2))
                .unwrap(),
            Ok(())
        );
        thread.join().unwrap();
        assert!(
            live.with_current(1, || panic!("retired revision callback"))
                .is_none()
        );
        assert_eq!(live.snapshot().revision, 2);
    }

    #[test]
    fn exhausted_revision_is_rejected_without_wraparound() {
        let current = RoutingSnapshot::new(&RoutingSettings::default(), u64::MAX).unwrap();
        let live = LiveRouting::new(current);
        let next = Arc::new(RoutingSnapshot::new(&RoutingSettings::default(), 0).unwrap());
        assert_eq!(
            live.publish(u64::MAX, next),
            Err(RoutingUpdateError::RevisionExhausted)
        );
        assert_eq!(live.snapshot().revision, u64::MAX);
    }
}
