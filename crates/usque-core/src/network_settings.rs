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
            "data_plane" => data_plane,
            "congestion_control" => congestion_control,
            "endpoint.ipv4" => endpoint.ipv4,
            "endpoint.ipv6" => endpoint.ipv6,
            "endpoint.port" => endpoint.port,
            "endpoint.sni" => endpoint.sni,
            "endpoint.selection" => endpoint.selection,
            "ip_policy" => ip_policy,
            "mtu" => mtu,
            "dns_mode" => dns_mode,
            "dns_servers" => dns_servers,
            "warp_dns" => warp_dns,
            "allow_lan" => allow_lan,
            "disable_quic" => disable_quic,
            "split_exclusions" => split_exclusions,
            "kill_switch" => kill_switch,
            "auto_connect" => auto_connect,
            "geo_direct_countries" => geo_direct_countries,
            "bypass_domains" => bypass_domains,
            "direct_dns" => direct_dns,
            "vpn_gate" => vpn_gate,
            "chain_exit" => chain_exit,
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
            if field == "vpn_gate" && source.chain_exit.is_none() {
                if target.custom_chain().is_some_and(|c| c.profile_id.is_some()) {
                    return Err(SettingsError::InvalidField);
                }
                target.chain_exit = None;
            }
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
    pub shared_network_profile: Option<Profile>,
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
            shared_network_profile: None,
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
        if let Some(profile) = &mut self.shared_network_profile {
            profile.proxy.auth_password = None;
        }
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
    #[error("Zero Trust endpoint selection is fixed or registered addresses are missing")]
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
        if managed
            && (field == "endpoint.selection"
                || matches!(field.as_str(), "endpoint.ipv4" | "endpoint.ipv6")
                    && config
                        .account(patch.account_id)
                        .is_none_or(|account| account.managed_endpoint_ips.is_none()))
        {
            return Err(SettingsError::ManagedEndpoint);
        }
        copy_field(&mut profile, &patch.values, field)?;
    }
    normalize(&mut profile)?;
    let mut network = SharedNetworkSettings::from_profile(&profile);
    if managed {
        network.endpoint.ipv4 = config.network.endpoint.ipv4;
        network.endpoint.ipv6 = config.network.endpoint.ipv6;
        network.endpoint.selection = config.network.endpoint.selection;
        if patch
            .changed_fields
            .iter()
            .any(|field| matches!(field.as_str(), "endpoint.ipv4" | "endpoint.ipv6"))
        {
            let pair = crate::ManagedEndpointIps::from_endpoint(&profile.endpoint);
            let account = config
                .account_mut(patch.account_id)
                .ok_or(SettingsError::AccountChanged)?;
            account.zero_trust_endpoint_override =
                (account.managed_endpoint_ips.as_ref() != Some(&pair)).then_some(pair);
        }
    }
    config.network = network;
    Ok(profile)
}

