//! Export boundary: never serialize the Agent response or terminal error text.
use serde_json::{Value, json};
use usque_ipc::agent_v1::{self, PlatformState};

#[cfg(test)]
mod trace_tests;

macro_rules! label {
    ($kind:ident, $value:expr, $prefix:literal) => {
        agent_v1::$kind::try_from($value)
            .ok()
            .filter(|value| *value as i32 != 0)
            .map(|value| {
                value
                    .as_str_name()
                    .trim_start_matches($prefix)
                    .to_ascii_lowercase()
            })
            .unwrap_or_else(|| "unknown".to_owned())
    };
}

mod trace;

#[cfg(windows)]
pub(crate) async fn capture() -> PlatformState {
    match tokio::time::timeout(
        std::time::Duration::from_secs(2),
        crate::windows_agent::inspect_platform_state_if_running(),
    )
    .await
    {
        Ok(Ok(state)) => state,
        result => PlatformState {
            service_state: if result.is_err() {
                "timeout"
            } else {
                "unavailable"
            }
            .to_owned(),
            recovery_diagnostics: Some(Box::new(agent_v1::RecoveryDiagnostics {
                current: Some(agent_v1::RecoveryObservation {
                    status: if result.is_err() {
                        agent_v1::RecoverySampleStatus::Timeout
                    } else {
                        agent_v1::RecoverySampleStatus::Unavailable
                    } as i32,
                    ..Default::default()
                }),
                history_status: agent_v1::RecoveryHistoryStatus::Unavailable as i32,
                ..Default::default()
            })),
            ..Default::default()
        },
    }
}

pub(crate) fn summary(platform: Option<&PlatformState>) -> Value {
    let diagnostics = platform.and_then(|state| state.recovery_diagnostics.as_ref());
    let availability = match platform {
        None => "not_sampled",
        Some(state) if state.service_state == "unavailable" => "agent_unavailable",
        Some(state) if state.service_state == "timeout" => "timeout",
        Some(_) if diagnostics.is_none() => "extension_unavailable",
        Some(_) => "available",
    };
    let automatic = platform
        .and_then(|state| state.automatic_recovery.as_ref())
        .map(|status| {
            let valid = status.attempt_limit == 3 && status.attempts_completed <= 3;
            let code = status
                .terminal_error
                .as_ref()
                .map(|error| match error.code.as_str() {
                    "AGENT_AUTOMATIC_RECOVERY_EXHAUSTED" => "AGENT_AUTOMATIC_RECOVERY_EXHAUSTED",
                    "AGENT_AUTOMATIC_RECOVERY_BLOCKED" => "AGENT_AUTOMATIC_RECOVERY_BLOCKED",
                    _ => "unknown",
                });
            json!({
                "phase": label!(AutomaticRecoveryPhase, status.phase, "AUTOMATIC_RECOVERY_PHASE_"),
                "attempts_completed": valid.then_some(status.attempts_completed),
                "attempt_limit": valid.then_some(status.attempt_limit),
                "terminal_code": code,
            })
        });
    let current = diagnostics
        .and_then(|diagnostics| diagnostics.current.as_ref())
        .map(|sample| observation(sample, sample.journal_generation));
    let mut history: Vec<_> = diagnostics.into_iter().flat_map(|diagnostics| diagnostics.history.iter()).rev()
        .filter(|event| event.occurred_at_unix_ms != 0 && event.journal_generation != 0
            && agent_v1::RecoveryStep::try_from(event.step).is_ok_and(|step| step != agent_v1::RecoveryStep::Unspecified))
        .take(32).map(|event| json!({
            "occurred_at_unix_ms": event.occurred_at_unix_ms,
            "journal_generation": event.journal_generation,
            "step": label!(RecoveryStep, event.step, "RECOVERY_STEP_"),
            "restored": event.restored,
            "elapsed_ms": event.elapsed_ms,
            "api": api(event.api),
            "win32_code": event.win32_code,
            "adapter": event.adapter.as_ref().map(|adapter| json!({
                "stage": label!(RecoveryRemovalStage, adapter.stage, "RECOVERY_REMOVAL_STAGE_"),
                "failure": label!(RecoveryRemovalFailure, adapter.failure, "RECOVERY_REMOVAL_FAILURE_"),
                "interface": presence(adapter.interface),
                "pnp_device": presence(adapter.pnp_device),
                "request_accepted": adapter.request_accepted,
                "elapsed_ms": adapter.elapsed_ms,
                "api": api(adapter.api),
                "win32_code": adapter.win32_code,
            })),
        })).collect();
    history.reverse();
    json!({
        "schema_version": 1,
        "availability": availability,
        "agent_phase": platform.map(|state| label!(AgentPhase, state.agent_phase, "AGENT_PHASE_")),
        "pending_cleanup": platform.filter(|state| state.agent_phase != 0).map(|state| state.pending_cleanup),
        "automatic_recovery": automatic,
        "current_observation": current,
        "history_status": diagnostics.map(|value| label!(RecoveryHistoryStatus, value.history_status, "RECOVERY_HISTORY_STATUS_")),
        "history": history,
        "trace": trace::summary(diagnostics.and_then(|value| value.trace.as_ref()), availability),
    })
}

fn api(value: i32) -> String {
    label!(RecoveryDiagnosticApi, value, "RECOVERY_DIAGNOSTIC_API_")
}
fn presence(value: i32) -> String {
    label!(RecoveryPresence, value, "RECOVERY_PRESENCE_")
}

