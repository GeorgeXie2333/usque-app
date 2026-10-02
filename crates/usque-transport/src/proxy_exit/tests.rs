//! Protocol peers in memory: no account, physical socket, routes or TUN.
use super::*;
use tokio::io::DuplexStream;

struct MemoryStream(DuplexStream);
impl TcpIo for MemoryStream {
    fn local_addr(&self) -> io::Result<SocketAddr> {
        Ok("192.0.2.1:49152".parse().unwrap())
    }
}
impl AsyncRead for MemoryStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.0).poll_read(cx, buf)
    }
}
impl AsyncWrite for MemoryStream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.0).poll_write(cx, buf)
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.0).poll_flush(cx)
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.0).poll_shutdown(cx)
    }
}
fn pair() -> (TcpStream, DuplexStream) {
    let (a, b) = tokio::io::duplex(32768);
    (Box::new(MemoryStream(a)), b)
}

struct SocksPeer {
    targets: std::sync::Mutex<Vec<TcpTarget>>,
    stop_udp: CancellationToken,
}
#[async_trait]
impl TcpDialer for SocksPeer {
    async fn connect(
        &self,
        target: TcpTarget,
        _deadline: Instant,
        _cancel: &CancellationToken,
        _class: FlowClass,
    ) -> Result<TcpStream, DialError> {
        self.targets.lock().unwrap().push(target);
        let (client, mut peer) = pair();
        let stop = self.stop_udp.clone();
        tokio::spawn(async move {
            let work = async {
                let mut greeting = [0; 3];
                peer.read_exact(&mut greeting).await?;
                assert_eq!(greeting, [5, 1, 0]);
                peer.write_all(&[5, 0]).await?;
                let mut header = [0; 4];
                peer.read_exact(&mut header).await?;
                let len = match header[3] {
                    1 => 4,
                    4 => 16,
                    3 => usize::from(peer.read_u8().await?),
                    _ => panic!("invalid target"),
                };
                let mut body = vec![0; len + 2];
                peer.read_exact(&mut body).await?;
                peer.write_all(&[5, 0, 0, 1, 203, 0, 113, 2, 0x23, 0x28])
                    .await?;
                if header[1] == 3 {
                    stop.cancelled().await;
                    return Ok::<_, io::Error>(());
                }
                let mut buffer = [0; 256];
                loop {
                    let n = peer.read(&mut buffer).await?;
                    if n == 0 {
                        return Ok(());
                    }
                    peer.write_all(&buffer[..n]).await?;
                }
            };
            let _ = work.await;
        });
        Ok(client)
    }
}

