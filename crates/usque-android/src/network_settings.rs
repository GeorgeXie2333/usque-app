//! Settings control is independent of the JNI runtime-start mutex.
use std::sync::{Mutex, OnceLock};

use serde::Deserialize;
use serde_json::{Value, json};
use usque_core::{
    ConnectionPhase, ReconfigureClass,
    network_settings::{
        ApplyStatus, NetworkSettingsPatch, NetworkSettingsState, merge_patch, plan_application,
    },
    storage::{ConfigStore, StoreError},
};
use uuid::Uuid;

use crate::{AndroidProfile, android_profile_to_core, android_profile_value};

static STATE: OnceLock<Mutex<NetworkSettingsState>> = OnceLock::new();

#[cfg(test)]
pub(crate) static TEST_STATE: Mutex<()> = Mutex::new(());

pub(crate) fn scenario_snapshot() -> Result<NetworkSettingsState, String> {
    STATE
        .get_or_init(|| Mutex::new(NetworkSettingsState::default()))
        .lock()
        .map(|state| state.clone())
        .map_err(|_| "SCENARIO_UNCONFIRMED".into())
}

pub(crate) fn record_scenario_commit(
    config: &usque_core::AppConfig,
    operation_id: Uuid,
    apply_status: ApplyStatus,
) -> Result<NetworkSettingsState, String> {
    let mut state = STATE
        .get_or_init(|| Mutex::new(NetworkSettingsState::default()))
        .lock()
        .map_err(|_| "SCENARIO_UNCONFIRMED")?;
    state.operation_id = Some(operation_id);
    state.persisted = Some(true);
    state.record_persisted_operation(operation_id);
    state.stored_profile = config.active_profile();
    state.shared_network_profile = Some(
        config
            .network
            .hydrate(&usque_core::Account::default_account()),
    );
    state.apply_status = apply_status;
    state.error_code = None;
    state.advance();
    Ok(state.clone())
}

pub(crate) fn encode_scenario_state(state: &NetworkSettingsState) -> Result<Value, String> {
    serde_json::from_str(&encode(state, None)?).map_err(|_| "SCENARIO_UNCONFIRMED".into())
}

pub(crate) fn record_scenario_uncertain(
    config: &usque_core::AppConfig,
    operation_id: Uuid,
) -> Result<(), String> {
    let mut state = STATE
        .get_or_init(|| Mutex::new(NetworkSettingsState::default()))
        .lock()
        .map_err(|_| "SCENARIO_UNCONFIRMED")?;
    state.operation_id = Some(operation_id);
    state.persisted = None;
    state.stored_profile = config.active_profile();
    state.apply_status = ApplyStatus::Unknown;
    state.error_code = Some("SCENARIO_COMMIT_UNCONFIRMED".into());
    state.advance();
    Ok(())
}

#[derive(Default, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
enum RuntimeFailureOutcome {
    #[default]
    Unconfirmed,
    Rejected,
}

#[derive(Deserialize)]
#[serde(tag = "command", rename_all = "snake_case")]
enum Command {
    Save {
        operation_id: Uuid,
        account_id: Uuid,
        values: AndroidProfile,
        changed_fields: Vec<String>,
        phase: String,
        available: bool,
        session_id: String,
    },
    Get,
    Reset,
    Observe {
        profile: Option<AndroidProfile>,
        session_id: String,
        applying: bool,
        #[serde(default)]
        unconfirmed: bool,
    },
    Failed {
        operation_id: Uuid,
        session_id: String,
        #[serde(default)]
        outcome: RuntimeFailureOutcome,
        #[serde(default)]
        confirmed_profile: Option<AndroidProfile>,
    },
}

