//! Bounded, flow-pinned CONNECT-IP connections. A failed member closes the
//! complete set; the existing supervisor owns recovery and physical generations.
use std::collections::HashMap;
use std::future::poll_fn;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::sync::Arc;
use std::task::Poll;
use std::time::{Duration, Instant};

use tokio::sync::{Notify, mpsc, oneshot, watch};
use tokio::task::{AbortHandle, JoinHandle, JoinSet};
use tokio_util::sync::CancellationToken;
use tokio_util::task::AbortOnDropHandle;
use usque_core::{AddressFamily, Transport};
use usque_protocol::{IpAddressRange, IpPrefix, PeerNetworkState};

use crate::endpoint_race::RaceConnection;
use crate::h2::TransportError;
use crate::netstack::{RuntimePath, apply_peer_network_state};
use crate::packet_batch::{PACKET_BATCH_CHANNEL_CAPACITY, PacketBatch, PacketBatchResult};
use crate::packet_mux::{FragmentKey, PoolPacketKey, pool_packet_key};
use crate::tcp_packet_budget::H3PacketBudget;
use crate::tunnel::{
    BatchSendFuture, MasqueDriver, MasqueReceiveHalf, MasqueSendHalf, MasqueTunnel,
};

const FRAGMENTS: usize = 1024;
const FRAGMENT_IDLE: Duration = Duration::from_secs(60);

struct FlowRouter {
    count: usize,
    fragments: HashMap<FragmentKey, (usize, Instant)>,
}

impl FlowRouter {
    fn new(count: usize) -> Self {
        Self {
            count,
            fragments: HashMap::new(),
        }
    }

    fn lane(&mut self, packet: &[u8], now: Instant) -> Option<usize> {
        match pool_packet_key(packet) {
            Some(PoolPacketKey::Flow { hash, fragment }) => {
                let lane = (hash % self.count as u64) as usize;
                if let Some(fragment) = fragment {
                    self.fragments.retain(|_, (_, seen)| {
                        now.saturating_duration_since(*seen) < FRAGMENT_IDLE
                    });
                    if self.fragments.len() >= FRAGMENTS && !self.fragments.contains_key(&fragment)
                    {
                        return None;
                    }
                    self.fragments.insert(fragment, (lane, now));
                }
                Some(lane)
            }
            Some(PoolPacketKey::Fragment(fragment)) => self
                .fragments
                .get(&fragment)
                .filter(|(_, seen)| now.saturating_duration_since(*seen) < FRAGMENT_IDLE)
                .map(|(lane, _)| *lane),
            // Non-flow control traffic stays on the primary connection.
            None => Some(0),
        }
    }
}

fn allowed_families(
    states: impl IntoIterator<Item = PeerNetworkState>,
    transport: Transport,
    family: AddressFamily,
    addresses: (Ipv4Addr, Ipv6Addr),
) -> RuntimePath {
    states.into_iter().fold(
        RuntimePath {
            transport,
            endpoint_family: family,
            ipv4_available: true,
            ipv6_available: true,
        },
        |path, state| apply_peer_network_state(path, &state, addresses.0, addresses.1),
    )
}

fn control_for(path: RuntimePath, addresses: (Ipv4Addr, Ipv6Addr)) -> PeerNetworkState {
    let mut state = PeerNetworkState {
        assignments_advertised: true,
        routes_advertised: true,
        ..Default::default()
    };
    if path.ipv4_available {
        state.assigned_addresses.push(IpPrefix {
            request_id: 0,
            address: IpAddr::V4(addresses.0),
            prefix_len: 32,
        });
        state.available_routes.push(IpAddressRange {
            start: IpAddr::V4(Ipv4Addr::UNSPECIFIED),
            end: IpAddr::V4(Ipv4Addr::BROADCAST),
            protocol: 0,
        });
    }
    if path.ipv6_available {
        state.assigned_addresses.push(IpPrefix {
            request_id: 0,
            address: IpAddr::V6(addresses.1),
            prefix_len: 128,
        });
        state.available_routes.push(IpAddressRange {
            start: IpAddr::V6(Ipv6Addr::UNSPECIFIED),
            end: IpAddr::V6(Ipv6Addr::from(u128::MAX)),
            protocol: 0,
        });
    }
    state
}

