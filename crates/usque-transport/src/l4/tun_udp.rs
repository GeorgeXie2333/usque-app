//! Per-application UDP workers. The packet pump never awaits a relay handshake.
use super::performance::MeasuredSender;
use super::tun_reject::UdpRejector;
use crate::direct_gateway::NatPacket;
use crate::geo_direct::{GeoRoute, bind_protected_udp};
use crate::split_dns::DnsRouteCache;
use crate::tcp::{DialError, ProxyServices, TcpTarget};
use bytes::Bytes;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Semaphore, mpsc};
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;
use tokio_util::task::{AbortOnDropHandle, TaskTracker};

#[cfg(test)]
#[path = "tun_udp/tests.rs"]
mod tests;

pub(super) struct UdpFlows {
    senders: HashMap<SocketAddr, mpsc::Sender<(NatPacket, Bytes)>>,
    idle: Duration,
    hints: Arc<DnsRouteCache>,
    rejector: UdpRejector,
}
impl UdpFlows {
    pub(super) fn new(idle: u32, hints: Arc<DnsRouteCache>, rejector: UdpRejector) -> Self {
        Self {
            senders: HashMap::new(),
            idle: Duration::from_secs(u64::from(idle.max(1))),
            hints,
            rejector,
        }
    }
    #[expect(
        clippy::too_many_arguments,
        reason = "UDP ownership shares the TUN worker lifecycle and resource budgets"
    )]
    pub(super) fn enqueue(
        &mut self,
        meta: NatPacket,
        packet: Bytes,
        services: &ProxyServices,
        replies: &MeasuredSender,
        permits: &Arc<Semaphore>,
        budget: &Arc<super::BufferBudget>,
        tracker: &TaskTracker,
        cancel: &CancellationToken,
        mtu: usize,
    ) -> bool {
        self.senders.retain(|_, s| !s.is_closed());
        let key = SocketAddr::new(meta.source, meta.source_port);
        if !self.senders.contains_key(&key) {
            let Ok(permit) = permits.clone().try_acquire_owned() else {
                return false;
            };
            let Some(lease) = budget.reserve_admission(32 * mtu) else {
                return false;
            };
            let (sender, receiver) = mpsc::channel(16);
            let services = services.clone();
            let replies = replies.clone();
            let cancel = cancel.child_token();
            let idle = self.idle;
            let hints = self.hints.clone();
            let rejector = self.rejector.clone();
            let budget = budget.clone();
            tracker.spawn(async move {
                let _permit = permit;
                let _lease = lease;
                let guard = cancel.clone().drop_guard();
                let work = worker(
                    receiver, services, hints, replies, rejector, budget, &cancel, idle, mtu,
                );
                tokio::select! { _ = cancel.cancelled() => {}, _ = work => {} }
                drop(guard);
            });
            self.senders.insert(key, sender);
        }
        self.senders
            .get(&key)
            .is_some_and(|s| s.try_send((meta, packet)).is_ok())
    }
}

enum UdpEvent {
    Datagram(SocketAddr, Bytes, GeoRoute),
    AssociationClosed(DialError),
}

