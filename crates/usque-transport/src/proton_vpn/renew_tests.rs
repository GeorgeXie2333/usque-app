//! Memory HTTPS and storage peers. No real credential or network is used.
use super::*;
use crate::internal_network::{InternalHttpError, InternalRequest, InternalResponse};
use boring::{
    asn1::{Asn1Integer, Asn1Time},
    bn::BigNum,
    ec::{EcGroup, EcKey},
    hash::MessageDigest,
    nid::Nid,
    pkey::PKey,
    x509::{X509, X509NameBuilder},
};
use serde_json::{Value, json};
use std::{
    collections::VecDeque,
    sync::{
        Mutex,
        atomic::{AtomicU64, Ordering},
    },
};
use usque_core::{
    chain_exit::{Endpoint, ImportError},
    proton_vpn::{Node, SelectedNode, SessionSecrets},
};
use uuid::Uuid;
use zeroize::Zeroize;

struct MemoryStore {
    record: Mutex<Record>,
    writes: Mutex<Vec<Record>>,
}
impl LeasePersistence for MemoryStore {
    fn load(&self) -> Result<Record, ImportError> {
        Ok(self.record.lock().unwrap().clone())
    }
    fn save(&self, record: &Record) -> Result<(), ImportError> {
        record.validate()?;
        self.writes.lock().unwrap().push(record.clone());
        *self.record.lock().unwrap() = record.clone();
        Ok(())
    }
    fn lock(&self) -> Result<File, ImportError> {
        Ok(tempfile::tempfile().unwrap())
    }
}

