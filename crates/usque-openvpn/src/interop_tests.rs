//! Controlled protocol peer for the shipping C ABI. All I/O is memory-only.
use super::*;
use std::ffi::CStr;
use std::time::Duration;
use tokio::time::{Instant, timeout};

const CA: &str = include_str!("../tests/fixtures/ca.crt");
const CERT: &str = include_str!("../tests/fixtures/server.crt");
const KEY: &str = include_str!("../tests/fixtures/server.key");
const CLIENT_CERT: &str = include_str!("../tests/fixtures/client.crt");
const CLIENT_KEY: &str = include_str!("../tests/fixtures/client.key");
static SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

unsafe extern "C" {
    fn usque_test_peer_create(
        ca: *const c_char,
        cert: *const c_char,
        key: *const c_char,
        reject_auth: i32,
        pushed_mtu: u32,
    ) -> *mut c_void;
    fn usque_test_peer_destroy(peer: *mut c_void);
    fn usque_test_peer_receive(peer: *mut c_void, data: *const u8, length: usize) -> i32;
    fn usque_test_peer_pop(peer: *mut c_void, data: *mut u8, capacity: usize) -> i32;
    fn usque_test_peer_data_count(peer: *mut c_void) -> u32;
    fn usque_test_peer_handshakes(peer: *mut c_void) -> u32;
    fn usque_test_peer_data_keys(peer: *mut c_void) -> u32;
    fn usque_test_peer_error() -> *const c_char;
}
struct Peer(NonNull<c_void>);
impl Peer {
    fn new(reject_auth: bool) -> Self {
        Self::with_mtu(reject_auth, None)
    }
    fn with_mtu(reject_auth: bool, mtu: Option<u16>) -> Self {
        let (ca, cert, key) = (
            CString::new(CA).unwrap(),
            CString::new(CERT).unwrap(),
            CString::new(KEY).unwrap(),
        );
        // SAFETY: Valid NUL-terminated fixture strings are copied during create.
        let raw = unsafe {
            usque_test_peer_create(
                ca.as_ptr(),
                cert.as_ptr(),
                key.as_ptr(),
                i32::from(reject_auth),
                u32::from(mtu.unwrap_or_default()),
            )
        };
        Self(NonNull::new(raw).unwrap_or_else(|| panic!("{}", peer_error())))
    }
    fn receive(&self, data: &[u8]) -> bool {
        // SAFETY: This test owns the peer and passes a readable slice for this call.
        let result = unsafe { usque_test_peer_receive(self.0.as_ptr(), data.as_ptr(), data.len()) };
        result == 0
    }
    fn pop(&self, bytes: &mut [u8]) -> usize {
        // SAFETY: The exclusive writable slice has the capacity passed to the peer.
        let result =
            unsafe { usque_test_peer_pop(self.0.as_ptr(), bytes.as_mut_ptr(), bytes.len()) };
        assert!(result >= 0, "{}", peer_error());
        result as usize
    }
    fn count(&self) -> u32 {
        // SAFETY: The peer is live and used only on this test thread.
        unsafe { usque_test_peer_data_count(self.0.as_ptr()) }
    }
}
impl Drop for Peer {
    fn drop(&mut self) {
        // SAFETY: Unique ownership; all calls have returned and destroy runs once.
        unsafe { usque_test_peer_destroy(self.0.as_ptr()) };
    }
}
fn peer_error() -> String {
    // SAFETY: The peer returns a NUL-terminated thread-local string that remains
    // live until the next peer call. Copy it immediately on this same thread.
    unsafe { CStr::from_ptr(usque_test_peer_error()) }
        .to_string_lossy()
        .into_owned()
}
fn profile(ca: &str) -> String {
    format!(
        "client\ndev tun\nproto tcp\nremote 192.0.2.1 1194\ncipher AES-128-CBC\ndata-ciphers AES-128-CBC\nauth SHA1\nreneg-sec 10\ntls-version-min 1.2\nremote-cert-tls server\n<ca>\n{ca}</ca>\n<cert>\n{CLIENT_CERT}</cert>\n<key>\n{CLIENT_KEY}</key>\n"
    )
}
fn udp_packet(sequence: u8) -> Vec<u8> {
    // IPv4 + UDP + a small DNS-format question. Protocol tests require byte
    // preservation; the host never injects this packet into a network stack.
    // Include full-MTU datagrams so the CBC padding and TCP framing boundary
    // is exercised before and after renegotiation, not just tiny packets.
    let length = if sequence.is_multiple_of(2) { 64 } else { 1500 };
    let mut packet = vec![0; length];
    packet[0] = 0x45;
    packet[2..4].copy_from_slice(&(length as u16).to_be_bytes());
    packet[8] = 64;
    packet[9] = 17;
    packet[12..16].copy_from_slice(&[10, 8, 0, 2]);
    packet[16..20].copy_from_slice(&[10, 8, 0, 1]);
    packet[20..28].copy_from_slice(&[0xcb, 0x01, 0, 53, 0, 44, 0, 0]);
    packet[24..26].copy_from_slice(&((length - 20) as u16).to_be_bytes());
    packet[28] = sequence;
    packet[30] = 1;
    packet[33] = 1;
    packet[40..46].copy_from_slice(&[1, b'a', 0, 0, 1, 0]);
    packet[46] = 1;
    packet
}

