//! Dependency and CONNECT-IP contract tests exchange real QUIC wire buffers
//! entirely in memory. No sockets, routes, adapters or external peers are used.
use super::*;
use crate::h3::tests::{
    ipv4_packet_with_length, test_quic_pair_with_config, test_quic_pair_with_server_config,
};
use crate::pmtu::{IPV4_MAX_UDP_PAYLOAD, IPV6_MAX_UDP_PAYLOAD};

const QUIC_MIN_PAYLOAD: usize = 1200;
const FLIGHT_BOUND: usize = 256;

struct Wire {
    bytes: Vec<u8>,
    from: SocketAddr,
    to: SocketAddr,
}

fn collect(
    connection: &mut H3QuicConnection,
    from: Option<SocketAddr>,
    to: Option<SocketAddr>,
) -> Vec<Wire> {
    let mut packets = Vec::new();
    for _ in 0..FLIGHT_BOUND {
        let mut bytes = vec![0; IPV4_MAX_UDP_PAYLOAD];
        match connection.send_on_path(&mut bytes, from, to) {
            Ok((length, info)) => {
                bytes.truncate(length);
                packets.push(Wire {
                    bytes,
                    from: info.from,
                    to: info.to,
                });
            }
            Err(quiche::Error::Done) => return packets,
            Err(error) => panic!("in-memory QUIC send failed: {error:?}"),
        }
    }
    panic!("in-memory flight exceeded its packet bound")
}

fn deliver(destination: &mut H3QuicConnection, mut packet: Wire) {
    destination
        .recv(
            &mut packet.bytes,
            quiche::RecvInfo {
                from: packet.from,
                to: packet.to,
            },
        )
        .unwrap();
}

struct Pair {
    client: H3QuicConnection,
    server: H3QuicConnection,
    client_addr: SocketAddr,
    server_addr: SocketAddr,
}

impl Pair {
    fn new(configure: impl FnOnce(&mut quiche::Config, &mut quiche::Config)) -> Self {
        let (client, server, client_addr, server_addr) = test_quic_pair_with_config(
            "127.0.0.1:12340".parse().unwrap(),
            "127.0.0.1:44330".parse().unwrap(),
            |client, server| {
                client.set_max_ack_delay(0);
                server.set_max_ack_delay(0);
                configure(client, server);
            },
        );
        Self::handshake(client, server, client_addr, server_addr)
    }

    fn with_server_handshake_override(
        config_discover: bool,
        discover: bool,
        max_probes: u8,
    ) -> Self {
        let (client, server, client_addr, server_addr) = test_quic_pair_with_server_config(
            "127.0.0.1:12340".parse().unwrap(),
            "127.0.0.1:44330".parse().unwrap(),
            |identity| {
                let mut tls = SslContextBuilder::new(SslMethod::tls()).unwrap();
                configure_client_identity_and_pin(&mut tls, identity).unwrap();
                tls.set_select_certificate_callback(move |mut hello| {
                    H3QuicConnection::set_discover_pmtu_in_handshake(
                        hello.ssl_mut(),
                        discover,
                        max_probes,
                    )
                    .unwrap();
                    Ok(())
                });
                let mut config =
                    quiche::Config::with_boring_ssl_ctx_builder(quiche::PROTOCOL_VERSION, tls)
                        .unwrap();
                config
                    .set_application_protos(quiche::h3::APPLICATION_PROTOCOL)
                    .unwrap();
                config.set_max_recv_udp_payload_size(IPV4_MAX_UDP_PAYLOAD);
                config.set_max_send_udp_payload_size(IPV4_MAX_UDP_PAYLOAD);
                config.set_initial_max_data(10_000_000);
                config.set_initial_max_stream_data_bidi_remote(1_000_000);
                config.set_initial_max_streams_bidi(16);
                config.set_active_connection_id_limit(ACTIVE_CONNECTION_ID_LIMIT);
                config.discover_pmtu(config_discover);
                config.set_pmtud_max_probes(7);
                config
            },
            |client, server| {
                client.discover_pmtu(false);
                client.set_max_ack_delay(0);
                server.set_max_ack_delay(0);
            },
        );
        Self::handshake(client, server, client_addr, server_addr)
    }

