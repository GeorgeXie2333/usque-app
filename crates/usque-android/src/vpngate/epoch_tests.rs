//! Synthetic owner/cache requests only; physical networking is disabled.
use super::*;

const SERVER: &str = "v1:b2dab72eea6ea72b04ea181b0f96f2476a94a94c62610d34984e3f2910f25f53";
const HASH: &str = "75e04c90d2663ef1f799522757713e4bedeb97b3db9129aa6ba3fece3d249828";

struct Cleanup;
impl Drop for Cleanup {
    fn drop(&mut self) {
        assert!(cancel_refresh());
        *CATALOGUE.get().unwrap().lock().unwrap() = None;
    }
}

fn fixture() -> (tempfile::TempDir, PathBuf, Cleanup) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("profiles-v2.json");
    ConfigStore::new(&path)
        .save(&usque_core::AppConfig::default())
        .unwrap();
    *CATALOGUE.get_or_init(|| Mutex::new(None)).lock().unwrap() = None;
    crate::network_settings::command(path.to_str().unwrap(), r#"{"command":"reset"}"#).unwrap();
    (directory, path, Cleanup)
}

fn request(epoch: impl ToString, action: &str, operation: &str) -> Value {
    json!({"command":"node", "operation_id":operation, "action":action,
        "server_id":SERVER,"config_sha256":HASH,"expected_source_epoch":epoch.to_string()})
}

fn prepared(path: &Path) -> PathBuf {
    path.parent()
        .unwrap()
        .join("vpngate/prepared")
        .join(format!("{}-{HASH}.json", &SERVER[3..]))
}

fn wait_for_job() {
    let done = CATALOGUE
        .get()
        .unwrap()
        .lock()
        .unwrap()
        .as_ref()
        .unwrap()
        .job
        .as_ref()
        .unwrap()
        .done
        .clone();
    let guard = done.0.lock().unwrap();
    let (guard, _) = done
        .1
        .wait_timeout_while(guard, Duration::from_secs(5), |finished| !*finished)
        .unwrap();
    assert!(*guard, "inert catalogue job did not finish");
}

fn cache_fixture(path: &Path) {
    // Hash-verified envelope with an intentionally fake CA; only local parsing
    // runs, never TLS. This avoids adding a test-only crypto/base64 dependency.
    let bytes = serde_json::to_vec(&json!({
        "schema_version":1,"source_csv_sha256":"0".repeat(64),"server_count":1,
        "servers":[{"id":SERVER,"hostname":"fixture","ip":"8.8.8.8",
            "country_code":"JP","country_name":"Japan","score":1,
            "ping_ms":null,"speed_bps":null,"num_vpn_sessions":1,
            "openvpn_config_base64":"Y2xpZW50CmRldiB0dW4KcHJvdG8gdGNwCnJlbW90ZSA4LjguOC44IDQ0MwpjaXBoZXIgQUVTLTEyOC1DQkMKYXV0aCBTSEExCjxjYT4KZml4dHVyZQo8L2NhPgo=",
            "openvpn_config_sha256":HASH,"openvpn_config_bytes":92}]
    })).unwrap();
    CatalogueStore::new(path.parent().unwrap())
        .save(&Catalogue::parse(&bytes).unwrap(), 1, RAW_URL)
        .unwrap();
}

fn prepare(path: &Path, epoch: impl ToString, operation: &str) {
    let request = parse_request(&request(epoch, "prepare", operation).to_string()).unwrap();
    command(
        path,
        request,
        Some(FetchContext {
            identity: None,
            protector: Arc::new(usque_transport::NoopSocketProtector),
            networks: None,
            allow_physical: false,
            refresher: None,
        }),
        GateStatus::default(),
    )
    .unwrap();
    wait_for_job();
    assert_eq!(
        CATALOGUE
            .get()
            .unwrap()
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .downloader
            .node_progress()
            .stage,
        "complete"
    );
    assert!(prepared(path).is_file());
}

