//! Every preferences operation here is in-memory; Keychain is replaced by the
//! Engine test vault. Nothing calls the native authorization/preferences APIs.
use super::*;
use sha2::{Digest, Sha256};
#[derive(Default)]
struct FakeState {
    services: Vec<Service>,
    cancel_once: bool,
    fail_after: Option<usize>,
    fail_restore: bool,
    fail_apply_once: bool,
    active_values: Vec<Values>,
    apply_calls: usize,
    mutations: usize,
    interactive_requests: usize,
}
struct FakeBackend {
    state: Mutex<FakeState>,
    journal_path: PathBuf,
}
impl FakeBackend {
    fn new(path: &Path) -> Self {
        Self {
            journal_path: path.into(),
            state: Mutex::new(FakeState {
                services: vec![
                    Service {
                        id: "physical-service".into(),
                        eligible: true,
                        pac_fingerprint: String::new(),
                        values: BTreeMap::from([
                            (
                                "HTTPProxy".into(),
                                Value::Text("prior-proxy.example".into()),
                            ),
                            ("HTTPPort".into(), Value::Integer(3128)),
                            ("HTTPEnable".into(), Value::Integer(1)),
                            ("ProxyAutoConfigEnable".into(), Value::Boolean(true)),
                            (
                                "ProxyAutoConfigURLString".into(),
                                Value::Text("https://pac.example/private-script".into()),
                            ),
                            ("SOCKSProxy".into(), Value::Text("socks.example".into())),
                            (
                                "HTTPPassword".into(),
                                Value::Text("synthetic-original-password".into()),
                            ),
                        ]),
                    },
                    Service {
                        id: "vpn-service".into(),
                        eligible: false,
                        pac_fingerprint: String::new(),
                        values: Values::new(),
                    },
                ],
                ..Default::default()
            }),
        }
    }
}
impl Backend for FakeBackend {
    fn transaction(&self, interactive: bool, edit: &mut Edit<'_>) -> Result<()> {
        let mut state = self.state.lock().unwrap();
        if interactive {
            state.interactive_requests += 1;
        }
        if state.cancel_once {
            state.cancel_once = false;
            return Err(Error::PermissionCancelled);
        }
        if !interactive && state.fail_restore {
            return Err(Error::PermissionRequired);
        }
        // Simulate the native boundary exposing only a PAC fingerprint.
        for service in &mut state.services {
            let mut hash = Sha256::new();
            for key in ["ProxyAutoConfigURLString", "ProxyAutoConfigJavaScript"] {
                hash.update(key.as_bytes());
                hash.update(serde_json::to_vec(&service.values.get(key)).unwrap());
            }
            service.pac_fingerprint = hash
                .finalize()
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect();
        }
        let patches = edit(&state.services)?;
        for (index, patch) in patches.into_iter().enumerate() {
            assert!(
                self.journal_path.exists(),
                "every mutation needs prior durable intent"
            );
            if state.fail_after == Some(index) {
                state.fail_after = None;
                return Err(Error::Unavailable);
            }
            let service = state
                .services
                .iter_mut()
                .find(|service| service.id == patch.service)
                .unwrap();
            assert_eq!(service.values.get(&patch.key), patch.before.as_ref());
            match patch.after {
                Some(value) => {
                    service.values.insert(patch.key, value);
                }
                None => {
                    service.values.remove(&patch.key);
                }
            }
            state.mutations += 1;
        }
        state.apply_calls += 1;
        if state.fail_apply_once {
            state.fail_apply_once = false;
            return Err(Error::RecoveryPending);
        }
        state.active_values = state
            .services
            .iter()
            .map(|service| service.values.clone())
            .collect();
        Ok(())
    }
}
fn fixture() -> (
    tempfile::TempDir,
    PathBuf,
    Arc<dyn ProfileCipher>,
    Arc<FakeBackend>,
) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("system-proxy.sealed");
    let cipher: Arc<dyn ProfileCipher> = Arc::new(crate::desktop_chain::DesktopProfileCipher::new(
        Arc::new(crate::tests::MemoryVault::default()),
    ));
    let backend = Arc::new(FakeBackend::new(&path));
    (directory, path, cipher, backend)
}
fn listener() -> SocketAddr {
    "127.0.0.1:8080".parse().unwrap()
}

