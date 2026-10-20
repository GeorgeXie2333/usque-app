//! Linux TUN ownership: the privileged Agent owns every host mutation while
//! this ordinary-user runtime owns protocol keys, transport and packet data.
use crate::{
    ControlServiceError, desktop_tunnel_plan, linux_agent::LinuxAgentClient,
    linux_socket::LinuxSocketProtector, linux_system_proxy::LinuxSystemProxyGuard,
};
use bytes::Bytes;
use std::{
    io,
    os::fd::{AsRawFd, OwnedFd},
    sync::Arc,
    time::Duration,
};
use tokio::{io::unix::AsyncFd, sync::watch};
use tokio_util::{sync::CancellationToken, task::AbortOnDropHandle};
use usque_core::{Profile, chain_exit::store::ProfileCipher};
use usque_ipc::agent_v1;
use usque_transport::{
    DataPlaneRuntime, EndpointPinRefresher, GeoDirectPolicy, MasqueTlsIdentity, RuntimeHealth,
    SocketProtector, TransportError, TunPacketIo,
};
use uuid::Uuid;

pub(crate) fn map_error(error: io::Error) -> ControlServiceError {
    let detail = crate::linux_agent::failure_detail(&error);
    let retryable = crate::linux_agent::retryable(&error);
    if let Some(inner) = error.into_inner() {
        let inner = match inner.downcast::<TransportError>() {
            Ok(transport) => return ControlServiceError::Transport(*transport),
            Err(inner) => inner,
        };
        if let Ok(startup) = inner.downcast::<StartupError>() {
            return map_error(startup.primary);
        }
    }
    ControlServiceError::LinuxPlatform {
        detail: detail.map(Box::new),
        retryable,
    }
}

#[derive(Debug)]
struct StartupError {
    primary: io::Error,
    cleanup: io::Error,
}
impl std::fmt::Display for StartupError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Linux activation and cleanup failed")
    }
}
impl std::error::Error for StartupError {}

pub(crate) fn startup_evidence(
    error: &io::Error,
) -> crate::connection_evidence::StartupFailureEvidence {
    if let Some(startup) = error
        .get_ref()
        .and_then(|e| e.downcast_ref::<StartupError>())
    {
        return crate::connection_evidence::StartupFailureEvidence {
            detail: crate::linux_agent::failure_detail(&startup.primary),
            cleanup_detail: crate::linux_agent::failure_detail(&startup.cleanup),
            cleanup_status: "failed",
            cleanup_owner: None,
        };
    }
    let mut detail = crate::linux_agent::failure_detail(error);
    let cleanup_detail = detail
        .as_mut()
        .and_then(|d| d.cleanup_detail.take())
        .map(|d| *d);
    crate::connection_evidence::StartupFailureEvidence {
        cleanup_detail,
        detail,
        cleanup_status: "pending",
        cleanup_owner: None,
    }
}

#[derive(Clone, Default)]
pub(crate) struct StartupAttempt {
    pub telemetry: usque_transport::ConnectionTelemetry,
    pub cleanup: crate::connection_evidence::StartupCleanup,
}

impl StartupAttempt {
    pub fn evidence(
        &self,
        error: &io::Error,
    ) -> crate::connection_evidence::StartupFailureEvidence {
        let mut evidence = startup_evidence(error);
        evidence.cleanup_owner = Some(self.cleanup.clone());
        evidence
    }
}
async fn bounded_startup<T>(
    cancellation: &CancellationToken,
    deadline: tokio::time::Instant,
    startup: impl std::future::Future<Output = io::Result<T>>,
) -> io::Result<T> {
    tokio::select! {
        biased;
        _ = cancellation.cancelled() => Err(io::Error::new(io::ErrorKind::Interrupted, "Linux startup cancelled")),
        result = tokio::time::timeout_at(deadline, startup) =>
            result.unwrap_or_else(|_| Err(usque_ipc::linux_failure::wrap(agent_v1::LinuxFailureStage::Activation, io::Error::new(io::ErrorKind::TimedOut, "Linux startup timed out")))),
    }
}

fn error(value: impl std::fmt::Display) -> io::Error {
    io::Error::other(value.to_string())
}

pub(crate) struct LinuxVpnRuntime {
    tunnel: DataPlaneRuntime,
    telemetry: usque_transport::ConnectionTelemetry,
    startup_failure: Option<crate::connection_evidence::StartupFailureEvidence>,
    agent: LinuxAgentClient,
    operation: Uuid,
    plan: agent_v1::TunnelPlan,
    cipher: Arc<dyn ProfileCipher>,
    protector: Arc<LinuxSocketProtector>,
    system_proxy: Option<LinuxSystemProxyGuard>,
    cancel: CancellationToken,
    packets_cancel: CancellationToken,
    packet_task: Option<AbortOnDropHandle<()>>,
    monitor: Option<AbortOnDropHandle<()>>,
    failure: watch::Receiver<Option<String>>,
    failure_tx: watch::Sender<Option<String>>,
    transaction_open: bool,
    retained: bool,
    replacement_target: Option<Uuid>,
    replacement_source: Option<Uuid>,
    target_committed: bool,
}
impl std::ops::Deref for LinuxVpnRuntime {
    type Target = DataPlaneRuntime;
    fn deref(&self) -> &Self::Target {
        &self.tunnel
    }
}
impl std::ops::DerefMut for LinuxVpnRuntime {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.tunnel
    }
}

