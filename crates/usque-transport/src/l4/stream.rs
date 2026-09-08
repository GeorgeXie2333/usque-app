use std::collections::VecDeque;
use std::io;
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::task::{Context, Poll, Waker};

use bytes::{Buf, Bytes};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::sync::{Notify, OwnedSemaphorePermit, Semaphore, oneshot};
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

use crate::tcp::{DialError, FlowClass, TcpIo, TcpStream, TcpTarget};

#[derive(Default)]
pub(crate) struct L4Metrics {
    pub(crate) value: Mutex<usque_core::L4Snapshot>,
}

impl L4Metrics {
    pub(crate) fn update(&self, f: impl FnOnce(&mut usque_core::L4Snapshot)) {
        f(&mut self.value.lock().unwrap_or_else(|e| e.into_inner()));
    }
    pub(crate) fn snapshot(&self) -> usque_core::L4Snapshot {
        self.value.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
}

pub(crate) struct BufferBudget {
    permits: Arc<Semaphore>,
    progress_headroom: usize,
    actors: Mutex<Vec<Weak<Notify>>>,
    pub(crate) wake: Arc<Notify>,
    pub(crate) metrics: Arc<L4Metrics>,
}

impl BufferBudget {
    pub(crate) fn new(bytes: usize, metrics: Arc<L4Metrics>, wake: Arc<Notify>) -> Self {
        Self {
            permits: Arc::new(Semaphore::new(bytes)),
            progress_headroom: (bytes / 8).min(16 << 20),
            actors: Mutex::new(Vec::new()),
            metrics,
            wake,
        }
    }

    pub(crate) fn reserve(self: &Arc<Self>, size: usize) -> Option<BufferLease> {
        self.reserve_with_headroom(size, 0)
    }

    pub(crate) fn reserve_admission(self: &Arc<Self>, size: usize) -> Option<BufferLease> {
        self.reserve_with_headroom(size, self.progress_headroom)
    }

    fn reserve_with_headroom(
        self: &Arc<Self>,
        size: usize,
        headroom: usize,
    ) -> Option<BufferLease> {
        let count = u32::try_from(size.checked_add(headroom)?).ok()?;
        let mut permit = self.permits.clone().try_acquire_many_owned(count).ok()?;
        // Atomically test the margin, then return it for actual packet/stream
        // progress. Fixed relay reservations must not strand every accepted
        // connection with zero budget for the next DATA read or write.
        if headroom != 0 {
            drop(permit.split(headroom).expect("acquired headroom"));
        }
        self.metrics.update(|m| m.buffer_bytes += size as u64);
        Some(BufferLease {
            permit: Some(permit),
            budget: self.clone(),
            size,
        })
    }

    pub(crate) fn copy(self: &Arc<Self>, bytes: &[u8]) -> Option<Bytes> {
        let lease = self.reserve(bytes.len())?;
        Some(Bytes::from_owner(OwnedBuffer {
            bytes: bytes.to_vec(),
            _lease: lease,
        }))
    }

    pub(crate) fn available(&self) -> usize {
        self.permits.available_permits()
    }

    pub(crate) fn register_actor(&self, wake: &Arc<Notify>) {
        let mut actors = self.actors.lock().unwrap_or_else(|e| e.into_inner());
        actors.retain(|actor| actor.strong_count() != 0);
        actors.push(Arc::downgrade(wake));
    }
}

pub(crate) struct BufferLease {
    permit: Option<OwnedSemaphorePermit>,
    budget: Arc<BufferBudget>,
    size: usize,
}

impl Drop for BufferLease {
    fn drop(&mut self) {
        drop(self.permit.take());
        self.budget
            .metrics
            .update(|m| m.buffer_bytes = m.buffer_bytes.saturating_sub(self.size as u64));
        // Both the main and draining actor can be waiting for the shared budget.
        self.budget.wake.notify_waiters();
        self.budget.wake.notify_one();
        // Each actor has its own stored notification. A shared notify_one can
        // be consumed by the other session between the budget check and wait.
        for actor in self
            .budget
            .actors
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
        {
            if let Some(wake) = actor.upgrade() {
                wake.notify_one();
            }
        }
    }
}

pub(crate) struct OwnedBuffer {
    pub(crate) bytes: Vec<u8>,
    pub(crate) _lease: BufferLease,
}
impl AsRef<[u8]> for OwnedBuffer {
    fn as_ref(&self) -> &[u8] {
        &self.bytes
    }
}

pub(crate) struct OpenRequest {
    pub(crate) target: TcpTarget,
    pub(crate) deadline: Instant,
    pub(crate) cancellation: CancellationToken,
    pub(crate) reply: oneshot::Sender<Result<TcpStream, DialError>>,
    pub(crate) class: FlowClass,
    pub(crate) _pending: OwnedSemaphorePermit,
}

pub(crate) struct Flow {
    pub(crate) session_generation: u64,
    pub(crate) state: Mutex<FlowState>,
    pub(crate) target: TcpTarget,
    pub(crate) deadline: Instant,
    pub(crate) cancellation: CancellationToken,
    pub(crate) budget: Arc<BufferBudget>,
    pub(crate) wake: Arc<Notify>,
    pub(crate) metrics: Arc<L4Metrics>,
    pub(crate) counters: Arc<crate::netstack::TrafficCounters>,
    pub(crate) reply: Mutex<Option<oneshot::Sender<Result<TcpStream, DialError>>>>,
    pub(crate) started: Instant,
    pub(crate) _active: OwnedSemaphorePermit,
    pub(crate) _relay: BufferLease,
    pub(crate) _pending: Mutex<Option<OwnedSemaphorePermit>>,
    pub(crate) delivered: AtomicBool,
    pub(crate) stream_id: AtomicU64,
}

#[derive(Default)]
pub(crate) struct FlowState {
    pub(crate) incoming: VecDeque<Bytes>,
    pub(crate) outgoing: VecDeque<Bytes>,
    pub(crate) incoming_bytes: usize,
    pub(crate) outgoing_bytes: usize,
    pub(crate) accepted: bool,
    pub(crate) read_finished: bool,
    pub(crate) read_pending: bool,
    pub(crate) fin_requested: bool,
    pub(crate) fin_sent: bool,
    pub(crate) dropped: bool,
    pub(crate) error: Option<DialError>,
    pub(crate) reader: Option<Waker>,
    pub(crate) writer: Option<Waker>,
    pub(crate) interim_responses: u8,
}

impl Flow {
    pub(crate) fn lock(&self) -> MutexGuard<'_, FlowState> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
    pub(crate) fn fail(&self, error: DialError) {
        let mut state = self.lock();
        state.error = Some(error);
        state.incoming.clear();
        state.outgoing.clear();
        state.incoming_bytes = 0;
        state.outgoing_bytes = 0;
        if let Some(w) = state.reader.take() {
            w.wake();
        }
        if let Some(w) = state.writer.take() {
            w.wake();
        }
        drop(state);
        if let Some(reply) = self.reply.lock().unwrap_or_else(|e| e.into_inner()).take() {
            self.metrics.update(|m| {
                m.connect_failures += 1;
                if error == DialError::Timeout {
                    m.connect_timeouts += 1;
                }
            });
            let _ = reply.send(Err(error));
        }
    }

