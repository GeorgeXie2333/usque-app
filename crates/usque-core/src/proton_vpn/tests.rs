use super::*;
use crate::chain_exit::store::{ProfileCipher, clear_library};
use ring::aead::{self, Aad, LessSafeKey, Nonce, UnboundKey};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

#[test]
fn lifecycle_lease_child_process_probe() {
    let Some(parent) = std::env::var_os("USQUE_PROTON_LEASE_TEST_DIR") else {
        return;
    };
    let result = lifecycle_lease(Path::new(&parent));
    if std::env::var_os("USQUE_PROTON_LEASE_TEST_BUSY").is_some() {
        assert_eq!(result.unwrap_err().reason, "proton_operation_busy");
    } else {
        assert!(result.is_ok());
    }
}

#[test]
fn a_job_lease_excludes_another_native_process_until_its_last_owner_exits() {
    let directory = tempfile::tempdir().unwrap();
    let caller = std::sync::Arc::new(lifecycle_lease(directory.path()).unwrap());
    let job = caller.clone();
    drop(caller);
    let probe = |busy| {
        let mut command = std::process::Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "proton_vpn::tests::lifecycle_lease_child_process_probe",
                "--nocapture",
            ])
            .env("USQUE_PROTON_LEASE_TEST_DIR", directory.path());
        if busy {
            command.env("USQUE_PROTON_LEASE_TEST_BUSY", "1");
        }
        let output = command.output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    };
    probe(true);
    clear_library(directory.path()).unwrap();
    probe(true);
    drop(job);
    probe(false);
}

// Synthetic local AEAD key. This fixture never represents a platform vault,
// account credential, certificate or an actual Proton session.
struct TestCipher {
    key: LessSafeKey,
    sequence: AtomicU64,
    fail_seal: AtomicBool,
}
impl TestCipher {
    fn new() -> Self {
        Self {
            key: LessSafeKey::new(UnboundKey::new(&aead::AES_256_GCM, &[0x43; 32]).unwrap()),
            sequence: AtomicU64::new(1),
            fail_seal: AtomicBool::new(false),
        }
    }
}
impl ProfileCipher for TestCipher {
    fn seal(&self, id: Uuid, value: &[u8]) -> Result<Vec<u8>, ImportError> {
        if self.fail_seal.load(Ordering::Relaxed) {
            return Err(error("synthetic_cipher_failure"));
        }
        let mut nonce = [0u8; 12];
        nonce[4..].copy_from_slice(&self.sequence.fetch_add(1, Ordering::Relaxed).to_be_bytes());
        let mut sealed = value.to_vec();
        self.key
            .seal_in_place_append_tag(
                Nonce::assume_unique_for_key(nonce),
                Aad::from(id.as_bytes()),
                &mut sealed,
            )
            .map_err(|_| error("synthetic_cipher_failure"))?;
        let mut result = nonce.to_vec();
        result.extend(sealed);
        Ok(result)
    }
    fn open(&self, id: Uuid, value: &[u8]) -> Result<Zeroizing<Vec<u8>>, ImportError> {
        if value.len() < 28 {
            return Err(error("synthetic_cipher_failure"));
        }
        let nonce: [u8; 12] = value[..12].try_into().unwrap();
        let mut result = Zeroizing::new(value[12..].to_vec());
        let size = self
            .key
            .open_in_place(
                Nonce::assume_unique_for_key(nonce),
                Aad::from(id.as_bytes()),
                &mut result,
            )
            .map_err(|_| error("synthetic_cipher_failure"))?
            .len();
        result.truncate(size);
        Ok(result)
    }
}
fn record() -> Record {
    Record {
        api_identity: Default::default(),
        version: 1,
        id: Uuid::new_v4(),
        revision: Uuid::new_v4(),
        name: "Synthetic account".into(),
        identity: "guest".into(),
        session: SessionSecrets {
            uid: "synthetic-sensitive-uid".into(),
            access_token: "synthetic-sensitive-access".into(),
            refresh_token: "synthetic-sensitive-refresh".into(),
            user_id: "synthetic-sensitive-user".into(),
        },
        ed25519_seed: Zeroizing::new(vec![0x47; 32]),
        certificate_chain: Zeroizing::new("synthetic-sensitive-certificate".into()),
        expires_at: 2_000_003_600,
        refresh_at: 2_000_001_800,
        max_tier: 0,
        node: SelectedNode {
            metadata: Node {
                id: "node-one".into(),
                name: "Node one".into(),
                country: "CH".into(),
                city: "Geneva".into(),
                load: 20,
            },
            domain: "node.example.test".into(),
            endpoint: Endpoint::parse("192.0.2.1", "51820", 0).unwrap(),
            public_key: "R0dHR0dHR0dHR0dHR0dHR0dHR0dHR0dHR0dHR0dHR0c=".into(),
            label: None,
        },
        rotation_pending: false,
        server_id: None,
        server_catalog: Vec::new(),
        retired_sessions: Vec::new(),
        catalog_revision: Uuid::nil(),
        tunnel_seed: None,
        key_recovery_at: 0,
    }
}
fn path(parent: &Path, id: Uuid) -> PathBuf {
    parent.join("proton-vpn").join(format!("{id}.sealed"))
}