#[tokio::test]
async fn initial_tcp_reset_matches_softether_protocol_detection() {
    let _serial = SERIAL.lock().await;
    let mut session = Session::start(&profile(CA), "192.0.2.1:1194".parse().unwrap()).unwrap();
    let packet = timeout(Duration::from_secs(3), async {
        loop {
            match session.next_event().await.unwrap() {
                Event::Dial { generation } => {
                    session
                        .input()
                        .transport_connected(generation)
                        .await
                        .unwrap();
                }
                Event::TransportPacket { packet, .. } => break packet,
                Event::State {
                    name, error, fatal, ..
                } => assert!(!error && !fatal, "{name}"),
                _ => {}
            }
        }
    })
    .await
    .unwrap();
    session.shutdown().await.unwrap();
    // SoftEther's TCP multiplexer recognizes the two-byte length 0x000e.
    assert_eq!(packet.len(), 14);
    assert_eq!(packet[0], 7 << 3); // P_CONTROL_HARD_RESET_CLIENT_V2, key zero
}

#[tokio::test]
async fn memory_tun_reports_effective_local_and_pushed_mtu() {
    let _serial = SERIAL.lock().await;
    for (pushed_mtu, expected) in [(None, 1400), (Some(1300), 1300)] {
        let peer = Peer::with_mtu(false, pushed_mtu);
        let config = format!("{}tun-mtu 1400\n", profile(CA));
        let mut session = Session::start(&config, "192.0.2.1:1194".parse().unwrap()).unwrap();
        let mut generation = 0;
        let mut network_mtu = None;
        let mut connected = false;
        let mut buffer = vec![0; MAX_PACKET];
        timeout(Duration::from_secs(3), async {
            while !connected || network_mtu.is_none() {
                while let length @ 1.. = peer.pop(&mut buffer) {
                    session
                        .input()
                        .receive_transport(generation, &buffer[..length])
                        .await
                        .unwrap();
                }
                if let Ok(event) = timeout(Duration::from_millis(5), session.next_event()).await {
                    match event.unwrap() {
                        Event::Dial { generation: next } => {
                            generation = next;
                            session
                                .input()
                                .transport_connected(generation)
                                .await
                                .unwrap();
                        }
                        Event::TransportPacket { packet, .. } => {
                            assert!(peer.receive(&packet), "{}", peer_error());
                        }
                        Event::Network { config, .. } => network_mtu = Some(config.mtu),
                        Event::State {
                            name, error, fatal, ..
                        } => {
                            assert!(!error && !fatal, "native event {name}");
                            connected |= name == "CONNECTED";
                        }
                        Event::Stopped => panic!("client stopped before network setup"),
                        Event::IpPacket { .. } => panic!("no IP packets were sent"),
                    }
                }
            }
        })
        .await
        .unwrap();
        session.shutdown().await.unwrap();
        assert_eq!(network_mtu, Some(expected), "pushed MTU: {pushed_mtu:?}");
    }
}