#[tokio::test]
async fn proxy_tcp_and_udp_use_underlay_and_release_cancelled_associations() {
    use crate::proxy_udp::{SocksFactory, UdpFactory};
    use ts_netstack_smoltcp::CreateSocket;
    use ts_netstack_smoltcp::netcore::{HasChannel, NetstackControl};
    let cancel = CancellationToken::new();
    let peer = Arc::new(SocksPeer {
        targets: Default::default(),
        stop_udp: CancellationToken::new(),
    });
    let profile = usque_core::Profile {
        mtu: 1280,
        ..Default::default()
    };
    let (config, _) = crate::netstack::proxy_netstack_config(&profile);
    let (left, mut lp) = crate::netstack::bounded_piped(config);
    let (config, _) = crate::netstack::proxy_netstack_config(&profile);
    let (right, mut rp) = crate::netstack::bounded_piped(config);
    let lc = left.command_channel();
    let rc = right.command_channel();
    let _left = tokio_util::task::AbortOnDropHandle::new(left.spawn_tokio());
    let _right = tokio_util::task::AbortOnDropHandle::new(right.spawn_tokio());
    lc.set_ips(["192.0.2.1".parse::<std::net::IpAddr>().unwrap()])
        .await
        .unwrap();
    rc.set_ips(["203.0.113.2".parse::<std::net::IpAddr>().unwrap()])
        .await
        .unwrap();
    let _wire = tokio_util::task::AbortOnDropHandle::new(tokio::spawn(async move {
        loop {
            tokio::select! {
                Some(p)=lp.rx.recv_async()=>{ rp.tx.send_owned_async(p).await; },
                Some(p)=rp.rx.recv_async()=>{ lp.tx.send_owned_async(p).await; },
                else=>break,
            }
        }
    }));
    let relay = rc
        .udp_bind("203.0.113.2:9000".parse().unwrap())
        .await
        .unwrap();
    let _relay = tokio_util::task::AbortOnDropHandle::new(tokio::spawn(async move {
        loop {
            let Ok((from, data)) = relay.recv_from_bytes().await else {
                break;
            };
            relay.send_to(from, &data).await.unwrap();
        }
    }));
    let (_, health) = watch::channel(crate::netstack::RuntimeHealth::Connected {
        path: crate::netstack::RuntimePath {
            transport: usque_core::Transport::Http3,
            endpoint_family: usque_core::AddressFamily::Ipv4,
            ipv4_available: true,
            ipv6_available: false,
        },
        reconnect_count: 0,
    });
    let network = crate::InternalNetwork::for_streams(peer.clone(), health, cancel.clone())
        .with_test_packets(
            lc,
            "192.0.2.1".parse().unwrap(),
            std::net::Ipv6Addr::UNSPECIFIED,
        );
    let mut secrets = ImportSecrets::default();
    secrets.proxy = Some(usque_core::chain_exit::ProxyExitConfiguration {
        host: "203.0.113.2".into(),
        port: 1080,
        auth_mode: ProxyAuthMode::None,
        dns_servers: vec![],
    });
    let parsed = usque_core::chain_exit::ValidatedProfile::parse(
        usque_core::chain_exit::ChainSource::Socks5Proxy,
        &secrets,
    )
    .unwrap();
    let summary = parsed
        .summary("Test", uuid::Uuid::new_v4(), uuid::Uuid::new_v4())
        .unwrap();
    let prepared = PreparedProfile::imported(summary, parsed, secrets);
    let metrics = Arc::new(crate::l4::L4Metrics::default());
    let budget = Arc::new(crate::l4::BufferBudget::new(
        16 << 20,
        metrics,
        Arc::new(tokio::sync::Notify::new()),
    ));
    let deadline = Instant::now() + std::time::Duration::from_secs(10);
    let proxy = ProxyDialer::start(
        &prepared,
        network,
        None,
        cancel.clone(),
        budget.clone(),
        Arc::default(),
        deadline,
    )
    .await
    .unwrap();
    assert!(!proxy.is_ready());
    assert!(!proxy.status.borrow().tcp_connect_verified);
    assert!(proxy.status.borrow().attempting_endpoint.is_none());
    assert_eq!(proxy.status.borrow().attempt_count, 1);
    let target = TcpTarget::new("example.test", 443).unwrap();
    assert!(
        proxy
            .connect(target.clone(), deadline, &cancel, FlowClass::Business)
            .await
            .is_err()
    );
    proxy.admitted.store(true, Ordering::Release);
    let before = proxy.active.available_permits();
    let mut tcp = proxy
        .connect(target.clone(), deadline, &cancel, FlowClass::Business)
        .await
        .unwrap();
    tcp.write_all(b"hello").await.unwrap();
    let mut bytes = [0; 5];
    tcp.read_exact(&mut bytes).await.unwrap();
    assert_eq!(&bytes, b"hello");
    assert!(proxy.status.borrow().tcp_connect_verified);
    let factory = SocksFactory(proxy.clone());
    let udp = factory.open(&cancel, deadline).await.unwrap();
    udp.send(&target, b"datagram").await.unwrap();
    let received = timeout_at(deadline, udp.recv()).await.unwrap().unwrap();
    assert_eq!(received, (target, bytes::Bytes::from_static(b"datagram")));
    assert_eq!(
        proxy.status.borrow().proxy_udp.as_deref(),
        Some("available")
    );
    peer.stop_udp.cancel();
    assert!(timeout_at(deadline, udp.recv()).await.unwrap().is_err());
    drop(udp);
    tcp.write_all(b"still").await.unwrap();
    tcp.read_exact(&mut bytes).await.unwrap();
    assert_eq!(&bytes, b"still");
    assert_eq!(proxy.active.available_permits(), before - 1);
    cancel.cancel();
    assert!(tcp.read_u8().await.is_err());
    drop(tcp);
    assert_eq!(proxy.active.available_permits(), before);
    assert_eq!(budget.available(), 16 << 20);
    assert!(
        peer.targets
            .lock()
            .unwrap()
            .iter()
            .all(|t| t.authority() == "203.0.113.2:1080")
    );
}
async fn read_header(peer: &mut DuplexStream) -> Vec<u8> {
    let mut header = vec![];
    while !header.ends_with(b"\r\n\r\n") {
        header.push(peer.read_u8().await.unwrap());
    }
    header
}

