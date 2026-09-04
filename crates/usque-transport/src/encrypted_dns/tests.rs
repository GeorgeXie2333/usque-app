use std::future::pending;
use std::net::{Ipv4Addr, Ipv6Addr};
use std::time::{SystemTime, UNIX_EPOCH};

use boring::asn1::{Asn1Integer, Asn1Time};
use boring::bn::BigNum;
use boring::hash::MessageDigest;
use boring::pkey::{PKey, Private};
use boring::x509::extension::{
    BasicConstraints, ExtendedKeyUsage, KeyUsage, SubjectAlternativeName,
};
use boring::x509::{X509, X509NameBuilder};
use p256::pkcs8::EncodePrivateKey;
use proptest::prelude::*;
use rustls::ServerConfig;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use tokio::net::TcpListener;
use tokio::task::JoinSet;
use tokio::time::timeout;

use super::*;
use crate::network_quality::NetworkQualitySampler;

struct PrefaceWriteFailure(std::io::ErrorKind);

impl tokio::io::AsyncRead for PrefaceWriteFailure {
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
        _buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::task::Poll::Pending
    }
}

impl tokio::io::AsyncWrite for PrefaceWriteFailure {
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
        _buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        std::task::Poll::Ready(Err(self.0.into()))
    }
    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::task::Poll::Ready(Ok(()))
    }
    fn poll_shutdown(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::task::Poll::Ready(Ok(()))
    }
}

#[tokio::test]
async fn doh_preface_io_timeout_is_not_reclassified_as_retryable_query_failure() {
    let endpoint = "127.0.0.1:443".parse().unwrap();
    for (kind, expected, retry) in [
        (std::io::ErrorKind::TimedOut, DirectDnsError::Timeout, false),
        (
            std::io::ErrorKind::BrokenPipe,
            DirectDnsError::QueryFailed,
            true,
        ),
    ] {
        let failure = doh_handshake(
            PrefaceWriteFailure(kind),
            endpoint,
            Instant::now() + Duration::from_secs(1),
        )
        .await
        .err()
        .expect("preface write error");
        assert_eq!(failure.endpoint, Some(endpoint));
        assert_eq!(failure.error, expected);
        assert_eq!(failure.error.permits_retry(), retry);
    }
}

#[tokio::test(start_paused = true)]
async fn doh_preface_failure_retains_the_selected_endpoint_and_releases_io() {
    let endpoint: SocketAddr = "127.0.0.1:443".parse().unwrap();
    for peer_closes in [true, false] {
        let (stream, peer) = tokio::io::duplex(1);
        let peer = (!peer_closes).then_some(peer);
        let counts = Arc::new(LeaseCounts::default());
        counts.active.store(1, Ordering::Release);
        let stream = LeasedIo::new(
            stream,
            DirectEgressLease::hold_for_generation(TestLease(counts.clone()), 7),
        );
        let failure = doh_handshake(stream, endpoint, Instant::now() + Duration::from_millis(10))
            .await
            .err()
            .expect("preface failure");
        assert_eq!(failure.endpoint, Some(endpoint));
        assert_eq!(
            failure.error,
            if peer_closes {
                DirectDnsError::QueryFailed
            } else {
                DirectDnsError::Timeout
            }
        );
        assert_eq!(failure.error.permits_retry(), peer_closes);
        assert_eq!(counts.active.load(Ordering::Acquire), 0);
        drop(peer);
    }
}

#[tokio::test]
async fn canonical_fault_catalog_drives_encrypted_dns_failures_and_cleanup() {
    use crate::{FaultKind, FaultScript, ScheduledFault};
    for (mode, fault, expected) in [
        (
            ConfigMode::Doh,
            FaultKind::DohTlsFailure,
            DirectDnsError::TlsFailed,
        ),
        (
            ConfigMode::Doh,
            FaultKind::DohHttpFailure,
            DirectDnsError::HttpRejected,
        ),
        (
            ConfigMode::Doh,
            FaultKind::DohBodyFailure,
            DirectDnsError::InvalidResponse,
        ),
        (
            ConfigMode::Dot,
            FaultKind::DotPrefixFailure,
            DirectDnsError::InvalidResponse,
        ),
        (
            ConfigMode::Dot,
            FaultKind::DotEof,
            DirectDnsError::QueryFailed,
        ),
        (
            ConfigMode::Dot,
            FaultKind::DnsPoolCancellation,
            DirectDnsError::NetworkChanged,
        ),
    ] {
        let harness = Harness::new(mode, Behavior::Echo).await;
        harness.quality.inject_fault_script(
            FaultScript::new(
                12,
                vec![ScheduledFault {
                    at: Duration::ZERO,
                    fault,
                }],
            )
            .unwrap(),
        );
        assert_eq!(
            harness.resolver.query(test_query(12), context()).await,
            Err(expected),
            "{fault:?}"
        );
        assert!(
            harness
                .protector
                .calls
                .lock()
                .unwrap()
                .iter()
                .all(|endpoint| endpoint.port() != 53)
        );
        harness.stop().await;
    }
}

#[test]
fn encrypted_capability_rollback_rejects_saved_config_without_rewriting_it() {
    assert!(validate_direct_dns_capability(&DirectDnsSettings::default(), false).is_ok());
    for mode in [ConfigMode::Doh, ConfigMode::Dot] {
        let settings = settings(mode, "127.0.0.1:443".parse().unwrap());
        let before = settings.clone();
        assert!(validate_direct_dns_capability(&settings, false).is_err());
        assert_eq!(settings, before);
    }
}

