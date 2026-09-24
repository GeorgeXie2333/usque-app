// Included in h2::tests to reuse the in-memory HTTP/2 peer and its flow control.

#[tokio::test]
async fn split_envelope_retains_ip_storage_and_returns_each_data_capacity() {
    let mut loopback = connect_h2_loopback().await;
    let packet = sized_ipv4_packet(1280);
    let capsule = encode_datagram_capsule(&packet).unwrap();
    let header_len = capsule.len() - packet.len();
    peer_send_all(&mut loopback.peer_send, capsule.slice(..header_len)).await;
    let header = loopback.receive.stream.data().await.unwrap().unwrap();
    loopback.receive.buffer_data(header).unwrap();
    loopback.receive.drain_ready_capsules().unwrap();
    assert_eq!(loopback.receive.stream.flow_control().used_capacity(), 0);
    // Dropping a pending receive cannot discard the already parsed envelope.
    assert!(std::future::poll_fn(|cx| Poll::Ready(
        std::pin::pin!(loopback.receive.receive_packet()).poll(cx)
    ))
    .await
    .is_pending());
    peer_send_all(&mut loopback.peer_send, capsule.slice(header_len..)).await;
    let data = loopback.receive.stream.data().await.unwrap().unwrap();
    let pointer = data.as_ptr();
    loopback.receive.buffer_data(data.clone()).unwrap();
    let received = loopback.receive.receive_packet().await.unwrap();
    assert_eq!(received, packet);
    assert_eq!(received.as_ptr(), pointer);
    assert_eq!(loopback.receive.stream.flow_control().used_capacity(), 0);
    assert_eq!(
        loopback.quality.performance().h2.snapshot().assembly_copy_bytes,
        header_len as u64
    );
}

#[tokio::test]
async fn complete_capsule_retains_data_storage_without_assembly_copy() {
    let mut loopback = connect_h2_loopback().await;
    let packet = sized_ipv4_packet(1280);
    let capsule = encode_datagram_capsule(&packet).unwrap();
    peer_send_all(&mut loopback.peer_send, capsule.clone()).await;
    let data = loopback.receive.stream.data().await.unwrap().unwrap();
    let payload_pointer = data.as_ptr().wrapping_add(capsule.len() - packet.len());
    loopback.receive.buffer_data(data.clone()).unwrap();
    let received = loopback.receive.receive_packet().await.unwrap();
    assert_eq!(received, packet);
    assert_eq!(received.as_ptr(), payload_pointer);
    assert_eq!(
        loopback
            .quality
            .performance()
            .h2
            .snapshot()
            .assembly_copy_bytes,
        0
    );
    assert_eq!(loopback.receive.stream.flow_control().used_capacity(), 0);
}

#[tokio::test]
async fn maximum_capsule_tail_and_following_capsule_can_share_data() {
    let mut loopback = connect_h2_loopback().await;
    let large = sized_ipv4_packet(65_535);
    let small = sized_ipv4_packet(20);
    let capsule = encode_datagram_capsule(&large).unwrap();
    let split = capsule.len() - 1;
    peer_send_all(&mut loopback.peer_send, capsule.slice(..split)).await;
    wait_for_buffered_data(&mut loopback.receive, split).await;
    let poll = std::future::poll_fn(|cx| {
        Poll::Ready(std::pin::pin!(loopback.receive.receive_packet()).poll(cx))
    })
    .await;
    assert!(poll.is_pending());
    let mut tail = BytesMut::from(&capsule[split..]);
    tail.extend_from_slice(&encode_datagram_capsule(&small).unwrap());
    peer_send_all(&mut loopback.peer_send, tail.freeze()).await;
    let mut batch = timeout(Duration::from_secs(2), loopback.receive.receive_batch())
        .await
        .unwrap()
        .expect("legal individual capsules must not trip an aggregate bound");
    assert_eq!(batch.pop_front().unwrap(), large);
    assert_eq!(batch.pop_front().unwrap(), small);
    assert!(batch.is_empty());
    assert_eq!(loopback.receive.stream.flow_control().used_capacity(), 0);
}

#[tokio::test]
async fn capsule_larger_than_receive_window_keeps_returning_capacity_once_per_data() {
    let mut loopback = connect_h2_loopback_with_config(H2FlowControlConfig {
        stream_receive_window: 1024,
        connection_receive_window: 1024,
    })
    .await;
    let packet = sized_ipv4_packet(65_535);
    let capsule = encode_datagram_capsule(&packet).unwrap();
    let bytes = capsule.len();
    timeout(Duration::from_secs(3), async {
        tokio::join!(peer_send_all(&mut loopback.peer_send, capsule), async {
            assert_eq!(loopback.receive.receive_packet().await.unwrap(), packet);
        });
    })
    .await
    .expect("fragment completion must not hold the small flow-control window");
    assert_eq!(loopback.receive.stream.flow_control().used_capacity(), 0);
    let counters = loopback.quality.performance().h2.snapshot();
    assert_eq!(counters.data_bytes, bytes as u64);
    assert!(counters.data_frames > 1);
}
