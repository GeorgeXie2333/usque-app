//! Local proxy listeners for a system-owned VPN. This module cannot create a
//! MASQUE session, bind to a physical interface, or mutate system routing/DNS.
//! The independent native filter must authenticate each exact flow before it
//! can emit a SYN/datagram. Isolated socket tests do not prove kernel routing.
use crate::{
    ConnectionTelemetry, ProxyPerformanceSnapshot, RuntimeHealth, TrafficSnapshot, TransportError,
    http_proxy::HttpProxyFrontend,
    netstack::TrafficCounters,
    proxy_udp::{UdpAssociation, UdpFactory},
    socks5::Socks5Frontend,
    tcp::{DialError, FlowClass, FrontendAdmission, ProxyServices, TcpDialer, TcpIo, TcpTarget},
};
use async_trait::async_trait;
use bytes::Bytes;
use std::{
    collections::HashSet,
    future::Future,
    io,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll},
};
use tokio::{
    io::{AsyncRead, AsyncWrite, ReadBuf},
    net::{TcpSocket, TcpStream, UdpSocket},
    sync::watch,
    time::{Duration, Instant, timeout_at},
};
use tokio_util::{sync::CancellationToken, task::AbortOnDropHandle};
use usque_core::{Profile, ProxyDnsMode};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemFlowProtocol {
    Tcp,
    Udp,
}

/// A single socket intent, never an IP-to-domain cache entry. Native control
/// adds the authenticated Engine identity, session, and configuration revision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemFlowIntent {
    pub local: SocketAddr,
    pub remote: SocketAddr,
    pub hostname: Option<String>,
    pub protocol: SystemFlowProtocol,
    pub epoch: u64,
}

#[async_trait]
pub trait SystemFlowAuthorizer: Send + Sync {
    async fn authorize(&self, intent: SystemFlowIntent) -> Result<(), String>;
}
struct DenyFlows;
#[async_trait]
impl SystemFlowAuthorizer for DenyFlows {
    async fn authorize(&self, _: SystemFlowIntent) -> Result<(), String> {
        Err("macos_flow_authorizer_unavailable".into())
    }
}

struct Epoch {
    active: bool,
    generation: u64,
    state: RuntimeHealth,
    token: CancellationToken,
}
struct Gate {
    inner: Mutex<Epoch>,
    source: watch::Receiver<RuntimeHealth>,
    effective: watch::Sender<RuntimeHealth>,
}
impl Gate {
    fn refresh(&self, active: Option<bool>) {
        let mut next = self.source.borrow().clone();
        if self.source.has_changed().is_err() {
            next = RuntimeHealth::Failed {
                last_path: next.path(),
                reconnect_count: next.reconnect_count(),
                message: "macos_provider_control_closed".into(),
                failure: usque_core::TransportFailure::new(
                    usque_core::TransportFailureCode::EngineUnavailable,
                    usque_core::TransportStage::TunnelStartup,
                ),
            };
        }
        let mut e = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        let changed = e.state != next || active.is_some_and(|v| v != e.active);
        if changed {
            e.token.cancel();
            e.token = CancellationToken::new();
            e.generation += 1;
        }
        e.state = next;
        if let Some(active) = active {
            e.active = active;
        }
        let effective = if e.active || matches!(e.state, RuntimeHealth::Failed { .. }) {
            e.state.clone()
        } else {
            RuntimeHealth::Connecting {
                requested: e.state.path(),
                attempt: 0,
            }
        };
        self.effective.send_if_modified(|value| {
            if *value == effective {
                false
            } else {
                *value = effective;
                true
            }
        });
    }
    fn lease(&self) -> Result<(u64, CancellationToken), DialError> {
        self.refresh(None);
        let e = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        if !e.active
            || !matches!(e.state, RuntimeHealth::Connected { .. })
            || self.source.has_changed().is_err()
        {
            return Err(DialError::Closed);
        }
        Ok((e.generation, e.token.clone()))
    }
    fn valid(&self, epoch: u64) -> Result<(), DialError> {
        let (current, _) = self.lease()?;
        if epoch == current {
            Ok(())
        } else {
            Err(DialError::Closed)
        }
    }
}

