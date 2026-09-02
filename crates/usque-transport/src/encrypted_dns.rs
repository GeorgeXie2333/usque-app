//! Explicit-bootstrap encrypted direct DNS. No encrypted error branch can
//! invoke the system resolver, physical DNS discovery, or a port-53 fallback.

use std::net::{IpAddr, SocketAddr};
use std::ops::{Deref, DerefMut};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;

use async_trait::async_trait;
use bytes::{Bytes, BytesMut};
use h2::client::SendRequest;
use http::{HeaderMap, Method, Request, StatusCode, Version};
use rustls::pki_types::ServerName;
use rustls::{ClientConfig, RootCertStore};
use thiserror::Error;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpSocket, TcpStream};
use tokio::sync::{Mutex, MutexGuard, Notify, OwnedSemaphorePermit, Semaphore};
use tokio::time::{Instant, MissedTickBehavior, interval, sleep, timeout_at};
use tokio_rustls::client::TlsStream;
use tokio_util::sync::CancellationToken;
use tokio_util::task::AbortOnDropHandle;
use usque_core::{DirectDnsMode as ConfigMode, DirectDnsSettings};

use crate::h2::TransportError;
use crate::network_quality::{DirectDnsMode, DirectDnsReasonCode, NetworkQualityTelemetry};
use crate::queue_metrics::{QueueKind, QueueMetrics};
use crate::socket::{
    DirectEgressLease, DirectProtocol, LeasedIo, STALE_GENERATION_REASON, SocketHandle,
    SocketProtector, socket_handle,
};
use crate::split_dns::{
    physical_wire_query, resolve_encrypted_host, validate_dns_exchange, validate_dns_query,
};

pub const ENCRYPTED_DIRECT_DNS_ENABLED: bool = true;
const MAX_CONNECTIONS: usize = 4;
const MAX_IN_FLIGHT: usize = 64;
const H2_REQUESTS_PER_CONNECTION: usize = MAX_IN_FLIGHT / MAX_CONNECTIONS;
const MAX_QUERIES_PER_CONNECTION: u32 = 1_000;
const MAX_DNS_MESSAGE: usize = u16::MAX as usize;
const MAX_HTTP_HEADERS: u32 = 16 * 1024;
const IDLE_TIMEOUT: Duration = Duration::from_secs(60);
const QUERY_TIMEOUT: Duration = Duration::from_secs(4);
const CONNECT_TIMEOUT: Duration = Duration::from_millis(2_500);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(3);
const HAPPY_EYEBALLS_DELAY: Duration = Duration::from_millis(250);
const GENERATION_POLL_INTERVAL: Duration = Duration::from_millis(100);

type DnsTlsStream = TlsStream<LeasedIo<TcpStream>>;

#[derive(Clone, Copy)]
pub struct DirectDnsQueryContext {
    pub network_generation: u64,
    pub deadline: Instant,
}

/// Fixed codes only: Debug/Display never contain a query, endpoint, name,
/// certificate, OS error, or HTTP header received from the peer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum DirectDnsError {
    #[error("unsupported")]
    Unsupported,
    #[error("invalid_configuration")]
    InvalidConfiguration,
    #[error("invalid_query")]
    InvalidQuery,
    #[error("bootstrap_unavailable")]
    BootstrapUnavailable,
    #[error("socket_protect_failed")]
    SocketProtectFailed,
    #[error("connect_failed")]
    ConnectFailed,
    #[error("tls_failed")]
    TlsFailed,
    #[error("alpn_mismatch")]
    AlpnMismatch,
    #[error("http_rejected")]
    HttpRejected,
    #[error("invalid_content_type")]
    InvalidContentType,
    #[error("response_too_large")]
    ResponseTooLarge,
    #[error("invalid_response")]
    InvalidResponse,
    #[error("query_failed")]
    QueryFailed,
    #[error("timeout")]
    Timeout,
    #[error("network_changed")]
    NetworkChanged,
    #[error("cancelled")]
    Cancelled,
    #[error("busy")]
    Busy,
}

impl DirectDnsError {
    fn quality_reason(self) -> DirectDnsReasonCode {
        match self {
            Self::Timeout => DirectDnsReasonCode::Timeout,
            Self::NetworkChanged => DirectDnsReasonCode::NetworkChanged,
            Self::Unsupported => DirectDnsReasonCode::Unsupported,
            _ => DirectDnsReasonCode::QueryFailed,
        }
    }

    fn permits_retry(self) -> bool {
        !matches!(
            self,
            Self::Unsupported
                | Self::InvalidConfiguration
                | Self::InvalidQuery
                | Self::NetworkChanged
                | Self::Cancelled
                | Self::Timeout
                | Self::Busy
        )
    }
}

pub enum DirectDnsResolver {
    PhysicalSystem(PhysicalDnsResolver),
    Doh(DohResolver),
    Dot(DotResolver),
}

pub struct PhysicalDnsResolver {
    protector: Arc<dyn SocketProtector>,
}

pub struct DohResolver {
    inner: Arc<EncryptedResolver>,
}

pub struct DotResolver {
    inner: Arc<EncryptedResolver>,
}

