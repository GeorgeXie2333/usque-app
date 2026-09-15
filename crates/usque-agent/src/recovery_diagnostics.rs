//! Bounded, non-authoritative recovery evidence. Never contains receipts,
//! addresses, device identifiers, paths, account data or arbitrary error text.
use std::{
    collections::VecDeque,
    fs,
    io::{self, Read, Seek, SeekFrom, Write},
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use usque_ipc::agent_v1;

use crate::journal::MutationKind;

pub const RECOVERY_LOG_NAME: &str = "recovery-events-v1.jsonl";
const MAX_LOG_BYTES: u64 = 1024 * 1024;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RemovalStage {
    Observe,
    Request,
    Confirm,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RemovalFailure {
    Pending,
    Identity,
    Native,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum RecoveryApi {
    GetIfTable2,
    SetupDiGetClassDevsW,
    SetupDiEnumDeviceInfo,
    SetupDiGetDeviceInstanceIdW,
    SetupDiSetClassInstallParamsW,
    SetupDiCallClassInstaller,
    Other,
}

impl RecoveryApi {
    pub fn from_name(name: &str) -> Self {
        match name {
            "GetIfTable2" => Self::GetIfTable2,
            "SetupDiGetClassDevsW" => Self::SetupDiGetClassDevsW,
            "SetupDiEnumDeviceInfo" => Self::SetupDiEnumDeviceInfo,
            "SetupDiGetDeviceInstanceIdW" => Self::SetupDiGetDeviceInstanceIdW,
            "SetupDiSetClassInstallParamsW" => Self::SetupDiSetClassInstallParamsW,
            "SetupDiCallClassInstaller(DIF_REMOVE)" => Self::SetupDiCallClassInstaller,
            _ => Self::Other,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct AdapterRemovalDiagnostic {
    pub stage: RemovalStage,
    pub failure: RemovalFailure,
    pub interface_present: Option<bool>,
    pub device_present: Option<bool>,
    pub request_accepted: bool,
    pub elapsed_ms: u64,
    pub api: Option<RecoveryApi>,
    pub win32_code: Option<u32>,
}

impl std::fmt::Display for AdapterRemovalDiagnostic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "adapter_cleanup stage={:?} failure={:?} interface={:?} device={:?} request_accepted={} elapsed_ms={} api={:?} win32={:?}",
            self.stage,
            self.failure,
            self.interface_present,
            self.device_present,
            self.request_accepted,
            self.elapsed_ms,
            self.api,
            self.win32_code
        )
    }
}

#[derive(Serialize, Deserialize)]
pub struct RecoveryEvent {
    pub journal_generation: u64,
    pub step: MutationKind,
    pub restored: bool,
    pub elapsed_ms: u64,
    pub api: Option<RecoveryApi>,
    pub win32_code: Option<u32>,
    pub adapter: Option<AdapterRemovalDiagnostic>,
}

pub fn unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u128::from(u64::MAX)) as u64
}

pub fn presence(value: Option<bool>) -> i32 {
    (match value {
        Some(true) => agent_v1::RecoveryPresence::Present,
        Some(false) => agent_v1::RecoveryPresence::Absent,
        None => agent_v1::RecoveryPresence::Unspecified,
    }) as i32
}

pub fn diagnostic_api(api: Option<RecoveryApi>) -> i32 {
    use agent_v1::RecoveryDiagnosticApi as Api;
    (match api {
        None => Api::Unspecified,
        Some(RecoveryApi::GetIfTable2) => Api::GetIfTable2,
        Some(RecoveryApi::SetupDiGetClassDevsW) => Api::SetupDiGetClassDevs,
        Some(RecoveryApi::SetupDiEnumDeviceInfo) => Api::SetupDiEnumDeviceInfo,
        Some(RecoveryApi::SetupDiGetDeviceInstanceIdW) => Api::SetupDiGetDeviceInstanceId,
        Some(RecoveryApi::SetupDiSetClassInstallParamsW) => Api::SetupDiSetClassInstallParams,
        Some(RecoveryApi::SetupDiCallClassInstaller) => Api::SetupDiCallClassInstaller,
        Some(RecoveryApi::Other) => Api::Other,
    }) as i32
}

#[derive(Deserialize)]
struct StoredEvent {
    timestamp_ms: u64,
    recovery: RecoveryEvent,
}

/// Reads only the protected sibling event file, never the recovery journal.
/// Unknown fields are discarded by typed deserialization and reserialization.
pub fn read_history(
    journal_path: &Path,
) -> (
    agent_v1::RecoveryHistoryStatus,
    Vec<agent_v1::RecoveryHistoryEvent>,
) {
    use agent_v1::RecoveryHistoryStatus as Status;
    let Some(parent) = journal_path.parent() else {
        return (Status::Unavailable, vec![]);
    };
    let path = parent.join(RECOVERY_LOG_NAME);
    let metadata = match fs::symlink_metadata(&path) {
        Ok(value) => value,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return (Status::Missing, vec![]),
        Err(_) => return (Status::Unavailable, vec![]),
    };
    if !metadata.is_file() || is_reparse(&metadata) {
        return (Status::Unavailable, vec![]);
    }
    if metadata.len() > MAX_LOG_BYTES {
        return (Status::TooLarge, vec![]);
    }
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // Open the reparse point itself, so a swapped symlink is never followed.
        options.custom_flags(0x0020_0000); // FILE_FLAG_OPEN_REPARSE_POINT
    }
    let file = match options.open(path) {
        Ok(file) => file,
        Err(_) => return (Status::Unavailable, vec![]),
    };
    if !file
        .metadata()
        .is_ok_and(|metadata| metadata.is_file() && !is_reparse(&metadata))
    {
        return (Status::Unavailable, vec![]);
    }
    let mut bytes = Vec::new();
    if file
        .take(MAX_LOG_BYTES + 1)
        .read_to_end(&mut bytes)
        .is_err()
    {
        return (Status::Unavailable, vec![]);
    }
    if bytes.len() as u64 > MAX_LOG_BYTES {
        return (Status::TooLarge, vec![]);
    }
    let mut status = Status::Complete;
    let mut events = VecDeque::with_capacity(32);
    for line in bytes
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
    {
        if line.len() > 4096 {
            status = Status::Partial;
            continue;
        }
        let Ok(event) = serde_json::from_slice::<StoredEvent>(line) else {
            status = Status::Partial;
            continue;
        };
        if event.timestamp_ms == 0 || event.recovery.journal_generation == 0 {
            status = Status::Partial;
            continue;
        }
        if events.len() == 32 {
            events.pop_front();
        }
        events.push_back(history_event(event));
    }
    (status, events.into_iter().collect())
}