enum Answer {
    Json(Value),
    Status(u16, Value),
    Failed,
    Cancel(CancellationToken),
    ChangeGeneration,
}
struct MockHttp {
    answers: Mutex<VecDeque<Answer>>,
    requests: Mutex<Vec<String>>,
    headers: Mutex<Vec<Vec<(String, String)>>>,
    bodies: Mutex<Vec<Value>>,
    observed: Mutex<Vec<Record>>,
    store: Mutex<Option<Arc<MemoryStore>>>,
    generation: AtomicU64,
}
#[async_trait]
impl Http for MockHttp {
    async fn exchange(
        &self,
        request: InternalRequest<'_>,
        _: &CancellationToken,
    ) -> Result<InternalResponse, InternalHttpError> {
        self.requests.lock().unwrap().push(request.url.to_owned());
        self.headers.lock().unwrap().push(
            request
                .headers
                .iter()
                .map(|(k, v)| ((*k).into(), (*v).into()))
                .collect(),
        );
        self.bodies
            .lock()
            .unwrap()
            .push(serde_json::from_slice(&request.body).unwrap_or(Value::Null));
        if let Some(store) = self.store.lock().unwrap().as_ref() {
            self.observed.lock().unwrap().push(store.load().unwrap());
        }
        let (status, value) = match self
            .answers
            .lock()
            .unwrap()
            .pop_front()
            .expect("unexpected network request")
        {
            Answer::Json(v) => (200, v),
            Answer::Status(status, v) => (status, v),
            Answer::Failed => return Err(InternalHttpError::Timeout),
            Answer::Cancel(cancel) => {
                cancel.cancel();
                (200, refresh())
            }
            Answer::ChangeGeneration => {
                self.generation.store(2, Ordering::Release);
                (200, refresh())
            }
        };
        Ok(InternalResponse {
            status,
            retry_after: None,
            body: Zeroizing::new(serde_json::to_vec(&value).unwrap()),
        })
    }
    fn generation(&self) -> Option<u64> {
        Some(self.generation.load(Ordering::Acquire))
    }
    fn current(&self, generation: Option<u64>) -> bool {
        self.generation() == generation
    }
}
fn mock(answers: Vec<Answer>) -> Arc<MockHttp> {
    Arc::new(MockHttp {
        answers: Mutex::new(answers.into()),
        requests: Mutex::new(Vec::new()),
        headers: Mutex::new(Vec::new()),
        bodies: Mutex::new(Vec::new()),
        observed: Mutex::new(Vec::new()),
        store: Mutex::new(None),
        generation: AtomicU64::new(1),
    })
}
fn refresh() -> Value {
    json!({"Code":1000,"UID":"uid","AccessToken":"fresh-access","RefreshToken":"fresh-refresh"})
}
fn unauthorized() -> Answer {
    Answer::Status(401, json!({"Code":401}))
}
fn user() -> Value {
    json!({"Code":1000,"User":{"ID":"user"}})
}
fn permissions() -> Value {
    json!({"Code":1000,"VPN":{"Status":1,"MaxConnect":1,"MaxTier":0},"Delinquent":0})
}
fn config() -> Value {
    json!({"Code":1000,"DefaultPorts":{"WireGuard":{"UDP":[443]}}})
}
fn directory() -> Value {
    json!({"Code":1000,"LogicalServers":[{"ID":"logical","Name":"Logical server","Tier":0,"Features":0,"ExitCountry":"IS","Servers":[
        {"Status":1,"EntryIP":"8.8.8.8","Domain":"fresh.example.invalid","X25519PublicKey":STANDARD.encode(KeyMaterial::from_seed([9;32]).unwrap().wireguard_public())},
        {"Status":1,"EntryIP":"9.9.9.9","Domain":"duplicate.example.invalid","X25519PublicKey":STANDARD.encode([8;32])}
    ]}]})
}
fn cert_answer(record: &Record) -> Answer {
    Answer::Json(
        json!({"Code":1000,"Certificate":&*record.certificate_chain,"ExpirationTime":record.expires_at,"RefreshTime":record.refresh_at}),
    )
}
fn success_answers() -> Vec<Answer> {
    vec![
        Answer::Json(user()),
        Answer::Json(permissions()),
        Answer::Json(config()),
        Answer::Json(directory()),
    ]
}
fn certificate(keys: &KeyMaterial, expiry: i64) -> String {
    let private = PKey::private_key_from_der(&keys.pkcs8_der()).unwrap();
    let mut name = X509NameBuilder::new().unwrap();
    name.append_entry_by_text("CN", "synthetic-runtime-certificate")
        .unwrap();
    let name = name.build();
    let mut cert = X509::builder().unwrap();
    cert.set_version(2).unwrap();
    cert.set_serial_number(&Asn1Integer::from_bn(&BigNum::from_u32(1).unwrap()).unwrap())
        .unwrap();
    cert.set_subject_name(&name).unwrap();
    cert.set_issuer_name(&name).unwrap();
    cert.set_pubkey(&private).unwrap();
    cert.set_not_before(&Asn1Time::from_unix(super::super::interaction::now() - 60).unwrap())
        .unwrap();
    cert.set_not_after(&Asn1Time::from_unix(expiry).unwrap())
        .unwrap();
    // A separate synthetic issuer signs the Ed25519 leaf. Runtime validation
    // binds leaf/key/time; production provenance comes from the HTTPS API.
    let group = EcGroup::from_curve_name(Nid::X9_62_PRIME256V1).unwrap();
    let issuer = PKey::from_ec_key(EcKey::generate(&group).unwrap()).unwrap();
    cert.sign(&issuer, MessageDigest::sha256()).unwrap();
    String::from_utf8(cert.build().to_pem().unwrap()).unwrap()
}

fn fixture() -> (Record, PreparedProfile, Arc<MemoryStore>) {
    let keys = KeyMaterial::from_seed([7; 32]).unwrap();
    let expires_at = super::super::interaction::now() + 3600;
    let certificate_chain = Zeroizing::new(certificate(&keys, expires_at));
    let node = SelectedNode {
        metadata: Node {
            id: "logical:physical".into(),
            name: "Synthetic node".into(),
            country: "IS".into(),
            city: "".into(),
            load: 0,
        },
        domain: "agent.example.invalid".into(),
        endpoint: Endpoint::parse("192.0.2.1", "51820", 0).unwrap(),
        public_key: STANDARD.encode(KeyMaterial::from_seed([8; 32]).unwrap().wireguard_public()),
    };
    let record = Record {
        api_identity: Default::default(),
        version: 2,
        id: Uuid::new_v4(),
        revision: Uuid::new_v4(),
        name: "Offline account".into(),
        identity: "login".into(),
        session: SessionSecrets {
            uid: "uid".into(),
            access_token: "old-access".into(),
            refresh_token: "old-refresh".into(),
            user_id: "user".into(),
        },
        ed25519_seed: Zeroizing::new(vec![7; 32]),
        certificate_chain: certificate_chain.clone(),
        expires_at,
        refresh_at: expires_at - 1800,
        max_tier: 0,
        node,
        rotation_pending: false,
        server_id: Some("logical".into()),
        server_catalog: Vec::new(),
        retired_sessions: Vec::new(),
        catalog_revision: Uuid::nil(),
    };
    let config = super::super::nodes::configuration(&keys, &record.node).unwrap();
    let parsed = ValidatedProfile::parse(ChainSource::ProtonVpn, &config).unwrap();
    let mut summary = parsed
        .summary(&record.name, record.id, record.revision)
        .unwrap();
    summary.source = ChainSource::ProtonVpn;
    let mut prepared = PreparedProfile::imported(summary, parsed, config);
    prepared.proton = Some(Arc::new(TunnelCredentials {
        certificate_chain,
        private_key_pkcs8: keys.pkcs8_der(),
        server_name: record.node.domain.clone(),
        expires_at,
    }));
    let storage = Arc::new(MemoryStore {
        record: Mutex::new(record.clone()),
        writes: Mutex::new(Vec::new()),
    });
    prepared.proton_persistence = Some(storage.clone());
    (record, prepared, storage)
}