impl DirectDnsResolver {
    pub fn new(
        settings: &DirectDnsSettings,
        protector: Arc<dyn SocketProtector>,
        quality: NetworkQualityTelemetry,
        cancellation: &CancellationToken,
    ) -> Result<Arc<Self>, DirectDnsError> {
        let mut settings = settings.clone();
        settings.canonicalize();
        settings
            .validate()
            .map_err(|_| DirectDnsError::InvalidConfiguration)?;
        if settings.mode == ConfigMode::PhysicalSystem {
            return Ok(Arc::new(Self::PhysicalSystem(PhysicalDnsResolver {
                protector,
            })));
        }
        if !ENCRYPTED_DIRECT_DNS_ENABLED {
            return Err(DirectDnsError::Unsupported);
        }
        let tls = encrypted_tls_config(settings.mode)?;
        Self::with_tls_config(settings, protector, quality, cancellation, tls)
    }

    fn with_tls_config(
        settings: DirectDnsSettings,
        protector: Arc<dyn SocketProtector>,
        quality: NetworkQualityTelemetry,
        cancellation: &CancellationToken,
        tls: ClientConfig,
    ) -> Result<Arc<Self>, DirectDnsError> {
        let mode = settings.mode;
        if !matches!(mode, ConfigMode::Doh | ConfigMode::Dot) {
            return Err(DirectDnsError::InvalidConfiguration);
        }
        settings
            .validate()
            .map_err(|_| DirectDnsError::InvalidConfiguration)?;
        let lifetime = cancellation.child_token();
        let generation = protector.network_generation().unwrap_or_default();
        quality.set_direct_dns_mode(if mode == ConfigMode::Doh {
            DirectDnsMode::Doh
        } else {
            DirectDnsMode::Dot
        });
        let queue = quality.register_unordered_queue(
            QueueKind::DirectDnsRequests,
            MAX_IN_FLIGHT,
            MAX_IN_FLIGHT * MAX_DNS_MESSAGE,
        );
        let pool = if mode == ConfigMode::Doh {
            ResolverPool::Doh(std::array::from_fn(|_| Mutex::new(None)))
        } else {
            ResolverPool::Dot(Box::new(std::array::from_fn(|_| Mutex::new(None))))
        };
        let inner = Arc::new(EncryptedResolver {
            settings,
            protector,
            quality,
            queue,
            tls: Arc::new(tls),
            pool,
            epoch: StdMutex::new(PoolEpoch {
                generation,
                cancellation: lifetime.child_token(),
            }),
            lifetime,
            query_permits: Arc::new(Semaphore::new(MAX_IN_FLIGHT)),
            dot_permits: Arc::new(Semaphore::new(MAX_CONNECTIONS)),
            socket_permits: Arc::new(Semaphore::new(MAX_CONNECTIONS)),
            changed: Arc::new(Notify::new()),
            monitor: StdMutex::new(None),
        });
        let weak = Arc::downgrade(&inner);
        let stop = inner.lifetime.clone();
        let monitor = tokio::spawn(async move {
            let mut tick = interval(GENERATION_POLL_INTERVAL);
            tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
            loop {
                tokio::select! {
                    _ = stop.cancelled() => {
                        if let Some(inner) = weak.upgrade() { inner.clear_idle_pool(true); }
                        break;
                    },
                    _ = tick.tick() => {
                        let Some(inner) = weak.upgrade() else { break; };
                        inner.sync_generation();
                        inner.clear_idle_pool(false);
                    }
                }
            }
        });
        *inner
            .monitor
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = Some(AbortOnDropHandle::new(monitor));
        Ok(Arc::new(if mode == ConfigMode::Doh {
            Self::Doh(DohResolver { inner })
        } else {
            Self::Dot(DotResolver { inner })
        }))
    }

    pub async fn query(
        &self,
        wire_query: Bytes,
        context: DirectDnsQueryContext,
    ) -> Result<Bytes, DirectDnsError> {
        match self {
            Self::PhysicalSystem(resolver) => {
                let deadline = context.deadline.min(Instant::now() + QUERY_TIMEOUT);
                timeout_at(
                    deadline,
                    physical_wire_query(
                        resolver.protector.as_ref(),
                        &wire_query,
                        context.network_generation,
                    ),
                )
                .await
                .map_err(|_| DirectDnsError::Timeout)?
                .map(Bytes::from)
                .map_err(|_| DirectDnsError::QueryFailed)
            }
            Self::Doh(resolver) => resolver.inner.query(wire_query, context).await,
            Self::Dot(resolver) => resolver.inner.query(wire_query, context).await,
        }
    }

    pub fn is_encrypted(&self) -> bool {
        !matches!(self, Self::PhysicalSystem(_))
    }
}

struct PoolEpoch {
    generation: u64,
    cancellation: CancellationToken,
}

enum ResolverPool {
    Doh([Mutex<Option<Arc<DohConnection>>>; MAX_CONNECTIONS]),
    Dot(Box<[Mutex<Option<DotConnection>>; MAX_CONNECTIONS]>),
}

