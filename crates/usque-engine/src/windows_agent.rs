use std::{io, net::SocketAddr, ptr::NonNull, sync::Arc, time::Duration};

use bytes::BytesMut;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::windows::named_pipe::{ClientOptions, NamedPipeClient},
    sync::{mpsc, watch},
    task::JoinHandle,
    time::timeout,
};
use tokio_util::sync::CancellationToken;
use usque_core::{IpPolicy, Profile, REGISTRATION_API_HOST, REGISTRATION_API_PORT};
use usque_ipc::{
    agent_v1::{
        self, AcquireTunnelLeaseRequest, AgentCapabilities, AgentRequest, AgentResponse,
        AgentState, ApplySystemProxyRequest, ClosePacketSessionRequest, CommitTunnelRequest,
        GetCapabilitiesRequest, GetStateRequest, OpenPacketSessionRequest, PacketSessionHandles,
        PauseKillSwitchRequest, PrepareTunnelRequest, RestoreSystemProxyRequest,
        ResumeTunnelRequest, RollbackTunnelRequest, agent_request, agent_response,
    },
    decode_frame, encode_frame,
};
use usque_platform::packet_ring::{
    PACKET_RING_LAYOUT_VERSION, PacketDirection, PacketRingError, SharedPacketRing,
};
use usque_transport::{
    EndpointPinRefresher, ManagedTunnelMonitor, ManagedTunnelRuntime, MasqueTlsIdentity,
    NoopSocketProtector, RuntimeHealth, RuntimePath, SocketHandle, SocketProtector,
    TrafficSnapshot, TransportError,
};
use uuid::Uuid;
use windows_sys::Win32::{
    Foundation::{
        CloseHandle, ERROR_FILE_NOT_FOUND, ERROR_PIPE_BUSY, HANDLE, WAIT_FAILED, WAIT_OBJECT_0,
    },
    System::{
        Memory::{FILE_MAP_ALL_ACCESS, MEMORY_MAPPED_VIEW_ADDRESS, MapViewOfFile, UnmapViewOfFile},
        Threading::{INFINITE, SetEvent, WaitForMultipleObjects},
    },
};

const AGENT_PIPE_NAME: &str = r"\\.\pipe\io.github.georgexie2333.usque.agent.v1";
const AGENT_PROTOCOL_VERSION: u32 = 2;
const MAX_AGENT_FRAME_BYTES: usize = 64 * 1024;
const AGENT_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const AGENT_RPC_TIMEOUT: Duration = Duration::from_secs(30);
const PUMP_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);
const DEFAULT_PACKET_RING_CAPACITY: u32 = 4 * 1024 * 1024;

struct CachedControlSocketProtector {
    registration_api: Vec<SocketAddr>,
}

impl SocketProtector for CachedControlSocketProtector {
    fn protect(&self, _socket: SocketHandle) -> Result<(), String> {
        Ok(())
    }

    fn resolve(&self, host: &str, port: u16) -> Result<Vec<SocketAddr>, String> {
        if host == REGISTRATION_API_HOST && port == REGISTRATION_API_PORT {
            Ok(self.registration_api.clone())
        } else {
            Err("the Windows VPN resolver accepts only the pinned registration API host".to_owned())
        }
    }
}

async fn resolve_registration_api() -> Result<Vec<SocketAddr>, WindowsVpnError> {
    tokio::task::spawn_blocking(|| {
        let resolver = NoopSocketProtector;
        resolver.resolve(REGISTRATION_API_HOST, REGISTRATION_API_PORT)
    })
    .await
    .map_err(|error| WindowsVpnError::ControlEndpointResolution(error.to_string()))?
    .map_err(WindowsVpnError::ControlEndpointResolution)
}

pub(crate) struct WindowsSystemProxyGuard {
    client: WindowsAgentClient,
    operation_id: Uuid,
    pipe: Option<NamedPipeClient>,
}

impl WindowsSystemProxyGuard {
    pub(crate) async fn start(listener: std::net::SocketAddr) -> Result<Self, WindowsVpnError> {
        if !listener.ip().is_loopback() || listener.port() == 0 {
            return Err(WindowsVpnError::InvalidSystemProxyListener(listener));
        }
        let client = WindowsAgentClient::production();
        let capabilities = client.get_capabilities().await?;
        if capabilities.protocol_version != AGENT_PROTOCOL_VERSION {
            return Err(WindowsVpnError::ProtocolVersion(
                capabilities.protocol_version,
            ));
        }
        if !capabilities.system_proxy {
            return Err(WindowsVpnError::MissingCapabilities(
                "system_proxy".to_owned(),
            ));
        }
        let state = client.get_state().await?;
        if state.phase != agent_v1::AgentPhase::Clean as i32 {
            return Err(WindowsVpnError::RecoveryRequired {
                phase: state.phase,
                operation_id: state.operation_id,
            });
        }
        let operation_id = Uuid::new_v4();
        let pipe = client
            .apply_system_proxy_lease(
                operation_id,
                format!("http://{listener}"),
                vec![
                    "localhost".to_owned(),
                    "127.*".to_owned(),
                    "[::1]".to_owned(),
                    "<local>".to_owned(),
                ],
            )
            .await?;
        Ok(Self {
            client,
            operation_id,
            pipe: Some(pipe),
        })
    }

    pub(crate) async fn shutdown(&mut self) -> Result<(), WindowsVpnError> {
        let Some(mut pipe) = self.pipe.take() else {
            return Ok(());
        };
        let result = timeout(
            AGENT_RPC_TIMEOUT,
            self.client
                .restore_system_proxy(&mut pipe, self.operation_id),
        )
        .await
        .map_err(|_| WindowsVpnError::RpcTimeout)
        .and_then(|result| result);
        let _ = pipe.shutdown().await;
        result.map(|_| ())
    }
}

impl Drop for WindowsSystemProxyGuard {
    fn drop(&mut self) {
        // Closing the leased pipe is itself the crash-safe restore signal.
        // The Agent also recovers this transaction when its service restarts.
        self.pipe.take();
    }
}

pub(crate) struct WindowsVpnRuntime {
    agent: WindowsAgentClient,
    operation_id: Uuid,
    monitor: WindowsVpnMonitor,
    cancellation: CancellationToken,
    mapping: Arc<PacketSessionMapping>,
    tasks: Vec<JoinHandle<()>>,
    transaction_open: bool,
}

