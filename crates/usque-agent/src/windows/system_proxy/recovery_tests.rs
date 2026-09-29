use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
};

use super::*;

const TEST_SID: &str = "S-1-5-21-1000";

#[derive(Clone)]
enum Value {
    Dword(u32),
    Text(String),
}

#[derive(Default)]
struct MemorySettings {
    values: RefCell<HashMap<String, Value>>,
    calls: Cell<usize>,
    fail_at: Cell<Option<usize>>,
    notifications: Cell<usize>,
    events: RefCell<Vec<&'static str>>,
}

impl MemorySettings {
    fn new(snapshot: &ProxySnapshot) -> Self {
        let mut values = HashMap::new();
        for (name, value) in [
            (PROXY_ENABLE_VALUE, snapshot.proxy_enable.map(Value::Dword)),
            (PROXY_SERVER_VALUE, snapshot.proxy.clone().map(Value::Text)),
            (
                PROXY_OVERRIDE_VALUE,
                snapshot.bypass.clone().map(Value::Text),
            ),
            (
                AUTO_CONFIG_URL_VALUE,
                snapshot.auto_config_url.clone().map(Value::Text),
            ),
            (AUTO_DETECT_VALUE, snapshot.auto_detect.map(Value::Dword)),
        ] {
            if let Some(value) = value {
                values.insert(name.to_owned(), value);
            }
        }
        Self {
            values: RefCell::new(values),
            ..Self::default()
        }
    }

    fn attempt(&self, event: &'static str) -> Result<(), SystemProxyError> {
        let call = self.calls.get() + 1;
        self.calls.set(call);
        self.events.borrow_mut().push(event);
        if self.fail_at.get() == Some(call) {
            self.fail_at.set(None);
            Err(injected_error())
        } else {
            Ok(())
        }
    }

    fn reset_calls(&self) {
        self.calls.set(0);
        self.events.borrow_mut().clear();
    }

    fn notify(&self) -> Result<(), SystemProxyError> {
        self.notifications.set(self.notifications.get() + 1);
        self.attempt("notify")
    }

    fn external_text(&self, name: &str, text: &str) {
        self.values
            .borrow_mut()
            .insert(name.to_owned(), Value::Text(text.to_owned()));
    }

    fn owner_present(&self) -> bool {
        self.values.borrow().contains_key(OWNER_VALUE)
    }
}

impl InternetSettings for MemorySettings {
    fn read_dword(&self, name: &str) -> Result<Option<u32>, SystemProxyError> {
        self.attempt("read_dword")?;
        match self.values.borrow().get(name) {
            None => Ok(None),
            Some(Value::Dword(value)) => Ok(Some(*value)),
            Some(_) => Err(SystemProxyError::UnexpectedRegistryType(name.to_owned())),
        }
    }

    fn read_string(&self, name: &str) -> Result<Option<String>, SystemProxyError> {
        self.attempt("read_string")?;
        match self.values.borrow().get(name) {
            None => Ok(None),
            Some(Value::Text(value)) => Ok(Some(value.clone())),
            Some(_) => Err(SystemProxyError::UnexpectedRegistryType(name.to_owned())),
        }
    }

    fn write_dword(&self, name: &str, value: u32) -> Result<(), SystemProxyError> {
        self.attempt("write_dword")?;
        self.values
            .borrow_mut()
            .insert(name.to_owned(), Value::Dword(value));
        Ok(())
    }

    fn write_string(&self, name: &str, value: &str) -> Result<(), SystemProxyError> {
        self.attempt("write_string")?;
        self.external_text(name, value);
        Ok(())
    }

    fn delete_value(&self, name: &str) -> Result<(), SystemProxyError> {
        self.attempt(if name == OWNER_VALUE {
            "delete_owner"
        } else {
            "delete_value"
        })?;
        self.values.borrow_mut().remove(name);
        Ok(())
    }

    fn flush(&self) -> Result<(), SystemProxyError> {
        self.attempt("flush")
    }
}

fn injected_error() -> SystemProxyError {
    SystemProxyError::Windows {
        operation: "in-memory fault",
        code: 8,
    }
}

fn snapshots() -> [ProxySnapshot; 3] {
    [
        ProxySnapshot {
            proxy_enable: None,
            proxy: None,
            bypass: None,
            auto_config_url: None,
            auto_detect: None,
        },
        ProxySnapshot {
            proxy_enable: Some(0),
            proxy: Some("192.0.2.10:3128".to_owned()),
            bypass: Some("*.example.test".to_owned()),
            auto_config_url: Some("https://example.test/previous.pac".to_owned()),
            auto_detect: Some(1),
        },
        ProxySnapshot {
            proxy_enable: Some(1),
            proxy: Some("192.0.2.20:3128".to_owned()),
            bypass: Some("<local>".to_owned()),
            auto_config_url: None,
            auto_detect: Some(0),
        },
    ]
}

