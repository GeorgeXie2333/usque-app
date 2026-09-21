use super::*;
use boringtun::x25519::{PublicKey, StaticSecret};
use tokio::time::timeout;
use zeroize::Zeroizing;

fn setup() -> (Session, Tunn) {
    setup_with_keepalive(None)
}
fn setup_with_keepalive(keepalive: Option<u16>) -> (Session, Tunn) {
    let private = StaticSecret::from([1u8; 32]);
    let peer = StaticSecret::from([2u8; 32]);
    let profile = WireGuardProfile {
        private_key: Zeroizing::new(private.to_bytes()),
        public_key: PublicKey::from(&peer).to_bytes(),
        preshared_key: None,
        endpoint: usque_core::chain_exit::Endpoint::parse("vpn.example", "51820", 0).unwrap(),
        addresses: vec!["10.8.0.2/32".parse().unwrap()],
        dns_servers: vec![],
        allowed_ips: vec!["10.8.0.0/24".parse().unwrap()],
        mtu: 1280,
        keepalive,
    };
    let public = PublicKey::from(&private);
    (
        Session::start(profile),
        Tunn::new(peer, public, None, None, 2, None),
    )
}

#[tokio::test]
async fn persistent_keepalive_keeps_idle_session_authenticated_without_business_packets() {
    let (mut session, mut peer) = setup_with_keepalive(Some(1));
    connect(&mut session, &mut peer).await;
    let input = session.input();
    timeout(Duration::from_secs(4), async {
        loop {
            if let Event::TransportPacket { packet, .. } = session.next_event().await.unwrap()
                && packet.len() == 32
            {
                assert!(peer_receive(&mut peer, &packet, &input).await.is_empty());
                break;
            }
        }
    })
    .await
    .unwrap();
    session.shutdown().await.unwrap();
}
async fn peer_receive(peer: &mut Tunn, bytes: &[u8], input: &Input) -> Vec<Vec<u8>> {
    let mut buffer = vec![0; MAX_PACKET + 256];
    let mut result = peer.decapsulate(None, bytes, &mut buffer);
    let mut plaintext = Vec::new();
    loop {
        match result {
            TunnResult::WriteToNetwork(bytes) => input.push(1, GENERATION, bytes).await.unwrap(),
            TunnResult::WriteToTunnelV4(bytes, _) | TunnResult::WriteToTunnelV6(bytes, _) => {
                plaintext.push(bytes.to_vec());
                break;
            }
            TunnResult::Done => break,
            TunnResult::Err(error) => panic!("memory peer rejected a packet: {error:?}"),
        }
        result = peer.decapsulate(None, &[], &mut buffer);
    }
    plaintext
}
async fn connect(session: &mut Session, peer: &mut Tunn) {
    let input = session.input();
    let mut assignment = false;
    timeout(Duration::from_secs(3), async {
        loop {
            match session.next_event().await.unwrap() {
                Event::Dial { generation } => input.push(3, generation, &[]).await.unwrap(),
                Event::TransportPacket { packet, .. } => {
                    peer_receive(peer, &packet, &input).await;
                }
                Event::Network { config, .. } => {
                    assert_eq!(config.ipv4, Some(Ipv4Addr::new(10, 8, 0, 2)));
                    assignment = true;
                }
                Event::State { name, .. } if name == "CONNECTED" => {
                    assert!(assignment);
                    break;
                }
                _ => {}
            }
        }
    })
    .await
    .unwrap();
}
fn packet(source: [u8; 4], destination: [u8; 4], length: usize) -> Vec<u8> {
    let mut bytes = vec![0; length];
    bytes[0] = 0x45;
    bytes[2..4].copy_from_slice(&(length as u16).to_be_bytes());
    bytes[8] = 64;
    bytes[9] = 17;
    bytes[12..16].copy_from_slice(&source);
    bytes[16..20].copy_from_slice(&destination);
    bytes[20..22].copy_from_slice(&1234u16.to_be_bytes());
    bytes[22..24].copy_from_slice(&5678u16.to_be_bytes());
    bytes[24..26].copy_from_slice(&((length - 20) as u16).to_be_bytes());
    bytes
}
#[tokio::test]
async fn authenticated_handshake_full_mtu_data_replay_and_allowed_ips_are_enforced() {
    let (mut session, mut peer) = setup();
    connect(&mut session, &mut peer).await;
    let input = session.input();
    let outbound = packet([10, 8, 0, 2], [10, 8, 0, 1], 1280);
    input.push(2, GENERATION, &outbound).await.unwrap();
    timeout(Duration::from_secs(2), async {
        loop {
            if let Event::TransportPacket { packet, .. } = session.next_event().await.unwrap() {
                let received = peer_receive(&mut peer, &packet, &input).await;
                if !received.is_empty() {
                    assert_eq!(received[0], outbound);
                    break;
                }
            }
        }
    })
    .await
    .unwrap();
    let incoming = packet([10, 8, 0, 1], [10, 8, 0, 2], 1280);
    let mut buffer = vec![0; MAX_PACKET + 256];
    let encrypted = match peer.encapsulate(&incoming, &mut buffer) {
        TunnResult::WriteToNetwork(bytes) => bytes.to_vec(),
        _ => panic!("missing data"),
    };
    input.push(1, GENERATION, &encrypted).await.unwrap();
    match timeout(Duration::from_secs(2), session.next_event())
        .await
        .unwrap()
        .unwrap()
    {
        Event::IpPacket { packet, .. } => assert_eq!(&packet[..], &incoming),
        other => panic!("unexpected event {other:?}"),
    }
    input.push(1, GENERATION, &encrypted).await.unwrap();
    assert!(
        timeout(Duration::from_millis(30), session.next_event())
            .await
            .is_err()
    );
    let rejected = packet([192, 0, 2, 1], [10, 8, 0, 2], 80);
    let TunnResult::WriteToNetwork(bytes) = peer.encapsulate(&rejected, &mut buffer) else {
        panic!()
    };
    input.push(1, GENERATION, bytes).await.unwrap();
    input
        .push(2, GENERATION, &packet([10, 8, 0, 2], [1, 1, 1, 1], 80))
        .await
        .unwrap();
    assert!(
        timeout(Duration::from_millis(30), session.next_event())
            .await
            .is_err()
    );
    assert!(input.push(2, GENERATION + 1, &outbound).await.is_err());
    session.shutdown().await.unwrap();
    assert_eq!(
        input.push(2, GENERATION, &outbound).await,
        Err(Error::Closed)
    );
}

