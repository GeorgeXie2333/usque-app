//! Synthetic, memory-only agent peers. None of these keys identify a real host.
use super::*;
use proptest::prelude::*;
use tokio::io::duplex;
use zeroize::Zeroizing;

fn credentials() -> TunnelCredentials {
    TunnelCredentials {
        certificate_chain: Zeroizing::new(CLIENT_CERT.to_owned()),
        private_key_pkcs8: Zeroizing::new(STANDARD.decode(CLIENT_KEY_DER).unwrap()),
        server_name: "agent.example.invalid".into(),
        expires_at: unix_now().unwrap() + 3600,
        label: None,
    }
}

fn packet(source: Ipv4Addr, sport: u16, destination: Ipv4Addr, dport: u16) -> Vec<u8> {
    let mut data = vec![0; 40];
    data[0] = 0x45;
    data[2..4].copy_from_slice(&40_u16.to_be_bytes());
    data[9] = 6;
    data[12..16].copy_from_slice(&source.octets());
    data[16..20].copy_from_slice(&destination.octets());
    data[20..22].copy_from_slice(&sport.to_be_bytes());
    data[22..24].copy_from_slice(&dport.to_be_bytes());
    data[32] = 0x50;
    data
}

#[test]
fn only_the_single_control_tuple_crosses_pre_authorization() {
    let tuple = ControlTuple { local_port: 34001 };
    let outbound = packet(CLIENT, 34001, SERVER, SERVER_PORT);
    let inbound = packet(SERVER, SERVER_PORT, CLIENT, 34001);
    assert!(tuple.outgoing(&outbound));
    assert!(tuple.incoming(&inbound));
    assert!(!tuple.incoming(&outbound));
    assert!(!tuple.outgoing(&inbound));
    for index in [0, 2, 6, 9, 12, 16, 20, 22, 32] {
        let mut malformed = inbound.clone();
        malformed[index] ^= if index == 32 { 0x10 } else { 1 };
        assert!(!tuple.incoming(&malformed), "field {index}");
    }
    assert!(!tuple.incoming(&packet(SERVER, SERVER_PORT, CLIENT, 34002)));
    assert!(ControlTuple::reserved_outgoing(&outbound));
    assert!(ControlTuple::reserved_incoming(&inbound));
}

#[test]
fn fragments_cannot_bypass_the_reserved_agent_port_in_either_direction() {
    let tuple = ControlTuple { local_port: 34001 };
    for fragments in [1_u16, 0x2000, 0x2001, 0x8000] {
        let mut outgoing = packet(CLIENT, 34001, SERVER, SERVER_PORT);
        let mut incoming = packet(SERVER, SERVER_PORT, CLIENT, 34001);
        outgoing[6..8].copy_from_slice(&fragments.to_be_bytes());
        incoming[6..8].copy_from_slice(&fragments.to_be_bytes());
        assert!(!tuple.outgoing(&outgoing));
        assert!(!tuple.incoming(&incoming));
        assert!(ControlTuple::reserved_outgoing(&outgoing));
        assert!(ControlTuple::reserved_incoming(&incoming));
        // UDP to the tunnel DNS and traffic to other addresses is separate.
        outgoing[9] = 17;
        incoming[9] = 17;
        assert!(!ControlTuple::reserved_outgoing(&outgoing));
        assert!(!ControlTuple::reserved_incoming(&incoming));
        outgoing[9] = 6;
        incoming[9] = 6;
        outgoing[16..20].copy_from_slice(&[192, 0, 2, 1]);
        incoming[12..16].copy_from_slice(&[192, 0, 2, 1]);
        assert!(!ControlTuple::reserved_outgoing(&outgoing));
        assert!(!ControlTuple::reserved_incoming(&incoming));
    }
    assert!(!ControlTuple::reserved_outgoing(&packet(
        CLIENT, 34001, SERVER, 443
    )));
    assert!(!ControlTuple::reserved_incoming(&packet(
        SERVER, 443, CLIENT, 34001
    )));
}

