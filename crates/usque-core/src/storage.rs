use std::fs::{self, File};
use std::io::{self, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};

use tempfile::NamedTempFile;
use thiserror::Error;

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::config::{
    Account, AppConfig, AppPreferences, CURRENT_SCHEMA_VERSION, ConfigError, DnsMode,
    EndpointSettings, FrontendSettings, LEGACY_DEFAULT_SNI, OperatingMode, Profile,
    SharedNetworkSettings,
};
use crate::identity::IdentityProvider;

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
        let value: serde_json::Value = serde_json::from_reader(BufReader::new(file))?;
        let schema = value
            .get("schema_version")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0) as u32;
        if schema > CURRENT_SCHEMA_VERSION {
            return Err(StoreError::Config(ConfigError::NewerSchema {
                found: schema,
                supported: CURRENT_SCHEMA_VERSION,
            }));
        }
        let config = if schema < 9 {
            self.back_up_existing()?;
            let mut legacy: LegacyStoredConfig = serde_json::from_value(value)?;
            migrate_legacy(&mut legacy)?;
            let config = AppConfig::from_legacy(legacy);
            self.save(&config)?;
            config
        } else {
            serde_json::from_value(value)?
        };
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

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LegacyStoredConfig {
    schema_version: u32,
    active_profile_id: Option<Uuid>,
    #[serde(default)]
    network: SharedNetworkSettings,
    profiles: Vec<Profile>,
    preferences: AppPreferences,
    #[serde(default)]
    identity_bindings: BTreeMap<Uuid, IdentityProvider>,
    #[serde(default)]
    pending_identity_deletions: Vec<Uuid>,
    #[serde(default)]
    pending_identity_creations: Vec<Uuid>,
}

impl AppConfig {
    fn from_legacy(legacy: LegacyStoredConfig) -> Self {
        let identity_bindings = legacy.identity_bindings;
        let mut network = legacy.network;
        if network.endpoint.is_zero_trust_managed() {
            network.endpoint = legacy
                .profiles
                .iter()
                .find(|profile| !profile.endpoint.is_zero_trust_managed())
                .map(|profile| profile.endpoint.clone())
                .unwrap_or_default();
        }
        Self {
            schema_version: CURRENT_SCHEMA_VERSION,
            active_profile_id: legacy.active_profile_id,
            network,
            profiles: legacy
                .profiles
                .into_iter()
                .map(|profile| {
                    let bound_zt = matches!(
                        identity_bindings.get(&profile.id),
                        Some(IdentityProvider::ZeroTrust { .. })
                    );
                    Account {
                        id: profile.id,
                        name: profile.name,
                        managed_endpoint: (bound_zt || profile.endpoint.is_zero_trust_managed())
                            .then_some(profile.endpoint)
                            .filter(EndpointSettings::is_zero_trust_managed),
                    }
                })
                .collect(),
            preferences: legacy.preferences,
            identity_bindings,
            pending_identity_deletions: legacy.pending_identity_deletions,
            pending_identity_creations: legacy.pending_identity_creations,
        }
    }
}

