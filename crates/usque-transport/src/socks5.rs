use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::num::NonZeroU16;
use std::sync::{Arc, Mutex as StdMutex, Weak};
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream, UdpSocket as TokioUdpSocket};
use tokio::sync::{Mutex, mpsc, watch};
use tokio::task::{JoinHandle, JoinSet};
use tokio::time::Instant;
#[cfg(test)]
use tokio::time::timeout;
use tokio_util::sync::CancellationToken;
#[cfg(test)]
use ts_netstack_smoltcp::CreateSocket;
#[cfg(test)]
use ts_netstack_smoltcp::netcore::Channel;
#[cfg(test)]
use ts_netstack_smoltcp::netsock::UdpSocket as StackUdpSocket;
use usque_core::{OperatingMode, Profile, ProxyAuthCredentials};

use crate::dns::{CandidateResolution, Resolver};
use crate::geo_direct::{
    DIRECT_FAMILY_UNAVAILABLE, GeoDirectPolicy, GeoRoute, GeoTarget, RoutedTcpStream,
    bind_protected_udp, connect_routed, direct_family_available,
};
use crate::h2::{MasqueTlsIdentity, TransportError};
use crate::netstack::{
    PacketStack, ProxyPerformanceSnapshot, RuntimeHealth, RuntimePath, TrafficCounters,
    TrafficSnapshot,
};
use crate::pin_refresh::EndpointPinRefresher;
#[cfg(test)]
use crate::port_allocator::next_tcp_port;
#[cfg(test)]
use crate::port_allocator::next_udp_port;
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
const MAX_UDP_DNS_QUERIES: usize = 4;
const MAX_UDP_TARGETS: usize = 1024;

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
        Self::activate_services(
            profile,
            crate::tcp::ProxyServices::from_stack(profile, assigned_ipv4, assigned_ipv6, stack),
            bound,
        )
    }

    pub(crate) fn activate_services(
        profile: &Profile,
        services: crate::tcp::ProxyServices,
        bound: Vec<TcpListener>,
    ) -> Result<Self, TransportError> {
        let auth = match profile.proxy.listener_credentials() {
            Ok(credentials) => credentials.map(Arc::new),
            Err(error) => return Err(TransportError::Socks5(error.to_string())),
        };
        let cancellation = services.cancellation.child_token();
        let (failure_tx, failure) = watch::channel(None);
        let context = Arc::new(SocksContext {
            telemetry: services.telemetry.clone(),
            traffic_policy: services.traffic_policy,
            relay_buffer: if profile.data_plane == usque_core::DataPlaneMode::L4Proxy {
                crate::l4::Limits::platform().relay
            } else {
                crate::relay::RELAY_BUFFER_SIZE
            },
            admission: services.admission,
            channel: services.udp,
            resolver: services
                .resolver
                .with_routing(services.geo_policy.clone())
                .with_readiness(
                    services.health.clone(),
                    services.cancellation.clone(),
                    services.telemetry.clone(),
                ),
            dialer: services.dialer,
            edge_resolved: profile.proxy.dns_mode == usque_core::ProxyDnsMode::EdgeResolved,
            protector: services.protector,
            geo_policy: services.geo_policy,
            counters: services.counters,
            udp_idle_timeout: Duration::from_secs(u64::from(
                profile.proxy.udp_idle_timeout_seconds.max(1),
            )),
            cancellation: cancellation.clone(),
            failure: failure_tx,
            health: services.health,
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

#[derive(Clone)]
struct SocksContext {
    telemetry: crate::ConnectionTelemetry,
    traffic_policy: Arc<crate::application_traffic::ApplicationTrafficPolicy>,
    relay_buffer: usize,
    admission: Option<Arc<crate::tcp::FrontendAdmission>>,
    channel: Option<Arc<dyn crate::proxy_udp::UdpFactory>>,
    dialer: Arc<dyn crate::tcp::TcpDialer>,
    edge_resolved: bool,
    resolver: Resolver,
    protector: Arc<dyn SocketProtector>,
    geo_policy: Arc<GeoDirectPolicy>,
    counters: Arc<TrafficCounters>,
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
        let permit = if let Some(admission) = &context.admission {
            let Some(permit) = admission.acquire() else {
                continue;
            };
            Some(permit)
        } else {
            None
        };
        let connection_context = Arc::clone(&context);
        let l4 = context.channel.is_none();
        tokio::spawn(async move {
            let _permit = permit;
            if let Err(error) = serve_client(stream, peer, connection_context).await {
                if l4 {
                    tracing::debug!("L4 SOCKS5 session ended");
                } else {
                    tracing::debug!(%peer, %error, "SOCKS5 session ended");
                }
            }
        });
    }
}

async fn serve_client(
    mut client: TcpStream,
    peer: SocketAddr,
    context: Arc<SocksContext>,
) -> Result<(), TransportError> {
    let request = tokio::select! {
        _ = context.cancellation.cancelled() => return Ok(()),
        result = tokio::time::timeout(REMOTE_CONNECT_TIMEOUT, async {
            negotiate_auth(&mut client, context.auth.as_deref()).await?;
            read_request(&mut client).await
        }) => result.map_err(|_| TransportError::Socks5("SOCKS5 negotiation timed out".to_owned()))??,
    };
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
    let mut early_data = Vec::new();
    let mut client_read_eof = false;
    let mut peeked_full_prefix = false;
    let connecting = connect_remote(&context, &request.target, request.port);
    tokio::pin!(connecting);
    let result = loop {
        // A bounded prefix preserves pipelined bytes and lets a waiting client
        // release its request immediately on EOF without a polling timer.
        let mut bytes = [0u8; 4096];
        let mut peek = [0_u8; 1];
        let room = context
            .relay_buffer
            .saturating_sub(early_data.len())
            .min(bytes.len());
        tokio::select! {
            biased;
            _ = context.cancellation.cancelled() => return Ok(()),
            result = async {
                if room > 0 { client.read(&mut bytes[..room]).await }
                else { client.peek(&mut peek).await }
            }, if !client_read_eof && (room > 0 || !peeked_full_prefix) => {
                let count = result?;
                if count == 0 {
                    if early_data.is_empty() { return Ok(()); }
                    client_read_eof = true;
                } else if room > 0 { early_data.extend_from_slice(&bytes[..count]); }
                if room == 0 { peeked_full_prefix = true; }
            },
            result = &mut connecting => break result,
        }
    };
    let mut remote = match result {
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
        biased;
        _ = context.cancellation.cancelled() => Ok(()),
        result = async {
            remote.write_all(&early_data).await?;
            drop(early_data);
            crate::relay::copy_bidirectional_with_buffer(&mut client, &mut remote, context.relay_buffer).await
        } => {
            result
                .map(|_| ())
                .map_err(|error| TransportError::Socks5(error.to_string()))
        }
    }
}

async fn udp_epoch_ended(health: &mut watch::Receiver<RuntimeHealth>, epoch: Option<u32>) {
    loop {
        if !matches!(*health.borrow_and_update(), RuntimeHealth::Connected { reconnect_count, .. } if Some(reconnect_count) == epoch)
        {
            return;
        }
        if health.changed().await.is_err() {
            return;
        }
    }
}

async fn serve_udp_association(
    mut control: TcpStream,
    peer: SocketAddr,
    context: Arc<SocksContext>,
    request: SocksRequest,
) -> Result<(), TransportError> {
    let dns = context.resolver.stream_dns();
    if context.channel.is_none() && dns.is_none() {
        send_reply(
            &mut control,
            REPLY_COMMAND_UNSUPPORTED,
            unspecified_for(peer),
        )
        .await?;
        return Ok(());
    }
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

    let association_cancel = context.cancellation.child_token();
    let association_guard = association_cancel.clone().drop_guard();
    let _udp_buffers = if dns.is_some()
        && let Some(admission) = &context.admission
    {
        let Some(lease) = admission.reserve_udp_buffers() else {
            send_reply(&mut control, REPLY_GENERAL_FAILURE, unspecified_for(peer)).await?;
            return Ok(());
        };
        Some(lease)
    } else {
        None
    };
    let opened = match context.channel.as_ref() {
        Some(channel) => Ok(Arc::new(crate::proxy_udp::LazyAssociation::new(
            channel.clone(),
            association_cancel.clone(),
        )) as Arc<dyn crate::proxy_udp::UdpAssociation>),
        None => {
            Ok(Arc::new(crate::proxy_udp::DirectOnly) as Arc<dyn crate::proxy_udp::UdpAssociation>)
        }
    };
    let (association, tunnel_udp_unavailable): (Arc<dyn crate::proxy_udp::UdpAssociation>, bool) =
        match opened {
            Ok(a) => (a, context.channel.is_none()),
            Err(crate::tcp::DialError::Rejected(7))
                if dns.is_some() || context.geo_policy.is_enabled() =>
            {
                (Arc::new(crate::proxy_udp::DirectOnly), true)
            }
            Err(error) => {
                send_reply(
                    &mut control,
                    if matches!(error, crate::tcp::DialError::Rejected(7)) {
                        REPLY_COMMAND_UNSUPPORTED
                    } else {
                        REPLY_GENERAL_FAILURE
                    },
                    unspecified_for(peer),
                )
                .await?;
                return Ok(());
            }
        };
    let relay_ip = control.local_addr()?.ip();
    let relay = Arc::new(TokioUdpSocket::bind(SocketAddr::new(relay_ip, 0)).await?);
    let relay_address = relay.local_addr()?;
    send_reply(&mut control, REPLY_SUCCEEDED, relay_address).await?;

    let packet_limit = if dns.is_some() {
        16 * 1024 - 48
    } else {
        MAX_UDP_DATAGRAM
    };
    let (response_tx, mut response_rx) = mpsc::channel(if dns.is_some() {
        16
    } else {
        UDP_RESPONSE_CAPACITY
    });
    let mut response_tasks = Vec::with_capacity(4);
    response_tasks.push(spawn_association_receiver(
        association.clone(),
        response_tx.clone(),
        association_cancel.clone(),
    ));
    // A later rule publication can introduce Direct on this association.
    let mut direct_udp = Arc::new(DirectUdpSockets::new(context.protector.as_ref()));
    let mut direct_receivers = start_direct_udp_receivers(
        &direct_udp,
        &response_tx,
        &association_cancel,
        &context,
        packet_limit,
    );
    let mut physical = context.protector.subscribe_physical_network();

    let requested_port = NonZeroU16::new(request.port);
    let mut client_endpoint = requested_port.map(|port| SocketAddr::new(peer.ip(), port.get()));
    let mut datagram = vec![0u8; packet_limit + 1];
    let mut dns_queries = JoinSet::<Option<UdpDnsReply>>::new();
    let mut udp_sends = JoinSet::new();
    let idle = tokio::time::sleep(context.udp_idle_timeout);
    tokio::pin!(idle);
    let mut health = context.health.clone();
    let mut paused_remaining = (!matches!(*health.borrow(), RuntimeHealth::Connected { .. }))
        .then_some(context.udp_idle_timeout);
    let result = loop {
        if context.cancellation.is_cancelled() {
            break Ok(());
        }
        if direct_udp.network_generation != context.protector.network_generation() {
            // Native UDP receive buffers cannot identify the epoch of a late
            // datagram after rebinding. Retire both old ports and their workers.
            udp_sends.abort_all();
            while udp_sends.join_next().await.is_some() {}
            dns_queries.abort_all();
            while dns_queries.join_next().await.is_some() {}
            replace_direct_udp_epoch(
                &mut direct_udp,
                &mut direct_receivers,
                &context,
                &response_tx,
                &association_cancel,
                packet_limit,
            )
            .await;
        }
        tokio::select! {
            biased;
            _ = context.cancellation.cancelled() => break Ok(()),
            changed = async { match physical.as_mut() { Some(physical) => physical.changed().await, None => std::future::pending().await } }, if physical.is_some() => {
                if changed.is_err() { physical = None; }
            },
            changed = health.changed() => {
                if changed.is_err() || matches!(*health.borrow(), RuntimeHealth::Failed { .. }) { break Ok(()); }
                if matches!(*health.borrow(), RuntimeHealth::Connected { .. }) {
                    if let Some(remaining) = paused_remaining.take() { idle.as_mut().reset(Instant::now() + remaining); }
                } else if paused_remaining.is_none() {
                    paused_remaining = Some(idle.deadline().saturating_duration_since(Instant::now()));
                }
            },
            _ = &mut idle, if paused_remaining.is_none() => break Ok(()),
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
            completed = dns_queries.join_next(), if !dns_queries.is_empty() => {
                let mut reply = match completed {
                    Some(Ok(Some(reply))) => reply,
                    Some(Ok(None)) => continue,
                    _ => break Err(TransportError::Socks5("UDP DNS worker stopped".to_owned())),
                };
                if !context.geo_policy.routing_is_current(reply.routing_revision)
                    || context.protector.network_generation() != reply.network_generation {
                    reply.packet.truncate(reply.header_bytes);
                    reply.packet.extend_from_slice(&crate::split_dns::l4_dns_error(&reply.query));
                }
                let Some(endpoint) = client_endpoint else { continue; };
                tokio::select! {
                    biased;
                    _ = association_cancel.cancelled() => break Ok(()),
                    _ = control.read_u8() => break Ok(()),
                    result = relay.send_to(&reply.packet, endpoint) => {
                        if let Err(error) = result { break Err(TransportError::Io(error)); }
                    }
                }
                // Keep the query/response reservation until delivery finishes.
                drop(reply);
                idle.as_mut().reset(Instant::now() + context.udp_idle_timeout);
                if let Some(remaining) = &mut paused_remaining { *remaining = context.udp_idle_timeout; }
            }
            _ = udp_sends.join_next(), if !udp_sends.is_empty() => {},
            received = relay.recv_from(&mut datagram) => {
                let (length, source) = match received {
                    Ok(value) => value,
                    Err(error) if crate::udp_io::is_message_too_long(&error) => continue,
                    Err(error) => break Err(TransportError::Io(error)),
                };
                if direct_udp.network_generation != context.protector.network_generation() {
                    // A platform without a watch may first reveal migration
                    // when this datagram arrives. Process it on fresh ports.
                    udp_sends.abort_all();
                    while udp_sends.join_next().await.is_some() {}
                    dns_queries.abort_all();
                    while dns_queries.join_next().await.is_some() {}
                    replace_direct_udp_epoch(&mut direct_udp, &mut direct_receivers, &context, &response_tx, &association_cancel, packet_limit).await;
                }
                if length > packet_limit { continue; }
                if source.ip() != peer.ip()
                    || requested_port.is_some_and(|port| source.port() != port.get())
                    || client_endpoint.is_some_and(|endpoint| endpoint != source)
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
                let valid_dns = parsed.port == 53
                    && crate::split_dns::validate_query_bytes(parsed.payload).is_ok();
                if context.channel.is_none() && !valid_dns {
                    // A DNS-only local relay does not enable ordinary UDP,
                    // including the existing L4 direct-routing restriction.
                    continue;
                }
                let slot = if valid_dns { None } else {
                    match direct_udp.target_route(&context, &parsed.target, parsed.port) {
                        Ok(slot) => Some(slot),
                        Err(_) => continue,
                    }
                };
                let policy = slot.as_ref().map(|slot| slot.policy.clone())
                    .unwrap_or_else(|| Arc::new(context.geo_policy.freeze_routing()));
                let route = slot.as_ref().and_then(|slot| slot.pinned()).map(|path| path.route()).unwrap_or_else(|| match &parsed.target {
                    Target::Address(address) => GeoTarget::Ip(*address),
                    Target::Domain(name) => GeoTarget::Host(name),
                }.route(&policy));
                if route == GeoRoute::Reject { continue; }
                if route == GeoRoute::Tunnel && !matches!(*health.borrow(), RuntimeHealth::Connected { .. }) { continue; }
                if valid_dns && let Some(refused) = crate::split_dns::routing_dns_refused(parsed.payload, &policy) {
                    let target = match &parsed.target {
                        Target::Address(ip) => crate::tcp::TcpTarget::address(SocketAddr::new(*ip, parsed.port)),
                        Target::Domain(name) => match crate::tcp::TcpTarget::new(name, parsed.port) { Ok(target) => target, Err(_) => continue },
                    };
                    let mut response = vec![0, 0, 0];
                    crate::proxy_exit::encode_target(&target, &mut response);
                    response.extend_from_slice(&refused);
                    let _ = relay.send_to(&response, source).await;
                    continue;
                }
                if tunnel_udp_unavailable && route == GeoRoute::Tunnel && !valid_dns
                    && !(policy.has_ip_rules() && matches!(&parsed.target, Target::Domain(_))) {
                    continue;
                }
                if valid_dns
                    && route == GeoRoute::Tunnel
                    && let Some(dns) = &dns
                {
                    // The local client uses UDP, but its DNS query is a framed
                    // TCP exchange through the final exit. An unavailable UDP
                    // relay must not disable this path or change its resolver.
                    let target = match &parsed.target {
                        Target::Address(address) => crate::tcp::TcpTarget::address(SocketAddr::new(*address, parsed.port)),
                        Target::Domain(name) => match crate::tcp::TcpTarget::new(name, parsed.port) {
                            Ok(target) => target,
                            Err(_) => continue,
                        },
                    };
                    let mut packet = vec![0, 0, 0];
                    crate::proxy_exit::encode_target(&target, &mut packet);
                    // Pin the first accepted client before asynchronous work.
                    // A pending query must not leave this association claimable
                    // by another endpoint sharing the TCP peer's address.
                    client_endpoint.get_or_insert(source);
                    idle.as_mut().reset(Instant::now() + context.udp_idle_timeout);
                if let Some(remaining) = &mut paused_remaining { *remaining = context.udp_idle_timeout; }
                    let lease = (dns_queries.len() < MAX_UDP_DNS_QUERIES)
                        .then(|| context.admission.as_ref()
                            .and_then(|admission| admission.reserve_udp_dns_query(parsed.payload.len())))
                        .flatten();
                    let Some(lease) = lease else {
                        let response = crate::split_dns::limit_udp_response(
                            parsed.payload,
                            crate::split_dns::l4_dns_error(parsed.payload),
                            packet_limit.saturating_sub(packet.len()),
                        );
                        packet.extend_from_slice(&response);
                        tokio::select! {
                            biased;
                            _ = association_cancel.cancelled() => break Ok(()),
                            _ = control.read_u8() => break Ok(()),
                            result = relay.send_to(&packet, source) => {
                                if let Err(error) = result { break Err(TransportError::Io(error)); }
                            }
                        }
                        continue;
                    };
                    let query = parsed.payload.to_vec();
                    let target = parsed.target;
                    let port = parsed.port;
                    let mut query_context = (*context).clone();
                    query_context.geo_policy = policy;
                    query_context.resolver = query_context.resolver.with_routing(query_context.geo_policy.clone());
                    let context = Arc::new(query_context);
                    let dns = dns.clone();
                    let cancel = association_cancel.clone();
                    let mut epoch_health = context.health.clone();
                    let epoch = match *epoch_health.borrow_and_update() { RuntimeHealth::Connected { reconnect_count, .. } => Some(reconnect_count), _ => None };
                    dns_queries.spawn(async move {
                        let network_generation = context.protector.network_generation();
                        let request = SocksUdpRequest { target, port, payload: &query };
                        let response = tokio::select! {
                            biased;
                            _ = cancel.cancelled() => return None,
                            _ = udp_epoch_ended(&mut epoch_health, epoch) => return None,
                            response = forward_udp_dns(&context, &dns, &request) => response,
                        };
                        let response = response?;
                        let response = crate::split_dns::limit_udp_response(
                            &query, response, packet_limit.saturating_sub(packet.len()),
                        );
                        let header_bytes = packet.len();
                        packet.extend_from_slice(&response);
                        Some(UdpDnsReply { packet, query, header_bytes, routing_revision: context.geo_policy.routing_revision(), network_generation, _lease: lease })
                    });
                    continue;
                }
                {
                    let lease = (udp_sends.len() < 4).then(|| context.admission.as_ref()
                        .and_then(|admission| admission.reserve_udp_dns_query(parsed.payload.len()))).flatten();
                    let Some(lease) = lease else { continue; };
                    let payload = parsed.payload.to_vec();
                    let target = parsed.target;
                    let port = parsed.port;
                    let mut send_context = (*context).clone();
                    send_context.geo_policy = policy;
                    send_context.resolver = send_context.resolver.with_routing(send_context.geo_policy.clone()).without_recovery_wait();
                    let send_context = Arc::new(send_context);
                    let association = association.clone();
                    let direct = direct_udp.clone();
                    let cancel = association_cancel.clone();
                    let mut epoch_health = context.health.clone();
                    let epoch = match *epoch_health.borrow_and_update() { RuntimeHealth::Connected { reconnect_count, .. } => Some(reconnect_count), _ => None };
                    udp_sends.spawn(async move {
                        let _lease = lease;
                        let request = SocksUdpRequest { target, port, payload: &payload };
                        tokio::select! {
                            biased;
                            _ = cancel.cancelled() => {},
                            _ = udp_epoch_ended(&mut epoch_health, epoch), if route == GeoRoute::Tunnel => {},
                            result = send_udp_with_slot(&send_context, &request, &direct,
                                TunnelUdpSockets::Association(association.as_ref()), slot) => {
                                if let Err(error) = result { tracing::debug!(%error, "SOCKS5 UDP send failed"); }
                            }
                        }
                    });
                    client_endpoint.get_or_insert(source);
                    idle.as_mut().reset(Instant::now() + context.udp_idle_timeout);
                if let Some(remaining) = &mut paused_remaining { *remaining = context.udp_idle_timeout; }
                    continue;
                }

            }
            response = response_rx.recv() => {
                let Some(response) = response else {
                    break Err(TransportError::Socks5(
                        "all SOCKS5 UDP tunnel receivers stopped".to_owned(),
                    ));
                };
                let mut response = match response {
                    Ok(response) => response,
                    Err(UdpReceiveFailure::Direct { network_generation, .. }) if network_generation != context.protector.network_generation() => continue,
                    Err(error) => break Err(TransportError::Socks5(error.message())),
                };
                if response.blocked_by(&context.traffic_policy) { continue; }
                if response.route == GeoRoute::Direct {
                    let Some(remote) = response.source.socket_address() else { continue; };
                    if !direct_udp.authorizes_reply(context.protector.as_ref(), remote, response.network_generation).await { continue; }
                }
                if response.source.host_port().1 == 53 {
                    let Some(server) = response.source.socket_address() else { continue; };
                    let Some(filtered) = direct_udp.routing_dns_queries.lock().await.filter(server, &response.payload, &context.geo_policy) else { continue; };
                    response.payload = bytes::Bytes::from(filtered);
                }
                if response.route == GeoRoute::Tunnel && !association.accounts_traffic() {
                    context.counters.record_received(response.payload.len());
                }
                let Some(client_endpoint) = client_endpoint else {
                    continue;
                };
                let mut packet = vec![0, 0, 0];
                crate::proxy_exit::encode_target(&response.source, &mut packet);
                if packet.len() + response.payload.len() > packet_limit { continue; }
                packet.extend_from_slice(&response.payload);
                if response.route == GeoRoute::Direct && response.network_generation != context.protector.network_generation() { continue; }
                if let Err(error) = relay.send_to(&packet, client_endpoint).await {
                    break Err(TransportError::Io(error));
                }
                idle.as_mut().reset(Instant::now() + context.udp_idle_timeout);
                if let Some(remaining) = &mut paused_remaining { *remaining = context.udp_idle_timeout; }
            }
        }
    };

    association_cancel.cancel();
    drop(association_guard);
    udp_sends.abort_all();
    while udp_sends.join_next().await.is_some() {}
    dns_queries.abort_all();
    while dns_queries.join_next().await.is_some() {}
    stop_direct_udp_receivers(&mut direct_receivers).await;
    for task in response_tasks {
        let _ = task.await;
    }
    result
}

struct UdpDnsReply {
    packet: Vec<u8>,
    query: Vec<u8>,
    header_bytes: usize,
    routing_revision: u64,
    network_generation: Option<u64>,
    _lease: crate::l4::stream::BufferLease,
}

async fn udp_dns_target(
    context: &SocksContext,
    request: &SocksUdpRequest<'_>,
) -> Result<Option<(crate::tcp::TcpTarget, GeoRoute)>, String> {
    let target = match &request.target {
        Target::Address(ip) => GeoTarget::Ip(*ip),
        Target::Domain(name) => GeoTarget::Host(name),
    };
    if target.route(&context.geo_policy) == GeoRoute::Reject {
        return Ok(None);
    }
    match &request.target {
        Target::Address(ip) => Ok(Some((
            crate::tcp::TcpTarget::address(SocketAddr::new(*ip, request.port)),
            target.route(&context.geo_policy),
        ))),
        Target::Domain(name) if context.edge_resolved && !context.geo_policy.has_ip_rules() => {
            crate::tcp::TcpTarget::new(name, request.port)
                .map(|target| Some((target, GeoRoute::Tunnel)))
                .map_err(|_| "invalid DNS target".into())
        }
        Target::Domain(name) => {
            let addresses = context
                .resolver
                .resolve_for_policy(name)
                .await
                .map_err(|error| error.to_string());
            let addresses = match addresses {
                Err(error) if error.contains("routing_rejected") => return Ok(None),
                other => other?,
            };
            Ok(addresses
                .into_iter()
                .find(|ip| !context.geo_policy.rejects_ip(*ip))
                .map(|ip| {
                    (
                        crate::tcp::TcpTarget::address(SocketAddr::new(ip, request.port)),
                        context.geo_policy.resolved_route(name, ip),
                    )
                }))
        }
    }
}

async fn forward_udp_dns(
    context: &SocksContext,
    dns: &crate::dns_stream::StreamDns,
    request: &SocksUdpRequest<'_>,
) -> Option<Vec<u8>> {
    let mut frozen = context.clone();
    frozen.geo_policy = Arc::new(context.geo_policy.freeze_routing());
    frozen.resolver = frozen.resolver.with_routing(frozen.geo_policy.clone());
    let context = &frozen;
    let network_generation = context.protector.network_generation();
    if let Some(refused) =
        crate::split_dns::routing_dns_refused(request.payload, &context.geo_policy)
    {
        return Some(refused);
    }
    let deadline = Instant::now() + Duration::from_secs(4);
    let work = async {
        let Some((target, route)) = udp_dns_target(context, request).await.map_err(|_| ())? else {
            return Ok(None);
        };
        let direct = if route == GeoRoute::Direct {
            let remote = target.socket_address().ok_or(())?;
            if !direct_family_available(context.protector.as_ref(), remote) {
                return Err(());
            }
            crate::split_dns::direct_udp(context.protector.as_ref(), remote, request.payload)
                .await
                .ok()
        } else {
            None
        };
        let response = if let Some(response) = direct {
            context.counters.record_sent(request.payload.len());
            context.counters.record_received(response.len());
            response
        } else {
            if route == GeoRoute::Direct
                && target.socket_address().is_some_and(|remote| {
                    !direct_family_available(context.protector.as_ref(), remote)
                })
            {
                return Err(());
            }
            dns.query_target(target, request.payload, deadline)
                .await
                .map_err(|_| ())?
        };
        if context.protector.network_generation() != network_generation
            || !context
                .geo_policy
                .routing_is_current(context.geo_policy.routing_revision())
        {
            return Ok(Some(crate::split_dns::l4_dns_error(request.payload)));
        }
        Ok(Some(crate::split_dns::filter_routing_dns_response(
            request.payload,
            response,
            &context.geo_policy,
        )))
    };
    tokio::time::timeout_at(deadline, work)
        .await
        .unwrap_or(Err(()))
        .unwrap_or_else(|_| Some(crate::split_dns::l4_dns_error(request.payload)))
}

struct UdpResponse {
    source: crate::tcp::TcpTarget,
    payload: bytes::Bytes,
    route: GeoRoute,
    network_generation: Option<u64>,
}

impl UdpResponse {
    fn blocked_by(&self, policy: &crate::application_traffic::ApplicationTrafficPolicy) -> bool {
        self.route == GeoRoute::Tunnel && policy.blocks_udp(self.source.host_port().1)
    }
}

#[derive(Default)]
struct DirectUdpSockets {
    network_generation: Option<u64>,
    routing_dns_queries: Mutex<crate::split_dns::RoutingDnsQueries>,
    v4: Option<Arc<TokioUdpSocket>>,
    v6: Option<Arc<TokioUdpSocket>>,
    leases: Mutex<HashMap<(Option<u64>, SocketAddr), DirectEgressLease>>,
    target_routes: StdMutex<UdpTargetRoutes>,
}

#[derive(Default)]
struct UdpTargetRoutes {
    entries: HashMap<(Target, u16), Arc<UdpTargetRoute>>,
    current_policy: Option<Arc<GeoDirectPolicy>>,
    direct_sources: HashMap<SocketAddr, Vec<Weak<UdpTargetRoute>>>,
}

#[derive(Clone)]
enum UdpPath {
    Direct(SocketAddr),
    Tunnel(crate::tcp::TcpTarget),
}

impl UdpPath {
    fn route(&self) -> GeoRoute {
        match self {
            Self::Direct(_) => GeoRoute::Direct,
            Self::Tunnel(_) => GeoRoute::Tunnel,
        }
    }
}

struct UdpTargetRoute {
    policy: Arc<GeoDirectPolicy>,
    network_generation: Option<u64>,
    idle_timeout: Duration,
    sent: StdMutex<Option<(UdpPath, Instant)>>,
    sending: Mutex<()>,
}

impl UdpTargetRoute {
    fn pinned(&self) -> Option<UdpPath> {
        self.sent
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .as_ref()
            .map(|(path, _)| path.clone())
    }
}

#[derive(Clone, Copy)]
enum TunnelUdpSockets<'a> {
    #[cfg(test)]
    Stack {
        v4: &'a StackUdpSocket,
        v6: &'a StackUdpSocket,
    },
    Association(&'a dyn crate::proxy_udp::UdpAssociation),
}

impl DirectUdpSockets {
    fn new(protector: &dyn SocketProtector) -> Self {
        Self {
            network_generation: protector.network_generation(),
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
            routing_dns_queries: Mutex::default(),
            target_routes: StdMutex::default(),
        }
    }

    fn target_route(
        &self,
        context: &SocksContext,
        target: &Target,
        port: u16,
    ) -> Result<Arc<UdpTargetRoute>, String> {
        let mut routes = self
            .target_routes
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let generation = context.protector.network_generation();
        let live = |slot: &Arc<UdpTargetRoute>| {
            slot.network_generation == generation
                && (Arc::strong_count(slot) > 1
                    || slot
                        .sent
                        .lock()
                        .unwrap_or_else(|error| error.into_inner())
                        .as_ref()
                        .is_some_and(|(_, used)| used.elapsed() < context.udp_idle_timeout))
        };
        let key = (
            match target {
                Target::Address(ip) => Target::Address(ip.to_canonical()),
                Target::Domain(name) => Target::Domain(
                    usque_core::config::canonical_bypass_domain(name)
                        .unwrap_or_else(|| name.clone()),
                ),
            },
            port,
        );
        if let Some(slot) = routes.entries.get(&key).filter(|slot| live(slot)) {
            return Ok(slot.clone());
        }
        // Established targets take the O(1) path above. Scan expiration only
        // when admitting a new target, never on every datagram.
        routes.entries.retain(|_, slot| live(slot));
        routes.direct_sources.retain(|_, slots| {
            slots.retain(|slot| slot.upgrade().is_some_and(|slot| live(&slot)));
            !slots.is_empty()
        });
        if routes.entries.len() >= MAX_UDP_TARGETS {
            return Err("UDP target route limit reached".into());
        }
        let policy = match routes.current_policy.as_ref().filter(|policy| {
            context
                .geo_policy
                .routing_is_current(policy.routing_revision())
        }) {
            Some(policy) => policy.clone(),
            None => {
                let policy = Arc::new(context.geo_policy.freeze_routing());
                routes.current_policy = Some(policy.clone());
                policy
            }
        };
        let slot = Arc::new(UdpTargetRoute {
            policy,
            network_generation: generation,
            idle_timeout: context.udp_idle_timeout,
            sent: StdMutex::new(None),
            sending: Mutex::new(()),
        });
        routes.entries.insert(key, slot.clone());
        Ok(slot)
    }

    async fn track_dns(&self, context: &SocksContext, remote: SocketAddr, payload: &[u8]) -> bool {
        if remote.port() != 53 || crate::split_dns::validate_query_bytes(payload).is_err() {
            return true;
        }
        self.routing_dns_queries
            .lock()
            .await
            .record(remote, payload, &context.geo_policy)
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
        if generation != self.network_generation {
            return Err(crate::socket::STALE_GENERATION_REASON.into());
        }
        {
            let mut leases = self.leases.lock().await;
            if protector.network_generation() != generation {
                return Err(crate::socket::STALE_GENERATION_REASON.into());
            }
            leases.retain(|(existing_generation, _), _| *existing_generation == generation);
            if leases.contains_key(&(generation, remote)) {
                return Ok(());
            }
            if leases.len() >= 1024 {
                return Err("direct UDP target lease limit reached".to_owned());
            }
        }
        let lease = match generation {
            Some(generation) => {
                protector
                    .protect_for_target_generation(
                        socket_handle(socket.as_ref()),
                        remote,
                        DirectProtocol::Udp,
                        generation,
                    )
                    .await
            }
            None => {
                protector
                    .protect_for_target(socket_handle(socket.as_ref()), remote, DirectProtocol::Udp)
                    .await
            }
        }
        .map_err(|error| format!("protect direct UDP target {remote}: {error}"))?;
        let mut leases = self.leases.lock().await;
        if protector.network_generation() != generation || lease.generation() != generation {
            return Err(crate::socket::STALE_GENERATION_REASON.into());
        }
        leases.retain(|(existing_generation, _), _| *existing_generation == generation);
        leases.entry((generation, remote)).or_insert(lease);
        Ok(())
    }

    async fn authorizes_reply(
        &self,
        protector: &dyn SocketProtector,
        remote: SocketAddr,
        generation: Option<u64>,
    ) -> bool {
        if self.network_generation != generation || protector.network_generation() != generation {
            return false;
        }
        let leases = self.leases.lock().await;
        if protector.network_generation() != generation
            || !leases.contains_key(&(generation, remote))
        {
            return false;
        }
        if remote.port() == 53 {
            return true;
        } // The query correlation is checked next.
        let mut routes = self
            .target_routes
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let Some(slots) = routes.direct_sources.get_mut(&remote) else {
            return false;
        };
        let mut authorized = false;
        slots.retain(|slot| {
            let Some(slot) = slot.upgrade() else {
                return false;
            };
            if slot.network_generation != generation {
                return false;
            }
            let mut sent = slot.sent.lock().unwrap_or_else(|error| error.into_inner());
            if let Some((UdpPath::Direct(target), used)) = sent.as_mut()
                && *target == remote
            {
                if used.elapsed() >= slot.idle_timeout {
                    return false;
                }
                *used = Instant::now();
                authorized = true;
            }
            true
        });
        authorized
    }
}

#[cfg(test)]
async fn send_udp_routed(
    context: &SocksContext,
    target: &Target,
    port: u16,
    payload: &[u8],
    direct: &DirectUdpSockets,
    tunnel: TunnelUdpSockets<'_>,
) -> Result<(), String> {
    let slot = if port == 53 && crate::split_dns::validate_query_bytes(payload).is_ok() {
        None
    } else {
        Some(direct.target_route(context, target, port)?)
    };
    send_udp_with_slot(
        context,
        &SocksUdpRequest {
            target: target.clone(),
            port,
            payload,
        },
        direct,
        tunnel,
        slot,
    )
    .await
}

async fn send_udp_with_slot(
    context: &SocksContext,
    request: &SocksUdpRequest<'_>,
    direct: &DirectUdpSockets,
    tunnel: TunnelUdpSockets<'_>,
    slot: Option<Arc<UdpTargetRoute>>,
) -> Result<(), String> {
    let _sending = match &slot {
        Some(slot) => Some(slot.sending.lock().await),
        None => None,
    };
    let generation = slot.as_ref().map_or_else(
        || context.protector.network_generation(),
        |slot| slot.network_generation,
    );
    verify_udp_generation(context, generation)?;
    let mut frozen = context.clone();
    frozen.geo_policy = slot
        .as_ref()
        .map(|slot| slot.policy.clone())
        .unwrap_or_else(|| Arc::new(context.geo_policy.freeze_routing()));
    frozen.resolver = frozen.resolver.with_routing(frozen.geo_policy.clone());
    let context = &frozen;
    let path = if let Some(path) = slot.as_ref().and_then(|slot| slot.pinned()) {
        match path {
            UdpPath::Direct(remote) => {
                if !direct_family_available(context.protector.as_ref(), remote) {
                    return Err(DIRECT_FAMILY_UNAVAILABLE.into());
                }
                direct
                    .ensure_target(context.protector.as_ref(), remote)
                    .await?;
                verify_udp_generation(context, generation)?;
                let socket = direct
                    .for_address(remote)
                    .ok_or("protected direct UDP socket unavailable")?;
                let written = socket
                    .send_to(request.payload, remote)
                    .await
                    .map_err(|error| error.to_string())?;
                if written != request.payload.len() {
                    return Err("partial direct UDP send".into());
                }
                context.counters.record_sent(written);
                Some(UdpPath::Direct(remote))
            }
            UdpPath::Tunnel(target) => {
                if context.traffic_policy.blocks_udp(request.port) {
                    return Ok(());
                }
                if let Some(remote) = target.socket_address() {
                    send_numeric_udp(context, direct, tunnel, remote, request.payload, generation)
                        .await?
                } else {
                    #[cfg(not(test))]
                    let TunnelUdpSockets::Association(association) = tunnel;
                    #[cfg(test)]
                    let association = match tunnel {
                        TunnelUdpSockets::Association(association) => association,
                        TunnelUdpSockets::Stack { .. } => {
                            return Err("named UDP target requires an upstream association".into());
                        }
                    };
                    association
                        .prepare()
                        .await
                        .map_err(|_| "final UDP unavailable")?;
                    verify_udp_generation(context, generation)?;
                    association
                        .send(&target, request.payload)
                        .await
                        .map_err(|_| "final UDP send failed")?;
                    if !association.accounts_traffic() {
                        context.counters.record_sent(request.payload.len());
                    }
                    Some(UdpPath::Tunnel(target))
                }
            }
        }
    } else {
        send_udp_unpinned(context, request, direct, tunnel, generation).await?
    };
    verify_udp_generation(context, generation)?;
    if let (Some(slot), Some(path)) = (&slot, path) {
        let mut routes = direct
            .target_routes
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let first = slot.pinned().is_none();
        if first && let UdpPath::Direct(remote) = &path {
            routes
                .direct_sources
                .entry(*remote)
                .or_default()
                .push(Arc::downgrade(slot));
        }
        *slot.sent.lock().unwrap_or_else(|error| error.into_inner()) = Some((path, Instant::now()));
    }
    Ok(())
}

fn verify_udp_generation(context: &SocksContext, generation: Option<u64>) -> Result<(), String> {
    if context.protector.network_generation() == generation {
        Ok(())
    } else {
        Err(crate::socket::STALE_GENERATION_REASON.into())
    }
}

async fn send_udp_unpinned(
    context: &SocksContext,
    request: &SocksUdpRequest<'_>,
    direct: &DirectUdpSockets,
    tunnel: TunnelUdpSockets<'_>,
    generation: Option<u64>,
) -> Result<Option<UdpPath>, String> {
    let target = &request.target;
    let port = request.port;
    let payload = request.payload;
    let mut resolved_for_tunnel = None;
    let geo_target = match target {
        Target::Address(address) => GeoTarget::Ip(*address),
        Target::Domain(name) => GeoTarget::Host(name),
    };
    if geo_target.route(&context.geo_policy) == GeoRoute::Reject {
        return Ok(None);
    }
    if context.geo_policy.has_ip_rules()
        && let Target::Domain(name) = target
    {
        let (addresses, tunnel_only) = crate::geo_direct::resolve_routing_host(
            &context.geo_policy,
            context.protector.as_ref(),
            &context.resolver,
            name,
            port,
        )
        .await?;
        verify_udp_generation(context, generation)?;
        for ip in addresses
            .into_iter()
            .filter(|ip| !context.geo_policy.rejects_ip(*ip))
            .filter(|ip| {
                context.geo_policy.resolved_route(name, *ip) != GeoRoute::Direct
                    || direct_family_available(
                        context.protector.as_ref(),
                        SocketAddr::new(*ip, port),
                    )
            })
            .take(MAX_TARGET_ADDRESSES)
        {
            let route = if tunnel_only {
                GeoRoute::Tunnel
            } else {
                context.geo_policy.resolved_route(name, ip)
            };
            if route == GeoRoute::Reject {
                continue;
            }
            let remote = SocketAddr::new(ip, port);
            if route == GeoRoute::Direct
                && let Some(socket) = direct.for_address(remote)
                && direct
                    .ensure_target(context.protector.as_ref(), remote)
                    .await
                    .is_ok()
                && direct.track_dns(context, remote, payload).await
                && verify_udp_generation(context, generation).is_ok()
                && socket.send_to(payload, remote).await.is_ok()
            {
                context.counters.record_sent(payload.len());
                return Ok(Some(UdpPath::Direct(remote)));
            }
            if route == GeoRoute::Direct
                && !direct_family_available(context.protector.as_ref(), remote)
            {
                continue;
            }
            if context.traffic_policy.blocks_udp(port) {
                return Ok(None);
            }
            return send_numeric_udp(context, direct, tunnel, remote, payload, generation).await;
        }
        return Ok(None);
    }
    if geo_target.route(&context.geo_policy) == GeoRoute::Direct {
        let addresses = match target {
            Target::Address(address) => Ok(vec![SocketAddr::new(*address, port)]),
            Target::Domain(name) => {
                crate::split_dns::resolve_direct_routed(
                    context.protector.as_ref(),
                    name,
                    port,
                    &context.geo_policy,
                )
                .await
            }
        };
        match addresses {
            Ok(addresses) => {
                let had_addresses = !addresses.is_empty();
                let addresses = addresses
                    .into_iter()
                    .filter(|address| direct_family_available(context.protector.as_ref(), *address))
                    .collect::<Vec<_>>();
                if had_addresses && addresses.is_empty() {
                    return Err(DIRECT_FAMILY_UNAVAILABLE.into());
                }
                // Keep the filtered answer for fallback even with system DNS;
                // resolving again at the exit could restore unusable IPv6.
                resolved_for_tunnel = Some(addresses.iter().map(|address| address.ip()).collect());
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
                    if !direct.track_dns(context, remote, payload).await {
                        return Ok(None);
                    }
                    verify_udp_generation(context, generation)?;
                    match socket.send_to(payload, remote).await {
                        Ok(written) if written == payload.len() => {
                            context.counters.record_sent(written);
                            return Ok(Some(UdpPath::Direct(remote)));
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
            Err(error) => {
                if error == "routing_rejected" {
                    return Ok(None);
                }
                if error == DIRECT_FAMILY_UNAVAILABLE {
                    return Err(error);
                }
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

    // GEO direct has already had its opportunity. A failed direct attempt
    // must not bypass the tunnel policy, nor trigger an unnecessary DNS query.
    if context.traffic_policy.blocks_udp(port) {
        return Ok(None);
    }
    match tunnel {
        #[cfg(test)]
        TunnelUdpSockets::Stack { .. } => {}
        TunnelUdpSockets::Association(association) => {
            association
                .prepare()
                .await
                .map_err(|_| "final UDP unavailable".to_owned())?;
        }
    }
    verify_udp_generation(context, generation)?;
    if context.edge_resolved
        && geo_target.route(&context.geo_policy) != GeoRoute::Direct
        && !(port == 53 && context.geo_policy.is_enabled())
        && resolved_for_tunnel.is_none()
        && let Target::Domain(name) = target
        && let TunnelUdpSockets::Association(association) = tunnel
    {
        let target = crate::tcp::TcpTarget::new(name, port).map_err(|_| "invalid target")?;
        association
            .send(&target, payload)
            .await
            .map_err(|_| "final UDP send failed".to_owned())?;
        if !association.accounts_traffic() {
            context.counters.record_sent(payload.len());
        }
        return Ok(Some(UdpPath::Tunnel(target)));
    }
    let addresses = if let Some(addresses) = resolved_for_tunnel {
        addresses
    } else {
        match target {
            Target::Address(address) => vec![*address],
            Target::Domain(name) => context
                .resolver
                .resolve_for_policy(name)
                .await
                .map_err(|error| error.to_string())?,
        }
    };
    let remote = addresses
        .into_iter()
        .map(|address| SocketAddr::new(address, port))
        .find(|remote| {
            geo_target.route(&context.geo_policy) != GeoRoute::Direct
                || direct_family_available(context.protector.as_ref(), *remote)
        })
        .ok_or_else(|| "target has no usable address".to_owned())?;
    if context.traffic_policy.blocks_udp(port) {
        return Ok(None);
    }
    send_numeric_udp(context, direct, tunnel, remote, payload, generation).await
}

async fn send_numeric_udp(
    context: &SocksContext,
    direct: &DirectUdpSockets,
    tunnel: TunnelUdpSockets<'_>,
    remote: SocketAddr,
    payload: &[u8],
    generation: Option<u64>,
) -> Result<Option<UdpPath>, String> {
    if context.geo_policy.rejects_ip(remote.ip())
        || !direct.track_dns(context, remote, payload).await
    {
        return Ok(None);
    }
    match tunnel {
        #[cfg(test)]
        TunnelUdpSockets::Stack { .. } => {}
        TunnelUdpSockets::Association(association) => association
            .prepare()
            .await
            .map_err(|_| "final UDP unavailable".to_owned())?,
    }
    verify_udp_generation(context, generation)?;
    match tunnel {
        #[cfg(test)]
        TunnelUdpSockets::Stack { v4, v6 } => {
            let socket = if remote.is_ipv4() { v4 } else { v6 };
            socket
                .send_to(remote, payload)
                .await
                .map(|()| Some(UdpPath::Tunnel(crate::tcp::TcpTarget::address(remote))))
                .map_err(|_| "tunnel UDP send failed".to_owned())
        }
        TunnelUdpSockets::Association(association) => {
            association
                .send(&crate::tcp::TcpTarget::address(remote), payload)
                .await
                .map_err(|_| "final UDP send failed".to_owned())?;
            if !association.accounts_traffic() {
                context.counters.record_sent(payload.len());
            }
            Ok(Some(UdpPath::Tunnel(crate::tcp::TcpTarget::address(
                remote,
            ))))
        }
    }
}

fn spawn_association_receiver(
    association: Arc<dyn crate::proxy_udp::UdpAssociation>,
    sender: mpsc::Sender<Result<UdpResponse, UdpReceiveFailure>>,
    cancel: CancellationToken,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let work = async {
            loop {
                let result = association
                    .recv()
                    .await
                    .map(|(source, payload)| UdpResponse {
                        source,
                        payload,
                        route: GeoRoute::Tunnel,
                        network_generation: None,
                    })
                    .map_err(|_| {
                        UdpReceiveFailure::Tunnel("final UDP association closed".to_owned())
                    });
                let failed = result.is_err();
                if sender.send(result).await.is_err() || failed {
                    break;
                }
            }
        };
        tokio::select! { _ = cancel.cancelled() => {}, _ = work => {} }
    })
}

#[derive(Debug)]
enum UdpReceiveFailure {
    Tunnel(String),
    Direct {
        message: String,
        network_generation: Option<u64>,
    },
}

impl UdpReceiveFailure {
    fn message(self) -> String {
        match self {
            Self::Tunnel(message) | Self::Direct { message, .. } => message,
        }
    }
}

struct DirectUdpReceiveContext {
    runtime_cancel: CancellationToken,
    counters: Arc<TrafficCounters>,
    network_generation: Option<u64>,
}

fn start_direct_udp_receivers(
    direct: &DirectUdpSockets,
    sender: &mpsc::Sender<Result<UdpResponse, UdpReceiveFailure>>,
    cancellation: &CancellationToken,
    context: &SocksContext,
    packet_limit: usize,
) -> [Option<JoinHandle<()>>; 2] {
    [&direct.v4, &direct.v6].map(|socket| {
        socket.as_ref().map(|socket| {
            spawn_direct_udp_receiver(
                socket.clone(),
                sender.clone(),
                cancellation.clone(),
                DirectUdpReceiveContext {
                    runtime_cancel: context.cancellation.clone(),
                    counters: context.counters.clone(),
                    network_generation: direct.network_generation,
                },
                packet_limit,
            )
        })
    })
}

async fn stop_direct_udp_receivers(receivers: &mut [Option<JoinHandle<()>>; 2]) {
    for receiver in receivers.iter_mut().filter_map(Option::take) {
        receiver.abort();
        let _ = receiver.await;
    }
}

async fn replace_direct_udp_epoch(
    direct: &mut Arc<DirectUdpSockets>,
    receivers: &mut [Option<JoinHandle<()>>; 2],
    context: &SocksContext,
    sender: &mpsc::Sender<Result<UdpResponse, UdpReceiveFailure>>,
    cancellation: &CancellationToken,
    packet_limit: usize,
) {
    // Keep old ports bound until the new epoch is allocated, preventing an
    // immediate port reuse from admitting an old in-flight datagram.
    stop_direct_udp_receivers(receivers).await;
    let next = Arc::new(DirectUdpSockets::new(context.protector.as_ref()));
    *receivers = start_direct_udp_receivers(&next, sender, cancellation, context, packet_limit);
    *direct = next;
}

fn spawn_direct_udp_receiver(
    socket: Arc<TokioUdpSocket>,
    sender: mpsc::Sender<Result<UdpResponse, UdpReceiveFailure>>,
    association_cancel: CancellationToken,
    context: DirectUdpReceiveContext,
    packet_limit: usize,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut buffer = vec![0u8; packet_limit + 1];
        loop {
            let received = tokio::select! {
                _ = association_cancel.cancelled() => break,
                _ = context.runtime_cancel.cancelled() => break,
                received = socket.recv_from(&mut buffer) => received,
            };
            let message = match received {
                Ok((length, source)) => {
                    if length > packet_limit {
                        continue;
                    }
                    context.counters.record_received(length);
                    Ok(UdpResponse {
                        source: crate::tcp::TcpTarget::address(source),
                        route: GeoRoute::Direct,
                        payload: bytes::Bytes::copy_from_slice(&buffer[..length]),
                        network_generation: context.network_generation,
                    })
                }
                Err(error) if crate::udp_io::is_message_too_long(&error) => continue,
                Err(error) => Err(UdpReceiveFailure::Direct {
                    message: format!("direct UDP receive failed: {error}"),
                    network_generation: context.network_generation,
                }),
            };
            let failed = message.is_err();
            let sent = tokio::select! {
                _ = association_cancel.cancelled() => break,
                _ = context.runtime_cancel.cancelled() => break,
                result = sender.send(message) => result,
            };
            if sent.is_err() || failed {
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

#[cfg(test)]
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

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
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
    connect_remote_inner(context, target, port).await
}

async fn connect_remote_inner(
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
        (geo_target, port, Some(&context.resolver)),
        (
            |reason| ConnectFailure {
                reply: REPLY_HOST_UNREACHABLE,
                message: reason.to_owned(),
            },
            || ConnectFailure {
                reply: REPLY_CONNECTION_NOT_ALLOWED,
                message: "routing_rejected".into(),
            },
        ),
        |resolved| async move {
            crate::tcp::with_ready_budget_tracked(
                &context.health,
                &context.cancellation,
                REMOTE_CONNECT_TIMEOUT,
                Some(&context.telemetry),
                |deadline| {
                    let resolved = resolved.clone();
                    async move {
                        if resolved.is_none()
                            && context.edge_resolved
                            && let Target::Domain(name) = target
                        {
                            let target =
                                crate::tcp::TcpTarget::new(name, port).map_err(connect_failure)?;
                            return context
                                .dialer
                                .connect(
                                    target,
                                    deadline,
                                    &context.cancellation,
                                    crate::tcp::FlowClass::Business,
                                )
                                .await
                                .map_err(connect_failure);
                        }
                        let resolution = if let Some(addresses) = resolved {
                            CandidateResolution::from_addresses(addresses)
                        } else {
                            match target {
                                Target::Address(address) => {
                                    CandidateResolution::from_addresses(vec![*address])
                                }
                                Target::Domain(name) => context
                                    .resolver
                                    .resolve_candidates(name, deadline)
                                    .map_err(|error| ConnectFailure {
                                        reply: REPLY_HOST_UNREACHABLE,
                                        message: error.to_string(),
                                    })?,
                            }
                        };
                        connect_tunnel_remote(context, resolution, port, deadline).await
                    }
                },
                connect_failure,
            )
            .await
        },
    )
    .await
}

async fn connect_tunnel_remote(
    context: &SocksContext,
    resolution: CandidateResolution,
    port: u16,
    deadline: tokio::time::Instant,
) -> Result<crate::tcp::TcpStream, ConnectFailure> {
    crate::tcp_candidates::connect_candidates(
        context.dialer.clone(),
        resolution,
        port,
        deadline,
        &context.cancellation,
    )
    .await
    .map_err(|error| match error {
        crate::tcp_candidates::CandidateDialError::Resolve(error) => ConnectFailure {
            reply: if error.to_string().contains("routing_rejected") {
                REPLY_CONNECTION_NOT_ALLOWED
            } else {
                REPLY_HOST_UNREACHABLE
            },
            message: error.to_string(),
        },
        crate::tcp_candidates::CandidateDialError::Dial(error) => connect_failure(error),
    })
}

fn connect_failure(error: crate::tcp::DialError) -> ConnectFailure {
    use crate::tcp::DialError;
    ConnectFailure {
        reply: match error {
            DialError::Refused => REPLY_CONNECTION_REFUSED,
            DialError::InvalidTarget => REPLY_ADDRESS_UNSUPPORTED,
            DialError::Rejected(401 | 403) => REPLY_CONNECTION_NOT_ALLOWED,
            DialError::Budget => REPLY_GENERAL_FAILURE,
            _ => REPLY_NETWORK_UNREACHABLE,
        },
        message: error.to_string(),
    }
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
mod dns_concurrency_tests;
#[cfg(test)]
mod dns_tests;

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
            (ip == Ipv4Addr::new(10, 0, 0, 2) || ip.is_loopback()) && country.as_str() == "CN"
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
        let mut tasks = vec![client_stack.spawn_tokio(), server_stack.spawn_tokio()];
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
        let (health_sender, health) = watch::channel(RuntimeHealth::Connected {
            path: RuntimePath {
                transport: usque_core::Transport::Http3,
                endpoint_family: usque_core::AddressFamily::Ipv4,
                ipv4_available: true,
                ipv6_available: true,
            },
            reconnect_count: 0,
        });
        tasks.push(tokio::spawn(async move {
            let _sender = health_sender;
            std::future::pending::<()>().await;
        }));
        let context = SocksContext {
            telemetry: crate::ConnectionTelemetry::default(),
            traffic_policy: Arc::default(),
            channel: Some(crate::proxy_udp::StackFactory::shared(
                channel.clone(),
                assigned_ipv4,
                Ipv6Addr::UNSPECIFIED,
            )),
            dialer: Arc::new(crate::tcp::StackDialer {
                channel: channel.clone(),
                ipv4: assigned_ipv4,
                ipv6: Ipv6Addr::LOCALHOST,
            }),
            edge_resolved: false,
            relay_buffer: crate::relay::RELAY_BUFFER_SIZE,
            admission: Some(crate::tcp::FrontendAdmission::for_connect_ip()),
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
            udp_idle_timeout: Duration::from_secs(10),
            cancellation: CancellationToken::new(),
            failure,
            health,
            auth: None,
        };
        (context, tunnel, server_channel, tasks)
    }

    #[tokio::test]
    async fn waiting_socks_client_close_releases_its_request() {
        let (mut context, _udp, _server, tasks) =
            test_socks_context(Arc::new(crate::socket::NoopSocketProtector)).await;
        context.geo_policy = Arc::new(GeoDirectPolicy::disabled());
        let (sender, health) = watch::channel(RuntimeHealth::Connecting {
            requested: context.health.borrow().path(),
            attempt: 1,
        });
        context.health = health;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            let (client, _) = listener.accept().await.unwrap();
            serve_connect(
                client,
                Arc::new(context),
                SocksRequest {
                    command: COMMAND_CONNECT,
                    target: Target::Address("10.0.0.2".parse().unwrap()),
                    port: 8080,
                },
            )
            .await
        });
        let client = TcpStream::connect(address).await.unwrap();
        tokio::time::sleep(Duration::from_millis(25)).await;
        assert!(!task.is_finished());
        drop(client);
        tokio::time::timeout(Duration::from_secs(1), task)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        drop(sender);
        for task in tasks {
            task.abort();
        }
    }

    #[tokio::test]
    async fn pipelined_socks_half_close_keeps_the_bounded_prefix_until_recovery() {
        for payload in [b"once".as_slice(), b"once-more".as_slice()] {
            let (mut context, _udp, server, tasks) =
                test_socks_context(Arc::new(crate::socket::NoopSocketProtector)).await;
            context.geo_policy = Arc::new(GeoDirectPolicy::disabled());
            context.relay_buffer = 4;
            let connected = context.health.borrow().clone();
            let (health, updates) = watch::channel(RuntimeHealth::Connecting {
                requested: connected.path(),
                attempt: 1,
            });
            context.health = updates;
            let cancel = context.cancellation.clone();
            let upstream = server
                .tcp_listen("10.0.0.2:8080".parse().unwrap())
                .await
                .unwrap();
            let expected = payload.to_vec();
            let origin = tokio::spawn(async move {
                let mut stream = upstream.accept().await.unwrap();
                let mut received = Vec::new();
                stream.read_to_end(&mut received).await.unwrap();
                assert_eq!(received, expected);
                stream.write_all(b"done").await.unwrap();
                stream.shutdown().await.unwrap();
            });
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let task = tokio::spawn(async move {
                let (client, _) = listener.accept().await.unwrap();
                serve_connect(
                    client,
                    Arc::new(context),
                    SocksRequest {
                        command: COMMAND_CONNECT,
                        target: Target::Address("10.0.0.2".parse().unwrap()),
                        port: 8080,
                    },
                )
                .await
            });
            let mut client = TcpStream::connect(address).await.unwrap();
            client.write_all(payload).await.unwrap();
            client.shutdown().await.unwrap();
            tokio::time::sleep(Duration::from_millis(25)).await;
            assert!(!task.is_finished());
            health.send_replace(connected);
            let mut reply = Vec::new();
            timeout(Duration::from_secs(3), client.read_to_end(&mut reply))
                .await
                .unwrap()
                .unwrap();
            assert_eq!(reply[1], REPLY_SUCCEEDED);
            assert_eq!(&reply[10..], b"done");
            origin.await.unwrap();
            task.await.unwrap().unwrap();
            cancel.cancel();
            for task in tasks {
                task.abort();
            }
        }
    }

    #[tokio::test]
    async fn socks_waits_thirty_minutes_and_keeps_established_relay_across_recovery() {
        let (mut context, _udp, server, tasks) =
            test_socks_context(Arc::new(crate::socket::NoopSocketProtector)).await;
        context.geo_policy = Arc::new(GeoDirectPolicy::disabled());
        let connected = context.health.borrow().clone();
        let pending = RuntimeHealth::Connecting {
            requested: connected.path(),
            attempt: 1,
        };
        let (health, updates) = watch::channel(pending.clone());
        context.health = updates;
        let cancel = context.cancellation.clone();
        let upstream = server
            .tcp_listen("10.0.0.2:8080".parse().unwrap())
            .await
            .unwrap();
        let origin = tokio::spawn(async move {
            let mut stream = upstream.accept().await.unwrap();
            for expected in [b"once", b"more"] {
                let mut payload = [0; 4];
                stream.read_exact(&mut payload).await.unwrap();
                assert_eq!(&payload, expected);
                stream.write_all(&payload).await.unwrap();
            }
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            let (client, _) = listener.accept().await.unwrap();
            serve_connect(
                client,
                Arc::new(context),
                SocksRequest {
                    command: COMMAND_CONNECT,
                    target: Target::Address("10.0.0.2".parse().unwrap()),
                    port: 8080,
                },
            )
            .await
        });
        let mut client = TcpStream::connect(address).await.unwrap();
        client.write_all(b"once").await.unwrap();
        tokio::time::sleep(Duration::from_millis(25)).await;
        tokio::time::pause();
        tokio::time::advance(Duration::from_secs(1_801)).await;
        assert!(!task.is_finished());
        tokio::time::resume();
        health.send_replace(connected.clone());
        let mut reply = [0; 10];
        tokio::time::timeout(Duration::from_secs(3), client.read_exact(&mut reply))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(reply[1], REPLY_SUCCEEDED);
        let mut payload = [0; 4];
        client.read_exact(&mut payload).await.unwrap();
        assert_eq!(&payload, b"once");
        health.send_replace(pending);
        client.write_all(b"more").await.unwrap();
        client.read_exact(&mut payload).await.unwrap();
        assert_eq!(&payload, b"more");
        health.send_replace(connected);
        origin.await.unwrap();
        cancel.cancel();
        let _ = task.await;
        for task in tasks {
            task.abort();
        }
    }

    #[tokio::test]
    async fn udp_association_retains_control_and_port_during_long_recovery() {
        let (mut context, _udp, server, tasks) =
            test_socks_context(Arc::new(crate::socket::NoopSocketProtector)).await;
        context.geo_policy = Arc::new(GeoDirectPolicy::disabled());
        let connected = context.health.borrow().clone();
        let (health, updates) = watch::channel(RuntimeHealth::Connecting {
            requested: connected.path(),
            attempt: 1,
        });
        context.health = updates;
        let upstream = server
            .udp_bind("10.0.0.2:9000".parse().unwrap())
            .await
            .unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            let (client, peer) = listener.accept().await.unwrap();
            serve_udp_association(
                client,
                peer,
                Arc::new(context),
                SocksRequest {
                    command: COMMAND_UDP_ASSOCIATE,
                    target: Target::Address("0.0.0.0".parse().unwrap()),
                    port: 0,
                },
            )
            .await
        });
        let mut control = TcpStream::connect(address).await.unwrap();
        let mut reply = [0; 10];
        control.read_exact(&mut reply).await.unwrap();
        assert_eq!(reply[1], 0);
        let port = u16::from_be_bytes([reply[8], reply[9]]);
        tokio::time::pause();
        tokio::time::advance(Duration::from_secs(1_801)).await;
        assert!(!task.is_finished());
        tokio::time::resume();
        health.send_replace(connected);
        let udp = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
        udp.send_to(
            &[0, 0, 0, 1, 10, 0, 0, 2, 0x23, 0x28, 42],
            SocketAddr::from(([127, 0, 0, 1], port)),
        )
        .await
        .unwrap();
        let mut data = [0; 16];
        let (_, count) =
            tokio::time::timeout(Duration::from_secs(2), upstream.recv_from(&mut data))
                .await
                .unwrap()
                .unwrap();
        assert_eq!(&data[..count], &[42]);
        drop(control);
        tokio::time::timeout(Duration::from_secs(1), task)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        for task in tasks {
            task.abort();
        }
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

    #[tokio::test]
    async fn allow_lan_socks_connect_and_udp_attempt_protected_direct_egress() {
        use crate::geo_direct::{LanProbeProtector, lan_test_policy};
        for allow_lan in [false, true] {
            let protector = Arc::new(LanProbeProtector::default());
            let (mut context, tunnel, server, tasks) = test_socks_context(protector.clone()).await;
            context.geo_policy = Arc::new(lan_test_policy(allow_lan));
            let remote: SocketAddr = "10.0.0.2:9000".parse().unwrap();
            let tcp = server.tcp_listen(remote).await.unwrap();
            let stream =
                connect_remote(&context, &Target::Address(remote.ip()), remote.port()).await;
            assert!(matches!(stream, Ok(RoutedTcpStream::Tunnel(_))));
            let _accepted = tcp.accept().await.unwrap();
            let udp = server.udp_bind(remote).await.unwrap();
            let direct = DirectUdpSockets::new(protector.as_ref());
            send_udp_routed(
                &context,
                &Target::Address(remote.ip()),
                remote.port(),
                b"lan",
                &direct,
                TunnelUdpSockets::Stack {
                    v4: &tunnel,
                    v6: &tunnel,
                },
            )
            .await
            .unwrap();
            let (_, payload) = timeout(Duration::from_secs(1), udp.recv_from_bytes())
                .await
                .unwrap()
                .unwrap();
            assert_eq!(&payload[..], b"lan");
            let expected = if allow_lan {
                vec![(remote, DirectProtocol::Tcp), (remote, DirectProtocol::Udp)]
            } else {
                vec![]
            };
            assert_eq!(*protector.attempts.lock().unwrap(), expected);
            for task in tasks {
                task.abort();
            }
        }
    }

    #[tokio::test]
    async fn unavailable_direct_family_never_enters_socks_udp_fallback() {
        struct RecordedUdp(Mutex<Vec<crate::tcp::TcpTarget>>);
        #[async_trait::async_trait]
        impl crate::proxy_udp::UdpAssociation for RecordedUdp {
            async fn send(
                &self,
                target: &crate::tcp::TcpTarget,
                _: &[u8],
            ) -> Result<(), crate::tcp::DialError> {
                self.0.lock().await.push(target.clone());
                Ok(())
            }
            async fn recv(
                &self,
            ) -> Result<(crate::tcp::TcpTarget, bytes::Bytes), crate::tcp::DialError> {
                std::future::pending().await
            }
        }
        for with_ip_rules in [false, true] {
            for mixed in [false, true] {
                let ipv6: IpAddr = "2001:db8::7".parse().unwrap();
                let ipv4: IpAddr = "192.0.2.7".parse().unwrap();
                let protector = Arc::new(crate::geo_direct::LanProbeProtector {
                    family_mask: Some(1),
                    resolved: Some(if mixed { vec![ipv6, ipv4] } else { vec![ipv6] }),
                    ..Default::default()
                });
                let (mut context, _, _, tasks) = test_socks_context(protector.clone()).await;
                let mut entries = vec![("direct.test", usque_core::RoutingAction::Direct)];
                if with_ip_rules {
                    entries.push(("2001:db8::7", usque_core::RoutingAction::Direct));
                }
                context.geo_policy = Arc::new(crate::geo_direct::routing_test_policy(&entries));
                context.edge_resolved = true;
                let association = RecordedUdp(Mutex::new(vec![]));
                let _ = send_udp_routed(
                    &context,
                    &Target::Domain("direct.test".into()),
                    9000,
                    b"direct",
                    &DirectUdpSockets::default(),
                    TunnelUdpSockets::Association(&association),
                )
                .await;
                let sent = association.0.lock().await;
                assert_eq!(sent.len(), usize::from(mixed));
                if mixed {
                    assert_eq!(sent[0].socket_address(), Some(SocketAddr::new(ipv4, 9000)));
                }
                assert!(protector.attempts.lock().unwrap().is_empty());
                for task in tasks {
                    task.abort();
                }
            }
        }
    }

    #[tokio::test]
    async fn rejected_connect_and_udp_never_reach_a_target() {
        let protector = Arc::new(TestProtector {
            resolved: "127.0.0.1:9000".parse().unwrap(),
            protect_calls: AtomicUsize::new(0),
            resolve_calls: AtomicUsize::new(0),
            reject: false,
        });
        let (mut context, tunnel, server, tasks) = test_socks_context(protector.clone()).await;
        context.geo_policy = Arc::new(crate::geo_direct::routing_test_policy(&[
            ("blocked.test", usque_core::RoutingAction::Reject),
            ("10.0.0.2", usque_core::RoutingAction::Reject),
        ]));
        let origin = server
            .udp_bind("10.0.0.2:9000".parse().unwrap())
            .await
            .unwrap();
        for target in [
            Target::Domain("blocked.test".into()),
            Target::Address("10.0.0.2".parse().unwrap()),
        ] {
            let result = connect_remote(&context, &target, 9000).await;
            assert!(matches!(
                result,
                Err(ConnectFailure {
                    reply: REPLY_CONNECTION_NOT_ALLOWED,
                    ..
                })
            ));
            send_udp_routed(
                &context,
                &target,
                9000,
                b"blocked",
                &DirectUdpSockets::default(),
                TunnelUdpSockets::Stack {
                    v4: &tunnel,
                    v6: &tunnel,
                },
            )
            .await
            .unwrap();
        }
        assert_eq!(protector.resolve_calls.load(Ordering::SeqCst), 0);
        assert!(
            tokio::time::timeout(Duration::from_millis(30), origin.recv_from(&mut [0_u8; 32]))
                .await
                .is_err()
        );
        for task in tasks {
            task.abort();
        }
    }

    #[tokio::test]
    async fn raw_udp_dns_replaces_a_blocked_cname_reply_with_refused() {
        let (mut context, _tunnel, server, tasks) =
            test_socks_context(Arc::new(crate::socket::NoopSocketProtector)).await;
        context.geo_policy = Arc::new(crate::geo_direct::routing_test_policy(&[(
            "blocked.test",
            usque_core::RoutingAction::Reject,
        )]));
        let cancellation = context.cancellation.clone();
        let dns = server
            .udp_bind("10.0.0.2:53".parse().unwrap())
            .await
            .unwrap();
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let mut control = TcpStream::connect(listener.local_addr().unwrap())
            .await
            .unwrap();
        let (accepted, peer) = listener.accept().await.unwrap();
        let worker = tokio::spawn(serve_udp_association(
            accepted,
            peer,
            Arc::new(context),
            SocksRequest {
                command: COMMAND_UDP_ASSOCIATE,
                target: Target::Address(Ipv4Addr::UNSPECIFIED.into()),
                port: 0,
            },
        ));
        let mut header = [0; 10];
        control.read_exact(&mut header).await.unwrap();
        assert_eq!(&header[..4], &[5, 0, 0, 1]);
        let relay = SocketAddr::new(
            Ipv4Addr::LOCALHOST.into(),
            u16::from_be_bytes([header[8], header[9]]),
        );
        let udp = TokioUdpSocket::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let query = crate::split_dns::test_host_query("allowed.test");
        let mut request = vec![0, 0, 0, ADDRESS_IPV4, 10, 0, 0, 2, 0, 53];
        request.extend(&query);
        udp.send_to(&request, relay).await.unwrap();
        let (peer, received) = timeout(Duration::from_secs(1), dns.recv_from_bytes())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(&received[..], &query);
        dns.send_to(
            peer,
            &crate::split_dns::test_cname_response(&query, &["blocked.test".into()]),
        )
        .await
        .unwrap();
        let mut response = [0; 4096];
        let (length, _) = timeout(Duration::from_secs(1), udp.recv_from(&mut response))
            .await
            .unwrap()
            .unwrap();
        let parsed = decode_udp_request(&response[..length]).unwrap();
        assert_eq!(parsed.payload[3] & 15, 5);
        assert_eq!(&parsed.payload[..2], &query[..2]);
        drop(control);
        timeout(Duration::from_secs(1), worker)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        cancellation.cancel();
        for task in tasks {
            task.abort();
        }
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
    async fn full_direct_reply_queue_does_not_block_association_cleanup() {
        for cancel_runtime in [false, true] {
            let socket = Arc::new(
                TokioUdpSocket::bind((Ipv4Addr::LOCALHOST, 0))
                    .await
                    .unwrap(),
            );
            let peer = TokioUdpSocket::bind((Ipv4Addr::LOCALHOST, 0))
                .await
                .unwrap();
            let (sender, _receiver) = mpsc::channel(1);
            sender
                .try_send(Ok(UdpResponse {
                    source: crate::tcp::TcpTarget::address(peer.local_addr().unwrap()),
                    payload: bytes::Bytes::new(),
                    route: GeoRoute::Direct,
                    network_generation: None,
                }))
                .unwrap_or_else(|_| panic!("reply queue must start full"));
            let association_cancel = CancellationToken::new();
            let runtime_cancel = CancellationToken::new();
            let counters = Arc::new(TrafficCounters::default());
            let task = spawn_direct_udp_receiver(
                socket.clone(),
                sender,
                association_cancel.clone(),
                DirectUdpReceiveContext {
                    runtime_cancel: runtime_cancel.clone(),
                    counters: counters.clone(),
                    network_generation: None,
                },
                MAX_UDP_DATAGRAM,
            );
            peer.send_to(b"queued", socket.local_addr().unwrap())
                .await
                .unwrap();
            timeout(Duration::from_secs(1), async {
                while counters.snapshot().bytes_received == 0 {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .unwrap();
            if cancel_runtime {
                runtime_cancel.cancel();
            } else {
                association_cancel.cancel();
            }
            timeout(Duration::from_secs(1), task)
                .await
                .unwrap()
                .unwrap();
        }
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
        let association_cancel = CancellationToken::new();
        let (response_tx, mut response_rx) = mpsc::channel(1);
        let receiver = spawn_direct_udp_receiver(
            Arc::clone(direct.v4.as_ref().unwrap()),
            response_tx,
            association_cancel.clone(),
            DirectUdpReceiveContext {
                runtime_cancel: context.cancellation.clone(),
                counters: context.counters.clone(),
                network_generation: direct.network_generation,
            },
            MAX_UDP_DATAGRAM,
        );

        send_udp_routed(
            &context,
            &Target::Domain("direct.test".to_owned()),
            server.local_addr().unwrap().port(),
            b"direct",
            &direct,
            TunnelUdpSockets::Stack {
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
        assert_eq!(
            response.source.socket_address(),
            Some(server.local_addr().unwrap())
        );
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
            TunnelUdpSockets::Stack {
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
        assert_eq!(source.ip(), IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)));
        assert_eq!(&payload[..], b"fallback");
        for task in tasks {
            task.abort();
        }
        assert!(direct.v4.is_none());
        assert!(protector.protect_calls.load(Ordering::SeqCst) >= 1);
    }

    #[tokio::test]
    async fn quic_hot_policy_preserves_geo_direct_socket_and_both_reply_families() {
        for ip in [
            IpAddr::V4(Ipv4Addr::LOCALHOST),
            IpAddr::V6(Ipv6Addr::LOCALHOST),
        ] {
            let server = TokioUdpSocket::bind(SocketAddr::new(ip, 0)).await.unwrap();
            let direct_port = server.local_addr().unwrap().port();
            let protector = Arc::new(TestProtector {
                resolved: server.local_addr().unwrap(),
                reject: false,
                protect_calls: AtomicUsize::new(0),
                resolve_calls: AtomicUsize::new(0),
            });
            let (mut context, tunnel, _, tasks) = test_socks_context(protector.clone()).await;
            context.traffic_policy = Arc::new(
                crate::application_traffic::ApplicationTrafficPolicy::for_loopback_quic(
                    direct_port,
                ),
            );
            let direct = DirectUdpSockets::new(protector.as_ref());
            let (tx, mut rx) = mpsc::channel(4);
            let cancel = CancellationToken::new();
            let receiver = spawn_direct_udp_receiver(
                direct
                    .for_address(server.local_addr().unwrap())
                    .unwrap()
                    .clone(),
                tx,
                cancel.clone(),
                DirectUdpReceiveContext {
                    runtime_cancel: context.cancellation.clone(),
                    counters: context.counters.clone(),
                    network_generation: direct.network_generation,
                },
                MAX_UDP_DATAGRAM,
            );
            let mut original_peer = None;
            for blocked in [false, true, false, true] {
                context.traffic_policy.set_disable_quic(blocked);
                assert_eq!(context.traffic_policy.blocks_udp(direct_port), blocked);
                for target in [Target::Domain("direct.test".into()), Target::Address(ip)] {
                    send_udp_routed(
                        &context,
                        &target,
                        direct_port,
                        b"quic",
                        &direct,
                        TunnelUdpSockets::Stack {
                            v4: &tunnel,
                            v6: &tunnel,
                        },
                    )
                    .await
                    .unwrap();
                    let mut buffer = [0; 16];
                    let (len, peer) =
                        timeout(Duration::from_secs(1), server.recv_from(&mut buffer))
                            .await
                            .unwrap()
                            .unwrap();
                    assert_eq!(&buffer[..len], b"quic");
                    assert_eq!(
                        *original_peer.get_or_insert(peer),
                        peer,
                        "live GEO socket must be retained"
                    );
                    server.send_to(b"reply", peer).await.unwrap();
                    let response = timeout(Duration::from_secs(1), rx.recv())
                        .await
                        .unwrap()
                        .unwrap()
                        .unwrap();
                    assert_eq!(response.route, GeoRoute::Direct);
                    assert_eq!(
                        response.source.socket_address(),
                        Some(server.local_addr().unwrap())
                    );
                    assert!(!response.blocked_by(&context.traffic_policy));
                    assert_eq!(&response.payload[..], b"reply");
                }
            }
            assert!(!context.cancellation.is_cancelled());
            cancel.cancel();
            receiver.await.unwrap();
            for task in tasks {
                task.abort();
            }
        }
    }

    #[tokio::test]
    async fn quic_fallback_is_blocked_without_closing_udp_context_or_resolving_tunnel_dns() {
        let protector = Arc::new(TestProtector {
            resolved: SocketAddr::from((Ipv4Addr::LOCALHOST, 443)),
            reject: true,
            protect_calls: AtomicUsize::new(0),
            resolve_calls: AtomicUsize::new(0),
        });
        let (context, tunnel, channel, tasks) = test_socks_context(protector.clone()).await;
        let target = Ipv4Addr::new(10, 0, 0, 2);
        let server = channel
            .udp_bind(SocketAddr::from((target, 443)))
            .await
            .unwrap();
        let direct = DirectUdpSockets::new(protector.as_ref());
        for blocked in [false, true, false] {
            context.traffic_policy.set_disable_quic(blocked);
            send_udp_routed(
                &context,
                &Target::Address(target.into()),
                443,
                b"quic",
                &direct,
                TunnelUdpSockets::Stack {
                    v4: &tunnel,
                    v6: &tunnel,
                },
            )
            .await
            .unwrap();
            let received = timeout(Duration::from_millis(100), server.recv_from_bytes()).await;
            assert_eq!(received.is_err(), blocked);
            if let Ok(Ok((_, payload))) = received {
                assert_eq!(&payload[..], b"quic");
            }
            let response = UdpResponse {
                source: crate::tcp::TcpTarget::address(SocketAddr::from((target, 443))),
                payload: bytes::Bytes::new(),
                route: GeoRoute::Tunnel,
                network_generation: None,
            };
            assert_eq!(response.blocked_by(&context.traffic_policy), blocked);
        }
        context.traffic_policy.set_disable_quic(true);
        send_udp_routed(
            &context,
            &Target::Domain("unmatched.test".into()),
            443,
            b"quic",
            &direct,
            TunnelUdpSockets::Stack {
                v4: &tunnel,
                v6: &tunnel,
            },
        )
        .await
        .unwrap();
        assert_eq!(protector.resolve_calls.load(Ordering::SeqCst), 0);
        let dns = channel
            .udp_bind(SocketAddr::from((target, 53)))
            .await
            .unwrap();
        send_udp_routed(
            &context,
            &Target::Address(target.into()),
            53,
            b"dns",
            &direct,
            TunnelUdpSockets::Stack {
                v4: &tunnel,
                v6: &tunnel,
            },
        )
        .await
        .unwrap();
        assert_eq!(
            &timeout(Duration::from_secs(1), dns.recv_from_bytes())
                .await
                .unwrap()
                .unwrap()
                .1[..],
            b"dns"
        );
        assert!(!context.cancellation.is_cancelled());
        for task in tasks {
            task.abort();
        }
    }

    #[tokio::test]
    async fn udp_dns_uses_a_fresh_query_revision_and_rejects_a_reply_from_before_publication() {
        let (mut context, _, channel, tasks) =
            test_socks_context(Arc::new(crate::socket::NoopSocketProtector)).await;
        let _tasks = tasks
            .into_iter()
            .map(tokio_util::task::AbortOnDropHandle::new)
            .collect::<Vec<_>>();
        context.geo_policy = Arc::new(GeoDirectPolicy::disabled());
        let policy = context.geo_policy.clone();
        let _cancel = context.cancellation.clone().drop_guard();
        let dns = channel
            .udp_bind("10.0.0.2:53".parse().unwrap())
            .await
            .unwrap();
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let mut control = TcpStream::connect(listener.local_addr().unwrap())
            .await
            .unwrap();
        let (accepted, peer) = listener.accept().await.unwrap();
        let worker = tokio_util::task::AbortOnDropHandle::new(tokio::spawn(serve_udp_association(
            accepted,
            peer,
            Arc::new(context),
            SocksRequest {
                command: COMMAND_UDP_ASSOCIATE,
                target: Target::Address(Ipv4Addr::UNSPECIFIED.into()),
                port: 0,
            },
        )));
        let mut header = [0; 10];
        control.read_exact(&mut header).await.unwrap();
        assert_eq!(&header[..4], &[5, 0, 0, 1]);
        let relay = SocketAddr::from((
            Ipv4Addr::LOCALHOST,
            u16::from_be_bytes([header[8], header[9]]),
        ));
        let client = TokioUdpSocket::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        for (index, name) in ["before.test", "after.test"].into_iter().enumerate() {
            let query = crate::split_dns::test_host_query(name);
            let mut packet = vec![0, 0, 0, ADDRESS_IPV4, 10, 0, 0, 2, 0, 53];
            packet.extend_from_slice(&query);
            client.send_to(&packet, relay).await.unwrap();
            let (source, received) = timeout(Duration::from_secs(1), dns.recv_from_bytes())
                .await
                .unwrap()
                .unwrap();
            assert_eq!(&received[..], &query);
            if index == 0 {
                hot_udp_rules(&policy, "unrelated.test", usque_core::RoutingAction::Reject);
            }
            dns.send_to(
                source,
                &crate::split_dns::test_cname_response(&query, &["safe.test".into()]),
            )
            .await
            .unwrap();
            let mut bytes = [0; 4096];
            let (length, _) = timeout(Duration::from_secs(1), client.recv_from(&mut bytes))
                .await
                .unwrap()
                .unwrap();
            let response = decode_udp_request(&bytes[..length]).unwrap();
            assert_eq!(response.payload[3] & 0x0f, if index == 0 { 2 } else { 0 });
        }
        drop(control);
        timeout(Duration::from_secs(2), worker)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
    }

    #[derive(Default)]
    struct HotUdpAssociation {
        sent: StdMutex<Vec<crate::tcp::TcpTarget>>,
        fail: std::sync::atomic::AtomicBool,
    }

    #[async_trait::async_trait]
    impl crate::proxy_udp::UdpAssociation for HotUdpAssociation {
        fn accounts_traffic(&self) -> bool {
            false
        }
        async fn send(
            &self,
            target: &crate::tcp::TcpTarget,
            _: &[u8],
        ) -> Result<(), crate::tcp::DialError> {
            if self.fail.load(Ordering::SeqCst) {
                return Err(crate::tcp::DialError::Closed);
            }
            self.sent.lock().unwrap().push(target.clone());
            Ok(())
        }
        async fn recv(
            &self,
        ) -> Result<(crate::tcp::TcpTarget, bytes::Bytes), crate::tcp::DialError> {
            std::future::pending().await
        }
    }

    struct HotUdpProtector {
        generation: std::sync::atomic::AtomicU64,
        reject_target: std::sync::atomic::AtomicBool,
        target_attempts: AtomicUsize,
    }

    #[async_trait::async_trait]
    impl SocketProtector for HotUdpProtector {
        fn protect(&self, _: SocketHandle) -> Result<(), String> {
            Ok(())
        }
        fn network_generation(&self) -> Option<u64> {
            Some(self.generation.load(Ordering::SeqCst))
        }
        async fn protect_for_target(
            &self,
            _: SocketHandle,
            _: SocketAddr,
            _: DirectProtocol,
        ) -> Result<DirectEgressLease, String> {
            self.target_attempts.fetch_add(1, Ordering::SeqCst);
            if self.reject_target.load(Ordering::SeqCst) {
                Err("test target rejected".into())
            } else {
                Ok(DirectEgressLease::for_generation(
                    self.generation.load(Ordering::SeqCst),
                ))
            }
        }
    }

    fn hot_udp_rules(policy: &GeoDirectPolicy, target: &str, action: usque_core::RoutingAction) {
        let profile = Profile {
            allow_lan: false,
            routing: usque_core::RoutingSettings {
                rules: vec![usque_core::RoutingRule {
                    id: uuid::Uuid::new_v4(),
                    kind: if target.parse::<IpAddr>().is_ok() {
                        usque_core::RoutingMatch::Cidr
                    } else {
                        usque_core::RoutingMatch::Domain
                    },
                    target: target.into(),
                    action,
                }],
                ads_enabled: false,
            },
            ..Default::default()
        };
        policy
            .publish_routing_update(policy.prepare_routing_update(&profile).unwrap())
            .unwrap();
    }

    async fn hot_udp_send(
        context: &SocksContext,
        direct: &DirectUdpSockets,
        association: &HotUdpAssociation,
        remote: SocketAddr,
    ) -> Result<(), String> {
        send_udp_routed(
            context,
            &Target::Address(remote.ip()),
            remote.port(),
            b"flow",
            direct,
            TunnelUdpSockets::Association(association),
        )
        .await
    }

    #[tokio::test]
    async fn udp_hot_rules_keep_established_paths_and_apply_to_new_targets_in_both_families() {
        for ip in [
            IpAddr::V4(Ipv4Addr::LOCALHOST),
            IpAddr::V6(Ipv6Addr::LOCALHOST),
        ] {
            let server = TokioUdpSocket::bind(SocketAddr::new(ip, 0)).await.unwrap();
            let other = TokioUdpSocket::bind(SocketAddr::new(ip, 0)).await.unwrap();
            let blocked = TokioUdpSocket::bind(SocketAddr::new(ip, 0)).await.unwrap();
            let (mut context, _, _, tasks) =
                test_socks_context(Arc::new(crate::socket::NoopSocketProtector)).await;
            let _tasks = tasks
                .into_iter()
                .map(tokio_util::task::AbortOnDropHandle::new)
                .collect::<Vec<_>>();
            context.geo_policy = Arc::new(GeoDirectPolicy::disabled());
            let direct = DirectUdpSockets::new(context.protector.as_ref());
            let association = HotUdpAssociation::default();
            hot_udp_send(&context, &direct, &association, other.local_addr().unwrap())
                .await
                .unwrap();
            hot_udp_rules(
                &context.geo_policy,
                &ip.to_string(),
                usque_core::RoutingAction::Direct,
            );
            hot_udp_send(&context, &direct, &association, other.local_addr().unwrap())
                .await
                .unwrap();
            assert_eq!(
                association.sent.lock().unwrap().len(),
                2,
                "old Tunnel flow remains tunneled"
            );
            hot_udp_send(
                &context,
                &direct,
                &association,
                server.local_addr().unwrap(),
            )
            .await
            .unwrap();
            let mut bytes = [0; 16];
            let (_, first_peer) = timeout(Duration::from_secs(1), server.recv_from(&mut bytes))
                .await
                .unwrap()
                .unwrap();
            hot_udp_rules(
                &context.geo_policy,
                &ip.to_string(),
                usque_core::RoutingAction::Reject,
            );
            hot_udp_send(
                &context,
                &direct,
                &association,
                server.local_addr().unwrap(),
            )
            .await
            .unwrap();
            let (_, same_peer) = timeout(Duration::from_secs(1), server.recv_from(&mut bytes))
                .await
                .unwrap()
                .unwrap();
            assert_eq!(
                same_peer, first_peer,
                "old Direct flow keeps its exact socket and target"
            );
            hot_udp_send(
                &context,
                &direct,
                &association,
                blocked.local_addr().unwrap(),
            )
            .await
            .unwrap();
            assert!(
                timeout(Duration::from_millis(30), blocked.recv_from(&mut bytes))
                    .await
                    .is_err()
            );
            assert_eq!(association.sent.lock().unwrap().len(), 2);
        }
    }

    #[tokio::test]
    async fn successful_udp_fallback_stays_tunneled_when_direct_later_becomes_available() {
        let server = TokioUdpSocket::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let protector = Arc::new(HotUdpProtector {
            generation: 1.into(),
            reject_target: true.into(),
            target_attempts: 0.into(),
        });
        let (mut context, _, _, tasks) = test_socks_context(protector.clone()).await;
        let _tasks = tasks
            .into_iter()
            .map(tokio_util::task::AbortOnDropHandle::new)
            .collect::<Vec<_>>();
        context.geo_policy = Arc::new(crate::geo_direct::routing_test_policy(&[(
            "127.0.0.1",
            usque_core::RoutingAction::Direct,
        )]));
        let direct = DirectUdpSockets::new(protector.as_ref());
        let association = HotUdpAssociation::default();
        let remote = server.local_addr().unwrap();
        hot_udp_send(&context, &direct, &association, remote)
            .await
            .unwrap();
        protector.reject_target.store(false, Ordering::SeqCst);
        hot_udp_send(&context, &direct, &association, remote)
            .await
            .unwrap();
        assert_eq!(protector.target_attempts.load(Ordering::SeqCst), 1);
        assert_eq!(association.sent.lock().unwrap().len(), 2);
        assert!(
            timeout(Duration::from_millis(30), server.recv_from(&mut [0; 16]))
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn udp_generation_changes_reselect_routes_and_cancel_an_admission_waiter() {
        let protector = Arc::new(HotUdpProtector {
            generation: 1.into(),
            reject_target: false.into(),
            target_attempts: 0.into(),
        });
        let (mut context, _, _, tasks) = test_socks_context(protector.clone()).await;
        let _tasks = tasks
            .into_iter()
            .map(tokio_util::task::AbortOnDropHandle::new)
            .collect::<Vec<_>>();
        context.geo_policy = Arc::new(GeoDirectPolicy::disabled());
        let direct = DirectUdpSockets::default();
        let association = HotUdpAssociation::default();
        let remote: SocketAddr = "127.0.0.1:9000".parse().unwrap();
        hot_udp_send(&context, &direct, &association, remote)
            .await
            .unwrap();
        let target = Target::Address(remote.ip());
        let slot = direct
            .target_route(&context, &target, remote.port())
            .unwrap();
        let locked = slot.sending.lock().await;
        let request = SocksUdpRequest {
            target,
            port: remote.port(),
            payload: b"pending",
        };
        let mut pending = Box::pin(send_udp_with_slot(
            &context,
            &request,
            &direct,
            TunnelUdpSockets::Association(&association),
            Some(slot.clone()),
        ));
        tokio::select! {
            biased;
            result = &mut pending => panic!("pending target bypassed its send lock: {result:?}"),
            () = tokio::task::yield_now() => {}
        }
        hot_udp_rules(
            &context.geo_policy,
            "127.0.0.1",
            usque_core::RoutingAction::Reject,
        );
        protector.generation.store(2, Ordering::SeqCst);
        drop(locked);
        assert_eq!(
            pending.await.unwrap_err(),
            crate::socket::STALE_GENERATION_REASON
        );
        hot_udp_send(&context, &direct, &association, remote)
            .await
            .unwrap();
        assert_eq!(association.sent.lock().unwrap().len(), 1);
    }

    struct LeaseDropCount(Arc<AtomicUsize>);
    impl Drop for LeaseDropCount {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    struct PausedUdpAuthorization {
        generation: std::sync::atomic::AtomicU64,
        entered: tokio::sync::Notify,
        release: tokio::sync::Notify,
        dropped: Arc<AtomicUsize>,
    }

    #[async_trait::async_trait]
    impl SocketProtector for PausedUdpAuthorization {
        fn protect(&self, _: SocketHandle) -> Result<(), String> {
            Ok(())
        }
        fn network_generation(&self) -> Option<u64> {
            Some(self.generation.load(Ordering::SeqCst))
        }
        async fn protect_for_target_generation(
            &self,
            _: SocketHandle,
            _: SocketAddr,
            _: DirectProtocol,
            generation: u64,
        ) -> Result<DirectEgressLease, String> {
            // Model an already authorized reply arriving after migration.
            self.entered.notify_one();
            self.release.notified().await;
            Ok(DirectEgressLease::hold_for_generation(
                LeaseDropCount(self.dropped.clone()),
                generation,
            ))
        }
    }

    #[tokio::test]
    async fn late_udp_authorization_drops_its_lease_without_erasing_a_new_epoch() {
        let old_dropped = Arc::new(AtomicUsize::new(0));
        let protector = PausedUdpAuthorization {
            generation: 1.into(),
            entered: tokio::sync::Notify::new(),
            release: tokio::sync::Notify::new(),
            dropped: old_dropped.clone(),
        };
        let direct = DirectUdpSockets::new(&protector);
        let old: SocketAddr = "127.0.0.1:9000".parse().unwrap();
        let newer: SocketAddr = "127.0.0.1:9001".parse().unwrap();
        let mut pending = Box::pin(direct.ensure_target(&protector, old));
        tokio::select! {
            () = protector.entered.notified() => {}
            result = &mut pending => panic!("authorization must stay pending: {result:?}"),
        }
        protector.generation.store(2, Ordering::SeqCst);
        let new_dropped = Arc::new(AtomicUsize::new(0));
        direct.leases.lock().await.insert(
            (Some(2), newer),
            DirectEgressLease::hold_for_generation(LeaseDropCount(new_dropped.clone()), 2),
        );
        protector.release.notify_one();
        assert_eq!(
            pending.await.unwrap_err(),
            crate::socket::STALE_GENERATION_REASON
        );
        let leases = direct.leases.lock().await;
        assert!(leases.contains_key(&(Some(2), newer)));
        assert!(!leases.contains_key(&(Some(1), old)));
        assert_eq!(old_dropped.load(Ordering::SeqCst), 1);
        assert_eq!(new_dropped.load(Ordering::SeqCst), 0);
        drop(leases);
        drop(direct);
        assert_eq!(new_dropped.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn queued_direct_udp_replies_keep_the_socket_epoch_and_require_a_successful_direct_target()
     {
        let server = TokioUdpSocket::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let protector = Arc::new(HotUdpProtector {
            generation: 1.into(),
            reject_target: false.into(),
            target_attempts: 0.into(),
        });
        let (mut context, _, _, tasks) = test_socks_context(protector.clone()).await;
        let _tasks = tasks
            .into_iter()
            .map(tokio_util::task::AbortOnDropHandle::new)
            .collect::<Vec<_>>();
        context.geo_policy = Arc::new(crate::geo_direct::routing_test_policy(&[(
            "127.0.0.1",
            usque_core::RoutingAction::Direct,
        )]));
        let direct = DirectUdpSockets::new(protector.as_ref());
        let association = HotUdpAssociation::default();
        hot_udp_send(
            &context,
            &direct,
            &association,
            server.local_addr().unwrap(),
        )
        .await
        .unwrap();
        let (_, peer) = timeout(Duration::from_secs(1), server.recv_from(&mut [0; 16]))
            .await
            .unwrap()
            .unwrap();
        let (sender, mut replies) = mpsc::channel(2);
        let cancel = CancellationToken::new();
        let receiver = tokio_util::task::AbortOnDropHandle::new(spawn_direct_udp_receiver(
            direct.v4.as_ref().unwrap().clone(),
            sender,
            cancel.clone(),
            DirectUdpReceiveContext {
                runtime_cancel: context.cancellation.clone(),
                counters: context.counters.clone(),
                network_generation: direct.network_generation,
            },
            MAX_UDP_DATAGRAM,
        ));
        server.send_to(b"queued", peer).await.unwrap();
        let queued = timeout(Duration::from_secs(1), replies.recv())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(queued.network_generation, Some(1));
        assert!(
            direct
                .authorizes_reply(
                    protector.as_ref(),
                    server.local_addr().unwrap(),
                    queued.network_generation
                )
                .await
        );
        protector.generation.store(2, Ordering::SeqCst);
        assert!(
            !direct
                .authorizes_reply(
                    protector.as_ref(),
                    server.local_addr().unwrap(),
                    queued.network_generation
                )
                .await
        );
        server.send_to(b"late old port", peer).await.unwrap();
        let late = timeout(Duration::from_secs(1), replies.recv())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(
            late.network_generation,
            Some(1),
            "read-time current generation must not relabel an old socket"
        );
        let next = DirectUdpSockets::new(protector.as_ref());
        assert!(
            !next
                .authorizes_reply(
                    protector.as_ref(),
                    server.local_addr().unwrap(),
                    queued.network_generation
                )
                .await
        );
        let remote = server.local_addr().unwrap();
        // A lease granted during an unsuccessful Direct attempt is insufficient
        // to authorize a reply from a target that actually selected Tunnel.
        next.ensure_target(protector.as_ref(), remote)
            .await
            .unwrap();
        assert!(
            !next
                .authorizes_reply(protector.as_ref(), remote, Some(2))
                .await
        );
        cancel.cancel();
        receiver.await.unwrap();
    }

    #[tokio::test]
    async fn udp_association_migration_rebuilds_both_direct_ports_and_keeps_the_local_relay() {
        for ip in [
            IpAddr::V4(Ipv4Addr::LOCALHOST),
            IpAddr::V6(Ipv6Addr::LOCALHOST),
        ] {
            let server = TokioUdpSocket::bind(SocketAddr::new(ip, 0)).await.unwrap();
            let protector = Arc::new(HotUdpProtector {
                generation: 1.into(),
                reject_target: false.into(),
                target_attempts: 0.into(),
            });
            let (mut context, _, _, tasks) = test_socks_context(protector.clone()).await;
            let _tasks = tasks
                .into_iter()
                .map(tokio_util::task::AbortOnDropHandle::new)
                .collect::<Vec<_>>();
            context.geo_policy = Arc::new(crate::geo_direct::routing_test_policy(&[(
                &ip.to_string(),
                usque_core::RoutingAction::Direct,
            )]));
            let _cancel = context.cancellation.clone().drop_guard();
            let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
            let mut control = TcpStream::connect(listener.local_addr().unwrap())
                .await
                .unwrap();
            let (accepted, peer) = listener.accept().await.unwrap();
            let worker =
                tokio_util::task::AbortOnDropHandle::new(tokio::spawn(serve_udp_association(
                    accepted,
                    peer,
                    Arc::new(context),
                    SocksRequest {
                        command: COMMAND_UDP_ASSOCIATE,
                        target: Target::Address(Ipv4Addr::UNSPECIFIED.into()),
                        port: 0,
                    },
                )));
            let mut header = [0; 10];
            control.read_exact(&mut header).await.unwrap();
            assert_eq!(&header[..4], &[5, 0, 0, ADDRESS_IPV4]);
            let relay = SocketAddr::from((
                Ipv4Addr::LOCALHOST,
                u16::from_be_bytes([header[8], header[9]]),
            ));
            let client = TokioUdpSocket::bind((Ipv4Addr::LOCALHOST, 0))
                .await
                .unwrap();
            let mut packet = vec![0, 0, 0];
            crate::proxy_exit::encode_target(
                &crate::tcp::TcpTarget::address(server.local_addr().unwrap()),
                &mut packet,
            );
            packet.extend_from_slice(b"request");
            let mut old_peer = None;
            for generation in [1, 2] {
                protector.generation.store(generation, Ordering::SeqCst);
                client.send_to(&packet, relay).await.unwrap();
                let (length, current_peer) =
                    timeout(Duration::from_secs(2), server.recv_from(&mut [0; 32]))
                        .await
                        .unwrap()
                        .unwrap();
                assert_eq!(length, 7);
                if let Some(old) = old_peer {
                    assert_ne!(
                        old, current_peer,
                        "new epoch requires a distinct native port"
                    );
                    server.send_to(b"stale", old).await.unwrap();
                }
                server.send_to(b"current", current_peer).await.unwrap();
                let mut reply = [0; 128];
                let (length, source) =
                    timeout(Duration::from_secs(2), client.recv_from(&mut reply))
                        .await
                        .unwrap()
                        .unwrap();
                assert_eq!(source, relay);
                assert_eq!(
                    decode_udp_request(&reply[..length]).unwrap().payload,
                    b"current"
                );
                old_peer = Some(current_peer);
            }
            drop(control);
            timeout(Duration::from_secs(2), worker)
                .await
                .unwrap()
                .unwrap()
                .unwrap();
        }
    }

    #[tokio::test]
    async fn udp_target_state_is_bounded_expires_and_does_not_pin_failed_sends() {
        let (mut context, _, _, tasks) =
            test_socks_context(Arc::new(crate::socket::NoopSocketProtector)).await;
        let _tasks = tasks
            .into_iter()
            .map(tokio_util::task::AbortOnDropHandle::new)
            .collect::<Vec<_>>();
        context.geo_policy = Arc::new(GeoDirectPolicy::disabled());
        let direct = DirectUdpSockets::default();
        let association = HotUdpAssociation::default();
        for port in 1..=MAX_UDP_TARGETS as u16 {
            hot_udp_send(
                &context,
                &direct,
                &association,
                SocketAddr::from((Ipv4Addr::LOCALHOST, port)),
            )
            .await
            .unwrap();
        }
        let remote = SocketAddr::from((Ipv4Addr::LOCALHOST, MAX_UDP_TARGETS as u16 + 1));
        assert!(
            hot_udp_send(&context, &direct, &association, remote)
                .await
                .is_err()
        );
        assert_eq!(
            direct.target_routes.lock().unwrap().entries.len(),
            MAX_UDP_TARGETS
        );
        tokio::time::pause();
        tokio::time::advance(context.udp_idle_timeout + Duration::from_secs(1)).await;
        tokio::time::resume();
        association.fail.store(true, Ordering::SeqCst);
        assert!(
            hot_udp_send(&context, &direct, &association, remote)
                .await
                .is_err()
        );
        association.fail.store(false, Ordering::SeqCst);
        hot_udp_rules(
            &context.geo_policy,
            "127.0.0.1",
            usque_core::RoutingAction::Reject,
        );
        hot_udp_send(&context, &direct, &association, remote)
            .await
            .unwrap();
        assert_eq!(association.sent.lock().unwrap().len(), MAX_UDP_TARGETS);
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
