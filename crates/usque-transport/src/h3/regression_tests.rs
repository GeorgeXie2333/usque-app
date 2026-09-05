//! Wire-level and loopback regressions for the H3 receive and lifecycle fixes.
use super::tests::{
    advance_test_pair, encode_for_test, established_test_pair, ipv4_packet_with_length,
    test_quic_pair_with_config,
};
use super::*;

#[derive(Clone, Copy, Debug)]
pub(crate) enum ClosedTestChannel {
    Send,
    Receive,
    Control,
    All,
}

/// Simulate the actor's channel teardown preceding asynchronous path cleanup.
/// No network sockets or platform state are created by this fixture.
pub(crate) fn tunnel_with_closed_channel(
    channel: ClosedTestChannel,
    finish: oneshot::Receiver<Result<(), TransportError>>,
    closed: oneshot::Sender<()>,
) -> H3Tunnel {
    let (outgoing_tx, outgoing_rx) = mpsc::channel(1);
    let (incoming_tx, incoming_rx) = mpsc::channel(1);
    let (control_tx, control_rx) = watch::channel(PeerNetworkState::default());
    let (migration_tx, migration_rx) = mpsc::channel(1);
    let task = tokio::spawn(async move {
        let mut outgoing = Some(outgoing_rx);
        let mut incoming = Some(incoming_tx);
        let mut control = Some(control_tx);
        if matches!(channel, ClosedTestChannel::Send | ClosedTestChannel::All) {
            outgoing.take();
        }
        if matches!(channel, ClosedTestChannel::Receive | ClosedTestChannel::All) {
            incoming.take();
        }
        if matches!(channel, ClosedTestChannel::Control | ClosedTestChannel::All) {
            control.take();
        }
        let _ = closed.send(());
        let result = finish.await.expect("test supplies the driver result");
        drop((outgoing, incoming, control, migration_rx));
        result
    });
    H3Tunnel {
        send: H3SendHalf {
            sender: Some(outgoing_tx),
        },
        receive: H3ReceiveHalf {
            receiver: incoming_rx,
            pending: PacketBatch::new(),
        },
        driver: H3Driver { task: Some(task) },
        control: control_rx,
        migration: H3MigrationHandle::new(
            migration_tx,
            "127.0.0.1:443".parse().unwrap(),
            None,
            false,
        ),
        attempt: None,
    }
}

fn settle(client: &mut H3QuicConnection, server: &mut H3QuicConnection) {
    for _ in 0..32 {
        advance_test_pair(client, server).unwrap();
        client.on_timeout();
        server.on_timeout();
    }
}

fn burst(interleave: bool, datagrams_per_flight: u8) -> (usize, usize, usize) {
    let (mut client, mut server, _, _) = established_test_pair();
    settle(&mut client, &mut server);
    let mut flight = Vec::new();
    for index in 0_u8..128 {
        let mut packet = ipv4_packet_with_length(64);
        packet[20] = index;
        server.dgram_send_buf(encode_for_test(0, &packet)).unwrap();
        if (index + 1) % datagrams_per_flight != 0 {
            continue;
        }
        loop {
            let mut wire = vec![0; crate::pmtu::IPV4_MAX_UDP_PAYLOAD];
            match server.send(&mut wire) {
                Ok((length, info)) => {
                    wire.truncate(length);
                    flight.push((wire, info));
                }
                Err(quiche::Error::Done) => break,
                Err(error) => panic!("send: {error:?}"),
            }
        }
    }
    assert_eq!(server.dgram_send_queue_len(), 0);
    assert!(flight.len() <= UDP_ACTOR_DRAIN_LIMIT);
    let wire_count = flight.len();
    let (incoming_tx, mut incoming_rx) = mpsc::channel(INCOMING_BATCH_CHANNEL_CAPACITY);
    let mut pending = PacketBatch::new();
    let mut drops = 0;
    for (mut wire, info) in flight {
        drops += if interleave {
            receive_and_drain_quic_datagram(
                &mut client,
                &mut wire,
                quiche::RecvInfo {
                    from: info.from,
                    to: info.to,
                },
                Some(0),
                &incoming_tx,
                &mut pending,
            )
            .unwrap()
        } else {
            // Counterfactual: the old actor waited until the entire UDP batch
            // had been decoded. The same valid input then lost 64 packets.
            receive_quic_datagram(&mut client, &mut wire, info.from, info.to).unwrap()
        };
    }
    drain_received_datagrams(&mut client, 0, true, &incoming_tx, &mut pending).unwrap();
    let mut delivered = 0;
    while let Ok(batch) = incoming_rx.try_recv() {
        delivered += batch.len();
    }
    (wire_count, delivered, drops)
}

