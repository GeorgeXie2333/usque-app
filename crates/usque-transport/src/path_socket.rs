use std::future::pending;
use std::io;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use thiserror::Error;
use tokio::sync::{Mutex, mpsc};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::network_quality::NetworkQualityTelemetry;
use crate::socket::DirectEgressLease;
use crate::udp_io::{RecvBatch, UdpBatchIo, UdpReceivePool};

const PATH_RECEIVE_CHANNEL_CAPACITY: usize = 4;
pub(crate) const MAX_PATH_SOCKETS: usize = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct PathId(u64);

impl PathId {
    pub(crate) const fn new(value: u64) -> Self {
        Self(value)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PathSocketRole {
    Active,
    Candidate,
    Retiring,
}

#[derive(Clone, Copy)]
pub(crate) struct PathBinding {
    pub(crate) path_id: PathId,
    pub(crate) local_addr: SocketAddr,
    pub(crate) peer_addr: SocketAddr,
    pub(crate) network_generation: u64,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub(crate) enum PathSocketSetError {
    #[error("the path socket role is already occupied")]
    RoleOccupied,
    #[error("the path socket set is full")]
    Capacity,
    #[error("the path socket identifier is already present")]
    DuplicatePathId,
    #[error("no socket exactly matches the QUIC send source")]
    SendSourceNotFound,
    #[error("the QUIC send peer does not match its path socket")]
    SendPeerMismatch,
    #[error("path sockets must share one receive buffer budget")]
    ReceivePoolMismatch,
    #[error("a retiring path cannot send packets")]
    RetiringSendForbidden,
    #[error("the path socket set is not ready for atomic promotion")]
    PromotionUnavailable,
}

struct PathIoLease {
    // Field order is intentional: the socket closes before its authorization
    // lease is released, including when an aborted receiver owns the last Arc.
    io: UdpBatchIo,
    _egress_lease: DirectEgressLease,
}

struct ReceiverRunningGuard(Arc<AtomicBool>);

impl Drop for ReceiverRunningGuard {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

pub(crate) enum PathReceiveEvent {
    Batch { path_id: PathId, batch: RecvBatch },
    Failed { path_id: PathId, error: io::Error },
}

pub(crate) struct PathSocket {
    pub(crate) path_id: PathId,
    pub(crate) local_addr: SocketAddr,
    pub(crate) peer_addr: SocketAddr,
    pub(crate) network_generation: u64,
    pub(crate) role: PathSocketRole,
    io_lease: Option<Arc<PathIoLease>>,
    receiver: Mutex<mpsc::Receiver<io::Result<RecvBatch>>>,
    receiver_cancel: CancellationToken,
    receiver_task: Option<JoinHandle<()>>,
    receiver_running: Arc<AtomicBool>,
}

impl PathSocket {
    #[expect(
        clippy::too_many_arguments,
        reason = "a path socket atomically binds identity, generation, I/O, lease, and role"
    )]
    pub(crate) fn spawn(
        path_id: PathId,
        local_addr: SocketAddr,
        peer_addr: SocketAddr,
        network_generation: u64,
        role: PathSocketRole,
        socket: tokio::net::UdpSocket,
        egress_lease: DirectEgressLease,
        quality: NetworkQualityTelemetry,
        receive_pool: UdpReceivePool,
    ) -> io::Result<Self> {
        if egress_lease.generation() != Some(network_generation)
            || socket.local_addr()? != local_addr
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "path socket identity does not match its prepared lease",
            ));
        }
        let io_lease = Arc::new(PathIoLease {
            io: UdpBatchIo::with_receive_pool(socket, quality, receive_pool)?,
            _egress_lease: egress_lease,
        });
        let (sender, receiver) = mpsc::channel(PATH_RECEIVE_CHANNEL_CAPACITY);
        let receiver_cancel = CancellationToken::new();
        let task_cancel = receiver_cancel.clone();
        let task_io = Arc::clone(&io_lease);
        let receiver_running = Arc::new(AtomicBool::new(true));
        let task_running = ReceiverRunningGuard(Arc::clone(&receiver_running));
        let receiver_task = tokio::spawn(async move {
            let _running = task_running;
            loop {
                let mut batch = task_io.io.new_recv_batch();
                let received = task_io.io.recv_batch(&mut batch, &task_cancel).await;
                let message = match received {
                    Ok(_) => Ok(batch),
                    Err(_error) if task_cancel.is_cancelled() => break,
                    Err(error) => Err(error),
                };
                let terminal = message.is_err();
                tokio::select! {
                    biased;
                    _ = task_cancel.cancelled() => break,
                    result = sender.send(message) => {
                        if result.is_err() || terminal {
                            break;
                        }
                    }
                }
            }
        });
        Ok(Self {
            path_id,
            local_addr,
            peer_addr,
            network_generation,
            role,
            io_lease: Some(io_lease),
            receiver: Mutex::new(receiver),
            receiver_cancel,
            receiver_task: Some(receiver_task),
            receiver_running,
        })
    }

    fn io(&self) -> &UdpBatchIo {
        &self
            .io_lease
            .as_ref()
            .expect("live path socket retains its I/O lease")
            .io
    }

    pub(crate) fn binding(&self) -> PathBinding {
        PathBinding {
            path_id: self.path_id,
            local_addr: self.local_addr,
            peer_addr: self.peer_addr,
            network_generation: self.network_generation,
        }
    }

    async fn receive(&self) -> PathReceiveEvent {
        match self.receiver.lock().await.recv().await {
            Some(Ok(batch)) => PathReceiveEvent::Batch {
                path_id: self.path_id,
                batch,
            },
            Some(Err(error)) => PathReceiveEvent::Failed {
                path_id: self.path_id,
                error,
            },
            None => PathReceiveEvent::Failed {
                path_id: self.path_id,
                error: io::Error::new(io::ErrorKind::BrokenPipe, "path receive task stopped"),
            },
        }
    }

    pub(crate) async fn shutdown(mut self) {
        self.receiver_cancel.cancel();
        if let Some(task) = self.receiver_task.take() {
            task.abort();
            let _ = task.await;
        }
        debug_assert!(!self.receiver_running.load(Ordering::Acquire));
        let receiver = self.receiver.get_mut();
        receiver.close();
        while receiver.try_recv().is_ok() {}
        self.io_lease.take();
    }

    #[cfg(test)]
    fn receiver_is_running(&self) -> bool {
        self.receiver_running.load(Ordering::Acquire)
    }
}