    fn handshake(
        client: H3QuicConnection,
        server: H3QuicConnection,
        client_addr: SocketAddr,
        server_addr: SocketAddr,
    ) -> Self {
        let mut pair = Self {
            client,
            server,
            client_addr,
            server_addr,
        };
        for _ in 0..32 {
            pair.round(usize::MAX);
            if pair.client.is_established() && pair.server.is_established() {
                return pair;
            }
        }
        panic!("in-memory QUIC handshake did not finish")
    }

    fn round(&mut self, client_limit: usize) -> usize {
        let packets = collect(&mut self.client, None, None);
        let mut count = packets.len();
        for packet in packets {
            if packet.bytes.len() <= client_limit {
                deliver(&mut self.server, packet);
            }
        }
        self.server.on_timeout();
        let packets = collect(&mut self.server, None, None);
        count += packets.len();
        for packet in packets {
            deliver(&mut self.client, packet);
        }
        self.client.on_timeout();
        count
    }

    fn settle(&mut self) {
        for _ in 0..64 {
            if self.round(usize::MAX) == 0 {
                return;
            }
        }
        panic!("in-memory QUIC pair did not settle")
    }

    fn exchange_ids(&mut self) {
        maintain_connection_ids(&mut self.client).unwrap();
        maintain_connection_ids(&mut self.server).unwrap();
        self.settle();
        assert_eq!(
            maintain_connection_ids(&mut self.client).unwrap(),
            CidAvailability::Ready
        );
    }

    fn validate_candidate(&mut self, candidate: SocketAddr, limit: usize) -> Vec<usize> {
        self.client.probe_path(candidate, self.server_addr).unwrap();
        let mut sizes = Vec::new();
        for _ in 0..32 {
            let packets = collect(&mut self.client, Some(candidate), Some(self.server_addr));
            for packet in packets {
                sizes.push(packet.bytes.len());
                if packet.bytes.len() <= limit {
                    deliver(&mut self.server, packet);
                }
            }
            self.server.on_timeout();
            for packet in collect(&mut self.server, None, None) {
                deliver(&mut self.client, packet);
            }
            if self.client.is_path_validated(candidate, self.server_addr) == Ok(true) {
                return sizes;
            }
        }
        panic!("candidate did not validate within bounded memory flights")
    }
}

fn hold_client_probe(pair: &mut Pair) -> Vec<Wire> {
    let packets = collect(&mut pair.client, None, None);
    assert!(
        packets
            .iter()
            .any(|packet| packet.bytes.len() > QUIC_MIN_PAYLOAD)
    );
    assert_eq!(pair.client.pmtu(), None);
    packets
}

struct BatchQueue {
    pending: Option<OutgoingBatch>,
    result: oneshot::Receiver<PacketBatchResult>,
    entries: VecDeque<QueueEntry>,
    queue: Arc<QueueMetrics>,
    quality: NetworkQualityTelemetry,
    pool: DatagramEncodePool,
}

impl BatchQueue {
    fn new(batch: PacketBatch) -> Self {
        Self::with_automatic(batch, true)
    }

    fn with_automatic(batch: PacketBatch, automatic_pmtu: bool) -> Self {
        let quality = NetworkQualityTelemetry::with_features(crate::NetworkFeatureFlags {
            automatic_pmtu,
            ..crate::PRODUCTION_NETWORK_FEATURES
        });
        let queue = quality.register_queue(QueueKind::H3DatagramSend, 1024, 64 * 1024);
        let pool = DatagramEncodePool::new(quality.clone());
        let (completion, result) = oneshot::channel();
        Self {
            pending: Some(OutgoingBatch {
                batch,
                result: PacketBatchResult::default(),
                completion,
            }),
            result,
            entries: VecDeque::new(),
            queue,
            quality,
            pool,
        }
    }