fn receipt(previous: &ProxySnapshot) -> MutationReceipt {
    MutationReceipt::SystemProxy {
        user_sid: TEST_SID.to_owned(),
        operation_id: Uuid::new_v4(),
        previous_proxy_enable: previous.proxy_enable,
        previous_proxy: previous.proxy.clone(),
        previous_bypass: previous.bypass.clone(),
        previous_auto_config_url: previous.auto_config_url.clone(),
        previous_auto_detect: previous.auto_detect,
        applied_proxy: "127.0.0.1:8080".to_owned(),
        applied_bypass: "localhost;127.*;<local>".to_owned(),
    }
}

fn apply_memory(
    key: &MemorySettings,
    receipt: MutationReceipt,
) -> Result<MutationReceipt, SystemProxyError> {
    apply_with(key, receipt, || key.notify())
}

fn restore_memory(key: &MemorySettings, receipt: &MutationReceipt) -> Result<(), SystemProxyError> {
    restore_with(key, receipt, || key.notify())
}

#[test]
fn apply_and_restore_preserve_enabled_disabled_and_missing_original_values() {
    for previous in snapshots() {
        let key = MemorySettings::new(&previous);
        let receipt = apply_memory(&key, receipt(&previous)).expect("apply");
        assert_eq!(key.read_dword(PROXY_ENABLE_VALUE).expect("enable"), Some(1));
        assert!(key.owner_present());
        restore_memory(&key, &receipt).expect("restore");
        assert_eq!(key.snapshot().expect("snapshot"), previous);
        assert!(!key.owner_present());
        assert_eq!(key.notifications.get(), 2);
    }
}

#[test]
fn replacement_manual_proxy_keeps_its_enable_bypass_and_automatic_settings() {
    let previous = snapshots()[1].clone();
    let key = MemorySettings::new(&previous);
    let receipt = apply_memory(&key, receipt(&previous)).expect("apply");
    key.external_text(PROXY_SERVER_VALUE, "192.0.2.30:8080");
    key.external_text(PROXY_OVERRIDE_VALUE, "*.replacement.test");
    let replacement = key.snapshot().expect("replacement");

    restore_memory(&key, &receipt).expect("restore after replacement");

    assert_eq!(key.snapshot().expect("snapshot"), replacement);
    assert_eq!(key.read_dword(PROXY_ENABLE_VALUE).expect("enable"), Some(1));
    assert_eq!(key.read_string(AUTO_CONFIG_URL_VALUE).expect("PAC"), None);
    assert!(!key.owner_present());
}

#[test]
fn editing_only_bypass_does_not_leave_a_stopped_usque_proxy_enabled() {
    let previous = snapshots()[1].clone();
    let key = MemorySettings::new(&previous);
    let receipt = apply_memory(&key, receipt(&previous)).expect("apply");
    key.external_text(PROXY_OVERRIDE_VALUE, "*.replacement.test");

    restore_memory(&key, &receipt).expect("restore");

    let current = key.snapshot().expect("snapshot");
    assert_eq!(current.proxy_enable, previous.proxy_enable);
    assert_eq!(current.proxy, previous.proxy);
    assert_eq!(current.auto_config_url, previous.auto_config_url);
    assert_eq!(current.bypass.as_deref(), Some("*.replacement.test"));
}

#[test]
fn every_apply_read_write_flush_and_notification_failure_can_be_recovered() {
    for previous in snapshots() {
        let baseline = MemorySettings::new(&previous);
        apply_memory(&baseline, receipt(&previous)).expect("baseline apply");
        let apply_calls = baseline.calls.get();
        for fail_at in 1..=apply_calls {
            let key = MemorySettings::new(&previous);
            let receipt = receipt(&previous);
            key.fail_at.set(Some(fail_at));
            assert!(
                apply_memory(&key, receipt.clone()).is_err(),
                "fault {fail_at}"
            );

            restore_memory(&key, &receipt).expect("recover interrupted apply");

            assert_eq!(
                key.snapshot().expect("snapshot"),
                previous,
                "fault {fail_at}"
            );
            assert!(!key.owner_present(), "fault {fail_at}");
        }
    }
}

