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
pub(super) static SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

unsafe extern "C" {
    fn usque_test_input_capacity_wakeup() -> i32;
    fn usque_test_peer_create(
        ca: *const c_char,
        cert: *const c_char,
        key: *const c_char,
        reject_auth: i32,
        pushed_mtu: u32,
        udp: i32,
        password_only: i32,
        tls_key: *const c_char,
    ) -> *mut c_void;
    fn usque_test_peer_destroy(peer: *mut c_void);
    fn usque_test_peer_receive(peer: *mut c_void, data: *const u8, length: usize) -> i32;
    fn usque_test_peer_pop(peer: *mut c_void, data: *mut u8, capacity: usize) -> i32;
    fn usque_test_peer_data_count(peer: *mut c_void) -> u32;
    fn usque_test_peer_handshakes(peer: *mut c_void) -> u32;
    fn usque_test_peer_data_keys(peer: *mut c_void) -> u32;
    fn usque_test_peer_error() -> *const c_char;
}

#[test]
fn native_input_capacity_wakes_after_a_full_batch_and_blocked_direction() {
    // SAFETY: This isolated helper owns all of its queue state and invokes no
    // protocol runtime or external I/O; the return value is a boolean result.
    assert_eq!(unsafe { usque_test_input_capacity_wakeup() }, 1);
}
struct Peer(NonNull<c_void>);
impl Peer {
    fn new(reject_auth: bool) -> Self {
        Self::with_mtu(reject_auth, None)
    }
    fn with_mtu(reject_auth: bool, mtu: Option<u16>) -> Self {
        Self::with_transport(reject_auth, mtu, false)
    }
    fn with_transport(reject_auth: bool, mtu: Option<u16>, udp: bool) -> Self {
        Self::with_options(reject_auth, mtu, udp, false)
    }
    fn with_options(reject_auth: bool, mtu: Option<u16>, udp: bool, password_only: bool) -> Self {
        Self::with_crypto(reject_auth, mtu, udp, password_only, false)
    }
    fn with_crypto(
        reject_auth: bool,
        mtu: Option<u16>,
        udp: bool,
        password_only: bool,
        tls_crypt: bool,
    ) -> Self {
        let tls_key = CString::new(if tls_crypt {
            include_str!("../tests/fixtures/tls-crypt.key")
        } else {
            ""
        })
        .unwrap();
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
                i32::from(udp),
                i32::from(password_only),
                tls_key.as_ptr(),
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
    for (pushed_mtu, expected, encrypted_key) in [
        (None, 1400, false),
        (Some(1300), 1300, false),
        (None, 1400, true),
    ] {
        let peer = Peer::with_mtu(false, pushed_mtu);
        let mut config = format!("{}tun-mtu 1400\n", profile(CA));
        if encrypted_key {
            config = config.replace(
                CLIENT_KEY,
                include_str!("../tests/fixtures/client-encrypted.key"),
            );
        }
        let mut session = Session::start_with_credentials(
            &config,
            "192.0.2.1:1194".parse().unwrap(),
            "",
            "",
            if encrypted_key {
                "fixture-key-password"
            } else {
                ""
            },
        )
        .unwrap();
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
    memory_transport_roundtrip(false).await;
}
#[tokio::test]
async fn udp_transport_memory_tun_preserves_data_across_renegotiation_and_stops() {
    memory_transport_roundtrip(true).await;
}
async fn memory_transport_roundtrip(udp: bool) {
    let _serial = SERIAL.lock().await;
    let peer = Peer::with_transport(false, None, udp);
    let profile = if udp {
        profile(CA).replace("proto tcp", "proto udp")
    } else {
        profile(CA)
    };
    let mut session = Session::start(&profile, "192.0.2.1:1194".parse().unwrap()).unwrap();
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
                            max_data_frame =
                                max_data_frame.max(packet.len() + if udp { 0 } else { 2 });
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
    assert_eq!(max_data_frame, if udp { 1557 } else { 1559 });
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

#[tokio::test]
async fn password_only_and_split_outputs_preserve_mss_zero_and_reject_wrong_credentials() {
    let _serial = SERIAL.lock().await;
    for (udp, password, tls_crypt) in [
        (false, "fixture-password", false),
        (true, "fixture-password", false),
        (true, "fixture-password", true),
        (true, "wrong", true),
    ] {
        let peer = Peer::with_crypto(false, None, udp, true, tls_crypt);
        let base = profile(CA);
        let mut config = format!(
            "{}auth-user-pass\nmssfix 0\ntun-mtu 1500\n",
            base.split("<cert>").next().unwrap()
        );
        if tls_crypt {
            config.push_str(&format!(
                "<tls-crypt>\n{}</tls-crypt>\n",
                include_str!("../tests/fixtures/tls-crypt.key")
            ));
        }
        if udp {
            config = config.replace("proto tcp", "proto udp");
        }
        let mut session = Session::start_with_options(
            &config,
            "192.0.2.1:1194".parse().unwrap(),
            "fixture-user",
            password,
            "",
            true,
        )
        .unwrap();
        let (mut transport, mut packets) = session.split_packet_outputs().unwrap();
        let mut payload = vec![0; 44];
        payload[0] = 0x45;
        payload[2..4].copy_from_slice(&44u16.to_be_bytes());
        payload[8] = 64;
        payload[9] = 6;
        payload[12..16].copy_from_slice(&[10, 8, 0, 2]);
        payload[16..20].copy_from_slice(&[10, 8, 0, 1]);
        payload[32] = 0x60;
        payload[33] = 2;
        payload[40..44].copy_from_slice(&[2, 4, 0x05, 0xb4]); // TCP SYN MSS 1460
        let mut generation = 0;
        let mut buffer = vec![0; MAX_PACKET];
        timeout(Duration::from_secs(8), async {
            loop {
                while let length @ 1.. = peer.pop(&mut buffer) {
                    session
                        .input()
                        .receive_transport(generation, &buffer[..length])
                        .await
                        .unwrap();
                }
                let event = tokio::select! {
                    event = session.next_event() => event,
                    event = transport.next_event() => event,
                    event = packets.next_event() => event,
                    _ = tokio::time::sleep(Duration::from_millis(2)) => continue,
                }
                .unwrap();
                match event {
                    Event::Dial { generation: next } => {
                        generation = next;
                        session
                            .input()
                            .transport_connected(generation)
                            .await
                            .unwrap();
                    }
                    Event::TransportPacket { packet, .. } => {
                        assert!(peer.receive(&packet), "{}", peer_error())
                    }
                    Event::State {
                        name, error, fatal, ..
                    } => {
                        if name == "CONNECTED" {
                            assert_eq!(password, "fixture-password");
                            session.input().send_ip(generation, &payload).await.unwrap();
                        } else if error || fatal {
                            assert_eq!(password, "wrong");
                            assert_eq!(name, "AUTH_FAILED");
                            assert_eq!(peer.count(), 0);
                            break;
                        }
                    }
                    Event::IpPacket { packet, .. } => {
                        assert_eq!(&packet[..], &payload);
                        break;
                    }
                    Event::Stopped => panic!("stopped before expected result"),
                    _ => {}
                }
            }
        })
        .await
        .unwrap();
        let _ = session.shutdown().await;
        for mut output in [transport, packets] {
            timeout(Duration::from_secs(1), async {
                loop {
                    match output.next_event().await {
                        Ok(Event::Stopped) | Err(Error::Closed) => break,
                        Ok(_) => {}
                        Err(error) => panic!("unexpected decode error {error:?}"),
                    }
                }
            })
            .await
            .expect("all split readers finish after queued packets are drained");
        }
    }
}

#[tokio::test]
async fn split_packet_queues_resume_after_bidirectional_saturation() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let _serial = SERIAL.lock().await;
    const COUNT: usize = 1024;
    for udp in [false, true] {
        let peer = Peer::with_transport(false, None, udp);
        let config = if udp {
            profile(CA).replace("proto tcp", "proto udp")
        } else {
            profile(CA)
        };
        let mut session = Session::start(&config, "192.0.2.1:1194".parse().unwrap()).unwrap();
        let mut generation = 0;
        let mut buffer = vec![0; MAX_PACKET];
        timeout(Duration::from_secs(4), async {
            loop {
                while let length @ 1.. = peer.pop(&mut buffer) {
                    session
                        .input()
                        .receive_transport(generation, &buffer[..length])
                        .await
                        .unwrap();
                }
                match timeout(Duration::from_millis(2), session.next_event()).await {
                    Ok(Ok(Event::Dial { generation: next })) => {
                        generation = next;
                        session
                            .input()
                            .transport_connected(generation)
                            .await
                            .unwrap();
                    }
                    Ok(Ok(Event::TransportPacket { packet, .. })) => assert!(peer.receive(&packet)),
                    Ok(Ok(Event::State {
                        name, error, fatal, ..
                    })) => {
                        assert!(!error && !fatal, "{name}");
                        if name == "CONNECTED" {
                            break;
                        }
                    }
                    _ => {}
                }
            }
        })
        .await
        .unwrap();
        let (mut transport, mut packets) = session.split_packet_outputs().unwrap();
        let input = session.input();
        let sent = AtomicUsize::new(0);
        let produce = async {
            for sequence in 0..COUNT {
                let mut packet = udp_packet(1);
                packet[28..32].copy_from_slice(&(sequence as u32).to_be_bytes());
                input.send_ip(generation, &packet).await.unwrap();
                sent.fetch_add(1, Ordering::SeqCst);
            }
        };
        let relay = async {
            loop {
                while let length @ 1.. = peer.pop(&mut buffer) {
                    input
                        .receive_transport(generation, &buffer[..length])
                        .await
                        .unwrap();
                }
                if peer.count() as usize == COUNT {
                    break;
                }
                if let Event::TransportPacket { packet, .. } = transport.next_event().await.unwrap()
                {
                    assert!(peer.receive(&packet), "{}", peer_error());
                }
            }
        };
        let consume = async {
            tokio::time::sleep(Duration::from_millis(100)).await;
            assert!(
                sent.load(Ordering::SeqCst) < COUNT,
                "paused output must backpressure input"
            );
            let mut seen = std::collections::HashSet::new();
            while seen.len() < COUNT {
                if let Event::IpPacket { packet, .. } = packets.next_event().await.unwrap() {
                    let sequence = u32::from_be_bytes(packet[28..32].try_into().unwrap());
                    assert!((sequence as usize) < COUNT && seen.insert(sequence));
                }
            }
        };
        timeout(Duration::from_secs(8), async {
            tokio::join!(produce, relay, consume);
        })
        .await
        .unwrap();
        assert_eq!(peer.count() as usize, COUNT);
        session.shutdown().await.unwrap();
    }
}
