use super::{SelectedNode, error};
use crate::chain_exit::ImportError;
use serde::{Deserialize, Serialize};

const MAX_PEERS: usize = super::MAX_CATALOG_NODES;
const MAX_PEER_BYTES: usize = 4 * 1024 * 1024;
// Match the Windows client's VpnPlanRequestInterval, ServerUpdateInterval and
// ClientConfigUpdateInterval. These hints never replace live agent admission.
const PERMISSIONS_TTL: i64 = 12 * 60 * 60;
const DIRECTORY_TTL: i64 = 12 * 60 * 60;
const CONFIG_TTL: i64 = 12 * 60 * 60;

/// Only parsed peer hints enter the encrypted account sidecar. API responses,
/// feature authorization and local-agent admission are deliberately excluded.
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectionCache {
    pub permissions_at: i64,
    pub directory_at: i64,
    pub peers: Vec<SelectedNode>,
    #[serde(default)]
    pub config_at: i64,
    #[serde(default)]
    pub udp_ports: Vec<u16>,
    #[serde(default)]
    pub directory_complete: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub directory_last_modified: Option<String>,
}

/// An opaque validator from the authenticated, fixed API origin. Bound it before
/// persisting or reflecting it into a header; it is never an authorization token.
pub fn valid_directory_validator(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b" ,:-".contains(&b))
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
        !self.permissions_fresh(now)
    }
    pub fn directory_refresh_due(&self, now: i64) -> bool {
        !fresh(self.directory_at, now, DIRECTORY_TTL)
    }
    pub fn config_fresh(&self, now: i64) -> bool {
        !self.udp_ports.is_empty() && fresh(self.config_at, now, CONFIG_TTL)
    }
    pub fn config_refresh_due(&self, now: i64) -> bool {
        !self.config_fresh(now)
    }
    pub fn set_config(&mut self, ports: Vec<u16>, now: i64) {
        if self.udp_ports != ports {
            self.invalidate_directory();
        }
        self.udp_ports = ports;
        self.config_at = now;
    }
    pub fn invalidate_directory(&mut self) {
        self.directory_at = 0;
        self.peers.clear();
        self.directory_complete = false;
        self.directory_last_modified = None;
    }
    pub fn directory_validator(&self, now: i64) -> Option<&str> {
        // Clock rollback and incomplete/legacy snapshots require a full reply.
        (self.directory_complete
            && !self.peers.is_empty()
            && self.directory_at > 0
            && now >= self.directory_at)
            .then_some(self.directory_last_modified.as_deref())
            .flatten()
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
        self.directory_complete = self.peers.len() == nodes.len();
        self.directory_last_modified = None;
        while serde_json::to_vec(&self.peers).map_or(true, |v| v.len() > MAX_PEER_BYTES)
            && self.shrink(selected)
        {}
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
    /// Preserve the automatic first choice and the explicit choice when the
    /// enclosing encrypted record reaches its existing byte limit.
    pub fn shrink(&mut self, selected: Option<&str>) -> bool {
        if self.peers.len() <= 2 {
            return false;
        }
        let selected = selected
            .and_then(|id| self.peers.iter().find(|p| p.metadata.id == id))
            .cloned();
        self.peers.truncate(self.peers.len() / 2);
        if let Some(peer) = selected
            && !self.peers.iter().any(|p| p.metadata.id == peer.metadata.id)
        {
            self.peers.push(peer);
        }
        self.directory_complete = false;
        self.directory_last_modified = None;
        true
    }
    pub(super) fn validate(&self) -> Result<(), ImportError> {
        if self.permissions_at < 0
            || self.directory_at < 0
            || self.peers.len() > MAX_PEERS
            || self.config_at < 0
            || self.udp_ports.len() > 32
            || self.udp_ports.contains(&0)
            || self
                .directory_last_modified
                .as_deref()
                .is_some_and(|v| !valid_directory_validator(v))
            || serde_json::to_vec(&self.peers).map_or(true, |v| v.len() > MAX_PEER_BYTES + 4096)
        {
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
