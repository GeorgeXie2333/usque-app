use std::collections::HashSet;
use std::convert::Infallible;
use std::error::Error as StdError;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::str::FromStr;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use bytes::Bytes;
use http::header::{
    CONNECTION, CONTENT_TYPE, HOST, PROXY_AUTHENTICATE, PROXY_AUTHORIZATION, RETRY_AFTER,
};
use http::uri::Authority;
use http::{
    HeaderMap, HeaderName, HeaderValue, Method, Request, Response, StatusCode, Uri, Version,
};
use http_body_util::combinators::BoxBody;
use http_body_util::{BodyExt, Empty, Full};
use hyper::body::{Body, Incoming};
use hyper::client::conn::http1::SendRequest;
use hyper::service::service_fn;
use hyper_util::rt::{TokioIo, TokioTimer};
use tokio::io::AsyncWriteExt;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{Mutex, OwnedSemaphorePermit, Semaphore, watch};
use tokio::task::JoinHandle;
use tokio::time::timeout;
use tokio_util::sync::CancellationToken;
use ts_netstack_smoltcp::CreateSocket;
use ts_netstack_smoltcp::netcore::Channel;
use ts_netstack_smoltcp::netsock::TcpStream as StackTcpStream;
use usque_core::{OperatingMode, Profile, ProxyAuthCredentials};

use crate::dns::Resolver;
use crate::h2::{MasqueTlsIdentity, TransportError};
use crate::netstack::{
    PacketStack, ProxyPerformanceSnapshot, RuntimeHealth, RuntimePath, TrafficSnapshot,
};
use crate::pin_refresh::EndpointPinRefresher;
use crate::port_allocator::next_tcp_port;
use crate::socket::{SocketProtector, noop_socket_protector};

/// Hyper HTTP/1 `max_buf_size` is both the connection I/O window and the
/// unparsed-header cap. Independent of the CONNECT/SOCKS5 relay buffer so
/// relay sizing cannot silently raise the header budget.
const HTTP_IO_BUFFER_SIZE: usize = 128 * 1024;
const MAX_HEADERS: usize = 128;
const HEADER_TIMEOUT: Duration = Duration::from_secs(15);
const REMOTE_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_TARGET_ADDRESSES: usize = 16;
const MAX_SESSION_CONNECTIONS: usize = 32;
const MAX_IDLE_PER_AUTHORITY: usize = 2;
const UPSTREAM_IDLE_TIMEOUT: Duration = Duration::from_secs(90);

type BoxError = Box<dyn StdError + Send + Sync>;
type ProxyBody = BoxBody<Bytes, BoxError>;

pub struct HttpProxyRuntime {
    stack: PacketStack,
    frontend: HttpProxyFrontend,
}

pub(crate) struct HttpProxyFrontend {
    listener_tasks: Vec<JoinHandle<()>>,
    listeners: Vec<SocketAddr>,
    cancellation: CancellationToken,
    failure: watch::Receiver<Option<String>>,
    performance: Arc<HttpPoolCounters>,
}

impl HttpProxyRuntime {
    pub async fn start(
        profile: &Profile,
        identity: MasqueTlsIdentity,
    ) -> Result<Self, TransportError> {
        Self::start_with_protector(profile, identity, noop_socket_protector()).await
    }

    pub async fn start_with_protector(
        profile: &Profile,
        identity: MasqueTlsIdentity,
        protector: Arc<dyn SocketProtector>,
    ) -> Result<Self, TransportError> {
        Self::start_with_refresh(profile, identity, protector, None).await
    }

    pub async fn start_with_refresh(
        profile: &Profile,
        identity: MasqueTlsIdentity,
        protector: Arc<dyn SocketProtector>,
        pin_refresher: Option<Arc<dyn EndpointPinRefresher>>,
    ) -> Result<Self, TransportError> {
        if profile.mode != OperatingMode::HttpProxy {
            return Err(TransportError::UnsupportedOperatingMode);
        }
        if let Err(error) = profile.proxy.listener_credentials() {
            return Err(TransportError::HttpProxy(error.to_string()));
        }

        let bound = HttpProxyFrontend::prebind(profile)?;
        let assigned_ipv4 = identity.assigned_ipv4;
        let assigned_ipv6 = identity.assigned_ipv6;
        let mut stack =
            PacketStack::start_with_refresh(profile, Arc::new(identity), protector, pin_refresher)
                .await?;
        let frontend =
            HttpProxyFrontend::activate(profile, assigned_ipv4, assigned_ipv6, &stack, bound)?;

        tokio::task::yield_now().await;
        let startup_failure = stack.failure.borrow().clone();
        if let Some(message) = startup_failure {
            stack.shutdown().await;
            return Err(TransportError::HttpProxy(message));
        }

        Ok(Self { stack, frontend })
    }

    pub fn path(&self) -> RuntimePath {
        self.stack.path()
    }

    pub fn health(&self) -> RuntimeHealth {
        self.stack.health()
    }

    pub fn listeners(&self) -> &[SocketAddr] {
        self.frontend.listeners()
    }

    pub fn statistics(&self) -> TrafficSnapshot {
        self.stack.counters.snapshot()
    }

