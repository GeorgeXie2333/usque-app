use std::fs::{self, File};
use std::io::{self, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};

use tempfile::NamedTempFile;
use thiserror::Error;

use crate::config::{
    AppConfig, CURRENT_SCHEMA_VERSION, ConfigError, DnsMode, FrontendSettings, LEGACY_DEFAULT_SNI,
    OperatingMode,
};

#[derive(Debug, Clone)]
pub struct ConfigStore {
    path: PathBuf,
}

impl ConfigStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn backup_path(&self) -> PathBuf {
        self.path.with_extension("json.bak")
    }

    pub fn load_or_default(&self) -> Result<AppConfig, StoreError> {
        if !self.path.exists() {
            return Ok(AppConfig::default());
        }
        self.load()
    }

    pub fn load(&self) -> Result<AppConfig, StoreError> {
        let file = File::open(&self.path)?;
        let mut config: AppConfig = serde_json::from_reader(BufReader::new(file))?;
        if config.schema_version > CURRENT_SCHEMA_VERSION {
            return Err(StoreError::Config(ConfigError::NewerSchema {
                found: config.schema_version,
                supported: CURRENT_SCHEMA_VERSION,
            }));
        }
        if config.schema_version < CURRENT_SCHEMA_VERSION {
            self.back_up_existing()?;
            migrate(&mut config)?;
            self.save(&config)?;
        }
        config.validate()?;
        Ok(config)
    }

    pub fn save(&self, config: &AppConfig) -> Result<(), StoreError> {
        config.validate()?;
        let parent = self
            .path
            .parent()
            .ok_or_else(|| StoreError::MissingParent(self.path.clone()))?;
        fs::create_dir_all(parent)?;

        let mut temporary = NamedTempFile::new_in(parent)?;
        {
            let mut writer = BufWriter::new(temporary.as_file_mut());
            serde_json::to_writer_pretty(&mut writer, config)?;
            writer.write_all(b"\n")?;
            writer.flush()?;
        }
        temporary.as_file().sync_all()?;

        replace_file(temporary.path(), &self.path)?;
        let _ = temporary.keep();
        sync_parent(parent)?;
        Ok(())
    }

    fn back_up_existing(&self) -> Result<(), StoreError> {
        if self.path.exists() {
            fs::copy(&self.path, self.backup_path())?;
        }
        Ok(())
    }
}