#[tokio::test]
async fn deep_dns_probes_release_all_pool_sockets_on_success_failure_and_cancellation() {
    use crate::diagnostic_probe::{NetworkProbeResult, run_dns_probe};
    for mode in [ConfigMode::Doh, ConfigMode::Dot] {
        for behavior in [Behavior::Echo, Behavior::BadId] {
            let harness = Harness::new(mode, behavior).await;
            let result = run_dns_probe(
                harness.resolver.clone(),
                harness.protector.clone(),
                CancellationToken::new(),
                harness.cancellation.clone(),
                Instant::now(),
            )
            .await;
            assert_eq!(
                matches!(result, NetworkProbeResult::Passed { .. }),
                matches!(behavior, Behavior::Echo)
            );
            assert_eq!(harness.protector.leases.active.load(Ordering::Acquire), 0);
            assert!(
                encrypted(&harness.resolver)
                    .monitor
                    .lock()
                    .unwrap()
                    .is_none()
            );
        }
        let harness = Harness::new(mode, Behavior::Delay(Duration::from_secs(10))).await;
        let cancellation = CancellationToken::new();
        let operation = run_dns_probe(
            harness.resolver.clone(),
            harness.protector.clone(),
            cancellation.clone(),
            harness.cancellation.clone(),
            Instant::now(),
        );
        tokio::pin!(operation);
        tokio::select! {
            result = &mut operation => panic!("probe finished before cancellation: {result:?}"),
            _ = tokio::time::sleep(Duration::from_millis(50)) => cancellation.cancel(),
        }
        assert_eq!(operation.await, NetworkProbeResult::Cancelled);
        assert_eq!(harness.protector.leases.active.load(Ordering::Acquire), 0);
        assert!(
            encrypted(&harness.resolver)
                .monitor
                .lock()
                .unwrap()
                .is_none()
        );
    }
}

#[derive(Default)]
struct LeaseCounts {
    active: AtomicUsize,
    peak: AtomicUsize,
    dropped: AtomicUsize,
}

struct TestLease(Arc<LeaseCounts>);
impl Drop for TestLease {
    fn drop(&mut self) {
        self.0.active.fetch_sub(1, Ordering::AcqRel);
        self.0.dropped.fetch_add(1, Ordering::AcqRel);
    }
}

struct SpyProtector {
    generation: std::sync::atomic::AtomicU64,
    calls: StdMutex<Vec<SocketAddr>>,
    leases: Arc<LeaseCounts>,
    blocked: AtomicBool,
    block_ipv6: AtomicBool,
    reject: StdMutex<Option<IpAddr>>,
    race: AtomicBool,
}

impl SpyProtector {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            generation: std::sync::atomic::AtomicU64::new(7),
            calls: StdMutex::new(Vec::new()),
            leases: Arc::new(LeaseCounts::default()),
            blocked: AtomicBool::new(false),
            block_ipv6: AtomicBool::new(false),
            reject: StdMutex::new(None),
            race: AtomicBool::new(false),
        })
    }
}

#[async_trait]
impl SocketProtector for SpyProtector {
    fn protect(&self, _socket: SocketHandle) -> Result<(), String> {
        panic!("expected exact target and generation");
    }
    async fn protect_for_target(
        &self,
        _socket: SocketHandle,
        _endpoint: SocketAddr,
        _protocol: DirectProtocol,
    ) -> Result<DirectEgressLease, String> {
        // Data-path tests fail before any non-loopback application connect.
        Err("test application socket denial".to_owned())
    }
    async fn protect_for_target_generation(
        &self,
        _socket: SocketHandle,
        endpoint: SocketAddr,
        protocol: DirectProtocol,
        generation: u64,
    ) -> Result<DirectEgressLease, String> {
        assert_eq!(protocol, DirectProtocol::Tcp);
        assert_ne!(
            endpoint.port(),
            53,
            "encrypted failure must never use plaintext port 53"
        );
        self.calls.lock().unwrap().push(endpoint);
        if self.generation.load(Ordering::Acquire) != generation {
            return Err(STALE_GENERATION_REASON.to_owned());
        }
        if *self.reject.lock().unwrap() == Some(endpoint.ip()) {
            return Err("test denial".to_owned());
        }
        let active = self.leases.active.fetch_add(1, Ordering::AcqRel) + 1;
        self.leases.peak.fetch_max(active, Ordering::AcqRel);
        let lease =
            DirectEgressLease::hold_for_generation(TestLease(Arc::clone(&self.leases)), generation);
        if self.blocked.load(Ordering::Acquire)
            || self.block_ipv6.load(Ordering::Acquire) && endpoint.is_ipv6()
        {
            pending::<()>().await;
        }
        if self.race.load(Ordering::Acquire) {
            self.generation.fetch_add(1, Ordering::AcqRel);
        }
        Ok(lease)
    }
    fn network_generation(&self) -> Option<u64> {
        Some(self.generation.load(Ordering::Acquire))
    }
    fn endpoint_family_available(&self, _endpoint: SocketAddr) -> Option<bool> {
        Some(true)
    }
    fn tun_direct_available(&self) -> bool {
        true
    }
    fn physical_dns_servers(&self) -> Vec<SocketAddr> {
        panic!("encrypted DNS must not discover physical DNS");
    }
    fn resolve(&self, _host: &str, _port: u16) -> Result<Vec<SocketAddr>, String> {
        panic!("bootstrap must never resolve a hostname");
    }
    async fn resolve_direct(&self, _host: &str, _port: u16) -> Result<Vec<SocketAddr>, String> {
        panic!("encrypted policy cannot fall through to a platform hostname resolver");
    }
}

#[derive(Clone, Copy, Debug)]
enum Behavior {
    Echo,
    Delay(Duration),
    Non200,
    Redirect,
    WrongType,
    BadParameters,
    Oversized,
    DeclaredOversized,
    Reset,
    ZeroLength,
    Eof,
    BadId,
    BadQuestion,
    Truncated,
}

#[derive(Default)]
struct ServerCounts {
    connections: AtomicUsize,
    queries: AtomicUsize,
    active: AtomicUsize,
    peak: AtomicUsize,
}

