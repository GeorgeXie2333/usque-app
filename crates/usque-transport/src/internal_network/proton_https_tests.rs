use super::*;
use crate::netstack::RuntimePath;
use async_trait::async_trait;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use tokio::io::{AsyncReadExt, AsyncWriteExt, DuplexStream};

struct Dialer {
    generation: AtomicU64,
    calls: AtomicUsize,
}
#[async_trait]
impl TcpDialer for Dialer {
    fn session_generation(&self) -> Option<u64> {
        Some(self.generation.load(Ordering::SeqCst))
    }
    async fn connect(
        &self,
        _: TcpTarget,
        _: Instant,
        _: &CancellationToken,
        _: FlowClass,
    ) -> Result<TcpStream, DialError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Err(DialError::Refused)
    }
}

async fn fixture() -> (
    Session,
    DuplexStream,
    Arc<Dialer>,
    watch::Sender<RuntimeHealth>,
) {
    let dialer = Arc::new(Dialer {
        generation: AtomicU64::new(1),
        calls: AtomicUsize::new(0),
    });
    let (health, receiver) = watch::channel(RuntimeHealth::Connected {
        path: RuntimePath {
            transport: usque_core::Transport::Http3,
            endpoint_family: usque_core::AddressFamily::Ipv4,
            ipv4_available: true,
            ipv6_available: false,
        },
        reconnect_count: 0,
    });
    let network = InternalNetwork::for_streams(dialer.clone(), receiver, CancellationToken::new());
    let session = Session::new(network);
    let (client, server) = tokio::io::duplex(4096);
    let connection = session
        .handshake(client, session.network.control_generation().unwrap())
        .await
        .unwrap();
    *session.idle.lock().await = Some(connection);
    (session, server, dialer, health)
}

fn request() -> InternalRequest<'static> {
    InternalRequest {
        url: "https://vpn-api.proton.me/vpn/v1/certificate",
        method: http::Method::POST,
        headers: vec![],
        body: Bytes::from_static(b"{}"),
        limit: 32,
        ipv6: None,
    }
}

async fn read_request(server: &mut DuplexStream) {
    let mut bytes = Vec::new();
    while !bytes.ends_with(b"\r\n\r\n") {
        bytes.push(server.read_u8().await.unwrap());
        assert!(bytes.len() < 4096);
    }
    assert!(
        !String::from_utf8(bytes)
            .unwrap()
            .to_ascii_lowercase()
            .contains("connection: close")
    );
    let mut body = [0; 2];
    server.read_exact(&mut body).await.unwrap();
    assert_eq!(&body, b"{}");
}

#[tokio::test]
async fn complete_responses_reuse_one_connection_and_shutdown_awaits_its_driver() {
    let (session, mut server, dialer, _health) = fixture().await;
    let peer = tokio::spawn(async move {
        for _ in 0..3 {
            read_request(&mut server).await;
            server
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n{}")
                .await
                .unwrap();
        }
        let mut byte = [0];
        assert_eq!(server.read(&mut byte).await.unwrap(), 0);
    });
    for _ in 0..3 {
        assert_eq!(
            &*session
                .exchange(request(), &CancellationToken::new())
                .await
                .unwrap()
                .body,
            b"{}"
        );
    }
    assert_eq!(dialer.calls.load(Ordering::SeqCst), 0);
    session.shutdown().await;
    peer.await.unwrap();
    assert!(session.idle.lock().await.is_none());
}

