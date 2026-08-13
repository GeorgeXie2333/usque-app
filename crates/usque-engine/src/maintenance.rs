use std::{
    fs::{self, File, OpenOptions},
    io::{self, BufReader, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    time::Duration,
};

use crate::logging::{log_directory, sanitize_log_bytes};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tempfile::NamedTempFile;
use thiserror::Error;
use tokio::sync::Mutex;
use usque_core::{
    AppConfig, ConnectionSnapshot,
    update::{UpdateChecker, UpdateError, UpdateInfo},
};

const UPDATE_STATE_SCHEMA: u32 = 1;
const UPDATE_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);
const MAX_UPDATE_STATE_BYTES: u64 = 64 * 1024;
const MAX_DIAGNOSTIC_LOG_BYTES: usize = 2 * 1024 * 1024;

pub struct Maintenance {
    update_checker: UpdateChecker,
    update_state_path: PathBuf,
    log_directory: PathBuf,
    flag_cache_directory: PathBuf,
    config_backup_path: PathBuf,
    update_lock: Mutex<()>,
}

impl Maintenance {
    pub fn new(config_path: &Path) -> Self {
        let parent = config_path.parent().unwrap_or_else(|| Path::new("."));
        Self {
            update_checker: UpdateChecker::new()
                .expect("the static GitHub update client configuration must be valid"),
            update_state_path: parent.join("update-state-v1.json"),
            log_directory: log_directory(config_path),
            flag_cache_directory: parent.join("cache").join("flag-icons-7.5.0"),
            config_backup_path: config_path.with_extension("json.bak"),
            update_lock: Mutex::new(()),
        }
    }

    pub async fn check_update(
        &self,
        manual: bool,
        enabled: bool,
    ) -> Result<UpdateInfo, MaintenanceError> {
        if !manual && !enabled {
            return Ok(UpdateInfo::current());
        }
        let _guard = self.update_lock.lock().await;
        if !manual
            && let Some(cached) = load_update_state(&self.update_state_path)?
            && Utc::now()
                .signed_duration_since(cached.checked_at)
                .to_std()
                .unwrap_or_default()
                < UPDATE_INTERVAL
        {
            return Ok(cached.info);
        }

        let info = self.update_checker.check(env!("CARGO_PKG_VERSION")).await?;
        let state_path = self.update_state_path.clone();
        let cached = CachedUpdateState {
            schema_version: UPDATE_STATE_SCHEMA,
            checked_at: Utc::now(),
            info: info.clone(),
        };
        tokio::task::spawn_blocking(move || save_update_state(&state_path, &cached))
            .await
            .map_err(|error| MaintenanceError::Worker(error.to_string()))??;
        Ok(info)
    }

    pub async fn export_diagnostics(
        &self,
        destination: PathBuf,
        config: AppConfig,
        snapshot: ConnectionSnapshot,
    ) -> Result<(), MaintenanceError> {
        let log_directory = self.log_directory.clone();
        tokio::task::spawn_blocking(move || {
            write_diagnostic_bundle(&destination, &config, &snapshot, &log_directory)
        })
        .await
        .map_err(|error| MaintenanceError::Worker(error.to_string()))?
    }

    pub async fn clear_local_state(&self) -> Result<(), MaintenanceError> {
        let update_state_path = self.update_state_path.clone();
        let log_directory = self.log_directory.clone();
        let flag_cache_directory = self.flag_cache_directory.clone();
        let config_backup_path = self.config_backup_path.clone();
        tokio::task::spawn_blocking(move || {
            remove_file_if_present(&update_state_path)?;
            remove_file_if_present(&config_backup_path)?;
            if flag_cache_directory.is_dir() {
                fs::remove_dir_all(&flag_cache_directory)?;
            }
            clear_engine_logs(&log_directory)
        })
        .await
        .map_err(|error| MaintenanceError::Worker(error.to_string()))??;
        Ok(())
    }
}

