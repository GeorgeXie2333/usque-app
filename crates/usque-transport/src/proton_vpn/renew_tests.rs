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
    FreshCertificate,
    Cancel(CancellationToken),
    ChangeGeneration,
    WaitForCancel(Arc<tokio::sync::Notify>),
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
        cancel: &CancellationToken,
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
        let answer = self
            .answers
            .lock()
            .unwrap()
            .pop_front()
            .expect("unexpected network request");
        let (status, value) = match answer {
            Answer::WaitForCancel(started) => {
                started.notify_one();
                cancel.cancelled().await;
                return Err(InternalHttpError::Cancelled);
            }
            Answer::Json(v) => (200, v),
            Answer::Status(status, v) => (status, v),
            Answer::FreshCertificate => {
                let body: Value = serde_json::from_slice(&request.body).unwrap();
                let public =
                    PKey::public_key_from_pem(body["ClientPublicKey"].as_str().unwrap().as_bytes())
                        .unwrap();
                let expiry = super::super::interaction::now() + 3600;
                (
                    200,
                    json!({"Code":1000,"Certificate":certificate_with_public(&public, expiry),"ExpirationTime":expiry,"RefreshTime":expiry-1800}),
                )
            }
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
    let public = PKey::public_key_from_pem(&private.public_key_to_pem().unwrap()).unwrap();
    certificate_with_public(&public, expiry)
}
fn certificate_with_public(
    public: &boring::pkey::PKeyRef<boring::pkey::Public>,
    expiry: i64,
) -> String {
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
    cert.set_pubkey(public).unwrap();
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
        label: None,
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
        tunnel_seed: None,
        key_recovery_at: 0,
        connection_cache: None,
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
        label: None,
    }));
    let storage = Arc::new(MemoryStore {
        record: Mutex::new(record.clone()),
        writes: Mutex::new(Vec::new()),
    });
    prepared.proton_persistence = Some(storage.clone());
    (record, prepared, storage)
}

fn cache_record(record: &mut Record) {
    record.promote();
    let now = super::super::interaction::now();
    let mut cache = usque_core::proton_vpn::ConnectionCache {
        permissions_at: now,
        ..Default::default()
    };
    let mut peer = record.node.clone();
    peer.metadata.id = "logical".into();
    cache.set_directory(&[peer], record.selected_server(), now);
    record.connection_cache = Some(cache);
}

#[tokio::test]
async fn warm_connection_uses_no_api_and_first_connection_only_requests_certificate() {
    for deferred in [false, true] {
        let (mut record, mut profile, store) = fixture();
        cache_record(&mut record);
        if deferred {
            record.certificate_chain.zeroize();
            record.expires_at = 0;
            record.refresh_at = 0;
        }
        *store.record.lock().unwrap() = record;
        let http = mock(if deferred {
            vec![Answer::FreshCertificate]
        } else {
            vec![]
        });
        let (_, lease) = prepare_with_http(&mut profile, http.clone(), &CancellationToken::new())
            .await
            .unwrap();
        let paths = http.requests.lock().unwrap();
        assert_eq!(paths.len(), usize::from(deferred));
        if deferred {
            assert!(paths[0].ends_with("/vpn/v1/certificate"));
        }
        assert_eq!(lease.current.node.metadata.id, "logical:physical");
        assert_eq!(lease.peer.unwrap().metadata.id, "logical");
    }
}