#[test]
fn an_interrupted_apply_can_resume_with_the_same_receipt_and_owner() {
    let previous = snapshots()[1].clone();
    let baseline = MemorySettings::new(&previous);
    apply_memory(&baseline, receipt(&previous)).expect("baseline apply");
    let apply_calls = baseline.calls.get();
    for fail_at in 1..=apply_calls {
        let key = MemorySettings::new(&previous);
        let receipt = receipt(&previous);
        key.fail_at.set(Some(fail_at));
        assert!(
            apply_memory(&key, receipt.clone()).is_err(),
            "fault {fail_at}"
        );
        apply_memory(&key, receipt.clone()).expect("resume interrupted apply");
        assert_eq!(key.read_dword(PROXY_ENABLE_VALUE).expect("enable"), Some(1));
        restore_memory(&key, &receipt).expect("restore resumed apply");
        assert_eq!(
            key.snapshot().expect("snapshot"),
            previous,
            "fault {fail_at}"
        );
    }
}

#[test]
fn every_restore_read_write_flush_and_notification_failure_can_be_retried() {
    for previous in snapshots() {
        let baseline = MemorySettings::new(&previous);
        let baseline_receipt = apply_memory(&baseline, receipt(&previous)).expect("apply");
        baseline.reset_calls();
        restore_memory(&baseline, &baseline_receipt).expect("baseline restore");
        let restore_calls = baseline.calls.get();
        for fail_at in 1..=restore_calls {
            let key = MemorySettings::new(&previous);
            let receipt = apply_memory(&key, receipt(&previous)).expect("apply");
            key.reset_calls();
            key.fail_at.set(Some(fail_at));
            assert!(restore_memory(&key, &receipt).is_err(), "fault {fail_at}");

            restore_memory(&key, &receipt).expect("retry interrupted restore");

            assert_eq!(
                key.snapshot().expect("snapshot"),
                previous,
                "fault {fail_at}"
            );
            assert!(!key.owner_present(), "fault {fail_at}");
        }
    }
}

#[test]
fn failed_notifications_keep_ownership_and_never_become_false_success() {
    let previous = snapshots()[1].clone();
    let key = MemorySettings::new(&previous);
    let receipt = apply_memory(&key, receipt(&previous)).expect("apply");
    for _ in 0..3 {
        assert!(restore_with(&key, &receipt, || Err(injected_error())).is_err());
        assert!(key.owner_present());
        assert_eq!(key.snapshot().expect("snapshot"), previous);
    }
    restore_memory(&key, &receipt).expect("notification succeeds on retry");
    assert!(!key.owner_present());
}

#[test]
fn absent_owner_retries_both_flush_and_notification_without_writing_settings() {
    let previous = snapshots()[1].clone();
    let key = MemorySettings::new(&previous);
    let receipt = receipt(&previous);
    for fail_at in [2, 3] {
        key.reset_calls();
        key.fail_at.set(Some(fail_at));
        assert!(restore_memory(&key, &receipt).is_err());
        assert!(!key.owner_present());
    }
    key.reset_calls();
    restore_memory(&key, &receipt).expect("retry absent owner");
    assert_eq!(&*key.events.borrow(), &["read_string", "flush", "notify"]);
    assert_eq!(key.snapshot().expect("snapshot"), previous);
}

#[test]
fn stale_snapshot_and_other_owners_do_not_write_proxy_settings() {
    let previous = snapshots()[1].clone();
    let key = MemorySettings::new(&previous);
    let receipt = receipt(&previous);
    key.external_text(PROXY_SERVER_VALUE, "192.0.2.30:8080");
    let replacement = key.snapshot().expect("replacement");
    assert!(matches!(
        apply_memory(&key, receipt.clone()),
        Err(SystemProxyError::SnapshotChanged)
    ));
    assert!(!key.owner_present());
    assert_eq!(key.snapshot().expect("snapshot"), replacement);

    key.external_text(OWNER_VALUE, &Uuid::new_v4().to_string());
    assert!(matches!(
        apply_memory(&key, receipt.clone()),
        Err(SystemProxyError::AlreadyOwned)
    ));
    assert!(matches!(
        restore_memory(&key, &receipt),
        Err(SystemProxyError::OwnerChanged)
    ));
    assert_eq!(key.snapshot().expect("snapshot"), replacement);
    assert_eq!(key.notifications.get(), 0);
}

