use super::*;
use crate::{
    netstack::RuntimePath,
    tcp::{DialError, FlowClass, TcpDialer, TcpStream, TcpTarget},
};
use std::{
    collections::VecDeque,
    sync::{Mutex as StdMutex, atomic::AtomicUsize},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    sync::watch,
};
use zeroize::Zeroizing;

struct Dialer {
    calls: AtomicUsize,
    generation: AtomicU64,
}
#[async_trait]
impl TcpDialer for Dialer {
    fn session_generation(&self) -> Option<u64> {
        Some(self.generation.load(Ordering::Acquire))
    }
    async fn connect(
        &self,
        _: TcpTarget,
        _: Instant,
        _: &CancellationToken,
        _: FlowClass,
    ) -> Result<TcpStream, DialError> {
        self.calls.fetch_add(1, Ordering::AcqRel);
        Err(DialError::Refused)
    }
}
struct Provider {
    network: InternalNetwork,
    calls: AtomicUsize,
}
#[async_trait]
impl AccountWarpProvider for Provider {
    async fn context(&self, _: &CancellationToken) -> Result<Context, ImportError> {
        self.calls.fetch_add(1, Ordering::AcqRel);
        Ok(Context {
            profile: usque_core::Profile::default(),
            existing: Some(self.network.clone()),
            identity: None,
            protector: Arc::new(crate::NoopSocketProtector),
            refresher: None,
            allow_physical: false,
            lifecycle_lease: None,
        })
    }
}
struct System {
    replies: StdMutex<VecDeque<Result<u16, HttpFailure>>>,
    calls: AtomicUsize,
}
#[async_trait]
impl Http for System {
    async fn exchange(
        &self,
        request: InternalRequest<'_>,
        cancel: &CancellationToken,
    ) -> Result<InternalResponse, InternalHttpError> {
        self.exchange_delivery(request, cancel)
            .await
            .map_err(|e| e.kind)
    }
    async fn exchange_delivery(
        &self,
        request: InternalRequest<'_>,
        _: &CancellationToken,
    ) -> Result<InternalResponse, HttpFailure> {
        if request.url.ends_with("/tests/ping") {
            assert!(request.headers.is_empty() && request.body.is_empty());
            assert_eq!(request.method, Method::GET);
        }
        self.calls.fetch_add(1, Ordering::AcqRel);
        let status = self
            .replies
            .lock()
            .unwrap()
            .pop_front()
            .expect("unexpected system request")?;
        Ok(InternalResponse {
            status,
            retry_after: None,
            last_modified: None,
            body: Zeroizing::new(b"{\"Code\":1000}".to_vec()),
        })
    }
    fn generation(&self) -> Option<u64> {
        None
    }
    fn current(&self, generation: Option<u64>) -> bool {
        generation.is_none()
    }
}

#[tokio::test]
async fn verification_answers_require_a_fresh_challenge_before_migration() {
    let error = HttpFailure {
        kind: InternalHttpError::Connect,
        not_submitted: true,
        reason: None,
    };
    let (session, system, provider, _, _health) = fixture(vec![Err(error)]);
    let mut protected = request(Method::POST);
    protected
        .headers
        .push(("x-pm-human-verification-token", "old-route-answer"));
    let failure = session
        .exchange_delivery(protected, &CancellationToken::new())
        .await
        .err()
        .unwrap();
    assert_eq!(
        failure.reason.as_deref(),
        Some("verification_network_failed")
    );
    assert!(failure.not_submitted);
    assert_eq!(system.calls.load(Ordering::Acquire), 1);
    assert_eq!(provider.calls.load(Ordering::Acquire), 0);
    session.shutdown().await;
}
fn fixture(
    replies: Vec<Result<u16, HttpFailure>>,
) -> (
    AccountNetworkSession,
    Arc<System>,
    Arc<Provider>,
    Arc<Dialer>,
    watch::Sender<RuntimeHealth>,
) {
    let dialer = Arc::new(Dialer {
        calls: AtomicUsize::new(0),
        generation: AtomicU64::new(1),
    });
    let (health, receiver) = watch::channel(RuntimeHealth::Connected {
        path: RuntimePath {
            transport: usque_core::Transport::Http3,
            endpoint_family: usque_core::AddressFamily::Ipv4,
            ipv4_available: true,
            ipv6_available: false,
        },
        reconnect_count: 0,
    });
    let provider = Arc::new(Provider {
        network: InternalNetwork::for_streams(dialer.clone(), receiver, CancellationToken::new()),
        calls: AtomicUsize::new(0),
    });
    let system = Arc::new(System {
        replies: StdMutex::new(replies.into()),
        calls: AtomicUsize::new(0),
    });
    (
        AccountNetworkSession::with_system(
            provider.clone(),
            CancellationToken::new(),
            system.clone(),
        ),
        system,
        provider,
        dialer,
        health,
    )
}
fn request(method: Method) -> InternalRequest<'static> {
    InternalRequest {
        url: "https://vpn-api.proton.me/auth/v4",
        method,
        headers: vec![],
        body: Bytes::new(),
        limit: 8192,
        ipv6: None,
    }
}