#[expect(
    clippy::too_many_arguments,
    reason = "UDP worker owns the shared DNS route cache and TUN lifecycle resources"
)]
async fn worker(
    mut packets: mpsc::Receiver<(NatPacket, Bytes)>,
    services: ProxyServices,
    hints: Arc<DnsRouteCache>,
    replies: MeasuredSender,
    rejector: UdpRejector,
    budget: Arc<super::BufferBudget>,
    cancel: &CancellationToken,
    idle: Duration,
    mtu: usize,
) {
    let mut association: Option<Arc<dyn crate::proxy_udp::UdpAssociation>> = None;
    let mut association_reader = None;
    let mut targets: HashMap<SocketAddr, (NatPacket, Instant)> = HashMap::new();
    // Only one outstanding quote per bounded target. Charge retained bytes to
    // the same runtime budget and discard them after a reply or idle expiry.
    let mut pending_quotes: HashMap<SocketAddr, Bytes> = HashMap::new();
    let mut direct = HashMap::new();
    let (tx, mut rx) = mpsc::channel::<UdpEvent>(16);
    let generation = services.protector.network_generation();
    let mut last = Instant::now();
    let mut tick = tokio::time::interval(Duration::from_secs(1));
    let mut failed = false;
    loop {
        tokio::select! {
            _ = cancel.cancelled() => break,
            _ = tick.tick() => {
                if last.elapsed() >= idle || generation != services.protector.network_generation() { break; }
                targets.retain(|_, (_, time)| time.elapsed() < idle);
                direct.retain(|a, _| targets.contains_key(a));
                pending_quotes.retain(|a, _| targets.contains_key(a));
            }
            packet = packets.recv() => {
                let Some((meta, packet)) = packet else { break; };
                if generation != services.protector.network_generation() { break; }
                let remote = SocketAddr::new(meta.destination, meta.destination_port);
                let route = hints.route_ip(remote.ip(), generation, &services.geo_policy);
                if route == GeoRoute::Reject { rejector.reject(&packet, &meta); continue; }
                if route != GeoRoute::Direct && services.traffic_policy.blocks_udp(remote.port()) {
                    rejector.reject(&packet, &meta); continue;
                }
                if targets.len() >= 256 && !targets.contains_key(&remote) {
                    rejector.reject(&packet, &meta); continue;
                }
                let payload = &packet[meta.transport_offset + 8..];
                if route == GeoRoute::Direct {
                    if let std::collections::hash_map::Entry::Vacant(entry) = direct.entry(remote)
                        && let Ok(socket) = bind_protected_udp(services.protector.as_ref(), remote.is_ipv6()) {
                        let socket = Arc::new(socket);
                        if let Ok(lease) = services.protector.protect_for_target_generation(crate::socket::socket_handle(socket.as_ref()), remote, crate::socket::DirectProtocol::Udp, generation.unwrap_or_default()).await {
                            if generation != services.protector.network_generation() { break; }
                            let read = socket.clone(); let tx = tx.clone(); let cancel = cancel.clone();
                            let task = AbortOnDropHandle::new(tokio::spawn(async move {
                                let mut buffer = vec![0; mtu];
                                loop {
                                    let result = tokio::select! { _ = cancel.cancelled() => break, r = read.recv_from(&mut buffer) => r };
                                    let Ok((n, source)) = result else { break; };
                                    if source == remote {
                                        tokio::select! { _ = cancel.cancelled() => break, r = tx.send(UdpEvent::Datagram(source, Bytes::copy_from_slice(&buffer[..n]), GeoRoute::Direct)) => if r.is_err() { break; } }
                                    }
                                }
                            }));
                            entry.insert((socket, lease, task));
                        }
                    }
                    if let Some((socket, _, _)) = direct.get(&remote)
                        && socket.send_to(payload, remote).await.is_ok() {
                        targets.insert(remote, (meta, Instant::now()));
                        last = Instant::now();
                        services.counters.record_sent(payload.len()); continue;
                    }
                    if !targets.contains_key(&remote) {
                        direct.remove(&remote);
                    }
                }
                if services.traffic_policy.blocks_udp(remote.port()) {
                    rejector.reject(&packet, &meta); continue;
                }
                let Some(quote) = budget.copy_admission(&packet) else {
                    rejector.reject(&packet, &meta); continue;
                };
                if association.is_none() {
                    let Some(factory) = &services.udp else {
                        rejector.reject(&packet, &meta); continue;
                    };
                    let opened = match factory.open(cancel, Instant::now() + Duration::from_secs(10)).await {
                        Ok(opened) => opened,
                        Err(error) => {
                            if error != DialError::Cancelled && !cancel.is_cancelled() {
                                rejector.reject(&packet, &meta); failed = true;
                            }
                            break;
                        }
                    };
                    let read = opened.clone(); let tx = tx.clone(); let cancel = cancel.clone();
                    association_reader = Some(AbortOnDropHandle::new(tokio::spawn(async move {
                        let work = async { loop {
                            match read.recv().await {
                                Ok((source, payload)) => {
                                    if let Some(source) = source.socket_address()
                                        && tx.send(UdpEvent::Datagram(source, payload, GeoRoute::Tunnel)).await.is_err() { break; }
                                }
                                Err(error) => {
                                    let _ = tx.send(UdpEvent::AssociationClosed(error)).await;
                                    break;
                                }
                            }
                        }};
                        tokio::select! { _ = cancel.cancelled() => {}, _ = work => {} }
                    })));
                    association = Some(opened);
                }
                if let Some(association) = &association {
                    match tokio::time::timeout(Duration::from_secs(10), association.send(&TcpTarget::address(remote), payload)).await {
                        Ok(Ok(())) => {
                            targets.insert(remote, (meta, Instant::now()));
                            last = Instant::now();
                            services.counters.record_sent(payload.len());
                            pending_quotes.insert(remote, quote);
                        }
                        Ok(Err(DialError::Cancelled)) => break,
                        _ => {
                            if !cancel.is_cancelled() {
                                rejector.reject(&packet, &meta); failed = true;
                            }
                            break;
                        },
                    }
                }
            }
            response = rx.recv() => {
                let (source, payload, route) = match response {
                    Some(UdpEvent::Datagram(source, payload, route)) => (source, payload, route),
                    Some(UdpEvent::AssociationClosed(error)) => {
                        failed = error != DialError::Cancelled;
                        break;
                    }
                    None => break,
                };
                if generation != services.protector.network_generation() { break; }
                let Some((meta,_)) = targets.get(&source) else { continue; };
                pending_quotes.remove(&source);
                if route == GeoRoute::Tunnel && services.traffic_policy.blocks_udp(source.port()) { continue; }
                if payload.len() + meta.transport_offset + 8 > mtu { continue; }
                services.counters.record_received(payload.len());
                if replies.send(super::tun_wire::udp_response(meta, &payload)).await.is_err() { break; }
                last = Instant::now();
            }
        }
    }
    drop(association_reader);
    if failed && !cancel.is_cancelled() {
        for packet in pending_quotes.into_values() {
            if let Some(meta) = NatPacket::parse(&packet) {
                rejector.reject(&packet, &meta);
            }
        }
        // Closing first prevents enqueues racing an unbounded drain. The
        // caller rejects new packets; this worker owns at most sixteen more.
        packets.close();
        for _ in 0..16 {
            let Ok((meta, packet)) = packets.try_recv() else {
                break;
            };
            rejector.reject(&packet, &meta);
        }
    }
}
