use super::tests::{established_test_pair, ipv4_packet_with_length};
use super::*;

fn outgoing(count: usize) -> (OutgoingBatch, oneshot::Receiver<PacketBatchResult>) {
    let mut batch = PacketBatch::new();
    for _ in 0..count {
        batch
            .push_back(Bytes::from(ipv4_packet_with_length(64)))
            .unwrap();
    }
    let (completion, result) = oneshot::channel();
    (
        OutgoingBatch {
            batch,
            result: PacketBatchResult::default(),
            completion,
        },
        result,
    )
}

#[tokio::test]
async fn oversized_probe_preserves_unsent_tail_after_partial_udp_send() {
    use crate::{FaultKind, FaultScript, ScheduledFault};
    let receiver = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let from = socket.local_addr().unwrap();
    let to = receiver.local_addr().unwrap();
    let quality = NetworkQualityTelemetry::default();
    let path = PathSocket::spawn(
        PathId::new(0),
        from,
        to,
        0,
        PathSocketRole::Active,
        socket,
        DirectEgressLease::for_generation(0),
        quality.clone(),
        UdpReceivePool::default(),
    )
    .unwrap();
    let mut paths = PathSocketSet::with_active(path).unwrap();
    let queue = quality.register_queue(QueueKind::H3WireSend, 64, 64 * 1472);
    let mut pending = VecDeque::new();
    for (marker, length) in [(1, 100), (2, 1472), (3, 100)] {
        pending.push_back(WireDatagram {
            bytes: vec![marker; length],
            send_info: quiche::SendInfo {
                from,
                to,
                at: StdInstant::now(),
            },
            queue_entry: queue.start_entry(length),
        });
    }
    quality.inject_fault_script(
        FaultScript::new(
            2,
            vec![
                ScheduledFault {
                    at: Duration::ZERO,
                    fault: FaultKind::SendMmsgPartial(1),
                },
                ScheduledFault {
                    at: Duration::ZERO,
                    fault: FaultKind::SendMessageTooLarge,
                },
            ],
        )
        .unwrap(),
    );
    let mut free = Vec::new();
    let cancel = CancellationToken::new();
    assert_eq!(
        send_due_wire_datagrams(
            &paths,
            &mut pending,
            &mut free,
            4096,
            &queue,
            &quality,
            &cancel
        )
        .await
        .unwrap(),
        WireSendOutcome::Sent
    );
    assert_eq!(pending.len(), 2);
    assert_eq!(
        send_due_wire_datagrams(
            &paths,
            &mut pending,
            &mut free,
            4096,
            &queue,
            &quality,
            &cancel
        )
        .await
        .unwrap(),
        WireSendOutcome::MessageTooLarge {
            payload_len: 1472,
            from,
            to
        }
    );
    assert_eq!(
        pending.len(),
        1,
        "an oversized probe must not discard the following small packet"
    );
    assert_eq!(pending.front().unwrap().bytes, vec![3; 100]);
    assert_eq!(
        send_due_wire_datagrams(
            &paths,
            &mut pending,
            &mut free,
            4096,
            &queue,
            &quality,
            &cancel
        )
        .await
        .unwrap(),
        WireSendOutcome::Sent
    );
    assert!(pending.is_empty());
    for marker in [1, 3] {
        let mut received = [0; 2048];
        let length = timeout(Duration::from_millis(250), receiver.recv(&mut received))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(&received[..length], &[marker; 100]);
    }
    assert_eq!(queue.snapshot(Instant::now()).current_items, 0);
    paths.shutdown_all().await;
}

#[test]
fn one_ready_batch_queries_the_effective_payload_limit_once() {
    let (mut connection, _, _, _) = established_test_pair();
    let quality = NetworkQualityTelemetry::default();
    let queue = quality.register_queue(QueueKind::H3DatagramSend, 1024, 1024 * 2048);
    let pool = DatagramEncodePool::new(quality.clone());
    let (batch, mut result) = outgoing(64);
    let mut pending = Some(batch);
    PAYLOAD_LIMIT_LOOKUPS.with(|count| count.set(0));
    queue_pending_batch(
        &mut connection,
        0,
        &mut pending,
        &mut VecDeque::new(),
        &queue,
        &quality,
        &pool,
        1280,
    )
    .unwrap();
    assert_eq!(result.try_recv().unwrap().accepted_bytes, 64 * 64);
    assert_eq!(PAYLOAD_LIMIT_LOOKUPS.with(|count| count.get()), 1);
}

