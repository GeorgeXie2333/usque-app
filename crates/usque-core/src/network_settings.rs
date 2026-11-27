//! Non-secret, field-scoped network settings and session application policy.
//!
//! Persistence and runtime application are separate outcomes. A saved profile
//! is never evidence that a running session uses that profile.

use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

use crate::{
    AppConfig, ConnectionPhase, EndpointPresetTier, Profile, ReconfigureClass,
    SharedNetworkSettings,
};

macro_rules! network_fields {
    ($consumer:ident) => {
        $consumer! {
            "frontends.tunnel" => frontends.tunnel,
            "frontends.socks5" => frontends.socks5,
            "frontends.http" => frontends.http,
            "transport" => transport,
            "tls_fingerprint" => tls_fingerprint,
            "data_plane" => data_plane,
            "congestion_control" => congestion_control,
            "masque_connections" => masque_connections,
            "endpoint.ipv4" => endpoint.ipv4,
            "endpoint.ipv6" => endpoint.ipv6,
            "endpoint.port" => endpoint.port,
            "endpoint.sni" => endpoint.sni,
            "endpoint.selection" => endpoint.selection,
            "endpoint.preset_region" => endpoint.preset_region,
            "endpoint.subscription_id" => endpoint.subscription_id,
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
            "routing" => routing,
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
    /// Bounded, process-local evidence of completed durable saves, independent
    /// of the latest operation's runtime application result.
    #[serde(default)]
    pub persisted_operation_ids: Vec<Uuid>,
}

pub const PERSISTED_OPERATION_HISTORY_LIMIT: usize = 64;

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
            persisted_operation_ids: Vec::new(),
        }
    }
}

impl NetworkSettingsState {
    /// Record only a successful durable store update. Uncertain commits and
    /// matching configuration values do not establish persistence evidence.
    pub fn record_persisted_operation(&mut self, operation_id: Uuid) {
        self.persisted_operation_ids
            .retain(|id| *id != operation_id);
        self.persisted_operation_ids.push(operation_id);
        if self.persisted_operation_ids.len() > PERSISTED_OPERATION_HISTORY_LIMIT {
            self.persisted_operation_ids.remove(0);
        }
    }