struct ActiveQuery(Arc<ServerCounts>);
impl ActiveQuery {
    fn new(counts: Arc<ServerCounts>) -> Self {
        counts.queries.fetch_add(1, Ordering::AcqRel);
        let active = counts.active.fetch_add(1, Ordering::AcqRel) + 1;
        counts.peak.fetch_max(active, Ordering::AcqRel);
        Self(counts)
    }
}
impl Drop for ActiveQuery {
    fn drop(&mut self) {
        self.0.active.fetch_sub(1, Ordering::AcqRel);
    }
}

struct TestServer {
    endpoint: SocketAddr,
    counts: Arc<ServerCounts>,
    task: AbortOnDropHandle<()>,
}

impl TestServer {
    async fn start(mode: ConfigMode, behavior: Behavior, config: ServerConfig) -> Self {
        Self::start_at(
            mode,
            behavior,
            config,
            SocketAddr::from((Ipv4Addr::LOCALHOST, 0)),
        )
        .await
    }

    async fn start_at(
        mode: ConfigMode,
        behavior: Behavior,
        config: ServerConfig,
        address: SocketAddr,
    ) -> Self {
        let listener = TcpListener::bind(address).await.unwrap();
        let endpoint = listener.local_addr().unwrap();
        let counts = Arc::new(ServerCounts::default());
        let observed = Arc::clone(&counts);
        let config = Arc::new(config);
        let task = tokio::spawn(async move {
            let mut children = JoinSet::new();
            loop {
                tokio::select! {
                    accepted = listener.accept() => {
                        let Ok((socket, _)) = accepted else { break; };
                        socket.set_nodelay(true).unwrap();
                        observed.connections.fetch_add(1, Ordering::AcqRel);
                        let counts = Arc::clone(&observed);
                        let config = Arc::clone(&config);
                        children.spawn(async move {
                            let Ok(tls) = tokio_rustls::TlsAcceptor::from(config).accept(socket).await else { return; };
                            if mode == ConfigMode::Doh { serve_doh(tls, behavior, counts).await; }
                            else { serve_dot(tls, behavior, counts).await; }
                        });
                    }
                    result = children.join_next(), if !children.is_empty() => {
                        if let Some(Err(error)) = result { assert!(error.is_cancelled(), "fake server failed: {error}"); }
                    }
                }
            }
        });
        Self {
            endpoint,
            counts,
            task: AbortOnDropHandle::new(task),
        }
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn serve_doh<S>(tls: S, behavior: Behavior, counts: Arc<ServerCounts>)
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let Ok(mut connection) = h2::server::handshake(tls).await else {
        return;
    };
    let mut streams = JoinSet::new();
    loop {
        tokio::select! {
            request = connection.accept() => {
                let Some(Ok((request, mut response))) = request else { break; };
                let counts = Arc::clone(&counts);
                streams.spawn(async move {
                    assert_eq!(request.method(), Method::POST);
                    assert_eq!(request.version(), Version::HTTP_2);
                    assert_eq!(request.uri().scheme_str(), Some("https"));
                    assert_eq!(request.uri().path(), "/dns-query");
                    assert!(request.uri().authority().unwrap().as_str().starts_with("resolver.test:"));
                    assert_eq!(request.headers()[http::header::CONTENT_TYPE], "application/dns-message");
                    assert_eq!(request.headers()[http::header::ACCEPT], "application/dns-message");
                    let mut body = request.into_body();
                    let mut query = Vec::new();
                    while let Some(chunk) = body.data().await {
                        let Ok(chunk) = chunk else { return; };
                        query.extend_from_slice(&chunk);
                        if body.flow_control().release_capacity(chunk.len()).is_err() { return; }
                    }
                    let _active = ActiveQuery::new(counts);
                    if let Behavior::Delay(delay) = behavior { sleep(delay).await; }
                    if matches!(behavior, Behavior::Reset) { response.send_reset(h2::Reason::INTERNAL_ERROR); return; }
                    let status = match behavior { Behavior::Non200 => StatusCode::SERVICE_UNAVAILABLE, Behavior::Redirect => StatusCode::TEMPORARY_REDIRECT, _ => StatusCode::OK };
                    let content_type = match behavior {
                        Behavior::WrongType => "application/dns-message-evil",
                        Behavior::BadParameters => "application/dns-message; broken=\"",
                        _ => "Application/Dns-Message; test=\"valid parameter\"",
                    };
                    let mut headers = http::Response::builder().status(status)
                        .header(http::header::CONTENT_TYPE, content_type);
                    if matches!(behavior, Behavior::Redirect) { headers = headers.header(http::header::LOCATION, "https://unconfigured.test/dns-query"); }
                    if matches!(behavior, Behavior::DeclaredOversized) { headers = headers.header(http::header::CONTENT_LENGTH, MAX_DNS_MESSAGE + 1); }
                    let Ok(mut output) = response.send_response(headers.body(()).unwrap(), false) else { return; };
                    let answer = if matches!(behavior, Behavior::Oversized | Behavior::DeclaredOversized) { vec![0; MAX_DNS_MESSAGE + 1] }
                        else { test_response(&query, behavior) };
                    let _ = output.send_data(Bytes::from(answer), true);
                });
            }
            result = streams.join_next(), if !streams.is_empty() => {
                if let Some(Err(error)) = result { assert!(error.is_cancelled(), "fake DoH stream failed: {error}"); }
            }
        }
    }
}

async fn serve_dot<S>(mut tls: S, behavior: Behavior, counts: Arc<ServerCounts>)
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    while let Ok(length) = tls.read_u16().await {
        let mut query = vec![0; usize::from(length)];
        if tls.read_exact(&mut query).await.is_err() {
            break;
        }
        let _active = ActiveQuery::new(Arc::clone(&counts));
        if let Behavior::Delay(delay) = behavior {
            sleep(delay).await;
        }
        if matches!(behavior, Behavior::ZeroLength) {
            let _ = tls.write_u16(0).await;
            break;
        }
        if matches!(behavior, Behavior::Eof) {
            let _ = tls.write_u16(20).await;
            let _ = tls.write_all(&[0; 2]).await;
            let _ = tls.shutdown().await;
            break;
        }
        let response = test_response(&query, behavior);
        if tls.write_u16(response.len() as u16).await.is_err()
            || tls.write_all(&response).await.is_err()
            || tls.flush().await.is_err()
        {
            break;
        }
    }
}