pub(crate) fn command(path: &str, request: &str) -> Result<String, String> {
    let path = std::path::Path::new(path);
    if path.as_os_str().len() > 4096
        || request.len() > 2 * 1024 * 1024
        || !path.is_absolute()
        || path.file_name().and_then(|name| name.to_str()) != Some("profiles-v2.json")
    {
        return Err("invalid network settings request".into());
    }
    let command: Command =
        serde_json::from_str(request).map_err(|_| "invalid network settings message")?;
    #[cfg(feature = "wireguard")]
    if matches!(&command, Command::Save { .. } | Command::Reset)
        && (!crate::warp_wireguard::stop() || !crate::proton_vpn::stop())
    {
        return Err("WARP_GENERATION_CLEANUP_PENDING".into());
    }
    let _provider_lease = matches!(&command, Command::Save { .. } | Command::Reset)
        .then(|| {
            usque_core::proton_vpn::lifecycle_lease(
                path.parent().ok_or("invalid configuration path")?,
            )
            .map_err(|_| "PROTON_OPERATION_BUSY")
        })
        .transpose()?;
    let store = ConfigStore::new(path);
    let mut state = STATE
        .get_or_init(|| Mutex::new(NetworkSettingsState::default()))
        .lock()
        .map_err(|_| "network settings state is unavailable")?;
    let mut target = None;
    match command {
        Command::Save {
            operation_id,
            account_id,
            values,
            changed_fields,
            phase,
            available,
            session_id,
        } => {
            let patch = NetworkSettingsPatch {
                operation_id,
                account_id,
                values: android_profile_to_core(values)?,
                changed_fields,
            };
            let commit = store.update(|config| {
                if patch.values.custom_chain().is_none()
                    && patch.changed_fields.iter().any(|f| f == "vpn_gate")
                {
                    crate::vpngate::pin_settings(
                        path,
                        &patch.values.vpn_gate,
                        &config.network.vpn_gate,
                    )
                    .map_err(StoreError::NetworkSettings)?;
                }
                let profile = merge_patch(config, &patch)
                    .map_err(|error| StoreError::NetworkSettings(error.to_string()))?;
                if profile.chain_enabled() && profile.custom_chain().is_some() {
                    crate::chain_exit::prepare(
                        path.parent().ok_or_else(|| {
                            StoreError::NetworkSettings("Invalid storage path".into())
                        })?,
                        &profile,
                    )
                    .map_err(StoreError::NetworkSettings)?;
                }
                Ok(profile)
            });
            let stored = match commit {
                Ok((config, profile)) => {
                    state.shared_network_profile = Some(
                        config
                            .network
                            .hydrate(&usque_core::config::Account::default_account()),
                    );
                    profile
                }
                Err(StoreError::CommitUncertain(_)) => {
                    if let Ok(config) = store.load() {
                        state.shared_network_profile = Some(
                            config
                                .network
                                .hydrate(&usque_core::config::Account::default_account()),
                        );
                        state.stored_profile = config.active_profile();
                    }
                    state.operation_id = Some(operation_id);
                    state.persisted = None;
                    state.apply_status = ApplyStatus::Unknown;
                    state.error_code = Some("NETWORK_SETTINGS_COMMIT_UNCONFIRMED".into());
                    state.advance();
                    return encode(&state, None);
                }
                Err(error) => return Err(save_failure(error)),
            };
            let phase = match phase.as_str() {
                "connected" => ConnectionPhase::Connected,
                "degraded" => ConnectionPhase::Degraded,
                _ => ConnectionPhase::Reconnecting,
            };
            let same_session = state.session_id.as_deref() == Some(session_id.as_str());
            let plan = plan_application(
                state.applied_profile.as_ref(),
                &stored,
                &patch.changed_fields,
                phase,
                available && same_session,
            );
            state.operation_id = Some(operation_id);
            state.persisted = Some(true);
            state.record_persisted_operation(operation_id);
            state.stored_profile = Some(stored);
            state.error_code = None;
            match plan {
                Ok(plan) => {
                    state.apply_status = plan.status;
                    if plan.class != ReconfigureClass::Reject {
                        target = plan.target;
                    }
                }
                Err(_) => {
                    state.apply_status = ApplyStatus::Failed;
                    state.error_code = Some("NETWORK_SETTINGS_APPLY_INVALID".into());
                }
            }
        }
        Command::Reset => {
            *state = NetworkSettingsState::default();
        }
        Command::Get => {
            let _lock = store
                .lock_exclusive()
                .map_err(|_| "NETWORK_SETTINGS_UNCONFIRMED")?;
            let config = store
                .load_or_default()
                .map_err(|_| "NETWORK_SETTINGS_UNCONFIRMED")?;
            state.stored_profile = config.active_profile();
            state.shared_network_profile = Some(
                config
                    .network
                    .hydrate(&usque_core::config::Account::default_account()),
            );
        }
        Command::Observe {
            profile,
            session_id,
            applying,
            unconfirmed,
        } => {
            let profile = profile.map(android_profile_to_core).transpose()?;
            // Polling the unchanged session cannot turn a rejected application
            // into a success. A new save or changed session/profile retires it.
            let retain_rejection = !unconfirmed
                && !applying
                && profile.is_some()
                && profile == state.applied_profile
                && state.session_id.as_deref() == Some(session_id.as_str())
                && state.apply_status == ApplyStatus::Failed
                && state.error_code.as_deref() == Some("NETWORK_SETTINGS_APPLY_FAILED");
            state.applied_profile = profile;
            state.session_id = Some(session_id);
            state.apply_status = if unconfirmed {
                state.applied_profile = None;
                ApplyStatus::Unknown
            } else if applying {
                ApplyStatus::Applying
            } else if retain_rejection {
                ApplyStatus::Failed
            } else if state.applied_profile.is_some() {
                ApplyStatus::Applied
            } else {
                ApplyStatus::Deferred
            };
            if !retain_rejection {
                state.error_code =
                    unconfirmed.then(|| "NETWORK_SETTINGS_RECOVERY_UNCONFIRMED".into());
            }
        }
        Command::Failed {
            operation_id,
            session_id,
            outcome,
            confirmed_profile,
        } => {
            if state.session_id.as_deref() == Some(session_id.as_str())
                && (outcome != RuntimeFailureOutcome::Rejected
                    || state.operation_id == Some(operation_id))
            {
                let confirmed_profile =
                    confirmed_profile.and_then(|profile| android_profile_to_core(profile).ok());
                let retained = outcome == RuntimeFailureOutcome::Rejected
                    && confirmed_profile.is_some()
                    && confirmed_profile == state.applied_profile;
                if !retained {
                    state.applied_profile = None;
                }
                state.apply_status = if state.operation_id == Some(operation_id) {
                    ApplyStatus::Failed
                } else {
                    ApplyStatus::Unknown
                };
                state.error_code = Some("NETWORK_SETTINGS_APPLY_FAILED".into());
            }
        }
    }
    state.advance();
    encode(&state, target.as_ref())
}

