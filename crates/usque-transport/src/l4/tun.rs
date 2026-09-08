//! TCP-only TUN bridge. Unhandled traffic is consumed, never returned to a
//! packet tunnel or a physical UDP socket. DNS is parsed before any dial.
use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bytes::{Bytes, BytesMut};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::{Semaphore, mpsc};
use tokio::task::{JoinHandle, JoinSet};
use tokio::time::{Instant, timeout};
use tokio_util::sync::CancellationToken;
use ts_netstack_smoltcp::netcore::{
    Config, HasChannel, NetstackControl, TcpBufferPolicy, TcpBufferTier,
};
use usque_core::Profile;

use super::stream::BufferLease;
use super::tun_stream::{OnceListener, TunStream};
use super::tun_wire::{TcpReset, reply_allowed, udp_response, udp_unreachable, valid_transport};
use super::{BufferBudget, L4Metrics, Limits};
use crate::direct_gateway::{NatPacket, rewrite_destination, rewrite_source};
use crate::geo_direct::{GeoRoute, RoutedTcpStream, connect_direct_ip};
use crate::h2::TransportError;
use crate::split_dns::{SPLIT_DNS_IPV4, SPLIT_DNS_IPV6, SplitDnsResolver};
use crate::tcp::{FlowClass, ProxyServices, TcpTarget};

const PACKETS: usize = 64;

#[derive(Clone, Copy, Hash, PartialEq, Eq)]
struct Key {
    client: SocketAddr,
    remote: SocketAddr,
}
struct Mapping {
    id: u64,
    gateway: SocketAddr,
    cancel: CancellationToken,
    reset: TcpReset,
    half_open: bool,
    direct: bool,
    generation: Option<u64>,
}
#[derive(Default)]
struct Flows {
    forward: HashMap<Key, Mapping>,
    reverse: HashMap<(SocketAddr, SocketAddr), Key>,
    next: u16,
    serial: u64,
}

pub(crate) struct TunBridge {
    pub(crate) outgoing: mpsc::Sender<Bytes>,
    incoming: Option<mpsc::Receiver<Bytes>>,
    cancellation: CancellationToken,
    tasks: Vec<JoinHandle<()>>,
    metrics: Arc<L4Metrics>,
    _reservations: Vec<BufferLease>,
    mtu: usize,
}

pub(crate) struct L4TunIo {
    metrics: Arc<L4Metrics>,
    outgoing: mpsc::Sender<Bytes>,
    incoming: mpsc::Receiver<Bytes>,
    cancellation: CancellationToken,
    mtu: usize,
}

impl L4TunIo {
    pub(crate) async fn send_owned_packet(&self, packet: Bytes) -> Result<(), TransportError> {
        if self.cancellation.is_cancelled() {
            return Err(TransportError::TunnelClosed);
        }
        if crate::h2::validate_ip_packet(&packet).is_err() || packet.len() > self.mtu {
            self.metrics.update(|m| m.unsupported_packets += 1);
            return Ok(());
        }
        tokio::select! {
            _ = self.cancellation.cancelled() => Err(TransportError::TunnelClosed),
            result = self.outgoing.send(packet) => result.map_err(|_| TransportError::TunnelClosed),
        }
    }
    pub(crate) async fn receive_packet(&mut self) -> Result<Bytes, TransportError> {
        tokio::select! {
            _ = self.cancellation.cancelled() => Err(TransportError::TunnelClosed),
            result = self.incoming.recv() => result.ok_or(TransportError::TunnelClosed),
        }
    }
    pub(crate) fn try_receive_packet(&mut self) -> Result<Option<Bytes>, TransportError> {
        match self.incoming.try_recv() {
            Ok(v) => Ok(Some(v)),
            Err(mpsc::error::TryRecvError::Empty) => Ok(None),
            Err(_) => Err(TransportError::TunnelClosed),
        }
    }
}
impl Drop for L4TunIo {
    fn drop(&mut self) {
        self.cancellation.cancel();
    }
}

