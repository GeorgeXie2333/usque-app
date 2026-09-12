//! Backend-neutral runtime and TUN I/O. Platform lifecycle code owns this
//! boundary without learning CONNECT-IP or L4 implementation details.
use crate::geo_direct::GeoDirectPolicy;
use crate::h2::{MasqueTlsIdentity, TransportError};
use crate::l4::{L4Runtime, L4TunIo};
use crate::masque_runtime::{MasqueRuntime, MasqueTunIo};
use crate::netstack::{
    ManagedTunnelMonitor, ProxyPerformanceSnapshot, RuntimeHealth, RuntimePath, TrafficSnapshot,
};
use crate::pin_refresh::EndpointPinRefresher;
use crate::socket::SocketProtector;
use crate::{ConnectionTimelineSnapshot, NetworkQualitySnapshot};
use bytes::Bytes;
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::Arc;
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;
use usque_core::vpngate::{
    FinalNetworkParameters, GateFailure, GateStatus, PreparedProfile, ServerSummary,
};
use usque_core::{DataPlaneMode, L4Snapshot, Profile};

pub struct DataPlaneRuntime {
    inner: RuntimeInner,
    gate: Option<Box<GateRuntime>>,
    warp_network: FinalNetworkParameters,
    final_blocked: bool,
    transition_status: GateStatus,
    pending_frontends: Option<Profile>,
}
#[derive(Default)]
pub struct VpnGateStart {
    pub selected: Option<(ServerSummary, PreparedProfile)>,
    pub status: Option<watch::Sender<GateStatus>>,
    /// Cancels startup only; the established OpenVPN worker owns its lifetime.
    pub cancellation: CancellationToken,
}
struct GateRuntime {
    frontend: MasqueRuntime,
    driver: crate::vpngate::GateDriver,
    network: FinalNetworkParameters,
    server: ServerSummary,
}
enum RuntimeInner {
    ConnectIp(Box<MasqueRuntime>),
    L4(Box<L4Runtime>),
}

impl DataPlaneRuntime {
    pub async fn start_with_geo_policy(
        profile: &Profile,
        identity: MasqueTlsIdentity,
        protector: Arc<dyn SocketProtector>,
        refresher: Option<Arc<dyn EndpointPinRefresher>>,
        policy: Arc<GeoDirectPolicy>,
    ) -> Result<Self, TransportError> {
        if profile.vpn_gate.enabled {
            // Every caller must supply a validated pinned profile explicitly.
            return Err(TransportError::VpnGate(GateFailure::Configuration));
        }
        let warp_network = FinalNetworkParameters {
            ipv4: Some(identity.assigned_ipv4),
            ipv6: Some(identity.assigned_ipv6),
            mtu: profile.mtu,
            dns_servers: profile.dns_servers.clone(),
        };
        let inner = match profile.data_plane {
            DataPlaneMode::ConnectIp => RuntimeInner::ConnectIp(Box::new(
                MasqueRuntime::start_with_geo_policy(
                    profile, identity, protector, refresher, policy,
                )
                .await?,
            )),
            DataPlaneMode::L4Proxy => RuntimeInner::L4(Box::new(
                L4Runtime::start(profile, identity, protector, refresher, policy).await?,
            )),
        };
        Ok(Self {
            inner,
            gate: None,
            warp_network,
            final_blocked: false,
            transition_status: GateStatus::default(),
            pending_frontends: None,
        })
    }

