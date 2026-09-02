use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::num::NonZeroU16;
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream, UdpSocket as TokioUdpSocket};
use tokio::sync::{Mutex, mpsc, watch};
use tokio::task::JoinHandle;
use tokio::time::{Instant, timeout};
use tokio_util::sync::CancellationToken;
use ts_netstack_smoltcp::CreateSocket;
use ts_netstack_smoltcp::netcore::Channel;
use ts_netstack_smoltcp::netsock::{TcpStream as StackTcpStream, UdpSocket as StackUdpSocket};
use usque_core::{OperatingMode, Profile, ProxyAuthCredentials};

use crate::dns::Resolver;
use crate::geo_direct::{
    GeoDirectPolicy, GeoRoute, GeoTarget, RoutedTcpStream, bind_protected_udp, connect_routed,
};
use crate::h2::{MasqueTlsIdentity, TransportError};
use crate::netstack::{
    PacketStack, ProxyPerformanceSnapshot, RuntimeHealth, RuntimePath, TrafficCounters,
    TrafficSnapshot,
};
use crate::pin_refresh::EndpointPinRefresher;
use crate::port_allocator::{next_tcp_port, next_udp_port};
use crate::socket::{
    DirectEgressLease, DirectProtocol, SocketProtector, noop_socket_protector, socket_handle,
};

const SOCKS_VERSION: u8 = 5;
const AUTH_NONE: u8 = 0;
const AUTH_USERPASS: u8 = 2;
const AUTH_UNACCEPTABLE: u8 = 0xff;
const USERPASS_VERSION: u8 = 1;
const USERPASS_SUCCESS: u8 = 0;
const USERPASS_FAILURE: u8 = 1;
const COMMAND_CONNECT: u8 = 1;
const COMMAND_UDP_ASSOCIATE: u8 = 3;
const ADDRESS_IPV4: u8 = 1;
const ADDRESS_DOMAIN: u8 = 3;
const ADDRESS_IPV6: u8 = 4;
const REPLY_SUCCEEDED: u8 = 0;
const REPLY_GENERAL_FAILURE: u8 = 1;
const REPLY_CONNECTION_NOT_ALLOWED: u8 = 2;
const REPLY_NETWORK_UNREACHABLE: u8 = 3;
const REPLY_HOST_UNREACHABLE: u8 = 4;
const REPLY_CONNECTION_REFUSED: u8 = 5;
const REPLY_COMMAND_UNSUPPORTED: u8 = 7;
const REPLY_ADDRESS_UNSUPPORTED: u8 = 8;
const REMOTE_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_TARGET_ADDRESSES: usize = 16;
const MAX_UDP_DATAGRAM: usize = 65_535;
const UDP_RESPONSE_CAPACITY: usize = 128;

pub struct Socks5Runtime {
    stack: PacketStack,
    frontend: Socks5Frontend,
}

pub(crate) struct Socks5Frontend {
    listener_tasks: Vec<JoinHandle<()>>,
    listeners: Vec<SocketAddr>,
    cancellation: CancellationToken,
    failure: watch::Receiver<Option<String>>,
}

impl Socks5Runtime {
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
        if profile.mode != OperatingMode::Socks5 {
            return Err(TransportError::UnsupportedOperatingMode);
        }
        if let Err(error) = profile.proxy.listener_credentials() {
            return Err(TransportError::Socks5(error.to_string()));
        }

        // Reserve every configured address before opening the remote session so
        // a partial listener set can never be reported as ready.
        let bound = Socks5Frontend::prebind(profile)?;

        let assigned_ipv4 = identity.assigned_ipv4;
        let assigned_ipv6 = identity.assigned_ipv6;
        let mut stack =
            PacketStack::start_with_refresh(profile, Arc::new(identity), protector, pin_refresher)
                .await?;
        let frontend =
            Socks5Frontend::activate(profile, assigned_ipv4, assigned_ipv6, &stack, bound)?;

        // Yield once so immediately-failed accept loops cannot be presented as
        // successfully started.
        tokio::task::yield_now().await;
        let startup_failure = stack.failure.borrow().clone();
        if let Some(message) = startup_failure {
            stack.shutdown().await;
            return Err(TransportError::Socks5(message));
        }

        Ok(Self { stack, frontend })
    }

    pub fn path(&self) -> RuntimePath {
        self.stack.path()
    }

    pub fn health(&self) -> RuntimeHealth {
        self.stack.health()
    }

    pub fn listeners(&self) -> &[SocketAddr] {
        self.frontend.listeners()
    }

    pub fn statistics(&self) -> TrafficSnapshot {
        self.stack.counters.snapshot()
    }

    pub fn performance(&self) -> ProxyPerformanceSnapshot {
        self.stack.performance()
    }

    pub fn network_quality(&self) -> crate::NetworkQualitySnapshot {
        self.stack.network_quality()
    }

    pub fn failure(&self) -> Option<String> {
        self.stack
            .failure
            .borrow()
            .clone()
            .or_else(|| self.frontend.failure())
    }

    pub fn cancel_immediately(&mut self) {
        self.stack.cancel_immediately();
        self.frontend.cancel_immediately();
    }

    pub async fn shutdown(&mut self) {
        self.cancel_immediately();
        self.frontend.shutdown().await;
        self.stack.shutdown().await;
    }
}

impl Drop for Socks5Runtime {
    fn drop(&mut self) {
        self.cancel_immediately();
    }
}

impl Socks5Frontend {
    pub(crate) fn prebind(profile: &Profile) -> Result<Vec<TcpListener>, TransportError> {
        crate::socket::bind_tcp_listeners(&profile.proxy.socks5_listeners)
            .map_err(|(address, source)| TransportError::SocksListener { address, source })
    }