pub struct SystemProxyRuntime {
    socks: Option<Socks5Frontend>,
    http: Option<HttpProxyFrontend>,
    gate: Arc<Gate>,
    authorizer: Arc<dyn SystemFlowAuthorizer>,
    counters: Arc<TrafficCounters>,
    cancellation: CancellationToken,
    monitor: Option<AbortOnDropHandle<()>>,
    listeners: Vec<SocketAddr>,
    profile: Profile,
}
impl SystemProxyRuntime {
    /// Safe default: listeners may bind, but no flow can leave without an
    /// authenticated native authorizer supplied by the Engine.
    pub async fn start(
        profile: &Profile,
        health: watch::Receiver<RuntimeHealth>,
    ) -> Result<Self, TransportError> {
        Self::start_with_authorizer(profile, health, Arc::new(DenyFlows)).await
    }
    pub async fn start_with_authorizer(
        profile: &Profile,
        health: watch::Receiver<RuntimeHealth>,
        authorizer: Arc<dyn SystemFlowAuthorizer>,
    ) -> Result<Self, TransportError> {
        let initial = health.borrow().clone();
        let (effective, _) = watch::channel(RuntimeHealth::Connecting {
            requested: initial.path(),
            attempt: 0,
        });
        let gate = Arc::new(Gate {
            inner: Mutex::new(Epoch {
                active: false,
                generation: 0,
                state: initial,
                token: CancellationToken::new(),
            }),
            source: health.clone(),
            effective,
        });
        let cancellation = CancellationToken::new();
        let monitor_gate = gate.clone();
        let monitor_cancel = cancellation.clone();
        let mut source = health;
        let monitor = AbortOnDropHandle::new(tokio::spawn(async move {
            loop {
                tokio::select! {
                    biased;
                    _ = monitor_cancel.cancelled() => break,
                    result = source.changed() => {
                        if result.is_err() { monitor_gate.refresh(Some(false)); break; }
                        monitor_gate.refresh(None);
                    }
                }
            }
        }));
        let mut runtime = Self {
            socks: None,
            http: None,
            gate,
            authorizer,
            counters: Arc::default(),
            cancellation,
            monitor: Some(monitor),
            listeners: Vec::new(),
            profile: profile.clone(),
        };
        runtime.bind(profile)?;
        Ok(runtime)
    }
    fn services(&self, profile: &Profile) -> ProxyServices {
        let network = Arc::new(SystemNetwork {
            gate: self.gate.clone(),
            authorizer: self.authorizer.clone(),
            counters: self.counters.clone(),
        });
        let protector: Arc<dyn crate::SocketProtector> = Arc::new(crate::NoopSocketProtector);
        ProxyServices {
            telemetry: ConnectionTelemetry::default(),
            traffic_policy: Arc::new(crate::application_traffic::ApplicationTrafficPolicy::new(
                profile.disable_quic,
            )),
            admission: Some(FrontendAdmission::for_connect_ip()),
            dialer: network.clone(),
            udp: Some(network),
            resolver: crate::dns::Resolver::system_proxy(protector.clone()),
            protector,
            geo_policy: Arc::new(crate::GeoDirectPolicy::disabled()),
            counters: self.counters.clone(),
            cancellation: self.cancellation.clone(),
            health: self.gate.effective.subscribe(),
        }
    }
    fn bind(&mut self, profile: &Profile) -> Result<(), TransportError> {
        // No Engine-side country/direct rules: all routing is provider-owned.
        // Preserve original hostnames through the existing frontend parsers.
        let mut proxy_profile = profile.clone();
        proxy_profile.proxy.dns_mode = ProxyDnsMode::EdgeResolved;
        if !profile.frontends.socks5 {
            proxy_profile.proxy.socks5_listeners.clear();
        }
        if !profile.frontends.http {
            proxy_profile.proxy.http_listeners.clear();
        }
        let socks = Socks5Frontend::prebind(&proxy_profile)?;
        let http = HttpProxyFrontend::prebind(&proxy_profile)?;
        self.socks = Some(Socks5Frontend::activate_services(
            &proxy_profile,
            self.services(profile),
            socks,
        )?);
        self.http = Some(HttpProxyFrontend::activate_services(
            &proxy_profile,
            self.services(profile),
            http,
        )?);
        self.listeners = self
            .socks5_listeners()
            .iter()
            .chain(self.http_listeners())
            .copied()
            .collect();
        self.profile = profile.clone();
        Ok(())
    }
    pub fn activate_final(&mut self) {
        self.gate.refresh(Some(true));
    }
    pub fn quiesce_final(&mut self) {
        self.gate.refresh(Some(false));
    }
    pub fn cancel_immediately(&mut self) {
        self.quiesce_final();
        self.cancellation.cancel();
        if let Some(frontend) = &mut self.socks {
            frontend.cancel_immediately();
        }
        if let Some(frontend) = &mut self.http {
            frontend.cancel_immediately();
        }
    }
    pub async fn shutdown(&mut self) {
        self.cancel_immediately();
        self.monitor.take();
        if let Some(mut frontend) = self.socks.take() {
            frontend.shutdown().await;
        }
        if let Some(mut frontend) = self.http.take() {
            frontend.shutdown().await;
        }
    }
    pub async fn reconfigure_frontends(&mut self, profile: &Profile) -> Result<(), TransportError> {
        let active = self
            .gate
            .inner
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .active;
        self.quiesce_final();
        if let Some(mut frontend) = self.socks.take() {
            frontend.shutdown().await;
        }
        if let Some(mut frontend) = self.http.take() {
            frontend.shutdown().await;
        }
        self.listeners.clear();
        self.bind(profile)?;
        if active {
            self.activate_final();
        }
        Ok(())
    }
    pub fn listeners(&self) -> &[SocketAddr] {
        &self.listeners
    }
    pub fn socks5_listeners(&self) -> &[SocketAddr] {
        self.socks.as_ref().map_or(&[], Socks5Frontend::listeners)
    }
    pub fn http_listeners(&self) -> &[SocketAddr] {
        self.http.as_ref().map_or(&[], HttpProxyFrontend::listeners)
    }
    pub fn statistics(&self) -> TrafficSnapshot {
        self.counters.snapshot()
    }
    pub fn performance(&self) -> ProxyPerformanceSnapshot {
        let mut value = ProxyPerformanceSnapshot::default();
        if let Some(http) = &self.http {
            http.augment_performance(&mut value);
        }
        value
    }
    pub fn failure(&self) -> Option<String> {
        self.socks
            .as_ref()
            .and_then(Socks5Frontend::failure)
            .or_else(|| self.http.as_ref().and_then(HttpProxyFrontend::failure))
    }
}
impl Drop for SystemProxyRuntime {
    fn drop(&mut self) {
        self.cancel_immediately();
    }
}

