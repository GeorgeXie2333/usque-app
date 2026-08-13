use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::num::NonZeroU16;
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpSocket, TcpStream, UdpSocket as TokioUdpSocket};
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;
use tokio::time::{Instant, timeout};
use tokio_util::sync::CancellationToken;
use ts_netstack_smoltcp::CreateSocket;
use ts_netstack_smoltcp::netcore::Channel;
use ts_netstack_smoltcp::netsock::{TcpStream as StackTcpStream, UdpSocket as StackUdpSocket};
use usque_core::{OperatingMode, Profile};

use crate::dns::Resolver;
use crate::h2::{MasqueTlsIdentity, TransportError};
use crate::netstack::{
    PacketStack, ProxyPerformanceSnapshot, RuntimeHealth, RuntimePath, TrafficSnapshot,
};
use crate::pin_refresh::EndpointPinRefresher;
use crate::port_allocator::{next_tcp_port, next_udp_port};
use crate::socket::{SocketProtector, noop_socket_protector};

const SOCKS_VERSION: u8 = 5;
const AUTH_NONE: u8 = 0;
const AUTH_UNACCEPTABLE: u8 = 0xff;
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

        // Reserve every configured address before opening the remote session so
        // a partial listener set can never be reported as ready.
        let bound = Socks5Frontend::prebind(profile)?;

        let assigned_ipv4 = identity.assigned_ipv4;
        let assigned_ipv6 = identity.assigned_ipv6;
        let mut stack =
            PacketStack::start_with_refresh(profile, Arc::new(identity), protector, pin_refresher)
                .await?;
        let frontend =
            Socks5Frontend::activate(profile, assigned_ipv4, assigned_ipv6, &stack, bound);

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
        profile
            .proxy
            .socks5_listeners
            .iter()
            .copied()
            .map(bind_listener)
            .collect()
    }

    pub(crate) fn activate(
        profile: &Profile,
        assigned_ipv4: Ipv4Addr,
        assigned_ipv6: Ipv6Addr,
        stack: &PacketStack,
        bound: Vec<TcpListener>,
    ) -> Self {
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
            assigned_ipv4,
            assigned_ipv6,
            udp_idle_timeout: Duration::from_secs(u64::from(
                profile.proxy.udp_idle_timeout_seconds.max(1),
            )),
            cancellation: cancellation.clone(),
            failure: failure_tx,
            health: stack.subscribe_health(),
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
        Self {
            listener_tasks,
            listeners,
            cancellation,
            failure,
        }
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
    assigned_ipv4: Ipv4Addr,
    assigned_ipv6: Ipv6Addr,
    udp_idle_timeout: Duration,
    cancellation: tokio_util::sync::CancellationToken,
    failure: watch::Sender<Option<String>>,
    health: watch::Receiver<RuntimeHealth>,
}

fn bind_listener(address: SocketAddr) -> Result<TcpListener, TransportError> {
    let socket = if address.is_ipv4() {
        TcpSocket::new_v4()
    } else {
        TcpSocket::new_v6()
    }
    .map_err(|source| TransportError::SocksListener { address, source })?;
    socket
        .bind(address)
        .map_err(|source| TransportError::SocksListener { address, source })?;
    socket
        .listen(256)
        .map_err(|source| TransportError::SocksListener { address, source })
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
    negotiate_auth(&mut client).await?;
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
    let addresses = match request.target {
        Target::Address(address) => vec![address],
        Target::Domain(name) => match context.resolver.resolve(&name).await {
            Ok(addresses) => addresses,
            Err(error) => {
                send_reply(
                    &mut client,
                    REPLY_HOST_UNREACHABLE,
                    SocketAddr::from(([0, 0, 0, 0], 0)),
                )
                .await?;
                return Err(error);
            }
        },
    };
    let mut remote = match connect_remote(&context, &addresses, request.port).await {
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

    send_reply(&mut client, REPLY_SUCCEEDED, remote.local_addr()).await?;
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
    let mut response_tasks = Vec::with_capacity(2);
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
        response_tx,
        association_cancel.clone(),
        context.cancellation.clone(),
    ));

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
                let addresses = match parsed.target {
                    Target::Address(address) => vec![address],
                    Target::Domain(name) => match context.resolver.resolve(&name).await {
                        Ok(addresses) => addresses,
                        Err(error) => {
                            tracing::debug!(%name, %error, "SOCKS5 UDP target resolution failed");
                            continue;
                        }
                    },
                };
                let Some(remote_ip) = addresses.into_iter().next() else {
                    tracing::debug!("SOCKS5 UDP target has no usable address");
                    continue;
                };
                let remote = SocketAddr::new(remote_ip, parsed.port);
                let socket = if remote.is_ipv4() {
                    &v4_socket
                } else {
                    &v6_socket
                };
                if let Err(error) = socket.send_to(remote, parsed.payload).await {
                    tracing::debug!(%remote, %error, "SOCKS5 UDP tunnel send failed");
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

async fn negotiate_auth(client: &mut TcpStream) -> Result<(), TransportError> {
    let version = client.read_u8().await?;
    let method_count = usize::from(client.read_u8().await?);
    if version != SOCKS_VERSION || method_count == 0 {
        return Err(TransportError::Socks5("invalid SOCKS5 greeting".to_owned()));
    }
    let mut methods = vec![0u8; method_count];
    client.read_exact(&mut methods).await?;
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
    use super::*;

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
}
