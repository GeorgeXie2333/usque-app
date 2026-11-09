use super::*;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    time::timeout,
};
use usque_core::{
    AddressFamily, Transport, TransportFailure, TransportFailureCode, TransportStage,
};

#[derive(Default)]
struct Recorder(Mutex<Vec<SystemFlowIntent>>);
#[async_trait]
impl SystemFlowAuthorizer for Recorder {
    async fn authorize(&self, intent: SystemFlowIntent) -> Result<(), String> {
        self.0.lock().unwrap().push(intent);
        Ok(())
    }
}
fn profile() -> Profile {
    let mut p = Profile::default();
    p.frontends.socks5 = true;
    p.frontends.http = true;
    p.proxy.socks5_listeners = vec!["127.0.0.1:0".parse().unwrap()];
    p.proxy.http_listeners = vec!["127.0.0.1:0".parse().unwrap()];
    p
}
fn connected(reconnect_count: u32) -> RuntimeHealth {
    RuntimeHealth::Connected {
        path: crate::RuntimePath {
            transport: Transport::Http3,
            endpoint_family: AddressFamily::Ipv4,
            ipv4_available: true,
            ipv6_available: false,
        },
        reconnect_count,
    }
}
async fn http_connect(address: SocketAddr, port: u16) -> (TcpStream, String) {
    let mut stream = TcpStream::connect(address).await.unwrap();
    stream
        .write_all(
            format!("CONNECT localhost:{port} HTTP/1.1\r\nHost: localhost:{port}\r\n\r\n")
                .as_bytes(),
        )
        .await
        .unwrap();
    let mut header = Vec::new();
    timeout(Duration::from_secs(3), async {
        while !header.ends_with(b"\r\n\r\n") {
            header.push(stream.read_u8().await.unwrap());
            assert!(header.len() < 4096);
        }
    })
    .await
    .unwrap();
    (stream, String::from_utf8(header).unwrap())
}
async fn echo(listener: TcpListener) {
    let (mut stream, _) = listener.accept().await.unwrap();
    let mut buf = [0; 32];
    let n = stream.read(&mut buf).await.unwrap();
    stream.write_all(&buf[..n]).await.unwrap();
}
#[tokio::test]
async fn http_and_socks_use_os_sockets_with_hostname_intents() {
    let observer = Arc::new(Recorder::default());
    let (_tx, rx) = watch::channel(connected(0));
    let mut runtime = SystemProxyRuntime::start_with_authorizer(&profile(), rx, observer.clone())
        .await
        .unwrap();
    runtime.activate_final();
    let http_origin = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let http_port = http_origin.local_addr().unwrap().port();
    let http_echo = tokio::spawn(echo(http_origin));
    let (mut http, header) = http_connect(runtime.http_listeners()[0], http_port).await;
    assert!(header.starts_with("HTTP/1.1 200"), "{header}");
    http.write_all(b"http").await.unwrap();
    let mut buf = [0; 4];
    http.read_exact(&mut buf).await.unwrap();
    assert_eq!(&buf, b"http");
    http_echo.await.unwrap();
    let socks_origin = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let socks_port = socks_origin.local_addr().unwrap().port();
    let socks_echo = tokio::spawn(echo(socks_origin));
    let mut socks = TcpStream::connect(runtime.socks5_listeners()[0])
        .await
        .unwrap();
    socks.write_all(&[5, 1, 0]).await.unwrap();
    let mut hello = [0; 2];
    socks.read_exact(&mut hello).await.unwrap();
    assert_eq!(hello, [5, 0]);
    let mut request = vec![5, 1, 0, 3, 9];
    request.extend_from_slice(b"localhost");
    request.extend_from_slice(&socks_port.to_be_bytes());
    socks.write_all(&request).await.unwrap();
    let mut response = [0; 10];
    socks.read_exact(&mut response).await.unwrap();
    assert_eq!(&response[..4], &[5, 0, 0, 1]);
    socks.write_all(b"sock").await.unwrap();
    socks.read_exact(&mut buf).await.unwrap();
    assert_eq!(&buf, b"sock");
    socks_echo.await.unwrap();
    let intents = observer.0.lock().unwrap().clone();
    assert!(
        intents
            .iter()
            .all(|i| i.hostname.as_deref() == Some("localhost")
                && i.local.ip().is_loopback()
                && i.local.port() != 0)
    );
    assert!(intents.iter().any(|i| i.remote.port() == http_port));
    assert!(intents.iter().any(|i| i.remote.port() == socks_port));
    assert!(runtime.statistics().bytes_sent >= 8);
    runtime.shutdown().await;
}

