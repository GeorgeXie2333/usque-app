use std::collections::VecDeque;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use tokio::sync::{Notify, Semaphore, mpsc, oneshot, watch};
use tokio::task::JoinHandle;
use tokio::time::{Instant, interval, sleep_until, timeout_at};
use tokio_util::sync::CancellationToken;
use usque_core::{AddressFamily, IpPolicy, Profile, Transport};

use super::stream::{Flow, FlowState};
use super::{BufferBudget, L4Actor, L4Metrics, Limits, OpenRequest, SessionHandle};
use crate::h2::{MasqueTlsIdentity, TransportError};
use crate::h3::{H3ConnectSettings, H3MigrationResult, H3Tunnel, connect_l4_h3};
use crate::netstack::{RuntimeHealth, RuntimePath, TrafficCounters};
use crate::pin_refresh::EndpointPinRefresher;
use crate::socket::SocketProtector;
use crate::tcp::{DialError, FlowClass, TcpDialer, TcpStream, TcpTarget};
use crate::telemetry::{ConnectionAttemptTelemetry, ConnectionTelemetry};

pub(crate) struct L4Client {
    epoch: Arc<AtomicU64>,
    requests: mpsc::Sender<OpenRequest>,
    pending: Arc<Semaphore>,
    dns_pending: Arc<Semaphore>,
    pub(crate) budget: Arc<BufferBudget>,
    pub(crate) metrics: Arc<L4Metrics>,
    pub(crate) health: watch::Receiver<RuntimeHealth>,
    pub(crate) cancellation: CancellationToken,
    pub(crate) changed: Arc<Notify>,
    task: Mutex<Option<JoinHandle<()>>>,
}

struct CancelRequest {
    token: CancellationToken,
    changed: Arc<Notify>,
    disarmed: bool,
}
impl Drop for CancelRequest {
    fn drop(&mut self) {
        if !self.disarmed {
            self.token.cancel();
            self.changed.notify_one();
        }
    }
}

#[async_trait]
impl TcpDialer for L4Client {
    fn session_generation(&self) -> Option<u64> {
        Some(self.epoch.load(Ordering::Acquire))
    }
    async fn connect(
        &self,
        target: TcpTarget,
        deadline: Instant,
        cancellation: &CancellationToken,
        class: FlowClass,
    ) -> Result<TcpStream, DialError> {
        // A retry here can only repeat an undelivered CONNECT, never application
        // bytes: the caller has not received its stream yet.
        for attempt in 0..2 {
            let permits = if class == FlowClass::Dns {
                &self.dns_pending
            } else {
                &self.pending
            };
            let pending = permits.clone().try_acquire_owned().map_err(|_| {
                self.metrics.update(|m| m.budget_rejections += 1);
                DialError::Budget
            })?;
            let child = cancellation.child_token();
            let mut guard = CancelRequest {
                token: child.clone(),
                changed: self.changed.clone(),
                disarmed: false,
            };
            let (reply, response) = oneshot::channel();
            let request = OpenRequest {
                target: target.clone(),
                deadline,
                cancellation: child,
                reply,
                class,
                _pending: pending,
            };
            let operation = async {
                self.requests
                    .send(request)
                    .await
                    .map_err(|_| DialError::Closed)?;
                response.await.map_err(|_| DialError::Closed)?
            };
            let result = tokio::select! {
                _ = cancellation.cancelled() => Err(DialError::Cancelled),
                _ = self.cancellation.cancelled() => Err(DialError::Closed),
                result = timeout_at(deadline, operation) => result.unwrap_or(Err(DialError::Timeout)),
            };
            match result {
                Ok(stream) => {
                    guard.disarmed = true;
                    return Ok(stream);
                }
                Err(DialError::Closed)
                    if attempt == 0
                        && !self.cancellation.is_cancelled()
                        && Instant::now() < deadline =>
                {
                    continue;
                }
                Err(error) => return Err(error),
            }
        }
        Err(DialError::Closed)
    }
}

struct Session {
    epoch: u64,
    tunnel: H3Tunnel,
    handle: Arc<SessionHandle>,
    family: AddressFamily,
    generation: Option<u64>,
}

