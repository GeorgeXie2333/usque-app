use std::{
    collections::{HashMap, VecDeque},
    future::Future,
    io, mem,
    os::windows::io::AsRawHandle,
    ptr,
    sync::{
        Arc,
        atomic::{AtomicU64, AtomicUsize, Ordering},
    },
    time::Duration,
};

use bytes::BytesMut;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::windows::named_pipe::{NamedPipeServer, ServerOptions},
    sync::{Mutex, Notify},
};
use tracing::{info, warn};
use usque_ipc::{
    agent_v1::{
        self, AgentCapabilities, AgentRequest, AgentResponse, AgentState, agent_request,
        agent_response,
    },
    decode_frame, encode_frame, split_frame,
};
use uuid::Uuid;
use windows_sys::Win32::{
    Foundation::{HANDLE, LocalFree},
    Security::{
        Authorization::{ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1},
        PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES,
    },
};

use crate::{
    AGENT_PROTOCOL_VERSION, AuthenticatedCaller,
    coordinator::{
        AgentCoordinator, BackendError, CoordinatorError, ORPHANED_TUNNEL_RECOVERY_GRACE,
        PrivilegedBackend, SystemProxySettings,
    },
    journal::{RecoveryJournal, RecoveryPhase},
    plan::ValidatedTunnelPlan,
    windows::{
        auth::{AuthenticationError, CallerPolicy, authenticate_named_pipe},
        service_config::{
            NoopServiceStartModeController, ServiceConfigError, ServiceStartMode,
            ServiceStartModeController, desired_start_mode,
        },
    },
};

pub const AGENT_PIPE_NAME: &str = r"\\.\pipe\io.github.georgexie2333.usque.agent.v1";
const MAX_AGENT_FRAME_BYTES: usize = 64 * 1024;
const READ_CHUNK_BYTES: usize = 16 * 1024;
const MAX_REQUEST_ID_BYTES: usize = 128;
const MAX_REPLAY_ENTRIES: usize = 256;
pub const AGENT_IDLE_TIMEOUT: Duration = Duration::from_secs(10);
const DEMAND_RETRY_DELAYS: [Duration; 3] = [
    Duration::from_millis(100),
    Duration::from_millis(500),
    Duration::from_secs(2),
];

pub struct AgentService<Backend> {
    coordinator: Arc<AgentCoordinator<Backend>>,
    capabilities: AgentCapabilities,
    replay: Mutex<ReplayCache>,
    start_mode: Arc<dyn ServiceStartModeController>,
    mutation_gate: Mutex<()>,
    activity: Arc<ActivityTracker>,
}

#[derive(Debug, Clone, Copy)]
enum MutationPolicy {
    Forward,
    Cleanup,
}

#[derive(Default)]
struct ActivityTracker {
    connections: AtomicUsize,
    background: AtomicUsize,
    generation: AtomicU64,
    notify: Notify,
}

impl ActivityTracker {
    fn begin(self: &Arc<Self>, kind: ActivityKind) -> ActivityGuard {
        match kind {
            ActivityKind::Connection => self.connections.fetch_add(1, Ordering::AcqRel),
            ActivityKind::Background => self.background.fetch_add(1, Ordering::AcqRel),
        };
        self.changed();
        ActivityGuard {
            tracker: Arc::clone(self),
            kind,
        }
    }

    fn is_empty(&self) -> bool {
        self.connections.load(Ordering::Acquire) == 0
            && self.background.load(Ordering::Acquire) == 0
    }

    fn changed(&self) {
        self.generation.fetch_add(1, Ordering::AcqRel);
        self.notify.notify_waiters();
    }
}

#[derive(Clone, Copy)]
enum ActivityKind {
    Connection,
    Background,
}

struct ActivityGuard {
    tracker: Arc<ActivityTracker>,
    kind: ActivityKind,
}

impl Drop for ActivityGuard {
    fn drop(&mut self) {
        match self.kind {
            ActivityKind::Connection => self.tracker.connections.fetch_sub(1, Ordering::AcqRel),
            ActivityKind::Background => self.tracker.background.fetch_sub(1, Ordering::AcqRel),
        };
        self.tracker.changed();
    }
}