proptest! {
    #[test]
    fn malformed_control_packets_never_panic(data in proptest::collection::vec(any::<u8>(), 0..2000)) {
        let tuple = ControlTuple { local_port: 34001 };
        let _ = (tuple.incoming(&data), tuple.outgoing(&data), ControlTuple::reserved_incoming(&data), ControlTuple::reserved_outgoing(&data));
    }
    #[test]
    fn malformed_status_json_cannot_open_gate(data in proptest::collection::vec(any::<u8>(), 0..4096)) {
        if let Ok(value) = serde_json::from_slice::<Value>(&data)
            && status(&value).unwrap_or(false) {
                prop_assert_eq!(value["status"]["state"].as_str(), Some("connected"));
                prop_assert!(value["status"]["reason"].is_null());
        }
    }
}

#[tokio::test]
async fn malicious_lengths_and_truncation_are_bounded() {
    for length in [0, MAX_FRAME as u32 + 1, u32::MAX] {
        let (mut client, mut peer) = duplex(64);
        peer.write_all(&length.to_be_bytes()).await.unwrap();
        assert!(matches!(
            read_frame(&mut client).await,
            Err(GateFailure::Protocol)
        ));
    }
    let (mut client, mut peer) = duplex(64);
    peer.write_all(&100_u32.to_be_bytes()).await.unwrap();
    peer.write_all(b"incomplete").await.unwrap();
    drop(peer);
    assert!(matches!(
        read_frame(&mut client).await,
        Err(GateFailure::Transport)
    ));
}

#[tokio::test(start_paused = true)]
async fn an_incomplete_frame_cannot_extend_its_deadline() {
    let (mut client, mut peer) = duplex(64);
    peer.write_all(&100_u32.to_be_bytes()).await.unwrap();
    let deadline = Instant::now() + IO_BUDGET;
    assert!(timeout_at(deadline, read_frame(&mut client)).await.is_err());
}

#[test]
fn certificate_parser_and_client_key_binding_fail_closed() {
    assert_eq!(certificates(ROOT_CERTIFICATES).unwrap().len(), 2);
    assert!(certificates("garbage").is_err());
    assert!(certificates(&(CLIENT_CERT.to_owned() + "garbage")).is_err());
    assert!(certificates(&CLIENT_CERT.repeat(2)).is_err());
    assert!(certificates(&CLIENT_CERT.repeat(9)).is_err());
    assert!(certificates(&"x".repeat(usque_core::proton_vpn::MAX_CERTIFICATE_BYTES + 1)).is_err());
    let mut credentials = credentials();
    assert!(tls_config(&credentials, ROOT_CA).is_ok());
    credentials.private_key_pkcs8 = Zeroizing::new(STANDARD.decode(SERVER_KEY_DER).unwrap());
    assert!(tls_config(&credentials, ROOT_CA).is_err());
    credentials = self::credentials();
    credentials.expires_at = unix_now().unwrap();
    assert!(tls_config(&credentials, ROOT_CA).is_err());
}

#[test]
fn expired_credential_closes_gate_even_before_background_timer_runs() {
    let (publisher, status) = watch::channel(AgentState::Connected);
    let expires_at = Arc::new(AtomicI64::new(unix_now().unwrap() + 60));
    let gate = AgentGate {
        status,
        expires_at: expires_at.clone(),
    };
    assert!(gate.authorized());
    expires_at.store(unix_now().unwrap(), Ordering::Release);
    assert!(!gate.authorized());
    expires_at.store(unix_now().unwrap() + 60, Ordering::Release);
    publisher.send_replace(AgentState::Connecting);
    assert!(!gate.authorized());
    publisher.send_replace(AgentState::Failed(GateFailure::Certificate));
    assert!(!gate.authorized());
    publisher.send_replace(AgentState::Connected);
    drop(publisher);
    assert!(!gate.authorized());
}

fn server_config() -> Arc<rustls::ServerConfig> {
    let mut roots = rustls::RootCertStore::empty();
    roots.add(certificates(ROOT_CA).unwrap().remove(0)).unwrap();
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let verifier = rustls::server::WebPkiClientVerifier::builder_with_provider(
        Arc::new(roots),
        provider.clone(),
    )
    .build()
    .unwrap();
    Arc::new(
        rustls::ServerConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_client_cert_verifier(verifier)
            .with_single_cert(
                certificates(&(SERVER_CERT.to_owned() + ROOT_CA)).unwrap(),
                PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(
                    STANDARD.decode(SERVER_KEY_DER).unwrap(),
                )),
            )
            .unwrap(),
    )
}