    fn step(&mut self, connection: &mut H3QuicConnection) {
        queue_pending_batch(
            connection,
            0,
            &mut self.pending,
            &mut self.entries,
            &self.queue,
            &self.quality,
            &self.pool,
            1500,
        )
        .unwrap();
    }
}

fn ipv6_packet_with_length(length: usize) -> Bytes {
    let mut packet = vec![0; length];
    packet[0] = 0x60;
    packet[4..6].copy_from_slice(&u16::try_from(length - 40).unwrap().to_be_bytes());
    packet[6] = 59;
    packet[7] = 64;
    packet[8..24].copy_from_slice(
        &"2001:db8::1"
            .parse::<std::net::Ipv6Addr>()
            .unwrap()
            .octets(),
    );
    packet[24..40].copy_from_slice(
        &"2001:db8::2"
            .parse::<std::net::Ipv6Addr>()
            .unwrap()
            .octets(),
    );
    validate_ip_packet(&packet).unwrap();
    Bytes::from(packet)
}

#[test]
fn reviewer_ipv6_minimum_waits_for_discovery_instead_of_terminal_ptb() {
    let mut pair = Pair::new(|_, server| server.discover_pmtu(false));
    let _unacknowledged_probe = hold_client_probe(&mut pair);
    let mut queue = BatchQueue::new(PacketBatch::single(ipv6_packet_with_length(1280)));
    queue.step(&mut pair.client);
    assert!(matches!(
        queue.result.try_recv(),
        Err(oneshot::error::TryRecvError::Empty)
    ));
    let pending = queue.pending.as_ref().unwrap();
    assert_eq!(pending.batch.len(), 1);
    assert!(pending.result.oversized.is_empty());
}

#[test]
fn reviewer_promotion_preserves_pending_candidate_path_response() {
    let mut pair = Pair::new(|_, _| {});
    pair.settle();
    pair.exchange_ids();
    let candidate: SocketAddr = "127.0.0.1:12341".parse().unwrap();
    pair.client.probe_path(candidate, pair.server_addr).unwrap();
    for packet in collect(&mut pair.client, Some(candidate), Some(pair.server_addr)) {
        deliver(&mut pair.server, packet);
    }
    for packet in collect(&mut pair.server, Some(pair.server_addr), Some(candidate)) {
        deliver(&mut pair.client, packet);
    }
    assert_eq!(
        pair.client.is_path_validated(candidate, pair.server_addr),
        Ok(true)
    );
    assert_eq!(
        pair.server.is_path_validated(pair.server_addr, candidate),
        Ok(false)
    );
    // Match the actor's pre-promotion barrier: flush the global ACK/control
    // frames on the old active path before switching application ownership.
    for packet in collect(
        &mut pair.client,
        Some(pair.client_addr),
        Some(pair.server_addr),
    ) {
        deliver(&mut pair.server, packet);
    }
    pair.client.migrate_source(candidate).unwrap();
    pair.client.revalidate_pmtu();
    for packet in collect(&mut pair.client, Some(candidate), Some(pair.server_addr)) {
        deliver(&mut pair.server, packet);
    }
    // A full-size PMTU probe must not pop and lose the pending PATH_RESPONSE.
    assert_eq!(
        pair.server.is_path_validated(pair.server_addr, candidate),
        Ok(true)
    );
}