#[test]
fn owned_restore_preserves_external_edits_pac_socks_credentials_and_other_services() {
    let (_directory, path, cipher, backend) = fixture();
    let original = backend.state.lock().unwrap().services.clone();
    let mut transaction =
        Transaction::start(path.clone(), cipher, backend.clone(), listener()).unwrap();
    assert_eq!(fs::metadata(&path).unwrap().mode() & 0o777, 0o600);
    let encrypted = fs::read(&path).unwrap();
    for secret in [
        "prior-proxy.example",
        "synthetic-original-password",
        "private-script",
    ] {
        assert!(
            !encrypted
                .windows(secret.len())
                .any(|bytes| bytes == secret.as_bytes())
        );
    }
    {
        let mut state = backend.state.lock().unwrap();
        assert_eq!(
            state.services[0].values.get("ProxyAutoConfigEnable"),
            Some(&Value::Integer(0))
        );
        assert_eq!(state.services[1].values, original[1].values);
        state.services[0].values.insert(
            "HTTPSProxy".into(),
            Value::Text("later-user-edit.example".into()),
        );
        state.services[0].values.insert(
            "ProxyAutoConfigURLString".into(),
            Value::Text("https://pac.example/later-script".into()),
        );
    }
    transaction.restore(false).unwrap();
    assert!(!path.exists());
    let state = backend.state.lock().unwrap();
    assert_eq!(
        state.services[0].values.get("HTTPProxy"),
        original[0].values.get("HTTPProxy")
    );
    assert_eq!(
        state.services[0].values.get("HTTPSProxy"),
        Some(&Value::Text("later-user-edit.example".into()))
    );
    assert_eq!(
        state.services[0].values.get("ProxyAutoConfigURLString"),
        Some(&Value::Text("https://pac.example/later-script".into()))
    );
    assert_eq!(
        state.services[0].values.get("ProxyAutoConfigEnable"),
        Some(&Value::Integer(0))
    );
    assert_eq!(
        state.services[0].values.get("HTTPSPort"),
        Some(&Value::Integer(8080))
    );
    assert_eq!(
        state.services[0].values.get("HTTPSEnable"),
        Some(&Value::Integer(1))
    );
    for key in ["SOCKSProxy", "HTTPPassword"] {
        assert_eq!(
            state.services[0].values.get(key),
            original[0].values.get(key)
        );
    }
}

#[test]
fn each_partial_apply_failure_restores_its_successful_prefix() {
    let (_directory, path, cipher, backend) = fixture();
    let mut applied = Transaction::start(path, cipher, backend.clone(), listener()).unwrap();
    let stages = backend.state.lock().unwrap().mutations;
    applied.restore(false).unwrap();
    assert!(stages > 0);
    for failure in 0..stages {
        let (_directory, path, cipher, backend) = fixture();
        let original = backend.state.lock().unwrap().services[0].values.clone();
        backend.state.lock().unwrap().fail_after = Some(failure);
        assert!(Transaction::start(path.clone(), cipher, backend.clone(), listener()).is_err());
        assert_eq!(backend.state.lock().unwrap().services[0].values, original);
        assert!(!path.exists());
    }
}

#[test]
fn permission_cancellation_changes_nothing_and_is_retryable() {
    let (_directory, path, cipher, backend) = fixture();
    backend.state.lock().unwrap().cancel_once = true;
    assert_eq!(
        Transaction::start(path.clone(), cipher.clone(), backend.clone(), listener()).err(),
        Some(Error::PermissionCancelled)
    );
    assert_eq!(backend.state.lock().unwrap().mutations, 0);
    assert!(!path.exists());
    let transaction = Transaction::start(path.clone(), cipher, backend, listener()).unwrap();
    drop(transaction);
    assert!(!path.exists());
    let error = crate::ControlServiceError::MacSystemProxy(Error::PermissionCancelled)
        .as_structured_error();
    assert_eq!(error.code, "SYSTEM_PROXY_PERMISSION_CANCELLED");
    assert!(error.retryable);
}