#[tokio::test]
async fn memory_mutual_tls_verifies_both_names_and_identity() {
    for (name, trusted, wrong_identity, succeeds) in [
        ("agent.example.invalid", true, false, true),
        ("other.example.invalid", true, false, false),
        ("agent.example.invalid", false, false, false),
        ("agent.example.invalid", true, true, false),
    ] {
        let mut credentials = credentials();
        if wrong_identity {
            credentials.certificate_chain = Zeroizing::new(SERVER_CERT.to_owned());
            credentials.private_key_pkcs8 =
                Zeroizing::new(STANDARD.decode(SERVER_KEY_DER).unwrap());
        }
        let roots = if trusted { ROOT_CA } else { ROOT_CERTIFICATES };
        let connector = TlsConnector::from(tls_config(&credentials, roots).unwrap());
        let (client, peer) = duplex(64 * 1024);
        let acceptor = tokio_rustls::TlsAcceptor::from(server_config());
        let client_handshake =
            connector.connect(ServerName::try_from(name.to_owned()).unwrap(), client);
        let (client, server) = tokio::join!(client_handshake, acceptor.accept(peer));
        assert_eq!(client.is_ok() && server.is_ok(), succeeds);
    }
}

#[tokio::test]
async fn the_private_tcp_stack_completes_mutual_tls_before_business_admission() {
    tokio::time::timeout(IO_BUDGET * 2, async {
        let config = Config {
            mtu: 1216,
            command_channel_capacity: Some(8),
            tcp_buffer_size: 32 * 1024,
            ..Config::default()
        };
        let (client, client_pipe) =
            crate::netstack::bounded_piped_with_capacity(config.clone(), 16);
        let (server, server_pipe) = crate::netstack::bounded_piped_with_capacity(config, 16);
        let client_channel = client.command_channel();
        let server_channel = server.command_channel();
        let _client_stack = AbortOnDropHandle::new(client.spawn_tokio());
        let _server_stack = AbortOnDropHandle::new(server.spawn_tokio());
        client_channel.set_ips([CLIENT.into()]).await.unwrap();
        server_channel.set_ips([SERVER.into()]).await.unwrap();
        let tuple = ControlTuple {
            local_port: crate::port_allocator::next_tcp_port(),
        };
        let mut from_client = client_pipe.rx;
        let mut from_server = server_pipe.rx;
        let _outgoing = AbortOnDropHandle::new(tokio::spawn(async move {
            while let Some(packet) = from_client.recv_async().await {
                assert!(tuple.outgoing(&packet));
                if !server_pipe.tx.send_owned_checked(packet).await {
                    break;
                }
            }
        }));
        let _incoming = AbortOnDropHandle::new(tokio::spawn(async move {
            while let Some(packet) = from_server.recv_async().await {
                assert!(tuple.incoming(&packet));
                if !client_pipe.tx.send_owned_checked(packet).await {
                    break;
                }
            }
        }));
        let listener = crate::stack_tcp::OnceListener::bind(
            server_channel,
            SocketAddr::new(SERVER.into(), SERVER_PORT),
        )
        .await
        .unwrap();
        let (revoke, revocation) = tokio::sync::oneshot::channel::<()>();
        let (done, completion) = tokio::sync::oneshot::channel::<()>();
        let peer = AbortOnDropHandle::new(tokio::spawn(async move {
            let tcp = listener
                .accept(SocketAddr::new(CLIENT.into(), tuple.local_port))
                .await
                .unwrap();
            let mut tls = tokio_rustls::TlsAcceptor::from(server_config())
                .accept(tcp)
                .await
                .unwrap();
            assert!(
                read_frame(&mut tls)
                    .await
                    .unwrap()
                    .get("status-get")
                    .is_some()
            );
            write_frame(
                &mut tls,
                &json!({"status":{"state":"jailed","reason":null}}),
            )
            .await
            .unwrap();
            assert_eq!(
                read_frame(&mut tls).await.unwrap()["features-set"],
                requested_features()
            );
            read_frame(&mut tls).await.unwrap();
            write_frame(
            &mut tls,
            &json!({"status":{"state":"connected","reason":null,"features":requested_features()}}),
        )
        .await
        .unwrap();
            revocation.await.unwrap();
            write_frame(
                &mut tls,
                &json!({"status":{"state":"hard-jailed","reason":{"code":86102}}}),
            )
            .await
            .unwrap();
            completion.await.unwrap();
        }));
        let credentials = credentials();
        let tcp = crate::stack_tcp::StackTcpStream::connect(
            client_channel,
            SocketAddr::new(CLIENT.into(), tuple.local_port),
            SocketAddr::new(SERVER.into(), SERVER_PORT),
        )
        .await
        .unwrap();
        let mut tls = TlsConnector::from(tls_config(&credentials, ROOT_CA).unwrap())
            .connect(
                ServerName::try_from(credentials.server_name.clone()).unwrap(),
                tcp,
            )
            .await
            .unwrap();
        let (publisher, mut observed) = watch::channel(AgentState::Connecting);
        let expires_at = Arc::new(AtomicI64::new(credentials.expires_at));
        let gate = AgentGate {
            status: observed.clone(),
            expires_at,
        };
        let protocol = AbortOnDropHandle::new(tokio::spawn(async move {
            protocol_loop(
                &mut tls,
                &publisher,
                Instant::now() + IO_BUDGET,
                credentials.expires_at,
            )
            .await
        }));
        tokio::time::timeout(IO_BUDGET, observed.changed())
            .await
            .unwrap()
            .unwrap();
        assert!(gate.authorized());
        revoke.send(()).unwrap();
        let result = tokio::time::timeout(IO_BUDGET, protocol)
            .await
            .unwrap()
            .unwrap();
        done.send(()).unwrap();
        assert_eq!(
            result,
            Err(GateFailure::Proton(ConnectionFailure::agent(86102)))
        );
        assert!(!gate.authorized());
        peer.await.unwrap();
    })
    .await
    .expect("private stack exchange must finish within its budget");
}