fn history_event(stored: StoredEvent) -> agent_v1::RecoveryHistoryEvent {
    use agent_v1::{
        RecoveryRemovalFailure as Failure, RecoveryRemovalStage as Stage, RecoveryStep as Step,
    };
    let event = stored.recovery;
    agent_v1::RecoveryHistoryEvent {
        occurred_at_unix_ms: stored.timestamp_ms,
        journal_generation: event.journal_generation,
        step: match event.step {
            MutationKind::WintunAdapter => Step::WintunAdapter,
            MutationKind::EndpointBypass => Step::EndpointBypass,
            MutationKind::KillSwitch => Step::KillSwitch,
            MutationKind::InterfaceConfiguration => Step::InterfaceConfiguration,
            MutationKind::Dns => Step::Dns,
            MutationKind::PacketSession => Step::PacketSession,
            MutationKind::DefaultRoutes => Step::DefaultRoutes,
            MutationKind::SystemProxy => Step::SystemProxy,
        } as i32,
        restored: event.restored,
        elapsed_ms: event.elapsed_ms,
        api: diagnostic_api(event.api),
        win32_code: event.win32_code,
        adapter: event
            .adapter
            .map(|adapter| agent_v1::RecoveryRemovalResult {
                stage: match adapter.stage {
                    RemovalStage::Observe => Stage::Observe,
                    RemovalStage::Request => Stage::Request,
                    RemovalStage::Confirm => Stage::Confirm,
                } as i32,
                failure: match adapter.failure {
                    RemovalFailure::Pending => Failure::Pending,
                    RemovalFailure::Identity => Failure::Identity,
                    RemovalFailure::Native => Failure::Native,
                } as i32,
                interface: presence(adapter.interface_present),
                pnp_device: presence(adapter.device_present),
                request_accepted: adapter.request_accepted,
                elapsed_ms: adapter.elapsed_ms,
                api: diagnostic_api(adapter.api),
                win32_code: adapter.win32_code,
            }),
    }
}

/// The production journal parent is already SYSTEM/Administrators-only.
/// Logging failure must never prevent the authoritative cleanup or journal save.
pub fn record(journal_path: &Path, event: &RecoveryEvent) -> io::Result<()> {
    let parent = journal_path
        .parent()
        .ok_or_else(|| io::Error::other("missing journal parent"))?;
    let path = parent.join(RECOVERY_LOG_NAME);
    match fs::symlink_metadata(&path) {
        Ok(metadata) if !metadata.is_file() || is_reparse(&metadata) => {
            return Err(io::Error::other("unsafe recovery log entry"));
        }
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    let mut file = fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(path)?;
    let timestamp_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u128::from(u64::MAX)) as u64;
    let mut bytes =
        serde_json::to_vec(&serde_json::json!({"timestamp_ms": timestamp_ms, "recovery": event}))?;
    bytes.push(b'\n');
    if file.metadata()?.len().saturating_add(bytes.len() as u64) > MAX_LOG_BYTES {
        file.set_len(0)?;
    }
    // Coordinator calls are serialized by the journal transaction lock.
    file.seek(SeekFrom::End(0))?;
    file.write_all(&bytes)?;
    file.sync_data()
}