fn minimum_budget(budgets: &[watch::Receiver<Option<H3PacketBudget>>]) -> Option<H3PacketBudget> {
    if budgets.is_empty() {
        return None;
    }
    Some(budgets.iter().fold(
        H3PacketBudget {
            limit: usize::MAX,
            confirmed: true,
        },
        |mut result, rx| {
            let value = rx.borrow().unwrap_or(H3PacketBudget {
                limit: 128,
                confirmed: false,
            });
            result.limit = result.limit.min(value.limit);
            result.confirmed &= value.confirmed;
            result
        },
    ))
}

pub(crate) struct ParallelTunnel {
    members: Vec<MasqueTunnel>,
    family: AddressFamily,
    addresses: (Ipv4Addr, Ipv6Addr),
    control: watch::Sender<PeerNetworkState>,
    budget: watch::Sender<Option<H3PacketBudget>>,
    budgets: Vec<watch::Receiver<Option<H3PacketBudget>>>,
}

impl ParallelTunnel {
    pub(crate) fn new(
        members: Vec<MasqueTunnel>,
        family: AddressFamily,
        addresses: (Ipv4Addr, Ipv6Addr),
    ) -> Self {
        assert!((2..=usize::from(usque_core::MAX_MASQUE_CONNECTIONS)).contains(&members.len()));
        let path = allowed_families(
            members.iter().map(MasqueTunnel::control_state),
            members[0].transport(),
            family,
            addresses,
        );
        let budgets: Vec<_> = members
            .iter()
            .filter_map(MasqueTunnel::packet_budget)
            .collect();
        Self {
            control: watch::channel(control_for(path, addresses)).0,
            budget: watch::channel(minimum_budget(&budgets)).0,
            members,
            family,
            addresses,
            budgets,
        }
    }

    pub(crate) fn transport(&self) -> Transport {
        self.members[0].transport()
    }
    pub(crate) fn control_state(&self) -> PeerNetworkState {
        self.control.borrow().clone()
    }
    pub(crate) fn packet_budget(&self) -> Option<watch::Receiver<Option<H3PacketBudget>>> {
        (!self.budgets.is_empty()).then(|| self.budget.subscribe())
    }
    pub(crate) fn activate_network_quality(&self) {
        self.members[0].activate_network_quality();
    }
    pub(crate) fn is_alive(&self) -> bool {
        self.members.iter().all(RaceConnection::is_alive)
    }
    pub(crate) async fn shutdown(self) {
        for member in self.members {
            member.shutdown().await;
        }
    }