#[tokio::test(start_paused = true)]
async fn agent_requires_status_after_features_and_drops_authorization_on_jail() {
    let (mut client, mut server) = duplex(128 * 1024);
    let (publisher, mut observed) = watch::channel(AgentState::Connecting);
    let worker = tokio::spawn(async move {
        protocol_loop(
            &mut client,
            &publisher,
            Instant::now() + IO_BUDGET,
            unix_now().unwrap() + 3600,
        )
        .await
    });
    assert_eq!(
        read_frame(&mut server).await.unwrap()["status-get"]["features-statistics"],
        false
    );
    write_frame(
        &mut server,
        &json!({"status": {"state": "jailed", "reason": null, "features": {}}}),
    )
    .await
    .unwrap();
    assert!(
        read_frame(&mut server)
            .await
            .unwrap()
            .get("features-set")
            .is_some()
    );
    assert!(
        read_frame(&mut server)
            .await
            .unwrap()
            .get("status-get")
            .is_some()
    );
    assert_eq!(*observed.borrow(), AgentState::Connecting);
    write_frame(
        &mut server,
        &json!({"status": {"state": "connected", "reason": null, "features":requested_features()}}),
    )
    .await
    .unwrap();
    observed.changed().await.unwrap();
    assert_eq!(*observed.borrow(), AgentState::Connected);
    // A pushed revocation is read immediately, without waiting for a heartbeat.
    write_frame(
        &mut server,
        &json!({"status": {"state": "hard-jailed", "reason": {"code":86102,"final":true}}}),
    )
    .await
    .unwrap();
    assert!(matches!(
        worker.await.unwrap(),
        Err(GateFailure::Proton(ConnectionFailure {
            code: Some(86102),
            ..
        }))
    ));
}