    pub async fn start_with_vpngate(
        profile: &Profile,
        identity: MasqueTlsIdentity,
        protector: Arc<dyn SocketProtector>,
        refresher: Option<Arc<dyn EndpointPinRefresher>>,
        policy: Arc<GeoDirectPolicy>,
        gate: VpnGateStart,
    ) -> Result<Self, TransportError> {
        let VpnGateStart {
            selected,
            status,
            cancellation,
        } = gate;
        if !profile.vpn_gate.enabled {
            return Self::start_with_geo_policy(profile, identity, protector, refresher, policy)
                .await;
        }
        let selected = selected.ok_or(TransportError::VpnGate(GateFailure::Configuration))?;
        if let Some(status) = &status {
            status.send_replace(GateStatus {
                stage: usque_core::vpngate::GateStage::ConnectingWarp,
                current_server: Some(selected.0.clone()),
                ..Default::default()
            });
        }
        let mut underlay_profile = profile.clone();
        underlay_profile.vpn_gate.enabled = false;
        underlay_profile.frontends.socks5 = false;
        underlay_profile.frontends.http = false;
        underlay_profile.proxy.system_proxy = false;
        underlay_profile.canonicalize_mode();
        let mut runtime = Self::start_with_geo_policy(
            &underlay_profile,
            identity,
            protector.clone(),
            refresher,
            policy.clone(),
        )
        .await?;
        runtime.quiesce_final();
        runtime.transition_status.current_server = Some(selected.0.clone());
        let status_copy = status.clone();
        if let Err(error) = runtime
            .install_gate(
                profile,
                protector,
                policy,
                VpnGateStart {
                    selected: Some(selected),
                    status,
                    cancellation,
                },
            )
            .await
        {
            let reason = match error {
                TransportError::VpnGate(reason) => reason,
                _ => GateFailure::Transport,
            };
            if let Some(status) = &status_copy {
                runtime.transition_status = status.borrow().clone();
            }
            runtime.fail_gate(reason).await;
            if let Some(status) = status_copy {
                status.send_replace(runtime.transition_status.clone());
            }
        }
        Ok(runtime)
    }
    pub fn headless_profile(profile: &Profile) -> Profile {
        let mut headless = profile.clone();
        headless.vpn_gate.enabled = false;
        headless.frontends = usque_core::FrontendSettings {
            tunnel: false,
            socks5: false,
            http: false,
        };
        headless.proxy.system_proxy = false;
        headless.geo_direct_countries.clear();
        headless.split_exclusions.clear();
        headless.canonicalize_mode();
        headless
    }
    async fn install_gate(
        &mut self,
        profile: &Profile,
        protector: Arc<dyn SocketProtector>,
        policy: Arc<GeoDirectPolicy>,
        gate: VpnGateStart,
    ) -> Result<(), TransportError> {
        let VpnGateStart {
            selected,
            status,
            cancellation,
        } = gate;
        let selected = selected.ok_or(TransportError::VpnGate(GateFailure::Configuration))?;
        let (server, prepared) = selected;
        if profile.vpn_gate.selection.as_ref().is_none_or(|selection| {
            selection.server_id != server.id || selection.config_sha256 != server.config_sha256
        }) {
            return Err(TransportError::VpnGate(GateFailure::Configuration));
        }
        let (mut driver, tunnel, mut network) = crate::vpngate::GateDriver::start(
            &prepared,
            self.warp_internal_network(),
            self.underlay_monitor().network_quality_telemetry(),
            status,
            &cancellation,
        )
        .await?;
        network.mtu = profile.mtu.min(network.mtu);
        if network.dns_servers.is_empty() {
            network.dns_servers = profile
                .dns_servers
                .iter()
                .copied()
                .filter(|ip| network.supports(*ip))
                .collect();
        }
        if network.dns_servers.is_empty() {
            driver.shutdown().await;
            return Err(TransportError::VpnGate(GateFailure::Configuration));
        }
        let effective = final_profile(profile, &network);
        let mut frontend = match MasqueRuntime::start_over_tunnel(
            &effective,
            tunnel,
            (
                network.ipv4.unwrap_or(Ipv4Addr::UNSPECIFIED),
                network.ipv6.unwrap_or(Ipv6Addr::UNSPECIFIED),
            ),
            protector,
            policy,
        )
        .await
        {
            Ok(frontend) => frontend,
            Err(error) => {
                driver.shutdown().await;
                return Err(error);
            }
        };
        if cancellation.is_cancelled() {
            frontend.shutdown().await;
            driver.shutdown().await;
            return Err(TransportError::TunnelClosed);
        }
        self.gate = Some(Box::new(GateRuntime {
            frontend,
            driver,
            network,
            server,
        }));
        self.final_blocked = false;
        Ok(())
    }
    /// Close all old final flows before dialing the explicitly selected node.
    /// On failure the closed Gate slot remains installed, so no accessor can
    /// fall through to the headless WARP backend.
    pub async fn replace_gate(
        &mut self,
        profile: &Profile,
        selected: Option<(ServerSummary, PreparedProfile)>,
        policy: Arc<GeoDirectPolicy>,
        status: watch::Sender<GateStatus>,
        cancellation: &CancellationToken,
    ) -> Result<(), TransportError> {
        self.quiesce_final();
        let protector = match &self.inner {
            RuntimeInner::ConnectIp(runtime) => runtime.diagnostic_dns_context().0,
            RuntimeInner::L4(runtime) => runtime.diagnostic_dns_context().0,
        };
        if let Some(gate) = &mut self.gate {
            gate.frontend.shutdown().await;
            gate.driver.shutdown().await;
        }
        match &mut self.inner {
            RuntimeInner::ConnectIp(runtime) => runtime.suspend_frontends().await,
            RuntimeInner::L4(runtime) => runtime.suspend_frontends().await,
        }
        self.transition_status = GateStatus {
            stage: usque_core::vpngate::GateStage::ConnectingServer,
            current_server: selected.as_ref().map(|(server, _)| server.clone()),
            ..Default::default()
        };
        status.send_replace(self.transition_status.clone());
        let result = if profile.vpn_gate.enabled {
            match selected {
                Some(selected) => {
                    Box::pin(self.install_gate(
                        profile,
                        protector,
                        policy,
                        VpnGateStart {
                            selected: Some(selected),
                            status: Some(status.clone()),
                            cancellation: cancellation.clone(),
                        },
                    ))
                    .await
                }
                None => Err(TransportError::VpnGate(GateFailure::Configuration)),
            }
        } else {
            self.gate = None;
            self.pending_frontends = Some(profile.clone());
            match &mut self.inner {
                RuntimeInner::L4(runtime) => runtime.prepare_tun(profile).await,
                RuntimeInner::ConnectIp(_) => Ok(()),
            }
        };
        if let Err(error) = &result {
            status.send_modify(|s| {
                s.stage = usque_core::vpngate::GateStage::Error;
                s.network = None;
                s.failure = Some(match error {
                    TransportError::VpnGate(reason) => *reason,
                    _ => GateFailure::Transport,
                });
            });
            self.transition_status = status.borrow().clone();
        }
        result
    }
    /// Revokes all final flows synchronously without cancelling the WARP session.
    /// The closed Gate slot deliberately remains installed.
    pub fn quiesce_final(&mut self) {
        self.final_blocked = true;
        self.pending_frontends = None;
        if let Some(gate) = &mut self.gate {
            gate.driver.cancel();
            gate.frontend.cancel_immediately();
        } else {
            match &mut self.inner {
                RuntimeInner::ConnectIp(runtime) => runtime.quiesce_frontends(),
                RuntimeInner::L4(runtime) => runtime.quiesce_frontends(),
            }
        }
    }
    pub async fn fail_gate(&mut self, reason: GateFailure) {
        self.quiesce_final();
        self.transition_status.stage = usque_core::vpngate::GateStage::Error;
        self.transition_status.failure = Some(reason);
        self.transition_status.network = None;
        if let Some(gate) = &mut self.gate {
            gate.frontend.shutdown().await;
            gate.driver.shutdown().await;
            gate.driver.fail(reason);
        }
    }
    /// Called after platform address/DNS/route application and packet attach.
    /// Until then local proxy requests and outbound final packets are blocked.
    pub async fn activate_final(&mut self) -> Result<(), TransportError> {
        if self.final_blocked && self.pending_frontends.is_none() {
            return Err(TransportError::VpnGate(
                self.transition_status
                    .failure
                    .unwrap_or(GateFailure::Transport),
            ));
        }
        if let Some(profile) = self.pending_frontends.take() {
            let result = match &mut self.inner {
                RuntimeInner::ConnectIp(runtime) => runtime.reconfigure_frontends(&profile).await,
                RuntimeInner::L4(runtime) => runtime.reconfigure_frontends(&profile).await,
            };
            if let Err(error) = result {
                self.fail_gate(GateFailure::Transport).await;
                return Err(error);
            }
            self.final_blocked = false;
            self.transition_status = GateStatus::default();
        }
        if let Some(gate) = &self.gate {
            let mut status = gate.driver.status.clone();
            gate.driver.admit();
            loop {
                match status.borrow().stage {
                    usque_core::vpngate::GateStage::Connected => return Ok(()),
                    usque_core::vpngate::GateStage::Error => {
                        return Err(TransportError::VpnGate(
                            status.borrow().failure.unwrap_or(GateFailure::Transport),
                        ));
                    }
                    _ => {}
                }
                status
                    .changed()
                    .await
                    .map_err(|_| TransportError::VpnGate(GateFailure::Transport))?;
            }
        }
        Ok(())
    }
    pub fn network_parameters(&self) -> FinalNetworkParameters {
        self.gate
            .as_ref()
            .map_or_else(|| self.warp_network.clone(), |gate| gate.network.clone())
    }
    pub fn gate_status(&self) -> GateStatus {
        let mut status = if self.final_blocked {
            self.transition_status.clone()
        } else {
            self.gate.as_ref().map_or_else(GateStatus::default, |gate| {
                let mut status = gate.driver.status.borrow().clone();
                status.current_server = Some(gate.server.clone());
                if matches!(
                    status.stage,
                    usque_core::vpngate::GateStage::Connected
                        | usque_core::vpngate::GateStage::ConfiguringNetwork
                ) {
                    status.network = Some(gate.network.clone());
                }
                status
            })
        };
        if status.stage != usque_core::vpngate::GateStage::Disabled {
            status.warp_stage = Some(
                match self.underlay_monitor().health() {
                    RuntimeHealth::Connected { .. } => "connected",
                    RuntimeHealth::Reconnecting { .. } => "reconnecting",
                    RuntimeHealth::Failed { .. } => "error",
                }
                .to_owned(),
            );
        }
        status
    }
    pub fn internal_network(&self) -> crate::InternalNetwork {
        if self.final_blocked {
            return self.warp_internal_network().blocked();
        }
        self.gate.as_ref().map_or_else(
            || self.warp_internal_network(),
            |gate| gate.frontend.internal_network(),
        )
    }
    pub fn warp_internal_network(&self) -> crate::InternalNetwork {
        match &self.inner {
            RuntimeInner::ConnectIp(r) => r.internal_network(),
            RuntimeInner::L4(r) => r.internal_network(),
        }
    }
    pub fn underlay_monitor(&self) -> ManagedTunnelMonitor {
        match &self.inner {
            RuntimeInner::ConnectIp(r) => r.monitor(),
            RuntimeInner::L4(r) => r.monitor.clone(),
        }
    }
    pub fn mode(&self) -> DataPlaneMode {
        match &self.inner {
            RuntimeInner::ConnectIp(_) => DataPlaneMode::ConnectIp,
            RuntimeInner::L4(_) => DataPlaneMode::L4Proxy,
        }
    }
    pub fn l4_snapshot(&self) -> Option<L4Snapshot> {
        match &self.inner {
            RuntimeInner::ConnectIp(_) => None,
            RuntimeInner::L4(runtime) => Some(runtime.client.snapshot()),
        }
    }
    pub fn assigned_ipv4(&self) -> Ipv4Addr {
        if let Some(gate) = &self.gate {
            return gate.network.ipv4.unwrap_or(Ipv4Addr::UNSPECIFIED);
        }
        match &self.inner {
            RuntimeInner::ConnectIp(r) => r.assigned_ipv4(),
            RuntimeInner::L4(r) => r.assigned_ipv4,
        }
    }
    pub fn assigned_ipv6(&self) -> Ipv6Addr {
        if let Some(gate) = &self.gate {
            return gate.network.ipv6.unwrap_or(Ipv6Addr::UNSPECIFIED);
        }
        match &self.inner {
            RuntimeInner::ConnectIp(r) => r.assigned_ipv6(),
            RuntimeInner::L4(r) => r.assigned_ipv6,
        }
    }
    pub fn monitor(&self) -> ManagedTunnelMonitor {
        if let Some(gate) = &self.gate {
            return gate.frontend.monitor();
        }
        match &self.inner {
            RuntimeInner::ConnectIp(r) => r.monitor(),
            RuntimeInner::L4(r) => r.monitor.clone(),
        }
    }
    pub fn path(&self) -> RuntimePath {
        self.monitor().path()
    }
    pub fn health(&self) -> RuntimeHealth {
        let gate = self.gate_status();
        if gate.stage == usque_core::vpngate::GateStage::Error {
            let current = self.monitor().health();
            let error = TransportError::VpnGate(gate.failure.unwrap_or(GateFailure::Transport));
            return RuntimeHealth::Failed {
                last_path: current.path(),
                reconnect_count: current.reconnect_count(),
                message: error.to_string(),
                failure: error.failure(None, None),
            };
        }
        if self.final_blocked {
            let path = self.underlay_monitor().health().path();
            let error = TransportError::VpnGate(GateFailure::Transport);
            return RuntimeHealth::Reconnecting {
                last_path: path,
                attempt: 0,
                reconnect_count: 0,
                reason: "Final network configuration pending".into(),
                failure: error.failure(None, None),
            };
        }
        self.monitor().health()
    }
    pub fn statistics(&self) -> TrafficSnapshot {
        self.monitor().statistics()
    }
    pub fn connection_timeline(&self) -> ConnectionTimelineSnapshot {
        self.underlay_monitor().connection_timeline()
    }
    pub fn network_quality(&self) -> NetworkQualitySnapshot {
        self.monitor().network_quality()
    }
    pub fn subscribe_network_quality(&self) -> watch::Receiver<NetworkQualitySnapshot> {
        self.monitor().subscribe_network_quality()
    }
    pub fn diagnostic_dns_context(&self) -> (Arc<dyn SocketProtector>, CancellationToken) {
        if let Some(gate) = &self.gate {
            return gate.frontend.diagnostic_dns_context();
        }
        match &self.inner {
            RuntimeInner::ConnectIp(r) => r.diagnostic_dns_context(),
            RuntimeInner::L4(r) => r.diagnostic_dns_context(),
        }
    }
    pub fn performance(&self) -> ProxyPerformanceSnapshot {
        if let Some(gate) = &self.gate {
            return gate.frontend.performance();
        }
        match &self.inner {
            RuntimeInner::ConnectIp(r) => r.performance(),
            RuntimeInner::L4(r) => r.performance(),
        }
    }
    pub fn failure(&self) -> Option<String> {
        if let Some(gate) = &self.gate {
            return gate.frontend.failure();
        }
        match &self.inner {
            RuntimeInner::ConnectIp(r) => r.failure(),
            RuntimeInner::L4(r) => r.failure(),
        }
    }
    pub fn listeners(&self) -> &[SocketAddr] {
        if self.final_blocked || self.gate_status().stage == usque_core::vpngate::GateStage::Error {
            return &[];
        }
        if let Some(gate) = &self.gate {
            return gate.frontend.listeners();
        }
        match &self.inner {
            RuntimeInner::ConnectIp(r) => r.listeners(),
            RuntimeInner::L4(r) => r.listeners(),
        }
    }
    pub fn socks5_listeners(&self) -> &[SocketAddr] {
        if self.final_blocked || self.gate_status().stage == usque_core::vpngate::GateStage::Error {
            return &[];
        }
        if let Some(gate) = &self.gate {
            return gate.frontend.socks5_listeners();
        }
        match &self.inner {
            RuntimeInner::ConnectIp(r) => r.socks5_listeners(),
            RuntimeInner::L4(r) => r.socks5_listeners(),
        }
    }
    pub fn http_listeners(&self) -> &[SocketAddr] {
        if self.final_blocked || self.gate_status().stage == usque_core::vpngate::GateStage::Error {
            return &[];
        }
        if let Some(gate) = &self.gate {
            return gate.frontend.http_listeners();
        }
        match &self.inner {
            RuntimeInner::ConnectIp(r) => r.http_listeners(),
            RuntimeInner::L4(r) => r.http_listeners(),
        }
    }
    pub async fn reconfigure_frontends(&mut self, profile: &Profile) -> Result<(), TransportError> {
        if self.final_blocked {
            return Err(TransportError::VpnGate(
                self.transition_status
                    .failure
                    .unwrap_or(GateFailure::Transport),
            ));
        }
        if let Some(gate) = &mut self.gate {
            return gate
                .frontend
                .reconfigure_frontends(&final_profile(profile, &gate.network))
                .await;
        }
        if profile.data_plane != self.mode() {
            return Err(TransportError::UnsupportedOperatingMode);
        }
        match &mut self.inner {
            RuntimeInner::ConnectIp(r) => r.reconfigure_frontends(profile).await,
            RuntimeInner::L4(r) => r.reconfigure_frontends(profile).await,
        }
    }
    pub fn attach_tun(&mut self) -> Result<TunPacketIo, TransportError> {
        if self.final_blocked && self.pending_frontends.is_none() {
            return Err(TransportError::VpnGate(
                self.transition_status
                    .failure
                    .unwrap_or(GateFailure::Transport),
            ));
        }
        if let Some(gate) = &mut self.gate {
            return Ok(TunPacketIo {
                inner: TunIoInner::ConnectIp(gate.frontend.attach_tun()?),
            });
        }
        let inner = match &mut self.inner {
            RuntimeInner::ConnectIp(r) => TunIoInner::ConnectIp(r.attach_tun()?),
            RuntimeInner::L4(r) => TunIoInner::L4(
                r.bridge
                    .as_mut()
                    .ok_or(TransportError::UnsupportedOperatingMode)?
                    .attach()?,
            ),
        };
        Ok(TunPacketIo { inner })
    }
    pub fn detach_tun(&mut self) {
        if let Some(gate) = &mut self.gate {
            gate.frontend.detach_tun();
            return;
        }
        match &mut self.inner {
            RuntimeInner::ConnectIp(r) => r.detach_tun(),
            RuntimeInner::L4(r) => {
                if let Some(b) = &r.bridge {
                    b.cancel();
                }
            }
        }
    }
    pub async fn send_packet(&self, packet: &[u8]) -> Result<(), TransportError> {
        self.send_owned_packet(Bytes::copy_from_slice(packet)).await
    }
    pub async fn send_owned_packet(&self, packet: Bytes) -> Result<(), TransportError> {
        if self.final_blocked {
            return Err(TransportError::TunnelClosed);
        }
        if let Some(gate) = &self.gate {
            return gate.frontend.send_owned_packet(packet).await;
        }
        crate::h2::validate_ip_packet(&packet)?;
        match &self.inner {
            RuntimeInner::ConnectIp(r) => r.send_owned_packet(packet).await,
            RuntimeInner::L4(r) => r
                .bridge
                .as_ref()
                .ok_or(TransportError::TunnelClosed)?
                .outgoing
                .send(packet)
                .await
                .map_err(|_| TransportError::TunnelClosed),
        }
    }
    pub fn cancel_immediately(&mut self) {
        if let Some(gate) = &mut self.gate {
            gate.driver.cancel();
            gate.frontend.cancel_immediately();
        }
        match &mut self.inner {
            RuntimeInner::ConnectIp(r) => r.cancel_immediately(),
            RuntimeInner::L4(r) => r.cancel_immediately(),
        }
    }
    pub async fn shutdown(&mut self) {
        if let Some(mut gate) = self.gate.take() {
            gate.driver.cancel();
            gate.frontend.shutdown().await;
            gate.driver.shutdown().await;
        }
        match &mut self.inner {
            RuntimeInner::ConnectIp(r) => r.shutdown().await,
            RuntimeInner::L4(r) => r.shutdown().await,
        }
    }
}