    pub fn advance(&mut self) {
        self.sequence = self.sequence.saturating_add(1);
        self.deferred_fields = match (&self.applied_profile, &self.stored_profile) {
            (Some(applied), Some(stored)) if applied.id == stored.id => {
                let mut fields = changed_fields(applied, stored)
                    .into_iter()
                    .filter(|field| field != "auto_connect")
                    .collect::<Vec<_>>();
                if applied.subscription_endpoint != stored.subscription_endpoint
                    && !fields
                        .iter()
                        .any(|field| field == "endpoint.subscription_id")
                {
                    fields.push("endpoint.subscription_id".to_owned());
                }
                fields
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
    let tier = config.account_tier(patch.account_id);
    merge_patch_with_tier(config, patch, tier)
}

pub fn merge_patch_with_tier(
    config: &mut AppConfig,
    patch: &NetworkSettingsPatch,
    tier: Option<EndpointPresetTier>,
) -> Result<Profile, SettingsError> {
    if config.active_profile_id != Some(patch.account_id) || patch.values.id != patch.account_id {
        return Err(SettingsError::AccountChanged);
    }
    let account = config
        .account(patch.account_id)
        .ok_or(SettingsError::AccountChanged)?;
    let mut profile = config.network.hydrate_for_tier(account, tier);
    if patch.changed_fields.len() > NETWORK_FIELDS.len() {
        return Err(SettingsError::InvalidField);
    }
    let managed = account.managed_endpoint_ips.is_some()
        || config.is_zero_trust_account(patch.account_id)
        || tier == Some(EndpointPresetTier::ZeroTrust);
    for field in &patch.changed_fields {
        if matches!(field.as_str(), "split_exclusions" | "bypass_domains") {
            return Err(crate::ConfigError::RoutingUpgradeRequired.into());
        }
        if field == "endpoint.subscription_id"
            && patch.values.endpoint.subscription_id.is_some()
            && tier.is_none()
        {
            return Err(crate::ConfigError::EndpointSubscriptionRequiresVerifiedAccount.into());
        }
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
    let sub_changed = patch
        .changed_fields
        .iter()
        .any(|field| field == "endpoint.subscription_id");
    let preset_changed = patch
        .changed_fields
        .iter()
        .any(|field| field == "endpoint.preset_region");
    let selection_changed = patch
        .changed_fields
        .iter()
        .any(|field| field == "endpoint.selection");
    let manual_ip_changed = patch
        .changed_fields
        .iter()
        .any(|field| matches!(field.as_str(), "endpoint.ipv4" | "endpoint.ipv6"));
    // One patch cannot ask for two endpoint sources at once.
    if sub_changed && profile.endpoint.subscription_id.is_some() {
        if preset_changed && profile.endpoint.preset_region.is_some() {
            return Err(crate::ConfigError::EndpointPresetAndSubscriptionConflict.into());
        }
        if selection_changed && profile.endpoint.selection == crate::EndpointSelection::Automatic {
            return Err(crate::ConfigError::EndpointSubscriptionRequiresCustom.into());
        }
    }
    if sub_changed && profile.endpoint.subscription_id.is_some() {
        profile.endpoint.selection = crate::EndpointSelection::Custom;
        profile.endpoint.preset_region = None;
    } else if selection_changed && profile.endpoint.selection == crate::EndpointSelection::Automatic
    {
        profile.endpoint.preset_region = None;
        profile.endpoint.subscription_id = None;
    } else if preset_changed && profile.endpoint.preset_region.is_some() {
        profile.endpoint.selection = crate::EndpointSelection::Custom;
        profile.endpoint.subscription_id = None;
    } else if manual_ip_changed && !sub_changed {
        // Manual addresses (with or without clearing a preset) replace any
        // bound subscription unless the patch explicitly re-binds one.
        profile.endpoint.subscription_id = None;
    }
    normalize(&mut profile)?;
    let updated_subscription_id = profile.endpoint.subscription_id;
    let deactivating_only = sub_changed
        && updated_subscription_id.is_none()
        && !selection_changed
        && !preset_changed
        && !manual_ip_changed;
    let mut network = SharedNetworkSettings::from_profile(&profile);
    network.consumer_tier_subscriptions = config.network.consumer_tier_subscriptions.clone();
    network.endpoint.subscription_id = None;
    if updated_subscription_id.is_some() || deactivating_only {
        // Keep the user's underlying Automatic / Preset / Manual fallback intact.
        network.endpoint.selection = config.network.endpoint.selection;
        network.endpoint.preset_region = config.network.endpoint.preset_region;
    }
    let mut updated_account = account.clone();
    if managed {
        network.endpoint.ipv4 = config.network.endpoint.ipv4;
        network.endpoint.ipv6 = config.network.endpoint.ipv6;
        network.endpoint.selection = if network.endpoint.preset_region.is_some() {
            crate::EndpointSelection::Custom
        } else {
            config.network.endpoint.selection
        };
        updated_account.zero_trust_subscription_id = updated_subscription_id;
        if manual_ip_changed {
            let pair = crate::ManagedEndpointIps::from_endpoint(&profile.endpoint);
            updated_account.zero_trust_endpoint_override =
                (updated_account.managed_endpoint_ips.as_ref() != Some(&pair)).then_some(pair);
        }
    } else if let Some(tier) = tier {
        network
            .consumer_tier_subscriptions
            .set_for_tier(tier, updated_subscription_id);
    }
    if network.endpoint.preset_region.is_some()
        && !matches!(
            network.ip_policy,
            crate::IpPolicy::Ipv6Only | crate::IpPolicy::PreferIpv6
        )
    {
        network.endpoint.preset_region = None;
    }
    let mut final_profile = network.hydrate_for_tier(&updated_account, tier);
    if final_profile.endpoint.subscription_id == profile.endpoint.subscription_id {
        final_profile.subscription_endpoint = profile.subscription_endpoint.clone();
    }
    normalize(&mut final_profile)?;
    if let Some(account_slot) = config.account_mut(patch.account_id) {
        *account_slot = updated_account;
    }
    config.network = network;
    Ok(final_profile)
}

fn normalize(profile: &mut Profile) -> Result<(), SettingsError> {
    if profile.endpoint.selection == crate::EndpointSelection::Automatic {
        profile.endpoint.preset_region = None;
        profile.endpoint.subscription_id = None;
    }
    if profile.endpoint.subscription_id.is_none() {
        profile.subscription_endpoint = None;
    }
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
            if field == "endpoint.subscription_id" {
                target
                    .subscription_endpoint
                    .clone_from(&stored.subscription_endpoint);
                if target.endpoint.subscription_id.is_some() {
                    target.endpoint.selection = crate::EndpointSelection::Custom;
                    target.endpoint.preset_region = None;
                }
            } else if (field == "endpoint.preset_region" && target.endpoint.preset_region.is_some())
                || (matches!(field.as_str(), "endpoint.ipv4" | "endpoint.ipv6")
                    && stored.endpoint.subscription_id.is_none())
            {
                target.endpoint.subscription_id = None;
                target.subscription_endpoint = None;
            }
        }
    }
    normalize(&mut target)?;
    let class = crate::reconfigure::platform_class(crate::classify_reconfigure(previous, &target));
    if class == ReconfigureClass::PersistOnly {
        return Ok(ApplicationPlan {
            target: None,
            class,
            status: if changed_fields(previous, stored)
                .iter()
                .any(|field| field != "auto_connect")
                || previous.subscription_endpoint != stored.subscription_endpoint
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

    #[test]
    fn persistence_receipts_are_bounded_unique_and_reset_with_the_epoch() {
        let mut state = NetworkSettingsState::default();
        let first = Uuid::from_u128(1);
        for index in 1..=(PERSISTED_OPERATION_HISTORY_LIMIT + 1) {
            state.record_persisted_operation(Uuid::from_u128(index as u128));
        }
        assert_eq!(
            state.persisted_operation_ids.len(),
            PERSISTED_OPERATION_HISTORY_LIMIT
        );
        assert!(!state.persisted_operation_ids.contains(&first));
        let recent = *state.persisted_operation_ids.last().unwrap();
        state.record_persisted_operation(recent);
        assert_eq!(
            state.persisted_operation_ids.len(),
            PERSISTED_OPERATION_HISTORY_LIMIT
        );
        assert_eq!(state.persisted_operation_ids.last(), Some(&recent));
        state.apply_status = ApplyStatus::Failed;
        state.persisted = None;
        state.advance();
        assert!(state.persisted_operation_ids.contains(&recent));
        assert_eq!(
            state.persisted_operation_ids.len(),
            PERSISTED_OPERATION_HISTORY_LIMIT
        );
        let mut legacy = serde_json::to_value(&state).unwrap();
        legacy
            .as_object_mut()
            .unwrap()
            .remove("persisted_operation_ids");
        let decoded: NetworkSettingsState = serde_json::from_value(legacy).unwrap();
        assert!(decoded.persisted_operation_ids.is_empty());
        let reset = NetworkSettingsState::default();
        assert_ne!(reset.source_epoch, state.source_epoch);
        assert!(reset.persisted_operation_ids.is_empty());
    }

    #[test]
    fn a_disconnected_snapshot_fits_the_deferred_fields_wire_budget() {
        // The desktop decoder reserves 64 entries for append-only settings.
        assert!(NETWORK_FIELDS.len() <= 64);
        let mut state = NetworkSettingsState {
            stored_profile: AppConfig::default().active_profile(),
            ..Default::default()
        };
        state.advance();
        assert_eq!(state.deferred_fields.len(), NETWORK_FIELDS.len() - 1);
        assert!(
            !state
                .deferred_fields
                .iter()
                .any(|field| field == "auto_connect")
        );
    }

    #[test]
    fn preset_region_is_shared_cold_and_preserves_managed_manual_fallback() {
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
        let manual = crate::ManagedEndpointIps {
            ipv4: "192.0.2.45".parse().unwrap(),
            ipv6: "2001:db8::45".parse().unwrap(),
        };
        config.account_mut(id).unwrap().zero_trust_endpoint_override = Some(manual.clone());
        let consumer_id = Uuid::new_v4();
        config
            .insert_account(consumer_id, "Consumer".into(), None)
            .unwrap();
        let shared_pair = (config.network.endpoint.ipv4, config.network.endpoint.ipv6);
        let previous = config.active_profile().unwrap();
        let mut edit = patch(&config, &["endpoint.preset_region", "ip_policy"]);
        edit.values.endpoint.preset_region = Some(crate::EndpointPresetRegion::De);
        edit.values.ip_policy = crate::IpPolicy::PreferIpv6;
        let stored = merge_patch(&mut config, &edit).unwrap();
        assert_eq!(stored.endpoint.ipv4, manual.ipv4);
        assert_eq!(stored.endpoint.ipv6, manual.ipv6);
        assert_eq!(
            config.account(id).unwrap().zero_trust_endpoint_override,
            Some(manual)
        );
        assert_eq!(
            (config.network.endpoint.ipv4, config.network.endpoint.ipv6),
            shared_pair
        );
        assert_eq!(
            config.network.endpoint.selection,
            crate::EndpointSelection::Custom
        );
        assert_eq!(
            config
                .runtime_profile(consumer_id)
                .unwrap()
                .endpoint
                .preset_region,
            Some(crate::EndpointPresetRegion::De)
        );
        assert_eq!(
            plan_application(
                Some(&previous),
                &stored,
                &edit.changed_fields,
                ConnectionPhase::Connected,
                true
            )
            .unwrap()
            .class,
            ReconfigureClass::ColdReconnect
        );
        config.validate().unwrap();
        config
            .set_managed_endpoint_ips(id, registered.clone())
            .unwrap();
        assert_eq!(
            config.active_profile().unwrap().endpoint.preset_region,
            Some(crate::EndpointPresetRegion::De)
        );
        assert_eq!(
            config.active_profile().unwrap().endpoint.ipv4,
            registered.ipv4
        );
        assert!(
            config
                .account(id)
                .unwrap()
                .zero_trust_endpoint_override
                .is_none()
        );
        config.network.reset_user_defaults();
        assert!(
            config
                .active_profile()
                .unwrap()
                .endpoint
                .preset_region
                .is_none()
        );
    }

    #[test]
    fn preset_policy_validation_is_atomic_and_manual_and_auto_clear_selection() {
        let mut config = AppConfig::default();
        let mut edit = patch(
            &config,
            &["endpoint.preset_region", "endpoint.selection", "ip_policy"],
        );
        edit.values.endpoint.selection = crate::EndpointSelection::Custom;
        edit.values.endpoint.preset_region = Some(crate::EndpointPresetRegion::Us);
        let previous = config.clone();
        assert!(matches!(
            merge_patch(&mut config, &edit),
            Err(SettingsError::Configuration(
                crate::ConfigError::EndpointPresetRequiresIpv6
            ))
        ));
        assert_eq!(config, previous);
        edit.values.ip_policy = crate::IpPolicy::Ipv6Only;
        merge_patch(&mut config, &edit).unwrap();
        let mut manual = patch(&config, &["endpoint.preset_region"]);
        manual.values.endpoint.preset_region = None;
        assert!(
            merge_patch(&mut config, &manual)
                .unwrap()
                .endpoint
                .preset_region
                .is_none()
        );
        merge_patch(&mut config, &edit).unwrap();
        let mut automatic = patch(&config, &["endpoint.selection"]);
        automatic.values.endpoint.selection = crate::EndpointSelection::Automatic;
        assert!(
            merge_patch(&mut config, &automatic)
                .unwrap()
                .endpoint
                .preset_region
                .is_none()
        );
        config.validate().unwrap();
    }

    fn patch(config: &AppConfig, fields: &[&str]) -> NetworkSettingsPatch {
        NetworkSettingsPatch {
            operation_id: Uuid::new_v4(),
            account_id: config.active_profile_id.unwrap(),
            values: config.active_profile().unwrap(),
            changed_fields: fields.iter().map(|value| (*value).to_owned()).collect(),
        }
    }

    #[test]
    fn fingerprint_patch_is_shared_and_requires_reconnection() {
        let mut config = AppConfig::default();
        let previous = config.active_profile().unwrap();
        let mut edit = patch(&config, &["tls_fingerprint"]);
        edit.values.tls_fingerprint = crate::TlsFingerprint::Default;
        // An unrelated stale value must not be copied by a field-scoped edit.
        edit.values.mtu = 1400;
        let stored = merge_patch(&mut config, &edit).unwrap();
        assert_eq!(stored.mtu, previous.mtu);
        assert_eq!(
            config.network.tls_fingerprint,
            crate::TlsFingerprint::Default
        );
        assert_eq!(changed_fields(&previous, &stored), ["tls_fingerprint"]);
        let plan = plan_application(
            Some(&previous),
            &stored,
            &edit.changed_fields,
            ConnectionPhase::Connected,
            true,
        )
        .unwrap();
        assert_eq!(plan.class, ReconfigureClass::ColdReconnect);
        assert_eq!(
            plan.target.unwrap().tls_fingerprint,
            crate::TlsFingerprint::Default
        );
        let deferred = plan_application(
            None,
            &stored,
            &edit.changed_fields,
            ConnectionPhase::Disconnected,
            true,
        )
        .unwrap();
        assert_eq!(deferred.status, ApplyStatus::Deferred);
    }

    #[test]
    fn legacy_routing_edits_are_atomic_and_other_fields_preserve_new_rules() {
        let mut config = AppConfig::default();
        config.network.routing.ads_enabled = true;
        for field in ["split_exclusions", "bypass_domains"] {
            let old = config.clone();
            let edit = patch(&config, &[field]);
            assert!(matches!(
                merge_patch(&mut config, &edit),
                Err(SettingsError::Configuration(
                    crate::ConfigError::RoutingUpgradeRequired
                ))
            ));
            assert_eq!(config, old);
        }
        let mut edit = patch(&config, &["auto_connect"]);
        edit.values.auto_connect = true;
        edit.values.routing = Default::default();
        merge_patch(&mut config, &edit).unwrap();
        assert!(config.network.routing.ads_enabled);
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
            proton_server_id: None,
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

    #[test]
    fn subscription_patch_rejects_conflicting_sources_and_manual_edits_unbind() {
        let mut config = AppConfig::default();
        let free_id = config.active_profile_id.unwrap();
        config.set_consumer_entitlement(free_id, Some(crate::ConsumerEntitlement::Free));
        let sub = Uuid::new_v4();

        let mut both = patch(
            &config,
            &["endpoint.subscription_id", "endpoint.preset_region"],
        );
        both.values.endpoint.subscription_id = Some(sub);
        both.values.endpoint.preset_region = Some(crate::EndpointPresetRegion::De);
        assert!(matches!(
            merge_patch(&mut config, &both),
            Err(SettingsError::Configuration(
                crate::ConfigError::EndpointPresetAndSubscriptionConflict
            ))
        ));

        let mut automatic = patch(&config, &["endpoint.subscription_id", "endpoint.selection"]);
        automatic.values.endpoint.subscription_id = Some(sub);
        automatic.values.endpoint.selection = crate::EndpointSelection::Automatic;
        assert!(matches!(
            merge_patch(&mut config, &automatic),
            Err(SettingsError::Configuration(
                crate::ConfigError::EndpointSubscriptionRequiresCustom
            ))
        ));

        let mut bind = patch(&config, &["endpoint.subscription_id"]);
        bind.values.endpoint.subscription_id = Some(sub);
        merge_patch(&mut config, &bind).unwrap();

        // Manual addresses while clearing a preset replace the binding even
        // when an older client omits endpoint.subscription_id from the mask.
        let mut manual = patch(
            &config,
            &[
                "endpoint.ipv4",
                "endpoint.preset_region",
                "endpoint.selection",
            ],
        );
        manual.values.endpoint.selection = crate::EndpointSelection::Custom;
        manual.values.endpoint.preset_region = None;
        manual.values.endpoint.ipv4 = "162.159.192.9".parse().unwrap();
        let stored = merge_patch(&mut config, &manual).unwrap();
        assert_eq!(stored.endpoint.subscription_id, None);
    }

    #[test]
    fn endpoint_subscriptions_are_tier_isolated_and_preserve_fallbacks() {
        let mut config = AppConfig::default();
        let free_id = config.active_profile_id.unwrap();
        let plus_id = Uuid::new_v4();
        let zt_id = Uuid::new_v4();
        let unknown_consumer_id = Uuid::new_v4();
        config.insert_account(plus_id, "Plus".into(), None).unwrap();
        config
            .insert_account(unknown_consumer_id, "Unverified".into(), None)
            .unwrap();
        config
            .insert_account(
                zt_id,
                "Work".into(),
                Some(crate::ManagedEndpointIps {
                    ipv4: "162.159.197.8".parse().unwrap(),
                    ipv6: "2606:4700:102::8".parse().unwrap(),
                }),
            )
            .unwrap();
        config
            .identity_bindings
            .insert(zt_id, crate::IdentityProvider::zero_trust("acme").unwrap());
        config.set_consumer_entitlement(free_id, Some(crate::ConsumerEntitlement::Free));
        config.set_consumer_entitlement(plus_id, Some(crate::ConsumerEntitlement::WarpPlus));

        // Unverified consumer accounts fail closed.
        config.active_profile_id = Some(unknown_consumer_id);
        let mut reject_edit = patch(&config, &["endpoint.subscription_id"]);
        reject_edit.values.endpoint.subscription_id = Some(Uuid::new_v4());
        assert!(matches!(
            merge_patch(&mut config, &reject_edit),
            Err(SettingsError::Configuration(
                crate::ConfigError::EndpointSubscriptionRequiresVerifiedAccount
            ))
        ));

        // Free starts with a built-in IPv6 preset fallback, then activates free_sub.
        config.active_profile_id = Some(free_id);
        let mut preset_edit = patch(
            &config,
            &["ip_policy", "endpoint.selection", "endpoint.preset_region"],
        );
        preset_edit.values.ip_policy = crate::IpPolicy::PreferIpv6;
        preset_edit.values.endpoint.selection = crate::EndpointSelection::Custom;
        preset_edit.values.endpoint.preset_region = Some(crate::EndpointPresetRegion::De);
        merge_patch(&mut config, &preset_edit).unwrap();

        let free_sub = Uuid::new_v4();
        let plus_sub = Uuid::new_v4();
        let zt_sub = Uuid::new_v4();

        let mut free_edit = patch(&config, &["endpoint.subscription_id"]);
        free_edit.values.endpoint.subscription_id = Some(free_sub);
        let free_stored = merge_patch(&mut config, &free_edit).unwrap();
        assert_eq!(free_stored.endpoint.subscription_id, Some(free_sub));
        assert_eq!(
            free_stored.endpoint.selection,
            crate::EndpointSelection::Custom
        );
        assert!(free_stored.endpoint.preset_region.is_none());

        // Switching to Plus and ZT sets independent subscriptions without overwriting Free.
        config.active_profile_id = Some(plus_id);
        let mut plus_edit = patch(&config, &["endpoint.subscription_id"]);
        plus_edit.values.endpoint.subscription_id = Some(plus_sub);
        merge_patch(&mut config, &plus_edit).unwrap();

        config.active_profile_id = Some(zt_id);
        let mut zt_edit = patch(&config, &["endpoint.subscription_id"]);
        zt_edit.values.endpoint.subscription_id = Some(zt_sub);
        merge_patch(&mut config, &zt_edit).unwrap();

        // Editing MTU on Free preserves Free's dormant preset fallback and Plus/ZT subs.
        config.active_profile_id = Some(free_id);
        let mut mtu_edit = patch(&config, &["mtu"]);
        mtu_edit.values.mtu = 1400;
        let free_after_mtu = merge_patch(&mut config, &mtu_edit).unwrap();
        assert_eq!(free_after_mtu.endpoint.subscription_id, Some(free_sub));
        assert_eq!(
            config
                .runtime_profile(plus_id)
                .unwrap()
                .endpoint
                .subscription_id,
            Some(plus_sub)
        );
        assert_eq!(
            config
                .runtime_profile(zt_id)
                .unwrap()
                .endpoint
                .subscription_id,
            Some(zt_sub)
        );

        // Deactivating Free restores its underlying preset fallback while keeping Plus/ZT intact.
        let mut deactivate_edit = patch(&config, &["endpoint.subscription_id"]);
        deactivate_edit.values.endpoint.subscription_id = None;
        let restored = merge_patch(&mut config, &deactivate_edit).unwrap();
        assert_eq!(restored.endpoint.subscription_id, None);
        assert_eq!(
            restored.endpoint.preset_region,
            Some(crate::EndpointPresetRegion::De)
        );
        assert_eq!(
            config
                .runtime_profile(plus_id)
                .unwrap()
                .endpoint
                .subscription_id,
            Some(plus_sub)
        );
        assert_eq!(
            config
                .runtime_profile(zt_id)
                .unwrap()
                .endpoint
                .subscription_id,
            Some(zt_sub)
        );

        // Changing subscription_endpoint produces a valid NETWORK_FIELDS deferred diff and ColdReconnect plan.
        let mut applied = config.runtime_profile(plus_id).unwrap();
        let mut stored = applied.clone();
        applied.subscription_endpoint = Some(crate::endpoint_subscriptions::SubscriptionEndpoint {
            id: "v4:162.159.193.10:2408".into(),
            host: crate::endpoint_subscriptions::SubscriptionEndpointHost::Ipv4(
                "162.159.193.10".parse().unwrap(),
            ),
            port: Some(2408),
            note: String::new(),
            pinned: false,
        });
        stored.subscription_endpoint = Some(crate::endpoint_subscriptions::SubscriptionEndpoint {
            id: "v4:162.159.193.11:443".into(),
            host: crate::endpoint_subscriptions::SubscriptionEndpointHost::Ipv4(
                "162.159.193.11".parse().unwrap(),
            ),
            port: Some(443),
            note: String::new(),
            pinned: false,
        });
        let mut state = NetworkSettingsState {
            applied_profile: Some(applied.clone()),
            stored_profile: Some(stored.clone()),
            ..Default::default()
        };
        state.advance();
        assert!(
            state
                .deferred_fields
                .iter()
                .all(|f| NETWORK_FIELDS.contains(&f.as_str()))
        );
        assert!(
            state
                .deferred_fields
                .iter()
                .any(|f| f == "endpoint.subscription_id")
        );
        let plan = plan_application(
            Some(&applied),
            &stored,
            &state.deferred_fields,
            ConnectionPhase::Connected,
            true,
        )
        .unwrap();
        assert_eq!(plan.class, ReconfigureClass::ColdReconnect);
        assert_eq!(
            plan.target.unwrap().subscription_endpoint,
            stored.subscription_endpoint
        );
    }
}