    pub fn performance(&self) -> ProxyPerformanceSnapshot {
        let mut snapshot = self.stack.performance();
        self.frontend.augment_performance(&mut snapshot);
        snapshot
    }

    pub fn failure(&self) -> Option<String> {
        self.stack
            .failure
            .borrow()
            .clone()
            .or_else(|| self.frontend.failure())
    }

    pub fn cancel_immediately(&mut self) {
        self.stack.cancel_immediately();
        self.frontend.cancel_immediately();
    }

    pub async fn shutdown(&mut self) {
        self.cancel_immediately();
        self.frontend.shutdown().await;
        self.stack.shutdown().await;
    }
}

impl Drop for HttpProxyRuntime {
    fn drop(&mut self) {
        self.cancel_immediately();
    }
}

impl HttpProxyFrontend {
    pub(crate) fn prebind(profile: &Profile) -> Result<Vec<TcpListener>, TransportError> {
        crate::socket::bind_tcp_listeners(&profile.proxy.http_listeners)
            .map_err(|(address, source)| TransportError::HttpProxyListener { address, source })
    }

    pub(crate) fn activate(
        profile: &Profile,
        assigned_ipv4: Ipv4Addr,
        assigned_ipv6: Ipv6Addr,
        stack: &PacketStack,
        bound: Vec<TcpListener>,
    ) -> Result<Self, TransportError> {
        let auth = match profile.proxy.listener_credentials() {
            Ok(credentials) => credentials.map(Arc::new),
            Err(error) => return Err(TransportError::HttpProxy(error.to_string())),
        };
        let cancellation = stack.cancellation.child_token();
        let (failure_tx, failure) = watch::channel(None);
        let performance = Arc::new(HttpPoolCounters::default());
        let dns_servers = if profile.proxy.dns_mode == usque_core::ProxyDnsMode::LocalConfigured {
            profile.proxy.dns_servers.clone()
        } else {
            profile.dns_servers.clone()
        };
        let resolver = Resolver::new(
            stack.channel.clone(),
            assigned_ipv4,
            assigned_ipv6,
            dns_servers,
            profile.proxy.dns_mode,
            Arc::clone(&stack.protector),
        );
        let context = Arc::new(HttpContext {
            channel: stack.channel.clone(),
            resolver,
            assigned_ipv4,
            assigned_ipv6,
            cancellation: cancellation.clone(),
            failure: failure_tx,
            health: stack.subscribe_health(),
            performance: Arc::clone(&performance),
            auth,
        });
        let listeners = bound
            .iter()
            .filter_map(|listener| listener.local_addr().ok())
            .collect::<Vec<_>>();
        let listener_tasks = bound
            .into_iter()
            .map(|listener| {
                let context = Arc::clone(&context);
                tokio::spawn(async move {
                    run_listener(listener, context).await;
                })
            })
            .collect();
        Ok(Self {
            listener_tasks,
            listeners,
            cancellation,
            failure,
            performance,
        })
    }

    pub(crate) fn listeners(&self) -> &[SocketAddr] {
        &self.listeners
    }

    pub(crate) fn failure(&self) -> Option<String> {
        self.failure.borrow().clone()
    }

    pub(crate) fn augment_performance(&self, snapshot: &mut ProxyPerformanceSnapshot) {
        self.performance.augment(snapshot);
    }

    pub(crate) fn cancel_immediately(&mut self) {
        self.cancellation.cancel();
        for task in &self.listener_tasks {
            task.abort();
        }
    }

    pub(crate) async fn shutdown(&mut self) {
        self.cancel_immediately();
        for task in self.listener_tasks.drain(..) {
            let _ = task.await;
        }
    }
}

impl Drop for HttpProxyFrontend {
    fn drop(&mut self) {
        self.cancel_immediately();
    }
}

struct HttpContext {
    channel: Channel,
    resolver: Resolver,
    assigned_ipv4: Ipv4Addr,
    assigned_ipv6: Ipv6Addr,
    cancellation: CancellationToken,
    failure: watch::Sender<Option<String>>,
    health: watch::Receiver<RuntimeHealth>,
    performance: Arc<HttpPoolCounters>,
    auth: Option<Arc<ProxyAuthCredentials>>,
}

#[derive(Default)]
struct HttpPoolCounters {
    hits: AtomicU64,
    misses: AtomicU64,
    stale_retries: AtomicU64,
    busy_rejections: AtomicU64,
}

impl HttpPoolCounters {
    fn augment(&self, snapshot: &mut ProxyPerformanceSnapshot) {
        snapshot.http_pool_hits = self.hits.load(Ordering::Relaxed);
        snapshot.http_pool_misses = self.misses.load(Ordering::Relaxed);
        snapshot.http_stale_retries = self.stale_retries.load(Ordering::Relaxed);
        snapshot.http_busy_rejections = self.busy_rejections.load(Ordering::Relaxed);
    }
}

