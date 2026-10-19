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
