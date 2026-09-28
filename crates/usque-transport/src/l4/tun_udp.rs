//! Per-application UDP workers. The packet pump never awaits a relay handshake.
use super::performance::MeasuredSender;
use crate::direct_gateway::NatPacket;
use crate::geo_direct::{GeoRoute, bind_protected_udp};
use crate::tcp::{ProxyServices, TcpTarget};
use bytes::Bytes;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Semaphore, mpsc};
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;
use tokio_util::task::{AbortOnDropHandle, TaskTracker};

pub(super) struct UdpFlows {
    senders: HashMap<SocketAddr, mpsc::Sender<(NatPacket, Bytes)>>,
    idle: Duration,
}
impl UdpFlows {
    pub(super) fn new(idle: u32) -> Self {
        Self {
            senders: HashMap::new(),
            idle: Duration::from_secs(u64::from(idle.max(1))),
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
            tracker.spawn(async move {
                let _permit = permit;
                let _lease = lease;
                let guard = cancel.clone().drop_guard();
                let work = worker(receiver, services, replies, &cancel, idle, mtu);
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

async fn worker(
    mut packets: mpsc::Receiver<(NatPacket, Bytes)>,
    services: ProxyServices,
    replies: MeasuredSender,
    cancel: &CancellationToken,
    idle: Duration,
    mtu: usize,
) {
    let mut association: Option<Arc<dyn crate::proxy_udp::UdpAssociation>> = None;
    let mut association_reader = None;
    let mut targets: HashMap<SocketAddr, (NatPacket, Instant)> = HashMap::new();
    let mut direct = HashMap::new();
    let (tx, mut rx) = mpsc::channel::<(SocketAddr, Bytes, GeoRoute)>(16);
    let generation = services.protector.network_generation();
    let mut last = Instant::now();
    let mut tick = tokio::time::interval(Duration::from_secs(1));
    loop {
        tokio::select! {
            _ = tick.tick() => {
                if last.elapsed() >= idle || generation != services.protector.network_generation() { break; }
                targets.retain(|_, (_, time)| time.elapsed() < idle);
                direct.retain(|a, _| targets.contains_key(a));
            }
            packet = packets.recv() => {
                let Some((meta, packet)) = packet else { break; };
                let remote = SocketAddr::new(meta.destination, meta.destination_port);
                if targets.len() >= 256 && !targets.contains_key(&remote) { continue; }
                let payload = &packet[meta.transport_offset + 8..];
                targets.insert(remote, (meta, Instant::now()));
                last = Instant::now();
                if services.geo_policy.route_ip(remote.ip()) == GeoRoute::Direct {
                    if let std::collections::hash_map::Entry::Vacant(entry) = direct.entry(remote)
                        && let Ok(socket) = bind_protected_udp(services.protector.as_ref(), remote.is_ipv6()) {
                        let socket = Arc::new(socket);
                        if let Ok(lease) = services.protector.protect_for_target(crate::socket::socket_handle(socket.as_ref()), remote, crate::socket::DirectProtocol::Udp).await {
                            let read = socket.clone(); let tx = tx.clone(); let cancel = cancel.clone();
                            let task = AbortOnDropHandle::new(tokio::spawn(async move {
                                let mut buffer = vec![0; mtu];
                                loop {
                                    let result = tokio::select! { _ = cancel.cancelled() => break, r = read.recv_from(&mut buffer) => r };
                                    let Ok((n, source)) = result else { break; };
                                    if source == remote {
                                        tokio::select! { _ = cancel.cancelled() => break, r = tx.send((source, Bytes::copy_from_slice(&buffer[..n]), GeoRoute::Direct)) => if r.is_err() { break; } }
                                    }
                                }
                            }));
                            entry.insert((socket, lease, task));
                        }
                    }
                    if let Some((socket, _, _)) = direct.get(&remote)
                        && socket.send_to(payload, remote).await.is_ok() {
                        services.counters.record_sent(payload.len()); continue;
                    }
                }
                if services.traffic_policy.blocks_udp(remote.port()) { continue; }
                if association.is_none() {
                    let Some(factory) = &services.udp else { continue; };
                    let Ok(opened) = factory.open(cancel, Instant::now() + Duration::from_secs(10)).await else { break; };
                    let read = opened.clone(); let tx = tx.clone(); let cancel = cancel.clone();
                    association_reader = Some(AbortOnDropHandle::new(tokio::spawn(async move {
                        let work = async { loop {
                            let Ok((source,payload)) = read.recv().await else { break; };
                            if let Some(source) = source.socket_address() && tx.send((source,payload,GeoRoute::Tunnel)).await.is_err() { break; }
                        }};
                        tokio::select! { _ = cancel.cancelled() => {}, _ = work => {} }
                        cancel.cancel();
                    })));
                    association = Some(opened);
                }
                if let Some(association) = &association {
                    match tokio::time::timeout(Duration::from_secs(10), association.send(&TcpTarget::address(remote), payload)).await {
                        Ok(Ok(())) => services.counters.record_sent(payload.len()),
                        _ => break,
                    }
                }
            }
            response = rx.recv() => {
                let Some((source,payload,route)) = response else { break; };
                let Some((meta,_)) = targets.get(&source) else { continue; };
                if route == GeoRoute::Tunnel && services.traffic_policy.blocks_udp(source.port()) { continue; }
                if payload.len() + meta.transport_offset + 8 > mtu { continue; }
                services.counters.record_received(payload.len());
                if replies.send(super::tun_wire::udp_response(meta, &payload)).await.is_err() { break; }
                last = Instant::now();
            }
        }
    }
    drop(association_reader);
}