struct EncryptedResolver {
    settings: DirectDnsSettings,
    protector: Arc<dyn SocketProtector>,
    quality: NetworkQualityTelemetry,
    queue: Arc<QueueMetrics>,
    tls: Arc<ClientConfig>,
    pool: ResolverPool,
    epoch: StdMutex<PoolEpoch>,
    lifetime: CancellationToken,
    query_permits: Arc<Semaphore>,
    dot_permits: Arc<Semaphore>,
    // Held by the actual I/O through driver destruction, including retired
    // connections and Happy Eyeballs losers. Pending cleanup cannot exceed 4.
    socket_permits: Arc<Semaphore>,
    changed: Arc<Notify>,
    monitor: StdMutex<Option<AbortOnDropHandle<()>>>,
}

struct DohConnection {
    endpoint: SocketAddr,
    generation: u64,
    sender: SendRequest<Bytes>,
    permits: Arc<Semaphore>,
    queries: AtomicU32,
    active: AtomicUsize,
    last_used: StdMutex<Instant>,
    closed: Arc<AtomicBool>,
    cancellation: CancellationToken,
    _driver: AbortOnDropHandle<()>,
}

impl Drop for DohConnection {
    fn drop(&mut self) {
        self.cancellation.cancel();
    }
}

struct DotConnection {
    stream: DnsTlsStream,
    endpoint: SocketAddr,
    generation: u64,
    queries: u32,
    last_used: Instant,
}

struct DohReservation {
    connection: Arc<DohConnection>,
    permit: Option<OwnedSemaphorePermit>,
    changed: Arc<Notify>,
}

impl Drop for DohReservation {
    fn drop(&mut self) {
        *self
            .connection
            .last_used
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = Instant::now();
        self.connection.active.fetch_sub(1, Ordering::AcqRel);
        self.permit.take();
        self.changed.notify_waiters();
    }
}

/// Only wraps a slot while it is being changed across an await. Release the
/// mutex before notifying; a canceled construction must wake bounded waiters.
struct ChangingSlot<'a, T> {
    guard: Option<MutexGuard<'a, Option<T>>>,
    changed: Arc<Notify>,
}

impl<T> Deref for ChangingSlot<'_, T> {
    type Target = Option<T>;
    fn deref(&self) -> &Self::Target {
        self.guard.as_ref().expect("live slot guard")
    }
}
impl<T> DerefMut for ChangingSlot<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.guard.as_mut().expect("live slot guard")
    }
}
impl<T> Drop for ChangingSlot<'_, T> {
    fn drop(&mut self) {
        self.guard.take();
        self.changed.notify_waiters();
    }
}

struct QueryFailure {
    error: DirectDnsError,
    endpoint: Option<SocketAddr>,
}

impl From<DirectDnsError> for QueryFailure {
    fn from(error: DirectDnsError) -> Self {
        Self {
            error,
            endpoint: None,
        }
    }
}

struct ClosedGuard(Arc<AtomicBool>, Arc<Notify>);
impl Drop for ClosedGuard {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
        self.1.notify_waiters();
    }
}

struct RequestStream {
    stream: h2::SendStream<Bytes>,
    complete: bool,
}
impl Drop for RequestStream {
    fn drop(&mut self) {
        if !self.complete {
            self.stream.send_reset(h2::Reason::CANCEL);
        }
    }
}

impl EncryptedResolver {
    fn sync_generation(&self) {
        let generation = self.protector.network_generation().unwrap_or_default();
        let mut epoch = self.epoch.lock().unwrap_or_else(|error| error.into_inner());
        if epoch.generation != generation {
            epoch.cancellation.cancel();
            *epoch = PoolEpoch {
                generation,
                cancellation: self.lifetime.child_token(),
            };
            self.changed.notify_waiters();
        }
    }

    fn query_epoch(
        &self,
        context: DirectDnsQueryContext,
    ) -> Result<CancellationToken, DirectDnsError> {
        self.sync_generation();
        self.ensure_current(context)?;
        let epoch = self.epoch.lock().unwrap_or_else(|error| error.into_inner());
        if epoch.generation != context.network_generation {
            return Err(DirectDnsError::NetworkChanged);
        }
        Ok(epoch.cancellation.clone())
    }

    fn ensure_current(&self, context: DirectDnsQueryContext) -> Result<(), DirectDnsError> {
        if self.lifetime.is_cancelled() {
            return Err(DirectDnsError::Cancelled);
        }
        if self.protector.network_generation().unwrap_or_default() != context.network_generation {
            return Err(DirectDnsError::NetworkChanged);
        }
        Ok(())
    }

    fn clear_idle_pool(&self, all: bool) {
        let generation = self.protector.network_generation().unwrap_or_default();
        let now = Instant::now();
        match &self.pool {
            ResolverPool::Doh(slots) => {
                for slot in slots.iter() {
                    if let Ok(mut slot) = slot.try_lock() {
                        let expired = slot.as_ref().is_some_and(|entry| {
                            all || entry.generation != generation
                                || entry.closed.load(Ordering::Acquire)
                                || entry.active.load(Ordering::Acquire) == 0
                                    && (entry.queries.load(Ordering::Acquire)
                                        >= MAX_QUERIES_PER_CONNECTION
                                        || now.duration_since(
                                            *entry
                                                .last_used
                                                .lock()
                                                .unwrap_or_else(|error| error.into_inner()),
                                        ) >= IDLE_TIMEOUT)
                        });
                        if expired {
                            slot.take();
                        }
                    }
                }
            }
            ResolverPool::Dot(slots) => {
                for slot in slots.iter() {
                    if let Ok(mut slot) = slot.try_lock() {
                        let expired = slot.as_ref().is_some_and(|entry| {
                            all || entry.generation != generation
                                || entry.queries >= MAX_QUERIES_PER_CONNECTION
                                || now.duration_since(entry.last_used) >= IDLE_TIMEOUT
                        });
                        if expired {
                            slot.take();
                        }
                    }
                }
            }
        }
    }

