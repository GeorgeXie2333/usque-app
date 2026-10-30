use super::*;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};
use usque_core::{FrontendSettings, IdentityProvider, storage::ConfigStore};
use usque_ipc::v1::{self, control_request};
use usque_macos_runtime::{
    backend::{SessionBackend, SessionHealth},
    controller::Controller,
    protocol::ProviderPath,
};

#[derive(Default)]
struct Evidence {
    starts: AtomicUsize,
    stops: AtomicUsize,
    live: AtomicUsize,
    protective: AtomicBool,
    ready: AtomicBool,
    flows: Mutex<Vec<usque_transport::SystemFlowIntent>>,
}
struct IsolatedBackend(Arc<Evidence>);
#[async_trait]
impl SessionBackend for IsolatedBackend {
    async fn protect(&mut self, _: &SessionKey) -> Result<(), String> {
        self.0.protective.store(true, Ordering::SeqCst);
        Ok(())
    }
    async fn configure_dns_routes(
        &mut self,
        _: &SessionKey,
        _: &SessionConfiguration,
        _: &SessionCredentials,
    ) -> Result<(), String> {
        assert!(self.0.protective.load(Ordering::SeqCst));
        Ok(())
    }
    async fn connect(
        &mut self,
        _: &SessionKey,
        _: SessionConfiguration,
        credentials: SessionCredentials,
        _: CancellationToken,
    ) -> Result<(), String> {
        assert!(self.0.protective.load(Ordering::SeqCst));
        assert_eq!(
            self.0.live.fetch_add(1, Ordering::SeqCst),
            0,
            "one provider session only"
        );
        credentials.identity()?;
        self.0.starts.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    fn health(&self) -> SessionHealth {
        if self.0.ready.load(Ordering::SeqCst) {
            SessionHealth::Connected(ProviderPath {
                transport: usque_core::Transport::Http3,
                endpoint_family: usque_core::AddressFamily::Ipv4,
                ipv4_available: true,
                ipv6_available: true,
            })
        } else {
            SessionHealth::Starting
        }
    }
    async fn allow(&mut self, _: &SessionKey) -> Result<(), String> {
        assert!(self.0.protective.load(Ordering::SeqCst));
        Ok(())
    }
    async fn block(&mut self, _: &SessionKey) -> Result<(), String> {
        assert!(self.0.protective.load(Ordering::SeqCst));
        Ok(())
    }
    async fn disconnect(&mut self) -> Result<(), String> {
        if self.0.live.swap(0, Ordering::SeqCst) == 1 {
            self.0.stops.fetch_add(1, Ordering::SeqCst);
        }
        Ok(())
    }
    async fn clear_dns_routes(&mut self, _: &SessionKey) -> Result<(), String> {
        assert!(self.0.protective.load(Ordering::SeqCst));
        Ok(())
    }
    async fn release_protection(&mut self, _: &SessionKey) -> Result<(), String> {
        assert_eq!(self.0.live.load(Ordering::SeqCst), 0);
        self.0.protective.store(false, Ordering::SeqCst);
        Ok(())
    }
}
struct IsolatedControl {
    controller: Controller,
    evidence: Arc<Evidence>,
}
#[async_trait]
impl ProviderControl for IsolatedControl {
    async fn exchange(
        &self,
        request: Envelope,
        _: Arc<dyn CredentialBroker>,
    ) -> Result<Reply, String> {
        self.controller.submit(request).map_err(|e| e.to_string())
    }
    async fn network_generation(&self) -> Result<u64, String> {
        Ok(7)
    }
    async fn authorize_flow(
        &self,
        key: &SessionKey,
        intent: usque_transport::SystemFlowIntent,
    ) -> Result<(), String> {
        let snapshot = self.controller.snapshot();
        if snapshot.session.as_ref() != Some(key)
            || !snapshot.protection_active
            || snapshot.phase != Phase::Connected
            || !intent.remote.ip().is_loopback()
        {
            return Err("isolated_flow_rejected".into());
        }
        self.evidence.flows.lock().await.push(intent);
        Ok(())
    }
    fn isolated_test(&self) -> bool {
        true
    }
}
fn free_port() -> SocketAddr {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap()
}
async fn service() -> (
    tempfile::TempDir,
    ControlService,
    Profile,
    Arc<Evidence>,
    Controller,
) {
    let directory = tempfile::tempdir().unwrap();
    let vault = Arc::new(crate::tests::MemoryVault::default());
    let service = ControlService::open_with_vault(
        ConfigStore::new(directory.path().join("config.json")),
        vault,
    )
    .unwrap();
    let mut profile = service.config_snapshot().await.active_profile().unwrap();
    profile.frontends = FrontendSettings {
        tunnel: true,
        http: true,
        socks5: true,
    };
    profile.proxy.system_proxy = false;
    profile.proxy.http_listeners = vec![free_port()];
    profile.proxy.socks5_listeners = vec![free_port()];
    profile.canonicalize_mode();
    service.upsert_profile(profile.clone()).await.unwrap();
    let identity = crate::tests::test_identity(IdentityProvider::Consumer, None);
    service
        .persist_identity(profile.id, &identity, None)
        .await
        .unwrap();
    let evidence = Arc::new(Evidence::default());
    let controller = Controller::spawn(Box::new(IsolatedBackend(evidence.clone())), 16);
    service
        .set_macos_provider_control(Arc::new(IsolatedControl {
            controller: controller.clone(),
            evidence: evidence.clone(),
        }))
        .await
        .unwrap();
    (directory, service, profile, evidence, controller)
}
async fn connect(service: &ControlService, profile: &Profile) {
    service
        .handle_payload(control_request::Payload::Connect(v1::ConnectRequest {
            profile_id: profile.id.to_string(),
            continuous: false,
        }))
        .await
        .unwrap();
}
async fn connected(service: &ControlService) {
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        loop {
            if service.status_snapshot().await.phase == usque_core::ConnectionPhase::Connected {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
    })
    .await
    .expect("provider connected through real controller");
}
#[tokio::test]
async fn control_service_uses_one_provider_owner_for_http_and_socks() {
    let (_dir, service, profile, evidence, controller) = service().await;
    connect(&service, &profile).await;
    assert_ne!(
        service.status_snapshot().await.phase,
        usque_core::ConnectionPhase::Connected,
        "Accepted is not Connected"
    );
    assert!(matches!(
        service.data_plane.lock().await.as_ref().unwrap().runtime,
        crate::ActiveRuntime::MacVpn(_)
    ));
    evidence.ready.store(true, Ordering::SeqCst);
    connected(&service).await;
    let destination = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let remote = destination.local_addr().unwrap();
    let peer = tokio::spawn(async move {
        for _ in 0..2 {
            let (mut socket, _) = destination.accept().await.unwrap();
            let mut buffer = [0; 4];
            socket.read_exact(&mut buffer).await.unwrap();
            assert_eq!(&buffer, b"ping");
            socket.write_all(b"pong").await.unwrap();
        }
    });
    let mut http = TcpStream::connect(profile.proxy.http_listeners[0])
        .await
        .unwrap();
    http.write_all(
        format!(
            "CONNECT localhost:{} HTTP/1.1\r\nHost: localhost:{}\r\n\r\n",
            remote.port(),
            remote.port()
        )
        .as_bytes(),
    )
    .await
    .unwrap();
    let mut response = Vec::new();
    loop {
        response.push(http.read_u8().await.unwrap());
        if response.ends_with(b"\r\n\r\n") {
            break;
        }
        assert!(response.len() < 4096);
    }
    assert!(response.starts_with(b"HTTP/1.1 200"));
    http.write_all(b"ping").await.unwrap();
    let mut answer = [0; 4];
    http.read_exact(&mut answer).await.unwrap();
    assert_eq!(&answer, b"pong");
    let mut socks = TcpStream::connect(profile.proxy.socks5_listeners[0])
        .await
        .unwrap();
    socks.write_all(&[5, 1, 0]).await.unwrap();
    let mut greeting = [0; 2];
    socks.read_exact(&mut greeting).await.unwrap();
    assert_eq!(greeting, [5, 0]);
    let mut request = vec![5, 1, 0, 3, 9];
    request.extend_from_slice(b"localhost");
    request.extend_from_slice(&remote.port().to_be_bytes());
    socks.write_all(&request).await.unwrap();
    let mut reply = [0; 10];
    socks.read_exact(&mut reply).await.unwrap();
    assert_eq!(reply[1], 0);
    socks.write_all(b"ping").await.unwrap();
    socks.read_exact(&mut answer).await.unwrap();
    assert_eq!(&answer, b"pong");
    peer.await.unwrap();
    let intents = evidence.flows.lock().await;
    // localhost may first try the other family. Every attempted SYN is
    // authorized; exactly two of these tuples reach this IPv4 origin.
    let delivered = intents
        .iter()
        .filter(|intent| intent.remote == remote)
        .collect::<Vec<_>>();
    assert_eq!(delivered.len(), 2);
    assert!(
        intents
            .iter()
            .all(|i| i.hostname.as_deref() == Some("localhost"))
    );
    assert_ne!(delivered[0].local.port(), delivered[1].local.port());
    drop(intents);
    assert_eq!(evidence.starts.load(Ordering::SeqCst), 1);
    assert_eq!(evidence.live.load(Ordering::SeqCst), 1);
    service.disconnect().await.unwrap();
    service.await_disconnect_cleanup().await.unwrap();
    assert_eq!(controller.snapshot().phase, Phase::Stopped);
    assert!(!evidence.protective.load(Ordering::SeqCst));
    assert_eq!(evidence.stops.load(Ordering::SeqCst), 1);
}
#[tokio::test]
async fn process_detach_keeps_protection_and_explicit_stop_releases_it() {
    let (_dir, service, profile, evidence, controller) = service().await;
    evidence.ready.store(true, Ordering::SeqCst);
    connect(&service, &profile).await;
    connected(&service).await;
    service.detach_macos_process().await.unwrap();
    assert_eq!(controller.snapshot().phase, Phase::Connected);
    assert!(evidence.protective.load(Ordering::SeqCst));
    assert_eq!(evidence.stops.load(Ordering::SeqCst), 0);
    service.disconnect().await.unwrap();
    service.await_disconnect_cleanup().await.unwrap();
    assert_eq!(controller.snapshot().phase, Phase::Stopped);
}
#[tokio::test]
async fn replace_retains_protection_and_never_overlaps_provider_sessions() {
    let (_dir, service, profile, evidence, controller) = service().await;
    evidence.ready.store(true, Ordering::SeqCst);
    connect(&service, &profile).await;
    connected(&service).await;
    let previous = controller.snapshot().session.unwrap();
    connect(&service, &profile).await;
    connected(&service).await;
    let next = controller.snapshot().session.unwrap();
    assert_eq!(previous.session_id, next.session_id);
    assert_eq!(next.config_revision, previous.config_revision + 1);
    assert_eq!(evidence.starts.load(Ordering::SeqCst), 2);
    assert_eq!(evidence.live.load(Ordering::SeqCst), 1);
    assert!(evidence.protective.load(Ordering::SeqCst));
    service.disconnect().await.unwrap();
    service.await_disconnect_cleanup().await.unwrap();
}

struct FaultVault {
    inner: crate::tests::MemoryVault,
    fail_once: AtomicBool,
    writes: AtomicUsize,
}
#[async_trait]
impl SecretVault for FaultVault {
    async fn put(
        &self,
        profile: Uuid,
        record: SecretRecord,
        value: &[u8],
    ) -> Result<(), usque_platform::VaultError> {
        self.writes.fetch_add(1, Ordering::SeqCst);
        if record == SecretRecord::AssignedIpv4 && self.fail_once.swap(false, Ordering::SeqCst) {
            return Err(usque_platform::VaultError::Unavailable(
                "isolated_failure".into(),
            ));
        }
        self.inner.put(profile, record, value).await
    }
    async fn get(
        &self,
        profile: Uuid,
        record: SecretRecord,
    ) -> Result<Option<Zeroizing<Vec<u8>>>, usque_platform::VaultError> {
        self.inner.get(profile, record).await
    }
    async fn delete(
        &self,
        profile: Uuid,
        record: SecretRecord,
    ) -> Result<(), usque_platform::VaultError> {
        self.inner.delete(profile, record).await
    }
}
#[tokio::test]
async fn pin_update_ack_follows_vault_commit_and_failed_write_rolls_back() {
    let directory = tempfile::tempdir().unwrap();
    let vault = Arc::new(FaultVault {
        inner: crate::tests::MemoryVault::default(),
        fail_once: AtomicBool::new(false),
        writes: AtomicUsize::new(0),
    });
    let service = ControlService::open_with_vault(
        ConfigStore::new(directory.path().join("config.json")),
        vault.clone(),
    )
    .unwrap();
    let profile_id = service.config_snapshot().await.active_profile_id.unwrap();
    let previous = crate::tests::test_identity(IdentityProvider::Consumer, None);
    let mut next =
        usque_core::parse_manual_warp_secret(&previous.to_portable_secret_json().unwrap()).unwrap();
    next.endpoint_pin = usque_core::EndpointPin::from_spki_der(
        &usque_core::MasqueKeyPair::generate()
            .public_spki_der()
            .unwrap(),
    )
    .unwrap();
    next.assigned_ipv4 = "172.16.0.3".parse().unwrap();
    service
        .persist_identity(profile_id, &previous, None)
        .await
        .unwrap();
    let session = SessionKey {
        session_id: Uuid::new_v4(),
        config_revision: 1,
        network_generation: 7,
    };
    let broker = EngineCredentialBroker {
        current: Arc::new(StdMutex::new(Some(session.clone()))),
        profile_id,
        vault: vault.clone(),
        pin_lock: Arc::new(Mutex::new(())),
        proton: None,
    };
    vault.fail_once.store(true, Ordering::SeqCst);
    assert!(
        broker
            .persist_pin(&session, profile_id, &previous, &next)
            .await
            .is_err()
    );
    assert_eq!(
        vault
            .get(profile_id, SecretRecord::EndpointPin)
            .await
            .unwrap()
            .unwrap()
            .as_slice(),
        previous.endpoint_pin.spki_der()
    );
    assert_eq!(
        vault
            .get(profile_id, SecretRecord::AssignedIpv4)
            .await
            .unwrap()
            .unwrap()
            .as_slice(),
        previous.assigned_ipv4.to_string().as_bytes()
    );
    let ack = broker
        .persist_pin(&session, profile_id, &previous, &next)
        .await
        .unwrap();
    assert_eq!(ack.session, session);
    assert_eq!(ack.profile_id, profile_id);
    assert!(!ack.revision.is_nil());
    assert_eq!(
        vault
            .get(profile_id, SecretRecord::EndpointPin)
            .await
            .unwrap()
            .unwrap()
            .as_slice(),
        next.endpoint_pin.spki_der()
    );
    let writes = vault.writes.load(Ordering::SeqCst);
    let stale = SessionKey {
        config_revision: 2,
        ..session
    };
    assert!(
        broker
            .persist_pin(&stale, profile_id, &next, &previous)
            .await
            .is_err()
    );
    assert_eq!(vault.writes.load(Ordering::SeqCst), writes);
}

struct StaleReply;
#[async_trait]
impl ProviderControl for StaleReply {
    async fn exchange(
        &self,
        request: Envelope,
        _: Arc<dyn CredentialBroker>,
    ) -> Result<Reply, String> {
        Ok(Reply {
            operation_id: Uuid::new_v4(),
            acceptance: usque_macos_runtime::protocol::Acceptance::Accepted,
            snapshot: Snapshot {
                session: Some(request.session),
                ..Default::default()
            },
        })
    }
}
#[tokio::test]
async fn stale_provider_reply_cannot_ack_a_new_operation() {
    let key = SessionKey {
        session_id: Uuid::new_v4(),
        config_revision: 1,
        network_generation: 7,
    };
    assert!(
        exchange(
            &StaleReply,
            Arc::new(usque_macos_runtime::credentials::UnavailableCredentialBroker),
            &key,
            Command::Snapshot
        )
        .await
        .is_err()
    );
}

type PreflightRequest = (
    SessionKey,
    tokio::sync::oneshot::Sender<Result<u64, String>>,
);
struct PreflightControl {
    requests: tokio::sync::mpsc::UnboundedSender<PreflightRequest>,
    stops: StdMutex<Vec<SessionKey>>,
}
async fn next_preflight(
    requests: &mut tokio::sync::mpsc::UnboundedReceiver<PreflightRequest>,
) -> PreflightRequest {
    tokio::time::timeout(std::time::Duration::from_secs(3), requests.recv())
        .await
        .expect("preflight request is bounded")
        .expect("preflight adapter remains available")
}
#[async_trait]
impl ProviderControl for PreflightControl {
    async fn prepare_protection(&self, session: &SessionKey) -> Result<u64, String> {
        let (send, receive) = tokio::sync::oneshot::channel();
        self.requests
            .send((session.clone(), send))
            .map_err(|_| "isolated_preflight_closed")?;
        receive.await.map_err(|_| "isolated_preflight_closed")?
    }
    async fn exchange(
        &self,
        request: Envelope,
        _: Arc<dyn CredentialBroker>,
    ) -> Result<Reply, String> {
        assert!(matches!(request.command, Command::Stop));
        self.stops.lock().unwrap().push(request.session.clone());
        Ok(Reply {
            operation_id: request.operation_id,
            acceptance: usque_macos_runtime::protocol::Acceptance::Accepted,
            snapshot: Snapshot {
                session: Some(request.session),
                ..Default::default()
            },
        })
    }
}

#[tokio::test]
async fn cancelled_preflight_keeps_recoverable_identity_and_retry_reuses_it() {
    let (_directory, service, _, _, _) = service().await;
    let service = Arc::new(service);
    let (requests, mut preflights) = tokio::sync::mpsc::unbounded_channel();
    let provider = Arc::new(PreflightControl {
        requests,
        stops: StdMutex::new(Vec::new()),
    });
    service
        .set_macos_provider_control(provider.clone())
        .await
        .unwrap();
    let first = tokio::spawn({
        let service = service.clone();
        async move { service.reserve_macos_start().await }
    });
    let (pending, abandoned_reply) = next_preflight(&mut preflights).await;
    assert_eq!(pending.network_generation, 0);
    assert_eq!(service.macos_reserved.lock().await.as_ref(), Some(&pending));
    first.abort();
    assert!(first.await.unwrap_err().is_cancelled());
    drop(abandoned_reply);
    assert_eq!(service.macos_reserved.lock().await.as_ref(), Some(&pending));

    let retry = tokio::spawn({
        let service = service.clone();
        async move { service.reserve_macos_start().await }
    });
    let (retried, reply) = next_preflight(&mut preflights).await;
    assert_eq!(retried, pending);
    reply.send(Ok(9)).unwrap();
    let prepared = retry.await.unwrap().unwrap();
    assert_eq!(prepared.session_id, pending.session_id);
    assert_eq!(prepared.config_revision, pending.config_revision);
    assert_eq!(prepared.network_generation, 9);
    assert_eq!(service.reserve_macos_start().await.unwrap(), prepared);
    assert!(preflights.try_recv().is_err());
    service.stop_reserved_macos().await.unwrap();
    assert_eq!(*provider.stops.lock().unwrap(), vec![prepared]);
    assert!(service.macos_reserved.lock().await.is_none());
}

#[tokio::test]
async fn rejected_preflight_can_be_explicitly_stopped_without_a_packet_runtime() {
    let (_directory, service, _, _, _) = service().await;
    let service = Arc::new(service);
    let (requests, mut preflights) = tokio::sync::mpsc::unbounded_channel();
    let provider = Arc::new(PreflightControl {
        requests,
        stops: StdMutex::new(Vec::new()),
    });
    service
        .set_macos_provider_control(provider.clone())
        .await
        .unwrap();
    let start = tokio::spawn({
        let service = service.clone();
        async move { service.reserve_macos_start().await }
    });
    let (pending, reply) = next_preflight(&mut preflights).await;
    reply.send(Err("awaiting_approval".into())).unwrap();
    assert!(matches!(
        start.await.unwrap(),
        Err(ControlServiceError::PlatformVpn(code)) if code == "awaiting_approval"
    ));
    assert_eq!(service.macos_reserved.lock().await.as_ref(), Some(&pending));
    service.stop_reserved_macos().await.unwrap();
    assert_eq!(*provider.stops.lock().unwrap(), vec![pending]);
    assert!(service.macos_reserved.lock().await.is_none());
}

struct DelayedControl(std::time::Duration);
#[async_trait]
impl ProviderControl for DelayedControl {
    async fn exchange(
        &self,
        request: Envelope,
        _: Arc<dyn CredentialBroker>,
    ) -> Result<Reply, String> {
        tokio::time::sleep(self.0).await;
        Ok(Reply {
            operation_id: request.operation_id,
            acceptance: usque_macos_runtime::protocol::Acceptance::Accepted,
            snapshot: Snapshot {
                session: Some(request.session),
                ..Default::default()
            },
        })
    }
}

#[tokio::test(start_paused = true)]
async fn stop_allows_ordered_native_teardown_and_still_has_a_deadline() {
    let key = SessionKey {
        session_id: Uuid::new_v4(),
        config_revision: 1,
        network_generation: 7,
    };
    let broker: Arc<dyn CredentialBroker> =
        Arc::new(usque_macos_runtime::credentials::UnavailableCredentialBroker);
    assert!(
        exchange(
            &DelayedControl(std::time::Duration::from_secs(61)),
            broker.clone(),
            &key,
            Command::Stop,
        )
        .await
        .is_ok()
    );
    assert!(
        exchange(
            &DelayedControl(std::time::Duration::from_secs(91)),
            broker.clone(),
            &key,
            Command::Stop,
        )
        .await
        .is_err()
    );
    assert!(
        exchange(
            &DelayedControl(std::time::Duration::from_secs(11)),
            broker,
            &key,
            Command::Snapshot,
        )
        .await
        .is_err()
    );
}
