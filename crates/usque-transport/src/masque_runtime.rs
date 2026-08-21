use std::collections::HashSet;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::Arc;

use bytes::Bytes;
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use ts_netstack_smoltcp::WakingPipe;
use usque_core::{Profile, ProxyAuthCredentials, ProxyDnsMode};

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
    socks5_spec: Option<FrontendSpec>,
    http: Option<HttpProxyFrontend>,
    http_spec: Option<FrontendSpec>,
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
        let credentials = match profile.proxy.listener_credentials() {
            Ok(credentials) => credentials,
            Err(error) => {
                return Err(if profile.frontends.socks5 {
                    TransportError::Socks5(error.to_string())
                } else {
                    TransportError::HttpProxy(error.to_string())
                });
            }
        };

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
        let socks5_spec = socks5.as_ref().map(|frontend| {
            FrontendSpec::socks5(frontend.listeners(), profile, credentials.clone())
        });
        let http_spec = http
            .as_ref()
            .map(|frontend| FrontendSpec::http(frontend.listeners(), profile, credentials));
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
            socks5_spec,
            http,
            http_spec,
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
    /// A frontend is kept only when its bound addresses and hot-reconfigure
    /// identity (credentials, proxy DNS, and SOCKS UDP idle) still match.
    /// New sockets are bound before any removed frontend is shut down so a
    /// later bind failure can restore from the still-held listeners. Identity
    /// changes on the same addresses still release that protocol first,
    /// because Windows will not let a second socket claim the same address.
    pub async fn reconfigure_frontends(&mut self, profile: &Profile) -> Result<(), TransportError> {
        let keep_socks5 = profile.frontends.socks5
            && self.socks5.is_some()
            && self.socks5_spec.as_ref() == FrontendSpec::from_socks5_profile(profile).as_ref();
        let keep_http = profile.frontends.http
            && self.http.is_some()
            && self.http_spec.as_ref() == FrontendSpec::from_http_profile(profile).as_ref();

        let add_socks5 = profile.frontends.socks5 && !keep_socks5;
        let add_http = profile.frontends.http && !keep_http;
        let socks5_rebind_same = add_socks5
            && self.socks5.as_ref().is_some_and(|frontend| {
                listeners_overlap(frontend.listeners(), &profile.proxy.socks5_listeners)
            });
        let http_rebind_same = add_http
            && self.http.as_ref().is_some_and(|frontend| {
                listeners_overlap(frontend.listeners(), &profile.proxy.http_listeners)
            });

        let mut socks5_bound = None;
        if add_socks5 && !socks5_rebind_same {
            match Socks5Frontend::prebind(profile) {
                Ok(bound) => socks5_bound = Some(bound),
                Err(error) => {
                    self.refresh_listeners();
                    return Err(error);
                }
            }
        }
        let mut http_bound = None;
        if add_http && !http_rebind_same {
            match HttpProxyFrontend::prebind(profile) {
                Ok(bound) => http_bound = Some(bound),
                Err(error) => {
                    self.refresh_listeners();
                    return Err(error);
                }
            }
        }

        if !keep_socks5 && let Some(mut frontend) = self.socks5.take() {
            self.socks5_spec.take();
            frontend.shutdown().await;
        }
        if !keep_http && let Some(mut frontend) = self.http.take() {
            self.http_spec.take();
            frontend.shutdown().await;
        }

        if add_socks5 && socks5_rebind_same {
            match Socks5Frontend::prebind(profile) {
                Ok(bound) => socks5_bound = Some(bound),
                Err(error) => {
                    self.refresh_listeners();
                    return Err(error);
                }
            }
        }
        if add_http && http_rebind_same {
            match HttpProxyFrontend::prebind(profile) {
                Ok(bound) => http_bound = Some(bound),
                Err(error) => {
                    self.refresh_listeners();
                    return Err(error);
                }
            }
        }

        if let Some(bound) = socks5_bound {
            let frontend = Socks5Frontend::activate(
                profile,
                self.assigned_ipv4,
                self.assigned_ipv6,
                &self.stack,
                bound,
            );
            self.socks5_spec = FrontendSpec::from_socks5_frontend(&frontend, profile);
            self.socks5 = Some(frontend);
        }
        if let Some(bound) = http_bound {
            let frontend = HttpProxyFrontend::activate(
                profile,
                self.assigned_ipv4,
                self.assigned_ipv6,
                &self.stack,
                bound,
            );
            self.http_spec = FrontendSpec::from_http_frontend(&frontend, profile);
            self.http = Some(frontend);
        }

        self.refresh_listeners();
        tokio::task::yield_now().await;
        if let Some(message) = self.socks5.as_ref().and_then(Socks5Frontend::failure) {
            return Err(TransportError::Socks5(message));
        }
        if let Some(message) = self.http.as_ref().and_then(HttpProxyFrontend::failure) {
            return Err(TransportError::HttpProxy(message));
        }
        Ok(())
    }

    fn refresh_listeners(&mut self) {
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

fn same_listeners(active: &[SocketAddr], wanted: &[SocketAddr]) -> bool {
    let active: HashSet<SocketAddr> = active.iter().copied().collect();
    let wanted: HashSet<SocketAddr> = wanted.iter().copied().collect();
    active == wanted
}

fn listeners_overlap(active: &[SocketAddr], wanted: &[SocketAddr]) -> bool {
    let active: HashSet<SocketAddr> = active.iter().copied().collect();
    wanted.iter().any(|address| active.contains(address))
}

/// Identity that must match for a hot-reconfigure to keep a live frontend.
///
/// Listener addresses alone are not enough: auth, proxy DNS, and SOCKS UDP
/// idle are also applied at `activate` time and live in the accept-loop
/// context until the frontend is rebuilt.
#[derive(Clone, Debug, PartialEq, Eq)]
struct FrontendSpec {
    listeners: HashSet<SocketAddr>,
    credentials: Option<ProxyAuthCredentials>,
    dns_mode: ProxyDnsMode,
    dns_servers: Vec<IpAddr>,
    udp_idle_timeout_seconds: Option<u32>,
}

impl FrontendSpec {
    fn socks5(
        listeners: &[SocketAddr],
        profile: &Profile,
        credentials: Option<ProxyAuthCredentials>,
    ) -> Self {
        Self {
            listeners: listeners.iter().copied().collect(),
            credentials,
            dns_mode: profile.proxy.dns_mode,
            dns_servers: profile.proxy.dns_servers.clone(),
            udp_idle_timeout_seconds: Some(profile.proxy.udp_idle_timeout_seconds),
        }
    }

    fn http(
        listeners: &[SocketAddr],
        profile: &Profile,
        credentials: Option<ProxyAuthCredentials>,
    ) -> Self {
        Self {
            listeners: listeners.iter().copied().collect(),
            credentials,
            dns_mode: profile.proxy.dns_mode,
            dns_servers: profile.proxy.dns_servers.clone(),
            udp_idle_timeout_seconds: None,
        }
    }

    fn from_socks5_profile(profile: &Profile) -> Option<Self> {
        Some(Self::socks5(
            &profile.proxy.socks5_listeners,
            profile,
            profile.proxy.listener_credentials().ok()?,
        ))
    }

    fn from_http_profile(profile: &Profile) -> Option<Self> {
        Some(Self::http(
            &profile.proxy.http_listeners,
            profile,
            profile.proxy.listener_credentials().ok()?,
        ))
    }

    fn from_socks5_frontend(frontend: &Socks5Frontend, profile: &Profile) -> Option<Self> {
        Some(Self::socks5(
            frontend.listeners(),
            profile,
            profile.proxy.listener_credentials().ok()?,
        ))
    }

    fn from_http_frontend(frontend: &HttpProxyFrontend, profile: &Profile) -> Option<Self> {
        Some(Self::http(
            frontend.listeners(),
            profile,
            profile.proxy.listener_credentials().ok()?,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use usque_core::FrontendSettings;
    use zeroize::Zeroizing;

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
        assert!(!same_listeners(&left, &["127.0.0.1:8080".parse().unwrap()]));
    }

    #[test]
    fn frontend_spec_includes_auth_dns_and_idle() {
        let profile = Profile::default();
        let socks = FrontendSpec::from_socks5_profile(&profile).unwrap();
        let http = FrontendSpec::from_http_profile(&profile).unwrap();

        let mut auth = profile.clone();
        auth.proxy.auth_username = Some("lan-user".to_owned());
        auth.proxy.auth_password = Some(Zeroizing::new(b"s3cret".to_vec()));
        assert_ne!(socks, FrontendSpec::from_socks5_profile(&auth).unwrap());
        assert_ne!(http, FrontendSpec::from_http_profile(&auth).unwrap());

        let mut dns = profile.clone();
        dns.proxy.dns_mode = ProxyDnsMode::System;
        assert_ne!(socks, FrontendSpec::from_socks5_profile(&dns).unwrap());
        assert_ne!(http, FrontendSpec::from_http_profile(&dns).unwrap());

        let mut servers = profile.clone();
        servers.proxy.dns_servers = vec!["8.8.8.8".parse().unwrap()];
        assert_ne!(socks, FrontendSpec::from_socks5_profile(&servers).unwrap());
        assert_ne!(http, FrontendSpec::from_http_profile(&servers).unwrap());

        let mut idle = profile.clone();
        idle.proxy.udp_idle_timeout_seconds = 12;
        assert_ne!(socks, FrontendSpec::from_socks5_profile(&idle).unwrap());
        assert_eq!(http, FrontendSpec::from_http_profile(&idle).unwrap());
    }

    #[tokio::test]
    async fn reconfigure_rebuilds_auth_on_identical_ports() {
        let socks_addr = free_loopback();
        let http_addr = free_loopback();
        let profile = proxy_profile(socks_addr, http_addr);
        let mut runtime = start_local(&profile).await;

        assert_eq!(socks_no_auth_method(socks_addr).await, 0);

        let mut authed = profile.clone();
        authed.proxy.auth_username = Some("lan-user".to_owned());
        authed.proxy.auth_password = Some(Zeroizing::new(b"s3cret".to_vec()));
        runtime.reconfigure_frontends(&authed).await.unwrap();
        assert_eq!(runtime.socks5_listeners(), &[socks_addr]);
        assert_eq!(runtime.http_listeners(), &[http_addr]);

        assert_eq!(socks_no_auth_method(socks_addr).await, 0xff);
        assert_eq!(
            socks_userpass_status(socks_addr, b"lan-user", b"wrong").await,
            1
        );
        assert_eq!(
            socks_userpass_status(socks_addr, b"lan-user", b"s3cret").await,
            0
        );
        assert_eq!(http_status(http_addr, None).await, 407);
        assert_eq!(
            http_status(http_addr, Some("Basic bGFuLXVzZXI6d3Jvbmc=")).await,
            407
        );

        runtime.shutdown().await;
    }

    #[tokio::test]
    async fn reconfigure_bind_failure_leaves_previous_listeners() {
        let socks_addr = free_loopback();
        let http_addr = free_loopback();
        let profile = proxy_profile(socks_addr, http_addr);
        let mut runtime = start_local(&profile).await;
        let previous = runtime.listeners().to_vec();
        let previous_socks = runtime.socks5_listeners().to_vec();
        let previous_http = runtime.http_listeners().to_vec();

        let occupied = std::net::TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))
            .expect("hold HTTP port");
        let occupied_addr = occupied.local_addr().expect("occupied addr");
        let next_socks = free_loopback();
        let mut next = profile.clone();
        next.proxy.socks5_listeners = vec![next_socks];
        next.proxy.http_listeners = vec![occupied_addr];

        let error = runtime
            .reconfigure_frontends(&next)
            .await
            .expect_err("HTTP bind must fail");
        assert!(matches!(
            error,
            TransportError::HttpProxyListener { address, .. } if address == occupied_addr
        ));
        assert_eq!(runtime.listeners(), previous.as_slice());
        assert_eq!(runtime.socks5_listeners(), previous_socks.as_slice());
        assert_eq!(runtime.http_listeners(), previous_http.as_slice());
        assert_eq!(socks_no_auth_method(socks_addr).await, 0);
        tokio::net::TcpStream::connect(http_addr)
            .await
            .expect("previous HTTP still accepts");
        std::net::TcpListener::bind(next_socks).expect("failed SOCKS bind must not keep the port");

        drop(occupied);
        runtime.shutdown().await;
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

    fn free_loopback() -> SocketAddr {
        let bound = std::net::TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))
            .expect("ephemeral loopback");
        bound.local_addr().expect("local addr")
    }

    fn proxy_profile(socks5: SocketAddr, http: SocketAddr) -> Profile {
        let mut profile = Profile::default();
        profile.frontends = FrontendSettings {
            tunnel: false,
            socks5: true,
            http: true,
        };
        profile.proxy.socks5_listeners = vec![socks5];
        profile.proxy.http_listeners = vec![http];
        profile
    }

    async fn start_local(profile: &Profile) -> MasqueRuntime {
        let credentials = profile
            .proxy
            .listener_credentials()
            .expect("test listener credentials");
        let assigned_ipv4 = Ipv4Addr::new(172, 16, 0, 2);
        let assigned_ipv6 = Ipv6Addr::new(0x2606, 0x4700, 0, 0, 0, 0, 0, 2);
        let socks5_bound = profile
            .frontends
            .socks5
            .then(|| Socks5Frontend::prebind(profile).expect("bind SOCKS5"));
        let http_bound = profile
            .frontends
            .http
            .then(|| HttpProxyFrontend::prebind(profile).expect("bind HTTP"));
        let monitor = ManagedTunnelMonitor::stub();
        let cancellation = CancellationToken::new();
        let (stack, _pipe) = PacketStack::start_detached(
            profile,
            assigned_ipv4,
            assigned_ipv6,
            &monitor,
            &cancellation,
            crate::socket::noop_socket_protector(),
        )
        .await
        .expect("local packet stack");
        let socks5 = socks5_bound.map(|bound| {
            Socks5Frontend::activate(profile, assigned_ipv4, assigned_ipv6, &stack, bound)
        });
        let http = http_bound.map(|bound| {
            HttpProxyFrontend::activate(profile, assigned_ipv4, assigned_ipv6, &stack, bound)
        });
        let socks5_spec = socks5.as_ref().map(|frontend| {
            FrontendSpec::socks5(frontend.listeners(), profile, credentials.clone())
        });
        let http_spec = http
            .as_ref()
            .map(|frontend| FrontendSpec::http(frontend.listeners(), profile, credentials));
        let listeners = socks5
            .iter()
            .flat_map(|frontend| frontend.listeners().iter().copied())
            .chain(
                http.iter()
                    .flat_map(|frontend| frontend.listeners().iter().copied()),
            )
            .collect();
        let (tun_sink, tun_sink_rx) = watch::channel(None);
        tokio::task::yield_now().await;
        MasqueRuntime {
            monitor,
            stack,
            socks5,
            socks5_spec,
            http,
            http_spec,
            listeners,
            raw_outgoing: None,
            tun_sink,
            _tun_sink_rx: tun_sink_rx,
            cancellation,
            mux_task: None,
            assigned_ipv4,
            assigned_ipv6,
        }
    }

    async fn socks_no_auth_method(address: SocketAddr) -> u8 {
        let mut stream = tokio::net::TcpStream::connect(address)
            .await
            .expect("connect SOCKS5");
        stream.write_all(&[5, 1, 0]).await.expect("SOCKS greeting");
        let mut reply = [0u8; 2];
        stream.read_exact(&mut reply).await.expect("SOCKS method");
        reply[1]
    }

    async fn socks_userpass_status(address: SocketAddr, username: &[u8], password: &[u8]) -> u8 {
        let mut stream = tokio::net::TcpStream::connect(address)
            .await
            .expect("connect SOCKS5");
        stream.write_all(&[5, 1, 2]).await.expect("SOCKS greeting");
        let mut method = [0u8; 2];
        stream.read_exact(&mut method).await.expect("SOCKS method");
        assert_eq!(method, [5, 2]);
        let mut request = vec![1, username.len() as u8];
        request.extend_from_slice(username);
        request.push(password.len() as u8);
        request.extend_from_slice(password);
        stream.write_all(&request).await.expect("userpass");
        let mut status = [0u8; 2];
        stream
            .read_exact(&mut status)
            .await
            .expect("userpass status");
        status[1]
    }

    async fn http_status(address: SocketAddr, proxy_authorization: Option<&str>) -> u16 {
        let mut stream = tokio::net::TcpStream::connect(address)
            .await
            .expect("connect HTTP");
        let mut request = String::from("GET http://example.com/ HTTP/1.1\r\nHost: example.com\r\n");
        if let Some(value) = proxy_authorization {
            request.push_str("Proxy-Authorization: ");
            request.push_str(value);
            request.push_str("\r\n");
        }
        request.push_str("Connection: close\r\n\r\n");
        stream
            .write_all(request.as_bytes())
            .await
            .expect("HTTP request");
        let mut buf = [0u8; 128];
        let n = tokio::time::timeout(Duration::from_secs(2), stream.read(&mut buf))
            .await
            .expect("HTTP response deadline")
            .expect("HTTP response");
        let text = String::from_utf8_lossy(&buf[..n]);
        text.split_whitespace()
            .nth(1)
            .expect("HTTP status")
            .parse()
            .expect("status code")
    }
}
