use std::{
    io, mem,
    net::SocketAddr,
    ptr::{self, NonNull},
    sync::{
        Arc, RwLock, Weak,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use async_trait::async_trait;
use bytes::{Bytes, BytesMut};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::windows::named_pipe::{ClientOptions, NamedPipeClient},
    sync::{mpsc, watch},
    task::JoinHandle,
    time::{sleep, timeout},
};
use tokio_util::sync::CancellationToken;
use usque_core::{
    IpPolicy, Profile, REGISTRATION_API_HOST, REGISTRATION_API_PORT, TransportFailure,
    TransportFailureCode, TransportStage,
};
use usque_ipc::{
    agent_v1::{
        self, AcquireDirectEgressRequest, AcquireTunnelLeaseRequest, AgentCapabilities,
        AgentRequest, AgentResponse, AgentState, ApplySystemProxyRequest,
        ClosePacketSessionRequest, CommitTunnelRequest,
        DirectEgressLease as AgentDirectEgressLease, GetCapabilitiesRequest,
        GetPhysicalNetworkInfoRequest, GetStateRequest, InspectPlatformStateRequest,
        OpenPacketSessionRequest, PacketSessionHandles, PhysicalNetworkInfo, PlatformState,
        PrepareTunnelRequest, RestoreSystemProxyRequest, ResumeTunnelRequest,
        RollbackTunnelRequest, agent_request, agent_response,
    },
    decode_frame, encode_frame,
};
use usque_platform::packet_ring::{
    PACKET_RING_LAYOUT_VERSION, PacketDirection, PacketRingError, SharedPacketRing,
};
use usque_transport::{
    ConnectionTimelineSnapshot, DirectEgressLease, DirectProtocol, EndpointPinRefresher,
    GeoDirectPolicy, ManagedTunnelMonitor, MasqueRuntime, MasqueTlsIdentity, MasqueTunIo,
    NoopSocketProtector, RuntimeHealth, RuntimePath, SPLIT_DNS_IPV4, SPLIT_DNS_IPV6,
    STALE_GENERATION_REASON, SocketHandle, SocketProtector, TrafficSnapshot, TransportError,
    resolve_physical_host,
};
use uuid::Uuid;
use windows_sys::Win32::{
    Foundation::{
        CloseHandle, ERROR_ACCESS_DENIED, ERROR_FILE_NOT_FOUND, ERROR_PIPE_BUSY,
        ERROR_SERVICE_ALREADY_RUNNING, ERROR_SERVICE_DISABLED, ERROR_SERVICE_DOES_NOT_EXIST,
        ERROR_SERVICE_MARKED_FOR_DELETE, ERROR_SERVICE_REQUEST_TIMEOUT, HANDLE, WAIT_FAILED,
        WAIT_OBJECT_0,
    },
    Networking::WinSock::{
        IP_UNICAST_IF, IPPROTO_IP, IPPROTO_IPV6, IPV6_UNICAST_IF, SOCKET_ERROR, WSAGetLastError,
        setsockopt,
    },
    System::{
        Memory::{FILE_MAP_ALL_ACCESS, MEMORY_MAPPED_VIEW_ADDRESS, MapViewOfFile, UnmapViewOfFile},
        Services::{
            CloseServiceHandle, OpenSCManagerW, OpenServiceW, QueryServiceStatusEx, SC_HANDLE,
            SC_MANAGER_CONNECT, SC_STATUS_PROCESS_INFO, SERVICE_PAUSED, SERVICE_QUERY_STATUS,
            SERVICE_RUNNING, SERVICE_START, SERVICE_START_PENDING, SERVICE_STATUS_PROCESS,
            SERVICE_STOP_PENDING, SERVICE_STOPPED, StartServiceW,
        },
        Threading::{INFINITE, SetEvent, WaitForMultipleObjects},
    },
};

const AGENT_PIPE_NAME: &str = r"\\.\pipe\io.github.georgexie2333.usque.agent.v1";
const AGENT_PROTOCOL_VERSION: u32 = 3;
const MAX_AGENT_FRAME_BYTES: usize = 64 * 1024;
const AGENT_START_TIMEOUT: Duration = Duration::from_secs(30);
const AGENT_RPC_TIMEOUT: Duration = Duration::from_secs(30);
const AGENT_START_POLL_INTERVAL: Duration = Duration::from_millis(50);
const AGENT_START_RECHECK_INTERVAL: Duration = Duration::from_millis(250);
const PUMP_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);
const DEFAULT_PACKET_RING_CAPACITY: u32 = 4 * 1024 * 1024;
const PACKET_WAKE_BATCH: usize = 64;
const PACKET_RING_RETRY_INTERVAL: Duration = Duration::from_millis(1);
const PHYSICAL_NETWORK_POLL_INTERVAL: Duration = Duration::from_millis(500);

struct WindowsVpnSocketProtector {
    registration_api: Vec<SocketAddr>,
    agent: WindowsAgentClient,
    operation_id: Uuid,
    physical: RwLock<WindowsPhysicalState>,
    monitor_cancel: CancellationToken,
    proxy_mode: AtomicBool,
}

struct WindowsPhysicalState {
    generation: u64,
    agent_generation: Option<u64>,
    dns_servers: Vec<SocketAddr>,
    family_mask: u32,
}

impl WindowsPhysicalState {
    fn update(&mut self, snapshot: Option<(u64, Vec<SocketAddr>, u32)>) -> bool {
        let (agent_generation, dns_servers, family_mask) = match snapshot {
            Some((generation, servers, mask)) => (Some(generation), servers, mask & 3),
            None => (None, Vec::new(), 0),
        };
        let changed = self.agent_generation != agent_generation
            || self.dns_servers != dns_servers
            || self.family_mask != family_mask;
        if changed {
            self.generation = self.generation.saturating_add(1);
            self.agent_generation = agent_generation;
            self.dns_servers = dns_servers;
            self.family_mask = family_mask;
        }
        changed
    }
}

#[async_trait]
impl SocketProtector for WindowsVpnSocketProtector {
    fn protect(&self, _socket: SocketHandle) -> Result<(), String> {
        Ok(())
    }

    async fn protect_for_target(
        &self,
        socket: SocketHandle,
        remote: SocketAddr,
        protocol: DirectProtocol,
    ) -> Result<DirectEgressLease, String> {
        if self.proxy_mode.load(Ordering::Acquire) {
            return NoopSocketProtector
                .protect_for_target(socket, remote, protocol)
                .await;
        }
        let generation = self
            .network_generation()
            .ok_or_else(|| STALE_GENERATION_REASON.to_owned())?;
        self.protect_target_generation(socket, remote, protocol, generation)
            .await
    }

    async fn protect_for_target_generation(
        &self,
        socket: SocketHandle,
        remote: SocketAddr,
        protocol: DirectProtocol,
        expected_generation: u64,
    ) -> Result<DirectEgressLease, String> {
        self.protect_target_generation(socket, remote, protocol, expected_generation)
            .await
    }

    fn tun_direct_available(&self) -> bool {
        !self.proxy_mode.load(Ordering::Acquire)
    }

    fn network_generation(&self) -> Option<u64> {
        if self.proxy_mode.load(Ordering::Acquire) {
            return None;
        }
        self.physical.read().ok().map(|state| state.generation)
    }

    fn endpoint_family_available(&self, endpoint: SocketAddr) -> Option<bool> {
        if self.proxy_mode.load(Ordering::Acquire) {
            return None;
        }
        self.physical.read().ok().map(|state| {
            state.agent_generation.is_some()
                && state.family_mask & if endpoint.is_ipv4() { 1 } else { 2 } != 0
        })
    }

    fn physical_dns_servers(&self) -> Vec<SocketAddr> {
        if self.proxy_mode.load(Ordering::Acquire) {
            return Vec::new();
        }
        self.physical
            .read()
            .map(|state| state.dns_servers.clone())
            .unwrap_or_default()
    }

    async fn resolve_direct(&self, host: &str, port: u16) -> Result<Vec<SocketAddr>, String> {
        if self.proxy_mode.load(Ordering::Acquire) {
            return NoopSocketProtector.resolve_direct(host, port).await;
        }
        resolve_physical_host(self, host, port).await
    }

    fn resolve(&self, host: &str, port: u16) -> Result<Vec<SocketAddr>, String> {
        if self.proxy_mode.load(Ordering::Acquire) {
            return NoopSocketProtector.resolve(host, port);
        }
        if host == REGISTRATION_API_HOST && port == REGISTRATION_API_PORT {
            Ok(self.registration_api.clone())
        } else {
            Err("the Windows VPN resolver accepts only the pinned registration API host".to_owned())
        }
    }
}

impl WindowsVpnSocketProtector {
    /// Only an acknowledged, fully closed Agent transaction may release the
    /// VPN binding policy. A failed rollback must keep exact-egress checks.
    fn complete_proxy_detach(
        &self,
        state: &AgentState,
        system_proxy_cleanup: Result<(), WindowsVpnError>,
    ) -> Result<(), WindowsVpnError> {
        system_proxy_cleanup?;
        validate_proxy_detach_state(state)?;
        self.proxy_mode.store(true, Ordering::Release);
        self.monitor_cancel.cancel();
        Ok(())
    }

    fn egress_generation(&self, expected: u64) -> Result<Option<u64>, String> {
        if self.proxy_mode.load(Ordering::Acquire) {
            return if expected == 0 {
                Ok(None)
            } else {
                Err(STALE_GENERATION_REASON.to_owned())
            };
        }
        let state = self
            .physical
            .read()
            .map_err(|_| STALE_GENERATION_REASON.to_owned())?;
        if state.generation != expected {
            return Err(STALE_GENERATION_REASON.to_owned());
        }
        state
            .agent_generation
            .map(Some)
            .ok_or_else(|| STALE_GENERATION_REASON.to_owned())
    }

