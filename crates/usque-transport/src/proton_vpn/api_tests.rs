//! Synthetic HTTPS peers and actual interaction futures; no sockets or account.
use super::{
    api::{Client, Http},
    interaction::JobIo,
    *,
};
use crate::internal_network::{InternalHttpError, InternalRequest, InternalResponse};
use base64::{Engine, engine::general_purpose::STANDARD};
use http::Method;
use serde_json::{Value, json};
use std::{
    collections::VecDeque,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, AtomicUsize, Ordering},
    },
};
use tokio_util::sync::CancellationToken;
use usque_core::proton_vpn::{Request, SessionSecrets};
use zeroize::Zeroizing;

struct Seen {
    method: Method,
    url: String,
    body: Vec<u8>,
    headers: Vec<(String, String)>,
}

struct RecoveryPeer {
    inner: Arc<Peer>,
    recoveries: AtomicUsize,
}
#[async_trait]
impl Http for RecoveryPeer {
    async fn exchange(
        &self,
        request: InternalRequest<'_>,
        cancel: &CancellationToken,
    ) -> Result<InternalResponse, InternalHttpError> {
        self.inner.exchange(request, cancel).await
    }
    async fn recover(&self, _: &CancellationToken) -> Result<bool, Failure> {
        Ok(self.recoveries.fetch_add(1, Ordering::AcqRel) == 0)
    }
    fn generation(&self) -> Option<u64> {
        self.inner.generation()
    }
    fn current(&self, generation: Option<u64>) -> bool {
        self.inner.current(generation)
    }
}
struct BrokenVerification {
    calls: AtomicUsize,
    always: bool,
}
#[async_trait]
impl Interaction for BrokenVerification {
    async fn verify(
        &self,
        _: Zeroizing<String>,
        _: Vec<String>,
        _: &CancellationToken,
    ) -> Result<(Zeroizing<String>, String), Failure> {
        if self.calls.fetch_add(1, Ordering::AcqRel) == 0 || self.always {
            Err(Failure::new("verification_network_failed"))
        } else {
            Ok((
                Zeroizing::new("replacement-answer".into()),
                "captcha".into(),
            ))
        }
    }
    async fn totp(&self, _: &CancellationToken) -> Result<Zeroizing<String>, Failure> {
        unreachable!()
    }
    fn phase(&self, _: &str) {}
}