#[test]
fn failed_restore_retains_encrypted_journal_for_silent_startup_retry() {
    let (_directory, path, cipher, backend) = fixture();
    let original = backend.state.lock().unwrap().services[0].values.clone();
    let mut transaction =
        Transaction::start(path.clone(), cipher.clone(), backend.clone(), listener()).unwrap();
    backend.state.lock().unwrap().fail_restore = true;
    assert_eq!(transaction.restore(false), Err(Error::PermissionRequired));
    assert!(path.exists());
    drop(transaction);
    backend.state.lock().unwrap().fail_restore = false;
    let prompts = backend.state.lock().unwrap().interactive_requests;
    let mut recovered = Transaction::open(path.clone(), cipher, backend.clone()).unwrap();
    recovered.restore(false).unwrap();
    assert!(!path.exists());
    let state = backend.state.lock().unwrap();
    assert_eq!(state.interactive_requests, prompts);
    assert_eq!(state.services[0].values, original);
}

#[test]
fn a_second_engine_cannot_take_over_proxy_recovery() {
    let (_directory, path, cipher, backend) = fixture();
    let transaction =
        Transaction::start(path.clone(), cipher.clone(), backend.clone(), listener()).unwrap();
    assert_eq!(
        Transaction::open(path.clone(), cipher, backend).err(),
        Some(Error::Conflict)
    );
    assert!(path.exists());
    drop(transaction);
    assert!(!path.exists());
}

#[test]
fn invalid_recovery_records_never_modify_preferences() {
    let (_directory, path, cipher, backend) = fixture();
    let journal = Journal {
        version: 2,
        observations: Vec::new(),
        pac_fingerprints: BTreeMap::new(),
        changes: vec![Change {
            service: "physical-service".into(),
            key: "HTTPPassword".into(),
            before: None,
            applied: Value::Text("never-restored".into()),
        }],
    };
    let invalid = cipher
        .seal(JOURNAL_ID, &serde_json::to_vec(&journal).unwrap())
        .unwrap();
    fs::write(&path, invalid).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    assert_eq!(
        Transaction::open(path.clone(), cipher, backend.clone()).err(),
        Some(Error::RecoveryPending)
    );
    assert_eq!(backend.state.lock().unwrap().mutations, 0);
    assert!(path.exists());
}

#[test]
fn restore_retries_active_apply_even_after_persistent_keys_are_already_restored() {
    let (_directory, path, cipher, backend) = fixture();
    let original: Vec<_> = backend
        .state
        .lock()
        .unwrap()
        .services
        .iter()
        .map(|service| service.values.clone())
        .collect();
    let mut transaction =
        Transaction::start(path.clone(), cipher, backend.clone(), listener()).unwrap();
    let applied = backend.state.lock().unwrap().active_values.clone();
    assert_ne!(applied, original);
    backend.state.lock().unwrap().fail_apply_once = true;
    assert_eq!(transaction.restore(false), Err(Error::RecoveryPending));
    assert!(path.exists());
    let committed_mutations = {
        let state = backend.state.lock().unwrap();
        assert_eq!(
            state
                .services
                .iter()
                .map(|service| service.values.clone())
                .collect::<Vec<_>>(),
            original
        );
        assert_eq!(state.active_values, applied);
        state.mutations
    };
    transaction.restore(false).unwrap();
    assert!(!path.exists());
    let state = backend.state.lock().unwrap();
    assert_eq!(
        state.mutations, committed_mutations,
        "retry needs no additional key mutations"
    );
    assert_eq!(state.active_values, original);
    assert_eq!(state.apply_calls, 3);
}

