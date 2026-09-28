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
