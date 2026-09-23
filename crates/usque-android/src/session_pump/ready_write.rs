use super::ReadyBatch;
use bytes::Bytes;
use std::io;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReadyWriteEvent {
    Started,
    Finished,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReadyDrainStop {
    Idle,
    Budget,
    WouldBlock,
    Cancelled,
}

#[derive(Debug)]
pub(crate) enum ReadyDrainError<E> {
    Receive(E),
    Write(io::Error),
}

/// Drain only ready packets through one pending slot. Every write is one IP
/// packet; WouldBlock and budget exhaustion retain exactly that pending packet.
pub(crate) fn drain_ready_packets<E>(
    pending: &mut Option<Bytes>,
    batch: &mut ReadyBatch,
    cancelled: impl Fn() -> bool,
    mut receive: impl FnMut() -> Result<Option<Bytes>, E>,
    mut write: impl FnMut(&[u8]) -> io::Result<()>,
    mut observer: Option<impl FnMut(ReadyWriteEvent)>,
) -> Result<ReadyDrainStop, ReadyDrainError<E>> {
    loop {
        if cancelled() {
            return Ok(ReadyDrainStop::Cancelled);
        }
        if pending.is_none() {
            let Some(packet) = receive().map_err(ReadyDrainError::Receive)? else {
                return Ok(ReadyDrainStop::Idle);
            };
            *pending = Some(packet);
            if let Some(observer) = observer.as_mut() {
                observer(ReadyWriteEvent::Started);
            }
        }
        let packet = pending.as_ref().expect("one pending packet");
        if !batch.allows(packet.len()) {
            return Ok(ReadyDrainStop::Budget);
        }
        match write(packet) {
            Ok(()) => {
                batch.completed(packet.len());
                *pending = None;
                if let Some(observer) = observer.as_mut() {
                    observer(ReadyWriteEvent::Finished);
                }
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                return Ok(ReadyDrainStop::WouldBlock);
            }
            Err(error) => return Err(ReadyDrainError::Write(error)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::time::{Duration, Instant};
    use tokio_util::sync::CancellationToken;

    fn packets(count: u8, bytes: usize) -> VecDeque<Bytes> {
        (0..count).map(|id| Bytes::from(vec![id; bytes])).collect()
    }
    fn untimed() -> ReadyBatch {
        let mut batch = ReadyBatch::new();
        batch.started = Instant::now() + Duration::from_secs(60);
        batch
    }

    #[test]
    fn ready_masque_and_l4_writes_use_the_same_packet_and_byte_budgets() {
        for observed in [false, true] {
            for size in [1280, 9000] {
                let mut queue = packets(20, size);
                let mut pending = None;
                let mut sent = Vec::new();
                let mut events = Vec::new();
                let mut batch = untimed();
                let stop = drain_ready_packets(
                    &mut pending,
                    &mut batch,
                    || false,
                    || Ok::<_, ()>(queue.pop_front()),
                    |packet| {
                        sent.push(packet[0]);
                        Ok(())
                    },
                    observed.then_some(|event| events.push(event)),
                )
                .unwrap();
                assert_eq!(stop, ReadyDrainStop::Budget);
                let count = 16.min((64 << 10) / size);
                assert_eq!(sent, (0..count as u8).collect::<Vec<_>>());
                assert_eq!(pending.as_ref().unwrap()[0], count as u8);
                assert_eq!(batch.bytes, count * size);
                if observed {
                    assert_eq!(
                        events
                            .iter()
                            .filter(|&&e| e == ReadyWriteEvent::Finished)
                            .count(),
                        count
                    );
                    assert_eq!(events.len(), count * 2 + 1);
                }
            }
        }
    }

    #[test]
    fn would_block_keeps_the_packet_and_resumes_without_duplicate_write() {
        let mut queue = packets(3, 1280);
        let mut pending = None;
        let stop = drain_ready_packets(
            &mut pending,
            &mut untimed(),
            || false,
            || Ok::<_, ()>(queue.pop_front()),
            |_| Err(io::ErrorKind::WouldBlock.into()),
            None::<fn(ReadyWriteEvent)>,
        )
        .unwrap();
        assert_eq!(stop, ReadyDrainStop::WouldBlock);
        assert_eq!(pending.as_ref().unwrap()[0], 0);
        assert_eq!(queue.len(), 2);
        let mut sent = Vec::new();
        let stop = drain_ready_packets(
            &mut pending,
            &mut untimed(),
            || false,
            || Ok::<_, ()>(queue.pop_front()),
            |packet| {
                sent.push(packet[0]);
                Ok(())
            },
            None::<fn(ReadyWriteEvent)>,
        )
        .unwrap();
        assert_eq!(stop, ReadyDrainStop::Idle);
        assert_eq!(sent, [0, 1, 2]);
        assert!(pending.is_none());
    }

    #[test]
    fn cancellation_is_checked_between_packets_and_a_new_session_starts_empty() {
        let token = CancellationToken::new();
        let mut queue = packets(3, 20);
        let mut pending = None;
        let mut sent = Vec::new();
        let stop = drain_ready_packets(
            &mut pending,
            &mut untimed(),
            || token.is_cancelled(),
            || Ok::<_, ()>(queue.pop_front()),
            |packet| {
                sent.push(packet[0]);
                token.cancel();
                Ok(())
            },
            None::<fn(ReadyWriteEvent)>,
        )
        .unwrap();
        assert_eq!(stop, ReadyDrainStop::Cancelled);
        assert_eq!(sent, [0]);
        assert!(pending.is_none());
        pending = Some(Bytes::from_static(b"old"));
        let stop = drain_ready_packets(
            &mut pending,
            &mut untimed(),
            || token.is_cancelled(),
            || Ok::<_, ()>(None),
            |_| panic!("no write after cancellation"),
            None::<fn(ReadyWriteEvent)>,
        )
        .unwrap();
        assert_eq!(stop, ReadyDrainStop::Cancelled);
        drop(pending.take()); // Session cleanup owns the pending slot.
        let fresh = CancellationToken::new();
        drain_ready_packets(
            &mut pending,
            &mut untimed(),
            || fresh.is_cancelled(),
            || Ok::<_, ()>(queue.pop_front()),
            |packet| {
                sent.push(packet[0]);
                Ok(())
            },
            None::<fn(ReadyWriteEvent)>,
        )
        .unwrap();
        assert_eq!(sent, [0, 1, 2]);
    }

    #[test]
    fn elapsed_budget_and_io_errors_return_control_with_owned_pending_packet() {
        let mut pending = Some(Bytes::from_static(b"packet"));
        let mut batch = ReadyBatch::new();
        batch.started = Instant::now() - Duration::from_micros(201);
        assert_eq!(
            drain_ready_packets(
                &mut pending,
                &mut batch,
                || false,
                || Ok::<_, ()>(None),
                |_| panic!("time budget exhausted"),
                None::<fn(ReadyWriteEvent)>
            )
            .unwrap(),
            ReadyDrainStop::Budget
        );
        let error = drain_ready_packets(
            &mut pending,
            &mut untimed(),
            || false,
            || Ok::<_, ()>(None),
            |_| Err(io::ErrorKind::BrokenPipe.into()),
            None::<fn(ReadyWriteEvent)>,
        )
        .unwrap_err();
        assert!(
            matches!(error, ReadyDrainError::Write(error) if error.kind() == io::ErrorKind::BrokenPipe)
        );
        assert_eq!(pending.as_deref(), Some(&b"packet"[..]));
        pending = None;
        let error = drain_ready_packets(
            &mut pending,
            &mut untimed(),
            || false,
            || Err::<Option<Bytes>, _>("closed"),
            |_| Ok(()),
            None::<fn(ReadyWriteEvent)>,
        )
        .unwrap_err();
        assert!(matches!(error, ReadyDrainError::Receive("closed")));
    }
}