#[tokio::test]
async fn corrupt_recovery_is_visible_over_ipc_and_blocks_start_and_key_purge() {
    use usque_ipc::v1::{self, control_request, control_response};
    let directory = tempfile::tempdir().unwrap();
    let vault = Arc::new(crate::tests::MemoryVault::default());
    let service = crate::ControlService::open_with_vault(
        usque_core::storage::ConfigStore::new(directory.path().join("config.json")),
        vault.clone(),
    )
    .unwrap();
    let journal_path = directory.path().join("macos-system-proxy-v1.sealed");
    // An invalid envelope is rejected before any Keychain or native preference
    // operation; this test cannot request a host authorization dialog.
    fs::write(&journal_path, b"invalid synthetic encrypted journal").unwrap();
    fs::set_permissions(&journal_path, fs::Permissions::from_mode(0o600)).unwrap();
    assert!(service.recover_macos_user_state().await.is_err());
    let profile_id = service.config_snapshot().await.active_profile_id.unwrap();
    let request = |payload| v1::ControlRequest {
        request_id: "recovery-status".into(),
        payload: Some(payload),
    };
    let status = service
        .handle(request(control_request::Payload::GetStatus(
            v1::GetStatusRequest {},
        )))
        .await;
    let Some(control_response::Payload::Status(status)) = status.payload else {
        panic!("status");
    };
    assert_eq!(status.phase, v1::ConnectionPhase::Error as i32);
    assert!(status.frontends.iter().any(|frontend| frontend.kind
        == v1::FrontendKind::SystemProxy as i32
        && frontend.phase == v1::FrontendPhase::Error as i32));
    let start = service
        .handle(request(control_request::Payload::Connect(
            v1::ConnectRequest {
                profile_id: profile_id.to_string(),
                continuous: true,
            },
        )))
        .await;
    assert_eq!(start.error.unwrap().code, "SYSTEM_PROXY_RECOVERY_PENDING");
    assert!(service.data_plane.lock().await.is_none());
    assert!(service.continuous_connection.lock().await.is_none());
    assert!(service.clear_all_data(true).await.is_err());
    assert!(journal_path.exists());
    assert_eq!(
        service.config_snapshot().await.active_profile_id,
        Some(profile_id)
    );
    // Removing the external corruption represents successful external repair;
    // a later retry clears only the matching recovery error.
    fs::remove_file(journal_path).unwrap();
    service.recover_macos_user_state().await.unwrap();
    assert!(service.macos_proxy_recovery_error.lock().await.is_none());
    assert_eq!(
        service.status_snapshot().await.phase,
        usque_core::ConnectionPhase::Disconnected
    );
}

#[test]
fn a_foreign_proxy_endpoint_preserves_its_entire_protocol_group() {
    for (enable, host, port) in [
        ("HTTPEnable", "HTTPProxy", "HTTPPort"),
        ("HTTPSEnable", "HTTPSProxy", "HTTPSPort"),
    ] {
        let (_directory, path, cipher, backend) = fixture();
        {
            let mut state = backend.state.lock().unwrap();
            state.services[0]
                .values
                .insert(enable.into(), Value::Integer(0));
            // This unchanged member must still be observed in the journal.
            state.services[0]
                .values
                .insert(port.into(), Value::Integer(8080));
        }
        let mut transaction =
            Transaction::start(path, cipher, backend.clone(), listener()).unwrap();
        let journal = transaction.journal.as_ref().unwrap();
        assert!(
            journal
                .observations
                .iter()
                .any(|observation| observation.key == port)
        );
        assert!(!journal.changes.iter().any(|change| change.key == port));
        {
            let mut state = backend.state.lock().unwrap();
            state.services[0]
                .values
                .insert(host.into(), Value::Text("foreign-proxy.example".into()));
            state.services[0]
                .values
                .insert(port.into(), Value::Integer(9090));
        }
        transaction.restore(false).unwrap();
        let state = backend.state.lock().unwrap();
        assert_eq!(
            state.services[0].values.get(enable),
            Some(&Value::Integer(1)),
            "foreign {host} must remain enabled"
        );
        assert_eq!(
            state.services[0].values.get(host),
            Some(&Value::Text("foreign-proxy.example".into()))
        );
        assert_eq!(
            state.services[0].values.get(port),
            Some(&Value::Integer(9090))
        );
    }
}

#[test]
fn changed_pac_url_or_script_never_gets_enabled_by_owned_restore() {
    for key in ["ProxyAutoConfigURLString", "ProxyAutoConfigJavaScript"] {
        let (_directory, path, cipher, backend) = fixture();
        let mut transaction =
            Transaction::start(path, cipher, backend.clone(), listener()).unwrap();
        backend.state.lock().unwrap().services[0]
            .values
            .insert(key.into(), Value::Text("foreign PAC content".into()));
        transaction.restore(false).unwrap();
        let state = backend.state.lock().unwrap();
        assert_eq!(
            state.services[0].values.get("ProxyAutoConfigEnable"),
            Some(&Value::Integer(0))
        );
        assert_eq!(
            state.services[0].values.get(key),
            Some(&Value::Text("foreign PAC content".into()))
        );
    }
}
