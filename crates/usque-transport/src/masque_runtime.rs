use std::collections::HashSet;
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::Arc;

use bytes::Bytes;
use tokio::sync::{mpsc, watch};
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

/// Exclusive TUN packet I/O for one attach lifetime.
///
/// Dropping this detaches TUN from the mux without closing MASQUE. Inbound
/// TUN-origin packets are discarded until [`MasqueRuntime::attach_tun`].
pub struct MasqueTunIo {
    outgoing: mpsc::Sender<Bytes>,
    incoming: mpsc::Receiver<Bytes>,
}

impl MasqueTunIo {
    pub async fn send_packet(&self, packet: &[u8]) -> Result<(), TransportError> {
        crate::h2::validate_ip_packet(packet)?;
        self.outgoing
            .send(Bytes::copy_from_slice(packet))
            .await
            .map_err(|_| TransportError::TunnelClosed)
    }

    pub async fn receive_packet(&mut self) -> Result<Bytes, TransportError> {
        self.incoming
            .recv()
            .await
            .ok_or(TransportError::TunnelClosed)
    }
}

/// One reconnecting MASQUE connection shared by the platform TUN/VPN and the
/// optional SOCKS5/HTTP listeners.
pub struct MasqueRuntime {
    monitor: ManagedTunnelMonitor,
    stack: PacketStack,
    socks5: Option<Socks5Frontend>,
    http: Option<HttpProxyFrontend>,
    listeners: Vec<SocketAddr>,
    raw_outgoing: Option<mpsc::Sender<Bytes>>,
    tun_sink: watch::Sender<Option<mpsc::Sender<Bytes>>>,
    _tun_sink_rx: watch::Receiver<Option<mpsc::Sender<Bytes>>>,
    cancellation: CancellationToken,
    mux_task: Option<JoinHandle<()>>,
    assigned_ipv4: Ipv4Addr,
    assigned_ipv6: Ipv6Addr,
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
        if let Err(error) = profile.proxy.listener_credentials() {
            return Err(if profile.frontends.socks5 {
                TransportError::Socks5(error.to_string())
            } else {
                TransportError::HttpProxy(error.to_string())
            });
        }

        // Reserve every requested local resource before opening the remote
        // session, so listener conflicts cannot leave a partial runtime.
        // Bind IPv4 for both protocols before any IPv6 socket: Windows
        // dual-stack IPv6 listeners otherwise occupy the matching IPv4 port.
        let (socks5_bound, http_bound) = prebind_frontends(profile)?;

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
        let (tun_sink, tun_sink_rx) = watch::channel(None);
        let mux_tun_sink = tun_sink.clone();
        let mux_cancel = cancellation.clone();
        let mux_task = tokio::spawn(async move {
            run_packet_mux(
                &mut tunnel,
                proxy_pipe,
                raw_outgoing_rx,
                mux_tun_sink,
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
            tun_sink,
            _tun_sink_rx: tun_sink_rx,
            cancellation,
            mux_task: Some(mux_task),
            assigned_ipv4,
            assigned_ipv6,
        })
    }

    /// Replace SOCKS5/HTTP listeners without tearing the MASQUE mux.
    ///
    /// Unchanged listeners are kept. A protocol that actually changes is
    /// shut down before its replacement is bound, because Windows will not
    /// let a second socket claim the same address.
    pub async fn reconfigure_frontends(&mut self, profile: &Profile) -> Result<(), TransportError> {
        let keep_socks5 = profile.frontends.socks5
            && self.socks5.as_ref().is_some_and(|frontend| {
                same_listeners(frontend.listeners(), &profile.proxy.socks5_listeners)
            });
        let keep_http = profile.frontends.http
            && self.http.as_ref().is_some_and(|frontend| {
                same_listeners(frontend.listeners(), &profile.proxy.http_listeners)
            });

        if !keep_socks5
            && let Some(mut frontend) = self.socks5.take()
        {
            frontend.shutdown().await;
        }
        if !keep_http
            && let Some(mut frontend) = self.http.take()
        {
            frontend.shutdown().await;
        }

        if profile.frontends.socks5 && !keep_socks5 {
            let bound = Socks5Frontend::prebind(profile)?;
            self.socks5 = Some(Socks5Frontend::activate(
                profile,
                self.assigned_ipv4,
                self.assigned_ipv6,
                &self.stack,
                bound,
            ));
        }
        if profile.frontends.http && !keep_http {
            let bound = HttpProxyFrontend::prebind(profile)?;
            self.http = Some(HttpProxyFrontend::activate(
                profile,
                self.assigned_ipv4,
                self.assigned_ipv6,
                &self.stack,
                bound,
            ));
        }
        self.listeners = self
            .socks5
            .iter()
            .flat_map(|frontend| frontend.listeners().iter().copied())
            .chain(
                self.http
                    .iter()
                    .flat_map(|frontend| frontend.listeners().iter().copied()),
            )
            .collect();

        tokio::task::yield_now().await;
        if let Some(message) = self.socks5.as_ref().and_then(Socks5Frontend::failure) {
            return Err(TransportError::Socks5(message));
        }
        if let Some(message) = self.http.as_ref().and_then(HttpProxyFrontend::failure) {
            return Err(TransportError::HttpProxy(message));
        }
        Ok(())
    }