fn migrate_legacy(config: &mut LegacyStoredConfig) -> Result<(), StoreError> {
    while config.schema_version < 8 {
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
            6 => {
                for profile in &mut config.profiles {
                    profile.proxy.normalize_auth();
                }
                config.schema_version = 7;
            }
            7 => {
                let source = config
                    .active_profile_id
                    .and_then(|id| {
                        config
                            .profiles
                            .iter()
                            .find(|profile| profile.id == id)
                            .cloned()
                    })
                    .unwrap_or_default();
                config.network = SharedNetworkSettings::from_profile(&source);
                config.schema_version = 8;
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

    fn fat_legacy(schema_version: u32) -> LegacyStoredConfig {
        let profile = Profile::default();
        LegacyStoredConfig {
            schema_version,
            active_profile_id: Some(profile.id),
            network: SharedNetworkSettings::default(),
            profiles: vec![profile],
            preferences: AppPreferences::default(),
            identity_bindings: BTreeMap::new(),
            pending_identity_deletions: Vec::new(),
            pending_identity_creations: Vec::new(),
        }
    }

    #[test]
    fn schema_one_is_backed_up_and_migrated_to_the_rust_profile_marker() {
        let directory = tempfile::tempdir().unwrap();
        let store = ConfigStore::new(directory.path().join("config.json"));
        let mut legacy = serde_json::to_value(fat_legacy(1)).unwrap();
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
        let mut legacy = fat_legacy(1);
        legacy.profiles[0].mode = OperatingMode::Socks5;
        legacy.profiles[0].proxy.system_proxy = true;
        fs::write(store.path(), serde_json::to_vec_pretty(&legacy).unwrap()).unwrap();

        let migrated = store.load().unwrap();
        let profile = migrated.active_profile().unwrap();

        assert_eq!(migrated.schema_version, CURRENT_SCHEMA_VERSION);
        assert_eq!(profile.mode, OperatingMode::Vpn);
        assert_eq!(profile.frontends, FrontendSettings::platform_default());
        assert!(!profile.proxy.system_proxy);
        let backup: serde_json::Value =
            serde_json::from_slice(&fs::read(store.backup_path()).unwrap()).unwrap();
        assert_eq!(backup["profiles"][0]["proxy"]["system_proxy"], true);
    }

    #[test]
    fn schema_three_migrates_vpn_system_dns_to_tunnel_dns() {
        let directory = tempfile::tempdir().unwrap();
        let store = ConfigStore::new(directory.path().join("config.json"));
        let mut legacy = fat_legacy(3);
        legacy.profiles[0].mode = OperatingMode::Vpn;
        legacy.profiles[0].dns_mode = DnsMode::System;
        fs::write(store.path(), serde_json::to_vec_pretty(&legacy).unwrap()).unwrap();

        let migrated = store.load().unwrap();

        assert_eq!(migrated.schema_version, CURRENT_SCHEMA_VERSION);
        assert_eq!(migrated.active_profile().unwrap().dns_mode, DnsMode::Tunnel);
        let backup: serde_json::Value =
            serde_json::from_slice(&fs::read(store.backup_path()).unwrap()).unwrap();
        assert_eq!(backup["schema_version"], 3);
        assert_eq!(backup["profiles"][0]["dns_mode"], "system");
    }

    #[test]
    fn schema_five_migrates_only_the_exact_legacy_sni_and_keeps_custom_listeners() {
        let directory = tempfile::tempdir().unwrap();
        let store = ConfigStore::new(directory.path().join("config.json"));
        let mut legacy = fat_legacy(5);
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
        let profiles = migrated.runtime_profiles();

        assert_eq!(profiles[0].endpoint.sni, crate::config::DEFAULT_SNI);
        assert_eq!(profiles[1].endpoint.sni, crate::config::DEFAULT_SNI);
        assert_eq!(migrated.network.endpoint.sni, crate::config::DEFAULT_SNI);
        assert!(!profiles[0].auto_connect);
        assert!(
            profiles[0]
                .proxy
                .http_listeners
                .contains(&"192.0.2.5:9090".parse().unwrap())
        );
        if cfg!(windows) {
            assert!(
                profiles[0]
                    .proxy
                    .http_listeners
                    .contains(&"127.0.0.1:8080".parse().unwrap())
            );
        }
        let saved: serde_json::Value =
            serde_json::from_slice(&fs::read(store.path()).unwrap()).unwrap();
        assert!(saved["profiles"][0].get("mtu").is_none());
        assert!(saved["profiles"][0].get("frontends").is_none());
    }

    #[test]
    fn schema_seven_keeps_username_and_never_writes_password_bytes() {
        let directory = tempfile::tempdir().unwrap();
        let store = ConfigStore::new(directory.path().join("config.json"));
        let mut legacy = fat_legacy(6);
        legacy.profiles[0].proxy.auth_username = Some("lan-user".to_owned());
        legacy.profiles[0].proxy.auth_password =
            Some(zeroize::Zeroizing::new(b"super-secret-pass".to_vec()));
        fs::write(store.path(), serde_json::to_vec_pretty(&legacy).unwrap()).unwrap();

        let migrated = store.load().unwrap();
        assert_eq!(migrated.schema_version, CURRENT_SCHEMA_VERSION);
        assert_eq!(
            migrated.network.proxy.auth_username.as_deref(),
            Some("lan-user")
        );
        assert!(migrated.network.proxy.auth_password.is_none());

        store.save(&migrated).unwrap();
        let json = fs::read_to_string(store.path()).unwrap();
        assert!(json.contains("\"auth_username\": \"lan-user\""));
        assert!(!json.to_ascii_lowercase().contains("password"));
        assert!(!json.contains("super-secret-pass"));
        let reloaded: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(reloaded["network"]["proxy"]["auth_username"], "lan-user");
        assert!(
            reloaded["network"]["proxy"]
                .as_object()
                .unwrap()
                .keys()
                .all(|key| !key.to_ascii_lowercase().contains("password"))
        );
    }

    #[test]
    fn schema_eight_fat_profiles_become_slim_accounts() {
        let directory = tempfile::tempdir().unwrap();
        let store = ConfigStore::new(directory.path().join("config.json"));
        let mut legacy = fat_legacy(8);
        legacy.network.mtu = 1400;
        legacy.profiles[0].mtu = 1500;
        let zero_trust = Profile {
            id: uuid::Uuid::new_v4(),
            name: "Work".to_owned(),
            endpoint: EndpointSettings {
                ipv4: "162.159.197.8".parse().unwrap(),
                ipv6: "2606:4700:102::8".parse().unwrap(),
                port: 443,
                sni: "zt-masque.cloudflareclient.com".to_owned(),
            },
            ..Profile::default()
        };
        legacy.identity_bindings.insert(
            zero_trust.id,
            IdentityProvider::ZeroTrust {
                organization: "example-team".to_owned(),
            },
        );
        legacy.network.endpoint = zero_trust.endpoint.clone();
        legacy.profiles.push(zero_trust.clone());
        fs::write(store.path(), serde_json::to_vec_pretty(&legacy).unwrap()).unwrap();

        let migrated = store.load().unwrap();
        assert_eq!(migrated.schema_version, CURRENT_SCHEMA_VERSION);
        assert_eq!(migrated.network.mtu, 1400);
        assert!(!migrated.network.endpoint.is_zero_trust_managed());
        assert_eq!(
            migrated.account(zero_trust.id).unwrap().managed_endpoint,
            Some(zero_trust.endpoint.clone())
        );
        assert_eq!(
            migrated.runtime_profile(zero_trust.id).unwrap().endpoint,
            zero_trust.endpoint
        );
        assert_eq!(migrated.active_profile().unwrap().mtu, 1400);

        let saved: serde_json::Value =
            serde_json::from_slice(&fs::read(store.path()).unwrap()).unwrap();
        assert!(saved["profiles"][0].get("mtu").is_none());
        assert!(saved["profiles"][0].get("frontends").is_none());
        assert!(saved["profiles"][1].get("managed_endpoint").is_some());
    }
}