#[tokio::test]
async fn healthy_session_and_certificate_are_reused_while_the_peer_is_resolved_at_connection() {
    let (record, mut prepared, storage) = fixture();
    let old_key = prepared.proton.as_ref().unwrap().private_key_pkcs8.clone();
    let http = mock(success_answers());
    let (credentials, lease) =
        prepare_with_http(&mut prepared, http.clone(), &CancellationToken::new())
            .await
            .unwrap();
    assert_eq!(credentials.private_key_pkcs8, old_key);
    assert_eq!(credentials.server_name, "fresh.example.invalid");
    assert_eq!(prepared.summary.as_ref().unwrap().endpoint.host, "8.8.8.8");
    assert_eq!(prepared.summary.as_ref().unwrap().endpoint.port, 443);
    assert_eq!(prepared.summary.as_ref().unwrap().id, record.id);
    assert_eq!(prepared.summary.as_ref().unwrap().revision, record.revision);
    assert_eq!(lease.current.session.access_token, "old-access");
    let saved = storage.record.lock().unwrap();
    assert_eq!(saved.node.endpoint, record.node.endpoint); // saved binding stays immutable
    assert_eq!(saved.server_catalog.len(), 1);
    assert_eq!(saved.server_catalog[0].id, "logical");
    let requests = http.requests.lock().unwrap();
    assert_eq!(requests.len(), 4);
    assert!(
        !requests
            .iter()
            .any(|url| url.contains("/refresh") || url.contains("/certificate"))
    );
    assert!(
        storage
            .writes
            .lock()
            .unwrap()
            .iter()
            .all(|record| !record.rotation_pending)
    );
}
#[tokio::test]
async fn deferred_certificate_is_requested_once_for_the_saved_key_at_first_connection() {
    let (cert, mut prepared, storage) = fixture();
    {
        let mut record = storage.record.lock().unwrap();
        record.certificate_chain.zeroize();
        record.expires_at = 0;
        record.refresh_at = 0;
    }
    let mut answers = success_answers();
    answers.push(cert_answer(&cert));
    let http = mock(answers);
    let (creds, _) = prepare_with_http(&mut prepared, http.clone(), &CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(creds.certificate_chain, cert.certificate_chain);
    assert_eq!(storage.record.lock().unwrap().expires_at, cert.expires_at);
    let bodies = http.bodies.lock().unwrap();
    assert_eq!(bodies.last().unwrap()["Mode"], "session");
    assert_eq!(
        bodies.last().unwrap()["ClientPublicKey"],
        KeyMaterial::from_seed([7; 32]).unwrap().spki_pem()
    );
    assert_eq!(http.requests.lock().unwrap().len(), 5);
}
#[tokio::test]
async fn guest_recovery_retains_android_headers_and_rechecks_guest_scopes() {
    let (_, mut prepared, storage) = fixture();
    {
        let mut record = storage.record.lock().unwrap();
        record.identity = "guest".into();
        record.api_identity =
            super::super::ClientConfig::android(super::super::client_tests::ANDROID_CONTEXT)
                .unwrap()
                .identity;
    }
    let http = mock(vec![
        unauthorized(),
        Answer::Json(refresh()),
        Answer::Json(json!({"Code":1000,"Scopes":["vpn"]})),
        Answer::Json(permissions()),
        Answer::Json(config()),
        Answer::Json(directory()),
    ]);
    prepare_with_http(&mut prepared, http.clone(), &CancellationToken::new())
        .await
        .unwrap();
    let requests = http.requests.lock().unwrap();
    assert!(requests[0].ends_with("/auth/v4/scopes"));
    assert!(requests[1].ends_with("/auth/v4/refresh"));
    assert!(!requests.iter().any(|url| url.ends_with("/core/v4/users")));
    for headers in http.headers.lock().unwrap().iter() {
        assert!(headers.contains(&("x-pm-appversion".into(), "android-vpn@5.20.74.0+apk".into())));
        assert!(headers.contains(&(
            "user-agent".into(),
            "ProtonVPN/5.20.74.0 (Android 15; google Pixel 8)".into()
        )));
    }
    assert_eq!(storage.record.lock().unwrap().session.user_id, "user");
}
#[tokio::test]
async fn only_401_rotates_once_and_a_known_replacement_precedes_later_protected_requests() {
    let (_, mut prepared, storage) = fixture();
    let mut answers = vec![unauthorized(), Answer::Json(refresh())];
    answers.extend(success_answers());
    let http = mock(answers);
    *http.store.lock().unwrap() = Some(storage.clone());
    prepare_with_http(&mut prepared, http.clone(), &CancellationToken::new())
        .await
        .unwrap();
    let observed = http.observed.lock().unwrap();
    assert!(!observed[0].rotation_pending);
    assert!(observed[1].rotation_pending);
    assert_eq!(observed[1].session.refresh_token, "old-refresh");
    assert!(!observed[2].rotation_pending);
    assert_eq!(observed[2].session.refresh_token, "fresh-refresh");
    let bodies = http.bodies.lock().unwrap();
    assert_eq!(bodies[1]["RefreshToken"], "old-refresh");
    assert_eq!(
        http.requests
            .lock()
            .unwrap()
            .iter()
            .filter(|url| url.ends_with("/auth/v4/refresh"))
            .count(),
        1
    );
}
#[tokio::test]
async fn failed_permissions_after_a_known_refresh_does_not_mark_the_session_for_login() {
    let (_, mut prepared, storage) = fixture();
    let http = mock(vec![
        unauthorized(),
        Answer::Json(refresh()),
        Answer::Json(user()),
        Answer::Status(403, json!({"Code":403})),
    ]);
    assert!(
        prepare_with_http(&mut prepared, http.clone(), &CancellationToken::new())
            .await
            .is_err()
    );
    let saved = storage.record.lock().unwrap();
    assert!(!saved.rotation_pending);
    assert_eq!(saved.session.refresh_token, "fresh-refresh");
    assert_eq!(http.requests.lock().unwrap().len(), 4);
}
#[tokio::test]
async fn a_second_401_is_terminal_without_replaying_a_refresh() {
    let (_, mut prepared, storage) = fixture();
    let http = mock(vec![
        unauthorized(),
        Answer::Json(refresh()),
        Answer::Json(user()),
        Answer::Json(permissions()),
        unauthorized(),
    ]);
    assert!(
        prepare_with_http(&mut prepared, http.clone(), &CancellationToken::new())
            .await
            .is_err()
    );
    assert!(!storage.record.lock().unwrap().rotation_pending);
    assert_eq!(http.requests.lock().unwrap().len(), 5);
}
#[tokio::test]
async fn ambiguous_rotation_and_interactive_challenge_remain_pending_and_are_never_replayed() {
    for answer in [
        Answer::Failed,
        Answer::Status(
            422,
            json!({"Code":9001,"Details":{"HumanVerificationToken":"challenge","HumanVerificationMethods":["captcha"]}}),
        ),
    ] {
        let (_, mut prepared, storage) = fixture();
        let http = mock(vec![unauthorized(), answer]);
        assert!(
            prepare_with_http(&mut prepared, http.clone(), &CancellationToken::new())
                .await
                .is_err()
        );
        assert!(storage.record.lock().unwrap().rotation_pending);
        assert!(
            prepare_with_http(&mut prepared, http.clone(), &CancellationToken::new())
                .await
                .is_err()
        );
        assert_eq!(http.requests.lock().unwrap().len(), 2);
    }
}
#[tokio::test]
async fn cancellation_or_underlay_change_with_a_valid_refresh_saves_only_the_known_replacement() {
    for changed in [false, true] {
        let (_, mut prepared, storage) = fixture();
        let cancel = CancellationToken::new();
        let answer = if changed {
            Answer::ChangeGeneration
        } else {
            Answer::Cancel(cancel.clone())
        };
        let http = mock(vec![unauthorized(), answer]);
        assert!(
            prepare_with_http(&mut prepared, http.clone(), &cancel)
                .await
                .is_err()
        );
        let saved = storage.record.lock().unwrap();
        assert!(!saved.rotation_pending);
        assert_eq!(saved.session.refresh_token, "fresh-refresh");
        assert_eq!(http.requests.lock().unwrap().len(), 2);
    }
}
#[tokio::test]
async fn malformed_or_wrong_user_refresh_keeps_a_cleanup_only_handle_without_authorizing_it() {
    for response in [
        json!({"Code":1000,"UID":"new-uid","AccessToken":"partial-access"}),
        json!({"Code":1000,"UID":"uid","AccessToken":"new-access","RefreshToken":"new-refresh","UserID":"wrong-user"}),
    ] {
        let (_, mut prepared, storage) = fixture();
        let http = mock(vec![unauthorized(), Answer::Json(response)]);
        assert!(
            prepare_with_http(&mut prepared, http.clone(), &CancellationToken::new())
                .await
                .is_err()
        );
        let saved = storage.record.lock().unwrap();
        assert!(saved.rotation_pending);
        assert_eq!(saved.session.refresh_token, "old-refresh");
        assert_eq!(saved.retired_sessions.len(), 1);
        assert!(saved.retired_sessions[0].session().validate().is_err());
        assert_eq!(http.requests.lock().unwrap().len(), 2);
    }
}
#[tokio::test]
async fn uid_rotation_invalidates_the_old_session_certificate_and_retains_the_key() {
    let (cert, mut prepared, storage) = fixture();
    let mut replacement = refresh();
    replacement["UID"] = json!("new-uid");
    let mut answers = vec![
        unauthorized(),
        Answer::Json(replacement),
        Answer::Json(json!({"Code":1000})),
    ];
    answers.extend(success_answers());
    answers.push(cert_answer(&cert));
    let http = mock(answers);
    prepare_with_http(&mut prepared, http.clone(), &CancellationToken::new())
        .await
        .unwrap();
    let writes = storage.writes.lock().unwrap();
    assert!(writes.iter().any(|record| !record.rotation_pending
        && record.session.uid == "new-uid"
        && record.certificate_chain.is_empty()));
    assert_eq!(writes.last().unwrap().session.uid, "new-uid");
    assert!(writes.last().unwrap().retired_sessions.is_empty());
    assert!(http.requests.lock().unwrap()[2].ends_with("/auth/v4"));
}
#[tokio::test]
async fn manual_server_is_never_silently_replaced_but_automatic_uses_the_fresh_catalogue() {
    for automatic in [false, true] {
        let (_, mut prepared, storage) = fixture();
        storage.record.lock().unwrap().server_id = if automatic {
            None
        } else {
            Some("missing".into())
        };
        let http = mock(success_answers());
        let result =
            prepare_with_http(&mut prepared, http.clone(), &CancellationToken::new()).await;
        if automatic {
            assert!(result.is_ok());
        } else {
            assert_eq!(result.err().unwrap().reason, "selected_server_unavailable");
        }
        assert_eq!(http.requests.lock().unwrap().len(), 4);
    }
}
#[tokio::test(start_paused = true)]
async fn sidecar_change_revokes_the_running_lease_without_network_or_overwriting_it() {
    let (record, prepared, storage) = fixture();
    let http = mock(Vec::new());
    let lease = Lease {
        current: record,
        persistence: storage.clone(),
        generation: Some(1),
        http: http.clone(),
        peer: None,
    };
    let (sender, _) = watch::channel(prepared.proton.unwrap());
    storage.record.lock().unwrap().rotation_pending = true;
    assert_eq!(
        lease.run(sender, &CancellationToken::new()).await,
        Err(GateFailure::Authentication)
    );
    assert!(storage.writes.lock().unwrap().is_empty());
    assert!(http.requests.lock().unwrap().is_empty());
}
#[tokio::test]
async fn due_certificate_renewal_uses_the_existing_session_and_publishes_the_same_identity() {
    let (mut record, prepared, storage) = fixture();
    record.refresh_at = super::super::interaction::now() - 1;
    *storage.record.lock().unwrap() = record.clone();
    let mut renewed = record.clone();
    renewed.expires_at += 3600;
    renewed.refresh_at = renewed.expires_at - 1800;
    renewed.certificate_chain = Zeroizing::new(certificate(
        &KeyMaterial::from_seed([7; 32]).unwrap(),
        renewed.expires_at,
    ));
    let http = mock(vec![
        Answer::Json(user()),
        Answer::Json(permissions()),
        cert_answer(&renewed),
    ]);
    let lease = Lease {
        current: record,
        persistence: storage.clone(),
        generation: Some(1),
        http: http.clone(),
        peer: None,
    };
    let original = prepared.proton.unwrap();
    let (sender, mut observed) = watch::channel(original.clone());
    let cancel = CancellationToken::new();
    let worker_cancel = cancel.clone();
    let worker = tokio::spawn(async move { lease.run(sender, &worker_cancel).await });
    observed.changed().await.unwrap();
    assert_eq!(
        observed.borrow().private_key_pkcs8,
        original.private_key_pkcs8
    );
    assert_eq!(observed.borrow().server_name, original.server_name);
    assert_eq!(observed.borrow().expires_at, renewed.expires_at);
    cancel.cancel();
    assert_eq!(worker.await.unwrap(), Ok(()));
    assert_eq!(http.requests.lock().unwrap().len(), 3);
}
#[tokio::test]
async fn expiry_stops_a_lease_before_any_further_network_request() {
    let (mut record, prepared, storage) = fixture();
    record.expires_at = super::super::interaction::now();
    record.refresh_at = record.expires_at - 1;
    let http = mock(Vec::new());
    let lease = Lease {
        current: record,
        persistence: storage,
        generation: Some(1),
        http: http.clone(),
        peer: None,
    };
    let (sender, _) = watch::channel(prepared.proton.unwrap());
    assert_eq!(
        lease.run(sender, &CancellationToken::new()).await,
        Err(GateFailure::Certificate)
    );
    assert!(http.requests.lock().unwrap().is_empty());
}
struct BlockingStore {
    inner: Arc<MemoryStore>,
    entered: tokio::sync::Notify,
    released: (Mutex<bool>, std::sync::Condvar),
}
impl LeasePersistence for BlockingStore {
    fn load(&self) -> Result<Record, ImportError> {
        self.inner.load()
    }
    fn lock(&self) -> Result<File, ImportError> {
        self.inner.lock()
    }
    fn save(&self, record: &Record) -> Result<(), ImportError> {
        self.inner.save(record)?;
        self.entered.notify_one();
        let (mutex, condition) = &self.released;
        let mut released = mutex.lock().unwrap();
        while !*released {
            released = condition.wait(released).unwrap();
        }
        Ok(())
    }
}

#[tokio::test]
async fn cancellation_waits_for_the_owned_journal_write_and_sends_no_request_after_it() {
    let (_, mut prepared, storage) = fixture();
    let blocking = Arc::new(BlockingStore {
        inner: storage.clone(),
        entered: tokio::sync::Notify::new(),
        released: (Mutex::new(false), std::sync::Condvar::new()),
    });
    prepared.proton_persistence = Some(blocking.clone());
    let http = mock(vec![unauthorized()]);
    let worker_http = http.clone();
    let cancel = CancellationToken::new();
    let worker_cancel = cancel.clone();
    let worker =
        tokio::spawn(
            async move { prepare_with_http(&mut prepared, worker_http, &worker_cancel).await },
        );
    blocking.entered.notified().await;
    cancel.cancel();
    tokio::task::yield_now().await;
    let still_owned = !worker.is_finished();
    *blocking.released.0.lock().unwrap() = true;
    blocking.released.1.notify_all();
    assert!(worker.await.unwrap().is_err());
    assert!(still_owned);
    assert!(storage.record.lock().unwrap().rotation_pending);
    assert_eq!(storage.writes.lock().unwrap().len(), 1);
    assert_eq!(http.requests.lock().unwrap().len(), 1);
}

#[test]
fn a_different_key_peer_revision_or_domain_cannot_borrow_the_runtime_lease() {
    let (record, prepared, _) = fixture();
    assert!(bound(&prepared, &record).is_ok());
    for field in 0..4 {
        let mut wrong = record.clone();
        match field {
            0 => wrong.ed25519_seed[0] ^= 1,
            1 => wrong.node.public_key = STANDARD.encode([9; 32]),
            2 => wrong.revision = Uuid::new_v4(),
            _ => wrong.node.domain = "other.example.invalid".into(),
        }
        assert!(bound(&prepared, &wrong).is_err());
    }
}
