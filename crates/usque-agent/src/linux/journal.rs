//! Durable Linux-only resource intents. No file descriptor or process handle is
//! serialized. Missing/corrupt evidence never authorizes broad system cleanup.

use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::plan::ValidatedTunnelPlan;

pub const SCHEMA_VERSION: u32 = 1;
pub const MAX_BYTES: u64 = 256 * 1024;
const MAX_DHCP_CLIENT_UIDS: usize = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Clean,
    Preparing,
    Prepared,
    Active,
    Recovering,
    RecoveryRequired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Step {
    Protection,
    Tun,
    Addresses,
    Dns,
    Routes,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepState {
    Intended,
    Applied,
    Restored,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Record {
    pub step: Step,
    pub state: StepState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Owner {
    pub uid: u32,
    pub process_id: u32,
    pub process_start_time: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Resources {
    pub interface_name: String,
    pub interface_alias: String,
    pub interface_index: Option<u32>,
    pub nft_table: String,
    pub route_table: u32,
    pub rule_priority: u32,
    pub socket_mark: u32,
    /// Durable allocation watermark; socket marks are never reused after an
    /// Agent restart within the same operation.
    pub egress_mark_highwater: u16,
    pub network_generation: u64,
    pub physical_interfaces: Vec<PhysicalInterface>,
    pub dns: super::dns::DnsReceipt,
    /// Sorted, unique non-root system DHCP client users, resolved once per
    /// operation. The installed guard is inspected against exactly this list.
    #[serde(default)]
    pub dhcp_client_uids: Option<Vec<u32>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PhysicalInterface {
    pub index: u32,
    pub family_mask: u32,
    pub dns_servers: Vec<std::net::IpAddr>,
}

impl Resources {
    pub fn for_operation(id: Uuid) -> Self {
        let hex = id.simple().to_string();
        // A root-owned single journal serializes all host TUN operations.
        // Kernel create-exclusive requests reject occupied IDs; they are never
        // reused on collision, even if a foreign object looks similar.
        let suffix = u32::from_le_bytes(id.as_bytes()[..4].try_into().expect("UUID prefix"));
        Self {
            interface_name: format!("usq{}", &hex[..12]),
            interface_alias: format!("usque:{id}"),
            interface_index: None,
            nft_table: format!("usque_{hex}"),
            route_table: 0x7500_0000 | (suffix & 0x00ff_ffff),
            rule_priority: 11000,
            socket_mark: 0x7500_0000 | ((suffix & 0xff) << 16),
            egress_mark_highwater: 0,
            network_generation: 1,
            physical_interfaces: Vec::new(),
            dns: super::dns::DnsReceipt::Resolved,
            dhcp_client_uids: None,
        }
    }
}

pub const RECOVERY_ATTEMPT_LIMIT: u8 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AutomaticPhase {
    #[default]
    Inactive,
    Waiting,
    Running,
    Exhausted,
    Blocked,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct AutomaticRecovery {
    pub phase: AutomaticPhase,
    pub attempts: u8,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Replacement {
    pub operation_id: Uuid,
    pub plan: ValidatedTunnelPlan,
    pub resources: Resources,
    pub state: StepState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Journal {
    pub schema_version: u32,
    pub generation: u64,
    pub phase: Phase,
    pub operation_id: Option<Uuid>,
    pub owner: Option<Owner>,
    pub plan: Option<ValidatedTunnelPlan>,
    pub resources: Option<Resources>,
    pub records: Vec<Record>,
    #[serde(default)]
    pub replacement: Option<Replacement>,
    #[serde(default)]
    pub replacement_origin: Option<Uuid>,
    #[serde(default)]
    pub replacement_initial_plan: Option<ValidatedTunnelPlan>,
    #[serde(default)]
    pub automatic_recovery: AutomaticRecovery,
}

impl Default for Journal {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            generation: 0,
            phase: Phase::Clean,
            operation_id: None,
            owner: None,
            plan: None,
            resources: None,
            records: Vec::new(),
            replacement: None,
            replacement_origin: None,
            replacement_initial_plan: None,
            automatic_recovery: AutomaticRecovery::default(),
        }
    }
}

impl Journal {
    pub fn validate(&self) -> io::Result<()> {
        if self.schema_version != SCHEMA_VERSION {
            return Err(invalid("unsupported Linux recovery schema"));
        }
        if self.phase == Phase::Clean {
            if self.operation_id.is_some()
                || self.owner.is_some()
                || self.plan.is_some()
                || self.resources.is_some()
                || !self.records.is_empty()
                || self.replacement.is_some()
                || self.replacement_origin.is_some()
                || self.replacement_initial_plan.is_some()
                || self.automatic_recovery != AutomaticRecovery::default()
            {
                return Err(invalid("clean journal retains an operation"));
            }
            return Ok(());
        }
        if self.automatic_recovery.attempts > RECOVERY_ATTEMPT_LIMIT {
            return Err(invalid("invalid Linux automatic recovery budget"));
        }
        let id = self
            .operation_id
            .filter(|id| !id.is_nil())
            .ok_or_else(|| invalid("missing operation identity"))?;
        let owner = self
            .owner
            .as_ref()
            .ok_or_else(|| invalid("missing Linux owner"))?;
        if owner.uid == 0 || owner.process_id == 0 || owner.process_start_time == 0 {
            return Err(invalid("invalid Linux owner"));
        }
        self.plan
            .as_ref()
            .ok_or_else(|| invalid("missing validated plan"))?
            .validate()
            .map_err(|_| invalid("invalid recovery plan"))?;
        let resources = self
            .resources
            .as_ref()
            .ok_or_else(|| invalid("missing Linux resources"))?;
        let mut expected = Resources::for_operation(id);
        expected.interface_index = resources.interface_index;
        expected.egress_mark_highwater = resources.egress_mark_highwater;
        expected.network_generation = resources.network_generation;
        expected
            .physical_interfaces
            .clone_from(&resources.physical_interfaces);
        expected.dns = resources.dns.clone();
        if let Some(uids) = &resources.dhcp_client_uids
            && (uids.len() > MAX_DHCP_CLIENT_UIDS
                || uids.contains(&0)
                || uids.windows(2).any(|pair| pair[0] >= pair[1]))
        {
            return Err(invalid("invalid Linux DHCP client users"));
        }
        expected
            .dhcp_client_uids
            .clone_from(&resources.dhcp_client_uids);
        if resources.network_generation == 0
            || resources.physical_interfaces.len() > 16
            || resources.physical_interfaces.iter().any(|link| {
                link.index == 0
                    || !(1..=3).contains(&link.family_mask)
                    || link.dns_servers.len() > 8
                    || link
                        .dns_servers
                        .iter()
                        .any(|ip| ip.is_unspecified() || ip.is_multicast() || ip.is_loopback())
            })
        {
            return Err(invalid("invalid Linux physical network snapshot"));
        }
        if let super::dns::DnsReceipt::NetworkManager { previous } = &resources.dns {
            previous.validate()?;
        }
        if *resources != expected || resources.interface_index == Some(0) {
            return Err(invalid("foreign Linux resource identity"));
        }
        if self.replacement_origin.is_some() != self.replacement_initial_plan.is_some() {
            return Err(invalid("incomplete Linux replacement provenance"));
        }
        if let Some(initial) = &self.replacement_initial_plan {
            initial
                .validate()
                .map_err(|_| invalid("invalid Linux initial replacement plan"))?;
        }
        if self
            .replacement_origin
            .is_some_and(|source| source.is_nil() || source == id)
        {
            return Err(invalid("invalid Linux replacement origin"));
        }
        if let Some(replacement) = &self.replacement {
            if replacement.operation_id == id || replacement.state == StepState::Restored {
                return Err(invalid("invalid Linux replacement bridge"));
            }
            Journal {
                phase: Phase::Preparing,
                operation_id: Some(replacement.operation_id),
                owner: Some(owner.clone()),
                plan: Some(replacement.plan.clone()),
                resources: Some(replacement.resources.clone()),
                ..Journal::default()
            }
            .validate()?;
        }
        let order = [
            Step::Protection,
            Step::Tun,
            Step::Addresses,
            Step::Dns,
            Step::Routes,
        ];
        if self.records.len() > order.len()
            || self
                .records
                .iter()
                .zip(order)
                .any(|(record, step)| record.step != step)
        {
            return Err(invalid("invalid Linux recovery ordering"));
        }
        if matches!(self.phase, Phase::Prepared | Phase::Active)
            && (self.records.len()
                < if self
                    .plan
                    .as_ref()
                    .is_some_and(|plan| plan.defer_network_configuration)
                {
                    2
                } else {
                    4
                }
                || self
                    .records
                    .iter()
                    .any(|record| record.state != StepState::Applied)
                || resources.interface_index.is_none())
        {
            return Err(invalid("incomplete prepared Linux operation"));
        }
        if self.phase == Phase::Active && self.records.len() != 5 {
            return Err(invalid("active Linux operation has no routes"));
        }
        // A restored guard with live dependent mutations would permit leaks.
        if self
            .records
            .first()
            .is_some_and(|r| r.state == StepState::Restored)
            && self.records.iter().any(|r| r.state != StepState::Restored)
        {
            return Err(invalid("protection was removed before dependent recovery"));
        }
        Ok(())
    }
}

pub struct JournalStore {
    path: PathBuf,
    uid: u32,
}

impl JournalStore {
    pub fn open(path: impl Into<PathBuf>) -> io::Result<Self> {
        let path = path.into();
        super::auth::verify_trusted_parents(&path)?;
        Ok(Self { path, uid: 0 })
    }

    #[cfg(test)]
    pub(crate) fn for_test(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            // SAFETY: geteuid returns a scalar and has no preconditions.
            uid: unsafe { libc::geteuid() },
        }
    }

    pub fn load(&self) -> io::Result<Journal> {
        let mut file = match OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(&self.path)
        {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Journal::default()),
            Err(error) => return Err(error),
        };
        let metadata = file.metadata()?;
        if !metadata.is_file()
            || metadata.uid() != self.uid
            || metadata.mode() & 0o077 != 0
            || metadata.nlink() != 1
            || metadata.len() > MAX_BYTES
        {
            return Err(invalid("unsafe Linux recovery file"));
        }
        let mut bytes = Vec::new();
        Read::by_ref(&mut file)
            .take(MAX_BYTES + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_BYTES {
            return Err(invalid("Linux recovery file too large"));
        }
        let journal: Journal = serde_json::from_slice(&bytes)
            .map_err(|_| invalid("corrupt Linux recovery journal"))?;
        journal.validate()?;
        Ok(journal)
    }

    pub fn save(&self, journal: &mut Journal) -> io::Result<()> {
        journal.validate()?;
        let generation = journal
            .generation
            .checked_add(1)
            .ok_or_else(|| invalid("Linux journal generation exhausted"))?;
        let mut next = journal.clone();
        next.generation = generation;
        let parent = self
            .path
            .parent()
            .ok_or_else(|| invalid("Linux journal parent missing"))?;
        let metadata = fs::symlink_metadata(parent)?;
        if !metadata.is_dir() || metadata.uid() != self.uid || metadata.mode() & 0o077 != 0 {
            return Err(invalid("Linux recovery directory must have mode 0700"));
        }
        let mut temp = tempfile::NamedTempFile::new_in(parent)?;
        temp.as_file()
            .set_permissions(fs::Permissions::from_mode(0o600))?;
        serde_json::to_writer(&mut temp, &next).map_err(io::Error::other)?;
        temp.flush()?;
        temp.as_file().sync_all()?;
        temp.persist(&self.path).map_err(|error| error.error)?;
        File::open(parent)?.sync_all()?;
        *journal = next;
        Ok(())
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corrupted_evidence_does_not_become_clean() {
        let directory = tempfile::tempdir().unwrap();
        fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let store = JournalStore::for_test(directory.path().join("journal.json"));
        let mut journal = Journal::default();
        store.save(&mut journal).unwrap();
        assert_eq!(journal.generation, 1);
        assert_eq!(store.load().unwrap(), journal);
        fs::write(store.path(), b"{broken").unwrap();
        assert!(store.load().is_err());
    }

    #[test]
    fn journal_never_follows_a_symlink() {
        let directory = tempfile::tempdir().unwrap();
        let target = directory.path().join("target");
        fs::write(&target, b"{}").unwrap();
        let path = directory.path().join("journal");
        std::os::unix::fs::symlink(target, &path).unwrap();
        assert!(JournalStore::for_test(path).load().is_err());
    }
}