impl Drop for PathSocket {
    fn drop(&mut self) {
        self.receiver_cancel.cancel();
        if let Some(task) = self.receiver_task.take() {
            task.abort();
        }
        let receiver = self.receiver.get_mut();
        receiver.close();
        while receiver.try_recv().is_ok() {}
        self.io_lease.take();
    }
}

pub(crate) struct PathSocketSet {
    active: Option<PathSocket>,
    candidate: Option<PathSocket>,
    retiring: Option<PathSocket>,
    receive_pool: UdpReceivePool,
}

impl PathSocketSet {
    pub(crate) fn with_active(active: PathSocket) -> Result<Self, PathSocketSetError> {
        if active.role != PathSocketRole::Active {
            return Err(PathSocketSetError::RoleOccupied);
        }
        let receive_pool = active.io().receive_pool();
        Ok(Self {
            active: Some(active),
            candidate: None,
            retiring: None,
            receive_pool,
        })
    }

    pub(crate) fn receive_pool(&self) -> UdpReceivePool {
        self.receive_pool.clone()
    }

    pub(crate) fn len(&self) -> usize {
        usize::from(self.active.is_some())
            + usize::from(self.candidate.is_some())
            + usize::from(self.retiring.is_some())
    }

    pub(crate) fn active(&self) -> Option<&PathSocket> {
        self.active.as_ref()
    }

    pub(crate) fn contains(&self, path_id: PathId) -> bool {
        self.iter().any(|path| path.path_id == path_id)
    }

    pub(crate) fn insert(&mut self, socket: PathSocket) -> Result<(), PathSocketSetError> {
        if self.len() >= MAX_PATH_SOCKETS {
            return Err(PathSocketSetError::Capacity);
        }
        if self.contains(socket.path_id) {
            return Err(PathSocketSetError::DuplicatePathId);
        }
        if !self
            .receive_pool
            .shares_budget_with(&socket.io().receive_pool())
        {
            return Err(PathSocketSetError::ReceivePoolMismatch);
        }
        let slot = match socket.role {
            PathSocketRole::Active => &mut self.active,
            PathSocketRole::Candidate => &mut self.candidate,
            PathSocketRole::Retiring => &mut self.retiring,
        };
        if slot.is_some() {
            return Err(PathSocketSetError::RoleOccupied);
        }
        *slot = Some(socket);
        Ok(())
    }