    pub(crate) fn activate(
        profile: &Profile,
        assigned_ipv4: Ipv4Addr,
        assigned_ipv6: Ipv6Addr,
        stack: &PacketStack,
        bound: Vec<TcpListener>,
    ) -> Result<Self, TransportError> {
        let auth = match profile.proxy.listener_credentials() {
            Ok(credentials) => credentials.map(Arc::new),
            Err(error) => return Err(TransportError::Socks5(error.to_string())),
        };
        let cancellation = stack.cancellation.child_token();
        let (failure_tx, failure) = watch::channel(None);
        let dns_servers = if profile.proxy.dns_mode == usque_core::ProxyDnsMode::LocalConfigured {
            profile.proxy.dns_servers.clone()
        } else {
            profile.dns_servers.clone()
        };
        let resolver = Resolver::new(
            stack.channel.clone(),
            assigned_ipv4,
            assigned_ipv6,
            dns_servers,
            profile.proxy.dns_mode,
            Arc::clone(&stack.protector),
        );
        let context = Arc::new(SocksContext {
            channel: stack.channel.clone(),
            resolver,
            protector: Arc::clone(&stack.protector),
            geo_policy: Arc::clone(&stack.geo_policy),
            counters: Arc::clone(&stack.counters),
            assigned_ipv4,
            assigned_ipv6,
            udp_idle_timeout: Duration::from_secs(u64::from(
                profile.proxy.udp_idle_timeout_seconds.max(1),
            )),
            cancellation: cancellation.clone(),
            failure: failure_tx,
            health: stack.subscribe_health(),
            auth,
        });
        let listeners = bound
            .iter()
            .filter_map(|listener| listener.local_addr().ok())
            .collect::<Vec<_>>();
        let listener_tasks = bound
            .into_iter()
            .map(|listener| {
                let context = Arc::clone(&context);
                tokio::spawn(async move {
                    run_listener(listener, context).await;
                })
            })
            .collect();
        Ok(Self {
            listener_tasks,
            listeners,
            cancellation,
            failure,
        })
    }

    pub(crate) fn listeners(&self) -> &[SocketAddr] {
        &self.listeners
    }

    pub(crate) fn failure(&self) -> Option<String> {
        self.failure.borrow().clone()
    }

    pub(crate) fn cancel_immediately(&mut self) {
        self.cancellation.cancel();
        for task in &self.listener_tasks {
            task.abort();
        }
    }

    pub(crate) async fn shutdown(&mut self) {
        self.cancel_immediately();
        for task in self.listener_tasks.drain(..) {
            let _ = task.await;
        }
    }
}

impl Drop for Socks5Frontend {
    fn drop(&mut self) {
        self.cancel_immediately();
    }
}

struct SocksContext {
    channel: Channel,
    resolver: Resolver,
    protector: Arc<dyn SocketProtector>,
    geo_policy: Arc<GeoDirectPolicy>,
    counters: Arc<TrafficCounters>,
    assigned_ipv4: Ipv4Addr,
    assigned_ipv6: Ipv6Addr,
    udp_idle_timeout: Duration,
    cancellation: tokio_util::sync::CancellationToken,
    failure: watch::Sender<Option<String>>,
    health: watch::Receiver<RuntimeHealth>,
    auth: Option<Arc<ProxyAuthCredentials>>,
}

async fn run_listener(listener: TcpListener, context: Arc<SocksContext>) {
    loop {
        let accepted = tokio::select! {
            _ = context.cancellation.cancelled() => break,
            accepted = listener.accept() => accepted,
        };
        let (stream, peer) = match accepted {
            Ok(value) => value,
            Err(error) => {
                tracing::error!(%error, "SOCKS5 listener stopped");
                if !context.cancellation.is_cancelled() && context.failure.borrow().is_none() {
                    let _ = context
                        .failure
                        .send(Some(format!("SOCKS5 listener failed: {error}")));
                }
                break;
            }
        };
        if let Err(error) = stream.set_nodelay(true) {
            tracing::debug!(%peer, %error, "could not disable Nagle on SOCKS5 client socket");
        }
        if !peer.ip().is_loopback()
            && stream
                .local_addr()
                .is_ok_and(|addr| addr.ip().is_loopback())
        {
            tracing::warn!(%peer, "rejected non-loopback peer on a loopback SOCKS5 listener");
            continue;
        }
        let connection_context = Arc::clone(&context);
        tokio::spawn(async move {
            if let Err(error) = serve_client(stream, peer, connection_context).await {
                tracing::debug!(%peer, %error, "SOCKS5 session ended");
            }
        });
    }
}

async fn serve_client(
    mut client: TcpStream,
    peer: SocketAddr,
    context: Arc<SocksContext>,
) -> Result<(), TransportError> {
    negotiate_auth(&mut client, context.auth.as_deref()).await?;
    let request = read_request(&mut client).await?;
    if !matches!(&*context.health.borrow(), RuntimeHealth::Connected { .. }) {
        send_reply(
            &mut client,
            REPLY_NETWORK_UNREACHABLE,
            SocketAddr::from(([0, 0, 0, 0], 0)),
        )
        .await?;
        return Ok(());
    }
    match request.command {
        COMMAND_CONNECT => serve_connect(client, context, request).await,
        COMMAND_UDP_ASSOCIATE => serve_udp_association(client, peer, context, request).await,
        _ => {
            send_reply(
                &mut client,
                REPLY_COMMAND_UNSUPPORTED,
                SocketAddr::from(([0, 0, 0, 0], 0)),
            )
            .await?;
            Ok(())
        }
    }
}

async fn serve_connect(
    mut client: TcpStream,
    context: Arc<SocksContext>,
    request: SocksRequest,
) -> Result<(), TransportError> {
    if request.port == 0 {
        send_reply(
            &mut client,
            REPLY_ADDRESS_UNSUPPORTED,
            SocketAddr::from(([0, 0, 0, 0], 0)),
        )
        .await?;
        return Err(TransportError::Socks5(
            "SOCKS5 CONNECT target port cannot be zero".to_owned(),
        ));
    }
    let mut remote = match connect_remote(&context, &request.target, request.port).await {
        Ok(remote) => remote,
        Err(error) => {
            send_reply(
                &mut client,
                error.reply,
                SocketAddr::from(([0, 0, 0, 0], 0)),
            )
            .await?;
            return Err(TransportError::Socks5(error.message));
        }
    };

    send_reply(&mut client, REPLY_SUCCEEDED, remote.local_addr()?).await?;
    tokio::select! {
        _ = context.cancellation.cancelled() => Ok(()),
        result = crate::relay::copy_bidirectional(&mut client, &mut remote) => {
            result
                .map(|_| ())
                .map_err(|error| TransportError::Socks5(error.to_string()))
        }
    }
}