async fn run_listener(listener: TcpListener, context: Arc<HttpContext>) {
    loop {
        let accepted = tokio::select! {
            _ = context.cancellation.cancelled() => break,
            accepted = listener.accept() => accepted,
        };
        let (stream, peer) = match accepted {
            Ok(value) => value,
            Err(error) => {
                tracing::error!(%error, "HTTP proxy listener stopped");
                if !context.cancellation.is_cancelled() && context.failure.borrow().is_none() {
                    let _ = context
                        .failure
                        .send(Some(format!("HTTP proxy listener failed: {error}")));
                }
                break;
            }
        };
        if let Err(error) = stream.set_nodelay(true) {
            tracing::debug!(%peer, %error, "could not disable Nagle on HTTP proxy client socket");
        }
        if !peer.ip().is_loopback()
            && stream
                .local_addr()
                .is_ok_and(|address| address.ip().is_loopback())
        {
            tracing::warn!(%peer, "rejected non-loopback peer on a loopback HTTP proxy listener");
            continue;
        }
        let connection_context = Arc::clone(&context);
        tokio::spawn(async move {
            if let Err(error) = serve_client(stream, Arc::clone(&connection_context)).await {
                tracing::debug!(%peer, %error, "HTTP proxy session ended");
            }
        });
    }
}

async fn serve_client(client: TcpStream, context: Arc<HttpContext>) -> Result<(), TransportError> {
    let pool = Arc::new(SessionPool::new(Arc::clone(&context)));
    let service_context = Arc::clone(&context);
    let service = service_fn(move |request| {
        handle_request(request, Arc::clone(&service_context), Arc::clone(&pool))
    });
    let connection = hyper::server::conn::http1::Builder::new()
        .timer(TokioTimer::new())
        .header_read_timeout(HEADER_TIMEOUT)
        .max_headers(MAX_HEADERS)
        .max_buf_size(HTTP_IO_BUFFER_SIZE)
        .keep_alive(true)
        .serve_connection(TokioIo::new(client), service)
        .with_upgrades();
    tokio::select! {
        _ = context.cancellation.cancelled() => Ok(()),
        result = connection => result.map_err(|error| TransportError::HttpProxy(error.to_string())),
    }
}

async fn handle_request(
    mut request: Request<Incoming>,
    context: Arc<HttpContext>,
    pool: Arc<SessionPool>,
) -> Result<Response<ProxyBody>, Infallible> {
    if let Some(response) = authenticate_http_proxy(request.headers(), context.auth.as_deref()) {
        return Ok(response);
    }

    if !matches!(&*context.health.borrow(), RuntimeHealth::Connected { .. }) {
        return Ok(error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "the MASQUE channel is reconnecting",
            true,
        ));
    }

    let response = if request.method() == Method::CONNECT {
        handle_connect(&mut request, context).await
    } else {
        handle_forward(request, pool).await
    };
    Ok(response)
}

fn authenticate_http_proxy(
    headers: &HeaderMap,
    credentials: Option<&ProxyAuthCredentials>,
) -> Option<Response<ProxyBody>> {
    let expected = credentials?;
    let Some(header) = headers.get(PROXY_AUTHORIZATION) else {
        return Some(proxy_auth_required());
    };
    let Ok(value) = header.to_str() else {
        return Some(proxy_auth_required());
    };
    let Some((username, password)) = ProxyAuthCredentials::decode_http_basic(value) else {
        return Some(proxy_auth_required());
    };
    if expected.matches(&username, &password) {
        None
    } else {
        Some(proxy_auth_required())
    }
}