impl L4Client {
    #[expect(
        clippy::too_many_arguments,
        reason = "startup captures one identity, protection, telemetry and cancellation scope"
    )]
    pub(crate) async fn start(
        profile: Profile,
        identity: MasqueTlsIdentity,
        protector: Arc<dyn SocketProtector>,
        pin_refresher: Option<Arc<dyn EndpointPinRefresher>>,
        telemetry: ConnectionTelemetry,
        counters: Arc<TrafficCounters>,
        parent: &CancellationToken,
    ) -> Result<Arc<Self>, TransportError> {
        if identity.provider.is_none() {
            return Err(TransportError::InvalidIdentity);
        }
        let limits = Limits::platform();
        let epoch = Arc::new(AtomicU64::new(0));
        let cancellation = parent.child_token();
        let changed = Arc::new(Notify::new());
        let metrics = Arc::new(L4Metrics::default());
        let budget = Arc::new(BufferBudget::new(
            limits.buffers,
            metrics.clone(),
            Arc::new(Notify::new()),
        ));
        let (requests, rx) = mpsc::channel(limits.pending + 80);
        let path = path_for(AddressFamily::Ipv4);
        let initial_failure =
            TransportError::UnderlyingNetworkChanged.failure(Some(Transport::Http3), None);
        let (health_tx, health) = watch::channel(RuntimeHealth::Reconnecting {
            last_path: path,
            attempt: 0,
            reconnect_count: 0,
            reason: "L4 connecting".to_owned(),
            failure: initial_failure,
        });
        let (startup_tx, startup_rx) = oneshot::channel();
        let client = Arc::new(Self {
            epoch: epoch.clone(),
            requests,
            pending: Arc::new(Semaphore::new(limits.pending)),
            dns_pending: Arc::new(Semaphore::new(80)),
            budget,
            metrics,
            health,
            cancellation,
            changed,
            task: Mutex::new(None),
        });
        let supervisor = Supervisor {
            epoch,
            profile,
            identity: Arc::new(identity),
            protector,
            pin_refresher,
            telemetry,
            counters,
            budget: client.budget.clone(),
            metrics: client.metrics.clone(),
            changed: client.changed.clone(),
            cancellation: client.cancellation.clone(),
            health: health_tx,
            requests: rx,
            active: Arc::new(Semaphore::new(limits.active)),
            dns: Arc::new(Semaphore::new(super::DNS_OPERATIONS)),
            slots: Arc::new(Semaphore::new(2)),
            startup: Some(startup_tx),
        };
        *client.task.lock().unwrap_or_else(|e| e.into_inner()) =
            Some(tokio::spawn(supervisor.run()));
        match startup_rx.await {
            Ok(Ok(())) => Ok(client),
            Ok(Err(error)) => {
                client.shutdown().await;
                Err(error)
            }
            Err(_) => {
                client.shutdown().await;
                Err(TransportError::TunnelClosed)
            }
        }
    }

    pub(crate) fn snapshot(&self) -> usque_core::L4Snapshot {
        let mut value = self.metrics.snapshot();
        value.pending_flows =
            (Limits::platform().pending - self.pending.available_permits()) as u32;
        value
    }

    pub(crate) fn cancel(&self) {
        self.cancellation.cancel();
    }

    pub(crate) async fn shutdown(&self) {
        self.cancel();
        let task = self.task.lock().unwrap_or_else(|e| e.into_inner()).take();
        if let Some(task) = task {
            let _ = task.await;
        }
    }
}

impl Drop for L4Client {
    fn drop(&mut self) {
        self.cancellation.cancel();
        if let Some(task) = self.task.lock().unwrap_or_else(|e| e.into_inner()).take() {
            task.abort();
        }
    }
}

struct Supervisor {
    epoch: Arc<AtomicU64>,
    profile: Profile,
    identity: Arc<MasqueTlsIdentity>,
    protector: Arc<dyn SocketProtector>,
    pin_refresher: Option<Arc<dyn EndpointPinRefresher>>,
    telemetry: ConnectionTelemetry,
    counters: Arc<TrafficCounters>,
    budget: Arc<BufferBudget>,
    metrics: Arc<L4Metrics>,
    changed: Arc<Notify>,
    cancellation: CancellationToken,
    health: watch::Sender<RuntimeHealth>,
    requests: mpsc::Receiver<OpenRequest>,
    active: Arc<Semaphore>,
    dns: Arc<Semaphore>,
    slots: Arc<Semaphore>,
    startup: Option<oneshot::Sender<Result<(), TransportError>>>,
}