#[derive(Clone)]
pub(crate) struct WindowsVpnMonitor {
    tunnel: ManagedTunnelMonitor,
    pump_failure: watch::Receiver<Option<String>>,
    agent_disconnected: watch::Receiver<bool>,
}

impl WindowsVpnMonitor {
    pub(crate) fn path(&self) -> RuntimePath {
        self.tunnel.path()
    }

    pub(crate) fn health(&self) -> RuntimeHealth {
        if let Some(message) = self.pump_failure.borrow().clone() {
            let transport = self.tunnel.health();
            RuntimeHealth::Failed {
                last_path: transport.path(),
                reconnect_count: transport.reconnect_count(),
                message,
            }
        } else {
            self.tunnel.health()
        }
    }

    pub(crate) fn statistics(&self) -> TrafficSnapshot {
        self.tunnel.statistics()
    }

    pub(crate) fn failure(&self) -> Option<String> {
        self.pump_failure
            .borrow()
            .clone()
            .or_else(|| self.tunnel.failure())
    }

    pub(crate) fn agent_disconnected(&self) -> bool {
        *self.agent_disconnected.borrow()
    }
}

impl WindowsVpnRuntime {
    pub(crate) async fn start(
        profile: &Profile,
        identity: MasqueTlsIdentity,
        pin_refresher: Arc<dyn EndpointPinRefresher>,
    ) -> Result<Self, WindowsVpnError> {
        // Resolve before the Agent installs the fail-closed WFP policy. The
        // authenticated refresh client later uses only these exact numeric
        // addresses, so neither DNS nor arbitrary physical egress is opened
        // while the tunnel is active.
        let registration_api = resolve_registration_api().await?;
        let protector: Arc<dyn SocketProtector> = Arc::new(CachedControlSocketProtector {
            registration_api: registration_api.clone(),
        });
        let agent = WindowsAgentClient::production();
        let capabilities = agent.get_capabilities().await?;
        validate_capabilities(&capabilities, profile.kill_switch)?;
        let state = agent.get_state().await?;
        let (operation_id, resuming) = match agent_v1::AgentPhase::try_from(state.phase) {
            Ok(agent_v1::AgentPhase::Clean) => {
                let operation_id = Uuid::new_v4();
                let plan = tunnel_plan(profile, &identity, &registration_api);
                agent.prepare(operation_id, plan).await?;
                (operation_id, false)
            }
            Ok(agent_v1::AgentPhase::Active) if state.profile_id == profile.id.to_string() => {
                let operation_id = Uuid::parse_str(&state.operation_id)
                    .map_err(|_| WindowsVpnError::InvalidAgentOperationId)?;
                (operation_id, true)
            }
            Ok(agent_v1::AgentPhase::Active) => {
                return Err(WindowsVpnError::ActiveProfileMismatch {
                    active: state.profile_id,
                    requested: profile.id,
                });
            }
            _ => {
                return Err(WindowsVpnError::RecoveryRequired {
                    phase: state.phase,
                    operation_id: state.operation_id,
                });
            }
        };

        let mut tunnel = match ManagedTunnelRuntime::start_with_refresh(
            profile,
            identity,
            protector,
            Some(pin_refresher),
        )
        .await
        {
            Ok(tunnel) => tunnel,
            Err(error) => {
                abort_startup(&agent, operation_id, resuming, "TRANSPORT_START_FAILED").await?;
                return Err(error.into());
            }
        };
        let tunnel_monitor = tunnel.monitor();

        let handles_result = if resuming {
            agent.resume_tunnel(operation_id, profile.id).await
        } else {
            agent
                .open_packet_session(operation_id, DEFAULT_PACKET_RING_CAPACITY)
                .await
        };
        let handles = match handles_result {
            Ok(handles) => handles,
            Err(error) => {
                tunnel.shutdown().await;
                abort_startup(&agent, operation_id, resuming, "PACKET_SESSION_FAILED").await?;
                return Err(error);
            }
        };
        let mapping = match PacketSessionMapping::attach(handles) {
            Ok(mapping) => Arc::new(mapping),
            Err(error) => {
                tunnel.shutdown().await;
                abort_startup(&agent, operation_id, resuming, "PACKET_MAPPING_FAILED").await?;
                return Err(error);
            }
        };

        let cancellation = CancellationToken::new();
        let (pump_failure_tx, pump_failure) = watch::channel(None);
        let (agent_disconnected_tx, agent_disconnected) = watch::channel(false);
        let sender = match tunnel.packet_sender() {
            Ok(sender) => sender,
            Err(error) => {
                mapping.signal_shutdown();
                tunnel.shutdown().await;
                abort_startup(&agent, operation_id, resuming, "PACKET_SENDER_FAILED").await?;
                return Err(error.into());
            }
        };
        let mut tasks = start_packet_pumps(
            tunnel,
            sender,
            Arc::clone(&mapping),
            cancellation.clone(),
            pump_failure_tx.clone(),
        );

        if !resuming && let Err(error) = agent.commit(operation_id).await {
            mapping.signal_shutdown();
            cancellation.cancel();
            stop_tasks(tasks).await;
            rollback_startup(&agent, operation_id, "COMMIT_FAILED").await?;
            return Err(error);
        }

        let lease = match agent.open_liveness_lease(operation_id).await {
            Ok(lease) => lease,
            Err(error) => {
                mapping.signal_shutdown();
                cancellation.cancel();
                stop_tasks(tasks).await;
                abort_startup(&agent, operation_id, resuming, "LIVENESS_LEASE_FAILED").await?;
                return Err(error);
            }
        };
        tasks.push(start_agent_liveness_watch(
            lease,
            Arc::clone(&mapping),
            cancellation.clone(),
            pump_failure_tx,
            agent_disconnected_tx,
        ));

        if resuming {
            tracing::info!(
                %operation_id,
                profile_id = %profile.id,
                "reattached Engine data plane to active Windows Agent transaction"
            );
        }

        Ok(Self {
            agent,
            operation_id,
            monitor: WindowsVpnMonitor {
                tunnel: tunnel_monitor,
                pump_failure,
                agent_disconnected,
            },
            cancellation,
            mapping,
            tasks,
            transaction_open: true,
        })
    }