    async fn protect_target_generation(
        &self,
        socket: SocketHandle,
        remote: SocketAddr,
        protocol: DirectProtocol,
        expected_generation: u64,
    ) -> Result<DirectEgressLease, String> {
        let Some(agent_generation) = self.egress_generation(expected_generation)? else {
            return NoopSocketProtector
                .protect_for_target_generation(socket, remote, protocol, expected_generation)
                .await;
        };
        let (pipe, lease) = self
            .agent
            .acquire_direct_egress(self.operation_id, remote, protocol, agent_generation)
            .await
            .map_err(socket_lease_error)?;
        self.verify_generation(expected_generation, agent_generation)?;
        bind_socket_to_interface(socket, remote, lease.interface_index)?;
        let current = self
            .agent
            .get_physical_network_info(self.operation_id)
            .await
            .map_err(socket_lease_error)?;
        self.observe_physical_snapshot(&current);
        let family_mask = if remote.is_ipv4() { 1 } else { 2 };
        if current.generation != agent_generation
            || !current.interfaces.iter().any(|interface| {
                interface.interface_luid == lease.interface_luid
                    && interface.interface_index == lease.interface_index
                    && interface.address_family_mask & family_mask != 0
            })
        {
            return Err(STALE_GENERATION_REASON.to_owned());
        }
        self.verify_generation(expected_generation, agent_generation)?;
        Ok(DirectEgressLease::hold_for_generation(
            pipe,
            expected_generation,
        ))
    }

    fn verify_generation(&self, expected: u64, agent_generation: u64) -> Result<(), String> {
        if self.egress_generation(expected)? != Some(agent_generation) {
            Err(STALE_GENERATION_REASON.to_owned())
        } else {
            Ok(())
        }
    }

    fn observe_physical_snapshot(&self, info: &PhysicalNetworkInfo) {
        if self.proxy_mode.load(Ordering::Acquire) {
            return;
        }
        let snapshot = physical_dns_endpoints(info).ok().map(|servers| {
            (
                info.generation,
                servers,
                info.interfaces
                    .iter()
                    .fold(0, |mask, interface| mask | interface.address_family_mask),
            )
        });
        if let Ok(mut state) = self.physical.write() {
            state.update(snapshot);
        }
    }
}

impl Drop for WindowsVpnSocketProtector {
    fn drop(&mut self) {
        self.monitor_cancel.cancel();
    }
}

fn validate_proxy_detach_state(state: &AgentState) -> Result<(), WindowsVpnError> {
    if state.phase != agent_v1::AgentPhase::Clean as i32 || state.packet_session_active {
        return Err(WindowsVpnError::RecoveryRequired {
            phase: state.phase,
            operation_id: state.operation_id.clone(),
        });
    }
    Ok(())
}

fn socket_lease_error(error: WindowsVpnError) -> String {
    match error {
        WindowsVpnError::Remote { code, .. } if code == "AGENT_STALE_GENERATION" => {
            STALE_GENERATION_REASON.to_owned()
        }
        _ => "Windows exact-generation egress preparation failed".to_owned(),
    }
}

fn start_physical_network_monitor(protector: &Arc<WindowsVpnSocketProtector>) {
    let cancellation = protector.monitor_cancel.clone();
    let weak_protector = Arc::downgrade(protector);
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(PHYSICAL_NETWORK_POLL_INTERVAL);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        // The initial snapshot was read synchronously during startup.
        ticker.tick().await;
        loop {
            tokio::select! {
                _ = cancellation.cancelled() => break,
                _ = ticker.tick() => {}
            }
            let Some(protector) = Weak::upgrade(&weak_protector) else {
                break;
            };
            let agent = protector.agent.clone();
            let operation_id = protector.operation_id;
            drop(protector);
            let update = tokio::select! {
                _ = cancellation.cancelled() => break,
                result = agent.get_physical_network_info(operation_id) => result,
            };
            let Some(protector) = Weak::upgrade(&weak_protector) else {
                break;
            };
            match update {
                Ok(info) => protector.observe_physical_snapshot(&info),
                Err(_) => {
                    if let Ok(mut state) = protector.physical.write()
                        && state.update(None)
                    {
                        tracing::warn!(
                            reason_code = "physical_snapshot_unavailable",
                            "Windows physical network snapshot became unavailable"
                        );
                    }
                }
            }
        }
    });
}

fn bind_socket_to_interface(
    socket: SocketHandle,
    remote: SocketAddr,
    interface_index: u32,
) -> Result<(), String> {
    if interface_index == 0 {
        return Err("Agent returned an empty physical interface index".to_owned());
    }
    let value = if remote.is_ipv4() {
        interface_index.to_be()
    } else {
        interface_index
    };
    // SAFETY: the socket handle is live, `value` remains valid for the
    // synchronous call, and the option size exactly matches its storage.
    let result = unsafe {
        setsockopt(
            socket.value() as usize,
            if remote.is_ipv4() {
                IPPROTO_IP
            } else {
                IPPROTO_IPV6
            },
            if remote.is_ipv4() {
                IP_UNICAST_IF
            } else {
                IPV6_UNICAST_IF
            },
            (&raw const value).cast(),
            mem::size_of_val(&value) as i32,
        )
    };
    if result == SOCKET_ERROR {
        // SAFETY: this reads the calling thread's Winsock error immediately
        // after the failed `setsockopt` call and has no pointer preconditions.
        let error = unsafe { WSAGetLastError() };
        Err(format!(
            "bind socket to physical interface {interface_index}: WSA {}",
            error
        ))
    } else {
        Ok(())
    }
}

fn physical_dns_endpoints(info: &PhysicalNetworkInfo) -> Result<Vec<SocketAddr>, WindowsVpnError> {
    if info.generation == 0 || info.interfaces.is_empty() || info.interfaces.len() > 2 {
        return Err(WindowsVpnError::InvalidPhysicalNetworkInfo);
    }
    let mut output = Vec::new();
    for interface in &info.interfaces {
        if interface.interface_luid == 0
            || interface.interface_index == 0
            || interface.address_family_mask == 0
            || interface.address_family_mask & !3 != 0
            || interface.dns_servers.len() > 8
        {
            return Err(WindowsVpnError::InvalidPhysicalNetworkInfo);
        }
        for value in &interface.dns_servers {
            let address = value
                .parse::<std::net::IpAddr>()
                .map_err(|_| WindowsVpnError::InvalidPhysicalNetworkInfo)?;
            if address.is_unspecified() || address.is_multicast() {
                return Err(WindowsVpnError::InvalidPhysicalNetworkInfo);
            }
            let endpoint = match address {
                std::net::IpAddr::V4(address) => SocketAddr::from((address, 53)),
                std::net::IpAddr::V6(address) => std::net::SocketAddrV6::new(
                    address,
                    53,
                    0,
                    if address.is_unicast_link_local() {
                        interface.interface_index
                    } else {
                        0
                    },
                )
                .into(),
            };
            output.push(endpoint);
        }
    }
    output.sort();
    output.dedup();
    Ok(output)
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
    tunnel_lease: bool,
}

impl WindowsSystemProxyGuard {
    pub(crate) async fn start(listener: std::net::SocketAddr) -> Result<Self, WindowsVpnError> {
        Self::start_internal(listener, None).await
    }

    pub(crate) async fn start_for_tunnel(
        listener: std::net::SocketAddr,
        operation_id: Uuid,
    ) -> Result<Self, WindowsVpnError> {
        Self::start_internal(listener, Some(operation_id)).await
    }

    async fn start_internal(
        listener: std::net::SocketAddr,
        tunnel_operation_id: Option<Uuid>,
    ) -> Result<Self, WindowsVpnError> {
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
        let operation_id = match tunnel_operation_id {
            Some(operation_id) => operation_id,
            None => {
                let state = client.get_state().await?;
                if state.phase != agent_v1::AgentPhase::Clean as i32 {
                    return Err(WindowsVpnError::RecoveryRequired {
                        phase: state.phase,
                        operation_id: state.operation_id,
                    });
                }
                Uuid::new_v4()
            }
        };
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
            tunnel_lease: tunnel_operation_id.is_some(),
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
        let state = result?;
        if system_proxy_restore_succeeded(self.tunnel_lease, self.operation_id, &state) {
            Ok(())
        } else {
            Err(WindowsVpnError::UnexpectedAgentPhase(state.phase))
        }
    }

    pub(crate) async fn shutdown_slot(slot: &mut Option<Self>) -> Result<(), WindowsVpnError> {
        match slot.take() {
            Some(mut previous) => previous.shutdown().await,
            None => Ok(()),
        }
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
    listeners: Vec<SocketAddr>,
    socks5_listeners: Vec<SocketAddr>,
    http_listeners: Vec<SocketAddr>,
    system_proxy: Option<WindowsSystemProxyGuard>,
    transaction_open: bool,
    tunnel: Option<MasqueRuntime>,
    // Present when this runtime created the VPN-bound MASQUE protector.
    socket_protector: Option<Arc<WindowsVpnSocketProtector>>,
    // Retained after rollback succeeds even if another cleanup step fails.
    // A retry may consume this acknowledgement, but it grants no host egress.
    proxy_detach_state: Option<AgentState>,
}

#[derive(Clone)]
struct WindowsPumpFailure {
    message: String,
    failure: TransportFailure,
}

impl WindowsPumpFailure {
    fn agent(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            failure: TransportFailure::new(
                TransportFailureCode::AgentUnreachable,
                TransportStage::PlatformRecovery,
            ),
        }
    }

    fn transport(context: &str, error: &TransportError, path: RuntimePath) -> Self {
        Self {
            message: format!("{context}: {error}"),
            failure: error.failure(Some(path.transport), Some(path.endpoint_family)),
        }
    }
}

#[derive(Clone)]
pub(crate) struct WindowsVpnMonitor {
    tunnel: ManagedTunnelMonitor,
    pump_failure: watch::Receiver<Option<WindowsPumpFailure>>,
    agent_disconnected: watch::Receiver<bool>,
}

impl WindowsVpnMonitor {
    pub(crate) fn path(&self) -> RuntimePath {
        self.tunnel.path()
    }

