//! Non-secret, field-scoped network settings and session application policy.
//!
//! Persistence and runtime application are separate outcomes. A saved profile
//! is never evidence that a running session uses that profile.

use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

use crate::{AppConfig, ConnectionPhase, Profile, ReconfigureClass, SharedNetworkSettings};

macro_rules! network_fields {
    ($consumer:ident) => {
        $consumer! {
            "frontends.tunnel" => frontends.tunnel,
            "frontends.socks5" => frontends.socks5,
            "frontends.http" => frontends.http,
            "transport" => transport,
            "congestion_control" => congestion_control,
            "endpoint.ipv4" => endpoint.ipv4,
            "endpoint.ipv6" => endpoint.ipv6,
            "endpoint.port" => endpoint.port,
            "endpoint.sni" => endpoint.sni,
            "ip_policy" => ip_policy,
            "mtu" => mtu,
            "dns_mode" => dns_mode,
            "dns_servers" => dns_servers,
            "allow_lan" => allow_lan,
            "split_exclusions" => split_exclusions,
            "kill_switch" => kill_switch,
            "auto_connect" => auto_connect,
            "geo_direct_countries" => geo_direct_countries,
            "direct_dns" => direct_dns,
            "proxy.socks5_listeners" => proxy.socks5_listeners,
            "proxy.http_listeners" => proxy.http_listeners,
            "proxy.system_proxy" => proxy.system_proxy,
            "proxy.udp_idle_timeout_seconds" => proxy.udp_idle_timeout_seconds,
            "proxy.dns_mode" => proxy.dns_mode,
            "proxy.dns_servers" => proxy.dns_servers,
        }
    };
}