fn proxy_auth_required() -> Response<ProxyBody> {
    let mut response = error_response(
        StatusCode::PROXY_AUTHENTICATION_REQUIRED,
        "proxy authentication required",
        false,
    );
    response.headers_mut().insert(
        PROXY_AUTHENTICATE,
        HeaderValue::from_static(r#"Basic realm="Usque""#),
    );
    response
}

async fn handle_connect(
    request: &mut Request<Incoming>,
    context: Arc<HttpContext>,
) -> Response<ProxyBody> {
    let destination = match connect_destination(request.uri()) {
        Ok(destination) => destination,
        Err(message) => return error_response(StatusCode::BAD_REQUEST, &message, false),
    };
    let addresses = match context.resolver.resolve(&destination.host).await {
        Ok(addresses) => addresses,
        Err(error) => {
            return error_response(StatusCode::BAD_GATEWAY, &error.to_string(), false);
        }
    };
    let remote = match connect_remote(&context, &addresses, destination.port).await {
        Ok(remote) => remote,
        Err(RemoteConnectError::BudgetExhausted) => {
            return error_response(
                StatusCode::SERVICE_UNAVAILABLE,
                "the proxy connection memory budget is temporarily exhausted",
                true,
            );
        }
        Err(RemoteConnectError::Failed(message)) => {
            return error_response(StatusCode::BAD_GATEWAY, &message, false);
        }
    };
    let upgrade = hyper::upgrade::on(request);
    let cancellation = context.cancellation.clone();
    tokio::spawn(async move {
        let Ok(upgraded) = upgrade.await else {
            return;
        };
        tokio::select! {
            _ = cancellation.cancelled() => {}
            result = relay_connect_upgrade(upgraded, remote) => {
                if let Err(error) = result {
                    tracing::debug!(%error, "HTTP CONNECT relay ended");
                }
            }
        }
    });

    let mut response = Response::new(empty_body());
    *response.status_mut() = StatusCode::OK;
    response
}

async fn relay_connect_upgrade(
    upgraded: hyper::upgrade::Upgraded,
    mut remote: StackTcpStream,
) -> std::io::Result<()> {
    let parts = upgraded.downcast::<TokioIo<TcpStream>>().map_err(|_| {
        std::io::Error::other("HTTP CONNECT upgrade did not wrap TokioIo<TcpStream>")
    })?;
    let mut client = parts.io.into_inner();
    if !parts.read_buf.is_empty() {
        remote.write_all(&parts.read_buf).await?;
        remote.flush().await?;
    }
    crate::relay::copy_bidirectional(&mut client, &mut remote).await?;
    Ok(())
}

async fn handle_forward(
    mut request: Request<Incoming>,
    pool: Arc<SessionPool>,
) -> Response<ProxyBody> {
    let destination = match forward_destination(request.uri(), request.headers()) {
        Ok(destination) => destination,
        Err(message) => return error_response(StatusCode::BAD_REQUEST, &message, false),
    };
    if let Err(message) = prepare_forward_request(&mut request, &destination) {
        return error_response(StatusCode::BAD_REQUEST, &message, false);
    }

    let replay = ReplayTemplate::from_request(&request);
    let replayable =
        request.body().size_hint().exact() == Some(0) && is_safely_replayable(request.method());
    let (parts, body) = request.into_parts();
    let body = body
        .map_err(|error| -> BoxError { Box::new(error) })
        .boxed();
    let request = Request::from_parts(parts, body);

    let first = pool.send(&destination.authority, request, false).await;
    let mut response = match first {
        Ok(response) => response,
        Err(PoolError::StaleReused) if replayable => {
            let Some(replay) = replay else {
                return error_response(StatusCode::BAD_GATEWAY, "upstream closed", false);
            };
            match pool
                .send(&destination.authority, replay.into_request(), true)
                .await
            {
                Ok(response) => response,
                Err(error) => return pool_error_response(error),
            }
        }
        Err(error) => return pool_error_response(error),
    };
    strip_hop_by_hop(response.headers_mut());
    let (parts, body) = response.into_parts();
    let body = body
        .map_err(|error| -> BoxError { Box::new(error) })
        .boxed();
    Response::from_parts(parts, body)
}

#[derive(Clone)]
struct ReplayTemplate {
    method: Method,
    uri: Uri,
    version: Version,
    headers: HeaderMap,
}

impl ReplayTemplate {
    fn from_request(request: &Request<Incoming>) -> Option<Self> {
        (request.body().size_hint().exact() == Some(0)).then(|| Self {
            method: request.method().clone(),
            uri: request.uri().clone(),
            version: request.version(),
            headers: request.headers().clone(),
        })
    }

    fn into_request(self) -> Request<ProxyBody> {
        let mut request = Request::new(empty_body());
        *request.method_mut() = self.method;
        *request.uri_mut() = self.uri;
        *request.version_mut() = self.version;
        *request.headers_mut() = self.headers;
        request
    }
}

fn is_safely_replayable(method: &Method) -> bool {
    matches!(
        *method,
        Method::GET | Method::HEAD | Method::OPTIONS | Method::TRACE
    )
}

struct SessionPool {
    context: Arc<HttpContext>,
    idle: Mutex<Vec<PoolEntry>>,
    permits: Arc<Semaphore>,
}

struct PoolEntry {
    authority: String,
    last_used: Instant,
    sender: SendRequest<ProxyBody>,
    _permit: OwnedSemaphorePermit,
}

impl SessionPool {
    fn new(context: Arc<HttpContext>) -> Self {
        Self {
            context,
            idle: Mutex::new(Vec::new()),
            permits: Arc::new(Semaphore::new(MAX_SESSION_CONNECTIONS)),
        }
    }

    async fn send(
        &self,
        authority: &str,
        request: Request<ProxyBody>,
        force_new: bool,
    ) -> Result<Response<Incoming>, PoolError> {
        let (mut entry, reused) = self.acquire(authority, force_new).await?;
        if entry.sender.ready().await.is_err() {
            if reused {
                self.context
                    .performance
                    .stale_retries
                    .fetch_add(1, Ordering::Relaxed);
            }
            return Err(if reused {
                PoolError::StaleReused
            } else {
                PoolError::Upstream("new upstream connection closed before use".to_owned())
            });
        }
        let response = match entry.sender.send_request(request).await {
            Ok(response) => response,
            Err(error) => {
                if reused {
                    self.context
                        .performance
                        .stale_retries
                        .fetch_add(1, Ordering::Relaxed);
                }
                return Err(if reused {
                    PoolError::StaleReused
                } else {
                    PoolError::Upstream(error.to_string())
                });
            }
        };
        self.release(entry).await;
        Ok(response)
    }

    async fn acquire(
        &self,
        authority: &str,
        force_new: bool,
    ) -> Result<(PoolEntry, bool), PoolError> {
        if !force_new {
            let mut idle = self.idle.lock().await;
            prune_idle(&mut idle);
            if let Some(index) = idle.iter().rposition(|entry| {
                entry.authority.eq_ignore_ascii_case(authority) && entry.sender.is_ready()
            }) {
                self.context
                    .performance
                    .hits
                    .fetch_add(1, Ordering::Relaxed);
                return Ok((idle.swap_remove(index), true));
            }
        }

        self.context
            .performance
            .misses
            .fetch_add(1, Ordering::Relaxed);

        let permit = Arc::clone(&self.permits).try_acquire_owned().map_err(|_| {
            self.context
                .performance
                .busy_rejections
                .fetch_add(1, Ordering::Relaxed);
            PoolError::Busy
        })?;
        let destination = parse_destination(authority, 80).map_err(PoolError::Upstream)?;
        let addresses = self
            .context
            .resolver
            .resolve(&destination.host)
            .await
            .map_err(|error| PoolError::Upstream(error.to_string()))?;
        let remote = connect_remote(&self.context, &addresses, destination.port)
            .await
            .map_err(|error| match error {
                RemoteConnectError::BudgetExhausted => PoolError::Busy,
                RemoteConnectError::Failed(message) => PoolError::Upstream(message),
            })?;
        let (sender, connection) = hyper::client::conn::http1::Builder::new()
            .max_buf_size(HTTP_IO_BUFFER_SIZE)
            .handshake::<_, ProxyBody>(TokioIo::new(remote))
            .await
            .map_err(|error| PoolError::Upstream(error.to_string()))?;
        let cancellation = self.context.cancellation.clone();
        tokio::spawn(async move {
            tokio::select! {
                _ = cancellation.cancelled() => {}
                result = connection => {
                    if let Err(error) = result {
                        tracing::debug!(%error, "pooled HTTP upstream connection ended");
                    }
                }
            }
        });
        Ok((
            PoolEntry {
                authority: destination.authority,
                last_used: Instant::now(),
                sender,
                _permit: permit,
            },
            false,
        ))
    }

    async fn release(&self, mut entry: PoolEntry) {
        if entry.sender.is_closed() {
            return;
        }
        entry.last_used = Instant::now();
        let mut idle = self.idle.lock().await;
        prune_idle(&mut idle);
        let same_authority = idle
            .iter()
            .filter(|candidate| candidate.authority.eq_ignore_ascii_case(&entry.authority))
            .count();
        if same_authority < MAX_IDLE_PER_AUTHORITY {
            idle.push(entry);
        }
    }
}

fn prune_idle(idle: &mut Vec<PoolEntry>) {
    idle.retain(|entry| {
        !entry.sender.is_closed() && entry.last_used.elapsed() < UPSTREAM_IDLE_TIMEOUT
    });
}

#[derive(Debug)]
enum PoolError {
    Busy,
    StaleReused,
    Upstream(String),
}

fn pool_error_response(error: PoolError) -> Response<ProxyBody> {
    match error {
        PoolError::Busy => error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "the proxy connection memory budget is temporarily exhausted",
            true,
        ),
        PoolError::StaleReused => error_response(
            StatusCode::BAD_GATEWAY,
            "the reused upstream connection closed before responding",
            false,
        ),
        PoolError::Upstream(message) => error_response(StatusCode::BAD_GATEWAY, &message, false),
    }
}

