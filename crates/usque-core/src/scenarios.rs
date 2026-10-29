//! Bounded, non-secret connection scenarios. Platform owners validate resources
//! and execute returned targets; a configuration commit is never a runtime ACK.
use std::collections::HashSet;
use std::net::IpAddr;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use thiserror::Error;
use uuid::Uuid;

use crate::chain_exit::ChainExitSettings;
use crate::network_settings::ApplyStatus;
use crate::vpngate::{Selection, VpnGateSettings};
use crate::{
    AppConfig, DirectDnsSettings, DnsMode, FrontendSettings, IpPolicy, Profile, ProxyDnsMode,
    RoutingSettings, SharedNetworkSettings, TransportPolicy, WarpDnsSettings,
};

pub const MAX_SCENARIOS: usize = 32;
pub const MAX_SCENARIO_BYTES: usize = 64 * 1024;
pub const MAX_SCENARIOS_BYTES: usize = 256 * 1024;
pub const MAX_COMMAND_BYTES: usize = 256 * 1024;
/// UTF-16 Android Binder strings can take twice their UTF-8 ASCII size.
pub const MAX_RESPONSE_BYTES: usize = 384 * 1024;

/// The complete v1 allowlist. Credentials, endpoints, listeners, Kill Switch,
/// automatic connection and platform per-app settings cannot enter this DTO.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ScenarioSettings {
    pub frontends: FrontendSettings,
    pub transport: TransportPolicy,
    pub ip_policy: IpPolicy,
    pub allow_lan: bool,
    pub routing: RoutingSettings,
    pub geo_direct_countries: Vec<String>,
    pub direct_dns: DirectDnsSettings,
    pub warp_dns: WarpDnsSettings,
    pub dns_mode: DnsMode,
    pub dns_servers: Vec<IpAddr>,
    pub proxy_system_proxy: bool,
    pub proxy_dns_mode: ProxyDnsMode,
    pub proxy_dns_servers: Vec<IpAddr>,
    pub vpn_gate: VpnGateSettings,
    pub chain_exit: Option<ChainExitSettings>,
}

impl Default for ScenarioSettings {
    fn default() -> Self {
        Self::from_network(&SharedNetworkSettings::default())
    }
}

impl ScenarioSettings {
    pub fn from_network(network: &SharedNetworkSettings) -> Self {
        Self {
            frontends: network.frontends,
            transport: network.transport,
            ip_policy: network.ip_policy,
            allow_lan: network.allow_lan,
            routing: network.routing.clone(),
            geo_direct_countries: network.geo_direct_countries.clone(),
            direct_dns: network.direct_dns.clone(),
            warp_dns: network.warp_dns.clone(),
            dns_mode: network.dns_mode,
            dns_servers: network.dns_servers.clone(),
            proxy_system_proxy: network.proxy.system_proxy,
            proxy_dns_mode: network.proxy.dns_mode,
            proxy_dns_servers: network.proxy.dns_servers.clone(),
            vpn_gate: network.vpn_gate.clone(),
            chain_exit: network.chain_exit.clone(),
        }
    }

    pub fn from_profile(profile: &Profile) -> Self {
        Self::from_network(&SharedNetworkSettings::from_profile(profile))
    }

    fn copy_into(&self, network: &mut SharedNetworkSettings) {
        network.frontends = self.frontends;
        network.transport = self.transport;
        network.ip_policy = self.ip_policy;
        network.allow_lan = self.allow_lan;
        if !routing_equivalent(&network.routing, &self.routing) {
            network.routing = self.routing.clone();
        }
        network.geo_direct_countries = self.geo_direct_countries.clone();
        network.direct_dns = self.direct_dns.clone();
        network.warp_dns = self.warp_dns.clone();
        network.dns_mode = self.dns_mode;
        network.dns_servers = self.dns_servers.clone();
        network.proxy.system_proxy = self.proxy_system_proxy && self.frontends.http;
        network.proxy.dns_mode = self.proxy_dns_mode;
        network.proxy.dns_servers = self.proxy_dns_servers.clone();
        network.vpn_gate = self.vpn_gate.clone();
        network.chain_exit = self.chain_exit.clone();
        if let Some(chain) = &network.chain_exit {
            network.vpn_gate.enabled =
                chain.enabled && chain.source == crate::chain_exit::ChainSource::VpnGate;
        }
    }