    /// Attach TUN I/O. Replaces any previous attach; the old receiver closes.
    pub fn attach_tun(&mut self) -> Result<MasqueTunIo, TransportError> {
        let outgoing = self
            .raw_outgoing
            .clone()
            .ok_or(TransportError::TunnelClosed)?;
        let (incoming_tx, incoming) = mpsc::channel(PACKET_QUEUE_CAPACITY);
        self.tun_sink.send_replace(Some(incoming_tx));
        Ok(MasqueTunIo { outgoing, incoming })
    }

    /// Stop delivering TUN-origin packets. SOCKS/HTTP and MASQUE stay up.
    pub fn detach_tun(&mut self) {
        self.tun_sink.send_replace(None);
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

    pub fn assigned_ipv4(&self) -> Ipv4Addr {
        self.assigned_ipv4
    }

    pub fn assigned_ipv6(&self) -> Ipv6Addr {
        self.assigned_ipv6
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
    tun_sink: watch::Sender<Option<mpsc::Sender<Bytes>>>,
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
                        dispatch_tun_incoming(&tun_sink, packet.freeze());
                    }
                    Some(PacketOrigin::Proxy) => proxy_incoming.send_async(&packet).await,
                    None => tracing::debug!("dropped an unattributed MASQUE return packet"),
                }
            }
        }
    }
}

/// Deliver a TUN-destined packet, or drop it when TUN is detached.
///
/// A closed or full TUN sink must not tear the MASQUE mux: SOCKS/HTTP still
/// need the session.
fn dispatch_tun_incoming(tun_sink: &watch::Sender<Option<mpsc::Sender<Bytes>>>, packet: Bytes) {
    let sink = tun_sink.borrow().clone();
    let Some(sink) = sink else {
        return;
    };
    match sink.try_send(packet) {
        Ok(()) => {}
        Err(mpsc::error::TrySendError::Full(_)) => {}
        Err(mpsc::error::TrySendError::Closed(_)) => {
            tun_sink.send_replace(None);
        }
    }
}

type BoundFrontends = (
    Option<Vec<tokio::net::TcpListener>>,
    Option<Vec<tokio::net::TcpListener>>,
);

fn prebind_frontends(profile: &Profile) -> Result<BoundFrontends, TransportError> {
    let mut jobs = Vec::new();
    if profile.frontends.socks5 {
        jobs.extend(
            profile
                .proxy
                .socks5_listeners
                .iter()
                .copied()
                .map(|address| (false, address)),
        );
    }
    if profile.frontends.http {
        jobs.extend(
            profile
                .proxy
                .http_listeners
                .iter()
                .copied()
                .map(|address| (true, address)),
        );
    }
    jobs.sort_by_key(|(_, address)| address.is_ipv6());

    let mut socks5 = Vec::new();
    let mut http = Vec::new();
    for (is_http, address) in jobs {
        let listener = crate::socket::bind_tcp_listener(address).map_err(|source| {
            if is_http {
                TransportError::HttpProxyListener { address, source }
            } else {
                TransportError::SocksListener { address, source }
            }
        })?;
        if is_http {
            http.push(listener);
        } else {
            socks5.push(listener);
        }
    }
    Ok((
        profile.frontends.socks5.then_some(socks5),
        profile.frontends.http.then_some(http),
    ))
}

fn same_listeners(active: &[SocketAddr], wanted: &[SocketAddr]) -> bool {
    let active: HashSet<SocketAddr> = active.iter().copied().collect();
    let wanted: HashSet<SocketAddr> = wanted.iter().copied().collect();
    active == wanted
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_listeners_compares_as_a_set() {
        let left = vec![
            "127.0.0.1:8080".parse().unwrap(),
            "[::1]:8080".parse().unwrap(),
        ];
        let right = vec![
            "[::1]:8080".parse().unwrap(),
            "127.0.0.1:8080".parse().unwrap(),
        ];
        assert!(same_listeners(&left, &right));
        assert!(!same_listeners(
            &left,
            &["127.0.0.1:8080".parse().unwrap()]
        ));
    }

    #[tokio::test]
    async fn detached_tun_sink_drops_packets_without_closing_the_channel() {
        let (tun_sink, _rx) = watch::channel(None);
        let (tx, mut rx) = mpsc::channel(4);
        tun_sink.send_replace(Some(tx.clone()));
        dispatch_tun_incoming(&tun_sink, Bytes::from_static(b"keep"));
        assert_eq!(rx.recv().await.unwrap(), Bytes::from_static(b"keep"));

        tun_sink.send_replace(None);
        dispatch_tun_incoming(&tun_sink, Bytes::from_static(b"drop"));
        assert!(rx.try_recv().is_err());
        assert!(tun_sink.borrow().is_none());

        drop(rx);
        tun_sink.send_replace(Some(tx));
        dispatch_tun_incoming(&tun_sink, Bytes::from_static(b"closed"));
        assert!(tun_sink.borrow().is_none());
    }
}
