use super::{SelectedNode, error};
use crate::chain_exit::ImportError;
use serde::{Deserialize, Serialize};

const MAX_PEERS: usize = 32;
const PERMISSIONS_TTL: i64 = 300;
const DIRECTORY_TTL: i64 = 900;

/// Only parsed peer hints enter the encrypted account sidecar. API responses,
/// feature authorization and local-agent admission are deliberately excluded.
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectionCache {
    pub permissions_at: i64,
    pub directory_at: i64,
    pub peers: Vec<SelectedNode>,
}

fn fresh(at: i64, now: i64, ttl: i64) -> bool {
    at > 0
        && now
            .checked_sub(at)
            .is_some_and(|age| (0..ttl).contains(&age))
}

impl ConnectionCache {
    pub fn permissions_fresh(&self, now: i64) -> bool {
        fresh(self.permissions_at, now, PERMISSIONS_TTL)
    }
    pub fn permissions_refresh_due(&self, now: i64) -> bool {
        !fresh(self.permissions_at, now, PERMISSIONS_TTL / 2)
    }
    pub fn directory_refresh_due(&self, now: i64) -> bool {
        !fresh(self.directory_at, now, DIRECTORY_TTL / 2)
    }
    pub fn peer(&self, selected: Option<&str>, now: i64) -> Option<&SelectedNode> {
        if !fresh(self.directory_at, now, DIRECTORY_TTL) {
            return None;
        }
        match selected {
            Some(id) => self.peers.iter().find(|node| node.metadata.id == id),
            None => self.peers.first(),
        }
    }
    pub fn set_directory(&mut self, nodes: &[SelectedNode], selected: Option<&str>, now: i64) {
        self.peers = nodes.iter().take(MAX_PEERS).cloned().collect();
        if let Some(node) = selected.and_then(|id| nodes.iter().find(|n| n.metadata.id == id))
            && !self.peers.iter().any(|n| n.metadata.id == node.metadata.id)
        {
            if self.peers.len() == MAX_PEERS {
                self.peers.pop();
            }
            self.peers.push(node.clone());
        }
        self.directory_at = now;
    }
    pub(super) fn validate(&self) -> Result<(), ImportError> {
        if self.permissions_at < 0 || self.directory_at < 0 || self.peers.len() > MAX_PEERS {
            return Err(error("invalid_record"));
        }
        let mut ids = std::collections::BTreeSet::new();
        for node in &self.peers {
            node.validate()?;
            if !ids.insert(&node.metadata.id) {
                return Err(error("invalid_record"));
            }
        }
        Ok(())
    }
}