#[tokio::test(start_paused = true)]
async fn a_partial_frame_survives_a_heartbeat_and_then_revokes_authorization() {
    let (mut client, mut server) = duplex(128 * 1024);
    let (publisher, mut observed) = watch::channel(AgentState::Connecting);
    let worker = tokio::spawn(async move {
        protocol_loop(
            &mut client,
            &publisher,
            Instant::now() + IO_BUDGET,
            unix_now().unwrap() + 3600,
        )
        .await
    });
    let connected =
        json!({"status":{"state":"connected","reason":null,"features":requested_features()}});
    read_frame(&mut server).await.unwrap();
    write_frame(&mut server, &connected).await.unwrap();
    read_frame(&mut server).await.unwrap();
    read_frame(&mut server).await.unwrap();
    write_frame(&mut server, &connected).await.unwrap();
    observed.changed().await.unwrap();
    assert_eq!(*observed.borrow(), AgentState::Connected);
    let body =
        serde_json::to_vec(&json!({"status":{"state":"hard-jailed","reason":{"code":86102}}}))
            .unwrap();
    let header = (body.len() as u32).to_be_bytes();
    server.write_all(&header[..2]).await.unwrap();
    tokio::task::yield_now().await;
    tokio::time::advance(STATUS_INTERVAL).await;
    assert!(
        read_frame(&mut server)
            .await
            .unwrap()
            .get("status-get")
            .is_some()
    );
    server.write_all(&header[2..]).await.unwrap();
    server.write_all(&body).await.unwrap();
    assert!(matches!(
        worker.await.unwrap(),
        Err(GateFailure::Proton(ConnectionFailure {
            code: Some(86102),
            ..
        }))
    ));
}

#[tokio::test(start_paused = true)]
async fn connected_without_feature_confirmation_never_opens_the_gate() {
    let (mut client, mut server) = duplex(128 * 1024);
    let (publisher, observed) = watch::channel(AgentState::Connecting);
    let worker = tokio::spawn(async move {
        protocol_loop(
            &mut client,
            &publisher,
            Instant::now() + IO_BUDGET,
            unix_now().unwrap() + 3600,
        )
        .await
    });
    read_frame(&mut server).await.unwrap();
    write_frame(&mut server, &json!({"status":{"state":"connected"}}))
        .await
        .unwrap();
    read_frame(&mut server).await.unwrap();
    read_frame(&mut server).await.unwrap();
    write_frame(
        &mut server,
        &json!({"status":{"state":"connected","features":{"jail":false}}}),
    )
    .await
    .unwrap();
    assert!(matches!(worker.await.unwrap(), Err(GateFailure::Protocol)));
    assert_eq!(*observed.borrow(), AgentState::Connecting);
}

#[test]
fn unknown_or_ambiguous_status_never_authorizes() {
    for value in [
        json!({}),
        json!({"status":{"state":"unknown"}}),
        json!({"status":{"state":"connected","reason":{"code":86151}}}),
        json!({"status":{"state":"connected"},"error":{"code":86100}}),
    ] {
        assert!(status(&value).is_err());
    }
}

