//! The actual guest/provision/store/attach path against a synthetic API peer.
//! The fixture codec is deterministic; AEAD/platform protection has separate
//! core/platform tests. No remote request, socket, TUN or host setting is used.
use super::{
    api::{Client, Http},
    workflow::{Storage, provision},
    *,
};
use crate::internal_network::{InternalHttpError, InternalRequest, InternalResponse};
use base64::{Engine, engine::general_purpose::STANDARD};
use boring::{
    asn1::Asn1Time,
    bn::BigNum,
    ec::{EcGroup, EcKey},
    hash::MessageDigest,
    nid::Nid,
    pkey::PKey,
    x509::{X509, X509NameBuilder},
};
use http::Method;
use serde_json::{Value, json};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU64, Ordering},
};
use usque_core::{
    Profile,
    chain_exit::{
        ChainSource, ImportError,
        store::{ChainProfileStore, ProfileCipher},
    },
    proton_vpn::{Node, Request, store::SessionStore},
};
use uuid::Uuid;
use zeroize::Zeroizing;

struct FixtureCipher;
impl ProfileCipher for FixtureCipher {
    fn seal(&self, id: Uuid, value: &[u8]) -> Result<Vec<u8>, ImportError> {
        let mut result = id.as_bytes().to_vec();
        result.extend(value.iter().map(|b| b ^ 0xa5));
        Ok(result)
    }
    fn open(&self, id: Uuid, value: &[u8]) -> Result<Zeroizing<Vec<u8>>, ImportError> {
        if value.len() < 16 || &value[..16] != id.as_bytes() {
            return Err(usque_core::proton_vpn::error("invalid_ciphertext"));
        }
        Ok(Zeroizing::new(
            value[16..].iter().map(|b| b ^ 0xa5).collect(),
        ))
    }
}

