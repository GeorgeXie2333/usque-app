use std::net::SocketAddr;
use std::sync::Arc;

use bytes::Bytes;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use ts_netstack_smoltcp::WakingPipe;
use usque_core::Profile;

use crate::h2::{MasqueTlsIdentity, TransportError};
use crate::http_proxy::HttpProxyFrontend;
use crate::netstack::{
    ManagedTunnelMonitor, ManagedTunnelRuntime, PacketStack, ProxyPerformanceSnapshot,
    RuntimeHealth, RuntimePath, TrafficSnapshot,
};
use crate::packet_mux::{PacketMuxTable, PacketOrigin};
use crate::pin_refresh::EndpointPinRefresher;
use crate::socket::{SocketProtector, noop_socket_protector};
use crate::socks5::Socks5Frontend;

const PACKET_QUEUE_CAPACITY: usize = 1_024;

/// One reconnecting MASQUE connection shared by the platform TUN/VPN and the
/// optional SOCKS5/HTTP listeners.
pub struct MasqueRuntime {
    monitor: ManagedTunnelMonitor,
    stack: PacketStack,
    socks5: Option<Socks5Frontend>,
    http: Option<HttpProxyFrontend>,
    listeners: Vec<SocketAddr>,
    raw_outgoing: Option<mpsc::Sender<Bytes>>,
    raw_incoming: mpsc::Receiver<Bytes>,
    cancellation: CancellationToken,
    mux_task: Option<JoinHandle<()>>,
}

impl MasqueRuntime {
    pub async fn start(
        profile: &Profile,
        identity: MasqueTlsIdentity,
    ) -> Result<Self, TransportError> {
        Self::start_with_protector(profile, identity, noop_socket_protector()).await
    }

    pub async fn start_with_protector(
        profile: &Profile,
        identity: MasqueTlsIdentity,
        protector: Arc<dyn SocketProtector>,
    ) -> Result<Self, TransportError> {
        Self::start_with_refresh(profile, identity, protector, None).await
    }

    pub async fn start_with_refresh(
        profile: &Profile,
        identity: MasqueTlsIdentity,
        protector: Arc<dyn SocketProtector>,
        pin_refresher: Option<Arc<dyn EndpointPinRefresher>>,
    ) -> Result<Self, TransportError> {
        // Reserve every requested local resource before opening the remote
        // session, so listener conflicts cannot leave a partial runtime.
        let socks5_bound = if profile.frontends.socks5 {
            Some(Socks5Frontend::prebind(profile)?)
        } else {
            None
        };
        let http_bound = if profile.frontends.http {
            Some(HttpProxyFrontend::prebind(profile)?)
        } else {
            None
        };

        let assigned_ipv4 = identity.assigned_ipv4;
        let assigned_ipv6 = identity.assigned_ipv6;
        let mut tunnel = ManagedTunnelRuntime::start_with_refresh(
            profile,
            identity,
            Arc::clone(&protector),
            pin_refresher,
        )
        .await?;
        let monitor = tunnel.monitor();
        let cancellation = CancellationToken::new();
        let (mut stack, proxy_pipe) = PacketStack::start_detached(
            profile,
            assigned_ipv4,
            assigned_ipv6,
            &monitor,
            &cancellation,
            protector,
        )
        .await?;

        let socks5 = socks5_bound.map(|bound| {
            Socks5Frontend::activate(profile, assigned_ipv4, assigned_ipv6, &stack, bound)
        });
        let http = http_bound.map(|bound| {
            HttpProxyFrontend::activate(profile, assigned_ipv4, assigned_ipv6, &stack, bound)
        });
        let listeners = socks5
            .iter()
            .flat_map(|frontend| frontend.listeners().iter().copied())
            .chain(
                http.iter()
                    .flat_map(|frontend| frontend.listeners().iter().copied()),
            )
            .collect();

        tokio::task::yield_now().await;
        if let Some(message) = socks5.as_ref().and_then(Socks5Frontend::failure) {
            stack.shutdown().await;
            tunnel.shutdown().await;
            return Err(TransportError::Socks5(message));
        }
        if let Some(message) = http.as_ref().and_then(HttpProxyFrontend::failure) {
            stack.shutdown().await;
            tunnel.shutdown().await;
            return Err(TransportError::HttpProxy(message));
        }

        let (raw_outgoing, raw_outgoing_rx) = mpsc::channel(PACKET_QUEUE_CAPACITY);
        let (raw_incoming_tx, raw_incoming) = mpsc::channel(PACKET_QUEUE_CAPACITY);
        let mux_cancel = cancellation.clone();
        let mux_task = tokio::spawn(async move {
            run_packet_mux(
                &mut tunnel,
                proxy_pipe,
                raw_outgoing_rx,
                raw_incoming_tx,
                &mux_cancel,
            )
            .await;
            tunnel.shutdown().await;
        });

        Ok(Self {
            monitor,
            stack,
            socks5,
            http,
            listeners,
            raw_outgoing: Some(raw_outgoing),
            raw_incoming,
            cancellation,
            mux_task: Some(mux_task),
        })
    }