// Deterministic Ed25519 keys and certificates generated only for these memory peers.
const ROOT_CA: &str = r#"-----BEGIN CERTIFICATE-----
MIIBCDCBu6ADAgECAgFkMAUGAytlcDAiMSAwHgYDVQQDDBdTeW50aGV0aWMgQWdl
bnQgVGVzdCBDQTAeFw0yMDAxMDEwMDAwMDBaFw00MDAxMDEwMDAwMDBaMCIxIDAe
BgNVBAMMF1N5bnRoZXRpYyBBZ2VudCBUZXN0IENBMCowBQYDK2VwAyEAxQG1iMDv
Uj3jba6OKfT7d6C+x/xBvvH6RV7oZib8pqijFjAUMBIGA1UdEwEB/wQIMAYBAf8C
AQAwBQYDK2VwA0EAEkrpxFLHUOmeh7yHdincQ4XMTMkrSGhLBXpf0JU+5SwGSnL5
o+d6OT8nhOb4EN4WBVs0va+aLfJi5tlKfGlqAA==
-----END CERTIFICATE-----
"#;
const SERVER_CERT: &str = r#"-----BEGIN CERTIFICATE-----
MIINZzCCDRmgAwIBAgIBZTAFBgMrZXAwIjEgMB4GA1UEAwwXU3ludGhldGljIEFn
ZW50IFRlc3QgQ0EwHhcNMjAwMTAxMDAwMDAwWhcNNDAwMTAxMDAwMDAwWjAgMR4w
HAYDVQQDDBVhZ2VudC5leGFtcGxlLmludmFsaWQwKjAFBgMrZXADIQAw9gHUnSzC
bASwynj0B0QJYqsUeEVIdAI7lEDHaN1yK6OCDHQwggxwMAwGA1UdEwEB/wQCMAAw
EwYDVR0lBAwwCgYIKwYBBQUHAwEwIAYDVR0RBBkwF4IVYWdlbnQuZXhhbXBsZS5p
bnZhbGlkMIIMJwYJKwYBBAGDsgMBBIIMGG9mZmxpbmUgZml4dHVyZSBwYWRkaW5n
OgAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA
AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA
AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA
AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA
AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA
AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA
AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA
AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA
AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA
AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA
AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA
AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA
AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA
AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA
AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA
AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA
AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA
AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA
AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA
AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA
AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA
AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA
AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA
AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA
AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA
AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA
AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA
AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA
AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA
AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA
AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA
AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA
AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA
AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA
AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA
AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA
AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA
AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA
AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA
AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA
AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA
AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA
AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA
AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA
AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA
AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA
AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA
AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA
AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA
AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA
AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA
AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA
AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA
AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA
AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA
AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA
AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA
AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA
AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA
AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA
AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA
AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA
AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA
AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA
ADAFBgMrZXADQQDLZj5hiihg++i0rL+tHp/Wjwdtz6urQinsugQql4upyGLEyzGu
Iv6suiUrm0/u55D8IRLXL45Gu/f2oTK5eS8C
-----END CERTIFICATE-----
"#;
const CLIENT_CERT: &str = r#"-----BEGIN CERTIFICATE-----
MIIBFjCByaADAgECAgFmMAUGAytlcDAiMSAwHgYDVQQDDBdTeW50aGV0aWMgQWdl
bnQgVGVzdCBDQTAeFw0yMDAxMDEwMDAwMDBaFw00MDAxMDEwMDAwMDBaMCExHzAd
BgNVBAMMFlN5bnRoZXRpYyBBZ2VudCBDbGllbnQwKjAFBgMrZXADIQC4ql/GJleH
8tta3EvbUkzgp1LP0gdTAYr9hbPuz4HOn6MlMCMwDAYDVR0TAQH/BAIwADATBgNV
HSUEDDAKBggrBgEFBQcDAjAFBgMrZXADQQCatyUR6h14cbTDZ24Q/iuw6GfnONHw
VZ3TYmWZpSx47s6vXFzzi3PmKJOeTN8P0zs078kXdVElgYh5BZ+8V8IJ
-----END CERTIFICATE-----
"#;
const SERVER_KEY_DER: &str = "MC4CAQAwBQYDK2VwBCIEIM3qQd7LQxDZB9/evuGV97JkvDCKbbRukjFcPOopuuVF";
const CLIENT_KEY_DER: &str = "MC4CAQAwBQYDK2VwBCIEIBmB1o89tTHu+GlbLpA4177UYUj+hFS0wIfZPxHbmAk1";

#[test]
fn reason_codes_and_final_flag_are_distinct_from_http_authentication() {
    for code in [
        86100, 86101, 86102, 86103, 86105, 86106, 86111, 86151, 86153,
    ] {
        assert_eq!(
            status(&json!({"status":{"state":"hard-jailed","reason":{"code":code,"final":false}}})),
            Err(GateFailure::Proton(ConnectionFailure::agent(code)))
        );
    }
    for state in ["jailed", "hard-jailed", "connected"] {
        assert_eq!(
            status(&json!({"status":{"state":state,"reason":{"code":86104,"final":false}}})),
            Ok(false)
        );
        assert_eq!(
            status(&json!({"status":{"state":state,"reason":{"code":99000,"final":false}}})),
            Ok(false)
        );
    }
    assert_eq!(
        status(&json!({"status":{"state":"hard-jailed","reason":{"code":99000,"final":true}}})),
        Err(GateFailure::Proton(ConnectionFailure::agent(99000)))
    );
    assert_eq!(
        status(&json!({"status":{"state":"connected","reason":null},"error":null})),
        Ok(true)
    );
    for reason in [
        json!({"code":-1}),
        json!({"code":4294967296_u64}),
        json!({"code":86104,"final":"false"}),
        json!("secret"),
    ] {
        assert!(status(&json!({"status":{"state":"connected","reason":reason}})).is_err());
    }
}