    async fn query(
        &self,
        query: Bytes,
        mut context: DirectDnsQueryContext,
    ) -> Result<Bytes, DirectDnsError> {
        context.deadline = context.deadline.min(Instant::now() + QUERY_TIMEOUT);
        let started = Instant::now();
        if query.len() > MAX_DNS_MESSAGE || query.len() < 12 {
            self.quality
                .record_direct_dns_failure(DirectDnsReasonCode::QueryFailed, false);
            return Err(DirectDnsError::InvalidQuery);
        }
        let permit = self.query_permits.clone().try_acquire_owned();
        let Ok(_permit) = permit else {
            self.queue.record_rejected(query.len());
            self.quality
                .record_direct_dns_failure(DirectDnsReasonCode::QueryFailed, false);
            return Err(DirectDnsError::Busy);
        };
        let entry = self.queue.start_entry(query.len());
        let result = async {
            validate_dns_query(&query).map_err(|_| DirectDnsError::InvalidQuery)?;
            let epoch = self.query_epoch(context)?;
            tokio::select! {
                biased;
                _ = epoch.cancelled() => {
                    self.ensure_current(context)?;
                    Err(DirectDnsError::NetworkChanged)
                },
                result = timeout_at(context.deadline, self.query_with_retry(query, context, &epoch)) => {
                    result.map_err(|_| DirectDnsError::Timeout)?
                }
            }
        }.await;
        match &result {
            Ok(_) => self.quality.record_direct_dns_success(started.elapsed()),
            Err(error) => self.quality.record_direct_dns_failure(
                error.quality_reason(),
                *error == DirectDnsError::Timeout,
            ),
        }
        entry.complete();
        result
    }

    async fn query_with_retry(
        &self,
        query: Bytes,
        context: DirectDnsQueryContext,
        epoch: &CancellationToken,
    ) -> Result<Bytes, DirectDnsError> {
        let mut visited = Vec::with_capacity(2);
        let mut excluded = None;
        for attempt in 0..2 {
            self.ensure_current(context)?;
            let result = match self.pool {
                ResolverPool::Doh(_) => {
                    self.query_doh(query.clone(), context, epoch, &mut visited, excluded)
                        .await
                }
                ResolverPool::Dot(_) => {
                    self.query_dot(&query, context, &mut visited, excluded)
                        .await
                }
            };
            self.ensure_current(context)?;
            match result {
                Ok(response) => return Ok(response),
                Err(failure) if attempt == 0 && failure.error.permits_retry() => {
                    let Some(endpoint) = failure.endpoint else {
                        return Err(failure.error);
                    };
                    if self
                        .bootstrap_candidates(&visited, Some(endpoint.ip()))
                        .is_empty()
                    {
                        return Err(failure.error);
                    }
                    excluded = Some(endpoint.ip());
                }
                Err(failure) => return Err(failure.error),
            }
        }
        Err(DirectDnsError::QueryFailed)
    }

    fn bootstrap_candidates(
        &self,
        visited: &[IpAddr],
        excluded: Option<IpAddr>,
    ) -> Vec<SocketAddr> {
        let mut candidates = self
            .settings
            .bootstrap_ips
            .iter()
            .copied()
            .filter(|ip| Some(*ip) != excluded)
            .filter(|ip| visited.len() < 2 || visited.contains(ip))
            .map(|ip| SocketAddr::new(ip, self.settings.port))
            .filter(|endpoint| self.protector.endpoint_family_available(*endpoint) != Some(false))
            .collect::<Vec<_>>();
        candidates.sort_by_key(SocketAddr::is_ipv4);
        if let Some(first) = candidates.first().copied()
            && let Some(alternate) = candidates
                .iter()
                .position(|address| address.is_ipv4() != first.is_ipv4())
        {
            candidates.swap(1, alternate);
        }
        let mut budget = visited.to_vec();
        candidates
            .into_iter()
            .filter(|candidate| {
                if budget.contains(&candidate.ip()) {
                    return true;
                }
                if budget.len() == 2 {
                    return false;
                }
                budget.push(candidate.ip());
                true
            })
            .take(2)
            .collect()
    }