async fn serve_udp_association(
    mut control: TcpStream,
    peer: SocketAddr,
    context: Arc<SocksContext>,
    request: SocksRequest,
) -> Result<(), TransportError> {
    let requested_ip = match request.target {
        Target::Address(address) if !address.is_unspecified() => Some(address),
        Target::Address(_) | Target::Domain(_) => None,
    };
    if requested_ip.is_some_and(|address| address != peer.ip()) {
        send_reply(
            &mut control,
            REPLY_CONNECTION_NOT_ALLOWED,
            unspecified_for(peer),
        )
        .await?;
        return Err(TransportError::Socks5(
            "UDP ASSOCIATE address does not match the TCP client".to_owned(),
        ));
    }

    let relay_ip = control.local_addr()?.ip();
    let relay = Arc::new(TokioUdpSocket::bind(SocketAddr::new(relay_ip, 0)).await?);
    let relay_address = relay.local_addr()?;
    send_reply(&mut control, REPLY_SUCCEEDED, relay_address).await?;

    let association_cancel = CancellationToken::new();
    let (response_tx, mut response_rx) = mpsc::channel(UDP_RESPONSE_CAPACITY);
    let mut response_tasks = Vec::with_capacity(4);
    let v4_socket = Arc::new(
        context
            .channel
            .udp_bind(SocketAddr::new(
                IpAddr::V4(context.assigned_ipv4),
                next_udp_port(),
            ))
            .await
            .map_err(|error| TransportError::Socks5(format!("bind tunnel UDP/IPv4: {error}")))?,
    );
    response_tasks.push(spawn_udp_receiver(
        Arc::clone(&v4_socket),
        response_tx.clone(),
        association_cancel.clone(),
        context.cancellation.clone(),
    ));
    let v6_socket = Arc::new(
        context
            .channel
            .udp_bind(SocketAddr::new(
                IpAddr::V6(context.assigned_ipv6),
                next_udp_port(),
            ))
            .await
            .map_err(|error| TransportError::Socks5(format!("bind tunnel UDP/IPv6: {error}")))?,
    );
    response_tasks.push(spawn_udp_receiver(
        Arc::clone(&v6_socket),
        response_tx.clone(),
        association_cancel.clone(),
        context.cancellation.clone(),
    ));
    let direct_udp = if context.geo_policy.is_enabled() {
        DirectUdpSockets::new(context.protector.as_ref())
    } else {
        DirectUdpSockets::default()
    };
    if let Some(socket) = &direct_udp.v4 {
        response_tasks.push(spawn_direct_udp_receiver(
            Arc::clone(socket),
            response_tx.clone(),
            association_cancel.clone(),
            context.cancellation.clone(),
            Arc::clone(&context.counters),
        ));
    }
    if let Some(socket) = &direct_udp.v6 {
        response_tasks.push(spawn_direct_udp_receiver(
            Arc::clone(socket),
            response_tx,
            association_cancel.clone(),
            context.cancellation.clone(),
            Arc::clone(&context.counters),
        ));
    }

    let requested_port = NonZeroU16::new(request.port);
    let mut client_endpoint = requested_port.map(|port| SocketAddr::new(peer.ip(), port.get()));
    let mut datagram = vec![0u8; MAX_UDP_DATAGRAM];
    let idle = tokio::time::sleep(context.udp_idle_timeout);
    tokio::pin!(idle);
    let result = loop {
        tokio::select! {
            _ = context.cancellation.cancelled() => break Ok(()),
            _ = &mut idle => break Ok(()),
            control_result = control.read_u8() => {
                match control_result {
                    Ok(_) => {
                        break Err(TransportError::Socks5(
                            "unexpected data on UDP ASSOCIATE control connection".to_owned(),
                        ));
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => break Ok(()),
                    Err(error) => break Err(TransportError::Io(error)),
                }
            }
            received = relay.recv_from(&mut datagram) => {
                let (length, source) = match received {
                    Ok(value) => value,
                    Err(error) => break Err(TransportError::Io(error)),
                };
                if source.ip() != peer.ip()
                    || requested_port.is_some_and(|port| source.port() != port.get())
                {
                    tracing::warn!(%source, %peer, "rejected UDP datagram outside its SOCKS5 association");
                    continue;
                }
                let parsed = match decode_udp_request(&datagram[..length]) {
                    Ok(parsed) => parsed,
                    Err(error) => {
                        tracing::debug!(%source, %error, "discarded malformed SOCKS5 UDP datagram");
                        continue;
                    }
                };
                if let Err(error) = send_udp_routed(
                    &context,
                    &parsed.target,
                    parsed.port,
                    parsed.payload,
                    &direct_udp,
                    TunnelUdpSockets {
                        v4: &v4_socket,
                        v6: &v6_socket,
                    },
                ).await {
                    tracing::debug!(%error, "SOCKS5 UDP send failed");
                    continue;
                }
                client_endpoint.get_or_insert(source);
                idle.as_mut().reset(Instant::now() + context.udp_idle_timeout);
            }
            response = response_rx.recv() => {
                let Some(response) = response else {
                    break Err(TransportError::Socks5(
                        "all SOCKS5 UDP tunnel receivers stopped".to_owned(),
                    ));
                };
                let response = match response {
                    Ok(response) => response,
                    Err(error) => break Err(TransportError::Socks5(error)),
                };
                let Some(client_endpoint) = client_endpoint else {
                    continue;
                };
                let packet = encode_udp_response(response.source, &response.payload);
                if let Err(error) = relay.send_to(&packet, client_endpoint).await {
                    break Err(TransportError::Io(error));
                }
                idle.as_mut().reset(Instant::now() + context.udp_idle_timeout);
            }
        }
    };

    association_cancel.cancel();
    for task in response_tasks {
        let _ = task.await;
    }
    result
}

struct UdpResponse {
    source: SocketAddr,
    payload: bytes::Bytes,
}

#[derive(Default)]
struct DirectUdpSockets {
    v4: Option<Arc<TokioUdpSocket>>,
    v6: Option<Arc<TokioUdpSocket>>,
    leases: Mutex<HashMap<(Option<u64>, SocketAddr), DirectEgressLease>>,
}

#[derive(Clone, Copy)]
struct TunnelUdpSockets<'a> {
    v4: &'a StackUdpSocket,
    v6: &'a StackUdpSocket,
}

