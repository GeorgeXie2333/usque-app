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
        PrepareTunnelRequest, RecoverOrphanedRequest, RestartAutomaticRecoveryRequest,
        RestoreSystemProxyRequest, ResumeTunnelRequest, RollbackTunnelRequest, agent_request,
        agent_response,
    },
    decode_frame, encode_frame,
};
use usque_platform::packet_ring::{
    PACKET_RING_LAYOUT_VERSION, PacketDirection, PacketRingError, SharedPacketRing,
};
use usque_transport::{
    ConnectionTimelineSnapshot, DataPlaneRuntime, DirectEgressLease, DirectProtocol,
    EndpointPinRefresher, GeoDirectPolicy, ManagedTunnelMonitor, MasqueTlsIdentity,
    NoopSocketProtector, RuntimeHealth, RuntimePath, SPLIT_DNS_IPV4, SPLIT_DNS_IPV6,
    STALE_GENERATION_REASON, SocketHandle, SocketProtector, TrafficSnapshot, TransportError,
    TunPacketIo, resolve_physical_host,
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
const AGENT_RECOVERY_TIMEOUT: Duration = Duration::from_secs(30);
const AGENT_RECOVERY_POLL_INTERVAL: Duration = Duration::from_millis(100);
const AGENT_START_POLL_INTERVAL: Duration = Duration::from_millis(50);
const AGENT_START_RECHECK_INTERVAL: Duration = Duration::from_millis(250);
const PUMP_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);
const DEFAULT_PACKET_RING_CAPACITY: u32 = 4 * 1024 * 1024;
const PACKET_WAKE_BATCH: usize = 64;
const PACKET_RING_RETRY_INTERVAL: Duration = Duration::from_millis(1);
const PHYSICAL_NETWORK_POLL_INTERVAL: Duration = Duration::from_millis(500);
const AUTOMATIC_RECOVERY_ATTEMPT_LIMIT: u32 = 3;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AutomaticRecoveryFailure {
    pub(crate) operation_id: String,
    pub(crate) journal_generation: u64,
    pub(crate) code: String,
    pub(crate) message: String,
    pub(crate) retryable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AutomaticRecoveryObservation {
    Clean,
    Pending {
        operation_id: String,
        journal_generation: u64,
    },
    Exhausted(AutomaticRecoveryFailure),
    Blocked(AutomaticRecoveryFailure),
}

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
            .map_err(|error| socket_lease_error("ACQUIRE_DIRECT_EGRESS", error))?;
        self.verify_generation(expected_generation, agent_generation)?;
        bind_socket_to_interface(socket, remote, lease.interface_index)?;
        let current = self
            .agent
            .get_physical_network_info(self.operation_id)
            .await
            .map_err(|error| socket_lease_error("VERIFY_PHYSICAL_NETWORK", error))?;
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

fn require_open_vpn_transaction(open: bool, operation_id: Uuid) -> Result<(), WindowsVpnError> {
    if !open {
        return Err(WindowsVpnError::RecoveryRequired {
            phase: agent_v1::AgentPhase::RecoveryRequired as i32,
            operation_id: operation_id.to_string(),
        });
    }
    Ok(())
}

fn socket_lease_error(stage: &'static str, error: WindowsVpnError) -> String {
    if matches!(&error, WindowsVpnError::Remote { code, .. } if code == "AGENT_STALE_GENERATION") {
        return STALE_GENERATION_REASON.to_owned();
    }
    let code = match &error {
        WindowsVpnError::RpcTimeout => "AGENT_RPC_TIMEOUT",
        WindowsVpnError::InvalidDirectEgressLease => "AGENT_INVALID_DIRECT_EGRESS_LEASE",
        WindowsVpnError::Frame(_)
        | WindowsVpnError::FrameTooLarge(_)
        | WindowsVpnError::ResponseIdMismatch
        | WindowsVpnError::MissingResponse
        | WindowsVpnError::UnexpectedResponse(_) => "AGENT_INVALID_RESPONSE",
        _ => error.diagnostic_code(),
    };
    // Never log raw Agent messages, I/O details, addresses or caller identity.
    tracing::warn!(
        reason_code = stage,
        error_code = code,
        "Windows exact-generation egress preparation failed"
    );
    format!("Windows exact-generation egress preparation failed ({stage}: {code})")
}

pub(crate) fn log_recovery_error(error: &crate::ControlServiceError) {
    if let crate::ControlServiceError::PlatformRecovery {
        code,
        message,
        retryable,
    } = error
    {
        let adapter = sanitized_adapter_recovery_detail(message);
        let historical_terminal = matches!(
            *code,
            "WINDOWS_RECOVERY_EXHAUSTED" | "WINDOWS_RECOVERY_BLOCKED"
        );
        tracing::warn!(error_code = *code, retryable, historical_terminal, adapter_cleanup = ?adapter,
            "Windows network recovery did not complete");
    }
}

fn sanitized_adapter_recovery_detail(message: &str) -> Option<String> {
    let (_, detail) = message.split_once("restore WintunAdapter: ")?;
    let mut safe = Vec::new();
    for token in detail.split(';').next()?.split_whitespace().take(16) {
        let Some((key, value)) = token.split_once('=') else {
            continue;
        };
        let accepted = match key {
            "stage" => matches!(value, "Observe" | "Request" | "Confirm"),
            "failure" => matches!(value, "Pending" | "Identity" | "Native"),
            "interface" | "device" => matches!(value, "None" | "Some(true)" | "Some(false)"),
            "request_accepted" => matches!(value, "true" | "false"),
            "elapsed_ms" => value.parse::<u64>().is_ok(),
            "win32" => {
                value == "None"
                    || value
                        .strip_prefix("Some(")
                        .and_then(|s| s.strip_suffix(')'))
                        .is_some_and(|s| s.parse::<u32>().is_ok())
            }
            "api" => matches!(
                value,
                "None"
                    | "Some(GetIfTable2)"
                    | "Some(SetupDiGetClassDevsW)"
                    | "Some(SetupDiEnumDeviceInfo)"
                    | "Some(SetupDiGetDeviceInstanceIdW)"
                    | "Some(SetupDiSetClassInstallParamsW)"
                    | "Some(SetupDiCallClassInstaller)"
                    | "Some(Other)"
            ),
            _ => false,
        };
        if accepted {
            safe.push(token);
        }
    }
    Some(format!("WintunAdapter {}", safe.join(" ")))
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
                let state = client.connection_state(&capabilities).await?;
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

// Only for a Prepared Gate transaction: final traffic is not admitted yet.
// Drop the startup future first so its producer guards observe cancellation,
// then release the pipe independently of asynchronous worker joins. Agent EOF
// recovery still checks the operation, owner and lease epoch after its grace.
async fn await_prepared_gate_startup<T, E: From<TransportError>, L>(
    startup_cancel: &CancellationToken,
    startup_lease: &mut Option<L>,
    startup: impl std::future::Future<Output = Result<T, E>>,
) -> Result<T, E> {
    let mut startup = Box::pin(startup);
    tokio::select! {
        biased;
        _ = startup_cancel.cancelled() => {
            drop(startup);
            let released = startup_lease.take().is_some();
            tracing::info!(recovery_event = "PREPARED_STARTUP_CANCELLED", startup_lease_released = released, "Cancelled prepared startup released its recovery lease");
            Err(TransportError::TunnelClosed.into())
        }
        result = &mut startup => result,
    }
}

pub(crate) struct WindowsVpnRuntime {
    agent: WindowsAgentClient,
    operation_id: Uuid,
    monitor: WindowsVpnMonitor,
    cancellation: CancellationToken,
    mapping: Option<Arc<PacketSessionMapping>>,
    tasks: Vec<JoinHandle<()>>,
    lifetime: CancellationToken,
    liveness: Option<tokio_util::task::AbortOnDropHandle<()>>,
    startup_lease: Option<NamedPipeClient>,
    pump_failure_tx: watch::Sender<Option<WindowsPumpFailure>>,
    listeners: Vec<SocketAddr>,
    socks5_listeners: Vec<SocketAddr>,
    http_listeners: Vec<SocketAddr>,
    system_proxy: Option<WindowsSystemProxyGuard>,
    transaction_open: bool,
    tunnel: Option<DataPlaneRuntime>,
    bootstrap: Option<WarpBootstrap>,
    // Present when this runtime created the VPN-bound MASQUE protector.
    socket_protector: Option<Arc<WindowsVpnSocketProtector>>,
}

struct WarpBootstrap {
    identity: MasqueTlsIdentity,
    refresher: Arc<dyn EndpointPinRefresher>,
    registration_api: Vec<SocketAddr>,
    status: usque_core::vpngate::GateStatus,
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
    fn blocked_chain(
        agent: WindowsAgentClient,
        operation_id: Uuid,
        startup_lease: Option<NamedPipeClient>,
        mut bootstrap: WarpBootstrap,
        protector: Option<Arc<WindowsVpnSocketProtector>>,
        error: &TransportError,
    ) -> Self {
        let failure = error.failure(None, None);
        bootstrap.status.failure = Some(match error {
            TransportError::VpnGate(reason) => *reason,
            _ if !failure.retryable => usque_core::vpngate::GateFailure::Configuration,
            _ => usque_core::vpngate::GateFailure::Transport,
        });
        let path = RuntimePath {
            transport: failure.transport.unwrap_or(usque_core::Transport::Http2),
            endpoint_family: failure
                .address_family
                .unwrap_or(usque_core::AddressFamily::Ipv4),
            ipv4_available: false,
            ipv6_available: false,
        };
        let lifetime = CancellationToken::new();
        let (pump_failure_tx, pump_failure) = watch::channel(None);
        Self {
            agent,
            operation_id,
            monitor: WindowsVpnMonitor {
                tunnel: ManagedTunnelMonitor::failed(path, error),
                pump_failure,
                agent_disconnected: watch::channel(false).1,
            },
            cancellation: lifetime.child_token(),
            lifetime,
            mapping: None,
            tasks: Vec::new(),
            liveness: None,
            startup_lease,
            pump_failure_tx,
            listeners: Vec::new(),
            socks5_listeners: Vec::new(),
            http_listeners: Vec::new(),
            system_proxy: None,
            transaction_open: true,
            tunnel: None,
            bootstrap: Some(bootstrap),
            socket_protector: protector,
        }
    }

    pub(crate) fn l4_snapshot(&self) -> Option<usque_core::L4Snapshot> {
        self.tunnel.as_ref().and_then(DataPlaneRuntime::l4_snapshot)
    }
    pub(crate) async fn start(
        profile: &Profile,
        identity: MasqueTlsIdentity,
        pin_refresher: Arc<dyn EndpointPinRefresher>,
        geo_policy: Arc<GeoDirectPolicy>,
        gate: usque_transport::VpnGateStart,
    ) -> Result<Self, WindowsVpnError> {
        let startup_cancel = gate.cancellation.clone();
        let geo_enabled = geo_policy.is_enabled();
        let agent = WindowsAgentClient::production();
        let capabilities = agent.get_capabilities().await?;
        validate_capabilities(&capabilities, profile.kill_switch)?;
        if profile.vpn_gate.enabled && !capabilities.deferred_network_configuration {
            return Err(WindowsVpnError::MissingCapabilities(
                "deferred_network_configuration".into(),
            ));
        }
        // Old DNS/WFP state can itself prevent endpoint resolution. Complete
        // guarded local recovery before ANY startup DNS or MASQUE operation.
        let state = agent.connection_state(&capabilities).await?;
        // Still resolve before installing a new fail-closed policy.
        let registration_api = resolve_registration_api().await?;
        if startup_cancel.is_cancelled() {
            return Err(TransportError::TunnelClosed.into());
        }
        let (operation_id, resuming, mut startup_lease) =
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
                    let lease = if profile.vpn_gate.enabled {
                        Some(agent.begin_chain_transition_lease(operation_id).await?)
                    } else {
                        None
                    };
                    (operation_id, true, lease)
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

        let bootstrap = profile.vpn_gate.enabled.then(|| WarpBootstrap {
            identity: identity.clone(),
            refresher: pin_refresher.clone(),
            registration_api: registration_api.clone(),
            status: usque_core::vpngate::GateStatus {
                stage: usque_core::vpngate::GateStage::Error,
                warp_stage: Some("error".into()),
                current_server: gate.selected.as_ref().map(|(server, _)| server.clone()),
                failure: Some(usque_core::vpngate::GateFailure::Transport),
                ..Default::default()
            },
        });
        let preparation =
            prepare_vpn_protector(&agent, operation_id, registration_api, profile, geo_enabled);
        let preparation = if profile.vpn_gate.enabled {
            await_prepared_gate_startup(&startup_cancel, &mut startup_lease, preparation).await
        } else {
            preparation.await
        };
        let protector = match preparation {
            Ok(protector) => protector,
            Err(error) => {
                if let Some(bootstrap) = bootstrap {
                    return Ok(Self::blocked_chain(
                        agent,
                        operation_id,
                        startup_lease,
                        bootstrap,
                        None,
                        &TransportError::VpnGate(error.gate_failure()),
                    ));
                }
                return Err(fail_startup(
                    &agent,
                    operation_id,
                    resuming,
                    "PHYSICAL_NETWORK_PREPARATION_FAILED",
                    error,
                )
                .await);
            }
        };
        let transport_protector: Arc<dyn SocketProtector> = protector.clone();

        let startup = Box::pin(DataPlaneRuntime::start_with_vpngate(
            profile,
            identity,
            transport_protector,
            Some(pin_refresher),
            geo_policy,
            gate,
        ));
        let startup = if profile.vpn_gate.enabled {
            await_prepared_gate_startup(&startup_cancel, &mut startup_lease, startup).await
        } else {
            startup.await
        };
        let tunnel = match startup {
            Ok(tunnel) => tunnel,
            Err(error) => {
                if let Some(bootstrap) = bootstrap {
                    return Ok(Self::blocked_chain(
                        agent,
                        operation_id,
                        startup_lease,
                        bootstrap,
                        Some(protector),
                        &error,
                    ));
                }
                return Err(fail_startup(
                    &agent,
                    operation_id,
                    resuming,
                    "TRANSPORT_START_FAILED",
                    error.into(),
                )
                .await);
            }
        };
        if profile.vpn_gate.enabled {
            let lifetime = CancellationToken::new();
            let (pump_failure_tx, pump_failure) = watch::channel(None);
            let (_, agent_disconnected) = watch::channel(false);
            let mut runtime = Self {
                agent,
                operation_id,
                monitor: WindowsVpnMonitor {
                    tunnel: tunnel.monitor(),
                    pump_failure,
                    agent_disconnected,
                },
                cancellation: lifetime.child_token(),
                lifetime,
                mapping: None,
                tasks: Vec::new(),
                liveness: None,
                startup_lease,
                pump_failure_tx,
                listeners: Vec::new(),
                socks5_listeners: Vec::new(),
                http_listeners: Vec::new(),
                system_proxy: None,
                transaction_open: true,
                tunnel: Some(tunnel),
                bootstrap: None,
                socket_protector: Some(protector),
            };
            if runtime.gate_status().stage != usque_core::vpngate::GateStage::Error {
                // The helper records an error and closes admission on failure.
                // Ownership of the WFP guard remains until explicit disconnect.
                let _ = runtime.finish_chain_network(profile, &startup_cancel).await;
            }
            return Ok(runtime);
        }
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
        if let Some(tunnel) = &self.tunnel {
            let health = tunnel.health();
            if !matches!(health, RuntimeHealth::Connected { .. }) {
                return health;
            }
        }
        self.monitor.health()
    }

    pub(crate) fn statistics(&self) -> TrafficSnapshot {
        self.monitor.statistics()
    }

    pub(crate) fn connection_timeline(&self) -> ConnectionTimelineSnapshot {
        self.tunnel.as_ref().map_or_else(
            || self.monitor.connection_timeline(),
            DataPlaneRuntime::connection_timeline,
        )
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
            .map(DataPlaneRuntime::diagnostic_dns_context)
    }

    pub(crate) fn failure(&self) -> Option<String> {
        self.monitor.failure()
    }

    pub(crate) fn internal_networks(
        &self,
    ) -> Option<(
        usque_transport::InternalNetwork,
        usque_transport::InternalNetwork,
    )> {
        self.tunnel
            .as_ref()
            .map(|r| (r.internal_network(), r.warp_internal_network()))
    }
    pub(crate) fn gate_status(&self) -> usque_core::vpngate::GateStatus {
        self.tunnel.as_ref().map_or_else(
            || {
                self.bootstrap
                    .as_ref()
                    .map(|pending| pending.status.clone())
                    .unwrap_or_default()
            },
            DataPlaneRuntime::gate_status,
        )
    }

    pub(crate) fn needs_warp_bootstrap(&self) -> bool {
        self.bootstrap.is_some()
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
        require_open_vpn_transaction(self.transaction_open, self.operation_id)?;
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

    pub(crate) async fn replace_gate(
        &mut self,
        profile: &Profile,
        selected: Option<(
            usque_core::vpngate::ServerSummary,
            usque_core::vpngate::PreparedProfile,
        )>,
        policy: Arc<GeoDirectPolicy>,
        status: watch::Sender<usque_core::vpngate::GateStatus>,
        startup_cancel: &CancellationToken,
    ) -> Result<(), WindowsVpnError> {
        self.quiesce_final();
        require_open_vpn_transaction(self.transaction_open, self.operation_id)?;
        self.agent.begin_chain_transition(self.operation_id).await?;
        self.stop_packet_pumps().await;
        if self.mapping.is_some() {
            self.agent.close_packet_session(self.operation_id).await?;
        }
        if let Some(bootstrap) = &self.bootstrap {
            if self.socket_protector.is_none() {
                self.socket_protector = Some(
                    prepare_vpn_protector(
                        &self.agent,
                        self.operation_id,
                        bootstrap.registration_api.clone(),
                        profile,
                        policy.is_enabled(),
                    )
                    .await
                    .map_err(|error| TransportError::VpnGate(error.gate_failure()))?,
                );
            }
            let protector = self
                .socket_protector
                .clone()
                .ok_or(WindowsVpnError::MissingMasqueRuntime)?;
            let startup = Box::pin(DataPlaneRuntime::start_with_vpngate(
                profile,
                bootstrap.identity.clone(),
                protector,
                Some(bootstrap.refresher.clone()),
                policy,
                usque_transport::VpnGateStart {
                    selected,
                    status: Some(status),
                    cancellation: startup_cancel.clone(),
                },
            ));
            let tunnel =
                await_prepared_gate_startup(startup_cancel, &mut self.startup_lease, startup)
                    .await?;
            self.monitor.tunnel = tunnel.monitor();
            self.tunnel = Some(tunnel);
            self.bootstrap = None;
            return self.finish_chain_network(profile, startup_cancel).await;
        }
        let tunnel = self
            .tunnel
            .as_mut()
            .ok_or(WindowsVpnError::MissingMasqueRuntime)?;
        tunnel.detach_tun();
        if let Err(error) = tunnel
            .replace_gate(profile, selected, policy, status, startup_cancel)
            .await
        {
            let reason = match &error {
                TransportError::VpnGate(reason) => *reason,
                _ => usque_core::vpngate::GateFailure::Transport,
            };
            tunnel.fail_gate(reason).await;
            return Err(error.into());
        }
        self.finish_chain_network(profile, startup_cancel).await
    }

    /// Retain the operation's guard and underlay on every finalization failure.
    async fn finish_chain_network(
        &mut self,
        profile: &Profile,
        startup_cancel: &CancellationToken,
    ) -> Result<(), WindowsVpnError> {
        let tunnel = self
            .tunnel
            .as_mut()
            .ok_or(WindowsVpnError::MissingMasqueRuntime)?;
        if profile.vpn_gate.enabled
            && tunnel.gate_status().stage == usque_core::vpngate::GateStage::Error
        {
            return Err(TransportError::VpnGate(
                tunnel
                    .gate_status()
                    .failure
                    .unwrap_or(usque_core::vpngate::GateFailure::Transport),
            )
            .into());
        }
        let finalization = async {
            let network = tunnel.network_parameters();
            let mut final_profile = profile.clone();
            final_profile.mtu = network.mtu;
            if profile.dns_mode == usque_core::DnsMode::Tunnel {
                final_profile.dns_servers = network.dns_servers;
            }
            // Use the immutable bootstrap policy recorded for this operation.
            let agent_state = self.agent.get_state().await?;
            let mut plan = *agent_state
                .plan
                .ok_or(WindowsVpnError::MissingMasqueRuntime)?;
            let final_values = tunnel_plan_from_assignment(
                &final_profile,
                tunnel.assigned_ipv4(),
                tunnel.assigned_ipv6(),
                &[],
                !profile.geo_direct_countries.is_empty(),
            );
            plan.assigned_ipv4 = final_values.assigned_ipv4;
            plan.assigned_ipv6 = final_values.assigned_ipv6;
            plan.dns_servers = final_values.dns_servers;
            plan.split_dns = final_values.split_dns;
            plan.mtu = final_values.mtu;
            plan.defer_network_configuration = false;
            self.agent.finalize_tunnel(self.operation_id, plan).await?;
            let io = tunnel.attach_tun()?;
            let handles = self
                .agent
                .open_packet_session(self.operation_id, DEFAULT_PACKET_RING_CAPACITY)
                .await?;
            let mapping = Arc::new(PacketSessionMapping::attach(handles)?);
            self.mapping = Some(mapping.clone());
            self.cancellation = self.lifetime.child_token();
            self.pump_failure_tx.send_replace(None);
            self.monitor.tunnel = tunnel.monitor();
            self.tasks = start_packet_pumps(
                io,
                mapping.clone(),
                self.monitor.tunnel.clone(),
                self.cancellation.clone(),
                self.pump_failure_tx.clone(),
            );
            self.agent.commit(self.operation_id).await?;
            if self.liveness.is_none() {
                let lease = match self.startup_lease.take() {
                    Some(lease) => {
                        self.agent
                            .promote_liveness_lease(self.operation_id, lease)
                            .await?
                    }
                    None => self.agent.open_liveness_lease(self.operation_id).await?,
                };
                let (disconnected_tx, disconnected_rx) = watch::channel(false);
                self.monitor.agent_disconnected = disconnected_rx;
                self.liveness = Some(tokio_util::task::AbortOnDropHandle::new(
                    start_agent_liveness_watch(
                        lease,
                        mapping,
                        self.lifetime.clone(),
                        self.pump_failure_tx.clone(),
                        disconnected_tx,
                    ),
                ));
            }
            if profile.frontends.http && profile.proxy.system_proxy && self.system_proxy.is_none() {
                let listener = loopback_http_listener(tunnel.http_listeners())
                    .ok_or(WindowsVpnError::MissingSystemProxyListener)?;
                self.system_proxy = Some(
                    WindowsSystemProxyGuard::start_for_tunnel(listener, self.operation_id).await?,
                );
            }
            if startup_cancel.is_cancelled() {
                return Err(TransportError::TunnelClosed.into());
            }
            tunnel.activate_final().await?;
            Ok::<_, WindowsVpnError>(())
        };
        let result = tokio::select! {
            biased;
            _ = startup_cancel.cancelled() => Err(WindowsVpnError::from(TransportError::TunnelClosed)),
            result = finalization => result,
        };
        if let Err(error) = &result {
            // Close admission and packet producers before any asynchronous
            // teardown. Failure handling must not delay the stop boundary.
            tunnel.quiesce_final();
            self.cancellation.cancel();
            if let Some(mapping) = &self.mapping {
                mapping.signal_shutdown();
            }
            let reason = error.gate_failure();
            tunnel.fail_gate(reason).await;
            let path = tunnel.path();
            self.pump_failure_tx.send_replace(Some(match error {
                WindowsVpnError::Transport(error) => {
                    WindowsPumpFailure::transport("VPN Gate switch failed", error, path)
                }
                _ => WindowsPumpFailure::agent("VPN Gate final network configuration failed"),
            }));
        }
        self.listeners = if result.is_ok() {
            tunnel.listeners().to_vec()
        } else {
            Vec::new()
        };
        self.socks5_listeners = if result.is_ok() {
            tunnel.socks5_listeners().to_vec()
        } else {
            Vec::new()
        };
        self.http_listeners = if result.is_ok() {
            tunnel.http_listeners().to_vec()
        } else {
            Vec::new()
        };
        result.map_err(|error| TransportError::VpnGate(error.gate_failure()).into())
    }

    pub(crate) fn quiesce_final(&mut self) {
        if let Some(tunnel) = &mut self.tunnel {
            tunnel.quiesce_final();
        }
        self.listeners.clear();
        self.socks5_listeners.clear();
        self.http_listeners.clear();
    }
    pub(crate) async fn fail_gate(&mut self, reason: usque_core::vpngate::GateFailure) {
        self.quiesce_final();
        if let Some(tunnel) = &mut self.tunnel {
            tunnel.fail_gate(reason).await;
        }
        if let Some(bootstrap) = &mut self.bootstrap {
            bootstrap.status.failure = Some(reason);
        }
    }

    /// Wrap an already-running MASQUE session with Wintun/WFP. On failure the
    /// caller receives the live MASQUE runtime back so SOCKS/HTTP survive.
    pub(crate) async fn attach_existing(
        profile: &Profile,
        tunnel: DataPlaneRuntime,
    ) -> Result<Self, (DataPlaneRuntime, WindowsVpnError)> {
        let agent = WindowsAgentClient::production();
        let capabilities = match agent.get_capabilities().await {
            Ok(capabilities) => capabilities,
            Err(error) => return Err((tunnel, error)),
        };
        if let Err(error) = validate_capabilities(&capabilities, profile.kill_switch) {
            return Err((tunnel, error));
        }
        if profile.vpn_gate.enabled && !capabilities.deferred_network_configuration {
            return Err((
                tunnel,
                WindowsVpnError::MissingCapabilities("deferred_network_configuration".into()),
            ));
        }
        let state = match agent.connection_state(&capabilities).await {
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
        let registration_api = match resolve_registration_api().await {
            Ok(addresses) => addresses,
            Err(error) => return Err((tunnel, error)),
        };
        let operation_id = Uuid::new_v4();
        let mut effective = profile.clone();
        if profile.vpn_gate.enabled {
            let network = tunnel.network_parameters();
            effective.mtu = network.mtu;
            if profile.dns_mode == usque_core::DnsMode::Tunnel {
                effective.dns_servers = network.dns_servers;
            }
        }
        let plan = tunnel_plan_from_assignment(
            &effective,
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
    pub(crate) async fn detach_into_masque(&mut self) -> Result<DataPlaneRuntime, WindowsVpnError> {
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
            // The previous Clean response is not a lease on future Agent
            // state. A cleanup retry must obtain a fresh acknowledgement.
            self.agent.get_state().await.and_then(|state| {
                validate_proxy_detach_state(&state)?;
                Ok(state)
            })
        };
        if rollback.is_ok() {
            self.transaction_open = false;
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
        require_open_vpn_transaction(self.transaction_open, self.operation_id)?;
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
        self.bootstrap = None;
        // The replacement runtime must adopt the same persistent transaction.
        // Drop must therefore not perform a rollback between detach and resume.
        self.transaction_open = false;
        Ok(())
    }

    pub(crate) async fn shutdown(&mut self) -> Result<(), WindowsVpnError> {
        let started = tokio::time::Instant::now();
        self.bootstrap = None;
        // Cut packet forwarding before any Agent RPC. Rollback may need to
        // restore routes, DNS, WFP, and the adapter, but no user packet may
        // remain attached to MASQUE while that cleanup is in progress.
        self.cancel_immediately();
        tracing::info!(
            recovery_event = "PACKET_PUMPS_JOIN_STARTED",
            "Waiting for Windows packet pumps to stop"
        );
        self.stop_packet_pumps().await;
        tracing::info!(
            recovery_event = "PACKET_PUMPS_JOIN_FINISHED",
            "Windows packet pumps stopped"
        );
        // Join/cancel all MASQUE, proxy and GEO producers before asking the
        // Agent to roll back. Otherwise a stopped TUN consumer still receives
        // packets and direct-egress leases can outlive platform cleanup.
        if let Some(mut tunnel) = self.tunnel.take() {
            tracing::info!(
                recovery_event = "DATAPLANE_JOIN_STARTED",
                "Waiting for Windows data plane shutdown"
            );
            tunnel.shutdown().await;
            tracing::info!(
                recovery_event = "DATAPLANE_JOIN_FINISHED",
                "Windows data plane shutdown finished"
            );
        }
        let system_proxy_result = match self.system_proxy.as_mut() {
            Some(system_proxy) => system_proxy.shutdown().await,
            None => Ok(()),
        };
        let rollback = if self.transaction_open {
            self.agent.rollback_for_disconnect(self.operation_id).await
        } else {
            Ok(AgentState::default())
        };
        if rollback.is_ok() {
            self.transaction_open = false;
        }
        tracing::info!(
            recovery_event = "WINDOWS_ROLLBACK_RETURNED",
            success = rollback.is_ok(),
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Windows disconnect rollback returned"
        );
        system_proxy_result?;
        rollback.map(|_| ())
    }

    pub(crate) fn cancel_immediately(&mut self) {
        self.lifetime.cancel();
        if let Some(tunnel) = self.tunnel.as_mut() {
            tunnel.quiesce_final();
            tunnel.cancel_immediately();
        }
        if let Some(protector) = self.socket_protector.as_ref() {
            protector.monitor_cancel.cancel();
        }
        self.cancel_packet_pumps();
        // Ordinary Gate failures retain these leases for in-place retry. An
        // explicit stop releases both only after final forwarding is closed;
        // recovery cannot be held hostage by the following async shutdown.
        let active_released = self.liveness.take().is_some();
        let startup_released = self.startup_lease.take().is_some();
        if active_released || startup_released {
            tracing::info!(
                recovery_event = "TUNNEL_LEASES_RELEASED",
                startup_lease_released = startup_released,
                active_lease_released = active_released,
                "Stopped forwarding and released Windows tunnel leases"
            );
        }
    }

    fn cancel_packet_pumps(&mut self) {
        if let Some(mapping) = &self.mapping {
            mapping.signal_shutdown();
        }
        self.cancellation.cancel();
        for task in &self.tasks {
            task.abort();
        }
    }

    async fn stop_packet_pumps(&mut self) {
        // Hot TUN detach must keep the shared MASQUE/proxy runtime alive.
        self.cancel_packet_pumps();
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

async fn prepare_vpn_protector(
    agent: &WindowsAgentClient,
    operation_id: Uuid,
    registration_api: Vec<SocketAddr>,
    profile: &Profile,
    geo_enabled: bool,
) -> Result<Arc<WindowsVpnSocketProtector>, WindowsVpnError> {
    let physical_info = agent.get_physical_network_info(operation_id).await?;
    let dns_servers = physical_dns_endpoints(&physical_info)?;
    validate_physical_dns(geo_enabled, profile.direct_dns.mode, &dns_servers)?;
    let generation = physical_info.generation;
    let protector = Arc::new(WindowsVpnSocketProtector {
        registration_api,
        agent: agent.clone(),
        operation_id,
        physical: RwLock::new(WindowsPhysicalState {
            generation,
            agent_generation: Some(generation),
            dns_servers,
            family_mask: physical_info
                .interfaces
                .iter()
                .fold(0, |mask, interface| mask | interface.address_family_mask),
        }),
        monitor_cancel: CancellationToken::new(),
        proxy_mode: AtomicBool::new(false),
    });
    start_physical_network_monitor(&protector);
    Ok(protector)
}

fn validate_physical_dns(
    geo_enabled: bool,
    mode: usque_core::DirectDnsMode,
    servers: &[SocketAddr],
) -> Result<(), WindowsVpnError> {
    if geo_enabled && mode == usque_core::DirectDnsMode::PhysicalSystem && servers.is_empty() {
        Err(WindowsVpnError::PhysicalDnsUnavailable)
    } else {
        Ok(())
    }
}

async fn fail_startup(
    agent: &WindowsAgentClient,
    operation_id: Uuid,
    resuming: bool,
    stage: &'static str,
    startup: WindowsVpnError,
) -> WindowsVpnError {
    // Keep the FIRST failure even if cleanup times out or fails. These codes
    // are allowlisted constants, not remote messages, addresses or identities.
    tracing::warn!(
        reason_code = stage,
        error_code = startup.diagnostic_code(),
        "Windows VPN startup failed; attempting rollback"
    );
    match abort_startup(agent, operation_id, resuming, stage).await {
        Ok(()) => startup,
        Err(recovery) => {
            tracing::warn!(
                reason_code = stage,
                error_code = recovery.diagnostic_code(),
                "Windows VPN startup rollback remains incomplete"
            );
            WindowsVpnError::StartupAndRecovery {
                stage,
                startup: Box::new(startup),
                recovery: Box::new(recovery),
            }
        }
    }
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
    mut tunnel: DataPlaneRuntime,
    agent: WindowsAgentClient,
    operation_id: Uuid,
    resuming: bool,
    startup_lease: Option<NamedPipeClient>,
) -> Result<WindowsVpnRuntime, (DataPlaneRuntime, WindowsVpnError)> {
    let tun_io = match tunnel.attach_tun() {
        Ok(tun_io) => tun_io,
        Err(error) => {
            let error = fail_startup(
                &agent,
                operation_id,
                resuming,
                "TUN_ATTACH_FAILED",
                error.into(),
            )
            .await;
            return Err((tunnel, error));
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
            let error = fail_startup(
                &agent,
                operation_id,
                resuming,
                "PACKET_SESSION_FAILED",
                error,
            )
            .await;
            return Err((tunnel, error));
        }
    };
    let mapping = match PacketSessionMapping::attach(handles) {
        Ok(mapping) => Arc::new(mapping),
        Err(error) => {
            tunnel.detach_tun();
            let error = fail_startup(
                &agent,
                operation_id,
                resuming,
                "PACKET_MAPPING_FAILED",
                error,
            )
            .await;
            return Err((tunnel, error));
        }
    };

    let lifetime = CancellationToken::new();
    let cancellation = lifetime.child_token();
    let (pump_failure_tx, pump_failure) = watch::channel(None);
    let (agent_disconnected_tx, agent_disconnected) = watch::channel(false);
    let listeners = tunnel.listeners().to_vec();
    let socks5_listeners = tunnel.socks5_listeners().to_vec();
    let http_listeners = tunnel.http_listeners().to_vec();
    let tunnel_monitor = tunnel.monitor();
    let tasks = start_packet_pumps(
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
        let error = fail_startup(&agent, operation_id, resuming, "COMMIT_FAILED", error).await;
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
            let error = fail_startup(
                &agent,
                operation_id,
                resuming,
                "LIVENESS_LEASE_FAILED",
                error,
            )
            .await;
            return Err((tunnel, error));
        }
    };
    let liveness = tokio_util::task::AbortOnDropHandle::new(start_agent_liveness_watch(
        lease,
        Arc::clone(&mapping),
        lifetime.clone(),
        pump_failure_tx.clone(),
        agent_disconnected_tx,
    ));

    let system_proxy = if profile.frontends.http && profile.proxy.system_proxy {
        let Some(listener) = loopback_http_listener(&http_listeners) else {
            mapping.signal_shutdown();
            cancellation.cancel();
            stop_tasks(tasks).await;
            tunnel.detach_tun();
            let error = fail_startup(
                &agent,
                operation_id,
                resuming,
                "SYSTEM_PROXY_LISTENER_MISSING",
                WindowsVpnError::MissingSystemProxyListener,
            )
            .await;
            return Err((tunnel, error));
        };
        match WindowsSystemProxyGuard::start_for_tunnel(listener, operation_id).await {
            Ok(guard) => Some(guard),
            Err(error) => {
                mapping.signal_shutdown();
                cancellation.cancel();
                stop_tasks(tasks).await;
                tunnel.detach_tun();
                let error = fail_startup(
                    &agent,
                    operation_id,
                    resuming,
                    "SYSTEM_PROXY_APPLY_FAILED",
                    error,
                )
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

    if let Err(error) = tunnel.activate_final().await {
        mapping.signal_shutdown();
        cancellation.cancel();
        stop_tasks(tasks).await;
        tunnel.detach_tun();
        if let Some(mut guard) = system_proxy {
            let _ = guard.shutdown().await;
        }
        let error = fail_startup(
            &agent,
            operation_id,
            resuming,
            "FINAL_ADMISSION_FAILED",
            error.into(),
        )
        .await;
        return Err((tunnel, error));
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
        mapping: Some(mapping),
        tasks,
        lifetime,
        liveness: Some(liveness),
        startup_lease: None,
        pump_failure_tx,
        listeners,
        socks5_listeners,
        http_listeners,
        system_proxy,
        transaction_open: true,
        tunnel: Some(tunnel),
        bootstrap: None,
        socket_protector: None,
    })
}

pub(crate) async fn catalogue_physical_network_permitted() -> bool {
    // Read only. A refresh never starts recovery or changes an Agent policy.
    WindowsAgentClient::production()
        .get_state()
        .await
        .is_ok_and(|state| state.phase == agent_v1::AgentPhase::Clean as i32)
}

fn tunnel_plan(
    profile: &Profile,
    identity: &MasqueTlsIdentity,
    registration_api: &[SocketAddr],
    split_dns: bool,
) -> agent_v1::TunnelPlan {
    let mut plan = tunnel_plan_from_assignment(
        profile,
        identity.assigned_ipv4,
        identity.assigned_ipv6,
        registration_api,
        split_dns,
    );
    plan.defer_network_configuration = profile.vpn_gate.enabled;
    plan
}

fn tunnel_plan_from_assignment(
    profile: &Profile,
    assigned_ipv4: std::net::Ipv4Addr,
    assigned_ipv6: std::net::Ipv6Addr,
    registration_api: &[SocketAddr],
    split_dns: bool,
) -> agent_v1::TunnelPlan {
    let split_dns = split_dns
        || profile.vpn_gate.enabled && profile.dns_mode == usque_core::DnsMode::Tunnel
        || profile.data_plane == usque_core::DataPlaneMode::L4Proxy && !profile.vpn_gate.enabled;
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
            [
                (!assigned_ipv4.is_unspecified()).then_some(SPLIT_DNS_IPV4.to_string()),
                (!assigned_ipv6.is_unspecified()).then_some(SPLIT_DNS_IPV6.to_string()),
            ]
            .into_iter()
            .flatten()
            .collect()
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
        assigned_ipv4: if assigned_ipv4.is_unspecified() {
            String::new()
        } else {
            format!("{assigned_ipv4}/32")
        },
        assigned_ipv6: if assigned_ipv6.is_unspecified() {
            String::new()
        } else {
            format!("{assigned_ipv6}/128")
        },
        endpoint_candidates,
        control_api_candidates: registration_api.iter().map(ToString::to_string).collect(),
        split_dns,
        vpn_chain: profile.vpn_gate.enabled,
        defer_network_configuration: false,
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
    mut tun_io: TunPacketIo,
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
    tun_io: &mut TunPacketIo,
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
pub(crate) struct WindowsAgentClient {
    pipe_name: Arc<str>,
    service_controller: Arc<dyn AgentServiceController>,
}

impl WindowsAgentClient {
    pub(crate) async fn recovery_preflight(
        &self,
        restart_exhausted: bool,
    ) -> Result<Option<AutomaticRecoveryObservation>, WindowsVpnError> {
        let capabilities = self.get_capabilities().await?;
        if capabilities.protocol_version != AGENT_PROTOCOL_VERSION {
            return Err(WindowsVpnError::ProtocolVersion(
                capabilities.protocol_version,
            ));
        }
        self.automatic_recovery_preflight(&capabilities, restart_exhausted)
            .await
    }
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

    async fn connection_state(
        &self,
        capabilities: &AgentCapabilities,
    ) -> Result<AgentState, WindowsVpnError> {
        self.connection_state_with_timeout(capabilities, AGENT_RECOVERY_TIMEOUT)
            .await
    }

    async fn connection_state_with_timeout(
        &self,
        capabilities: &AgentCapabilities,
        budget: Duration,
    ) -> Result<AgentState, WindowsVpnError> {
        timeout(budget, async {
            loop {
                let state = self.get_state().await.map_err(recovery_rpc_error)?;
                match agent_v1::AgentPhase::try_from(state.phase) {
                    Ok(agent_v1::AgentPhase::Clean) => {
                        require_recovered_state(&state)?;
                        return Ok(state);
                    }
                    Ok(agent_v1::AgentPhase::Active) => return Ok(state),
                    Ok(agent_v1::AgentPhase::RecoveryRequired) => {
                        if capabilities.automatic_recovery {
                            return Err(automatic_recovery_connection_error(&state)?);
                        }
                        if !capabilities.guarded_recovery {
                            return Err(WindowsVpnError::RecoveryUnsupported);
                        }
                        if state.packet_session_active || state.operation_id.is_empty() {
                            return Err(WindowsVpnError::RecoveryConflict);
                        }
                        let recovered = self
                            .call(agent_request::Payload::RecoverOrphaned(
                                RecoverOrphanedRequest {
                                    operation_id: state.operation_id,
                                    expected_journal_generation: state.journal_generation,
                                },
                            ))
                            .await
                            .map_err(recovery_rpc_error)?;
                        let agent_response::Payload::State(recovered) = recovered else {
                            return Err(WindowsVpnError::RecoveryFailed);
                        };
                        require_recovered_state(&recovered)?;
                        // One attempt only, even if another process changes the
                        // transaction between the reply and this fresh read.
                        let current = self.get_state().await.map_err(recovery_rpc_error)?;
                        require_recovered_state(&current)
                            .map_err(|_| WindowsVpnError::RecoveryConflict)?;
                        return Ok(current);
                    }
                    Ok(agent_v1::AgentPhase::Recovering) if capabilities.automatic_recovery => {
                        return Err(automatic_recovery_connection_error(&state)?);
                    }
                    Ok(
                        agent_v1::AgentPhase::Preparing
                        | agent_v1::AgentPhase::Prepared
                        | agent_v1::AgentPhase::Recovering,
                    ) => {
                        sleep(AGENT_RECOVERY_POLL_INTERVAL).await;
                    }
                    _ => return Err(WindowsVpnError::RecoveryConflict),
                }
            }
        })
        .await
        .map_err(|_| WindowsVpnError::RecoveryTimeout)?
    }

    async fn restart_automatic_recovery(
        &self,
        operation_id: String,
        expected_journal_generation: u64,
    ) -> Result<AutomaticRecoveryObservation, WindowsVpnError> {
        let payload = self
            .call(agent_request::Payload::RestartAutomaticRecovery(
                RestartAutomaticRecoveryRequest {
                    operation_id: operation_id.clone(),
                    expected_journal_generation,
                },
            ))
            .await?;
        let agent_response::Payload::State(state) = payload else {
            return Err(WindowsVpnError::RecoveryFailed);
        };
        if state.journal_generation < expected_journal_generation
            || (state.phase != agent_v1::AgentPhase::Clean as i32
                && state.operation_id != operation_id)
        {
            return Err(WindowsVpnError::RecoveryConflict);
        }
        automatic_recovery_observation(&state)
    }

    /// Shared by explicit Connect/Retry and observation-only internal connects.
    /// There is deliberately no loop: one request can reset the budget once.
    async fn automatic_recovery_preflight(
        &self,
        capabilities: &AgentCapabilities,
        restart_exhausted: bool,
    ) -> Result<Option<AutomaticRecoveryObservation>, WindowsVpnError> {
        if !capabilities.automatic_recovery {
            return Ok(None);
        }
        let state = self.get_state().await?;
        if state.phase == agent_v1::AgentPhase::Clean as i32 {
            require_recovered_state(&state)?;
        }
        if !matches!(
            agent_v1::AgentPhase::try_from(state.phase),
            Ok(agent_v1::AgentPhase::RecoveryRequired | agent_v1::AgentPhase::Recovering)
        ) {
            // The runtime's guarded connection_state check still validates
            // Clean/Active and older Agent transactions before any prepare.
            return Ok(None);
        }
        let observation = automatic_recovery_observation(&state)?;
        if restart_exhausted && matches!(observation, AutomaticRecoveryObservation::Exhausted(_)) {
            tracing::info!("Explicit connection requested a new Windows recovery attempt");
            self.restart_automatic_recovery(state.operation_id, state.journal_generation)
                .await
                .map(Some)
        } else {
            Ok(Some(observation))
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
                if matches!(
                    agent_v1::AgentPhase::try_from(state.phase),
                    Ok(agent_v1::AgentPhase::Active | agent_v1::AgentPhase::Prepared)
                ) && state.operation_id == operation_id.to_string() =>
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

    async fn begin_chain_transition(
        &self,
        operation_id: Uuid,
    ) -> Result<AgentState, WindowsVpnError> {
        match self
            .call(agent_request::Payload::BeginChainTransition(
                agent_v1::BeginChainTransitionRequest {
                    operation_id: operation_id.to_string(),
                    retain_startup_lease: false,
                },
            ))
            .await?
        {
            agent_response::Payload::State(state) => Ok(state),
            payload => Err(WindowsVpnError::UnexpectedResponse(payload_name(&payload))),
        }
    }

    async fn begin_chain_transition_lease(
        &self,
        operation_id: Uuid,
    ) -> Result<NamedPipeClient, WindowsVpnError> {
        let mut pipe = self.open_pipe().await?;
        let response = timeout(
            AGENT_RPC_TIMEOUT,
            self.exchange(
                &mut pipe,
                agent_request::Payload::BeginChainTransition(
                    agent_v1::BeginChainTransitionRequest {
                        operation_id: operation_id.to_string(),
                        retain_startup_lease: true,
                    },
                ),
            ),
        )
        .await
        .map_err(|_| WindowsVpnError::RpcTimeout)??;
        match response {
            agent_response::Payload::State(state)
                if state.operation_id == operation_id.to_string()
                    && matches!(
                        agent_v1::AgentPhase::try_from(state.phase),
                        Ok(agent_v1::AgentPhase::Active | agent_v1::AgentPhase::Prepared)
                    ) =>
            {
                Ok(pipe)
            }
            agent_response::Payload::State(state) => {
                Err(WindowsVpnError::UnexpectedAgentPhase(state.phase))
            }
            payload => Err(WindowsVpnError::UnexpectedResponse(payload_name(&payload))),
        }
    }

    async fn finalize_tunnel(
        &self,
        operation_id: Uuid,
        plan: agent_v1::TunnelPlan,
    ) -> Result<AgentState, WindowsVpnError> {
        match self
            .call(agent_request::Payload::FinalizeTunnel(
                agent_v1::FinalizeTunnelRequest {
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

    async fn rollback_for_disconnect(
        &self,
        operation_id: Uuid,
    ) -> Result<AgentState, WindowsVpnError> {
        let result = self.rollback(operation_id, "USER_DISCONNECT").await;
        match result {
            Err(WindowsVpnError::Remote {
                ref code,
                retryable: true,
                ..
            }) if code == "AGENT_RECOVERY_FAILED" => {
                // Wintun/PnP teardown can outlive the synchronous rollback.
                // Follow the Agent's bounded recovery for this transaction;
                // never replay the stale failure after it has reached Clean.
                let state = self.get_state().await?;
                if state.phase == agent_v1::AgentPhase::Clean as i32 {
                    require_recovered_state(&state)?;
                    return Ok(state);
                }
                if state.operation_id != operation_id.to_string() {
                    return Err(WindowsVpnError::RecoveryConflict);
                }
                if state.automatic_recovery.is_some() {
                    return Err(automatic_recovery_connection_error(&state)?);
                }
                result
            }
            Ok(state) => {
                require_recovered_state(&state)?;
                Ok(state)
            }
            Err(error) => Err(error),
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
        let lifecycle = agent_lifecycle_method(&payload);
        let started = tokio::time::Instant::now();
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
        if let Some(method) = lifecycle {
            tracing::info!(
                recovery_event = "AGENT_REQUEST_STARTED",
                method,
                "Windows Agent lifecycle request started"
            );
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
        if let Some(method) = lifecycle {
            let (journal_generation, agent_phase) = match response.payload.as_ref() {
                Some(agent_response::Payload::State(state)) => (
                    state.journal_generation,
                    agent_v1::AgentPhase::try_from(state.phase).unwrap_or_default() as i32,
                ),
                _ => (0, 0),
            };
            tracing::info!(
                recovery_event = "AGENT_REQUEST_RETURNED",
                method,
                success = response.error.is_none() && response.payload.is_some(),
                journal_generation,
                agent_phase,
                elapsed_ms = started.elapsed().as_millis() as u64,
                "Windows Agent lifecycle request returned"
            );
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

// Only mutation/ownership boundaries are logged. Periodic observations remain
// quiet; labels and numeric journal generations cannot copy plan/receipt data.
fn agent_lifecycle_method(payload: &agent_request::Payload) -> Option<&'static str> {
    use agent_request::Payload;
    Some(match payload {
        Payload::PrepareTunnel(_) => "prepare",
        Payload::FinalizeTunnel(_) => "finalize",
        Payload::BeginChainTransition(_) => "chain_transition",
        Payload::OpenPacketSession(_) => "open_packet_session",
        Payload::ClosePacketSession(_) => "close_packet_session",
        Payload::AcquireTunnelLease(_) => "promote_lease",
        Payload::CommitTunnel(_) => "commit",
        Payload::RollbackTunnel(_) => "rollback",
        Payload::ResumeTunnel(_) => "resume",
        Payload::RecoverOrphaned(_) => "recover_orphaned",
        Payload::RestartAutomaticRecovery(_) => "restart_recovery",
        _ => return None,
    })
}

pub(crate) async fn inspect_platform_state_if_running() -> Result<PlatformState, WindowsVpnError> {
    WindowsAgentClient::production()
        .inspect_platform_state_if_running()
        .await
}

pub(crate) async fn observe_automatic_recovery()
-> Result<AutomaticRecoveryObservation, WindowsVpnError> {
    let state = WindowsAgentClient::production().get_state().await?;
    automatic_recovery_observation(&state)
}

pub(crate) async fn automatic_recovery_preflight(
    restart_exhausted: bool,
) -> Result<Option<AutomaticRecoveryObservation>, WindowsVpnError> {
    WindowsAgentClient::production()
        .recovery_preflight(restart_exhausted)
        .await
}

fn automatic_recovery_observation(
    state: &AgentState,
) -> Result<AutomaticRecoveryObservation, WindowsVpnError> {
    if state.phase == agent_v1::AgentPhase::Clean as i32 {
        require_recovered_state(state)?;
        return Ok(AutomaticRecoveryObservation::Clean);
    }
    if !matches!(
        agent_v1::AgentPhase::try_from(state.phase),
        Ok(agent_v1::AgentPhase::RecoveryRequired | agent_v1::AgentPhase::Recovering)
    ) || state.operation_id.is_empty()
        || Uuid::parse_str(&state.operation_id).is_err()
        || state.journal_generation == 0
        || state.packet_session_active
    {
        return Err(WindowsVpnError::RecoveryConflict);
    }
    let status = state
        .automatic_recovery
        .as_ref()
        .ok_or(WindowsVpnError::RecoveryUnsupported)?;
    if status.attempt_limit != AUTOMATIC_RECOVERY_ATTEMPT_LIMIT
        || status.attempts_completed > status.attempt_limit
    {
        return Err(WindowsVpnError::RecoveryConflict);
    }
    match agent_v1::AutomaticRecoveryPhase::try_from(status.phase) {
        Ok(
            agent_v1::AutomaticRecoveryPhase::Waiting | agent_v1::AutomaticRecoveryPhase::Running,
        ) if status.terminal_error.is_none()
            && status.attempts_completed < status.attempt_limit =>
        {
            Ok(AutomaticRecoveryObservation::Pending {
                operation_id: state.operation_id.clone(),
                journal_generation: state.journal_generation,
            })
        }
        Ok(
            phase @ (agent_v1::AutomaticRecoveryPhase::Exhausted
            | agent_v1::AutomaticRecoveryPhase::Blocked),
        ) if state.phase == agent_v1::AgentPhase::RecoveryRequired as i32 => {
            let terminal = status
                .terminal_error
                .as_ref()
                .ok_or(WindowsVpnError::RecoveryConflict)?;
            let failure = AutomaticRecoveryFailure {
                operation_id: state.operation_id.clone(),
                journal_generation: state.journal_generation,
                code: terminal.code.clone(),
                message: terminal.message.clone(),
                retryable: terminal.retryable,
            };
            if failure.message.is_empty() || failure.message.chars().count() > 512 {
                return Err(WindowsVpnError::RecoveryConflict);
            }
            if phase == agent_v1::AutomaticRecoveryPhase::Exhausted {
                if status.attempts_completed != status.attempt_limit
                    || failure.code != "AGENT_AUTOMATIC_RECOVERY_EXHAUSTED"
                    || !failure.retryable
                {
                    Err(WindowsVpnError::RecoveryConflict)
                } else {
                    Ok(AutomaticRecoveryObservation::Exhausted(failure))
                }
            } else if status.attempts_completed == 0
                || failure.code != "AGENT_AUTOMATIC_RECOVERY_BLOCKED"
                || failure.retryable
            {
                Err(WindowsVpnError::RecoveryConflict)
            } else {
                Ok(AutomaticRecoveryObservation::Blocked(failure))
            }
        }
        _ => Err(WindowsVpnError::RecoveryConflict),
    }
}

fn automatic_recovery_connection_error(
    state: &AgentState,
) -> Result<WindowsVpnError, WindowsVpnError> {
    Ok(match automatic_recovery_observation(state)? {
        AutomaticRecoveryObservation::Pending {
            operation_id,
            journal_generation,
        } => WindowsVpnError::AutomaticRecoveryPending {
            operation_id,
            journal_generation,
        },
        AutomaticRecoveryObservation::Exhausted(failure) => {
            WindowsVpnError::AutomaticRecoveryExhausted {
                message: failure.message,
            }
        }
        AutomaticRecoveryObservation::Blocked(failure) => {
            WindowsVpnError::AutomaticRecoveryBlocked {
                message: failure.message,
            }
        }
        AutomaticRecoveryObservation::Clean => WindowsVpnError::RecoveryConflict,
    })
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

fn require_recovered_state(state: &AgentState) -> Result<(), WindowsVpnError> {
    if state.phase == agent_v1::AgentPhase::Clean as i32
        && !state.packet_session_active
        && !state.kill_switch_active
        && !state.system_proxy_active
        && state.operation_id.is_empty()
        && state.profile_id.is_empty()
    {
        Ok(())
    } else {
        Err(WindowsVpnError::RecoveryFailed)
    }
}

fn recovery_rpc_error(error: WindowsVpnError) -> WindowsVpnError {
    match &error {
        WindowsVpnError::RpcTimeout => WindowsVpnError::RecoveryTimeout,
        WindowsVpnError::Remote { code, .. }
            if matches!(
                code.as_str(),
                "AGENT_RECOVERY_CONFLICT"
                    | "AGENT_RECOVERY_BUSY"
                    | "AGENT_OWNER_MISMATCH"
                    | "AGENT_SHUTTING_DOWN"
            ) =>
        {
            WindowsVpnError::RecoveryConflict
        }
        WindowsVpnError::Remote { .. } => {
            tracing::warn!(%error, "guarded Windows platform recovery failed");
            WindowsVpnError::RecoveryFailed
        }
        _ => error,
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
    #[error("Windows VPN startup failed ({stage}: {primary_code}); rollback also failed ({recovery_code}); network recovery is required",
        primary_code = .startup.diagnostic_code(), recovery_code = .recovery.diagnostic_code())]
    StartupAndRecovery {
        stage: &'static str,
        #[source]
        startup: Box<WindowsVpnError>,
        recovery: Box<WindowsVpnError>,
    },
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
    #[error("Windows network recovery did not finish in time; no new VPN transaction was started")]
    RecoveryTimeout,
    #[error(
        "Windows network recovery is incomplete; retry the connection or inspect local diagnostics"
    )]
    RecoveryFailed,
    #[error(
        "Windows network state changed or belongs to an active session; no automatic recovery was performed"
    )]
    RecoveryConflict,
    #[error(
        "this Windows Agent cannot safely recover automatically; update the application and Agent together"
    )]
    RecoveryUnsupported,
    #[error("Windows Agent is repairing the previous network transaction")]
    AutomaticRecoveryPending {
        operation_id: String,
        journal_generation: u64,
    },
    #[error("Windows automatic recovery exhausted its retry budget: {message}")]
    AutomaticRecoveryExhausted { message: String },
    #[error("Windows automatic recovery stopped for safety: {message}")]
    AutomaticRecoveryBlocked { message: String },
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

impl WindowsVpnError {
    fn gate_failure(&self) -> usque_core::vpngate::GateFailure {
        use usque_core::vpngate::GateFailure;
        match self {
            Self::Transport(TransportError::VpnGate(reason)) => *reason,
            Self::Transport(error) if error.failure(None, None).retryable => GateFailure::Transport,
            Self::Io(_) | Self::RpcTimeout | Self::PhysicalDnsUnavailable => GateFailure::Transport,
            Self::Remote {
                retryable: true, ..
            } => GateFailure::Transport,
            _ => GateFailure::Configuration,
        }
    }

    fn diagnostic_code(&self) -> &'static str {
        match self {
            Self::PhysicalDnsUnavailable => "PHYSICAL_DNS_UNAVAILABLE",
            Self::InvalidPhysicalNetworkInfo => "PHYSICAL_NETWORK_SNAPSHOT_INVALID",
            Self::RpcTimeout | Self::RecoveryTimeout => "WINDOWS_RECOVERY_TIMEOUT",
            Self::Transport(error) => error.failure(None, None).code.as_str(),
            Self::Remote { code, .. } => match code.as_str() {
                "AGENT_RECOVERY_FAILED" => "AGENT_RECOVERY_FAILED",
                "AGENT_RECOVERY_BUSY" => "AGENT_RECOVERY_BUSY",
                "AGENT_RECOVERY_CONFLICT" => "AGENT_RECOVERY_CONFLICT",
                "AGENT_OWNER_MISMATCH" => "AGENT_OWNER_MISMATCH",
                "AGENT_DIRECT_EGRESS_FAILED" => "AGENT_DIRECT_EGRESS_FAILED",
                "AGENT_DIRECT_EGRESS_UNAVAILABLE" => "AGENT_DIRECT_EGRESS_UNAVAILABLE",
                "AGENT_DIRECT_EGRESS_NOT_READY" => "AGENT_DIRECT_EGRESS_NOT_READY",
                "AGENT_DIRECT_EGRESS_LIMIT" => "AGENT_DIRECT_EGRESS_LIMIT",
                "AGENT_INVALID_DIRECT_EGRESS" => "AGENT_INVALID_DIRECT_EGRESS",
                "AGENT_PHYSICAL_NETWORK_UNAVAILABLE" => "AGENT_PHYSICAL_NETWORK_UNAVAILABLE",
                "AGENT_WFP_PROVIDER_NOT_FOUND" => "AGENT_WFP_PROVIDER_NOT_FOUND",
                "AGENT_WFP_SUBLAYER_NOT_FOUND" => "AGENT_WFP_SUBLAYER_NOT_FOUND",
                "AGENT_PROTOCOL_MISMATCH" => "AGENT_PROTOCOL_MISMATCH",
                "AGENT_SHUTTING_DOWN" => "AGENT_SHUTTING_DOWN",
                _ => "AGENT_OPERATION_FAILED",
            },
            Self::Io(_) | Self::AgentService(_) => "AGENT_UNREACHABLE",
            Self::StartupAndRecovery { .. } => "WINDOWS_RECOVERY_FAILED",
            _ => "WINDOWS_VPN_STARTUP_FAILED",
        }
    }
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

    #[tokio::test]
    async fn cancelled_prepared_startup_quiesces_before_releasing_its_lease() {
        let cancel = CancellationToken::new();
        let producer_cancel = CancellationToken::new();
        let producer_guard = producer_cancel.clone().drop_guard();
        let (lease, mut agent) = tokio::io::duplex(64);
        let mut lease = Some(lease);
        let (entered, entering) = tokio::sync::oneshot::channel();
        let startup = async move {
            let _producer_guard = producer_guard;
            entered.send(()).unwrap();
            std::future::pending::<Result<(), TransportError>>().await
        };
        let observation = async {
            entering.await.unwrap();
            cancel.cancel();
            assert_eq!(agent.read(&mut [0; 1]).await.unwrap(), 0);
            assert!(producer_cancel.is_cancelled());
        };
        let (result, ()) = timeout(Duration::from_secs(1), async {
            tokio::join!(
                await_prepared_gate_startup(&cancel, &mut lease, startup),
                observation
            )
        })
        .await
        .expect("lease EOF must not wait for the blocked startup");
        assert!(matches!(result, Err(TransportError::TunnelClosed)));
        assert!(lease.is_none());
    }

    #[tokio::test]
    async fn ordinary_prepared_failure_retains_the_lease_for_retry() {
        let cancel = CancellationToken::new();
        let (pipe, mut agent) = tokio::io::duplex(64);
        let mut lease = Some(pipe);
        let result = await_prepared_gate_startup(&cancel, &mut lease, async {
            Err::<(), _>(TransportError::ConnectTimeout)
        })
        .await;
        assert!(matches!(result, Err(TransportError::ConnectTimeout)));
        lease.as_mut().unwrap().write_all(b"retry").await.unwrap();
        let mut data = [0; 5];
        agent.read_exact(&mut data).await.unwrap();
        assert_eq!(&data, b"retry");
    }

    #[tokio::test]
    async fn already_cancelled_startup_cannot_poll_new_connection_work() {
        let cancel = CancellationToken::new();
        cancel.cancel();
        let mut lease = Some(());
        let result: Result<(), TransportError> =
            await_prepared_gate_startup(&cancel, &mut lease, async {
                panic!("cancelled startup must not initiate a new connection");
            })
            .await;
        assert!(matches!(result, Err(TransportError::TunnelClosed)));
        assert!(lease.is_none());
    }

    #[test]
    fn adapter_recovery_details_keep_only_typed_observations_and_numeric_errors() {
        let detail = sanitized_adapter_recovery_detail(
            "automatic recovery exhausted after 3 attempts: restore WintunAdapter: adapter_cleanup stage=Confirm failure=Pending interface=Some(false) device=Some(true) request_accepted=true elapsed_ms=10000 api=None win32=None private-token 192.0.2.1").unwrap();
        assert!(detail.contains("interface=Some(false)") && detail.contains("device=Some(true)"));
        assert!(detail.contains("elapsed_ms=10000"));
        assert!(!detail.contains("private-token") && !detail.contains("192.0.2.1"));
        let hostile = sanitized_adapter_recovery_detail(
            "restore WintunAdapter: api=Some(private-token) win32=Some(192.0.2.1) elapsed_ms=private-path stage=secret failure=secret").unwrap();
        assert_eq!(hostile, "WintunAdapter ");
        assert!(sanitized_adapter_recovery_detail("private text without a step").is_none());
    }

    #[test]
    fn geo_startup_accepts_effective_dhcp_dns_but_rejects_an_empty_physical_snapshot() {
        use usque_core::DirectDnsMode;
        let servers = ["192.0.2.53:53".parse().unwrap()];
        assert!(validate_physical_dns(true, DirectDnsMode::PhysicalSystem, &servers).is_ok());
        assert!(matches!(
            validate_physical_dns(true, DirectDnsMode::PhysicalSystem, &[]),
            Err(WindowsVpnError::PhysicalDnsUnavailable)
        ));
        assert!(validate_physical_dns(false, DirectDnsMode::PhysicalSystem, &[]).is_ok());
        assert!(validate_physical_dns(true, DirectDnsMode::Doh, &[]).is_ok());
    }

    #[tokio::test]
    async fn startup_failure_keeps_the_original_cause_when_rollback_also_fails() {
        let (client, task) = scripted_recovery_client(vec![AgentResponse {
            error: Some(agent_v1::AgentError {
                code: "AGENT_RECOVERY_FAILED".to_owned(),
                message: "private diagnostic fixture".to_owned(),
                retryable: false,
            }),
            ..Default::default()
        }]);
        let error = fail_startup(
            &client,
            Uuid::new_v4(),
            false,
            "PHYSICAL_DNS_UNAVAILABLE",
            WindowsVpnError::PhysicalDnsUnavailable,
        )
        .await;
        let WindowsVpnError::StartupAndRecovery {
            startup, recovery, ..
        } = &error
        else {
            panic!("both causes required")
        };
        assert!(matches!(
            startup.as_ref(),
            WindowsVpnError::PhysicalDnsUnavailable
        ));
        assert!(
            matches!(recovery.as_ref(), WindowsVpnError::Remote { code, .. } if code == "AGENT_RECOVERY_FAILED")
        );
        assert!(error.to_string().contains("PHYSICAL_DNS_UNAVAILABLE"));
        assert!(error.to_string().contains("AGENT_RECOVERY_FAILED"));
        assert!(!error.to_string().contains("private diagnostic fixture"));
        assert!(
            matches!(task.await.unwrap().as_slice(), [agent_request::Payload::RollbackTunnel(request)] if request.reason_code == "PHYSICAL_DNS_UNAVAILABLE")
        );
        assert!(matches!(
            crate::map_windows_vpn_error(error),
            crate::ControlServiceError::PlatformRecovery {
                code: "WINDOWS_RECOVERY_FAILED",
                retryable: false,
                ..
            }
        ));
    }

    #[tokio::test]
    async fn successful_startup_rollback_preserves_the_original_error() {
        let (client, task) =
            scripted_recovery_client(vec![recovery_state_response(agent_v1::AgentPhase::Clean)]);
        let error = fail_startup(
            &client,
            Uuid::new_v4(),
            false,
            "PHYSICAL_DNS_UNAVAILABLE",
            WindowsVpnError::PhysicalDnsUnavailable,
        )
        .await;
        assert!(matches!(error, WindowsVpnError::PhysicalDnsUnavailable));
        assert_eq!(task.await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn a_reattachment_failure_never_rolls_back_the_active_tunnel() {
        let (client, task) = scripted_recovery_client(vec![]);
        let error = fail_startup(
            &client,
            Uuid::new_v4(),
            true,
            "TRANSPORT_START_FAILED",
            WindowsVpnError::Transport(TransportError::ConnectTimeout),
        )
        .await;
        assert!(matches!(
            error,
            WindowsVpnError::Transport(TransportError::ConnectTimeout)
        ));
        assert!(task.await.unwrap().is_empty());
    }

    #[test]
    fn compound_startup_errors_hide_sensitive_details_and_never_allow_transport_fallback() {
        let error = WindowsVpnError::StartupAndRecovery {
            stage: "TRANSPORT_START_FAILED",
            startup: Box::new(WindowsVpnError::Transport(TransportError::Dns(
                "private.example 203.0.113.7 token-fixture".to_owned(),
            ))),
            recovery: Box::new(WindowsVpnError::RpcTimeout),
        };
        let text = error.to_string();
        assert!(text.contains("PHYSICAL_DNS_UNAVAILABLE"));
        assert!(text.contains("WINDOWS_RECOVERY_TIMEOUT"));
        for private in ["private.example", "203.0.113.7", "token-fixture"] {
            assert!(!text.contains(private));
        }
        assert!(matches!(
            crate::map_windows_vpn_error(error),
            crate::ControlServiceError::PlatformRecovery {
                retryable: false,
                ..
            }
        ));
    }

    fn scripted_recovery_client(
        script: Vec<AgentResponse>,
    ) -> (WindowsAgentClient, JoinHandle<Vec<agent_request::Payload>>) {
        scripted_recovery_client_paused(script, None)
    }

    fn scripted_recovery_client_paused(
        script: Vec<AgentResponse>,
        pause_restart: Option<Arc<(tokio::sync::Notify, tokio::sync::Notify)>>,
    ) -> (WindowsAgentClient, JoinHandle<Vec<agent_request::Payload>>) {
        let pipe_name = format!("{AGENT_PIPE_NAME}.test-{}", Uuid::new_v4());
        let mut next = ServerOptions::new()
            .first_pipe_instance(true)
            .create(&pipe_name)
            .unwrap();
        let client = WindowsAgentClient::for_test(pipe_name.clone());
        let task = tokio::spawn(async move {
            let mut requests = Vec::new();
            for mut response in script {
                next.connect().await.unwrap();
                let mut pipe = next;
                next = ServerOptions::new().create(&pipe_name).unwrap();
                let mut header = [0; 4];
                pipe.read_exact(&mut header).await.unwrap();
                let mut payload = vec![0; u32::from_be_bytes(header) as usize];
                pipe.read_exact(&mut payload).await.unwrap();
                let mut frame = BytesMut::from(header.as_slice());
                frame.extend_from_slice(&payload);
                let request: AgentRequest = decode_frame(frame.freeze()).unwrap();
                assert_eq!(request.protocol_version, AGENT_PROTOCOL_VERSION);
                response.request_id = request.request_id;
                let payload = request.payload.unwrap();
                if matches!(payload, agent_request::Payload::RestartAutomaticRecovery(_))
                    && let Some(pause) = &pause_restart
                {
                    pause.0.notify_one();
                    pause.1.notified().await;
                }
                requests.push(payload);
                if let Err(error) = pipe.write_all(&encode_frame(&response).unwrap()).await {
                    assert!(
                        pause_restart.is_some(),
                        "unexpected IPC write failure: {error}"
                    );
                    break;
                }
            }
            requests
        });
        (client, task)
    }

    fn recovery_state_response(phase: agent_v1::AgentPhase) -> AgentResponse {
        AgentResponse {
            payload: Some(agent_response::Payload::State(AgentState {
                phase: phase as i32,
                operation_id: if phase == agent_v1::AgentPhase::Clean {
                    String::new()
                } else {
                    "00000000-0000-4000-8000-000000000001".to_owned()
                },
                journal_generation: 19,
                ..Default::default()
            })),
            ..Default::default()
        }
    }

    fn automatic_recovery_state_response(phase: agent_v1::AutomaticRecoveryPhase) -> AgentResponse {
        let terminal_error = match phase {
            agent_v1::AutomaticRecoveryPhase::Exhausted => Some(agent_v1::AgentError {
                code: "AGENT_AUTOMATIC_RECOVERY_EXHAUSTED".to_owned(),
                message: "sanitized exhausted".to_owned(),
                retryable: true,
            }),
            agent_v1::AutomaticRecoveryPhase::Blocked => Some(agent_v1::AgentError {
                code: "AGENT_AUTOMATIC_RECOVERY_BLOCKED".to_owned(),
                message: "sanitized blocked".to_owned(),
                retryable: false,
            }),
            _ => None,
        };
        AgentResponse {
            payload: Some(agent_response::Payload::State(AgentState {
                phase: agent_v1::AgentPhase::RecoveryRequired as i32,
                operation_id: "00000000-0000-4000-8000-000000000001".to_owned(),
                journal_generation: 19,
                automatic_recovery: Some(agent_v1::AutomaticRecoveryStatus {
                    phase: phase as i32,
                    attempts_completed: if phase == agent_v1::AutomaticRecoveryPhase::Exhausted {
                        AUTOMATIC_RECOVERY_ATTEMPT_LIMIT
                    } else {
                        1
                    },
                    attempt_limit: 3,
                    terminal_error,
                }),
                ..Default::default()
            })),
            ..Default::default()
        }
    }

    fn pending_adapter_removal_response() -> AgentResponse {
        AgentResponse {
            error: Some(agent_v1::AgentError {
                code: "AGENT_RECOVERY_FAILED".to_owned(),
                message: "adapter_cleanup stage=Confirm failure=Pending interface=Some(true) device=Some(false)".to_owned(),
                retryable: true,
            }),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn disconnect_rollback_observes_pending_recovery_and_completed_cleanup() {
        let operation = Uuid::parse_str("00000000-0000-4000-8000-000000000001").unwrap();
        for clean in [false, true] {
            let observed = if clean {
                recovery_state_response(agent_v1::AgentPhase::Clean)
            } else {
                automatic_recovery_state_response(agent_v1::AutomaticRecoveryPhase::Waiting)
            };
            let (client, task) =
                scripted_recovery_client(vec![pending_adapter_removal_response(), observed]);
            let result = client.rollback_for_disconnect(operation).await;
            if clean {
                require_recovered_state(&result.unwrap()).unwrap();
            } else {
                assert!(
                    matches!(result, Err(WindowsVpnError::AutomaticRecoveryPending {
                    operation_id, journal_generation: 19,
                }) if operation_id == operation.to_string())
                );
            }
            assert!(matches!(task.await.unwrap().as_slice(), [
                agent_request::Payload::RollbackTunnel(request), agent_request::Payload::GetState(_),
            ] if request.operation_id == operation.to_string()));
        }
    }

    #[tokio::test]
    async fn disconnect_rollback_never_admits_conflicting_or_incomplete_agent_state() {
        let operation = Uuid::parse_str("00000000-0000-4000-8000-000000000001").unwrap();
        for case in 0..5 {
            let mut response = match case {
                0 => automatic_recovery_state_response(agent_v1::AutomaticRecoveryPhase::Waiting),
                1 => recovery_state_response(agent_v1::AgentPhase::Clean),
                2 => recovery_state_response(agent_v1::AgentPhase::Active),
                3 => automatic_recovery_state_response(agent_v1::AutomaticRecoveryPhase::Exhausted),
                _ => automatic_recovery_state_response(agent_v1::AutomaticRecoveryPhase::Blocked),
            };
            let Some(agent_response::Payload::State(state)) = response.payload.as_mut() else {
                unreachable!()
            };
            if case == 0 {
                state.operation_id = Uuid::new_v4().to_string();
            }
            if case == 1 {
                state.packet_session_active = true;
            }
            let (client, task) =
                scripted_recovery_client(vec![pending_adapter_removal_response(), response]);
            let result = client.rollback_for_disconnect(operation).await;
            assert!(result.is_err());
            assert!(!matches!(
                result,
                Err(WindowsVpnError::AutomaticRecoveryPending { .. })
            ));
            assert_eq!(task.await.unwrap().len(), 2);
        }
    }

    #[tokio::test]
    async fn disconnect_rollback_preserves_nonretryable_failures_without_polling() {
        let mut response = pending_adapter_removal_response();
        response.error.as_mut().unwrap().retryable = false;
        let (client, task) = scripted_recovery_client(vec![response]);
        assert!(matches!(
            client.rollback_for_disconnect(Uuid::new_v4()).await,
            Err(WindowsVpnError::Remote {
                retryable: false,
                ..
            })
        ));
        assert_eq!(task.await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn guarded_connection_recovery_is_one_compare_and_recover_then_a_fresh_clean_read() {
        let (client, task) = scripted_recovery_client(vec![
            recovery_state_response(agent_v1::AgentPhase::RecoveryRequired),
            recovery_state_response(agent_v1::AgentPhase::Clean),
            recovery_state_response(agent_v1::AgentPhase::Clean),
        ]);
        let capabilities = AgentCapabilities {
            guarded_recovery: true,
            ..Default::default()
        };
        let state = client.connection_state(&capabilities).await.unwrap();
        require_recovered_state(&state).unwrap();
        let requests = task.await.unwrap();
        assert_eq!(requests.len(), 3);
        assert!(matches!(&requests[0], agent_request::Payload::GetState(_)));
        assert!(
            matches!(&requests[1], agent_request::Payload::RecoverOrphaned(value)
            if value.operation_id == "00000000-0000-4000-8000-000000000001" && value.expected_journal_generation == 19)
        );
        assert!(matches!(&requests[2], agent_request::Payload::GetState(_)));
    }

    #[tokio::test]
    async fn automatic_recovery_pending_never_sends_a_second_recovery_mutation() {
        let (client, task) = scripted_recovery_client(vec![automatic_recovery_state_response(
            agent_v1::AutomaticRecoveryPhase::Waiting,
        )]);
        assert!(matches!(
            client
                .connection_state(&AgentCapabilities {
                    automatic_recovery: true,
                    guarded_recovery: true,
                    ..Default::default()
                })
                .await,
            Err(WindowsVpnError::AutomaticRecoveryPending {
                ref operation_id,
                journal_generation: 19,
            }) if operation_id == "00000000-0000-4000-8000-000000000001"
        ));
        assert!(matches!(
            task.await.unwrap().as_slice(),
            [agent_request::Payload::GetState(_)]
        ));
    }

    #[tokio::test]
    async fn connect_retry_and_fresh_engine_all_continue_after_exact_recovery_without_duplicate_transaction()
     {
        for retry in [false, true] {
            let (directory, service, profile_id) = recovery_entry_service().await;
            // Reopen the Engine with the same configuration/vault: recovery
            // must not depend on a previous in-memory error snapshot.
            let service = crate::ControlService::open_with_vault(
                crate::ConfigStore::new(directory.path().join("config.json")),
                Arc::clone(&service.vault),
            )
            .unwrap();
            let (client, task) = scripted_recovery_client(vec![
                recovery_capabilities_response(),
                automatic_recovery_state_response(agent_v1::AutomaticRecoveryPhase::Exhausted),
                recovery_state_response(agent_v1::AgentPhase::Clean),
            ]);
            *service.test_windows_agent.lock().await = Some(client);
            assert!(service.state.lock().await.snapshot().error.is_none());
            let connected = if retry {
                service.retry().await
            } else {
                service.connect(profile_id).await
            }
            .unwrap();
            assert_eq!(connected.phase, usque_core::ConnectionPhase::Connected);
            let generation = service
                .data_plane
                .lock()
                .await
                .as_ref()
                .unwrap()
                .session_generation;
            assert_eq!(
                service.connect(profile_id).await.unwrap().phase,
                usque_core::ConnectionPhase::Connected
            );
            assert_eq!(
                service
                    .data_plane
                    .lock()
                    .await
                    .as_ref()
                    .unwrap()
                    .session_generation,
                generation
            );
            let requests = task.await.unwrap();
            assert_eq!(
                requests
                    .iter()
                    .filter(|request| matches!(
                        request,
                        agent_request::Payload::RestartAutomaticRecovery(_)
                    ))
                    .count(),
                1
            );
            service.shutdown().await.unwrap();
        }
    }

    fn recovery_capabilities_response() -> AgentResponse {
        AgentResponse {
            payload: Some(agent_response::Payload::Capabilities(AgentCapabilities {
                protocol_version: AGENT_PROTOCOL_VERSION,
                automatic_recovery: true,
                ..Default::default()
            })),
            ..Default::default()
        }
    }

    async fn recovery_entry_service() -> (tempfile::TempDir, crate::ControlService, Uuid) {
        let directory = tempfile::tempdir().unwrap();
        let service = crate::ControlService::open_with_vault(
            crate::ConfigStore::new(directory.path().join("config.json")),
            Arc::new(crate::tests::MemoryVault::default()),
        )
        .unwrap();
        let mut profile = service.config_snapshot().await.active_profile().unwrap();
        profile.frontends.tunnel = true;
        profile.mode = usque_core::OperatingMode::Vpn;
        profile.proxy.system_proxy = false;
        let profile_id = profile.id;
        service.upsert_profile_locked(profile).await.unwrap();
        service
            .persist_identity(
                profile_id,
                &crate::tests::test_identity(usque_core::IdentityProvider::Consumer, None),
                None,
            )
            .await
            .unwrap();
        (directory, service, profile_id)
    }

    #[tokio::test]
    async fn connecting_during_recovery_tracks_one_watch_and_internal_reconnect_cannot_reset_exhaustion()
     {
        let (_directory, service, profile_id) = recovery_entry_service().await;
        let (client, task) = scripted_recovery_client(vec![
            recovery_capabilities_response(),
            automatic_recovery_state_response(agent_v1::AutomaticRecoveryPhase::Waiting),
            recovery_capabilities_response(),
            automatic_recovery_state_response(agent_v1::AutomaticRecoveryPhase::Running),
            recovery_capabilities_response(),
            automatic_recovery_state_response(agent_v1::AutomaticRecoveryPhase::Exhausted),
        ]);
        *service.test_windows_agent.lock().await = Some(client);
        assert_eq!(
            service.connect(profile_id).await.unwrap().phase,
            usque_core::ConnectionPhase::Reconnecting
        );
        assert_eq!(
            service.connect(profile_id).await.unwrap().phase,
            usque_core::ConnectionPhase::Reconnecting
        );
        assert!(service.data_plane.lock().await.is_none());
        let error = service.connect_locked(profile_id).await.unwrap_err();
        assert_eq!(
            error.as_structured_error().code,
            "WINDOWS_RECOVERY_EXHAUSTED"
        );
        assert!(task.await.unwrap().iter().all(|request| !matches!(
            request,
            agent_request::Payload::RestartAutomaticRecovery(_)
        )));
        service.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn cancelled_connect_can_finish_cleanup_but_cannot_connect_or_install_a_watch() {
        for phase in [
            agent_v1::AgentPhase::Clean,
            agent_v1::AgentPhase::RecoveryRequired,
        ] {
            let (_directory, service, profile_id) = recovery_entry_service().await;
            let pause = Arc::new((tokio::sync::Notify::new(), tokio::sync::Notify::new()));
            let final_response = if phase == agent_v1::AgentPhase::Clean {
                recovery_state_response(phase)
            } else {
                automatic_recovery_state_response(agent_v1::AutomaticRecoveryPhase::Waiting)
            };
            let (client, task) = scripted_recovery_client_paused(
                vec![
                    recovery_capabilities_response(),
                    automatic_recovery_state_response(agent_v1::AutomaticRecoveryPhase::Exhausted),
                    final_response,
                ],
                Some(Arc::clone(&pause)),
            );
            *service.test_windows_agent.lock().await = Some(client);
            let connect = {
                let service = service.clone();
                tokio::spawn(async move { service.connect(profile_id).await })
            };
            tokio::time::timeout(Duration::from_secs(2), pause.0.notified())
                .await
                .unwrap();
            service.disconnect().await.unwrap();
            pause.1.notify_one();
            connect.await.unwrap().unwrap();
            assert!(service.data_plane.lock().await.is_none());
            assert!(service.windows_recovery.lock().await.pending.is_none());
            assert_eq!(
                service.state.lock().await.snapshot().phase,
                usque_core::ConnectionPhase::Disconnected
            );
            assert_eq!(task.await.unwrap().len(), 3);
            service.shutdown().await.unwrap();
        }
    }

    #[tokio::test]
    async fn stale_cleanup_result_is_rechecked_and_recovery_failure_is_logged_once_per_request() {
        use tracing::instrument::WithSubscriber;
        let (directory, service, profile_id) = recovery_entry_service().await;
        *service.disconnect_cleanup.lock().await = Some(tokio::spawn(async {
            Err(crate::ControlServiceError::PlatformRecovery {
                code: "WINDOWS_RECOVERY_EXHAUSTED",
                message: "previous shutdown result".to_owned(),
                retryable: true,
            })
        }));
        let (client, task) = scripted_recovery_client(vec![
            recovery_capabilities_response(),
            automatic_recovery_state_response(agent_v1::AutomaticRecoveryPhase::Exhausted),
            automatic_recovery_state_response(agent_v1::AutomaticRecoveryPhase::Exhausted),
        ]);
        *service.test_windows_agent.lock().await = Some(client);
        let config = directory.path().join("config.json");
        let subscriber = tracing_subscriber::fmt()
            .json()
            .with_writer(crate::logging::LogWriterFactory::open(&config).unwrap())
            .finish();
        let response = service
            .handle(usque_ipc::v1::ControlRequest {
                request_id: "recovery-entry".to_owned(),
                payload: Some(usque_ipc::v1::control_request::Payload::Connect(
                    usque_ipc::v1::ConnectRequest {
                        profile_id: profile_id.to_string(),
                    },
                )),
            })
            .with_subscriber(subscriber)
            .await;
        assert_eq!(response.error.unwrap().code, "WINDOWS_RECOVERY_EXHAUSTED");
        assert_eq!(task.await.unwrap().len(), 3);
        let log =
            std::fs::read_to_string(crate::logging::log_directory(&config).join("engine.jsonl"))
                .unwrap();
        assert_eq!(
            log.matches("Windows network recovery did not complete")
                .count(),
            1
        );
        assert!(
            log.contains("historical_terminal")
                && log.contains("Explicit connection requested a new Windows recovery attempt")
        );
        service.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn preflight_rejects_a_changed_operation_or_regressed_generation() {
        for wrong_operation in [false, true] {
            let mut response =
                automatic_recovery_state_response(agent_v1::AutomaticRecoveryPhase::Waiting);
            let Some(agent_response::Payload::State(state)) = response.payload.as_mut() else {
                unreachable!()
            };
            if wrong_operation {
                state.operation_id = Uuid::new_v4().to_string();
            } else {
                state.journal_generation = 18;
            }
            let (client, task) = scripted_recovery_client(vec![
                automatic_recovery_state_response(agent_v1::AutomaticRecoveryPhase::Exhausted),
                response,
            ]);
            assert!(matches!(
                client
                    .automatic_recovery_preflight(
                        &AgentCapabilities {
                            automatic_recovery: true,
                            ..Default::default()
                        },
                        true
                    )
                    .await,
                Err(WindowsVpnError::RecoveryConflict)
            ));
            assert_eq!(task.await.unwrap().len(), 2);
        }
    }

    #[tokio::test]
    async fn explicit_preflight_reconciles_exhausted_recovery_without_an_engine_error_snapshot() {
        let (client, task) = scripted_recovery_client(vec![
            automatic_recovery_state_response(agent_v1::AutomaticRecoveryPhase::Exhausted),
            recovery_state_response(agent_v1::AgentPhase::Clean),
        ]);
        let result = client
            .automatic_recovery_preflight(
                &AgentCapabilities {
                    automatic_recovery: true,
                    ..Default::default()
                },
                true,
            )
            .await
            .unwrap();
        assert!(matches!(result, Some(AutomaticRecoveryObservation::Clean)));
        assert!(matches!(task.await.unwrap().as_slice(), [
            agent_request::Payload::GetState(_),
            agent_request::Payload::RestartAutomaticRecovery(request)
        ] if request.operation_id == "00000000-0000-4000-8000-000000000001"
            && request.expected_journal_generation == 19));
    }

    #[tokio::test]
    async fn preflight_never_restarts_waiting_running_blocked_or_background_recovery() {
        for (phase, explicit) in [
            (agent_v1::AutomaticRecoveryPhase::Waiting, true),
            (agent_v1::AutomaticRecoveryPhase::Running, true),
            (agent_v1::AutomaticRecoveryPhase::Blocked, true),
            (agent_v1::AutomaticRecoveryPhase::Exhausted, false),
        ] {
            let (client, task) =
                scripted_recovery_client(vec![automatic_recovery_state_response(phase)]);
            let result = client
                .automatic_recovery_preflight(
                    &AgentCapabilities {
                        automatic_recovery: true,
                        ..Default::default()
                    },
                    explicit,
                )
                .await
                .unwrap();
            assert!(result.is_some());
            assert!(matches!(
                task.await.unwrap().as_slice(),
                [agent_request::Payload::GetState(_)]
            ));
        }
    }

    #[tokio::test]
    async fn preflight_cannot_reset_the_budget_twice_in_one_request() {
        let (client, task) = scripted_recovery_client(vec![
            automatic_recovery_state_response(agent_v1::AutomaticRecoveryPhase::Exhausted),
            automatic_recovery_state_response(agent_v1::AutomaticRecoveryPhase::Exhausted),
        ]);
        assert!(matches!(
            client
                .automatic_recovery_preflight(
                    &AgentCapabilities {
                        automatic_recovery: true,
                        ..Default::default()
                    },
                    true
                )
                .await
                .unwrap(),
            Some(AutomaticRecoveryObservation::Exhausted(_))
        ));
        assert_eq!(task.await.unwrap().len(), 2);
    }

    #[tokio::test]
    async fn manual_restart_uses_the_exact_append_only_request() {
        let (client, task) = scripted_recovery_client(vec![automatic_recovery_state_response(
            agent_v1::AutomaticRecoveryPhase::Waiting,
        )]);
        assert!(matches!(
            client
                .restart_automatic_recovery("00000000-0000-4000-8000-000000000001".to_owned(), 19,)
                .await
                .unwrap(),
            AutomaticRecoveryObservation::Pending { .. }
        ));
        assert!(matches!(
            task.await.unwrap().as_slice(),
            [agent_request::Payload::RestartAutomaticRecovery(request)]
                if request.operation_id == "00000000-0000-4000-8000-000000000001"
                    && request.expected_journal_generation == 19
        ));
    }

    #[test]
    fn terminal_automatic_recovery_status_is_strictly_typed() {
        for (phase, retryable) in [
            (agent_v1::AutomaticRecoveryPhase::Exhausted, true),
            (agent_v1::AutomaticRecoveryPhase::Blocked, false),
        ] {
            let response = automatic_recovery_state_response(phase);
            let Some(agent_response::Payload::State(state)) = response.payload else {
                panic!("state response");
            };
            let observation = automatic_recovery_observation(&state).unwrap();
            let failure = match observation {
                AutomaticRecoveryObservation::Exhausted(failure)
                | AutomaticRecoveryObservation::Blocked(failure) => failure,
                _ => panic!("terminal observation"),
            };
            assert_eq!(failure.retryable, retryable);
        }
    }

    #[test]
    fn automatic_recovery_rejects_incompatible_status_combinations() {
        let state_for = |phase| {
            let response = automatic_recovery_state_response(phase);
            let Some(agent_response::Payload::State(state)) = response.payload else {
                panic!("state response");
            };
            state
        };

        let mut waiting = state_for(agent_v1::AutomaticRecoveryPhase::Waiting);
        waiting.automatic_recovery.as_mut().unwrap().terminal_error = Some(agent_v1::AgentError {
            code: "must-not-surface-yet".to_owned(),
            message: "intermediate".to_owned(),
            retryable: true,
        });
        assert!(matches!(
            automatic_recovery_observation(&waiting),
            Err(WindowsVpnError::RecoveryConflict)
        ));

        let mut exhausted = state_for(agent_v1::AutomaticRecoveryPhase::Exhausted);
        exhausted
            .automatic_recovery
            .as_mut()
            .unwrap()
            .attempts_completed = AUTOMATIC_RECOVERY_ATTEMPT_LIMIT - 1;
        assert!(matches!(
            automatic_recovery_observation(&exhausted),
            Err(WindowsVpnError::RecoveryConflict)
        ));

        let mut blocked = state_for(agent_v1::AutomaticRecoveryPhase::Blocked);
        blocked.automatic_recovery.as_mut().unwrap().attempt_limit += 1;
        assert!(matches!(
            automatic_recovery_observation(&blocked),
            Err(WindowsVpnError::RecoveryConflict)
        ));
    }

    #[test]
    fn running_automatic_recovery_is_observable_while_the_journal_is_locked() {
        let mut response =
            automatic_recovery_state_response(agent_v1::AutomaticRecoveryPhase::Running);
        let Some(agent_response::Payload::State(state)) = response.payload.as_mut() else {
            panic!("state response");
        };
        state.phase = agent_v1::AgentPhase::Recovering as i32;
        assert!(matches!(
            automatic_recovery_observation(state).unwrap(),
            AutomaticRecoveryObservation::Pending {
                journal_generation: 19,
                ..
            }
        ));
    }

    #[tokio::test]
    async fn legacy_agent_is_never_sent_unguarded_maintenance_recovery() {
        let (client, task) = scripted_recovery_client(vec![recovery_state_response(
            agent_v1::AgentPhase::RecoveryRequired,
        )]);
        assert!(matches!(
            client.connection_state(&AgentCapabilities::default()).await,
            Err(WindowsVpnError::RecoveryUnsupported)
        ));
        assert!(matches!(
            task.await.unwrap().as_slice(),
            [agent_request::Payload::GetState(_)]
        ));
    }

    #[tokio::test]
    async fn healthy_and_active_transactions_are_not_automatically_recovered() {
        for phase in [agent_v1::AgentPhase::Clean, agent_v1::AgentPhase::Active] {
            let (client, task) = scripted_recovery_client(vec![recovery_state_response(phase)]);
            assert_eq!(
                client
                    .connection_state(&AgentCapabilities::default())
                    .await
                    .unwrap()
                    .phase,
                phase as i32
            );
            assert!(matches!(
                task.await.unwrap().as_slice(),
                [agent_request::Payload::GetState(_)]
            ));
        }
    }

    #[tokio::test]
    async fn initial_clean_state_with_live_resources_is_rejected_without_mutation() {
        let mut response = recovery_state_response(agent_v1::AgentPhase::Clean);
        if let Some(agent_response::Payload::State(state)) = response.payload.as_mut() {
            state.packet_session_active = true;
        }
        let (client, task) = scripted_recovery_client(vec![response]);
        assert!(matches!(
            client.connection_state(&AgentCapabilities::default()).await,
            Err(WindowsVpnError::RecoveryFailed)
        ));
        assert!(matches!(
            task.await.unwrap().as_slice(),
            [agent_request::Payload::GetState(_)]
        ));
    }

    #[tokio::test]
    async fn guarded_recovery_failure_is_not_retried_or_downgraded() {
        for code in [
            "AGENT_RECOVERY_FAILED",
            "AGENT_RECOVERY_CONFLICT",
            "AGENT_RECOVERY_BUSY",
            "AGENT_OWNER_MISMATCH",
        ] {
            let (client, task) = scripted_recovery_client(vec![
                recovery_state_response(agent_v1::AgentPhase::RecoveryRequired),
                AgentResponse {
                    error: Some(agent_v1::AgentError {
                        code: code.to_owned(),
                        message: "test failure".to_owned(),
                        retryable: false,
                    }),
                    ..Default::default()
                },
            ]);
            let result = client
                .connection_state(&AgentCapabilities {
                    guarded_recovery: true,
                    ..Default::default()
                })
                .await;
            if code == "AGENT_RECOVERY_FAILED" {
                assert!(matches!(result, Err(WindowsVpnError::RecoveryFailed)));
            } else {
                assert!(matches!(result, Err(WindowsVpnError::RecoveryConflict)));
            }
            let requests = task.await.unwrap();
            assert_eq!(requests.len(), 2);
            assert!(matches!(
                &requests[1],
                agent_request::Payload::RecoverOrphaned(_)
            ));
        }
    }

    #[tokio::test]
    async fn a_new_transaction_after_recovery_is_not_rolled_back() {
        let (client, task) = scripted_recovery_client(vec![
            recovery_state_response(agent_v1::AgentPhase::RecoveryRequired),
            recovery_state_response(agent_v1::AgentPhase::Clean),
            recovery_state_response(agent_v1::AgentPhase::Active),
        ]);
        assert!(matches!(
            client
                .connection_state(&AgentCapabilities {
                    guarded_recovery: true,
                    ..Default::default()
                })
                .await,
            Err(WindowsVpnError::RecoveryConflict)
        ));
        assert_eq!(task.await.unwrap().len(), 3);
    }

    #[tokio::test]
    async fn in_progress_recovery_is_waited_for_without_sending_another_mutation() {
        let (client, task) = scripted_recovery_client(vec![
            recovery_state_response(agent_v1::AgentPhase::Recovering),
            recovery_state_response(agent_v1::AgentPhase::Clean),
        ]);
        assert_eq!(
            client
                .connection_state(&AgentCapabilities::default())
                .await
                .unwrap()
                .phase,
            agent_v1::AgentPhase::Clean as i32
        );
        assert!(
            task.await
                .unwrap()
                .iter()
                .all(|request| matches!(request, agent_request::Payload::GetState(_)))
        );
    }

    #[tokio::test]
    async fn recovery_deadline_covers_state_queries_not_only_the_mutation_rpc() {
        let (client, task) = scripted_recovery_client(vec![recovery_state_response(
            agent_v1::AgentPhase::Recovering,
        )]);
        assert!(matches!(
            client
                .connection_state_with_timeout(&AgentCapabilities::default(), Duration::ZERO)
                .await,
            Err(WindowsVpnError::RecoveryTimeout)
        ));
        task.abort();
        let _ = task.await;
    }

    #[test]
    fn a_clean_label_cannot_hide_live_platform_resources() {
        let clean = AgentState {
            phase: agent_v1::AgentPhase::Clean as i32,
            ..Default::default()
        };
        require_recovered_state(&clean).unwrap();
        for state in [
            AgentState {
                packet_session_active: true,
                ..clean.clone()
            },
            AgentState {
                kill_switch_active: true,
                ..clean.clone()
            },
            AgentState {
                system_proxy_active: true,
                ..clean.clone()
            },
            AgentState {
                operation_id: "old".to_owned(),
                ..clean
            },
        ] {
            assert!(require_recovered_state(&state).is_err());
        }
    }

    #[test]
    fn recovery_error_codes_survive_control_replies_and_snapshot_events() {
        assert!(matches!(
            crate::map_windows_vpn_error(WindowsVpnError::AutomaticRecoveryPending {
                operation_id: "operation".to_owned(),
                journal_generation: 7,
            }),
            crate::ControlServiceError::PlatformRecoveryPending {
                operation_id,
                journal_generation: 7,
            } if operation_id == "operation"
        ));
        for (error, code, retryable) in [
            (
                WindowsVpnError::RecoveryFailed,
                "WINDOWS_RECOVERY_FAILED",
                false,
            ),
            (
                WindowsVpnError::RecoveryTimeout,
                "WINDOWS_RECOVERY_TIMEOUT",
                true,
            ),
            (
                WindowsVpnError::RecoveryConflict,
                "WINDOWS_RECOVERY_CONFLICT",
                false,
            ),
            (
                WindowsVpnError::RecoveryUnsupported,
                "WINDOWS_RECOVERY_UNSUPPORTED",
                false,
            ),
            (
                WindowsVpnError::AutomaticRecoveryExhausted {
                    message: "sanitized".to_owned(),
                },
                "WINDOWS_RECOVERY_EXHAUSTED",
                true,
            ),
            (
                WindowsVpnError::AutomaticRecoveryBlocked {
                    message: "sanitized".to_owned(),
                },
                "WINDOWS_RECOVERY_BLOCKED",
                false,
            ),
        ] {
            let error = crate::map_windows_vpn_error(error);
            assert_eq!(error.as_structured_error().code, code);
            assert_eq!(error.as_structured_error().retryable, retryable);
            assert_eq!(
                crate::connection_error_wire_code(crate::connection_error_for(&error).code),
                code
            );
        }
    }
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
            socket_lease_error(
                "ACQUIRE_DIRECT_EGRESS",
                WindowsVpnError::Remote {
                    code: "AGENT_STALE_GENERATION".to_owned(),
                    message: "192.0.2.4 private-network".to_owned(),
                    retryable: true,
                }
            ),
            STALE_GENERATION_REASON
        );
        let error = socket_lease_error(
            "ACQUIRE_DIRECT_EGRESS",
            WindowsVpnError::Remote {
                code: "AGENT_DIRECT_EGRESS_FAILED".to_owned(),
                message: "192.0.2.4 private-network".to_owned(),
                retryable: true,
            },
        );
        assert!(error.contains("ACQUIRE_DIRECT_EGRESS: AGENT_DIRECT_EGRESS_FAILED"));
        assert!(!error.contains("192.0.2.4") && !error.contains("private-network"));
    }

    #[test]
    fn egress_diagnostics_preserve_only_allowlisted_codes_and_local_stages() {
        let directory = tempfile::tempdir().unwrap();
        let config = directory.path().join("config.json");
        let writer = crate::logging::LogWriterFactory::open(&config).unwrap();
        let subscriber = tracing_subscriber::fmt()
            .with_ansi(false)
            .json()
            .with_writer(writer)
            .finish();
        tracing::subscriber::with_default(subscriber, || {
            for code in [
                "AGENT_OWNER_MISMATCH",
                "AGENT_DIRECT_EGRESS_FAILED",
                "AGENT_DIRECT_EGRESS_NOT_READY",
                "AGENT_DIRECT_EGRESS_UNAVAILABLE",
                "AGENT_DIRECT_EGRESS_LIMIT",
                "AGENT_INVALID_DIRECT_EGRESS",
                "AGENT_PHYSICAL_NETWORK_UNAVAILABLE",
                "AGENT_WFP_PROVIDER_NOT_FOUND",
                "AGENT_WFP_SUBLAYER_NOT_FOUND",
                "AGENT_PROTOCOL_MISMATCH",
                "AGENT_SHUTTING_DOWN",
                "private-code-fixture",
            ] {
                let text = socket_lease_error(
                    "ACQUIRE_DIRECT_EGRESS",
                    WindowsVpnError::Remote {
                        code: code.to_owned(),
                        message: "192.0.2.4 private-network token-fixture".to_owned(),
                        retryable: true,
                    },
                );
                let expected = if code == "private-code-fixture" {
                    "AGENT_OPERATION_FAILED"
                } else {
                    code
                };
                assert!(text.contains(&format!("ACQUIRE_DIRECT_EGRESS: {expected}")));
                assert!(!text.contains("192.0.2.4") && !text.contains("private-"));
            }
            for (error, code) in [
                (WindowsVpnError::RpcTimeout, "AGENT_RPC_TIMEOUT"),
                (
                    WindowsVpnError::InvalidDirectEgressLease,
                    "AGENT_INVALID_DIRECT_EGRESS_LEASE",
                ),
                (
                    WindowsVpnError::ResponseIdMismatch,
                    "AGENT_INVALID_RESPONSE",
                ),
                (
                    WindowsVpnError::Io(io::Error::other("private-io-fixture")),
                    "AGENT_UNREACHABLE",
                ),
            ] {
                let text = socket_lease_error("VERIFY_PHYSICAL_NETWORK", error);
                assert!(text.contains(&format!("VERIFY_PHYSICAL_NETWORK: {code}")));
                assert!(!text.contains("private-"));
            }
        });
        let log = std::fs::read_to_string(directory.path().join("logs/engine.jsonl")).unwrap();
        assert!(log.contains("AGENT_WFP_SUBLAYER_NOT_FOUND"));
        assert!(log.contains("ACQUIRE_DIRECT_EGRESS"));
        assert!(log.contains("VERIFY_PHYSICAL_NETWORK"));
        for private in [
            "192.0.2.4",
            "private-network",
            "token-fixture",
            "private-code-fixture",
            "private-io-fixture",
        ] {
            assert!(!log.contains(private));
        }
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
        // A retry must still present a currently Clean state. A new active
        // transaction cannot be authorized using an earlier Clean response.
        state.phase = agent_v1::AgentPhase::Active as i32;
        assert!(protector.complete_proxy_detach(&state, Ok(())).is_err());
        assert_eq!(protector.egress_generation(7), Ok(Some(3)));
        state.phase = agent_v1::AgentPhase::Clean as i32;
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
    fn closed_vpn_transaction_cannot_authorize_new_hot_mutations() {
        let operation = Uuid::new_v4();
        assert!(require_open_vpn_transaction(true, operation).is_ok());
        assert!(matches!(
            require_open_vpn_transaction(false, operation),
            Err(WindowsVpnError::RecoveryRequired { .. })
        ));
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
    fn gate_private_dns_is_advertised_through_internal_host_routes_without_geo() {
        let mut profile = Profile {
            allow_lan: true,
            dns_servers: vec!["10.8.0.1".parse().unwrap()],
            ..Profile::default()
        };
        profile.vpn_gate.enabled = true;
        let plan = tunnel_plan_from_assignment(
            &profile,
            "10.8.0.2".parse().unwrap(),
            Ipv6Addr::UNSPECIFIED,
            &[],
            false,
        );
        assert!(plan.split_dns);
        assert_eq!(plan.dns_servers, [SPLIT_DNS_IPV4.to_string()]);
        assert!(plan.assigned_ipv6.is_empty());
        profile.dns_mode = usque_core::DnsMode::LocalConfigured;
        let local = tunnel_plan_from_assignment(
            &profile,
            "10.8.0.2".parse().unwrap(),
            Ipv6Addr::UNSPECIFIED,
            &[],
            false,
        );
        assert!(!local.split_dns);
        assert_eq!(local.dns_servers, ["10.8.0.1"]);
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
    fn l4_tun_plan_uses_registered_addresses_and_internal_dns_without_geo() {
        let identity = identity();
        let profile = Profile {
            data_plane: usque_core::DataPlaneMode::L4Proxy,
            ..Profile::default()
        };
        let plan = tunnel_plan(&profile, &identity, &[], false);
        assert!(plan.split_dns);
        assert_eq!(plan.dns_servers, ["198.18.0.1", "fd00::1"]);
        assert_eq!(plan.assigned_ipv4, format!("{}/32", identity.assigned_ipv4));
        assert_eq!(
            plan.assigned_ipv6,
            format!("{}/128", identity.assigned_ipv6)
        );
        assert_eq!(plan.endpoint_candidates.len(), 2);
        assert!(!plan.allow_lan);
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
    async fn diagnostics_never_start_an_unavailable_agent_and_accept_an_old_response() {
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
        assert!(client.inspect_platform_state_if_running().await.is_err());
        assert_eq!(controller.starts.load(Ordering::Acquire), 0);
        let (client, task) = scripted_recovery_client(vec![AgentResponse {
            payload: Some(agent_response::Payload::PlatformState(PlatformState {
                journal_generation: 19,
                ..Default::default()
            })),
            ..Default::default()
        }]);
        let state = client.inspect_platform_state_if_running().await.unwrap();
        assert_eq!(
            crate::recovery_diagnostics::summary(Some(&state))["availability"],
            "extension_unavailable"
        );
        assert!(matches!(
            task.await.unwrap().as_slice(),
            [agent_request::Payload::InspectPlatformState(_)]
        ));
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
    async fn lifecycle_logs_link_journal_generations_without_copying_ipc_data_or_poll_noise() {
        use tracing::instrument::WithSubscriber;

        let directory = tempfile::tempdir().unwrap();
        let config = directory.path().join("config.json");
        let writer = crate::logging::LogWriterFactory::open(&config).unwrap();
        let subscriber = tracing_subscriber::fmt()
            .json()
            .with_writer(writer)
            .finish();
        let dispatch = tracing::Dispatch::new(subscriber);
        let mut prepared = recovery_state_response(agent_v1::AgentPhase::Prepared);
        if let Some(agent_response::Payload::State(state)) = prepared.payload.as_mut() {
            state.profile_id = "private-agent-fixture".into();
        }
        let (client, server) =
            scripted_recovery_client(vec![prepared.clone(), prepared, AgentResponse::default()]);
        let lease = client
            .prepare(
                Uuid::new_v4(),
                agent_v1::TunnelPlan {
                    profile_id: "private-request-fixture".into(),
                    ..Default::default()
                },
            )
            .with_subscriber(dispatch.clone())
            .await
            .unwrap();
        drop(lease);
        client
            .get_state()
            .with_subscriber(dispatch.clone())
            .await
            .unwrap();
        assert!(matches!(
            client
                .commit(Uuid::new_v4())
                .with_subscriber(dispatch)
                .await,
            Err(WindowsVpnError::MissingResponse)
        ));
        server.await.unwrap();
        let text =
            std::fs::read_to_string(crate::logging::log_directory(&config).join("engine.jsonl"))
                .unwrap();
        assert!(!text.contains("private-agent-fixture"));
        assert!(!text.contains("private-request-fixture"));
        let events: Vec<serde_json::Value> = text
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(events.len(), 4, "GetState polling must remain quiet");
        assert_eq!(events[0]["fields"]["method"], "prepare");
        assert_eq!(events[1]["fields"]["journal_generation"], 19);
        assert_eq!(
            events[1]["fields"]["agent_phase"],
            agent_v1::AgentPhase::Prepared as i32
        );
        assert_eq!(events[1]["fields"]["success"], true);
        assert_eq!(
            events[3]["fields"]["success"], false,
            "an empty reply cannot be logged as success"
        );
        assert_eq!(events[0]["engine_run_id"], events[1]["engine_run_id"]);
    }

    #[tokio::test]
    async fn cancelled_gate_startup_releases_the_actual_prepare_ipc_pipe() {
        let pipe_name = format!("{AGENT_PIPE_NAME}.test-{}", Uuid::new_v4());
        let mut server = ServerOptions::new()
            .first_pipe_instance(true)
            .create(&pipe_name)
            .unwrap();
        let operation = Uuid::new_v4();
        let server_task = tokio::spawn(async move {
            server.connect().await.unwrap();
            let mut header = [0; 4];
            server.read_exact(&mut header).await.unwrap();
            let mut payload = vec![0; u32::from_be_bytes(header) as usize];
            server.read_exact(&mut payload).await.unwrap();
            let mut frame = BytesMut::from(header.as_slice());
            frame.extend_from_slice(&payload);
            let request: AgentRequest = decode_frame(frame.freeze()).unwrap();
            assert!(matches!(
                request.payload,
                Some(agent_request::Payload::PrepareTunnel(_))
            ));
            let response = AgentResponse {
                request_id: request.request_id,
                payload: Some(agent_response::Payload::State(AgentState {
                    phase: agent_v1::AgentPhase::Prepared as i32,
                    operation_id: operation.to_string(),
                    ..Default::default()
                })),
                ..Default::default()
            };
            server
                .write_all(&encode_frame(&response).unwrap())
                .await
                .unwrap();
            match server.read(&mut [0; 1]).await {
                Ok(0) => {}
                Err(error) if error.kind() == io::ErrorKind::BrokenPipe => {}
                other => panic!("expected lease EOF without promotion: {other:?}"),
            }
        });
        let client = WindowsAgentClient::for_test(pipe_name);
        let mut lease = Some(
            client
                .prepare(operation, agent_v1::TunnelPlan::default())
                .await
                .unwrap(),
        );
        let cancel = CancellationToken::new();
        cancel.cancel();
        let result: Result<(), TransportError> =
            await_prepared_gate_startup(&cancel, &mut lease, std::future::pending()).await;
        assert!(matches!(result, Err(TransportError::TunnelClosed)));
        timeout(Duration::from_secs(1), server_task)
            .await
            .unwrap()
            .unwrap();
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