    async fn connect_bootstrap(
        &self,
        context: DirectDnsQueryContext,
        visited: &mut Vec<IpAddr>,
        excluded: Option<IpAddr>,
    ) -> Result<(DnsTlsStream, SocketAddr), DirectDnsError> {
        let candidates = self.bootstrap_candidates(visited, excluded);
        let first = candidates
            .first()
            .copied()
            .ok_or(DirectDnsError::BootstrapUnavailable)?;
        let deadline = context.deadline.min(Instant::now() + CONNECT_TIMEOUT);
        remember_bootstrap(visited, first.ip());
        let operation = async {
            let first_connect = self.connect_one(first, context);
            tokio::pin!(first_connect);
            let first_error = tokio::select! {
                result = &mut first_connect => match result {
                    Ok(stream) => return Ok((stream, first)),
                    Err(error) => Some(error),
                },
                _ = sleep(HAPPY_EYEBALLS_DELAY), if candidates.len() > 1 => None,
            };
            let Some(second) = candidates.get(1).copied() else {
                return Err(first_error.unwrap_or(DirectDnsError::ConnectFailed));
            };
            remember_bootstrap(visited, second.ip());
            if first_error.is_some() {
                return self
                    .connect_one(second, context)
                    .await
                    .map(|stream| (stream, second));
            }
            let second_connect = self.connect_one(second, context);
            tokio::pin!(second_connect);
            tokio::select! {
                result = &mut first_connect => match result {
                    Ok(stream) => Ok((stream, first)),
                    Err(_) => second_connect.await.map(|stream| (stream, second)),
                },
                result = &mut second_connect => match result {
                    Ok(stream) => Ok((stream, second)),
                    Err(_) => first_connect.await.map(|stream| (stream, first)),
                },
            }
        };
        timeout_at(deadline, operation)
            .await
            .map_err(|_| DirectDnsError::Timeout)?
    }