impl DirectUdpSockets {
    fn new(protector: &dyn SocketProtector) -> Self {
        Self {
            v4: bind_protected_udp(protector, false)
                .map(Arc::new)
                .map_err(|error| {
                    tracing::debug!(%error, "protected direct UDP/IPv4 socket unavailable");
                })
                .ok(),
            v6: bind_protected_udp(protector, true)
                .map(Arc::new)
                .map_err(|error| {
                    tracing::debug!(%error, "protected direct UDP/IPv6 socket unavailable");
                })
                .ok(),
            leases: Mutex::new(HashMap::new()),
        }
    }

    fn for_address(&self, address: SocketAddr) -> Option<&Arc<TokioUdpSocket>> {
        if address.is_ipv4() {
            self.v4.as_ref()
        } else {
            self.v6.as_ref()
        }
    }

    async fn ensure_target(
        &self,
        protector: &dyn SocketProtector,
        remote: SocketAddr,
    ) -> Result<(), String> {
        let socket = self
            .for_address(remote)
            .ok_or_else(|| format!("{remote}: protected socket unavailable"))?;
        let generation = protector.network_generation();
        {
            let mut leases = self.leases.lock().await;
            leases.retain(|(existing_generation, _), _| *existing_generation == generation);
            if leases.contains_key(&(generation, remote)) {
                return Ok(());
            }
            if leases.len() >= 1024 {
                return Err("direct UDP target lease limit reached".to_owned());
            }
        }
        let lease = protector
            .protect_for_target(socket_handle(socket.as_ref()), remote, DirectProtocol::Udp)
            .await
            .map_err(|error| format!("protect direct UDP target {remote}: {error}"))?;
        let mut leases = self.leases.lock().await;
        leases.retain(|(existing_generation, _), _| *existing_generation == generation);
        leases.entry((generation, remote)).or_insert(lease);
        Ok(())
    }
}

async fn send_udp_routed(
    context: &SocksContext,
    target: &Target,
    port: u16,
    payload: &[u8],
    direct: &DirectUdpSockets,
    tunnel: TunnelUdpSockets<'_>,
) -> Result<(), String> {
    let mut resolved_for_tunnel = None;
    let geo_target = match target {
        Target::Address(address) => GeoTarget::Ip(*address),
        Target::Domain(name) => GeoTarget::Host(name),
    };
    if geo_target.route(&context.geo_policy) == GeoRoute::Direct {
        let addresses = match target {
            Target::Address(address) => Ok(vec![SocketAddr::new(*address, port)]),
            Target::Domain(name) => context.protector.resolve_direct(name, port).await,
        };
        match addresses {
            Ok(addresses) => {
                if context.protector.direct_dns_resolver().is_some() {
                    resolved_for_tunnel = Some(
                        addresses
                            .iter()
                            .map(|address| address.ip())
                            .collect::<Vec<_>>(),
                    );
                }
                let mut failures = Vec::new();
                for address in addresses.into_iter().take(MAX_TARGET_ADDRESSES) {
                    let remote = SocketAddr::new(address.ip(), port);
                    if remote.ip().is_unspecified() || remote.ip().is_multicast() {
                        failures.push(format!("{remote}: unusable address"));
                        continue;
                    }
                    let Some(socket) = direct.for_address(remote) else {
                        failures.push(format!("{remote}: protected socket unavailable"));
                        continue;
                    };
                    if let Err(error) = direct
                        .ensure_target(context.protector.as_ref(), remote)
                        .await
                    {
                        failures.push(format!("{remote}: {error}"));
                        continue;
                    }
                    match socket.send_to(payload, remote).await {
                        Ok(written) if written == payload.len() => {
                            context.counters.record_sent(written);
                            return Ok(());
                        }
                        Ok(written) => failures.push(format!(
                            "{remote}: wrote {written} of {} bytes",
                            payload.len()
                        )),
                        Err(error) => failures.push(format!("{remote}: {error}")),
                    }
                }
                if !failures.is_empty() {
                    tracing::debug!(
                        reason_code = "direct_send_failed",
                        "GEO direct UDP send failed; falling back to tunnel"
                    );
                }
            }
            Err(_) => {
                if context.protector.direct_dns_resolver().is_some() {
                    return Err("encrypted_direct_dns_failed".to_owned());
                }
                tracing::debug!(
                    reason_code = "direct_resolution_failed",
                    "GEO direct UDP resolution failed; falling back to tunnel"
                );
            }
        }
    }

    let addresses = if let Some(addresses) = resolved_for_tunnel {
        addresses
    } else {
        match target {
            Target::Address(address) => vec![*address],
            Target::Domain(name) => context
                .resolver
                .resolve(name)
                .await
                .map_err(|error| error.to_string())?,
        }
    };
    let remote = addresses
        .into_iter()
        .map(|address| SocketAddr::new(address, port))
        .next()
        .ok_or_else(|| "target has no usable address".to_owned())?;
    let socket = if remote.is_ipv4() {
        tunnel.v4
    } else {
        tunnel.v6
    };
    socket
        .send_to(remote, payload)
        .await
        .map_err(|error| format!("tunnel send to {remote}: {error}"))
}

fn spawn_udp_receiver(
    socket: Arc<StackUdpSocket>,
    sender: mpsc::Sender<Result<UdpResponse, String>>,
    association_cancel: CancellationToken,
    runtime_cancel: CancellationToken,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            let received = tokio::select! {
                _ = association_cancel.cancelled() => break,
                _ = runtime_cancel.cancelled() => break,
                received = socket.recv_from_bytes() => received,
            };
            let message = match received {
                Ok((source, payload)) => Ok(UdpResponse { source, payload }),
                Err(error) => Err(format!("tunnel UDP receive failed: {error}")),
            };
            let failed = message.is_err();
            if sender.send(message).await.is_err() || failed {
                break;
            }
        }
    })
}

fn spawn_direct_udp_receiver(
    socket: Arc<TokioUdpSocket>,
    sender: mpsc::Sender<Result<UdpResponse, String>>,
    association_cancel: CancellationToken,
    runtime_cancel: CancellationToken,
    counters: Arc<TrafficCounters>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut buffer = vec![0u8; MAX_UDP_DATAGRAM];
        loop {
            let received = tokio::select! {
                _ = association_cancel.cancelled() => break,
                _ = runtime_cancel.cancelled() => break,
                received = socket.recv_from(&mut buffer) => received,
            };
            let message = match received {
                Ok((length, source)) => {
                    counters.record_received(length);
                    Ok(UdpResponse {
                        source,
                        payload: bytes::Bytes::copy_from_slice(&buffer[..length]),
                    })
                }
                Err(error) => Err(format!("direct UDP receive failed: {error}")),
            };
            let failed = message.is_err();
            if sender.send(message).await.is_err() || failed {
                break;
            }
        }
    })
}