    pub(crate) fn health(&self) -> RuntimeHealth {
        if let Some(pump_failure) = self.pump_failure.borrow().clone() {
            let transport = self.tunnel.health();
            let path = transport.path();
            let failure = if pump_failure.failure.transport.is_none()
                || pump_failure.failure.address_family.is_none()
            {
                pump_failure
                    .failure
                    .on_path(path.transport, path.endpoint_family)
            } else {
                pump_failure.failure
            };
            RuntimeHealth::Failed {
                last_path: path,
                reconnect_count: transport.reconnect_count(),
                message: pump_failure.message,
                failure,
            }
        } else {
            self.tunnel.health()
        }
    }

    pub(crate) fn statistics(&self) -> TrafficSnapshot {
        self.tunnel.statistics()
    }

    pub(crate) fn connection_timeline(&self) -> ConnectionTimelineSnapshot {
        self.tunnel.connection_timeline()
    }

    pub(crate) fn failure(&self) -> Option<String> {
        self.pump_failure
            .borrow()
            .as_ref()
            .map(|failure| failure.message.clone())
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
        geo_policy: Arc<GeoDirectPolicy>,
    ) -> Result<Self, WindowsVpnError> {
        // Resolve before the Agent installs the fail-closed WFP policy. The
        // authenticated refresh client later uses only these exact numeric
        // addresses, so neither DNS nor arbitrary physical egress is opened
        // while the tunnel is active.
        let registration_api = resolve_registration_api().await?;
        let geo_enabled = geo_policy.is_enabled();
        let agent = WindowsAgentClient::production();
        let capabilities = agent.get_capabilities().await?;
        validate_capabilities(&capabilities, profile.kill_switch)?;
        let state = agent.get_state().await?;
        let (operation_id, resuming, startup_lease) =
            match agent_v1::AgentPhase::try_from(state.phase) {
                Ok(agent_v1::AgentPhase::Clean) => {
                    let operation_id = Uuid::new_v4();
                    let plan = tunnel_plan(profile, &identity, &registration_api, geo_enabled);
                    let lease = agent.prepare(operation_id, plan).await?;
                    (operation_id, false, Some(lease))
                }
                Ok(agent_v1::AgentPhase::Active) if state.profile_id == profile.id.to_string() => {
                    let operation_id = Uuid::parse_str(&state.operation_id)
                        .map_err(|_| WindowsVpnError::InvalidAgentOperationId)?;
                    (operation_id, true, None)
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

        let physical_info = match agent.get_physical_network_info(operation_id).await {
            Ok(info) => info,
            Err(error) => {
                abort_startup(
                    &agent,
                    operation_id,
                    resuming,
                    "PHYSICAL_NETWORK_SNAPSHOT_FAILED",
                )
                .await?;
                return Err(error);
            }
        };
        let physical_dns = match physical_dns_endpoints(&physical_info) {
            Ok(servers) => servers,
            Err(error) => {
                abort_startup(
                    &agent,
                    operation_id,
                    resuming,
                    "PHYSICAL_DNS_SNAPSHOT_INVALID",
                )
                .await?;
                return Err(error);
            }
        };
        if geo_enabled
            && profile.direct_dns.mode == usque_core::DirectDnsMode::PhysicalSystem
            && physical_dns.is_empty()
        {
            abort_startup(&agent, operation_id, resuming, "PHYSICAL_DNS_UNAVAILABLE").await?;
            return Err(WindowsVpnError::PhysicalDnsUnavailable);
        }
        let initial_generation = physical_info.generation;
        let protector = Arc::new(WindowsVpnSocketProtector {
            registration_api,
            agent: agent.clone(),
            operation_id,
            physical: RwLock::new(WindowsPhysicalState {
                generation: initial_generation,
                agent_generation: Some(initial_generation),
                dns_servers: physical_dns,
                family_mask: physical_info
                    .interfaces
                    .iter()
                    .fold(0, |mask, interface| mask | interface.address_family_mask),
            }),
            monitor_cancel: CancellationToken::new(),
            proxy_mode: AtomicBool::new(false),
        });
        start_physical_network_monitor(&protector);
        let transport_protector: Arc<dyn SocketProtector> = protector.clone();

        let tunnel = match MasqueRuntime::start_with_geo_policy(
            profile,
            identity,
            transport_protector,
            Some(pin_refresher),
            geo_policy,
        )
        .await
        {
            Ok(tunnel) => tunnel,
            Err(error) => {
                abort_startup(&agent, operation_id, resuming, "TRANSPORT_START_FAILED").await?;
                return Err(error.into());
            }
        };
        match bind_agent_session(
            profile,
            tunnel,
            agent,
            operation_id,
            resuming,
            startup_lease,
        )
        .await
        {
            Ok(mut runtime) => {
                runtime.socket_protector = Some(protector);
                Ok(runtime)
            }
            Err((mut tunnel, error)) => {
                tunnel.shutdown().await;
                Err(error)
            }
        }
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

    pub(crate) fn connection_timeline(&self) -> ConnectionTimelineSnapshot {
        self.monitor.connection_timeline()
    }

    pub(crate) fn subscribe_network_quality(
        &self,
    ) -> tokio::sync::watch::Receiver<usque_transport::NetworkQualitySnapshot> {
        self.monitor.tunnel.subscribe_network_quality()
    }

    pub(crate) fn diagnostic_dns_context(
        &self,
    ) -> Option<(Arc<dyn SocketProtector>, CancellationToken)> {
        self.tunnel
            .as_ref()
            .map(MasqueRuntime::diagnostic_dns_context)
    }

    pub(crate) fn failure(&self) -> Option<String> {
        self.monitor.failure()
    }

    pub(crate) fn listeners(&self) -> &[SocketAddr] {
        &self.listeners
    }

    pub(crate) fn socks5_listeners(&self) -> &[SocketAddr] {
        &self.socks5_listeners
    }

    pub(crate) fn http_listeners(&self) -> &[SocketAddr] {
        &self.http_listeners
    }

    pub(crate) async fn reconfigure_frontends(
        &mut self,
        profile: &Profile,
    ) -> Result<(), WindowsVpnError> {
        let tunnel = self
            .tunnel
            .as_mut()
            .ok_or(WindowsVpnError::MissingMasqueRuntime)?;
        tunnel.reconfigure_frontends(profile).await?;
        self.listeners = tunnel.listeners().to_vec();
        self.socks5_listeners = tunnel.socks5_listeners().to_vec();
        self.http_listeners = tunnel.http_listeners().to_vec();
        Ok(())
    }

    /// Wrap an already-running MASQUE session with Wintun/WFP. On failure the
    /// caller receives the live MASQUE runtime back so SOCKS/HTTP survive.
    pub(crate) async fn attach_existing(
        profile: &Profile,
        tunnel: MasqueRuntime,
    ) -> Result<Self, (MasqueRuntime, WindowsVpnError)> {
        let registration_api = match resolve_registration_api().await {
            Ok(addresses) => addresses,
            Err(error) => return Err((tunnel, error)),
        };
        let agent = WindowsAgentClient::production();
        let capabilities = match agent.get_capabilities().await {
            Ok(capabilities) => capabilities,
            Err(error) => return Err((tunnel, error)),
        };
        if let Err(error) = validate_capabilities(&capabilities, profile.kill_switch) {
            return Err((tunnel, error));
        }
        let state = match agent.get_state().await {
            Ok(state) => state,
            Err(error) => return Err((tunnel, error)),
        };
        if state.phase != agent_v1::AgentPhase::Clean as i32 {
            return Err((
                tunnel,
                WindowsVpnError::RecoveryRequired {
                    phase: state.phase,
                    operation_id: state.operation_id,
                },
            ));
        }
        let operation_id = Uuid::new_v4();
        let plan = tunnel_plan_from_assignment(
            profile,
            tunnel.assigned_ipv4(),
            tunnel.assigned_ipv6(),
            &registration_api,
            false,
        );
        let startup_lease = match agent.prepare(operation_id, plan).await {
            Ok(lease) => lease,
            Err(error) => return Err((tunnel, error)),
        };
        bind_agent_session(
            profile,
            tunnel,
            agent,
            operation_id,
            false,
            Some(startup_lease),
        )
        .await
    }

    /// Tear down Wintun/WFP and return the live MASQUE session.
    pub(crate) async fn detach_into_masque(&mut self) -> Result<MasqueRuntime, WindowsVpnError> {
        if self.tunnel.is_none() {
            return Err(WindowsVpnError::MissingMasqueRuntime);
        }
        self.stop_packet_pumps().await;
        if let Some(tunnel) = self.tunnel.as_mut() {
            tunnel.detach_tun();
        }
        let system_proxy_result = match self.system_proxy.as_mut() {
            Some(system_proxy) => system_proxy.shutdown().await,
            None => Ok(()),
        };
        self.system_proxy = None;
        let rollback = if self.transaction_open {
            self.agent
                .rollback(self.operation_id, "HOT_TUNNEL_DETACH")
                .await
                .and_then(|state| {
                    validate_proxy_detach_state(&state)?;
                    Ok(state)
                })
        } else {
            self.proxy_detach_state
                .clone()
                .ok_or_else(|| WindowsVpnError::RecoveryRequired {
                    phase: agent_v1::AgentPhase::RecoveryRequired as i32,
                    operation_id: self.operation_id.to_string(),
                })
        };
        if let Ok(state) = &rollback {
            self.transaction_open = false;
            self.proxy_detach_state = Some(state.clone());
        }
        let state = rollback?;
        // Failure retains the Vpn runtime/profile in the caller. Never enable
        // ordinary host egress unless the complete detach can return success.
        if let Some(protector) = &self.socket_protector {
            protector.complete_proxy_detach(&state, system_proxy_result)?;
        } else {
            system_proxy_result?;
        }
        self.tunnel
            .take()
            .ok_or(WindowsVpnError::MissingMasqueRuntime)
    }

    pub(crate) async fn replace_system_proxy(
        &mut self,
        profile: &Profile,
    ) -> Result<(), WindowsVpnError> {
        WindowsSystemProxyGuard::shutdown_slot(&mut self.system_proxy).await?;
        self.system_proxy = if profile.frontends.http && profile.proxy.system_proxy {
            let listener = loopback_http_listener(&self.http_listeners)
                .ok_or(WindowsVpnError::MissingSystemProxyListener)?;
            Some(WindowsSystemProxyGuard::start_for_tunnel(listener, self.operation_id).await?)
        } else {
            None
        };
        Ok(())
    }

    pub(crate) fn system_proxy_active(&self) -> bool {
        self.system_proxy.is_some()
    }

    pub(crate) fn requires_agent_reattach(&self) -> bool {
        self.monitor.agent_disconnected()
    }

    pub(crate) async fn detach_for_agent_reattach(&mut self) -> Result<(), WindowsVpnError> {
        self.stop_packet_pumps().await;
        if let Some(tunnel) = self.tunnel.as_mut() {
            tunnel.detach_tun();
        }
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
        if let Some(mut tunnel) = self.tunnel.take() {
            tunnel.shutdown().await;
        }
        // The replacement runtime must adopt the same persistent transaction.
        // Drop must therefore not perform a rollback between detach and resume.
        self.transaction_open = false;
        Ok(())
    }

    pub(crate) async fn shutdown(&mut self) -> Result<(), WindowsVpnError> {
        // Cut packet forwarding before any Agent RPC. Rollback may need to
        // restore routes, DNS, WFP, and the adapter, but no user packet may
        // remain attached to MASQUE while that cleanup is in progress.
        self.cancel_immediately();
        let system_proxy_result = match self.system_proxy.as_mut() {
            Some(system_proxy) => system_proxy.shutdown().await,
            None => Ok(()),
        };
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
        if let Some(mut tunnel) = self.tunnel.take() {
            tunnel.shutdown().await;
        }
        system_proxy_result?;
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

pub(crate) fn loopback_http_listener(listeners: &[SocketAddr]) -> Option<SocketAddr> {
    listeners
        .iter()
        .copied()
        .find(|listener| listener.ip().is_loopback() && listener.ip().is_ipv4())
        .or_else(|| {
            listeners
                .iter()
                .copied()
                .find(|listener| listener.ip().is_loopback())
        })
}

async fn bind_agent_session(
    profile: &Profile,
    mut tunnel: MasqueRuntime,
    agent: WindowsAgentClient,
    operation_id: Uuid,
    resuming: bool,
    startup_lease: Option<NamedPipeClient>,
) -> Result<WindowsVpnRuntime, (MasqueRuntime, WindowsVpnError)> {
    let tun_io = match tunnel.attach_tun() {
        Ok(tun_io) => tun_io,
        Err(error) => {
            let _ = abort_startup(&agent, operation_id, resuming, "TUN_ATTACH_FAILED").await;
            return Err((tunnel, error.into()));
        }
    };
    let handles = if resuming {
        agent.resume_tunnel(operation_id, profile.id).await
    } else {
        agent
            .open_packet_session(operation_id, DEFAULT_PACKET_RING_CAPACITY)
            .await
    };
    let handles = match handles {
        Ok(handles) => handles,
        Err(error) => {
            tunnel.detach_tun();
            let _ = abort_startup(&agent, operation_id, resuming, "PACKET_SESSION_FAILED").await;
            return Err((tunnel, error));
        }
    };
    let mapping = match PacketSessionMapping::attach(handles) {
        Ok(mapping) => Arc::new(mapping),
        Err(error) => {
            tunnel.detach_tun();
            let _ = abort_startup(&agent, operation_id, resuming, "PACKET_MAPPING_FAILED").await;
            return Err((tunnel, error));
        }
    };

    let cancellation = CancellationToken::new();
    let (pump_failure_tx, pump_failure) = watch::channel(None);
    let (agent_disconnected_tx, agent_disconnected) = watch::channel(false);
    let listeners = tunnel.listeners().to_vec();
    let socks5_listeners = tunnel.socks5_listeners().to_vec();
    let http_listeners = tunnel.http_listeners().to_vec();
    let tunnel_monitor = tunnel.monitor();
    let mut tasks = start_packet_pumps(
        tun_io,
        Arc::clone(&mapping),
        tunnel_monitor.clone(),
        cancellation.clone(),
        pump_failure_tx.clone(),
    );

    if !resuming && let Err(error) = agent.commit(operation_id).await {
        mapping.signal_shutdown();
        cancellation.cancel();
        stop_tasks(tasks).await;
        tunnel.detach_tun();
        let _ = abort_startup(&agent, operation_id, resuming, "COMMIT_FAILED").await;
        return Err((tunnel, error));
    }

    let lease_result = match startup_lease {
        Some(lease) => agent.promote_liveness_lease(operation_id, lease).await,
        None => agent.open_liveness_lease(operation_id).await,
    };
    let lease = match lease_result {
        Ok(lease) => lease,
        Err(error) => {
            mapping.signal_shutdown();
            cancellation.cancel();
            stop_tasks(tasks).await;
            tunnel.detach_tun();
            let _ = abort_startup(&agent, operation_id, resuming, "LIVENESS_LEASE_FAILED").await;
            return Err((tunnel, error));
        }
    };
    tasks.push(start_agent_liveness_watch(
        lease,
        Arc::clone(&mapping),
        cancellation.clone(),
        pump_failure_tx,
        agent_disconnected_tx,
    ));

    let system_proxy = if profile.frontends.http && profile.proxy.system_proxy {
        let Some(listener) = loopback_http_listener(&http_listeners) else {
            mapping.signal_shutdown();
            cancellation.cancel();
            stop_tasks(tasks).await;
            tunnel.detach_tun();
            let _ = abort_startup(
                &agent,
                operation_id,
                resuming,
                "SYSTEM_PROXY_LISTENER_MISSING",
            )
            .await;
            return Err((tunnel, WindowsVpnError::MissingSystemProxyListener));
        };
        match WindowsSystemProxyGuard::start_for_tunnel(listener, operation_id).await {
            Ok(guard) => Some(guard),
            Err(error) => {
                mapping.signal_shutdown();
                cancellation.cancel();
                stop_tasks(tasks).await;
                tunnel.detach_tun();
                let _ = abort_startup(&agent, operation_id, resuming, "SYSTEM_PROXY_APPLY_FAILED")
                    .await;
                return Err((tunnel, error));
            }
        }
    } else {
        None
    };

    if resuming {
        tracing::info!(
            %operation_id,
            profile_id = %profile.id,
            "reattached Engine data plane to active Windows Agent transaction"
        );
    }

    Ok(WindowsVpnRuntime {
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
        listeners,
        socks5_listeners,
        http_listeners,
        system_proxy,
        transaction_open: true,
        tunnel: Some(tunnel),
        socket_protector: None,
        proxy_detach_state: None,
    })
}

fn tunnel_plan(
    profile: &Profile,
    identity: &MasqueTlsIdentity,
    registration_api: &[SocketAddr],
    split_dns: bool,
) -> agent_v1::TunnelPlan {
    tunnel_plan_from_assignment(
        profile,
        identity.assigned_ipv4,
        identity.assigned_ipv6,
        registration_api,
        split_dns,
    )
}

fn tunnel_plan_from_assignment(
    profile: &Profile,
    assigned_ipv4: std::net::Ipv4Addr,
    assigned_ipv6: std::net::Ipv6Addr,
    registration_api: &[SocketAddr],
    split_dns: bool,
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
        // Endpoint policy selects the physical MASQUE ingress only. DNS is
        // carried inside CONNECT-IP and remains dual-stack over either ingress.
        dns_servers: if split_dns {
            vec![SPLIT_DNS_IPV4.to_string(), SPLIT_DNS_IPV6.to_string()]
        } else {
            profile
                .dns_servers
                .iter()
                .map(ToString::to_string)
                .collect()
        },
        split_exclusions: profile
            .split_exclusions
            .iter()
            .map(ToString::to_string)
            .collect(),
        allow_lan: profile.allow_lan,
        kill_switch: profile.kill_switch,
        assigned_ipv4: format!("{assigned_ipv4}/32"),
        assigned_ipv6: format!("{assigned_ipv6}/128"),
        endpoint_candidates,
        control_api_candidates: registration_api.iter().map(ToString::to_string).collect(),
        split_dns,
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
    if !capabilities.dynamic_direct_egress {
        missing.push("dynamic_direct_egress");
    }
    if !capabilities.physical_dns_snapshot {
        missing.push("physical_dns_snapshot");
    }
    if !capabilities.exact_generation_egress {
        missing.push("exact_generation_egress");
    }
    if missing.is_empty() {
        Ok(())
    } else {
        Err(WindowsVpnError::MissingCapabilities(missing.join(",")))
    }
}

fn start_packet_pumps(
    mut tun_io: MasqueTunIo,
    mapping: Arc<PacketSessionMapping>,
    tunnel_monitor: ManagedTunnelMonitor,
    cancellation: CancellationToken,
    failure: watch::Sender<Option<WindowsPumpFailure>>,
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
            Ok(Err(error)) => report_pump_failure(
                &wait_failure,
                &wait_cancel,
                WindowsPumpFailure::agent(error.to_string()),
            ),
            Err(error) => report_pump_failure(
                &wait_failure,
                &wait_cancel,
                WindowsPumpFailure::agent(format!("Agent packet wait task failed: {error}")),
            ),
        }
    });

    let pump_mapping = Arc::clone(&mapping);
    let pump_cancel = cancellation.clone();
    let pump_failure = failure.clone();
    let pump_task = tokio::spawn(async move {
        let mut ring_saturation_count = 0u64;
        loop {
            tokio::select! {
                () = pump_cancel.cancelled() => break,
                ready = packet_ready_rx.recv() => {
                    if ready.is_none() {
                        if !pump_cancel.is_cancelled() {
                            report_pump_failure(
                                &pump_failure,
                                &pump_cancel,
                                WindowsPumpFailure::agent(
                                    "Agent packet notification channel closed",
                                ),
                            );
                        }
                        break;
                    }
                    loop {
                        match pump_mapping
                            .ring()
                            .try_pop(PacketDirection::AgentToEngine)
                        {
                            Ok(Some(packet)) => {
                                let send = tokio::select! {
                                    biased;
                                    _ = pump_cancel.cancelled() => return,
                                    result = tun_io.send_owned_packet(Bytes::from(packet)) => result,
                                };
                                if let Err(error) = send {
                                    if !pump_cancel.is_cancelled() {
                                        report_pump_failure(
                                            &pump_failure,
                                            &pump_cancel,
                                            WindowsPumpFailure::transport(
                                                "failed to send a TUN packet into MASQUE",
                                                &error,
                                                tunnel_monitor.path(),
                                            ),
                                        );
                                    }
                                    return;
                                }
                            }
                            Ok(None) => break,
                            Err(error) => {
                                report_pump_failure(
                                    &pump_failure,
                                    &pump_cancel,
                                    WindowsPumpFailure::agent(format!(
                                        "Agent-to-Engine packet ring failed: {error}"
                                    )),
                                );
                                return;
                            }
                        }
                    }
                }
                packet = tun_io.receive_packet() => {
                    match packet {
                        Ok(packet) => {
                            match publish_engine_packet_batch(
                                &mut tun_io,
                                &pump_mapping,
                                &tunnel_monitor,
                                &pump_cancel,
                                &mut ring_saturation_count,
                                packet,
                            )
                            .await
                            {
                                Ok(true) => {}
                                Ok(false) => break,
                                Err(failure) => {
                                    report_pump_failure(&pump_failure, &pump_cancel, failure);
                                    break;
                                }
                            }
                        }
                        Err(error) => {
                            if !pump_cancel.is_cancelled() {
                                report_pump_failure(
                                    &pump_failure,
                                    &pump_cancel,
                                    WindowsPumpFailure::transport(
                                        "failed to receive a MASQUE packet",
                                        &error,
                                        tunnel_monitor.path(),
                                    ),
                                );
                            }
                            break;
                        }
                    }
                }
            }
        }
        // Dropping tun_io detaches TUN. MASQUE stays owned by WindowsVpnRuntime.
    });

    vec![wait_task, pump_task]
}

async fn publish_engine_packet_batch(
    tun_io: &mut MasqueTunIo,
    mapping: &PacketSessionMapping,
    tunnel_monitor: &ManagedTunnelMonitor,
    cancellation: &CancellationToken,
    saturation_count: &mut u64,
    first: Bytes,
) -> Result<bool, WindowsPumpFailure> {
    let ring = mapping.ring();
    let wake_bytes = (ring.capacity() as usize / 4).max(1);
    let mut packet = first;
    let mut published = false;
    let mut published_bytes = 0usize;
    let mut packet_count = 0usize;
    let result: Result<bool, WindowsPumpFailure> = 'batch: loop {
        loop {
            match ring.try_push_preserving(PacketDirection::EngineToAgent, &packet) {
                Ok(true) => break,
                Ok(false) => {
                    if published {
                        mapping
                            .signal_engine_to_agent()
                            .map_err(|error| WindowsPumpFailure::agent(error.to_string()))?;
                        published = false;
                        published_bytes = 0;
                        packet_count = 0;
                    }
                    *saturation_count = saturation_count.saturating_add(1);
                    if saturation_count.is_power_of_two() {
                        tracing::warn!(
                            wait_count = *saturation_count,
                            ring_capacity = ring.capacity(),
                            "waiting for capacity in the Engine-to-Agent packet ring"
                        );
                    }
                    tokio::select! {
                        biased;
                        _ = cancellation.cancelled() => break 'batch Ok(false),
                        () = sleep(PACKET_RING_RETRY_INTERVAL) => {}
                    }
                }
                Err(error) => {
                    break 'batch Err(WindowsPumpFailure::agent(format!(
                        "Engine-to-Agent packet ring failed: {error}"
                    )));
                }
            }
        }
        published = true;
        published_bytes = published_bytes.saturating_add(packet.len());
        packet_count += 1;
        if packet_count == PACKET_WAKE_BATCH || published_bytes >= wake_bytes {
            break 'batch Ok(true);
        }
        match tun_io.try_receive_packet() {
            Ok(Some(next)) => packet = next,
            Ok(None) => break 'batch Ok(true),
            Err(error) => {
                break 'batch Err(WindowsPumpFailure::transport(
                    "failed to receive a MASQUE packet",
                    &error,
                    tunnel_monitor.path(),
                ));
            }
        }
    };

    if published {
        // Signal after publication, once per bounded batch. The Agent drains
        // until empty, so coalesced auto-reset signals cannot strand packets.
        mapping
            .signal_engine_to_agent()
            .map_err(|error| WindowsPumpFailure::agent(error.to_string()))?;
    }
    result
}

