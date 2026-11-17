//! Real durable store in a private temp directory; preferences stay in memory.
use super::*;
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    sync::{Arc, Mutex},
};
use usque_platform::macos_proxy::{Edit, Service, Value, Values};

#[derive(Clone)]
struct Preferences(Arc<Mutex<(Vec<Service>, bool)>>);
impl Backend for Preferences {
    fn transaction(&self, interactive: bool, edit: &mut Edit<'_>) -> Result<(), Error> {
        assert!(
            !interactive,
            "the daily Agent path must never request authorization"
        );
        let mut data = self.0.lock().unwrap();
        let patches = edit(&data.0)?;
        for patch in patches {
            let values = &mut data.0[0].values;
            assert_eq!(values.get(&patch.key), patch.before.as_ref());
            match patch.after {
                Some(value) => {
                    values.insert(patch.key, value);
                }
                None => {
                    values.remove(&patch.key);
                }
            }
            if data.1 {
                return Err(Error::RecoveryPending);
            }
        }
        if data.1 {
            return Err(Error::RecoveryPending);
        }
        Ok(())
    }
}
fn setup() -> (
    tempfile::TempDir,
    SystemProxy<Preferences>,
    Owner,
    Uuid,
    MacosProxyListener,
) {
    let dir = tempfile::tempdir().unwrap();
    fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o700)).unwrap();
    let mut store = SecureStore::for_test(dir.path().join("proxy.json")).unwrap();
    store.save_value(None).unwrap();
    let services = vec![Service {
        id: "Wi-Fi".into(),
        eligible: true,
        values: Values::new(),
        pac_fingerprint: "a".repeat(64),
    }];
    let proxy = SystemProxy {
        backend: Preferences(Arc::new(Mutex::new((services, false)))),
        store,
        connection: None,
    };
    let owner = Owner {
        uid: 501,
        pid: 20,
        pid_version: 3,
    };
    let request = MacosProxyListener {
        loopback_ip: "127.0.0.1".into(),
        port: 8080,
        lease: Uuid::new_v4().to_string(),
    };
    (dir, proxy, owner, Uuid::new_v4(), request)
}
#[test]
fn repeated_connections_restore_owned_fields_and_preserve_foreign_edits_without_prompting() {
    let (_dir, mut proxy, owner, connection, mut request) = setup();
    for host in ["127.0.0.1", "::1"] {
        request.loopback_ip = host.into();
        request.lease = Uuid::new_v4().to_string();
        proxy.acquire(owner, connection, request.clone()).unwrap();
        let values = &proxy.backend.0.lock().unwrap().0[0].values.clone();
        assert_eq!(values.get("HTTPSProxy"), Some(&Value::Text(host.into())));
        assert!(!proxy.pending());
        proxy
            .release(owner, connection, Some(lease_id(&request.lease).unwrap()))
            .unwrap();
        assert!(proxy.backend.0.lock().unwrap().0[0].values.is_empty());
    }
    for key in ["ProxyAutoConfigEnable", "ProxyAutoDiscoveryEnable"] {
        proxy.backend.0.lock().unwrap().0[0]
            .values
            .insert(key.into(), Value::Integer(1));
    }
    proxy.acquire(owner, connection, request.clone()).unwrap();
    proxy.backend.0.lock().unwrap().0[0].values.insert(
        "HTTPProxy".into(),
        Value::Text("third-party.example".into()),
    );
    proxy.lost(connection);
    let values = &proxy.backend.0.lock().unwrap().0[0].values.clone();
    assert_eq!(
        values.get("HTTPProxy"),
        Some(&Value::Text("third-party.example".into()))
    );
    assert!(!values.contains_key("HTTPSProxy"));
    for key in ["ProxyAutoConfigEnable", "ProxyAutoDiscoveryEnable"] {
        assert_eq!(values.get(key), Some(&Value::Integer(0)));
    }
    assert!(!proxy.pending());
}
#[test]
fn rejects_unbounded_listeners_and_other_user_connection_or_lease() {
    let (_dir, mut proxy, owner, connection, request) = setup();
    for (ip, port) in [
        ("192.0.2.1", 3128),
        ("localhost", 80),
        ("::1", 0),
        ("127.0.0.1", 65536),
    ] {
        let mut bad = request.clone();
        bad.loopback_ip = ip.into();
        bad.port = port;
        assert_eq!(
            proxy.acquire(owner, connection, bad),
            Err(Failure::Protocol)
        );
    }
    proxy.acquire(owner, connection, request.clone()).unwrap();
    let lease = Some(lease_id(&request.lease).unwrap());
    assert_eq!(
        proxy.release(Owner { uid: 502, ..owner }, connection, lease),
        Err(Failure::OwnershipConflict)
    );
    assert_eq!(
        proxy.release(owner, Uuid::new_v4(), lease),
        Err(Failure::Busy)
    );
    assert_eq!(
        proxy.release(owner, connection, Some(Uuid::new_v4())),
        Ok(())
    );
    assert_eq!(proxy.release(owner, connection, None), Err(Failure::Busy));
    assert_eq!(
        proxy.acquire(owner, Uuid::new_v4(), request),
        Err(Failure::Busy)
    );
    proxy.release(owner, connection, lease).unwrap();
    proxy.release(owner, connection, lease).unwrap();
}
#[test]
fn failed_apply_and_failed_rollback_survive_restart_for_owned_noninteractive_retry() {
    let (dir, mut proxy, owner, connection, request) = setup();
    let prefs = proxy.backend.clone();
    prefs.0.lock().unwrap().1 = true;
    assert_eq!(
        proxy.acquire(owner, connection, request),
        Err(Failure::RecoveryIncomplete)
    );
    assert!(proxy.pending());
    drop(proxy);
    let store = SecureStore::for_test(dir.path().join("proxy.json")).unwrap();
    let mut proxy = SystemProxy {
        store,
        backend: prefs.clone(),
        connection: None,
    };
    assert!(proxy.pending());
    assert_eq!(
        proxy.release(Owner { uid: 502, ..owner }, connection, None),
        Err(Failure::OwnershipConflict)
    );
    prefs.0.lock().unwrap().1 = false;
    proxy.release(owner, connection, None).unwrap();
    assert!(!proxy.pending());
    assert!(prefs.0.lock().unwrap().0[0].values.is_empty());
}
#[test]
fn corrupt_receipt_blocks_new_settings_and_keeps_evidence() {
    let (dir, mut proxy, owner, connection, request) = setup();
    let path = dir.path().join("proxy.json");
    fs::write(&path, b"broken").unwrap();
    assert!(proxy.pending());
    assert_eq!(
        proxy.acquire(owner, connection, request),
        Err(Failure::Journal)
    );
    assert_eq!(proxy.restore(), Err(Failure::Journal));
    assert_eq!(fs::read(path).unwrap(), b"broken");
}

#[test]
fn refuses_to_persist_a_proxy_url_with_inline_credentials() {
    let (dir, mut proxy, owner, connection, request) = setup();
    proxy.backend.0.lock().unwrap().0[0].values.insert(
        "HTTPProxy".into(),
        Value::Text("http://user:secret@proxy.test".into()),
    );
    assert!(proxy.acquire(owner, connection, request).is_err());
    assert!(!proxy.pending());
    assert!(
        !fs::read_to_string(dir.path().join("proxy.json"))
            .unwrap()
            .contains("secret")
    );
}