#[derive(Debug)]
struct SocksUdpRequest<'a> {
    target: Target,
    port: u16,
    payload: &'a [u8],
}

fn decode_udp_request(packet: &[u8]) -> Result<SocksUdpRequest<'_>, &'static str> {
    if packet.len() < 4 || packet[0] != 0 || packet[1] != 0 {
        return Err("invalid reserved field");
    }
    if packet[2] != 0 {
        return Err("fragmented SOCKS5 UDP datagrams are unsupported");
    }
    let mut offset = 4;
    let target = match packet[3] {
        ADDRESS_IPV4 => {
            let octets = packet
                .get(offset..offset + 4)
                .ok_or("truncated IPv4 address")?;
            offset += 4;
            Target::Address(IpAddr::V4(Ipv4Addr::new(
                octets[0], octets[1], octets[2], octets[3],
            )))
        }
        ADDRESS_IPV6 => {
            let octets: [u8; 16] = packet
                .get(offset..offset + 16)
                .ok_or("truncated IPv6 address")?
                .try_into()
                .map_err(|_| "invalid IPv6 address")?;
            offset += 16;
            Target::Address(IpAddr::V6(Ipv6Addr::from(octets)))
        }
        ADDRESS_DOMAIN => {
            let length = usize::from(*packet.get(offset).ok_or("missing domain length")?);
            offset += 1;
            if length == 0 {
                return Err("empty domain");
            }
            let name = std::str::from_utf8(
                packet
                    .get(offset..offset + length)
                    .ok_or("truncated domain")?,
            )
            .map_err(|_| "non-UTF-8 domain")?
            .to_owned();
            offset += length;
            Target::Domain(name)
        }
        _ => return Err("unsupported address type"),
    };
    let port_bytes = packet
        .get(offset..offset + 2)
        .ok_or("missing target port")?;
    let port = u16::from_be_bytes([port_bytes[0], port_bytes[1]]);
    if port == 0 {
        return Err("target port is zero");
    }
    offset += 2;
    let payload = packet.get(offset..).ok_or("missing payload")?;
    Ok(SocksUdpRequest {
        target,
        port,
        payload,
    })
}

fn encode_udp_response(source: SocketAddr, payload: &[u8]) -> Vec<u8> {
    let mut packet = Vec::with_capacity(payload.len() + 22);
    packet.extend_from_slice(&[0, 0, 0]);
    match source.ip() {
        IpAddr::V4(address) => {
            packet.push(ADDRESS_IPV4);
            packet.extend_from_slice(&address.octets());
        }
        IpAddr::V6(address) => {
            packet.push(ADDRESS_IPV6);
            packet.extend_from_slice(&address.octets());
        }
    }
    packet.extend_from_slice(&source.port().to_be_bytes());
    packet.extend_from_slice(payload);
    packet
}

fn unspecified_for(peer: SocketAddr) -> SocketAddr {
    if peer.is_ipv6() {
        SocketAddr::new(Ipv6Addr::UNSPECIFIED.into(), 0)
    } else {
        SocketAddr::new(Ipv4Addr::UNSPECIFIED.into(), 0)
    }
}

async fn negotiate_auth<S>(
    client: &mut S,
    credentials: Option<&ProxyAuthCredentials>,
) -> Result<(), TransportError>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let version = client.read_u8().await?;
    let method_count = usize::from(client.read_u8().await?);
    if version != SOCKS_VERSION || method_count == 0 {
        return Err(TransportError::Socks5("invalid SOCKS5 greeting".to_owned()));
    }
    let mut methods = vec![0u8; method_count];
    client.read_exact(&mut methods).await?;
    match credentials {
        None => {
            let selected = if methods.contains(&AUTH_NONE) {
                AUTH_NONE
            } else {
                AUTH_UNACCEPTABLE
            };
            client.write_all(&[SOCKS_VERSION, selected]).await?;
            if selected == AUTH_UNACCEPTABLE {
                return Err(TransportError::Socks5(
                    "the client did not offer no-auth SOCKS5".to_owned(),
                ));
            }
            Ok(())
        }
        Some(expected) => {
            let selected = if methods.contains(&AUTH_USERPASS) {
                AUTH_USERPASS
            } else {
                AUTH_UNACCEPTABLE
            };
            client.write_all(&[SOCKS_VERSION, selected]).await?;
            if selected == AUTH_UNACCEPTABLE {
                return Err(TransportError::Socks5(
                    "the client did not offer username/password SOCKS5".to_owned(),
                ));
            }
            negotiate_userpass(client, expected).await
        }
    }
}

async fn negotiate_userpass<S>(
    client: &mut S,
    expected: &ProxyAuthCredentials,
) -> Result<(), TransportError>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let version = client.read_u8().await?;
    let username_len = usize::from(client.read_u8().await?);
    if version != USERPASS_VERSION || username_len == 0 {
        client
            .write_all(&[USERPASS_VERSION, USERPASS_FAILURE])
            .await?;
        return Err(TransportError::Socks5(
            "invalid SOCKS5 username/password request".to_owned(),
        ));
    }
    let mut username = vec![0u8; username_len];
    client.read_exact(&mut username).await?;
    let password_len = usize::from(client.read_u8().await?);
    if password_len == 0 {
        username.fill(0);
        client
            .write_all(&[USERPASS_VERSION, USERPASS_FAILURE])
            .await?;
        return Err(TransportError::Socks5(
            "invalid SOCKS5 username/password request".to_owned(),
        ));
    }
    let mut password = vec![0u8; password_len];
    client.read_exact(&mut password).await?;
    let accepted = expected.matches(&username, &password);
    username.fill(0);
    password.fill(0);
    let status = if accepted {
        USERPASS_SUCCESS
    } else {
        USERPASS_FAILURE
    };
    client.write_all(&[USERPASS_VERSION, status]).await?;
    if accepted {
        Ok(())
    } else {
        Err(TransportError::Socks5(
            "SOCKS5 username/password authentication failed".to_owned(),
        ))
    }
}

struct SocksRequest {
    command: u8,
    target: Target,
    port: u16,
}