impl TunBridge {
    pub(crate) async fn start(
        profile: &Profile,
        services: ProxyServices,
        dns: Arc<crate::dns_stream::StreamDns>,
        budget: Arc<BufferBudget>,
        metrics: Arc<L4Metrics>,
        quality: crate::NetworkQualityTelemetry,
    ) -> Result<Self, TransportError> {
        let limits = Limits::platform();
        let stack_budget = limits.buffers / 4;
        let packet_budget = limits.buffers / 8;
        let stack_reservation = budget
            .reserve_admission(stack_budget)
            .ok_or(TransportError::SendQueueFull)?;
        let packet_reservation = budget
            .reserve_admission(packet_budget)
            .ok_or(TransportError::SendQueueFull)?;
        let cancellation = services.cancellation.child_token();
        let config = Config {
            mtu: usize::from(profile.mtu),
            command_channel_capacity: Some(256),
            tcp_buffer_size: 64 << 10,
            tcp_nagle_enabled: false,
            tcp_listener_budgeted: true,
            tcp_buffer_policy: Some(TcpBufferPolicy {
                preferred: TcpBufferTier {
                    receive: 256 << 10,
                    transmit: 256 << 10,
                },
                fallback: TcpBufferTier {
                    receive: 64 << 10,
                    transmit: 64 << 10,
                },
                preferred_budget: stack_budget / 2,
                total_budget: stack_budget,
            }),
            ..Config::default()
        };
        let (stack, pipe) = crate::netstack::bounded_piped_with_capacity(config, PACKETS);
        let channel = stack.command_channel();
        let stack_task = tokio_util::task::AbortOnDropHandle::new(stack.spawn_tokio());
        channel
            .set_ips([IpAddr::V4(SPLIT_DNS_IPV4), IpAddr::V6(SPLIT_DNS_IPV6)])
            .await
            .map_err(|_| TransportError::Netstack("L4 TUN addresses unavailable".to_owned()))?;
        let resolver = Arc::new(SplitDnsResolver::for_l4(
            dns.clone(),
            &profile.dns_servers,
            services.geo_policy.clone(),
            services.protector.clone(),
            quality,
        ));
        let flows = Arc::new(Mutex::new(Flows::default()));
        let (outgoing, mut packets) = mpsc::channel::<Bytes>(PACKETS);
        let (response_tx, incoming) = mpsc::channel::<Bytes>(PACKETS);
        let ts_netstack_smoltcp::WakingPipe {
            tx: stack_incoming,
            mut rx,
        } = pipe;
        let receive_flows = flows.clone();
        let receive_cancel = cancellation.clone();
        let receive_tx = response_tx.clone();
        let incoming_task = tokio::spawn(async move {
            loop {
                let packet = tokio::select! { _ = receive_cancel.cancelled() => break, packet = rx.recv_async() => packet };
                let Some(packet) = packet else {
                    break;
                };
                let mut packet = packet
                    .try_into_mut()
                    .unwrap_or_else(|p| BytesMut::from(p.as_ref()));
                let Some(meta) = NatPacket::parse(&packet) else {
                    continue;
                };
                let key = receive_flows
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .reverse
                    .get(&(
                        SocketAddr::new(meta.source, meta.source_port),
                        SocketAddr::new(meta.destination, meta.destination_port),
                    ))
                    .copied();
                if let Some(key) = key
                    && rewrite_source(&mut packet, &meta, key.remote)
                {
                    tokio::select! { _ = receive_cancel.cancelled() => break, _ = receive_tx.send(packet.freeze()) => {} }
                }
            }
        });
        let task_cancel = cancellation.clone();
        let mtu = usize::from(profile.mtu);
        let pump_metrics = metrics.clone();
        let task = tokio::spawn(async move {
            let mut jobs = JoinSet::new();
            let dns_permits = Arc::new(Semaphore::new(80));
            let tcp_permits = Arc::new(Semaphore::new(limits.active));
            let mut sweep = tokio::time::interval(Duration::from_secs(1));
            let mut icmp_window = Instant::now();
            let mut icmp_count = 0u16;
            loop {
                let packet = tokio::select! {
                    _ = task_cancel.cancelled() => break,
                    _ = jobs.join_next(), if !jobs.is_empty() => continue,
                    _ = sweep.tick() => {
                        let generation = services.protector.network_generation();
                        for mapping in flows.lock().unwrap_or_else(|e| e.into_inner()).forward.values() {
                            if mapping.direct && mapping.generation != generation { mapping.cancel.cancel(); }
                        }
                        dns.prune(); continue;
                    }
                    packet = packets.recv() => match packet { Some(p) => p, None => break },
                };
                let Some(meta) = NatPacket::parse(&packet) else {
                    pump_metrics.update(|m| m.unsupported_packets += 1);
                    continue;
                };
                if !valid_transport(&packet, &meta) || !reply_allowed(&meta) {
                    pump_metrics.update(|m| m.unsupported_packets += 1);
                    continue;
                }
                if meta.protocol == 17 {
                    let query = &packet[meta.transport_offset + 8..];
                    if meta.destination_port == 53
                        && crate::split_dns::validate_query_bytes(query).is_ok()
                    {
                        let Ok(permit) = dns_permits.clone().try_acquire_owned() else {
                            continue;
                        };
                        let dns = dns.clone();
                        let resolver = resolver.clone();
                        let replies = response_tx.clone();
                        let cancel = task_cancel.clone();
                        jobs.spawn(async move {
                            let _permit = permit;
                            let response = tokio::select! {
                                _ = cancel.cancelled() => return,
                                response = dns_query(&resolver, &dns, &meta, &packet[meta.transport_offset + 8..]) => response,
                            };
                            let response = crate::split_dns::limit_udp_response(&packet[meta.transport_offset + 8..], response, mtu.saturating_sub(meta.transport_offset + 8));
                            let wire = udp_response(&meta, &response);
                            tokio::select! { _ = cancel.cancelled() => {}, _ = replies.send(wire) => {} }
                        });
                    } else {
                        pump_metrics.update(|m| m.udp_rejected += 1);
                        if icmp_window.elapsed() >= Duration::from_secs(1) {
                            icmp_window = Instant::now();
                            icmp_count = 0;
                        }
                        if icmp_count < 32
                            && let Some(error) = udp_unreachable(&packet, &meta)
                        {
                            icmp_count += 1;
                            let _ = response_tx.try_send(error);
                        }
                    }
                    continue;
                }
                let Some(reset) = TcpReset::from_packet(&packet, &meta) else {
                    pump_metrics.update(|m| m.unsupported_packets += 1);
                    continue;
                };
                let key = Key {
                    client: SocketAddr::new(meta.source, meta.source_port),
                    remote: SocketAddr::new(meta.destination, meta.destination_port),
                };
                let existing = {
                    let mut table = flows.lock().unwrap_or_else(|e| e.into_inner());
                    table.forward.get_mut(&key).map(|mapping| {
                        mapping.reset = reset.clone();
                        mapping.gateway
                    })
                };
                let gateway = if let Some(gateway) = existing {
                    gateway
                } else {
                    if !reset.syn() {
                        if let Some(response) = reset.response() {
                            let _ = response_tx.try_send(response);
                        }
                        continue;
                    }
                    let Ok(permit) = tcp_permits.clone().try_acquire_owned() else {
                        if let Some(response) = reset.response() {
                            let _ = response_tx.try_send(response);
                        }
                        pump_metrics.update(|m| m.budget_rejections += 1);
                        continue;
                    };
                    let (gateway, id) = {
                        let mut table = flows.lock().unwrap_or_else(|e| e.into_inner());
                        let mut port = None;
                        for _ in 0..16383 {
                            table.next = if table.next < 49152 || table.next >= 65534 {
                                49152
                            } else {
                                table.next + 1
                            };
                            if !table.forward.values().any(|m| {
                                m.gateway.port() == table.next
                                    && m.gateway.is_ipv4() == key.client.is_ipv4()
                            }) {
                                port = Some(table.next);
                                break;
                            }
                        }
                        let Some(port) = port else {
                            continue;
                        };
                        table.serial = table.serial.wrapping_add(1);
                        let ip = if key.client.is_ipv4() {
                            IpAddr::V4(SPLIT_DNS_IPV4)
                        } else {
                            IpAddr::V6(SPLIT_DNS_IPV6)
                        };
                        (SocketAddr::new(ip, port), table.serial)
                    };
                    let listener = tokio::select! { _ = task_cancel.cancelled() => break, listener = OnceListener::bind(channel.clone(), gateway) => listener };
                    let Ok(listener) = listener else {
                        if let Some(response) = reset.response() {
                            let _ = response_tx.try_send(response);
                        }
                        pump_metrics.update(|m| m.budget_rejections += 1);
                        continue;
                    };
                    let generation = services.protector.network_generation();
                    let route = resolver.hints().route_ip(
                        key.remote.ip(),
                        generation,
                        &services.geo_policy,
                    );
                    let cancel = task_cancel.child_token();
                    {
                        let mut table = flows.lock().unwrap_or_else(|e| e.into_inner());
                        table.reverse.insert((gateway, key.client), key);
                        table.forward.insert(
                            key,
                            Mapping {
                                id,
                                gateway,
                                cancel: cancel.clone(),
                                reset,
                                half_open: true,
                                // The configured route may fall back to L4.
                                // Physical-generation invalidation starts only
                                // after a real direct socket is established.
                                direct: false,
                                generation,
                            },
                        );
                    }
                    pump_metrics.update(|m| {
                        m.tun_flows += 1;
                        m.half_open_flows += 1;
                    });
                    let services = services.clone();
                    let resolver = resolver.clone();
                    let table = flows.clone();
                    let replies = response_tx.clone();
                    let stats = pump_metrics.clone();
                    let budget = budget.clone();
                    jobs.spawn(async move {
                        let _permit = permit;
                        let guard = FlowGuard {
                            table,
                            key,
                            id,
                            stats,
                            replies,
                        };
                        let result = run_tcp(
                            listener, key, route, &services, &resolver, &budget, &cancel, &guard,
                        )
                        .await;
                        if result.is_err() {
                            guard.reset();
                        }
                    });
                    gateway
                };
                let mut packet = packet
                    .try_into_mut()
                    .unwrap_or_else(|p| BytesMut::from(p.as_ref()));
                if rewrite_destination(&mut packet, &meta, gateway) {
                    tokio::select! { _ = task_cancel.cancelled() => break, _ = stack_incoming.send_async(&packet) => {} }
                }
            }
            task_cancel.cancel();
            jobs.abort_all();
            while jobs.join_next().await.is_some() {}
            pump_metrics.update(|m| {
                m.tun_flows = 0;
                m.half_open_flows = 0;
            });
        });
        Ok(Self {
            outgoing,
            incoming: Some(incoming),
            cancellation,
            tasks: vec![stack_task.detach(), incoming_task, task],
            metrics,
            _reservations: vec![stack_reservation, packet_reservation],
            mtu,
        })
    }

