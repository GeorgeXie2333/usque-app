use super::*;
use bytes::Bytes;

fn packet(port: u16) -> Vec<u8> {
    let mut packet = vec![0; 40];
    packet[0] = 0x45;
    packet[2..4].copy_from_slice(&40u16.to_be_bytes());
    packet[8] = 64;
    packet[9] = 6;
    packet[12..16].copy_from_slice(&[172, 16, 0, 2]);
    packet[16..20].copy_from_slice(&[192, 0, 2, 1]);
    packet[20..22].copy_from_slice(&port.to_be_bytes());
    packet[22..24].copy_from_slice(&443u16.to_be_bytes());
    packet[32] = 0x50;
    packet
}

#[test]
fn flows_are_pinned_and_use_every_requested_connection() {
    for count in 2..=6 {
        let mut router = FlowRouter::new(count);
        let now = Instant::now();
        let mut used = std::collections::HashSet::new();
        for port in 49152..49408 {
            let mut first = packet(port);
            let lane = router.lane(&first, now).unwrap();
            first[25] = 7; // TCP sequence changes, not the flow.
            first[33] = 0x11;
            assert_eq!(router.lane(&first, now), Some(lane));
            used.insert(lane);
        }
        assert_eq!(used.len(), count);
    }
}

#[test]
fn fragments_follow_the_first_fragment_and_expire_without_reassignment() {
    let now = Instant::now();
    let mut router = FlowRouter::new(6);
    let mut first = packet(50000);
    first[4..6].copy_from_slice(&123u16.to_be_bytes());
    first[6..8].copy_from_slice(&0x2000u16.to_be_bytes());
    let mut tail = first.clone();
    tail[6..8].copy_from_slice(&1u16.to_be_bytes());
    tail[20..].fill(0);
    assert_eq!(router.lane(&tail, now), None);
    let lane = router.lane(&first, now).unwrap();
    assert_eq!(router.lane(&tail, now), Some(lane));
    assert_eq!(router.lane(&tail, now + FRAGMENT_IDLE), None);
}

#[test]
fn ipv6_udp_fragments_keep_the_same_connection() {
    let mut first = vec![0; 56];
    first[0] = 0x60;
    first[4..6].copy_from_slice(&16u16.to_be_bytes());
    first[6] = 44;
    first[8..24].copy_from_slice(&"2001:db8::2".parse::<Ipv6Addr>().unwrap().octets());
    first[24..40].copy_from_slice(&"2001:db8::3".parse::<Ipv6Addr>().unwrap().octets());
    first[40] = 17;
    first[43] = 1;
    first[44..48].copy_from_slice(&1234u32.to_be_bytes());
    first[48..50].copy_from_slice(&50000u16.to_be_bytes());
    first[50..52].copy_from_slice(&53u16.to_be_bytes());
    let mut tail = first.clone();
    tail[42..44].copy_from_slice(&8u16.to_be_bytes());
    tail[48..].fill(0);
    let mut router = FlowRouter::new(6);
    let now = Instant::now();
    assert_eq!(router.lane(&tail, now), None);
    let lane = router.lane(&first, now).unwrap();
    assert_eq!(router.lane(&tail, now), Some(lane));
    tail[47] ^= 1;
    assert_eq!(router.lane(&tail, now), None);
}

#[test]
fn fragment_admission_has_a_hard_bound() {
    let now = Instant::now();
    let mut router = FlowRouter::new(2);
    for id in 0..FRAGMENTS {
        let mut value = packet(50000);
        value[4..6].copy_from_slice(&(id as u16).to_be_bytes());
        value[6..8].copy_from_slice(&0x2000u16.to_be_bytes());
        assert!(router.lane(&value, now).is_some());
    }
    let mut rejected = packet(50000);
    rejected[4..6].copy_from_slice(&(FRAGMENTS as u16).to_be_bytes());
    rejected[6..8].copy_from_slice(&0x2000u16.to_be_bytes());
    assert_eq!(router.lane(&rejected, now), None);
    assert_eq!(router.fragments.len(), FRAGMENTS);
    assert!(router.lane(&rejected, now + FRAGMENT_IDLE).is_some());
}

