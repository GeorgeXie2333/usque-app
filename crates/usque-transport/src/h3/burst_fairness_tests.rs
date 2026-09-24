//! Deterministic encrypted-QUIC receive bursts under transient downstream pressure.
use super::tests::{
    advance_test_pair, encode_for_test, established_test_pair, ipv4_packet_with_length,
};
use super::*;
use std::task::{Context, Poll, Waker};

type Flight = Vec<(Vec<u8>, quiche::SendInfo)>;

fn packed_flight() -> (H3QuicConnection, Flight) {
    let (mut client, mut server, _, _) = established_test_pair();
    for _ in 0..32 {
        advance_test_pair(&mut client, &mut server).unwrap();
        client.on_timeout();
        server.on_timeout();
    }
    for marker in 0_u8..128 {
        let mut packet = ipv4_packet_with_length(64);
        packet[20] = marker;
        server.dgram_send_buf(encode_for_test(0, &packet)).unwrap();
    }
    let mut flight = Vec::new();
    loop {
        let mut wire = vec![0; crate::pmtu::IPV4_MAX_UDP_PAYLOAD];
        match server.send(&mut wire) {
            Ok((length, info)) => {
                wire.truncate(length);
                flight.push((wire, info));
            }
            Err(quiche::Error::Done) => break,
            Err(error) => panic!("prepare encrypted burst: {error:?}"),
        }
    }
    assert_eq!(server.dgram_send_queue_len(), 0);
    assert!((2..=UDP_ACTOR_DRAIN_LIMIT).contains(&flight.len()));
    (client, flight)
}

fn marker_batch(marker: u8) -> PacketBatch {
    let mut batch = PacketBatch::new();
    for _ in 0..MAX_PACKET_BATCH_PACKETS {
        let mut packet = ipv4_packet_with_length(64);
        packet[20] = marker;
        batch.push_back(Bytes::from(packet)).unwrap();
    }
    batch
}

fn append_markers(mut batch: PacketBatch, output: &mut Vec<u8>) {
    while let Some(packet) = batch.pop_front() {
        output.push(packet[20]);
    }
}

async fn receive_flight(
    client: &mut H3QuicConnection,
    flight: Flight,
    ready_stream_id: Option<u64>,
    incoming_tx: &mpsc::Sender<PacketBatch>,
    pending: &mut PacketBatch,
) -> Result<(usize, bool), TransportError> {
    let mut fairness = IncomingBurstFairness::default();
    let mut dropped = 0;
    for (mut wire, info) in flight {
        dropped += receive_and_drain_quic_datagram(
            client,
            &mut wire,
            quiche::RecvInfo {
                from: info.from,
                to: info.to,
            },
            ready_stream_id,
            incoming_tx,
            pending,
        )?;
        fairness
            .yield_once_if_blocked(client, ready_stream_id, incoming_tx, pending)
            .await?;
    }
    Ok((dropped, fairness.yielded))
}

#[tokio::test]
async fn blocked_consumer_can_progress_before_the_next_wire_packet_overflows_quic() {
    let (mut client, flight) = packed_flight();
    let (incoming_tx, mut incoming_rx) = mpsc::channel(1);
    incoming_tx.try_send(marker_batch(0xa0)).unwrap();
    let mut pending = marker_batch(0xb0);
    let mut receive = Box::pin(receive_flight(
        &mut client,
        flight,
        Some(0),
        &incoming_tx,
        &mut pending,
    ));
    let mut cx = Context::from_waker(Waker::noop());
    match receive.as_mut().poll(&mut cx) {
        Poll::Pending => {}
        Poll::Ready(Ok((dropped, _))) => {
            assert_eq!(
                dropped, 0,
                "the original uninterrupted burst loses 64 valid DATAGRAMs"
            );
            panic!("blocked consumer was never given a scheduling opportunity");
        }
        Poll::Ready(Err(error)) => panic!("receive failed: {error:?}"),
    }
    let mut delivered = Vec::new();
    append_markers(incoming_rx.try_recv().unwrap(), &mut delivered);
    assert_eq!(delivered, vec![0xa0; MAX_PACKET_BATCH_PACKETS]);
    assert!(matches!(
        receive.as_mut().poll(&mut cx),
        Poll::Ready(Ok((0, true)))
    ));
    drop(receive);
    // Drain without introducing more wire input; verify original batch ordering
    // and every input packet exactly once after the cooperative opportunity.
    for _ in 0..8 {
        while let Ok(batch) = incoming_rx.try_recv() {
            append_markers(batch, &mut delivered);
        }
        drain_received_datagrams(&mut client, 0, true, &incoming_tx, &mut pending).unwrap();
        if client.dgram_recv_queue_len() == 0 && pending.is_empty() {
            while let Ok(batch) = incoming_rx.try_recv() {
                append_markers(batch, &mut delivered);
            }
            break;
        }
    }
    let mut expected = vec![0xa0; MAX_PACKET_BATCH_PACKETS];
    expected.extend(vec![0xb0; MAX_PACKET_BATCH_PACKETS]);
    expected.extend(0_u8..128);
    assert_eq!(delivered, expected);
    assert_eq!(client.dgram_recv_queue_len(), 0);
    assert!(pending.is_empty());
}