    /// Validate shape and intrinsic relationships, without requiring a live
    /// account, platform, downloaded catalogue or an unlocked credential store.
    pub fn normalized(&self) -> Result<Self, ScenarioError> {
        if encoded_size(self)? > MAX_SCENARIO_BYTES {
            return Err(ScenarioError::Limit);
        }
        let mut network = SharedNetworkSettings::default();
        self.copy_into(&mut network);
        network.routing = network
            .routing
            .normalized()
            .map_err(|_| ScenarioError::InvalidSettings)?;
        let mut profile = network.hydrate(&crate::Account::default_account());
        // Edge-resolved DNS has a global data-plane dependency. Validate that
        // dependency against the real inherited configuration at activation.
        if profile.proxy.dns_mode == ProxyDnsMode::EdgeResolved {
            profile.data_plane = crate::DataPlaneMode::L4Proxy;
        }
        profile
            .canonicalize_geo_direct()
            .map_err(|_| ScenarioError::InvalidSettings)?;
        profile.canonicalize_direct_dns();
        profile.canonicalize_warp_dns();
        profile
            .validate()
            .map_err(|_| ScenarioError::InvalidSettings)?;
        Ok(Self::from_profile(&profile))
    }

    pub fn apply_to(&self, network: &mut SharedNetworkSettings) -> Result<(), ScenarioError> {
        self.normalized()?.copy_into(network);
        Ok(())
    }

    fn semantic_value(&self) -> Result<Value, ScenarioError> {
        let mut normalized = self.normalized()?;
        normalized.routing = semantic_routing(&normalized.routing)?;
        normalized.geo_direct_countries.sort();
        serde_json::to_value(normalized).map_err(|_| ScenarioError::InvalidSettings)
    }

    pub fn equivalent(&self, other: &Self) -> bool {
        match (self.semantic_value(), other.semantic_value()) {
            (Ok(left), Ok(right)) => left == right,
            _ => false,
        }
    }
}

fn semantic_routing(routing: &RoutingSettings) -> Result<RoutingSettings, ScenarioError> {
    let mut normalized = routing
        .normalized()
        .map_err(|_| ScenarioError::InvalidSettings)?;
    normalized
        .rules
        .sort_by_cached_key(|rule| format!("{:?}:{}:{:?}", rule.kind, rule.target, rule.action));
    for rule in &mut normalized.rules {
        rule.id = Uuid::nil();
    }
    Ok(normalized)
}

fn routing_equivalent(left: &RoutingSettings, right: &RoutingSettings) -> bool {
    matches!((semantic_routing(left),semantic_routing(right)),(Ok(left),Ok(right)) if left==right)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ConnectionScenario {
    pub id: Uuid,
    pub name: String,
    pub revision: Uuid,
    pub bound_account_id: Option<Uuid>,
    pub settings: ScenarioSettings,
}

/// The selected template version is retained independently of later edits.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ScenarioSelection {
    pub scenario_id: Uuid,
    pub scenario_revision: Uuid,
    pub bound_account_id: Option<Uuid>,
    pub settings: ScenarioSettings,
}

