//! Scenario commands run on the VPN service's serialized settings executor.
//! Only its trusted envelope supplies runtime observations and permission state.
use std::path::Path;
use std::sync::Mutex;
use std::time::UNIX_EPOCH;

use serde::Deserialize;
use serde_json::{Value, json};
use usque_core::{
    Profile,
    network_settings::ApplyStatus,
    scenarios::{self, ScenarioCommand, ScenarioContext},
    storage::{ConfigStore, StoreError},
};
use uuid::Uuid;

static UNCONFIRMED_OPERATION: Mutex<Option<Uuid>> = Mutex::new(None);

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    request: Value,
    phase: String,
    available: bool,
    session_id: String,
    vpn_authorized: bool,
    #[serde(default)]
    identity_account_id: Option<Uuid>,
}

pub(crate) fn command(path: &str, request: &str) -> Result<String, String> {
    let path = Path::new(path);
    if path.as_os_str().len() > 4096
        || !path.is_absolute()
        || path.file_name().and_then(|name| name.to_str()) != Some("profiles-v2.json")
        || request.len() > 2 * 1024 * 1024
    {
        return Err("SCENARIO_INVALID_REQUEST".into());
    }
    let envelope: Envelope =
        serde_json::from_str(request).map_err(|_| "SCENARIO_INVALID_REQUEST")?;
    if envelope.session_id.len() > 64 || envelope.phase.len() > 32 {
        return Err("SCENARIO_INVALID_REQUEST".into());
    }
    let request =
        ScenarioCommand::parse(&envelope.request.to_string()).map_err(|error| error.to_string())?;
    let operation = match &request {
        ScenarioCommand::Activate { operation_id, .. }
        | ScenarioCommand::Restore { operation_id, .. } => Some(*operation_id),
        _ => None,
    };
    #[cfg(feature = "wireguard")]
    if operation.is_some() && (!crate::warp_wireguard::stop() || !crate::proton_vpn::stop()) {
        return Err("SCENARIO_BUSY".into());
    }
    if operation.is_some() && !crate::vpngate::cancel_refresh() {
        return Err("SCENARIO_BUSY".into());
    }
    let parent = path.parent().ok_or("SCENARIO_INVALID_REQUEST")?;
    let _provider_lease = operation
        .map(|_| usque_core::proton_vpn::lifecycle_lease(parent).map_err(|_| "SCENARIO_BUSY"))
        .transpose()?;
    let state = crate::network_settings::scenario_snapshot()?;
    let mut confirmed_operations = state.persisted_operation_ids.clone();
    let stable = envelope.available
        && matches!(envelope.phase.as_str(), "connected" | "degraded")
        && state.session_id.as_deref() == Some(envelope.session_id.as_str());
    if operation.is_some() && envelope.available && (!stable || state.applied_profile.is_none()) {
        return Err("SCENARIO_UNCONFIRMED".into());
    }
    let mut context = ScenarioContext {
        source_epoch: state.source_epoch,
        session_id: (!matches!(envelope.phase.as_str(), "disconnected" | "idle"))
            .then(|| envelope.session_id.clone()),
        applied_profile: stable.then_some(state.applied_profile).flatten(),
        apply_status: if stable
            || state.operation_id.is_some()
                && matches!(
                    state.apply_status,
                    ApplyStatus::Unknown | ApplyStatus::Failed | ApplyStatus::Applying
                ) {
            state.apply_status
        } else {
            ApplyStatus::Deferred
        },
        dependency_fingerprint: String::new(),
    };
    let store = ConfigStore::new(path);
    // Keep config -> provider/library lock ordering. No network requests occur
    // while preparing or committing these local metadata operations.
    let _guard = store.lock_exclusive().map_err(|_| "SCENARIO_UNCONFIRMED")?;
    let mut config = store
        .load_or_default()
        .map_err(|_| "SCENARIO_UNCONFIRMED")?;
    let candidate =
        scenarios::candidate_profile(&config, &request).map_err(|error| error.to_string())?;
    if let Some(target) = candidate.as_ref() {
        context.dependency_fingerprint = validate_dependencies(parent, target)?;
        if operation.is_some() {
            if envelope.identity_account_id != Some(target.id) {
                return Err("SCENARIO_ACCOUNT_UNAVAILABLE".into());
            }
            if stable && target.frontends.tunnel && !envelope.vpn_authorized {
                return Err("SCENARIO_VPN_PERMISSION_REQUIRED".into());
            }
        }
    }
    let mut result = scenarios::command(&mut config, request.clone(), &context)
        .map_err(|error| error.to_string())?;
    if result.changed {
        // Pin saved scenario references before the config commit. Any leftover
        // pin after a failed save is reclaimed by normal reference recovery.
        if let ScenarioCommand::Save { settings, .. } = &request
            && let Some(selection) = &settings.vpn_gate.selection
        {
            usque_core::vpngate::CatalogueStore::new(parent)
                .pin(selection)
                .map_err(|_| "SCENARIO_DEPENDENCY_UNAVAILABLE")?;
        }
        if let Err(error) = store.save(&config) {
            if matches!(error, StoreError::CommitUncertain(_)) {
                *UNCONFIRMED_OPERATION
                    .lock()
                    .map_err(|_| "SCENARIO_UNCONFIRMED")? = operation;
                if let Some(id) = operation {
                    crate::network_settings::record_scenario_uncertain(&config, id)?;
                }
                return Err("SCENARIO_COMMIT_UNCONFIRMED".into());
            }
            return Err("SCENARIO_SAVE_FAILED".into());
        }
        result.response["persisted"] = json!(true);
        if stable {
            let mut retained = scenarios::retained_vpn_gate_selections(&config);
            if let Some(selection) = context
                .applied_profile
                .as_ref()
                .and_then(|profile| profile.vpn_gate.selection.clone())
            {
                retained.push(selection);
            }
            // Unconfirmed/recovering owners may still hold an older exit.
            // Defer their collection to the owner's reference recovery.
            let _ = usque_core::vpngate::CatalogueStore::new(parent).retain_selections(&retained);
        }
    }
    let runtime_target = result.target.as_ref().map(|stored| {
        context.applied_profile.as_ref().map_or_else(
            || stored.clone(),
            |applied| scenarios::runtime_target(stored, applied),
        )
    });
    let requires_runtime = stable
        && runtime_target.as_ref().is_some_and(|target| {
            context.applied_profile.as_ref().is_none_or(|applied| {
                applied.id != target.id
                    || usque_core::classify_reconfigure(applied, target)
                        != usque_core::ReconfigureClass::PersistOnly
            })
        });
    if let Some(operation_id) = operation {
        *UNCONFIRMED_OPERATION
            .lock()
            .map_err(|_| "SCENARIO_UNCONFIRMED")? = None;
        let apply_status = if requires_runtime {
            ApplyStatus::Applying
        } else if stable {
            ApplyStatus::Applied
        } else {
            ApplyStatus::Deferred
        };
        let confirmed =
            crate::network_settings::record_scenario_commit(&config, operation_id, apply_status)?;
        confirmed_operations.clone_from(&confirmed.persisted_operation_ids);
        // This marker is written only after the target commit returned success.
        // Failure to persist it cannot erase that process-local acknowledgement;
        // it merely keeps a future process conservative about the old operation.
        if scenarios::acknowledge_operation(&mut config, operation_id) {
            let _ = store.save(&config);
        }
        context.apply_status = confirmed.apply_status;
        result.response = scenarios::catalogue(&config, &context);
        result.response["persisted"] = json!(true);
        result.response["network_settings"] =
            crate::network_settings::encode_scenario_state(&confirmed)?;
    }
    let uncertain = *UNCONFIRMED_OPERATION
        .lock()
        .map_err(|_| "SCENARIO_UNCONFIRMED")?;
    if let Some(record) = config.last_scenario_switch.as_ref().filter(|record| {
        Some(record.operation_id) != uncertain
            && (record.acknowledged || confirmed_operations.contains(&record.operation_id))
    }) {
        result.response["persisted_operation_id"] = json!(record.operation_id);
    }
    result.response["target"] = runtime_target
        .as_ref()
        .filter(|_| requires_runtime)
        .map(|profile| crate::android_profile_value(profile, None, false))
        .unwrap_or(Value::Null);
    result.response["preview_target"] = result
        .preview_target
        .as_ref()
        .map(|profile| crate::android_profile_value(profile, None, false))
        .unwrap_or(Value::Null);
    serde_json::to_string(&result.response).map_err(|_| "SCENARIO_UNCONFIRMED".into())
}