fn observation(sample: &agent_v1::RecoveryObservation, generation: u64) -> Value {
    use agent_v1::RecoverySampleStatus as Status;
    let mut status = sample.status;
    if status == Status::Complete as i32 {
        if generation == 0 || sample.journal_generation != generation {
            status = Status::GenerationChanged as i32;
        } else if sample.sampled_at_unix_ms == 0 {
            status = Status::Unavailable as i32;
        }
    }
    let complete = status == Status::Complete as i32;
    json!({
        "sampled_at_unix_ms": sample.sampled_at_unix_ms,
        "journal_generation": sample.journal_generation,
        "status": label!(RecoverySampleStatus, status, "RECOVERY_SAMPLE_STATUS_"),
        "interface": resource(complete.then_some(sample.interface.as_ref()).flatten(), ResourceKind::Interface),
        "pnp_device": resource(complete.then_some(sample.pnp_device.as_ref()).flatten(), ResourceKind::Pnp),
    })
}

#[derive(Clone, Copy, PartialEq)]
enum ResourceKind {
    Interface,
    Pnp,
}

fn resource(value: Option<&agent_v1::RecoveryResourceObservation>, kind: ResourceKind) -> Value {
    let value = value.copied().unwrap_or_default();
    // An identity error or native failure cannot be presented as absence even
    // if a buggy or hostile Agent supplies a contradictory presence enum.
    let verified = value.identity_check == agent_v1::RecoveryIdentityCheck::Verified as i32
        && value.win32_code.is_none()
        && value.configret_code.is_none();
    let present = verified && value.presence == agent_v1::RecoveryPresence::Present as i32;
    let interface = present && kind == ResourceKind::Interface;
    let pnp = present && kind == ResourceKind::Pnp;
    json!({
        "presence": presence(if verified { value.presence } else { 0 }),
        "identity_check": label!(RecoveryIdentityCheck, value.identity_check, "RECOVERY_IDENTITY_CHECK_"),
        "api": api(value.api),
        "win32_code": value.win32_code,
        "configret_code": value.configret_code,
        "interface_oper_status": value.interface_oper_status.filter(|n| interface && (1..=7).contains(n)),
        "interface_admin_status": value.interface_admin_status.filter(|n| interface && (1..=3).contains(n)),
        "media_connect_state": value.media_connect_state.filter(|n| interface && *n <= 2),
        "devnode_status": value.devnode_status.filter(|_| pnp),
        "problem_code": value.problem_code.filter(|_| pnp),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_agent_and_unavailable_sampling_never_imply_absence() {
        let old = summary(Some(&PlatformState::default()));
        assert_eq!(old["availability"], "extension_unavailable");
        assert!(old["current_observation"].is_null());
        for status in [
            agent_v1::RecoverySampleStatus::Timeout,
            agent_v1::RecoverySampleStatus::Busy,
            agent_v1::RecoverySampleStatus::GenerationChanged,
            agent_v1::RecoverySampleStatus::Unavailable,
        ] {
            let state = PlatformState {
                recovery_diagnostics: Some(Box::new(agent_v1::RecoveryDiagnostics {
                    current: Some(agent_v1::RecoveryObservation {
                        status: status as i32,
                        interface: Some(agent_v1::RecoveryResourceObservation {
                            presence: agent_v1::RecoveryPresence::Absent as i32,
                            identity_check: agent_v1::RecoveryIdentityCheck::Verified as i32,
                            ..Default::default()
                        }),
                        ..Default::default()
                    }),
                    ..Default::default()
                })),
                ..Default::default()
            };
            assert_eq!(
                summary(Some(&state))["current_observation"]["interface"]["presence"],
                "unknown"
            );
        }
    }

    #[test]
    fn export_is_bounded_typed_and_separates_current_sample_from_original_event_time() {
        let hostile = "SID GUID LUID 192.0.2.1 password token adapter-name".repeat(4096);
        let state = PlatformState {
            service_state: hostile.clone(),
            wintun_adapter_state: hostile.clone(),
            automatic_recovery: Some(agent_v1::AutomaticRecoveryStatus {
                terminal_error: Some(agent_v1::AgentError {
                    code: hostile.clone(),
                    message: hostile,
                    retryable: true,
                }),
                ..Default::default()
            }),
            recovery_diagnostics: Some(Box::new(agent_v1::RecoveryDiagnostics {
                current: Some(agent_v1::RecoveryObservation {
                    sampled_at_unix_ms: 200,
                    journal_generation: 10,
                    status: 1,
                    ..Default::default()
                }),
                history_status: 1,
                history: (1..=100)
                    .map(|generation| agent_v1::RecoveryHistoryEvent {
                        occurred_at_unix_ms: 123,
                        journal_generation: generation,
                        step: 1,
                        elapsed_ms: 10039,
                        api: i32::MAX,
                        ..Default::default()
                    })
                    .collect(),
                trace: None,
            })),
            ..Default::default()
        };
        let value = summary(Some(&state));
        assert_eq!(value["history"].as_array().unwrap().len(), 32);
        assert_eq!(value["current_observation"]["sampled_at_unix_ms"], 200);
        assert_eq!(value["history"][0]["occurred_at_unix_ms"], 123);
        assert_eq!(value["history"][0]["journal_generation"], 69);
        assert_eq!(value["history"][0]["api"], "unknown");
        let bytes = serde_json::to_vec_pretty(&value).unwrap();
        assert!(bytes.len() < 32 * 1024);
        let text = String::from_utf8(bytes).unwrap();
        for forbidden in [
            "SID",
            "GUID",
            "LUID",
            "192.0.2.1",
            "password",
            "token",
            "adapter-name",
        ] {
            assert!(!text.contains(forbidden));
        }
    }
}
