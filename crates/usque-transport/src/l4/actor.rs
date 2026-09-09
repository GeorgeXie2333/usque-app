use std::collections::{BTreeMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use quiche::h3::NameValue;
use tokio::sync::Notify;
use tokio::time::Instant;

use super::stream::Flow;
use super::{BufferBudget, L4Metrics};
use super::{budget_wait::BudgetWaiter, pool::ReceivePool};
use crate::h2::TransportError;
use crate::h3_buffer::H3BufferFactory;
use crate::tcp::DialError;

type Connection = quiche::Connection<H3BufferFactory>;

pub(crate) struct SessionHandle {
    pub(crate) epoch: AtomicU64,
    pub(crate) closed: AtomicBool,
    pub(crate) draining: AtomicBool,
    pub(crate) verified: AtomicBool,
    pub(crate) wake: Arc<Notify>,
    pub(crate) changed: Arc<Notify>,
    incoming: Mutex<VecDeque<Arc<Flow>>>,
}

impl SessionHandle {
    pub(crate) fn enqueue(&self, flow: Arc<Flow>) -> Result<(), Arc<Flow>> {
        let mut queue = self.incoming.lock().unwrap_or_else(|e| e.into_inner());
        if self.closed.load(Ordering::Acquire) || self.draining.load(Ordering::Acquire) {
            return Err(flow);
        }
        queue.push_back(flow);
        self.wake.notify_one();
        Ok(())
    }
}

pub(crate) struct L4Actor {
    #[cfg(test)]
    pub(crate) test_options: super::test_options::TestOptions,
    pub(crate) session_slot: Option<Arc<tokio::sync::OwnedSemaphorePermit>>,
    pub(crate) handle: Arc<SessionHandle>,
    pub(crate) budget: Arc<BufferBudget>,
    pending: VecDeque<Arc<Flow>>,
    flows: BTreeMap<u64, Arc<Flow>>,
    round_robin: VecDeque<u64>,
    goaway_id: Option<u64>,
    pub(crate) drain_deadline: Option<Instant>,
    metrics: Arc<L4Metrics>,
    pool: Arc<ReceivePool>,
    receive_waiter: Arc<BudgetWaiter>,
    progressed: bool,
    woken: bool,
}

impl L4Actor {
    pub(crate) fn record_wakeup(&mut self) {
        self.woken = true;
        self.metrics
            .performance
            .actor_wakeups
            .fetch_add(1, Ordering::Relaxed);
    }
    pub(crate) fn has_activity(&self) -> bool {
        !self.flows.is_empty()
            || !self.pending.is_empty()
            || !self
                .handle
                .incoming
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .is_empty()
    }

    pub(crate) fn new(
        budget: Arc<BufferBudget>,
        metrics: Arc<L4Metrics>,
        changed: Arc<Notify>,
    ) -> Self {
        let wake = Arc::new(Notify::new());
        let pool = ReceivePool::shared(&budget);
        let receive_waiter = BudgetWaiter::new(&budget, Some(wake.clone()));
        Self {
            #[cfg(test)]
            test_options: super::test_options::TestOptions::default(),
            session_slot: None,
            handle: Arc::new(SessionHandle {
                epoch: AtomicU64::new(0),
                closed: AtomicBool::new(false),
                draining: AtomicBool::new(false),
                verified: AtomicBool::new(false),
                wake,
                changed,
                incoming: Mutex::new(VecDeque::new()),
            }),
            budget,
            pending: VecDeque::new(),
            flows: BTreeMap::new(),
            round_robin: VecDeque::new(),
            goaway_id: None,
            drain_deadline: None,
            metrics,
            pool,
            receive_waiter,
            progressed: false,
            woken: false,
        }
    }

    pub(crate) fn pump(
        &mut self,
        h3: &mut quiche::h3::Connection,
        conn: &mut Connection,
        allow_send: bool,
    ) -> Result<(), TransportError> {
        self.metrics
            .performance
            .actor_polls
            .fetch_add(1, Ordering::Relaxed);
        self.progressed = false;
        let woken = std::mem::take(&mut self.woken);
        self.receive_waiter.disarm();
        if self
            .drain_deadline
            .is_some_and(|deadline| Instant::now() >= deadline)
        {
            return Err(TransportError::TunnelClosed);
        }
        self.pending.extend(
            self.handle
                .incoming
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .drain(..),
        );
        self.events(h3, conn)?;
        if allow_send {
            for _ in 0..self.pending.len() {
                let Some(flow) = self.pending.pop_front() else {
                    break;
                };
                if flow.cancellation.is_cancelled()
                    || flow
                        .reply
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .as_ref()
                        .is_none_or(|r| r.is_closed())
                {
                    flow.fail(DialError::Cancelled);
                    continue;
                }
                if Instant::now() >= flow.deadline {
                    flow.fail(DialError::Timeout);
                    continue;
                }
                if self.goaway_id.is_some() {
                    flow.fail(DialError::Closed);
                    continue;
                }
                let headers = [
                    quiche::h3::Header::new(b":method", b"CONNECT"),
                    quiche::h3::Header::new(b":authority", flow.target.authority().as_bytes()),
                ];
                match h3.send_request(conn, &headers, false) {
                    Ok(id) => {
                        self.progressed = true;
                        flow.stream_id.store(id, Ordering::Release);
                        self.flows.insert(id, flow);
                        self.round_robin.push_back(id);
                    }
                    Err(
                        quiche::h3::Error::StreamBlocked
                        | quiche::h3::Error::TransportError(quiche::Error::StreamLimit),
                    ) => {
                        self.pending.push_front(flow);
                        break;
                    }
                    Err(_) => {
                        flow.fail(DialError::Protocol);
                    }
                }
            }
        }
        let mut units = 64usize;
        let mut budget_waiting = false;
        let count = self.round_robin.len();
        for _ in 0..count {
            let Some(id) = self.round_robin.pop_front() else {
                break;
            };
            let Some(flow) = self.flows.get(&id).cloned() else {
                continue;
            };
            if !flow.lock().accepted && Instant::now() >= flow.deadline {
                flow.fail(DialError::Timeout);
            }
            if flow.cancellation.is_cancelled() {
                flow.fail(DialError::Cancelled);
            }
            let mut state = flow.lock();
            if state.error.is_some() || state.dropped {
                if !(state.fin_sent && state.read_finished && state.incoming.is_empty()) {
                    let _ = conn.stream_shutdown(id, quiche::Shutdown::Read, 0x10c);
                    let _ = conn.stream_shutdown(id, quiche::Shutdown::Write, 0x10c);
                }
                drop(state);
                self.flows.remove(&id);
                continue;
            }
            if allow_send && state.accepted && units != 0 {
                if let Some(bytes) = state.outgoing.front_mut() {
                    let before = bytes.len();
                    match h3.send_body_zc(conn, id, bytes, false) {
                        Ok(n) => {
                            self.progressed |= n != 0;
                            if n != 0
                                && let Some(w) = state.writer.take()
                            {
                                w.wake();
                            }
                            if n == before {
                                state.outgoing.pop_front();
                            }
                            state.outgoing_bytes = state.outgoing_bytes.saturating_sub(n);
                            if n != 0 && !state.outgoing.is_empty() {
                                // Drain already accepted bytes without waiting
                                // for another packet or a producer to write.
                                self.handle.wake.notify_one();
                            }
                            units -= 1;
                        }
                        Err(quiche::h3::Error::Done) => {}
                        Err(_) => {
                            drop(state);
                            flow.fail(DialError::Closed);
                            self.round_robin.push_back(id);
                            continue;
                        }
                    }
                }
                if state.fin_requested && !state.fin_sent && state.outgoing.is_empty() {
                    match h3.send_body(conn, id, &[], true) {
                        Ok(_) => {
                            state.fin_sent = true;
                            self.progressed = true;
                            if let Some(w) = state.writer.take() {
                                w.wake();
                            }
                        }
                        Err(quiche::h3::Error::Done) => {}
                        Err(_) => {
                            drop(state);
                            flow.fail(DialError::Closed);
                            self.round_robin.push_back(id);
                            continue;
                        }
                    }
                }
            }
            if state.read_pending && state.accepted && units != 0 {
                let size =
                    super::CHUNK_SIZE.min(super::FLOW_BUFFER.saturating_sub(state.incoming_bytes));
                if size > 0
                    && let Some(mut buffer) = self.pool.take().or_else(|| {
                        self.receive_waiter.arm(None);
                        let buffer = self.pool.take();
                        if buffer.is_none() {
                            budget_waiting = true;
                        } else if !budget_waiting {
                            self.receive_waiter.disarm();
                        }
                        buffer
                    })
                {
                    self.metrics
                        .performance
                        .h3_read_calls
                        .fetch_add(1, Ordering::Relaxed);
                    match h3.recv_body(conn, id, &mut buffer.as_mut()[..size]) {
                        Ok(n) if n != 0 => {
                            self.progressed = true;
                            self.metrics
                                .performance
                                .h3_read_bytes
                                .fetch_add(n as u64, Ordering::Relaxed);
                            state.incoming.push_back(buffer.freeze(n));
                            state.incoming_bytes += n;
                            units -= 1;
                            if let Some(w) = state.reader.take() {
                                w.wake();
                            }
                            // Edge-triggered DATA must be drained even without another packet.
                            self.handle.wake.notify_one();
                        }
                        Ok(_) | Err(quiche::h3::Error::Done) => {
                            self.metrics
                                .performance
                                .h3_empty_reads
                                .fetch_add(1, Ordering::Relaxed);
                            state.read_pending = false;
                            if state.read_finished
                                && let Some(w) = state.reader.take()
                            {
                                w.wake();
                            }
                        }
                        Err(_) => {
                            drop(state);
                            flow.fail(DialError::Closed);
                            self.round_robin.push_back(id);
                            continue;
                        }
                    }
                } else {
                    self.metrics.update(|m| m.receive_backpressure += 1);
                }
            }
            let finished = state.fin_sent
                && state.read_finished
                && !state.read_pending
                && state.incoming.is_empty();
            drop(state);
            if finished {
                self.flows.remove(&id);
            } else {
                self.round_robin.push_back(id);
            }
            if units == 0 {
                self.handle.wake.notify_one();
                break;
            }
        }
        if self.goaway_id.is_some() && self.flows.is_empty() && self.pending.is_empty() {
            return Err(TransportError::TunnelClosed);
        }
        if !self.progressed {
            if woken {
                self.metrics
                    .performance
                    .actor_no_progress_wakeups
                    .fetch_add(1, Ordering::Relaxed);
            }
            self.metrics
                .performance
                .actor_no_progress_polls
                .fetch_add(1, Ordering::Relaxed);
        }
        Ok(())
    }

    fn events(
        &mut self,
        h3: &mut quiche::h3::Connection,
        conn: &mut Connection,
    ) -> Result<(), TransportError> {
        for _ in 0..1024 {
            match h3.poll(conn) {
                Ok((id, quiche::h3::Event::Headers { list, .. })) => {
                    self.progressed = true;
                    let Some(flow) = self.flows.get(&id).cloned() else {
                        return Err(protocol_error());
                    };
                    let status = response_status(&list);
                    match status {
                        Ok(code @ 100..=199) if code != 101 => {
                            let mut state = flow.lock();
                            state.interim_responses += 1;
                            if state.accepted || state.interim_responses > 8 {
                                drop(state);
                                flow.fail(DialError::Protocol);
                            }
                        }
                        Ok(200..=299) if !flow.lock().accepted => {
                            flow.accept();
                            self.handle.verified.store(true, Ordering::Release);
                            self.handle.changed.notify_one();
                        }
                        Ok(300..=599) if !flow.lock().accepted => {
                            flow.fail(DialError::Rejected(status.unwrap()))
                        }
                        _ => flow.fail(DialError::Protocol),
                    }
                }
                Ok((id, quiche::h3::Event::Data)) => {
                    self.progressed = true;
                    if let Some(flow) = self.flows.get(&id) {
                        flow.lock().read_pending = true;
                    }
                }
                Ok((id, quiche::h3::Event::Finished)) => {
                    self.progressed = true;
                    if let Some(flow) = self.flows.get(&id) {
                        let mut state = flow.lock();
                        if !state.accepted {
                            drop(state);
                            flow.fail(DialError::Protocol);
                        } else {
                            state.read_finished = true;
                            if let Some(w) = state.reader.take() {
                                w.wake();
                            }
                        }
                    }
                }
                Ok((id, quiche::h3::Event::Reset(_))) => {
                    self.progressed = true;
                    if let Some(flow) = self.flows.get(&id) {
                        flow.fail(DialError::Closed);
                    }
                }
                Ok((id, quiche::h3::Event::GoAway)) => {
                    self.progressed = true;
                    if id % 4 != 0 || self.goaway_id.is_some_and(|old| id > old) {
                        return Err(protocol_error());
                    }
                    self.goaway_id = Some(id);
                    self.handle.draining.store(true, Ordering::Release);
                    self.drain_deadline
                        .get_or_insert(Instant::now() + super::DRAIN_TIMEOUT);
                    self.handle.changed.notify_one();
                    for (_, flow) in self.flows.range(id..) {
                        flow.fail(DialError::Closed);
                    }
                }
                Ok((_, quiche::h3::Event::PriorityUpdate)) => {}
                Err(quiche::h3::Error::Done) => return Ok(()),
                Err(_) => return Err(protocol_error()),
            }
        }
        self.handle.wake.notify_one();
        Ok(())
    }
}

fn protocol_error() -> TransportError {
    TransportError::Http3("L4 HTTP/3 protocol error".to_owned())
}

pub(super) fn response_status(headers: &[quiche::h3::Header]) -> Result<u16, DialError> {
    if headers.len() > 128
        || headers
            .iter()
            .map(|h| h.name().len() + h.value().len())
            .sum::<usize>()
            > 16 * 1024
    {
        return Err(DialError::Protocol);
    }
    let mut statuses = headers.iter().filter(|h| h.name() == b":status");
    let bytes = statuses.next().ok_or(DialError::Protocol)?.value();
    if statuses.next().is_some() || bytes.len() != 3 || !bytes.iter().all(u8::is_ascii_digit) {
        return Err(DialError::Protocol);
    }
    Ok(u16::from(bytes[0] - b'0') * 100
        + u16::from(bytes[1] - b'0') * 10
        + u16::from(bytes[2] - b'0'))
}

impl Drop for L4Actor {
    fn drop(&mut self) {
        self.handle.closed.store(true, Ordering::Release);
        for flow in self.flows.values().chain(self.pending.iter()) {
            if flow.delivered.load(Ordering::Acquire) {
                self.metrics.update(|m| m.reconnect_terminated_flows += 1);
            }
            flow.fail(DialError::Closed);
        }
        for flow in self
            .handle
            .incoming
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .drain(..)
        {
            flow.fail(DialError::Closed);
        }
        self.handle.changed.notify_one();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn response_headers_are_strict_and_bounded() {
        let status = |s: &[u8]| vec![quiche::h3::Header::new(b":status", s)];
        assert_eq!(response_status(&status(b"200")), Ok(200));
        assert_eq!(response_status(&status(b"403")), Ok(403));
        for value in [b"2x0".as_slice(), b"2000", b"", b" 20"] {
            assert!(response_status(&status(value)).is_err());
        }
        assert!(
            response_status(&[
                quiche::h3::Header::new(b":status", b"200"),
                quiche::h3::Header::new(b":status", b"201")
            ])
            .is_err()
        );
    }
}