    pub(crate) fn path(&self) -> RuntimePath {
        self.monitor.path()
    }

    pub(crate) fn health(&self) -> RuntimeHealth {
        self.monitor.health()
    }

    pub(crate) fn statistics(&self) -> TrafficSnapshot {
        self.monitor.statistics()
    }

    pub(crate) fn failure(&self) -> Option<String> {
        self.monitor.failure()
    }

    pub(crate) fn requires_agent_reattach(&self) -> bool {
        self.monitor.agent_disconnected()
    }

    pub(crate) async fn detach_for_agent_reattach(&mut self) -> Result<(), WindowsVpnError> {
        self.stop_packet_pumps().await;
        let state = self.agent.get_state().await?;
        if state.phase != agent_v1::AgentPhase::Active as i32
            || state.operation_id != self.operation_id.to_string()
        {
            return Err(WindowsVpnError::RecoveryRequired {
                phase: state.phase,
                operation_id: state.operation_id,
            });
        }
        if state.packet_session_active {
            self.agent.close_packet_session(self.operation_id).await?;
        }
        // The replacement runtime must adopt the same persistent transaction.
        // Drop must therefore not perform a rollback between detach and resume.
        self.transaction_open = false;
        Ok(())
    }

    pub(crate) async fn pause_for_captive_portal(
        &mut self,
        seconds: u32,
    ) -> Result<(), WindowsVpnError> {
        self.agent.pause(self.operation_id, seconds).await?;
        self.stop_packet_pumps().await;
        Ok(())
    }

    pub(crate) async fn shutdown(&mut self) -> Result<(), WindowsVpnError> {
        // Cut packet forwarding before any Agent RPC. Rollback may need to
        // restore routes, DNS, WFP, and the adapter, but no user packet may
        // remain attached to MASQUE while that cleanup is in progress.
        self.cancel_immediately();
        let rollback = if self.transaction_open {
            self.agent
                .rollback(self.operation_id, "USER_DISCONNECT")
                .await
        } else {
            Ok(AgentState::default())
        };
        if rollback.is_ok() {
            self.transaction_open = false;
        }
        self.stop_packet_pumps().await;
        rollback.map(|_| ())
    }

    pub(crate) fn cancel_immediately(&mut self) {
        self.mapping.signal_shutdown();
        self.cancellation.cancel();
        for task in &self.tasks {
            task.abort();
        }
    }

    async fn stop_packet_pumps(&mut self) {
        self.cancel_immediately();
        stop_tasks(std::mem::take(&mut self.tasks)).await;
    }
}

impl Drop for WindowsVpnRuntime {
    fn drop(&mut self) {
        // Async rollback is deliberately not attempted from Drop. If the
        // Engine is torn down unexpectedly, the Agent's persistent WFP state
        // remains fail-closed until an authenticated recovery operation.
        self.cancel_immediately();
    }
}

async fn rollback_startup(
    agent: &WindowsAgentClient,
    operation_id: Uuid,
    reason: &'static str,
) -> Result<(), WindowsVpnError> {
    agent.rollback(operation_id, reason).await.map(|_| ())
}

async fn abort_startup(
    agent: &WindowsAgentClient,
    operation_id: Uuid,
    resuming: bool,
    reason: &'static str,
) -> Result<(), WindowsVpnError> {
    if resuming {
        // Persistent WFP/routes deliberately remain fail-closed. A later
        // authenticated retry can reattach without exposing physical traffic.
        Ok(())
    } else {
        rollback_startup(agent, operation_id, reason).await
    }
}

fn tunnel_plan(
    profile: &Profile,
    identity: &MasqueTlsIdentity,
    registration_api: &[SocketAddr],
) -> agent_v1::TunnelPlan {
    let ipv4 = profile.endpoint.ipv4_socket();
    let ipv6 = profile.endpoint.ipv6_socket();
    let endpoint = match profile.ip_policy {
        IpPolicy::PreferIpv6 | IpPolicy::Ipv6Only => ipv6,
        IpPolicy::Auto | IpPolicy::PreferIpv4 | IpPolicy::Ipv4Only => ipv4,
    };
    let endpoint_candidates = match profile.ip_policy {
        IpPolicy::Ipv4Only => vec![ipv4.to_string()],
        IpPolicy::Ipv6Only => vec![ipv6.to_string()],
        IpPolicy::Auto | IpPolicy::PreferIpv4 | IpPolicy::PreferIpv6 => {
            vec![ipv4.to_string(), ipv6.to_string()]
        }
    };
    agent_v1::TunnelPlan {
        profile_id: profile.id.to_string(),
        endpoint: endpoint.to_string(),
        mtu: u32::from(profile.mtu),
        dns_servers: profile
            .dns_servers
            .iter()
            .filter(|server| match profile.ip_policy {
                IpPolicy::Ipv4Only => server.is_ipv4(),
                IpPolicy::Ipv6Only => server.is_ipv6(),
                IpPolicy::Auto | IpPolicy::PreferIpv4 | IpPolicy::PreferIpv6 => true,
            })
            .map(ToString::to_string)
            .collect(),
        split_exclusions: profile
            .split_exclusions
            .iter()
            .map(ToString::to_string)
            .collect(),
        allow_lan: profile.allow_lan,
        kill_switch: profile.kill_switch,
        assigned_ipv4: format!("{}/32", identity.assigned_ipv4),
        assigned_ipv6: format!("{}/128", identity.assigned_ipv6),
        endpoint_candidates,
        control_api_candidates: registration_api.iter().map(ToString::to_string).collect(),
    }
}

fn validate_capabilities(
    capabilities: &AgentCapabilities,
    require_kill_switch: bool,
) -> Result<(), WindowsVpnError> {
    if capabilities.protocol_version != AGENT_PROTOCOL_VERSION {
        return Err(WindowsVpnError::ProtocolVersion(
            capabilities.protocol_version,
        ));
    }
    let mut missing = Vec::new();
    if !capabilities.wintun {
        missing.push("wintun");
    }
    if !capabilities.interface_addresses {
        missing.push("interface_addresses");
    }
    if !capabilities.interface_dns {
        missing.push("interface_dns");
    }
    if !capabilities.shared_packet_ring {
        missing.push("shared_packet_ring");
    }
    if require_kill_switch && !capabilities.wfp_kill_switch {
        missing.push("wfp_kill_switch");
    }
    if missing.is_empty() {
        Ok(())
    } else {
        Err(WindowsVpnError::MissingCapabilities(missing.join(",")))
    }
}