    async fn connect_one(
        &self,
        endpoint: SocketAddr,
        context: DirectDnsQueryContext,
    ) -> Result<DnsTlsStream, DirectDnsError> {
        self.ensure_current(context)?;
        let budget = self
            .socket_permits
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| DirectDnsError::Cancelled)?;
        let socket = if endpoint.is_ipv4() {
            TcpSocket::new_v4()
        } else {
            TcpSocket::new_v6()
        }
        .map_err(|_| DirectDnsError::ConnectFailed)?;
        let lease = self
            .protector
            .protect_for_target_generation(
                socket_handle(&socket),
                endpoint,
                DirectProtocol::Tcp,
                context.network_generation,
            )
            .await
            .map_err(|error| {
                if error == STALE_GENERATION_REASON {
                    DirectDnsError::NetworkChanged
                } else {
                    DirectDnsError::SocketProtectFailed
                }
            })?;
        self.ensure_current(context)?;
        if lease.generation() != Some(context.network_generation) {
            return Err(DirectDnsError::NetworkChanged);
        }
        let stream = socket
            .connect(endpoint)
            .await
            .map_err(|_| DirectDnsError::ConnectFailed)?;
        stream
            .set_nodelay(true)
            .map_err(|_| DirectDnsError::ConnectFailed)?;
        let stream = LeasedIo::new(
            stream,
            DirectEgressLease::hold_for_generation((lease, budget), context.network_generation),
        );
        self.ensure_current(context)?;
        let name = ServerName::try_from(self.settings.server_name.clone())
            .map_err(|_| DirectDnsError::InvalidConfiguration)?;
        let tls = tokio_rustls::TlsConnector::from(Arc::clone(&self.tls))
            .connect(name, stream)
            .await;
        self.ensure_current(context)?;
        let tls = tls.map_err(|_| DirectDnsError::TlsFailed)?;
        if self.settings.mode == ConfigMode::Doh && tls.get_ref().1.alpn_protocol() != Some(b"h2") {
            return Err(DirectDnsError::AlpnMismatch);
        }
        Ok(tls)
    }

    async fn acquire_doh(
        &self,
        context: DirectDnsQueryContext,
        epoch: &CancellationToken,
        visited: &mut Vec<IpAddr>,
        excluded: Option<IpAddr>,
    ) -> Result<DohReservation, DirectDnsError> {
        let ResolverPool::Doh(slots) = &self.pool else {
            return Err(DirectDnsError::Unsupported);
        };
        loop {
            let notified = self.changed.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            self.ensure_current(context)?;
            for slot in slots.iter() {
                let Ok(mut slot) = slot.try_lock() else {
                    continue;
                };
                if let Some(connection) = slot.as_ref() {
                    let endpoint_ok = Some(connection.endpoint.ip()) != excluded
                        && (visited.len() < 2 || visited.contains(&connection.endpoint.ip()));
                    let active = connection.active.load(Ordering::Acquire);
                    let obsolete = connection.generation != context.network_generation
                        || connection.closed.load(Ordering::Acquire)
                        || connection.queries.load(Ordering::Acquire) >= MAX_QUERIES_PER_CONNECTION
                        || Instant::now().duration_since(
                            *connection
                                .last_used
                                .lock()
                                .unwrap_or_else(|error| error.into_inner()),
                        ) >= IDLE_TIMEOUT;
                    if !obsolete
                        && endpoint_ok
                        && let Ok(permit) = connection.permits.clone().try_acquire_owned()
                    {
                        connection.queries.fetch_add(1, Ordering::AcqRel);
                        connection.active.fetch_add(1, Ordering::AcqRel);
                        remember_bootstrap(visited, connection.endpoint.ip());
                        return Ok(DohReservation {
                            connection: Arc::clone(connection),
                            permit: Some(permit),
                            changed: Arc::clone(&self.changed),
                        });
                    }
                    if active != 0 || !obsolete && endpoint_ok {
                        continue;
                    }
                    slot.take();
                }
                let mut slot = ChangingSlot {
                    guard: Some(slot),
                    changed: Arc::clone(&self.changed),
                };
                let (tls, endpoint) = self.connect_bootstrap(context, visited, excluded).await?;
                let deadline = context.deadline.min(Instant::now() + CONNECT_TIMEOUT);
                let (sender, connection) =
                    timeout_at(deadline, dns_h2_builder().handshake::<_, Bytes>(tls))
                        .await
                        .map_err(|_| DirectDnsError::Timeout)?
                        .map_err(|_| DirectDnsError::QueryFailed)?;
                self.ensure_current(context)?;
                let cancellation = epoch.child_token();
                let stop = cancellation.clone();
                let closed = Arc::new(AtomicBool::new(false));
                let closed_guard = ClosedGuard(Arc::clone(&closed), Arc::clone(&self.changed));
                let driver = tokio::spawn(async move {
                    let _closed = closed_guard;
                    tokio::select! { _ = stop.cancelled() => {}, _ = connection => {} }
                });
                *slot = Some(Arc::new(DohConnection {
                    endpoint,
                    generation: context.network_generation,
                    sender,
                    permits: Arc::new(Semaphore::new(H2_REQUESTS_PER_CONNECTION)),
                    queries: AtomicU32::new(0),
                    active: AtomicUsize::new(0),
                    last_used: StdMutex::new(Instant::now()),
                    closed,
                    cancellation,
                    _driver: AbortOnDropHandle::new(driver),
                }));
                self.changed.notify_waiters();
                // Re-enter the short reservation path, so even the first query
                // shares the same per-connection accounting and recycle bound.
                drop(slot);
                break;
            }
            notified.await;
        }
    }

    async fn query_doh(
        &self,
        query: Bytes,
        context: DirectDnsQueryContext,
        epoch: &CancellationToken,
        visited: &mut Vec<IpAddr>,
        excluded: Option<IpAddr>,
    ) -> Result<Bytes, QueryFailure> {
        let reservation = self.acquire_doh(context, epoch, visited, excluded).await?;
        let endpoint = reservation.connection.endpoint;
        let request_deadline = context.deadline.min(Instant::now() + REQUEST_TIMEOUT);
        let request = async {
            self.ensure_current(context)?;
            let authority = if self.settings.port == 443 {
                self.settings.server_name.clone()
            } else {
                format!("{}:{}", self.settings.server_name, self.settings.port)
            };
            let request = Request::builder()
                .method(Method::POST)
                .version(Version::HTTP_2)
                .uri(format!("https://{authority}{}", self.settings.doh_path))
                .header(http::header::CONTENT_TYPE, "application/dns-message")
                .header(http::header::ACCEPT, "application/dns-message")
                .header(http::header::CONTENT_LENGTH, query.len())
                .body(())
                .map_err(|_| DirectDnsError::InvalidConfiguration)?;
            let mut sender = reservation
                .connection
                .sender
                .clone()
                .ready()
                .await
                .map_err(|_| DirectDnsError::QueryFailed)?;
            let (response, stream) = sender
                .send_request(request, false)
                .map_err(|_| DirectDnsError::QueryFailed)?;
            let mut stream = RequestStream {
                stream,
                complete: false,
            };
            stream
                .stream
                .send_data(query.clone(), true)
                .map_err(|_| DirectDnsError::QueryFailed)?;
            let response = response.await.map_err(|_| DirectDnsError::QueryFailed)?;
            if response.status() != StatusCode::OK {
                return Err(DirectDnsError::HttpRejected);
            }
            validate_doh_headers(response.headers())?;
            let mut body = response.into_body();
            let mut bytes = BytesMut::with_capacity(512);
            while let Some(chunk) = body.data().await {
                let chunk = chunk.map_err(|_| DirectDnsError::QueryFailed)?;
                if bytes.len().saturating_add(chunk.len()) > MAX_DNS_MESSAGE {
                    return Err(DirectDnsError::ResponseTooLarge);
                }
                bytes.extend_from_slice(&chunk);
                body.flow_control()
                    .release_capacity(chunk.len())
                    .map_err(|_| DirectDnsError::QueryFailed)?;
            }
            validate_dns_exchange(&query, &bytes).map_err(|_| DirectDnsError::InvalidResponse)?;
            self.ensure_current(context)?;
            stream.complete = true;
            Ok(bytes.freeze())
        };
        timeout_at(request_deadline, request)
            .await
            .map_err(|_| DirectDnsError::Timeout)
            .and_then(|result| result)
            .map_err(|error| QueryFailure {
                error,
                endpoint: Some(endpoint),
            })
    }

    async fn query_dot(
        &self,
        query: &[u8],
        context: DirectDnsQueryContext,
        visited: &mut Vec<IpAddr>,
        excluded: Option<IpAddr>,
    ) -> Result<Bytes, QueryFailure> {
        let _active = self
            .dot_permits
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| DirectDnsError::Cancelled)?;
        let ResolverPool::Dot(slots) = &self.pool else {
            return Err(DirectDnsError::Unsupported.into());
        };
        loop {
            self.ensure_current(context)?;
            for slot in slots.iter() {
                let Ok(slot) = slot.try_lock() else {
                    continue;
                };
                let mut slot = ChangingSlot {
                    guard: Some(slot),
                    changed: Arc::clone(&self.changed),
                };
                let mut connection = match slot.take() {
                    Some(entry)
                        if entry.generation == context.network_generation
                            && Some(entry.endpoint.ip()) != excluded
                            && (visited.len() < 2 || visited.contains(&entry.endpoint.ip()))
                            && entry.queries < MAX_QUERIES_PER_CONNECTION
                            && entry.last_used.elapsed() < IDLE_TIMEOUT =>
                    {
                        entry
                    }
                    _ => {
                        let (stream, endpoint) =
                            self.connect_bootstrap(context, visited, excluded).await?;
                        DotConnection {
                            stream,
                            endpoint,
                            generation: context.network_generation,
                            queries: 0,
                            last_used: Instant::now(),
                        }
                    }
                };
                let endpoint = connection.endpoint;
                remember_bootstrap(visited, endpoint.ip());
                let deadline = context.deadline.min(Instant::now() + REQUEST_TIMEOUT);
                let result = timeout_at(deadline, async {
                    self.ensure_current(context)?;
                    connection
                        .stream
                        .write_u16(query.len() as u16)
                        .await
                        .map_err(|_| DirectDnsError::QueryFailed)?;
                    connection
                        .stream
                        .write_all(query)
                        .await
                        .map_err(|_| DirectDnsError::QueryFailed)?;
                    connection
                        .stream
                        .flush()
                        .await
                        .map_err(|_| DirectDnsError::QueryFailed)?;
                    let length = connection
                        .stream
                        .read_u16()
                        .await
                        .map_err(|_| DirectDnsError::QueryFailed)?;
                    if length == 0 {
                        return Err(DirectDnsError::InvalidResponse);
                    }
                    let mut response = vec![0; usize::from(length)];
                    connection
                        .stream
                        .read_exact(&mut response)
                        .await
                        .map_err(|_| DirectDnsError::QueryFailed)?;
                    validate_dns_exchange(query, &response)
                        .map_err(|_| DirectDnsError::InvalidResponse)?;
                    self.ensure_current(context)?;
                    Ok(Bytes::from(response))
                })
                .await
                .map_err(|_| DirectDnsError::Timeout)
                .and_then(|result| result);
                if result.is_ok() {
                    connection.queries += 1;
                    connection.last_used = Instant::now();
                    if connection.queries < MAX_QUERIES_PER_CONNECTION {
                        *slot = Some(connection);
                    }
                }
                self.changed.notify_waiters();
                return result.map_err(|error| QueryFailure {
                    error,
                    endpoint: Some(endpoint),
                });
            }
            tokio::task::yield_now().await;
        }
    }
}

