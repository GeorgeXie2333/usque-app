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
    proton_vpn::{SelectedNode, SessionSecrets},
};
use uuid::Uuid;

struct MemoryStore {
    record: Mutex<Record>,
    writes: Mutex<Vec<Record>>,
}
impl LeasePersistence for MemoryStore {
    fn load(&self) -> Result<Record, ImportError> {
        Ok(self.record.lock().unwrap().clone())
    }
    fn save(&self, record: &Record) -> Result<(), ImportError> {
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
    Failed,
    Cancel(CancellationToken),
    ChangeGeneration,
}
struct MockHttp {
    answers: Mutex<VecDeque<Answer>>,
    requests: Mutex<Vec<String>>,
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
        let value = match self
            .answers
            .lock()
            .unwrap()
            .pop_front()
            .expect("unexpected network request")
        {
            Answer::Json(value) => value,
            Answer::Failed => return Err(InternalHttpError::Timeout),
            Answer::Cancel(cancel) => {
                cancel.cancel();
                refresh()
            }
            Answer::ChangeGeneration => {
                self.generation.store(2, Ordering::Release);
                refresh()
            }
        };
        Ok(InternalResponse {
            status: 200,
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
        generation: AtomicU64::new(1),
    })
}
fn refresh() -> Value {
    json!({"Code":1000,"UID":"uid","AccessToken":"fresh-access","RefreshToken":"fresh-refresh","UserID":"user"})
}
fn user() -> Value {
    json!({"Code":1000,"User":{"ID":"user"}})
}
fn permissions() -> Value {
    json!({"Code":1000,"VPN":{"Status":1,"MaxConnect":1,"MaxTier":0},"Delinquent":0})
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
        version: 1,
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
fn success_answers(record: &Record) -> Vec<Answer> {
    vec![
        Answer::Json(refresh()),
        Answer::Json(user()),
        Answer::Json(permissions()),
        Answer::Json(
            json!({"Code":1000,"Certificate":&*record.certificate_chain,"ExpirationTime":record.expires_at,"RefreshTime":record.refresh_at}),
        ),
    ]
}

#[tokio::test]
async fn rotation_journals_before_network_and_commits_new_tokens_and_same_identity() {
    let (record, prepared, storage) = fixture();
    let http = mock(success_answers(&record));
    let (credentials, lease) =
        prepare_with_http(&prepared, http.clone(), &CancellationToken::new())
            .await
            .unwrap();
    assert_eq!(
        credentials.private_key_pkcs8,
        prepared.proton.as_ref().unwrap().private_key_pkcs8
    );
    assert_eq!(credentials.server_name, record.node.domain);
    let writes = storage.writes.lock().unwrap();
    assert_eq!(writes.len(), 3);
    assert!(writes[0].rotation_pending);
    assert_eq!(writes[0].session.refresh_token, "old-refresh");
    assert!(!writes[1].rotation_pending);
    assert_eq!(writes[1].session.refresh_token, "fresh-refresh");
    assert_eq!(lease.current.session.user_id, "user");
    assert_eq!(http.requests.lock().unwrap().len(), 4);
}

#[tokio::test]
async fn ambiguous_rotation_and_interactive_challenge_remain_pending_and_are_never_replayed() {
    for answer in [
        Answer::Failed,
        Answer::Json(
            json!({"Code":9001,"Details":{"HumanVerificationToken":"challenge","HumanVerificationMethods":["captcha"]}}),
        ),
    ] {
        let (_, prepared, storage) = fixture();
        let http = mock(vec![answer]);
        assert!(
            prepare_with_http(&prepared, http.clone(), &CancellationToken::new())
                .await
                .is_err()
        );
        assert!(storage.record.lock().unwrap().rotation_pending);
        assert!(
            prepare_with_http(&prepared, http.clone(), &CancellationToken::new())
                .await
                .is_err()
        );
        assert_eq!(http.requests.lock().unwrap().len(), 1);
    }
}

#[tokio::test]
async fn cancelled_or_changed_underlay_never_commits_a_rotated_result() {
    for change_generation in [false, true] {
        let (_, prepared, storage) = fixture();
        let cancel = CancellationToken::new();
        let answer = if change_generation {
            Answer::ChangeGeneration
        } else {
            Answer::Cancel(cancel.clone())
        };
        let http = mock(vec![answer]);
        assert!(
            prepare_with_http(&prepared, http.clone(), &cancel)
                .await
                .is_err()
        );
        let record = storage.record.lock().unwrap();
        assert!(record.rotation_pending);
        assert_eq!(record.session.refresh_token, "old-refresh");
        assert_eq!(http.requests.lock().unwrap().len(), 1);
    }
}

#[tokio::test]
async fn protected_user_mismatch_never_commits_tokens_or_issues_certificate() {
    let (_, prepared, storage) = fixture();
    let http = mock(vec![
        Answer::Json(refresh()),
        Answer::Json(json!({"Code":1000,"User":{"ID":"another-user"}})),
    ]);
    assert!(
        prepare_with_http(&prepared, http.clone(), &CancellationToken::new())
            .await
            .is_err()
    );
    assert!(storage.record.lock().unwrap().rotation_pending);
    assert_eq!(http.requests.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn certificate_failure_preserves_the_already_known_replacement_token() {
    let (_, prepared, storage) = fixture();
    let http = mock(vec![
        Answer::Json(refresh()),
        Answer::Json(user()),
        Answer::Json(permissions()),
        Answer::Failed,
    ]);
    assert!(
        prepare_with_http(&prepared, http, &CancellationToken::new())
            .await
            .is_err()
    );
    let saved = storage.record.lock().unwrap();
    assert!(!saved.rotation_pending);
    assert_eq!(saved.session.refresh_token, "fresh-refresh");
}

#[tokio::test(start_paused = true)]
async fn sidecar_change_revokes_the_running_lease_without_network_or_overwriting_it() {
    let (record, _, storage) = fixture();
    let http = mock(Vec::new());
    let lease = Lease {
        current: record,
        persistence: storage.clone(),
        generation: Some(1),
        http: http.clone(),
    };
    let keys = KeyMaterial::from_seed([7; 32]).unwrap();
    let (sender, _) = watch::channel(Arc::new(TunnelCredentials {
        certificate_chain: Zeroizing::new(String::new()),
        private_key_pkcs8: keys.pkcs8_der(),
        server_name: "agent.example.invalid".into(),
        expires_at: super::super::interaction::now() + 3600,
    }));
    storage.record.lock().unwrap().rotation_pending = true;
    let result = lease.run(sender, &CancellationToken::new()).await;
    assert_eq!(result, Err(GateFailure::Authentication));
    assert!(storage.writes.lock().unwrap().is_empty());
    assert!(http.requests.lock().unwrap().is_empty());
}

#[tokio::test]
async fn a_due_lease_publishes_same_key_renewal_and_joins_on_cancellation() {
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
    let http = mock(success_answers(&renewed));
    let lease = Lease {
        current: record,
        persistence: storage.clone(),
        generation: Some(1),
        http: http.clone(),
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
    assert_eq!(http.requests.lock().unwrap().len(), 4);
    assert_eq!(
        storage.record.lock().unwrap().expires_at,
        renewed.expires_at
    );
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
    let http = mock(Vec::new());
    let worker_http = http.clone();
    let cancel = CancellationToken::new();
    let worker_cancel = cancel.clone();
    let worker =
        tokio::spawn(
            async move { prepare_with_http(&prepared, worker_http, &worker_cancel).await },
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
    assert!(http.requests.lock().unwrap().is_empty());
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