fn validate_dependencies(parent: &Path, profile: &Profile) -> Result<String, String> {
    profile
        .validate()
        .map_err(|_| "SCENARIO_INVALID_SETTINGS")?;
    if profile.proxy.system_proxy {
        return Err("SCENARIO_PLATFORM_UNSUPPORTED".into());
    }
    profile
        .validate_geo_cache(parent)
        .map_err(|_| "SCENARIO_RULES_UNAVAILABLE")?;
    let mut revisions = Vec::new();
    for code in &profile.geo_direct_countries {
        let country =
            usque_geo::CountryCode::parse(code).map_err(|_| "SCENARIO_INVALID_SETTINGS")?;
        for path in [
            usque_geo::geoip_cache_path(parent, &country),
            usque_geo::geosite_cache_path(parent, &country),
        ] {
            revisions.push(file_revision(&path));
        }
    }
    if profile.routing.ads_enabled {
        let rules = usque_geo::AdsRules::load(parent).map_err(|_| "SCENARIO_RULES_UNAVAILABLE")?;
        revisions.push(rules.revision().to_owned());
    }
    if !profile.geo_direct_countries.is_empty() || profile.routing.ads_enabled {
        revisions.push(file_revision(&usque_geo::global_geosite_cache_path(parent)));
    }
    if profile.chain_enabled() {
        // This prepares local, encrypted records only; no session is started.
        crate::chain_exit::prepare(parent, profile)
            .map_err(|_| "SCENARIO_DEPENDENCY_UNAVAILABLE")?;
        if profile.custom_chain().is_some() {
            revisions.push(crate::chain_exit::scenario_dependency_revision(
                parent,
                profile.custom_chain().unwrap(),
            )?);
        } else if let Some(selection) = &profile.vpn_gate.selection {
            revisions.push(selection.config_sha256.clone());
        }
    }
    serde_json::to_string(&revisions).map_err(|_| "SCENARIO_UNCONFIRMED".into())
}