#[tokio::test]
async fn labelled_restricted_node_waits_for_matching_authorized_status_and_closes_again_on_jail() {
    let (mut client, mut server) = duplex(8192);
    let (publisher, mut observed) = watch::channel(AgentState::Connecting);
    let worker = tokio::spawn(async move {
        protocol_loop_with_label(
            &mut client,
            &publisher,
            Instant::now() + IO_BUDGET,
            unix_now().unwrap() + 3600,
            Some("0"),
        )
        .await
    });
    read_frame(&mut server).await.unwrap();
    write_frame(
        &mut server,
        &json!({"status":{"state":"hard-jailed","reason":{"code":86104,"final":false}}}),
    )
    .await
    .unwrap();
    let request = read_frame(&mut server).await.unwrap();
    assert_eq!(request["features-set"]["bouncing"], "0");
    assert_eq!(request["features-set"]["jail"], false);
    read_frame(&mut server).await.unwrap();
    write_frame(
        &mut server,
        &json!({"status":{"state":"connected","features":features(Some("1"))}}),
    )
    .await
    .unwrap();
    for _ in 0..10 {
        tokio::task::yield_now().await;
    }
    assert_eq!(*observed.borrow(), AgentState::Connecting);
    write_frame(
        &mut server,
        &json!({"status":{"state":"connected","features":features(Some("0"))}}),
    )
    .await
    .unwrap();
    observed
        .wait_for(|state| *state == AgentState::Connected)
        .await
        .unwrap();
    write_frame(
        &mut server,
        &json!({"status":{"state":"hard-jailed","reason":{"code":86104,"final":false}}}),
    )
    .await
    .unwrap();
    observed
        .wait_for(|state| *state == AgentState::Connecting)
        .await
        .unwrap();
    assert!(!worker.is_finished());
    write_frame(
        &mut server,
        &json!({"status":{"state":"connected","features":features(Some("0"))}}),
    )
    .await
    .unwrap();
    observed
        .wait_for(|state| *state == AgentState::Connected)
        .await
        .unwrap();
    drop(server);
    assert!(worker.await.unwrap().is_err());
}

#[test]
fn server_trust_errors_are_not_mistaken_for_client_certificate_alerts() {
    for alert in [
        rustls::AlertDescription::UnknownCA,
        rustls::AlertDescription::CertificateExpired,
    ] {
        assert!(client_certificate_alert(&std::io::Error::other(
            rustls::Error::AlertReceived(alert)
        )));
    }
    assert!(!client_certificate_alert(&std::io::Error::other(
        rustls::Error::InvalidCertificate(rustls::CertificateError::UnknownIssuer)
    )));
    assert!(!client_certificate_alert(&std::io::Error::other(
        "unknown_ca secret"
    )));
}