impl Supervisor {
    async fn run(mut self) {
        let mut sessions: Vec<Session> = Vec::new();
        let mut queue: VecDeque<OpenRequest> = VecDeque::new();
        let mut connecting: Option<
            tokio_util::task::AbortOnDropHandle<Result<Session, TransportError>>,
        > = None;
        let mut migration: Option<tokio_util::task::AbortOnDropHandle<H3MigrationResult>> = None;
        let mut generation = self.protector.network_generation();
        let mut tick = interval(Duration::from_millis(100));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut next_connect = Instant::now();
        let mut retry = 0usize;
        let mut reconnects = 0u32;
        let mut pin_refreshed = false;
        let mut established_at: Option<Instant> = None;
        loop {
            let lost_main = sessions.iter().any(|s| {
                s.handle.closed.load(Ordering::Acquire)
                    && !s.handle.draining.load(Ordering::Acquire)
            });
            sessions.retain(|s| !s.handle.closed.load(Ordering::Acquire));
            if lost_main {
                if established_at
                    .take()
                    .is_some_and(|at| at.elapsed() >= Duration::from_secs(30))
                {
                    retry = 0;
                }
                next_connect = Instant::now() + retry_delay(retry);
                retry = retry.saturating_add(1);
                let last_path = self.health.borrow().path();
                self.health.send_replace(RuntimeHealth::Reconnecting {
                    last_path,
                    attempt: retry as u32,
                    reconnect_count: reconnects,
                    reason: "L4 session reconnecting".to_owned(),
                    failure: super::failure(&TransportError::TunnelClosed),
                });
            }
            let main = sessions
                .iter()
                .position(|s| !s.handle.draining.load(Ordering::Acquire));
            self.metrics.update(|m| {
                m.sessions = sessions.len() as u32;
                m.draining_sessions = sessions
                    .iter()
                    .filter(|s| s.handle.draining.load(Ordering::Acquire))
                    .count() as u32;
                m.connect_verified =
                    main.is_some_and(|i| sessions[i].handle.verified.load(Ordering::Acquire));
            });
            if let Some(index) = main {
                let session = &sessions[index];
                for _ in 0..queue.len() {
                    let Some(request) = queue.pop_front() else {
                        break;
                    };
                    if request.cancellation.is_cancelled() || request.reply.is_closed() {
                        continue;
                    }
                    if Instant::now() >= request.deadline {
                        let _ = request.reply.send(Err(DialError::Timeout));
                        continue;
                    }
                    let semaphore = if request.class == FlowClass::Dns {
                        &self.dns
                    } else {
                        &self.active
                    };
                    let Ok(active) = semaphore.clone().try_acquire_owned() else {
                        queue.push_back(request);
                        continue;
                    };
                    // Reserve relay/DNS staging space before accepting a stream;
                    // Bytes retained by QUIC are charged separately until ACKed.
                    let Some(relay) = self.budget.reserve_admission(2 * Limits::platform().relay)
                    else {
                        self.metrics.update(|m| m.budget_rejections += 1);
                        queue.push_back(request);
                        continue;
                    };
                    let flow = Arc::new(Flow {
                        session_generation: session.epoch,
                        state: Mutex::new(FlowState::default()),
                        target: request.target,
                        deadline: request.deadline,
                        cancellation: request.cancellation,
                        budget: self.budget.clone(),
                        wake: session.handle.wake.clone(),
                        metrics: self.metrics.clone(),
                        counters: self.counters.clone(),
                        reply: Mutex::new(Some(request.reply)),
                        started: Instant::now(),
                        _active: active,
                        _relay: relay,
                        _pending: Mutex::new(Some(request._pending)),
                        delivered: AtomicBool::new(false),
                        stream_id: AtomicU64::new(u64::MAX),
                    });
                    self.metrics.update(|m| m.active_flows += 1);
                    if let Err(flow) = session.handle.enqueue(flow) {
                        flow.fail(DialError::Closed);
                    }
                }
            }
            // This is a bounded cold start or replacement, never a background
            // CONNECT-IP probe. One draining connection leaves one slot.
            if main.is_none()
                && (self.startup.is_some() || !queue.is_empty())
                && connecting.is_none()
                && sessions.len() < 2
                && Instant::now() >= next_connect
            {
                let setup = Setup {
                    profile: self.profile.clone(),
                    identity: self.identity.clone(),
                    protector: self.protector.clone(),
                    telemetry: self.telemetry.clone(),
                    budget: self.budget.clone(),
                    metrics: self.metrics.clone(),
                    changed: self.changed.clone(),
                    slots: self.slots.clone(),
                    race: sessions.is_empty(),
                };
                connecting = Some(tokio_util::task::AbortOnDropHandle::new(tokio::spawn(
                    setup.connect(),
                )));
            }
            let observed = self.protector.network_generation();
            if observed != generation {
                generation = observed;
                migration.take();
                if let Some(index) = main {
                    let handle = sessions[index].tunnel.migration_handle();
                    if let Some(target) = observed
                        && handle.enabled()
                        && self.protector.endpoint_family_available(handle.endpoint())
                            != Some(false)
                    {
                        migration = Some(tokio_util::task::AbortOnDropHandle::new(tokio::spawn(
                            handle.start_migration(target),
                        )));
                    } else {
                        sessions.clear();
                        next_connect = Instant::now();
                    }
                }
            }
            tokio::select! {
                _ = self.cancellation.cancelled() => break,
                request = self.requests.recv() => match request { Some(request) => queue.push_back(request), None => break },
                result = wait_connect(&mut connecting), if connecting.is_some() => {
                    connecting.take();
                    match result {
                        Ok(session) if session.generation != self.protector.network_generation() => {
                            drop(session);
                            next_connect = Instant::now();
                        }
                        Ok(mut session) => {
                            session.epoch = self.epoch.fetch_add(1, Ordering::AcqRel).wrapping_add(1);
                            session.tunnel.activate_network_quality();
                            let path = path_for(session.family);
                            sessions.push(session);
                            if self.startup.is_none() { reconnects = reconnects.saturating_add(1); }
                            self.health.send_replace(RuntimeHealth::Connected { path, reconnect_count: reconnects });
                            if let Some(startup) = self.startup.take() { let _ = startup.send(Ok(())); }
                            established_at = Some(Instant::now());
                        }
                        Err(TransportError::EndpointPinMismatch) if !pin_refreshed && self.pin_refresher.is_some() => {
                            pin_refreshed = true;
                            let result = tokio::select! {
                                _ = self.cancellation.cancelled() => break,
                                result = self.pin_refresher.as_ref().expect("checked refresher").refresh(self.protector.clone()) => result,
                            };
                            match result {
                                Ok(identity) if identity.provider == self.identity.provider && identity.assigned_ipv4 == self.identity.assigned_ipv4 && identity.assigned_ipv6 == self.identity.assigned_ipv6 => {
                                    self.identity = Arc::new(identity); next_connect = Instant::now();
                                }
                                Ok(_) => { self.fail(TransportError::InvalidIdentity, reconnects); break; }
                                Err(error) => { self.fail(error, reconnects); break; }
                            }
                        }
                        Err(error) => {
                            let failure = super::failure(&error);
                            if self.startup.is_some() || !failure.retryable || matches!(error, TransportError::EndpointPinMismatch | TransportError::InvalidIdentity | TransportError::SocketProtection(_)) {
                                self.fail(error, reconnects); break;
                            }
                            next_connect = Instant::now() + retry_delay(retry);
                            retry = retry.saturating_add(1);
                            let last_path = self.health.borrow().path();
                            self.health.send_replace(RuntimeHealth::Reconnecting { last_path, attempt: retry as u32, reconnect_count: reconnects, reason: "L4 session reconnecting".to_owned(), failure });
                        }
                    }
                }
                result = wait_migration(&mut migration), if migration.is_some() => {
                    migration.take();
                    match result {
                        H3MigrationResult::Promoted { .. } => self.metrics.update(|m| m.migration_preserved_flows += u64::from(m.active_flows)),
                        _ => { sessions.clear(); next_connect = Instant::now(); }
                    }
                }
                _ = self.changed.notified() => {
                    for session in &sessions { session.handle.wake.notify_one(); }
                },
                _ = self.budget.wake.notified() => {},
                _ = tick.tick() => {
                    queue.retain(|r| !r.reply.is_closed() && !r.cancellation.is_cancelled());
                },
                _ = sleep_until(next_connect), if main.is_none() && connecting.is_none() && next_connect > Instant::now() => {},
            }
        }
        self.cancellation.cancel();
        drop(connecting);
        drop(migration);
        drop(sessions);
        for request in queue {
            let _ = request.reply.send(Err(DialError::Closed));
        }
        self.metrics.update(|m| {
            m.sessions = 0;
            m.draining_sessions = 0;
            m.connect_verified = false;
        });
    }