fn file_revision(path: &Path) -> String {
    std::fs::metadata(path)
        .ok()
        .map(|metadata| {
            let modified = metadata
                .modified()
                .ok()
                .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
                .map(|value| value.as_nanos());
            format!("{}:{modified:?}", metadata.len())
        })
        .unwrap_or_else(|| "absent".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use usque_core::AppConfig;

    #[test]
    fn disconnected_scenario_selection_commits_atomically_without_a_runtime_target() {
        let _guard = crate::network_settings::TEST_STATE.lock().unwrap();
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("profiles-v2.json");
        let store = ConfigStore::new(&path);
        let config = AppConfig::default();
        store.save(&config).unwrap();
        crate::network_settings::command(path.to_str().unwrap(), r#"{"command":"reset"}"#).unwrap();
        let invoke = |request: Value, identity: Option<Uuid>| -> Result<Value, String> {
            let source = command(path.to_str().unwrap(), &json!({"request": request, "phase":"disconnected","available":false,"session_id":"1","vpn_authorized":false,"identity_account_id":identity}).to_string())?;
            serde_json::from_str(&source).map_err(|error| error.to_string())
        };
        let saved = invoke(json!({"action":"save","name":"Work","settings":scenarios::ScenarioSettings::default()}), None).unwrap();
        let scenario = &saved["scenarios"][0];
        assert_eq!(saved["saved_scenario_id"], scenario["id"]);
        let preview = invoke(json!({"action":"preview","id":scenario["id"],"expected_revision":scenario["revision"]}), None).unwrap();
        assert_eq!(preview["preview"]["apply_kind"], "select");
        assert_eq!(preview["preview"]["needs_reconnect"], false);
        for phase in ["idle", "error", "reconnecting"] {
            let response: Value = serde_json::from_str(&command(path.to_str().unwrap(), &json!({
                "request":{"action":"preview","id":scenario["id"],"expected_revision":scenario["revision"]},
                "phase":phase,"available":false,"session_id":"1","vpn_authorized":false
            }).to_string()).unwrap()).unwrap();
            assert_eq!(
                response["preview"]["apply_kind"],
                if phase == "idle" {
                    "select"
                } else {
                    "unconfirmed"
                }
            );
            assert_eq!(
                response["preview"]["needs_reconnect"],
                if phase == "idle" {
                    json!(false)
                } else {
                    Value::Null
                }
            );
        }
        let operation = Uuid::new_v4();
        let activation = json!({"action":"activate","id":scenario["id"],"expected_revision":scenario["revision"],"preview_token":preview["preview"]["preview_token"],"operation_id":operation});
        assert_eq!(
            invoke(activation.clone(), None).unwrap_err(),
            "SCENARIO_ACCOUNT_UNAVAILABLE"
        );
        assert!(store.load().unwrap().scenario_selection.is_none());
        let applied = invoke(activation.clone(), config.active_profile_id).unwrap();
        assert!(applied["target"].is_null());
        assert_eq!(applied["persisted"], true);
        assert_eq!(applied["state"]["apply_status"], "deferred");
        assert_eq!(
            store
                .load()
                .unwrap()
                .last_scenario_switch
                .unwrap()
                .operation_id,
            operation
        );
        assert!(invoke(activation, config.active_profile_id).is_err());

        // A process restart loses in-memory receipts. Matching saved settings
        // without a durable success marker must not confirm the old command.
        let mut stored = store.load().unwrap();
        stored.last_scenario_switch.as_mut().unwrap().acknowledged = false;
        store.save(&stored).unwrap();
        crate::network_settings::command(path.to_str().unwrap(), r#"{"command":"reset"}"#).unwrap();
        let unknown = invoke(json!({"action":"list"}), None).unwrap();
        assert!(unknown.get("persisted_operation_id").is_none());
        assert!(scenarios::acknowledge_operation(&mut stored, operation));
        store.save(&stored).unwrap();
        let acknowledged = invoke(json!({"action":"list"}), None).unwrap();
        assert_eq!(
            acknowledged["persisted_operation_id"],
            operation.to_string()
        );
    }

    #[test]
    fn gui_cannot_supply_runtime_or_secrets_in_the_scenario_command() {
        let path = std::env::temp_dir().join("profiles-v2.json");
        let envelope = json!({"request":{"action":"list","applied_profile":{}},"phase":"disconnected","available":false,"session_id":"1","vpn_authorized":false});
        assert_eq!(
            command(path.to_str().unwrap(), &envelope.to_string()).unwrap_err(),
            "SCENARIO_INVALID_REQUEST"
        );
    }
}