fn normalize(profile: &mut Profile) -> Result<(), SettingsError> {
    if let Some(chain) = &profile.chain_exit {
        profile.vpn_gate.enabled =
            chain.enabled && chain.source == crate::chain_exit::ChainSource::VpnGate;
    }
    if !profile.frontends.http {
        profile.proxy.system_proxy = false;
    }
    profile.canonicalize_mode();
    profile.canonicalize_geo_direct()?;
    profile.canonicalize_direct_dns();
    profile.canonicalize_warp_dns();
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
    fn zero_trust_overrides_are_account_scoped_masked_cold_and_resettable() {
        let mut config = AppConfig::default();
        let id = config.active_profile_id.unwrap();
        let registered = crate::ManagedEndpointIps {
            ipv4: "162.159.197.2".parse().unwrap(),
            ipv6: "2606:4700:102::2".parse().unwrap(),
        };
        config
            .set_managed_endpoint_ips(id, registered.clone())
            .unwrap();
        config
            .identity_bindings
            .insert(id, crate::IdentityProvider::zero_trust("example").unwrap());
        let other = Uuid::new_v4();
        config
            .insert_account(other, "Other".into(), Some(registered.clone()))
            .unwrap();
        let shared = config.network.clone();
        let previous = config.active_profile().unwrap();
        let mut edit = patch(&config, &["endpoint.ipv4"]);
        edit.values.endpoint.ipv4 = "192.0.2.45".parse().unwrap();
        // The IPv6 draft is omitted from the field mask.
        edit.values.endpoint.ipv6 = "2001:db8::45".parse().unwrap();
        let stored = merge_patch(&mut config, &edit).unwrap();
        assert_eq!(stored.endpoint.ipv4, edit.values.endpoint.ipv4);
        assert_eq!(stored.endpoint.ipv6, registered.ipv6);
        assert_eq!(config.network, shared);
        assert_eq!(
            config.account(id).unwrap().managed_endpoint_ips,
            Some(registered.clone())
        );
        assert_eq!(
            config.runtime_profile(other).unwrap().endpoint,
            previous.endpoint
        );
        let plan = plan_application(
            Some(&previous),
            &stored,
            &edit.changed_fields,
            ConnectionPhase::Connected,
            true,
        )
        .unwrap();
        assert_eq!(plan.class, ReconfigureClass::ColdReconnect);
        assert_eq!(plan.target.unwrap().endpoint, stored.endpoint);
        let mut reset = patch(&config, &["endpoint.ipv4", "endpoint.ipv6"]);
        reset.values.endpoint.ipv4 = registered.ipv4;
        reset.values.endpoint.ipv6 = registered.ipv6;
        merge_patch(&mut config, &reset).unwrap();
        assert!(
            config
                .account(id)
                .unwrap()
                .zero_trust_endpoint_override
                .is_none()
        );
        assert_eq!(config.active_profile().unwrap(), previous);
        edit.changed_fields.push("not_a_field".into());
        let before = config.clone();
        assert!(merge_patch(&mut config, &edit).is_err());
        assert_eq!(config, before);
        config.account_mut(id).unwrap().managed_endpoint_ips = None;
        let edit = patch(&config, &["endpoint.ipv4"]);
        assert!(matches!(
            merge_patch(&mut config, &edit),
            Err(SettingsError::ManagedEndpoint)
        ));
    }

    #[test]
    fn disabling_server_resolved_chain_requires_a_compatible_dns_patch() {
        let mut config = AppConfig::default();
        config.network.chain_exit = Some(crate::chain_exit::ChainExitSettings {
            enabled: true,
            source: crate::chain_exit::ChainSource::HttpProxy,
            profile_id: Some(Uuid::new_v4()),
            revision: Some(Uuid::new_v4()),
            endpoint_override: None,
        });
        config.network.proxy.dns_mode = crate::ProxyDnsMode::EdgeResolved;
        let before = config.active_profile().unwrap();
        before.validate().unwrap();
        let mut edit = patch(&config, &["chain_exit"]);
        edit.values.chain_exit.as_mut().unwrap().enabled = false;
        assert!(matches!(
            merge_patch(&mut config, &edit),
            Err(SettingsError::Configuration(
                crate::ConfigError::EdgeDnsRequiresL4
            ))
        ));
        assert!(config.active_profile().unwrap().chain_enabled());
        assert_eq!(
            config.network.proxy.dns_mode,
            crate::ProxyDnsMode::EdgeResolved
        );

        edit.values.proxy.dns_mode = crate::ProxyDnsMode::Remote;
        edit.changed_fields.push("proxy.dns_mode".into());
        let stored = merge_patch(&mut config, &edit).unwrap();
        assert!(!stored.chain_enabled());
        assert_eq!(stored.proxy.dns_mode, crate::ProxyDnsMode::Remote);
        assert_eq!(stored.dns_servers, before.dns_servers);
        assert_eq!(stored.proxy.dns_servers, before.proxy.dns_servers);
    }

    #[test]
    fn warp_dns_patch_is_canonical_field_scoped_and_cold() {
        let mut config = AppConfig::default();
        let previous = config.active_profile().unwrap();
        let mut edit = patch(&config, &["warp_dns"]);
        edit.values.warp_dns = crate::WarpDnsSettings {
            mode: crate::WarpDnsMode::Doh,
            server_name: "DNS.Example.COM".into(),
            bootstrap_ips: vec!["192.0.2.53".parse().unwrap()],
            ..Default::default()
        };
        edit.values.dns_servers = vec!["9.9.9.9".parse().unwrap()];
        let stored = merge_patch(&mut config, &edit).unwrap();
        assert_eq!(stored.warp_dns.server_name, "dns.example.com");
        assert_eq!(stored.warp_dns.port, 443);
        assert_eq!(stored.warp_dns.doh_path, "/dns-query");
        assert_eq!(stored.dns_servers, previous.dns_servers);
        assert_eq!(changed_fields(&previous, &stored), ["warp_dns"]);
        let plan = plan_application(
            Some(&previous),
            &stored,
            &edit.changed_fields,
            ConnectionPhase::Connected,
            true,
        )
        .unwrap();
        assert_eq!(plan.class, ReconfigureClass::ColdReconnect);
        assert_eq!(plan.target.unwrap().warp_dns, stored.warp_dns);
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
    fn quic_patch_preserves_geo_and_defers_only_when_session_is_unavailable() {
        let mut config = AppConfig::default();
        config.network.geo_direct_countries = vec!["JP".into()];
        let previous = config.active_profile().unwrap();
        let mut edit = patch(&config, &["disable_quic"]);
        edit.values.disable_quic = true;
        edit.values.geo_direct_countries.clear();
        let stored = merge_patch(&mut config, &edit).unwrap();
        assert!(stored.disable_quic);
        assert_eq!(stored.geo_direct_countries, previous.geo_direct_countries);
        for available in [true, false] {
            let plan = plan_application(
                Some(&previous),
                &stored,
                &edit.changed_fields,
                ConnectionPhase::Connected,
                available,
            )
            .unwrap();
            assert_eq!(
                plan.class,
                if available {
                    ReconfigureClass::HotTrafficPolicy
                } else {
                    ReconfigureClass::PersistOnly
                }
            );
            assert_eq!(
                plan.status,
                if available {
                    ApplyStatus::Applying
                } else {
                    ApplyStatus::Deferred
                }
            );
        }
        for phase in [
            ConnectionPhase::Disconnected,
            ConnectionPhase::Reconnecting,
            ConnectionPhase::Disconnecting,
        ] {
            let plan =
                plan_application(Some(&previous), &stored, &edit.changed_fields, phase, true)
                    .unwrap();
            assert_eq!(plan.status, ApplyStatus::Deferred);
            assert!(plan.target.is_none());
        }
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
    fn automatic_proxy_to_vpn_application_requires_a_new_underlay() {
        let mut config = AppConfig::default();
        config.network.frontends.tunnel = false;
        let previous = config.active_profile().unwrap();
        let mut edit = patch(&config, &["frontends.tunnel"]);
        edit.values.frontends.tunnel = true;
        let stored = merge_patch(&mut config, &edit).unwrap();
        let plan = plan_application(
            Some(&previous),
            &stored,
            &edit.changed_fields,
            ConnectionPhase::Connected,
            true,
        )
        .unwrap();
        assert_eq!(plan.class, ReconfigureClass::ColdReconnect);
        assert_eq!(plan.status, ApplyStatus::Applying);
        assert_eq!(plan.target.as_ref(), Some(&stored));
        assert!(stored.frontends.tunnel);
    }

    #[test]
    fn endpoint_selection_is_cold_and_managed_accounts_preserve_shared_mode() {
        let mut config = AppConfig::default();
        let previous = config.active_profile().unwrap();
        let mut edit = patch(&config, &["endpoint.selection"]);
        edit.values.endpoint.selection = crate::EndpointSelection::Custom;
        let stored = merge_patch(&mut config, &edit).unwrap();
        let plan = plan_application(
            Some(&previous),
            &stored,
            &edit.changed_fields,
            ConnectionPhase::Connected,
            true,
        )
        .unwrap();
        assert_eq!(plan.class, ReconfigureClass::ColdReconnect);
        config.network.endpoint.selection = crate::EndpointSelection::Automatic;
        let id = config.active_profile_id.unwrap();
        config
            .set_managed_endpoint_ips(
                id,
                crate::ManagedEndpointIps {
                    ipv4: "162.159.197.2".parse().unwrap(),
                    ipv6: "2606:4700:102::2".parse().unwrap(),
                },
            )
            .unwrap();
        let edit = patch(&config, &["endpoint.port"]);
        let stored = merge_patch(&mut config, &edit).unwrap();
        assert_eq!(stored.endpoint.selection, crate::EndpointSelection::Custom);
        assert_eq!(
            config.network.endpoint.selection,
            crate::EndpointSelection::Automatic
        );
        let edit = patch(&config, &["endpoint.selection"]);
        assert!(matches!(
            merge_patch(&mut config, &edit),
            Err(SettingsError::ManagedEndpoint)
        ));
    }

    #[test]
    fn disabling_http_clears_dependent_system_proxy() {
        let mut config = AppConfig::default();
        config.network.frontends.http = true;
        config.network.proxy.system_proxy = true;
        let mut edit = patch(&config, &["frontends.http"]);
        edit.values.frontends.http = false;
        let previous = config.active_profile().unwrap();
        let stored = merge_patch(&mut config, &edit).unwrap();
        assert!(!stored.proxy.system_proxy);
        let plan = plan_application(
            Some(&previous),
            &stored,
            &edit.changed_fields,
            ConnectionPhase::Connected,
            true,
        )
        .unwrap();
        assert_eq!(plan.class, ReconfigureClass::HotFrontends);
        let target = plan.target.unwrap();
        assert!(!target.frontends.http);
        assert!(!target.proxy.system_proxy);
        assert_eq!(target.frontends.tunnel, previous.frontends.tunnel);
        assert_eq!(target.frontends.socks5, previous.frontends.socks5);
    }
}
