use super::*;

fn device(state: DeviceState) -> ManagedDevice {
    let device_id = Uuid::new_v4();
    ManagedDevice {
        device_id,
        generation: 1,
        agent_instance: Uuid::new_v4(),
        owner_sid: "S-1-5-21-1000".into(),
        owner_process_id: 42,
        state,
        receipt: MutationReceipt::WintunAdapter {
            adapter_name: ManagedDevice::name(device_id),
            adapter_guid: device_id,
            interface_luid: 7,
        },
    }
}

#[test]
fn idle_device_survives_atomic_connection_cleanup_and_prevents_uninstall() {
    let directory = tempfile::tempdir().unwrap();
    let store = JournalStore::new(directory.path().join("recovery.json"));
    let mut journal = RecoveryJournal::clean(0);
    journal.device = Some(device(DeviceState::Idle));
    store.save(&mut journal).unwrap();
    let loaded = store.load_or_clean().unwrap();
    assert_eq!(loaded, journal);
    assert_eq!(loaded.phase, RecoveryPhase::Clean);
    assert!(!loaded.is_fully_clean());
    assert!(store.remove_if_clean().is_err());
    assert!(store.path().exists());
}

#[test]
fn v2_migration_preserves_every_existing_receipt_without_creating_a_device() {
    let directory = tempfile::tempdir().unwrap();
    let store = JournalStore::new(directory.path().join("recovery.json"));
    let legacy = RecoveryJournal {
        phase: RecoveryPhase::Preparing,
        operation_kind: Some(OperationKind::Tunnel),
        operation_id: Some(Uuid::new_v4()),
        owner_sid: Some("S-1-5-21-1000".into()),
        owner_process_id: Some(42),
        plan: Some(super::tests::plan()),
        steps: vec![MutationRecord {
            kind: MutationKind::WintunAdapter,
            state: MutationState::Intended,
            receipt: device(DeviceState::Creating).receipt,
        }],
        ..RecoveryJournal::clean(11)
    };
    let mut old = serde_json::to_value(&legacy).unwrap();
    old["schema_version"] = 2.into();
    old.as_object_mut().unwrap().remove("device");
    old.as_object_mut().unwrap().remove("device_binding");
    let original = serde_json::to_vec(&old).unwrap();
    fs::write(store.path(), &original).unwrap();
    let loaded = store.load_or_clean().unwrap();
    assert_eq!(loaded, legacy);
    assert!(loaded.device.is_none());
    assert_eq!(
        fs::read(store.path()).unwrap(),
        original,
        "read is not a migration write"
    );
}

#[test]
fn a_failed_idle_save_keeps_the_durable_device_recovery_record() {
    let directory = tempfile::tempdir().unwrap();
    let store = JournalStore::new(directory.path().join("recovery.json"));
    let mut journal = RecoveryJournal::clean(0);
    journal.device = Some(device(DeviceState::RecoveryRequired));
    store.save(&mut journal).unwrap();
    let persisted = store.load_or_clean().unwrap();
    let mut next = journal.clone();
    next.device.as_mut().unwrap().state = DeviceState::Idle;
    store.fail_next_clean_save();
    assert!(store.save(&mut next).is_err());
    assert_eq!(store.load_or_clean().unwrap(), persisted);
}

#[test]
fn device_and_connection_binding_cannot_hide_or_misidentify_resources() {
    let mut journal = RecoveryJournal::clean(0);
    let owned = device(DeviceState::Idle);
    journal.device = Some(owned.clone());
    journal.validate().unwrap();
    journal.device_binding = Some(DeviceBinding {
        device_id: owned.device_id,
        generation: owned.generation,
    });
    assert!(
        journal.validate().is_err(),
        "clean connection cannot retain a binding"
    );
    journal.device_binding = None;
    journal.device.as_mut().unwrap().state = DeviceState::InUse;
    assert!(
        journal.validate().is_err(),
        "in-use device needs a connection"
    );
    journal.device = Some(owned.clone());
    journal.device.as_mut().unwrap().owner_sid = "untrusted".into();
    assert!(journal.validate().is_err());
    journal.device = Some(owned);
    if let MutationReceipt::WintunAdapter { adapter_guid, .. } =
        &mut journal.device.as_mut().unwrap().receipt
    {
        *adapter_guid = Uuid::new_v4();
    }
    assert!(journal.validate().is_err());
}

#[test]
fn old_or_future_schema_cannot_smuggle_managed_device_state() {
    let directory = tempfile::tempdir().unwrap();
    let store = JournalStore::new(directory.path().join("recovery.json"));
    let mut journal = RecoveryJournal::clean(0);
    journal.device = Some(device(DeviceState::Idle));
    for schema in [2, JOURNAL_SCHEMA_VERSION + 1] {
        let mut value = serde_json::to_value(&journal).unwrap();
        value["schema_version"] = schema.into();
        fs::write(store.path(), serde_json::to_vec(&value).unwrap()).unwrap();
        assert!(store.load_or_clean().is_err());
    }
}
