//! Ordinary-user data plane with explicit root Agent network ownership.
use crate::{
    desktop_tunnel_plan,
    macos_agent::{self, MacAgentClient},
    macos_socket::MacSocketProtector,
    macos_system_proxy::{Context, MacSystemProxyGuard},
};
use std::{io, sync::Arc, time::Duration};
use tokio::sync::watch;
use tokio_util::{sync::CancellationToken, task::AbortOnDropHandle};
use usque_core::Profile;
use usque_ipc::agent_v1::{self as p, macos_agent_request::Operation};
use usque_transport::{
    DataPlaneRuntime, EndpointPinRefresher, GeoDirectPolicy, MasqueTlsIdentity, SocketProtector,
};

pub(crate) fn map_error(error: io::Error) -> crate::ControlServiceError {
    log_tun_failure("runtime", &error);
    // Shared transport failures carry the retry policy for continuous
    // connections. Keep that policy across the platform I/O boundary.
    let message = error.to_string();
    match error
        .into_inner()
        .and_then(|cause| cause.downcast::<usque_transport::TransportError>().ok())
    {
        Some(transport) => crate::ControlServiceError::Transport(*transport),
        None => crate::ControlServiceError::PlatformVpn(message),
    }
}
pub(super) fn log_tun_failure(stage: &'static str, error: &io::Error) {
    let cause = error.get_ref();
    let agent_failure = cause
        .and_then(|e| e.downcast_ref::<macos_agent::AgentFailure>())
        .map(|e| e.0.as_str_name());
    let transport_failure = cause
        .and_then(|e| e.downcast_ref::<usque_transport::TransportError>())
        .map(|e| e.failure(None, None).code.as_str());
    tracing::warn!(stage, kind = ?error.kind(), os_error = error.raw_os_error(), agent_failure, transport_failure, "macOS TUN operation failed");
}
pub(crate) struct MacAgentRuntime {
    tunnel: DataPlaneRuntime,
    identity: MasqueTlsIdentity,
    refresher: Arc<dyn EndpointPinRefresher>,
    agent: MacAgentClient,
    fence: p::MacosFence,
    cancel: CancellationToken,
    packets: Option<AbortOnDropHandle<()>>,
    monitor: Option<AbortOnDropHandle<()>>,
    failure: watch::Receiver<Option<String>>,
    retained: bool,
    proxy: Option<MacSystemProxyGuard>,
    proxy_context: Context,
}
impl std::ops::Deref for MacAgentRuntime {
    type Target = DataPlaneRuntime;
    fn deref(&self) -> &Self::Target {
        &self.tunnel
    }
}
impl std::ops::DerefMut for MacAgentRuntime {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.tunnel
    }
}
struct StartupOwner(MacAgentClient, bool);
impl Drop for StartupOwner {
    fn drop(&mut self) {
        if self.1
            && let Ok(runtime) = tokio::runtime::Handle::try_current()
        {
            let agent = self.0.clone();
            runtime.spawn(async move {
                agent.abandon().await;
            });
        }
    }
}
impl MacAgentRuntime {
    pub async fn start(
        profile: &Profile,
        identity: MasqueTlsIdentity,
        refresher: Arc<dyn EndpointPinRefresher>,
        policy: Arc<GeoDirectPolicy>,
        mut gate: usque_transport::VpnGateStart,
        proxy_context: Context,
    ) -> io::Result<Self> {
        let agent = MacAgentClient::connect().await?;
        let status = agent.status().await?;
        let resume = macos_agent::resumable(&status);
        if !macos_agent::capability(&status) && !resume {
            return Err(io::Error::other(
                "macOS TUN acceptance or service preflight incomplete",
            ));
        }
        let physical = status
            .physical_network
            .ok_or_else(|| io::Error::other("physical network unavailable"))?;
        let control = if resume {
            status
                .control_api_candidates
                .iter()
                .map(|s| s.parse())
                .collect::<Result<Vec<_>, _>>()
                .map_err(io::Error::other)?
        } else {
            tokio::time::timeout(
                Duration::from_secs(5),
                tokio::task::spawn_blocking(|| {
                    usque_transport::NoopSocketProtector.resolve(
                        usque_core::REGISTRATION_API_HOST,
                        usque_core::REGISTRATION_API_PORT,
                    )
                }),
            )
            .await
            .map_err(io::Error::other)?
            .map_err(io::Error::other)?
            .map_err(io::Error::other)?
        };
        let mut plan = desktop_tunnel_plan::tunnel_plan(profile, &identity, &control, true);
        plan.defer_network_configuration = true;
        let fence = if resume {
            let previous = status
                .fence
                .as_ref()
                .ok_or_else(|| io::Error::other("missing retained fence"))?;
            p::MacosFence {
                session_id: previous.session_id.clone(),
                configuration: previous
                    .configuration
                    .checked_add(1)
                    .ok_or_else(|| io::Error::other("configuration exhausted"))?,
                network: physical.generation,
            }
        } else {
            p::MacosFence {
                session_id: uuid::Uuid::new_v4().to_string(),
                configuration: 1,
                network: physical.generation,
            }
        };
        let mut owner = StartupOwner(agent.clone(), true);
        let configure = p::MacosConfigure {
            fence: Some(fence.clone()),
            plan: Some(plan.clone()),
        };
        agent
            .empty(if resume {
                Operation::Resume(p::MacosResume {
                    previous: status.fence,
                    next: Some(configure),
                })
            } else {
                Operation::Prepare(configure)
            })
            .await?;
        tracing::info!(
            resumed = resume,
            "macOS TUN Agent prepared the physical egress"
        );
        let protector = MacSocketProtector::new(agent.clone(), fence.clone(), &physical, control)?;
        let cancel = gate.cancellation.child_token();
        gate.cancellation = cancel.clone();
        let (failure_tx, failure) = watch::channel(None);
        let monitor = {
            let (agent, protector, cancel, failure) = (
                agent.clone(),
                protector.clone(),
                cancel.clone(),
                failure_tx.clone(),
            );
            AbortOnDropHandle::new(tokio::spawn(async move {
                let mut tick = tokio::time::interval(Duration::from_secs(1));
                loop {
                    tokio::select! {
                        biased;
                        _ = cancel.cancelled() => {
                            if failure.borrow().is_some() {
                                protector.invalidate();
                                agent.abandon().await;
                            }
                            break;
                        },
                        _ = tick.tick() => {}
                    }
                    match agent.status().await {
                        Ok(status) if protector.observe(&status) => continue,
                        Ok(status) => tracing::warn!(
                            agent_phase = status.phase,
                            agent_failure = status.failure,
                            recovery_pending = status.recovery_pending,
                            "macOS TUN lost its Agent session or network generation"
                        ),
                        Err(error) => log_tun_failure("agent_status", &error),
                    }
                    failure.send_replace(Some(
                        "macOS Agent or physical network changed; recovery required".into(),
                    ));
                    cancel.cancel();
                    protector.invalidate();
                    agent.abandon().await;
                    break;
                }
            }))
        };
        let mut tunnel = Box::pin(DataPlaneRuntime::start_with_vpngate(
            profile,
            identity.clone(),
            protector,
            Some(refresher.clone()),
            policy,
            gate,
        ))
        .await
        .map_err(io::Error::other)?;
        tracing::info!("macOS TUN remote data plane prepared");
        apply_final_network(&mut plan, &tunnel.network_parameters());
        let ready = async {
            agent
                .empty(Operation::Configure(p::MacosConfigure {
                    fence: Some(fence.clone()),
                    plan: Some(plan),
                }))
                .await?;
            tracing::info!("macOS TUN Agent applied the interface, DNS and routes");
            let (descriptor, fd) = agent.descriptor(Operation::OpenTun(fence.clone())).await?;
            macos_agent::verify_utun(&fd, &descriptor)?;
            let packets = crate::macos_tun_io::spawn(
                tunnel.attach_tun().map_err(io::Error::other)?,
                fd,
                cancel.clone(),
                failure_tx,
            )
            .await?;
            agent.empty(Operation::Commit(fence.clone())).await?;
            tunnel.activate_final().await.map_err(io::Error::other)?;
            Ok::<_, io::Error>(packets)
        }
        .await;
        let packets = match ready {
            Ok(packets) => packets,
            Err(error) => {
                cancel.cancel();
                tunnel.shutdown().await;
                return Err(error);
            }
        };
        let mut runtime = Self {
            tunnel,
            identity,
            refresher,
            agent,
            fence,
            cancel,
            packets: Some(packets),
            monitor: Some(monitor),
            failure,
            retained: false,
            proxy: None,
            proxy_context,
        };
        runtime.replace_system_proxy(profile).await?;
        owner.1 = false;
        tracing::info!("macOS TUN data plane active");
        Ok(runtime)
    }
    pub fn target_committed(&self) -> bool {
        // This runtime is only constructed after Commit. A later packet
        // failure does not authorize removing its retained protection.
        true
    }
    pub fn requires_agent_reattach(&self) -> bool {
        false
    }
    pub fn replacement_pending(&self) -> bool {
        false
    }
    pub fn failure_retained(&self) -> bool {
        self.retained
    }
    pub fn gate_status(&self) -> usque_core::vpngate::GateStatus {
        let mut status = self.tunnel.gate_status();
        if self.failure.borrow().is_some() {
            status.stage = usque_core::vpngate::GateStage::Error;
            status.failure = Some(usque_core::vpngate::GateFailure::Transport);
            status.network = None;
        }
        status
    }
    pub fn needs_warp_bootstrap(&self) -> bool {
        false
    }
    pub fn internal_networks(
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
    pub fn diagnostic_dns_context(&self) -> Option<(Arc<dyn SocketProtector>, CancellationToken)> {
        Some(self.tunnel.diagnostic_dns_context())
    }
    pub fn failure(&self) -> Option<String> {
        self.failure
            .borrow()
            .clone()
            .or_else(|| self.proxy.as_ref().and_then(|proxy| proxy.failure()))
            .or_else(|| self.tunnel.failure())
    }
    pub fn update_traffic_policy(&mut self, disabled: bool) -> io::Result<()> {
        self.tunnel.update_traffic_policy(disabled);
        Ok(())
    }
    pub fn prepare_routing_update(
        &self,
        profile: &Profile,
    ) -> io::Result<usque_transport::PreparedTrafficPolicyUpdate> {
        self.tunnel
            .prepare_routing_update(profile)
            .map_err(io::Error::other)
    }
    pub fn publish_routing_update(
        &mut self,
        update: usque_transport::PreparedTrafficPolicyUpdate,
    ) -> io::Result<()> {
        self.tunnel
            .publish_routing_update(update)
            .map_err(io::Error::other)
    }
    pub async fn reconfigure_frontends(&mut self, profile: &Profile) -> io::Result<()> {
        self.tunnel
            .reconfigure_frontends(profile)
            .await
            .map_err(io::Error::other)
    }
    pub async fn replace_system_proxy(&mut self, profile: &Profile) -> io::Result<()> {
        if let Some(proxy) = &mut self.proxy {
            proxy.shutdown(false).await.map_err(io::Error::other)?;
        }
        self.proxy = None;
        if profile.frontends.http && profile.proxy.system_proxy {
            let listener = self
                .tunnel
                .http_listeners()
                .iter()
                .copied()
                .find(|a| a.ip().is_loopback())
                .ok_or_else(|| io::Error::other("system proxy requires loopback HTTP"))?;
            self.proxy = Some(
                self.proxy_context
                    .start(listener)
                    .await
                    .map_err(io::Error::other)?,
            );
        }
        Ok(())
    }
    pub fn system_proxy_active(&self) -> bool {
        self.proxy.as_ref().is_some_and(|proxy| proxy.active())
    }
    /// Mark intentional teardown before shared frontends can close TunPacketIo.
    /// A cancelled pump then exits normally instead of abandoning ownership
    /// ahead of the explicit Suspend/Disconnect transaction.
    pub fn quiesce_final(&mut self) {
        self.cancel.cancel();
        self.tunnel.quiesce_final();
    }
    pub fn cancel_immediately(&mut self) {
        self.cancel.cancel();
        self.tunnel.cancel_immediately();
    }
    async fn stop_packets(&mut self) {
        self.cancel.cancel();
        if let Some(task) = self.packets.take() {
            let _ = task.await;
        }
        self.monitor.take();
    }
    pub async fn replace_gate(
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
        self.replace_connection(
            profile,
            self.identity.clone(),
            self.refresher.clone(),
            policy,
            usque_transport::VpnGateStart {
                selected,
                status: Some(status),
                cancellation: cancellation.clone(),
                ..Default::default()
            },
        )
        .await
    }
    pub async fn replace_connection(
        &mut self,
        profile: &Profile,
        identity: MasqueTlsIdentity,
        refresher: Arc<dyn EndpointPinRefresher>,
        policy: Arc<GeoDirectPolicy>,
        gate: usque_transport::VpnGateStart,
    ) -> io::Result<()> {
        self.quiesce_final();
        self.stop_packets().await;
        self.tunnel.shutdown().await;
        if let Some(proxy) = &mut self.proxy {
            proxy.shutdown(false).await.map_err(io::Error::other)?;
        }
        self.proxy = None;
        if !self.failure_retained() && self.failure.borrow().is_none() {
            self.agent
                .empty(Operation::Suspend(self.fence.clone()))
                .await?;
        }
        self.retained = true;
        self.agent.abandon().await;
        let replacement = Box::pin(Self::start(
            profile,
            identity,
            refresher,
            policy,
            gate,
            self.proxy_context.clone(),
        ))
        .await?;
        *self = replacement;
        Ok(())
    }
    pub async fn retain_failed_gate(
        &mut self,
        reason: usque_core::vpngate::GateFailure,
    ) -> io::Result<()> {
        self.retained = true;
        self.cancel.cancel();
        self.tunnel.fail_gate(reason).await;
        self.stop_packets().await;
        self.agent.abandon().await;
        macos_agent::confirm_retained_protection(&self.fence).await
    }
    pub async fn detach(&mut self) {
        self.retained = true;
        self.quiesce_final();
        self.stop_packets().await;
        self.tunnel.shutdown().await;
        self.agent.abandon().await;
    }
    pub async fn shutdown(&mut self) -> io::Result<()> {
        self.quiesce_final();
        self.stop_packets().await;
        self.tunnel.shutdown().await;
        if let Some(proxy) = &mut self.proxy {
            proxy.shutdown(false).await.map_err(io::Error::other)?;
        }
        self.proxy = None;
        if self
            .agent
            .empty(Operation::Disconnect(self.fence.clone()))
            .await
            .is_ok()
        {
            return Ok(());
        }
        macos_agent::disconnect_retained(self.fence.clone()).await
    }
}
impl Drop for MacAgentRuntime {
    fn drop(&mut self) {
        self.cancel.cancel();
        self.tunnel.cancel_immediately();
        let agent = self.agent.clone();
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            runtime.spawn(async move {
                agent.abandon().await;
            });
        }
    }
}
fn apply_final_network(
    plan: &mut p::TunnelPlan,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closed_endpoint_is_retryable_but_failed_agent_recovery_stops() {
        let refusal = usque_transport::TransportError::Io(io::ErrorKind::ConnectionRefused.into());
        let mapped = map_error(io::Error::other(refusal));
        assert!(crate::continuous_connection::retry_network_error(&mapped));
        assert!(matches!(mapped, crate::ControlServiceError::Transport(_)));

        let recovery = map_error(io::Error::other(macos_agent::AgentFailure(
            p::MacosFailure::RecoveryIncomplete,
        )));
        assert!(!crate::continuous_connection::retry_network_error(
            &recovery
        ));
    }

    #[test]
    fn macos_tun_diagnostics_record_codes_without_private_error_text() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("config.json");
        let writer = crate::logging::LogWriterFactory::open(&config).unwrap();
        let subscriber = tracing_subscriber::fmt()
            .json()
            .with_writer(writer.clone())
            .finish();
        tracing::subscriber::with_default(subscriber, || {
            map_error(io::Error::other(macos_agent::AgentFailure(
                p::MacosFailure::RecoveryIncomplete,
            )));
            map_error(io::Error::other("private-test-endpoint.example"));
        });
        writer.flush(Duration::from_secs(5)).unwrap();
        let log =
            std::fs::read_to_string(crate::logging::log_directory(&config).join("engine.jsonl"))
                .unwrap_or_default();
        assert!(log.contains("MACOS_FAILURE_RECOVERY_INCOMPLETE"));
        assert!(!log.contains("private-test-endpoint.example"));
    }
}