fn assert_ipv6_waiter_resumes_after_probe_ack(mut pair: Pair) {
    let probe = hold_client_probe(&mut pair);
    let ipv6 = ipv6_packet_with_length(1280);
    let small = Bytes::from(ipv4_packet_with_length(64));
    let mut batch = PacketBatch::single(ipv6.clone());
    batch.push_back(small.clone()).unwrap();
    let mut queue = BatchQueue::new(batch);
    for _ in 0..4 {
        queue.step(&mut pair.client);
        assert!(matches!(
            queue.result.try_recv(),
            Err(oneshot::error::TryRecvError::Empty)
        ));
        let pending = queue.pending.as_ref().unwrap();
        assert_eq!(pending.batch.len(), 1);
        assert_eq!(pending.batch.bytes(), ipv6.len());
        assert_eq!(pending.result.accepted_bytes, small.len());
        assert!(pending.result.oversized.is_empty());
    }
    for packet in collect(&mut pair.client, None, None) {
        assert!(packet.bytes.len() <= QUIC_MIN_PAYLOAD);
        deliver(&mut pair.server, packet);
    }
    let received = pair.server.dgram_recv_buf().unwrap();
    assert_eq!(
        decode_http_datagram(0, received.as_ref()).unwrap().unwrap(),
        small
    );
    assert_eq!(pair.client.pmtu(), None);

    for packet in probe {
        deliver(&mut pair.server, packet);
    }
    pair.settle();
    assert_eq!(pair.client.pmtu(), Some(IPV4_MAX_UDP_PAYLOAD));
    queue.step(&mut pair.client);
    let result = queue.result.try_recv().unwrap();
    assert_eq!(result.accepted_bytes, ipv6.len() + small.len());
    assert!(result.oversized.is_empty());
    assert!(queue.pending.is_none());
    for packet in collect(&mut pair.client, None, None) {
        deliver(&mut pair.server, packet);
    }
    let received = pair.server.dgram_recv_buf().unwrap();
    assert_eq!(
        decode_http_datagram(0, received.as_ref()).unwrap().unwrap(),
        ipv6
    );
    reconcile_datagram_queue(&pair.client, &mut queue.entries, &queue.queue);
    assert!(queue.entries.is_empty());
}

#[test]
fn ipv6_waiter_preserves_small_packet_progress_until_initial_probe_ack() {
    assert_ipv6_waiter_resumes_after_probe_ack(Pair::new(|_, server| server.discover_pmtu(false)));
}

#[test]
fn ipv6_waiter_resumes_after_pmtu_revalidation() {
    let mut pair = Pair::new(|_, server| server.discover_pmtu(false));
    pair.settle();
    pair.client.revalidate_pmtu();
    assert_ipv6_waiter_resumes_after_probe_ack(pair);
}

#[test]
fn ipv6_waiter_resumes_after_path_promotion() {
    let mut pair = Pair::new(|_, server| server.discover_pmtu(false));
    pair.settle();
    pair.exchange_ids();
    let candidate: SocketAddr = "127.0.0.1:12341".parse().unwrap();
    pair.validate_candidate(candidate, IPV4_MAX_UDP_PAYLOAD);
    pair.client.migrate_source(candidate).unwrap();
    pair.client.revalidate_pmtu();
    assert_ipv6_waiter_resumes_after_probe_ack(pair);
}

#[test]
fn confirmed_low_pmtu_still_rejects_ipv6_minimum() {
    let mut pair = Pair::new(|client, server| {
        client.set_max_send_udp_payload_size(QUIC_MIN_PAYLOAD);
        server.discover_pmtu(false);
    });
    pair.settle();
    assert_eq!(pair.client.pmtu(), Some(QUIC_MIN_PAYLOAD));
    let mut queue = BatchQueue::new(PacketBatch::single(ipv6_packet_with_length(1280)));
    queue.step(&mut pair.client);
    let result = queue.result.try_recv().unwrap();
    assert_eq!(result.accepted_bytes, 0);
    assert_eq!(result.oversized.len(), 1);
    let (packet, maximum) = &result.oversized[0];
    assert!(matches!(
        crate::icmp::packet_too_big(packet, *maximum),
        Err(TransportError::Ipv6MinimumMtuUnavailable(_))
    ));
}