    pub(crate) fn into_parts(
        self,
    ) -> (
        MasqueSendHalf,
        MasqueReceiveHalf,
        MasqueDriver,
        Option<watch::Receiver<PeerNetworkState>>,
    ) {
        let transport = self.transport();
        let cancel = CancellationToken::new();
        let (outgoing, commands) = mpsc::channel(PACKET_BATCH_CHANNEL_CAPACITY);
        let (incoming, receive) = mpsc::channel(PACKET_BATCH_CHANNEL_CAPACITY);
        let control_updates = self.control.subscribe();
        let mut children = JoinSet::new();
        let mut sends = Vec::new();
        let mut controls = Vec::new();
        let mut aborts = Vec::new();
        let changed = Arc::new(Notify::new());
        for member in self.members {
            let (send, mut read, driver, control) = member.into_parts();
            sends.push(send);
            let control = control.expect("CONNECT-IP member has control state");
            controls.push(control.clone());
            aborts.push(driver.abort_handle());
            children.spawn(driver.wait());
            let child_cancel = cancel.clone();
            let incoming = incoming.clone();
            children.spawn(async move {
                loop {
                    let batch = tokio::select! {
                        biased;
                        _ = child_cancel.cancelled() => return Ok(()),
                        batch = read.receive_batch() => batch?,
                    };
                    tokio::select! {
                        biased;
                        _ = child_cancel.cancelled() => return Ok(()),
                        result = incoming.send(batch) => result.map_err(|_| TransportError::TunnelClosed)?,
                    }
                }
            });
            let changed = changed.clone();
            let child_cancel = cancel.clone();
            children.spawn(async move {
                let mut control = Some(control);
                loop {
                    tokio::select! {
                        biased;
                        _ = child_cancel.cancelled() => return Ok(()),
                        result = watch_change(&mut control) => {
                            if result.is_err() { control = None; }
                            changed.notify_one();
                        }
                    }
                }
            });
        }
        for budget in &self.budgets {
            let mut source = Some(budget.clone());
            let changed = changed.clone();
            let child_cancel = cancel.clone();
            children.spawn(async move {
                loop {
                    tokio::select! {
                        biased;
                        _ = child_cancel.cancelled() => return Ok(()),
                        result = watch_change(&mut source) => {
                            if result.is_err() { source = None; }
                            changed.notify_one();
                        }
                    }
                }
            });
        }
        drop(incoming);
        let manager_cancel = cancel.clone();
        let task = tokio::spawn(async move {
            let mut router = FlowRouter::new(sends.len());
            let mut commands = commands;
            let result = loop {
                let path = allowed_families(
                    controls.iter().map(|rx| rx.borrow().clone()),
                    transport,
                    self.family,
                    self.addresses,
                );
                self.control.send_if_modified(|state| {
                    let next = control_for(path, self.addresses);
                    if *state == next {
                        false
                    } else {
                        *state = next;
                        true
                    }
                });
                self.budget.send_if_modified(|state| {
                    let next = minimum_budget(&self.budgets);
                    if *state == next {
                        false
                    } else {
                        *state = next;
                        true
                    }
                });
                tokio::select! {
                    biased;
                    _ = manager_cancel.cancelled() => break Ok(()),
                    child = children.join_next() => break child_failure(child),
                    _ = changed.notified() => {},
                    request = commands.recv() => {
                        let Some(SendRequest { batch, completion }) = request else { break Ok(()); };
                        let path = allowed_families(
                            controls.iter().map(|rx| rx.borrow().clone()),
                            transport,
                            self.family,
                            self.addresses,
                        );
                        let send = send_batch(&sends, &mut router, batch, path);
                        tokio::pin!(send);
                        let result = tokio::select! {
                            biased;
                            _ = manager_cancel.cancelled() => Err(TransportError::TunnelClosed),
                            child = children.join_next() => child_failure(child).map(|()| PacketBatchResult::default()),
                            result = &mut send => result,
                        };
                        let failed = result.is_err();
                        let _ = completion.send(result);
                        if failed { break Err(TransportError::TunnelClosed); }
                    }
                }
            };
            manager_cancel.cancel();
            for send in &mut sends {
                send.close();
            }
            for handle in aborts {
                handle.abort();
            }
            // Joining the driver waits is the socket/lease cleanup barrier.
            while children.join_next().await.is_some() {}
            result
        });
        (
            MasqueSendHalf::Parallel(ParallelSendHalf {
                sender: Some(outgoing),
                cancel: cancel.clone(),
            }),
            MasqueReceiveHalf::Parallel(receive),
            MasqueDriver::Parallel(ParallelDriver {
                task: Some(task),
                cancel,
            }),
            Some(control_updates),
        )
    }
}

async fn watch_change<T: Clone>(source: &mut Option<watch::Receiver<T>>) -> Result<(), ()> {
    match source {
        Some(source) => source.changed().await.map_err(|_| ()),
        None => std::future::pending().await,
    }
}

