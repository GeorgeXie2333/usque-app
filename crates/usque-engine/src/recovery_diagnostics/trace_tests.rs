use super::*;
use agent_v1::{RecoveryHistoryStatus, RecoveryTrace, RecoveryTraceEvent, RecoveryTraceStage};

fn event(stage: RecoveryTraceStage) -> RecoveryTraceEvent {
    RecoveryTraceEvent {
        schema_version: 1,
        agent_run_id: 42,
        event_sequence: 1,
        occurred_at_unix_ms: 100,
        monotonic_ms: 17,
        journal_generation: 9,
        resource_id: 2,
        stage: stage as i32,
        ..Default::default()
    }
}

fn platform(events: Vec<RecoveryTraceEvent>) -> PlatformState {
    PlatformState {
        recovery_diagnostics: Some(Box::new(agent_v1::RecoveryDiagnostics {
            current: Some(agent_v1::RecoveryObservation {
                sampled_at_unix_ms: 200,
                journal_generation: 10,
                status: 1,
                ..Default::default()
            }),
            trace: Some(RecoveryTrace {
                status: RecoveryHistoryStatus::Complete as i32,
                events,
                dropped_events: 5,
                write_failures: 3,
            }),
            ..Default::default()
        })),
        ..Default::default()
    }
}

#[test]
fn lifecycle_export_keeps_original_times_and_never_calls_a_void_return_success() {
    let mut returned = event(RecoveryTraceStage::CloseAdapterReturned);
    returned.elapsed_ms = Some(10039);
    returned.succeeded = Some(true); // Contradictory input from the wire.
    returned.native_level = Some(3);
    returned.reference_count = Some(8);
    let value = summary(Some(&platform(vec![returned])));
    let row = &value["trace"]["events"][0];
    assert_eq!(row["stage"], "close_adapter_returned");
    assert_eq!(row["occurred_at_unix_ms"], 100);
    assert_eq!(row["monotonic_ms"], 17);
    assert_eq!(row["journal_generation"], 9);
    assert_eq!(row["elapsed_ms"], 10039);
    assert!(row["succeeded"].is_null());
    assert!(row["native_level"].is_null());
    assert!(row["reference_count"].is_null());
    assert_eq!(value["current_observation"]["sampled_at_unix_ms"], 200);
    assert_eq!(value["trace"]["dropped_events"], 5);
    assert_eq!(value["trace"]["write_failures"], 3);
}

#[test]
fn lifecycle_export_bounds_count_and_bytes_and_rejects_unknown_or_sensitive_fields() {
    let hostile = json!({
        "schema_version": 1, "agent_run_id": 42, "event_sequence": 1,
        "occurred_at_unix_ms": 100, "journal_generation": 9, "resource_id": 2,
        "stage": RecoveryTraceStage::EndSessionReturned as i32,
        "native_message": "SID GUID LUID 192.0.2.1 token password adapter-name",
        "adapter_guid": "injected-device-id",
    });
    let row: RecoveryTraceEvent = serde_json::from_value(hostile).unwrap();
    let mut rows = vec![row; 200];
    for (i, row) in rows.iter_mut().enumerate() {
        row.event_sequence = i as u64 + 1;
    }
    rows.push(RecoveryTraceEvent {
        stage: i32::MAX,
        ..row
    });
    rows.push(RecoveryTraceEvent {
        schema_version: 99,
        ..row
    });
    let value = summary(Some(&platform(rows)));
    let rows = value["trace"]["events"].as_array().unwrap();
    assert_eq!(rows.len(), 128);
    assert_eq!(rows[0]["event_sequence"], 73);
    assert_eq!(value["trace"]["status"], "partial");
    assert_eq!(value["trace"]["truncated"], true);
    let text = serde_json::to_string_pretty(&value).unwrap();
    assert!(text.len() < 256 * 1024);
    for forbidden in [
        "SID",
        "GUID",
        "LUID",
        "192.0.2.1",
        "password",
        "token",
        "adapter-name",
        "injected-device-id",
        "native_message",
    ] {
        assert!(!text.contains(forbidden));
    }

    let mut large = event(RecoveryTraceStage::ObservationChanged);
    large.agent_run_id = u64::MAX;
    large.event_sequence = u64::MAX;
    large.occurred_at_unix_ms = u64::MAX;
    large.journal_generation = u64::MAX;
    large.resource_id = u64::MAX;
    large.dropped_events = u64::MAX;
    large.write_failures = u64::MAX;
    large.observation = Some(agent_v1::RecoveryObservation {
        sampled_at_unix_ms: u64::MAX,
        journal_generation: u64::MAX,
        status: 1,
        interface: Some(agent_v1::RecoveryResourceObservation {
            presence: 1,
            identity_check: 1,
            interface_oper_status: Some(7),
            interface_admin_status: Some(3),
            media_connect_state: Some(2),
            ..Default::default()
        }),
        pnp_device: Some(agent_v1::RecoveryResourceObservation {
            presence: 1,
            identity_check: 1,
            devnode_status: Some(u32::MAX),
            problem_code: Some(u32::MAX),
            ..Default::default()
        }),
    });
    let value = summary(Some(&platform(vec![large; 200])));
    assert!(serde_json::to_vec_pretty(&value).unwrap().len() < 256 * 1024);
    assert!(value["trace"]["events"].as_array().unwrap().len() < 128);
    assert_eq!(value["trace"]["truncated"], true);
}

