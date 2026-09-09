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
use usque_core::{DataPlaneMode, L4Snapshot, Profile};

pub struct DataPlaneRuntime {
    inner: RuntimeInner,
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
        Ok(Self { inner })
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
        match &self.inner {
            RuntimeInner::ConnectIp(r) => r.assigned_ipv4(),
            RuntimeInner::L4(r) => r.assigned_ipv4,
        }
    }
    pub fn assigned_ipv6(&self) -> Ipv6Addr {
        match &self.inner {
            RuntimeInner::ConnectIp(r) => r.assigned_ipv6(),
            RuntimeInner::L4(r) => r.assigned_ipv6,
        }
    }
    pub fn monitor(&self) -> ManagedTunnelMonitor {
        match &self.inner {
            RuntimeInner::ConnectIp(r) => r.monitor(),
            RuntimeInner::L4(r) => r.monitor.clone(),
        }
    }
    pub fn path(&self) -> RuntimePath {
        self.monitor().path()
    }
    pub fn health(&self) -> RuntimeHealth {
        self.monitor().health()
    }
    pub fn statistics(&self) -> TrafficSnapshot {
        self.monitor().statistics()
    }
    pub fn connection_timeline(&self) -> ConnectionTimelineSnapshot {
        self.monitor().connection_timeline()
    }
    pub fn network_quality(&self) -> NetworkQualitySnapshot {
        self.monitor().network_quality()
    }
    pub fn subscribe_network_quality(&self) -> watch::Receiver<NetworkQualitySnapshot> {
        self.monitor().subscribe_network_quality()
    }
    pub fn diagnostic_dns_context(&self) -> (Arc<dyn SocketProtector>, CancellationToken) {
        match &self.inner {
            RuntimeInner::ConnectIp(r) => r.diagnostic_dns_context(),
            RuntimeInner::L4(r) => r.diagnostic_dns_context(),
        }
    }
    pub fn performance(&self) -> ProxyPerformanceSnapshot {
        match &self.inner {
            RuntimeInner::ConnectIp(r) => r.performance(),
            RuntimeInner::L4(r) => r.performance(),
        }
    }
    pub fn failure(&self) -> Option<String> {
        match &self.inner {
            RuntimeInner::ConnectIp(r) => r.failure(),
            RuntimeInner::L4(r) => r.failure(),
        }
    }
    pub fn listeners(&self) -> &[SocketAddr] {
        match &self.inner {
            RuntimeInner::ConnectIp(r) => r.listeners(),
            RuntimeInner::L4(r) => r.listeners(),
        }
    }
    pub fn socks5_listeners(&self) -> &[SocketAddr] {
        match &self.inner {
            RuntimeInner::ConnectIp(r) => r.socks5_listeners(),
            RuntimeInner::L4(r) => r.socks5_listeners(),
        }
    }
    pub fn http_listeners(&self) -> &[SocketAddr] {
        match &self.inner {
            RuntimeInner::ConnectIp(r) => r.http_listeners(),
            RuntimeInner::L4(r) => r.http_listeners(),
        }
    }
    pub async fn reconfigure_frontends(&mut self, profile: &Profile) -> Result<(), TransportError> {
        if profile.data_plane != self.mode() {
            return Err(TransportError::UnsupportedOperatingMode);
        }
        match &mut self.inner {
            RuntimeInner::ConnectIp(r) => r.reconfigure_frontends(profile).await,
            RuntimeInner::L4(r) => r.reconfigure_frontends(profile).await,
        }
    }
    pub fn attach_tun(&mut self) -> Result<TunPacketIo, TransportError> {
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
        match &mut self.inner {
            RuntimeInner::ConnectIp(r) => r.cancel_immediately(),
            RuntimeInner::L4(r) => r.cancel_immediately(),
        }
    }
    pub async fn shutdown(&mut self) {
        match &mut self.inner {
            RuntimeInner::ConnectIp(r) => r.shutdown().await,
            RuntimeInner::L4(r) => r.shutdown().await,
        }
    }
}

pub struct TunPacketIo {
    inner: TunIoInner,
}
enum TunIoInner {
    ConnectIp(MasqueTunIo),
    L4(L4TunIo),
}
impl TunPacketIo {
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
