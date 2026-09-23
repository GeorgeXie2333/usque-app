use std::sync::Mutex;
use usque_core::{
    Profile,
    chain_exit::{store::*, *},
    warp_wireguard::{self as warp, *},
};
use uuid::Uuid;
use zeroize::Zeroizing;

// Authenticated deterministic fixture, only for storage transaction tests.
#[derive(Default)]
struct FixtureCipher {
    fail: Mutex<Option<Uuid>>,
}
impl ProfileCipher for FixtureCipher {
    fn seal(&self, id: Uuid, value: &[u8]) -> Result<Vec<u8>, ImportError> {
        use sha2::{Digest, Sha256};
        if *self.fail.lock().unwrap() == Some(id) {
            return Err(warp::error("injected_write_failure"));
        }
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
fn only_outer_settings_invalidate_scan_context() {
    let original = Profile::default();
    let mut changed = original.clone();
    changed.name = "Renamed account".into();
    changed.chain_exit = Some(ChainExitSettings {
        source: ChainSource::WarpWireguard,
        ..Default::default()
    });
    changed.proxy.system_proxy = !changed.proxy.system_proxy;
    assert_eq!(
        warp::context(&original).unwrap(),
        warp::context(&changed).unwrap()
    );
    changed.endpoint.port = 8443;
    assert_ne!(
        warp::context(&original).unwrap(),
        warp::context(&changed).unwrap()
    );
}
#[test]
fn warp_source_and_endpoint_override_leave_credentials_and_revision_unchanged() {
    let temp = tempfile::tempdir().unwrap();
    let cipher = FixtureCipher::default();
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
    let cipher = FixtureCipher::default();
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
fn row(job: &Job, index: usize) -> ProbeResult {
    ProbeResult {
        index,
        endpoint: job.endpoint(index).unwrap(),
        checked_at: now(),
        ipv4: Some(Observation {
            country: Some(if index.is_multiple_of(2) { "US" } else { "SG" }.into()),
            exit_ip: Some("104.28.1.1".parse().unwrap()),
            ..Default::default()
        }),
        ipv6: None,
        failure: None,
    }
}
#[test]
fn encrypted_results_page_by_endpoint_and_filter_observed_country() {
    let temp = tempfile::tempdir().unwrap();
    let cipher = FixtureCipher::default();
    let store = Store::new(temp.path(), &cipher);
    let request = Request::parse(r#"{"action":"start","mode":"full"}"#).unwrap();
    let mut job = Job::new(&request, "context".into()).unwrap();
    store.insert(&job).unwrap();
    for batch in 0..2 {
        let mut rows = (batch * 64..(batch + 1) * 64)
            .map(|i| row(&job, i))
            .collect::<Vec<_>>();
        job.next = (batch + 1) * 64;
        store.checkpoint(&mut job, &mut rows).unwrap();
    }
    let reloaded = store.job(job.id).unwrap();
    assert_eq!(reloaded.plan, ScanPlan::SinglePortV1);
    assert_eq!(reloaded.endpoint(128), job.endpoint(128));
    assert_eq!(reloaded.snapshot().completed, 128);
    assert_eq!(reloaded.snapshot().total, 3584);
    let (page, next) = store.page(&reloaded, 0, None).unwrap();
    assert_eq!(page.len(), 100);
    assert_eq!(next, Some(100));
    assert_eq!(store.page(&reloaded, 100, None).unwrap().0.len(), 28);
    assert_eq!(store.page(&reloaded, 0, Some("SG")).unwrap().0.len(), 64);
    assert!(store.page(&reloaded, 0, Some("DE")).unwrap().0.is_empty());
    store.save_identity(&secrets()).unwrap();
    assert_eq!(
        store.identity().unwrap().unwrap().configuration,
        secrets().configuration
    );
    for entry in std::fs::read_dir(temp.path().join("warp-wireguard")).unwrap() {
        let data = std::fs::read(entry.unwrap().path()).unwrap();
        assert!(
            !data
                .windows(b"PrivateKey".len())
                .any(|v| v == b"PrivateKey")
        );
    }
    warp::clear(temp.path()).unwrap();
    assert!(store.jobs().unwrap().is_empty());
    assert!(store.identity().unwrap().is_none());
}
#[test]
fn interrupted_checkpoint_never_exposes_uncommitted_rows() {
    let temp = tempfile::tempdir().unwrap();
    let cipher = FixtureCipher::default();
    let store = Store::new(temp.path(), &cipher);
    let request = Request::parse(r#"{"action":"start"}"#).unwrap();
    let mut job = Job::new(&request, "context".into()).unwrap();
    store.insert(&job).unwrap();
    let mut rows = vec![row(&job, 0)];
    job.next = 1;
    *cipher.fail.lock().unwrap() = Some(job.id);
    assert!(store.checkpoint(&mut job, &mut rows).is_err());
    assert_eq!(rows.len(), 1);
    assert_eq!(job.chunks, 0);
    let mut recovered = store.job(job.id).unwrap();
    assert_eq!(recovered.next, 0);
    assert!(store.page(&recovered, 0, None).unwrap().0.is_empty());
    *cipher.fail.lock().unwrap() = None;
    let mut rows = vec![row(&recovered, 0)];
    recovered.next = 1;
    store.checkpoint(&mut recovered, &mut rows).unwrap();
    assert_eq!(
        store
            .page(&store.job(job.id).unwrap(), 0, None)
            .unwrap()
            .0
            .len(),
        1
    );
}

#[test]
fn partial_checkpoints_reuse_chunks_and_record_failed_attempts() {
    let temp = tempfile::tempdir().unwrap();
    let cipher = FixtureCipher::default();
    let store = Store::new(temp.path(), &cipher);
    let request = Request::parse(r#"{"action":"start"}"#).unwrap();
    let mut job = Job::new(&request, "context".into()).unwrap();
    store.insert(&job).unwrap();
    for index in 0..70 {
        let mut result = row(&job, index);
        if index.is_multiple_of(2) {
            result.failure = Some("no_tunnel_data".into());
            result.ipv4 = None;
        }
        job.next = index + 1;
        store.checkpoint(&mut job, &mut vec![result]).unwrap();
    }
    assert_eq!(job.chunks, 2);
    assert_eq!(store.page(&job, 0, None).unwrap().0.len(), 35);
    assert_eq!(store.page(&job, 64, None).unwrap().0.len(), 3);
    let mut attempted = 0;
    let mut failures = 0;
    for entry in std::fs::read_dir(temp.path().join("warp-wireguard")).unwrap() {
        let entry = entry.unwrap();
        let id = Uuid::parse_str(entry.path().file_stem().unwrap().to_str().unwrap()).unwrap();
        let plain = cipher
            .open(id, &std::fs::read(entry.path()).unwrap())
            .unwrap();
        if let Ok(rows) = serde_json::from_slice::<Vec<ProbeResult>>(&plain) {
            attempted += rows.len();
            failures += rows.iter().filter(|r| r.failure.is_some()).count();
        }
    }
    assert_eq!(attempted, 70);
    assert_eq!(failures, 35);
    assert_eq!(job.snapshot().completed, 70);
    assert_eq!(job.snapshot().total, 70);
}