fn remove_file_if_present(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn clear_engine_logs(directory: &Path) -> io::Result<()> {
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    for entry in entries {
        let entry = entry?;
        let metadata = entry.metadata()?;
        if !metadata.is_file() {
            continue;
        }
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name == "engine.jsonl" {
            OpenOptions::new()
                .write(true)
                .truncate(true)
                .open(entry.path())?;
        } else if name.starts_with("engine-") && name.ends_with(".jsonl") {
            fs::remove_file(entry.path())?;
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CachedUpdateState {
    schema_version: u32,
    checked_at: DateTime<Utc>,
    info: UpdateInfo,
}

fn load_update_state(path: &Path) -> Result<Option<CachedUpdateState>, MaintenanceError> {
    let metadata = match fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    if metadata.len() > MAX_UPDATE_STATE_BYTES {
        return Ok(None);
    }
    let state: CachedUpdateState = serde_json::from_reader(BufReader::new(File::open(path)?))?;
    Ok((state.schema_version == UPDATE_STATE_SCHEMA).then_some(state))
}

fn save_update_state(path: &Path, state: &CachedUpdateState) -> Result<(), MaintenanceError> {
    let parent = path
        .parent()
        .ok_or_else(|| MaintenanceError::InvalidDestination(path.to_owned()))?;
    fs::create_dir_all(parent)?;
    let mut temporary = NamedTempFile::new_in(parent)?;
    serde_json::to_writer(&mut temporary, state)?;
    temporary.write_all(b"\n")?;
    temporary.as_file().sync_all()?;
    replace_file(temporary.path(), path)?;
    let _ = temporary.keep();
    Ok(())
}

fn write_diagnostic_bundle(
    destination: &Path,
    config: &AppConfig,
    snapshot: &ConnectionSnapshot,
    log_directory: &Path,
) -> Result<(), MaintenanceError> {
    if !destination.is_absolute()
        || !destination
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("zip"))
    {
        return Err(MaintenanceError::InvalidDestination(destination.to_owned()));
    }
    let parent = destination
        .parent()
        .ok_or_else(|| MaintenanceError::InvalidDestination(destination.to_owned()))?;
    if !parent.is_dir() {
        return Err(MaintenanceError::InvalidDestination(destination.to_owned()));
    }

    let log = collect_sanitized_logs(log_directory)?;
    let mut contents = vec![
        "manifest.json",
        "configuration-summary.json",
        "connection-summary.json",
        "README.txt",
    ];
    if !log.is_empty() {
        contents.push("engine-log.jsonl");
    }
    let manifest = serde_json::json!({
        "schema_version": 1,
        "created_at": Utc::now(),
        "app_version": env!("CARGO_PKG_VERSION"),
        "operating_system": std::env::consts::OS,
        "architecture": std::env::consts::ARCH,
        "contents": contents,
        "excluded": [
            "WARP Secret",
            "private key",
            "access token",
            "device ID",
            "license",
            "endpoint pin",
            "exit IP addresses",
            "custom endpoint and DNS addresses",
            "split-exclusion CIDRs"
        ]
    });
    let configuration = configuration_summary(config);
    let connection = connection_summary(snapshot);
    let readme = concat!(
        "Usque diagnostic bundle\n\n",
        "This archive is created locally and is never uploaded automatically.\n",
        "Identity secrets, cryptographic material, full network addresses, and ",
        "user-provided profile names are deliberately excluded.\n"
    );

    let mut entries = vec![
        (
            "manifest.json".to_owned(),
            serde_json::to_vec_pretty(&manifest)?.into_boxed_slice(),
        ),
        (
            "configuration-summary.json".to_owned(),
            serde_json::to_vec_pretty(&configuration)?.into_boxed_slice(),
        ),
        (
            "connection-summary.json".to_owned(),
            serde_json::to_vec_pretty(&connection)?.into_boxed_slice(),
        ),
        (
            "README.txt".to_owned(),
            readme.as_bytes().to_vec().into_boxed_slice(),
        ),
    ];
    if !log.is_empty() {
        entries.push(("engine-log.jsonl".to_owned(), log.into_boxed_slice()));
    }
    let mut temporary = NamedTempFile::new_in(parent)?;
    write_stored_zip(&mut temporary, &entries)?;
    temporary.as_file().sync_all()?;
    replace_file(temporary.path(), destination)?;
    let _ = temporary.keep();
    Ok(())
}

fn configuration_summary(config: &AppConfig) -> serde_json::Value {
    let profiles = config
        .profiles
        .iter()
        .enumerate()
        .map(|(index, profile)| {
            serde_json::json!({
                "profile": index + 1,
                "active": config.active_profile_id == Some(profile.id),
                "mode": profile.mode,
                "transport": profile.transport,
                "ip_policy": profile.ip_policy,
                "mtu": profile.mtu,
                "dns_mode": profile.dns_mode,
                "dns_server_count": profile.dns_servers.len(),
                "allow_lan": profile.allow_lan,
                "split_exclusion_count": profile.split_exclusions.len(),
                "kill_switch": profile.kill_switch,
                "auto_connect": profile.auto_connect,
                "endpoint": {
                    "uses_default_ipv4": profile.endpoint.ipv4
                        == usque_core::config::DEFAULT_ENDPOINT_V4,
                    "uses_default_ipv6": profile.endpoint.ipv6
                        == usque_core::config::DEFAULT_ENDPOINT_V6,
                    "port": profile.endpoint.port,
                    "uses_default_sni": profile.endpoint.sni
                        == usque_core::config::DEFAULT_SNI,
                },
                "proxy": {
                    "socks5_listener_count": profile.proxy.socks5_listeners.len(),
                    "http_listener_count": profile.proxy.http_listeners.len(),
                    "system_proxy": profile.proxy.system_proxy,
                    "dns_mode": profile.proxy.dns_mode,
                    "dns_server_count": profile.proxy.dns_servers.len(),
                    "udp_idle_timeout_seconds": profile.proxy.udp_idle_timeout_seconds,
                }
            })
        })
        .collect::<Vec<_>>();
    serde_json::json!({
        "schema_version": config.schema_version,
        "profile_count": profiles.len(),
        "preferences": {
            "locale": config.preferences.locale,
            "theme": config.preferences.theme,
            "update_check_enabled": config.preferences.update_check_enabled,
            "log_level": config.preferences.log_level,
        },
        "profiles": profiles,
    })
}

fn connection_summary(snapshot: &ConnectionSnapshot) -> serde_json::Value {
    serde_json::json!({
        "phase": snapshot.phase,
        "changed_at": snapshot.changed_at,
        "transport": snapshot.transport,
        "address_family": snapshot.address_family,
        "ipv4_available": snapshot.ipv4_available,
        "ipv6_available": snapshot.ipv6_available,
        "statistics": snapshot.statistics,
        "exit_ipv4_observed": snapshot.exit.as_ref().and_then(|exit| exit.ipv4).is_some(),
        "exit_ipv6_observed": snapshot.exit.as_ref().and_then(|exit| exit.ipv6).is_some(),
        "error": snapshot.error.as_ref().map(|error| serde_json::json!({
            "code": error.code,
            "retryable": error.retryable,
        })),
        "kill_switch_state": snapshot.kill_switch_state,
        "lockdown_state": snapshot.lockdown_state,
        "reconnect_count": snapshot.reconnect_count,
        "active_listener_count": snapshot.active_listeners.len(),
        "warning_codes": snapshot
            .warnings
            .iter()
            .map(|warning| warning.code.as_str())
            .collect::<Vec<_>>(),
    })
}

fn write_stored_zip(
    writer: &mut impl Write,
    entries: &[(String, Box<[u8]>)],
) -> Result<(), MaintenanceError> {
    if entries.len() > usize::from(u16::MAX) {
        return Err(MaintenanceError::BundleTooLarge);
    }
    let mut central_entries = Vec::with_capacity(entries.len());
    let mut offset = 0_u32;
    for (name, contents) in entries {
        let name = name.as_bytes();
        let name_length =
            u16::try_from(name.len()).map_err(|_| MaintenanceError::BundleTooLarge)?;
        let content_length =
            u32::try_from(contents.len()).map_err(|_| MaintenanceError::BundleTooLarge)?;
        let crc32 = crc32(contents);
        write_u32(writer, 0x0403_4b50)?;
        write_u16(writer, 20)?;
        write_u16(writer, 0x0800)?;
        write_u16(writer, 0)?;
        write_u16(writer, 0)?;
        write_u16(writer, 0)?;
        write_u32(writer, crc32)?;
        write_u32(writer, content_length)?;
        write_u32(writer, content_length)?;
        write_u16(writer, name_length)?;
        write_u16(writer, 0)?;
        writer.write_all(name)?;
        writer.write_all(contents)?;

        central_entries.push((name.to_vec(), crc32, content_length, offset));
        offset = offset
            .checked_add(30)
            .and_then(|value| value.checked_add(u32::from(name_length)))
            .and_then(|value| value.checked_add(content_length))
            .ok_or(MaintenanceError::BundleTooLarge)?;
    }

    let central_offset = offset;
    for (name, crc32, content_length, local_offset) in &central_entries {
        let name_length =
            u16::try_from(name.len()).map_err(|_| MaintenanceError::BundleTooLarge)?;
        write_u32(writer, 0x0201_4b50)?;
        write_u16(writer, 0x0314)?;
        write_u16(writer, 20)?;
        write_u16(writer, 0x0800)?;
        write_u16(writer, 0)?;
        write_u16(writer, 0)?;
        write_u16(writer, 0)?;
        write_u32(writer, *crc32)?;
        write_u32(writer, *content_length)?;
        write_u32(writer, *content_length)?;
        write_u16(writer, name_length)?;
        write_u16(writer, 0)?;
        write_u16(writer, 0)?;
        write_u16(writer, 0)?;
        write_u16(writer, 0)?;
        write_u32(writer, 0o100600 << 16)?;
        write_u32(writer, *local_offset)?;
        writer.write_all(name)?;
        offset = offset
            .checked_add(46)
            .and_then(|value| value.checked_add(u32::from(name_length)))
            .ok_or(MaintenanceError::BundleTooLarge)?;
    }
    let central_size = offset
        .checked_sub(central_offset)
        .ok_or(MaintenanceError::BundleTooLarge)?;
    let entry_count =
        u16::try_from(central_entries.len()).map_err(|_| MaintenanceError::BundleTooLarge)?;
    write_u32(writer, 0x0605_4b50)?;
    write_u16(writer, 0)?;
    write_u16(writer, 0)?;
    write_u16(writer, entry_count)?;
    write_u16(writer, entry_count)?;
    write_u32(writer, central_size)?;
    write_u32(writer, central_offset)?;
    write_u16(writer, 0)?;
    Ok(())
}

fn collect_sanitized_logs(directory: &Path) -> Result<Vec<u8>, MaintenanceError> {
    let mut files = match fs::read_dir(directory) {
        Ok(entries) => entries
            .filter_map(Result::ok)
            .filter_map(|entry| {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                if !(name == "engine.jsonl"
                    || (name.starts_with("engine-") && name.ends_with(".jsonl")))
                {
                    return None;
                }
                let metadata = fs::symlink_metadata(entry.path()).ok()?;
                metadata.file_type().is_file().then_some((
                    entry.path(),
                    metadata.modified().unwrap_or(std::time::UNIX_EPOCH),
                    metadata.len(),
                ))
            })
            .collect::<Vec<_>>(),
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.into()),
    };
    files.sort_by_key(|(_, modified, _)| *modified);

    let mut selected = Vec::new();
    let mut selected_bytes = 0_u64;
    for file in files.into_iter().rev() {
        if selected_bytes >= MAX_DIAGNOSTIC_LOG_BYTES as u64 {
            break;
        }
        selected_bytes = selected_bytes.saturating_add(file.2);
        selected.push(file);
    }
    selected.reverse();

    let mut output = Vec::new();
    for (path, _, length) in selected {
        let remaining = MAX_DIAGNOSTIC_LOG_BYTES.saturating_sub(output.len());
        if remaining == 0 {
            break;
        }
        let mut file = File::open(path)?;
        if length > remaining as u64 {
            file.seek(SeekFrom::End(-(remaining as i64)))?;
        }
        let mut source = Vec::with_capacity(remaining);
        file.take(remaining as u64).read_to_end(&mut source)?;
        if length > remaining as u64
            && let Some(first_newline) = source.iter().position(|byte| *byte == b'\n')
        {
            source.drain(..=first_newline);
        }
        for line in source.split(|byte| *byte == b'\n') {
            let sanitized = sanitize_log_bytes(line);
            if sanitized.is_empty() {
                continue;
            }
            if output.len().saturating_add(sanitized.len() + 1) > MAX_DIAGNOSTIC_LOG_BYTES {
                return Ok(output);
            }
            output.extend_from_slice(&sanitized);
            output.push(b'\n');
        }
    }
    Ok(output)
}

fn write_u16(writer: &mut impl Write, value: u16) -> io::Result<()> {
    writer.write_all(&value.to_le_bytes())
}

fn write_u32(writer: &mut impl Write, value: u32) -> io::Result<()> {
    writer.write_all(&value.to_le_bytes())
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = u32::MAX;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb8_8320 & 0_u32.wrapping_sub(crc & 1));
        }
    }
    !crc
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

    let source = source
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let destination = destination
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    // SAFETY: source and destination are null-terminated wide paths that outlive
    // the synchronous MoveFileExW call.
    let result = unsafe {
        MoveFileExW(
            source.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if result == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[derive(Debug, Error)]
pub enum MaintenanceError {
    #[error("update check failed: {0}")]
    Update(#[from] UpdateError),
    #[error(
        "diagnostic destination must be an absolute path to an existing directory and end in .zip: {0}"
    )]
    InvalidDestination(PathBuf),
    #[error("maintenance I/O failed: {0}")]
    Io(#[from] io::Error),
    #[error("maintenance JSON failed: {0}")]
    Json(#[from] serde_json::Error),
    #[error("the diagnostic bundle exceeded the classic ZIP safety limit")]
    BundleTooLarge,
    #[error("maintenance worker failed: {0}")]
    Worker(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnostic_bundle_contains_only_sanitized_summaries() {
        let directory = tempfile::tempdir().unwrap();
        let destination = directory.path().join("diagnostics.zip");
        let mut config = AppConfig::default();
        config.profiles[0].name = "private hotel name".to_owned();
        config.profiles[0].endpoint.sni = "private.example".to_owned();
        let log_directory = directory.path().join("logs");
        fs::create_dir_all(&log_directory).unwrap();
        fs::write(
            log_directory.join("engine.jsonl"),
            br#"{"peer":"192.0.2.1:443","message":"failed example.com"}"#,
        )
        .unwrap();
        write_diagnostic_bundle(
            &destination,
            &config,
            &ConnectionSnapshot::default(),
            &log_directory,
        )
        .unwrap();

        let combined = String::from_utf8_lossy(&fs::read(destination).unwrap()).into_owned();
        assert!(!combined.contains("private hotel name"));
        assert!(!combined.contains("private.example"));
        assert!(!combined.contains("192.0.2.1"));
        assert!(!combined.contains("example.com"));
        assert!(combined.contains("uses_default_sni"));
        assert!(combined.contains("WARP Secret"));
    }

    #[test]
    fn diagnostic_bundle_rejects_relative_or_non_zip_destinations() {
        assert!(matches!(
            write_diagnostic_bundle(
                Path::new("diagnostics.zip"),
                &AppConfig::default(),
                &ConnectionSnapshot::default(),
                Path::new("missing-logs")
            ),
            Err(MaintenanceError::InvalidDestination(_))
        ));
    }

    #[tokio::test]
    async fn clear_local_state_removes_caches_backups_and_rotated_logs() {
        let directory = tempfile::tempdir().unwrap();
        let config_path = directory.path().join("config.json");
        let maintenance = Maintenance::new(&config_path);
        fs::write(directory.path().join("update-state-v1.json"), b"cached").unwrap();
        fs::write(config_path.with_extension("json.bak"), b"backup").unwrap();
        let flag_cache = directory.path().join("cache").join("flag-icons-7.5.0");
        fs::create_dir_all(&flag_cache).unwrap();
        fs::write(flag_cache.join("us.svg"), b"<svg/>").unwrap();
        let logs = directory.path().join("logs");
        fs::create_dir_all(&logs).unwrap();
        fs::write(logs.join("engine.jsonl"), b"active").unwrap();
        fs::write(logs.join("engine-1-0.jsonl"), b"rotated").unwrap();

        maintenance.clear_local_state().await.unwrap();

        assert!(!directory.path().join("update-state-v1.json").exists());
        assert!(!config_path.with_extension("json.bak").exists());
        assert!(!flag_cache.exists());
        assert_eq!(fs::read(logs.join("engine.jsonl")).unwrap(), b"");
        assert!(!logs.join("engine-1-0.jsonl").exists());
    }
}