#[tokio::test]
async fn agent_certificate_recovery_is_available_before_initial_admission() {
    tokio::time::timeout(Duration::from_secs(20), async {
        for code in [86101, 86102] {
            let config = Config { mtu: 1216, command_channel_capacity: Some(8), tcp_buffer_size: 32*1024, ..Config::default() };
            let (client, client_pipe) = crate::netstack::bounded_piped_with_capacity(config.clone(), 16);
            let (server, server_pipe) = crate::netstack::bounded_piped_with_capacity(config, 16);
            let client_channel = client.command_channel(); let server_channel = server.command_channel();
            let _client_stack = AbortOnDropHandle::new(client.spawn_tokio());
            let _server_stack = AbortOnDropHandle::new(server.spawn_tokio());
            client_channel.set_ips([CLIENT.into()]).await.unwrap(); server_channel.set_ips([SERVER.into()]).await.unwrap();
            let tuple = ControlTuple { local_port: crate::port_allocator::next_tcp_port() };
            let mut from_client = client_pipe.rx; let mut from_server = server_pipe.rx;
            let _outgoing = AbortOnDropHandle::new(tokio::spawn(async move { while let Some(packet) = from_client.recv_async().await { if !server_pipe.tx.send_owned_checked(packet).await { break; } } }));
            let _incoming = AbortOnDropHandle::new(tokio::spawn(async move { while let Some(packet) = from_server.recv_async().await { if !client_pipe.tx.send_owned_checked(packet).await { break; } } }));
            let listener = crate::stack_tcp::OnceListener::bind(server_channel.clone(), SocketAddr::new(SERVER.into(), SERVER_PORT)).await.unwrap();
            let (publisher, mut observed) = watch::channel(AgentState::Connecting);
            let expiry = Arc::new(AtomicI64::new(unix_now().unwrap()+3600));
            let gate = AgentGate { status: observed.clone(), expires_at: expiry.clone() };
            let (requests, mut received) = mpsc::channel::<RecoveryRequest>(1);
            let (updates, credentials) = watch::channel(Arc::new(self::credentials()));
            let (continue_peer, resumed) = oneshot::channel();
            let peer = AbortOnDropHandle::new(tokio::spawn(async move {
                let tcp = listener.accept(SocketAddr::new(CLIENT.into(), tuple.local_port)).await.unwrap();
                let mut tls = tokio_rustls::TlsAcceptor::from(server_config()).accept(tcp).await.unwrap();
                read_frame(&mut tls).await.unwrap();
                write_frame(&mut tls, &json!({"status":{"state":"hard-jailed","reason":{"code":code,"final":true}}})).await.unwrap();
                resumed.await.unwrap(); drop(tls);
                if code == 86101 {
                    let listener = crate::stack_tcp::OnceListener::bind(server_channel, SocketAddr::new(SERVER.into(), SERVER_PORT)).await.unwrap();
                    let tcp = listener.accept(SocketAddr::new(CLIENT.into(), tuple.local_port)).await.unwrap();
                    let mut tls = tokio_rustls::TlsAcceptor::from(server_config()).accept(tcp).await.unwrap();
                    read_frame(&mut tls).await.unwrap();
                    write_frame(&mut tls, &json!({"status":{"state":"jailed","reason":null}})).await.unwrap();
                    read_frame(&mut tls).await.unwrap(); read_frame(&mut tls).await.unwrap();
                    write_frame(&mut tls, &json!({"status":{"state":"connected","reason":null,"features":requested_features()}})).await.unwrap();
                    std::future::pending::<()>().await;
                }
            }));
            let cancel = CancellationToken::new(); let task_cancel = cancel.clone(); let task_expiry = expiry.clone();
            let worker = AbortOnDropHandle::new(tokio::spawn(async move {
                credential_loop(credentials, client_channel, tuple, Lifetime { publisher: &publisher, cancellation: &task_cancel, startup_deadline: Instant::now()+Duration::from_secs(15), expires_at_gate: &task_expiry, recovery: Some(requests), roots: ROOT_CA }).await
            }));
            let request = received.recv().await.unwrap(); assert_eq!(request.code, code); assert!(!gate.authorized());
            let mut replacement = self::credentials();
            if code == 86102 { replacement.private_key_pkcs8 = Zeroizing::new(STANDARD.decode(SERVER_KEY_DER).unwrap()); }
            let replacement = Arc::new(replacement);
            continue_peer.send(()).unwrap();
            tokio::task::yield_now().await;
            updates.send_replace(replacement.clone()); request.reply.send(Ok(replacement)).unwrap();
            if code == 86102 {
                assert_eq!(worker.await.unwrap(), Err(GateFailure::Proton(ConnectionFailure::new(FailureKind::Reconnect))));
                assert!(!gate.authorized());
            } else {
                observed.wait_for(|state| *state == AgentState::Connected).await.unwrap();
                assert!(gate.authorized()); cancel.cancel(); let _ = worker.await;
            }
            drop(peer);
        }
    }).await.unwrap();
}

#[tokio::test(start_paused = true)]
async fn restricted_status_watchdog_preserves_reason_and_never_admits_business() {
    for response in [
        json!({"status":{"state":"hard-jailed","reason":{"code":86104,"final":false}}}),
        json!({"error":{"code":86104}}),
    ] {
        let (mut client, mut server) = duplex(8192);
        let (publisher, observed) = watch::channel(AgentState::Connecting);
        let worker = tokio::spawn(async move {
            protocol_loop(
                &mut client,
                &publisher,
                Instant::now() + IO_BUDGET,
                unix_now().unwrap() + 3600,
            )
            .await
        });
        read_frame(&mut server).await.unwrap();
        write_frame(&mut server, &response).await.unwrap();
        read_frame(&mut server).await.unwrap();
        read_frame(&mut server).await.unwrap();
        assert_eq!(*observed.borrow(), AgentState::Connecting);
        tokio::time::advance(IO_BUDGET + Duration::from_secs(1)).await;
        assert_eq!(
            worker.await.unwrap(),
            Err(GateFailure::Proton(ConnectionFailure::agent(86104)))
        );
        assert_eq!(*observed.borrow(), AgentState::Connecting);
    }
}