fn final_profile(profile: &Profile, network: &FinalNetworkParameters) -> Profile {
    let mut final_profile = profile.clone();
    final_profile.data_plane = DataPlaneMode::ConnectIp;
    final_profile.mtu = profile.mtu.min(network.mtu);
    if profile.dns_mode == usque_core::DnsMode::Tunnel {
        final_profile.dns_servers = network.dns_servers.clone();
    }
    if final_profile.proxy.dns_mode == usque_core::ProxyDnsMode::EdgeResolved {
        final_profile.proxy.dns_mode = usque_core::ProxyDnsMode::Remote;
    }
    final_profile
}

pub struct TunPacketIo {
    inner: TunIoInner,
}
enum TunIoInner {
    ConnectIp(MasqueTunIo),
    L4(L4TunIo),
}
impl TunPacketIo {
    /// Present only for a data plane that supports stream performance sampling.
    pub fn write_observer(&self) -> Option<crate::TunWriteObserver> {
        match &self.inner {
            TunIoInner::ConnectIp(_) => None,
            TunIoInner::L4(io) => Some(io.write_observer()),
        }
    }
    /// Starts a cancellation-safe, owned enqueue without borrowing the receive
    /// half. At most one pending send is retained by each platform packet pump.
    pub fn start_send_owned_packet(
        &self,
        packet: Bytes,
    ) -> impl std::future::Future<Output = Result<(), TransportError>> + Send + use<> {
        enum BackendSend<C, L> {
            ConnectIp(C),
            L4(L),
        }
        let send = match &self.inner {
            TunIoInner::ConnectIp(io) => BackendSend::ConnectIp(io.start_send_owned_packet(packet)),
            TunIoInner::L4(io) => BackendSend::L4(io.start_send_owned_packet(packet)),
        };
        async move {
            match send {
                BackendSend::ConnectIp(send) => send.await,
                BackendSend::L4(send) => send.await,
            }
        }
    }

    pub async fn send_packet(&self, packet: &[u8]) -> Result<(), TransportError> {
        self.send_owned_packet(Bytes::copy_from_slice(packet)).await
    }
    pub async fn send_owned_packet(&self, packet: Bytes) -> Result<(), TransportError> {
        match &self.inner {
            TunIoInner::ConnectIp(io) => io.send_owned_packet(packet).await,
            TunIoInner::L4(io) => io.send_owned_packet(packet).await,
        }
    }
    pub async fn receive_packet(&mut self) -> Result<Bytes, TransportError> {
        match &mut self.inner {
            TunIoInner::ConnectIp(io) => io.receive_packet().await,
            TunIoInner::L4(io) => io.receive_packet().await,
        }
    }
    pub fn try_receive_packet(&mut self) -> Result<Option<Bytes>, TransportError> {
        match &mut self.inner {
            TunIoInner::ConnectIp(io) => io.try_receive_packet(),
            TunIoInner::L4(io) => io.try_receive_packet(),
        }
    }
    pub fn record_platform_packet_buffer_allocation(&self) {
        if let TunIoInner::ConnectIp(io) = &self.inner {
            io.record_platform_packet_buffer_allocation();
        }
    }
}