struct SystemNetwork {
    gate: Arc<Gate>,
    authorizer: Arc<dyn SystemFlowAuthorizer>,
    counters: Arc<TrafficCounters>,
}
impl SystemNetwork {
    async fn addresses(
        &self,
        target: &TcpTarget,
        epoch: u64,
    ) -> Result<Vec<SocketAddr>, DialError> {
        self.gate.valid(epoch)?;
        let values = if let Some(address) = target.socket_address() {
            vec![address]
        } else {
            // The OS resolver is captured by the DNS provider. No configured
            // server or plaintext fallback is attempted by these frontends.
            tokio::net::lookup_host(target.host_port())
                .await
                .map_err(|_| DialError::Network)?
                .take(16)
                .collect()
        };
        self.gate.valid(epoch)?;
        if values.is_empty() {
            Err(DialError::Network)
        } else {
            Ok(values)
        }
    }
    async fn authorize(
        &self,
        target: &TcpTarget,
        local: SocketAddr,
        remote: SocketAddr,
        protocol: SystemFlowProtocol,
        epoch: u64,
    ) -> Result<(), DialError> {
        self.gate.valid(epoch)?;
        let intent = SystemFlowIntent {
            local,
            remote,
            hostname: target
                .socket_address()
                .is_none()
                .then(|| target.host_port().0.to_owned()),
            protocol,
            epoch,
        };
        // Do not log or interpolate the native response.
        self.authorizer
            .authorize(intent)
            .await
            .map_err(|_| DialError::Rejected(403))?;
        self.gate.valid(epoch)
    }
}
/// UDP connect selects an ordinary routed source address without emitting a
/// packet. This is not a physical-interface bypass or a VPN safety proof.
async fn routed_source(remote: SocketAddr) -> Result<IpAddr, DialError> {
    let any: SocketAddr = if remote.is_ipv4() {
        (Ipv4Addr::UNSPECIFIED, 0).into()
    } else {
        (Ipv6Addr::UNSPECIFIED, 0).into()
    };
    let probe = UdpSocket::bind(any).await.map_err(|_| DialError::Network)?;
    probe
        .connect(remote)
        .await
        .map_err(|_| DialError::Network)?;
    probe
        .local_addr()
        .map(|a| a.ip())
        .map_err(|_| DialError::Network)
}
#[async_trait]
impl TcpDialer for SystemNetwork {
    fn is_ready(&self) -> bool {
        self.gate.lease().is_ok()
    }
    fn session_generation(&self) -> Option<u64> {
        self.gate.lease().ok().map(|v| v.0)
    }
    async fn connect(
        &self,
        target: TcpTarget,
        deadline: Instant,
        cancel: &CancellationToken,
        _: FlowClass,
    ) -> Result<crate::tcp::TcpStream, DialError> {
        let (epoch, lost) = self.gate.lease()?;
        let operation = async {
            let addresses = self.addresses(&target, epoch).await?;
            let mut failure = DialError::Network;
            for remote in addresses {
                self.gate.valid(epoch)?;
                let source = routed_source(remote).await?;
                let socket = if remote.is_ipv4() {
                    TcpSocket::new_v4()
                } else {
                    TcpSocket::new_v6()
                }
                .map_err(|_| DialError::Network)?;
                socket
                    .bind(SocketAddr::new(source, 0))
                    .map_err(|_| DialError::Network)?;
                self.authorize(
                    &target,
                    socket.local_addr().map_err(|_| DialError::Network)?,
                    remote,
                    SystemFlowProtocol::Tcp,
                    epoch,
                )
                .await?;
                match socket.connect(remote).await {
                    Ok(stream) => {
                        self.gate.valid(epoch)?;
                        return Ok(Box::new(SystemTcp {
                            stream,
                            cancelled: Box::pin(lost.clone().cancelled_owned()),
                            epoch,
                            gate: self.gate.clone(),
                            counters: self.counters.clone(),
                        }) as crate::tcp::TcpStream);
                    }
                    Err(error) => {
                        failure = if error.kind() == io::ErrorKind::ConnectionRefused {
                            DialError::Refused
                        } else {
                            DialError::Network
                        };
                    }
                }
            }
            Err(failure)
        };
        tokio::select! { biased; _ = cancel.cancelled() => Err(DialError::Cancelled), _ = lost.cancelled() => Err(DialError::Closed), result = timeout_at(deadline, operation) => result.map_err(|_| DialError::Timeout)? }
    }
}
struct SystemTcp {
    stream: TcpStream,
    cancelled: Pin<Box<dyn Future<Output = ()> + Send>>,
    epoch: u64,
    gate: Arc<Gate>,
    counters: Arc<TrafficCounters>,
}
impl SystemTcp {
    fn check(&mut self, cx: &mut Context<'_>) -> io::Result<()> {
        self.gate.valid(self.epoch)?;
        if self.cancelled.as_mut().poll(cx).is_ready() {
            Err(DialError::Closed.into())
        } else {
            Ok(())
        }
    }
}
impl TcpIo for SystemTcp {
    fn local_addr(&self) -> io::Result<SocketAddr> {
        self.stream.local_addr()
    }
    fn session_generation(&self) -> Option<u64> {
        Some(self.epoch)
    }
}
impl AsyncRead for SystemTcp {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        if let Err(error) = self.check(cx) {
            return Poll::Ready(Err(error));
        }
        let before = buf.filled().len();
        let result = Pin::new(&mut self.stream).poll_read(cx, buf);
        if matches!(result, Poll::Ready(Ok(()))) {
            self.counters
                .record_received(buf.filled().len().saturating_sub(before));
        }
        result
    }
}
impl AsyncWrite for SystemTcp {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        if let Err(error) = self.check(cx) {
            return Poll::Ready(Err(error));
        }
        let result = Pin::new(&mut self.stream).poll_write(cx, buf);
        if let Poll::Ready(Ok(n)) = result {
            self.counters.record_sent(n);
        }
        result
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        if let Err(error) = self.check(cx) {
            return Poll::Ready(Err(error));
        }
        Pin::new(&mut self.stream).poll_flush(cx)
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.stream).poll_shutdown(cx)
    }
}