#[derive(Debug)]
enum Target {
    Address(IpAddr),
    Domain(String),
}

async fn read_request(client: &mut TcpStream) -> Result<SocksRequest, TransportError> {
    let version = client.read_u8().await?;
    let command = client.read_u8().await?;
    let reserved = client.read_u8().await?;
    let address_type = client.read_u8().await?;
    if version != SOCKS_VERSION || reserved != 0 {
        return Err(TransportError::Socks5(
            "invalid SOCKS5 request header".to_owned(),
        ));
    }
    let target = match address_type {
        ADDRESS_IPV4 => {
            let mut octets = [0u8; 4];
            client.read_exact(&mut octets).await?;
            Target::Address(IpAddr::V4(Ipv4Addr::from(octets)))
        }
        ADDRESS_IPV6 => {
            let mut octets = [0u8; 16];
            client.read_exact(&mut octets).await?;
            Target::Address(IpAddr::V6(Ipv6Addr::from(octets)))
        }
        ADDRESS_DOMAIN => {
            let length = usize::from(client.read_u8().await?);
            if length == 0 {
                send_reply(
                    client,
                    REPLY_ADDRESS_UNSUPPORTED,
                    SocketAddr::from(([0, 0, 0, 0], 0)),
                )
                .await?;
                return Err(TransportError::Socks5(
                    "empty SOCKS5 domain name".to_owned(),
                ));
            }
            let mut bytes = vec![0u8; length];
            client.read_exact(&mut bytes).await?;
            let name = String::from_utf8(bytes)
                .map_err(|_| TransportError::Socks5("non-UTF-8 domain name".to_owned()))?;
            Target::Domain(name)
        }
        _ => {
            send_reply(
                client,
                REPLY_ADDRESS_UNSUPPORTED,
                SocketAddr::from(([0, 0, 0, 0], 0)),
            )
            .await?;
            return Err(TransportError::Socks5(
                "unsupported SOCKS5 address type".to_owned(),
            ));
        }
    };
    let port = client.read_u16().await?;
    Ok(SocksRequest {
        command,
        target,
        port,
    })
}

struct ConnectFailure {
    reply: u8,
    message: String,
}

async fn connect_remote(
    context: &SocksContext,
    target: &Target,
    port: u16,
) -> Result<RoutedTcpStream, ConnectFailure> {
    let geo_target = match target {
        Target::Address(address) => GeoTarget::Ip(*address),
        Target::Domain(name) => GeoTarget::Host(name),
    };
    connect_routed(
        &context.geo_policy,
        context.protector.as_ref(),
        Arc::clone(&context.counters),
        (geo_target, port),
        || ConnectFailure {
            reply: REPLY_HOST_UNREACHABLE,
            message: "encrypted_direct_dns_failed".to_owned(),
        },
        |resolved| async {
            let addresses =
                if let Some(addresses) = resolved {
                    addresses
                } else {
                    match target {
                        Target::Address(address) => vec![*address],
                        Target::Domain(name) => {
                            context.resolver.resolve(name).await.map_err(|error| {
                                ConnectFailure {
                                    reply: REPLY_HOST_UNREACHABLE,
                                    message: error.to_string(),
                                }
                            })?
                        }
                    }
                };
            connect_tunnel_remote(context, &addresses, port).await
        },
    )
    .await
}

async fn connect_tunnel_remote(
    context: &SocksContext,
    addresses: &[IpAddr],
    port: u16,
) -> Result<StackTcpStream, ConnectFailure> {
    let mut failures = Vec::new();
    for address in addresses.iter().take(MAX_TARGET_ADDRESSES) {
        let local_ip = match address {
            IpAddr::V4(_) => IpAddr::V4(context.assigned_ipv4),
            IpAddr::V6(_) => IpAddr::V6(context.assigned_ipv6),
        };
        let local = SocketAddr::new(local_ip, next_tcp_port());
        let remote = SocketAddr::new(*address, port);
        match timeout(
            REMOTE_CONNECT_TIMEOUT,
            context.channel.tcp_connect(local, remote),
        )
        .await
        {
            Ok(Ok(stream)) => return Ok(stream),
            Ok(Err(error)) if error.is_tcp_buffer_budget_exhausted() => {
                return Err(ConnectFailure {
                    reply: REPLY_GENERAL_FAILURE,
                    message: "the proxy connection memory budget is temporarily exhausted"
                        .to_owned(),
                });
            }
            Ok(Err(error)) => failures.push(format!("{remote}: {error}")),
            Err(_) => failures.push(format!("{remote}: timed out")),
        }
    }
    let reply = if failures.iter().any(|value| {
        value.to_ascii_lowercase().contains("refused")
            || value.to_ascii_lowercase().contains("reset")
    }) {
        REPLY_CONNECTION_REFUSED
    } else if addresses.is_empty() {
        REPLY_HOST_UNREACHABLE
    } else {
        REPLY_NETWORK_UNREACHABLE
    };
    Err(ConnectFailure {
        reply,
        message: if failures.is_empty() {
            "no usable target address".to_owned()
        } else {
            failures.join("; ")
        },
    })
}