struct Destination {
    host: String,
    port: u16,
    authority: String,
    origin_form: Option<Uri>,
}

fn connect_destination(uri: &Uri) -> Result<Destination, String> {
    let authority = uri
        .authority()
        .map(Authority::as_str)
        .unwrap_or_else(|| uri.path());
    parse_destination(authority, 443)
}

fn forward_destination(uri: &Uri, headers: &HeaderMap) -> Result<Destination, String> {
    if let Some(scheme) = uri.scheme_str()
        && !scheme.eq_ignore_ascii_case("http")
    {
        return Err(
            "ordinary proxy forwarding supports only http://; use CONNECT for TLS".to_owned(),
        );
    }
    let authority = if let Some(authority) = uri.authority() {
        authority.as_str()
    } else {
        headers
            .get(HOST)
            .and_then(|value| value.to_str().ok())
            .ok_or_else(|| "origin-form proxy request requires a valid Host header".to_owned())?
    };
    let mut destination = parse_destination(authority, 80)?;
    destination.origin_form = Some(
        uri.path_and_query()
            .map(|value| value.as_str())
            .filter(|value| !value.is_empty())
            .unwrap_or("/")
            .parse()
            .map_err(|_| "invalid HTTP request target".to_owned())?,
    );
    Ok(destination)
}

fn parse_destination(value: &str, default_port: u16) -> Result<Destination, String> {
    if value.contains('@') {
        return Err("userinfo is not allowed in proxy authorities".to_owned());
    }
    let authority = Authority::from_str(value).map_err(|_| "invalid proxy authority".to_owned())?;
    if authority.host().is_empty() {
        return Err("proxy authority host is empty".to_owned());
    }
    let port = authority.port_u16().unwrap_or(default_port);
    if port == 0 {
        return Err("proxy target port is zero".to_owned());
    }
    Ok(Destination {
        host: authority.host().to_owned(),
        port,
        authority: authority.to_string(),
        origin_form: None,
    })
}