struct SystemUdp {
    network: Arc<SystemNetwork>,
    v4: Option<UdpSocket>,
    v6: Option<UdpSocket>,
    epoch: u64,
    lost: CancellationToken,
    cancel: CancellationToken,
    peers: Mutex<HashSet<SocketAddr>>,
}
#[async_trait]
impl UdpFactory for SystemNetwork {
    fn generation(&self) -> Option<u64> {
        self.gate.lease().ok().map(|v| v.0)
    }
    async fn open(
        &self,
        cancel: &CancellationToken,
        deadline: Instant,
    ) -> Result<Arc<dyn UdpAssociation>, DialError> {
        let (epoch, lost) = self.gate.lease()?;
        let path = self.gate.source.borrow().path();
        let operation = async {
            let v4 = if path.ipv4_available {
                Some(
                    UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0))
                        .await
                        .map_err(|_| DialError::Network)?,
                )
            } else {
                None
            };
            let v6 = if path.ipv6_available {
                Some(
                    UdpSocket::bind((Ipv6Addr::UNSPECIFIED, 0))
                        .await
                        .map_err(|_| DialError::Network)?,
                )
            } else {
                None
            };
            self.gate.valid(epoch)?;
            Ok(Arc::new(SystemUdp {
                network: Arc::new(SystemNetwork {
                    gate: self.gate.clone(),
                    authorizer: self.authorizer.clone(),
                    counters: self.counters.clone(),
                }),
                v4,
                v6,
                epoch,
                lost: lost.clone(),
                cancel: cancel.clone(),
                peers: Mutex::new(HashSet::new()),
            }) as Arc<dyn UdpAssociation>)
        };
        tokio::select! { biased; _ = cancel.cancelled() => Err(DialError::Cancelled), _ = lost.cancelled() => Err(DialError::Closed), result = timeout_at(deadline, operation) => result.map_err(|_| DialError::Timeout)? }
    }
}
impl SystemUdp {
    fn socket(&self, address: SocketAddr) -> Result<&UdpSocket, DialError> {
        if address.is_ipv4() {
            self.v4.as_ref()
        } else {
            self.v6.as_ref()
        }
        .ok_or(DialError::Network)
    }
    async fn receive(socket: &Option<UdpSocket>) -> Result<(SocketAddr, Bytes), DialError> {
        let Some(socket) = socket else {
            return std::future::pending().await;
        };
        let mut bytes = vec![0; 65535];
        let (size, remote) = socket
            .recv_from(&mut bytes)
            .await
            .map_err(|_| DialError::Network)?;
        bytes.truncate(size);
        Ok((remote, Bytes::from(bytes)))
    }
}
#[async_trait]
impl UdpAssociation for SystemUdp {
    fn accounts_traffic(&self) -> bool {
        false
    }
    async fn prepare(&self) -> Result<(), DialError> {
        self.network.gate.valid(self.epoch)
    }
    async fn send(&self, target: &TcpTarget, payload: &[u8]) -> Result<(), DialError> {
        let operation = async {
            let remote = self
                .network
                .addresses(target, self.epoch)
                .await?
                .into_iter()
                .find(|a| self.socket(*a).is_ok())
                .ok_or(DialError::Network)?;
            let socket = self.socket(remote)?;
            let source = routed_source(remote).await?;
            let local = SocketAddr::new(
                source,
                socket.local_addr().map_err(|_| DialError::Network)?.port(),
            );
            self.network
                .authorize(target, local, remote, SystemFlowProtocol::Udp, self.epoch)
                .await?;
            {
                let mut peers = self.peers.lock().unwrap_or_else(|p| p.into_inner());
                if peers.len() >= 1024 && !peers.contains(&remote) {
                    return Err(DialError::Budget);
                }
                peers.insert(remote);
            }
            socket
                .send_to(payload, remote)
                .await
                .map_err(|_| DialError::Network)?;
            Ok(())
        };
        tokio::select! { biased; _ = self.cancel.cancelled() => Err(DialError::Cancelled), _ = self.lost.cancelled() => Err(DialError::Closed), result = timeout_at(Instant::now() + Duration::from_secs(10), operation) => result.map_err(|_| DialError::Timeout)? }
    }
    async fn recv(&self) -> Result<(TcpTarget, Bytes), DialError> {
        loop {
            self.network.gate.valid(self.epoch)?;
            let (remote, bytes) = tokio::select! { biased; _ = self.cancel.cancelled() => return Err(DialError::Cancelled), _ = self.lost.cancelled() => return Err(DialError::Closed), result = Self::receive(&self.v4) => result?, result = Self::receive(&self.v6) => result? };
            self.network.gate.valid(self.epoch)?;
            if self
                .peers
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .contains(&remote)
            {
                return Ok((TcpTarget::address(remote), bytes));
            }
        }
    }
}

#[cfg(test)]
mod tests;