#[tokio::test]
async fn partial_encoded_oversized_and_close_responses_cannot_reenter_pool() {
    for reply in [
        b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n{".as_slice(),
        b"HTTP/1.1 200 OK\r\nContent-Encoding: gzip\r\nContent-Length: 2\r\n\r\n{}",
        b"HTTP/1.1 200 OK\r\nContent-Length: 999\r\n\r\n{}",
        b"HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Length: 2\r\n\r\n{}",
    ] {
        let (session, mut server, dialer, _health) = fixture().await;
        let peer = tokio::spawn(async move {
            read_request(&mut server).await;
            server.write_all(reply).await.unwrap();
        });
        let _ = session.exchange(request(), &CancellationToken::new()).await;
        peer.await.unwrap();
        assert!(session.idle.lock().await.is_none());
        assert_eq!(
            dialer.calls.load(Ordering::SeqCst),
            0,
            "submitted POST was replayed"
        );
        session.shutdown().await;
    }
}

#[tokio::test(start_paused = true)]
async fn idle_timeout_and_generation_changes_discard_old_connection_before_submission() {
    for change_generation in [false, true] {
        let (session, mut server, dialer, _health) = fixture().await;
        if change_generation {
            dialer.generation.store(2, Ordering::SeqCst);
        } else {
            tokio::time::advance(IDLE_TTL).await;
        }
        assert_eq!(
            session
                .exchange(request(), &CancellationToken::new())
                .await
                .err(),
            Some(InternalHttpError::Connect)
        );
        assert_eq!(
            dialer.calls.load(Ordering::SeqCst),
            usize::from(!change_generation)
        );
        assert!(session.idle.lock().await.is_none());
        let mut byte = [0];
        assert_eq!(server.read(&mut byte).await.unwrap(), 0);
    }
}

#[tokio::test]
async fn interrupted_post_is_never_replayed_and_other_origins_are_rejected() {
    let (session, mut server, dialer, _health) = fixture().await;
    for url in [
        "https://vpn-api.proton.me.evil.test/",
        "http://vpn-api.proton.me/",
        "https://user@vpn-api.proton.me/",
        "https://vpn-api.proton.me:444/",
    ] {
        let mut invalid = request();
        invalid.url = url;
        assert_eq!(
            session
                .exchange(invalid, &CancellationToken::new())
                .await
                .err(),
            Some(InternalHttpError::Protocol)
        );
    }
    let peer = tokio::spawn(async move {
        read_request(&mut server).await;
    });
    assert_eq!(
        session
            .exchange(request(), &CancellationToken::new())
            .await
            .err(),
        Some(InternalHttpError::Protocol)
    );
    peer.await.unwrap();
    assert_eq!(dialer.calls.load(Ordering::SeqCst), 0);
    assert!(session.idle.lock().await.is_none());
}

#[tokio::test]
async fn cancellation_during_response_closes_owned_driver() {
    let (session, mut server, dialer, _health) = fixture().await;
    let cancel = CancellationToken::new();
    let peer_cancel = cancel.clone();
    let peer = tokio::spawn(async move {
        read_request(&mut server).await;
        server
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n{")
            .await
            .unwrap();
        peer_cancel.cancel();
        let mut byte = [0];
        assert_eq!(server.read(&mut byte).await.unwrap(), 0);
    });
    assert_eq!(
        session.exchange(request(), &cancel).await.err(),
        Some(InternalHttpError::Cancelled)
    );
    peer.await.unwrap();
    assert_eq!(dialer.calls.load(Ordering::SeqCst), 0);
    assert!(session.idle.lock().await.is_none());
}

struct MemoryStream(DuplexStream);
impl tokio::io::AsyncRead for MemoryStream {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.0).poll_read(cx, buf)
    }
}
impl tokio::io::AsyncWrite for MemoryStream {
    fn poll_write(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        bytes: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        std::pin::Pin::new(&mut self.0).poll_write(cx, bytes)
    }
    fn poll_flush(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.0).poll_flush(cx)
    }
    fn poll_shutdown(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.0).poll_shutdown(cx)
    }
}
impl crate::tcp::TcpIo for MemoryStream {
    fn local_addr(&self) -> std::io::Result<std::net::SocketAddr> {
        Ok("192.0.2.1:40000".parse().unwrap())
    }
}
struct RaceDialer(std::sync::Mutex<Vec<CancellationToken>>);
#[async_trait]
impl TcpDialer for RaceDialer {
    async fn connect(
        &self,
        target: TcpTarget,
        _: Instant,
        cancel: &CancellationToken,
        class: FlowClass,
    ) -> Result<TcpStream, DialError> {
        self.0.lock().unwrap().push(cancel.clone());
        assert_eq!(class, FlowClass::Business);
        assert_eq!(target.socket_address().unwrap().port(), 443);
        if target.socket_address().unwrap().ip()
            == "198.51.100.1".parse::<std::net::IpAddr>().unwrap()
        {
            std::future::pending::<()>().await;
        }
        let (client, _server) = tokio::io::duplex(32);
        Ok(Box::new(MemoryStream(client)))
    }
}