#[tokio::test]
async fn business_responses_never_acquire_warp() {
    for status in [200, 400, 401, 403, 429, 500] {
        let (session, system, provider, _, _health) = fixture(vec![Ok(200), Ok(status)]);
        let cancel = CancellationToken::new();
        session.prepare(&cancel).await.unwrap();
        assert_eq!(
            session
                .exchange(request(Method::POST), &cancel)
                .await
                .unwrap()
                .status,
            status
        );
        assert_eq!(system.calls.load(Ordering::Acquire), 2);
        assert_eq!(provider.calls.load(Ordering::Acquire), 0);
        session.shutdown().await;
    }
}

#[tokio::test]
async fn ambiguous_mutations_are_not_replayed_but_safe_reads_migrate_once() {
    for kind in [InternalHttpError::Timeout, InternalHttpError::Connect] {
        for (method, not_submitted, expected) in [
            (Method::POST, false, 0),
            (Method::POST, true, 1),
            (Method::GET, false, 1),
            (Method::DELETE, false, 0),
        ] {
            let error = HttpFailure {
                kind,
                not_submitted,
                reason: None,
            };
            let (session, system, provider, _, _health) = fixture(vec![Err(error)]);
            assert!(
                session
                    .exchange(request(method), &CancellationToken::new())
                    .await
                    .is_err()
            );
            assert_eq!(provider.calls.load(Ordering::Acquire), expected);
            assert_eq!(system.calls.load(Ordering::Acquire), 1);
            if expected == 1 {
                assert!(!session.recover(&CancellationToken::new()).await.unwrap());
                assert!(
                    session
                        .exchange(request(Method::GET), &CancellationToken::new())
                        .await
                        .is_err()
                );
                assert_eq!(system.calls.load(Ordering::Acquire), 1);
            }
            session.shutdown().await;
        }
    }
}

#[tokio::test]
async fn preflight_migrates_without_credentials_and_generation_change_fails_closed() {
    let (session, system, provider, dialer, _health) =
        fixture(vec![Err(InternalHttpError::Dns.into())]);
    session.prepare(&CancellationToken::new()).await.unwrap();
    assert_eq!(session.network_status(), Some(("warp", "active")));
    assert_eq!(provider.calls.load(Ordering::Acquire), 1);
    assert_eq!(system.calls.load(Ordering::Acquire), 1);
    dialer.generation.store(2, Ordering::Release);
    assert!(!session.current(Some(0)));
    assert!(
        session
            .verification_proxy(&CancellationToken::new())
            .await
            .is_err()
    );
    session.shutdown().await;
    assert_eq!(dialer.calls.load(Ordering::Acquire), 0);
}