fn start_packet_pumps(
    mut tunnel: ManagedTunnelRuntime,
    sender: usque_transport::ManagedTunnelSender,
    mapping: Arc<PacketSessionMapping>,
    cancellation: CancellationToken,
    failure: watch::Sender<Option<String>>,
) -> Vec<JoinHandle<()>> {
    let (packet_ready_tx, mut packet_ready_rx) = mpsc::channel(1);

    let wait_mapping = Arc::clone(&mapping);
    let wait_cancel = cancellation.clone();
    let wait_failure = failure.clone();
    let wait_task = tokio::spawn(async move {
        let result = tokio::task::spawn_blocking(move || {
            wait_for_agent_packets(&wait_mapping, packet_ready_tx)
        })
        .await;
        match result {
            Ok(Ok(())) => {}
            Ok(Err(error)) => report_pump_failure(&wait_failure, &wait_cancel, error.to_string()),
            Err(error) => report_pump_failure(
                &wait_failure,
                &wait_cancel,
                format!("Agent packet wait task failed: {error}"),
            ),
        }
    });

    let inbound_mapping = Arc::clone(&mapping);
    let inbound_cancel = cancellation.clone();
    let inbound_failure = failure.clone();
    let inbound_task = tokio::spawn(async move {
        loop {
            tokio::select! {
                () = inbound_cancel.cancelled() => break,
                ready = packet_ready_rx.recv() => {
                    if ready.is_none() {
                        if !inbound_cancel.is_cancelled() {
                            report_pump_failure(
                                &inbound_failure,
                                &inbound_cancel,
                                "Agent packet notification channel closed".to_owned(),
                            );
                        }
                        break;
                    }
                    loop {
                        match inbound_mapping
                            .ring()
                            .try_pop(PacketDirection::AgentToEngine)
                        {
                            Ok(Some(packet)) => {
                                if let Err(error) = sender.send_packet(&packet).await {
                                    if !inbound_cancel.is_cancelled() {
                                        report_pump_failure(
                                            &inbound_failure,
                                            &inbound_cancel,
                                            format!("failed to send a TUN packet into MASQUE: {error}"),
                                        );
                                    }
                                    return;
                                }
                            }
                            Ok(None) => break,
                            Err(error) => {
                                report_pump_failure(
                                    &inbound_failure,
                                    &inbound_cancel,
                                    format!("Agent-to-Engine packet ring failed: {error}"),
                                );
                                return;
                            }
                        }
                    }
                }
            }
        }
    });

    let outbound_mapping = mapping;
    let outbound_cancel = cancellation;
    let outbound_failure = failure;
    let outbound_task = tokio::spawn(async move {
        loop {
            tokio::select! {
                () = outbound_cancel.cancelled() => break,
                packet = tunnel.receive_packet() => {
                    match packet {
                        Ok(packet) => {
                            match outbound_mapping
                                .ring()
                                .try_push(PacketDirection::EngineToAgent, &packet)
                            {
                                Ok(true) => {
                                    if let Err(error) = outbound_mapping.signal_engine_to_agent() {
                                        report_pump_failure(
                                            &outbound_failure,
                                            &outbound_cancel,
                                            error.to_string(),
                                        );
                                        break;
                                    }
                                }
                                Ok(false) => {
                                    // Ring pressure is accounted by the shared
                                    // dropped counter. Keep the tunnel alive.
                                }
                                Err(error) => {
                                    report_pump_failure(
                                        &outbound_failure,
                                        &outbound_cancel,
                                        format!("Engine-to-Agent packet ring failed: {error}"),
                                    );
                                    break;
                                }
                            }
                        }
                        Err(error) => {
                            if !outbound_cancel.is_cancelled() {
                                report_pump_failure(
                                    &outbound_failure,
                                    &outbound_cancel,
                                    format!("failed to receive a MASQUE packet: {error}"),
                                );
                            }
                            break;
                        }
                    }
                }
            }
        }
        tunnel.shutdown().await;
    });

    vec![wait_task, inbound_task, outbound_task]
}

fn report_pump_failure(
    failure: &watch::Sender<Option<String>>,
    cancellation: &CancellationToken,
    message: String,
) {
    if failure.borrow().is_none() {
        failure.send_replace(Some(message));
    }
    cancellation.cancel();
}

fn start_agent_liveness_watch(
    mut pipe: NamedPipeClient,
    mapping: Arc<PacketSessionMapping>,
    cancellation: CancellationToken,
    failure: watch::Sender<Option<String>>,
    agent_disconnected: watch::Sender<bool>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut probe = [0_u8; 1];
        let result = tokio::select! {
            () = cancellation.cancelled() => return,
            result = pipe.read(&mut probe) => result,
        };
        if cancellation.is_cancelled() {
            return;
        }
        mapping.signal_shutdown();
        agent_disconnected.send_replace(true);
        let message = match result {
            Ok(0) => "Windows Agent service connection closed".to_owned(),
            Ok(_) => "Windows Agent sent unexpected liveness data".to_owned(),
            Err(error) => format!("Windows Agent service connection failed: {error}"),
        };
        report_pump_failure(&failure, &cancellation, message);
    })
}

fn wait_for_agent_packets(
    mapping: &PacketSessionMapping,
    ready: mpsc::Sender<()>,
) -> Result<(), WindowsVpnError> {
    let handles = [mapping.shutdown_event.0, mapping.agent_to_engine_event.0];
    loop {
        // SAFETY: both owned handles outlive this blocking call and the slice is
        // valid for its complete duration.
        let result =
            unsafe { WaitForMultipleObjects(handles.len() as u32, handles.as_ptr(), 0, INFINITE) };
        match result {
            value if value == WAIT_OBJECT_0 => return Ok(()),
            value if value == WAIT_OBJECT_0 + 1 => match ready.try_send(()) {
                Ok(()) | Err(mpsc::error::TrySendError::Full(())) => {}
                Err(mpsc::error::TrySendError::Closed(())) => return Ok(()),
            },
            WAIT_FAILED => {
                return Err(WindowsVpnError::Io(last_error(
                    "WaitForMultipleObjects(packet session)",
                )));
            }
            value => return Err(WindowsVpnError::UnexpectedWait(value)),
        }
    }
}