    pub(crate) fn attach(&mut self) -> Result<L4TunIo, TransportError> {
        Ok(L4TunIo {
            metrics: self.metrics.clone(),
            outgoing: self.outgoing.clone(),
            incoming: self.incoming.take().ok_or(TransportError::TunnelClosed)?,
            cancellation: self.cancellation.clone(),
            mtu: self.mtu,
        })
    }
    pub(crate) fn cancel(&self) {
        self.cancellation.cancel();
    }
    pub(crate) async fn shutdown(&mut self) {
        self.cancel();
        for task in self.tasks.drain(..) {
            task.abort();
            let _ = task.await;
        }
    }
}
impl Drop for TunBridge {
    fn drop(&mut self) {
        self.cancel();
        for task in &self.tasks {
            task.abort();
        }
    }
}

struct FlowGuard {
    table: Arc<Mutex<Flows>>,
    key: Key,
    id: u64,
    stats: Arc<L4Metrics>,
    replies: mpsc::Sender<Bytes>,
}
impl FlowGuard {
    fn outbound(&self, stream: &RoutedTcpStream) {
        if let Some(mapping) = self
            .table
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .forward
            .get_mut(&self.key)
            && mapping.id == self.id
        {
            mapping.direct = matches!(stream, RoutedTcpStream::Direct { .. });
            if let RoutedTcpStream::Direct { _lease, .. } = stream {
                mapping.generation = _lease.generation();
            }
        }
    }