#[tokio::test]
async fn tls12_cbc_sha1_memory_tun_preserves_udp_across_renegotiation_and_stops() {
    let _serial = SERIAL.lock().await;
    let peer = Peer::new(false);
    let mut session = Session::start(&profile(CA), "192.0.2.1:1194".parse().unwrap()).unwrap();
    let mut generation = 0;
    let mut connected_at = None;
    let mut send_at = Instant::now();
    let mut sent = 0_u8;
    let mut received = 0_u8;
    let mut max_data_frame = 0;
    let mut network_seen = false;
    let mut buffer = vec![0; MAX_PACKET];
    timeout(Duration::from_secs(25), async {
        loop {
            while let length @ 1.. = peer.pop(&mut buffer) {
                session
                    .input()
                    .receive_transport(generation, &buffer[..length])
                    .await
                    .unwrap();
            }
            if connected_at.is_some() && Instant::now() >= send_at && sent < 72 {
                session
                    .input()
                    .send_ip(generation, &udp_packet(sent))
                    .await
                    .unwrap();
                sent += 1;
                send_at = Instant::now() + Duration::from_millis(250);
            }
            if let Ok(event) = timeout(Duration::from_millis(5), session.next_event()).await {
                match event.unwrap() {
                    Event::Dial { generation: next } => {
                        assert_eq!(generation, 0, "renegotiation must keep its TCP transport");
                        generation = next;
                        session
                            .input()
                            .transport_connected(generation)
                            .await
                            .unwrap();
                    }
                    Event::TransportPacket { packet, .. } => {
                        if packet[0] >> 3 == 6 {
                            // P_DATA_V1
                            max_data_frame = max_data_frame.max(packet.len() + 2);
                        }
                        assert!(peer.receive(&packet), "{}", peer_error())
                    }
                    Event::Network { config, .. } => {
                        assert_eq!(
                            config.mtu, 1500,
                            "an omitted PUSH_REPLY MTU uses the OpenVPN default"
                        );
                        assert_eq!(config.ipv4, Some("10.8.0.2".parse().unwrap()));
                        assert_eq!(config.ipv6, None);
                        assert_eq!(
                            config.dns_servers,
                            vec!["10.8.0.1".parse::<IpAddr>().unwrap()]
                        );
                        network_seen = true;
                    }
                    Event::State {
                        name, error, fatal, ..
                    } => {
                        assert!(!error && !fatal, "native event {name}");
                        if name == "CONNECTED" {
                            connected_at.get_or_insert(Instant::now());
                        }
                    }
                    Event::IpPacket { packet, .. } => {
                        assert_eq!(packet.as_ref(), udp_packet(received));
                        received += 1;
                    }
                    Event::Stopped => panic!("client stopped before cancellation"),
                }
            }
            if received == 72 {
                break;
            }
        }
    })
    .await
    .unwrap_or_else(|_| {
        panic!(
            "TLS/data/renegotiation timed out: sent={sent}, received={received}, peer={}",
            peer.count()
        )
    });
    assert!(network_seen);
    // 1500 IP + 4 packet ID + 16 CBC padding + 16 IV + 20 SHA1 HMAC
    // + 1 opcode + 2 TCP length bytes. Compression is disabled.
    assert_eq!(max_data_frame, 1559);
    assert_eq!(peer.count(), 72);
    // SAFETY: The live peer is uniquely owned on this test thread.
    let handshakes = unsafe { usque_test_peer_handshakes(peer.0.as_ptr()) };
    assert!(handshakes >= 2, "completed handshakes: {handshakes}");
    // SAFETY: The live peer is uniquely owned on this test thread.
    let data_keys = unsafe { usque_test_peer_data_keys(peer.0.as_ptr()) };
    assert!(data_keys.count_ones() >= 2, "data key mask: {data_keys}");
    assert!(connected_at.unwrap().elapsed() >= Duration::from_secs(7));
    timeout(Duration::from_secs(2), session.shutdown())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        session.input().send_ip(generation, &udp_packet(0)).await,
        Err(Error::Closed)
    );
}

#[tokio::test]
async fn authentication_and_ca_failures_never_publish_connected_or_ip_data() {
    let _serial = SERIAL.lock().await;
    for case in ["auth", "ca", "key_usage", "client_role"] {
        let reject_auth = case == "auth";
        let peer = Peer::new(reject_auth);
        let ca = if case == "ca" { CLIENT_CERT } else { CA };
        let mut config = profile(ca);
        if case == "key_usage" {
            config.push_str("remote-cert-ku 04\n"); // certificate signing is forbidden on the leaf
        }
        if case == "client_role" {
            config = config.replace("remote-cert-tls server", "remote-cert-tls client");
        }
        let mut session = Session::start(&config, "192.0.2.1:1194".parse().unwrap()).unwrap();
        let mut generation = 0;
        let mut failed = false;
        let mut peer_live = true;
        let mut buffer = vec![0; MAX_PACKET];
        timeout(Duration::from_secs(8), async {
            while !failed {
                while peer_live {
                    let length = peer.pop(&mut buffer);
                    if length == 0 {
                        break;
                    }
                    if session
                        .input()
                        .receive_transport(generation, &buffer[..length])
                        .await
                        .is_err()
                    {
                        peer_live = false;
                    }
                }
                if let Ok(event) = timeout(Duration::from_millis(5), session.next_event()).await {
                    match event.unwrap() {
                        Event::Dial { generation: next } => {
                            generation = next;
                            session
                                .input()
                                .transport_connected(generation)
                                .await
                                .unwrap();
                        }
                        Event::TransportPacket { packet, .. } => {
                            if peer_live {
                                peer_live = peer.receive(&packet);
                            }
                        }
                        Event::State {
                            name, error, fatal, ..
                        } => {
                            assert_ne!(name, "CONNECTED", "{case}");
                            if error || fatal {
                                assert_eq!(
                                    name,
                                    if reject_auth {
                                        "AUTH_FAILED"
                                    } else {
                                        "CERT_VERIFY_FAIL"
                                    }
                                );
                                failed = true;
                            }
                        }
                        Event::IpPacket { .. } => panic!("failed authentication exposed data"),
                        Event::Stopped => {
                            panic!("stopped without the terminal authentication event")
                        }
                        _ => {}
                    }
                }
            }
        })
        .await
        .unwrap_or_else(|_| {
            panic!("certificate/authentication rejected: {case}, peer_live={peer_live}")
        });
        assert_eq!(peer.count(), 0);
        let _ = session.shutdown().await;
    }
}