async fn stop_tasks(tasks: Vec<JoinHandle<()>>) {
    for mut task in tasks {
        if timeout(PUMP_SHUTDOWN_TIMEOUT, &mut task).await.is_err() {
            task.abort();
            let _ = task.await;
        }
    }
}

#[derive(Clone)]
struct WindowsAgentClient {
    pipe_name: Arc<str>,
}

impl WindowsAgentClient {
    fn production() -> Self {
        Self {
            pipe_name: Arc::from(AGENT_PIPE_NAME),
        }
    }

    #[cfg(test)]
    fn for_test(pipe_name: String) -> Self {
        Self {
            pipe_name: Arc::from(pipe_name),
        }
    }

    async fn get_capabilities(&self) -> Result<AgentCapabilities, WindowsVpnError> {
        match self
            .call(agent_request::Payload::GetCapabilities(
                GetCapabilitiesRequest {},
            ))
            .await?
        {
            agent_response::Payload::Capabilities(capabilities) => Ok(capabilities),
            payload => Err(WindowsVpnError::UnexpectedResponse(payload_name(&payload))),
        }
    }

    async fn get_state(&self) -> Result<AgentState, WindowsVpnError> {
        match self
            .call(agent_request::Payload::GetState(GetStateRequest {}))
            .await?
        {
            agent_response::Payload::State(state) => Ok(state),
            payload => Err(WindowsVpnError::UnexpectedResponse(payload_name(&payload))),
        }
    }

    async fn prepare(
        &self,
        operation_id: Uuid,
        plan: agent_v1::TunnelPlan,
    ) -> Result<AgentState, WindowsVpnError> {
        match self
            .call(agent_request::Payload::PrepareTunnel(
                PrepareTunnelRequest {
                    operation_id: operation_id.to_string(),
                    plan: Some(plan),
                },
            ))
            .await?
        {
            agent_response::Payload::State(state)
                if state.phase == agent_v1::AgentPhase::Prepared as i32 =>
            {
                Ok(state)
            }
            agent_response::Payload::State(state) => {
                Err(WindowsVpnError::UnexpectedAgentPhase(state.phase))
            }
            payload => Err(WindowsVpnError::UnexpectedResponse(payload_name(&payload))),
        }
    }

    async fn open_packet_session(
        &self,
        operation_id: Uuid,
        capacity: u32,
    ) -> Result<PacketSessionHandles, WindowsVpnError> {
        match self
            .call(agent_request::Payload::OpenPacketSession(
                OpenPacketSessionRequest {
                    operation_id: operation_id.to_string(),
                    ring_capacity: capacity,
                },
            ))
            .await?
        {
            agent_response::Payload::PacketSession(handles) => Ok(handles),
            payload => Err(WindowsVpnError::UnexpectedResponse(payload_name(&payload))),
        }
    }

    async fn resume_tunnel(
        &self,
        operation_id: Uuid,
        profile_id: Uuid,
    ) -> Result<PacketSessionHandles, WindowsVpnError> {
        match self
            .call(agent_request::Payload::ResumeTunnel(ResumeTunnelRequest {
                operation_id: operation_id.to_string(),
                profile_id: profile_id.to_string(),
            }))
            .await?
        {
            agent_response::Payload::PacketSession(handles) => Ok(handles),
            payload => Err(WindowsVpnError::UnexpectedResponse(payload_name(&payload))),
        }
    }

    async fn close_packet_session(
        &self,
        operation_id: Uuid,
    ) -> Result<AgentState, WindowsVpnError> {
        match self
            .call(agent_request::Payload::ClosePacketSession(
                ClosePacketSessionRequest {
                    operation_id: operation_id.to_string(),
                },
            ))
            .await?
        {
            agent_response::Payload::State(state)
                if state.phase == agent_v1::AgentPhase::Active as i32 =>
            {
                Ok(state)
            }
            agent_response::Payload::State(state) => {
                Err(WindowsVpnError::UnexpectedAgentPhase(state.phase))
            }
            payload => Err(WindowsVpnError::UnexpectedResponse(payload_name(&payload))),
        }
    }

    async fn open_liveness_lease(
        &self,
        operation_id: Uuid,
    ) -> Result<NamedPipeClient, WindowsVpnError> {
        timeout(AGENT_RPC_TIMEOUT, async {
            let mut pipe = self.open_pipe().await?;
            match self
                .exchange(
                    &mut pipe,
                    agent_request::Payload::AcquireTunnelLease(AcquireTunnelLeaseRequest {
                        operation_id: operation_id.to_string(),
                    }),
                )
                .await?
            {
                agent_response::Payload::State(state)
                    if state.phase == agent_v1::AgentPhase::Active as i32
                        && state.operation_id == operation_id.to_string()
                        && state.packet_session_active =>
                {
                    Ok(pipe)
                }
                agent_response::Payload::State(state) => {
                    Err(WindowsVpnError::UnexpectedAgentPhase(state.phase))
                }
                payload => Err(WindowsVpnError::UnexpectedResponse(payload_name(&payload))),
            }
        })
        .await
        .map_err(|_| WindowsVpnError::RpcTimeout)?
    }

    async fn commit(&self, operation_id: Uuid) -> Result<AgentState, WindowsVpnError> {
        match self
            .call(agent_request::Payload::CommitTunnel(CommitTunnelRequest {
                operation_id: operation_id.to_string(),
            }))
            .await?
        {
            agent_response::Payload::State(state)
                if state.phase == agent_v1::AgentPhase::Active as i32 =>
            {
                Ok(state)
            }
            agent_response::Payload::State(state) => {
                Err(WindowsVpnError::UnexpectedAgentPhase(state.phase))
            }
            payload => Err(WindowsVpnError::UnexpectedResponse(payload_name(&payload))),
        }
    }