/// Any cancelled startup closes its authenticated ownership connection after a
/// best-effort exact-operation rollback. Agent-side orphan recovery is the
/// durable fallback when the process exits or the response is lost.
struct StartupGuard {
    agent: LinuxAgentClient,
    id: Uuid,
    armed: bool,
    cleanup: crate::connection_evidence::StartupCleanup,
}
impl Drop for StartupGuard {
    fn drop(&mut self) {
        if self.armed {
            let agent = self.agent.clone();
            let id = self.id;
            let cleanup = self.cleanup.clone();
            if let Ok(runtime) = tokio::runtime::Handle::try_current() {
                runtime.spawn(async move {
                    let result = restore_agent_transaction(&agent, id, None).await;
                    cleanup.complete(&result);
                });
            }
        }
    }
}
impl LinuxVpnRuntime {
    pub fn startup_failure(&self) -> Option<crate::connection_evidence::StartupFailureEvidence> {
        self.startup_failure.clone()
    }
    pub(crate) fn operation_id(&self) -> Uuid {
        self.operation
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "startup receives caller-owned evidence alongside transport and platform dependencies"
    )]
    pub(crate) async fn start(
        profile: &Profile,
        identity: MasqueTlsIdentity,
        refresher: Arc<dyn EndpointPinRefresher>,
        policy: Arc<GeoDirectPolicy>,
        gate: usque_transport::VpnGateStart,
        cipher: Arc<dyn ProfileCipher>,
        attempt: StartupAttempt,
    ) -> io::Result<Self> {
        let cancellation = gate.cancellation.clone();
        let deadline = gate.deadline.unwrap_or_else(|| {
            tokio::time::Instant::now() + usque_core::endpoint_connection_budget(profile)
        });
        attempt.telemetry.record(
            usque_transport::ConnectionEventType::AttemptStarted,
            Some(usque_core::TransportStage::TunnelStartup),
            Default::default(),
            None,
            None,
        );
        bounded_startup(
            &cancellation,
            deadline,
            Self::start_inner(profile, identity, refresher, policy, gate, cipher, attempt),
        )
        .await
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "startup receives caller-owned evidence alongside transport and platform dependencies"
    )]
    async fn start_inner(
        profile: &Profile,
        identity: MasqueTlsIdentity,
        refresher: Arc<dyn EndpointPinRefresher>,
        policy: Arc<GeoDirectPolicy>,
        gate: usque_transport::VpnGateStart,
        cipher: Arc<dyn ProfileCipher>,
        attempt: StartupAttempt,
    ) -> io::Result<Self> {
        let telemetry = attempt.telemetry;
        let agent = LinuxAgentClient::connect().await.map_err(|e| {
            usque_ipc::linux_failure::wrap(agent_v1::LinuxFailureStage::AgentConnect, e)
        })?;
        let caps = agent.capabilities().await.map_err(|e| {
            crate::linux_agent::local_stage(agent_v1::LinuxFailureStage::AgentHandshake, e)
        })?;
        validate_capabilities(&caps, profile).map_err(|e| {
            usque_ipc::linux_failure::wrap(agent_v1::LinuxFailureStage::Preflight, e)
        })?;
        let state = agent.state().await?;
        if state.phase != agent_v1::AgentPhase::Clean as i32 {
            if state.operation_id.is_empty() {
                return Err(error("Linux Agent recovery state is incomplete"));
            }
            agent.recover(&state).await?;
        }
        let state = agent.state().await?;
        if state.phase != agent_v1::AgentPhase::Clean as i32 {
            return Err(error(
                "Linux Agent recovery must complete before connecting",
            ));
        }
        // Resolve the one control host while system DNS still belongs to the
        // physical network. Later refreshes only consume this numeric snapshot.
        let registration = tokio::task::spawn_blocking(|| {
            usque_transport::NoopSocketProtector.resolve(
                usque_core::REGISTRATION_API_HOST,
                usque_core::REGISTRATION_API_PORT,
            )
        })
        .await
        .map_err(error)?
        .map_err(error)?;
        let mut plan = desktop_tunnel_plan::tunnel_plan(
            profile,
            &identity,
            &registration,
            policy.is_enabled() || profile.needs_domain_routing(),
        );
        let operation = Uuid::new_v4();
        let mut startup = StartupGuard {
            agent: agent.clone(),
            id: operation,
            armed: true,
            cleanup: attempt.cleanup.clone(),
        };
        attempt.cleanup.pending();
        agent
            .prepare(operation, plan.clone(), state.journal_generation)
            .await?;
        let protector =
            LinuxSocketProtector::new(agent.clone(), operation, registration, &plan).await?;
        let cancel = CancellationToken::new();
        let (failure_tx, failure) = watch::channel(None);
        let monitor = protector.monitor(cancel.clone(), failure_tx.clone());
        let mut tunnel = match Box::pin(DataPlaneRuntime::start_with_vpngate_and_telemetry(
            profile,
            identity,
            protector.clone(),
            Some(refresher),
            policy,
            gate,
            telemetry.clone(),
        ))
        .await
        {
            Ok(runtime) => runtime,
            Err(error_value) => {
                cancel.cancel();
                drop(monitor);
                return Err(io::Error::other(error_value));
            }
        };
        let prepared = async {
            if plan.defer_network_configuration {
                apply_final_network(&mut plan, &tunnel.network_parameters());
                agent.finalize(operation, plan.clone()).await?;
            }
            if tunnel.gate_status().stage == usque_core::vpngate::GateStage::Error {
                return Err(error("Linux final exit did not become ready"));
            }
            let (_, descriptor) = agent.packet_session(operation).await?;
            protector.set_device(&descriptor).map_err(|e| {
                usque_ipc::linux_failure::wrap(agent_v1::LinuxFailureStage::PacketDescriptor, e)
            })?;
            let device = register_packet_descriptor(descriptor).map_err(|e| {
                usque_ipc::linux_failure::wrap(agent_v1::LinuxFailureStage::PacketDescriptor, e)
            })?;
            let io = tunnel.attach_tun().map_err(io::Error::other)?;
            agent.commit(operation).await?;
            Ok::<_, io::Error>((device, io))
        }
        .await;
        let (device, io) = match prepared {
            Ok(value) => value,
            Err(failure) => {
                cancel.cancel();
                tunnel.shutdown().await;
                return Err(failure);
            }
        };
        let packets_cancel = cancel.child_token();
        let packet_task = spawn_packet_pump(io, device, packets_cancel.clone(), failure_tx.clone());
        let mut runtime = Self {
            tunnel,
            telemetry,
            startup_failure: None,
            agent,
            operation,
            plan,
            cipher,
            protector,
            system_proxy: None,
            cancel,
            packets_cancel,
            packet_task: Some(packet_task),
            monitor: Some(monitor),
            failure,
            failure_tx,
            transaction_open: true,
            retained: false,
            replacement_target: None,
            replacement_source: None,
            target_committed: true,
        };
        startup.armed = false;
        let activation = async {
            runtime.replace_system_proxy(profile).await?;
            runtime
                .tunnel
                .activate_final()
                .await
                .map_err(io::Error::other)
        }
        .await;
        if let Err(activation) = activation {
            let result = runtime.shutdown().await;
            attempt.cleanup.complete(&result);
            return match result {
                Ok(()) => Err(activation),
                Err(cleanup) => Err(io::Error::other(StartupError {
                    primary: activation,
                    cleanup,
                })),
            };
        }
        Ok(runtime)
    }
    pub(crate) async fn replace_system_proxy(&mut self, profile: &Profile) -> io::Result<()> {
        if let Some(proxy) = &mut self.system_proxy {
            proxy.shutdown().await?;
        }
        self.system_proxy = None;
        if profile.frontends.http && profile.proxy.system_proxy {
            let listener = self
                .tunnel
                .http_listeners()
                .iter()
                .copied()
                .find(|address| address.ip().is_loopback())
                .ok_or_else(|| error("system proxy requires a loopback HTTP listener"))?;
            self.system_proxy =
                Some(LinuxSystemProxyGuard::start(listener, Arc::clone(&self.cipher)).await?);
        }
        Ok(())
    }
    pub(crate) fn system_proxy_active(&self) -> bool {
        self.system_proxy.is_some()
    }
    pub(crate) fn internal_networks(
        &self,
    ) -> Option<(
        usque_transport::InternalNetwork,
        usque_transport::InternalNetwork,
    )> {
        Some((
            self.tunnel.internal_network(),
            self.tunnel.warp_internal_network(),
        ))
    }
    pub(crate) fn diagnostic_dns_context(
        &self,
    ) -> Option<(Arc<dyn SocketProtector>, CancellationToken)> {
        Some(self.tunnel.diagnostic_dns_context())
    }
    pub(crate) fn needs_warp_bootstrap(&self) -> bool {
        false
    }
    pub(crate) fn replacement_pending(&self) -> bool {
        self.replacement_target.is_some()
    }
    pub(crate) fn target_committed(&self) -> bool {
        self.target_committed
    }
    pub(crate) fn requires_agent_reattach(&self) -> bool {
        false
    }
    pub(crate) fn failure_retained(&self) -> bool {
        self.retained
    }
    pub(crate) fn update_traffic_policy(&mut self, disabled: bool) -> io::Result<()> {
        self.tunnel.update_traffic_policy(disabled);
        Ok(())
    }
    pub(crate) async fn reconfigure_frontends(&mut self, profile: &Profile) -> io::Result<()> {
        self.tunnel
            .reconfigure_frontends(profile)
            .await
            .map_err(io::Error::other)
    }
    pub(crate) fn health(&self) -> RuntimeHealth {
        if let Some(message) = self.failure.borrow().clone() {
            let transport = TransportError::SocketProtection(message.clone());
            RuntimeHealth::Failed {
                last_path: self.tunnel.path(),
                reconnect_count: 0,
                failure: transport.failure(None, None),
                message,
            }
        } else {
            self.tunnel.health()
        }
    }
    pub(crate) fn failure(&self) -> Option<String> {
        self.failure
            .borrow()
            .clone()
            .or_else(|| self.tunnel.failure())
    }
    pub(crate) fn cancel_immediately(&mut self) {
        self.tunnel.cancel_immediately();
        self.packets_cancel.cancel();
    }
    async fn stop_packets(&mut self) {
        self.packets_cancel.cancel();
        if let Some(mut task) = self.packet_task.take() {
            let _ = tokio::time::timeout(Duration::from_secs(5), &mut task).await;
        }
        self.tunnel.detach_tun();
        self.protector.clear_device();
    }
    pub(crate) async fn retain_failed_gate(
        &mut self,
        reason: usque_core::vpngate::GateFailure,
    ) -> io::Result<()> {
        self.tunnel.quiesce_final();
        self.stop_packets().await;
        if self.replacement_target.is_none() && self.plan.vpn_chain {
            self.agent.begin_chain_transition(self.operation).await?;
        } else {
            let state = self.agent.state().await?;
            if state.phase == agent_v1::AgentPhase::Clean as i32 || !state.kill_switch_active {
                return Err(error("Linux protection could not be verified"));
            }
        }
        self.tunnel.fail_gate(reason).await;
        self.retained = true;
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
        cancellation: &CancellationToken,
    ) -> io::Result<()> {
        self.startup_failure = None;
        let result = self
            .replace_gate_inner(profile, selected, policy, status, cancellation)
            .await;
        if let Err(error) = &result {
            let mut failure = startup_evidence(error);
            failure.cleanup_status = "retained";
            self.startup_failure = Some(failure);
        }
        result
    }
    async fn replace_gate_inner(
        &mut self,
        profile: &Profile,
        selected: Option<(
            usque_core::vpngate::ServerSummary,
            usque_core::vpngate::PreparedProfile,
        )>,
        policy: Arc<GeoDirectPolicy>,
        status: watch::Sender<usque_core::vpngate::GateStatus>,
        cancellation: &CancellationToken,
    ) -> io::Result<()> {
        self.tunnel.quiesce_final();
        self.stop_packets().await;
        self.agent.begin_chain_transition(self.operation).await?;
        self.retained = true;
        Box::pin(
            self.tunnel
                .replace_gate(profile, selected, policy, status, cancellation),
        )
        .await
        .map_err(io::Error::other)?;
        let mut plan = self.plan.clone();
        plan.vpn_chain = profile.chain_enabled();
        apply_final_network(&mut plan, &self.tunnel.network_parameters());
        self.agent.finalize(self.operation, plan.clone()).await?;
        let (_, descriptor) = self.agent.packet_session(self.operation).await?;
        self.protector.set_device(&descriptor).map_err(|e| {
            usque_ipc::linux_failure::wrap(agent_v1::LinuxFailureStage::PacketDescriptor, e)
        })?;
        let device = register_packet_descriptor(descriptor).map_err(|e| {
            usque_ipc::linux_failure::wrap(agent_v1::LinuxFailureStage::PacketDescriptor, e)
        })?;
        let io = self.tunnel.attach_tun().map_err(io::Error::other)?;
        self.agent.commit(self.operation).await?;
        self.packets_cancel = self.cancel.child_token();
        self.packet_task = Some(spawn_packet_pump(
            io,
            device,
            self.packets_cancel.clone(),
            self.failure_tx.clone(),
        ));
        self.replace_system_proxy(profile).await?;
        self.tunnel
            .activate_final()
            .await
            .map_err(io::Error::other)?;
        self.plan = plan;
        self.retained = false;
        Ok(())
    }
    /// Replaces the entire transport behind an Agent-owned durable guard. A
    /// failed attempt remains owned by this runtime until retry or disconnect.
    pub(crate) async fn replace_connection(
        &mut self,
        profile: &Profile,
        identity: MasqueTlsIdentity,
        refresher: Arc<dyn EndpointPinRefresher>,
        policy: Arc<GeoDirectPolicy>,
        gate: usque_transport::VpnGateStart,
    ) -> io::Result<()> {
        self.startup_failure = None;
        self.telemetry.record(
            usque_transport::ConnectionEventType::AttemptStarted,
            Some(usque_core::TransportStage::TunnelStartup),
            Default::default(),
            None,
            None,
        );
        let result = self
            .replace_connection_inner(profile, identity, refresher, policy, gate)
            .await;
        if let Err(error) = &result {
            let mut failure = startup_evidence(error);
            failure.cleanup_status = "retained";
            self.startup_failure = Some(failure);
        }
        result
    }

    async fn replace_connection_inner(
        &mut self,
        profile: &Profile,
        identity: MasqueTlsIdentity,
        refresher: Arc<dyn EndpointPinRefresher>,
        policy: Arc<GeoDirectPolicy>,
        gate: usque_transport::VpnGateStart,
    ) -> io::Result<()> {
        let registration = self
            .plan
            .control_api_candidates
            .iter()
            .map(|value| value.parse())
            .collect::<Result<Vec<std::net::SocketAddr>, _>>()
            .map_err(error)?;
        let mut plan = desktop_tunnel_plan::tunnel_plan(
            profile,
            &identity,
            &registration,
            policy.is_enabled() || profile.needs_domain_routing(),
        );
        validate_capabilities(&self.agent.capabilities().await?, profile)?;
        let state = self.agent.state().await?;
        let target = self.replacement_target.unwrap_or_else(Uuid::new_v4);
        self.tunnel.quiesce_final();
        if let Some(proxy) = &mut self.system_proxy {
            proxy.shutdown().await?;
        }
        self.system_proxy = None;
        self.stop_packets().await;
        self.cancel.cancel();
        self.monitor = None;
        self.tunnel.shutdown().await;
        self.replacement_target = Some(target);
        let source = *self.replacement_source.get_or_insert(self.operation);
        self.retained = true;
        self.target_committed = false;
        self.agent
            .replace(source, target, plan.clone(), state.journal_generation)
            .await?;
        self.operation = target;
        let protector =
            LinuxSocketProtector::new(self.agent.clone(), target, registration, &plan).await?;
        self.cancel = CancellationToken::new();
        self.failure_tx.send_replace(None);
        self.monitor = Some(protector.monitor(self.cancel.clone(), self.failure_tx.clone()));
        self.protector = protector.clone();
        let mut tunnel = Box::pin(DataPlaneRuntime::start_with_vpngate_and_telemetry(
            profile,
            identity,
            protector.clone(),
            Some(refresher),
            policy,
            gate,
            self.telemetry.clone(),
        ))
        .await
        .map_err(io::Error::other)?;
        let activation = async {
            if plan.defer_network_configuration {
                apply_final_network(&mut plan, &tunnel.network_parameters());
                self.agent.finalize(target, plan.clone()).await?;
            }
            if tunnel.gate_status().stage == usque_core::vpngate::GateStage::Error {
                return Err(error("Linux final exit did not become ready"));
            }
            let (_, descriptor) = self.agent.packet_session(target).await?;
            self.protector.set_device(&descriptor).map_err(|e| {
                usque_ipc::linux_failure::wrap(agent_v1::LinuxFailureStage::PacketDescriptor, e)
            })?;
            let device = register_packet_descriptor(descriptor).map_err(|e| {
                usque_ipc::linux_failure::wrap(agent_v1::LinuxFailureStage::PacketDescriptor, e)
            })?;
            let io = tunnel.attach_tun().map_err(io::Error::other)?;
            self.agent.commit(target).await?;
            self.target_committed = true;
            self.packets_cancel = self.cancel.child_token();
            self.packet_task = Some(spawn_packet_pump(
                io,
                device,
                self.packets_cancel.clone(),
                self.failure_tx.clone(),
            ));
            tunnel.activate_final().await.map_err(io::Error::other)
        }
        .await;
        self.tunnel = tunnel;
        activation?;
        self.replace_system_proxy(profile).await?;
        self.plan = plan;
        self.replacement_target = None;
        self.replacement_source = None;
        self.retained = false;
        Ok(())
    }
    pub(crate) async fn shutdown(&mut self) -> io::Result<()> {
        self.tunnel.quiesce_final();
        let proxy = match self.system_proxy.as_mut() {
            Some(proxy) => proxy.shutdown().await,
            None => Ok(()),
        };
        self.stop_packets().await;
        self.tunnel.shutdown().await;
        self.cancel.cancel();
        self.monitor = None;
        if self.transaction_open {
            let replacement = self
                .replacement_target
                .map(|target| {
                    self.replacement_source
                        .map(|source| (source, target))
                        .ok_or_else(|| error("Linux replacement source is missing"))
                })
                .transpose()?;
            restore_agent_transaction(&self.agent, self.operation, replacement).await?;
            self.transaction_open = false;
        }
        proxy?;
        self.system_proxy = None;
        Ok(())
    }
}

