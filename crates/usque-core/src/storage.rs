use std::fs::{self, File};
use std::io::{self, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};

use tempfile::NamedTempFile;
use thiserror::Error;

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::config::{
    Account, AppConfig, AppPreferences, CURRENT_SCHEMA_VERSION, ConfigError, DirectDnsSettings,
    DnsMode, EndpointSettings, FrontendSettings, ManagedEndpointIps, OperatingMode, Profile,
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
        } else if schema < CURRENT_SCHEMA_VERSION {
            let mut config: AppConfig = serde_json::from_value(value)?;
            let preserve_recovery_backup = schema == 11
                && recover_schema_eleven_managed_endpoint_ips(&mut config, &self.backup_path());
            if !preserve_recovery_backup {
                self.back_up_existing()?;
            }
            migrate_app_config(&mut config);
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
        if let Some(profile) = config.active_profile() {
            profile.validate_geo_cache(parent)?;
        }
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

/// Schema 11 intentionally discarded per-account Zero Trust endpoints. Its
/// migration backup is the only durable source that can still identify the
/// exact registration-returned address pair, so consume it before the normal
/// pre-migration backup step could overwrite it.
///
/// A recognized pre-schema-11 backup is preserved even when it is incomplete.
/// Callers can then fail closed and request reauthentication without destroying
/// the last potentially recoverable artifact.
fn recover_schema_eleven_managed_endpoint_ips(config: &mut AppConfig, backup_path: &Path) -> bool {
    let Ok(file) = File::open(backup_path) else {
        return false;
    };
    let Ok(value) = serde_json::from_reader::<_, serde_json::Value>(BufReader::new(file)) else {
        return false;
    };
    let schema = value
        .get("schema_version")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0) as u32;
    if schema >= 11 {
        return false;
    }

    let recovered = if schema < 9 {
        serde_json::from_value::<LegacyStoredConfig>(value)
            .ok()
            .map(|legacy| {
                legacy
                    .profiles
                    .into_iter()
                    .map(|profile| {
                        (
                            profile.id,
                            ManagedEndpointIps::from_endpoint(&profile.endpoint),
                        )
                    })
                    .collect::<BTreeMap<_, _>>()
            })
    } else {
        serde_json::from_value::<AppConfig>(value)
            .ok()
            .map(|backup| {
                backup
                    .profiles
                    .into_iter()
                    .filter_map(|account| {
                        account
                            .managed_endpoint_ips
                            .map(|endpoint| (account.id, endpoint))
                    })
                    .collect::<BTreeMap<_, _>>()
            })
    };

    if let Some(recovered) = recovered {
        let zero_trust_ids = config
            .identity_bindings
            .iter()
            .filter_map(|(profile_id, provider)| {
                matches!(provider, IdentityProvider::ZeroTrust { .. }).then_some(*profile_id)
            })
            .collect::<std::collections::HashSet<_>>();
        for account in &mut config.profiles {
            if account.managed_endpoint_ips.is_none()
                && zero_trust_ids.contains(&account.id)
                && let Some(endpoint) = recovered
                    .get(&account.id)
                    .filter(|endpoint| endpoint.matches_zero_trust_contract())
            {
                account.managed_endpoint_ips = Some(endpoint.clone());
            }
        }
    }
    true
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
    pending_identity_local_deletions: Vec<Uuid>,
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
                    let bound_zero_trust = matches!(
                        identity_bindings.get(&profile.id),
                        Some(IdentityProvider::ZeroTrust { .. })
                    );
                    let managed_endpoint_ips = (bound_zero_trust
                        || profile.endpoint.is_zero_trust_managed())
                    .then(|| ManagedEndpointIps::from_endpoint(&profile.endpoint));
                    Account {
                        id: profile.id,
                        name: profile.name,
                        managed_endpoint_ips,
                    }
                })
                .collect(),
            preferences: legacy.preferences,
            identity_bindings,
            pending_identity_deletions: legacy.pending_identity_deletions,
            pending_identity_local_deletions: legacy.pending_identity_local_deletions,
            pending_identity_creations: legacy.pending_identity_creations,
            pending_identity_replacements: Default::default(),
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