#[test]
fn ready_admission_claims_one_batch_and_respects_startup_and_migration() {
    let quality = NetworkQualityTelemetry::default();
    let (tx, mut rx) = mpsc::channel(2);
    let (first, _first_result) = outgoing(1);
    let (second, _second_result) = outgoing(2);
    assert!(tx.try_send(first).is_ok());
    assert!(tx.try_send(second).is_ok());
    let mut pending = None;
    for (ready, allowed) in [(false, true), (true, false)] {
        assert_eq!(
            claim_ready_batch(&mut rx, &mut pending, ready, allowed, &quality),
            ReadyAdmission::Blocked
        );
        assert_eq!(rx.len(), 2);
    }
    assert_eq!(
        claim_ready_batch(&mut rx, &mut pending, true, true, &quality),
        ReadyAdmission::Received
    );
    assert_eq!(pending.as_ref().unwrap().batch.len(), 1);
    assert_eq!(rx.len(), 1);
    assert_eq!(
        claim_ready_batch(&mut rx, &mut pending, true, true, &quality),
        ReadyAdmission::Blocked
    );
    pending.take();
    assert_eq!(
        claim_ready_batch(&mut rx, &mut pending, true, true, &quality),
        ReadyAdmission::Received
    );
    assert_eq!(pending.take().unwrap().batch.len(), 2);
    assert_eq!(
        claim_ready_batch(&mut rx, &mut pending, true, true, &quality),
        ReadyAdmission::Empty
    );
    drop(tx);
    assert_eq!(
        claim_ready_batch(&mut rx, &mut pending, true, true, &quality),
        ReadyAdmission::Closed
    );
    let counters = quality.performance().h3.snapshot();
    assert_eq!(
        (counters.application_batches, counters.application_packets),
        (2, 3)
    );
}

#[test]
fn exhausted_encode_pool_resumes_one_packet_then_completes_the_same_batch() {
    let (mut connection, _, _, _) = established_test_pair();
    let quality = NetworkQualityTelemetry::default();
    let queue = quality.register_queue(QueueKind::H3DatagramSend, 1024, 1024 * 2048);
    let pool = DatagramEncodePool::new(quality.clone());
    let mut held: Vec<_> = (0..crate::h3_buffer::HTTP_DATAGRAM_ENCODE_POOL_LIMIT)
        .map(|_| pool.take().unwrap())
        .collect();
    let (batch, mut result) = outgoing(64);
    let mut pending = Some(batch);
    let mut entries = VecDeque::new();
    let mut step = |connection: &mut H3QuicConnection| {
        queue_pending_batch(
            connection,
            0,
            &mut pending,
            &mut entries,
            &queue,
            &quality,
            &pool,
            1280,
        )
        .unwrap()
    };
    let progress = step(&mut connection);
    assert_eq!(progress.stop, BatchStop::EncodePoolExhausted);
    assert_eq!(progress.accepted, 0);
    drop(held.pop());
    let progress = step(&mut connection);
    assert_eq!(
        (progress.accepted, progress.stop),
        (1, BatchStop::EncodePoolExhausted)
    );
    drop(held);
    let progress = step(&mut connection);
    assert_eq!(
        (progress.accepted, progress.stop),
        (63, BatchStop::Completed)
    );
    assert_eq!(result.try_recv().unwrap().accepted_bytes, 64 * 64);
    assert_eq!(connection.dgram_send_queue_len(), 64);
    assert!(pending.is_none());
    assert_eq!(quality.performance().h3.snapshot().encode_pool_exhausted, 2);
    connection.dgram_purge_outgoing(|_: &[u8]| true);
    reconcile_datagram_queue(&connection, &mut entries, &queue);
    assert_eq!(queue.snapshot(Instant::now()).current_items, 0);
}