#[test]
fn packed_receive_burst_is_delivered_without_local_drops() {
    for datagrams_per_flight in [2, 128] {
        let actual = burst(true, datagrams_per_flight);
        let control = burst(false, datagrams_per_flight);
        assert_eq!((actual.1, actual.2), (128, 0));
        assert_eq!((control.1, control.2), (64, 64));
    }
}

#[test]
fn early_datagram_waits_for_response_processing() {
    let (mut client, mut server, _, _) = established_test_pair();
    settle(&mut client, &mut server);
    server
        .dgram_send_buf(encode_for_test(0, &ipv4_packet_with_length(64)))
        .unwrap();
    let (incoming_tx, mut incoming_rx) = mpsc::channel(1);
    let mut pending = PacketBatch::new();
    loop {
        let mut wire = vec![0; crate::pmtu::IPV4_MAX_UDP_PAYLOAD];
        match server.send(&mut wire) {
            Ok((length, info)) => {
                receive_and_drain_quic_datagram(
                    &mut client,
                    &mut wire[..length],
                    quiche::RecvInfo {
                        from: info.from,
                        to: info.to,
                    },
                    None,
                    &incoming_tx,
                    &mut pending,
                )
                .unwrap();
            }
            Err(quiche::Error::Done) => break,
            Err(error) => panic!("send: {error:?}"),
        }
    }
    assert_eq!(client.dgram_recv_queue_len(), 1);
    assert!(incoming_rx.try_recv().is_err());
    drain_received_datagrams(&mut client, 0, true, &incoming_tx, &mut pending).unwrap();
    assert_eq!(incoming_rx.try_recv().unwrap().len(), 1);
}

#[test]
fn goaway_preserves_an_accepted_open_connect_stream() {
    let (mut client, mut server, _, _) = established_test_pair();
    let mut config = quiche::h3::Config::new().unwrap();
    config.enable_extended_connect(true);
    let mut client_h3 = quiche::h3::Connection::with_transport(&mut client, &config).unwrap();
    let mut server_h3 = quiche::h3::Connection::with_transport(&mut server, &config).unwrap();
    settle(&mut client, &mut server);
    assert!(matches!(
        client_h3.poll(&mut client),
        Err(quiche::h3::Error::Done)
    ));
    assert!(matches!(
        server_h3.poll(&mut server),
        Err(quiche::h3::Error::Done)
    ));
    let stream = client_h3
        .send_request(&mut client, &connect_headers(), false)
        .unwrap();
    settle(&mut client, &mut server);
    assert!(
        matches!(server_h3.poll(&mut server), Ok((id, quiche::h3::Event::Headers { .. })) if id == stream)
    );
    server_h3
        .send_response(
            &mut server,
            stream,
            &[
                quiche::h3::Header::new(b":status", b"200"),
                quiche::h3::Header::new(b"capsule-protocol", b"?1"),
            ],
            false,
        )
        .unwrap();
    settle(&mut client, &mut server);
    let (control_tx, _) = watch::channel(PeerNetworkState::default());
    let mut control = ConnectIpControlPlane::new(control_tx);
    let mut accepted = false;
    let mut goaway = GoAwayState::default();
    process_http3_events(
        &mut client_h3,
        &mut client,
        Some(stream),
        &mut accepted,
        &mut control,
        &mut goaway,
    )
    .unwrap();
    assert!(accepted);
    server_h3.send_goaway(&mut server, stream + 4).unwrap();
    settle(&mut client, &mut server);
    let result = process_http3_events(
        &mut client_h3,
        &mut client,
        Some(stream),
        &mut accepted,
        &mut control,
        &mut goaway,
    );
    assert!(result.is_ok());
    assert!(goaway.deadline.is_some());
    assert!(!client.is_closed());
    assert!(!client.stream_finished(stream));
    server
        .dgram_send_buf(encode_for_test(stream, &ipv4_packet_with_length(20)))
        .unwrap();
    settle(&mut client, &mut server);
    assert!(
        client.dgram_recv_buf().is_ok(),
        "QUIC still transports data after GOAWAY"
    );
}

