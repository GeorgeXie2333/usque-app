use super::*;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(super) enum BatchStop {
    #[default]
    Idle,
    Completed,
    ProducerCancelled,
    DatagramFull,
    EncodePoolExhausted,
    PmtuDeferred,
    DrainBudget,
}

#[derive(Debug, Default, Clone, Copy)]
pub(super) struct BatchProgress {
    pub(super) accepted: usize,
    pub(super) rejected: usize,
    pub(super) deferred: usize,
    pub(super) stop: BatchStop,
}

impl BatchProgress {
    pub(super) fn observe(self, quality: &NetworkQualityTelemetry) {
        debug_assert!(self.accepted + self.rejected + self.deferred <= UDP_ACTOR_DRAIN_LIMIT);
        let counters = &quality.performance().h3;
        if self.deferred != 0 {
            crate::transport_performance::add(&counters.pmtu_deferred, self.deferred as u64);
        }
        let stopped = match self.stop {
            BatchStop::DatagramFull => Some(&counters.datagram_queue_full),
            BatchStop::EncodePoolExhausted => Some(&counters.encode_pool_exhausted),
            _ => None,
        };
        if let Some(counter) = stopped {
            crate::transport_performance::add(counter, 1);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum WireStop {
    QueueFull,
    Quantum,
    Done { backlog: bool },
}

#[derive(Debug, Clone, Copy)]
pub(super) struct WireProgress {
    pub(super) packets: usize,
    pub(super) bytes: usize,
    pub(super) stop: WireStop,
}

impl WireProgress {
    pub(super) fn observe(self, quality: &NetworkQualityTelemetry) {
        debug_assert!(self.packets <= MAX_PENDING_WIRE_DATAGRAMS);
        debug_assert!(self.packets != 0 || self.bytes == 0);
        let counters = &quality.performance().h3;
        let stopped = match self.stop {
            WireStop::QueueFull => Some(&counters.wire_queue_full),
            WireStop::Quantum => Some(&counters.quantum_limited),
            WireStop::Done { backlog: true } => Some(&counters.quic_no_progress_with_backlog),
            WireStop::Done { backlog: false } => None,
        };
        if let Some(counter) = stopped {
            crate::transport_performance::add(counter, 1);
        }
    }
}

/// An immutable header prepared once per synchronous batch step. No heap
/// allocation and no assumptions about the negotiated request stream's width.
pub(super) struct DatagramHeader {
    bytes: [u8; 16],
    pub(super) len: usize,
}
impl DatagramHeader {
    pub(super) fn new(stream_id: u64) -> Result<Self, TransportError> {
        let mut header = Self {
            bytes: [0; 16],
            len: 0,
        };
        for value in [stream_id / 4, usque_protocol::DEFAULT_CONTEXT_ID] {
            let width = encoded_varint_len(value)?;
            let target = &mut header.bytes[header.len..header.len + width];
            target.copy_from_slice(&value.to_be_bytes()[8 - width..]);
            target[0] |= (width.ilog2() as u8) << 6;
            header.len += width;
        }
        Ok(header)
    }
    pub(super) fn as_bytes(&self) -> &[u8] {
        &self.bytes[..self.len]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ReadyAdmission {
    Blocked,
    Empty,
    Received,
    Closed,
}

pub(super) fn claim_ready_batch(
    outgoing: &mut mpsc::Receiver<OutgoingBatch>,
    pending: &mut Option<OutgoingBatch>,
    ready: bool,
    injection_allowed: bool,
    quality: &NetworkQualityTelemetry,
) -> ReadyAdmission {
    if !ready || !injection_allowed || pending.is_some() {
        return ReadyAdmission::Blocked;
    }
    match outgoing.try_recv() {
        Ok(batch) => {
            observe_outgoing_batch(quality, &batch);
            *pending = Some(batch);
            ReadyAdmission::Received
        }
        Err(mpsc::error::TryRecvError::Empty) => ReadyAdmission::Empty,
        Err(mpsc::error::TryRecvError::Disconnected) => ReadyAdmission::Closed,
    }
}