    fn fail(&mut self, error: TransportError, reconnects: u32) {
        let failure = super::failure(&error);
        let last_path = self.health.borrow().path();
        self.health.send_replace(RuntimeHealth::Failed {
            last_path,
            reconnect_count: reconnects,
            message: "L4 session unavailable".to_owned(),
            failure,
        });
        if let Some(startup) = self.startup.take() {
            let _ = startup.send(Err(error));
        }
    }
}

struct Setup {
    profile: Profile,
    identity: Arc<MasqueTlsIdentity>,
    protector: Arc<dyn SocketProtector>,
    telemetry: ConnectionTelemetry,
    budget: Arc<BufferBudget>,
    metrics: Arc<L4Metrics>,
    changed: Arc<Notify>,
    slots: Arc<Semaphore>,
    race: bool,
}

impl Setup {
    async fn connect(self) -> Result<Session, TransportError> {
        let v4 = self.profile.endpoint.ipv4_socket();
        let v6 = self.profile.endpoint.ipv6_socket();
        let (first, second) = match self.profile.ip_policy {
            IpPolicy::Ipv4Only => (v4, None),
            IpPolicy::Ipv6Only => (v6, None),
            IpPolicy::PreferIpv4 => (v4, Some(v6)),
            _ => (v6, Some(v4)),
        };
        let first_attempt = self.candidate(first);
        tokio::pin!(first_attempt);
        let Some(second) = second else {
            return first_attempt.await;
        };
        if !self.race {
            return match first_attempt.await {
                Ok(s) => Ok(s),
                Err(error) if terminal(&error) => Err(error),
                Err(_) => self.candidate(second).await,
            };
        }
        tokio::select! {
            result = &mut first_attempt => match result { Ok(s) => Ok(s), Err(error) if terminal(&error) => Err(error), Err(_) => self.candidate(second).await },
            _ = tokio::time::sleep(Duration::from_millis(250)) => {
                let second_attempt = self.candidate(second); tokio::pin!(second_attempt);
                tokio::select! {
                    result = &mut first_attempt => match result { Ok(s) => Ok(s), Err(error) if terminal(&error) => Err(error), Err(_) => second_attempt.await },
                    result = &mut second_attempt => match result { Ok(s) => Ok(s), Err(error) if terminal(&error) => Err(error), Err(_) => first_attempt.await },
                }
            }
        }
    }