async fn send_reply(
    client: &mut TcpStream,
    reply: u8,
    address: SocketAddr,
) -> Result<(), TransportError> {
    let mut response = Vec::with_capacity(22);
    response.extend_from_slice(&[SOCKS_VERSION, reply, 0]);
    match address.ip() {
        IpAddr::V4(ip) => {
            response.push(ADDRESS_IPV4);
            response.extend_from_slice(&ip.octets());
        }
        IpAddr::V6(ip) => {
            response.push(ADDRESS_IPV6);
            response.extend_from_slice(&ip.octets());
        }
    }
    response.extend_from_slice(&address.port().to_be_bytes());
    client.write_all(&response).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use ts_netstack_smoltcp::netcore::{Config, HasChannel, NetstackControl};

    use super::*;
    use crate::geo_direct::GeoDirectClassifier;
    use crate::socket::SocketHandle;

    struct TestGeoClassifier;

    impl GeoDirectClassifier for TestGeoClassifier {
        fn host_matches(&self, host: &str, country: &usque_geo::CountryCode) -> bool {
            host == "direct.test" && country.as_str() == "CN"
        }

        fn ip_matches(&self, ip: IpAddr, country: &usque_geo::CountryCode) -> bool {
            ip == Ipv4Addr::new(10, 0, 0, 2) && country.as_str() == "CN"
        }
    }

    struct TestProtector {
        resolved: SocketAddr,
        reject: bool,
        protect_calls: AtomicUsize,
        resolve_calls: AtomicUsize,
    }

    impl SocketProtector for TestProtector {
        fn protect(&self, _socket: SocketHandle) -> Result<(), String> {
            self.protect_calls.fetch_add(1, Ordering::SeqCst);
            if self.reject {
                Err("test rejection".to_owned())
            } else {
                Ok(())
            }
        }

        fn resolve(&self, host: &str, port: u16) -> Result<Vec<SocketAddr>, String> {
            self.resolve_calls.fetch_add(1, Ordering::SeqCst);
            if host != "direct.test" || port != self.resolved.port() {
                return Err("unexpected test resolution".to_owned());
            }
            Ok(vec![self.resolved])
        }
    }

    fn test_geo_policy() -> Arc<GeoDirectPolicy> {
        Arc::new(GeoDirectPolicy::with_classifier(
            Arc::new(TestGeoClassifier),
            [usque_geo::CountryCode::parse("CN").unwrap()],
        ))
    }

    async fn test_socks_context(
        protector: Arc<dyn SocketProtector>,
    ) -> (
        SocksContext,
        Arc<StackUdpSocket>,
        Channel,
        Vec<JoinHandle<()>>,
    ) {
        let (client_stack, server_stack) = ts_netstack_smoltcp::piped_pair(Config::default());
        let channel = client_stack.command_channel();
        let server_channel = server_stack.command_channel();
        let tasks = vec![client_stack.spawn_tokio(), server_stack.spawn_tokio()];
        let assigned_ipv4 = Ipv4Addr::new(10, 0, 0, 1);
        channel.set_ips([IpAddr::V4(assigned_ipv4)]).await.unwrap();
        server_channel
            .set_ips([IpAddr::V4(Ipv4Addr::new(10, 0, 0, 2))])
            .await
            .unwrap();
        let tunnel = Arc::new(
            channel
                .udp_bind(SocketAddr::new(IpAddr::V4(assigned_ipv4), 49_152))
                .await
                .unwrap(),
        );
        let (failure, _) = watch::channel(None);
        let (_, health) = watch::channel(RuntimeHealth::Connected {
            path: RuntimePath {
                transport: usque_core::Transport::Http3,
                endpoint_family: usque_core::AddressFamily::Ipv4,
                ipv4_available: true,
                ipv6_available: true,
            },
            reconnect_count: 0,
        });
        let context = SocksContext {
            channel: channel.clone(),
            resolver: Resolver::new(
                channel,
                assigned_ipv4,
                Ipv6Addr::LOCALHOST,
                Vec::new(),
                usque_core::ProxyDnsMode::Remote,
                Arc::clone(&protector),
            ),
            protector,
            geo_policy: test_geo_policy(),
            counters: Arc::new(TrafficCounters::default()),
            assigned_ipv4,
            assigned_ipv6: Ipv6Addr::LOCALHOST,
            udp_idle_timeout: Duration::from_secs(10),
            cancellation: CancellationToken::new(),
            failure,
            health,
            auth: None,
        };
        (context, tunnel, server_channel, tasks)
    }

    #[test]
    fn ephemeral_port_allocator_stays_in_dynamic_range() {
        for _ in 0..100 {
            assert!((49_152..=65_534).contains(&next_tcp_port()));
            assert!((49_152..=65_534).contains(&next_udp_port()));
        }
    }

    #[test]
    fn udp_request_codec_supports_all_address_types() {
        let ipv4 = [0, 0, 0, ADDRESS_IPV4, 1, 1, 1, 1, 0, 53, 0xaa];
        let parsed = decode_udp_request(&ipv4).unwrap();
        assert!(matches!(
            parsed.target,
            Target::Address(IpAddr::V4(address)) if address == Ipv4Addr::new(1, 1, 1, 1)
        ));
        assert_eq!(parsed.port, 53);
        assert_eq!(parsed.payload, &[0xaa]);

        let mut domain = vec![0, 0, 0, ADDRESS_DOMAIN, 11];
        domain.extend_from_slice(b"example.com");
        domain.extend_from_slice(&443u16.to_be_bytes());
        domain.extend_from_slice(b"body");
        let parsed = decode_udp_request(&domain).unwrap();
        assert!(matches!(parsed.target, Target::Domain(ref name) if name == "example.com"));
        assert_eq!(parsed.port, 443);
        assert_eq!(parsed.payload, b"body");

        let source = SocketAddr::new(Ipv6Addr::LOCALHOST.into(), 5353);
        let encoded = encode_udp_response(source, b"dns");
        assert_eq!(&encoded[..4], &[0, 0, 0, ADDRESS_IPV6]);
        assert_eq!(&encoded[20..22], &5353u16.to_be_bytes());
        assert_eq!(&encoded[22..], b"dns");
    }

    #[test]
    fn udp_request_codec_rejects_fragments_and_truncation() {
        assert_eq!(
            decode_udp_request(&[0, 0, 1, ADDRESS_IPV4, 1, 1, 1, 1, 0, 53]).unwrap_err(),
            "fragmented SOCKS5 UDP datagrams are unsupported"
        );
        assert!(decode_udp_request(&[0, 0, 0, ADDRESS_IPV6, 1]).is_err());
        assert!(decode_udp_request(&[0, 0, 0, ADDRESS_IPV4, 1, 1, 1, 1, 0, 0]).is_err());
    }

    #[tokio::test]
    async fn geo_direct_udp_uses_protected_socket_and_physical_resolver() {
        let server = TokioUdpSocket::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let protector = Arc::new(TestProtector {
            resolved: server.local_addr().unwrap(),
            reject: false,
            protect_calls: AtomicUsize::new(0),
            resolve_calls: AtomicUsize::new(0),
        });
        let (context, tunnel, _server_channel, tasks) = test_socks_context(protector.clone()).await;
        let direct = DirectUdpSockets::new(protector.as_ref());
        let (response_tx, mut response_rx) = mpsc::channel(1);
        let association_cancel = CancellationToken::new();
        let receiver = spawn_direct_udp_receiver(
            Arc::clone(direct.v4.as_ref().unwrap()),
            response_tx,
            association_cancel.clone(),
            context.cancellation.clone(),
            Arc::clone(&context.counters),
        );

        send_udp_routed(
            &context,
            &Target::Domain("direct.test".to_owned()),
            server.local_addr().unwrap().port(),
            b"direct",
            &direct,
            TunnelUdpSockets {
                v4: &tunnel,
                v6: &tunnel,
            },
        )
        .await
        .unwrap();
        let mut received = [0u8; 16];
        let (length, source) = timeout(Duration::from_secs(1), server.recv_from(&mut received))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(&received[..length], b"direct");
        server.send_to(b"return", source).await.unwrap();
        let response = timeout(Duration::from_secs(1), response_rx.recv())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(response.source, server.local_addr().unwrap());
        assert_eq!(&response.payload[..], b"return");
        association_cancel.cancel();
        let _ = receiver.await;
        assert_eq!(protector.resolve_calls.load(Ordering::SeqCst), 1);
        assert!(protector.protect_calls.load(Ordering::SeqCst) >= 1);
        assert_eq!(
            context.counters.snapshot(),
            TrafficSnapshot {
                bytes_sent: 6,
                bytes_received: 6,
            }
        );
        for task in tasks {
            task.abort();
        }
    }

    #[tokio::test]
    async fn geo_direct_udp_protection_failure_falls_back_to_tunnel() {
        let protector = Arc::new(TestProtector {
            resolved: SocketAddr::from((Ipv4Addr::LOCALHOST, 53)),
            reject: true,
            protect_calls: AtomicUsize::new(0),
            resolve_calls: AtomicUsize::new(0),
        });
        let (context, tunnel, server_channel, tasks) = test_socks_context(protector.clone()).await;
        let server_ip = Ipv4Addr::new(10, 0, 0, 2);
        let server = server_channel
            .udp_bind(SocketAddr::from((server_ip, 53)))
            .await
            .unwrap();
        let direct = DirectUdpSockets::new(protector.as_ref());

        send_udp_routed(
            &context,
            &Target::Address(server_ip.into()),
            53,
            b"fallback",
            &direct,
            TunnelUdpSockets {
                v4: &tunnel,
                v6: &tunnel,
            },
        )
        .await
        .unwrap();
        let (source, payload) = timeout(Duration::from_secs(1), server.recv_from_bytes())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(source.ip(), IpAddr::V4(context.assigned_ipv4));
        assert_eq!(&payload[..], b"fallback");
        for task in tasks {
            task.abort();
        }
        assert!(direct.v4.is_none());
        assert!(protector.protect_calls.load(Ordering::SeqCst) >= 1);
    }

    #[tokio::test]
    async fn no_credentials_accepts_only_no_auth() {
        let (mut client, mut server) = tokio::io::duplex(64);
        let server = tokio::spawn(async move { negotiate_auth(&mut server, None).await });
        client
            .write_all(&[SOCKS_VERSION, 2, AUTH_NONE, AUTH_USERPASS])
            .await
            .unwrap();
        let mut reply = [0u8; 2];
        client.read_exact(&mut reply).await.unwrap();
        assert_eq!(reply, [SOCKS_VERSION, AUTH_NONE]);
        server.await.unwrap().unwrap();

        let (mut client, mut server) = tokio::io::duplex(64);
        let server = tokio::spawn(async move { negotiate_auth(&mut server, None).await });
        client
            .write_all(&[SOCKS_VERSION, 1, AUTH_USERPASS])
            .await
            .unwrap();
        let mut reply = [0u8; 2];
        client.read_exact(&mut reply).await.unwrap();
        assert_eq!(reply, [SOCKS_VERSION, AUTH_UNACCEPTABLE]);
        assert!(server.await.unwrap().is_err());
    }

    #[tokio::test]
    async fn credentials_offer_only_rfc1929_and_reject_wrong_password() {
        let credentials = ProxyAuthCredentials::parse("lan-user", b"s3cret").unwrap();

        let (mut client, mut server) = tokio::io::duplex(64);
        let expected = credentials.clone();
        let server =
            tokio::spawn(async move { negotiate_auth(&mut server, Some(&expected)).await });
        client
            .write_all(&[SOCKS_VERSION, 2, AUTH_NONE, AUTH_USERPASS])
            .await
            .unwrap();
        let mut method = [0u8; 2];
        client.read_exact(&mut method).await.unwrap();
        assert_eq!(method, [SOCKS_VERSION, AUTH_USERPASS]);
        client.write_all(&[USERPASS_VERSION, 8]).await.unwrap();
        client.write_all(b"lan-user").await.unwrap();
        client.write_all(&[6]).await.unwrap();
        client.write_all(b"s3cret").await.unwrap();
        let mut status = [0u8; 2];
        client.read_exact(&mut status).await.unwrap();
        assert_eq!(status, [USERPASS_VERSION, USERPASS_SUCCESS]);
        server.await.unwrap().unwrap();

        let (mut client, mut server) = tokio::io::duplex(64);
        let expected = credentials.clone();
        let server =
            tokio::spawn(async move { negotiate_auth(&mut server, Some(&expected)).await });
        client
            .write_all(&[SOCKS_VERSION, 1, AUTH_NONE])
            .await
            .unwrap();
        let mut method = [0u8; 2];
        client.read_exact(&mut method).await.unwrap();
        assert_eq!(method, [SOCKS_VERSION, AUTH_UNACCEPTABLE]);
        assert!(server.await.unwrap().is_err());

        let (mut client, mut server) = tokio::io::duplex(64);
        let server =
            tokio::spawn(async move { negotiate_auth(&mut server, Some(&credentials)).await });
        client
            .write_all(&[SOCKS_VERSION, 1, AUTH_USERPASS])
            .await
            .unwrap();
        let mut method = [0u8; 2];
        client.read_exact(&mut method).await.unwrap();
        assert_eq!(method, [SOCKS_VERSION, AUTH_USERPASS]);
        client.write_all(&[USERPASS_VERSION, 8]).await.unwrap();
        client.write_all(b"lan-user").await.unwrap();
        client.write_all(&[5]).await.unwrap();
        client.write_all(b"wrong").await.unwrap();
        let mut status = [0u8; 2];
        client.read_exact(&mut status).await.unwrap();
        assert_eq!(status, [USERPASS_VERSION, USERPASS_FAILURE]);
        assert!(server.await.unwrap().is_err());
    }
}