fn migrate(config: &mut AppConfig) -> Result<(), StoreError> {
    while config.schema_version < CURRENT_SCHEMA_VERSION {
        match config.schema_version {
            0 => config.schema_version = 1,
            1 => {
                config.preferences.profiles_migrated_from_flutter = false;
                config.schema_version = 2;
            }
            2 => {
                config.pending_identity_deletions.clear();
                config.schema_version = 3;
            }
            3 => {
                for profile in &mut config.profiles {
                    if profile.mode == OperatingMode::Vpn && profile.dns_mode == DnsMode::System {
                        profile.dns_mode = DnsMode::Tunnel;
                    }
                }
                config.schema_version = 4;
            }
            4 => {
                config.pending_identity_creations.clear();
                config.schema_version = 5;
            }
            5 => {
                for profile in &mut config.profiles {
                    profile.frontends = FrontendSettings::platform_default();
                    profile.mode = OperatingMode::legacy_platform_default();
                    profile.auto_connect = false;
                    profile.proxy.system_proxy = false;
                    if cfg!(windows)
                        && !profile
                            .proxy
                            .http_listeners
                            .iter()
                            .any(|listener| listener.ip().is_loopback())
                    {
                        profile
                            .proxy
                            .http_listeners
                            .push("127.0.0.1:8080".parse().expect("static listener"));
                    }
                    if profile.endpoint.sni == LEGACY_DEFAULT_SNI {
                        profile.endpoint.sni = crate::config::DEFAULT_SNI.to_owned();
                    }
                }
                config.schema_version = 6;
            }
            found => {
                return Err(StoreError::UnsupportedMigration {
                    found,
                    target: CURRENT_SCHEMA_VERSION,
                });
            }
        }
    }
    Ok(())
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

fn sync_parent(parent: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        File::open(parent)?.sync_all()
    }
    #[cfg(not(unix))]
    {
        let _ = parent;
        Ok(())
    }
}

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("configuration I/O failed: {0}")]
    Io(#[from] io::Error),
    #[error("configuration JSON is invalid: {0}")]
    Json(#[from] serde_json::Error),
    #[error("configuration is invalid: {0}")]
    Config(#[from] ConfigError),
    #[error("configuration path has no parent: {0}")]
    MissingParent(PathBuf),
    #[error("cannot migrate configuration schema {found} to {target}")]
    UnsupportedMigration { found: u32, target: u32 },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_and_load_round_trip() {
        let directory = tempfile::tempdir().unwrap();
        let store = ConfigStore::new(directory.path().join("config.json"));
        let config = AppConfig::default();
        store.save(&config).unwrap();
        assert_eq!(store.load().unwrap(), config);
    }

    #[test]
    fn missing_file_returns_defaults_without_creating_plaintext_secrets() {
        let directory = tempfile::tempdir().unwrap();
        let store = ConfigStore::new(directory.path().join("config.json"));
        let config = store.load_or_default().unwrap();
        assert_eq!(config.schema_version, CURRENT_SCHEMA_VERSION);
        assert!(!store.path().exists());
    }

    #[test]
    fn schema_one_is_backed_up_and_migrated_to_the_rust_profile_marker() {
        let directory = tempfile::tempdir().unwrap();
        let store = ConfigStore::new(directory.path().join("config.json"));
        let mut legacy = serde_json::to_value(AppConfig::default()).unwrap();
        legacy["schema_version"] = serde_json::json!(1);
        legacy["preferences"]
            .as_object_mut()
            .unwrap()
            .remove("profiles_migrated_from_flutter");
        legacy
            .as_object_mut()
            .unwrap()
            .remove("pending_identity_deletions");
        fs::write(store.path(), serde_json::to_vec_pretty(&legacy).unwrap()).unwrap();

        let migrated = store.load().unwrap();

        assert_eq!(migrated.schema_version, CURRENT_SCHEMA_VERSION);
        assert!(!migrated.preferences.profiles_migrated_from_flutter);
        let backup: serde_json::Value =
            serde_json::from_slice(&fs::read(store.backup_path()).unwrap()).unwrap();
        assert_eq!(backup["schema_version"], 1);
    }

    #[test]
    fn schema_six_applies_platform_frontend_defaults() {
        let directory = tempfile::tempdir().unwrap();
        let store = ConfigStore::new(directory.path().join("config.json"));
        let mut legacy = AppConfig {
            schema_version: 1,
            ..AppConfig::default()
        };
        legacy.profiles[0].mode = OperatingMode::Socks5;
        legacy.profiles[0].proxy.system_proxy = true;
        fs::write(store.path(), serde_json::to_vec_pretty(&legacy).unwrap()).unwrap();

        let migrated = store.load().unwrap();

        assert_eq!(migrated.schema_version, CURRENT_SCHEMA_VERSION);
        assert_eq!(
            migrated.profiles[0].mode,
            OperatingMode::legacy_platform_default()
        );
        assert_eq!(
            migrated.profiles[0].frontends,
            FrontendSettings::platform_default()
        );
        assert!(!migrated.profiles[0].proxy.system_proxy);
        let backup: serde_json::Value =
            serde_json::from_slice(&fs::read(store.backup_path()).unwrap()).unwrap();
        assert_eq!(backup["profiles"][0]["proxy"]["system_proxy"], true);
    }

    #[test]
    fn schema_three_migrates_vpn_system_dns_to_tunnel_dns() {
        let directory = tempfile::tempdir().unwrap();
        let store = ConfigStore::new(directory.path().join("config.json"));
        let mut legacy = AppConfig {
            schema_version: 3,
            ..AppConfig::default()
        };
        legacy.profiles[0].mode = OperatingMode::Vpn;
        legacy.profiles[0].dns_mode = DnsMode::System;
        fs::write(store.path(), serde_json::to_vec_pretty(&legacy).unwrap()).unwrap();

        let migrated = store.load().unwrap();

        assert_eq!(migrated.schema_version, CURRENT_SCHEMA_VERSION);
        assert_eq!(migrated.profiles[0].dns_mode, DnsMode::Tunnel);
        let backup: serde_json::Value =
            serde_json::from_slice(&fs::read(store.backup_path()).unwrap()).unwrap();
        assert_eq!(backup["schema_version"], 3);
        assert_eq!(backup["profiles"][0]["dns_mode"], "system");
    }

    #[test]
    fn schema_five_migrates_only_the_exact_legacy_sni_and_keeps_custom_listeners() {
        let directory = tempfile::tempdir().unwrap();
        let store = ConfigStore::new(directory.path().join("config.json"));
        let mut legacy = AppConfig {
            schema_version: 5,
            ..AppConfig::default()
        };
        legacy.profiles[0].endpoint.sni = LEGACY_DEFAULT_SNI.to_owned();
        legacy.profiles[0].auto_connect = true;
        legacy.profiles[0].proxy.http_listeners = vec!["192.0.2.5:9090".parse().unwrap()];
        let mut custom = legacy.profiles[0].clone();
        custom.id = uuid::Uuid::new_v4();
        custom.name = "Custom".to_owned();
        custom.endpoint.sni = "custom.example.com".to_owned();
        legacy.profiles.push(custom);
        fs::write(store.path(), serde_json::to_vec_pretty(&legacy).unwrap()).unwrap();

        let migrated = store.load().unwrap();

        assert_eq!(
            migrated.profiles[0].endpoint.sni,
            crate::config::DEFAULT_SNI
        );
        assert_eq!(migrated.profiles[1].endpoint.sni, "custom.example.com");
        assert!(!migrated.profiles[0].auto_connect);
        assert!(
            migrated.profiles[0]
                .proxy
                .http_listeners
                .contains(&"192.0.2.5:9090".parse().unwrap())
        );
        if cfg!(windows) {
            assert!(
                migrated.profiles[0]
                    .proxy
                    .http_listeners
                    .contains(&"127.0.0.1:8080".parse().unwrap())
            );
        }
    }
}