    pub async fn send_packet(&self, packet: &[u8]) -> Result<(), TransportError> {
        crate::h2::validate_ip_packet(packet)?;
        self.raw_outgoing
            .as_ref()
            .ok_or(TransportError::TunnelClosed)?
            .send(Bytes::copy_from_slice(packet))
            .await
            .map_err(|_| TransportError::TunnelClosed)
    }

    pub async fn receive_packet(&mut self) -> Result<Bytes, TransportError> {
        self.raw_incoming
            .recv()
            .await
            .ok_or(TransportError::TunnelClosed)
    }

    pub fn monitor(&self) -> ManagedTunnelMonitor {
        self.monitor.clone()
    }

    pub fn path(&self) -> RuntimePath {
        self.monitor.path()
    }

    pub fn health(&self) -> RuntimeHealth {
        self.monitor.health()
    }

    pub fn statistics(&self) -> TrafficSnapshot {
        self.monitor.statistics()
    }

    pub fn performance(&self) -> ProxyPerformanceSnapshot {
        let mut snapshot = self.stack.performance();
        if let Some(http) = &self.http {
            http.augment_performance(&mut snapshot);
        }
        snapshot
    }

    pub fn failure(&self) -> Option<String> {
        self.monitor
            .failure()
            .or_else(|| self.socks5.as_ref().and_then(Socks5Frontend::failure))
            .or_else(|| self.http.as_ref().and_then(HttpProxyFrontend::failure))
    }

    pub fn listeners(&self) -> &[SocketAddr] {
        &self.listeners
    }

    pub fn socks5_listeners(&self) -> &[SocketAddr] {
        self.socks5.as_ref().map_or(&[], Socks5Frontend::listeners)
    }

    pub fn http_listeners(&self) -> &[SocketAddr] {
        self.http.as_ref().map_or(&[], HttpProxyFrontend::listeners)
    }

    pub fn cancel_immediately(&mut self) {
        // Cut every ingress before any slower platform cleanup begins.
        self.raw_outgoing.take();
        if let Some(frontend) = self.socks5.as_mut() {
            frontend.cancel_immediately();
        }
        if let Some(frontend) = self.http.as_mut() {
            frontend.cancel_immediately();
        }
        self.stack.cancel_immediately();
        self.cancellation.cancel();
        if let Some(task) = self.mux_task.as_ref() {
            task.abort();
        }
    }

    pub async fn shutdown(&mut self) {
        self.cancel_immediately();
        if let Some(frontend) = self.socks5.as_mut() {
            frontend.shutdown().await;
        }
        if let Some(frontend) = self.http.as_mut() {
            frontend.shutdown().await;
        }
        self.stack.shutdown().await;
        if let Some(task) = self.mux_task.take() {
            let _ = task.await;
        }
    }
}

impl Drop for MasqueRuntime {
    fn drop(&mut self) {
        self.cancel_immediately();
    }
}

async fn run_packet_mux(
    tunnel: &mut ManagedTunnelRuntime,
    proxy_pipe: WakingPipe,
    mut raw_outgoing: mpsc::Receiver<Bytes>,
    raw_incoming: mpsc::Sender<Bytes>,
    cancellation: &CancellationToken,
) {
    let WakingPipe {
        mut rx,
        tx: proxy_incoming,
    } = proxy_pipe;
    let sender = match tunnel.packet_sender() {
        Ok(sender) => sender,
        Err(_) => return,
    };
    let mut flows = PacketMuxTable::default();

    loop {
        // Tokio randomizes ready branch order, so the two ingress queues get
        // equal scheduling opportunities instead of a fixed preference.
        tokio::select! {
            _ = cancellation.cancelled() => break,
            packet = raw_outgoing.recv() => {
                let Some(packet) = packet else { break; };
                let mut packet = packet
                    .try_into_mut()
                    .unwrap_or_else(|packet| bytes::BytesMut::from(packet.as_ref()));
                if flows.route_outgoing(PacketOrigin::Tunnel, &mut packet)
                    && sender.send_owned_packet(packet.freeze()).await.is_err()
                {
                    break;
                }
            }
            packet = rx.recv_async() => {
                let Some(packet) = packet else { break; };
                let mut packet = packet
                    .try_into_mut()
                    .unwrap_or_else(|packet| bytes::BytesMut::from(packet.as_ref()));
                if flows.route_outgoing(PacketOrigin::Proxy, &mut packet)
                    && sender.send_owned_packet(packet.freeze()).await.is_err()
                {
                    break;
                }
            }
            packet = tunnel.receive_packet() => {
                let Ok(packet) = packet else { break; };
                let mut packet = packet
                    .try_into_mut()
                    .unwrap_or_else(|packet| bytes::BytesMut::from(packet.as_ref()));
                match flows.route_incoming(&mut packet) {
                    Some(PacketOrigin::Tunnel) => {
                        if raw_incoming.send(packet.freeze()).await.is_err() {
                            break;
                        }
                    }
                    Some(PacketOrigin::Proxy) => proxy_incoming.send_async(&packet).await,
                    None => tracing::debug!("dropped an unattributed MASQUE return packet"),
                }
            }
        }
    }
}