/// A replacement request can fail before its intent is recorded or after the
/// target guard is installed. Keep both operation IDs until authenticated state
/// identifies which transaction owns recovery; an IPC error alone cannot do so.
async fn restore_agent_transaction(
    agent: &LinuxAgentClient,
    operation: Uuid,
    replacement: Option<(Uuid, Uuid)>,
) -> io::Result<()> {
    let state = agent.recovery_state().await?;
    if state.phase == agent_v1::AgentPhase::Clean as i32
        && state.operation_id.is_empty()
        && state.replacement.is_none()
        && !state.kill_switch_active
    {
        // Recovery may have succeeded before its acknowledgement was lost.
        return Ok(());
    }
    let Some((source, target)) = replacement else {
        let operation_id = operation.to_string();
        if state.operation_id != operation_id
            || state
                .replacement
                .as_ref()
                .is_some_and(|status| status.operation_id != operation_id)
        {
            return Err(error("Linux recovery ownership changed"));
        }
        return agent.rollback(operation).await;
    };
    let source_id = source.to_string();
    let target_id = target.to_string();
    if let Some(status) = &state.replacement
        && status.operation_id == target_id
        && status.source_operation_id == source_id
        && (state.operation_id == source_id || state.operation_id == target_id)
    {
        return agent
            .abort_replacement(target, state.journal_generation)
            .await;
    }
    if state.operation_id == source_id
        && state
            .replacement
            .as_ref()
            .is_none_or(|status| status.operation_id == source_id)
    {
        // A stale generation or failed preflight can reject the request before
        // creating any target. The source may itself be a completed replacement.
        return agent.rollback(source).await;
    }
    Err(error("Linux replacement recovery ownership changed"))
}
impl Drop for LinuxVpnRuntime {
    fn drop(&mut self) {
        self.cancel.cancel();
        self.packets_cancel.cancel();
        self.protector.clear_device();
        self.tunnel.cancel_immediately();
    }
}
fn apply_final_network(
    plan: &mut agent_v1::TunnelPlan,
    network: &usque_core::vpngate::FinalNetworkParameters,
) {
    plan.mtu = u32::from(network.mtu);
    plan.assigned_ipv4 = network
        .ipv4
        .map(|ip| format!("{ip}/32"))
        .unwrap_or_default();
    plan.assigned_ipv6 = network
        .ipv6
        .map(|ip| format!("{ip}/128"))
        .unwrap_or_default();
    plan.dns_servers = if plan.split_dns {
        [
            network
                .ipv4
                .map(|_| usque_transport::SPLIT_DNS_IPV4.to_string()),
            network
                .ipv6
                .map(|_| usque_transport::SPLIT_DNS_IPV6.to_string()),
        ]
        .into_iter()
        .flatten()
        .collect()
    } else {
        network
            .dns_servers
            .iter()
            .map(ToString::to_string)
            .collect()
    };
    plan.defer_network_configuration = false;
}
pub(crate) fn validate_capabilities(
    caps: &agent_v1::AgentCapabilities,
    profile: &Profile,
) -> io::Result<()> {
    if !(caps.linux_tun
        && caps.unix_fd_transfer
        && caps.nftables_kill_switch
        && caps.interface_addresses
        && caps.interface_dns
        && caps.dynamic_direct_egress
        && caps.physical_dns_snapshot
        && caps.exact_generation_egress
        && caps.guarded_recovery)
    {
        return Err(error(
            "Linux Agent does not support the required protection protocol",
        ));
    }
    if profile.chain_enabled() && !caps.deferred_network_configuration {
        return Err(error(
            "Linux Agent does not support deferred chain networking",
        ));
    }
    if profile.endpoint.selection == usque_core::EndpointSelection::Automatic
        && !caps.automatic_endpoint_leases
    {
        return Err(error(
            "Linux Agent does not support automatic endpoint leases",
        ));
    }
    if profile.endpoint.preset_region.is_some() && !caps.endpoint_preset_leases {
        return Err(error("Linux Agent does not support preset endpoint leases"));
    }
    Ok(())
}
fn register_packet_descriptor(descriptor: OwnedFd) -> io::Result<AsyncFd<OwnedFd>> {
    // SAFETY: AsyncFd takes ownership of this live descriptor. Packet I/O only
    // borrows it through get_ref; no code closes or replaces the descriptor or
    // changes its underlying file description before AsyncFd deregisters it.
    // Converting a registration error to io::Error also drops its OwnedFd.
    unsafe { AsyncFd::register(descriptor) }.map_err(io::Error::from)
}