    async fn rollback(
        &self,
        operation_id: Uuid,
        reason: &'static str,
    ) -> Result<AgentState, WindowsVpnError> {
        match self
            .call(agent_request::Payload::RollbackTunnel(
                RollbackTunnelRequest {
                    operation_id: operation_id.to_string(),
                    reason_code: reason.to_owned(),
                },
            ))
            .await?
        {
            agent_response::Payload::State(state)
                if state.phase == agent_v1::AgentPhase::Clean as i32 =>
            {
                Ok(state)
            }
            agent_response::Payload::State(state) => {
                Err(WindowsVpnError::UnexpectedAgentPhase(state.phase))
            }
            payload => Err(WindowsVpnError::UnexpectedResponse(payload_name(&payload))),
        }
    }

    async fn pause(&self, operation_id: Uuid, seconds: u32) -> Result<AgentState, WindowsVpnError> {
        match self
            .call(agent_request::Payload::PauseKillSwitch(
                PauseKillSwitchRequest {
                    operation_id: operation_id.to_string(),
                    seconds,
                },
            ))
            .await?
        {
            agent_response::Payload::State(state)
                if state.phase == agent_v1::AgentPhase::Paused as i32 =>
            {
                Ok(state)
            }
            agent_response::Payload::State(state) => {
                Err(WindowsVpnError::UnexpectedAgentPhase(state.phase))
            }
            payload => Err(WindowsVpnError::UnexpectedResponse(payload_name(&payload))),
        }
    }

    async fn apply_system_proxy_lease(
        &self,
        operation_id: Uuid,
        proxy_uri: String,
        bypass_hosts: Vec<String>,
    ) -> Result<NamedPipeClient, WindowsVpnError> {
        timeout(AGENT_RPC_TIMEOUT, async {
            let mut pipe = self.open_pipe().await?;
            match self
                .exchange(
                    &mut pipe,
                    agent_request::Payload::ApplySystemProxy(ApplySystemProxyRequest {
                        operation_id: operation_id.to_string(),
                        proxy_uri,
                        bypass_hosts,
                    }),
                )
                .await?
            {
                agent_response::Payload::State(state)
                    if state.phase == agent_v1::AgentPhase::Active as i32 =>
                {
                    Ok(pipe)
                }
                agent_response::Payload::State(state) => {
                    Err(WindowsVpnError::UnexpectedAgentPhase(state.phase))
                }
                payload => Err(WindowsVpnError::UnexpectedResponse(payload_name(&payload))),
            }
        })
        .await
        .map_err(|_| WindowsVpnError::RpcTimeout)?
    }

    async fn restore_system_proxy(
        &self,
        pipe: &mut NamedPipeClient,
        operation_id: Uuid,
    ) -> Result<AgentState, WindowsVpnError> {
        match self
            .exchange(
                pipe,
                agent_request::Payload::RestoreSystemProxy(RestoreSystemProxyRequest {
                    operation_id: operation_id.to_string(),
                }),
            )
            .await?
        {
            agent_response::Payload::State(state)
                if state.phase == agent_v1::AgentPhase::Clean as i32 =>
            {
                Ok(state)
            }
            agent_response::Payload::State(state) => {
                Err(WindowsVpnError::UnexpectedAgentPhase(state.phase))
            }
            payload => Err(WindowsVpnError::UnexpectedResponse(payload_name(&payload))),
        }
    }

    async fn call(
        &self,
        payload: agent_request::Payload,
    ) -> Result<agent_response::Payload, WindowsVpnError> {
        timeout(AGENT_RPC_TIMEOUT, self.call_inner(payload))
            .await
            .map_err(|_| WindowsVpnError::RpcTimeout)?
    }

    async fn call_inner(
        &self,
        payload: agent_request::Payload,
    ) -> Result<agent_response::Payload, WindowsVpnError> {
        let mut pipe = self.open_pipe().await?;
        self.exchange(&mut pipe, payload).await
    }

    async fn exchange(
        &self,
        pipe: &mut NamedPipeClient,
        payload: agent_request::Payload,
    ) -> Result<agent_response::Payload, WindowsVpnError> {
        let request_id = Uuid::new_v4().to_string();
        let request = AgentRequest {
            request_id: request_id.clone(),
            protocol_version: AGENT_PROTOCOL_VERSION,
            payload: Some(payload),
        };
        let encoded = encode_frame(&request)?;
        if encoded.len() > MAX_AGENT_FRAME_BYTES + 4 {
            return Err(WindowsVpnError::FrameTooLarge(encoded.len() - 4));
        }
        pipe.write_all(&encoded).await?;

        let mut header = [0_u8; 4];
        pipe.read_exact(&mut header).await?;
        let declared = u32::from_be_bytes(header) as usize;
        if declared > MAX_AGENT_FRAME_BYTES {
            return Err(WindowsVpnError::FrameTooLarge(declared));
        }
        let mut payload = vec![0_u8; declared];
        pipe.read_exact(&mut payload).await?;
        let mut frame = BytesMut::from(header.as_slice());
        frame.extend_from_slice(&payload);
        let response: AgentResponse = decode_frame(frame.freeze())?;
        if response.request_id != request_id {
            return Err(WindowsVpnError::ResponseIdMismatch);
        }
        if let Some(error) = response.error {
            return Err(WindowsVpnError::Remote {
                code: error.code,
                message: error.message,
                retryable: error.retryable,
            });
        }
        response.payload.ok_or(WindowsVpnError::MissingResponse)
    }

    async fn open_pipe(&self) -> Result<NamedPipeClient, WindowsVpnError> {
        let deadline = tokio::time::Instant::now() + AGENT_CONNECT_TIMEOUT;
        loop {
            match ClientOptions::new().open(self.pipe_name.as_ref()) {
                Ok(pipe) => return Ok(pipe),
                Err(error)
                    if matches!(
                        error.raw_os_error().map(|value| value as u32),
                        Some(ERROR_FILE_NOT_FOUND | ERROR_PIPE_BUSY)
                    ) && tokio::time::Instant::now() < deadline =>
                {
                    tokio::time::sleep(Duration::from_millis(50)).await;
                }
                Err(error) => return Err(error.into()),
            }
        }
    }
}

