//! Synthetic HTTPS peers and actual interaction futures; no sockets or account.
use super::{
    api::{Client, Http},
    interaction::JobIo,
    *,
};
use crate::internal_network::{InternalHttpError, InternalRequest, InternalResponse};
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
use usque_core::proton_vpn::{Node, Request, SessionSecrets};
use zeroize::Zeroizing;

struct Seen {
    method: Method,
    url: String,
    body: Vec<u8>,
    headers: Vec<(String, String)>,
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
    async fn node(&self, _: Vec<Node>, _: &CancellationToken) -> Result<String, Failure> {
        Err(Failure::new("unexpected_node"))
    }
    fn phase(&self, _: &str) {}
}
fn manual() -> Manual {
    Manual {
        calls: AtomicUsize::new(0),
        change: None,
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
    ]);
    let mut client = Client::new(peer);
    let io = manual();
    workflow::guest(&mut client, &io, &CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(io.calls.load(Ordering::Acquire), 2);
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
        action: action.into(),
        job_id: Some(job_id),
        profile_id: None,
        challenge_id,
        node_id: None,
        name: String::new(),
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