fn prepare_forward_request(
    request: &mut Request<Incoming>,
    destination: &Destination,
) -> Result<(), String> {
    strip_hop_by_hop(request.headers_mut());
    request.headers_mut().insert(
        HOST,
        HeaderValue::from_str(&destination.authority)
            .map_err(|_| "invalid proxy authority".to_owned())?,
    );
    *request.uri_mut() = destination
        .origin_form
        .clone()
        .ok_or_else(|| "forward target has no origin form".to_owned())?;
    Ok(())
}

fn strip_hop_by_hop(headers: &mut HeaderMap) {
    let connection_tokens = headers
        .get_all(CONNECTION)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .filter_map(|token| HeaderName::from_bytes(token.trim().as_bytes()).ok())
        .collect::<HashSet<_>>();
    for name in connection_tokens {
        headers.remove(name);
    }
    for name in [
        "connection",
        "proxy-connection",
        "proxy-authorization",
        "proxy-authenticate",
        "keep-alive",
        "te",
        "trailer",
        "transfer-encoding",
        "upgrade",
    ] {
        headers.remove(name);
    }
}

enum RemoteConnectError {
    BudgetExhausted,
    Failed(String),
}

async fn connect_remote(
    context: &HttpContext,
    addresses: &[IpAddr],
    port: u16,
) -> Result<StackTcpStream, RemoteConnectError> {
    let mut failures = Vec::new();
    for address in addresses.iter().take(MAX_TARGET_ADDRESSES) {
        let local_ip = match address {
            IpAddr::V4(_) => IpAddr::V4(context.assigned_ipv4),
            IpAddr::V6(_) => IpAddr::V6(context.assigned_ipv6),
        };
        let local = SocketAddr::new(local_ip, next_tcp_port());
        let remote = SocketAddr::new(*address, port);
        match timeout(
            REMOTE_CONNECT_TIMEOUT,
            context.channel.tcp_connect(local, remote),
        )
        .await
        {
            Ok(Ok(stream)) => return Ok(stream),
            Ok(Err(error)) if error.is_tcp_buffer_budget_exhausted() => {
                return Err(RemoteConnectError::BudgetExhausted);
            }
            Ok(Err(error)) => failures.push(format!("{remote}: {error}")),
            Err(_) => failures.push(format!("{remote}: timed out")),
        }
    }
    Err(RemoteConnectError::Failed(if failures.is_empty() {
        "no usable target address".to_owned()
    } else {
        failures.join("; ")
    }))
}

fn error_response(status: StatusCode, message: &str, retry: bool) -> Response<ProxyBody> {
    let body = format!(
        "{} {}\n",
        status.as_u16(),
        status.canonical_reason().unwrap_or(message)
    );
    let mut response = Response::new(full_body(Bytes::from(body)));
    *response.status_mut() = status;
    response.headers_mut().insert(
        CONTENT_TYPE,
        HeaderValue::from_static("text/plain; charset=utf-8"),
    );
    if retry {
        response
            .headers_mut()
            .insert(RETRY_AFTER, HeaderValue::from_static("1"));
    }
    response
}

fn empty_body() -> ProxyBody {
    Empty::<Bytes>::new()
        .map_err(|never| -> BoxError { match never {} })
        .boxed()
}