#[test]
fn fixed_low_payload_does_not_wait_for_disabled_pmtud() {
    let mut pair = Pair::new(|client, server| {
        client.discover_pmtu(false);
        client.set_max_send_udp_payload_size(QUIC_MIN_PAYLOAD);
        server.discover_pmtu(false);
    });
    pair.settle();
    let mut queue =
        BatchQueue::with_automatic(PacketBatch::single(ipv6_packet_with_length(1280)), false);
    queue.step(&mut pair.client);
    let result = queue.result.try_recv().unwrap();
    assert_eq!(result.oversized.len(), 1);
    assert!(queue.pending.is_none());
}

#[test]
fn cancelled_ipv6_waiter_releases_the_pending_batch_without_probe_ack() {
    let mut pair = Pair::new(|_, server| server.discover_pmtu(false));
    let _probe = hold_client_probe(&mut pair);
    let mut queue = BatchQueue::new(PacketBatch::single(ipv6_packet_with_length(1280)));
    queue.step(&mut pair.client);
    assert!(queue.pending.is_some());
    queue.result.close();
    queue.step(&mut pair.client);
    assert!(queue.pending.is_none());
    assert!(queue.entries.is_empty());
    assert_eq!(pair.client.dgram_send_queue_len(), 0);
}

#[test]
fn initial_writable_limit_rejects_large_datagram_without_blocking_small() {
    let mut pair = Pair::new(|_, server| server.discover_pmtu(false));
    let _unacknowledged_probe = hold_client_probe(&mut pair);
    let maximum = pair.client.dgram_max_writable_len().unwrap();
    assert!(maximum < QUIC_MIN_PAYLOAD);
    assert_eq!(
        pair.client.dgram_send(&[0x41; 1282]),
        Err(quiche::Error::BufferTooShort)
    );
    pair.client.dgram_send(&[0x42; 64]).unwrap();
    let packets = collect(&mut pair.client, None, None);
    assert!(!packets.is_empty());
    for packet in packets {
        assert!(packet.bytes.len() <= QUIC_MIN_PAYLOAD);
        deliver(&mut pair.server, packet);
    }
    let mut data = [0; 2048];
    assert_eq!(pair.server.dgram_recv(&mut data), Ok(64));
    assert_eq!(pair.client.dgram_send_queue_len(), 0);
}

#[test]
fn connect_ip_batch_returns_ptb_input_and_keeps_small_packet_progress() {
    let mut pair = Pair::new(|_, server| server.discover_pmtu(false));
    let _unacknowledged_probe = hold_client_probe(&mut pair);
    let quality = NetworkQualityTelemetry::default();
    let queue = quality.register_queue(QueueKind::H3DatagramSend, 1024, 64 * 1024);
    let pool = DatagramEncodePool::new(quality.clone());
    let oversized = Bytes::from(ipv4_packet_with_length(1280));
    let small = Bytes::from(ipv4_packet_with_length(64));
    let mut batch = PacketBatch::single(oversized.clone());
    batch.push_back(small.clone()).unwrap();
    let (completion, mut result) = oneshot::channel();
    let mut pending = Some(OutgoingBatch {
        batch,
        result: PacketBatchResult::default(),
        completion,
    });
    let mut entries = VecDeque::new();
    queue_pending_batch(
        &mut pair.client,
        0,
        &mut pending,
        &mut entries,
        &queue,
        &quality,
        &pool,
        1500,
    )
    .unwrap();
    let result = result.try_recv().unwrap();
    assert_eq!(result.accepted_bytes, small.len());
    assert_eq!(result.oversized.len(), 1);
    assert_eq!(result.oversized[0].0, oversized);
    assert!(result.oversized[0].1 < QUIC_MIN_PAYLOAD);
    for packet in collect(&mut pair.client, None, None) {
        assert!(packet.bytes.len() <= QUIC_MIN_PAYLOAD);
        deliver(&mut pair.server, packet);
    }
    let received = pair.server.dgram_recv_buf().unwrap();
    assert_eq!(
        decode_http_datagram(0, received.as_ref())
            .unwrap()
            .unwrap()
            .as_ref(),
        small.as_ref()
    );
    reconcile_datagram_queue(&pair.client, &mut entries, &queue);
    assert!(entries.is_empty());
}