    pub(crate) fn io_for_send(
        &self,
        from: SocketAddr,
        to: SocketAddr,
    ) -> Result<&UdpBatchIo, PathSocketSetError> {
        let Some(path) = self.iter().find(|path| path.local_addr == from) else {
            return Err(PathSocketSetError::SendSourceNotFound);
        };
        if path.peer_addr != to {
            return Err(PathSocketSetError::SendPeerMismatch);
        }
        if path.role == PathSocketRole::Retiring {
            return Err(PathSocketSetError::RetiringSendForbidden);
        }
        Ok(path.io())
    }

    pub(crate) async fn recv_any(&self) -> PathReceiveEvent {
        tokio::select! {
            event = receive_role(&self.active) => event,
            event = receive_role(&self.candidate) => event,
            event = receive_role(&self.retiring) => event,
        }
    }

    pub(crate) async fn clear_candidate(&mut self) {
        if let Some(candidate) = self.candidate.take() {
            candidate.shutdown().await;
        }
    }

    pub(crate) async fn clear_retiring(&mut self) {
        if let Some(retiring) = self.retiring.take() {
            retiring.shutdown().await;
        }
    }

    pub(crate) fn promotion_ready(&self) -> bool {
        self.active.is_some()
            && self.retiring.is_none()
            && self.candidate.as_ref().is_some_and(|path| {
                !path.receiver_cancel.is_cancelled()
                    && path.receiver_running.load(Ordering::Acquire)
                    && path.io_lease.as_ref().is_some_and(|lease| {
                        lease._egress_lease.generation() == Some(path.network_generation)
                    })
            })
    }

    pub(crate) fn promote_candidate(&mut self) -> Result<PathBinding, PathSocketSetError> {
        if !self.promotion_ready() {
            return Err(PathSocketSetError::PromotionUnavailable);
        }
        let mut next = self.candidate.take().expect("candidate checked above");
        let mut previous = self.active.take().expect("active checked above");
        next.role = PathSocketRole::Active;
        previous.role = PathSocketRole::Retiring;
        let binding = next.binding();
        self.active = Some(next);
        self.retiring = Some(previous);
        Ok(binding)
    }

    pub(crate) async fn shutdown_all(&mut self) {
        if let Some(active) = self.active.take() {
            active.shutdown().await;
        }
        if let Some(candidate) = self.candidate.take() {
            candidate.shutdown().await;
        }
        if let Some(retiring) = self.retiring.take() {
            retiring.shutdown().await;
        }
    }

    fn iter(&self) -> impl Iterator<Item = &PathSocket> {
        self.active
            .iter()
            .chain(self.candidate.iter())
            .chain(self.retiring.iter())
    }

    pub(crate) fn candidate(&self) -> Option<&PathSocket> {
        self.candidate.as_ref()
    }

    pub(crate) fn retiring(&self) -> Option<&PathSocket> {
        self.retiring.as_ref()
    }
}

async fn receive_role(slot: &Option<PathSocket>) -> PathReceiveEvent {
    match slot {
        Some(path) => path.receive().await,
        None => pending().await,
    }
}

#[cfg(test)]
mod tests {
    use std::net::UdpSocket as StdUdpSocket;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    struct LeaseDropCounter(Arc<AtomicUsize>);

