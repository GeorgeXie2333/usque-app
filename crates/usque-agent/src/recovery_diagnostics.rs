//! Bounded, non-authoritative recovery evidence. Never contains receipts,
//! addresses, device identifiers, paths, account data or arbitrary error text.
use std::{
    fs,
    io::{self, Seek, SeekFrom, Write},
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::Serialize;

use crate::journal::MutationKind;

pub const RECOVERY_LOG_NAME: &str = "recovery-events-v1.jsonl";
const MAX_LOG_BYTES: u64 = 1024 * 1024;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RemovalStage {
    Observe,
    Request,
    Confirm,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RemovalFailure {
    Pending,
    Identity,
    Native,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
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

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
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

#[derive(Serialize)]
pub struct RecoveryEvent {
    pub journal_generation: u64,
    pub step: MutationKind,
    pub restored: bool,
    pub elapsed_ms: u64,
    pub api: Option<RecoveryApi>,
    pub win32_code: Option<u32>,
    pub adapter: Option<AdapterRemovalDiagnostic>,
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

#[cfg(test)]
mod tests {
    use super::*;
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