#[tokio::test]
async fn cache_expiry_and_clock_rollback_refetch_only_necessary_metadata() {
    for (permissions_age, directory_age, permission_requests, directory_requests) in [
        (300, 0, true, false),
        (0, 900, false, true),
        (-60, -60, true, true),
    ] {
        let (mut record, mut profile, store) = fixture();
        cache_record(&mut record);
        let cache = record.connection_cache.as_mut().unwrap();
        cache.permissions_at -= permissions_age;
        cache.directory_at -= directory_age;
        *store.record.lock().unwrap() = record;
        let mut answers = Vec::new();
        if permission_requests {
            answers.extend([Answer::Json(user()), Answer::Json(permissions())]);
        }
        if directory_requests {
            answers.extend([Answer::Json(config()), Answer::Json(directory())]);
        }
        let http = mock(answers);
        prepare_with_http(&mut profile, http.clone(), &CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(
            http.requests.lock().unwrap().len(),
            usize::from(permission_requests) * 2 + usize::from(directory_requests) * 2
        );
    }
}

#[tokio::test]
async fn uncached_selection_and_failed_endpoint_require_a_fresh_directory() {
    let (mut record, mut profile, store) = fixture();
    cache_record(&mut record);
    record.server_id = Some("missing".into());
    *store.record.lock().unwrap() = record;
    let http = mock(vec![Answer::Json(config()), Answer::Json(directory())]);
    let failure = prepare_with_http(&mut profile, http.clone(), &CancellationToken::new())
        .await
        .err()
        .unwrap();
    assert_eq!(failure.reason, "selected_server_unavailable");
    assert_eq!(http.requests.lock().unwrap().len(), 2);
    let before = store.load().unwrap();
    invalidate_peer(store.clone()).await.unwrap();
    let after = store.load().unwrap();
    assert!(after.connection_cache.unwrap().peers.is_empty());
    assert_eq!(after.server_id, before.server_id);
    assert_eq!(after.session.access_token, before.session.access_token);
}

#[tokio::test(start_paused = true)]
async fn metadata_refresh_waits_for_admission_and_never_replaces_live_peer() {
    let (mut record, prepared, store) = fixture();
    cache_record(&mut record);
    record.connection_cache.as_mut().unwrap().permissions_at -= 151;
    record.connection_cache.as_mut().unwrap().directory_at -= 451;
    *store.record.lock().unwrap() = record.clone();
    let http = mock(success_answers());
    let peer = record.node.clone();
    let lease = Lease {
        current: record,
        persistence: store.clone(),
        generation: Some(1),
        http: http.clone(),
        peer: Some(peer.clone()),
        retry_at: None,
    };
    let (credentials, observed) = watch::channel(prepared.proton.unwrap());
    let (admit, admitted) = watch::channel(false);
    let (_requests, receiver) = mpsc::channel(1);
    let cancel = CancellationToken::new();
    let task_cancel = cancel.clone();
    let worker = tokio::spawn(async move {
        lease
            .run_with_recovery(credentials, receiver, admitted, &task_cancel)
            .await
    });
    for _ in 0..100 {
        tokio::task::yield_now().await;
    }
    assert!(http.requests.lock().unwrap().is_empty());
    admit.send_replace(true);
    tokio::time::advance(MONITOR_INTERVAL).await;
    while store.writes.lock().unwrap().is_empty() {
        tokio::task::yield_now().await;
    }
    cancel.cancel();
    assert_eq!(worker.await.unwrap(), Ok(()));
    assert_eq!(http.requests.lock().unwrap().len(), 4);
    assert_eq!(observed.borrow().server_name, peer.domain);
    assert!(
        store
            .load()
            .unwrap()
            .connection_cache
            .unwrap()
            .permissions_fresh(super::super::interaction::now())
    );
}

#[tokio::test]
async fn metadata_tier_change_invalidates_certificate_and_cache_without_touching_peer_binding() {
    let (mut record, _, store) = fixture();
    cache_record(&mut record);
    record.connection_cache.as_mut().unwrap().permissions_at -= 151;
    *store.record.lock().unwrap() = record.clone();
    let mut permissions = permissions();
    permissions["VPN"]["MaxTier"] = json!(2);
    let http = mock(vec![Answer::Json(user()), Answer::Json(permissions)]);
    let mut lease = Lease {
        current: record.clone(),
        persistence: store.clone(),
        generation: Some(1),
        http: http.clone(),
        peer: Some(record.node.clone()),
        retry_at: None,
    };
    assert_eq!(
        lease
            .refresh_metadata(&CancellationToken::new())
            .await
            .err()
            .unwrap()
            .reason,
        "account_tier_changed"
    );
    let saved = store.load().unwrap();
    assert!(saved.connection_cache.is_none());
    assert!(saved.certificate_chain.is_empty());
    assert_eq!(saved.node.endpoint, record.node.endpoint);
    assert_eq!(saved.revision, record.revision);
    assert_eq!(saved.max_tier, 2);
}

#[tokio::test(start_paused = true)]
async fn transient_metadata_failure_preserves_admission_and_uses_backoff() {
    let (mut record, prepared, store) = fixture();
    cache_record(&mut record);
    record.connection_cache.as_mut().unwrap().permissions_at -= 151;
    *store.record.lock().unwrap() = record.clone();
    let http = mock(vec![
        Answer::Failed,
        Answer::Json(user()),
        Answer::Json(permissions()),
    ]);
    let lease = Lease {
        current: record.clone(),
        persistence: store.clone(),
        generation: Some(1),
        http: http.clone(),
        peer: Some(record.node.clone()),
        retry_at: None,
    };
    let (credentials, _observed) = watch::channel(prepared.proton.unwrap());
    let (_admit, admitted) = watch::channel(true);
    let (_requests, receiver) = mpsc::channel(1);
    let cancel = CancellationToken::new();
    let task_cancel = cancel.clone();
    let worker = tokio::spawn(async move {
        lease
            .run_with_recovery(credentials, receiver, admitted, &task_cancel)
            .await
    });
    while http.requests.lock().unwrap().is_empty() {
        tokio::task::yield_now().await;
    }
    for _ in 0..100 {
        tokio::task::yield_now().await;
    }
    assert!(!worker.is_finished());
    assert_eq!(http.requests.lock().unwrap().len(), 1);
    assert_eq!(
        store.load().unwrap().certificate_chain,
        record.certificate_chain
    );
    tokio::time::advance(Duration::from_secs(31)).await;
    while store.writes.lock().unwrap().is_empty() {
        tokio::task::yield_now().await;
    }
    cancel.cancel();
    assert_eq!(worker.await.unwrap(), Ok(()));
    assert_eq!(http.requests.lock().unwrap().len(), 3);
}

#[tokio::test]
async fn recovery_and_shutdown_preempt_and_await_metadata_requests() {
    for recovery in [false, true] {
        let (mut record, prepared, store) = fixture();
        cache_record(&mut record);
        record.connection_cache.as_mut().unwrap().permissions_at -= 151;
        *store.record.lock().unwrap() = record.clone();
        let started = Arc::new(tokio::sync::Notify::new());
        let mut answers = vec![Answer::WaitForCancel(started.clone())];
        if recovery {
            answers.extend([
                Answer::Json(user()),
                Answer::Json(permissions()),
                Answer::FreshCertificate,
            ]);
        }
        let http = mock(answers);
        let lease = Lease {
            current: record.clone(),
            persistence: store,
            generation: Some(1),
            http: http.clone(),
            peer: Some(record.node.clone()),
            retry_at: None,
        };
        let (credentials, _observed) = watch::channel(prepared.proton.unwrap());
        let (_admit, admitted) = watch::channel(true);
        let (requests, receiver) = mpsc::channel(1);
        let cancel = CancellationToken::new();
        let task_cancel = cancel.clone();
        let worker = tokio::spawn(async move {
            lease
                .run_with_recovery(credentials, receiver, admitted, &task_cancel)
                .await
        });
        started.notified().await;
        if recovery {
            let (reply, result) = oneshot::channel();
            requests
                .send(RecoveryRequest { code: 86101, reply })
                .await
                .unwrap();
            assert!(result.await.unwrap().is_ok());
        }
        cancel.cancel();
        assert_eq!(worker.await.unwrap(), Ok(()));
        assert_eq!(
            http.requests.lock().unwrap().len(),
            if recovery { 4 } else { 1 }
        );
    }
}

#[tokio::test]
async fn explicit_background_permission_denial_is_terminal_and_invalidates_hints() {
    let (mut record, _, store) = fixture();
    cache_record(&mut record);
    record.connection_cache.as_mut().unwrap().permissions_at -= 151;
    *store.record.lock().unwrap() = record.clone();
    let mut denied = permissions();
    denied["VPN"]["Status"] = json!(0);
    let http = mock(vec![Answer::Json(user()), Answer::Json(denied)]);
    let mut lease = Lease {
        current: record.clone(),
        persistence: store.clone(),
        generation: Some(1),
        http,
        peer: Some(record.node),
        retry_at: None,
    };
    let failure = lease
        .refresh_metadata(&CancellationToken::new())
        .await
        .err()
        .unwrap();
    assert_eq!(failure.reason, "vpn_permission_denied");
    assert!(!transient(&failure));
    assert!(store.load().unwrap().connection_cache.is_none());
}

#[tokio::test]
async fn runtime_preparation_sends_its_first_api_request_through_the_warp_handle() {
    use crate::{
        RuntimeHealth,
        netstack::RuntimePath,
        tcp::{DialError, FlowClass, TcpDialer, TcpStream, TcpTarget},
    };
    struct RefusingWarp(Mutex<Vec<String>>);
    #[async_trait]
    impl TcpDialer for RefusingWarp {
        async fn connect(
            &self,
            target: TcpTarget,
            _: tokio::time::Instant,
            _: &CancellationToken,
            _: FlowClass,
        ) -> Result<TcpStream, DialError> {
            self.0.lock().unwrap().push(target.authority().into());
            Err(DialError::Refused)
        }
    }
    let dialer = Arc::new(RefusingWarp(Mutex::new(Vec::new())));
    let (_health, receiver) = watch::channel(RuntimeHealth::Connected {
        path: RuntimePath {
            transport: usque_core::Transport::Http3,
            endpoint_family: usque_core::AddressFamily::Ipv4,
            ipv4_available: true,
            ipv6_available: false,
        },
        reconnect_count: 0,
    });
    let warp =
        crate::InternalNetwork::for_streams(dialer.clone(), receiver, CancellationToken::new());
    let (record, mut prepared, storage) = fixture();
    assert!(matches!(
        prepare(&mut prepared, warp, &CancellationToken::new()).await,
        Err(GateFailure::Proton(ConnectionFailure {
            kind: FailureKind::Transport,
            ..
        }))
    ));
    assert_eq!(
        *dialer.0.lock().unwrap(),
        vec!["vpn-api.proton.me:443".to_owned()]
    );
    assert!(storage.writes.lock().unwrap().is_empty());
    assert_eq!(storage.load().unwrap().session.uid, record.session.uid);
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
        retry_at: None,
    };
    let (sender, _) = watch::channel(prepared.proton.unwrap());
    storage.record.lock().unwrap().rotation_pending = true;
    assert_eq!(
        lease.run(sender, &CancellationToken::new()).await,
        Err(gate_failure(&Failure::new("reauthentication_required")))
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
        retry_at: None,
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
        retry_at: None,
    };
    let (sender, _) = watch::channel(prepared.proton.unwrap());
    assert_eq!(
        lease.run(sender, &CancellationToken::new()).await,
        Err(GateFailure::Proton(ConnectionFailure::new(
            FailureKind::Certificate
        )))
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

#[tokio::test]
async fn revoked_certificate_rotates_only_runtime_key_and_next_start_uses_it() {
    let (record, prepared, storage) = fixture();
    let http = mock(vec![
        Answer::Json(user()),
        Answer::Json(permissions()),
        Answer::FreshCertificate,
    ]);
    let mut lease = Lease {
        current: record.clone(),
        persistence: storage.clone(),
        generation: Some(1),
        http: http.clone(),
        peer: None,
        retry_at: None,
    };
    let updated = lease
        .recover(86102, &CancellationToken::new())
        .await
        .unwrap();
    let saved = storage.load().unwrap();
    assert_eq!(saved.version, 4);
    assert_eq!(saved.ed25519_seed, record.ed25519_seed);
    assert_ne!(saved.tunnel_seed(), record.tunnel_seed());
    assert_eq!(saved.revision, record.revision);
    assert_eq!(saved.session.uid, record.session.uid);
    assert_ne!(
        updated.private_key_pkcs8,
        prepared.proton.as_ref().unwrap().private_key_pkcs8
    );
    assert_eq!(http.requests.lock().unwrap().len(), 3);
    {
        let writes = storage.writes.lock().unwrap();
        assert!(writes[0].certificate_chain.is_empty());
        assert_eq!(writes[0].tunnel_seed, saved.tunnel_seed);
    }
    let failure = lease
        .recover(86105, &CancellationToken::new())
        .await
        .err()
        .unwrap();
    assert_eq!(
        failure,
        GateFailure::Proton(ConnectionFailure::agent(86105))
    );
    assert_eq!(http.requests.lock().unwrap().len(), 3);
    let mut next = prepared;
    let (credentials, _) = prepare_with_http(
        &mut next,
        mock(vec![Answer::Json(config()), Answer::Json(directory())]),
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(credentials.private_key_pkcs8, updated.private_key_pkcs8);
    assert_eq!(storage.load().unwrap().tunnel_seed, saved.tunnel_seed);
}

#[tokio::test]
async fn server_expiry_renews_certificate_without_changing_wireguard_key() {
    let (record, prepared, storage) = fixture();
    let http = mock(vec![
        Answer::Json(user()),
        Answer::Json(permissions()),
        Answer::FreshCertificate,
    ]);
    let mut lease = Lease {
        current: record.clone(),
        persistence: storage.clone(),
        generation: Some(1),
        http: http.clone(),
        peer: None,
        retry_at: None,
    };
    let updated = lease
        .recover(86101, &CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(
        updated.private_key_pkcs8,
        prepared.proton.unwrap().private_key_pkcs8
    );
    assert!(storage.load().unwrap().tunnel_seed.is_none());
    assert_eq!(storage.load().unwrap().key_recovery_at, 0);
    assert_eq!(http.requests.lock().unwrap().len(), 3);
}

#[tokio::test]
async fn cancelled_key_recovery_never_persists_or_requests_a_new_key() {
    let (record, _, storage) = fixture();
    let http = mock(Vec::new());
    let mut lease = Lease {
        current: record.clone(),
        persistence: storage.clone(),
        generation: Some(1),
        http: http.clone(),
        peer: None,
        retry_at: None,
    };
    let cancel = CancellationToken::new();
    cancel.cancel();
    assert!(lease.recover(86102, &cancel).await.is_err());
    assert!(storage.writes.lock().unwrap().is_empty());
    assert!(http.requests.lock().unwrap().is_empty());
    assert_eq!(storage.load().unwrap().tunnel_seed(), record.tunnel_seed());
}

#[tokio::test(start_paused = true)]
async fn temporary_renewal_failure_keeps_unexpired_certificate_and_retries_with_backoff() {
    let (mut record, prepared, storage) = fixture();
    record.refresh_at = super::super::interaction::now() - 1;
    *storage.record.lock().unwrap() = record.clone();
    let http = mock(vec![
        Answer::Json(user()),
        Answer::Json(permissions()),
        Answer::Status(503, json!({"Code":503})),
        Answer::Json(user()),
        Answer::Json(permissions()),
        Answer::FreshCertificate,
    ]);
    let lease = Lease {
        current: record,
        persistence: storage.clone(),
        generation: Some(1),
        http: http.clone(),
        peer: None,
        retry_at: None,
    };
    let original = prepared.proton.unwrap();
    let (sender, mut observed) = watch::channel(original.clone());
    let cancel = CancellationToken::new();
    let task_cancel = cancel.clone();
    let worker = tokio::spawn(async move { lease.run(sender, &task_cancel).await });
    while http.requests.lock().unwrap().len() < 3 {
        tokio::task::yield_now().await;
    }
    for _ in 0..20 {
        tokio::task::yield_now().await;
    }
    assert!(!worker.is_finished());
    assert_eq!(
        observed.borrow().certificate_chain,
        original.certificate_chain
    );
    assert_eq!(http.requests.lock().unwrap().len(), 3);
    tokio::time::advance(Duration::from_secs(31)).await;
    observed.changed().await.unwrap();
    assert_eq!(
        observed.borrow().private_key_pkcs8,
        original.private_key_pkcs8
    );
    cancel.cancel();
    assert_eq!(worker.await.unwrap(), Ok(()));
    assert_eq!(http.requests.lock().unwrap().len(), 6);
}

#[test]
fn api_failure_classification_preserves_provider_status_and_never_becomes_warp_auth() {
    for (reason, code, status, expected) in [
        ("api_rejected", 401, 401, FailureKind::Authentication),
        ("rate_limited", 429, 429, FailureKind::RateLimited),
        ("api_rejected", 503, 503, FailureKind::Api),
        ("invalid_server_directory", 1000, 200, FailureKind::Protocol),
    ] {
        let failure = gate_failure(&Failure::api(reason, code, status));
        assert_eq!(
            failure,
            GateFailure::Proton(ConnectionFailure {
                kind: expected,
                code: Some(code),
                http_status: Some(status)
            })
        );
        let public = crate::TransportError::VpnGate(failure).failure(None, None);
        assert!(public.code.as_str().starts_with("PROTON_"));
        assert_eq!(
            public.sanitized_detail.as_deref(),
            Some(format!("proton api {code} http {status}").as_str())
        );
        assert!(!public.fallback_allowed);
    }
}

#[tokio::test]
async fn connect_can_reuse_a_valid_certificate_after_a_transient_due_refresh_failure() {
    let (mut record, mut prepared, storage) = fixture();
    record.refresh_at = super::super::interaction::now() - 1;
    *storage.record.lock().unwrap() = record.clone();
    let mut answers = success_answers();
    answers.push(Answer::Status(503, json!({"Code":503})));
    let (credentials, lease) =
        prepare_with_http(&mut prepared, mock(answers), &CancellationToken::new())
            .await
            .unwrap();
    assert_eq!(credentials.certificate_chain, record.certificate_chain);
    assert!(lease.retry_at.is_some());
    assert!(lease.current.expires_at > super::super::interaction::now());
}
