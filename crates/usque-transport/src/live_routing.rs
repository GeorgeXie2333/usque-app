//! Session-owned immutable routing revisions. Publication never mutates a reader.
use std::sync::{Arc, RwLock};

use usque_core::config::IpNet;
use usque_core::{ConfigError, RoutingAction, RoutingMatch, RoutingSettings};

#[derive(Debug)]
pub(crate) struct RoutingSnapshot {
    pub(crate) revision: u64,
    pub(crate) settings: RoutingSettings,
    pub(crate) networks: Vec<(IpNet, RoutingAction)>,
    pub(crate) domains: Vec<(String, RoutingAction)>,
}

impl RoutingSnapshot {
    pub(crate) fn new(settings: &RoutingSettings, revision: u64) -> Result<Self, ConfigError> {
        let settings = settings.normalized()?;
        let mut networks = Vec::new();
        let mut domains = Vec::new();
        for rule in &settings.rules {
            match rule.kind {
                RoutingMatch::Cidr => networks.push((
                    rule.target
                        .parse::<IpNet>()
                        .map_err(|_| ConfigError::InvalidRoutingRule(rule.id))?,
                    rule.action,
                )),
                RoutingMatch::Domain => domains.push((rule.target.clone(), rule.action)),
            }
        }
        networks.sort_by_key(|(net, _)| std::cmp::Reverse(net.prefix_len()));
        domains.sort_by_key(|(domain, _)| std::cmp::Reverse(domain.len()));
        Ok(Self {
            revision,
            settings,
            networks,
            domains,
        })
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