async fn loopback_peer(
    socket: UdpSocket,
    mut connection: H3QuicConnection,
    mut commands: mpsc::Receiver<Bytes>,
    cancel: CancellationToken,
) -> UdpSocket {
    let local = socket.local_addr().unwrap();
    let mut config = quiche::h3::Config::new().unwrap();
    config.enable_extended_connect(true);
    let mut http3 = quiche::h3::Connection::with_transport(&mut connection, &config).unwrap();
    let mut wire = vec![0; 65535];
    loop {
        maintain_connection_ids(&mut connection).unwrap();
        loop {
            match http3.poll(&mut connection) {
                Ok((stream, quiche::h3::Event::Headers { .. })) => {
                    http3
                        .send_response(
                            &mut connection,
                            stream,
                            &[
                                quiche::h3::Header::new(b":status", b"200"),
                                quiche::h3::Header::new(b"capsule-protocol", b"?1"),
                            ],
                            false,
                        )
                        .unwrap();
                }
                Ok(_) => {}
                Err(quiche::h3::Error::Done) => break,
                Err(error) => panic!("peer h3: {error:?}"),
            }
        }
        loop {
            match connection.send(&mut wire) {
                Ok((length, info)) => {
                    socket.send_to(&wire[..length], info.to).await.unwrap();
                }
                Err(quiche::Error::Done) => break,
                Err(error) => panic!("peer send: {error:?}"),
            }
        }
        let deadline = Instant::now() + connection.timeout().unwrap_or(Duration::from_secs(60));
        tokio::select! {
            _ = cancel.cancelled() => return socket,
            Some(packet) = commands.recv() => { connection.dgram_send_buf(encode_for_test(0, &packet)).unwrap(); }
            received = socket.recv_from(&mut wire) => {
                let (length, from) = received.unwrap();
                connection.recv(&mut wire[..length], quiche::RecvInfo { from, to: local }).unwrap();
            }
            _ = sleep_until(deadline) => connection.on_timeout(),
        }
    }
}

