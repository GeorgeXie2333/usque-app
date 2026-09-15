use super::*;

pub(crate) fn evidence() -> PlatformState {
    PlatformState {
        agent_phase: agent_v1::AgentPhase::RecoveryRequired as i32,
        pending_cleanup: true,
        recovery_diagnostics: Some(Box::new(agent_v1::RecoveryDiagnostics {
            current: Some(agent_v1::RecoveryObservation {
                sampled_at_unix_ms: 100,
                journal_generation: 25,
                status: agent_v1::RecoverySampleStatus::Complete as i32,
                interface: Some(agent_v1::RecoveryResourceObservation {
                    presence: agent_v1::RecoveryPresence::Present as i32,
                    identity_check: agent_v1::RecoveryIdentityCheck::Verified as i32,
                    interface_oper_status: Some(2),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            history_status: agent_v1::RecoveryHistoryStatus::Complete as i32,
            history: vec![agent_v1::RecoveryHistoryEvent {
                occurred_at_unix_ms: 90,
                journal_generation: 25,
                step: agent_v1::RecoveryStep::WintunAdapter as i32,
                restored: false,
                ..Default::default()
            }],
            trace: Some(agent_v1::RecoveryTrace {
                status: agent_v1::RecoveryHistoryStatus::Complete as i32,
                events: vec![agent_v1::RecoveryTraceEvent {
                    schema_version: 1,
                    agent_run_id: 3,
                    event_sequence: 4,
                    occurred_at_unix_ms: 89,
                    journal_generation: 25,
                    resource_id: 6,
                    stage: agent_v1::RecoveryTraceStage::CloseAdapterReturned as i32,
                    elapsed_ms: Some(196),
                    ..Default::default()
                }],
                ..Default::default()
            }),
        })),
        ..Default::default()
    }
}

#[test]
fn agent_exit_preserves_historical_evidence_without_claiming_a_current_state() {
    let directory = tempfile::tempdir().unwrap();
    assert!(store(directory.path(), &evidence(), 120).unwrap());
    let unavailable = PlatformState {
        service_state: "unavailable".into(),
        ..Default::default()
    };
    assert!(!store(directory.path(), &unavailable, 140).unwrap());
    let value = summary(Some(&unavailable), directory.path());
    assert_eq!(value["availability"], "agent_unavailable");
    assert_eq!(value["agent_phase"], "unknown");
    assert!(value["current_observation"].is_null());
    let cached = &value["cached_evidence"];
    assert_eq!(cached["captured_at_unix_ms"], 120);
    assert_eq!(cached["source"], "previous_agent_response");
    assert_eq!(
        cached["snapshot"]["observation_at_capture"]["sampled_at_unix_ms"],
        100
    );
    assert_eq!(cached["snapshot"]["history"][0]["occurred_at_unix_ms"], 90);
    assert_eq!(
        cached["snapshot"]["trace"]["events"][0]["occurred_at_unix_ms"],
        89
    );
    assert!(cached["snapshot"].get("current_observation").is_none());
    assert_eq!(
        summary(Some(&evidence()), directory.path())["cached_evidence"]["availability"],
        "not_needed"
    );
    // Old Agent and busy/generation-changed samples also keep the historical fallback.
    for state in [
        PlatformState::default(),
        PlatformState {
            service_state: "timeout".into(),
            ..Default::default()
        },
    ] {
        assert!(!store(directory.path(), &state, 150).unwrap());
        assert_eq!(
            summary(Some(&state), directory.path())["cached_evidence"]["captured_at_unix_ms"],
            120
        );
    }
}

#[test]
fn cache_and_export_rebuild_fields_and_bound_untrusted_evidence() {
    let directory = tempfile::tempdir().unwrap();
    let hostile = "secret-SID-GUID-LUID-token-password-192.0.2.1";
    let mut state = evidence();
    state.service_state = hostile.into();
    state.wintun_adapter_state = hostile.into();
    state.automatic_recovery = Some(agent_v1::AutomaticRecoveryStatus {
        terminal_error: Some(agent_v1::AgentError {
            code: hostile.into(),
            message: hostile.into(),
            retryable: true,
        }),
        ..Default::default()
    });
    let diagnostics = state.recovery_diagnostics.as_mut().unwrap();
    diagnostics.history = vec![diagnostics.history[0]; 1000];
    let trace = diagnostics.trace.as_mut().unwrap();
    trace.events = vec![trace.events[0]; 1000];
    assert!(store(directory.path(), &state, 123).unwrap());
    let path = directory.path().join(FILE_NAME);
    let bytes = fs::read(&path).unwrap();
    assert!(bytes.len() < MAX_BYTES as usize);
    assert!(!String::from_utf8_lossy(&bytes).contains(hostile));
    let mut injected: Value = serde_json::from_slice(&bytes).unwrap();
    injected["sid"] = json!(hostile);
    injected["diagnostics"]["current"]["adapter_guid"] = json!(hostile);
    injected["diagnostics"]["trace"]["events"][0]["native_message"] = json!(hostile);
    injected["diagnostics"]["current"]["interface"]["presence"] = json!(2);
    injected["diagnostics"]["current"]["interface"]["identity_check"] = json!(2);
    fs::write(&path, serde_json::to_vec(&injected).unwrap()).unwrap();
    let value = summary(None, directory.path());
    let snapshot = &value["cached_evidence"]["snapshot"];
    assert_eq!(snapshot["history"].as_array().unwrap().len(), 32);
    assert!(snapshot["trace"]["events"].as_array().unwrap().len() <= 128);
    assert_eq!(
        snapshot["observation_at_capture"]["interface"]["presence"],
        "unknown"
    );
    let exported = serde_json::to_vec_pretty(&value).unwrap();
    assert!(exported.len() < 256 * 1024);
    assert!(!String::from_utf8_lossy(&exported).contains(hostile));
}

#[test]
fn missing_corrupt_oversized_and_future_cache_never_prevent_export() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join(FILE_NAME);
    assert_eq!(
        summary(None, directory.path())["cached_evidence"]["availability"],
        "missing"
    );
    for bytes in [b"broken".to_vec(), vec![b' '; MAX_BYTES as usize + 1]] {
        fs::write(&path, bytes).unwrap();
        assert_eq!(
            summary(None, directory.path())["cached_evidence"]["availability"],
            "unavailable"
        );
    }
    store(directory.path(), &evidence(), 100).unwrap();
    let mut value: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    value["schema_version"] = json!(999);
    fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
    assert_eq!(
        summary(None, directory.path())["cached_evidence"]["availability"],
        "unavailable"
    );
    fs::remove_file(&path).unwrap();
    fs::create_dir(&path).unwrap();
    assert!(store(directory.path(), &evidence(), 100).is_err());
    assert_eq!(
        summary(None, directory.path())["cached_evidence"]["availability"],
        "unavailable"
    );
}

#[test]
fn out_of_order_or_partial_captures_cannot_erase_the_previous_trace() {
    let directory = tempfile::tempdir().unwrap();
    store(directory.path(), &evidence(), 200).unwrap();
    assert!(!store(directory.path(), &evidence(), 100).unwrap());
    let mut partial = evidence();
    let diagnostics = partial.recovery_diagnostics.as_mut().unwrap();
    diagnostics.trace = None;
    diagnostics.history[0].occurred_at_unix_ms = 205;
    store(directory.path(), &partial, 210).unwrap();
    let value = summary(None, directory.path());
    let snapshot = &value["cached_evidence"]["snapshot"];
    assert_eq!(snapshot["history"].as_array().unwrap().len(), 2);
    assert_eq!(snapshot["trace"]["events"].as_array().unwrap().len(), 1);
    assert_eq!(snapshot["trace"]["status"], "partial");
    assert_eq!(value["cached_evidence"]["captured_at_unix_ms"], 210);
}

#[test]
fn dense_cached_observations_fit_the_complete_export_budget() {
    let directory = tempfile::tempdir().unwrap();
    let mut state = evidence();
    let diagnostics = state.recovery_diagnostics.as_mut().unwrap();
    diagnostics.history = vec![diagnostics.history[0]; 32];
    let trace = diagnostics.trace.as_mut().unwrap();
    let mut event = trace.events[0];
    event.stage = agent_v1::RecoveryTraceStage::RemovalAttemptReturned as i32;
    event.agent_run_id = u64::MAX;
    event.event_sequence = u64::MAX;
    event.occurred_at_unix_ms = u64::MAX;
    event.monotonic_ms = u64::MAX;
    event.journal_generation = u64::MAX;
    event.resource_id = u64::MAX;
    event.elapsed_ms = Some(u64::MAX);
    event.succeeded = Some(false);
    event.dropped_events = u64::MAX;
    event.write_failures = u64::MAX;
    event.observation = Some(agent_v1::RecoveryObservation {
        sampled_at_unix_ms: u64::MAX,
        journal_generation: u64::MAX,
        status: 1,
        interface: Some(agent_v1::RecoveryResourceObservation {
            presence: 1,
            identity_check: 1,
            api: 1,
            interface_oper_status: Some(7),
            interface_admin_status: Some(3),
            media_connect_state: Some(2),
            ..Default::default()
        }),
        pnp_device: Some(agent_v1::RecoveryResourceObservation {
            presence: 1,
            identity_check: 1,
            api: 8,
            devnode_status: Some(u32::MAX),
            problem_code: Some(u32::MAX),
            ..Default::default()
        }),
    });
    trace.events = vec![event; 128];
    assert!(store(directory.path(), &state, 777).unwrap());
    let value = summary(None, directory.path());
    let bytes = serde_json::to_vec_pretty(&value).unwrap();
    assert!(bytes.len() < 256 * 1024, "{} bytes", bytes.len());
    assert_eq!(value["cached_evidence"]["availability"], "available");
}

#[test]
fn busy_cache_write_is_best_effort_and_does_not_block_another_directory() {
    let directory = tempfile::tempdir().unwrap();
    let other = tempfile::tempdir().unwrap();
    let gate = write_gate(directory.path()).unwrap();
    let _guard = gate.lock().unwrap();
    assert!(store(directory.path(), &evidence(), 100).is_err());
    assert!(store(other.path(), &evidence(), 100).unwrap());
}