#[tokio::test]
async fn readiness_recovery_and_control_loss_stop_existing_flows() {
    let observer = Arc::new(Recorder::default());
    let initial = RuntimeHealth::Connecting {
        requested: connected(0).path(),
        attempt: 1,
    };
    let (tx, rx) = watch::channel(initial);
    let mut runtime = SystemProxyRuntime::start_with_authorizer(&profile(), rx, observer.clone())
        .await
        .unwrap();
    runtime.activate_final();
    let origin = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = origin.local_addr().unwrap().port();
    let connect = tokio::spawn(http_connect(runtime.http_listeners()[0], port));
    assert!(
        timeout(Duration::from_millis(60), origin.accept())
            .await
            .is_err()
    );
    assert!(observer.0.lock().unwrap().is_empty());
    tx.send_replace(connected(0));
    let (mut peer, _) = timeout(Duration::from_secs(3), origin.accept())
        .await
        .unwrap()
        .unwrap();
    let (mut client, header) = connect.await.unwrap();
    assert!(header.contains("200"));
    client.write_all(b"x").await.unwrap();
    let mut byte = [0];
    peer.read_exact(&mut byte).await.unwrap();
    let failure = TransportFailure::new(
        TransportFailureCode::PhysicalNetworkChanged,
        TransportStage::SocketConnect,
    );
    tx.send_replace(RuntimeHealth::Reconnecting {
        last_path: connected(0).path(),
        attempt: 1,
        reconnect_count: 1,
        reason: "isolated_network_change".into(),
        failure,
    });
    assert_eq!(
        timeout(Duration::from_secs(3), client.read(&mut byte))
            .await
            .unwrap()
            .unwrap(),
        0
    );
    tx.send_replace(connected(1));
    let connect = tokio::spawn(http_connect(runtime.http_listeners()[0], port));
    let (_peer2, _) = timeout(Duration::from_secs(3), origin.accept())
        .await
        .unwrap()
        .unwrap();
    let (mut client2, _) = connect.await.unwrap();
    let values = observer.0.lock().unwrap().clone();
    assert!(values.last().unwrap().epoch > values.first().unwrap().epoch);
    drop(tx);
    assert_eq!(
        timeout(Duration::from_secs(3), client2.read(&mut byte))
            .await
            .unwrap()
            .unwrap(),
        0
    );
    runtime.shutdown().await;
}

#[tokio::test]
async fn absent_flow_authorizer_blocks_before_syn() {
    let (_tx, rx) = watch::channel(connected(0));
    let mut runtime = SystemProxyRuntime::start(&profile(), rx).await.unwrap();
    runtime.activate_final();
    let origin = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let (_, header) = http_connect(
        runtime.http_listeners()[0],
        origin.local_addr().unwrap().port(),
    )
    .await;
    assert!(!header.contains(" 200 "));
    assert!(
        timeout(Duration::from_millis(60), origin.accept())
            .await
            .is_err()
    );
    runtime.shutdown().await;
}