fn save_failure(error: StoreError) -> String {
    if let StoreError::NetworkSettings(message) = error {
        let message = message
            .strip_prefix("network settings validation failed: ")
            .unwrap_or(&message);
        let parts = message.split(':').collect::<Vec<_>>();
        let count = match parts[0] {
            "ROUTING_RULE_CONFLICT" => Some(2),
            "ROUTING_RULE_INVALID" => Some(1),
            "ROUTING_RULE_LIMIT" | "ROUTING_UPGRADE_REQUIRED" => Some(0),
            _ => None,
        };
        if count.is_some_and(|count| parts.len() == count + 1)
            && parts[1..].iter().all(|id| Uuid::parse_str(id).is_ok())
        {
            return message.to_owned();
        }
    }
    "NETWORK_SETTINGS_SAVE_FAILED".into()
}

fn profile_value(profile: &usque_core::Profile) -> Value {
    android_profile_value(profile, None, false)
}

fn encode(
    state: &NetworkSettingsState,
    target: Option<&usque_core::Profile>,
) -> Result<String, String> {
    let mut result = serde_json::to_value(state).map_err(|_| "network settings encoding failed")?;
    result["shared_network_profile"] = state
        .shared_network_profile
        .as_ref()
        .map(profile_value)
        .unwrap_or(Value::Null);
    result["stored_profile"] = state
        .stored_profile
        .as_ref()
        .map(profile_value)
        .unwrap_or(Value::Null);
    result["applied_profile"] = state
        .applied_profile
        .as_ref()
        .map(profile_value)
        .unwrap_or(Value::Null);
    // This private target stays inside the VPN host, never in the GUI result.
    result["target"] = target.map(profile_value).unwrap_or(Value::Null);
    Ok(json!(result).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    // The JNI adapter owns one process-wide state even when fixtures use
    // separate stores. Keep each command sequence under one test reservation.

    #[test]
    fn issue_138_confirmed_rejection_retains_runtime_and_allows_an_explicit_retry() {
        let _test = TEST_STATE.lock().unwrap();
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("profiles-v2.json");
        let store = ConfigStore::new(&path);
        let config = usque_core::AppConfig::default();
        store.save(&config).unwrap();
        let profile = config.active_profile().unwrap();
        let request = |value: Value| -> Value {
            serde_json::from_str(&command(path.to_str().unwrap(), &value.to_string()).unwrap())
                .unwrap()
        };
        request(json!({"command":"reset"}));
        let observation = json!({
            "command":"observe", "profile":profile_value(&profile),
            "session_id":"138", "applying":false
        });
        request(observation.clone());

        // An unrelated deferred edit must not be pulled into the routing retry.
        let mut desired = profile.clone();
        desired.mtu = 1400;
        request(json!({
            "command":"save", "operation_id":Uuid::new_v4(), "account_id":profile.id,
            "values":profile_value(&desired), "changed_fields":["mtu"],
            "phase":"connected", "available":false, "session_id":"138"
        }));
        desired.routing.rules.push(usque_core::RoutingRule {
            id: Uuid::new_v4(),
            kind: usque_core::RoutingMatch::Domain,
            target: "direct.test".into(),
            action: usque_core::RoutingAction::Direct,
        });
        let operation = Uuid::new_v4();
        let saving = |operation| {
            json!({
                "command":"save", "operation_id":operation, "account_id":profile.id,
                "values":profile_value(&desired), "changed_fields":["routing"],
                "phase":"connected", "available":true, "session_id":"138"
            })
        };
        let saved = request(saving(operation));
        assert_eq!(saved["persisted"], true);
        assert_eq!(saved["apply_status"], "applying");
        assert_eq!(saved["target"]["mtu"], profile.mtu);

        // This is the existing failure callback path. The native rejection
        // retained the old runtime and the host restored its recovery record.
        let rejected = request(json!({
            "command":"failed", "operation_id":operation, "session_id":"138",
            "outcome":"rejected", "confirmed_profile":profile_value(&profile)
        }));
        assert_eq!(rejected["applied_profile"], profile_value(&profile));
        assert_eq!(rejected["apply_status"], "failed");
        assert_eq!(rejected["persisted"], true);
        assert_eq!(
            rejected["stored_profile"]["routing"],
            saved["stored_profile"]["routing"]
        );
        assert!(
            rejected["persisted_operation_ids"]
                .as_array()
                .unwrap()
                .contains(&json!(operation))
        );
        assert_eq!(store.load().unwrap().network.routing, desired.routing);

        let observed = request(observation);
        assert_eq!(observed["applied_profile"], profile_value(&profile));
        assert_eq!(observed["apply_status"], "failed");
        let retry = request(saving(Uuid::new_v4()));
        assert_eq!(retry["apply_status"], "applying");
        assert_eq!(retry["target"]["routing"], saved["target"]["routing"]);
        assert_eq!(retry["target"]["mtu"], profile.mtu);
        assert_eq!(retry["stored_profile"]["mtu"], 1400);
        assert!(retry["error_code"].is_null());
        let late = request(json!({
            "command":"failed", "operation_id":operation, "session_id":"138",
            "outcome":"rejected", "confirmed_profile":profile_value(&profile)
        }));
        assert_eq!(late["operation_id"], retry["operation_id"]);
        assert_eq!(late["apply_status"], "applying");
        assert_eq!(late["applied_profile"], profile_value(&profile));
        assert!(late["error_code"].is_null());
        request(json!({
            "command":"failed", "operation_id":retry["operation_id"], "session_id":"138",
            "outcome":"rejected", "confirmed_profile":profile_value(&profile)
        }));
        let replacement = request(json!({
            "command":"observe", "profile":profile_value(&profile),
            "session_id":"139", "applying":false
        }));
        assert_eq!(replacement["apply_status"], "deferred");
        assert!(replacement["error_code"].is_null());
    }

    #[test]
    fn issue_138_rejection_requires_matching_confirmation_and_session() {
        let _test = TEST_STATE.lock().unwrap();
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("profiles-v2.json");
        let store = ConfigStore::new(&path);
        let config = usque_core::AppConfig::default();
        store.save(&config).unwrap();
        let profile = config.active_profile().unwrap();
        let request = |value: Value| -> Value {
            serde_json::from_str(&command(path.to_str().unwrap(), &value.to_string()).unwrap())
                .unwrap()
        };
        for evidence in ["missing", "different", "unconfirmed", "old_session"] {
            request(json!({"command":"reset"}));
            request(json!({
                "command":"observe", "profile":profile_value(&profile),
                "session_id":"138", "applying":false
            }));
            let mut desired = profile.clone();
            desired.disable_quic = !profile.disable_quic;
            let operation = Uuid::new_v4();
            request(json!({
                "command":"save", "operation_id":operation, "account_id":profile.id,
                "values":profile_value(&desired), "changed_fields":["disable_quic"],
                "phase":"connected", "available":true, "session_id":"138"
            }));
            let mut failure = json!({
                "command":"failed", "operation_id":operation, "session_id":"138",
                "outcome":"rejected", "confirmed_profile":profile_value(&profile)
            });
            match evidence {
                "missing" => failure["confirmed_profile"] = Value::Null,
                "different" => failure["confirmed_profile"] = profile_value(&desired),
                "unconfirmed" => {
                    failure.as_object_mut().unwrap().remove("outcome");
                }
                "old_session" => failure["session_id"] = json!("137"),
                _ => unreachable!(),
            }
            let result = request(failure);
            assert_eq!(result["persisted"], true, "{evidence}");
            assert_eq!(
                result["stored_profile"]["disable_quic"],
                desired.disable_quic
            );
            if evidence == "old_session" {
                assert_eq!(result["applied_profile"], profile_value(&profile));
                assert_eq!(result["apply_status"], "applying");
                assert!(result["error_code"].is_null());
            } else {
                assert!(result["applied_profile"].is_null(), "{evidence}");
                assert_eq!(result["apply_status"], "failed", "{evidence}");
            }
        }
    }

    #[test]
    fn json_readback_retains_durable_save_receipts_after_later_saves() {
        let _test = TEST_STATE.lock().unwrap();
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("profiles-v2.json");
        let store = ConfigStore::new(&path);
        let config = usque_core::AppConfig::default();
        store.save(&config).unwrap();
        let profile = config.active_profile().unwrap();
        let first = Uuid::new_v4();
        let second = Uuid::new_v4();
        let rejected = Uuid::new_v4();
        for operation in [first, second, rejected] {
            let field = if operation == rejected {
                "unsupported-field"
            } else {
                "mtu"
            };
            let result = command(
                path.to_str().unwrap(),
                &json!({
                    "command":"save", "operation_id":operation, "account_id":profile.id,
                    "values":profile_value(&profile), "changed_fields":[field],
                    "phase":"disconnected", "available":false, "session_id":"receipt-test"
                })
                .to_string(),
            );
            assert_eq!(result.is_ok(), operation != rejected);
        }
        let state: Value = serde_json::from_str(
            &command(path.to_str().unwrap(), "{\"command\":\"get\"}").unwrap(),
        )
        .unwrap();
        let receipts = state["persisted_operation_ids"].as_array().unwrap();
        assert!(receipts.contains(&json!(first)));
        assert!(receipts.contains(&json!(second)));
        assert!(!receipts.contains(&json!(rejected)));
        assert!(receipts.len() <= usque_core::network_settings::PERSISTED_OPERATION_HISTORY_LIMIT);
    }

    #[test]
    fn routing_validation_errors_are_definitive_and_preserve_rule_ids() {
        let _test = TEST_STATE.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("profiles-v2.json");
        let store = ConfigStore::new(&path);
        let config = usque_core::AppConfig::default();
        store.save(&config).unwrap();
        let mut profile = config.active_profile().unwrap();
        let first = Uuid::new_v4();
        let second = Uuid::new_v4();
        profile.routing.rules = vec![
            usque_core::RoutingRule {
                id: first,
                kind: usque_core::RoutingMatch::Domain,
                target: "bücher.example".into(),
                action: usque_core::RoutingAction::Direct,
            },
            usque_core::RoutingRule {
                id: second,
                kind: usque_core::RoutingMatch::Domain,
                target: "xn--bcher-kva.example".into(),
                action: usque_core::RoutingAction::Reject,
            },
        ];
        let save = |profile: &usque_core::Profile, field| {
            command(
                path.to_str().unwrap(),
                &json!({
                    "command":"save", "operation_id":Uuid::new_v4(), "account_id":profile.id,
                    "values":profile_value(profile), "changed_fields":[field],
                    "phase":"disconnected", "available":false, "session_id":"1"
                })
                .to_string(),
            )
        };
        assert_eq!(
            save(&profile, "routing").unwrap_err(),
            format!("ROUTING_RULE_CONFLICT:{first}:{second}")
        );
        profile.routing = Default::default();
        assert_eq!(
            save(&profile, "bypass_domains").unwrap_err(),
            "ROUTING_UPGRADE_REQUIRED"
        );
        assert!(store.load().unwrap().network.routing.rules.is_empty());
    }

    #[test]
    fn host_uses_shared_policy_and_never_activates_an_earlier_deferred_edit() {
        let _test = TEST_STATE.lock().unwrap();
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("profiles-v2.json");
        let store = ConfigStore::new(&path);
        let config = usque_core::AppConfig::default();
        store.save(&config).unwrap();
        let profile = config.active_profile().unwrap();
        let path = path.to_str().unwrap();
        command(
            path,
            &json!({"command":"observe","profile":profile_value(&profile),
            "session_id":"1","applying":false})
            .to_string(),
        )
        .unwrap();
        let mut edited = profile.clone();
        edited.mtu = 1400;
        for phase in [
            "disconnected",
            "preparing",
            "reconnecting",
            "disconnecting",
            "error",
        ] {
            let result: Value = serde_json::from_str(
                &command(
                    path,
                    &json!({
                        "command":"save", "operation_id":Uuid::new_v4(), "account_id":profile.id,
                        "values":profile_value(&edited), "changed_fields":["mtu"],
                        "phase":phase, "available":true, "session_id":"1"
                    })
                    .to_string(),
                )
                .unwrap(),
            )
            .unwrap();
            assert_eq!(result["persisted"], true);
            assert_eq!(result["apply_status"], "deferred");
            assert!(result["target"].is_null());
        }
        edited.congestion_control = usque_core::CongestionControlAlgorithm::Reno;
        edited.proxy.http_listeners[0].set_port(9090);
        let result: Value = serde_json::from_str(&command(path, &json!({
            "command":"save", "operation_id":Uuid::new_v4(), "account_id":profile.id,
            "values":profile_value(&edited), "changed_fields":["congestion_control","proxy.http_listeners"],
            "phase":"connected", "available":true, "session_id":"1"
        }).to_string()).unwrap()).unwrap();
        assert_eq!(result["target"]["mtu"], profile.mtu);
        assert_eq!(result["target"]["congestion_control"], "cubic");
        assert_eq!(result["target"]["proxy"]["http_port"], 9090);
        assert_eq!(store.load().unwrap().network.mtu, 1400);
        let unconfirmed: Value = serde_json::from_str(
            &command(
                path,
                &json!({"command":"observe", "profile":profile_value(&edited),
                "session_id":"1", "applying":false, "unconfirmed":true})
                .to_string(),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(unconfirmed["persisted"], true);
        assert_eq!(unconfirmed["apply_status"], "unknown");
        assert!(unconfirmed["applied_profile"].is_null());
        assert!(command(path, "{malformed").is_err());
    }
}