fn is_reparse(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        metadata.file_type().is_symlink()
    }
}

pub fn sanitize_resource(
    mut value: usque_ipc::agent_v1::RecoveryResourceObservation,
) -> usque_ipc::agent_v1::RecoveryResourceObservation {
    use usque_ipc::agent_v1::{
        RecoveryDiagnosticApi as Api, RecoveryIdentityCheck as Identity, RecoveryPresence,
    };
    if Api::try_from(value.api).is_err() {
        value.api = 0;
    }
    if Identity::try_from(value.identity_check).is_err() {
        value.identity_check = 0;
    }
    if RecoveryPresence::try_from(value.presence).is_err()
        || value.identity_check != Identity::Verified as i32
        || value.win32_code.is_some()
        || value.configret_code.is_some()
    {
        value.presence = 0;
    }
    value.interface_oper_status = value.interface_oper_status.filter(|n| (1..=7).contains(n));
    value.interface_admin_status = value.interface_admin_status.filter(|n| (1..=3).contains(n));
    value.media_connect_state = value.media_connect_state.filter(|n| *n <= 2);
    if value.presence != RecoveryPresence::Present as i32 {
        value.interface_oper_status = None;
        value.interface_admin_status = None;
        value.media_connect_state = None;
        value.devnode_status = None;
        value.problem_code = None;
    }
    value
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_preserves_event_time_and_bounds_valid_entries_without_forwarding_unknown_fields() {
        let directory = tempfile::tempdir().unwrap();
        let journal = directory.path().join("journal.json");
        let mut lines = String::from("broken\n");
        for generation in 1..=40 {
            lines.push_str(&format!("{{\"timestamp_ms\":123,\"private\":\"SID token 192.0.2.1\",\"recovery\":{{\"journal_generation\":{generation},\"step\":\"wintun_adapter\",\"restored\":false,\"elapsed_ms\":10039,\"adapter_guid\":\"private-guid\"}}}}\n"));
        }
        fs::write(directory.path().join(RECOVERY_LOG_NAME), lines).unwrap();
        let (status, events) = read_history(&journal);
        assert_eq!(status, usque_ipc::agent_v1::RecoveryHistoryStatus::Partial);
        assert_eq!(events.len(), 32);
        assert_eq!(events[0].journal_generation, 9);
        assert!(
            events
                .iter()
                .all(|event| event.occurred_at_unix_ms == 123 && event.elapsed_ms == 10039)
        );
        let output = format!("{events:?}");
        assert!(!output.contains("private") && !output.contains("192.0.2.1"));
        assert!(!journal.exists());
    }

    #[test]
    fn history_missing_oversized_and_invalid_enum_are_explicitly_unavailable() {
        use usque_ipc::agent_v1::RecoveryHistoryStatus;
        let directory = tempfile::tempdir().unwrap();
        let journal = directory.path().join("journal.json");
        let path = directory.path().join(RECOVERY_LOG_NAME);
        assert_eq!(
            read_history(&journal),
            (RecoveryHistoryStatus::Missing, vec![])
        );
        fs::write(&path, vec![b'x'; MAX_LOG_BYTES as usize + 1]).unwrap();
        assert_eq!(
            read_history(&journal),
            (RecoveryHistoryStatus::TooLarge, vec![])
        );
        fs::write(&path, r#"{"timestamp_ms":1,"recovery":{"journal_generation":1,"step":"private-token","restored":true,"elapsed_ms":1}}"#).unwrap();
        assert_eq!(
            read_history(&journal),
            (RecoveryHistoryStatus::Partial, vec![])
        );
    }
    #[test]
    fn recovery_evidence_is_bounded_and_does_not_copy_error_text() {
        let directory = tempfile::tempdir().unwrap();
        let journal = directory.path().join("journal.json");
        let path = directory.path().join(RECOVERY_LOG_NAME);
        fs::write(&path, vec![b'x'; MAX_LOG_BYTES as usize]).unwrap();
        record(
            &journal,
            &RecoveryEvent {
                journal_generation: 7,
                step: MutationKind::WintunAdapter,
                restored: false,
                elapsed_ms: 10,
                api: Some(RecoveryApi::from_name("private-token 192.0.2.1")),
                win32_code: Some(170),
                adapter: None,
            },
        )
        .unwrap();
        let output = fs::read_to_string(path).unwrap();
        assert!(output.contains("wintun_adapter") && output.contains("170"));
        assert!(!output.contains("private-token") && !output.contains("192.0.2.1"));
        assert!(output.len() < 1024);
        assert!(
            !journal.exists(),
            "diagnostics are never recovery authority"
        );
    }
}