    async fn candidate(&self, endpoint: SocketAddr) -> Result<Session, TransportError> {
        let generation = self.protector.network_generation();
        let family = if endpoint.is_ipv4() {
            AddressFamily::Ipv4
        } else {
            AddressFamily::Ipv6
        };
        if self.protector.endpoint_family_available(endpoint) == Some(false) {
            return Err(TransportError::EndpointFamilyUnavailable(family));
        }
        let slot = self
            .slots
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| TransportError::TunnelClosed)?;
        let mut actor = L4Actor::new(
            self.budget.clone(),
            self.metrics.clone(),
            self.changed.clone(),
        );
        actor.session_slot = Some(Arc::new(slot));
        let handle = actor.handle.clone();
        let attempt =
            ConnectionAttemptTelemetry::new(self.telemetry.clone(), Transport::Http3, family);
        let tunnel = connect_l4_h3(
            endpoint,
            &self.identity,
            H3ConnectSettings {
                inner_mtu: usize::from(self.profile.mtu),
                congestion_control: self.profile.congestion_control,
            },
            self.protector.clone(),
            Some(&attempt),
            actor,
        )
        .await?;
        Ok(Session {
            epoch: 0,
            tunnel,
            handle,
            family,
            generation,
        })
    }
}

fn terminal(error: &TransportError) -> bool {
    matches!(
        error,
        TransportError::EndpointPinMismatch
            | TransportError::InvalidIdentity
            | TransportError::SocketProtection(_)
    )
}

fn retry_delay(retry: usize) -> Duration {
    const BACKOFF: [u64; 6] = [1, 2, 4, 8, 15, 30];
    let mut random = [0u8; 1];
    let _ = boring::rand::rand_bytes(&mut random);
    Duration::from_millis(BACKOFF[retry.min(5)] * (900 + u64::from(random[0]) * 200 / 255))
}

fn path_for(family: AddressFamily) -> RuntimePath {
    RuntimePath {
        transport: Transport::Http3,
        endpoint_family: family,
        ipv4_available: true,
        ipv6_available: true,
    }
}

async fn wait_connect(
    task: &mut Option<tokio_util::task::AbortOnDropHandle<Result<Session, TransportError>>>,
) -> Result<Session, TransportError> {
    match task {
        Some(task) => task.await.map_err(|_| TransportError::TunnelClosed)?,
        None => std::future::pending().await,
    }
}
async fn wait_migration(
    task: &mut Option<tokio_util::task::AbortOnDropHandle<H3MigrationResult>>,
) -> H3MigrationResult {
    match task {
        Some(task) => task.await.unwrap_or(H3MigrationResult::Failed(
            crate::MigrationReasonCode::Unsupported,
        )),
        None => std::future::pending().await,
    }
}