fn child_failure(
    child: Option<Result<Result<(), TransportError>, tokio::task::JoinError>>,
) -> Result<(), TransportError> {
    match child {
        Some(Ok(Err(error))) => Err(error),
        Some(Err(error)) => Err(TransportError::Driver(error.to_string())),
        _ => Err(TransportError::TunnelClosed),
    }
}

async fn send_batch(
    sends: &[MasqueSendHalf],
    router: &mut FlowRouter,
    mut batch: PacketBatch,
    path: RuntimePath,
) -> Result<PacketBatchResult, TransportError> {
    let mut lanes: Vec<Option<PacketBatch>> = (0..sends.len()).map(|_| None).collect();
    let now = Instant::now();
    while let Some(packet) = batch.pop_front() {
        if !match packet.first().map(|b| b >> 4) {
            Some(4) => path.ipv4_available,
            Some(6) => path.ipv6_available,
            _ => false,
        } {
            continue;
        }
        if let Some(lane) = router.lane(&packet, now)
            && lanes[lane]
                .get_or_insert_with(PacketBatch::new)
                .push_back(packet)
                .is_err()
        {
            return Err(TransportError::TunnelClosed);
        }
    }
    let mut pending: Vec<_> = sends
        .iter()
        .zip(lanes)
        .map(|(send, batch)| batch.map(|batch| send.start_owned_batch(batch)))
        .collect();
    let mut result = PacketBatchResult::default();
    poll_fn(|cx| {
        for future in &mut pending {
            if let Some(send) = future
                && let Poll::Ready(value) = send.as_mut().poll(cx)
            {
                match value {
                    Ok(mut value) => {
                        result.accepted_bytes += value.accepted_bytes;
                        result.oversized.append(&mut value.oversized);
                    }
                    Err(error) => return Poll::Ready(Err(error)),
                }
                *future = None;
            }
        }
        if pending.iter().all(Option::is_none) {
            Poll::Ready(Ok(std::mem::take(&mut result)))
        } else {
            Poll::Pending
        }
    })
    .await
}

struct SendRequest {
    batch: PacketBatch,
    completion: oneshot::Sender<Result<PacketBatchResult, TransportError>>,
}

pub(crate) struct ParallelSendHalf {
    sender: Option<mpsc::Sender<SendRequest>>,
    cancel: CancellationToken,
}
impl ParallelSendHalf {
    pub(crate) fn start_owned_batch(&self, batch: PacketBatch) -> BatchSendFuture {
        let sender = self.sender.clone();
        let cancel = self.cancel.clone();
        Box::pin(async move {
            let sender = sender.ok_or(TransportError::TunnelClosed)?;
            let (completion, response) = oneshot::channel();
            tokio::select! {
                biased;
                _ = cancel.cancelled() => Err(TransportError::TunnelClosed),
                result = async { sender.send(SendRequest { batch, completion }).await.map_err(|_| TransportError::TunnelClosed)?; response.await.map_err(|_| TransportError::TunnelClosed)? } => result,
            }
        })
    }
    pub(crate) fn close(&mut self) {
        self.sender.take();
        self.cancel.cancel();
    }
}

pub(crate) struct ParallelDriver {
    task: Option<JoinHandle<Result<(), TransportError>>>,
    cancel: CancellationToken,
}
impl ParallelDriver {
    pub(crate) fn abort_handle(&self) -> AbortHandle {
        self.task
            .as_ref()
            .expect("pool driver is owned")
            .abort_handle()
    }
    pub(crate) async fn wait(mut self) -> Result<(), TransportError> {
        AbortOnDropHandle::new(self.task.take().expect("pool driver is owned"))
            .await
            .map_err(|e| TransportError::Driver(e.to_string()))?
    }
}
impl Drop for ParallelDriver {
    fn drop(&mut self) {
        self.cancel.cancel();
        if let Some(task) = &self.task {
            task.abort();
        }
    }
}

#[cfg(test)]
mod tests;