#[test]
fn control_and_packet_budget_are_conservative_across_all_members() {
    let addresses = (Ipv4Addr::new(172, 16, 0, 2), "2001:db8::2".parse().unwrap());
    let restricted = PeerNetworkState {
        assignments_advertised: true,
        assigned_addresses: vec![IpPrefix {
            request_id: 0,
            address: IpAddr::V4(addresses.0),
            prefix_len: 32,
        }],
        ..Default::default()
    };
    let path = allowed_families(
        [PeerNetworkState::default(), restricted],
        Transport::Http3,
        AddressFamily::Ipv4,
        addresses,
    );
    assert!(path.ipv4_available);
    assert!(!path.ipv6_available);
    let merged = control_for(path, addresses);
    let path = apply_peer_network_state(
        RuntimePath {
            ipv6_available: true,
            ..path
        },
        &merged,
        addresses.0,
        addresses.1,
    );
    assert!(!path.ipv6_available);
    let (first, a) = watch::channel(Some(H3PacketBudget {
        limit: 1400,
        confirmed: true,
    }));
    let (_, b) = watch::channel(Some(H3PacketBudget {
        limit: 1200,
        confirmed: false,
    }));
    assert_eq!(
        minimum_budget(&[a.clone(), b]),
        Some(H3PacketBudget {
            limit: 1200,
            confirmed: false
        })
    );
    first.send_replace(None);
    assert_eq!(
        minimum_budget(&[a]),
        Some(H3PacketBudget {
            limit: 128,
            confirmed: false
        })
    );
}

#[tokio::test]
async fn real_tunnel_halves_preserve_affinity_merge_receive_and_stop_every_member() {
    use crate::h3::regression_tests::scripted_tunnel;
    let (first, mut first_peer) = scripted_tunnel();
    let (second, mut second_peer) = scripted_tunnel();
    let members = vec![
        MasqueTunnel::Http3(Box::new(first)),
        MasqueTunnel::Http3(Box::new(second)),
    ];
    let pool = ParallelTunnel::new(
        members,
        AddressFamily::Ipv4,
        (Ipv4Addr::new(172, 16, 0, 2), "2001:db8::2".parse().unwrap()),
    );
    let (mut send, mut receive, driver, _) = pool.into_parts();
    let mut batch = PacketBatch::new();
    for port in 50000..50016 {
        batch.push_back(Bytes::from(packet(port))).unwrap();
    }
    let sending = send.start_owned_batch(batch);
    let sender = tokio::spawn(sending);
    let mut counts = [0; 2];
    for _ in 0..20 {
        tokio::task::yield_now().await;
        for (index, peer) in [&mut first_peer, &mut second_peer].into_iter().enumerate() {
            if let Some((batch, completion)) = peer.take_outgoing() {
                counts[index] += batch.len();
                let accepted_bytes = batch.bytes();
                completion
                    .send(PacketBatchResult {
                        accepted_bytes,
                        oversized: Vec::new(),
                    })
                    .unwrap();
            }
        }
        if counts.iter().sum::<usize>() == 16 {
            break;
        }
    }
    assert!(counts.iter().all(|n| *n > 0));
    assert_eq!(sender.await.unwrap().unwrap().accepted_bytes, 16 * 40);
    first_peer
        .incoming
        .send(PacketBatch::single(Bytes::from(packet(51000))))
        .await
        .unwrap();
    second_peer
        .incoming
        .send(PacketBatch::single(Bytes::from(packet(52000))))
        .await
        .unwrap();
    assert_eq!(receive.receive_batch().await.unwrap().len(), 1);
    assert_eq!(receive.receive_batch().await.unwrap().len(), 1);
    send.close();
    tokio::time::timeout(Duration::from_secs(2), driver.wait())
        .await
        .unwrap()
        .unwrap();
    assert!(first_peer.incoming.is_closed());
    assert!(second_peer.incoming.is_closed());
    assert!(
        send.start_owned_batch(PacketBatch::single(Bytes::from(packet(53000))))
            .await
            .is_err()
    );
}

#[tokio::test]
async fn one_failed_member_stops_all_six_connections_before_driver_completes() {
    let mut members = Vec::new();
    let mut peers = Vec::new();
    for _ in 0..6 {
        let (tunnel, peer) = crate::h3::regression_tests::scripted_tunnel();
        members.push(MasqueTunnel::Http3(Box::new(tunnel)));
        peers.push(peer);
    }
    let pool = MasqueTunnel::Parallel(Box::new(ParallelTunnel::new(
        members,
        AddressFamily::Ipv4,
        (Ipv4Addr::new(172, 16, 0, 2), "2001:db8::2".parse().unwrap()),
    )));
    assert!(pool.migration_handle().is_none());
    let (send, mut receive, driver, _) = pool.into_parts();
    drop(peers.pop());
    assert!(
        tokio::time::timeout(Duration::from_secs(2), driver.wait())
            .await
            .unwrap()
            .is_err()
    );
    for peer in peers {
        assert!(peer.incoming.is_closed());
        assert_eq!(peer.budget.receiver_count(), 0);
    }
    assert!(receive.receive_batch().await.is_err());
    assert!(send.start_owned_batch(PacketBatch::new()).await.is_err());
}