fn spawn_packet_pump(
    mut transport: TunPacketIo,
    device: AsyncFd<OwnedFd>,
    cancel: CancellationToken,
    failure: watch::Sender<Option<String>>,
) -> AbortOnDropHandle<()> {
    AbortOnDropHandle::new(tokio::spawn(async move {
        let mut inbound = [0u8; 65536];
        let mut outbound: Option<Bytes> = None;
        type PacketSend =
            std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), TransportError>> + Send>>;
        let mut upload: Option<PacketSend> = None;
        let result=async {loop {
            tokio::select! {
                ()=cancel.cancelled()=>return Ok::<(),io::Error>(()),
                result=async {match upload.as_mut(){Some(send)=>send.await,None=>std::future::pending().await}},if upload.is_some()=>{result.map_err(error)?;upload=None;},
                result=read_packet(&device,&mut inbound),if upload.is_none()=>{let size=result?;upload=Some(Box::pin(transport.start_send_owned_packet(Bytes::copy_from_slice(&inbound[..size]))));},
                result=transport.receive_packet(),if outbound.is_none()=>{outbound=Some(result.map_err(error)?);},
                result=async {match outbound.as_ref(){Some(packet)=>write_packet(&device,packet).await,None=>std::future::pending().await}},if outbound.is_some()=>{result?;outbound=None;},
            }
        }}.await;
        if result.is_err() && !cancel.is_cancelled() {
            failure.send_replace(Some(
                "Linux packet transport failed; protection is retained".into(),
            ));
            cancel.cancel();
        }
    }))
}
async fn read_packet(device: &AsyncFd<OwnedFd>, buffer: &mut [u8]) -> io::Result<usize> {
    loop {
        let mut ready = device.readable().await?;
        match ready.try_io(|descriptor| {
            // SAFETY: the owned FD stays alive and the buffer is valid and exclusive.
            let size = unsafe {
                libc::read(
                    descriptor.get_ref().as_raw_fd(),
                    buffer.as_mut_ptr().cast(),
                    buffer.len(),
                )
            };
            if size < 0 {
                Err(io::Error::last_os_error())
            } else if size == 0 {
                Err(io::Error::new(io::ErrorKind::UnexpectedEof, "TUN closed"))
            } else {
                Ok(size as usize)
            }
        }) {
            Ok(value) => return value,
            Err(_) => continue,
        }
    }
}
async fn write_packet(device: &AsyncFd<OwnedFd>, packet: &[u8]) -> io::Result<()> {
    loop {
        let mut ready = device.writable().await?;
        match ready.try_io(|descriptor| {
            // SAFETY: the descriptor and packet slice outlive the synchronous write.
            let size = unsafe {
                libc::write(
                    descriptor.get_ref().as_raw_fd(),
                    packet.as_ptr().cast(),
                    packet.len(),
                )
            };
            if size < 0 {
                Err(io::Error::last_os_error())
            } else if size as usize != packet.len() {
                Err(io::Error::new(
                    io::ErrorKind::WriteZero,
                    "partial TUN packet write",
                ))
            } else {
                Ok(())
            }
        }) {
            Ok(value) => return value,
            Err(_) => continue,
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn startup_cancellation_and_timeout_drop_the_pending_owner_and_keep_classification() {
        struct Owner(std::sync::Arc<std::sync::atomic::AtomicBool>);
        impl Drop for Owner {
            fn drop(&mut self) {
                self.0.store(true, std::sync::atomic::Ordering::Release);
            }
        }
        for cancelled in [false, true] {
            let dropped = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
            let owner = Owner(dropped.clone());
            let future = async move {
                let _owner = owner;
                std::future::pending::<io::Result<()>>().await
            };
            let cancellation = CancellationToken::new();
            if cancelled {
                cancellation.cancel();
            }
            let error = bounded_startup(
                &cancellation,
                tokio::time::Instant::now() + Duration::from_millis(1),
                future,
            )
            .await
            .unwrap_err();
            assert!(dropped.load(std::sync::atomic::Ordering::Acquire));
            assert_eq!(
                error.kind(),
                if cancelled {
                    io::ErrorKind::Interrupted
                } else {
                    io::ErrorKind::TimedOut
                }
            );
            if !cancelled {
                assert_eq!(
                    crate::linux_agent::failure_detail(&error).unwrap().reason,
                    agent_v1::LinuxFailureReason::Timeout as i32
                );
            }
        }
    }

    #[test]
    fn tun_startup_preserves_proxy_transport_failure_and_primary_cleanup_separately() {
        for transport in [
            TransportError::EndpointPinMismatch,
            TransportError::TlsHandshake("secret path /private".into()),
            TransportError::ConnectTimeout,
        ] {
            let expected = transport.failure(None, None);
            let cleanup = usque_ipc::linux_failure::wrap(
                agent_v1::LinuxFailureStage::Recovery,
                io::Error::from_raw_os_error(13),
            );
            let error = io::Error::other(StartupError {
                primary: io::Error::other(transport),
                cleanup,
            });
            let evidence = startup_evidence(&error);
            assert_eq!(evidence.cleanup_status, "failed");
            assert_eq!(evidence.cleanup_detail.unwrap().os_errno, Some(13));
            let ControlServiceError::Transport(actual) = map_error(error) else {
                panic!("typed transport lost")
            };
            assert_eq!(actual.failure(None, None), expected);
        }
    }

    use usque_ipc::{
        agent_v1::{AgentRequest, AgentResponse, agent_request, agent_response},
        decode_frame, encode_frame, unix_fd,
    };

    enum AgentReply {
        Payload(Box<agent_response::Payload>),
        Rejected,
        Lost,
        Disconnected,
        Mismatched,
    }

    fn agent_script(
        script: Vec<(agent_request::Payload, AgentReply)>,
    ) -> (LinuxAgentClient, std::thread::JoinHandle<()>) {
        let connection_count = 1 + script
            .iter()
            .filter(|(_, reply)| {
                matches!(
                    reply,
                    AgentReply::Lost | AgentReply::Disconnected | AgentReply::Mismatched
                )
            })
            .count();
        let (client, servers) = LinuxAgentClient::fake_connections(connection_count);
        let worker = std::thread::spawn(move || {
            let mut servers = servers.into_iter();
            let mut server = servers.next().unwrap();
            for (expected, reply) in script {
                let peer = unix_fd::peer_credentials(&server).unwrap();
                let frame = unix_fd::receive_frame(&server, peer).unwrap();
                let request: AgentRequest = decode_frame(frame.bytes.into()).unwrap();
                assert_eq!(request.payload, Some(expected));
                assert!(frame.descriptor.is_none());
                if matches!(reply, AgentReply::Lost | AgentReply::Mismatched) {
                    let response = AgentResponse {
                        request_id: if matches!(reply, AgentReply::Mismatched) {
                            "wrong-request".into()
                        } else {
                            request.request_id
                        },
                        payload: Some(agent_response::Payload::Empty(agent_v1::Empty {})),
                        error: None,
                    };
                    let frame = encode_frame(&response).unwrap();
                    if matches!(reply, AgentReply::Mismatched) {
                        unix_fd::send_frame(&server, &frame, None::<&OwnedFd>).unwrap();
                    }
                    // A failed exchange must close this stream before another
                    // request; a delayed acknowledgement cannot enter a new one.
                    assert!(unix_fd::receive_frame(&server, peer).is_err());
                    if matches!(reply, AgentReply::Lost) {
                        assert!(unix_fd::send_frame(&server, &frame, None::<&OwnedFd>).is_err());
                    }
                    server = servers.next().unwrap();
                    continue;
                }
                if matches!(reply, AgentReply::Disconnected) {
                    server = servers.next().unwrap();
                    continue;
                }
                let (payload, error) = match reply {
                    AgentReply::Payload(payload) => (Some(*payload), None),
                    AgentReply::Rejected => (
                        None,
                        Some(agent_v1::AgentError {
                            linux_detail: None,
                            code: "linux_permission_denied".into(),
                            message: "generation changed".into(),
                            retryable: false,
                        }),
                    ),
                    AgentReply::Lost | AgentReply::Disconnected | AgentReply::Mismatched => {
                        unreachable!()
                    }
                };
                let response = AgentResponse {
                    request_id: request.request_id,
                    payload,
                    error,
                };
                unix_fd::send_frame(&server, &encode_frame(&response).unwrap(), None::<&OwnedFd>)
                    .unwrap();
            }
            let peer = unix_fd::peer_credentials(&server).unwrap();
            assert!(unix_fd::receive_frame(&server, peer).is_err());
        });
        (client, worker)
    }

    fn state_reply(state: agent_v1::AgentState) -> (agent_request::Payload, AgentReply) {
        (
            agent_request::Payload::GetState(agent_v1::GetStateRequest {}),
            AgentReply::Payload(Box::new(agent_response::Payload::State(state))),
        )
    }

    fn rollback_request(operation: Uuid) -> agent_request::Payload {
        agent_request::Payload::RollbackTunnel(agent_v1::RollbackTunnelRequest {
            operation_id: operation.to_string(),
            ..Default::default()
        })
    }

    fn replace_request(source: Uuid, target: Uuid, generation: u64) -> agent_request::Payload {
        agent_request::Payload::ReplaceTunnel(agent_v1::ReplaceTunnelRequest {
            source_operation_id: source.to_string(),
            operation_id: target.to_string(),
            expected_journal_generation: generation,
            plan: Some(Default::default()),
            ..Default::default()
        })
    }

    fn empty_reply() -> AgentReply {
        AgentReply::Payload(Box::new(agent_response::Payload::Empty(agent_v1::Empty {})))
    }

    #[tokio::test]
    async fn rejected_replacement_restores_original_operation_instead_of_absent_target() {
        let source = Uuid::new_v4();
        let target = Uuid::new_v4();
        for previous_replacement in [
            None,
            Some(Box::new(agent_v1::TunnelReplacementStatus {
                operation_id: source.to_string(),
                source_operation_id: Uuid::new_v4().to_string(),
                phase: agent_v1::TunnelReplacementPhase::Complete as i32,
                ..Default::default()
            })),
        ] {
            let (agent, worker) = agent_script(vec![
                (replace_request(source, target, 5), AgentReply::Rejected),
                state_reply(agent_v1::AgentState {
                    operation_id: source.to_string(),
                    phase: agent_v1::AgentPhase::Active as i32,
                    journal_generation: 6,
                    replacement: previous_replacement,
                    ..Default::default()
                }),
                (rollback_request(source), empty_reply()),
            ]);
            assert!(
                agent
                    .replace(source, target, Default::default(), 5)
                    .await
                    .is_err()
            );
            restore_agent_transaction(&agent, source, Some((source, target)))
                .await
                .unwrap();
            drop(agent);
            worker.join().unwrap();
        }
    }

    async fn accepted_replacement_with_failed_reply_is_recovered(
        operation_is_target: bool,
        reply: AgentReply,
    ) {
        let source = Uuid::new_v4();
        let target = Uuid::new_v4();
        let operation = if operation_is_target { target } else { source };
        let (agent, worker) = agent_script(vec![
            (replace_request(source, target, 5), reply),
            state_reply(agent_v1::AgentState {
                operation_id: operation.to_string(),
                phase: agent_v1::AgentPhase::RecoveryRequired as i32,
                journal_generation: 9,
                replacement: Some(Box::new(agent_v1::TunnelReplacementStatus {
                    source_operation_id: source.to_string(),
                    operation_id: target.to_string(),
                    ..Default::default()
                })),
                ..Default::default()
            }),
            (
                agent_request::Payload::AbortReplacement(agent_v1::AbortReplacementRequest {
                    operation_id: target.to_string(),
                    expected_journal_generation: 9,
                }),
                empty_reply(),
            ),
        ]);
        assert!(
            agent
                .replace(source, target, Default::default(), 5)
                .await
                .is_err()
        );
        restore_agent_transaction(&agent, source, Some((source, target)))
            .await
            .unwrap();
        drop(agent);
        worker.join().unwrap();
    }

    #[tokio::test]
    async fn lost_replacement_reply_during_source_cleanup_keeps_target_guard_owned() {
        accepted_replacement_with_failed_reply_is_recovered(false, AgentReply::Lost).await;
    }

    #[tokio::test]
    async fn lost_replacement_reply_after_target_prepare_keeps_target_guard_owned() {
        accepted_replacement_with_failed_reply_is_recovered(true, AgentReply::Lost).await;
    }

    #[tokio::test]
    async fn replacement_socket_eof_reconnects_without_replaying_the_mutation() {
        accepted_replacement_with_failed_reply_is_recovered(true, AgentReply::Disconnected).await;
    }

    #[tokio::test]
    async fn mismatched_replacement_reply_requires_a_fresh_connection() {
        accepted_replacement_with_failed_reply_is_recovered(false, AgentReply::Mismatched).await;
    }

    #[tokio::test]
    async fn completed_replacement_rolls_back_the_current_operation() {
        let source = Uuid::new_v4();
        let target = Uuid::new_v4();
        let (agent, worker) = agent_script(vec![
            (replace_request(source, target, 5), empty_reply()),
            state_reply(agent_v1::AgentState {
                operation_id: target.to_string(),
                phase: agent_v1::AgentPhase::Active as i32,
                replacement: Some(Box::new(agent_v1::TunnelReplacementStatus {
                    source_operation_id: source.to_string(),
                    operation_id: target.to_string(),
                    phase: agent_v1::TunnelReplacementPhase::Complete as i32,
                    ..Default::default()
                })),
                ..Default::default()
            }),
            (rollback_request(target), empty_reply()),
        ]);
        agent
            .replace(source, target, Default::default(), 5)
            .await
            .unwrap();
        restore_agent_transaction(&agent, target, None)
            .await
            .unwrap();
        drop(agent);
        worker.join().unwrap();
    }

    #[tokio::test]
    async fn normal_rollback_acknowledgement_loss_is_completed_by_clean_state_on_retry() {
        for reply in [
            AgentReply::Lost,
            AgentReply::Disconnected,
            AgentReply::Mismatched,
        ] {
            let operation = Uuid::new_v4();
            let (agent, worker) = agent_script(vec![
                state_reply(agent_v1::AgentState {
                    operation_id: operation.to_string(),
                    phase: agent_v1::AgentPhase::Active as i32,
                    ..Default::default()
                }),
                (rollback_request(operation), reply),
                state_reply(agent_v1::AgentState {
                    phase: agent_v1::AgentPhase::Clean as i32,
                    ..Default::default()
                }),
            ]);
            assert!(
                restore_agent_transaction(&agent, operation, None)
                    .await
                    .is_err()
            );
            restore_agent_transaction(&agent, operation, None)
                .await
                .unwrap();
            drop(agent);
            worker.join().unwrap();
        }
    }

    #[tokio::test]
    async fn recovery_state_query_reconnects_after_eof_and_keeps_operation_identity() {
        let operation = Uuid::new_v4();
        let (agent, worker) = agent_script(vec![
            (
                agent_request::Payload::GetState(agent_v1::GetStateRequest {}),
                AgentReply::Disconnected,
            ),
            state_reply(agent_v1::AgentState {
                operation_id: operation.to_string(),
                phase: agent_v1::AgentPhase::Active as i32,
                ..Default::default()
            }),
            (rollback_request(operation), empty_reply()),
        ]);
        restore_agent_transaction(&agent, operation, None)
            .await
            .unwrap();
        drop(agent);
        worker.join().unwrap();
    }

    #[tokio::test]
    async fn normal_recovery_never_mutates_another_owner_or_accepts_inconsistent_clean_state() {
        let operation = Uuid::new_v4();
        for state in [
            agent_v1::AgentState {
                phase: agent_v1::AgentPhase::Active as i32,
                operation_id: Uuid::new_v4().to_string(),
                ..Default::default()
            },
            agent_v1::AgentState {
                phase: agent_v1::AgentPhase::Clean as i32,
                kill_switch_active: true,
                ..Default::default()
            },
        ] {
            let (agent, worker) = agent_script(vec![state_reply(state)]);
            assert!(
                restore_agent_transaction(&agent, operation, None)
                    .await
                    .is_err()
            );
            drop(agent);
            worker.join().unwrap();
        }
    }

    #[tokio::test]
    async fn stale_abort_does_not_discard_replacement_and_retry_refreshes_generation() {
        let source = Uuid::new_v4();
        let target = Uuid::new_v4();
        let mut script = Vec::new();
        for (generation, reply) in [(9, AgentReply::Rejected), (10, empty_reply())] {
            script.push(state_reply(agent_v1::AgentState {
                operation_id: target.to_string(),
                phase: agent_v1::AgentPhase::Prepared as i32,
                journal_generation: generation,
                replacement: Some(Box::new(agent_v1::TunnelReplacementStatus {
                    source_operation_id: source.to_string(),
                    operation_id: target.to_string(),
                    ..Default::default()
                })),
                ..Default::default()
            }));
            script.push((
                agent_request::Payload::AbortReplacement(agent_v1::AbortReplacementRequest {
                    operation_id: target.to_string(),
                    expected_journal_generation: generation,
                }),
                reply,
            ));
        }
        let (agent, worker) = agent_script(script);
        let pending = Some((source, target));
        assert!(
            restore_agent_transaction(&agent, source, pending)
                .await
                .is_err()
        );
        restore_agent_transaction(&agent, source, pending)
            .await
            .unwrap();
        drop(agent);
        worker.join().unwrap();
    }

    #[tokio::test]
    async fn unknown_replacement_owner_is_not_mutated_and_clean_state_finishes_recovery() {
        let source = Uuid::new_v4();
        let target = Uuid::new_v4();
        for state in [
            agent_v1::AgentState {
                phase: agent_v1::AgentPhase::Active as i32,
                operation_id: Uuid::new_v4().to_string(),
                ..Default::default()
            },
            agent_v1::AgentState {
                phase: agent_v1::AgentPhase::Recovering as i32,
                operation_id: source.to_string(),
                replacement: Some(Box::new(agent_v1::TunnelReplacementStatus {
                    source_operation_id: source.to_string(),
                    operation_id: Uuid::new_v4().to_string(),
                    ..Default::default()
                })),
                ..Default::default()
            },
            agent_v1::AgentState {
                phase: agent_v1::AgentPhase::RecoveryRequired as i32,
                ..Default::default()
            },
            agent_v1::AgentState {
                phase: agent_v1::AgentPhase::Clean as i32,
                ..Default::default()
            },
        ] {
            let clean = state.phase == agent_v1::AgentPhase::Clean as i32;
            let (agent, worker) = agent_script(vec![state_reply(state)]);
            let result = restore_agent_transaction(&agent, source, Some((source, target))).await;
            assert_eq!(result.is_ok(), clean);
            drop(agent);
            worker.join().unwrap();
        }
    }

    #[tokio::test]
    async fn rejected_packet_registration_releases_the_owned_descriptor() {
        let temporary = tempfile::NamedTempFile::new().unwrap();
        let descriptor = temporary.reopen().unwrap();
        let observer = temporary.reopen().unwrap();
        descriptor.try_lock().unwrap();
        assert!(observer.try_lock().is_err());

        // epoll rejects regular files. The independent open's lock becoming
        // available proves error conversion closed the registered OwnedFd,
        // without relying on a raw descriptor number that another test can reuse.
        let error = register_packet_descriptor(OwnedFd::from(descriptor))
            .err()
            .unwrap();
        assert_eq!(error.raw_os_error(), Some(libc::EPERM));
        observer.try_lock().unwrap();
    }

    #[tokio::test]
    async fn cancelling_a_pending_packet_read_releases_the_owned_descriptor() {
        use std::io::Read as _;

        let (device, mut observer) = std::os::unix::net::UnixStream::pair().unwrap();
        device.set_nonblocking(true).unwrap();
        observer.set_nonblocking(true).unwrap();
        let device = register_packet_descriptor(OwnedFd::from(device)).unwrap();
        let mut buffer = [0u8; 1280];
        let mut pending = Box::pin(read_packet(&device, &mut buffer));
        std::future::poll_fn(|context| {
            assert!(std::future::Future::poll(pending.as_mut(), context).is_pending());
            std::task::Poll::Ready(())
        })
        .await;
        drop(pending);
        drop(device);

        // A socketpair stands in for packet I/O; EOF proves cancellation can
        // deregister and close the owned endpoint without a TUN or host changes.
        assert_eq!(observer.read(&mut [0u8; 1]).unwrap(), 0);
    }

    #[tokio::test]
    async fn packet_io_is_bounded_and_preserves_datagram_boundaries() {
        let (device, observer) = std::os::unix::net::UnixDatagram::pair().unwrap();
        device.set_nonblocking(true).unwrap();
        observer
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let device = register_packet_descriptor(OwnedFd::from(device)).unwrap();
        observer.send(&[0x45, 0, 0, 4]).unwrap();
        let mut buffer = [0u8; 1280];
        let size = tokio::time::timeout(Duration::from_secs(3), read_packet(&device, &mut buffer))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(&buffer[..size], &[0x45, 0, 0, 4]);
        write_packet(&device, &[0x60, 1, 2, 3]).await.unwrap();
        assert_eq!(observer.recv(&mut buffer).unwrap(), 4);
        assert_eq!(&buffer[..4], &[0x60, 1, 2, 3]);
    }
    #[test]
    fn final_chain_assignment_never_advertises_an_absent_family() {
        let mut plan = agent_v1::TunnelPlan {
            split_dns: true,
            defer_network_configuration: true,
            ..Default::default()
        };
        apply_final_network(
            &mut plan,
            &usque_core::vpngate::FinalNetworkParameters {
                ipv4: Some("10.0.0.1".parse().unwrap()),
                ipv6: None,
                dns_servers: vec![],
                mtu: 1280,
            },
        );
        assert_eq!(plan.assigned_ipv4, "10.0.0.1/32");
        assert!(plan.assigned_ipv6.is_empty());
        assert_eq!(
            plan.dns_servers,
            vec![usque_transport::SPLIT_DNS_IPV4.to_string()]
        );
        assert!(!plan.defer_network_configuration);
    }
    #[test]
    fn missing_privileged_capabilities_fail_closed() {
        assert!(validate_capabilities(&Default::default(), &Profile::default()).is_err());
    }
}
