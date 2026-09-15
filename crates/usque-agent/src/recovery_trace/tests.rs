use super::*;

#[test]
fn blocked_writer_never_blocks_cleanup_and_reports_queue_loss() {
    let (sink, receiver) = TraceSink::channel(1);
    let resource = sink.resource(42);
    let called = std::cell::Cell::new(false);
    resource.call(Stage::EndSessionStarted, Stage::EndSessionReturned, || {
        called.set(true);
    });
    assert!(called.get());
    assert_eq!(sink.losses(), (1, 0));
    let start = receiver.recv().unwrap();
    assert_eq!(start.stage, Stage::EndSessionStarted as i32);
    assert_eq!(start.journal_generation, 42);
    assert_eq!(start.succeeded, None);
    resource.record(Stage::AdapterReleaseRequested);
    assert_eq!(receiver.recv().unwrap().dropped_events, 1);
}

#[test]
fn a_void_return_is_not_cleanup_success_and_keeps_the_resource_generation() {
    let (sink, receiver) = TraceSink::channel(8);
    let old = sink.resource(21);
    let new = sink.resource(99);
    old.set_generation(28);
    old.call(
        Stage::CloseAdapterStarted,
        Stage::CloseAdapterReturned,
        || {},
    );
    new.record(Stage::AdapterReleaseRequested);
    let rows: Vec<_> = receiver.try_iter().collect();
    assert_eq!(rows[0].journal_generation, 28);
    assert_eq!(rows[1].journal_generation, 28);
    assert_eq!(rows[0].resource_id, rows[1].resource_id);
    assert_ne!(rows[1].resource_id, rows[2].resource_id);
    assert!(rows[1].elapsed_ms.is_some());
    assert_eq!(rows[1].succeeded, None);
}

#[test]
fn removal_attempt_success_requires_verified_absence_in_the_same_generation() {
    for (status, generation, presence, success) in [
        (1, 9, 2, true),
        (1, 9, 1, false),
        (2, 9, 2, false),
        (1, 10, 2, false),
    ] {
        let (sink, receiver) = TraceSink::channel(1);
        let resource = sink.resource(9);
        let mut event = resource.event(Stage::RemovalAttemptReturned);
        event.succeeded = Some(true);
        let observed = agent_v1::RecoveryResourceObservation {
            presence,
            identity_check: 1,
            ..Default::default()
        };
        event.observation = Some(agent_v1::RecoveryObservation {
            sampled_at_unix_ms: 100,
            journal_generation: generation,
            status,
            interface: Some(observed),
            pnp_device: Some(observed),
        });
        resource.emit(event);
        let event = sanitize_event(receiver.recv().unwrap()).unwrap();
        assert_eq!(event.succeeded, success.then_some(true));
    }
}

#[test]
fn blocked_native_call_emits_start_without_fabricating_a_return() {
    let (sink, receiver) = TraceSink::channel(4);
    let (release, blocked) = mpsc::channel();
    let thread = std::thread::spawn(move || {
        sink.resource(21).call(
            Stage::CloseAdapterStarted,
            Stage::CloseAdapterReturned,
            || blocked.recv().unwrap(),
        );
    });
    let start = receiver.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_eq!(start.stage, Stage::CloseAdapterStarted as i32);
    assert_eq!(receiver.try_recv(), Err(mpsc::TryRecvError::Empty));
    release.send(()).unwrap();
    thread.join().unwrap();
    let returned = receiver.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_eq!(returned.stage, Stage::CloseAdapterReturned as i32);
    assert_eq!(returned.resource_id, start.resource_id);
    assert_eq!(returned.succeeded, None);
}

#[test]
fn raw_native_messages_and_unassociated_callbacks_do_not_leak_or_borrow_a_transaction() {
    let (sink, receiver) = TraceSink::channel(8);
    let secret = "Failed to remove adapter when closing SID GUID LUID password 192.0.2.1";
    sink.native_message(3, 123, secret);
    let row = receiver.recv().unwrap();
    assert_eq!(row.stage, Stage::NativeRemoveFailed as i32);
    assert_eq!(row.journal_generation, 0);
    assert_eq!(row.resource_id, 0);
    let encoded = serde_json::to_string(&row).unwrap();
    for value in ["SID", "GUID", "LUID", "password", "192.0.2.1"] {
        assert!(!encoded.contains(value));
    }
    let resource = sink.resource(10);
    resource.call(Stage::EndSessionStarted, Stage::EndSessionReturned, || {
        sink.native_message(2, 124, "unknown raw device name");
    });
    let rows: Vec<_> = receiver.try_iter().collect();
    assert_eq!(rows[1].journal_generation, 10);
    assert_eq!(rows[1].resource_id, rows[0].resource_id);
}