fn test_query(id: u16) -> Bytes {
    let mut query = vec![0, 0, 1, 0, 0, 1, 0, 0, 0, 0, 0, 0];
    query[..2].copy_from_slice(&id.to_be_bytes());
    query.extend_from_slice(b"\x06direct\x07example\x04test\0\0\x01\0\x01");
    Bytes::from(query)
}

fn test_response(query: &[u8], behavior: Behavior) -> Vec<u8> {
    let mut response = query.to_vec();
    response[2..4].copy_from_slice(&0x8180_u16.to_be_bytes());
    if matches!(behavior, Behavior::BadId) {
        response[0] ^= 1;
    }
    if matches!(behavior, Behavior::BadQuestion) {
        response[13] ^= 1;
    }
    if matches!(behavior, Behavior::Truncated) {
        response[2] |= 2;
        return response;
    }
    let kind = u16::from_be_bytes([query[query.len() - 4], query[query.len() - 3]]);
    response[6..8].copy_from_slice(&1_u16.to_be_bytes());
    response.extend_from_slice(&[0xc0, 0x0c]);
    response.extend_from_slice(&kind.to_be_bytes());
    response.extend_from_slice(&[0, 1, 0, 0, 0, 60]);
    if kind == 28 {
        response.extend_from_slice(&[0, 16]);
        response.extend_from_slice(&"2001:db8::17".parse::<Ipv6Addr>().unwrap().octets());
    } else {
        response.extend_from_slice(&[0, 4, 192, 0, 2, 17]);
    }
    response
}

fn test_certificates(mode: ConfigMode, name: &str, expired: bool) -> (ServerConfig, ClientConfig) {
    fn key(byte: u8) -> (PKey<Private>, Vec<u8>) {
        let key = p256::SecretKey::from_slice(&[byte; 32]).unwrap();
        let der = key.to_pkcs8_der().unwrap().as_bytes().to_vec();
        (PKey::private_key_from_der(&der).unwrap(), der)
    }
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    let (ca_key, _) = key(1);
    let (leaf_key, private) = key(2);
    let mut subject = X509NameBuilder::new().unwrap();
    subject
        .append_entry_by_text("CN", "Usque ephemeral DNS test CA")
        .unwrap();
    let subject = subject.build();
    let mut ca = X509::builder().unwrap();
    ca.set_version(2).unwrap();
    ca.set_serial_number(&Asn1Integer::from_bn(&BigNum::from_u32(1).unwrap()).unwrap())
        .unwrap();
    ca.set_subject_name(&subject).unwrap();
    ca.set_issuer_name(&subject).unwrap();
    ca.set_pubkey(&ca_key).unwrap();
    ca.set_not_before(&Asn1Time::from_unix(now - 86_400).unwrap())
        .unwrap();
    ca.set_not_after(&Asn1Time::from_unix(now + 86_400).unwrap())
        .unwrap();
    ca.append_extension(BasicConstraints::new().critical().ca().build().unwrap())
        .unwrap();
    ca.append_extension(
        KeyUsage::new()
            .critical()
            .key_cert_sign()
            .crl_sign()
            .build()
            .unwrap(),
    )
    .unwrap();
    ca.sign(&ca_key, MessageDigest::sha256()).unwrap();
    let ca = ca.build();
    let mut leaf = X509::builder().unwrap();
    leaf.set_version(2).unwrap();
    leaf.set_serial_number(&Asn1Integer::from_bn(&BigNum::from_u32(2).unwrap()).unwrap())
        .unwrap();
    leaf.set_subject_name(&subject).unwrap();
    leaf.set_issuer_name(ca.subject_name()).unwrap();
    leaf.set_pubkey(&leaf_key).unwrap();
    leaf.set_not_before(&Asn1Time::from_unix(now - 3_600).unwrap())
        .unwrap();
    leaf.set_not_after(&Asn1Time::from_unix(if expired { now - 60 } else { now + 3_600 }).unwrap())
        .unwrap();
    leaf.append_extension(BasicConstraints::new().critical().build().unwrap())
        .unwrap();
    leaf.append_extension(
        KeyUsage::new()
            .critical()
            .digital_signature()
            .build()
            .unwrap(),
    )
    .unwrap();
    leaf.append_extension(ExtendedKeyUsage::new().server_auth().build().unwrap())
        .unwrap();
    let san = SubjectAlternativeName::new()
        .dns(name)
        .build(&leaf.x509v3_context(Some(&ca), None))
        .unwrap();
    leaf.append_extension(san).unwrap();
    leaf.sign(&ca_key, MessageDigest::sha256()).unwrap();
    let leaf = CertificateDer::from(leaf.build().to_der().unwrap());
    let ca = CertificateDer::from(ca.to_der().unwrap());
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let mut server = ServerConfig::builder_with_provider(Arc::clone(&provider))
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(
            vec![leaf, ca.clone()],
            PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(private)),
        )
        .unwrap();
    let mut roots = RootCertStore::empty();
    roots.add(ca).unwrap();
    let mut client = ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_root_certificates(roots)
        .with_no_client_auth();
    if mode == ConfigMode::Doh {
        server.alpn_protocols = vec![b"h2".to_vec()];
        client.alpn_protocols = vec![b"h2".to_vec()];
    }
    (server, client)
}