#[test]
fn revalidation_discards_an_old_oversized_head_and_sends_small_datagram() {
    let mut pair = Pair::new(|_, server| server.discover_pmtu(false));
    pair.settle();
    assert_eq!(pair.client.pmtu(), Some(IPV4_MAX_UDP_PAYLOAD));
    pair.client.dgram_send(&[0x41; 1282]).unwrap();
    pair.client.revalidate_pmtu();
    pair.client.dgram_send(&[0x42; 64]).unwrap();
    for packet in collect(&mut pair.client, None, None) {
        if packet.bytes.len() <= QUIC_MIN_PAYLOAD {
            deliver(&mut pair.server, packet);
        }
    }
    let mut data = [0; 2048];
    assert_eq!(pair.server.dgram_recv(&mut data), Ok(64));
    assert_eq!(pair.client.dgram_send_queue_len(), 0);
    assert_eq!(pair.client.pmtu(), None);
}

#[test]
fn runtime_client_and_server_paths_start_with_independent_pmtud() {
    let mut pair = Pair::new(|_, _| {});
    pair.settle();
    pair.exchange_ids();
    assert_eq!(pair.client.pmtu(), Some(IPV4_MAX_UDP_PAYLOAD));
    let candidate: SocketAddr = "127.0.0.1:12341".parse().unwrap();
    pair.client.probe_path(candidate, pair.server_addr).unwrap();
    let client_path = pair
        .client
        .path_stats()
        .find(|path| path.local_addr == candidate)
        .unwrap();
    assert_eq!(client_path.pmtu, QUIC_MIN_PAYLOAD);
    for packet in collect(&mut pair.client, Some(candidate), Some(pair.server_addr)) {
        assert!(packet.bytes.len() <= QUIC_MIN_PAYLOAD);
        deliver(&mut pair.server, packet);
    }
    let server_path = pair
        .server
        .path_stats()
        .find(|path| path.peer_addr == candidate)
        .unwrap();
    assert_eq!(server_path.pmtu, QUIC_MIN_PAYLOAD);
    assert!(!server_path.active);
    assert_eq!(pair.server.pmtu(), Some(IPV4_MAX_UDP_PAYLOAD));
}

#[test]
fn candidate_response_does_not_consume_active_path_pmtu_probe() {
    let mut pair = Pair::new(|_, _| {});
    pair.settle();
    pair.exchange_ids();
    let candidate: SocketAddr = "127.0.0.1:12341".parse().unwrap();
    pair.client.probe_path(candidate, pair.server_addr).unwrap();
    let challenges = collect(&mut pair.client, Some(candidate), Some(pair.server_addr));
    pair.server.revalidate_pmtu();
    for packet in challenges {
        deliver(&mut pair.server, packet);
    }
    let response = collect(&mut pair.server, Some(pair.server_addr), Some(candidate));
    assert!(!response.is_empty());
    assert!(
        response
            .iter()
            .all(|packet| packet.bytes.len() <= QUIC_MIN_PAYLOAD)
    );
    let active = collect(
        &mut pair.server,
        Some(pair.server_addr),
        Some(pair.client_addr),
    );
    assert!(
        active
            .iter()
            .any(|packet| packet.bytes.len() == IPV4_MAX_UDP_PAYLOAD)
    );
    assert_eq!(pair.server.pmtu(), None);
}

