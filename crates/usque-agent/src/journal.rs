use std::{
    collections::HashSet,
    fs::{self, File},
    io::{self, BufReader, BufWriter, Read, Write},
    net::IpAddr,
    path::{Path, PathBuf},
};

use ipnet::IpNet;
use serde::{Deserialize, Serialize};
use tempfile::NamedTempFile;
use thiserror::Error;
use uuid::Uuid;

use crate::plan::{PlanError, ValidatedTunnelPlan};

pub const JOURNAL_SCHEMA_VERSION: u32 = 2;
pub const MAX_JOURNAL_BYTES: u64 = 1024 * 1024;
pub const MAX_JOURNAL_STEPS: usize = 16;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryPhase {
    Clean,
    Preparing,
    Prepared,
    Active,
    /// Legacy phase written by pre-removal captive-portal pause. New code never
    /// enters this phase; recovery must still accept and clean such journals.
    Paused,
    Recovering,
    RecoveryRequired,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OperationKind {
    Tunnel,
    SystemProxy,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum MutationKind {
    WintunAdapter,
    EndpointBypass,
    KillSwitch,
    InterfaceConfiguration,
    Dns,
    PacketSession,
    DefaultRoutes,
    SystemProxy,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MutationState {
    Intended,
    Applied,
    Restored,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RouteReceipt {
    pub destination: String,
    pub next_hop: Option<IpAddr>,
    pub next_hop_scope_id: u32,
    pub interface_luid: u64,
    pub metric: u32,
    /// True only when this operation created the exact route. Recovery must
    /// never delete a pre-existing route merely because it matches a receipt.
    pub owned: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AddressReceipt {
    pub address: String,
    /// True only when this operation created the exact interface address.
    pub owned: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum MutationReceipt {
    WintunAdapter {
        adapter_name: String,
        adapter_guid: Uuid,
        interface_luid: u64,
    },
    EndpointBypass {
        created: Vec<RouteReceipt>,
    },
    KillSwitch {
        provider_key: Uuid,
        sublayer_key: Uuid,
        filter_keys: Vec<Uuid>,
        filter_ids: Vec<u64>,
    },
    InterfaceConfiguration {
        interface_luid: u64,
        previous_ipv4_mtu: Option<u32>,
        previous_ipv6_mtu: Option<u32>,
        created_addresses: Vec<AddressReceipt>,
    },
    Dns {
        interface_guid: Uuid,
        previous_automatic: bool,
        previous_servers: Vec<IpAddr>,
    },
    PacketSession {
        session_id: Uuid,
        ring_capacity: u32,
    },
    DefaultRoutes {
        created: Vec<RouteReceipt>,
        replaced: Vec<RouteReceipt>,
    },
    SystemProxy {
        user_sid: String,
        operation_id: Uuid,
        previous_proxy_enable: Option<u32>,
        previous_proxy: Option<String>,
        previous_bypass: Option<String>,
        previous_auto_config_url: Option<String>,
        previous_auto_detect: Option<u32>,
        applied_proxy: String,
        applied_bypass: String,
    },
}

impl MutationReceipt {
    pub const fn kind(&self) -> MutationKind {
        match self {
            Self::WintunAdapter { .. } => MutationKind::WintunAdapter,
            Self::EndpointBypass { .. } => MutationKind::EndpointBypass,
            Self::KillSwitch { .. } => MutationKind::KillSwitch,
            Self::InterfaceConfiguration { .. } => MutationKind::InterfaceConfiguration,
            Self::Dns { .. } => MutationKind::Dns,
            Self::PacketSession { .. } => MutationKind::PacketSession,
            Self::DefaultRoutes { .. } => MutationKind::DefaultRoutes,
            Self::SystemProxy { .. } => MutationKind::SystemProxy,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct MutationRecord {
    pub kind: MutationKind,
    pub state: MutationState,
    pub receipt: MutationReceipt,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RecoveryJournal {
    pub schema_version: u32,
    pub generation: u64,
    pub phase: RecoveryPhase,
    pub operation_kind: Option<OperationKind>,
    pub operation_id: Option<Uuid>,
    pub owner_sid: Option<String>,
    pub owner_process_id: Option<u32>,
    pub plan: Option<ValidatedTunnelPlan>,
    /// Legacy field from captive-portal pause. Kept for upgrade recovery only.
    pub pause_deadline_unix_seconds: Option<i64>,
    pub steps: Vec<MutationRecord>,
}

impl Default for RecoveryJournal {
    fn default() -> Self {
        Self::clean(0)
    }
}

impl RecoveryJournal {
    pub fn clean(generation: u64) -> Self {
        Self {
            schema_version: JOURNAL_SCHEMA_VERSION,
            generation,
            phase: RecoveryPhase::Clean,
            operation_kind: None,
            operation_id: None,
            owner_sid: None,
            owner_process_id: None,
            plan: None,
            pause_deadline_unix_seconds: None,
            steps: Vec::new(),
        }
    }

    pub fn validate(&self) -> Result<(), JournalError> {
        if self.schema_version != JOURNAL_SCHEMA_VERSION {
            return Err(JournalError::Schema {
                found: self.schema_version,
                supported: JOURNAL_SCHEMA_VERSION,
            });
        }
        if self.steps.len() > MAX_JOURNAL_STEPS {
            return Err(JournalError::TooManySteps(self.steps.len()));
        }
        let mut kinds = HashSet::new();
        for step in &self.steps {
            if step.kind != step.receipt.kind() {
                return Err(JournalError::ReceiptMismatch(step.kind));
            }
            if !kinds.insert(step.kind) {
                return Err(JournalError::DuplicateStep(step.kind));
            }
        }

        if self.phase == RecoveryPhase::Clean {
            if self.operation_kind.is_some()
                || self.operation_id.is_some()
                || self.owner_sid.is_some()
                || self.owner_process_id.is_some()
                || self.plan.is_some()
                || self.pause_deadline_unix_seconds.is_some()
                || !self.steps.is_empty()
            {
                return Err(JournalError::InvalidCleanState);
            }
            return Ok(());
        }

        if self.operation_kind.is_none()
            || self.operation_id.is_none()
            || self.owner_process_id.is_none()
        {
            return Err(JournalError::MissingOwner);
        }
        let owner_sid = self
            .owner_sid
            .as_deref()
            .ok_or(JournalError::MissingOwner)?;
        if !valid_sid_text(owner_sid) {
            return Err(JournalError::InvalidOwnerSid);
        }
        let operation_kind = self.operation_kind.expect("checked above");
        let plan = match operation_kind {
            OperationKind::Tunnel => {
                let plan = self.plan.as_ref().ok_or(JournalError::MissingPlan)?;
                plan.validate()?;
                Some(plan)
            }
            OperationKind::SystemProxy => {
                if self.plan.is_some()
                    || self
                        .steps
                        .iter()
                        .any(|step| step.kind != MutationKind::SystemProxy)
                    || self.steps.len() > 1
                    || matches!(self.phase, RecoveryPhase::Prepared | RecoveryPhase::Paused)
                {
                    return Err(JournalError::InvalidOperationShape);
                }
                None
            }
        };
        let wintun_luid = self.steps.iter().find_map(|step| {
            if let MutationReceipt::WintunAdapter { interface_luid, .. } = step.receipt {
                Some(interface_luid)
            } else {
                None
            }
        });
        let operation_id = self.operation_id.expect("checked above");
        for step in &self.steps {
            validate_receipt(step, plan, wintun_luid, owner_sid, operation_id)?;
        }

        if self.phase == RecoveryPhase::Paused {
            if self.pause_deadline_unix_seconds.is_none() {
                return Err(JournalError::MissingPauseDeadline);
            }
        } else if self.pause_deadline_unix_seconds.is_some() {
            return Err(JournalError::UnexpectedPauseDeadline);
        }
        Ok(())
    }
}

fn validate_receipt(
    record: &MutationRecord,
    plan: Option<&ValidatedTunnelPlan>,
    wintun_luid: Option<u64>,
    owner_sid: &str,
    journal_operation_id: Uuid,
) -> Result<(), JournalError> {
    let unsafe_receipt = |reason: &str| JournalError::UnsafeReceipt {
        kind: record.kind,
        reason: reason.to_owned(),
    };
    let tunnel_plan = if record.kind == MutationKind::SystemProxy {
        None
    } else {
        Some(plan.ok_or_else(|| unsafe_receipt("tunnel receipt has no tunnel plan"))?)
    };
    match &record.receipt {
        MutationReceipt::WintunAdapter {
            adapter_name,
            adapter_guid,
            interface_luid,
        } => {
            if !valid_adapter_name(adapter_name) || adapter_guid.is_nil() {
                return Err(unsafe_receipt("invalid adapter identity"));
            }
            if record.state != MutationState::Intended && *interface_luid == 0 {
                return Err(unsafe_receipt("applied adapter has no interface LUID"));
            }
        }
        MutationReceipt::EndpointBypass { created } => {
            let plan = tunnel_plan.expect("validated tunnel operation");
            for route in created {
                validate_route(route).map_err(|reason| unsafe_receipt(&reason))?;
            }
            let expected = plan
                .endpoint_candidates
                .iter()
                .chain(plan.control_api_candidates.iter())
                .map(|endpoint| host_network(endpoint.ip()).to_string())
                .collect::<HashSet<_>>();
            let actual = created
                .iter()
                .map(|route| route.destination.clone())
                .collect::<HashSet<_>>();
            let endpoint_hosts = plan
                .endpoint_candidates
                .iter()
                .map(|endpoint| host_network(endpoint.ip()).to_string())
                .collect::<HashSet<_>>();
            let control_hosts = plan
                .control_api_candidates
                .iter()
                .map(|endpoint| host_network(endpoint.ip()).to_string())
                .collect::<HashSet<_>>();
            let missing_control = !control_hosts.is_empty() && actual.is_disjoint(&control_hosts);
            if actual.len() != created.len()
                || !actual.is_subset(&expected)
                || actual.is_disjoint(&endpoint_hosts)
                || missing_control
            {
                return Err(unsafe_receipt(
                    "endpoint receipt is not a safe reachable subset of data/control host routes",
                ));
            }
        }
        MutationReceipt::KillSwitch {
            provider_key,
            sublayer_key,
            filter_keys,
            filter_ids,
        } => {
            if provider_key.is_nil()
                || sublayer_key.is_nil()
                || filter_keys.is_empty()
                || filter_keys.len() > 256
                || filter_keys.iter().copied().collect::<HashSet<_>>().len() != filter_keys.len()
                || filter_ids.len() > 256
                || filter_ids.iter().copied().collect::<HashSet<_>>().len() != filter_ids.len()
                || !filter_ids.is_empty() && filter_ids.len() != filter_keys.len()
            {
                return Err(unsafe_receipt("invalid WFP resource identity"));
            }
        }
        MutationReceipt::InterfaceConfiguration {
            interface_luid,
            previous_ipv4_mtu,
            previous_ipv6_mtu,
            created_addresses,
        } => {
            let plan = tunnel_plan.expect("validated tunnel operation");
            if *interface_luid == 0 || wintun_luid != Some(*interface_luid) {
                return Err(unsafe_receipt(
                    "interface receipt does not match the Wintun LUID",
                ));
            }
            if previous_ipv4_mtu.is_some_and(|mtu| mtu == 0 || mtu > 65_535)
                || previous_ipv6_mtu.is_some_and(|mtu| mtu == 0 || mtu > 65_535)
                || created_addresses.len() > 2
            {
                return Err(unsafe_receipt("invalid interface snapshot bounds"));
            }
            let expected = plan
                .assigned_ipv4
                .iter()
                .chain(plan.assigned_ipv6.iter())
                .map(ToString::to_string)
                .collect::<HashSet<_>>();
            let actual = created_addresses
                .iter()
                .map(|entry| entry.address.clone())
                .collect::<HashSet<_>>();
            if actual.len() != created_addresses.len() || actual != expected {
                return Err(unsafe_receipt(
                    "interface addresses do not exactly match the validated plan",
                ));
            }
        }
        MutationReceipt::Dns {
            interface_guid,
            previous_automatic: _,
            previous_servers,
        } => {
            if interface_guid.is_nil() || previous_servers.len() > 8 {
                return Err(unsafe_receipt("invalid DNS snapshot"));
            }
        }
        MutationReceipt::PacketSession {
            session_id,
            ring_capacity,
        } => {
            if session_id.is_nil()
                || !(128 * 1024..=64 * 1024 * 1024).contains(ring_capacity)
                || !ring_capacity.is_power_of_two()
            {
                return Err(unsafe_receipt("invalid packet-ring identity or capacity"));
            }
        }
        MutationReceipt::DefaultRoutes { created, replaced } => {
            let plan = tunnel_plan.expect("validated tunnel operation");
            let split_dns_routes = usize::from(plan.split_dns && plan.assigned_ipv4.is_some())
                + usize::from(plan.split_dns && plan.assigned_ipv6.is_some());
            if created.len() > plan.split_exclusions.len() + 2 + split_dns_routes
                || !replaced.is_empty()
            {
                return Err(unsafe_receipt("unexpected route count or replacement"));
            }
            for route in created {
                validate_route(route).map_err(|reason| unsafe_receipt(&reason))?;
                let destination = route
                    .destination
                    .parse::<IpNet>()
                    .map_err(|_| unsafe_receipt("route destination is not a CIDR"))?;
                let is_expected_default = match destination {
                    IpNet::V4(network) if network.prefix_len() == 0 => {
                        plan.assigned_ipv4.is_some()
                            && route.interface_luid == wintun_luid.unwrap_or_default()
                            && route.next_hop.is_none()
                    }
                    IpNet::V6(network) if network.prefix_len() == 0 => {
                        plan.assigned_ipv6.is_some()
                            && route.interface_luid == wintun_luid.unwrap_or_default()
                            && route.next_hop.is_none()
                    }
                    _ if plan.split_dns
                        && destination
                            == "198.18.0.1/32".parse::<IpNet>().expect("static CIDR") =>
                    {
                        plan.assigned_ipv4.is_some()
                            && route.interface_luid == wintun_luid.unwrap_or_default()
                            && route.next_hop.is_none()
                    }
                    _ if plan.split_dns
                        && destination == "fd00::1/128".parse::<IpNet>().expect("static CIDR") =>
                    {
                        plan.assigned_ipv6.is_some()
                            && route.interface_luid == wintun_luid.unwrap_or_default()
                            && route.next_hop.is_none()
                    }
                    _ => plan.split_exclusions.contains(&destination),
                };
                if !is_expected_default {
                    return Err(unsafe_receipt(
                        "route is neither a planned default nor split exclusion",
                    ));
                }
            }
        }
        MutationReceipt::SystemProxy {
            user_sid,
            operation_id,
            previous_proxy_enable,
            previous_proxy,
            previous_bypass,
            previous_auto_config_url,
            previous_auto_detect,
            applied_proxy,
            applied_bypass,
        } => {
            if user_sid != owner_sid
                || !valid_sid_text(user_sid)
                || *operation_id != journal_operation_id
                || previous_proxy_enable.is_some_and(|value| value > 1)
                || previous_auto_detect.is_some_and(|value| value > 1)
                || previous_proxy
                    .as_ref()
                    .is_some_and(|value| value.len() > 4096)
                || previous_bypass
                    .as_ref()
                    .is_some_and(|value| value.len() > 4096)
                || previous_auto_config_url
                    .as_ref()
                    .is_some_and(|value| value.len() > 4096)
                || applied_proxy.is_empty()
                || applied_proxy.len() > 4096
                || applied_bypass.len() > 4096
            {
                return Err(unsafe_receipt("invalid per-user system-proxy snapshot"));
            }
        }
    }
    Ok(())
}

fn validate_route(route: &RouteReceipt) -> Result<(), String> {
    let destination = route
        .destination
        .parse::<IpNet>()
        .map_err(|_| "route destination is not a CIDR".to_owned())?;
    if destination.to_string() != route.destination
        || route.interface_luid == 0
        || route
            .next_hop
            .is_some_and(|next_hop| next_hop.is_ipv4() != destination.addr().is_ipv4())
        || route.next_hop_scope_id != 0 && route.next_hop.is_none_or(|next_hop| !next_hop.is_ipv6())
    {
        return Err("route key is malformed or uses mixed address families".to_owned());
    }
    Ok(())
}

fn host_network(address: IpAddr) -> IpNet {
    match address {
        IpAddr::V4(address) => ipnet::Ipv4Net::new(address, 32)
            .expect("IPv4 host prefix")
            .into(),
        IpAddr::V6(address) => ipnet::Ipv6Net::new(address, 128)
            .expect("IPv6 host prefix")
            .into(),
    }
}

fn valid_adapter_name(value: &str) -> bool {
    value.len() == 18
        && value.starts_with("Usque-")
        && value[6..].bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[derive(Debug, Clone)]
pub struct JournalStore {
    path: PathBuf,
}

impl JournalStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn load_or_clean(&self) -> Result<RecoveryJournal, JournalError> {
        if !self.path.exists() {
            return Ok(RecoveryJournal::default());
        }
        let metadata = fs::metadata(&self.path)?;
        if metadata.len() > MAX_JOURNAL_BYTES {
            return Err(JournalError::TooLarge(metadata.len()));
        }
        let file = File::open(&self.path)?;
        let mut reader = BufReader::new(file).take(MAX_JOURNAL_BYTES + 1);
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_JOURNAL_BYTES {
            return Err(JournalError::TooLarge(bytes.len() as u64));
        }
        let journal: RecoveryJournal = serde_json::from_slice(&bytes)?;
        journal.validate()?;
        Ok(journal)
    }

    pub fn save(&self, journal: &mut RecoveryJournal) -> Result<(), JournalError> {
        journal.generation = journal
            .generation
            .checked_add(1)
            .ok_or(JournalError::GenerationOverflow)?;
        journal.validate()?;
        let parent = self
            .path
            .parent()
            .ok_or_else(|| JournalError::MissingParent(self.path.clone()))?;
        fs::create_dir_all(parent)?;
        let mut temporary = NamedTempFile::new_in(parent)?;
        {
            let mut writer = BufWriter::new(temporary.as_file_mut());
            serde_json::to_writer_pretty(&mut writer, journal)?;
            writer.write_all(b"\n")?;
            writer.flush()?;
        }
        temporary.as_file().sync_all()?;
        replace_file(temporary.path(), &self.path)?;
        let _ = temporary.keep();
        Ok(())
    }

    /// Removes the durable recovery journal only after it proves that no
    /// privileged mutation remains. This is intentionally separate from
    /// normal recovery so major upgrades can retain a clean journal while a
    /// true uninstall can remove machine-owned state.
    pub fn remove_if_clean(&self) -> Result<bool, JournalError> {
        let journal = self.load_or_clean()?;
        if journal.phase != RecoveryPhase::Clean {
            return Err(JournalError::RemovalRequiresClean(journal.phase));
        }
        match fs::remove_file(&self.path) {
            Ok(()) => Ok(true),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(error.into()),
        }
    }
}

fn valid_sid_text(value: &str) -> bool {
    value.starts_with("S-")
        && value.len() <= 256
        && value[2..]
            .bytes()
            .all(|byte| byte.is_ascii_digit() || byte == b'-')
}

#[cfg(windows)]
pub(crate) fn valid_sid(value: &str) -> bool {
    valid_sid_text(value)
}

#[cfg(not(windows))]
fn replace_file(source: &Path, destination: &Path) -> io::Result<()> {
    fs::rename(source, destination)
}

#[cfg(windows)]
fn replace_file(source: &Path, destination: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    };

    let source: Vec<u16> = source
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let destination: Vec<u16> = destination
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    // SAFETY: both buffers are valid, null-terminated UTF-16 paths and remain
    // alive until MoveFileExW returns.
    if unsafe {
        MoveFileExW(
            source.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    } == 0
    {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[derive(Debug, Error)]
pub enum JournalError {
    #[error("journal I/O failed: {0}")]
    Io(#[from] io::Error),
    #[error("journal JSON is invalid: {0}")]
    Json(#[from] serde_json::Error),
    #[error("journal schema {found} is newer or unsupported; this binary supports {supported}")]
    Schema { found: u32, supported: u32 },
    #[error("journal exceeds {MAX_JOURNAL_BYTES} bytes: {0}")]
    TooLarge(u64),
    #[error("journal contains too many mutation steps: {0}")]
    TooManySteps(usize),
    #[error("journal contains duplicate step {0:?}")]
    DuplicateStep(MutationKind),
    #[error("journal receipt does not match step {0:?}")]
    ReceiptMismatch(MutationKind),
    #[error("journal contains unsafe {kind:?} receipt: {reason}")]
    UnsafeReceipt { kind: MutationKind, reason: String },
    #[error("clean journal contains active-operation data")]
    InvalidCleanState,
    #[error("non-clean journal is missing its authenticated owner")]
    MissingOwner,
    #[error("journal operation kind, plan, steps, and phase are inconsistent")]
    InvalidOperationShape,
    #[error("journal owner SID is malformed")]
    InvalidOwnerSid,
    #[error("non-clean journal is missing its tunnel plan")]
    MissingPlan,
    #[error("paused journal is missing its deadline")]
    MissingPauseDeadline,
    #[error("only a paused journal may contain a pause deadline")]
    UnexpectedPauseDeadline,
    #[error("journal generation overflowed")]
    GenerationOverflow,
    #[error("cannot remove Agent recovery state while journal phase is {0:?}")]
    RemovalRequiresClean(RecoveryPhase),
    #[error("journal path has no parent: {0}")]
    MissingParent(PathBuf),
    #[error("journal tunnel plan is invalid: {0}")]
    Plan(#[from] PlanError),
}

#[cfg(test)]
mod tests {
    use std::net::{Ipv4Addr, Ipv6Addr, SocketAddrV4, SocketAddrV6};

    use ipnet::Ipv4Net;

    use super::*;

    fn plan() -> ValidatedTunnelPlan {
        ValidatedTunnelPlan {
            profile_id: Uuid::new_v4(),
            endpoint: SocketAddrV4::new(Ipv4Addr::new(162, 159, 198, 2), 443).into(),
            endpoint_candidates: vec![
                SocketAddrV4::new(Ipv4Addr::new(162, 159, 198, 2), 443).into(),
            ],
            control_api_candidates: vec![
                SocketAddrV4::new(Ipv4Addr::new(198, 51, 100, 10), 443).into(),
            ],
            mtu: 1280,
            dns_servers: vec![Ipv4Addr::new(1, 1, 1, 1).into()],
            split_exclusions: vec![
                Ipv4Net::new(Ipv4Addr::new(192, 168, 0, 0), 16)
                    .expect("network")
                    .into(),
            ],
            allow_lan: true,
            kill_switch: true,
            split_dns: false,
            assigned_ipv4: Some(
                Ipv4Net::new(Ipv4Addr::new(172, 16, 0, 2), 32)
                    .expect("assignment")
                    .into(),
            ),
            assigned_ipv6: None,
        }
    }

    #[test]
    fn atomic_store_round_trips_a_write_ahead_intent() {
        let directory = tempfile::tempdir().expect("tempdir");
        let store = JournalStore::new(directory.path().join("recovery.json"));
        let mut journal = RecoveryJournal {
            schema_version: JOURNAL_SCHEMA_VERSION,
            generation: 0,
            phase: RecoveryPhase::Preparing,
            operation_kind: Some(OperationKind::Tunnel),
            operation_id: Some(Uuid::new_v4()),
            owner_sid: Some("S-1-5-21-1000".to_owned()),
            owner_process_id: Some(42),
            plan: Some(plan()),
            pause_deadline_unix_seconds: None,
            steps: vec![MutationRecord {
                kind: MutationKind::WintunAdapter,
                state: MutationState::Intended,
                receipt: MutationReceipt::WintunAdapter {
                    adapter_name: "Usque-0123456789ab".to_owned(),
                    adapter_guid: Uuid::new_v4(),
                    interface_luid: 7,
                },
            }],
        };

        store.save(&mut journal).expect("save");
        assert_eq!(journal.generation, 1);
        assert_eq!(store.load_or_clean().expect("load"), journal);
    }

    #[test]
    fn uninstall_removes_only_a_clean_journal() {
        let directory = tempfile::tempdir().expect("tempdir");
        let store = JournalStore::new(directory.path().join("recovery.json"));
        let mut journal = RecoveryJournal::default();
        store.save(&mut journal).expect("save clean journal");
        assert!(store.path().is_file());
        assert!(store.remove_if_clean().expect("remove clean journal"));
        assert!(!store.path().exists());
        assert!(!store.remove_if_clean().expect("missing journal is clean"));
    }

    #[test]
    fn uninstall_refuses_to_remove_recovery_evidence() {
        let directory = tempfile::tempdir().expect("tempdir");
        let store = JournalStore::new(directory.path().join("recovery.json"));
        let mut journal = RecoveryJournal {
            schema_version: JOURNAL_SCHEMA_VERSION,
            generation: 0,
            phase: RecoveryPhase::Preparing,
            operation_kind: Some(OperationKind::Tunnel),
            operation_id: Some(Uuid::new_v4()),
            owner_sid: Some("S-1-5-21-1000".to_owned()),
            owner_process_id: Some(42),
            plan: Some(plan()),
            pause_deadline_unix_seconds: None,
            steps: vec![MutationRecord {
                kind: MutationKind::WintunAdapter,
                state: MutationState::Intended,
                receipt: MutationReceipt::WintunAdapter {
                    adapter_name: "Usque-0123456789ab".to_owned(),
                    adapter_guid: Uuid::new_v4(),
                    interface_luid: 0,
                },
            }],
        };
        store.save(&mut journal).expect("save recovery journal");
        assert!(matches!(
            store.remove_if_clean(),
            Err(JournalError::RemovalRequiresClean(RecoveryPhase::Preparing))
        ));
        assert!(store.path().is_file());
    }

    #[test]
    fn clean_state_cannot_hide_unrestored_resources() {
        let mut journal = RecoveryJournal::default();
        journal.steps.push(MutationRecord {
            kind: MutationKind::PacketSession,
            state: MutationState::Applied,
            receipt: MutationReceipt::PacketSession {
                session_id: Uuid::new_v4(),
                ring_capacity: 1024 * 1024,
            },
        });
        assert!(matches!(
            journal.validate(),
            Err(JournalError::InvalidCleanState)
        ));
    }

    #[test]
    fn unknown_fields_fail_closed() {
        let directory = tempfile::tempdir().expect("tempdir");
        let store = JournalStore::new(directory.path().join("recovery.json"));
        fs::write(
            store.path(),
            br#"{"schema_version":2,"generation":0,"phase":"clean","operation_kind":null,"operation_id":null,"owner_sid":null,"owner_process_id":null,"plan":null,"pause_deadline_unix_seconds":null,"steps":[],"secret":"x"}"#,
        )
        .expect("fixture");
        assert!(matches!(store.load_or_clean(), Err(JournalError::Json(_))));
    }

    #[test]
    fn recovery_receipt_cannot_target_an_unrelated_route() {
        let journal = RecoveryJournal {
            schema_version: JOURNAL_SCHEMA_VERSION,
            generation: 1,
            phase: RecoveryPhase::Preparing,
            operation_kind: Some(OperationKind::Tunnel),
            operation_id: Some(Uuid::new_v4()),
            owner_sid: Some("S-1-5-21-1000".to_owned()),
            owner_process_id: Some(42),
            plan: Some(plan()),
            pause_deadline_unix_seconds: None,
            steps: vec![
                MutationRecord {
                    kind: MutationKind::WintunAdapter,
                    state: MutationState::Applied,
                    receipt: MutationReceipt::WintunAdapter {
                        adapter_name: "Usque-0123456789ab".to_owned(),
                        adapter_guid: Uuid::new_v4(),
                        interface_luid: 7,
                    },
                },
                MutationRecord {
                    kind: MutationKind::EndpointBypass,
                    state: MutationState::Intended,
                    receipt: MutationReceipt::EndpointBypass {
                        created: vec![RouteReceipt {
                            destination: "203.0.113.7/32".to_owned(),
                            next_hop: Some(Ipv4Addr::new(192, 0, 2, 1).into()),
                            next_hop_scope_id: 0,
                            interface_luid: 9,
                            metric: 0,
                            owned: true,
                        }],
                    },
                },
            ],
        };
        assert!(matches!(
            journal.validate(),
            Err(JournalError::UnsafeReceipt {
                kind: MutationKind::EndpointBypass,
                ..
            })
        ));
    }

    #[test]
    fn endpoint_receipt_accepts_a_reachable_family_subset() {
        let mut tunnel_plan = plan();
        tunnel_plan.endpoint_candidates.push(
            SocketAddrV6::new(
                "2606:4700:103::2"
                    .parse::<Ipv6Addr>()
                    .expect("IPv6 endpoint"),
                443,
                0,
                0,
            )
            .into(),
        );
        tunnel_plan.control_api_candidates.push(
            SocketAddrV6::new(
                "2001:db8::10".parse::<Ipv6Addr>().expect("IPv6 control"),
                443,
                0,
                0,
            )
            .into(),
        );
        let route = |destination: &str| RouteReceipt {
            destination: destination.to_owned(),
            next_hop: Some(Ipv4Addr::new(192, 0, 2, 1).into()),
            next_hop_scope_id: 0,
            interface_luid: 9,
            metric: 0,
            owned: true,
        };
        let journal = RecoveryJournal {
            schema_version: JOURNAL_SCHEMA_VERSION,
            generation: 1,
            phase: RecoveryPhase::Preparing,
            operation_kind: Some(OperationKind::Tunnel),
            operation_id: Some(Uuid::new_v4()),
            owner_sid: Some("S-1-5-21-1000".to_owned()),
            owner_process_id: Some(42),
            plan: Some(tunnel_plan),
            pause_deadline_unix_seconds: None,
            steps: vec![
                MutationRecord {
                    kind: MutationKind::WintunAdapter,
                    state: MutationState::Applied,
                    receipt: MutationReceipt::WintunAdapter {
                        adapter_name: "Usque-0123456789ab".to_owned(),
                        adapter_guid: Uuid::new_v4(),
                        interface_luid: 7,
                    },
                },
                MutationRecord {
                    kind: MutationKind::EndpointBypass,
                    state: MutationState::Intended,
                    receipt: MutationReceipt::EndpointBypass {
                        created: vec![route("162.159.198.2/32"), route("198.51.100.10/32")],
                    },
                },
            ],
        };

        journal
            .validate()
            .expect("one reachable family from each candidate group is safe");
    }

    #[test]
    fn legacy_endpoint_receipt_without_control_candidates_is_loadable() {
        let mut tunnel_plan = plan();
        tunnel_plan.control_api_candidates.clear();
        let route = |destination: &str| RouteReceipt {
            destination: destination.to_owned(),
            next_hop: Some(Ipv4Addr::new(192, 0, 2, 1).into()),
            next_hop_scope_id: 0,
            interface_luid: 9,
            metric: 0,
            owned: true,
        };
        let journal = RecoveryJournal {
            schema_version: JOURNAL_SCHEMA_VERSION,
            generation: 1,
            phase: RecoveryPhase::Preparing,
            operation_kind: Some(OperationKind::Tunnel),
            operation_id: Some(Uuid::new_v4()),
            owner_sid: Some("S-1-5-21-1000".to_owned()),
            owner_process_id: Some(42),
            plan: Some(tunnel_plan),
            pause_deadline_unix_seconds: None,
            steps: vec![
                MutationRecord {
                    kind: MutationKind::WintunAdapter,
                    state: MutationState::Applied,
                    receipt: MutationReceipt::WintunAdapter {
                        adapter_name: "Usque-0123456789ab".to_owned(),
                        adapter_guid: Uuid::new_v4(),
                        interface_luid: 7,
                    },
                },
                MutationRecord {
                    kind: MutationKind::EndpointBypass,
                    state: MutationState::Intended,
                    receipt: MutationReceipt::EndpointBypass {
                        created: vec![route("162.159.198.2/32")],
                    },
                },
            ],
        };

        journal
            .validate()
            .expect("empty control candidates skip the control-host intersection");
    }
}