fn payload_name(payload: &agent_response::Payload) -> &'static str {
    match payload {
        agent_response::Payload::Empty(_) => "empty",
        agent_response::Payload::Capabilities(_) => "capabilities",
        agent_response::Payload::State(_) => "state",
        agent_response::Payload::PacketSession(_) => "packet_session",
    }
}

struct PacketSessionMapping {
    _mapping: OwnedHandle,
    engine_to_agent_event: OwnedHandle,
    agent_to_engine_event: OwnedHandle,
    shutdown_event: OwnedHandle,
    view: MappedView,
    ring: SharedPacketRing,
}

// SAFETY: owned kernel handles and mapping view are process-scoped; the ring
// uses atomics with SPSC ownership by protocol contract.
unsafe impl Send for PacketSessionMapping {}
// SAFETY: `&PacketSessionMapping` is safe to share: HANDLE fields are immutable
// after attach, kernel waits/signals are thread-safe, and the ring is SPSC with
// atomic indices (no thread-affine interior mutability).
unsafe impl Sync for PacketSessionMapping {}

impl PacketSessionMapping {
    fn attach(handles: PacketSessionHandles) -> Result<Self, WindowsVpnError> {
        if handles.layout_version != PACKET_RING_LAYOUT_VERSION {
            return Err(WindowsVpnError::PacketLayoutVersion(handles.layout_version));
        }
        let mapped_bytes = SharedPacketRing::mapped_bytes(handles.ring_capacity)?;
        let mapping = OwnedHandle::from_wire(handles.mapping_handle, "mapping")?;
        let engine_to_agent_event = OwnedHandle::from_wire(
            handles.engine_to_agent_event_handle,
            "engine_to_agent_event",
        )?;
        let agent_to_engine_event = OwnedHandle::from_wire(
            handles.agent_to_engine_event_handle,
            "agent_to_engine_event",
        )?;
        let shutdown_event =
            OwnedHandle::from_wire(handles.shutdown_event_handle, "shutdown_event")?;
        // SAFETY: the authenticated Agent duplicated a live mapping handle into
        // this process and declared a size checked by the shared layout.
        let address = unsafe { MapViewOfFile(mapping.0, FILE_MAP_ALL_ACCESS, 0, 0, mapped_bytes) };
        let view = MappedView::new(address)?;
        // SAFETY: the view is page-aligned, remains owned by this object, and
        // was initialized by the matching Agent packet-ring implementation.
        let ring = unsafe { SharedPacketRing::attach(view.pointer(), mapped_bytes) }?;
        if ring.capacity() != handles.ring_capacity {
            return Err(WindowsVpnError::PacketCapacityMismatch);
        }
        Ok(Self {
            _mapping: mapping,
            engine_to_agent_event,
            agent_to_engine_event,
            shutdown_event,
            view,
            ring,
        })
    }

    fn ring(&self) -> SharedPacketRing {
        debug_assert!(!self.view.address.Value.is_null());
        self.ring
    }

    fn signal_engine_to_agent(&self) -> Result<(), WindowsVpnError> {
        // SAFETY: this object owns the live event handle.
        if unsafe { SetEvent(self.engine_to_agent_event.0) } == 0 {
            Err(WindowsVpnError::Io(last_error("SetEvent(engine_to_agent)")))
        } else {
            Ok(())
        }
    }

    fn signal_shutdown(&self) {
        // SAFETY: this object owns the live manual-reset event handle.
        unsafe {
            SetEvent(self.shutdown_event.0);
        }
    }
}

impl Drop for PacketSessionMapping {
    fn drop(&mut self) {
        self.signal_shutdown();
    }
}

struct OwnedHandle(HANDLE);

// SAFETY: uniquely owned Windows kernel handle; CloseHandle is thread-safe.
unsafe impl Send for OwnedHandle {}
// SAFETY: `&OwnedHandle` is safe to share: the HANDLE value is immutable after
// construction, kernel object ops are thread-safe, and Drop still closes once.
unsafe impl Sync for OwnedHandle {}

impl OwnedHandle {
    fn from_wire(value: u64, name: &'static str) -> Result<Self, WindowsVpnError> {
        let value =
            usize::try_from(value).map_err(|_| WindowsVpnError::InvalidHandle(name))? as HANDLE;
        if value.is_null() {
            Err(WindowsVpnError::InvalidHandle(name))
        } else {
            Ok(Self(value))
        }
    }
}

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: the authenticated Agent duplicated this uniquely owned
            // kernel handle into the Engine process.
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
}

struct MappedView {
    address: MEMORY_MAPPED_VIEW_ADDRESS,
}

// SAFETY: mapped view is process memory with no thread-affine state; unique
// ownership unmaps exactly once on drop.
unsafe impl Send for MappedView {}
// SAFETY: `&MappedView` is safe to share: the base address is immutable after
// MapViewOfFile, and concurrent byte access is coordinated by SharedPacketRing.
unsafe impl Sync for MappedView {}

impl MappedView {
    fn new(address: MEMORY_MAPPED_VIEW_ADDRESS) -> Result<Self, WindowsVpnError> {
        if address.Value.is_null() {
            Err(WindowsVpnError::Io(last_error("MapViewOfFile")))
        } else {
            Ok(Self { address })
        }
    }

    fn pointer(&self) -> NonNull<u8> {
        NonNull::new(self.address.Value.cast()).expect("validated mapping")
    }
}

impl Drop for MappedView {
    fn drop(&mut self) {
        if !self.address.Value.is_null() {
            // SAFETY: this object uniquely owns the mapped view.
            unsafe {
                UnmapViewOfFile(self.address);
            }
        }
    }
}