#[tokio::test(start_paused = true)]
async fn api_dial_starts_before_stalled_aaaa_and_races_stalled_first_address() {
    for stalled_first in [false, true] {
        let (session, _server, _, _health) = fixture().await;
        let mut network = session.network.clone();
        let dialer = Arc::new(RaceDialer(Default::default()));
        network.dialer = dialer.clone();
        let resolution = if stalled_first {
            crate::dns::CandidateResolution::from_addresses(vec![
                "198.51.100.1".parse().unwrap(),
                "198.51.100.2".parse().unwrap(),
            ])
        } else {
            crate::dns::CandidateResolution::test_queries(
                async { Ok(vec!["198.51.100.2".parse().unwrap()]) },
                std::future::pending(),
            )
        };
        let started = Instant::now();
        let stream = network
            .connect_proton_candidates(
                resolution,
                &CancellationToken::new(),
                started + Duration::from_secs(5),
            )
            .await
            .unwrap();
        assert!(started.elapsed() <= Duration::from_millis(250));
        {
            let tokens = dialer.0.lock().unwrap();
            assert_eq!(tokens.len(), if stalled_first { 2 } else { 1 });
            assert!(!tokens.last().unwrap().is_cancelled());
            if stalled_first {
                assert!(tokens[0].is_cancelled());
            }
        }
        drop(stream);
        session.shutdown().await;
    }
}

#[tokio::test(start_paused = true)]
async fn api_dial_same_family_backup_waits_for_pending_aaaa_with_a_stalled_first_address() {
    let (session, _server, _, _health) = fixture().await;
    let mut network = session.network.clone();
    let dialer = Arc::new(RaceDialer(Default::default()));
    network.dialer = dialer.clone();
    let resolution = crate::dns::CandidateResolution::test_queries(
        async {
            Ok(vec![
                "198.51.100.1".parse().unwrap(),
                "198.51.100.2".parse().unwrap(),
            ])
        },
        std::future::pending(),
    );
    let started = Instant::now();
    let stream = network
        .connect_proton_candidates(
            resolution,
            &CancellationToken::new(),
            started + Duration::from_secs(5),
        )
        .await
        .unwrap();
    // The shared scheduler reserves its second slot until the other DNS family
    // finishes. Preserve and document that policy in this minimal recovery fix.
    assert_eq!(started.elapsed(), Duration::from_secs(4));
    {
        let tokens = dialer.0.lock().unwrap();
        assert_eq!(tokens.len(), 2);
        assert!(tokens[0].is_cancelled());
        assert!(!tokens[1].is_cancelled());
    }
    drop(stream);
    session.shutdown().await;
}

#[tokio::test(start_paused = true)]
async fn api_dial_cancellation_stops_all_candidates_on_its_private_network() {
    let (session, _server, _, _health) = fixture().await;
    let mut network = session.network.clone();
    let dialer = Arc::new(RaceDialer(Default::default()));
    network.dialer = dialer.clone();
    let cancel = CancellationToken::new();
    let stop = cancel.clone();
    let stop_task = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(100)).await;
        stop.cancel();
    });
    let resolution =
        crate::dns::CandidateResolution::from_addresses(vec!["198.51.100.1".parse().unwrap()]);
    assert_eq!(
        network
            .connect_proton_candidates(resolution, &cancel, Instant::now() + Duration::from_secs(5))
            .await
            .err(),
        Some(InternalHttpError::Cancelled)
    );
    stop_task.await.unwrap();
    {
        let tokens = dialer.0.lock().unwrap();
        assert_eq!(tokens.len(), 1);
        assert!(tokens[0].is_cancelled());
    }
    session.shutdown().await;
}