#[test]
fn legacy_sessions_keep_default_headers_and_invalid_header_values_are_rejected() {
    let mut value = serde_json::to_value(record()).unwrap();
    value.as_object_mut().unwrap().remove("api_identity");
    let restored: Record = serde_json::from_value(value).unwrap();
    restored.validate().unwrap();
    assert_eq!(restored.api_identity.app_version, APP_VERSION);
    for identity in [
        ApiIdentity {
            app_version: "bad\r\nInjected: value".into(),
            ..Default::default()
        },
        ApiIdentity {
            user_agent: "bad\nagent".into(),
            ..Default::default()
        },
        ApiIdentity {
            locale: "ja\0".into(),
            ..Default::default()
        },
    ] {
        assert!(identity.validate().is_err());
    }
}
fn assert_private(text: &str) {
    for value in [
        "synthetic-sensitive",
        "sensitive-name",
        "sensitive-node",
        "sensitive-key",
        "sensitive-host",
    ] {
        assert!(
            !text.contains(value),
            "secret appeared in ordinary metadata or Debug"
        );
    }
}

#[test]
fn proton_vpn_store_round_trip_never_writes_plaintext() {
    let parent = tempfile::tempdir().unwrap();
    let cipher = TestCipher::new();
    let store = store::SessionStore::new(parent.path(), &cipher);
    let record = record();
    record.validate().unwrap();
    store.save(&record).unwrap();
    let loaded = store.load(record.id).unwrap();
    assert_eq!(loaded.session.access_token, record.session.access_token);
    assert_eq!(
        loaded.ed25519_seed.as_slice(),
        record.ed25519_seed.as_slice()
    );
    assert_eq!(store.list().unwrap().len(), 1);
    let plaintext = serde_json::to_vec(&record).unwrap();
    let encrypted = fs::read(path(parent.path(), record.id)).unwrap();
    assert_ne!(plaintext, encrypted);
    for entry in fs::read_dir(parent.path().join("proton-vpn")).unwrap() {
        let bytes = fs::read(entry.unwrap().path()).unwrap();
        assert!(
            !bytes
                .windows(b"synthetic-sensitive".len())
                .any(|v| v == b"synthetic-sensitive")
        );
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(parent.path().join("proton-vpn"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(path(parent.path(), record.id))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
}

#[test]
fn proton_vpn_store_rejects_ciphertext_rebound_to_another_id() {
    let parent = tempfile::tempdir().unwrap();
    let cipher = TestCipher::new();
    let store = store::SessionStore::new(parent.path(), &cipher);
    let record = record();
    store.save(&record).unwrap();
    let other = Uuid::new_v4();
    fs::copy(path(parent.path(), record.id), path(parent.path(), other)).unwrap();
    assert!(store.load(other).is_err());
    // Even an authenticated plaintext has to contain its requested record ID.
    fs::write(
        path(parent.path(), other),
        cipher
            .seal(other, &serde_json::to_vec(&record).unwrap())
            .unwrap(),
    )
    .unwrap();
    assert!(store.load(other).is_err());
    assert_eq!(
        store.load(record.id).unwrap().session.uid,
        record.session.uid
    );
}

#[test]
fn proton_vpn_store_rejects_truncation_tamper_and_size_limits() {
    let parent = tempfile::tempdir().unwrap();
    let cipher = TestCipher::new();
    let store = store::SessionStore::new(parent.path(), &cipher);
    let record = record();
    store.save(&record).unwrap();
    let file = path(parent.path(), record.id);
    let original = fs::read(&file).unwrap();
    for size in [0, 1, 11, 12, 27, original.len() - 1] {
        fs::write(&file, &original[..size]).unwrap();
        assert!(store.load(record.id).is_err());
    }
    for index in [0, 11, 12, original.len() - 1] {
        let mut corrupted = original.clone();
        corrupted[index] ^= 1;
        fs::write(&file, corrupted).unwrap();
        assert!(store.load(record.id).is_err());
    }
    fs::write(&file, vec![0u8; 12 * 1024 * 1024 + 1]).unwrap();
    assert!(store.load(record.id).is_err());
    fs::write(
        &file,
        cipher
            .seal(record.id, &vec![b' '; 8 * 1024 * 1024 + 1])
            .unwrap(),
    )
    .unwrap();
    assert!(store.load(record.id).is_err());
    fs::write(&file, original).unwrap();
    let mut oversized = record.clone();
    oversized.node.metadata.city = "x".repeat(128 * 1024);
    assert!(store.save(&oversized).is_err());
    assert_eq!(store.load(record.id).unwrap().node.metadata.city, "Geneva");
}

#[test]
fn proton_vpn_failed_save_preserves_authenticated_previous_record() {
    let parent = tempfile::tempdir().unwrap();
    let cipher = TestCipher::new();
    let store = store::SessionStore::new(parent.path(), &cipher);
    let mut record = record();
    store.save(&record).unwrap();
    let original = fs::read(path(parent.path(), record.id)).unwrap();
    record.session.access_token = "synthetic-sensitive-new-access".into();
    cipher.fail_seal.store(true, Ordering::Relaxed);
    assert!(store.save(&record).is_err());
    assert_eq!(fs::read(path(parent.path(), record.id)).unwrap(), original);
    assert_eq!(
        store.load(record.id).unwrap().session.access_token,
        "synthetic-sensitive-access"
    );
    assert!(
        !fs::read_dir(parent.path().join("proton-vpn"))
            .unwrap()
            .any(|v| v
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".proton-"))
    );
}

#[test]
fn proton_vpn_rotation_pending_survives_reopen_and_ordinary_reads() {
    let parent = tempfile::tempdir().unwrap();
    let cipher = TestCipher::new();
    let mut record = record();
    record.rotation_pending = true;
    store::SessionStore::new(parent.path(), &cipher)
        .save(&record)
        .unwrap();
    for _ in 0..3 {
        let store = store::SessionStore::new(parent.path(), &cipher);
        assert!(store.load(record.id).unwrap().rotation_pending);
        assert!(store.list().unwrap()[0].needs_login);
    }
    assert!(
        serde_json::from_slice::<Record>(
            &cipher
                .open(
                    record.id,
                    &fs::read(path(parent.path(), record.id)).unwrap()
                )
                .unwrap()
        )
        .unwrap()
        .rotation_pending
    );
}

#[test]
fn proton_vpn_lease_busy_returns_without_waiting_and_does_not_lock_other_ids() {
    let parent = tempfile::tempdir().unwrap();
    let cipher = TestCipher::new();
    let store = store::SessionStore::new(parent.path(), &cipher);
    let record = record();
    let first = store.lease(record.id).unwrap();
    let start = Instant::now();
    let failure = store.lease(record.id).unwrap_err();
    assert_eq!(failure.reason, "session_busy");
    assert!(start.elapsed() < Duration::from_secs(1));
    let other = store.lease(Uuid::new_v4()).unwrap();
    drop(other);
    drop(first);
    assert!(store.lease(record.id).is_ok());
}

#[test]
fn proton_vpn_clear_library_removes_own_sidecars_and_temporaries_only() {
    let parent = tempfile::tempdir().unwrap();
    let cipher = TestCipher::new();
    let store = store::SessionStore::new(parent.path(), &cipher);
    let record = record();
    store.save(&record).unwrap();
    let directory = parent.path().join("proton-vpn");
    fs::write(
        directory.join(".proton-interrupted"),
        b"synthetic ciphertext",
    )
    .unwrap();
    fs::write(directory.join("unrelated.txt"), b"preserve").unwrap();
    clear_library(parent.path()).unwrap();
    assert!(!path(parent.path(), record.id).exists());
    assert!(!directory.join(".proton-interrupted").exists());
    assert_eq!(
        fs::read(directory.join("unrelated.txt")).unwrap(),
        b"preserve"
    );
    assert!(store.list().unwrap().is_empty());
}

#[cfg(unix)]
#[test]
fn proton_vpn_store_rejects_linked_directory_and_record_without_touching_target() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let parent = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let cipher = TestCipher::new();
    let record = record();
    symlink(outside.path(), parent.path().join("proton-vpn")).unwrap();
    let store = store::SessionStore::new(parent.path(), &cipher);
    assert!(store.save(&record).is_err());
    assert!(store.list().is_err());
    assert!(store.lease(record.id).is_err());
    assert!(fs::read_dir(outside.path()).unwrap().next().is_none());
    fs::remove_file(parent.path().join("proton-vpn")).unwrap();
    store.save(&record).unwrap();
    let target = outside.path().join("preserve");
    fs::write(&target, b"preserve-external-file").unwrap();
    fs::remove_file(path(parent.path(), record.id)).unwrap();
    symlink(&target, path(parent.path(), record.id)).unwrap();
    assert!(store.load(record.id).is_err());
    assert!(store.save(&record).is_err());
    assert!(store.delete(record.id).is_err());
    clear_library(parent.path()).unwrap();
    assert!(
        fs::symlink_metadata(path(parent.path(), record.id))
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(fs::read(&target).unwrap(), b"preserve-external-file");
    fs::remove_file(path(parent.path(), record.id)).unwrap();
    fs::set_permissions(
        parent.path().join("proton-vpn"),
        fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    assert!(store.save(&record).is_err());
}

#[cfg(unix)]
#[test]
fn proton_vpn_store_rejects_linked_lockfiles_and_dangling_record() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let parent = tempfile::tempdir().unwrap();
    let cipher = TestCipher::new();
    let directory = parent.path().join("proton-vpn");
    fs::create_dir(&directory).unwrap();
    fs::set_permissions(&directory, fs::Permissions::from_mode(0o700)).unwrap();
    let target = parent.path().join("external-target");
    fs::write(&target, b"preserve").unwrap();
    let record = record();
    let store = store::SessionStore::new(parent.path(), &cipher);
    let catalogue = directory.join("catalog.json.lock");
    symlink(&target, &catalogue).unwrap();
    assert!(store.list().is_err());
    fs::remove_file(&catalogue).unwrap();
    let lease = directory.join(format!("{}.json.lock", record.id));
    symlink(&target, &lease).unwrap();
    assert!(store.lease(record.id).is_err());
    fs::remove_file(&lease).unwrap();
    symlink(
        parent.path().join("does-not-exist"),
        path(parent.path(), record.id),
    )
    .unwrap();
    assert!(store.save(&record).is_err());
    assert!(
        fs::symlink_metadata(path(parent.path(), record.id))
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(fs::read(&target).unwrap(), b"preserve");
}

#[test]
fn proton_vpn_sensitive_material_never_appears_in_debug_or_normal_status() {
    let record = record();
    assert_private(&format!("{record:?} {:?}", record.session));
    let credentials = TunnelCredentials {
        certificate_chain: record.certificate_chain.clone(),
        private_key_pkcs8: Zeroizing::new(b"synthetic-sensitive-key".to_vec()),
        server_name: "sensitive-host.example".into(),
        expires_at: record.expires_at,
        label: None,
    };
    assert_private(&format!("{credentials:?}"));
    assert_private(&serde_json::to_string(&record.summary()).unwrap());
    let request = Request::parse(&format!(r#"{{"action":"select_node","job_id":"{}","node_id":"sensitive-node","name":"sensitive-name"}}"#, Uuid::new_v4())).unwrap();
    assert_private(&format!("{request:?}"));
    for field in [
        "password",
        "username",
        "access_token",
        "refresh_token",
        "url",
        "certificate",
        "verification_token",
    ] {
        let input = format!(r#"{{"action":"login","{field}":"synthetic-sensitive-input"}}"#);
        let failure = Request::parse(&input).unwrap_err();
        assert_eq!(failure.reason, "invalid_request");
        assert_private(&format!("{failure:?}"));
    }
    assert_private(
        &serde_json::to_string(&Response {
            job: Some(Job::new()),
            ..Default::default()
        })
        .unwrap(),
    );
}

proptest::proptest! {
    #[test]
    fn proton_vpn_malformed_requests_are_bounded_and_errors_are_fixed(bytes in proptest::collection::vec(proptest::num::u8::ANY, 0..5000)) {
        let text = String::from_utf8_lossy(&bytes);
        if let Err(failure) = Request::parse(&text) {
            proptest::prop_assert_eq!(failure.field, "proton_vpn");
            proptest::prop_assert_eq!(failure.reason, "invalid_request");
        }
    }
}

#[cfg(unix)]
#[test]
fn proton_vpn_filesystem_write_failure_preserves_previous_encrypted_object() {
    // Deterministically make the destination directory unavailable only after
    // encryption. This exercises a real filesystem failure without chmod,
    // privileged fixtures, a full disk, or a production fault-injection hook.
    struct InterruptedDirectory<'a> {
        cipher: &'a TestCipher,
        directory: PathBuf,
        retained: PathBuf,
    }
    impl ProfileCipher for InterruptedDirectory<'_> {
        fn seal(&self, id: Uuid, value: &[u8]) -> Result<Vec<u8>, ImportError> {
            let sealed = self.cipher.seal(id, value)?;
            fs::rename(&self.directory, &self.retained).unwrap();
            fs::write(&self.directory, b"synthetic-unavailable-directory").unwrap();
            Ok(sealed)
        }
        fn open(&self, id: Uuid, value: &[u8]) -> Result<Zeroizing<Vec<u8>>, ImportError> {
            self.cipher.open(id, value)
        }
    }
    let parent = tempfile::tempdir().unwrap();
    let cipher = TestCipher::new();
    let store = store::SessionStore::new(parent.path(), &cipher);
    let mut record = record();
    store.save(&record).unwrap();
    let original = fs::read(path(parent.path(), record.id)).unwrap();
    let interrupted = InterruptedDirectory {
        cipher: &cipher,
        directory: parent.path().join("proton-vpn"),
        retained: parent.path().join("retained-private-directory"),
    };
    record.session.refresh_token = "synthetic-sensitive-new-refresh".into();
    assert_eq!(
        store::SessionStore::new(parent.path(), &interrupted)
            .save(&record)
            .unwrap_err()
            .reason,
        "secure_storage_failed"
    );
    assert_eq!(
        fs::read(interrupted.retained.join(format!("{}.sealed", record.id))).unwrap(),
        original
    );
    fs::remove_file(&interrupted.directory).unwrap();
    fs::rename(&interrupted.retained, &interrupted.directory).unwrap();
    assert_eq!(
        store.load(record.id).unwrap().session.refresh_token,
        "synthetic-sensitive-refresh"
    );
}

#[test]
fn proton_vpn_record_enumeration_limit_is_fail_closed() {
    let parent = tempfile::tempdir().unwrap();
    let cipher = TestCipher::new();
    let store = store::SessionStore::new(parent.path(), &cipher);
    assert!(store.list().unwrap().is_empty());
    for _ in 0..129 {
        // Contents are deliberately irrelevant: enumeration must stop before
        // decrypting any of these malformed sidecars beyond the entry bound.
        fs::write(
            path(parent.path(), Uuid::new_v4()),
            b"synthetic malformed ciphertext",
        )
        .unwrap();
    }
    let failure = store.list().err().unwrap();
    assert_eq!(failure.reason, "profile_limit");
    assert_eq!(store.save(&record()).unwrap_err().reason, "profile_limit");
}

#[cfg(windows)]
#[test]
fn proton_vpn_windows_failed_write_through_replace_keeps_old_file_and_cleans_temporary() {
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::Storage::FileSystem::FILE_SHARE_READ;

    let parent = tempfile::tempdir().unwrap();
    let cipher = TestCipher::new();
    let store = store::SessionStore::new(parent.path(), &cipher);
    let mut record = record();
    store.save(&record).unwrap();
    let path = path(parent.path(), record.id);
    let original = fs::read(&path).unwrap();
    // A read handle that denies delete sharing produces a real, deterministic
    // Windows rename failure after the new ciphertext has been written/flushed.
    // This touches only a synthetic sidecar inside this test's temp directory.
    let blocker = fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ)
        .open(&path)
        .unwrap();
    record.rotation_pending = true;
    record.session.refresh_token = "synthetic-sensitive-new-refresh".into();
    assert_eq!(
        store.save(&record).unwrap_err().reason,
        "secure_storage_failed"
    );
    assert_eq!(fs::read(&path).unwrap(), original);
    assert!(
        !fs::read_dir(parent.path().join("proton-vpn"))
            .unwrap()
            .any(|entry| {
                entry
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".proton-")
            })
    );
    drop(blocker);
    let previous = store.load(record.id).unwrap();
    assert!(!previous.rotation_pending);
    assert_eq!(
        previous.session.refresh_token,
        "synthetic-sensitive-refresh"
    );
}

#[test]
fn v1_records_migrate_without_inventing_a_certificate_or_changing_the_logical_selection() {
    let mut old = record();
    old.node.metadata.id = "logical:physical".into();
    let mut value = serde_json::to_value(&old).unwrap();
    for field in [
        "server_id",
        "server_catalog",
        "retired_sessions",
        "catalog_revision",
    ] {
        value.as_object_mut().unwrap().remove(field);
    }
    let mut restored: Record = serde_json::from_value(value).unwrap();
    restored.validate().unwrap();
    assert_eq!(restored.selected_server(), Some("logical"));
    restored.server_id = restored.selected_server().map(str::to_owned);
    restored.version = 2;
    restored.certificate_chain.zeroize();
    restored.expires_at = 0;
    restored.refresh_at = 0;
    restored.validate().unwrap();
    assert_eq!(restored.summary().server_id.as_deref(), Some("logical"));
    assert!(!restored.summary().needs_login);
    restored.version = 1;
    assert!(restored.validate().is_err());
}
#[test]
fn complete_catalogues_are_encrypted_and_reject_duplicates_and_invalid_metadata() {
    let parent = tempfile::tempdir().unwrap();
    let cipher = TestCipher::new();
    let store = store::SessionStore::new(parent.path(), &cipher);
    let mut record = record();
    record.version = 2;
    record.server_catalog = (0..600)
        .map(|i| Node {
            id: format!("logical-{i}"),
            name: format!("Server {i}"),
            country: "IS".into(),
            city: "".into(),
            load: 0,
        })
        .collect();
    record.catalog_revision = Uuid::new_v4();
    store.save(&record).unwrap();
    let loaded = store.load(record.id).unwrap();
    assert_eq!(loaded.server_catalog.len(), 600);
    assert_eq!(loaded.catalog_revision, record.catalog_revision);
    assert!(
        !String::from_utf8_lossy(&fs::read(path(parent.path(), record.id)).unwrap())
            .contains("logical-599")
    );
    record.server_catalog.push(record.server_catalog[0].clone());
    assert!(record.validate().is_err());
    record.server_catalog.pop();
    record.server_catalog[0].load = 101;
    assert!(record.validate().is_err());
}
#[test]
fn revocation_capabilities_never_become_authentication_records_or_public_metadata() {
    let mut record = record();
    let mut partial = SessionSecrets::default();
    partial.uid = "synthetic-sensitive-retired".into();
    partial.access_token = "synthetic-sensitive-partial".into();
    record
        .retired_sessions
        .push(RevocationHandle::capture(&partial, &record.api_identity).unwrap());
    record.validate().unwrap();
    assert!(record.retired_sessions[0].session().validate().is_err());
    assert!(record.summary().cleanup_pending);
    assert_private(&serde_json::to_string(&record.summary()).unwrap());
    record.retired_sessions[0].uid = record.session.uid.clone();
    assert!(record.validate().is_err());
    record.rotation_pending = true;
    record.validate().unwrap();
    assert!(RevocationHandle::capture(&SessionSecrets::default(), &record.api_identity).is_none());
}

#[test]
fn recovered_key_record_is_atomic_private_and_legacy_binding_remains_unchanged() {
    let parent = tempfile::tempdir().unwrap();
    let cipher = TestCipher::new();
    let mut record = record();
    let binding = record.ed25519_seed.clone();
    record.tunnel_seed = Some(Zeroizing::new(vec![8; 32]));
    record.key_recovery_at = 123;
    record.promote();
    let store = store::SessionStore::new(parent.path(), &cipher);
    store.save(&record).unwrap();
    let saved = store.load(record.id).unwrap();
    assert_eq!(saved.version, 3);
    assert_eq!(saved.tunnel_seed(), &[8; 32]);
    assert_eq!(saved.ed25519_seed, binding);
    assert!(
        !serde_json::to_string(&saved.summary())
            .unwrap()
            .contains("tunnel_seed")
    );
    record.version = 2;
    assert!(record.validate().is_err());
    record.promote();
    record.tunnel_seed.as_mut().unwrap().pop();
    assert!(record.validate().is_err());
}
