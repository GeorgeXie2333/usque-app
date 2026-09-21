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
use bytes::{Bytes, BytesMut};
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
    stopped: bool,
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
pub type ChainExitStart = VpnGateStart;
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
    /// Only the final application frontend observes this policy. The outer
    /// transport and an optional WARP underlay remain untouched.
    pub fn update_traffic_policy(&mut self, disable_quic: bool) {
        if let Some(pending) = &mut self.pending_frontends {
            pending.disable_quic = disable_quic;
        }
        if let Some(gate) = &self.gate {
            gate.frontend.update_traffic_policy(disable_quic);
        } else {
            match &self.inner {
                RuntimeInner::ConnectIp(runtime) => runtime.update_traffic_policy(disable_quic),
                RuntimeInner::L4(runtime) => runtime.update_traffic_policy(disable_quic),
            }
        }
    }
    pub async fn start_with_geo_policy(
        profile: &Profile,
        identity: MasqueTlsIdentity,
        protector: Arc<dyn SocketProtector>,
        refresher: Option<Arc<dyn EndpointPinRefresher>>,
        policy: Arc<GeoDirectPolicy>,
    ) -> Result<Self, TransportError> {
        if profile.chain_enabled() {
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
            stopped: false,
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
        if !profile.chain_enabled() {
            return Self::start_with_geo_policy(profile, identity, protector, refresher, policy)
                .await;
        }
        let selected = selected.ok_or(TransportError::VpnGate(GateFailure::Configuration))?;
        if let Some(status) = &status {
            status.send_replace(GateStatus {
                stage: usque_core::vpngate::GateStage::ConnectingWarp,
                current_server: Some(selected.0.clone()),
                current_profile: selected.1.summary.clone(),
                ..Default::default()
            });
        }
        let mut underlay_profile = profile.clone();
        underlay_profile.disable_chain();
        // The final MTU belongs to the exit. Keep WARP's private packet stack
        // at the IPv6 minimum so encapsulated UDP fits its bounded fragments.
        underlay_profile.mtu = 1280;
        underlay_profile.disable_quic = false;
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
        runtime.transition_status.current_profile = selected.1.summary.clone();
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
                status.send_replace(runtime.gate_status());
            }
            runtime.shutdown().await;
            return Err(error);
        }
        Ok(runtime)
    }
    pub fn headless_profile(profile: &Profile) -> Profile {
        let mut headless = profile.clone();
        headless.disable_chain();
        headless.disable_quic = false;
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
        let matches = if let Some(chain) = profile.custom_chain() {
            prepared.summary.as_ref().is_some_and(|summary| {
                Some(summary.id) == chain.profile_id
                    && Some(summary.revision) == chain.revision
                    && summary.protocol.source() == chain.source
                    && !(summary.protocol.requires_udp()
                        && profile.data_plane == DataPlaneMode::L4Proxy)
            })
        } else {
            profile
                .vpn_gate
                .selection
                .as_ref()
                .is_some_and(|selection| {
                    selection.server_id == server.id
                        && selection.config_sha256 == server.config_sha256
                })
        };
        if !matches {
            return Err(TransportError::VpnGate(GateFailure::Configuration));
        }
        let (mut driver, tunnel, mut network) = crate::vpngate::GateDriver::start(
            &prepared,
            self.warp_internal_network(),
            self.underlay_monitor().network_quality_telemetry(),
            status.clone(),
            &cancellation,
        )
        .await?;
        network.mtu = final_mtu(profile, network.mtu);
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
        if let Some(usque_core::chain_exit::ValidatedProfile::WireGuard(wg)) =
            prepared.custom.as_deref()
            && network.dns_servers.iter().all(|ip| !wg.allows(*ip))
            && let Some(status) = &status
        {
            status.send_modify(|s| s.dns_unavailable = true);
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
    /// On failure the owner stops the whole chain. The closed Gate slot keeps
    /// accessors from falling through to WARP during that cleanup.
    pub async fn replace_gate(
        &mut self,
        profile: &Profile,
        selected: Option<(ServerSummary, PreparedProfile)>,
        policy: Arc<GeoDirectPolicy>,
        status: watch::Sender<GateStatus>,
        cancellation: &CancellationToken,
    ) -> Result<(), TransportError> {
        if self.stopped {
            return Err(TransportError::TunnelClosed);
        }
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
            current_profile: selected
                .as_ref()
                .and_then(|(_, prepared)| prepared.summary.clone()),
            ..Default::default()
        };
        status.send_replace(self.transition_status.clone());
        let result = if profile.chain_enabled() {
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
        // A terminal Gate failure ends the entire chain, including WARP.
        // Cancel its producers before waiting for the native worker to exit.
        self.cancel_immediately();
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
                    _ if self.stopped => "disconnected",
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
        if let Some(pending) = &mut self.pending_frontends {
            if profile.data_plane != pending.data_plane
                || profile.vpn_gate != pending.vpn_gate
                || profile.chain_exit != pending.chain_exit
            {
                return Err(TransportError::VpnGate(GateFailure::Configuration));
            }
            // Android reconfigures frontends between attaching the replacement
            // TUN and activating it. Keep those settings pending while leaving
            // final admission closed; activate_final applies them after handoff.
            *pending = profile.clone();
            return Ok(());
        }
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
        if !self.stopped {
            self.transition_status = self.gate_status();
        }
        self.final_blocked = true;
        self.pending_frontends = None;
        self.stopped = true;
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
        self.cancel_immediately();
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

fn final_mtu(profile: &Profile, negotiated: u16) -> u16 {
    if profile
        .custom_chain()
        .is_some_and(|c| c.source == usque_core::chain_exit::ChainSource::WireguardCustom)
    {
        // The imported WireGuard MTU describes the inner interface, separately
        // from the WARP MTU. Its parser has already enforced 1280..=9000.
        negotiated
    } else {
        profile.mtu.min(negotiated)
    }
}
fn final_profile(profile: &Profile, network: &FinalNetworkParameters) -> Profile {
    let mut final_profile = profile.clone();
    final_profile.data_plane = DataPlaneMode::ConnectIp;
    final_profile.mtu = final_mtu(profile, network.mtu);
    if profile.dns_mode == usque_core::DnsMode::Tunnel || profile.custom_chain().is_some() {
        final_profile.dns_servers = network.dns_servers.clone();
    }
    if profile.custom_chain().is_some()
        || final_profile.proxy.dns_mode == usque_core::ProxyDnsMode::EdgeResolved
    {
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
    /// Retains a disjoint mutable slab view through CONNECT-IP header edits.
    /// L4 continues to receive its existing immutable packet representation.
    pub fn start_send_mut_packet(
        &self,
        packet: BytesMut,
    ) -> impl std::future::Future<Output = Result<(), TransportError>> + Send + use<> {
        enum BackendSend<C, L> {
            ConnectIp(C),
            L4(L),
        }
        let send = match &self.inner {
            TunIoInner::ConnectIp(io) => BackendSend::ConnectIp(io.start_send_mut_packet(packet)),
            TunIoInner::L4(io) => BackendSend::L4(io.start_send_owned_packet(packet.freeze())),
        };
        async move {
            match send {
                BackendSend::ConnectIp(send) => send.await,
                BackendSend::L4(send) => send.await,
            }
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::netstack::{ExternalPacketChannels, ManagedTunnelRuntime};
    use usque_core::vpngate::GateStage;

    #[test]
    fn wireguard_inner_mtu_is_independent_of_the_warp_interface_mtu() {
        let profile = Profile {
            mtu: 1280,
            chain_exit: Some(usque_core::chain_exit::ChainExitSettings {
                source: usque_core::chain_exit::ChainSource::WireguardCustom,
                ..Default::default()
            }),
            ..Default::default()
        };
        let network = FinalNetworkParameters {
            ipv4: Some("10.8.0.2".parse().unwrap()),
            ipv6: None,
            dns_servers: vec!["10.8.0.1".parse().unwrap()],
            mtu: 1420,
        };
        assert_eq!(final_profile(&profile, &network).mtu, 1420);
        assert_eq!(final_profile(&Profile::default(), &network).mtu, 1280);
    }

    fn slab_udp(slab: &mut crate::android_tun_read_slab::TunReadSlab, mtu: usize) -> BytesMut {
        let packet = [
            0x45, 0, 0, 28, 0, 0, 0, 0, 64, 17, 0, 0, 172, 16, 0, 2, 198, 51, 100, 1, 0xc3, 0x50,
            1, 0xbb, 0, 8, 0, 0,
        ];
        slab.prepare(mtu).unwrap();
        slab.read_buffer()[..packet.len()].copy_from_slice(&packet);
        slab.take_packet(packet.len()).unwrap()
    }

    #[tokio::test]
    async fn mutable_tun_api_rejects_the_old_attachment_and_preserves_external_ttl() {
        let (mut runtime, mut channels) = memory_warp().await;
        let mut slab = crate::android_tun_read_slab::TunReadSlab::new();
        let old = runtime.attach_tun().unwrap();
        let old_send = old.start_send_mut_packet(slab_udp(&mut slab, 1280));
        runtime.detach_tun();
        let current = runtime.attach_tun().unwrap();
        assert!(matches!(old_send.await, Err(TransportError::TunnelClosed)));
        let packet = slab_udp(&mut slab, 1500);
        let pointer = packet.as_ptr() as usize;
        current.start_send_mut_packet(packet).await.unwrap();
        let packet =
            tokio::time::timeout(std::time::Duration::from_secs(2), channels.outgoing.recv())
                .await
                .unwrap()
                .unwrap();
        assert!(matches!(
            packet,
            crate::outbound_packet::OutboundPacket::Mutable(_)
        ));
        // Same conversion used at the VPN Gate native boundary: no second
        // forwarding mutation is introduced into the external packet path.
        let packet = packet.freeze();
        assert_eq!(packet.as_ptr() as usize, pointer);
        assert_eq!(packet[8], 64);
        assert!(channels.outgoing.try_recv().is_err());
        runtime.shutdown().await;
    }

    #[tokio::test]
    async fn mutable_tun_api_keeps_l4_delivery_mtu_and_cancellation_behavior() {
        let (l4, mut outgoing) = L4TunIo::memory_test_io();
        let io = TunPacketIo {
            inner: TunIoInner::L4(l4),
        };
        let mut slab = crate::android_tun_read_slab::TunReadSlab::new();
        let packet = slab_udp(&mut slab, 1280);
        let pointer = packet.as_ptr() as usize;
        let sibling = slab_udp(&mut slab, 1280);
        io.start_send_mut_packet(packet).await.unwrap();
        let delivered = outgoing.recv().await.unwrap().into_bytes();
        assert_eq!(delivered.as_ptr() as usize, pointer);
        assert_eq!(delivered[8], 64);
        assert_eq!(sibling[8], 64);
        let mut oversized = BytesMut::zeroed(1281);
        oversized[0] = 0x45;
        oversized[2..4].copy_from_slice(&1281_u16.to_be_bytes());
        io.start_send_mut_packet(oversized).await.unwrap();
        assert!(outgoing.try_recv().is_err());
        io.start_send_mut_packet(sibling).await.unwrap();
        let mut waiting = Box::pin(io.start_send_mut_packet(slab_udp(&mut slab, 1280)));
        assert!(
            std::future::poll_fn(|cx| std::task::Poll::Ready(waiting.as_mut().poll(cx)))
                .await
                .is_pending()
        );
        drop(io);
        assert!(matches!(waiting.await, Err(TransportError::TunnelClosed)));
        assert!(outgoing.recv().await.is_some());
        assert!(outgoing.try_recv().is_err());
    }

    async fn memory_warp() -> (DataPlaneRuntime, ExternalPacketChannels) {
        // Memory packet queues stand in for WARP. No OS tunnel or remote
        // connection is created, but the actual frontend shutdown runs.
        let profile = DataPlaneRuntime::headless_profile(&Profile::default());
        let path = RuntimePath {
            transport: usque_core::Transport::Http3,
            endpoint_family: usque_core::AddressFamily::Ipv4,
            ipv4_available: true,
            ipv6_available: true,
        };
        let (tunnel, channels) = ManagedTunnelRuntime::for_external_packets(
            path,
            crate::NetworkQualityTelemetry::default(),
        );
        let protector = crate::socket::noop_socket_protector();
        let policy = Arc::new(GeoDirectPolicy::disabled());
        let addresses = (
            "172.16.0.2".parse().unwrap(),
            "2001:db8::2".parse().unwrap(),
        );
        let warp = MasqueRuntime::start_over_tunnel(
            &profile,
            tunnel,
            addresses,
            protector,
            policy.clone(),
        )
        .await
        .unwrap();
        let runtime = DataPlaneRuntime {
            inner: RuntimeInner::ConnectIp(Box::new(warp)),
            gate: None,
            warp_network: FinalNetworkParameters {
                ipv4: Some(addresses.0),
                ipv6: Some(addresses.1),
                mtu: 1500,
                dns_servers: vec![],
            },
            final_blocked: false,
            stopped: false,
            transition_status: GateStatus {
                stage: GateStage::ConnectingServer,
                ..Default::default()
            },
            pending_frontends: None,
        };
        (runtime, channels)
    }

    #[tokio::test]
    async fn disabling_gate_defers_frontends_until_tun_handoff_activation() {
        let (mut runtime, channels) = memory_warp().await;
        let mut profile = DataPlaneRuntime::headless_profile(&Profile::default());
        profile.frontends.tunnel = true;
        runtime
            .replace_gate(
                &profile,
                None,
                Arc::new(GeoDirectPolicy::disabled()),
                watch::channel(GateStatus::default()).0,
                &CancellationToken::new(),
            )
            .await
            .unwrap();
        let _tun = runtime.attach_tun().unwrap();
        // Use a loopback ephemeral listener to prove that reconfiguration is
        // deferred, rather than admitting proxy traffic before platform setup.
        profile.frontends.socks5 = true;
        profile.proxy.socks5_listeners = vec!["127.0.0.1:0".parse().unwrap()];
        runtime.reconfigure_frontends(&profile).await.unwrap();
        assert!(runtime.final_blocked);
        assert!(runtime.listeners().is_empty());
        if let RuntimeInner::ConnectIp(warp) = &runtime.inner {
            assert!(warp.listeners().is_empty());
        }
        assert!(runtime.send_packet(&[0x45; 20]).await.is_err());
        assert!(!channels.cancellation.is_cancelled());

        runtime.activate_final().await.unwrap();
        assert!(!runtime.final_blocked);
        assert!(runtime.pending_frontends.is_none());
        assert_eq!(runtime.socks5_listeners().len(), 1);
        assert_eq!(runtime.gate_status().stage, GateStage::Disabled);
        assert!(!channels.cancellation.is_cancelled());
        runtime.shutdown().await;
    }

    #[tokio::test]
    async fn cancelled_handoff_cannot_reconfigure_or_activate_pending_frontends() {
        let (mut runtime, _) = memory_warp().await;
        let profile = DataPlaneRuntime::headless_profile(&Profile::default());
        runtime
            .replace_gate(
                &profile,
                None,
                Arc::new(GeoDirectPolicy::disabled()),
                watch::channel(GateStatus::default()).0,
                &CancellationToken::new(),
            )
            .await
            .unwrap();
        let mut wrong_target = profile.clone();
        wrong_target.vpn_gate.enabled = true;
        assert!(runtime.reconfigure_frontends(&wrong_target).await.is_err());
        assert!(runtime.final_blocked);
        runtime.cancel_immediately();
        assert!(runtime.reconfigure_frontends(&profile).await.is_err());
        assert!(runtime.attach_tun().is_err());
        assert!(runtime.activate_final().await.is_err());
        assert!(runtime.final_blocked);
        runtime.shutdown().await;
    }

    #[tokio::test]
    async fn gate_failure_cancels_warp_but_a_pending_node_switch_does_not() {
        let (mut runtime, channels) = memory_warp().await;
        let profile = DataPlaneRuntime::headless_profile(&Profile::default());
        runtime.quiesce_final();
        assert!(runtime.reconfigure_frontends(&profile).await.is_err());
        assert!(!channels.cancellation.is_cancelled());
        runtime.fail_gate(GateFailure::Authentication).await;
        // The mux observes the synchronous stop signal on its next poll and
        // then closes the managed WARP channel it owns.
        tokio::time::timeout(
            std::time::Duration::from_secs(1),
            channels.cancellation.cancelled(),
        )
        .await
        .unwrap();
        assert!(channels.cancellation.is_cancelled());
        assert_eq!(
            runtime.gate_status().warp_stage.as_deref(),
            Some("disconnected")
        );
        assert!(matches!(runtime.health(), RuntimeHealth::Failed { .. }));
        assert!(runtime.send_packet(&[0x45; 20]).await.is_err());
        assert!(matches!(
            runtime
                .replace_gate(
                    &profile,
                    None,
                    Arc::new(GeoDirectPolicy::disabled()),
                    watch::channel(GateStatus::default()).0,
                    &CancellationToken::new()
                )
                .await,
            Err(TransportError::TunnelClosed)
        ));
        runtime.shutdown().await;
    }
}