#[tokio::test]
async fn verification_recovery_reissues_once_only_after_a_known_rejection() {
    for (status, always) in [(400, false), (400, true), (200, false)] {
        let inner = make_peer(vec![
            Ok((status, challenge())),
            Ok((400, challenge())),
            Ok((200, json!({"Code":1000}))),
        ]);
        let http = Arc::new(RecoveryPeer {
            inner: inner.clone(),
            recoveries: AtomicUsize::new(0),
        });
        let io = BrokenVerification {
            calls: AtomicUsize::new(0),
            always,
        };
        let mut client = Client::new(http.clone());
        let answer = client
            .request(
                Method::POST,
                "/auth/v4",
                &json!({}),
                &io,
                &CancellationToken::new(),
            )
            .await;
        assert_eq!(answer.is_ok(), status == 400 && !always);
        assert_eq!(
            http.recoveries.load(Ordering::Acquire),
            usize::from(status == 400)
        );
        let seen = inner.seen.lock().unwrap();
        assert_eq!(
            seen.len(),
            if status == 200 {
                1
            } else if always {
                2
            } else {
                3
            }
        );
        assert!(
            !seen[0]
                .headers
                .iter()
                .any(|(name, _)| name.starts_with("x-pm-human-verification"))
        );
        if seen.len() > 1 {
            assert!(
                !seen[1]
                    .headers
                    .iter()
                    .any(|(name, _)| name.starts_with("x-pm-human-verification"))
            );
        }
        if seen.len() == 3 {
            assert!(
                seen[2]
                    .headers
                    .iter()
                    .any(|(name, value)| name == "x-pm-human-verification-token"
                        && value == "replacement-answer")
            );
        }
    }
}
struct Peer {
    replies: Mutex<VecDeque<Result<(u16, Value), InternalHttpError>>>,
    seen: Mutex<Vec<Seen>>,
    generation: AtomicU64,
}
#[async_trait]
impl Http for Peer {
    async fn exchange(
        &self,
        request: InternalRequest<'_>,
        _: &CancellationToken,
    ) -> Result<InternalResponse, InternalHttpError> {
        self.seen.lock().unwrap().push(Seen {
            method: request.method,
            url: request.url.into(),
            body: request.body.to_vec(),
            headers: request
                .headers
                .into_iter()
                .map(|(k, v)| (k.into(), v.into()))
                .collect(),
        });
        let (status, value) = self
            .replies
            .lock()
            .unwrap()
            .pop_front()
            .expect("unexpected synthetic request")?;
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
fn make_peer(replies: Vec<Result<(u16, Value), InternalHttpError>>) -> Arc<Peer> {
    Arc::new(Peer {
        replies: Mutex::new(replies.into()),
        seen: Mutex::new(Vec::new()),
        generation: AtomicU64::new(1),
    })
}
struct Manual {
    calls: AtomicUsize,
    change: Option<Arc<Peer>>,
    phases: Mutex<Vec<String>>,
}
#[async_trait]
impl Interaction for Manual {
    async fn verify(
        &self,
        challenge: Zeroizing<String>,
        methods: Vec<String>,
        _: &CancellationToken,
    ) -> Result<(Zeroizing<String>, String), Failure> {
        assert_eq!(&*challenge, "synthetic-private-start");
        assert_eq!(methods, vec!["captcha"]);
        self.calls.fetch_add(1, Ordering::AcqRel);
        if let Some(peer) = &self.change {
            peer.generation.store(2, Ordering::Release);
        }
        Ok((
            Zeroizing::new("synthetic-private-success".into()),
            "captcha".into(),
        ))
    }
    async fn totp(&self, _: &CancellationToken) -> Result<Zeroizing<String>, Failure> {
        Err(Failure::new("unexpected_totp"))
    }
    fn phase(&self, phase: &str) {
        self.phases.lock().unwrap().push(phase.into());
    }
}
fn manual() -> Manual {
    Manual {
        calls: AtomicUsize::new(0),
        change: None,
        phases: Mutex::new(Vec::new()),
    }
}
fn challenge() -> Value {
    json!({"Code":9001,"Error":"synthetic-private-error-body","Details":{"HumanVerificationToken":"synthetic-private-start","HumanVerificationMethods":["captcha"]}})
}
fn session() -> SessionSecrets {
    SessionSecrets {
        uid: "synthetic-uid".into(),
        access_token: "synthetic-private-access".into(),
        refresh_token: "synthetic-private-refresh".into(),
        user_id: "synthetic-user".into(),
    }
}

fn login_info() -> Value {
    let fixture: Value = serde_json::from_str(include_str!(
        "../../../../tool/proton_poc/testdata/srp_v4.json"
    ))
    .unwrap();
    json!({"Code":1000,"Version":4,"Salt":fixture["salt"],"Modulus":fixture["signed_modulus"],"ServerEphemeral":fixture["server_ephemeral"],"SRPSession":"synthetic-private-srp-session"})
}

fn login_with_invalid_proof() -> Value {
    json!({"Code":1000,"UID":"signed-in","AccessToken":"login-access","RefreshToken":"login-refresh","UserID":"login-user","ServerProof":STANDARD.encode([0u8;256]),"2FA":{"Enabled":0}})
}

async fn try_login(
    client: &mut Client,
    io: &dyn Interaction,
    cancel: &CancellationToken,
) -> Failure {
    workflow::login(
        client,
        Zeroizing::new(br#"{"username":"synthetic-user","password":"abc123"}"#.to_vec()),
        io,
        cancel,
    )
    .await
    .err()
    .unwrap()
}

async fn provision_guest(
    client: &mut Client,
    parent: &std::path::Path,
    io: &dyn Interaction,
    cancel: &CancellationToken,
) -> Result<uuid::Uuid, Failure> {
    workflow::provision(
        client,
        &Request::parse(r#"{"action":"guest"}"#).unwrap(),
        Zeroizing::new(Vec::new()),
        workflow::Storage {
            parent,
            cipher: Arc::new(super::workflow_tests::FixtureCipher),
            accounts: Default::default(),
        },
        io,
        cancel,
    )
    .await
}

#[tokio::test]
async fn android_login_401_creates_anonymous_context_and_retries_before_checking_server_proof() {
    let peer = make_peer(vec![
        Ok((401, json!({"Code":401,"Error":"synthetic-private-error"}))),
        Ok((
            200,
            json!({"Code":1000,"UID":"anonymous","AccessToken":"anon-access","RefreshToken":"anon-refresh"}),
        )),
        Ok((200, login_info())),
        Ok((200, login_with_invalid_proof())),
        Ok((200, json!({"Code":1000}))),
        Ok((200, json!({"Code":1000}))),
    ]);
    let mut client = Client::new(peer.clone());
    client.config = ClientConfig::android(super::client_tests::ANDROID_CONTEXT).unwrap();
    let io = manual();
    let failure = try_login(&mut client, &io, &CancellationToken::new()).await;
    assert_eq!(failure.reason, "cryptographic_validation_failed");
    assert_eq!(io.calls.load(Ordering::Acquire), 0);
    assert_eq!(
        *io.phases.lock().unwrap(),
        ["srp_info", "anonymous_session", "srp_info", "srp_proof"]
    );
    assert_eq!(client.retired.len(), 1);
    {
        let seen = peer.seen.lock().unwrap();
        assert_eq!(seen.len(), 4);
        for (request, path) in seen.iter().zip([
            "/auth/v4/info",
            "/auth/v4/sessions",
            "/auth/v4/info",
            "/auth/v4",
        ]) {
            assert_eq!(request.method, Method::POST);
            assert!(request.url.ends_with(path));
            assert!(request.headers.contains(&(
                "user-agent".into(),
                "ProtonVPN/5.20.74.0 (Android 15; google Pixel 8)".into()
            )));
            assert!(
                request
                    .headers
                    .contains(&("x-pm-appversion".into(), "android-vpn@5.20.74.0+apk".into()))
            );
        }
        assert_eq!(seen[0].body, seen[2].body);
        for request in &seen[2..] {
            assert!(
                request
                    .headers
                    .contains(&("x-pm-uid".into(), "anonymous".into()))
            );
            assert!(
                request
                    .headers
                    .contains(&("authorization".into(), "Bearer anon-access".into()))
            );
        }
        let payload: Value = serde_json::from_slice(&seen[1].body).unwrap();
        assert!(payload.get("Username").is_none());
        assert!(payload.get("password").is_none());
        assert!(
            payload["Payload"]["vpn-android-v4-challenge-0"]
                .get("type")
                .is_none()
        );
    }
    assert!(client.revoke_all(&CancellationToken::new()).await);
    let seen = peer.seen.lock().unwrap();
    assert_eq!(seen.len(), 6);
    for (request, uid) in seen[4..].iter().zip(["signed-in", "anonymous"]) {
        assert_eq!(request.method, Method::DELETE);
        assert!(request.headers.contains(&("x-pm-uid".into(), uid.into())));
    }
}

fn anonymous_response() -> Value {
    json!({"Code":1000,"UID":"synthetic-uid","AccessToken":"fresh-access","RefreshToken":"fresh-refresh"})
}

#[tokio::test]
async fn login_reuses_anonymous_context_and_refreshes_it_only_after_401() {
    for rejected in [false, true] {
        let mut replies = Vec::new();
        if rejected {
            replies.extend([
                Ok((401, json!({"Code":401}))),
                Ok((200, anonymous_response())),
            ]);
        }
        replies.extend([
            Ok((200, login_info())),
            Ok((200, login_with_invalid_proof())),
        ]);
        let peer = make_peer(replies);
        let mut client = Client::new(peer.clone());
        client.session = session();
        client.session.user_id.clear();
        let io = manual();
        let failure = try_login(&mut client, &io, &CancellationToken::new()).await;
        assert_eq!(failure.reason, "cryptographic_validation_failed");
        let seen = peer.seen.lock().unwrap();
        assert_eq!(seen.len(), if rejected { 4 } else { 2 });
        assert!(seen.iter().all(|r| !r.url.ends_with("/auth/v4/sessions")));
        if rejected {
            assert!(seen[1].url.ends_with("/auth/v4/refresh"));
            let body: Value = serde_json::from_slice(&seen[1].body).unwrap();
            assert_eq!(
                body,
                json!({"UID":"synthetic-uid","RefreshToken":"synthetic-private-refresh","ResponseType":"token","GrantType":"refresh_token","RedirectURI":"http://protonmail.ch"})
            );
            assert_eq!(seen[0].body, seen[2].body);
            assert!(
                seen[2]
                    .headers
                    .contains(&("authorization".into(), "Bearer fresh-access".into()))
            );
            assert_eq!(
                *io.phases.lock().unwrap(),
                ["srp_info", "session_refresh", "srp_info", "srp_proof"]
            );
        }
    }
}

#[tokio::test]
async fn login_proof_401_replays_the_same_proof_after_recovering_anonymous_context() {
    for existing in [false, true] {
        let peer = make_peer(vec![
            Ok((200, login_info())),
            Ok((401, json!({"Code":401}))),
            Ok((200, anonymous_response())),
            Ok((200, login_with_invalid_proof())),
        ]);
        let mut client = Client::new(peer.clone());
        if existing {
            client.session = session();
            client.session.user_id.clear();
        }
        let failure = try_login(&mut client, &manual(), &CancellationToken::new()).await;
        assert_eq!(failure.reason, "cryptographic_validation_failed");
        let seen = peer.seen.lock().unwrap();
        assert_eq!(seen.len(), 4);
        assert_eq!(seen[1].url, seen[3].url);
        assert_eq!(seen[1].body, seen[3].body);
        assert!(seen[2].url.ends_with(if existing {
            "/auth/v4/refresh"
        } else {
            "/auth/v4/sessions"
        }));
        assert!(
            seen[3]
                .headers
                .contains(&("authorization".into(), "Bearer fresh-access".into()))
        );
    }
}

#[tokio::test]
async fn login_401_recovery_retries_the_original_call_only_once() {
    for existing in [false, true] {
        let peer = make_peer(vec![
            Ok((401, json!({"Code":401}))),
            Ok((200, anonymous_response())),
            Ok((401, json!({"Code":401}))),
        ]);
        let mut client = Client::new(peer.clone());
        if existing {
            client.session = session();
            client.session.user_id.clear();
        }
        let io = manual();
        let failure = try_login(&mut client, &io, &CancellationToken::new()).await;
        assert_eq!(failure.status, Some(401));
        let seen = peer.seen.lock().unwrap();
        assert_eq!(seen.len(), 3);
        assert_eq!(seen[0].url, seen[2].url);
        assert_eq!(seen[0].body, seen[2].body);
        assert_eq!(io.phases.lock().unwrap().last().unwrap(), "srp_info");
        assert_eq!(io.calls.load(Ordering::Acquire), 0);
    }
}

#[tokio::test]
async fn login_recovery_preserves_real_human_challenges_without_reusing_their_headers() {
    for challenge_on_session in [false, true] {
        let mut replies = vec![Ok((401, json!({"Code":401})))];
        if challenge_on_session {
            replies.push(Ok((422, challenge())));
        }
        replies.push(Ok((200, anonymous_response())));
        if !challenge_on_session {
            replies.push(Ok((422, challenge())));
        }
        replies.extend([
            Ok((200, login_info())),
            Ok((200, login_with_invalid_proof())),
        ]);
        let peer = make_peer(replies);
        let mut client = Client::new(peer.clone());
        let io = manual();
        let failure = try_login(&mut client, &io, &CancellationToken::new()).await;
        assert_eq!(failure.reason, "cryptographic_validation_failed");
        assert_eq!(io.calls.load(Ordering::Acquire), 1);
        let seen = peer.seen.lock().unwrap();
        assert_eq!(seen.len(), 5);
        let verified = if challenge_on_session { 2 } else { 3 };
        assert_eq!(seen[verified - 1].url, seen[verified].url);
        assert_eq!(seen[verified - 1].body, seen[verified].body);
        for (index, request) in seen.iter().enumerate() {
            assert_eq!(
                request
                    .headers
                    .iter()
                    .any(|(key, _)| key.starts_with("x-pm-human")),
                index == verified
            );
        }
    }
}

#[tokio::test]
async fn login_does_not_recover_credential_errors_or_ambiguous_login_mutations() {
    for code in [8002, 1000] {
        let peer = make_peer(vec![
            Ok((200, login_info())),
            Ok((401, json!({"Code":code,"Error":"synthetic-private-error"}))),
        ]);
        let mut client = Client::new(peer.clone());
        let io = manual();
        let failure = try_login(&mut client, &io, &CancellationToken::new()).await;
        assert_eq!(failure.code, Some(code));
        assert_eq!(client.uncertain, code == 1000);
        assert_eq!(peer.seen.lock().unwrap().len(), 2);
        assert_eq!(io.calls.load(Ordering::Acquire), 0);
        assert!(!format!("{failure:?}").contains("synthetic-private"));
    }
    for (status, code) in [(422, 8002), (422, 8004), (429, 429), (500, 401), (200, 401)] {
        let peer = make_peer(vec![Ok((status, json!({"Code":code})))]);
        let mut client = Client::new(peer.clone());
        let failure = try_login(&mut client, &manual(), &CancellationToken::new()).await;
        assert_eq!(failure.status, Some(status));
        assert_eq!(peer.seen.lock().unwrap().len(), 1);
    }
}

#[tokio::test]
async fn login_stops_on_failed_or_invalid_recovery_without_replaying_tokens() {
    for existing in [false, true] {
        for response in [
            Ok((401, json!({"Code":401}))),
            Err(InternalHttpError::Timeout),
            Ok((
                200,
                json!({"Code":1000,"UID":"synthetic-uid","AccessToken":"fresh-access"}),
            )),
            Ok((
                200,
                json!({"Code":1000,"UID":"synthetic-uid","AccessToken":"fresh-access","RefreshToken":"fresh-refresh","UserID":"unexpected-user"}),
            )),
        ] {
            let timeout = response.is_err();
            let peer = make_peer(vec![Ok((401, json!({"Code":401}))), response]);
            let mut client = Client::new(peer.clone());
            if existing {
                client.session = session();
                client.session.user_id.clear();
            }
            let io = manual();
            let failure = try_login(&mut client, &io, &CancellationToken::new()).await;
            assert_eq!(peer.seen.lock().unwrap().len(), 2);
            assert_eq!(client.uncertain, timeout);
            assert_eq!(
                io.phases.lock().unwrap().last().unwrap(),
                if existing {
                    "session_refresh"
                } else {
                    "anonymous_session"
                }
            );
            assert!(!format!("{failure:?}").contains("fresh-access"));
        }
    }
    for uncertain in [false, true] {
        let peer = make_peer(vec![]);
        let mut client = Client::new(peer.clone());
        if uncertain {
            client.uncertain = true;
        } else {
            client.session = session();
        }
        let failure = try_login(&mut client, &manual(), &CancellationToken::new()).await;
        assert_eq!(
            failure.reason,
            if uncertain {
                "session_state_unknown"
            } else {
                "unexpected_authenticated_session"
            }
        );
        assert!(peer.seen.lock().unwrap().is_empty());
    }
}

struct InterruptedLogin {
    peer: Arc<Peer>,
    after: usize,
    cancel: Option<CancellationToken>,
}

#[async_trait]
impl Http for InterruptedLogin {
    async fn exchange(
        &self,
        request: InternalRequest<'_>,
        cancel: &CancellationToken,
    ) -> Result<InternalResponse, InternalHttpError> {
        let response = self.peer.exchange(request, cancel).await?;
        if self.peer.seen.lock().unwrap().len() == self.after {
            if let Some(cancel) = &self.cancel {
                cancel.cancel();
            } else {
                self.peer.generation.fetch_add(1, Ordering::AcqRel);
            }
        }
        Ok(response)
    }
    fn generation(&self) -> Option<u64> {
        self.peer.generation()
    }
    fn current(&self, generation: Option<u64>) -> bool {
        self.peer.current(generation)
    }
}

#[tokio::test]
async fn login_recovery_stops_on_cancellation_or_changed_underlay_before_replaying() {
    for after in [1, 2] {
        for cancelled in [false, true] {
            let cancel = CancellationToken::new();
            let peer = make_peer(vec![
                Ok((401, json!({"Code":401}))),
                Ok((200, anonymous_response())),
            ]);
            let mut client = Client::new(Arc::new(InterruptedLogin {
                peer: peer.clone(),
                after,
                cancel: cancelled.then(|| cancel.clone()),
            }));
            let failure = try_login(&mut client, &manual(), &cancel).await;
            assert_eq!(
                failure.reason,
                if cancelled {
                    "cancelled"
                } else {
                    "underlay_changed"
                }
            );
            assert_eq!(peer.seen.lock().unwrap().len(), after);
            assert_eq!(client.session.uid.is_empty(), after == 1);
        }
    }
}

#[tokio::test]
async fn android_guest_uses_matching_headers_and_anonymous_session_for_credentialless() {
    let peer = make_peer(vec![
        Ok((
            200,
            json!({"Code":1000,"UID":"anonymous","AccessToken":"anon-access","RefreshToken":"anon-refresh"}),
        )),
        Ok((
            200,
            json!({"Code":1000,"UID":"guest","AccessToken":"guest-access","RefreshToken":"guest-refresh","UserID":"guest-user"}),
        )),
    ]);
    let mut client = Client::new(peer.clone());
    client.config = ClientConfig::android(super::client_tests::ANDROID_CONTEXT).unwrap();
    workflow::guest(&mut client, &manual(), &CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(client.session.user_id, "guest-user");
    let seen = peer.seen.lock().unwrap();
    assert_eq!(seen.len(), 2);
    for request in seen.iter() {
        let header = |name| {
            request
                .headers
                .iter()
                .find(|(key, _)| key == name)
                .map(|(_, value)| value.as_str())
        };
        assert_eq!(header("x-pm-appversion"), Some("android-vpn@5.20.74.0+apk"));
        assert_eq!(
            header("user-agent"),
            Some("ProtonVPN/5.20.74.0 (Android 15; google Pixel 8)")
        );
        assert_eq!(header("x-pm-locale"), Some("ja"));
        assert_eq!(header("content-type"), Some("application/json"));
        assert_eq!(header("accept"), Some("application/vnd.protonmail.v1+json"));
    }
    assert!(seen[0].url.ends_with("/auth/v4/sessions"));
    assert!(
        !seen[0]
            .headers
            .iter()
            .any(|(key, _)| key == "authorization" || key == "x-pm-uid")
    );
    assert!(seen[1].url.ends_with("/auth/v4/credentialless"));
    assert!(
        seen[1]
            .headers
            .contains(&("x-pm-uid".into(), "anonymous".into()))
    );
    assert!(
        seen[1]
            .headers
            .contains(&("authorization".into(), "Bearer anon-access".into()))
    );
    for (index, request) in seen.iter().enumerate() {
        let value: Value = serde_json::from_slice(&request.body).unwrap();
        let device = &value["Payload"]["vpn-android-v4-challenge-0"];
        assert_eq!(device["deviceName"], 73362205250i64);
        assert_eq!(device["timezoneOffset"], -540);
        assert_eq!(device["type"].is_string(), index == 1);
    }
}

#[tokio::test]
async fn credentialless_reuses_an_existing_anonymous_session() {
    let peer = make_peer(vec![Ok((
        200,
        json!({"Code":1000,"UID":"guest","AccessToken":"guest-access","RefreshToken":"guest-refresh","UserID":"guest-user"}),
    ))]);
    let mut client = Client::new(peer.clone());
    client.session = session();
    client.session.user_id.clear();
    workflow::guest(&mut client, &manual(), &CancellationToken::new())
        .await
        .unwrap();
    let seen = peer.seen.lock().unwrap();
    assert_eq!(seen.len(), 1);
    assert!(seen[0].url.ends_with("/auth/v4/credentialless"));
}

#[tokio::test]
async fn guest_permissions_use_scopes_and_vpn_permissions_without_a_core_user_request() {
    let peer = make_peer(vec![
        Ok((200, json!({"Code":1000,"Scopes":["vpn"]}))),
        Ok((
            200,
            json!({"Code":1000,"VPN":{"Status":1,"MaxConnect":1,"MaxTier":0},"Delinquent":0}),
        )),
    ]);
    let mut client = Client::new(peer.clone());
    client.session = session();
    assert_eq!(
        workflow::permissions(
            &mut client,
            &manual(),
            &CancellationToken::new(),
            Some("synthetic-user"),
            true
        )
        .await
        .unwrap(),
        0
    );
    let seen = peer.seen.lock().unwrap();
    assert!(seen[0].url.ends_with("/auth/v4/scopes"));
    assert!(seen[1].url.ends_with("/vpn/v2"));
}

#[tokio::test]
async fn refreshed_session_preserves_user_id_but_rejects_account_switching() {
    for changed_user in [false, true] {
        let mut response = json!({"Code":1000,"UID":"synthetic-uid","AccessToken":"fresh-access","RefreshToken":"fresh-refresh"});
        if changed_user {
            response["UserID"] = json!("another-user");
        }
        let peer = make_peer(vec![Ok((200, response))]);
        let mut client = Client::new(peer);
        client.session = session();
        let result =
            workflow::refresh_session(&mut client, &manual(), &CancellationToken::new()).await;
        if changed_user {
            assert_eq!(result.unwrap_err().reason, "protected_user_mismatch");
        } else {
            result.unwrap();
            assert_eq!(client.session.user_id, "synthetic-user");
            assert_eq!(client.session.access_token, "fresh-access");
        }
    }
}

#[tokio::test]
async fn each_authentication_step_can_present_its_own_manual_challenge() {
    let directory = tempfile::tempdir().unwrap();
    let peer = make_peer(vec![
        Ok((422, challenge())),
        Ok((
            200,
            json!({"Code":1000,"UID":"anonymous","AccessToken":"anon-access","RefreshToken":"anon-refresh"}),
        )),
        Ok((422, challenge())),
        Ok((
            200,
            json!({"Code":1000,"UID":"guest","AccessToken":"guest-access","RefreshToken":"guest-refresh","UserID":"guest-user"}),
        )),
        Ok((403, json!({"Code":403}))),
        Ok((200, json!({"Code":1000}))),
        Ok((200, json!({"Code":1000}))),
    ]);
    let mut client = Client::new(peer.clone());
    let io = manual();
    let failure = provision_guest(
        &mut client,
        directory.path(),
        &io,
        &CancellationToken::new(),
    )
    .await
    .unwrap_err();
    assert_eq!(failure.reason, "api_rejected");
    assert_eq!(io.calls.load(Ordering::Acquire), 2);
    assert!(client.revoke_all(&CancellationToken::new()).await);
    let seen = peer.seen.lock().unwrap();
    assert_eq!(seen.len(), 7);
    assert_eq!(seen[0].body, seen[1].body);
    assert_eq!(seen[2].body, seen[3].body);
    assert_eq!(seen[5].method, Method::DELETE);
    assert_eq!(seen[6].method, Method::DELETE);
    assert!(
        usque_core::proton_vpn::store::SessionStore::new(
            directory.path(),
            &super::workflow_tests::FixtureCipher
        )
        .records()
        .unwrap()
        .is_empty()
    );
}

#[tokio::test]
async fn guest_rejections_do_not_retry_switch_identity_or_start_verification() {
    for credentialless in [false, true] {
        for (code, reason) in [
            (8004, "session_not_allowed"),
            (10200, "credentialless_disabled"),
        ] {
            let directory = tempfile::tempdir().unwrap();
            let mut replies = Vec::new();
            if credentialless {
                replies.push(Ok((200, anonymous_response())));
            }
            replies.push(Ok((422, json!({"Code":code}))));
            if credentialless {
                replies.push(Ok((200, json!({"Code":1000}))));
            }
            let peer = make_peer(replies);
            let mut client = Client::new(peer.clone());
            let io = manual();
            let failure = provision_guest(
                &mut client,
                directory.path(),
                &io,
                &CancellationToken::new(),
            )
            .await
            .unwrap_err();
            assert_eq!(failure.reason, reason);
            assert_eq!(failure.code, Some(code));
            assert_eq!(failure.status, Some(422));
            assert_eq!(io.calls.load(Ordering::Acquire), 0);
            assert!(client.revoke_all(&CancellationToken::new()).await);
            let seen = peer.seen.lock().unwrap();
            assert_eq!(seen.len(), if credentialless { 3 } else { 1 });
            let expected = ClientConfig::default().for_guest().unwrap();
            for request in seen.iter() {
                assert!(request.headers.contains(&(
                    "x-pm-appversion".into(),
                    expected.identity.app_version.clone()
                )));
                assert!(
                    request
                        .headers
                        .contains(&("user-agent".into(), expected.identity.user_agent.clone()))
                );
            }
            if credentialless {
                assert_eq!(seen[2].method, Method::DELETE);
            }
            assert!(
                usque_core::proton_vpn::store::SessionStore::new(
                    directory.path(),
                    &super::workflow_tests::FixtureCipher
                )
                .records()
                .unwrap()
                .is_empty()
            );
        }
    }
}

#[tokio::test]
async fn verification_retries_only_original_request_and_does_not_leak_headers() {
    for path in ["/auth/v4/sessions", "/auth/v4/credentialless"] {
        let peer = make_peer(vec![
            Ok((422, challenge())),
            Ok((200, json!({"Code":1000}))),
            Ok((200, json!({"Code":1000}))),
        ]);
        let mut client = Client::new(peer.clone());
        if path.ends_with("credentialless") {
            client.session = session();
        }
        let io = manual();
        let cancel = CancellationToken::new();
        let payload = json!({"Payload":{"synthetic":"same-body"}});
        client
            .request(Method::POST, path, &payload, &io, &cancel)
            .await
            .unwrap();
        client
            .request(Method::GET, "/vpn/v2", &Value::Null, &io, &cancel)
            .await
            .unwrap();
        let seen = peer.seen.lock().unwrap();
        assert_eq!(seen.len(), 3);
        assert_eq!(seen[0].url, seen[1].url);
        assert_eq!(seen[0].method, seen[1].method);
        assert_eq!(seen[0].body, seen[1].body);
        assert!(
            !seen[0]
                .headers
                .iter()
                .any(|(key, _)| key.starts_with("x-pm-human"))
        );
        assert!(
            seen[1]
                .headers
                .iter()
                .any(|(key, value)| key == "x-pm-human-verification-token"
                    && value == "synthetic-private-success")
        );
        assert!(
            !seen[2]
                .headers
                .iter()
                .any(|(key, _)| key.starts_with("x-pm-human"))
        );
        assert_eq!(io.calls.load(Ordering::Acquire), 1);
    }
}
#[tokio::test]
async fn verification_is_one_attempt_and_bound_to_underlay_generation() {
    let peer = make_peer(vec![Ok((422, challenge())), Ok((422, challenge()))]);
    let mut client = Client::new(peer.clone());
    let io = manual();
    let failure = client
        .request(
            Method::POST,
            "/auth/v4/sessions",
            &Value::Null,
            &io,
            &CancellationToken::new(),
        )
        .await
        .err()
        .unwrap();
    assert_eq!(failure.reason, "verification_required");
    assert_eq!(io.calls.load(Ordering::Acquire), 1);
    assert_eq!(peer.seen.lock().unwrap().len(), 2);
    let peer = make_peer(vec![Ok((422, challenge()))]);
    let mut client = Client::new(peer.clone());
    let io = Manual {
        calls: AtomicUsize::new(0),
        change: Some(peer.clone()),
        phases: Mutex::new(Vec::new()),
    };
    let failure = client
        .request(
            Method::POST,
            "/auth/v4/sessions",
            &Value::Null,
            &io,
            &CancellationToken::new(),
        )
        .await
        .err()
        .unwrap();
    assert_eq!(failure.reason, "verification_underlay_changed");
    assert_eq!(peer.seen.lock().unwrap().len(), 1);
}
#[tokio::test]
async fn device_verification_and_unknown_rejections_are_not_captcha() {
    for (code, reason) in [
        (8004, "session_not_allowed"),
        (10200, "credentialless_disabled"),
        (9002, "device_verification_required"),
    ] {
        let peer = make_peer(vec![Ok((
            422,
            json!({"Code":code,"Error":"synthetic-private-raw-error"}),
        ))]);
        let mut client = Client::new(peer);
        let io = manual();
        let failure = client
            .request(
                Method::POST,
                "/auth/v4/sessions",
                &Value::Null,
                &io,
                &CancellationToken::new(),
            )
            .await
            .err()
            .unwrap();
        assert_eq!(failure.reason, reason);
        assert_eq!(failure.code, Some(code));
        assert_eq!(failure.status, Some(422));
        assert!(!format!("{failure:?}").contains("synthetic-private"));
        assert!(!client.uncertain);
        assert_eq!(io.calls.load(Ordering::Acquire), 0);
    }
}
#[tokio::test]
async fn ambiguous_mutation_and_partial_session_preserve_cleanup_boundaries() {
    let peer = make_peer(vec![Err(InternalHttpError::Timeout)]);
    let mut client = Client::new(peer);
    assert!(
        client
            .request(
                Method::POST,
                "/auth/v4/refresh",
                &Value::Null,
                &manual(),
                &CancellationToken::new()
            )
            .await
            .is_err()
    );
    assert!(client.uncertain);
    let peer = make_peer(vec![Ok((200, json!({"Code":1000})))]);
    let mut client = Client::new(peer.clone());
    client.uncertain = true;
    assert!(
        client
            .accept_session(
                &json!({"UID":"synthetic-uid","AccessToken":"synthetic-private-access"})
            )
            .is_err()
    );
    assert!(!client.uncertain);
    assert!(client.session.refresh_token.is_empty());
    assert!(client.revoke_all(&CancellationToken::new()).await);
    assert!(client.session.uid.is_empty());
    assert_eq!(peer.seen.lock().unwrap()[0].method, Method::DELETE);
}
fn request(action: &str, job_id: uuid::Uuid, challenge_id: Option<uuid::Uuid>) -> Request {
    Request {
        network_policy_version: 0,
        action: action.into(),
        job_id: Some(job_id),
        profile_id: None,
        challenge_id,
        node_id: None,
        name: String::new(),
        offset: 0,
    }
}
#[tokio::test]
async fn actual_waiting_interaction_has_private_one_use_handoff_and_cancellable_result() {
    let io = Arc::new(JobIo::new());
    let cancel = CancellationToken::new();
    let worker_io = io.clone();
    let worker_cancel = cancel.clone();
    let worker = tokio::spawn(async move {
        worker_io
            .verify(
                Zeroizing::new("synthetic-private-start".into()),
                vec!["captcha".into()],
                &worker_cancel,
            )
            .await
    });
    for _ in 0..100 {
        if io.snapshot().unwrap().state == "waiting_verification" {
            break;
        }
        tokio::task::yield_now().await;
    }
    let job = io.snapshot().unwrap();
    assert_eq!(job.state, "waiting_verification");
    let metadata = serde_json::to_string(&job).unwrap();
    assert!(!metadata.contains("synthetic-private"));
    let handoff = io
        .command(
            &request("handoff", job.id, job.challenge_id),
            Zeroizing::new(Vec::new()),
        )
        .unwrap();
    let fields: Value = serde_json::from_slice(&handoff).unwrap();
    assert!(
        fields["url"]
            .as_str()
            .unwrap()
            .contains("synthetic-private-start")
    );
    assert!(
        io.command(
            &request("handoff", job.id, job.challenge_id),
            Zeroizing::new(Vec::new())
        )
        .is_err()
    );
    let message=serde_json::to_vec(&json!({"type":"HUMAN_VERIFICATION_SUCCESS","payload":{"token":"synthetic-private-success","type":"captcha"}})).unwrap();
    assert!(
        io.command(
            &request("verify", job.id, Some(uuid::Uuid::new_v4())),
            Zeroizing::new(message.clone())
        )
        .is_err()
    );
    io.command(
        &request("verify", job.id, job.challenge_id),
        Zeroizing::new(message),
    )
    .unwrap();
    let (token, method) = worker.await.unwrap().unwrap();
    assert_eq!(&*token, "synthetic-private-success");
    assert_eq!(method, "captcha");
    let worker_io = io.clone();
    let worker_cancel = cancel.clone();
    let worker = tokio::spawn(async move { worker_io.totp(&worker_cancel).await });
    tokio::task::yield_now().await;
    cancel.cancel();
    assert_eq!(worker.await.unwrap().err().unwrap().reason, "cancelled");
}

proptest::proptest! {
    #[test]
    fn malformed_json_never_becomes_a_session_or_private_failure(bytes in proptest::collection::vec(proptest::prelude::any::<u8>(),0..2048)){
        if let Ok(value)=serde_json::from_slice::<Value>(&bytes)
            && let Err(failure)=api::token(value.get("AccessToken")){
            proptest::prop_assert_eq!(failure.reason,"invalid_api_response");
        }
    }
}

#[tokio::test]
async fn invalid_manual_verification_is_cleared_once_before_a_fresh_challenge() {
    let peer = make_peer(vec![
        Ok((422, challenge())),
        Ok((422, json!({"Code":12087}))),
        Ok((422, challenge())),
        Ok((200, json!({"Code":1000}))),
    ]);
    let mut client = Client::new(peer.clone());
    client.session = session();
    let io = manual();
    client
        .request(
            Method::POST,
            "/auth/v4/credentialless",
            &json!({"Payload":"unchanged"}),
            &io,
            &CancellationToken::new(),
        )
        .await
        .unwrap();
    assert_eq!(io.calls.load(Ordering::Acquire), 2);
    let seen = peer.seen.lock().unwrap();
    assert_eq!(seen.len(), 4);
    for (index, request) in seen.iter().enumerate() {
        assert_eq!(
            request.url,
            format!(
                "{}/auth/v4/credentialless",
                usque_core::proton_vpn::API_ORIGIN
            )
        );
        assert_eq!(request.body, seen[0].body);
        assert!(
            request
                .headers
                .contains(&("x-pm-uid".into(), "synthetic-uid".into()))
        );
        let hv = request
            .headers
            .iter()
            .find(|(key, _)| key == "x-pm-human-verification-token");
        if index % 2 == 0 {
            assert!(hv.is_none());
        } else {
            assert_eq!(hv.unwrap().1, "synthetic-private-success");
        }
    }
    assert_eq!(client.session.uid, "synthetic-uid");
}
#[tokio::test]
async fn invalid_verification_recovery_is_bounded_and_ambiguous_mutations_are_not_retried() {
    for status in [422, 500] {
        let mut replies = vec![Ok((422, challenge())), Ok((status, json!({"Code":12087})))];
        if status == 422 {
            replies.extend([Ok((422, challenge())), Ok((422, json!({"Code":12087})))]);
        }
        let peer = make_peer(replies);
        let mut client = Client::new(peer.clone());
        let io = manual();
        let failure = client
            .request(
                Method::POST,
                "/auth/v4/credentialless",
                &Value::Null,
                &io,
                &CancellationToken::new(),
            )
            .await
            .err()
            .unwrap();
        assert_eq!(failure.reason, "human_verification_invalid");
        assert_eq!(failure.code, Some(12087));
        assert_eq!(
            peer.seen.lock().unwrap().len(),
            if status == 422 { 4 } else { 2 }
        );
        assert_eq!(
            io.calls.load(Ordering::Acquire),
            if status == 422 { 2 } else { 1 }
        );
    }
}