impl From<&ConnectionScenario> for ScenarioSelection {
    fn from(scenario: &ConnectionScenario) -> Self {
        Self {
            scenario_id: scenario.id,
            scenario_revision: scenario.revision,
            bound_account_id: scenario.bound_account_id,
            settings: scenario.settings.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ScenarioSnapshot {
    pub account_id: Uuid,
    pub settings: ScenarioSettings,
    pub selection: Option<Box<ScenarioSelection>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ScenarioSwitchRecord {
    pub operation_id: Uuid,
    pub source: ScenarioSnapshot,
    pub target: ScenarioSnapshot,
    /// Written only after the owner received the first transaction's durable
    /// acknowledgement. Its presence can recover a lost reply after restart.
    #[serde(default)]
    pub acknowledged: bool,
    /// A restore is not itself offered as an endlessly alternating undo.
    #[serde(default)]
    pub restored: bool,
}

/// Trusted owner observations. This is deliberately not deserializable from
/// Flutter input. Dependency fingerprints are supplied by the native owner.
#[derive(Debug, Clone)]
pub struct ScenarioContext {
    pub source_epoch: Uuid,
    pub session_id: Option<String>,
    pub applied_profile: Option<Profile>,
    pub apply_status: ApplyStatus,
    pub dependency_fingerprint: String,
}

impl Default for ScenarioContext {
    fn default() -> Self {
        Self {
            source_epoch: Uuid::nil(),
            session_id: None,
            applied_profile: None,
            apply_status: ApplyStatus::Unknown,
            dependency_fingerprint: String::new(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum ScenarioCommand {
    List,
    Save {
        id: Option<Uuid>,
        name: String,
        bound_account_id: Option<Uuid>,
        settings: Box<ScenarioSettings>,
        expected_revision: Option<Uuid>,
    },
    Delete {
        id: Uuid,
        expected_revision: Uuid,
    },
    Preview {
        id: Uuid,
        expected_revision: Uuid,
    },
    Activate {
        id: Uuid,
        expected_revision: Uuid,
        preview_token: String,
        operation_id: Uuid,
    },
    PreviewRestore,
    Restore {
        preview_token: String,
        operation_id: Uuid,
    },
}

impl ScenarioCommand {
    pub fn parse(request: &str) -> Result<Self, ScenarioError> {
        if request.len() > MAX_COMMAND_BYTES {
            return Err(ScenarioError::Limit);
        }
        // Internally tagged unit variants do not enforce deny_unknown_fields.
        // Reject owner observations and secrets even on read-only commands.
        let value: Value =
            serde_json::from_str(request).map_err(|_| ScenarioError::InvalidRequest)?;
        let object = value.as_object().ok_or(ScenarioError::InvalidRequest)?;
        let allowed: &[&str] = match object.get("action").and_then(Value::as_str) {
            Some("list" | "preview_restore") => &["action"],
            Some("save") => &[
                "action",
                "id",
                "name",
                "bound_account_id",
                "settings",
                "expected_revision",
            ],
            Some("delete" | "preview") => &["action", "id", "expected_revision"],
            Some("activate") => &[
                "action",
                "id",
                "expected_revision",
                "preview_token",
                "operation_id",
            ],
            Some("restore") => &["action", "preview_token", "operation_id"],
            _ => return Err(ScenarioError::InvalidRequest),
        };
        if object.keys().any(|key| !allowed.contains(&key.as_str())) {
            return Err(ScenarioError::InvalidRequest);
        }
        serde_json::from_value(value).map_err(|_| ScenarioError::InvalidRequest)
    }

    pub fn is_mutation(&self) -> bool {
        matches!(
            self,
            Self::Save { .. } | Self::Delete { .. } | Self::Activate { .. } | Self::Restore { .. }
        )
    }
}

#[derive(Debug)]
pub struct ScenarioCommandResult {
    pub response: Value,
    pub target: Option<Profile>,
    /// Read-only resource/credential validation candidate. Never execute this.
    pub preview_target: Option<Profile>,
    pub changed: bool,
}

#[derive(Debug, Clone, Copy, Error, PartialEq, Eq)]
pub enum ScenarioError {
    #[error("SCENARIO_INVALID_REQUEST")]
    InvalidRequest,
    #[error("SCENARIO_INVALID_SETTINGS")]
    InvalidSettings,
    #[error("SCENARIO_INVALID_NAME")]
    InvalidName,
    #[error("SCENARIO_LIMIT")]
    Limit,
    #[error("SCENARIO_NOT_FOUND")]
    NotFound,
    #[error("SCENARIO_STALE_REVISION")]
    StaleRevision,
    #[error("SCENARIO_ACCOUNT_UNAVAILABLE")]
    AccountUnavailable,
    #[error("SCENARIO_PREVIEW_STALE")]
    PreviewStale,
    #[error("SCENARIO_RESTORE_UNAVAILABLE")]
    RestoreUnavailable,
    #[error("SCENARIO_OPERATION_REUSED")]
    OperationReused,
}

impl ScenarioError {
    pub const fn code(self) -> &'static str {
        match self {
            Self::InvalidRequest => "SCENARIO_INVALID_REQUEST",
            Self::InvalidSettings => "SCENARIO_INVALID_SETTINGS",
            Self::InvalidName => "SCENARIO_INVALID_NAME",
            Self::Limit => "SCENARIO_LIMIT",
            Self::NotFound => "SCENARIO_NOT_FOUND",
            Self::StaleRevision => "SCENARIO_STALE_REVISION",
            Self::AccountUnavailable => "SCENARIO_ACCOUNT_UNAVAILABLE",
            Self::PreviewStale => "SCENARIO_PREVIEW_STALE",
            Self::RestoreUnavailable => "SCENARIO_RESTORE_UNAVAILABLE",
            Self::OperationReused => "SCENARIO_OPERATION_REUSED",
        }
    }
}

fn encoded_size(value: &impl Serialize) -> Result<usize, ScenarioError> {
    serde_json::to_vec(value)
        .map(|bytes| bytes.len())
        .map_err(|_| ScenarioError::InvalidSettings)
}

fn valid_id(id: Uuid) -> bool {
    !id.is_nil()
}

fn validate_selection(selection: &ScenarioSelection) -> Result<(), ScenarioError> {
    if !valid_id(selection.scenario_id)
        || !valid_id(selection.scenario_revision)
        || selection.bound_account_id.is_some_and(|id| !valid_id(id))
    {
        return Err(ScenarioError::InvalidRequest);
    }
    selection.settings.normalized()?;
    Ok(())
}

/// Dangling account/library references are intentionally accepted here. They
/// make one scenario unavailable, never the entire application configuration.
pub fn validate_config(config: &AppConfig) -> Result<(), ScenarioError> {
    if config.scenarios.len() > MAX_SCENARIOS
        || encoded_size(&config.scenarios)? > MAX_SCENARIOS_BYTES
    {
        return Err(ScenarioError::Limit);
    }
    let mut ids = HashSet::new();
    for scenario in &config.scenarios {
        if !valid_id(scenario.id) || !valid_id(scenario.revision) || !ids.insert(scenario.id) {
            return Err(ScenarioError::InvalidRequest);
        }
        validate_name(&scenario.name)?;
        if scenario.bound_account_id.is_some_and(|id| !valid_id(id)) {
            return Err(ScenarioError::InvalidRequest);
        }
        scenario.settings.normalized()?;
    }
    if let Some(selection) = &config.scenario_selection {
        validate_selection(selection)?;
    }
    if let Some(record) = &config.last_scenario_switch {
        if !valid_id(record.operation_id) {
            return Err(ScenarioError::InvalidRequest);
        }
        for snapshot in [&record.source, &record.target] {
            if !valid_id(snapshot.account_id) {
                return Err(ScenarioError::InvalidRequest);
            }
            snapshot.settings.normalized()?;
            if let Some(selection) = &snapshot.selection {
                validate_selection(selection)?;
            }
        }
    }
    Ok(())
}

fn validate_name(name: &str) -> Result<(), ScenarioError> {
    if name.trim().is_empty() || name.chars().count() > 64 || name.chars().any(char::is_control) {
        return Err(ScenarioError::InvalidName);
    }
    Ok(())
}

fn scenario(
    config: &AppConfig,
    id: Uuid,
    revision: Uuid,
) -> Result<&ConnectionScenario, ScenarioError> {
    let scenario = config
        .scenarios
        .iter()
        .find(|s| s.id == id)
        .ok_or(ScenarioError::NotFound)?;
    if scenario.revision != revision {
        return Err(ScenarioError::StaleRevision);
    }
    Ok(scenario)
}

fn snapshot(config: &AppConfig) -> Result<ScenarioSnapshot, ScenarioError> {
    Ok(ScenarioSnapshot {
        account_id: config
            .active_profile_id
            .ok_or(ScenarioError::AccountUnavailable)?,
        settings: ScenarioSettings::from_network(&config.network).normalized()?,
        selection: config.scenario_selection.clone(),
    })
}

pub fn can_restore(config: &AppConfig) -> bool {
    let Some(record) = &config.last_scenario_switch else {
        return false;
    };
    !record.restored
        && config.account(record.source.account_id).is_some()
        && config.active_profile_id == Some(record.target.account_id)
        && record
            .target
            .settings
            .equivalent(&ScenarioSettings::from_network(&config.network))
        && config.scenario_selection == record.target.selection
}

/// Mark a previously acknowledged commit without replaying its target. Call
/// this in a separate ConfigStore::update only after the first update succeeds.
/// A stale operation cannot acknowledge the newer selection's transaction.
pub fn acknowledge_operation(config: &mut AppConfig, operation_id: Uuid) -> bool {
    let Some(record) = config
        .last_scenario_switch
        .as_mut()
        .filter(|record| record.operation_id == operation_id)
    else {
        return false;
    };
    if record.acknowledged {
        return false;
    }
    record.acknowledged = true;
    true
}

fn target_for(
    config: &AppConfig,
    settings: &ScenarioSettings,
    account_id: Uuid,
) -> Result<Profile, ScenarioError> {
    let account = config
        .account(account_id)
        .ok_or(ScenarioError::AccountUnavailable)?;
    let mut network = config.network.clone();
    settings.apply_to(&mut network)?;
    let target = network.hydrate(account);
    target
        .validate()
        .map_err(|_| ScenarioError::InvalidSettings)?;
    Ok(target)
}

fn target_snapshot(
    config: &AppConfig,
    id: Uuid,
    revision: Uuid,
) -> Result<ScenarioSnapshot, ScenarioError> {
    let scenario = scenario(config, id, revision)?;
    let account_id = scenario
        .bound_account_id
        .or(config.active_profile_id)
        .ok_or(ScenarioError::AccountUnavailable)?;
    Ok(ScenarioSnapshot {
        account_id,
        settings: scenario.settings.clone(),
        selection: Some(Box::new(scenario.into())),
    })
}

fn restore_snapshot(config: &AppConfig) -> Result<ScenarioSnapshot, ScenarioError> {
    if !can_restore(config) {
        return Err(ScenarioError::RestoreUnavailable);
    }
    let mut target = config
        .last_scenario_switch
        .as_ref()
        .ok_or(ScenarioError::RestoreUnavailable)?
        .source
        .clone();
    if target
        .selection
        .as_ref()
        .is_some_and(|s| !config.scenarios.iter().any(|v| v.id == s.scenario_id))
    {
        target.selection = None;
    }
    Ok(target)
}

/// Resolve a candidate before platform dependency checks build their preview
/// fingerprint. This has no storage or runtime side effects.
pub fn candidate_profile(
    config: &AppConfig,
    request: &ScenarioCommand,
) -> Result<Option<Profile>, ScenarioError> {
    let target = match request {
        ScenarioCommand::Preview {
            id,
            expected_revision,
        }
        | ScenarioCommand::Activate {
            id,
            expected_revision,
            ..
        } => target_snapshot(config, *id, *expected_revision)?,
        ScenarioCommand::PreviewRestore | ScenarioCommand::Restore { .. } => {
            restore_snapshot(config)?
        }
        _ => return Ok(None),
    };
    target_for(config, &target.settings, target.account_id).map(Some)
}

/// Keep session-owned, next-connection choices honest during a same-account
/// scene switch. Persisted globals are left untouched; changing identity starts
/// a fresh session and therefore uses the complete stored target.
pub fn runtime_target(stored: &Profile, applied: &Profile) -> Profile {
    let mut target = stored.clone();
    if stored.id == applied.id {
        // A catalogue rename does not reconfigure the network transport.
        target.name.clone_from(&applied.name);
        target.auto_connect = applied.auto_connect;
        target.congestion_control = applied.congestion_control;
    }
    target
}

fn digest(value: &impl Serialize) -> Result<String, ScenarioError> {
    let bytes = serde_json::to_vec(value).map_err(|_| ScenarioError::InvalidRequest)?;
    Ok(Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

fn preview_token(
    config: &AppConfig,
    target: &ScenarioSnapshot,
    context: &ScenarioContext,
    restore: bool,
) -> Result<String, ScenarioError> {
    if context.source_epoch.is_nil() {
        return Err(ScenarioError::PreviewStale);
    }
    // Profile's credential bytes are serde(skip); this digest is never a log
    // payload. Bind inherited global settings and actual owner observations.
    digest(&json!({
        "network":config.network,"selected_account":config.active_profile_id,
        "target_account":config.account(target.account_id),"target":target,
        "selection":config.scenario_selection,"last_operation":config.last_scenario_switch.as_ref().map(|r|r.operation_id),
        "source_epoch":context.source_epoch,"session_id":context.session_id,
        "applied":context.applied_profile,"apply_status":context.apply_status,
        "dependencies":context.dependency_fingerprint,"restore":restore
    }))
}

/// Public field paths needed to apply the complete target, including fields
/// equal to the stored value but still different in the running session.
pub fn managed_changed_fields(
    config: &AppConfig,
    target: &Profile,
    context: &ScenarioContext,
) -> Vec<String> {
    const FIELDS: &[&str] = &[
        "frontends.tunnel",
        "frontends.socks5",
        "frontends.http",
        "transport",
        "ip_policy",
        "allow_lan",
        "routing",
        "geo_direct_countries",
        "direct_dns",
        "warp_dns",
        "dns_mode",
        "dns_servers",
        "proxy.system_proxy",
        "proxy.dns_mode",
        "proxy.dns_servers",
        "vpn_gate",
        "chain_exit",
    ];
    let mut changed = config
        .active_profile()
        .map(|p| crate::network_settings::changed_fields(&p, target))
        .unwrap_or_default();
    if let Some(applied) = &context.applied_profile {
        changed.extend(crate::network_settings::changed_fields(applied, target));
    }
    FIELDS
        .iter()
        .filter(|field| changed.iter().any(|f| f == **field))
        .map(|s| (*s).to_owned())
        .collect()
}

/// Identify a confirmed scene only through the selected version or the source
/// of the current transition. Equal settings in an unrelated template are not
/// evidence that the user selected that template for the running connection.
fn applied_scenario_id(config: &AppConfig, applied: &Profile) -> Option<Uuid> {
    let applied_settings = ScenarioSettings::from_profile(applied);
    let matches = |selected: &ScenarioSelection, account_id: Uuid| {
        applied.id == account_id
            && selected.bound_account_id.is_none_or(|id| id == applied.id)
            && selected.settings.equivalent(&applied_settings)
            && config
                .scenarios
                .iter()
                .any(|scene| scene.id == selected.scenario_id)
    };
    if let Some(selected) = config.scenario_selection.as_deref()
        && config.active_profile_id == Some(applied.id)
        && matches(selected, applied.id)
    {
        return Some(selected.scenario_id);
    }
    let record = config.last_scenario_switch.as_ref()?;
    // A restore can itself still be applying. Use correspondence with the
    // current target, not undo eligibility, to validate the transition source.
    if config.active_profile_id != Some(record.target.account_id)
        || config.scenario_selection != record.target.selection
        || !record
            .target
            .settings
            .equivalent(&ScenarioSettings::from_network(&config.network))
    {
        return None;
    }
    let source = record.source.selection.as_deref()?;
    matches(source, record.source.account_id).then_some(source.scenario_id)
}

pub fn catalogue(config: &AppConfig, context: &ScenarioContext) -> Value {
    let current = ScenarioSettings::from_network(&config.network);
    let current_fits = encoded_size(&current).is_ok_and(|size| size <= MAX_SCENARIO_BYTES);
    let selection = config.scenario_selection.as_ref();
    let selection_metadata = selection.map(|selected| {
        json!({
            "scenario_id":selected.scenario_id,"scenario_revision":selected.scenario_revision,
            "bound_account_id":selected.bound_account_id
        })
    });
    let modified = selection.is_some_and(|selected| {
        !selected.settings.equivalent(&current)
            || selected
                .bound_account_id
                .is_some_and(|id| Some(id) != config.active_profile_id)
    });
    let template_updated = selection.is_some_and(|selected| {
        config.scenarios.iter().any(|s| {
            s.id == selected.scenario_id
                && (s.bound_account_id != selected.bound_account_id
                    || !s.settings.equivalent(&selected.settings))
        })
    });
    let bound_account_missing = selection.is_some_and(|selected| {
        selected
            .bound_account_id
            .is_some_and(|id| config.account(id).is_none())
    });
    let applied_matches = context.applied_profile.as_ref().map(|applied| {
        Some(applied.id) == config.active_profile_id
            && current.equivalent(&ScenarioSettings::from_profile(applied))
    });
    // Unmanaged next-session preferences can remain deferred even though the
    // entire scene is confirmed. Never turn a failed/unknown outcome into a pass.
    let apply_status =
        if context.apply_status == ApplyStatus::Deferred && applied_matches == Some(true) {
            ApplyStatus::Applied
        } else {
            context.apply_status
        };
    json!({
        "scenarios":config.scenarios,"selection":selection_metadata,
        "current_settings":current_fits.then_some(&current),
        "current_settings_error":(!current_fits).then_some("SCENARIO_LIMIT"),
        "current_account_id":config.active_profile_id,
        "applied_account_id":context.applied_profile.as_ref().map(|p|p.id),
        "applied_scenario_id":context.applied_profile.as_ref().and_then(|profile|applied_scenario_id(config,profile)),
        "applied_scene_confirmed":context.applied_profile.is_some(),
        "source_epoch":context.source_epoch,"session_id":context.session_id,
        "state":{"modified":modified,"template_updated":template_updated,"bound_account_missing":bound_account_missing,
            "applied_matches":applied_matches,"apply_status":apply_status},
        "can_restore":can_restore(config),
        "last_operation_id":config.last_scenario_switch.as_ref().map(|r|r.operation_id)
    })
}

/// Call mutations inside ConfigStore::update. The caller validates the returned
/// candidate's platform resources before allowing the store transaction to
/// commit, and adds persisted=true only after update() acknowledges durability.
pub fn command(
    config: &mut AppConfig,
    request: ScenarioCommand,
    context: &ScenarioContext,
) -> Result<ScenarioCommandResult, ScenarioError> {
    let mut next = config.clone();
    let mut changed = false;
    let mut target_profile = None;
    let mut preview_target = None;
    let mut preview = None;
    let mut saved_scenario_id = None;
    match request {
        ScenarioCommand::List => {}
        ScenarioCommand::Save {
            id,
            name,
            bound_account_id,
            settings,
            expected_revision,
        } => {
            let name = name.trim().to_owned();
            validate_name(&name)?;
            if bound_account_id.is_some_and(|id| !valid_id(id)) {
                return Err(ScenarioError::InvalidRequest);
            }
            let settings = settings.normalized()?;
            let id = match id {
                Some(id) => {
                    scenario(
                        &next,
                        id,
                        expected_revision.ok_or(ScenarioError::StaleRevision)?,
                    )?;
                    id
                }
                None => {
                    if expected_revision.is_some() {
                        return Err(ScenarioError::InvalidRequest);
                    }
                    Uuid::new_v4()
                }
            };
            let value = ConnectionScenario {
                id,
                name,
                revision: Uuid::new_v4(),
                bound_account_id,
                settings,
            };
            saved_scenario_id = Some(id);
            if let Some(existing) = next.scenarios.iter_mut().find(|s| s.id == id) {
                *existing = value;
            } else {
                next.scenarios.push(value);
            }
            changed = true;
        }
        ScenarioCommand::Delete {
            id,
            expected_revision,
        } => {
            scenario(&next, id, expected_revision)?;
            next.scenarios.retain(|s| s.id != id);
            if next
                .scenario_selection
                .as_ref()
                .is_some_and(|s| s.scenario_id == id)
            {
                next.scenario_selection = None;
            }
            changed = true;
        }
        ScenarioCommand::Preview {
            id,
            expected_revision,
        } => {
            // A successful preview must also be able to retain a bounded undo.
            snapshot(&next)?;
            let target = target_snapshot(&next, id, expected_revision)?;
            let profile = target_for(&next, &target.settings, target.account_id)?;
            preview = Some(preview_value(&next, &target, &profile, context, false)?);
            preview_target = Some(profile);
        }
        ScenarioCommand::PreviewRestore => {
            snapshot(&next)?;
            let target = restore_snapshot(&next)?;
            let profile = target_for(&next, &target.settings, target.account_id)?;
            preview = Some(preview_value(&next, &target, &profile, context, true)?);
            preview_target = Some(profile);
        }
        ScenarioCommand::Activate {
            id,
            expected_revision,
            preview_token: token,
            operation_id,
        } => {
            let target = target_snapshot(&next, id, expected_revision)?;
            target_profile = Some(commit_target(
                &mut next,
                target,
                context,
                &token,
                operation_id,
                false,
            )?);
            changed = true;
        }
        ScenarioCommand::Restore {
            preview_token: token,
            operation_id,
        } => {
            let target = restore_snapshot(&next)?;
            target_profile = Some(commit_target(
                &mut next,
                target,
                context,
                &token,
                operation_id,
                true,
            )?);
            changed = true;
        }
    }
    validate_config(&next)?;
    let mut response = catalogue(&next, context);
    if let Some(id) = saved_scenario_id {
        response["saved_scenario_id"] = json!(id);
    }
    if let Some(value) = preview {
        // Preview carries its own bounded before/target/applied projections;
        // do not repeat the potentially large template catalogue over Binder.
        response
            .as_object_mut()
            .expect("catalogue object")
            .remove("scenarios");
        response["preview"] = value;
    }
    if encoded_size(&response)? > MAX_RESPONSE_BYTES {
        return Err(ScenarioError::Limit);
    }
    if changed {
        *config = next;
    }
    Ok(ScenarioCommandResult {
        response,
        target: target_profile,
        preview_target,
        changed,
    })
}

fn preview_value(
    config: &AppConfig,
    target: &ScenarioSnapshot,
    profile: &Profile,
    context: &ScenarioContext,
    restore: bool,
) -> Result<Value, ScenarioError> {
    let runtime_target = context
        .applied_profile
        .as_ref()
        .map(|applied| runtime_target(profile, applied));
    let profile = runtime_target.as_ref().unwrap_or(profile);
    let before_settings = ScenarioSettings::from_network(&config.network);
    let before_fits = encoded_size(&before_settings)? <= MAX_SCENARIO_BYTES;
    let applied_settings = context
        .applied_profile
        .as_ref()
        .map(ScenarioSettings::from_profile);
    let applied_fits = applied_settings
        .as_ref()
        .is_none_or(|settings| encoded_size(settings).is_ok_and(|size| size <= MAX_SCENARIO_BYTES));
    let actual_changed_fields = context
        .applied_profile
        .as_ref()
        .map(|previous| crate::network_settings::changed_fields(previous, profile))
        .unwrap_or_default();
    let (apply_kind, needs_reconnect) = match &context.applied_profile {
        Some(previous) if previous.id != profile.id => ("cold_reconnect", Some(true)),
        Some(previous) => {
            match crate::reconfigure::platform_class(crate::classify_reconfigure(previous, profile))
            {
                crate::ReconfigureClass::PersistOnly => ("not_required", Some(false)),
                crate::ReconfigureClass::HotTrafficPolicy => ("hot_traffic_policy", Some(false)),
                crate::ReconfigureClass::ColdReconnect | crate::ReconfigureClass::Reject => {
                    ("cold_reconnect", Some(true))
                }
                crate::ReconfigureClass::HotSystemProxy => ("hot_system_proxy", Some(false)),
                crate::ReconfigureClass::HotFrontends => ("hot_frontends", Some(false)),
                crate::ReconfigureClass::HotTunnelAttach => ("hot_tunnel_attach", Some(false)),
                crate::ReconfigureClass::HotVpnGate => ("hot_chain_exit", Some(false)),
            }
        }
        None if context.session_id.is_none() => ("select", Some(false)),
        None => ("unconfirmed", None),
    };
    Ok(json!({
        "scenario_id":target.selection.as_ref().map(|s|s.scenario_id),
        "scenario_revision":target.selection.as_ref().map(|s|s.scenario_revision),
        "account_id":target.account_id,"previous_account_id":config.active_profile_id,
        "account_changed":config.active_profile_id!=Some(target.account_id),
        "changed_fields":managed_changed_fields(config,profile,context),
        "actual_changed_fields":actual_changed_fields,"apply_kind":apply_kind,"needs_reconnect":needs_reconnect,
        "preview_token":preview_token(config,target,context,restore)?,"restore":restore,
        "settings":target.settings,
        "before_settings":before_fits.then_some(before_settings),
        "before_settings_omitted":!before_fits,
        "target_settings":target.settings,
        "applied_settings":if applied_fits{applied_settings}else{None},
        "applied_settings_omitted":!applied_fits
    }))
}

fn commit_target(
    config: &mut AppConfig,
    target: ScenarioSnapshot,
    context: &ScenarioContext,
    token: &str,
    operation_id: Uuid,
    restore: bool,
) -> Result<Profile, ScenarioError> {
    if !valid_id(operation_id) {
        return Err(ScenarioError::InvalidRequest);
    }
    if config
        .last_scenario_switch
        .as_ref()
        .is_some_and(|r| r.operation_id == operation_id)
    {
        return Err(ScenarioError::OperationReused);
    }
    if token.len() != 64 || token != preview_token(config, &target, context, restore)? {
        return Err(ScenarioError::PreviewStale);
    }
    let profile = target_for(config, &target.settings, target.account_id)?;
    let source = snapshot(config)?;
    target.settings.apply_to(&mut config.network)?;
    config.active_profile_id = Some(target.account_id);
    config.scenario_selection = target.selection.clone();
    config.last_scenario_switch = Some(Box::new(ScenarioSwitchRecord {
        operation_id,
        source,
        target,
        acknowledged: false,
        restored: restore,
    }));
    Ok(profile)
}

fn retained_settings(config: &AppConfig) -> Vec<&ScenarioSettings> {
    let mut values: Vec<_> = config.scenarios.iter().map(|s| &s.settings).collect();
    if let Some(selection) = &config.scenario_selection {
        values.push(&selection.settings);
    }
    if let Some(record) = &config.last_scenario_switch
        && can_restore(config)
    {
        values.extend([&record.source.settings, &record.target.settings]);
        for selection in [
            record.source.selection.as_ref(),
            record.target.selection.as_ref(),
        ]
        .into_iter()
        .flatten()
        {
            values.push(&selection.settings);
        }
    }
    values
}

pub fn retained_chain_profiles(config: &AppConfig) -> Vec<Uuid> {
    let mut ids: Vec<_> = config
        .network
        .chain_exit
        .as_ref()
        .and_then(|s| s.profile_id)
        .into_iter()
        .collect();
    ids.extend(
        retained_settings(config)
            .into_iter()
            .filter_map(|s| s.chain_exit.as_ref().and_then(|c| c.profile_id)),
    );
    ids.sort();
    ids.dedup();
    ids
}

pub fn retained_vpn_gate_selections(config: &AppConfig) -> Vec<Selection> {
    let mut selections: Vec<_> = config
        .network
        .vpn_gate
        .selection
        .clone()
        .into_iter()
        .collect();
    selections.extend(
        retained_settings(config)
            .into_iter()
            .filter_map(|s| s.vpn_gate.selection.clone()),
    );
    selections
        .sort_by(|a, b| (&a.server_id, &a.config_sha256).cmp(&(&b.server_id, &b.config_sha256)));
    selections.dedup();
    selections
}

#[cfg(test)]
mod tests;
