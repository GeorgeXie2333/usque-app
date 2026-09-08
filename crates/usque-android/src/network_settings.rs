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
                merge_patch(config, &patch)
                    .map_err(|error| StoreError::NetworkSettings(error.to_string()))
            });
            let stored = match commit {
                Ok((_, profile)) => profile,
                Err(StoreError::CommitUncertain(_)) => {
                    state.stored_profile =
                        store.load().ok().and_then(|config| config.active_profile());
                    state.operation_id = Some(operation_id);
                    state.persisted = None;
                    state.apply_status = ApplyStatus::Unknown;
                    state.error_code = Some("NETWORK_SETTINGS_COMMIT_UNCONFIRMED".into());
                    state.advance();
                    return encode(&state, None);
                }
                Err(_) => return Err("NETWORK_SETTINGS_SAVE_FAILED".into()),
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
        Command::Get => {
            let _lock = store
                .lock_exclusive()
                .map_err(|_| "NETWORK_SETTINGS_UNCONFIRMED")?;
            state.stored_profile = store
                .load_or_default()
                .map_err(|_| "NETWORK_SETTINGS_UNCONFIRMED")?
                .active_profile();
        }
        Command::Observe {
            profile,
            session_id,
            applying,
            unconfirmed,
        } => {
            state.applied_profile = profile.map(android_profile_to_core).transpose()?;
            state.session_id = Some(session_id);
            state.apply_status = if unconfirmed {
                state.applied_profile = None;
                ApplyStatus::Unknown
            } else if applying {
                ApplyStatus::Applying
            } else if state.applied_profile.is_some() {
                ApplyStatus::Applied
            } else {
                ApplyStatus::Deferred
            };
            state.error_code = unconfirmed.then(|| "NETWORK_SETTINGS_RECOVERY_UNCONFIRMED".into());
        }
        Command::Failed {
            operation_id,
            session_id,
        } => {
            if state.session_id.as_deref() == Some(session_id.as_str()) {
                state.applied_profile = None;
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

fn profile_value(profile: &usque_core::Profile) -> Value {
    android_profile_value(profile, None, false)
}

fn encode(
    state: &NetworkSettingsState,
    target: Option<&usque_core::Profile>,
) -> Result<String, String> {
    let mut result = serde_json::to_value(state).map_err(|_| "network settings encoding failed")?;
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

    #[test]
    fn host_uses_shared_policy_and_never_activates_an_earlier_deferred_edit() {
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