impl Drop for EncryptedResolver {
    fn drop(&mut self) {
        self.lifetime.cancel();
    }
}

fn remember_bootstrap(visited: &mut Vec<IpAddr>, ip: IpAddr) {
    if !visited.contains(&ip) {
        visited.push(ip);
    }
    debug_assert!(visited.len() <= 2);
}

fn encrypted_tls_config(mode: ConfigMode) -> Result<ClientConfig, DirectDnsError> {
    let mut roots = RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let mut config =
        ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_safe_default_protocol_versions()
            .map_err(|_| DirectDnsError::Unsupported)?
            .with_root_certificates(roots)
            .with_no_client_auth();
    config.alpn_protocols = if mode == ConfigMode::Doh {
        vec![b"h2".to_vec()]
    } else {
        Vec::new()
    };
    config.enable_early_data = false;
    Ok(config)
}

fn dns_h2_builder() -> h2::client::Builder {
    let mut builder = h2::client::Builder::new();
    builder
        .initial_window_size(MAX_DNS_MESSAGE as u32)
        .initial_connection_window_size(256 * 1024)
        .max_header_list_size(MAX_HTTP_HEADERS)
        .enable_push(false);
    builder
}

fn validate_doh_headers(headers: &HeaderMap) -> Result<(), DirectDnsError> {
    let mut content_types = headers.get_all(http::header::CONTENT_TYPE).iter();
    let value = content_types
        .next()
        .ok_or(DirectDnsError::InvalidContentType)?;
    if content_types.next().is_some() || !valid_dns_media_type(value.as_bytes()) {
        return Err(DirectDnsError::InvalidContentType);
    }
    if headers.contains_key(http::header::CONTENT_ENCODING) {
        return Err(DirectDnsError::InvalidResponse);
    }
    for value in headers.get_all(http::header::CONTENT_LENGTH) {
        let length = value
            .to_str()
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .ok_or(DirectDnsError::InvalidResponse)?;
        if length > MAX_DNS_MESSAGE {
            return Err(DirectDnsError::ResponseTooLarge);
        }
    }
    Ok(())
}