fn settings(mode: ConfigMode, endpoint: SocketAddr) -> DirectDnsSettings {
    DirectDnsSettings {
        mode,
        server_name: "resolver.test".to_owned(),
        doh_path: if mode == ConfigMode::Doh {
            "/dns-query".to_owned()
        } else {
            String::new()
        },
        bootstrap_ips: vec![endpoint.ip()],
        port: endpoint.port(),
    }
}

fn context() -> DirectDnsQueryContext {
    DirectDnsQueryContext {
        network_generation: 7,
        deadline: Instant::now() + QUERY_TIMEOUT,
    }
}

fn encrypted(resolver: &DirectDnsResolver) -> &Arc<EncryptedResolver> {
    match resolver {
        DirectDnsResolver::Doh(resolver) => &resolver.inner,
        DirectDnsResolver::Dot(resolver) => &resolver.inner,
        _ => panic!("encrypted test resolver"),
    }
}

struct Harness {
    server: TestServer,
    resolver: Arc<DirectDnsResolver>,
    protector: Arc<SpyProtector>,
    cancellation: CancellationToken,
    quality: NetworkQualityTelemetry,
}

impl Harness {
    async fn new(mode: ConfigMode, behavior: Behavior) -> Self {
        Self::certificate(mode, behavior, "resolver.test", false, false).await
    }
    async fn certificate(
        mode: ConfigMode,
        behavior: Behavior,
        name: &str,
        expired: bool,
        production_trust: bool,
    ) -> Self {
        let (server, client) = test_certificates(mode, name, expired);
        let server = TestServer::start(mode, behavior, server).await;
        let protector = SpyProtector::new();
        let cancellation = CancellationToken::new();
        let quality = NetworkQualityTelemetry::default();
        let tls = if production_trust {
            encrypted_tls_config(mode).unwrap()
        } else {
            client
        };
        let resolver = DirectDnsResolver::with_tls_config(
            settings(mode, server.endpoint),
            protector.clone(),
            quality.clone(),
            &cancellation,
            tls,
        )
        .unwrap();
        Self {
            server,
            resolver,
            protector,
            cancellation,
            quality,
        }
    }
    async fn stop(&self) {
        self.cancellation.cancel();
        encrypted(&self.resolver).clear_idle_pool(true);
        timeout(Duration::from_secs(2), async {
            while self.protector.leases.active.load(Ordering::Acquire) != 0 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("all exact-target leases close after cancellation");
    }
}
impl Drop for Harness {
    fn drop(&mut self) {
        self.cancellation.cancel();
    }
}

#[tokio::test]
async fn doh_and_dot_reuse_verified_connections_and_never_use_plaintext_resolution() {
    for mode in [ConfigMode::Doh, ConfigMode::Dot] {
        let harness = Harness::new(mode, Behavior::Echo).await;
        for id in 1..4 {
            let query = test_query(id);
            assert_eq!(
                harness
                    .resolver
                    .query(query.clone(), context())
                    .await
                    .unwrap()
                    .as_ref(),
                test_response(&query, Behavior::Echo)
            );
        }
        assert_eq!(harness.server.counts.connections.load(Ordering::Acquire), 1);
        assert_eq!(harness.protector.calls.lock().unwrap().len(), 1);
        assert_eq!(
            NetworkQualitySampler::new(harness.quality.clone())
                .sample()
                .direct_dns
                .successes,
            3
        );
        harness.stop().await;
    }
}

#[tokio::test]
async fn doh_rejects_status_redirect_media_type_size_and_reset_without_fallback() {
    for (behavior, expected) in [
        (Behavior::Non200, DirectDnsError::HttpRejected),
        (Behavior::Redirect, DirectDnsError::HttpRejected),
        (Behavior::WrongType, DirectDnsError::InvalidContentType),
        (Behavior::BadParameters, DirectDnsError::InvalidContentType),
        (Behavior::Oversized, DirectDnsError::ResponseTooLarge),
        (
            Behavior::DeclaredOversized,
            DirectDnsError::ResponseTooLarge,
        ),
        (Behavior::Reset, DirectDnsError::QueryFailed),
        (Behavior::BadId, DirectDnsError::InvalidResponse),
        (Behavior::BadQuestion, DirectDnsError::InvalidResponse),
    ] {
        let harness = Harness::new(ConfigMode::Doh, behavior).await;
        assert_eq!(
            harness.resolver.query(test_query(9), context()).await,
            Err(expected),
            "{behavior:?}"
        );
        assert_eq!(harness.protector.calls.lock().unwrap().len(), 1);
        harness.stop().await;
    }
}

#[tokio::test]
async fn dot_framing_and_correlation_failures_discard_the_connection() {
    for behavior in [
        Behavior::ZeroLength,
        Behavior::Eof,
        Behavior::BadId,
        Behavior::BadQuestion,
    ] {
        let harness = Harness::new(ConfigMode::Dot, behavior).await;
        assert!(
            harness
                .resolver
                .query(test_query(9), context())
                .await
                .is_err()
        );
        assert_eq!(harness.protector.leases.active.load(Ordering::Acquire), 0);
        assert!(
            harness
                .resolver
                .query(test_query(10), context())
                .await
                .is_err()
        );
        assert_eq!(harness.server.counts.connections.load(Ordering::Acquire), 2);
        harness.stop().await;
    }
}

#[tokio::test]
async fn tls_requires_public_trust_correct_hostname_and_current_validity() {
    for mode in [ConfigMode::Doh, ConfigMode::Dot] {
        for (name, expired, production) in [
            ("resolver.test", false, true),
            ("wrong.test", false, false),
            ("resolver.test", true, false),
        ] {
            let harness =
                Harness::certificate(mode, Behavior::Echo, name, expired, production).await;
            assert_eq!(
                harness.resolver.query(test_query(3), context()).await,
                Err(DirectDnsError::TlsFailed)
            );
            assert_eq!(harness.server.counts.queries.load(Ordering::Acquire), 0);
            assert_eq!(harness.protector.leases.active.load(Ordering::Acquire), 0);
            harness.stop().await;
        }
    }
}

#[tokio::test]
async fn connection_and_query_concurrency_have_independent_hard_limits() {
    for (mode, count, maximum) in [(ConfigMode::Doh, 64, 64), (ConfigMode::Dot, 12, 4)] {
        let harness = Harness::new(mode, Behavior::Delay(Duration::from_millis(25))).await;
        let mut tasks = JoinSet::new();
        for id in 0..count {
            let resolver = Arc::clone(&harness.resolver);
            tasks.spawn(async move { resolver.query(test_query(id), context()).await });
        }
        while let Some(result) = tasks.join_next().await {
            assert!(result.unwrap().is_ok());
        }
        assert!(harness.server.counts.peak.load(Ordering::Acquire) <= maximum);
        assert!(harness.server.counts.peak.load(Ordering::Acquire) > 1);
        assert!(harness.server.counts.connections.load(Ordering::Acquire) <= MAX_CONNECTIONS);
        assert!(harness.protector.leases.peak.load(Ordering::Acquire) <= MAX_CONNECTIONS);
        harness.stop().await;
    }
}

#[tokio::test]
async fn total_query_deadline_and_network_change_cancel_in_flight_work() {
    for mode in [ConfigMode::Doh, ConfigMode::Dot] {
        let harness = Harness::new(mode, Behavior::Delay(Duration::from_secs(2))).await;
        let started = Instant::now();
        assert_eq!(
            harness
                .resolver
                .query(
                    test_query(2),
                    DirectDnsQueryContext {
                        deadline: started + Duration::from_millis(80),
                        ..context()
                    }
                )
                .await,
            Err(DirectDnsError::Timeout)
        );
        assert!(started.elapsed() < Duration::from_secs(1));
        let resolver = Arc::clone(&harness.resolver);
        let query = tokio::spawn(async move { resolver.query(test_query(3), context()).await });
        timeout(Duration::from_secs(1), async {
            while harness.server.counts.queries.load(Ordering::Acquire) < 2 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        harness.protector.generation.store(8, Ordering::Release);
        assert_eq!(
            timeout(Duration::from_millis(500), query)
                .await
                .unwrap()
                .unwrap(),
            Err(DirectDnsError::NetworkChanged)
        );
        assert_eq!(
            harness.resolver.query(test_query(4), context()).await,
            Err(DirectDnsError::NetworkChanged)
        );
        harness.stop().await;
    }
}

#[tokio::test]
async fn explicit_bootstrap_retry_and_happy_eyeballs_close_losers() {
    for blackhole_v6 in [false, true] {
        let (server, client) = test_certificates(ConfigMode::Doh, "resolver.test", false);
        let server = TestServer::start(ConfigMode::Doh, Behavior::Echo, server).await;
        let protector = SpyProtector::new();
        let mut configuration = settings(ConfigMode::Doh, server.endpoint);
        if blackhole_v6 {
            configuration
                .bootstrap_ips
                .insert(0, IpAddr::V6(Ipv6Addr::LOCALHOST));
            protector.block_ipv6.store(true, Ordering::Release);
        } else {
            configuration
                .bootstrap_ips
                .insert(0, "127.0.0.2".parse().unwrap());
            *protector.reject.lock().unwrap() = Some(configuration.bootstrap_ips[0]);
        }
        let cancel = CancellationToken::new();
        let resolver = DirectDnsResolver::with_tls_config(
            configuration,
            protector.clone(),
            NetworkQualityTelemetry::default(),
            &cancel,
            client,
        )
        .unwrap();
        let started = Instant::now();
        assert!(resolver.query(test_query(1), context()).await.is_ok());
        if blackhole_v6 {
            assert!(started.elapsed() >= HAPPY_EYEBALLS_DELAY);
        }
        assert_eq!(protector.calls.lock().unwrap().len(), 2);
        assert_eq!(protector.leases.active.load(Ordering::Acquire), 1);
        cancel.cancel();
        encrypted(&resolver).clear_idle_pool(true);
    }
}

#[tokio::test]
async fn pool_recycles_at_query_1000_and_closes_idle_connections() {
    for mode in [ConfigMode::Doh, ConfigMode::Dot] {
        let harness = Harness::new(mode, Behavior::Echo).await;
        assert!(
            harness
                .resolver
                .query(test_query(1), context())
                .await
                .is_ok()
        );
        let inner = encrypted(&harness.resolver);
        match &inner.pool {
            ResolverPool::Doh(slots) => slots[0]
                .lock()
                .await
                .as_ref()
                .unwrap()
                .queries
                .store(999, Ordering::Release),
            ResolverPool::Dot(slots) => slots[0].lock().await.as_mut().unwrap().queries = 999,
        }
        assert!(
            harness
                .resolver
                .query(test_query(2), context())
                .await
                .is_ok()
        );
        assert!(
            harness
                .resolver
                .query(test_query(3), context())
                .await
                .is_ok()
        );
        assert_eq!(harness.server.counts.connections.load(Ordering::Acquire), 2);
        match &inner.pool {
            ResolverPool::Doh(slots) => {
                *slots[0]
                    .lock()
                    .await
                    .as_ref()
                    .unwrap()
                    .last_used
                    .lock()
                    .unwrap() = Instant::now() - IDLE_TIMEOUT
            }
            ResolverPool::Dot(slots) => {
                slots[0].lock().await.as_mut().unwrap().last_used = Instant::now() - IDLE_TIMEOUT
            }
        }
        inner.clear_idle_pool(false);
        timeout(Duration::from_secs(1), async {
            while harness.protector.leases.active.load(Ordering::Acquire) != 0 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        harness.stop().await;
    }
}

#[tokio::test]
async fn bounded_queue_rejects_query_65_and_shutdown_releases_all_waiters() {
    let harness = Harness::new(ConfigMode::Doh, Behavior::Echo).await;
    harness.protector.blocked.store(true, Ordering::Release);
    let inner = encrypted(&harness.resolver);
    let mut tasks = JoinSet::new();
    for id in 0..MAX_IN_FLIGHT {
        let resolver = Arc::clone(&harness.resolver);
        tasks.spawn(async move { resolver.query(test_query(id as u16), context()).await });
    }
    timeout(Duration::from_secs(1), async {
        while inner.query_permits.available_permits() != 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        harness.resolver.query(test_query(65), context()).await,
        Err(DirectDnsError::Busy)
    );
    let queue = inner.queue.snapshot(Instant::now());
    assert_eq!(queue.current_items, 64);
    assert_eq!(queue.items_high_water, 64);
    assert_eq!(queue.drop_items, 1);
    harness.cancellation.cancel();
    while let Some(result) = tasks.join_next().await {
        assert_eq!(result.unwrap(), Err(DirectDnsError::Cancelled));
    }
    harness.stop().await;
    assert_eq!(inner.query_permits.available_permits(), 64);
    assert_eq!(inner.queue.snapshot(Instant::now()).current_items, 0);
}

#[tokio::test]
async fn setup_generation_race_and_oversized_query_fail_before_dns_payload() {
    let harness = Harness::new(ConfigMode::Dot, Behavior::Echo).await;
    assert_eq!(
        harness
            .resolver
            .query(Bytes::from(vec![0; MAX_DNS_MESSAGE + 1]), context())
            .await,
        Err(DirectDnsError::InvalidQuery)
    );
    assert!(harness.protector.calls.lock().unwrap().is_empty());
    harness.protector.race.store(true, Ordering::Release);
    assert_eq!(
        harness.resolver.query(test_query(1), context()).await,
        Err(DirectDnsError::NetworkChanged)
    );
    assert_eq!(harness.protector.leases.active.load(Ordering::Acquire), 0);
    assert_eq!(harness.server.counts.queries.load(Ordering::Acquire), 0);
    harness.stop().await;
}

#[tokio::test]
async fn encrypted_truncated_response_never_triggers_plaintext_tcp_retry() {
    for mode in [ConfigMode::Doh, ConfigMode::Dot] {
        let harness = Harness::new(mode, Behavior::Truncated).await;
        let response = harness
            .resolver
            .query(test_query(1), context())
            .await
            .unwrap();
        assert_ne!(response[2] & 2, 0);
        assert_eq!(harness.protector.calls.lock().unwrap().len(), 1);
        harness.stop().await;
    }
}

#[tokio::test]
async fn proxy_hostname_policy_uses_the_same_encrypted_pool() {
    let harness = Harness::new(ConfigMode::Doh, Behavior::Echo).await;
    let wrapper = ConfiguredDnsProtector {
        protector: harness.protector.clone(),
        resolver: Arc::clone(&harness.resolver),
    };
    let resolved = wrapper
        .resolve_direct("direct.example.test", 443)
        .await
        .unwrap();
    assert!(resolved.contains(&"192.0.2.17:443".parse().unwrap()));
    assert!(resolved.contains(&"[2001:db8::17]:443".parse().unwrap()));
    assert!(wrapper.physical_dns_servers().is_empty());
    assert!(wrapper.direct_dns_resolver().unwrap().is_encrypted());
    harness.stop().await;
}

#[tokio::test]
async fn proxy_dns_failure_is_terminal_and_data_fallback_reuses_encrypted_answers() {
    struct Classifier;
    impl crate::GeoDirectClassifier for Classifier {
        fn host_matches(&self, _host: &str, _country: &usque_geo::CountryCode) -> bool {
            true
        }
        fn ip_matches(&self, _ip: IpAddr, _country: &usque_geo::CountryCode) -> bool {
            false
        }
    }
    let policy = crate::GeoDirectPolicy::with_classifier(
        Arc::new(Classifier),
        [usque_geo::CountryCode::parse("CN").unwrap()],
    );
    for behavior in [Behavior::Non200, Behavior::Echo] {
        let harness = Harness::new(ConfigMode::Doh, behavior).await;
        let protector = ConfiguredDnsProtector {
            protector: harness.protector.clone(),
            resolver: Arc::clone(&harness.resolver),
        };
        let fallback = AtomicBool::new(false);
        let result = crate::geo_direct::connect_routed(
            &policy,
            &protector,
            Arc::new(crate::netstack::TrafficCounters::default()),
            (
                crate::geo_direct::GeoTarget::Host("direct.example.test"),
                443,
            ),
            || "encrypted_failure",
            |resolved| async {
                fallback.store(true, Ordering::Release);
                assert!(resolved.unwrap().contains(&"192.0.2.17".parse().unwrap()));
                Err("tunnel_intercepted")
            },
        )
        .await;
        if matches!(behavior, Behavior::Non200) {
            assert!(matches!(result, Err("encrypted_failure")));
            assert!(!fallback.load(Ordering::Acquire));
        } else {
            assert!(matches!(result, Err("tunnel_intercepted")));
            assert!(fallback.load(Ordering::Acquire));
        }
        assert_eq!(harness.server.counts.queries.load(Ordering::Acquire), 2);
        harness.stop().await;
    }
}

#[tokio::test]
async fn a_request_retry_changes_bootstrap_once_and_never_uses_a_third() {
    for both_fail in [false, true] {
        let (server_config, client) = test_certificates(ConfigMode::Doh, "resolver.test", false);
        let first =
            TestServer::start(ConfigMode::Doh, Behavior::Non200, server_config.clone()).await;
        let second = TestServer::start_at(
            ConfigMode::Doh,
            if both_fail {
                Behavior::Non200
            } else {
                Behavior::Echo
            },
            server_config,
            SocketAddr::new("127.0.0.2".parse().unwrap(), first.endpoint.port()),
        )
        .await;
        let protector = SpyProtector::new();
        let mut config = settings(ConfigMode::Doh, first.endpoint);
        config
            .bootstrap_ips
            .extend([second.endpoint.ip(), "127.0.0.3".parse().unwrap()]);
        let cancel = CancellationToken::new();
        let resolver = DirectDnsResolver::with_tls_config(
            config,
            protector.clone(),
            NetworkQualityTelemetry::default(),
            &cancel,
            client,
        )
        .unwrap();
        let result = resolver.query(test_query(9), context()).await;
        if both_fail {
            assert_eq!(result, Err(DirectDnsError::HttpRejected));
        } else {
            assert!(result.is_ok());
        }
        assert_eq!(
            *protector.calls.lock().unwrap(),
            [first.endpoint, second.endpoint]
        );
        assert_eq!(first.counts.queries.load(Ordering::Acquire), 1);
        assert_eq!(second.counts.queries.load(Ordering::Acquire), 1);
        cancel.cancel();
        encrypted(&resolver).clear_idle_pool(true);
    }
}

#[tokio::test]
async fn doh_refuses_a_tls_peer_without_h2_alpn() {
    let (mut server_config, client) = test_certificates(ConfigMode::Doh, "resolver.test", false);
    server_config.alpn_protocols.clear();
    let server = TestServer::start(ConfigMode::Doh, Behavior::Echo, server_config).await;
    let protector = SpyProtector::new();
    let cancel = CancellationToken::new();
    let resolver = DirectDnsResolver::with_tls_config(
        settings(ConfigMode::Doh, server.endpoint),
        protector,
        NetworkQualityTelemetry::default(),
        &cancel,
        client,
    )
    .unwrap();
    assert_eq!(
        resolver.query(test_query(1), context()).await,
        Err(DirectDnsError::AlpnMismatch)
    );
    assert_eq!(server.counts.queries.load(Ordering::Acquire), 0);
    cancel.cancel();
}

#[tokio::test(start_paused = true)]
async fn blocked_socket_preparation_is_in_the_two_point_five_second_connect_budget() {
    let harness = Harness::new(ConfigMode::Dot, Behavior::Echo).await;
    harness.protector.blocked.store(true, Ordering::Release);
    let started = Instant::now();
    assert_eq!(
        harness.resolver.query(test_query(1), context()).await,
        Err(DirectDnsError::Timeout)
    );
    assert_eq!(started.elapsed(), CONNECT_TIMEOUT);
    assert_eq!(harness.protector.leases.active.load(Ordering::Acquire), 0);
    harness.stop().await;
}

#[tokio::test]
async fn caller_drop_discards_a_partial_dot_exchange_and_closed_pool_rejects_new_queries() {
    let harness = Harness::new(ConfigMode::Dot, Behavior::Delay(Duration::from_secs(1))).await;
    let resolver = Arc::clone(&harness.resolver);
    let task = tokio::spawn(async move { resolver.query(test_query(1), context()).await });
    timeout(Duration::from_secs(1), async {
        while harness.server.counts.queries.load(Ordering::Acquire) == 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    task.abort();
    let _ = task.await;
    assert_eq!(harness.protector.leases.active.load(Ordering::Acquire), 0);
    harness.stop().await;
    assert_eq!(
        harness.resolver.query(test_query(2), context()).await,
        Err(DirectDnsError::Cancelled)
    );
}

#[test]
fn media_type_parser_accepts_only_complete_legal_dns_media_types() {
    for value in [
        "application/dns-message",
        " Application/DNS-Message\t",
        "application/dns-message; charset=binary",
        "application/dns-message; a=\"quoted; value\"; b=\"escaped\\\"quote\"",
    ] {
        assert!(valid_dns_media_type(value.as_bytes()), "{value}");
    }
    for value in [
        "application/dns-messageevil",
        "text/plain; x=application/dns-message",
        "application/dns-message, text/plain",
        "application/dns-message;",
        "application/dns-message; x=",
        "application/dns-message; x=\"unterminated",
        "application/dns-message\r\nX: value",
        "application/dns-message; x=y garbage",
    ] {
        assert!(!valid_dns_media_type(value.as_bytes()), "{value:?}");
    }
    let doh = encrypted_tls_config(ConfigMode::Doh).unwrap();
    let dot = encrypted_tls_config(ConfigMode::Dot).unwrap();
    assert_eq!(doh.alpn_protocols, [b"h2"]);
    assert!(dot.alpn_protocols.is_empty());
    assert!(!doh.enable_early_data && !dot.enable_early_data);
}

#[tokio::test]
async fn split_dns_uses_encrypted_transport_and_returns_servfail_without_a_system_resolver() {
    for behavior in [Behavior::Echo, Behavior::Non200] {
        let harness = Harness::new(ConfigMode::Doh, behavior).await;
        let wrapper = Arc::new(ConfiguredDnsProtector {
            protector: harness.protector.clone(),
            resolver: Arc::clone(&harness.resolver),
        });
        let response = crate::split_dns::tests::encrypted_handler_roundtrip(wrapper).await;
        let flags = u16::from_be_bytes([response[2], response[3]]);
        assert_eq!(
            flags & 15,
            if matches!(behavior, Behavior::Echo) {
                0
            } else {
                2
            }
        );
        assert_eq!(harness.server.counts.queries.load(Ordering::Acquire), 1);
        harness.stop().await;
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(1024))]
    #[test]
    fn arbitrary_dns_wire_and_http_media_types_never_panic(
        query in prop::collection::vec(any::<u8>(), 0..4096),
        response in prop::collection::vec(any::<u8>(), 0..4096),
        header in prop::collection::vec(any::<u8>(), 0..2048),
    ) {
        let _ = validate_dns_query(&query);
        let _ = validate_dns_exchange(&query, &response);
        let accepted = valid_dns_media_type(&header);
        if accepted { prop_assert!(header.len() <= 1024 && header.is_ascii()); }
    }
}