#[derive(Debug, thiserror::Error)]
pub enum AgentLifecycleError {
    #[error("{0}")]
    Coordinator(#[from] CoordinatorError),
    #[error("the Agent could not arm crash recovery before changing Windows state: {0}")]
    StartMode(#[from] ServiceConfigError),
}

impl<Backend> AgentService<Backend>
where
    Backend: PrivilegedBackend,
{
    pub fn new(
        coordinator: Arc<AgentCoordinator<Backend>>,
        capabilities: AgentCapabilities,
    ) -> Self {
        Self::with_start_mode_controller(
            coordinator,
            capabilities,
            Arc::new(NoopServiceStartModeController),
        )
    }

    pub fn with_start_mode_controller(
        coordinator: Arc<AgentCoordinator<Backend>>,
        capabilities: AgentCapabilities,
        start_mode: Arc<dyn ServiceStartModeController>,
    ) -> Self {
        Self {
            coordinator,
            capabilities,
            replay: Mutex::new(ReplayCache::default()),
            start_mode,
            mutation_gate: Mutex::new(()),
            activity: Arc::new(ActivityTracker::default()),
        }
    }

    pub async fn state(&self) -> RecoveryJournal {
        self.coordinator.state().await
    }

    pub async fn reconcile_removed_adapter_dependencies(
        &self,
    ) -> Result<bool, AgentLifecycleError> {
        self.mutate(MutationPolicy::Cleanup, |coordinator| async move {
            coordinator.reconcile_removed_adapter_dependencies().await
        })
        .await
    }

    pub async fn recover_stale(&self) -> Result<(), AgentLifecycleError> {
        self.mutate(MutationPolicy::Cleanup, |coordinator| async move {
            coordinator.recover_stale().await
        })
        .await
    }

    pub async fn recover_orphaned_tunnel(
        &self,
        operation_id: Uuid,
        lease_epoch: u64,
    ) -> Result<bool, AgentLifecycleError> {
        let _activity = self.activity.begin(ActivityKind::Background);
        self.mutate(MutationPolicy::Cleanup, move |coordinator| async move {
            coordinator
                .recover_orphaned_tunnel(operation_id, lease_epoch)
                .await
        })
        .await
    }

    pub async fn synchronize_start_mode(&self) {
        let _gate = self.mutation_gate.lock().await;
        self.reconcile_start_mode_locked().await;
    }

    async fn mutate<T, Action, ActionFuture>(
        &self,
        policy: MutationPolicy,
        action: Action,
    ) -> Result<T, AgentLifecycleError>
    where
        T: Send,
        Action: FnOnce(Arc<AgentCoordinator<Backend>>) -> ActionFuture + Send,
        ActionFuture: Future<Output = Result<T, CoordinatorError>> + Send,
    {
        let _gate = self.mutation_gate.lock().await;
        match policy {
            MutationPolicy::Forward => {
                if let Err(error) = self
                    .start_mode
                    .ensure_start_mode(ServiceStartMode::Auto)
                    .await
                {
                    self.reconcile_start_mode_locked().await;
                    return Err(AgentLifecycleError::StartMode(error));
                }
            }
            MutationPolicy::Cleanup => {
                let state = self.coordinator.state().await;
                if state.phase != RecoveryPhase::Clean
                    && let Err(error) = self
                        .start_mode
                        .ensure_start_mode(ServiceStartMode::Auto)
                        .await
                {
                    warn!(%error, phase = ?state.phase, "could not arm automatic startup before cleanup; continuing safety recovery");
                }
            }
        }
        let result = action(Arc::clone(&self.coordinator))
            .await
            .map_err(AgentLifecycleError::Coordinator);
        self.reconcile_start_mode_locked().await;
        result
    }

    async fn reconcile_start_mode_locked(&self) {
        let state = self.coordinator.state().await;
        let desired = desired_start_mode(state.phase);
        let mut error = match self.start_mode.ensure_start_mode(desired).await {
            Ok(()) => return,
            Err(error) => error,
        };
        if desired == ServiceStartMode::Demand {
            for delay in DEMAND_RETRY_DELAYS {
                tokio::time::sleep(delay).await;
                match self.start_mode.ensure_start_mode(desired).await {
                    Ok(()) => {
                        info!(phase = ?state.phase, "restored demand-start Agent configuration after retry");
                        return;
                    }
                    Err(next) => error = next,
                }
            }
        }
        warn!(%error, phase = ?state.phase, ?desired, "could not reconcile Agent service start type with the recovery journal");
    }

    fn connection_started(&self) -> ActivityGuard {
        self.activity.begin(ActivityKind::Connection)
    }

    async fn handle(&self, request: AgentRequest, caller: &AuthenticatedCaller) -> AgentResponse {
        if let Err(error) = validate_request_envelope(&request) {
            return error_response(request.request_id, error);
        }
        let replay_key = ReplayKey {
            sid: caller.user_sid.clone(),
            process_id: caller.process_id,
            request_id: request.request_id.clone(),
        };
        {
            let replay = self.replay.lock().await;
            if let Some(cached) = replay.entries.get(&replay_key) {
                return if cached.request == request {
                    cached.response.clone()
                } else {
                    error_response(request.request_id, ServiceError::RequestIdReused)
                };
            }
        }

        let request_for_cache = request.clone();
        let response = match self.dispatch(request, caller).await {
            Ok(response) => response,
            Err((request_id, error)) => error_response(request_id, error),
        };
        self.replay.lock().await.insert(
            replay_key,
            CachedResponse {
                request: request_for_cache,
                response: response.clone(),
            },
        );
        response
    }

    async fn dispatch(
        &self,
        request: AgentRequest,
        caller: &AuthenticatedCaller,
    ) -> Result<AgentResponse, (String, ServiceError)> {
        let request_id = request.request_id;
        let payload = request
            .payload
            .ok_or_else(|| (request_id.clone(), ServiceError::MissingPayload))?;
        let payload = match payload {
            agent_request::Payload::GetCapabilities(_) => {
                agent_response::Payload::Capabilities(self.capabilities.clone())
            }
            agent_request::Payload::GetState(_) => agent_response::Payload::State(state_to_proto(
                &self.state().await,
                self.coordinator.packet_session_attached(),
            )),
            agent_request::Payload::PrepareTunnel(request) => {
                let operation_id = parse_operation_id(&request.operation_id)
                    .map_err(|error| (request_id.clone(), error))?;
                let plan = request
                    .plan
                    .ok_or_else(|| (request_id.clone(), ServiceError::MissingTunnelPlan))?;
                let plan = ValidatedTunnelPlan::try_from(plan)
                    .map_err(|error| (request_id.clone(), ServiceError::Plan(error.to_string())))?;
                let caller = caller.clone();
                let state = self
                    .mutate(MutationPolicy::Forward, move |coordinator| async move {
                        coordinator.prepare(operation_id, plan, caller).await
                    })
                    .await
                    .map_err(|error| (request_id.clone(), ServiceError::Lifecycle(error)))?;
                agent_response::Payload::State(state_to_proto(
                    &state,
                    self.coordinator.packet_session_attached(),
                ))
            }
            agent_request::Payload::CommitTunnel(request) => {
                let operation_id = parse_operation_id(&request.operation_id)
                    .map_err(|error| (request_id.clone(), error))?;
                let caller = caller.clone();
                let state = self
                    .mutate(MutationPolicy::Forward, move |coordinator| async move {
                        coordinator.commit(operation_id, &caller).await
                    })
                    .await
                    .map_err(|error| (request_id.clone(), ServiceError::Lifecycle(error)))?;
                agent_response::Payload::State(state_to_proto(
                    &state,
                    self.coordinator.packet_session_attached(),
                ))
            }
            agent_request::Payload::RollbackTunnel(request) => {
                validate_reason_code(&request.reason_code)
                    .map_err(|error| (request_id.clone(), error))?;
                let operation_id = parse_operation_id(&request.operation_id)
                    .map_err(|error| (request_id.clone(), error))?;
                let caller = caller.clone();
                let state = self
                    .mutate(MutationPolicy::Cleanup, move |coordinator| async move {
                        coordinator.rollback(operation_id, &caller).await
                    })
                    .await
                    .map_err(|error| (request_id.clone(), ServiceError::Lifecycle(error)))?;
                agent_response::Payload::State(state_to_proto(
                    &state,
                    self.coordinator.packet_session_attached(),
                ))
            }
            agent_request::Payload::Recover(_) => {
                self.recover_stale()
                    .await
                    .map_err(|error| (request_id.clone(), ServiceError::Lifecycle(error)))?;
                agent_response::Payload::State(state_to_proto(
                    &self.state().await,
                    self.coordinator.packet_session_attached(),
                ))
            }
            agent_request::Payload::OpenPacketSession(request) => {
                let operation_id = parse_operation_id(&request.operation_id)
                    .map_err(|error| (request_id.clone(), error))?;
                let capacity = request.ring_capacity;
                let caller = caller.clone();
                let handles = self
                    .mutate(MutationPolicy::Forward, move |coordinator| async move {
                        coordinator
                            .open_packet_session(operation_id, capacity, &caller)
                            .await
                    })
                    .await
                    .map_err(|error| (request_id.clone(), ServiceError::Lifecycle(error)))?;
                agent_response::Payload::PacketSession(agent_v1::PacketSessionHandles {
                    mapping_handle: handles.mapping_handle,
                    engine_to_agent_event_handle: handles.engine_to_agent_event_handle,
                    agent_to_engine_event_handle: handles.agent_to_engine_event_handle,
                    shutdown_event_handle: handles.shutdown_event_handle,
                    ring_capacity: handles.ring_capacity,
                    layout_version: handles.layout_version,
                })
            }
            agent_request::Payload::ClosePacketSession(request) => {
                let operation_id = parse_operation_id(&request.operation_id)
                    .map_err(|error| (request_id.clone(), error))?;
                let caller = caller.clone();
                let state = self
                    .mutate(MutationPolicy::Cleanup, move |coordinator| async move {
                        coordinator
                            .close_packet_session(operation_id, &caller)
                            .await
                    })
                    .await
                    .map_err(|error| (request_id.clone(), ServiceError::Lifecycle(error)))?;
                agent_response::Payload::State(state_to_proto(
                    &state,
                    self.coordinator.packet_session_attached(),
                ))
            }
            agent_request::Payload::ResumeTunnel(request) => {
                let operation_id = parse_operation_id(&request.operation_id)
                    .map_err(|error| (request_id.clone(), error))?;
                let profile_id = parse_profile_id(&request.profile_id)
                    .map_err(|error| (request_id.clone(), error))?;
                let caller = caller.clone();
                let handles = self
                    .mutate(MutationPolicy::Forward, move |coordinator| async move {
                        coordinator
                            .resume_tunnel(operation_id, profile_id, &caller)
                            .await
                    })
                    .await
                    .map_err(|error| (request_id.clone(), ServiceError::Lifecycle(error)))?;
                agent_response::Payload::PacketSession(agent_v1::PacketSessionHandles {
                    mapping_handle: handles.mapping_handle,
                    engine_to_agent_event_handle: handles.engine_to_agent_event_handle,
                    agent_to_engine_event_handle: handles.agent_to_engine_event_handle,
                    shutdown_event_handle: handles.shutdown_event_handle,
                    ring_capacity: handles.ring_capacity,
                    layout_version: handles.layout_version,
                })
            }
            agent_request::Payload::AcquireTunnelLease(request) => {
                let operation_id = parse_operation_id(&request.operation_id)
                    .map_err(|error| (request_id.clone(), error))?;
                let caller = caller.clone();
                let state = self
                    .mutate(MutationPolicy::Forward, move |coordinator| async move {
                        coordinator
                            .acquire_tunnel_lease(operation_id, &caller)
                            .await
                    })
                    .await
                    .map_err(|error| (request_id.clone(), ServiceError::Lifecycle(error)))?;
                agent_response::Payload::State(state_to_proto(
                    &state,
                    self.coordinator.packet_session_attached(),
                ))
            }
            agent_request::Payload::ApplySystemProxy(request) => {
                let operation_id = parse_operation_id(&request.operation_id)
                    .map_err(|error| (request_id.clone(), error))?;
                let settings = SystemProxySettings {
                    proxy_uri: request.proxy_uri,
                    bypass_hosts: request.bypass_hosts,
                };
                let caller = caller.clone();
                let state = self
                    .mutate(MutationPolicy::Forward, move |coordinator| async move {
                        coordinator
                            .apply_system_proxy(operation_id, settings, caller)
                            .await
                    })
                    .await
                    .map_err(|error| (request_id.clone(), ServiceError::Lifecycle(error)))?;
                agent_response::Payload::State(state_to_proto(
                    &state,
                    self.coordinator.packet_session_attached(),
                ))
            }
            agent_request::Payload::RestoreSystemProxy(request) => {
                let operation_id = parse_operation_id(&request.operation_id)
                    .map_err(|error| (request_id.clone(), error))?;
                let caller = caller.clone();
                let state = self
                    .mutate(MutationPolicy::Cleanup, move |coordinator| async move {
                        coordinator
                            .restore_system_proxy(operation_id, &caller)
                            .await
                    })
                    .await
                    .map_err(|error| (request_id.clone(), ServiceError::Lifecycle(error)))?;
                agent_response::Payload::State(state_to_proto(
                    &state,
                    self.coordinator.packet_session_attached(),
                ))
            }
        };
        Ok(AgentResponse {
            request_id,
            error: None,
            payload: Some(payload),
        })
    }

    async fn release_system_proxy_lease(&self, operation_id: Uuid, caller: &AuthenticatedCaller) {
        let caller = caller.clone();
        if let Err(error) = self
            .mutate(MutationPolicy::Cleanup, move |coordinator| async move {
                coordinator
                    .restore_system_proxy(operation_id, &caller)
                    .await
            })
            .await
        {
            warn!(
                %operation_id,
                %error,
                "failed to restore system proxy after Engine lease disconnected"
            );
        }
    }

    async fn release_startup_tunnel_lease(&self, operation_id: Uuid, caller: &AuthenticatedCaller) {
        let caller = caller.clone();
        let lease_epoch = match self
            .mutate(MutationPolicy::Cleanup, move |coordinator| async move {
                coordinator
                    .release_startup_tunnel_lease(operation_id, &caller)
                    .await
            })
            .await
        {
            Ok(Some(lease_epoch)) => lease_epoch,
            Ok(None) => return,
            Err(error) => {
                warn!(%operation_id, %error, "failed to release the Engine startup lease");
                return;
            }
        };
        tokio::time::sleep(ORPHANED_TUNNEL_RECOVERY_GRACE).await;
        match self
            .recover_orphaned_startup_tunnel(operation_id, lease_epoch)
            .await
        {
            Ok(true) => warn!(
                %operation_id,
                grace_seconds = ORPHANED_TUNNEL_RECOVERY_GRACE.as_secs(),
                "recovered an incomplete tunnel whose Engine startup lease disappeared"
            ),
            Ok(false) => {}
            Err(error) => warn!(
                %operation_id,
                %error,
                "failed to recover an incomplete tunnel after its startup lease disappeared"
            ),
        }
    }

    async fn recover_orphaned_startup_tunnel(
        &self,
        operation_id: Uuid,
        lease_epoch: u64,
    ) -> Result<bool, AgentLifecycleError> {
        let _activity = self.activity.begin(ActivityKind::Background);
        self.mutate(MutationPolicy::Cleanup, move |coordinator| async move {
            coordinator
                .recover_orphaned_startup_tunnel(operation_id, lease_epoch)
                .await
        })
        .await
    }

    async fn release_tunnel_lease(&self, operation_id: Uuid, caller: &AuthenticatedCaller) {
        let caller = caller.clone();
        let lease_epoch = match self
            .mutate(MutationPolicy::Cleanup, move |coordinator| async move {
                coordinator
                    .release_tunnel_lease(operation_id, &caller)
                    .await
            })
            .await
        {
            Ok(Some(lease_epoch)) => lease_epoch,
            Ok(None) => return,
            Err(error) => {
                warn!(
                    %operation_id,
                    %error,
                    "failed to detach packet session after Engine tunnel lease disconnected"
                );
                if let Err(recovery_error) = self.recover_stale().await {
                    warn!(
                        %operation_id,
                        %recovery_error,
                        "emergency recovery after tunnel lease detach failure also failed"
                    );
                }
                return;
            }
        };
        tokio::time::sleep(ORPHANED_TUNNEL_RECOVERY_GRACE).await;
        match self
            .recover_orphaned_tunnel(operation_id, lease_epoch)
            .await
        {
            Ok(true) => warn!(
                %operation_id,
                grace_seconds = ORPHANED_TUNNEL_RECOVERY_GRACE.as_secs(),
                "recovered an active tunnel whose Engine lease was not reattached"
            ),
            Ok(false) => {}
            Err(error) => warn!(
                %operation_id,
                %error,
                "failed to recover an orphaned active tunnel after the reattach grace period"
            ),
        }
    }
}

pub async fn serve<Backend>(
    service: Arc<AgentService<Backend>>,
    policy: Arc<CallerPolicy>,
    pipe_name: String,
) -> Result<ServeExit, ServerError>
where
    Backend: PrivilegedBackend + 'static,
{
    serve_until(service, policy, pipe_name, std::future::pending()).await
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServeExit {
    Shutdown,
    Idle,
}

/// Verifies that the fixed Agent pipe name, security descriptor, and first
/// server instance can be created, then immediately releases the handle.
/// This is safe for installer diagnostics because it accepts no client and
/// performs no privileged network operation.
pub fn validate_pipe_creation(pipe_name: &str) -> Result<(), ServerError> {
    validate_pipe_name(pipe_name)?;
    drop(create_agent_pipe(pipe_name, true)?);
    Ok(())
}

pub async fn serve_until<Backend, Shutdown>(
    service: Arc<AgentService<Backend>>,
    policy: Arc<CallerPolicy>,
    pipe_name: String,
    shutdown: Shutdown,
) -> Result<ServeExit, ServerError>
where
    Backend: PrivilegedBackend + 'static,
    Shutdown: Future<Output = ()>,
{
    serve_until_ready(service, policy, pipe_name, shutdown, || Ok(())).await
}

pub async fn serve_until_ready<Backend, Shutdown, Ready>(
    service: Arc<AgentService<Backend>>,
    policy: Arc<CallerPolicy>,
    pipe_name: String,
    shutdown: Shutdown,
    ready: Ready,
) -> Result<ServeExit, ServerError>
where
    Backend: PrivilegedBackend + 'static,
    Shutdown: Future<Output = ()>,
    Ready: FnOnce() -> io::Result<()>,
{
    validate_pipe_name(&pipe_name)?;
    tokio::pin!(shutdown);
    let mut next = create_agent_pipe(&pipe_name, true)?;
    ready()?;
    let idle = wait_for_idle_exit(Arc::clone(&service));
    tokio::pin!(idle);
    loop {
        tokio::select! {
            biased;
            result = next.connect() => result?,
            () = &mut shutdown => return Ok(ServeExit::Shutdown),
            () = &mut idle => return Ok(ServeExit::Idle),
        }
        let connected = next;
        next = create_agent_pipe(&pipe_name, false)?;
        let activity = service.connection_started();
        let service = Arc::clone(&service);
        let policy = Arc::clone(&policy);
        tokio::spawn(async move {
            if let Err(error) =
                handle_connected_pipe_with_activity(connected, service, policy, activity).await
            {
                warn!(%error, "authenticated Agent client disconnected");
            }
        });
    }
}

async fn wait_for_idle_exit<Backend>(service: Arc<AgentService<Backend>>)
where
    Backend: PrivilegedBackend + 'static,
{
    wait_for_idle_exit_after(service, AGENT_IDLE_TIMEOUT).await;
}

async fn wait_for_idle_exit_after<Backend>(
    service: Arc<AgentService<Backend>>,
    idle_timeout: Duration,
) where
    Backend: PrivilegedBackend + 'static,
{
    loop {
        let notified = service.activity.notify.notified();
        tokio::pin!(notified);
        // Register before sampling the counters so a connection that finishes
        // between the sample and the await cannot leave this waiter asleep.
        let _ = notified.as_mut().enable();
        let generation = service.activity.generation.load(Ordering::Acquire);
        if !service.activity.is_empty() || service.state().await.phase != RecoveryPhase::Clean {
            notified.await;
            continue;
        }

        tokio::select! {
            () = tokio::time::sleep(idle_timeout) => {
                if service.activity.generation.load(Ordering::Acquire) == generation
                    && service.activity.is_empty()
                    && service.state().await.phase == RecoveryPhase::Clean
                {
                    return;
                }
            }
            () = &mut notified => {}
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum TunnelConnectionLease {
    Startup(Uuid),
    Active(Uuid),
}

#[cfg(test)]
async fn handle_connected_pipe<Backend>(
    pipe: NamedPipeServer,
    service: Arc<AgentService<Backend>>,
    policy: Arc<CallerPolicy>,
) -> Result<(), ServerError>
where
    Backend: PrivilegedBackend + 'static,
{
    let activity = service.connection_started();
    handle_connected_pipe_with_activity(pipe, service, policy, activity).await
}

async fn handle_connected_pipe_with_activity<Backend>(
    mut pipe: NamedPipeServer,
    service: Arc<AgentService<Backend>>,
    policy: Arc<CallerPolicy>,
    _activity: ActivityGuard,
) -> Result<(), ServerError>
where
    Backend: PrivilegedBackend + 'static,
{
    let raw_pipe = pipe.as_raw_handle() as usize;
    let authenticated =
        tokio::task::spawn_blocking(move || authenticate_named_pipe(raw_pipe as HANDLE, &policy))
            .await
            .map_err(|error| ServerError::AuthenticationTask(error.to_string()))??;
    let caller = authenticated.caller().clone();
    let mut buffer = BytesMut::new();
    let mut chunk = [0_u8; READ_CHUNK_BYTES];
    let mut system_proxy_lease = None;
    let mut tunnel_lease = None;

    let result = async {
        loop {
            let read = pipe.read(&mut chunk).await?;
            if read == 0 {
                break if buffer.is_empty() {
                    Ok(())
                } else {
                    Err(ServerError::TruncatedFrame)
                };
            }
            buffer.extend_from_slice(&chunk[..read]);
            if buffer.len() >= 4 {
                let declared = u32::from_be_bytes(buffer[..4].try_into().expect("header")) as usize;
                if declared > MAX_AGENT_FRAME_BYTES {
                    break Err(ServerError::FrameTooLarge(declared));
                }
            }
            while let Some(frame) = split_frame(&mut buffer)? {
                if frame.len() > MAX_AGENT_FRAME_BYTES + 4 {
                    return Err(ServerError::FrameTooLarge(frame.len() - 4));
                }
                let request: AgentRequest = decode_frame(frame)?;
                let lease_action = match request.payload.as_ref() {
                    Some(agent_request::Payload::ApplySystemProxy(request)) => {
                        Uuid::parse_str(request.operation_id.trim()).ok().map(Some)
                    }
                    Some(agent_request::Payload::RestoreSystemProxy(request)) => {
                        Uuid::parse_str(request.operation_id.trim())
                            .ok()
                            .map(|_| None)
                    }
                    _ => None,
                };
                let tunnel_lease_action = match request.payload.as_ref() {
                    Some(agent_request::Payload::PrepareTunnel(request)) => {
                        Uuid::parse_str(request.operation_id.trim())
                            .ok()
                            .map(TunnelConnectionLease::Startup)
                    }
                    Some(agent_request::Payload::AcquireTunnelLease(request)) => {
                        Uuid::parse_str(request.operation_id.trim())
                            .ok()
                            .map(TunnelConnectionLease::Active)
                    }
                    _ => None,
                };
                let response = service.handle(request, &caller).await;
                if response.error.is_none()
                    && let Some(next_lease) = lease_action
                {
                    system_proxy_lease = next_lease;
                }
                if response.error.is_none()
                    && let Some(next_lease) = tunnel_lease_action
                {
                    tunnel_lease = Some(next_lease);
                }
                let encoded = encode_frame(&response)?;
                if encoded.len() > MAX_AGENT_FRAME_BYTES + 4 {
                    return Err(ServerError::FrameTooLarge(encoded.len() - 4));
                }
                pipe.write_all(&encoded).await?;
            }
        }
    }
    .await;
    if let Some(operation_id) = system_proxy_lease {
        service
            .release_system_proxy_lease(operation_id, &caller)
            .await;
    }
    match tunnel_lease {
        Some(TunnelConnectionLease::Startup(operation_id)) => {
            service
                .release_startup_tunnel_lease(operation_id, &caller)
                .await;
        }
        Some(TunnelConnectionLease::Active(operation_id)) => {
            service.release_tunnel_lease(operation_id, &caller).await;
        }
        None => {}
    }
    result
}

fn create_agent_pipe(pipe_name: &str, first_instance: bool) -> io::Result<NamedPipeServer> {
    let descriptor = SecurityDescriptor::agent(pipe_name != AGENT_PIPE_NAME)?;
    let mut attributes = SECURITY_ATTRIBUTES {
        nLength: mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor.0,
        bInheritHandle: 0,
    };
    let mut options = ServerOptions::new();
    options
        .first_pipe_instance(first_instance)
        .reject_remote_clients(true);
    // SAFETY: attributes and its descriptor remain alive for the complete
    // CreateNamedPipeW call; Windows copies the descriptor before returning.
    unsafe {
        options.create_with_security_attributes_raw(
            pipe_name,
            (&mut attributes as *mut SECURITY_ATTRIBUTES).cast(),
        )
    }
}

fn validate_pipe_name(value: &str) -> Result<(), ServerError> {
    if value == AGENT_PIPE_NAME
        || cfg!(debug_assertions)
            && value.starts_with(&format!("{AGENT_PIPE_NAME}.test-"))
            && value.len() > AGENT_PIPE_NAME.len() + 6
            && value[AGENT_PIPE_NAME.len() + 6..]
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    {
        Ok(())
    } else {
        Err(ServerError::InvalidPipeName)
    }
}

fn validate_request_envelope(request: &AgentRequest) -> Result<(), ServiceError> {
    if request.protocol_version != AGENT_PROTOCOL_VERSION {
        return Err(ServiceError::ProtocolVersion(request.protocol_version));
    }
    if request.request_id.is_empty()
        || request.request_id.len() > MAX_REQUEST_ID_BYTES
        || !request
            .request_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return Err(ServiceError::RequestId);
    }
    Ok(())
}

fn parse_operation_id(value: &str) -> Result<Uuid, ServiceError> {
    Uuid::parse_str(value.trim()).map_err(|_| ServiceError::OperationId)
}

fn parse_profile_id(value: &str) -> Result<Uuid, ServiceError> {
    Uuid::parse_str(value.trim()).map_err(|_| ServiceError::ProfileId)
}

fn validate_reason_code(value: &str) -> Result<(), ServiceError> {
    if value.len() > 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
    {
        return Err(ServiceError::ReasonCode);
    }
    Ok(())
}

fn state_to_proto(journal: &RecoveryJournal, packet_session_attached: bool) -> AgentState {
    let applied = |kind| {
        journal
            .steps
            .iter()
            .any(|step| step.kind == kind && step.state == crate::journal::MutationState::Applied)
    };
    let mut warnings = Vec::new();
    if journal.phase == RecoveryPhase::RecoveryRequired {
        warnings.push("RECOVERY_REQUIRED".to_owned());
    }
    if journal.phase == RecoveryPhase::Active
        && applied(crate::journal::MutationKind::PacketSession)
        && !packet_session_attached
    {
        warnings.push("PACKET_SESSION_REATTACH_REQUIRED".to_owned());
    }
    AgentState {
        phase: match journal.phase {
            RecoveryPhase::Clean => agent_v1::AgentPhase::Clean as i32,
            RecoveryPhase::Preparing => agent_v1::AgentPhase::Preparing as i32,
            RecoveryPhase::Prepared => agent_v1::AgentPhase::Prepared as i32,
            RecoveryPhase::Active => agent_v1::AgentPhase::Active as i32,
            // Legacy journals may still deserialize as Paused after captive-portal
            // removal; surface them as recovery-required until recover_stale runs.
            RecoveryPhase::Paused => agent_v1::AgentPhase::RecoveryRequired as i32,
            RecoveryPhase::Recovering => agent_v1::AgentPhase::Recovering as i32,
            RecoveryPhase::RecoveryRequired => agent_v1::AgentPhase::RecoveryRequired as i32,
        },
        operation_id: journal
            .operation_id
            .map(|value| value.to_string())
            .unwrap_or_default(),
        profile_id: journal
            .plan
            .as_ref()
            .map(|plan| plan.profile_id.to_string())
            .unwrap_or_default(),
        kill_switch_active: applied(crate::journal::MutationKind::KillSwitch),
        system_proxy_active: applied(crate::journal::MutationKind::SystemProxy),
        packet_session_active: packet_session_attached,
        journal_generation: journal.generation,
        warnings,
    }
}

fn error_response(request_id: String, error: ServiceError) -> AgentResponse {
    let (code, retryable) = error.code();
    let message = error.to_string().chars().take(512).collect();
    AgentResponse {
        request_id,
        error: Some(agent_v1::AgentError {
            code: code.to_owned(),
            message,
            retryable,
        }),
        payload: None,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ReplayKey {
    sid: String,
    process_id: u32,
    request_id: String,
}

#[derive(Debug, Clone)]
struct CachedResponse {
    request: AgentRequest,
    response: AgentResponse,
}

#[derive(Debug, Default)]
struct ReplayCache {
    entries: HashMap<ReplayKey, CachedResponse>,
    order: VecDeque<ReplayKey>,
}

impl ReplayCache {
    fn insert(&mut self, key: ReplayKey, response: CachedResponse) {
        if self.entries.contains_key(&key) {
            return;
        }
        self.order.push_back(key.clone());
        self.entries.insert(key, response);
        while self.order.len() > MAX_REPLAY_ENTRIES {
            if let Some(expired) = self.order.pop_front() {
                self.entries.remove(&expired);
            }
        }
    }
}

struct SecurityDescriptor(PSECURITY_DESCRIPTOR);

impl SecurityDescriptor {
    fn agent(debug_test_pipe: bool) -> io::Result<Self> {
        // LocalSystem and Administrators receive full control. Authenticated
        // Users may connect/read/write, after which PID/SID/path/signature
        // authentication is mandatory before any frame is accepted.
        //
        // The Codex Windows test sandbox uses a restricted token whose access
        // check requires an Everyone ACE. That exception is limited to the
        // debug-only, randomly suffixed test pipe names accepted above.
        let sddl = if cfg!(debug_assertions) && debug_test_pipe {
            "D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;GA;;;WD)"
        } else {
            "D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;GA;;;AU)"
        };
        let wide: Vec<u16> = sddl.encode_utf16().chain(std::iter::once(0)).collect();
        let mut descriptor: PSECURITY_DESCRIPTOR = ptr::null_mut();
        // SAFETY: wide is null-terminated and descriptor is writable.
        if unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                wide.as_ptr(),
                SDDL_REVISION_1,
                &mut descriptor,
                ptr::null_mut(),
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(Self(descriptor))
    }
}

impl Drop for SecurityDescriptor {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: SDDL conversion allocates this descriptor with LocalAlloc.
            unsafe {
                LocalFree(self.0);
            }
        }
    }
}

#[derive(Debug, thiserror::Error)]
enum ServiceError {
    #[error("Agent protocol version {0} is unsupported")]
    ProtocolVersion(u32),
    #[error("request_id is missing or malformed")]
    RequestId,
    #[error("request_id was reused for a different request")]
    RequestIdReused,
    #[error("Agent request payload is missing")]
    MissingPayload,
    #[error("operation_id is not a UUID")]
    OperationId,
    #[error("profile_id is not a UUID")]
    ProfileId,
    #[error("rollback reason_code is malformed")]
    ReasonCode,
    #[error("prepare request is missing its tunnel plan")]
    MissingTunnelPlan,
    #[error("tunnel plan is invalid: {0}")]
    Plan(String),
    #[error("{0}")]
    Lifecycle(AgentLifecycleError),
}

impl ServiceError {
    const fn code(&self) -> (&'static str, bool) {
        match self {
            Self::ProtocolVersion(_) => ("AGENT_PROTOCOL_MISMATCH", false),
            Self::RequestId
            | Self::MissingPayload
            | Self::OperationId
            | Self::ProfileId
            | Self::ReasonCode => ("AGENT_INVALID_REQUEST", false),
            Self::RequestIdReused => ("AGENT_REQUEST_ID_REUSED", false),
            Self::MissingTunnelPlan | Self::Plan(_) => ("AGENT_INVALID_PLAN", false),
            Self::Lifecycle(AgentLifecycleError::StartMode(_)) => {
                ("SERVICE_START_MODE_UNAVAILABLE", false)
            }
            Self::Lifecycle(AgentLifecycleError::Coordinator(CoordinatorError::OwnerMismatch)) => {
                ("AGENT_OWNER_MISMATCH", false)
            }
            Self::Lifecycle(AgentLifecycleError::Coordinator(CoordinatorError::Backend(
                BackendError::EndpointUnreachable,
            ))) => ("AGENT_ENDPOINT_UNREACHABLE", true),
            Self::Lifecycle(AgentLifecycleError::Coordinator(CoordinatorError::Backend(
                BackendError::ControlApiUnreachable,
            ))) => ("AGENT_CONTROL_API_UNREACHABLE", true),
            Self::Lifecycle(AgentLifecycleError::Coordinator(
                CoordinatorError::RecoveryRequired(_) | CoordinatorError::ApplyAndRecovery { .. },
            )) => ("AGENT_RECOVERY_REQUIRED", false),
            Self::Lifecycle(AgentLifecycleError::Coordinator(_)) => {
                ("AGENT_OPERATION_FAILED", true)
            }
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ServerError {
    #[error("Agent pipe name is invalid")]
    InvalidPipeName,
    #[error("Agent pipe I/O failed: {0}")]
    Io(#[from] io::Error),
    #[error("Agent caller authentication failed: {0}")]
    Authentication(#[from] AuthenticationError),
    #[error("Agent authentication task failed: {0}")]
    AuthenticationTask(String),
    #[error("Agent protobuf frame failed: {0}")]
    Frame(#[from] usque_ipc::FrameError),
    #[error("Agent frame exceeds 64 KiB: {0}")]
    FrameTooLarge(usize),
    #[error("Agent client closed a truncated frame")]
    TruncatedFrame,
}

#[cfg(test)]
mod tests {
    use std::{
        io,
        sync::atomic::{AtomicBool, Ordering},
        time::Duration,
    };

    use async_trait::async_trait;
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::windows::named_pipe::ClientOptions,
    };
    use usque_ipc::{
        agent_v1::{ApplySystemProxyRequest, GetCapabilitiesRequest, agent_request},
        decode_frame, encode_frame,
    };

    use crate::{
        coordinator::{BackendError, StepOutput, StepParameter},
        journal::{JournalStore, MutationKind, MutationReceipt},
    };

    use super::*;

    #[tokio::test]
    async fn startup_pipe_validation_releases_the_first_instance() {
        let pipe_name = format!("{AGENT_PIPE_NAME}.test-{}", Uuid::new_v4());
        validate_pipe_creation(&pipe_name).expect("first validation");
        validate_pipe_creation(&pipe_name).expect("released validation pipe");
    }

    struct RejectingBackend;

    struct ProxyBackend;

    struct FailingStartModeController;

    #[async_trait]
    impl ServiceStartModeController for FailingStartModeController {
        async fn ensure_start_mode(
            &self,
            _mode: ServiceStartMode,
        ) -> Result<(), ServiceConfigError> {
            Err(ServiceConfigError::Change(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "test start-mode denial",
            )))
        }
    }

    #[async_trait]
    impl PrivilegedBackend for RejectingBackend {
        async fn plan_step(
            &self,
            _kind: MutationKind,
            _plan: &ValidatedTunnelPlan,
            _caller: &AuthenticatedCaller,
            _parameter: StepParameter,
        ) -> Result<MutationReceipt, BackendError> {
            Err(BackendError::Unavailable("test backend".to_owned()))
        }

        async fn apply_step(
            &self,
            _receipt: MutationReceipt,
            _plan: &ValidatedTunnelPlan,
            _caller: &AuthenticatedCaller,
        ) -> Result<(MutationReceipt, StepOutput), BackendError> {
            Err(BackendError::Unavailable("test backend".to_owned()))
        }

        async fn restore_step(&self, _receipt: &MutationReceipt) -> Result<(), BackendError> {
            Ok(())
        }
    }

    #[async_trait]
    impl PrivilegedBackend for ProxyBackend {
        async fn plan_step(
            &self,
            _kind: MutationKind,
            _plan: &ValidatedTunnelPlan,
            _caller: &AuthenticatedCaller,
            _parameter: StepParameter,
        ) -> Result<MutationReceipt, BackendError> {
            Err(BackendError::Unavailable("test tunnel backend".to_owned()))
        }

        async fn apply_step(
            &self,
            _receipt: MutationReceipt,
            _plan: &ValidatedTunnelPlan,
            _caller: &AuthenticatedCaller,
        ) -> Result<(MutationReceipt, StepOutput), BackendError> {
            Err(BackendError::Unavailable("test tunnel backend".to_owned()))
        }

        async fn restore_step(&self, _receipt: &MutationReceipt) -> Result<(), BackendError> {
            Ok(())
        }

        async fn plan_system_proxy(
            &self,
            operation_id: Uuid,
            caller: &AuthenticatedCaller,
            settings: &SystemProxySettings,
        ) -> Result<MutationReceipt, BackendError> {
            Ok(MutationReceipt::SystemProxy {
                user_sid: caller.user_sid.clone(),
                operation_id,
                previous_proxy_enable: Some(0),
                previous_proxy: None,
                previous_bypass: None,
                previous_auto_config_url: None,
                previous_auto_detect: Some(1),
                applied_proxy: settings.proxy_uri.clone(),
                applied_bypass: settings.bypass_hosts.join(";"),
            })
        }

        async fn apply_system_proxy(
            &self,
            receipt: MutationReceipt,
        ) -> Result<MutationReceipt, BackendError> {
            Ok(receipt)
        }
    }

    #[tokio::test]
    async fn current_process_round_trips_over_an_authenticated_pipe() {
        let directory = tempfile::tempdir().expect("tempdir");
        let coordinator = Arc::new(
            AgentCoordinator::open(
                JournalStore::new(directory.path().join("recovery.json")),
                Arc::new(RejectingBackend),
            )
            .expect("coordinator"),
        );
        let service = Arc::new(AgentService::new(
            coordinator,
            AgentCapabilities {
                wintun: false,
                wfp_kill_switch: false,
                interface_addresses: false,
                interface_dns: false,
                system_proxy: false,
                shared_packet_ring: false,
                operating_system: "windows".to_owned(),
                architecture: std::env::consts::ARCH.to_owned(),
                protocol_version: AGENT_PROTOCOL_VERSION,
            },
        ));
        let pipe_name = format!("{AGENT_PIPE_NAME}.test-{}", Uuid::new_v4());
        let server_pipe = create_agent_pipe(&pipe_name, true).expect("server");
        let executable = std::env::current_exe().expect("test path");
        let policy =
            Arc::new(CallerPolicy::new(vec![executable], None, true).expect("debug policy"));
        let server = tokio::spawn(async move {
            server_pipe.connect().await.expect("accept");
            handle_connected_pipe(server_pipe, service, policy)
                .await
                .expect("serve client");
        });

        let mut attempts = 0_u32;
        let mut client = loop {
            match ClientOptions::new().open(&pipe_name) {
                Ok(client) => break client,
                Err(error) if error.raw_os_error() == Some(2) => {
                    attempts += 1;
                    assert!(attempts < 100, "Agent pipe never appeared");
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
                Err(error) => panic!("open pipe: {error}"),
            }
        };
        let request = AgentRequest {
            request_id: "caps-1".to_owned(),
            protocol_version: AGENT_PROTOCOL_VERSION,
            payload: Some(agent_request::Payload::GetCapabilities(
                GetCapabilitiesRequest {},
            )),
        };
        tokio::time::timeout(
            Duration::from_secs(5),
            client.write_all(&encode_frame(&request).expect("encode")),
        )
        .await
        .expect("write timeout")
        .expect("write");
        let mut header = [0_u8; 4];
        if tokio::time::timeout(Duration::from_secs(5), client.read_exact(&mut header))
            .await
            .is_err()
        {
            let server_finished = server.is_finished();
            server.abort();
            panic!("response header timed out; server_finished={server_finished}");
        }
        let mut payload = vec![0_u8; u32::from_be_bytes(header) as usize];
        tokio::time::timeout(Duration::from_secs(5), client.read_exact(&mut payload))
            .await
            .expect("payload timeout")
            .expect("payload");
        let mut frame = BytesMut::from(header.as_slice());
        frame.extend_from_slice(&payload);
        let response: AgentResponse = decode_frame(frame.freeze()).expect("decode");
        assert!(response.error.is_none());
        assert!(matches!(
            response.payload,
            Some(agent_response::Payload::Capabilities(AgentCapabilities {
                protocol_version: AGENT_PROTOCOL_VERSION,
                ..
            }))
        ));
        client.shutdown().await.expect("shutdown");
        drop(client);
        tokio::time::timeout(Duration::from_secs(5), server)
            .await
            .expect("server shutdown timeout")
            .expect("join");
    }

    #[tokio::test]
    async fn forward_mutation_never_runs_when_auto_start_cannot_be_armed() {
        let directory = tempfile::tempdir().expect("tempdir");
        let coordinator = Arc::new(
            AgentCoordinator::open(
                JournalStore::new(directory.path().join("recovery.json")),
                Arc::new(RejectingBackend),
            )
            .expect("coordinator"),
        );
        let service = AgentService::with_start_mode_controller(
            Arc::clone(&coordinator),
            AgentCapabilities::default(),
            Arc::new(FailingStartModeController),
        );
        let reached = Arc::new(AtomicBool::new(false));
        let action_reached = Arc::clone(&reached);
        let result = service
            .mutate(MutationPolicy::Forward, move |_coordinator| async move {
                action_reached.store(true, Ordering::Release);
                Ok(())
            })
            .await;

        assert!(matches!(result, Err(AgentLifecycleError::StartMode(_))));
        assert!(!reached.load(Ordering::Acquire));
        assert_eq!(coordinator.state().await.phase, RecoveryPhase::Clean);
    }

    #[tokio::test]
    async fn demand_start_failure_does_not_turn_successful_cleanup_into_an_error() {
        let directory = tempfile::tempdir().expect("tempdir");
        let coordinator = Arc::new(
            AgentCoordinator::open(
                JournalStore::new(directory.path().join("recovery.json")),
                Arc::new(RejectingBackend),
            )
            .expect("coordinator"),
        );
        let service = AgentService::with_start_mode_controller(
            coordinator,
            AgentCapabilities::default(),
            Arc::new(FailingStartModeController),
        );
        let reached = Arc::new(AtomicBool::new(false));
        let action_reached = Arc::clone(&reached);
        service
            .mutate(MutationPolicy::Cleanup, move |_coordinator| async move {
                action_reached.store(true, Ordering::Release);
                Ok(())
            })
            .await
            .expect("cleanup result");
        assert!(reached.load(Ordering::Acquire));
    }

    #[tokio::test]
    async fn clean_service_waits_for_connections_before_idle_exit() {
        let directory = tempfile::tempdir().expect("tempdir");
        let coordinator = Arc::new(
            AgentCoordinator::open(
                JournalStore::new(directory.path().join("recovery.json")),
                Arc::new(RejectingBackend),
            )
            .expect("coordinator"),
        );
        let service = Arc::new(AgentService::new(coordinator, AgentCapabilities::default()));
        let connection = service.connection_started();
        assert!(
            tokio::time::timeout(
                Duration::from_millis(40),
                wait_for_idle_exit_after(Arc::clone(&service), Duration::from_millis(20)),
            )
            .await
            .is_err()
        );
        drop(connection);
        tokio::time::timeout(
            Duration::from_millis(100),
            wait_for_idle_exit_after(service, Duration::from_millis(20)),
        )
        .await
        .expect("idle exit");
    }

    #[tokio::test]
    async fn dropping_a_system_proxy_lease_restores_the_transaction() {
        let directory = tempfile::tempdir().expect("tempdir");
        let coordinator = Arc::new(
            AgentCoordinator::open(
                JournalStore::new(directory.path().join("recovery.json")),
                Arc::new(ProxyBackend),
            )
            .expect("coordinator"),
        );
        let service = Arc::new(AgentService::new(
            Arc::clone(&coordinator),
            AgentCapabilities {
                system_proxy: true,
                protocol_version: AGENT_PROTOCOL_VERSION,
                ..AgentCapabilities::default()
            },
        ));
        let pipe_name = format!("{AGENT_PIPE_NAME}.test-{}", Uuid::new_v4());
        let server_pipe = create_agent_pipe(&pipe_name, true).expect("server");
        let executable = std::env::current_exe().expect("test path");
        let policy =
            Arc::new(CallerPolicy::new(vec![executable], None, true).expect("debug policy"));
        let server = tokio::spawn(async move {
            server_pipe.connect().await.expect("accept");
            handle_connected_pipe(server_pipe, service, policy)
                .await
                .expect("serve client");
        });

        let mut client = ClientOptions::new().open(&pipe_name).expect("client");
        let operation_id = Uuid::new_v4();
        let request = AgentRequest {
            request_id: "proxy-lease".to_owned(),
            protocol_version: AGENT_PROTOCOL_VERSION,
            payload: Some(agent_request::Payload::ApplySystemProxy(
                ApplySystemProxyRequest {
                    operation_id: operation_id.to_string(),
                    proxy_uri: "http://127.0.0.1:8080".to_owned(),
                    bypass_hosts: vec!["<local>".to_owned()],
                },
            )),
        };
        client
            .write_all(&encode_frame(&request).expect("encode"))
            .await
            .expect("write");
        let mut header = [0_u8; 4];
        client.read_exact(&mut header).await.expect("header");
        let mut payload = vec![0_u8; u32::from_be_bytes(header) as usize];
        client.read_exact(&mut payload).await.expect("payload");
        let mut frame = BytesMut::from(header.as_slice());
        frame.extend_from_slice(&payload);
        let response: AgentResponse = decode_frame(frame.freeze()).expect("response");
        assert!(response.error.is_none(), "{:?}", response.error);
        assert_eq!(
            coordinator.state().await.operation_id,
            Some(operation_id),
            "lease must remain active while the pipe is open"
        );

        drop(client);
        tokio::time::timeout(Duration::from_secs(5), server)
            .await
            .expect("lease cleanup timeout")
            .expect("server task");
        assert_eq!(coordinator.state().await.phase, RecoveryPhase::Clean);
    }

    #[test]
    fn replay_cache_rejects_request_id_aliasing() {
        let key = ReplayKey {
            sid: "S-1-5-21-1".to_owned(),
            process_id: 1,
            request_id: "same".to_owned(),
        };
        let request = AgentRequest {
            request_id: "same".to_owned(),
            protocol_version: AGENT_PROTOCOL_VERSION,
            payload: Some(agent_request::Payload::GetCapabilities(
                GetCapabilitiesRequest {},
            )),
        };
        let response = AgentResponse {
            request_id: "same".to_owned(),
            error: None,
            payload: Some(agent_response::Payload::Empty(agent_v1::Empty {})),
        };
        let mut cache = ReplayCache::default();
        cache.insert(
            key.clone(),
            CachedResponse {
                request: request.clone(),
                response,
            },
        );
        assert_eq!(cache.entries.get(&key).expect("cached").request, request);
    }
}