    impl Drop for LeaseDropCounter {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::AcqRel);
        }
    }

    struct LeaseSocketClosedProbe {
        local_addr: SocketAddr,
        socket_was_closed: Arc<AtomicBool>,
    }

    impl Drop for LeaseSocketClosedProbe {
        fn drop(&mut self) {
            self.socket_was_closed.store(
                StdUdpSocket::bind(self.local_addr).is_ok(),
                Ordering::Release,
            );
        }
    }

    fn test_path(
        path_id: u64,
        role: PathSocketRole,
        peer: SocketAddr,
        lease_drops: Arc<AtomicUsize>,
    ) -> PathSocket {
        test_path_in_pool(path_id, role, peer, lease_drops, UdpReceivePool::default())
    }

    fn test_path_in_pool(
        path_id: u64,
        role: PathSocketRole,
        peer: SocketAddr,
        lease_drops: Arc<AtomicUsize>,
        receive_pool: UdpReceivePool,
    ) -> PathSocket {
        let socket = StdUdpSocket::bind("127.0.0.1:0").unwrap();
        socket.set_nonblocking(true).unwrap();
        let local = socket.local_addr().unwrap();
        PathSocket::spawn(
            PathId::new(path_id),
            local,
            peer,
            path_id,
            role,
            tokio::net::UdpSocket::from_std(socket).unwrap(),
            DirectEgressLease::hold_for_generation(LeaseDropCounter(lease_drops), path_id),
            NetworkQualityTelemetry::default(),
            receive_pool,
        )
        .unwrap()
    }

    #[tokio::test]
    async fn roles_are_unique_and_the_total_is_bounded_to_three() {
        let peer: SocketAddr = "127.0.0.1:443".parse().unwrap();
        let drops = Arc::new(AtomicUsize::new(0));
        let active = test_path(1, PathSocketRole::Active, peer, Arc::clone(&drops));
        let mut set = PathSocketSet::with_active(active).unwrap();
        assert_eq!(set.active().unwrap().network_generation, 1);
        let pool = set.receive_pool();
        set.insert(test_path_in_pool(
            2,
            PathSocketRole::Candidate,
            peer,
            Arc::clone(&drops),
            pool.clone(),
        ))
        .unwrap();
        set.insert(test_path_in_pool(
            3,
            PathSocketRole::Retiring,
            peer,
            Arc::clone(&drops),
            pool.clone(),
        ))
        .unwrap();
        assert_eq!(set.len(), MAX_PATH_SOCKETS);
        assert_eq!(
            set.insert(test_path_in_pool(
                4,
                PathSocketRole::Candidate,
                peer,
                Arc::clone(&drops),
                pool,
            )),
            Err(PathSocketSetError::Capacity)
        );
        set.shutdown_all().await;
        assert_eq!(drops.load(Ordering::Acquire), 4);
    }

    #[tokio::test]
    async fn a_second_active_is_rejected_even_below_total_capacity() {
        let peer: SocketAddr = "127.0.0.1:443".parse().unwrap();
        let drops = Arc::new(AtomicUsize::new(0));
        let active = test_path(1, PathSocketRole::Active, peer, Arc::clone(&drops));
        let mut set = PathSocketSet::with_active(active).unwrap();
        assert_eq!(
            set.insert(test_path_in_pool(
                2,
                PathSocketRole::Active,
                peer,
                Arc::clone(&drops),
                set.receive_pool(),
            )),
            Err(PathSocketSetError::RoleOccupied)
        );
        assert_eq!(set.len(), 1);
        set.shutdown_all().await;
        assert_eq!(drops.load(Ordering::Acquire), 2);
    }

    #[tokio::test]
    async fn candidate_cannot_expand_the_set_with_an_independent_buffer_budget() {
        let peer: SocketAddr = "127.0.0.1:443".parse().unwrap();
        let drops = Arc::new(AtomicUsize::new(0));
        let active = test_path(1, PathSocketRole::Active, peer, Arc::clone(&drops));
        let mut set = PathSocketSet::with_active(active).unwrap();
        assert_eq!(
            set.insert(test_path(
                2,
                PathSocketRole::Candidate,
                peer,
                Arc::clone(&drops)
            )),
            Err(PathSocketSetError::ReceivePoolMismatch)
        );
        assert_eq!(set.len(), 1);
        set.shutdown_all().await;
        assert_eq!(drops.load(Ordering::Acquire), 2);
    }

    #[tokio::test]
    async fn socket_closes_before_its_egress_lease_is_released() {
        let socket = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let local_addr = socket.local_addr().unwrap();
        assert!(StdUdpSocket::bind(local_addr).is_err());
        let socket_was_closed = Arc::new(AtomicBool::new(false));
        let path = PathSocket::spawn(
            PathId::new(0),
            local_addr,
            "127.0.0.1:443".parse().unwrap(),
            0,
            PathSocketRole::Active,
            socket,
            DirectEgressLease::hold_for_generation(
                LeaseSocketClosedProbe {
                    local_addr,
                    socket_was_closed: Arc::clone(&socket_was_closed),
                },
                0,
            ),
            NetworkQualityTelemetry::default(),
            UdpReceivePool::default(),
        )
        .unwrap();
        path.shutdown().await;
        assert!(socket_was_closed.load(Ordering::Acquire));
    }

    #[tokio::test]
    async fn send_routing_requires_exact_local_and_peer_addresses() {
        let peer: SocketAddr = "127.0.0.1:443".parse().unwrap();
        let drops = Arc::new(AtomicUsize::new(0));
        let active = test_path(1, PathSocketRole::Active, peer, Arc::clone(&drops));
        let local = active.local_addr;
        let mut set = PathSocketSet::with_active(active).unwrap();

        assert_eq!(set.io_for_send(local, peer).unwrap().local_addr(), local);
        assert_eq!(
            set.io_for_send(local, "127.0.0.1:444".parse().unwrap())
                .unwrap_err(),
            PathSocketSetError::SendPeerMismatch
        );
        assert_eq!(
            set.io_for_send("127.0.0.1:9".parse().unwrap(), peer)
                .unwrap_err(),
            PathSocketSetError::SendSourceNotFound
        );
        set.shutdown_all().await;
    }

    #[tokio::test]
    async fn candidate_supersede_releases_old_task_and_lease_before_prepare() {
        let peer: SocketAddr = "127.0.0.1:443".parse().unwrap();
        let drops = Arc::new(AtomicUsize::new(0));
        let active = test_path(1, PathSocketRole::Active, peer, Arc::clone(&drops));
        let mut set = PathSocketSet::with_active(active).unwrap();
        set.insert(test_path_in_pool(
            2,
            PathSocketRole::Candidate,
            peer,
            Arc::clone(&drops),
            set.receive_pool(),
        ))
        .unwrap();
        let observed = Arc::new(AtomicUsize::new(0));
        let observed_in_prepare = Arc::clone(&observed);
        let drop_count = Arc::clone(&drops);
        let candidate_pool = set.receive_pool();
        set.clear_candidate().await;
        observed_in_prepare.store(drop_count.load(Ordering::Acquire), Ordering::Release);
        set.insert(test_path_in_pool(
            3,
            PathSocketRole::Candidate,
            peer,
            Arc::clone(&drop_count),
            candidate_pool,
        ))
        .unwrap();

        assert_eq!(observed.load(Ordering::Acquire), 1);
        assert_eq!(set.candidate().unwrap().path_id, PathId::new(3));
        assert!(set.candidate().unwrap().receiver_is_running());
        set.shutdown_all().await;
        assert_eq!(drops.load(Ordering::Acquire), 3);
    }

    #[tokio::test]
    async fn promotion_swaps_the_complete_binding_and_retiring_never_sends() {
        let peer: SocketAddr = "127.0.0.1:443".parse().unwrap();
        let drops = Arc::new(AtomicUsize::new(0));
        let active = test_path(1, PathSocketRole::Active, peer, Arc::clone(&drops));
        let old_local = active.local_addr;
        let mut set = PathSocketSet::with_active(active).unwrap();
        let candidate = test_path_in_pool(
            2,
            PathSocketRole::Candidate,
            peer,
            Arc::clone(&drops),
            set.receive_pool(),
        );
        let new_local = candidate.local_addr;
        set.insert(candidate).unwrap();
        let promoted = set.promote_candidate().unwrap();
        assert_eq!(promoted.path_id, PathId::new(2));
        assert_eq!(promoted.local_addr, new_local);
        assert_eq!(promoted.network_generation, 2);
        assert_eq!(set.active().unwrap().role, PathSocketRole::Active);
        assert_eq!(set.retiring().unwrap().role, PathSocketRole::Retiring);
        assert!(set.candidate().is_none());
        assert_eq!(
            set.io_for_send(old_local, peer).unwrap_err(),
            PathSocketSetError::RetiringSendForbidden,
        );
        set.shutdown_all().await;
        assert_eq!(drops.load(Ordering::Acquire), 2);
    }
}