#[tokio::test]
async fn http_connect_auth_ipv6_and_coalesced_application_bytes() {
    let (mut client, mut peer) = pair();
    let server = tokio::spawn(async move {
        let header = read_header(&mut peer).await;
        assert_eq!(header, b"CONNECT [2001:db8::2]:443 HTTP/1.1\r\nHost: [2001:db8::2]:443\r\nProxy-Authorization: Basic dXNlcjpwYXNz\r\n\r\n");
        peer.write_all(b"HTTP/1.1 100 Continue\r\n\r\nHTTP/1.1 200 Established\r\n\r\nhello")
            .await
            .unwrap();
        let mut data = [0; 4];
        peer.read_exact(&mut data).await.unwrap();
        assert_eq!(&data, b"ping");
    });
    let credentials = ImportSecrets {
        username: "user".into(),
        password: "pass".into(),
        proxy: None,
        configuration: String::new(),
        private_key_password: String::new(),
    };
    http_connect(
        &mut client,
        &TcpTarget::new("2001:db8::2", 443).unwrap(),
        &credentials,
        ProxyAuthMode::UsernamePassword,
    )
    .await
    .unwrap();
    let mut data = [0; 5];
    client.read_exact(&mut data).await.unwrap();
    assert_eq!(&data, b"hello");
    client.write_all(b"ping").await.unwrap();
    server.await.unwrap();
}

#[tokio::test]
async fn http_rejects_auth_failure_bad_status_and_oversized_headers() {
    for response in [
        b"HTTP/1.1 407 Authentication Required\r\n\r\n".to_vec(),
        b"garbage 200 OK\r\n\r\n".to_vec(),
        vec![b'x'; 16385],
    ] {
        let (mut client, mut peer) = pair();
        let server = tokio::spawn(async move {
            read_header(&mut peer).await;
            let _ = peer.write_all(&response).await;
        });
        assert!(
            http_connect(
                &mut client,
                &TcpTarget::new("example.test", 80).unwrap(),
                &ImportSecrets::default(),
                ProxyAuthMode::None
            )
            .await
            .is_err()
        );
        server.await.unwrap();
    }
}

#[tokio::test]
async fn socks_userpass_and_domain_connect_preserve_payload() {
    let (mut client, mut peer) = pair();
    let server = tokio::spawn(async move {
        let mut hello = [0; 3];
        peer.read_exact(&mut hello).await.unwrap();
        assert_eq!(hello, [5, 1, 2]);
        peer.write_all(&[5, 2]).await.unwrap();
        let mut auth = [0; 5];
        peer.read_exact(&mut auth).await.unwrap();
        assert_eq!(auth, [1, 1, b'u', 1, b'p']);
        peer.write_all(&[1, 0]).await.unwrap();
        let mut request = [0; 19];
        peer.read_exact(&mut request).await.unwrap();
        assert_eq!(&request[..5], &[5, 1, 0, 3, 12]);
        assert_eq!(&request[5..17], b"example.test");
        peer.write_all(&[5, 0, 0, 1, 0, 0, 0, 0, 0, 0, b'o', b'k'])
            .await
            .unwrap();
    });
    socks_auth(
        &mut client,
        &ImportSecrets {
            username: "u".into(),
            password: "p".into(),
            proxy: None,
            configuration: String::new(),
            private_key_password: String::new(),
        },
        ProxyAuthMode::UsernamePassword,
    )
    .await
    .unwrap();
    socks_command(
        &mut client,
        1,
        &TcpTarget::new("example.test", 443).unwrap(),
    )
    .await
    .unwrap();
    let mut data = [0; 2];
    client.read_exact(&mut data).await.unwrap();
    assert_eq!(&data, b"ok");
    server.await.unwrap();
}