#[tokio::test]
async fn permanent_downstream_pressure_yields_only_once_and_returns_to_actor() {
    let (mut client, flight) = packed_flight();
    let (incoming_tx, mut incoming_rx) = mpsc::channel(1);
    incoming_tx.try_send(marker_batch(0xa0)).unwrap();
    let mut pending = marker_batch(0xb0);
    let mut receive = Box::pin(receive_flight(
        &mut client,
        flight,
        Some(0),
        &incoming_tx,
        &mut pending,
    ));
    let mut cx = Context::from_waker(Waker::noop());
    assert!(receive.as_mut().poll(&mut cx).is_pending());
    // Capacity stays full. The second poll must finish the bounded burst rather
    // than waiting for the consumer or repeatedly yielding away wire sends.
    assert!(matches!(
        receive.as_mut().poll(&mut cx),
        Poll::Ready(Ok((64, true)))
    ));
    drop(receive);
    assert_eq!(client.dgram_recv_queue_len(), DATAGRAM_RECV_QUEUE_CAPACITY);
    assert_eq!(pending.len(), MAX_PACKET_BATCH_PACKETS);
    let mut queued = Vec::new();
    append_markers(incoming_rx.try_recv().unwrap(), &mut queued);
    assert_eq!(queued, vec![0xa0; MAX_PACKET_BATCH_PACKETS]);
    let mut retained = Vec::new();
    append_markers(pending, &mut retained);
    assert_eq!(retained, vec![0xb0; MAX_PACKET_BATCH_PACKETS]);
}

#[tokio::test]
async fn available_consumer_capacity_and_unaccepted_stream_do_not_yield() {
    for ready_stream_id in [Some(0), None] {
        let (mut client, flight) = packed_flight();
        let (incoming_tx, _incoming_rx) = mpsc::channel(INCOMING_BATCH_CHANNEL_CAPACITY);
        let mut pending = PacketBatch::new();
        let mut receive = Box::pin(receive_flight(
            &mut client,
            flight,
            ready_stream_id,
            &incoming_tx,
            &mut pending,
        ));
        let mut cx = Context::from_waker(Waker::noop());
        let expected_drops = if ready_stream_id.is_some() { 0 } else { 64 };
        assert!(
            matches!(receive.as_mut().poll(&mut cx), Poll::Ready(Ok((dropped, false))) if dropped == expected_drops)
        );
    }
}

#[tokio::test]
async fn consumer_close_during_the_yield_is_reported_without_more_wire_input() {
    let (mut client, flight) = packed_flight();
    let (incoming_tx, incoming_rx) = mpsc::channel(1);
    incoming_tx.try_send(marker_batch(0xa0)).unwrap();
    let mut pending = marker_batch(0xb0);
    let mut receive = Box::pin(receive_flight(
        &mut client,
        flight,
        Some(0),
        &incoming_tx,
        &mut pending,
    ));
    let mut cx = Context::from_waker(Waker::noop());
    assert!(receive.as_mut().poll(&mut cx).is_pending());
    drop(incoming_rx);
    assert!(matches!(
        receive.as_mut().poll(&mut cx),
        Poll::Ready(Err(TransportError::TunnelClosed))
    ));
}

#[tokio::test]
async fn cancellation_at_the_yield_preserves_the_retained_batch_and_quic_queue() {
    let (mut client, mut flight) = packed_flight();
    let (incoming_tx, mut incoming_rx) = mpsc::channel(1);
    incoming_tx.try_send(marker_batch(0xa0)).unwrap();
    let mut pending = marker_batch(0xb0);
    let (mut wire, info) = flight.remove(0);
    assert_eq!(
        receive_and_drain_quic_datagram(
            &mut client,
            &mut wire,
            quiche::RecvInfo {
                from: info.from,
                to: info.to
            },
            Some(0),
            &incoming_tx,
            &mut pending,
        )
        .unwrap(),
        0
    );
    let queued_before = client.dgram_recv_queue_len();
    assert!(queued_before > 0);
    let mut fairness = IncomingBurstFairness::default();
    let mut yielded =
        Box::pin(fairness.yield_once_if_blocked(&mut client, Some(0), &incoming_tx, &mut pending));
    let mut cx = Context::from_waker(Waker::noop());
    assert!(yielded.as_mut().poll(&mut cx).is_pending());
    drop(yielded);
    assert_eq!(client.dgram_recv_queue_len(), queued_before);
    assert_eq!(pending.len(), MAX_PACKET_BATCH_PACKETS);
    assert_eq!(
        incoming_rx.try_recv().unwrap().len(),
        MAX_PACKET_BATCH_PACKETS
    );
    assert!(fairness.yielded);
}