#[tokio::test]
async fn cancellation_and_budget_exhaustion_cannot_start_or_revert_a_route() {
    let (session, system, provider, _, _health) = fixture(vec![]);
    session.used_ms.store(120_000, Ordering::Release);
    assert!(
        session
            .exchange(request(Method::GET), &CancellationToken::new())
            .await
            .is_err()
    );
    assert_eq!(system.calls.load(Ordering::Acquire), 0);
    assert_eq!(provider.calls.load(Ordering::Acquire), 0);
    session.shutdown().await;
    let (session, _, provider, _, _health) = fixture(vec![]);
    let cancel = CancellationToken::new();
    cancel.cancel();
    assert_eq!(
        session.recover(&cancel).await.err().unwrap().reason,
        "cancelled"
    );
    assert_eq!(provider.calls.load(Ordering::Acquire), 0);
}

async fn local_request(port: u16, request: &str) -> String {
    let mut stream = tokio::net::TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, port))
        .await
        .unwrap();
    stream.write_all(request.as_bytes()).await.unwrap();
    let mut answer = String::new();
    tokio::time::timeout(Duration::from_secs(2), stream.read_to_string(&mut answer))
        .await
        .unwrap()
        .unwrap();
    answer
}
#[tokio::test]
async fn proxy_authentication_preflight_allowlist_and_generation_are_enforced() {
    use base64::Engine;
    let (_, _, provider, dialer, _health) = fixture(vec![]);
    let mut proxy = VerificationProxy::start(provider.network.clone(), 1, CancellationToken::new())
        .await
        .unwrap();
    let descriptor = proxy.descriptor();
    let host = format!("{}.usque-proton-proxy.invalid", descriptor.nonce);
    let auth = base64::engine::general_purpose::STANDARD
        .encode(format!("{}:{}", descriptor.username, descriptor.password));
    let connect = |target: &str| {
        format!(
            "CONNECT {target} HTTP/1.1\r\nHost: {target}\r\nProxy-Authorization: Basic {auth}\r\n\r\n"
        )
    };
    let get = |path: &str| {
        format!(
            "GET http://{host}/{path} HTTP/1.1\r\nHost: {host}\r\nProxy-Authorization: Basic {auth}\r\n\r\n"
        )
    };
    assert!(
        local_request(
            descriptor.port,
            "CONNECT verify.proton.me:443 HTTP/1.1\r\n\r\n"
        )
        .await
        .starts_with("HTTP/1.1 407")
    );
    assert!(
        local_request(descriptor.port, &connect("verify.proton.me:443"))
            .await
            .starts_with("HTTP/1.1 403")
    );
    assert!(
        local_request(descriptor.port, &get("ready"))
            .await
            .starts_with("HTTP/1.1 403")
    );
    assert!(
        local_request(descriptor.port, &get("probe"))
            .await
            .starts_with("HTTP/1.1 302")
    );
    assert!(
        local_request(descriptor.port, &connect(&format!("{host}:443")))
            .await
            .starts_with("HTTP/1.1 502")
    );
    use sha2::Digest;
    let ready = local_request(descriptor.port, &get("ready")).await;
    assert!(ready.contains(&format!(
            "usque-proxy-ready://{}/{}",
            descriptor.nonce,
            sha2::Sha256::digest(descriptor.password.as_bytes())
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
        )));
    assert!(!ready.contains(&descriptor.password));
    for target in [
        "example.com:443",
        "127.0.0.1:443",
        "[::1]:443",
        "verify.proton.me:80",
        "user@verify.proton.me:443",
        "verify.proton.me.evil:443",
    ] {
        assert!(
            local_request(descriptor.port, &connect(target))
                .await
                .starts_with("HTTP/1.1 403")
        );
    }
    assert_eq!(dialer.calls.load(Ordering::Acquire), 0);
    assert!(
        local_request(descriptor.port, &connect("verify-api.proton.me:443"))
            .await
            .starts_with("HTTP/1.1 502")
    );
    assert_eq!(dialer.calls.load(Ordering::Acquire), 1);
    dialer.generation.store(2, Ordering::Release);
    tokio::time::sleep(Duration::from_millis(400)).await;
    proxy.close().await;
    assert!(
        tokio::net::TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, descriptor.port))
            .await
            .is_err()
    );
}