#[test]
fn promoted_path_discovers_new_limit_without_restoring_unverified_ceiling() {
    const OLD_LIMIT: usize = 1350;
    const NEW_LIMIT: usize = 1360;
    let mut pair = Pair::new(|_, server| server.discover_pmtu(false));
    for _ in 0..2000 {
        pair.client.stream_send(0, &[0x63; 64], false).unwrap();
        pair.round(OLD_LIMIT);
        if pair.client.pmtu() == Some(OLD_LIMIT) {
            break;
        }
    }
    assert_eq!(pair.client.pmtu(), Some(OLD_LIMIT));
    pair.exchange_ids();
    let candidate: SocketAddr = "127.0.0.1:12341".parse().unwrap();
    let challenges = pair.validate_candidate(candidate, NEW_LIMIT);
    assert!(challenges.iter().all(|&length| length <= QUIC_MIN_PAYLOAD));
    pair.client.migrate_source(candidate).unwrap();
    pair.client.revalidate_pmtu();
    assert!(pair.client.dgram_max_writable_len().unwrap() < QUIC_MIN_PAYLOAD);
    assert_eq!(
        pair.client.dgram_send(&[0x64; 1400]),
        Err(quiche::Error::BufferTooShort)
    );
    for _ in 0..2000 {
        pair.client.stream_send(0, &[0x63; 64], false).unwrap();
        pair.round(NEW_LIMIT);
        if pair.client.pmtu() == Some(NEW_LIMIT) {
            break;
        }
    }
    assert_eq!(pair.client.pmtu(), Some(NEW_LIMIT));
    let maximum = pair.client.dgram_max_writable_len().unwrap();
    pair.client.dgram_send(&vec![0x64; maximum]).unwrap();
    for packet in collect(&mut pair.client, Some(candidate), Some(pair.server_addr)) {
        assert!(packet.bytes.len() <= NEW_LIMIT);
        deliver(&mut pair.server, packet);
    }
    let mut data = [0; 2048];
    assert_eq!(pair.server.dgram_recv(&mut data), Ok(maximum));
    assert_eq!(pair.client.send_quantum(), 10 * IPV4_MAX_UDP_PAYLOAD);
}

#[test]
fn disabled_pmtud_keeps_fixed_payload_across_migration() {
    let mut pair = Pair::new(|client, server| {
        for config in [client, server] {
            config.discover_pmtu(false);
            config.set_max_send_udp_payload_size(INITIAL_SAFE_UDP_PAYLOAD);
        }
    });
    pair.settle();
    pair.exchange_ids();
    assert_eq!(pair.client.pmtu(), None);
    let limit = pair.client.dgram_max_writable_len().unwrap();
    let candidate: SocketAddr = "127.0.0.1:12341".parse().unwrap();
    pair.validate_candidate(candidate, INITIAL_SAFE_UDP_PAYLOAD);
    pair.client.migrate_source(candidate).unwrap();
    pair.client.revalidate_pmtu();
    pair.settle();
    assert_eq!(pair.client.pmtu(), None);
    assert_eq!(
        pair.client.max_send_udp_payload_size(),
        INITIAL_SAFE_UDP_PAYLOAD
    );
    assert_eq!(pair.client.dgram_max_writable_len(), Some(limit));
}

#[test]
fn runtime_paths_respect_peer_udp_payload_ceiling() {
    const PEER_LIMIT: usize = 1300;
    let mut pair = Pair::new(|_, server| {
        server.discover_pmtu(false);
        server.set_max_recv_udp_payload_size(PEER_LIMIT);
    });
    pair.exchange_ids();
    let candidate: SocketAddr = "127.0.0.1:12341".parse().unwrap();
    pair.validate_candidate(candidate, PEER_LIMIT);
    pair.client.migrate_source(candidate).unwrap();
    pair.settle();
    assert_eq!(pair.client.pmtu(), Some(PEER_LIMIT));
    let maximum = pair.client.dgram_max_writable_len().unwrap();
    assert!(maximum < PEER_LIMIT);
    pair.client.dgram_send(&vec![0x44; maximum]).unwrap();
    for packet in collect(&mut pair.client, Some(candidate), Some(pair.server_addr)) {
        assert!(packet.bytes.len() <= PEER_LIMIT);
        deliver(&mut pair.server, packet);
    }
    assert_eq!(
        pair.server.dgram_recv_buf().unwrap().as_ref().len(),
        maximum
    );
}