macro_rules! define_fields {
    ($($name:literal => $($member:ident).+),* $(,)?) => {
        pub const NETWORK_FIELDS: &[&str] = &[$($name),*];

        fn copy_field(target: &mut Profile, source: &Profile, field: &str) -> Result<(), SettingsError> {
            match field {
                $($name => target.$($member).+.clone_from(&source.$($member).+),)*
                _ => return Err(SettingsError::InvalidField),
            }
            Ok(())
        }

        pub fn changed_fields(previous: &Profile, next: &Profile) -> Vec<String> {
            let mut fields = Vec::new();
            $(if previous.$($member).+ != next.$($member).+ { fields.push($name.to_owned()); })*
            fields
        }
    };
}
network_fields!(define_fields);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkSettingsPatch {
    pub operation_id: Uuid,
    pub account_id: Uuid,
    pub values: Profile,
    pub changed_fields: Vec<String>,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ApplyStatus {
    NotRequired,
    Applying,
    Applied,
    Deferred,
    Failed,
    #[default]
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkSettingsState {
    pub source_epoch: Uuid,
    pub sequence: u64,
    pub operation_id: Option<Uuid>,
    pub session_id: Option<String>,
    pub stored_profile: Option<Profile>,
    pub applied_profile: Option<Profile>,
    pub apply_status: ApplyStatus,
    pub deferred_fields: Vec<String>,
    pub error_code: Option<String>,
    pub persisted: Option<bool>,
}

impl Default for NetworkSettingsState {
    fn default() -> Self {
        Self {
            source_epoch: Uuid::new_v4(),
            sequence: 0,
            operation_id: None,
            session_id: None,
            stored_profile: None,
            applied_profile: None,
            apply_status: ApplyStatus::Unknown,
            deferred_fields: Vec::new(),
            error_code: None,
            persisted: None,
        }
    }
}

impl NetworkSettingsState {
    pub fn advance(&mut self) {
        self.sequence = self.sequence.saturating_add(1);
        self.deferred_fields = match (&self.applied_profile, &self.stored_profile) {
            (Some(applied), Some(stored)) if applied.id == stored.id => {
                changed_fields(applied, stored)
                    .into_iter()
                    .filter(|field| field != "auto_connect")
                    .collect()
            }
            (_, Some(_)) => NETWORK_FIELDS
                .iter()
                .filter(|field| **field != "auto_connect")
                .map(|field| (*field).to_owned())
                .collect(),
            _ => Vec::new(),
        };
        if self.apply_status == ApplyStatus::Applied && !self.deferred_fields.is_empty() {
            self.apply_status = ApplyStatus::Deferred;
        }
        // Passwords are only carried by the private runtime plan.
        if let Some(profile) = &mut self.stored_profile {
            profile.proxy.auth_password = None;
        }
        if let Some(profile) = &mut self.applied_profile {
            profile.proxy.auth_password = None;
        }
    }
}

#[derive(Debug, Clone)]
pub struct ApplicationPlan {
    pub target: Option<Profile>,
    pub class: ReconfigureClass,
    pub status: ApplyStatus,
}

#[derive(Debug, Error)]
pub enum SettingsError {
    #[error("the network settings edit context is no longer active")]
    AccountChanged,
    #[error("the network settings patch contains an unsupported field")]
    InvalidField,
    #[error("registration-owned endpoint addresses cannot be edited")]
    ManagedEndpoint,
    #[error("network settings validation failed: {0}")]
    Configuration(#[from] crate::ConfigError),
}

/// Apply only explicit public fields to the latest persisted network settings.
pub fn merge_patch(
    config: &mut AppConfig,
    patch: &NetworkSettingsPatch,
) -> Result<Profile, SettingsError> {
    if config.active_profile_id != Some(patch.account_id) || patch.values.id != patch.account_id {
        return Err(SettingsError::AccountChanged);
    }
    let mut profile = config
        .runtime_profile(patch.account_id)
        .ok_or(SettingsError::AccountChanged)?;
    if patch.changed_fields.len() > NETWORK_FIELDS.len() {
        return Err(SettingsError::InvalidField);
    }
    let managed = config
        .account(patch.account_id)
        .is_some_and(|account| account.managed_endpoint_ips.is_some())
        || config.is_zero_trust_account(patch.account_id);
    for field in &patch.changed_fields {
        if managed && matches!(field.as_str(), "endpoint.ipv4" | "endpoint.ipv6") {
            return Err(SettingsError::ManagedEndpoint);
        }
        copy_field(&mut profile, &patch.values, field)?;
    }
    normalize(&mut profile)?;
    let mut network = SharedNetworkSettings::from_profile(&profile);
    if managed {
        network.endpoint.ipv4 = config.network.endpoint.ipv4;
        network.endpoint.ipv6 = config.network.endpoint.ipv6;
    }
    config.network = network;
    Ok(profile)
}

fn normalize(profile: &mut Profile) -> Result<(), SettingsError> {
    if !profile.frontends.http {
        profile.proxy.system_proxy = false;
    }
    profile.canonicalize_mode();
    profile.canonicalize_geo_direct()?;
    profile.canonicalize_direct_dns();
    profile.validate()?;
    Ok(())
}

/// Plan against the running profile, never against a previously saved draft.
pub fn plan_application(
    applied: Option<&Profile>,
    stored: &Profile,
    fields: &[String],
    phase: ConnectionPhase,
    available: bool,
) -> Result<ApplicationPlan, SettingsError> {
    if fields
        .iter()
        .any(|field| !NETWORK_FIELDS.contains(&field.as_str()))
    {
        return Err(SettingsError::InvalidField);
    }
    if fields.is_empty() || fields.iter().all(|field| field == "auto_connect") {
        return Ok(ApplicationPlan {
            target: None,
            class: ReconfigureClass::PersistOnly,
            status: ApplyStatus::NotRequired,
        });
    }
    let Some(previous) = applied.filter(|profile| profile.id == stored.id) else {
        return Ok(deferred_plan());
    };
    if !available
        || !matches!(
            phase,
            ConnectionPhase::Connected | ConnectionPhase::Degraded
        )
    {
        return Ok(deferred_plan());
    }
    let mut target = previous.clone();
    for field in fields {
        if !matches!(field.as_str(), "congestion_control" | "auto_connect") {
            copy_field(&mut target, stored, field)?;
        }
    }
    normalize(&mut target)?;
    let class = crate::classify_reconfigure(previous, &target);
    if class == ReconfigureClass::PersistOnly {
        return Ok(ApplicationPlan {
            target: None,
            class,
            status: if changed_fields(previous, stored)
                .iter()
                .any(|field| field != "auto_connect")
            {
                ApplyStatus::Deferred
            } else {
                ApplyStatus::Applied
            },
        });
    }
    Ok(ApplicationPlan {
        target: Some(target),
        class,
        status: ApplyStatus::Applying,
    })
}

fn deferred_plan() -> ApplicationPlan {
    ApplicationPlan {
        target: None,
        class: ReconfigureClass::PersistOnly,
        status: ApplyStatus::Deferred,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn patch(config: &AppConfig, fields: &[&str]) -> NetworkSettingsPatch {
        NetworkSettingsPatch {
            operation_id: Uuid::new_v4(),
            account_id: config.active_profile_id.unwrap(),
            values: config.active_profile().unwrap(),
            changed_fields: fields.iter().map(|value| (*value).to_owned()).collect(),
        }
    }

    #[test]
    fn patch_preserves_unrelated_edits_and_account_metadata() {
        let mut config = AppConfig::default();
        let mut edit = patch(&config, &["mtu"]);
        edit.values.mtu = 1400;
        edit.values.name = "stale name".into();
        config.network.allow_lan = true;
        let stored = merge_patch(&mut config, &edit).unwrap();
        assert!(stored.allow_lan);
        assert_eq!(stored.mtu, 1400);
        assert_ne!(stored.name, "stale name");
    }

    #[test]
    fn mixed_edit_retains_algorithm_and_earlier_deferred_fields() {
        let previous = Profile::default();
        let mut stored = previous.clone();
        stored.mtu = 1400;
        stored.congestion_control = crate::CongestionControlAlgorithm::Reno;
        stored.proxy.http_listeners = vec!["127.0.0.1:9090".parse().unwrap()];
        let plan = plan_application(
            Some(&previous),
            &stored,
            &["congestion_control".into(), "proxy.http_listeners".into()],
            ConnectionPhase::Connected,
            true,
        )
        .unwrap();
        assert_eq!(plan.class, ReconfigureClass::HotFrontends);
        let target = plan.target.unwrap();
        assert_eq!(target.mtu, previous.mtu);
        assert_eq!(target.congestion_control, previous.congestion_control);
        assert_eq!(target.proxy.http_listeners, stored.proxy.http_listeners);
    }

    #[test]
    fn transitional_and_busy_sessions_never_schedule_application() {
        let previous = Profile::default();
        let mut stored = previous.clone();
        stored.mtu = 1400;
        for phase in [
            ConnectionPhase::Disconnected,
            ConnectionPhase::Preparing,
            ConnectionPhase::ConnectingHttp3,
            ConnectionPhase::ConnectingHttp2,
            ConnectionPhase::Reconnecting,
            ConnectionPhase::Disconnecting,
            ConnectionPhase::Error,
        ] {
            let plan =
                plan_application(Some(&previous), &stored, &["mtu".into()], phase, true).unwrap();
            assert!(plan.target.is_none());
            assert_eq!(plan.status, ApplyStatus::Deferred);
        }
        assert!(
            plan_application(
                Some(&previous),
                &stored,
                &["mtu".into()],
                ConnectionPhase::Connected,
                false
            )
            .unwrap()
            .target
            .is_none()
        );
    }

    #[test]
    fn credentials_identity_and_unknown_fields_are_rejected() {
        for field in [
            "name",
            "id",
            "mode",
            "proxy.auth_username",
            "proxy.auth_password",
            "future",
        ] {
            let mut config = AppConfig::default();
            let edit = patch(&config, &[field]);
            assert!(merge_patch(&mut config, &edit).is_err());
        }
    }

    #[test]
    fn disabling_http_clears_dependent_system_proxy() {
        let mut config = AppConfig::default();
        config.network.frontends.http = true;
        config.network.proxy.system_proxy = true;
        let mut edit = patch(&config, &["frontends.http"]);
        edit.values.frontends.http = false;
        assert!(!merge_patch(&mut config, &edit).unwrap().proxy.system_proxy);
    }
}