#[test]
fn datagram_full_preserves_the_batch_until_capacity_returns() {
    let (mut connection, _, _, _) = established_test_pair();
    while !connection.is_dgram_send_queue_full() {
        connection.dgram_send(&[0]).unwrap();
    }
    let quality = NetworkQualityTelemetry::default();
    let queue = quality.register_queue(QueueKind::H3DatagramSend, 1024, 1024 * 2048);
    let pool = DatagramEncodePool::new(quality.clone());
    let (batch, mut result) = outgoing(1);
    let mut pending = Some(batch);
    let mut entries = VecDeque::new();
    let progress = queue_pending_batch(
        &mut connection,
        0,
        &mut pending,
        &mut entries,
        &queue,
        &quality,
        &pool,
        1280,
    )
    .unwrap();
    assert_eq!(
        (progress.accepted, progress.stop),
        (0, BatchStop::DatagramFull)
    );
    assert_eq!(pending.as_ref().unwrap().batch.len(), 1);
    connection.dgram_purge_outgoing(|_: &[u8]| true);
    let progress = queue_pending_batch(
        &mut connection,
        0,
        &mut pending,
        &mut entries,
        &queue,
        &quality,
        &pool,
        1280,
    )
    .unwrap();
    assert_eq!(
        (progress.accepted, progress.stop),
        (1, BatchStop::Completed)
    );
    assert_eq!(result.try_recv().unwrap().accepted_bytes, 64);
    assert_eq!(quality.performance().h3.snapshot().datagram_queue_full, 1);
}

#[test]
fn wire_progress_distinguishes_quantum_queue_capacity_and_done_with_backlog() {
    let (mut connection, _, from, to) = established_test_pair();
    for _ in 0..1000 {
        connection.dgram_send(&[1; 64]).unwrap();
    }
    let quality = NetworkQualityTelemetry::default();
    let queue = quality.register_queue(
        QueueKind::H3WireSend,
        MAX_PENDING_WIRE_DATAGRAMS,
        MAX_PENDING_WIRE_DATAGRAMS * 1500,
    );
    let active = crate::path_socket::PathBinding {
        path_id: PathId::new(0),
        local_addr: from,
        peer_addr: to,
        network_generation: 0,
    };
    let mut pending = VecDeque::new();
    let mut free = Vec::new();
    let zero = generate_wire_datagrams(
        &mut connection,
        &mut pending,
        &mut free,
        1199,
        1500,
        &queue,
        &quality,
        active,
    )
    .unwrap();
    assert_eq!(
        (zero.packets, zero.bytes, zero.stop),
        (0, 0, WireStop::Quantum)
    );
    let small = generate_wire_datagrams(
        &mut connection,
        &mut pending,
        &mut free,
        1200,
        1500,
        &queue,
        &quality,
        active,
    )
    .unwrap();
    assert!(small.bytes <= 1200 && small.packets <= 1);
    let full = generate_wire_datagrams(
        &mut connection,
        &mut pending,
        &mut free,
        usize::MAX,
        1500,
        &queue,
        &quality,
        active,
    )
    .unwrap();
    assert_eq!(full.stop, WireStop::Done { backlog: true });
    assert_eq!(
        quality
            .performance()
            .h3
            .snapshot()
            .quic_no_progress_with_backlog,
        1
    );
    let before = pending.len();
    assert_eq!(before, small.packets + full.packets);
    assert_eq!(
        pending.iter().map(|p| p.bytes.len()).sum::<usize>(),
        small.bytes + full.bytes
    );
    while pending.len() < MAX_PENDING_WIRE_DATAGRAMS {
        pending.push_back(WireDatagram {
            bytes: vec![0; 100],
            send_info: quiche::SendInfo {
                from,
                to,
                at: StdInstant::now(),
            },
            queue_entry: queue.start_entry(100),
        });
    }
    let blocked = generate_wire_datagrams(
        &mut connection,
        &mut pending,
        &mut free,
        usize::MAX,
        1500,
        &queue,
        &quality,
        active,
    )
    .unwrap();
    assert_eq!((blocked.packets, blocked.stop), (0, WireStop::QueueFull));
}

#[tokio::test]
async fn public_send_interfaces_still_reject_invalid_packets_before_admission() {
    let (tx, mut rx) = mpsc::channel(1);
    let mut send = H3SendHalf { sender: Some(tx) };
    assert!(matches!(
        send.send_packet(&[0]).await,
        Err(TransportError::MalformedIpPacket)
    ));
    assert!(matches!(
        send.send_owned_batch(PacketBatch::single(Bytes::from_static(&[0])))
            .await,
        Err(TransportError::MalformedIpPacket)
    ));
    assert!(rx.try_recv().is_err());
}