#[tokio::test]
async fn forced_rekey_preserves_authenticated_connection_and_stop_cancels_backpressure() {
    let (mut session, mut peer) = setup();
    connect(&mut session, &mut peer).await;
    let input = session.input();
    let mut buffer = vec![0; MAX_PACKET + 256];
    let TunnResult::WriteToNetwork(initiation) =
        peer.format_handshake_initiation(&mut buffer, true)
    else {
        panic!()
    };
    input.push(1, GENERATION, initiation).await.unwrap();
    let event = timeout(Duration::from_secs(2), session.next_event())
        .await
        .unwrap()
        .unwrap();
    let Event::TransportPacket {
        packet: encrypted, ..
    } = event
    else {
        panic!()
    };
    peer_receive(&mut peer, &encrypted, &input).await;
    let payload = packet([10, 8, 0, 1], [10, 8, 0, 2], 80);
    let TunnResult::WriteToNetwork(bytes) = peer.encapsulate(&payload, &mut buffer) else {
        panic!()
    };
    input.push(1, GENERATION, bytes).await.unwrap();
    let event = timeout(Duration::from_secs(2), session.next_event())
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(event, Event::IpPacket { .. }));
    let producer = tokio::spawn(async move {
        loop {
            if input.push(3, GENERATION, &[]).await.is_err() {
                break;
            }
        }
    });
    tokio::task::yield_now().await;
    timeout(Duration::from_secs(1), session.shutdown())
        .await
        .unwrap()
        .unwrap();
    timeout(Duration::from_secs(1), producer)
        .await
        .unwrap()
        .unwrap();
}