fn migrate_app_config(config: &mut AppConfig) {
    if config.schema_version < 10 {
        config.network.geo_direct_countries.clear();
        config.schema_version = 10;
    }
    if config.schema_version < 11 {
        // The shared network endpoint is already the Consumer/default value in
        // schema 9 and 10. Repair any out-of-contract legacy copy before schema
        // 12 restores only the registration-owned Zero Trust address pair.
        if config.network.endpoint.is_zero_trust_managed() {
            config.network.endpoint = EndpointSettings::default();
        }
        config.schema_version = 11;
    }
    if config.schema_version < 12 {
        config.schema_version = 12;
    }
    if config.schema_version < 13 {
        config.network.direct_dns = DirectDnsSettings::default();
        config.schema_version = 13;
    }
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
            pending_identity_local_deletions: Vec::new(),
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
    fn schema_five_resets_auto_connect_and_keeps_custom_listeners() {
        let directory = tempfile::tempdir().unwrap();
        let store = ConfigStore::new(directory.path().join("config.json"));
        let mut legacy = fat_legacy(5);
        legacy.profiles[0].auto_connect = true;
        legacy.profiles[0].proxy.http_listeners = vec!["192.0.2.5:9090".parse().unwrap()];
        fs::write(store.path(), serde_json::to_vec_pretty(&legacy).unwrap()).unwrap();

        let migrated = store.load().unwrap();
        let profiles = migrated.runtime_profiles();

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
        let migrated_zero_trust = migrated.runtime_profile(zero_trust.id).unwrap();
        assert_eq!(migrated_zero_trust.endpoint.ipv4, zero_trust.endpoint.ipv4);
        assert_eq!(migrated_zero_trust.endpoint.ipv6, zero_trust.endpoint.ipv6);
        assert_eq!(
            migrated_zero_trust.endpoint.port,
            migrated.network.endpoint.port
        );
        assert_eq!(
            migrated_zero_trust.endpoint.sni,
            migrated.network.endpoint.sni
        );
        assert_eq!(migrated.active_profile().unwrap().mtu, 1400);

        let saved: serde_json::Value =
            serde_json::from_slice(&fs::read(store.path()).unwrap()).unwrap();
        assert!(saved["profiles"][0].get("mtu").is_none());
        assert!(saved["profiles"][0].get("frontends").is_none());
        assert!(saved["profiles"][1].get("managed_endpoint").is_none());
        assert!(saved["profiles"][1].get("managed_endpoint_ips").is_some());
    }

    #[test]
    fn schema_nine_migrates_to_empty_geo_direct_countries() {
        let directory = tempfile::tempdir().unwrap();
        let store = ConfigStore::new(directory.path().join("config.json"));
        let config = AppConfig {
            schema_version: 9,
            ..AppConfig::default()
        };
        let mut value = serde_json::to_value(&config).unwrap();
        value.as_object_mut().unwrap().remove("schema_version");
        value["schema_version"] = serde_json::json!(9);
        value["network"]
            .as_object_mut()
            .unwrap()
            .remove("geo_direct_countries");
        fs::write(store.path(), serde_json::to_vec_pretty(&value).unwrap()).unwrap();

        let migrated = store.load().unwrap();
        assert_eq!(migrated.schema_version, CURRENT_SCHEMA_VERSION);
        assert!(migrated.network.geo_direct_countries.is_empty());
        let backup: serde_json::Value =
            serde_json::from_slice(&fs::read(store.backup_path()).unwrap()).unwrap();
        assert_eq!(backup["schema_version"], 9);
        assert!(backup["network"].get("geo_direct_countries").is_none());
    }

    #[test]
    fn schema_ten_keeps_zero_trust_ips_and_uses_shared_port_and_sni() {
        let directory = tempfile::tempdir().unwrap();
        let store = ConfigStore::new(directory.path().join("config.json"));
        let profile_id = Uuid::new_v4();
        let mut config = AppConfig {
            schema_version: 10,
            ..AppConfig::default()
        };
        config
            .insert_account(profile_id, "Work".to_owned(), None)
            .unwrap();
        config.identity_bindings.insert(
            profile_id,
            IdentityProvider::zero_trust("example-team").unwrap(),
        );
        config.network.endpoint.sni = "shared.example.com".to_owned();
        let mut value = serde_json::to_value(&config).unwrap();
        value["schema_version"] = serde_json::json!(10);
        value["profiles"][1]["managed_endpoint"] = serde_json::json!({
            "ipv4": "162.159.197.8",
            "ipv6": "2606:4700:102::8",
            "port": 443,
            "sni": "zt-masque.cloudflareclient.com"
        });
        fs::write(store.path(), serde_json::to_vec_pretty(&value).unwrap()).unwrap();

        let migrated = store.load().unwrap();

        assert_eq!(migrated.schema_version, CURRENT_SCHEMA_VERSION);
        let migrated_zero_trust = migrated.runtime_profile(profile_id).unwrap();
        assert_eq!(
            migrated_zero_trust.endpoint.ipv4,
            "162.159.197.8".parse::<std::net::Ipv4Addr>().unwrap()
        );
        assert_eq!(
            migrated_zero_trust.endpoint.ipv6,
            "2606:4700:102::8".parse::<std::net::Ipv6Addr>().unwrap()
        );
        assert_eq!(
            migrated_zero_trust.endpoint.port,
            migrated.network.endpoint.port
        );
        assert_eq!(migrated_zero_trust.endpoint.sni, "shared.example.com");
        assert_eq!(migrated.network.endpoint.sni, "shared.example.com");
        let saved: serde_json::Value =
            serde_json::from_slice(&fs::read(store.path()).unwrap()).unwrap();
        assert!(saved["profiles"][1].get("managed_endpoint").is_none());
        assert!(saved["profiles"][1].get("managed_endpoint_ips").is_some());
        let backup: serde_json::Value =
            serde_json::from_slice(&fs::read(store.backup_path()).unwrap()).unwrap();
        assert_eq!(backup["schema_version"], 10);
        assert!(backup["profiles"][1].get("managed_endpoint").is_some());
    }

    #[test]
    fn schema_eleven_recovers_zero_trust_ips_without_overwriting_schema_ten_backup() {
        let directory = tempfile::tempdir().unwrap();
        let store = ConfigStore::new(directory.path().join("config.json"));
        let profile_id = Uuid::new_v4();
        let mut config = AppConfig {
            schema_version: 11,
            ..AppConfig::default()
        };
        config
            .insert_account(profile_id, "Work".to_owned(), None)
            .unwrap();
        config.identity_bindings.insert(
            profile_id,
            IdentityProvider::zero_trust("example-team").unwrap(),
        );

        let mut schema_eleven = serde_json::to_value(&config).unwrap();
        schema_eleven["schema_version"] = serde_json::json!(11);
        schema_eleven["profiles"][1]
            .as_object_mut()
            .unwrap()
            .remove("managed_endpoint_ips");
        fs::write(
            store.path(),
            serde_json::to_vec_pretty(&schema_eleven).unwrap(),
        )
        .unwrap();

        let mut schema_ten_backup = schema_eleven.clone();
        schema_ten_backup["schema_version"] = serde_json::json!(10);
        schema_ten_backup["profiles"][1]["managed_endpoint"] = serde_json::json!({
            "ipv4": "162.159.197.8",
            "ipv6": "2606:4700:102::8",
            "port": 443,
            "sni": "zt-masque.cloudflareclient.com"
        });
        fs::write(
            store.backup_path(),
            serde_json::to_vec_pretty(&schema_ten_backup).unwrap(),
        )
        .unwrap();

        let migrated = store.load().unwrap();
        let managed = migrated
            .account(profile_id)
            .unwrap()
            .managed_endpoint_ips
            .as_ref()
            .unwrap();
        assert_eq!(managed.ipv4.to_string(), "162.159.197.8");
        assert_eq!(managed.ipv6.to_string(), "2606:4700:102::8");
        assert!(!migrated.zero_trust_endpoint_needs_reauthentication(profile_id));

        let preserved: serde_json::Value =
            serde_json::from_slice(&fs::read(store.backup_path()).unwrap()).unwrap();
        assert_eq!(preserved["schema_version"], 10);
        assert!(preserved["profiles"][1].get("managed_endpoint").is_some());
    }

    #[test]
    fn schema_eleven_without_recovery_data_requires_zero_trust_reauthentication() {
        let directory = tempfile::tempdir().unwrap();
        let store = ConfigStore::new(directory.path().join("config.json"));
        let profile_id = Uuid::new_v4();
        let mut config = AppConfig {
            schema_version: 11,
            ..AppConfig::default()
        };
        config
            .insert_account(profile_id, "Work".to_owned(), None)
            .unwrap();
        config.identity_bindings.insert(
            profile_id,
            IdentityProvider::zero_trust("example-team").unwrap(),
        );
        fs::write(store.path(), serde_json::to_vec_pretty(&config).unwrap()).unwrap();

        let migrated = store.load().unwrap();
        assert!(migrated.zero_trust_endpoint_needs_reauthentication(profile_id));
        assert!(
            migrated
                .account(profile_id)
                .unwrap()
                .managed_endpoint_ips
                .is_none()
        );
    }

    #[test]
    fn schema_ten_repairs_a_legacy_zero_trust_shared_endpoint() {
        let directory = tempfile::tempdir().unwrap();
        let store = ConfigStore::new(directory.path().join("config.json"));
        let mut config = AppConfig {
            schema_version: 10,
            ..AppConfig::default()
        };
        config.network.endpoint = EndpointSettings {
            ipv4: "162.159.197.8".parse().unwrap(),
            ipv6: "2606:4700:102::8".parse().unwrap(),
            port: 443,
            sni: "zt-masque.cloudflareclient.com".to_owned(),
        };
        fs::write(store.path(), serde_json::to_vec_pretty(&config).unwrap()).unwrap();

        let migrated = store.load().unwrap();

        assert_eq!(migrated.schema_version, CURRENT_SCHEMA_VERSION);
        assert_eq!(migrated.network.endpoint, EndpointSettings::default());
        assert_eq!(
            migrated.active_profile().unwrap().endpoint,
            EndpointSettings::default()
        );
    }

    #[test]
    fn schema_twelve_adds_canonical_physical_direct_dns() {
        let directory = tempfile::tempdir().unwrap();
        let store = ConfigStore::new(directory.path().join("config.json"));
        let mut value = serde_json::to_value(AppConfig::default()).unwrap();
        value["schema_version"] = serde_json::json!(12);
        value["network"]
            .as_object_mut()
            .unwrap()
            .remove("direct_dns");
        fs::write(store.path(), serde_json::to_vec_pretty(&value).unwrap()).unwrap();

        let migrated = store.load().unwrap();

        assert_eq!(migrated.schema_version, CURRENT_SCHEMA_VERSION);
        assert_eq!(migrated.network.direct_dns, DirectDnsSettings::default());
        assert_eq!(
            migrated.active_profile().unwrap().direct_dns,
            DirectDnsSettings::default()
        );
        assert!(store.backup_path().exists());
    }

    #[test]
    fn saving_enabled_country_without_cache_is_rejected() {
        let directory = tempfile::tempdir().unwrap();
        let store = ConfigStore::new(directory.path().join("config.json"));
        let mut config = AppConfig::default();
        config.network.geo_direct_countries = vec!["CN".to_owned()];
        let error = store.save(&config).unwrap_err();
        assert!(matches!(
            error,
            StoreError::Config(ConfigError::GeoDirectCountryNotDownloaded(_))
        ));
    }
}