#[test]
fn runtime_server_paths_inherit_effective_handshake_pmtud_setting() {
    for discover in [false, true] {
        let mut pair = Pair::with_server_handshake_override(!discover, discover, 2);
        pair.settle();
        pair.exchange_ids();
        assert_eq!(pair.server.pmtu(), discover.then_some(IPV4_MAX_UDP_PAYLOAD));
        let candidate: SocketAddr = "127.0.0.1:12341".parse().unwrap();
        pair.client.probe_path(candidate, pair.server_addr).unwrap();
        for packet in collect(&mut pair.client, Some(candidate), Some(pair.server_addr)) {
            deliver(&mut pair.server, packet);
        }
        let server_path = pair
            .server
            .path_stats()
            .find(|path| path.peer_addr == candidate)
            .unwrap();
        assert_eq!(
            server_path.pmtu,
            if discover {
                QUIC_MIN_PAYLOAD
            } else {
                IPV4_MAX_UDP_PAYLOAD
            }
        );
    }
}

#[test]
fn server_callback_probe_budget_survives_migration() {
    let mut pair = Pair::with_server_handshake_override(false, true, 2);
    pair.settle();
    pair.exchange_ids();
    let candidate: SocketAddr = "127.0.0.1:12341".parse().unwrap();
    pair.client.probe_path(candidate, pair.server_addr).unwrap();
    for packet in collect(&mut pair.client, Some(candidate), Some(pair.server_addr)) {
        deliver(&mut pair.server, packet);
    }
    for packet in collect(&mut pair.server, Some(pair.server_addr), Some(candidate)) {
        assert!(packet.bytes.len() <= QUIC_MIN_PAYLOAD);
        deliver(&mut pair.client, packet);
    }
    assert_eq!(
        pair.client.is_path_validated(candidate, pair.server_addr),
        Ok(true)
    );
    pair.client.migrate_source(candidate).unwrap();
    // A non-probing packet makes the peer select the new path. Collect every
    // server probe from this point; discard only probes, and ACK small traffic
    // to drive packet-threshold loss detection without wall-clock sleeps.
    pair.client.stream_send(0, &[0x33; 64], false).unwrap();
    let mut probes = Vec::new();
    for _ in 0..2000 {
        for packet in collect(&mut pair.client, Some(candidate), Some(pair.server_addr)) {
            deliver(&mut pair.server, packet);
        }
        pair.server.stream_send(1, &[0x55; 64], false).unwrap();
        for packet in collect(&mut pair.server, Some(pair.server_addr), Some(candidate)) {
            if packet.bytes.len() > QUIC_MIN_PAYLOAD {
                probes.push(packet.bytes.len());
            } else {
                deliver(&mut pair.client, packet);
            }
        }
        pair.client.on_timeout();
        pair.server.on_timeout();
        if probes.len() >= 3 {
            break;
        }
    }
    assert!(probes.len() >= 3);
    assert_eq!(
        &probes[..3],
        &[
            IPV4_MAX_UDP_PAYLOAD,
            IPV4_MAX_UDP_PAYLOAD,
            (IPV4_MAX_UDP_PAYLOAD + QUIC_MIN_PAYLOAD) / 2,
        ]
    );
}

#[test]
fn cubic_quantum_can_hold_family_buffer_before_and_after_discovery() {
    for ceiling in [IPV4_MAX_UDP_PAYLOAD, IPV6_MAX_UDP_PAYLOAD] {
        let mut pair = Pair::new(|client, server| {
            for config in [client, server] {
                config.set_max_recv_udp_payload_size(ceiling);
                config.set_max_send_udp_payload_size(ceiling);
            }
        });
        assert_eq!(pair.client.send_quantum(), 10 * ceiling);
        pair.settle();
        assert_eq!(pair.client.send_quantum(), 10 * ceiling);
        pair.client.revalidate_pmtu();
        assert_eq!(pair.client.send_quantum(), 10 * ceiling);
    }
}