#[tokio::test]
async fn socks_udp_preserves_domain_and_cancels_old_epoch() {
    let observer = Arc::new(Recorder::default());
    let (tx, rx) = watch::channel(connected(0));
    let mut runtime = SystemProxyRuntime::start_with_authorizer(&profile(), rx, observer.clone())
        .await
        .unwrap();
    runtime.activate_final();
    let upstream = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let port = upstream.local_addr().unwrap().port();
    let mut control = TcpStream::connect(runtime.socks5_listeners()[0])
        .await
        .unwrap();
    control.write_all(&[5, 1, 0]).await.unwrap();
    let mut hello = [0; 2];
    control.read_exact(&mut hello).await.unwrap();
    control
        .write_all(&[5, 3, 0, 1, 0, 0, 0, 0, 0, 0])
        .await
        .unwrap();
    let mut reply = [0; 10];
    control.read_exact(&mut reply).await.unwrap();
    assert_eq!(&reply[..4], &[5, 0, 0, 1]);
    let relay = SocketAddr::from((
        [reply[4], reply[5], reply[6], reply[7]],
        u16::from_be_bytes([reply[8], reply[9]]),
    ));
    let client = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let mut packet = vec![0, 0, 0, 3, 9];
    packet.extend_from_slice(b"localhost");
    packet.extend_from_slice(&port.to_be_bytes());
    packet.extend_from_slice(b"udp");
    client.send_to(&packet, relay).await.unwrap();
    let mut buf = [0; 128];
    let (n, peer) = timeout(Duration::from_secs(3), upstream.recv_from(&mut buf))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(&buf[..n], b"udp");
    upstream.send_to(b"answer", peer).await.unwrap();
    let (n, _) = timeout(Duration::from_secs(3), client.recv_from(&mut buf))
        .await
        .unwrap()
        .unwrap();
    assert!(buf[..n].ends_with(b"answer"));
    let intents = observer.0.lock().unwrap().clone();
    assert!(intents.iter().any(|i| i.protocol == SystemFlowProtocol::Udp
        && i.hostname.as_deref() == Some("localhost")
        && i.remote.port() == port));
    runtime.quiesce_final();
    client.send_to(&packet, relay).await.unwrap();
    assert!(
        timeout(Duration::from_millis(80), upstream.recv_from(&mut buf))
            .await
            .is_err()
    );
    drop(tx);
    runtime.shutdown().await;
}

#[tokio::test]
async fn ipv6_tcp_and_udp_register_ipv6_tuples() {
    let observer = Arc::new(Recorder::default());
    let mut healthy = connected(0);
    if let RuntimeHealth::Connected { path, .. } = &mut healthy {
        path.ipv6_available = true;
    }
    let (_tx, rx) = watch::channel(healthy);
    let mut runtime = SystemProxyRuntime::start_with_authorizer(&profile(), rx, observer.clone())
        .await
        .unwrap();
    runtime.activate_final();
    let origin = TcpListener::bind("[::1]:0").await.unwrap();
    let remote = origin.local_addr().unwrap();
    let origin_task = tokio::spawn(echo(origin));
    let services = runtime.services(&profile());
    let cancel = CancellationToken::new();
    let mut stream = services
        .dialer
        .connect(
            TcpTarget::address(remote),
            Instant::now() + Duration::from_secs(3),
            &cancel,
            FlowClass::Business,
        )
        .await
        .unwrap();
    stream.write_all(b"ipv6").await.unwrap();
    let mut bytes = [0; 4];
    stream.read_exact(&mut bytes).await.unwrap();
    assert_eq!(&bytes, b"ipv6");
    origin_task.await.unwrap();
    let udp_origin = UdpSocket::bind("[::1]:0").await.unwrap();
    let udp_target = udp_origin.local_addr().unwrap();
    let udp = services
        .udp
        .unwrap()
        .open(&cancel, Instant::now() + Duration::from_secs(3))
        .await
        .unwrap();
    udp.send(&TcpTarget::address(udp_target), b"v6udp")
        .await
        .unwrap();
    let mut data = [0; 32];
    let (n, peer) = timeout(Duration::from_secs(3), udp_origin.recv_from(&mut data))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(&data[..n], b"v6udp");
    udp_origin.send_to(b"v6reply", peer).await.unwrap();
    let (received, data) = timeout(Duration::from_secs(3), udp.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(received.socket_address(), Some(udp_target));
    assert_eq!(&data[..], b"v6reply");
    assert!(
        observer
            .0
            .lock()
            .unwrap()
            .iter()
            .all(|intent| intent.local.is_ipv6()
                && intent.remote.is_ipv6()
                && intent.hostname.is_none())
    );
    runtime.shutdown().await;
}