#[test]
fn malformed_receipts_are_rejected_before_any_registry_or_notification_operation() {
    let previous = snapshots()[0].clone();
    let key = MemorySettings::new(&previous);
    let mut malformed = receipt(&previous);
    if let MutationReceipt::SystemProxy { operation_id, .. } = &mut malformed {
        *operation_id = Uuid::nil();
    }
    assert!(matches!(
        apply_memory(&key, malformed.clone()),
        Err(SystemProxyError::InvalidReceipt)
    ));
    assert!(matches!(
        restore_memory(&key, &malformed),
        Err(SystemProxyError::InvalidReceipt)
    ));
    assert_eq!(key.calls.get(), 0);
}

fn key_name_information(path: &str) -> Vec<u8> {
    let name = path
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect::<Vec<_>>();
    let mut bytes = (name.len() as u32).to_le_bytes().to_vec();
    bytes.extend(name);
    bytes
}

fn query_reply(bytes: &[u8]) -> Result<String, SystemProxyError> {
    query_registry_key_path_with(|buffer, required| {
        *required = bytes.len() as u32;
        if buffer.is_empty() {
            return 0xc000_0023_u32 as i32;
        }
        assert_eq!(buffer.as_ptr() as usize % size_of::<u32>(), 0);
        buffer[..bytes.len()].copy_from_slice(bytes);
        0
    })
}

#[test]
fn resolved_key_identity_rejects_parent_links_cross_sid_and_machine_targets() {
    let expected = format!(r"\REGISTRY\USER\{TEST_SID}\{INTERNET_SETTINGS_PATH}");
    let queried = query_reply(&key_name_information(&expected.to_ascii_uppercase())).expect("name");
    verify_registry_key_path(TEST_SID, &queried).expect("resolved correct key");
    for target in [
        format!(r"\REGISTRY\USER\S-1-5-21-2000\{INTERNET_SETTINGS_PATH}"),
        format!(r"\REGISTRY\MACHINE\{INTERNET_SETTINGS_PATH}"),
        format!(r"\REGISTRY\USER\{TEST_SID}_Classes\{INTERNET_SETTINGS_PATH}"),
        format!(r"\REGISTRY\USER\{TEST_SID}\Redirected\{INTERNET_SETTINGS_PATH}"),
        format!(r"{expected}\child"),
        format!("{expected}\0"),
    ] {
        let queried = query_reply(&key_name_information(&target)).expect("queried target");
        let error =
            verify_registry_key_path(TEST_SID, &queried).expect_err("reject redirected handle");
        assert!(matches!(error, SystemProxyError::RegistryKeyIdentity));
        assert!(!error.to_string().contains(&target));
    }
}

#[test]
fn identity_verification_uses_the_returned_handle_name_after_the_size_query() {
    let requested = key_name_information(&format!(
        r"\REGISTRY\USER\{TEST_SID}\{INTERNET_SETTINGS_PATH}"
    ));
    let redirected = key_name_information(&format!(
        r"\REGISTRY\USER\S-1-5-21-2000\{INTERNET_SETTINGS_PATH}"
    ));
    assert_eq!(requested.len(), redirected.len());
    let resolved = query_registry_key_path_with(|buffer, required| {
        *required = requested.len() as u32;
        if buffer.is_empty() {
            return 0xc000_0023_u32 as i32;
        }
        buffer.copy_from_slice(&redirected);
        0
    })
    .expect("resolved name");
    assert!(matches!(
        verify_registry_key_path(TEST_SID, &resolved),
        Err(SystemProxyError::RegistryKeyIdentity)
    ));
}

#[test]
fn query_failures_and_malformed_native_names_fail_closed() {
    assert!(
        query_registry_key_path_with(|_, required| {
            *required = 32;
            -1
        })
        .is_err()
    );
    assert!(
        query_registry_key_path_with(|_, required| {
            *required = MAX_REGISTRY_VALUE_BYTES + 1;
            0xc000_0023_u32 as i32
        })
        .is_err()
    );
    for malformed in [
        [3_u32.to_le_bytes().as_slice(), &[b'A', 0, 0]].concat(),
        100_u32.to_le_bytes().to_vec(),
        [2_u32.to_le_bytes().as_slice(), &0xd800_u16.to_le_bytes()].concat(),
    ] {
        assert!(query_reply(&malformed).is_err());
    }
    let mut calls = 0;
    assert!(
        query_registry_key_path_with(|_, required| {
            calls += 1;
            *required = if calls == 1 { 16 } else { 32 };
            if calls == 1 {
                0xc000_0023_u32 as i32
            } else {
                0
            }
        })
        .is_err()
    );
}