#[test]
fn a_failed_writer_does_not_change_the_operation_result() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join(TRACE_LOG_NAME);
    std::fs::create_dir(&path).unwrap();
    let sink = TraceSink::open(&directory.path().join("recovery-v1.json"));
    let result = sink.resource(1).call(
        Stage::CloseAdapterStarted,
        Stage::CloseAdapterReturned,
        || 73,
    );
    assert_eq!(result, 73);
    let deadline = Instant::now() + Duration::from_secs(2);
    while sink.losses().1 == 0 && Instant::now() < deadline {
        std::thread::yield_now();
    }
    assert!(sink.losses().1 > 0);
}

#[test]
fn history_is_bounded_and_reserializes_only_valid_typed_fields() {
    let directory = tempfile::tempdir().unwrap();
    let journal = directory.path().join("recovery-v1.json");
    let path = directory.path().join(TRACE_LOG_NAME);
    assert_eq!(read(&journal).status, HistoryStatus::Missing as i32);
    let (sink, receiver) = TraceSink::channel(256);
    for _ in 0..140 {
        sink.resource(7).record(Stage::EndSessionStarted);
    }
    let mut lines = String::new();
    for row in receiver.try_iter() {
        let mut value = serde_json::to_value(row).unwrap();
        value["adapter_guid"] = serde_json::json!("SID GUID token 192.0.2.1");
        lines.push_str(&serde_json::to_string(&value).unwrap());
        lines.push('\n');
    }
    lines.push_str("broken\n");
    lines.push_str(&"x".repeat(MAX_RECORD_BYTES + 1));
    fs::write(&path, lines).unwrap();
    let trace = read(&journal);
    assert_eq!(trace.status, HistoryStatus::Partial as i32);
    assert_eq!(trace.events.len(), 128);
    assert_eq!(trace.events[0].event_sequence, 13);
    let encoded = serde_json::to_string(&trace).unwrap();
    assert!(!encoded.contains("192.0.2.1"));
    assert!(!encoded.contains("adapter_guid"));
    fs::write(&path, vec![b'x'; MAX_FILE_BYTES as usize + 1]).unwrap();
    assert_eq!(read(&journal).status, HistoryStatus::TooLarge as i32);
    fs::remove_file(&path).unwrap();
    fs::create_dir(&path).unwrap();
    assert_eq!(read(&journal).status, HistoryStatus::Unavailable as i32);
}

#[test]
fn trace_unknown_or_failed_samples_never_claim_resource_absence() {
    let (sink, receiver) = TraceSink::channel(2);
    let resource = sink.resource(9);
    let mut event = resource.event(Stage::ObservationChanged);
    event.observation = Some(agent_v1::RecoveryObservation {
        status: agent_v1::RecoverySampleStatus::Timeout as i32,
        interface: Some(agent_v1::RecoveryResourceObservation {
            presence: 2,
            identity_check: 1,
            ..Default::default()
        }),
        ..Default::default()
    });
    sink.record(event);
    let sanitized = sanitize_event(receiver.recv().unwrap()).unwrap();
    assert!(sanitized.observation.unwrap().interface.is_none());
    let value = sanitize_resource(agent_v1::RecoveryResourceObservation {
        presence: 2,
        identity_check: 1,
        configret_code: Some(5),
        ..Default::default()
    });
    assert_eq!(value.presence, 0);
    assert_eq!(value.win32_code, None);
    assert_eq!(value.configret_code, Some(5));
}

#[test]
fn invalid_absence_and_unrelated_stage_fields_are_rejected_at_the_agent_boundary() {
    let (sink, receiver) = TraceSink::channel(2);
    let resource = sink.resource(9);
    let mut event = resource.event(Stage::ObservationAbsent);
    event.observation = Some(agent_v1::RecoveryObservation {
        status: agent_v1::RecoverySampleStatus::Complete as i32,
        sampled_at_unix_ms: 100,
        journal_generation: 8,
        interface: Some(agent_v1::RecoveryResourceObservation {
            presence: 2,
            identity_check: 1,
            ..Default::default()
        }),
        ..Default::default()
    });
    sink.record(event);
    assert!(sanitize_event(receiver.recv().unwrap()).is_none());
    event.stage = Stage::CloseAdapterReturned as i32;
    event.succeeded = Some(true);
    event.native_level = Some(3);
    event.reference_count = Some(7);
    sink.record(event);
    let event = sanitize_event(receiver.recv().unwrap()).unwrap();
    assert!(event.observation.is_none());
    assert!(event.succeeded.is_none());
    assert!(event.native_level.is_none());
    assert!(event.reference_count.is_none());
}