#[tokio::test]
async fn socks_refuses_authentication_downgrade() {
    let (mut client, mut peer) = pair();
    let server = tokio::spawn(async move {
        let mut hello = [0; 3];
        peer.read_exact(&mut hello).await.unwrap();
        peer.write_all(&[5, 0]).await.unwrap();
    });
    assert_eq!(
        socks_auth(
            &mut client,
            &ImportSecrets::default(),
            ProxyAuthMode::UsernamePassword
        )
        .await,
        Err(DialError::Rejected(407))
    );
    server.await.unwrap();
}

#[tokio::test]
async fn socks_udp_command_uses_zero_address_and_distinguishes_unsupported() {
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../tests/fixtures/proxy-exit/contract.json"
    ))
    .unwrap();
    for code in [0, fixture["socks5_udp_unsupported"].as_u64().unwrap() as u8] {
        let (mut client, mut peer) = pair();
        let server = tokio::spawn(async move {
            let mut request = [0; 10];
            peer.read_exact(&mut request).await.unwrap();
            assert_eq!(request, [5, 3, 0, 1, 0, 0, 0, 0, 0, 0]);
            peer.write_all(&[5, code, 0, 1, 0, 0, 0, 0, 0x23, 0x28])
                .await
                .unwrap();
        });
        let result = socks_command(
            &mut client,
            3,
            &TcpTarget::address("0.0.0.0:0".parse().unwrap()),
        )
        .await;
        if code == 0 {
            assert_eq!(result.unwrap(), ("0.0.0.0".into(), 9000));
        } else {
            assert_eq!(result, Err(DialError::Rejected(7)));
        }
        server.await.unwrap();
    }
}

#[tokio::test]
async fn socks_rejects_unknown_reply_codes_before_they_can_enable_dns_only_udp() {
    for code in [9, 255] {
        let (mut client, mut peer) = pair();
        let server = tokio::spawn(async move {
            let mut request = [0; 10];
            peer.read_exact(&mut request).await.unwrap();
            peer.write_all(&[5, code, 0, 1]).await.unwrap();
        });
        assert_eq!(
            socks_command(
                &mut client,
                3,
                &TcpTarget::address("0.0.0.0:0".parse().unwrap()),
            )
            .await,
            Err(DialError::Protocol)
        );
        server.await.unwrap();
    }
}

#[tokio::test]
async fn malformed_command_denials_do_not_enable_dns_only_udp() {
    for code in [2, 7] {
        for (reply, error) in [
            (vec![5, code, 0, 255], DialError::Protocol),
            (vec![5, code, 0, 3, 0], DialError::Protocol),
            (vec![5, code, 0, 3, 1, 255, 0, 0], DialError::Protocol),
            (vec![5, code, 0], DialError::Closed),
            (vec![5, code, 0, 1], DialError::Closed),
            (vec![5, code, 0, 1, 0, 0, 0, 0, 0], DialError::Closed),
            (vec![5, code, 0, 3, 3, b'a', b'b'], DialError::Closed),
        ] {
            let (mut client, mut peer) = pair();
            let server = tokio::spawn(async move {
                let mut request = [0; 10];
                peer.read_exact(&mut request).await.unwrap();
                peer.write_all(&reply).await.unwrap();
            });
            assert_eq!(
                socks_command(
                    &mut client,
                    3,
                    &TcpTarget::address("0.0.0.0:0".parse().unwrap()),
                )
                .await,
                Err(error)
            );
            server.await.unwrap();
        }
    }
}

#[tokio::test]
async fn frozen_go_http_success_fixture_is_accepted() {
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../tests/fixtures/proxy-exit/contract.json"
    ))
    .unwrap();
    let (mut client, mut peer) = pair();
    let server = tokio::spawn(async move {
        read_header(&mut peer).await;
        peer.write_all(fixture["http_success"].as_str().unwrap().as_bytes())
            .await
            .unwrap();
    });
    http_connect(
        &mut client,
        &TcpTarget::new("example.test", 443).unwrap(),
        &ImportSecrets::default(),
        ProxyAuthMode::None,
    )
    .await
    .unwrap();
    server.await.unwrap();
}