#[test]
fn invalid_owner_is_rejected_before_controller_recovery_or_node_dispatch() {
    let _guard = crate::network_settings::TEST_STATE.lock().unwrap();
    let (_directory, path, _cleanup) = fixture();
    let reference = prepared(&path);
    std::fs::create_dir_all(reference.parent().unwrap()).unwrap();
    std::fs::write(&reference, b"new-owner-prepared-reference").unwrap();
    let stale = uuid::Uuid::new_v4();
    for action in ["prepare", "release", "cancel", "remove_favorite"] {
        let operation = uuid::Uuid::new_v4().to_string();
        let mut value = request(stale, action, &operation);
        // Even the general cancellation flag must not bypass epoch admission.
        value["cancel"] = json!(true);
        let parsed = parse_request(&value.to_string()).unwrap();
        assert_eq!(
            command(&path, parsed, None, GateStatus::default()).unwrap_err(),
            "VPN_GATE_OWNER_STALE"
        );
        assert!(CATALOGUE.get().unwrap().lock().unwrap().is_none());
        assert_eq!(
            std::fs::read(&reference).unwrap(),
            b"new-owner-prepared-reference"
        );
        value["expected_source_epoch"] = json!("invalid");
        assert!(
            matches!(parse_request(&value.to_string()), Err(error) if error=="VPN_GATE_REQUEST_INVALID")
        );
        assert!(reference.exists());
    }
}

#[test]
fn old_owner_release_cancel_and_prepare_leave_new_same_hash_draft_intact() {
    let _guard = crate::network_settings::TEST_STATE.lock().unwrap();
    let (_directory, path, _cleanup) = fixture();
    cache_fixture(&path);
    let old = crate::network_settings::scenario_snapshot()
        .unwrap()
        .source_epoch;
    prepare(&path, old, &uuid::Uuid::new_v4().to_string());
    crate::network_settings::command(path.to_str().unwrap(), r#"{"command":"reset"}"#).unwrap();
    let current = crate::network_settings::scenario_snapshot()
        .unwrap()
        .source_epoch;
    assert_ne!(current, old);
    let operation = uuid::Uuid::new_v4().to_string();
    prepare(&path, current, &operation);
    let before = std::fs::read(prepared(&path)).unwrap();
    for action in ["prepare", "release", "cancel"] {
        let parsed = parse_request(&request(old, action, &operation).to_string()).unwrap();
        assert_eq!(
            command(&path, parsed, None, GateStatus::default()).unwrap_err(),
            "VPN_GATE_OWNER_STALE"
        );
        assert_eq!(std::fs::read(prepared(&path)).unwrap(), before);
        assert_eq!(
            CATALOGUE
                .get()
                .unwrap()
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .downloader
                .node_progress()
                .stage,
            "complete"
        );
    }
    let parsed = parse_request(&request(current, "cancel", &operation).to_string()).unwrap();
    command(&path, parsed, None, GateStatus::default()).unwrap();
    assert!(!prepared(&path).exists());
    // The unbound global API remains supported and can release its own draft.
    prepare(&path, "", &uuid::Uuid::new_v4().to_string());
    let parsed = parse_request(&request("", "release", &operation).to_string()).unwrap();
    command(&path, parsed, None, GateStatus::default()).unwrap();
    assert!(!prepared(&path).exists());
}

#[test]
fn reset_joins_an_old_owner_worker_before_changing_its_epoch() {
    let _guard = crate::network_settings::TEST_STATE.lock().unwrap();
    let (_directory, path, _cleanup) = fixture();
    let before = crate::network_settings::scenario_snapshot()
        .unwrap()
        .source_epoch;
    let cancellation = CancellationToken::new();
    let worker_cancel = cancellation.clone();
    let done = Arc::new((Mutex::new(false), Condvar::new()));
    let worker_done = done.clone();
    let thread = std::thread::spawn(move || {
        while !worker_cancel.is_cancelled() {
            std::thread::park_timeout(Duration::from_millis(1));
        }
        *worker_done.0.lock().unwrap() = true;
        worker_done.1.notify_all();
    });
    *CATALOGUE.get().unwrap().lock().unwrap() = Some(Controller {
        path: path.clone(),
        downloader: Arc::new(DirectoryDownloader::new(CatalogueStore::new(
            path.parent().unwrap(),
        ))),
        job: Some(Job {
            cancellation: cancellation.clone(),
            done: done.clone(),
            thread: Some(thread),
        }),
    });
    crate::network_settings::command(path.to_str().unwrap(), r#"{"command":"reset"}"#).unwrap();
    assert!(cancellation.is_cancelled());
    assert!(*done.0.lock().unwrap());
    assert!(
        CATALOGUE
            .get()
            .unwrap()
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .job
            .is_none()
    );
    assert_ne!(
        crate::network_settings::scenario_snapshot()
            .unwrap()
            .source_epoch,
        before
    );
}
