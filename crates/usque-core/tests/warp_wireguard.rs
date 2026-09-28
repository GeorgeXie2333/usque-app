use usque_core::{
    Profile,
    chain_exit::{store::*, *},
    warp_wireguard::{self as warp, *},
};
use uuid::Uuid;
use zeroize::Zeroizing;

// Authenticated deterministic fixture, only for storage transaction tests.
#[derive(Default)]
struct FixtureCipher;
impl ProfileCipher for FixtureCipher {
    fn seal(&self, id: Uuid, value: &[u8]) -> Result<Vec<u8>, ImportError> {
        use sha2::{Digest, Sha256};
        let mut bytes = id.as_bytes().to_vec();
        bytes.extend(Sha256::digest(value));
        bytes.extend(value.iter().map(|b| b ^ 0xa5));
        Ok(bytes)
    }
    fn open(&self, id: Uuid, value: &[u8]) -> Result<Zeroizing<Vec<u8>>, ImportError> {
        use sha2::{Digest, Sha256};
        if value.len() < 48 || &value[..16] != id.as_bytes() {
            return Err(warp::error("invalid_ciphertext"));
        }
        let plain = Zeroizing::new(value[48..].iter().map(|b| b ^ 0xa5).collect::<Vec<_>>());
        if Sha256::digest(&plain)[..] != value[16..48] {
            return Err(warp::error("invalid_ciphertext"));
        }
        Ok(plain)
    }
}
fn secrets() -> ImportSecrets {
    use base64::{Engine, engine::general_purpose::STANDARD};
    ImportSecrets::new(format!(
        "[Interface]\nPrivateKey={}\nAddress=172.16.0.2/32,2606:4700:110::2/128\nDNS=1.1.1.1\n[Peer]\nPublicKey={}\nEndpoint=162.159.192.1:2408\nAllowedIPs=0.0.0.0/0,::/0\n",
        STANDARD.encode([1u8; 32]),
        STANDARD.encode([2u8; 32])
    ))
}

#[test]
fn warp_source_and_endpoint_override_leave_credentials_and_revision_unchanged() {
    let temp = tempfile::tempdir().unwrap();
    let cipher = FixtureCipher;
    let store = ChainProfileStore::new(temp.path(), &cipher);
    let summary = store
        .import(ChainSource::WarpWireguard, "WARP", secrets())
        .unwrap();
    assert_eq!(summary.source, ChainSource::WarpWireguard);
    let mut selection = summary.selection();
    selection.endpoint_override = Some(Endpoint::parse("2606:4700:d0::123", "500", 0).unwrap());
    let profile = Profile {
        chain_exit: Some(selection.clone()),
        ..Default::default()
    };
    let (_, prepared) = prepare_selection(temp.path(), &profile, &cipher)
        .unwrap()
        .unwrap();
    let parsed = prepared.custom.as_deref().unwrap();
    let ValidatedProfile::WireGuard(wg) = parsed else {
        panic!("WireGuard expected");
    };
    assert_eq!(wg.endpoint, selection.endpoint_override.clone().unwrap());
    let (persisted, _, stored) = store.load(&selection).unwrap();
    assert_eq!(persisted, summary);
    assert_eq!(stored.configuration, secrets().configuration);
    let mut wrong_source = selection.clone();
    wrong_source.source = ChainSource::WireguardCustom;
    assert!(wrong_source.validate().is_err());
    let mut invalid = selection;
    invalid.endpoint_override.as_mut().unwrap().port = 0;
    assert!(invalid.validate().is_err());
}
#[test]
fn old_wireguard_records_recover_their_source_without_reidentification() {
    let temp = tempfile::tempdir().unwrap();
    let cipher = FixtureCipher;
    let store = ChainProfileStore::new(temp.path(), &cipher);
    let summary = store
        .import(ChainSource::WireguardCustom, "Legacy", secrets())
        .unwrap();
    let path = temp
        .path()
        .join("chain-profiles")
        .join(format!("{}.profile", summary.id));
    let plain = cipher
        .open(summary.id, &std::fs::read(&path).unwrap())
        .unwrap();
    let mut record: serde_json::Value = serde_json::from_slice(&plain).unwrap();
    record["version"] = 2.into();
    record["summary"].as_object_mut().unwrap().remove("source");
    std::fs::write(
        path,
        cipher
            .seal(summary.id, &serde_json::to_vec(&record).unwrap())
            .unwrap(),
    )
    .unwrap();
    let (migrated, _, _) = store.load(&summary.selection()).unwrap();
    assert_eq!(migrated, summary);
}
#[test]
fn generation_commands_reject_removed_scans_and_invalid_input() {
    for action in ["start", "resume", "pause", "unknown"] {
        let failure = Request::parse(&format!(r#"{{"action":"{action}"}}"#)).unwrap_err();
        assert_eq!(failure.reason, "invalid_request");
    }
    for extra in [
        r#""mode":"quick""#,
        r#""target":"162.159.192.1""#,
        r#""ipv6":true"#,
        r#""cursor":0"#,
        r#""country":"US""#,
    ] {
        assert!(Request::parse(&format!(r#"{{"action":"generate",{extra}}}"#)).is_err());
    }
    assert!(Request::parse(r#"{"action":"cancel"}"#).is_err());
    assert!(Request::parse(&" ".repeat(4097)).is_err());
    assert!(
        Request::parse(
            &serde_json::json!({"action":"generate", "name":"x".repeat(65)}).to_string()
        )
        .is_err()
    );
    assert!(Request::parse(r#"{"action":"generate","name":"a\nb"}"#).is_err());
    assert!(
        Request::parse(r#"{"action":"generate"}"#)
            .unwrap()
            .needs_network()
    );
    assert!(
        !Request::parse(r#"{"action":"get"}"#)
            .unwrap()
            .needs_network()
    );
    assert!(
        !Request::parse(&format!(
            r#"{{"action":"cancel","job_id":"{}"}}"#,
            Uuid::new_v4()
        ))
        .unwrap()
        .needs_network()
    );
}

#[test]
fn explicit_clear_removes_only_owned_encrypted_sidecars() {
    let temp = tempfile::tempdir().unwrap();
    let directory = temp.path().join("warp-wireguard");
    std::fs::create_dir(&directory).unwrap();
    let owned = directory.join(format!("{}.sealed", Uuid::new_v4()));
    std::fs::write(&owned, b"opaque encrypted sidecar").unwrap();
    let pending = directory.join(".warp-pending");
    std::fs::write(&pending, b"pending write").unwrap();
    let foreign = directory.join("keep.txt");
    std::fs::write(&foreign, b"unrelated").unwrap();
    let nested = directory.join(format!("{}.sealed", Uuid::new_v4()));
    std::fs::create_dir(&nested).unwrap();
    clear_library(temp.path()).unwrap();
    assert!(!owned.exists());
    assert!(!pending.exists());
    assert!(nested.is_dir());
    assert_eq!(std::fs::read(foreign).unwrap(), b"unrelated");
}

#[test]
fn explicit_clear_rejects_non_directory_sidecar_paths() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("warp-wireguard");
    std::fs::write(&path, b"unrelated").unwrap();
    assert!(clear_library(temp.path()).is_err());
    assert_eq!(std::fs::read(path).unwrap(), b"unrelated");
}
