//! Local ICMP must never backpressure the stack's outgoing packet drain.
use std::collections::VecDeque;

use super::{Bytes, IncomingDeliveryFuture, PacketBatch, PacketIo};

pub(super) const FEEDBACK_CAPACITY: usize = 32;

#[derive(Default)]
pub(super) struct IcmpFeedback {
    queued: VecDeque<Bytes>,
    pub(super) pending: Option<IncomingDeliveryFuture>,
    dropped: u64,
    high_watermark: usize,
}

impl IcmpFeedback {
    fn retained(&self) -> usize {
        self.queued.len() + usize::from(self.pending.is_some())
    }

    pub(super) fn push(&mut self, packet: Bytes) {
        if self.retained() == FEEDBACK_CAPACITY {
            self.dropped = self.dropped.saturating_add(1);
            if self.dropped.is_power_of_two() {
                tracing::warn!(
                    dropped = self.dropped,
                    high_watermark = self.high_watermark,
                    "local ICMP feedback queue full; dropping generated feedback"
                );
            }
            return;
        }
        self.queued.push_back(packet);
        self.high_watermark = self.high_watermark.max(self.retained());
    }

    pub(super) fn start_delivery(&mut self, io: &PacketIo) {
        if self.pending.is_none()
            && let Some(packet) = self.queued.pop_front()
        {
            self.pending = Some(io.start_incoming_batch(PacketBatch::single(packet)));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::packet_pipe::PacketPipe;
    use std::future::poll_fn;
    use std::task::Poll;

    #[tokio::test]
    async fn feedback_bound_includes_inflight_and_cancellation_does_not_replay() {
        let (pipe, mut peer) = PacketPipe::bounded(1);
        assert!(pipe.tx.try_send(b"occupied"));
        let io = PacketIo::from_pipe(pipe);
        let mut feedback = IcmpFeedback::default();
        feedback.push(Bytes::from_static(b"first"));
        feedback.start_delivery(&io);
        poll_fn(|cx| {
            assert!(
                feedback
                    .pending
                    .as_mut()
                    .unwrap()
                    .as_mut()
                    .poll(cx)
                    .is_pending()
            );
            Poll::Ready(())
        })
        .await;
        for _ in 0..100 {
            feedback.push(Bytes::from_static(b"next"));
        }
        assert_eq!(feedback.retained(), FEEDBACK_CAPACITY);
        assert_eq!(feedback.high_watermark, FEEDBACK_CAPACITY);
        assert_eq!(feedback.dropped, 101 - FEEDBACK_CAPACITY as u64);
        drop(feedback);
        assert_eq!(peer.rx.recv_async().await.unwrap().as_ref(), b"occupied");
        assert!(peer.rx.try_recv().is_none());
    }

    #[tokio::test]
    async fn feedback_detects_closed_pipe_and_retains_fifo_order() {
        let (pipe, mut peer) = PacketPipe::bounded(1);
        let io = PacketIo::from_pipe(pipe);
        let mut feedback = IcmpFeedback::default();
        for packet in [b"one", b"two"] {
            feedback.push(Bytes::copy_from_slice(packet));
        }
        for packet in [b"one", b"two"] {
            feedback.start_delivery(&io);
            assert!(feedback.pending.take().unwrap().await);
            assert_eq!(peer.rx.recv_async().await.unwrap().as_ref(), packet);
        }
        feedback.push(Bytes::from_static(b"closed"));
        drop(peer);
        feedback.start_delivery(&io);
        assert!(!feedback.pending.take().unwrap().await);
    }
}