#[tokio::test]
async fn full_actor_wakes_immediately_on_returned_receive_capacity() {
    let client_socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let peer_socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let client_address = client_socket.local_addr().unwrap();
    let peer_address = peer_socket.local_addr().unwrap();
    let (mut client, mut server, _, _) =
        test_quic_pair_with_config(client_address, peer_address, |client, server| {
            client.set_max_ack_delay(0);
            server.set_max_ack_delay(0);
            server.enable_pacing(false);
        });
    settle(&mut client, &mut server);
    assert!(client.is_established() && server.is_established());
    let quality = NetworkQualityTelemetry::default();
    let active = PathSocket::spawn(
        PathId::new(0),
        client_address,
        peer_address,
        0,
        PathSocketRole::Active,
        client_socket,
        DirectEgressLease::for_generation(0),
        quality.clone(),
        UdpReceivePool::default(),
    )
    .unwrap();
    let paths = PathSocketSet::with_active(active).unwrap();
    let (outgoing_tx, outgoing_rx) = mpsc::channel(OUTGOING_BATCH_CHANNEL_CAPACITY);
    let (migration_tx, migration_rx) = mpsc::channel(H3_CONTROL_CAPACITY);
    let (incoming_tx, mut incoming_rx) = mpsc::channel(INCOMING_BATCH_CHANNEL_CAPACITY);
    let (control_tx, _control_rx) = watch::channel(PeerNetworkState::default());
    let (startup_tx, startup_rx) = oneshot::channel();
    let mut h3_config = quiche::h3::Config::new().unwrap();
    h3_config.enable_extended_connect(true);
    let datagram_queue = quality.register_queue(
        QueueKind::H3DatagramSend,
        DATAGRAM_SEND_QUEUE_CAPACITY,
        DATAGRAM_SEND_QUEUE_CAPACITY * 1472,
    );
    let wire_queue = quality.register_queue(
        QueueKind::H3WireSend,
        MAX_PENDING_WIRE_DATAGRAMS,
        MAX_PENDING_WIRE_DATAGRAMS * 1472,
    );
    let actor = AbortOnDropHandle::new(tokio::spawn(run_h3_actor(
        paths,
        client,
        h3_config,
        outgoing_rx,
        migration_rx,
        noop_socket_protector(),
        incoming_tx,
        control_tx,
        startup_tx,
        None,
        quality,
        datagram_queue,
        wire_queue,
        1280,
        1472,
        PmtuPathKey::new(client_address, peer_address),
    )));
    let (commands_tx, commands_rx) = mpsc::channel(1);
    let cancel_peer = CancellationToken::new();
    let peer = AbortOnDropHandle::new(tokio::spawn(loopback_peer(
        peer_socket,
        server,
        commands_rx,
        cancel_peer.clone(),
    )));
    assert!(
        timeout(Duration::from_secs(3), startup_rx)
            .await
            .unwrap()
            .unwrap()
            .is_ok()
    );
    for index in 0..=INCOMING_BATCH_CHANNEL_CAPACITY {
        let mut packet = ipv4_packet_with_length(64);
        packet[20] = index as u8;
        commands_tx.send(Bytes::from(packet)).await.unwrap();
        tokio::time::sleep(Duration::from_millis(15)).await;
    }
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(incoming_rx.len(), INCOMING_BATCH_CHANNEL_CAPACITY);
    cancel_peer.cancel();
    let _open_idle_peer_socket = peer.await.unwrap();
    tokio::time::pause();
    tokio::time::advance(Duration::from_secs(1)).await;
    for _ in 0..64 {
        tokio::task::yield_now().await;
    }
    let mut drained = 0;
    while let Ok(batch) = incoming_rx.try_recv() {
        drained += batch.len();
    }
    assert_eq!(drained, INCOMING_BATCH_CHANNEL_CAPACITY);
    let start = Instant::now();
    for _ in 0..64 {
        tokio::task::yield_now().await;
    }
    let recovered = incoming_rx
        .try_recv()
        .expect("capacity wake must deliver without a timer or peer event");
    assert_eq!(start.elapsed(), Duration::ZERO);
    assert_eq!(recovered.len(), 1);
    tokio::time::resume();
    drop(outgoing_tx);
    drop(migration_tx);
    assert!(
        timeout(Duration::from_secs(2), actor)
            .await
            .unwrap()
            .unwrap()
            .is_ok()
    );
}

#[test]
fn goaway_rejection_and_deadline_are_bounded() {
    let now = Instant::now();
    let mut state = GoAwayState::default();
    assert!(state.receive(4, None, now).is_err());
    assert!(state.receive(0, Some(0), now).is_err());
    state.receive(12, Some(0), now).unwrap();
    let deadline = state.deadline.unwrap();
    state
        .receive(4, Some(0), now + Duration::from_secs(10))
        .unwrap();
    assert_eq!(state.deadline, Some(deadline));
    assert!(
        state
            .check_deadline(deadline - Duration::from_millis(1))
            .is_ok()
    );
    assert!(state.check_deadline(deadline).is_err());
    assert!(state.receive(0, Some(0), now).is_err());
}