fn last_error(operation: &'static str) -> io::Error {
    io::Error::other(format!("{operation}: {}", io::Error::last_os_error()))
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum WindowsVpnError {
    #[error("Windows Agent I/O failed: {0}")]
    Io(#[from] io::Error),
    #[error("Windows Agent protobuf frame failed: {0}")]
    Frame(#[from] usque_ipc::FrameError),
    #[error("Windows Agent RPC timed out")]
    RpcTimeout,
    #[error("Windows Agent frame exceeds 64 KiB: {0}")]
    FrameTooLarge(usize),
    #[error("the Cloudflare registration API could not be resolved before enabling VPN: {0}")]
    ControlEndpointResolution(String),
    #[error("Windows Agent response request ID does not match")]
    ResponseIdMismatch,
    #[error("Windows Agent response has no payload")]
    MissingResponse,
    #[error("Windows Agent returned unexpected payload {0}")]
    UnexpectedResponse(&'static str),
    #[error("Windows Agent returned unexpected phase {0}")]
    UnexpectedAgentPhase(i32),
    #[error("Windows Agent protocol version {0} is unsupported")]
    ProtocolVersion(u32),
    #[error("Windows Agent returned a malformed active operation ID")]
    InvalidAgentOperationId,
    #[error(
        "Windows Agent active tunnel belongs to Profile {active}, not requested Profile {requested}"
    )]
    ActiveProfileMismatch { active: String, requested: Uuid },
    #[error("Windows Agent is missing required capabilities: {0}")]
    MissingCapabilities(String),
    #[error("Windows system proxy requires a Loopback listener, got {0}")]
    InvalidSystemProxyListener(std::net::SocketAddr),
    #[error(
        "Windows Agent has persistent recovery state (phase {phase}, operation {operation_id}); explicit recovery is required"
    )]
    RecoveryRequired { phase: i32, operation_id: String },
    #[error("Windows Agent rejected the operation ({code}, retryable={retryable}): {message}")]
    Remote {
        code: String,
        message: String,
        retryable: bool,
    },
    #[error("Windows Agent returned an invalid {0} handle")]
    InvalidHandle(&'static str),
    #[error("Windows Agent packet layout version {0} is unsupported")]
    PacketLayoutVersion(u32),
    #[error("Windows Agent packet-ring capacity does not match its mapped header")]
    PacketCapacityMismatch,
    #[error("Windows packet ring failed: {0}")]
    PacketRing(#[from] PacketRingError),
    #[error("Windows packet wait returned unexpected status {0}")]
    UnexpectedWait(u32),
    #[error("MASQUE transport failed: {0}")]
    Transport(#[from] TransportError),
}

#[cfg(test)]
mod tests {
    use std::net::{Ipv4Addr, Ipv6Addr};

    use tokio::net::windows::named_pipe::ServerOptions;
    use usque_core::{MasqueKeyPair, OperatingMode};

    use super::*;

    fn identity() -> MasqueTlsIdentity {
        let identity_key = MasqueKeyPair::generate();
        let endpoint_key = MasqueKeyPair::generate();
        MasqueTlsIdentity::new(
            identity_key.private_sec1_der().expect("SEC1"),
            &endpoint_key.public_spki_der().expect("SPKI"),
            Ipv4Addr::new(172, 16, 0, 2),
            "2606:4700:110::2".parse::<Ipv6Addr>().expect("IPv6"),
        )
        .expect("identity")
    }

    #[test]
    fn tunnel_plan_contains_both_happy_eyeballs_candidates() {
        let mut profile = Profile {
            mode: OperatingMode::Vpn,
            ..Profile::default()
        };
        profile.ip_policy = IpPolicy::PreferIpv6;
        let plan = tunnel_plan(
            &profile,
            &identity(),
            &["198.51.100.10:443".parse().unwrap()],
        );
        assert_eq!(plan.endpoint, "[2606:4700:103::2]:443");
        assert_eq!(
            plan.endpoint_candidates,
            ["162.159.198.2:443", "[2606:4700:103::2]:443"]
        );
        assert_eq!(plan.control_api_candidates, ["198.51.100.10:443"]);
        assert_eq!(plan.assigned_ipv4, "172.16.0.2/32");
        assert_eq!(plan.assigned_ipv6, "2606:4700:110::2/128");
    }

    #[test]
    fn tunnel_plan_filters_dns_to_the_enabled_tunnel_family() {
        let identity_key = MasqueKeyPair::generate();
        let endpoint_key = MasqueKeyPair::generate();
        let identity = MasqueTlsIdentity::new(
            identity_key.private_sec1_der().unwrap(),
            &endpoint_key.public_spki_der().unwrap(),
            "172.16.0.2".parse().unwrap(),
            "2606:4700:110::2".parse().unwrap(),
        )
        .unwrap();
        let profile = Profile {
            ip_policy: IpPolicy::Ipv4Only,
            ..Profile::default()
        };

        let plan = tunnel_plan(&profile, &identity, &["198.51.100.10:443".parse().unwrap()]);

        assert_eq!(plan.dns_servers, vec!["1.1.1.1"]);
    }

    #[test]
    fn single_family_policy_limits_agent_bypass_and_wfp_candidates() {
        let profile = Profile {
            ip_policy: IpPolicy::Ipv4Only,
            ..Profile::default()
        };
        let plan = tunnel_plan(
            &profile,
            &identity(),
            &["198.51.100.10:443".parse().unwrap()],
        );
        assert_eq!(plan.endpoint_candidates, ["162.159.198.2:443"]);
        assert_eq!(plan.control_api_candidates, ["198.51.100.10:443"]);
    }

    #[tokio::test]
    async fn agent_client_rejects_a_response_id_alias() {
        let pipe_name = format!("{AGENT_PIPE_NAME}.test-{}", Uuid::new_v4());
        let server = ServerOptions::new()
            .first_pipe_instance(true)
            .create(&pipe_name)
            .expect("server");
        let server_task = tokio::spawn(async move {
            server.connect().await.expect("connect");
            let mut server = server;
            let mut header = [0_u8; 4];
            server.read_exact(&mut header).await.expect("header");
            let mut payload = vec![0_u8; u32::from_be_bytes(header) as usize];
            server.read_exact(&mut payload).await.expect("payload");
            let response = AgentResponse {
                request_id: "different-request".to_owned(),
                error: None,
                payload: Some(agent_response::Payload::Capabilities(
                    AgentCapabilities::default(),
                )),
            };
            server
                .write_all(&encode_frame(&response).expect("encode"))
                .await
                .expect("write");
        });
        let client = WindowsAgentClient::for_test(pipe_name);
        assert!(matches!(
            client.get_capabilities().await,
            Err(WindowsVpnError::ResponseIdMismatch)
        ));
        server_task.await.expect("server task");
    }
}