fn full_body(bytes: Bytes) -> ProxyBody {
    Full::new(bytes)
        .map_err(|never| -> BoxError { match never {} })
        .boxed()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use ts_netstack_smoltcp::netcore::{Config, HasChannel, NetstackControl};

    #[test]
    fn parses_absolute_and_origin_form_targets() {
        let absolute: Uri = "http://example.com:8080/a?q=1".parse().unwrap();
        let destination = forward_destination(&absolute, &HeaderMap::new()).unwrap();
        assert_eq!(destination.host, "example.com");
        assert_eq!(destination.port, 8080);
        assert_eq!(destination.origin_form.unwrap().to_string(), "/a?q=1");

        let origin: Uri = "/resource".parse().unwrap();
        let mut headers = HeaderMap::new();
        headers.insert(HOST, HeaderValue::from_static("example.net"));
        let destination = forward_destination(&origin, &headers).unwrap();
        assert_eq!(destination.authority, "example.net");
        assert_eq!(destination.origin_form.unwrap().to_string(), "/resource");
    }

    #[test]
    fn strips_declared_and_standard_hop_headers() {
        let mut headers = HeaderMap::new();
        headers.insert(CONNECTION, HeaderValue::from_static("X-Hop, keep-alive"));
        headers.insert("x-hop", HeaderValue::from_static("remove"));
        headers.insert("proxy-authorization", HeaderValue::from_static("remove"));
        headers.insert("x-end-to-end", HeaderValue::from_static("keep"));
        strip_hop_by_hop(&mut headers);
        assert!(!headers.contains_key(CONNECTION));
        assert!(!headers.contains_key("x-hop"));
        assert!(!headers.contains_key("proxy-authorization"));
        assert_eq!(headers["x-end-to-end"], "keep");
    }

    #[test]
    fn missing_or_wrong_basic_auth_returns_407_without_contacting_upstream() {
        let credentials = ProxyAuthCredentials::parse("lan-user", b"s3cret").unwrap();
        let mut headers = HeaderMap::new();
        let missing = authenticate_http_proxy(&headers, Some(&credentials)).unwrap();
        assert_eq!(missing.status(), StatusCode::PROXY_AUTHENTICATION_REQUIRED);
        assert_eq!(
            missing.headers()[PROXY_AUTHENTICATE],
            r#"Basic realm="Usque""#
        );

        headers.insert(
            PROXY_AUTHORIZATION,
            HeaderValue::from_static("Basic d3Jvbmc6Y3JlZHM="),
        );
        let wrong = authenticate_http_proxy(&headers, Some(&credentials)).unwrap();
        assert_eq!(wrong.status(), StatusCode::PROXY_AUTHENTICATION_REQUIRED);
        assert_eq!(
            wrong.headers()[PROXY_AUTHENTICATE],
            r#"Basic realm="Usque""#
        );

        headers.insert(
            PROXY_AUTHORIZATION,
            HeaderValue::from_static("Basic bGFuLXVzZXI6czNjcmV0"),
        );
        assert!(authenticate_http_proxy(&headers, Some(&credentials)).is_none());
        assert!(authenticate_http_proxy(&HeaderMap::new(), None).is_none());
    }

    #[test]
    fn retry_policy_is_limited_to_safe_bodyless_methods() {
        assert!(is_safely_replayable(&Method::GET));
        assert!(is_safely_replayable(&Method::HEAD));
        assert!(is_safely_replayable(&Method::OPTIONS));
        assert!(is_safely_replayable(&Method::TRACE));
        assert!(!is_safely_replayable(&Method::POST));
        assert!(!is_safely_replayable(&Method::PUT));
    }

    #[test]
    fn rejects_non_http_forwarding_and_userinfo() {
        let https: Uri = "https://example.com/".parse().unwrap();
        assert!(forward_destination(&https, &HeaderMap::new()).is_err());
        assert!(parse_destination("user@example.com", 80).is_err());
    }

    struct StackTasks {
        client: JoinHandle<()>,
        server: JoinHandle<()>,
    }

    impl Drop for StackTasks {
        fn drop(&mut self) {
            self.client.abort();
            self.server.abort();
        }
    }

    struct StackedProxy {
        context: Arc<HttpContext>,
        origin: ts_netstack_smoltcp::netsock::TcpListener,
        _tasks: StackTasks,
    }

    impl StackedProxy {
        async fn new() -> Self {
            let (client_stack, server_stack) = ts_netstack_smoltcp::piped_pair(Config::default());
            let client_channel = client_stack.command_channel();
            let server_channel = server_stack.command_channel();
            let tasks = StackTasks {
                client: client_stack.spawn_tokio(),
                server: server_stack.spawn_tokio(),
            };
            let client_ip = Ipv4Addr::new(10, 0, 0, 1);
            let server_ip = Ipv4Addr::new(10, 0, 0, 2);
            client_channel
                .set_ips([IpAddr::V4(client_ip)])
                .await
                .unwrap();
            server_channel
                .set_ips([IpAddr::V4(server_ip)])
                .await
                .unwrap();
            let origin = server_channel
                .tcp_listen(SocketAddr::new(IpAddr::V4(server_ip), 8080))
                .await
                .unwrap();
            let (failure, _) = watch::channel(None);
            let (_, health) = watch::channel(RuntimeHealth::Connected {
                path: RuntimePath {
                    transport: usque_core::Transport::Http3,
                    endpoint_family: usque_core::AddressFamily::Ipv4,
                    ipv4_available: true,
                    ipv6_available: true,
                },
                reconnect_count: 0,
            });
            Self {
                context: Arc::new(HttpContext {
                    resolver: Resolver::new(
                        client_channel.clone(),
                        client_ip,
                        Ipv6Addr::LOCALHOST,
                        Vec::new(),
                        usque_core::ProxyDnsMode::Remote,
                        crate::socket::noop_socket_protector(),
                    ),
                    channel: client_channel,
                    assigned_ipv4: client_ip,
                    assigned_ipv6: Ipv6Addr::LOCALHOST,
                    cancellation: CancellationToken::new(),
                    failure,
                    health,
                    performance: Arc::new(HttpPoolCounters::default()),
                    auth: None,
                }),
                origin,
                _tasks: tasks,
            }
        }
    }

    #[tokio::test]
    async fn session_pool_reuses_one_upstream_connection_for_two_requests() {
        let StackedProxy {
            context,
            origin: listener,
            _tasks,
        } = StackedProxy::new().await;
        let accepts = Arc::new(AtomicUsize::new(0));
        let requests = Arc::new(AtomicUsize::new(0));
        let server_accepts = Arc::clone(&accepts);
        let server_requests = Arc::clone(&requests);
        let http_server = tokio::spawn(async move {
            let stream = listener.accept().await.unwrap();
            server_accepts.fetch_add(1, Ordering::Relaxed);
            let service = service_fn(move |_request| {
                server_requests.fetch_add(1, Ordering::Relaxed);
                async move { Ok::<_, Infallible>(Response::new(Full::new(Bytes::from_static(b"ok")))) }
            });
            hyper::server::conn::http1::Builder::new()
                .keep_alive(true)
                .serve_connection(TokioIo::new(stream), service)
                .await
                .unwrap();
        });
        let pool = SessionPool::new(Arc::clone(&context));

        for path in ["/one", "/two"] {
            let mut request = Request::new(empty_body());
            *request.method_mut() = Method::GET;
            *request.uri_mut() = path.parse().unwrap();
            request
                .headers_mut()
                .insert(HOST, HeaderValue::from_static("10.0.0.2:8080"));
            let response = pool.send("10.0.0.2:8080", request, false).await.unwrap();
            let body = response.into_body().collect().await.unwrap().to_bytes();
            assert_eq!(body, "ok");
        }

        assert_eq!(accepts.load(Ordering::Relaxed), 1);
        assert_eq!(requests.load(Ordering::Relaxed), 2);
        assert_eq!(context.performance.hits.load(Ordering::Relaxed), 1);
        assert_eq!(context.performance.misses.load(Ordering::Relaxed), 1);

        drop(pool);
        http_server.abort();
    }

    #[tokio::test]
    async fn connect_relays_pipelined_bytes_on_the_raw_socket() {
        let StackedProxy {
            context,
            origin: listener,
            _tasks,
        } = StackedProxy::new().await;
        let origin = tokio::spawn(async move {
            let mut stream = listener.accept().await.unwrap();
            let mut buf = [0u8; 5];
            stream.read_exact(&mut buf).await.unwrap();
            assert_eq!(&buf, b"HELLO");
            stream.write_all(b"WORLD").await.unwrap();
            stream
        });
        let frontend = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let proxy_addr = frontend.local_addr().unwrap();
        let proxy_task = tokio::spawn(run_listener(frontend, context));

        let mut client = TcpStream::connect(proxy_addr).await.unwrap();
        client
            .write_all(b"CONNECT 10.0.0.2:8080 HTTP/1.1\r\nHost: 10.0.0.2:8080\r\n\r\nHELLO")
            .await
            .unwrap();
        let mut head = Vec::new();
        while !head.ends_with(b"\r\n\r\n") {
            assert!(head.len() < 4096);
            let mut byte = [0u8; 1];
            client.read_exact(&mut byte).await.unwrap();
            head.push(byte[0]);
        }
        assert!(
            head.starts_with(b"HTTP/1.1 200 "),
            "CONNECT response: {}",
            String::from_utf8_lossy(&head)
        );
        let mut payload = [0u8; 5];
        tokio::time::timeout(Duration::from_secs(2), client.read_exact(&mut payload))
            .await
            .expect("CONNECT tunnel payload deadline")
            .unwrap();
        assert_eq!(&payload, b"WORLD");

        drop(client);
        tokio::time::timeout(Duration::from_secs(2), origin)
            .await
            .expect("origin task deadline")
            .unwrap();
        proxy_task.abort();
    }

    #[tokio::test]
    async fn forward_relays_a_body_larger_than_the_old_64kib_window() {
        const BODY_LEN: usize = 64 * 1024 + 1;
        let payload = Bytes::from(vec![0x5A; BODY_LEN]);
        let StackedProxy {
            context,
            origin: listener,
            _tasks,
        } = StackedProxy::new().await;
        let origin_body = payload.clone();
        let origin = tokio::spawn(async move {
            let stream = listener.accept().await.unwrap();
            let service = service_fn(move |_request| {
                let body = origin_body.clone();
                async move { Ok::<_, Infallible>(Response::new(Full::new(body))) }
            });
            hyper::server::conn::http1::Builder::new()
                .keep_alive(false)
                .serve_connection(TokioIo::new(stream), service)
                .await
                .unwrap();
        });
        let frontend = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let proxy_addr = frontend.local_addr().unwrap();
        let proxy_task = tokio::spawn(run_listener(frontend, context));

        let mut client = TcpStream::connect(proxy_addr).await.unwrap();
        client
            .write_all(
                b"GET http://10.0.0.2:8080/large HTTP/1.1\r\nHost: 10.0.0.2:8080\r\nConnection: close\r\n\r\n",
            )
            .await
            .unwrap();
        let mut buf = Vec::new();
        loop {
            let mut chunk = [0u8; 8192];
            let n = tokio::time::timeout(Duration::from_secs(5), client.read(&mut chunk))
                .await
                .expect("forward body deadline")
                .unwrap();
            if n == 0 {
                break;
            }
            buf.extend_from_slice(&chunk[..n]);
        }
        let header_end = buf
            .windows(4)
            .position(|window| window == b"\r\n\r\n")
            .unwrap()
            + 4;
        assert!(
            buf.starts_with(b"HTTP/1.1 200"),
            "forward response: {}",
            String::from_utf8_lossy(&buf[..header_end.min(buf.len())])
        );
        assert_eq!(&buf[header_end..], payload.as_ref());

        drop(client);
        tokio::time::timeout(Duration::from_secs(2), origin)
            .await
            .expect("origin task deadline")
            .unwrap();
        proxy_task.abort();
    }
}