fn report_pump_failure(
    failure: &watch::Sender<Option<WindowsPumpFailure>>,
    cancellation: &CancellationToken,
    pump_failure: WindowsPumpFailure,
) {
    if failure.borrow().is_none() {
        failure.send_replace(Some(pump_failure));
    }
    cancellation.cancel();
}

fn start_agent_liveness_watch(
    mut pipe: NamedPipeClient,
    mapping: Arc<PacketSessionMapping>,
    cancellation: CancellationToken,
    failure: watch::Sender<Option<WindowsPumpFailure>>,
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
        report_pump_failure(&failure, &cancellation, WindowsPumpFailure::agent(message));
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

#[derive(Debug, Clone, Copy)]
struct AgentServiceStatus {
    state: u32,
    win32_exit_code: u32,
    service_exit_code: u32,
}

#[async_trait]
trait AgentServiceController: Send + Sync {
    async fn ensure_started(
        &self,
        deadline: tokio::time::Instant,
    ) -> Result<(), AgentServiceControlError>;

    async fn status(&self) -> Result<AgentServiceStatus, AgentServiceControlError>;
}

struct ScmAgentServiceController {
    service_name: Arc<str>,
}

impl ScmAgentServiceController {
    fn production() -> Self {
        Self {
            service_name: Arc::from("UsqueAgent"),
        }
    }
}

#[async_trait]
impl AgentServiceController for ScmAgentServiceController {
    async fn ensure_started(
        &self,
        deadline: tokio::time::Instant,
    ) -> Result<(), AgentServiceControlError> {
        let service_name = Arc::clone(&self.service_name);
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        tokio::task::spawn_blocking(move || {
            ensure_service_started_sync(&service_name, std::time::Instant::now() + remaining)
        })
        .await
        .map_err(|error| AgentServiceControlError::Task(error.to_string()))?
    }

    async fn status(&self) -> Result<AgentServiceStatus, AgentServiceControlError> {
        let service_name = Arc::clone(&self.service_name);
        tokio::task::spawn_blocking(move || query_named_service_status(&service_name))
            .await
            .map_err(|error| AgentServiceControlError::Task(error.to_string()))?
    }
}

#[cfg(test)]
struct NoopAgentServiceController;

#[cfg(test)]
#[async_trait]
impl AgentServiceController for NoopAgentServiceController {
    async fn ensure_started(
        &self,
        _deadline: tokio::time::Instant,
    ) -> Result<(), AgentServiceControlError> {
        Ok(())
    }

    async fn status(&self) -> Result<AgentServiceStatus, AgentServiceControlError> {
        Ok(AgentServiceStatus {
            state: SERVICE_STOPPED,
            win32_exit_code: 0,
            service_exit_code: 0,
        })
    }
}

fn ensure_service_started_sync(
    service_name: &str,
    deadline: std::time::Instant,
) -> Result<(), AgentServiceControlError> {
    let (_manager, service) =
        open_agent_service(service_name, SERVICE_START | SERVICE_QUERY_STATUS)?;
    loop {
        let status = query_service_status(service.0)?;
        match status.state {
            SERVICE_RUNNING | SERVICE_START_PENDING => return Ok(()),
            SERVICE_STOPPED => {
                if std::time::Instant::now() >= deadline {
                    return Err(AgentServiceControlError::Timeout {
                        state: status.state,
                    });
                }
                // SAFETY: the handle carries SERVICE_START and the Agent does
                // not accept runtime service arguments.
                if unsafe { StartServiceW(service.0, 0, ptr::null()) } != 0 {
                    return Ok(());
                }
                let error = io::Error::last_os_error();
                if error.raw_os_error().map(|value| value as u32)
                    == Some(ERROR_SERVICE_ALREADY_RUNNING)
                {
                    return Ok(());
                }
                return Err(classify_service_error("start UsqueAgent", error));
            }
            SERVICE_STOP_PENDING => {
                if std::time::Instant::now() >= deadline {
                    return Err(AgentServiceControlError::Timeout {
                        state: status.state,
                    });
                }
                std::thread::sleep(AGENT_START_POLL_INTERVAL);
            }
            SERVICE_PAUSED => {
                return Err(AgentServiceControlError::UnexpectedState(status.state));
            }
            state => return Err(AgentServiceControlError::UnexpectedState(state)),
        }
    }
}

fn query_named_service_status(
    service_name: &str,
) -> Result<AgentServiceStatus, AgentServiceControlError> {
    let (_manager, service) = open_agent_service(service_name, SERVICE_QUERY_STATUS)?;
    query_service_status(service.0)
}

fn open_agent_service(
    service_name: &str,
    access: u32,
) -> Result<(OwnedScHandle, OwnedScHandle), AgentServiceControlError> {
    // SAFETY: null machine/database names select the local active SCM database.
    let manager =
        OwnedScHandle::new(unsafe { OpenSCManagerW(ptr::null(), ptr::null(), SC_MANAGER_CONNECT) })
            .map_err(|error| classify_service_error("open the Service Control Manager", error))?;
    let name = wide(service_name);
    // SAFETY: name is null-terminated and manager is a live SCM handle.
    let service = OwnedScHandle::new(unsafe { OpenServiceW(manager.0, name.as_ptr(), access) })
        .map_err(|error| classify_service_error("open UsqueAgent", error))?;
    Ok((manager, service))
}

fn query_service_status(
    service: SC_HANDLE,
) -> Result<AgentServiceStatus, AgentServiceControlError> {
    let mut status = mem::MaybeUninit::<SERVICE_STATUS_PROCESS>::zeroed();
    let mut required = 0_u32;
    // SAFETY: status points to writable storage of the exact required type.
    if unsafe {
        QueryServiceStatusEx(
            service,
            SC_STATUS_PROCESS_INFO,
            status.as_mut_ptr().cast(),
            mem::size_of::<SERVICE_STATUS_PROCESS>() as u32,
            &mut required,
        )
    } == 0
    {
        return Err(classify_service_error(
            "query UsqueAgent status",
            io::Error::last_os_error(),
        ));
    }
    // SAFETY: the successful API call initialized the structure.
    let status = unsafe { status.assume_init() };
    Ok(AgentServiceStatus {
        state: status.dwCurrentState,
        win32_exit_code: status.dwWin32ExitCode,
        service_exit_code: status.dwServiceSpecificExitCode,
    })
}

fn classify_service_error(operation: &'static str, error: io::Error) -> AgentServiceControlError {
    match error.raw_os_error().map(|value| value as u32) {
        Some(ERROR_ACCESS_DENIED) => AgentServiceControlError::AccessDenied,
        Some(ERROR_SERVICE_DISABLED) => AgentServiceControlError::Disabled,
        Some(ERROR_SERVICE_DOES_NOT_EXIST) => AgentServiceControlError::Missing,
        Some(ERROR_SERVICE_MARKED_FOR_DELETE) => AgentServiceControlError::MarkedForDelete,
        Some(ERROR_SERVICE_REQUEST_TIMEOUT) => AgentServiceControlError::RequestTimeout,
        _ => AgentServiceControlError::Io { operation, error },
    }
}

struct OwnedScHandle(SC_HANDLE);

impl OwnedScHandle {
    fn new(handle: SC_HANDLE) -> io::Result<Self> {
        if handle.is_null() {
            Err(io::Error::last_os_error())
        } else {
            Ok(Self(handle))
        }
    }
}

impl Drop for OwnedScHandle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: this wrapper uniquely owns the SCM handle.
            unsafe {
                CloseServiceHandle(self.0);
            }
        }
    }
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum AgentServiceControlError {
    #[error("Windows denied permission to start the Usque Agent service")]
    AccessDenied,
    #[error("the Usque Agent service is disabled")]
    Disabled,
    #[error("the Usque Agent service is not installed")]
    Missing,
    #[error("the Usque Agent service is marked for deletion")]
    MarkedForDelete,
    #[error("the Service Control Manager timed out while starting Usque Agent")]
    RequestTimeout,
    #[error("the Usque Agent service entered unsupported state {0}")]
    UnexpectedState(u32),
    #[error("the Usque Agent service did not become ready before timeout (state {state})")]
    Timeout { state: u32 },
    #[error(
        "the Usque Agent service stopped before its pipe became ready (Win32 {win32_exit_code}, service {service_exit_code})"
    )]
    Stopped {
        win32_exit_code: u32,
        service_exit_code: u32,
    },
    #[error("could not {operation}: {error}")]
    Io {
        operation: &'static str,
        #[source]
        error: io::Error,
    },
    #[error("Agent service-control task failed: {0}")]
    Task(String),
}