#[test]
fn setup_job_starts_without_a_warp_account_or_underlay() {
    let directory = tempfile::tempdir().unwrap();
    let manager = Arc::new(Manager::new(
        directory.path().join("profiles-v2.json"),
        Arc::new(FixtureCipher),
    ));
    // Invalid credentials stop before any HTTP exchange. This exercises the
    // real worker dispatch on an installation with no WARP configuration.
    let mut request = guest();
    request.action = "login".into();
    let result = manager
        .command(request, Zeroizing::new(b"{}".to_vec()), None)
        .unwrap();
    let id = result.metadata.job.unwrap().id;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        let request = Request::parse(&format!(r#"{{"action":"get","job_id":"{id}"}}"#)).unwrap();
        let result = manager
            .command(request, Zeroizing::new(Vec::new()), None)
            .unwrap();
        let job = result.metadata.job.unwrap();
        if job.state == "failed" {
            assert_eq!(job.failure.as_deref(), Some("invalid_credentials_input"));
            break;
        }
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(manager.stop_blocking());
}
struct Peer {
    seen: Mutex<Vec<(String, String, String)>>,
    generation: AtomicU64,
    cancel_on_retire: Option<CancellationToken>,
    invalid_directory: bool,
    authenticated_uid: String,
    failed_revocation: Mutex<Option<String>>,
}
#[async_trait]
impl Http for Peer {
    async fn exchange(
        &self,
        request: InternalRequest<'_>,
        _: &CancellationToken,
    ) -> Result<InternalResponse, InternalHttpError> {
        let path = request
            .url
            .strip_prefix(usque_core::proton_vpn::API_ORIGIN)
            .unwrap();
        let uid = request
            .headers
            .iter()
            .find(|(k, _)| *k == "x-pm-uid")
            .map(|(_, v)| (*v).to_owned())
            .unwrap_or_default();
        self.seen
            .lock()
            .unwrap()
            .push((request.method.to_string(), path.into(), uid.clone()));
        let value = match path {
            "/auth/v4/sessions" => {
                json!({"Code":1000,"UID":"synthetic-anonymous","AccessToken":"synthetic-private-anonymous-access","RefreshToken":"synthetic-private-anonymous-refresh"})
            }
            "/auth/v4/credentialless" => {
                json!({"Code":1000,"UID":self.authenticated_uid,"AccessToken":"synthetic-private-access","RefreshToken":"synthetic-private-refresh","UserID":"synthetic-user"})
            }
            "/auth/v4/refresh" => {
                json!({"Code":1000,"UID":"synthetic-auth","AccessToken":"synthetic-private-fresh-access","RefreshToken":"synthetic-private-fresh-refresh","UserID":"synthetic-user"})
            }
            "/core/v4/users" => json!({"Code":1000,"User":{"ID":"synthetic-user"}}),
            "/auth/v4/scopes" => json!({"Code":1000,"Scopes":["vpn"]}),
            "/vpn/v2" => {
                json!({"Code":1000,"VPN":{"Status":1,"MaxConnect":1,"MaxTier":0},"Delinquent":0})
            }
            "/vpn/v2/clientconfig" => {
                json!({"Code":1000,"DefaultPorts":{"WireGuard":{"UDP":[51820]}}})
            }
            "/vpn/v2/logicals?WithEntriesForProtocols=WireGuardUDP&WithState=true" => {
                json!({"Code":1000,"StatusID":"synthetic-status","LogicalServers":[{"ID":"logical","Name":"Synthetic Free","ExitCountry":"IS","City":"Synthetic","Tier":0,"Features":0,"StatusReference":{"Index":0,"Penalty":0.0,"Cost":0},"Servers":[{"ID":"physical","Status":1,"Domain":"agent.example.invalid","X25519PublicKey":STANDARD.encode([8u8;32]),"EntryIP":if self.invalid_directory{"127.0.0.1"}else{"8.8.8.8"}}]}]})
            }
            "/vpn/v1/certificate" => {
                let payload: Value = serde_json::from_slice(&request.body).unwrap();
                assert_eq!(payload["Mode"], "session");
                assert_eq!(payload["ClientPublicKeyMode"], "EC");
                let public = PKey::public_key_from_pem(
                    payload["ClientPublicKey"].as_str().unwrap().as_bytes(),
                )
                .unwrap();
                let expires = interaction::now() + 3600;
                let mut name = X509NameBuilder::new().unwrap();
                name.append_entry_by_text("CN", "synthetic-client").unwrap();
                let name = name.build();
                let mut cert = X509::builder().unwrap();
                cert.set_version(2).unwrap();
                cert.set_serial_number(&BigNum::from_u32(1).unwrap().to_asn1_integer().unwrap())
                    .unwrap();
                cert.set_subject_name(&name).unwrap();
                cert.set_issuer_name(&name).unwrap();
                cert.set_pubkey(&public).unwrap();
                cert.set_not_before(&Asn1Time::from_unix(interaction::now() - 60).unwrap())
                    .unwrap();
                cert.set_not_after(&Asn1Time::from_unix(expires).unwrap())
                    .unwrap();
                let group = EcGroup::from_curve_name(Nid::X9_62_PRIME256V1).unwrap();
                let issuer = PKey::from_ec_key(EcKey::generate(&group).unwrap()).unwrap();
                cert.sign(&issuer, MessageDigest::sha256()).unwrap();
                json!({"Code":1000,"Certificate":String::from_utf8(cert.build().to_pem().unwrap()).unwrap(),"ExpirationTime":expires,"RefreshTime":expires-1800})
            }
            "/auth/v4" if request.method == Method::DELETE => {
                if self.failed_revocation.lock().unwrap().as_deref() == Some(&uid) {
                    return Err(InternalHttpError::Timeout);
                }
                if let Some(cancel) = &self.cancel_on_retire {
                    cancel.cancel();
                }
                json!({"Code":1000})
            }
            "/auth/v4/info" => {
                let fixture: Value = serde_json::from_str(include_str!(
                    "../../../../tool/proton_poc/testdata/srp_v4.json"
                ))
                .unwrap();
                json!({"Code":1000,"Version":4,"Salt":fixture["salt"],"Modulus":fixture["signed_modulus"],"ServerEphemeral":fixture["server_ephemeral"],"SRPSession":"synthetic-private-srp-session"})
            }
            "/auth/v4" => {
                json!({"Code":1000,"UID":"synthetic-auth","AccessToken":"synthetic-private-access","RefreshToken":"synthetic-private-refresh","UserID":"synthetic-user","ServerProof":STANDARD.encode([0u8;256]),"2FA":{"Enabled":0}})
            }
            _ => panic!("unexpected synthetic API path"),
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
fn peer(cancel: Option<CancellationToken>, invalid: bool) -> Arc<Peer> {
    Arc::new(Peer {
        seen: Mutex::new(Vec::new()),
        generation: AtomicU64::new(1),
        cancel_on_retire: cancel,
        invalid_directory: invalid,
        authenticated_uid: "synthetic-auth".into(),
        failed_revocation: Mutex::new(None),
    })
}
struct Select;
#[async_trait]
impl Interaction for Select {
    async fn verify(
        &self,
        _: Zeroizing<String>,
        _: Vec<String>,
        _: &CancellationToken,
    ) -> Result<(Zeroizing<String>, String), Failure> {
        Err(Failure::new("unexpected_verification"))
    }
    async fn totp(&self, _: &CancellationToken) -> Result<Zeroizing<String>, Failure> {
        Err(Failure::new("unexpected_totp"))
    }
    fn phase(&self, _: &str) {}
}
fn guest() -> Request {
    Request {
        action: "guest".into(),
        job_id: None,
        profile_id: None,
        challenge_id: None,
        node_id: None,
        name: "Synthetic Proton".into(),
        offset: 0,
    }
}

#[tokio::test]
async fn guest_provisions_persistent_session_profile_and_bound_runtime_credentials() {
    let directory = tempfile::tempdir().unwrap();
    let http = peer(None, false);
    let mut client = Client::new(http.clone());
    let cancel = CancellationToken::new();
    let id = provision(
        &mut client,
        &guest(),
        Zeroizing::new(Vec::new()),
        Storage {
            parent: directory.path(),
            cipher: Arc::new(FixtureCipher),
            accounts: Default::default(),
        },
        &Select,
        &cancel,
    )
    .await
    .unwrap();
    assert!(client.session.uid.is_empty());
    assert!(client.retired.is_empty());
    let sessions = SessionStore::new(directory.path(), &FixtureCipher);
    let record = sessions.load(id).unwrap();
    assert_eq!(record.session.access_token, "synthetic-private-access");
    assert!(!record.rotation_pending);
    assert_eq!(record.version, 4);
    assert!(record.certificate_chain.is_empty());
    assert_eq!(record.server_catalog.len(), 1);
    assert!(record.server_id.is_none());
    let chain = ChainProfileStore::new(directory.path(), &FixtureCipher);
    let profiles = chain.list().unwrap();
    assert_eq!(profiles.len(), 1);
    assert_eq!(profiles[0].source, ChainSource::ProtonVpn);
    assert_eq!(profiles[0].id, id);
    let status = serde_json::to_string(&profiles).unwrap();
    for secret in [
        "synthetic-private",
        "synthetic-user",
        "PRIVATE KEY",
        "PrivateKey",
    ] {
        assert!(!status.contains(secret));
    }
    let profile = Profile {
        chain_exit: Some(profiles[0].selection()),
        ..Default::default()
    };
    let (_, mut prepared) =
        usque_core::chain_exit::prepare_selection(directory.path(), &profile, &FixtureCipher)
            .unwrap()
            .unwrap();
    attach(directory.path(), Arc::new(FixtureCipher), &mut prepared).unwrap();
    assert!(prepared.proton.is_some());
    assert!(prepared.proton_persistence.is_some());
    let seen = http.seen.lock().unwrap();
    assert_eq!(
        seen.iter()
            .map(|(_, path, _)| path.as_str())
            .collect::<Vec<_>>(),
        [
            "/auth/v4/sessions",
            "/auth/v4/credentialless",
            "/auth/v4/scopes",
            "/vpn/v2",
            "/vpn/v2/clientconfig",
            "/vpn/v2/logicals?WithEntriesForProtocols=WireGuardUDP&WithState=true",
            "/auth/v4",
        ]
    );
    let revoked: Vec<_> = seen
        .iter()
        .filter(|(method, _, _)| method == "DELETE")
        .collect();
    assert_eq!(revoked.len(), 1);
    assert_eq!(revoked[0].2, "synthetic-anonymous");
    let disk = std::fs::read(
        directory
            .path()
            .join("proton-vpn")
            .join(format!("{id}.sealed")),
    )
    .unwrap();
    assert!(!String::from_utf8_lossy(&disk).contains("synthetic-private"));
}
#[tokio::test]
async fn cancellation_during_retired_cleanup_cannot_commit_new_profile() {
    let directory = tempfile::tempdir().unwrap();
    let cancel = CancellationToken::new();
    let http = peer(Some(cancel.clone()), false);
    let mut client = Client::new(http);
    let failure = provision(
        &mut client,
        &guest(),
        Zeroizing::new(Vec::new()),
        Storage {
            parent: directory.path(),
            cipher: Arc::new(FixtureCipher),
            accounts: Default::default(),
        },
        &Select,
        &cancel,
    )
    .await
    .err()
    .unwrap();
    assert_eq!(failure.reason, "cancelled");
    assert!(
        ChainProfileStore::new(directory.path(), &FixtureCipher)
            .list()
            .unwrap()
            .is_empty()
    );
    assert!(
        SessionStore::new(directory.path(), &FixtureCipher)
            .list()
            .unwrap()
            .is_empty()
    );
    assert!(client.revoke_all(&CancellationToken::new()).await);
}
#[tokio::test]
async fn forbidden_directory_never_creates_configuration_and_known_sessions_can_be_revoked() {
    let directory = tempfile::tempdir().unwrap();
    let http = peer(None, true);
    let mut client = Client::new(http.clone());
    let failure = provision(
        &mut client,
        &guest(),
        Zeroizing::new(Vec::new()),
        Storage {
            parent: directory.path(),
            cipher: Arc::new(FixtureCipher),
            accounts: Default::default(),
        },
        &Select,
        &CancellationToken::new(),
    )
    .await
    .err()
    .unwrap();
    assert_eq!(failure.reason, "no_free_wireguard_nodes");
    assert!(client.revoke_all(&CancellationToken::new()).await);
    assert_eq!(
        http.seen
            .lock()
            .unwrap()
            .iter()
            .filter(|(method, _, _)| method == "DELETE")
            .count(),
        2
    );
    assert!(
        ChainProfileStore::new(directory.path(), &FixtureCipher)
            .list()
            .unwrap()
            .is_empty()
    );
}
#[tokio::test]
async fn login_is_independent_and_server_proof_precedes_any_protected_request() {
    let http = peer(None, false);
    let mut client = Client::new(http.clone());
    let failure = workflow::login(
        &mut client,
        Zeroizing::new(br#"{"username":"synthetic-user","password":"abc123"}"#.to_vec()),
        &Select,
        &CancellationToken::new(),
    )
    .await
    .err()
    .unwrap();
    assert_eq!(failure.reason, "cryptographic_validation_failed");
    {
        let seen = http.seen.lock().unwrap();
        assert_eq!(seen.len(), 2);
        assert_eq!(seen[0].1, "/auth/v4/info");
        assert_eq!(seen[1].1, "/auth/v4");
    }
    assert!(client.revoke_all(&CancellationToken::new()).await);
    assert_eq!(http.seen.lock().unwrap().len(), 3);
}

#[tokio::test]
async fn explicit_forget_removes_pending_local_identity_without_network_or_touching_other_sources()
{
    let directory = tempfile::tempdir().unwrap();
    let http = peer(None, false);
    let mut client = Client::new(http.clone());
    let id = provision(
        &mut client,
        &guest(),
        Zeroizing::new(Vec::new()),
        Storage {
            parent: directory.path(),
            cipher: Arc::new(FixtureCipher),
            accounts: Default::default(),
        },
        &Select,
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    let sessions = SessionStore::new(directory.path(), &FixtureCipher);
    let mut record = sessions.load(id).unwrap();
    record.rotation_pending = true;
    sessions.save(&record).unwrap();
    let chain = ChainProfileStore::new(directory.path(), &FixtureCipher);
    let managed = chain.list().unwrap().remove(0);
    let (_, _, configuration) = chain.load(&managed.selection()).unwrap();
    let other = chain
        .import(ChainSource::WireguardCustom, "Unrelated", configuration)
        .unwrap();
    assert_eq!(
        workflow::forget(other.id, directory.path(), &FixtureCipher)
            .unwrap_err()
            .reason,
        "invalid_profile_source"
    );
    let requests = http.seen.lock().unwrap().len();
    let manager = Arc::new(Manager::new(
        directory.path().join("profiles-v2.json"),
        Arc::new(FixtureCipher),
    ));
    let mut request = guest();
    request.action = "forget".into();
    request.profile_id = Some(id);
    let response = manager
        .command(request.clone(), Zeroizing::new(Vec::new()), None)
        .unwrap();
    assert_eq!(response.metadata.job.unwrap().state, "completed");
    assert!(response.metadata.profiles.is_empty());
    assert!(sessions.list().unwrap().is_empty());
    let remaining = chain.list().unwrap();
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].id, other.id);
    manager
        .command(request, Zeroizing::new(Vec::new()), None)
        .unwrap();
    assert_eq!(http.seen.lock().unwrap().len(), requests);
}

async fn saved_fixture(parent: &std::path::Path) -> (Uuid, Arc<Peer>) {
    let http = peer(None, false);
    let mut client = Client::new(http.clone());
    let id = provision(
        &mut client,
        &guest(),
        Zeroizing::new(Vec::new()),
        Storage {
            parent,
            cipher: Arc::new(FixtureCipher),
            accounts: Default::default(),
        },
        &Select,
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    (id, http)
}
fn duplicate(parent: &std::path::Path, id: Uuid) -> Uuid {
    let sessions = SessionStore::new(parent, &FixtureCipher);
    let mut record = sessions.load(id).unwrap();
    let chain = ChainProfileStore::new(parent, &FixtureCipher);
    let summary = chain
        .list()
        .unwrap()
        .into_iter()
        .find(|profile| profile.id == id)
        .unwrap();
    let (_, _, config) = chain.load(&summary.selection()).unwrap();
    let copy = chain
        .import(ChainSource::ProtonVpn, "Legacy duplicate", config)
        .unwrap();
    record.id = copy.id;
    record.revision = copy.revision;
    record.session.uid = "legacy-uid".into();
    sessions.save(&record).unwrap();
    copy.id
}
fn manager(parent: &std::path::Path) -> Arc<Manager> {
    Arc::new(Manager::new(
        parent.join("profiles-v2.json"),
        Arc::new(FixtureCipher),
    ))
}
fn request(action: &str, id: Uuid) -> Request {
    Request::parse(&format!(r#"{{"action":"{action}","profile_id":"{id}"}}"#)).unwrap()
}
#[tokio::test]
async fn legacy_duplicates_are_one_account_and_every_related_active_or_selected_id_is_protected() {
    let directory = tempfile::tempdir().unwrap();
    let parent = directory.path();
    let (id, _) = saved_fixture(parent).await;
    let legacy = duplicate(parent, id);
    let manager = manager(parent);
    let summary = manager
        .command(request("list", id), Zeroizing::new(Vec::new()), None)
        .unwrap()
        .metadata
        .profiles
        .remove(0);
    assert_eq!(summary.related_ids.len(), 2);
    assert!(summary.related_ids.contains(&legacy));
    let metadata = serde_json::to_string(&summary).unwrap();
    assert!(!metadata.contains("synthetic-user"));
    let sessions = SessionStore::new(parent, &FixtureCipher);
    let mut selected = sessions.load(id).unwrap();
    selected.rotation_pending = true;
    sessions.save(&selected).unwrap();
    let selected_summary = manager
        .command_scoped(
            request("list", id),
            Zeroizing::new(Vec::new()),
            None,
            Default::default(),
            AccountContext {
                active: Vec::new(),
                selected: Some(id),
            },
        )
        .unwrap()
        .metadata
        .profiles
        .remove(0);
    assert_eq!(selected_summary.id, id);
    assert!(selected_summary.needs_login); // never hide the selected stale lease behind a healthy duplicate

    for action in ["refresh", "forget", "logout", "choose_server"] {
        let failure = manager
            .command_scoped(
                request(action, id),
                Zeroizing::new(Vec::new()),
                None,
                Default::default(),
                AccountContext {
                    active: vec![legacy],
                    selected: None,
                },
            )
            .err()
            .unwrap();
        assert_eq!(failure.reason, "profile_in_use");
    }
    for action in ["forget", "logout"] {
        let failure = manager
            .command_scoped(
                request(action, id),
                Zeroizing::new(Vec::new()),
                None,
                Default::default(),
                AccountContext {
                    active: Vec::new(),
                    selected: Some(legacy),
                },
            )
            .err()
            .unwrap();
        assert_eq!(failure.reason, "profile_in_use");
    }
    workflow::forget(id, parent, &FixtureCipher).unwrap();
    assert!(
        SessionStore::new(parent, &FixtureCipher)
            .list()
            .unwrap()
            .is_empty()
    );
    assert!(
        ChainProfileStore::new(parent, &FixtureCipher)
            .list()
            .unwrap()
            .is_empty()
    );
}
#[tokio::test]
async fn local_catalogue_pages_are_complete_revision_bound_and_choices_do_not_request_certificates()
{
    let directory = tempfile::tempdir().unwrap();
    let parent = directory.path();
    let (id, http) = saved_fixture(parent).await;
    let sessions = SessionStore::new(parent, &FixtureCipher);
    let mut record = sessions.load(id).unwrap();
    record.server_catalog = (0..301)
        .map(|i| Node {
            id: format!("logical-{i}"),
            name: format!("Server {i}"),
            country: "IS".into(),
            city: "".into(),
            load: 0,
        })
        .collect();
    sessions.save(&record).unwrap();
    let requests = http.seen.lock().unwrap().len();
    let manager = manager(parent);
    let mut ids = Vec::new();
    for (offset, count) in [(0, 128), (128, 128), (256, 45), (301, 0)] {
        let mut command = request("servers", id);
        command.offset = offset;
        let result = manager
            .command(command, Zeroizing::new(Vec::new()), None)
            .unwrap()
            .metadata;
        assert_eq!(result.servers.len(), count);
        assert_eq!(result.server_total, 301);
        assert_eq!(result.server_offset, offset);
        assert_eq!(result.server_revision, Some(record.catalog_revision));
        ids.extend(result.servers.into_iter().map(|node| node.id));
    }
    assert_eq!(ids.len(), 301);
    assert_eq!(ids.last().unwrap(), "logical-300");
    let mut command = request("choose_server", id);
    command.node_id = Some("logical-300".into());
    manager
        .command(command.clone(), Zeroizing::new(Vec::new()), None)
        .unwrap();
    assert_eq!(
        sessions.load(id).unwrap().server_id.as_deref(),
        Some("logical-300")
    );
    command.node_id = None;
    manager
        .command(command.clone(), Zeroizing::new(Vec::new()), None)
        .unwrap();
    assert!(sessions.load(id).unwrap().server_id.is_none());
    command.node_id = Some("unlisted".into());
    assert!(
        manager
            .command(command, Zeroizing::new(Vec::new()), None)
            .is_err()
    );
    assert_eq!(http.seen.lock().unwrap().len(), requests);
    assert!(sessions.load(id).unwrap().certificate_chain.is_empty());
}
#[tokio::test]
async fn repeat_authentication_updates_the_selected_account_without_duplicating_profiles_or_keys() {
    let directory = tempfile::tempdir().unwrap();
    let parent = directory.path();
    let (id, _) = saved_fixture(parent).await;
    let before = SessionStore::new(parent, &FixtureCipher).load(id).unwrap();
    let http = peer(None, false);
    let mut client = Client::new(http);
    let mut command = guest();
    command.profile_id = Some(id);
    let updated = provision(
        &mut client,
        &command,
        Zeroizing::new(Vec::new()),
        Storage {
            parent,
            cipher: Arc::new(FixtureCipher),
            accounts: AccountContext {
                active: Vec::new(),
                selected: Some(id),
            },
        },
        &Select,
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(updated, id);
    let sessions = SessionStore::new(parent, &FixtureCipher);
    let after = sessions.load(id).unwrap();
    assert_eq!(after.ed25519_seed, before.ed25519_seed);
    assert_eq!(after.revision, before.revision);
    assert_eq!(sessions.records().unwrap().len(), 1);
    assert_eq!(
        ChainProfileStore::new(parent, &FixtureCipher)
            .list()
            .unwrap()
            .len(),
        1
    );
}
#[tokio::test]
async fn sign_out_revokes_all_owned_legacy_sessions_before_removing_the_account() {
    let directory = tempfile::tempdir().unwrap();
    let parent = directory.path();
    let (id, http) = saved_fixture(parent).await;
    duplicate(parent, id);
    let mut client = Client::new(http.clone());
    workflow::logout(
        &mut client,
        id,
        parent,
        &FixtureCipher,
        &Select,
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    let seen = http.seen.lock().unwrap();
    let revoked: Vec<_> = seen
        .iter()
        .filter(|(method, _, _)| method == "DELETE")
        .map(|(_, _, uid)| uid.as_str())
        .collect();
    assert!(revoked.contains(&"synthetic-auth"));
    assert!(revoked.contains(&"legacy-uid"));
    assert!(
        SessionStore::new(parent, &FixtureCipher)
            .records()
            .unwrap()
            .is_empty()
    );
    assert!(
        ChainProfileStore::new(parent, &FixtureCipher)
            .list()
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn failed_group_revocation_keeps_all_local_handles_and_requires_explicit_local_removal() {
    let directory = tempfile::tempdir().unwrap();
    let parent = directory.path();
    let (id, http) = saved_fixture(parent).await;
    duplicate(parent, id);
    *http.failed_revocation.lock().unwrap() = Some("synthetic-auth".into());
    let mut client = Client::new(http);
    let failure = workflow::logout(
        &mut client,
        id,
        parent,
        &FixtureCipher,
        &Select,
        &CancellationToken::new(),
    )
    .await
    .err()
    .unwrap();
    assert_eq!(failure.reason, "revocation_failed_remote_state_unknown");
    let records = SessionStore::new(parent, &FixtureCipher).records().unwrap();
    assert_eq!(records.len(), 2);
    assert!(records.iter().all(|record| record.rotation_pending));
    assert_eq!(
        ChainProfileStore::new(parent, &FixtureCipher)
            .list()
            .unwrap()
            .len(),
        2
    );
    workflow::forget(id, parent, &FixtureCipher).unwrap();
    assert!(
        SessionStore::new(parent, &FixtureCipher)
            .records()
            .unwrap()
            .is_empty()
    );
}
#[tokio::test]
async fn old_session_cleanup_failure_never_revokes_the_new_durably_saved_account() {
    let directory = tempfile::tempdir().unwrap();
    let parent = directory.path();
    let (id, _) = saved_fixture(parent).await;
    let mut http = peer(None, false);
    Arc::get_mut(&mut http).unwrap().authenticated_uid = "new-auth".into();
    *http.failed_revocation.lock().unwrap() = Some("synthetic-auth".into());
    let mut client = Client::new(http.clone());
    let mut command = guest();
    command.profile_id = Some(id);
    let failure = provision(
        &mut client,
        &command,
        Zeroizing::new(Vec::new()),
        Storage {
            parent,
            cipher: Arc::new(FixtureCipher),
            accounts: Default::default(),
        },
        &Select,
        &CancellationToken::new(),
    )
    .await
    .err()
    .unwrap();
    assert_eq!(failure.reason, "cleanup_failed_remote_state_unknown");
    let record = SessionStore::new(parent, &FixtureCipher).load(id).unwrap();
    assert_eq!(record.session.uid, "new-auth");
    assert!(!record.rotation_pending);
    assert!(record.summary().cleanup_pending);
    assert_eq!(record.retired_sessions[0].uid, "synthetic-auth");
    assert!(client.revoke_all(&CancellationToken::new()).await);
    assert!(
        !http
            .seen
            .lock()
            .unwrap()
            .iter()
            .any(|(method, _, uid)| method == "DELETE" && uid == "new-auth")
    );
}