    fn accepted(&self) {
        if let Some(mapping) = self
            .table
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .forward
            .get_mut(&self.key)
            && mapping.id == self.id
            && mapping.half_open
        {
            mapping.half_open = false;
            self.stats
                .update(|m| m.half_open_flows = m.half_open_flows.saturating_sub(1));
        }
    }
    fn reset(&self) {
        if let Some(mapping) = self
            .table
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .forward
            .get(&self.key)
            && mapping.id == self.id
            && let Some(packet) = mapping.reset.response()
        {
            let _ = self.replies.try_send(packet);
        }
    }
}
impl Drop for FlowGuard {
    fn drop(&mut self) {
        let mut table = self.table.lock().unwrap_or_else(|e| e.into_inner());
        if table
            .forward
            .get(&self.key)
            .is_some_and(|m| m.id == self.id)
            && let Some(mapping) = table.forward.remove(&self.key)
        {
            table.reverse.remove(&(mapping.gateway, self.key.client));
            mapping.cancel.cancel();
            self.stats.update(|m| {
                m.tun_flows = m.tun_flows.saturating_sub(1);
                if mapping.half_open {
                    m.half_open_flows = m.half_open_flows.saturating_sub(1);
                }
            });
        }
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "one TUN flow owns its route, streams, cancellation and cleanup"
)]
async fn run_tcp(
    listener: OnceListener,
    key: Key,
    route: GeoRoute,
    services: &ProxyServices,
    dns: &SplitDnsResolver,
    budget: &Arc<BufferBudget>,
    cancellation: &CancellationToken,
    guard: &FlowGuard,
) -> Result<(), ()> {
    let mut local = tokio::select! { _ = cancellation.cancelled() => return Err(()), accepted = timeout(Duration::from_secs(10), listener.accept(key.client)) => accepted.map_err(|_| ())?.map_err(|_| ())? };
    guard.accepted();
    if key.remote.port() == 53
        && matches!(
            key.remote.ip(),
            IpAddr::V4(SPLIT_DNS_IPV4) | IpAddr::V6(SPLIT_DNS_IPV6)
        )
    {
        let _dns_buffers = budget.reserve_admission(2 * 65536).ok_or(())?;
        serve_tcp_dns(&mut local, dns, cancellation).await?;
        tokio::select! { _ = cancellation.cancelled() => {}, _ = tokio::time::sleep(Duration::from_secs(2)) => {} }
        return Ok(());
    }
    let _relay = budget
        .reserve_admission(Limits::platform().relay * 2)
        .ok_or(())?;
    let mut remote = tokio::select! {
        _ = cancellation.cancelled() => return Err(()),
        result = timeout(Duration::from_secs(10), connect_target(key.remote, route, services)) => result.map_err(|_| ())??,
    };
    guard.outbound(&remote);
    tokio::select! {
        _ = cancellation.cancelled() => return Err(()),
        result = tokio::io::copy_bidirectional_with_sizes(&mut local, &mut remote, Limits::platform().relay, Limits::platform().relay) => { result.map_err(|_| ())?; }
    }
    // Allow the local FIN/ACK exchange before reclaiming the one-shot socket.
    drop(remote);
    tokio::select! { _ = cancellation.cancelled() => {}, _ = tokio::time::sleep(Duration::from_secs(2)) => {} }
    Ok(())
}