fn valid_dns_media_type(value: &[u8]) -> bool {
    if value.len() > 1024 || !value.is_ascii() {
        return false;
    }
    let mut cursor = 0;
    skip_ows(value, &mut cursor);
    let start = cursor;
    while value
        .get(cursor)
        .is_some_and(|byte| *byte != b';' && *byte != b' ' && *byte != b'\t')
    {
        cursor += 1;
    }
    if !value[start..cursor].eq_ignore_ascii_case(b"application/dns-message") {
        return false;
    }
    loop {
        skip_ows(value, &mut cursor);
        if cursor == value.len() {
            return true;
        }
        if value[cursor] != b';' {
            return false;
        }
        cursor += 1;
        skip_ows(value, &mut cursor);
        let name = cursor;
        while value.get(cursor).is_some_and(|byte| is_http_token(*byte)) {
            cursor += 1;
        }
        if cursor == name || value.get(cursor) != Some(&b'=') {
            return false;
        }
        cursor += 1;
        if value.get(cursor) == Some(&b'"') {
            cursor += 1;
            loop {
                match value.get(cursor).copied() {
                    Some(b'"') => {
                        cursor += 1;
                        break;
                    }
                    Some(b'\\') => {
                        cursor += 1;
                        if !value
                            .get(cursor)
                            .is_some_and(|byte| *byte == b'\t' || (32..=126).contains(byte))
                        {
                            return false;
                        }
                        cursor += 1;
                    }
                    Some(byte) if byte == b'\t' || (32..=126).contains(&byte) => cursor += 1,
                    _ => return false,
                }
            }
        } else {
            let start = cursor;
            while value.get(cursor).is_some_and(|byte| is_http_token(*byte)) {
                cursor += 1;
            }
            if start == cursor {
                return false;
            }
        }
    }
}

fn skip_ows(value: &[u8], cursor: &mut usize) {
    while value
        .get(*cursor)
        .is_some_and(|byte| *byte == b' ' || *byte == b'\t')
    {
        *cursor += 1;
    }
}

fn is_http_token(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&byte)
}

/// Wrap only the Geo/direct query policy. MASQUE/control-plane resolution
/// stays on its existing path, and the underlying protector owns all platform
/// authorization. PhysicalSystem returns the original object unchanged.
pub(crate) fn configure_direct_dns(
    settings: &DirectDnsSettings,
    protector: Arc<dyn SocketProtector>,
    quality: NetworkQualityTelemetry,
    cancellation: &CancellationToken,
) -> Result<Arc<dyn SocketProtector>, TransportError> {
    if settings.mode == ConfigMode::PhysicalSystem {
        return Ok(protector);
    }
    let resolver = DirectDnsResolver::new(settings, Arc::clone(&protector), quality, cancellation)
        .map_err(|error| TransportError::Dns(error.to_string()))?;
    Ok(Arc::new(ConfiguredDnsProtector {
        protector,
        resolver,
    }))
}

pub(crate) fn validate_direct_dns_support(
    settings: &DirectDnsSettings,
) -> Result<(), TransportError> {
    let mut settings = settings.clone();
    settings.canonicalize();
    settings
        .validate()
        .map_err(|_| TransportError::Dns(DirectDnsError::InvalidConfiguration.to_string()))?;
    if settings.mode != ConfigMode::PhysicalSystem && !ENCRYPTED_DIRECT_DNS_ENABLED {
        return Err(TransportError::Dns(DirectDnsError::Unsupported.to_string()));
    }
    Ok(())
}

struct ConfiguredDnsProtector {
    protector: Arc<dyn SocketProtector>,
    resolver: Arc<DirectDnsResolver>,
}

#[async_trait]
impl SocketProtector for ConfiguredDnsProtector {
    fn protect(&self, socket: SocketHandle) -> Result<(), String> {
        self.protector.protect(socket)
    }
    async fn protect_for_target(
        &self,
        socket: SocketHandle,
        remote: SocketAddr,
        protocol: DirectProtocol,
    ) -> Result<DirectEgressLease, String> {
        self.protector
            .protect_for_target(socket, remote, protocol)
            .await
    }
    async fn protect_for_target_generation(
        &self,
        socket: SocketHandle,
        remote: SocketAddr,
        protocol: DirectProtocol,
        generation: u64,
    ) -> Result<DirectEgressLease, String> {
        self.protector
            .protect_for_target_generation(socket, remote, protocol, generation)
            .await
    }
    async fn resolve_direct(&self, host: &str, port: u16) -> Result<Vec<SocketAddr>, String> {
        resolve_encrypted_host(self.resolver.as_ref(), self.protector.as_ref(), host, port)
            .await
            .map_err(|error| error.to_string())
    }
    fn direct_dns_resolver(&self) -> Option<Arc<DirectDnsResolver>> {
        Some(Arc::clone(&self.resolver))
    }
    fn tun_direct_available(&self) -> bool {
        self.protector.tun_direct_available()
    }
    fn endpoint_family_available(&self, endpoint: SocketAddr) -> Option<bool> {
        self.protector.endpoint_family_available(endpoint)
    }
    fn network_generation(&self) -> Option<u64> {
        self.protector.network_generation()
    }
    fn physical_dns_servers(&self) -> Vec<SocketAddr> {
        Vec::new()
    }
    fn resolve(&self, host: &str, port: u16) -> Result<Vec<SocketAddr>, String> {
        self.protector.resolve(host, port)
    }
}

#[cfg(test)]
mod tests;