#[test]
fn lifecycle_export_cannot_confuse_missing_evidence_or_configret_with_absence() {
    let mut state = platform(vec![]);
    state.recovery_diagnostics.as_mut().unwrap().trace = None;
    assert_eq!(
        summary(Some(&state))["trace"]["availability"],
        "extension_unavailable"
    );
    for status in [
        RecoveryHistoryStatus::Missing,
        RecoveryHistoryStatus::TooLarge,
        RecoveryHistoryStatus::Unavailable,
        RecoveryHistoryStatus::Unspecified,
    ] {
        state.recovery_diagnostics.as_mut().unwrap().trace = Some(RecoveryTrace {
            status: status as i32,
            events: vec![event(RecoveryTraceStage::CloseAdapterReturned)],
            ..Default::default()
        });
        assert!(
            summary(Some(&state))["trace"]["events"]
                .as_array()
                .unwrap()
                .is_empty()
        );
    }
    let mut row = event(RecoveryTraceStage::ObservationChanged);
    row.observation = Some(agent_v1::RecoveryObservation {
        sampled_at_unix_ms: 100,
        journal_generation: 9,
        status: 1,
        interface: Some(agent_v1::RecoveryResourceObservation {
            presence: 2,
            identity_check: 1,
            configret_code: Some(5),
            api: 8,
            ..Default::default()
        }),
        ..Default::default()
    });
    let value = summary(Some(&platform(vec![row])));
    let resource = &value["trace"]["events"][0]["observation"]["interface"];
    assert_eq!(resource["presence"], "unknown");
    assert_eq!(resource["api"], "cm_get_dev_node_status");
    assert_eq!(resource["configret_code"], 5);
    assert!(resource["win32_code"].is_null());
    row.stage = RecoveryTraceStage::ObservationAbsent as i32;
    assert!(
        summary(Some(&platform(vec![row])))["trace"]["events"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn lifecycle_export_generation_races_and_incomplete_samples_remain_unknown() {
    let absent = agent_v1::RecoveryResourceObservation {
        presence: 2,
        identity_check: 1,
        ..Default::default()
    };
    for status in [0, 2, 3, 4, 5, 6, i32::MAX] {
        let mut row = event(RecoveryTraceStage::ObservationChanged);
        row.observation = Some(agent_v1::RecoveryObservation {
            sampled_at_unix_ms: 100,
            journal_generation: 9,
            status,
            interface: Some(absent),
            pnp_device: Some(absent),
        });
        let value = summary(Some(&platform(vec![row])));
        assert_eq!(
            value["trace"]["events"][0]["observation"]["interface"]["presence"],
            "unknown"
        );
    }
    let mut row = event(RecoveryTraceStage::ObservationAbsent);
    row.observation = Some(agent_v1::RecoveryObservation {
        sampled_at_unix_ms: 100,
        journal_generation: 8,
        status: 1,
        interface: Some(absent),
        pnp_device: Some(absent),
    });
    assert!(
        summary(Some(&platform(vec![row])))["trace"]["events"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    row.observation.as_mut().unwrap().journal_generation = 9;
    assert_eq!(
        summary(Some(&platform(vec![row])))["trace"]["events"][0]["stage"],
        "observation_absent"
    );
}