#[derive(Clone)]
struct WindowsAgentClient {
    pipe_name: Arc<str>,
    service_controller: Arc<dyn AgentServiceController>,
}

impl WindowsAgentClient {
    fn production() -> Self {
        Self {
            pipe_name: Arc::from(AGENT_PIPE_NAME),
            service_controller: Arc::new(ScmAgentServiceController::production()),
        }
    }

    #[cfg(test)]
    fn for_test(pipe_name: String) -> Self {
        Self {
            pipe_name: Arc::from(pipe_name),
            service_controller: Arc::new(NoopAgentServiceController),
        }
    }

    #[cfg(test)]
    fn for_test_with_controller(
        pipe_name: String,
        service_controller: Arc<dyn AgentServiceController>,
    ) -> Self {
        Self {
            pipe_name: Arc::from(pipe_name),
            service_controller,
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

    async fn inspect_platform_state_if_running(&self) -> Result<PlatformState, WindowsVpnError> {
        // Diagnostics must be read-only. Opening an existing pipe is allowed;
        // unlike `call`, this deliberately never starts or reconfigures the
        // Agent service when it is not already available.
        let mut pipe = ClientOptions::new().open(self.pipe_name.as_ref())?;
        let exchange = self.exchange(
            &mut pipe,
            agent_request::Payload::InspectPlatformState(InspectPlatformStateRequest {}),
        );
        match timeout(AGENT_RPC_TIMEOUT, exchange)
            .await
            .map_err(|_| WindowsVpnError::RpcTimeout)??
        {
            agent_response::Payload::PlatformState(state) => Ok(state),
            payload => Err(WindowsVpnError::UnexpectedResponse(payload_name(&payload))),
        }
    }

    async fn get_physical_network_info(
        &self,
        operation_id: Uuid,
    ) -> Result<PhysicalNetworkInfo, WindowsVpnError> {
        match self
            .call(agent_request::Payload::GetPhysicalNetworkInfo(
                GetPhysicalNetworkInfoRequest {
                    operation_id: operation_id.to_string(),
                },
            ))
            .await?
        {
            agent_response::Payload::PhysicalNetworkInfo(info) => Ok(info),
            payload => Err(WindowsVpnError::UnexpectedResponse(payload_name(&payload))),
        }
    }

    async fn acquire_direct_egress(
        &self,
        operation_id: Uuid,
        remote: SocketAddr,
        protocol: DirectProtocol,
        expected_generation: u64,
    ) -> Result<(NamedPipeClient, AgentDirectEgressLease), WindowsVpnError> {
        let mut pipe = self.open_pipe().await?;
        let response = timeout(
            AGENT_RPC_TIMEOUT,
            self.exchange(
                &mut pipe,
                agent_request::Payload::AcquireDirectEgress(AcquireDirectEgressRequest {
                    operation_id: operation_id.to_string(),
                    remote_endpoint: remote.to_string(),
                    protocol: u32::from(protocol.iana_number()),
                    expected_generation,
                }),
            ),
        )
        .await
        .map_err(|_| WindowsVpnError::RpcTimeout)??;
        match response {
            agent_response::Payload::DirectEgressLease(lease)
                if lease.remote_endpoint == remote.to_string()
                    && lease.protocol == u32::from(protocol.iana_number())
                    && lease.interface_luid != 0
                    && lease.interface_index != 0
                    && lease.network_generation == expected_generation
                    && lease.network_generation != 0 =>
            {
                Ok((pipe, lease))
            }
            agent_response::Payload::DirectEgressLease(_) => {
                Err(WindowsVpnError::InvalidDirectEgressLease)
            }
            payload => Err(WindowsVpnError::UnexpectedResponse(payload_name(&payload))),
        }
    }

    async fn prepare(
        &self,
        operation_id: Uuid,
        plan: agent_v1::TunnelPlan,
    ) -> Result<NamedPipeClient, WindowsVpnError> {
        let mut pipe = self.open_pipe().await?;
        let response = timeout(
            AGENT_RPC_TIMEOUT,
            self.exchange(
                &mut pipe,
                agent_request::Payload::PrepareTunnel(PrepareTunnelRequest {
                    operation_id: operation_id.to_string(),
                    plan: Some(plan),
                }),
            ),
        )
        .await
        .map_err(|_| WindowsVpnError::RpcTimeout)??;
        match response {
            agent_response::Payload::State(state)
                if state.phase == agent_v1::AgentPhase::Prepared as i32 =>
            {
                Ok(pipe)
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
        let pipe = self.open_pipe().await?;
        self.promote_liveness_lease(operation_id, pipe).await
    }

    async fn promote_liveness_lease(
        &self,
        operation_id: Uuid,
        mut pipe: NamedPipeClient,
    ) -> Result<NamedPipeClient, WindowsVpnError> {
        let response = timeout(
            AGENT_RPC_TIMEOUT,
            self.exchange(
                &mut pipe,
                agent_request::Payload::AcquireTunnelLease(AcquireTunnelLeaseRequest {
                    operation_id: operation_id.to_string(),
                }),
            ),
        )
        .await
        .map_err(|_| WindowsVpnError::RpcTimeout)??;
        match response {
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

    async fn apply_system_proxy_lease(
        &self,
        operation_id: Uuid,
        proxy_uri: String,
        bypass_hosts: Vec<String>,
    ) -> Result<NamedPipeClient, WindowsVpnError> {
        let mut pipe = self.open_pipe().await?;
        let response = timeout(
            AGENT_RPC_TIMEOUT,
            self.exchange(
                &mut pipe,
                agent_request::Payload::ApplySystemProxy(ApplySystemProxyRequest {
                    operation_id: operation_id.to_string(),
                    proxy_uri,
                    bypass_hosts,
                }),
            ),
        )
        .await
        .map_err(|_| WindowsVpnError::RpcTimeout)??;
        match response {
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
            agent_response::Payload::State(state) => Ok(state),
            payload => Err(WindowsVpnError::UnexpectedResponse(payload_name(&payload))),
        }
    }

    async fn call(
        &self,
        payload: agent_request::Payload,
    ) -> Result<agent_response::Payload, WindowsVpnError> {
        let mut pipe = self.open_pipe().await?;
        timeout(AGENT_RPC_TIMEOUT, self.exchange(&mut pipe, payload))
            .await
            .map_err(|_| WindowsVpnError::RpcTimeout)?
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
        let deadline = tokio::time::Instant::now() + AGENT_START_TIMEOUT;
        let mut next_start_check = tokio::time::Instant::now();
        loop {
            match ClientOptions::new().open(self.pipe_name.as_ref()) {
                Ok(pipe) => return Ok(pipe),
                Err(error) => {
                    let code = error.raw_os_error().map(|value| value as u32);
                    if code == Some(ERROR_FILE_NOT_FOUND)
                        && tokio::time::Instant::now() >= next_start_check
                    {
                        self.service_controller.ensure_started(deadline).await?;
                        next_start_check =
                            tokio::time::Instant::now() + AGENT_START_RECHECK_INTERVAL;
                    } else if !matches!(code, Some(ERROR_FILE_NOT_FOUND | ERROR_PIPE_BUSY)) {
                        return Err(error.into());
                    }
                    if tokio::time::Instant::now() >= deadline {
                        let status = self.service_controller.status().await?;
                        return Err(if status.state == SERVICE_STOPPED {
                            AgentServiceControlError::Stopped {
                                win32_exit_code: status.win32_exit_code,
                                service_exit_code: status.service_exit_code,
                            }
                            .into()
                        } else {
                            AgentServiceControlError::Timeout {
                                state: status.state,
                            }
                            .into()
                        });
                    }
                    tokio::time::sleep(AGENT_START_POLL_INTERVAL).await;
                }
            }
        }
    }
}

pub(crate) async fn inspect_platform_state_if_running() -> Result<PlatformState, WindowsVpnError> {
    WindowsAgentClient::production()
        .inspect_platform_state_if_running()
        .await
}

fn payload_name(payload: &agent_response::Payload) -> &'static str {
    match payload {
        agent_response::Payload::Empty(_) => "empty",
        agent_response::Payload::Capabilities(_) => "capabilities",
        agent_response::Payload::State(_) => "state",
        agent_response::Payload::PacketSession(_) => "packet_session",
        agent_response::Payload::PhysicalNetworkInfo(_) => "physical_network_info",
        agent_response::Payload::DirectEgressLease(_) => "direct_egress_lease",
        agent_response::Payload::PlatformState(_) => "platform_state",
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

fn system_proxy_restore_succeeded(
    tunnel_lease: bool,
    operation_id: Uuid,
    state: &AgentState,
) -> bool {
    if tunnel_lease {
        state.phase == agent_v1::AgentPhase::Active as i32
            && state.operation_id == operation_id.to_string()
    } else {
        state.phase == agent_v1::AgentPhase::Clean as i32
    }
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum WindowsVpnError {
    #[error("Windows Agent I/O failed: {0}")]
    Io(#[from] io::Error),
    #[error("Windows Agent service startup failed: {0}")]
    AgentService(#[from] AgentServiceControlError),
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
    #[error("Windows Agent returned malformed physical network metadata")]
    InvalidPhysicalNetworkInfo,
    #[error("the selected physical network has no usable DNS server for Split DNS")]
    PhysicalDnsUnavailable,
    #[error("Windows Agent returned a mismatched direct-egress lease")]
    InvalidDirectEgressLease,
    #[error(
        "Windows Agent active tunnel belongs to Profile {active}, not requested Profile {requested}"
    )]
    ActiveProfileMismatch { active: String, requested: Uuid },
    #[error("Windows Agent is missing required capabilities: {0}")]
    MissingCapabilities(String),
    #[error("Windows system proxy requires a Loopback listener, got {0}")]
    InvalidSystemProxyListener(std::net::SocketAddr),
    #[error("Windows system proxy requires an active Loopback HTTP listener")]
    MissingSystemProxyListener,
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
    #[error("the Windows VPN session has no live MASQUE runtime")]
    MissingMasqueRuntime,
}

#[cfg(test)]
mod tests {
    use std::{
        net::{Ipv4Addr, Ipv6Addr},
        sync::{
            Mutex as StdMutex,
            atomic::{AtomicUsize, Ordering},
        },
    };

    use tokio::net::windows::named_pipe::ServerOptions;
    use usque_core::{AddressFamily, MasqueKeyPair, OperatingMode, Transport};

    use super::*;

    #[test]
    fn physical_generation_and_family_state_advance_together_without_dns_dependency() {
        let mut state = WindowsPhysicalState {
            generation: 10,
            agent_generation: Some(20),
            dns_servers: Vec::new(),
            family_mask: 3,
        };
        assert!(state.update(Some((21, Vec::new(), 3))));
        assert_eq!(
            (state.generation, state.agent_generation, state.family_mask),
            (11, Some(21), 3)
        );
        assert!(!state.update(Some((21, Vec::new(), 3))));
        assert!(state.update(None));
        assert_eq!(
            (state.generation, state.agent_generation, state.family_mask),
            (12, None, 0)
        );
        assert!(!state.update(None));
        assert!(state.update(Some((21, Vec::new(), 1))));
        assert_eq!(
            (state.generation, state.agent_generation, state.family_mask),
            (13, Some(21), 1)
        );
    }

    #[test]
    fn exact_egress_errors_never_forward_raw_agent_details_to_transport() {
        assert_eq!(
            socket_lease_error(WindowsVpnError::Remote {
                code: "AGENT_STALE_GENERATION".to_owned(),
                message: "192.0.2.4 private-network".to_owned(),
                retryable: true,
            }),
            STALE_GENERATION_REASON
        );
        let error = socket_lease_error(WindowsVpnError::Remote {
            code: "AGENT_DIRECT_EGRESS_FAILED".to_owned(),
            message: "192.0.2.4 private-network".to_owned(),
            retryable: true,
        });
        assert!(!error.contains("192.0.2.4") && !error.contains("private-network"));
    }

    #[test]
    fn dropping_the_windows_socket_protector_cancels_its_network_monitor() {
        let cancellation = CancellationToken::new();
        let protector = WindowsVpnSocketProtector {
            registration_api: Vec::new(),
            agent: WindowsAgentClient::production(),
            operation_id: Uuid::nil(),
            physical: RwLock::new(WindowsPhysicalState {
                generation: 1,
                agent_generation: Some(1),
                dns_servers: Vec::new(),
                family_mask: 3,
            }),
            monitor_cancel: cancellation.clone(),
            proxy_mode: AtomicBool::new(false),
        };
        drop(protector);
        assert!(cancellation.is_cancelled());
    }

    #[test]
    fn proxy_detach_requires_clean_closed_transaction_and_invalidates_old_generations() {
        let protector = WindowsVpnSocketProtector {
            registration_api: Vec::new(),
            agent: WindowsAgentClient::production(),
            operation_id: Uuid::nil(),
            physical: RwLock::new(WindowsPhysicalState {
                generation: 7,
                agent_generation: Some(3),
                dns_servers: Vec::new(),
                family_mask: 3,
            }),
            monitor_cancel: CancellationToken::new(),
            proxy_mode: AtomicBool::new(false),
        };
        let mut state = AgentState {
            phase: agent_v1::AgentPhase::Active as i32,
            ..AgentState::default()
        };
        assert!(protector.complete_proxy_detach(&state, Ok(())).is_err());
        assert_eq!(protector.egress_generation(7), Ok(Some(3)));
        assert!(!protector.monitor_cancel.is_cancelled());
        state.phase = agent_v1::AgentPhase::Clean as i32;
        state.packet_session_active = true;
        assert!(protector.complete_proxy_detach(&state, Ok(())).is_err());
        state.packet_session_active = false;
        assert!(
            protector
                .complete_proxy_detach(&state, Err(WindowsVpnError::MissingSystemProxyListener))
                .is_err()
        );
        assert_eq!(protector.egress_generation(7), Ok(Some(3)));
        assert!(!protector.monitor_cancel.is_cancelled());
        // The same validated rollback receipt can be used by a successful
        // cleanup retry; the failed attempt did not grant proxy mode.
        protector.complete_proxy_detach(&state, Ok(())).unwrap();
        assert!(protector.monitor_cancel.is_cancelled());
        assert_eq!(protector.network_generation(), None);
        assert_eq!(
            protector.endpoint_family_available("127.0.0.1:443".parse().unwrap()),
            None
        );
        assert!(!protector.tun_direct_available());
        assert_eq!(protector.egress_generation(0), Ok(None));
        assert!(protector.egress_generation(7).is_err());
        assert!(protector.verify_generation(0, 3).is_err());
    }

    #[test]
    fn windows_vpn_requires_the_exact_generation_lease_capability() {
        let mut capabilities = AgentCapabilities {
            protocol_version: AGENT_PROTOCOL_VERSION,
            wintun: true,
            interface_addresses: true,
            interface_dns: true,
            shared_packet_ring: true,
            dynamic_direct_egress: true,
            physical_dns_snapshot: true,
            exact_generation_egress: true,
            ..AgentCapabilities::default()
        };
        assert!(validate_capabilities(&capabilities, false).is_ok());
        capabilities.exact_generation_egress = false;
        assert!(matches!(
            validate_capabilities(&capabilities, false),
            Err(WindowsVpnError::MissingCapabilities(_))
        ));
    }

    #[test]
    fn packet_pump_transport_failures_keep_the_active_transport_code() {
        let path = RuntimePath {
            transport: Transport::Http3,
            endpoint_family: AddressFamily::Ipv4,
            ipv4_available: true,
            ipv6_available: true,
        };
        let closed = WindowsPumpFailure::transport(
            "receive MASQUE packet",
            &TransportError::TunnelClosed,
            path,
        );
        assert_eq!(
            closed.failure.code,
            TransportFailureCode::H3ConnectionClosed
        );
        assert_eq!(closed.failure.transport, Some(Transport::Http3));
        assert_eq!(closed.failure.address_family, Some(AddressFamily::Ipv4));

        let saturated = WindowsPumpFailure::transport(
            "send MASQUE packet",
            &TransportError::SendQueueFull,
            path,
        );
        assert_eq!(saturated.failure.code, TransportFailureCode::SendQueueFull);
        assert_ne!(
            saturated.failure.code,
            TransportFailureCode::AgentUnreachable
        );
    }

    #[test]
    fn only_agent_liveness_failures_use_agent_unreachable() {
        let failure = WindowsPumpFailure::agent("agent pipe closed");
        assert_eq!(failure.failure.code, TransportFailureCode::AgentUnreachable);
        assert_eq!(failure.failure.stage, TransportStage::PlatformRecovery);
    }

    struct StartingTestController {
        pipe_name: String,
        starts: AtomicUsize,
        create_on_call: usize,
        server: StdMutex<Option<tokio::net::windows::named_pipe::NamedPipeServer>>,
    }

    #[async_trait]
    impl AgentServiceController for StartingTestController {
        async fn ensure_started(
            &self,
            _deadline: tokio::time::Instant,
        ) -> Result<(), AgentServiceControlError> {
            let call = self.starts.fetch_add(1, Ordering::AcqRel) + 1;
            if call < self.create_on_call {
                return Ok(());
            }
            let mut server_slot = self.server.lock().expect("server slot");
            if server_slot.is_some() {
                return Ok(());
            }
            let server = ServerOptions::new()
                .first_pipe_instance(true)
                .create(&self.pipe_name)
                .map_err(|error| AgentServiceControlError::Io {
                    operation: "create test Agent pipe",
                    error,
                })?;
            *server_slot = Some(server);
            Ok(())
        }

        async fn status(&self) -> Result<AgentServiceStatus, AgentServiceControlError> {
            Ok(AgentServiceStatus {
                state: SERVICE_RUNNING,
                win32_exit_code: 0,
                service_exit_code: 0,
            })
        }
    }

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
            false,
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
    fn endpoint_only_policy_preserves_dual_stack_tunnel_dns() {
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

        let plan = tunnel_plan(
            &profile,
            &identity,
            &["198.51.100.10:443".parse().unwrap()],
            false,
        );

        assert_eq!(plan.dns_servers, vec!["1.1.1.1", "2606:4700:4700::1111"]);
    }

    #[test]
    fn geo_tunnel_plan_publishes_only_internal_split_dns() {
        let profile = Profile {
            mode: OperatingMode::Vpn,
            ..Profile::default()
        };
        let plan = tunnel_plan(
            &profile,
            &identity(),
            &["198.51.100.10:443".parse().unwrap()],
            true,
        );
        assert!(plan.split_dns);
        assert_eq!(plan.dns_servers, ["198.18.0.1", "fd00::1"]);
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
            false,
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

    #[tokio::test]
    async fn missing_pipe_starts_the_service_controller_only_once() {
        let pipe_name = format!("{AGENT_PIPE_NAME}.test-{}", Uuid::new_v4());
        let controller = Arc::new(StartingTestController {
            pipe_name: pipe_name.clone(),
            starts: AtomicUsize::new(0),
            create_on_call: 1,
            server: StdMutex::new(None),
        });
        let client = WindowsAgentClient::for_test_with_controller(
            pipe_name,
            Arc::clone(&controller) as Arc<dyn AgentServiceController>,
        );
        let pipe = client.open_pipe().await.expect("on-demand pipe");
        assert_eq!(controller.starts.load(Ordering::Acquire), 1);
        drop(pipe);
    }

    #[tokio::test]
    async fn missing_pipe_rechecks_service_after_clean_idle_exit_race() {
        let pipe_name = format!("{AGENT_PIPE_NAME}.test-{}", Uuid::new_v4());
        let controller = Arc::new(StartingTestController {
            pipe_name: pipe_name.clone(),
            starts: AtomicUsize::new(0),
            // The first check models SCM still reporting Running while the
            // clean Agent has already dropped its final pipe. The next check
            // observes Stopped and makes the demand-start pipe available.
            create_on_call: 2,
            server: StdMutex::new(None),
        });
        let client = WindowsAgentClient::for_test_with_controller(
            pipe_name,
            Arc::clone(&controller) as Arc<dyn AgentServiceController>,
        );
        let pipe = client.open_pipe().await.expect("restarted Agent pipe");
        assert_eq!(controller.starts.load(Ordering::Acquire), 2);
        drop(pipe);
    }

    #[tokio::test]
    async fn prepare_pipe_is_promoted_to_liveness_without_reconnecting() {
        let pipe_name = format!("{AGENT_PIPE_NAME}.test-{}", Uuid::new_v4());
        let server = ServerOptions::new()
            .first_pipe_instance(true)
            .create(&pipe_name)
            .expect("server");
        let operation_id = Uuid::new_v4();
        let expected_operation = operation_id.to_string();
        let server_task = tokio::spawn(async move {
            server.connect().await.expect("connect");
            let mut server = server;
            for phase in [agent_v1::AgentPhase::Prepared, agent_v1::AgentPhase::Active] {
                let mut header = [0_u8; 4];
                server.read_exact(&mut header).await.expect("header");
                let mut payload = vec![0_u8; u32::from_be_bytes(header) as usize];
                server.read_exact(&mut payload).await.expect("payload");
                let mut frame = BytesMut::from(header.as_slice());
                frame.extend_from_slice(&payload);
                let request: AgentRequest = decode_frame(frame.freeze()).expect("request");
                let response = AgentResponse {
                    request_id: request.request_id,
                    error: None,
                    payload: Some(agent_response::Payload::State(AgentState {
                        phase: phase as i32,
                        operation_id: expected_operation.clone(),
                        packet_session_active: phase == agent_v1::AgentPhase::Active,
                        ..AgentState::default()
                    })),
                };
                server
                    .write_all(&encode_frame(&response).expect("response"))
                    .await
                    .expect("write");
            }
        });
        let client = WindowsAgentClient::for_test(pipe_name);
        let startup = client
            .prepare(operation_id, agent_v1::TunnelPlan::default())
            .await
            .expect("prepare lease");
        let active = client
            .promote_liveness_lease(operation_id, startup)
            .await
            .expect("active lease");
        drop(active);
        server_task.await.expect("server task");
    }

    fn restore_state(phase: agent_v1::AgentPhase, operation_id: Uuid) -> AgentState {
        AgentState {
            phase: phase as i32,
            operation_id: operation_id.to_string(),
            ..AgentState::default()
        }
    }

    #[test]
    fn standalone_proxy_restore_accepts_clean() {
        let operation_id = Uuid::new_v4();
        assert!(system_proxy_restore_succeeded(
            false,
            operation_id,
            &restore_state(agent_v1::AgentPhase::Clean, operation_id)
        ));
        assert!(!system_proxy_restore_succeeded(
            false,
            operation_id,
            &restore_state(agent_v1::AgentPhase::Active, operation_id)
        ));
    }

    #[test]
    fn tunnel_proxy_restore_accepts_active_for_the_same_operation() {
        let operation_id = Uuid::new_v4();
        assert!(system_proxy_restore_succeeded(
            true,
            operation_id,
            &restore_state(agent_v1::AgentPhase::Active, operation_id)
        ));
        assert!(!system_proxy_restore_succeeded(
            true,
            operation_id,
            &restore_state(agent_v1::AgentPhase::Clean, operation_id)
        ));
        assert!(!system_proxy_restore_succeeded(
            true,
            operation_id,
            &restore_state(agent_v1::AgentPhase::Active, Uuid::new_v4())
        ));
    }

    #[tokio::test]
    async fn tunnel_proxy_shutdown_accepts_agent_active() {
        let pipe_name = format!("{AGENT_PIPE_NAME}.test-{}", Uuid::new_v4());
        let server = ServerOptions::new()
            .first_pipe_instance(true)
            .create(&pipe_name)
            .expect("server");
        let operation_id = Uuid::new_v4();
        let response_operation_id = operation_id.to_string();
        let server_task = tokio::spawn(async move {
            server.connect().await.expect("connect");
            let mut server = server;
            let mut header = [0_u8; 4];
            server.read_exact(&mut header).await.expect("header");
            let mut payload = vec![0_u8; u32::from_be_bytes(header) as usize];
            server.read_exact(&mut payload).await.expect("payload");
            let mut frame = BytesMut::from(header.as_slice());
            frame.extend_from_slice(&payload);
            let request: AgentRequest = decode_frame(frame.freeze()).expect("decode");
            assert!(matches!(
                request.payload,
                Some(agent_request::Payload::RestoreSystemProxy(_))
            ));
            let response = AgentResponse {
                request_id: request.request_id,
                error: None,
                payload: Some(agent_response::Payload::State(AgentState {
                    phase: agent_v1::AgentPhase::Active as i32,
                    operation_id: response_operation_id,
                    ..AgentState::default()
                })),
            };
            server
                .write_all(&encode_frame(&response).expect("encode"))
                .await
                .expect("write");
        });
        let client = WindowsAgentClient::for_test(pipe_name);
        let pipe = client.open_pipe().await.expect("open");
        let mut guard = WindowsSystemProxyGuard {
            client,
            operation_id,
            pipe: Some(pipe),
            tunnel_lease: true,
        };
        guard.shutdown().await.expect("restore Active tunnel lease");
        server_task.await.expect("server task");
    }

    #[tokio::test]
    async fn shutdown_slot_restores_before_dropping_a_standalone_lease() {
        let pipe_name = format!("{AGENT_PIPE_NAME}.test-{}", Uuid::new_v4());
        let server = ServerOptions::new()
            .first_pipe_instance(true)
            .create(&pipe_name)
            .expect("server");
        let operation_id = Uuid::new_v4();
        let response_operation_id = operation_id.to_string();
        let server_task = tokio::spawn(async move {
            server.connect().await.expect("connect");
            let mut server = server;
            let mut header = [0_u8; 4];
            server.read_exact(&mut header).await.expect("header");
            let mut payload = vec![0_u8; u32::from_be_bytes(header) as usize];
            server.read_exact(&mut payload).await.expect("payload");
            let mut frame = BytesMut::from(header.as_slice());
            frame.extend_from_slice(&payload);
            let request: AgentRequest = decode_frame(frame.freeze()).expect("decode");
            assert!(matches!(
                request.payload,
                Some(agent_request::Payload::RestoreSystemProxy(_))
            ));
            let response = AgentResponse {
                request_id: request.request_id,
                error: None,
                payload: Some(agent_response::Payload::State(AgentState {
                    phase: agent_v1::AgentPhase::Clean as i32,
                    operation_id: response_operation_id,
                    ..AgentState::default()
                })),
            };
            server
                .write_all(&encode_frame(&response).expect("encode"))
                .await
                .expect("write");
        });
        let client = WindowsAgentClient::for_test(pipe_name);
        let pipe = client.open_pipe().await.expect("open");
        let mut slot = Some(WindowsSystemProxyGuard {
            client,
            operation_id,
            pipe: Some(pipe),
            tunnel_lease: false,
        });
        WindowsSystemProxyGuard::shutdown_slot(&mut slot)
            .await
            .expect("restore standalone lease");
        assert!(slot.is_none());
        server_task.await.expect("server task");
    }
}