    pub(crate) fn accept(self: &Arc<Self>) {
        self.lock().accepted = true;
        self._pending
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        if let Some(reply) = self.reply.lock().unwrap_or_else(|e| e.into_inner()).take() {
            self.metrics.update(|m| {
                m.connect_successes += 1;
                m.connect_latency_us =
                    self.started.elapsed().as_micros().min(u128::from(u64::MAX)) as u64;
            });
            self.delivered.store(true, Ordering::Release);
            let _ = reply.send(Ok(Box::new(L4Stream(self.clone()))));
        }
    }
}

impl Drop for Flow {
    fn drop(&mut self) {
        self.metrics
            .update(|m| m.active_flows = m.active_flows.saturating_sub(1));
        self.wake.notify_one();
        self.budget.wake.notify_waiters();
    }
}

pub(crate) struct L4Stream(pub(crate) Arc<Flow>);

impl TcpIo for L4Stream {
    fn session_generation(&self) -> Option<u64> {
        Some(self.0.session_generation)
    }
    fn local_addr(&self) -> io::Result<SocketAddr> {
        Ok(if self.0.target.authority().starts_with('[') {
            SocketAddr::from((Ipv6Addr::UNSPECIFIED, 0))
        } else {
            SocketAddr::from((Ipv4Addr::UNSPECIFIED, 0))
        })
    }
}

impl AsyncRead for L4Stream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        out: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let flow = &self.0;
        let mut state = flow.lock();
        if out.remaining() == 0 {
            return Poll::Ready(Ok(()));
        }
        if let Some(error) = state.error {
            return Poll::Ready(Err(error.into()));
        }
        if let Some(bytes) = state.incoming.front_mut() {
            let n = out.remaining().min(bytes.len());
            out.put_slice(&bytes[..n]);
            bytes.advance(n);
            if bytes.is_empty() {
                state.incoming.pop_front();
            }
            state.incoming_bytes -= n;
            flow.counters.record_received(n);
            flow.wake.notify_one();
            return Poll::Ready(Ok(()));
        }
        if state.read_finished && !state.read_pending {
            return Poll::Ready(Ok(()));
        }
        state.reader = Some(cx.waker().clone());
        Poll::Pending
    }
}

impl AsyncWrite for L4Stream {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        let flow = &self.0;
        let mut state = flow.lock();
        if let Some(error) = state.error {
            return Poll::Ready(Err(error.into()));
        }
        if state.fin_requested {
            return Poll::Ready(Err(io::ErrorKind::BrokenPipe.into()));
        }
        if bytes.is_empty() {
            return Poll::Ready(Ok(0));
        }
        let n = bytes
            .len()
            .min(super::CHUNK_SIZE)
            .min(super::FLOW_BUFFER.saturating_sub(state.outgoing_bytes));
        if n != 0
            && let Some(buffer) = flow.budget.copy(&bytes[..n])
        {
            state.outgoing.push_back(buffer);
            state.outgoing_bytes += n;
            flow.counters.record_sent(n);
            flow.wake.notify_one();
            return Poll::Ready(Ok(n));
        }
        state.writer = Some(cx.waker().clone());
        flow.metrics.update(|m| m.send_backpressure += 1);
        Poll::Pending
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let mut state = self.0.lock();
        if let Some(error) = state.error {
            return Poll::Ready(Err(error.into()));
        }
        if state.outgoing_bytes == 0 {
            return Poll::Ready(Ok(()));
        }
        state.writer = Some(cx.waker().clone());
        Poll::Pending
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let mut state = self.0.lock();
        if let Some(error) = state.error {
            return Poll::Ready(Err(error.into()));
        }
        state.fin_requested = true;
        if state.fin_sent {
            return Poll::Ready(Ok(()));
        }
        state.writer = Some(cx.waker().clone());
        self.0.wake.notify_one();
        Poll::Pending
    }
}

impl Drop for L4Stream {
    fn drop(&mut self) {
        self.0.lock().dropped = true;
        self.0.wake.notify_one();
    }
}