async fn connect_target(
    remote: SocketAddr,
    route: GeoRoute,
    services: &ProxyServices,
) -> Result<RoutedTcpStream, ()> {
    if route == GeoRoute::Direct
        && let Ok((stream, lease)) = connect_direct_ip(services.protector.as_ref(), remote).await
    {
        return Ok(RoutedTcpStream::Direct {
            stream,
            counters: services.counters.clone(),
            _lease: lease,
        });
    }
    services
        .dialer
        .connect(
            TcpTarget::address(remote),
            Instant::now() + Duration::from_secs(10),
            &services.cancellation,
            FlowClass::Business,
        )
        .await
        .map(RoutedTcpStream::Tunnel)
        .map_err(|_| ())
}

async fn dns_query(
    resolver: &SplitDnsResolver,
    dns: &crate::dns_stream::StreamDns,
    meta: &NatPacket,
    query: &[u8],
) -> Vec<u8> {
    if matches!(
        meta.destination,
        IpAddr::V4(SPLIT_DNS_IPV4) | IpAddr::V6(SPLIT_DNS_IPV6)
    ) {
        resolver.handle_l4(query, true).await
    } else {
        dns.query(
            SocketAddr::new(meta.destination, 53),
            query,
            Instant::now() + Duration::from_secs(4),
        )
        .await
        .unwrap_or_else(|_| crate::split_dns::l4_dns_error(query))
    }
}

async fn serve_tcp_dns(
    local: &mut TunStream,
    dns: &SplitDnsResolver,
    cancellation: &CancellationToken,
) -> Result<(), ()> {
    loop {
        let operation = async {
            let mut prefix = [0u8; 2];
            if local.read(&mut prefix[..1]).await.map_err(|_| ())? == 0 {
                local.shutdown().await.map_err(|_| ())?;
                return Ok(false);
            }
            local.read_exact(&mut prefix[1..]).await.map_err(|_| ())?;
            let length = u16::from_be_bytes(prefix) as usize;
            if length < 12 {
                return Err(());
            }
            let mut query = vec![0u8; length];
            local.read_exact(&mut query).await.map_err(|_| ())?;
            let response = dns.handle_l4(&query, false).await;
            local
                .write_u16(response.len() as u16)
                .await
                .map_err(|_| ())?;
            local.write_all(&response).await.map_err(|_| ())?;
            Ok(true)
        };
        let more = tokio::select! { _ = cancellation.cancelled() => return Err(()), result = timeout(Duration::from_secs(30), operation) => result.map_err(|_| ())?? };
        if !more {
            return Ok(());
        }
    }
}
