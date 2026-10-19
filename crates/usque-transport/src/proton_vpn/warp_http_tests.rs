//! Private dialer only: no OS DNS, OS socket, WARP service or public API.
use super::*;
use crate::{
    RuntimeHealth,
    netstack::RuntimePath,
    tcp::{DialError, FlowClass, TcpDialer, TcpStream, TcpTarget},
};
use bytes::Bytes;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU64, Ordering},
};
use tokio::{sync::watch, time::Instant};

struct PrivateDialer {
    calls: Mutex<Vec<(String, FlowClass)>>,
    generation: AtomicU64,
    failure: DialError,
}

#[async_trait]
impl TcpDialer for PrivateDialer {
    fn session_generation(&self) -> Option<u64> {
        match self.generation.load(Ordering::Acquire) {
            0 => None,
            generation => Some(generation),
        }
    }

    async fn connect(
        &self,
        target: TcpTarget,
        _: Instant,
        _: &CancellationToken,
        class: FlowClass,
    ) -> Result<TcpStream, DialError> {
        self.calls
            .lock()
            .unwrap()
            .push((target.authority().into(), class));
        Err(self.failure)
    }
}

fn connected(reconnect_count: u32) -> RuntimeHealth {
    RuntimeHealth::Connected {
        path: RuntimePath {
            transport: usque_core::Transport::Http3,
            endpoint_family: usque_core::AddressFamily::Ipv4,
            ipv4_available: true,
            ipv6_available: false,
        },
        reconnect_count,
    }
}

fn network(
    generation: u64,
    failure: DialError,
) -> (
    WarpHttp,
    Arc<PrivateDialer>,
    watch::Sender<RuntimeHealth>,
    CancellationToken,
) {
    let dialer = Arc::new(PrivateDialer {
        calls: Mutex::new(Vec::new()),
        generation: AtomicU64::new(generation),
        failure,
    });
    let (health, receiver) = watch::channel(connected(7));
    let cancel = CancellationToken::new();
    (
        WarpHttp::new(InternalNetwork::for_streams(
            dialer.clone(),
            receiver,
            cancel.clone(),
        )),
        dialer,
        health,
        cancel,
    )
}

fn request() -> InternalRequest<'static> {
    InternalRequest {
        url: "https://vpn-api.proton.me/vpn/v1/certificate",
        method: http::Method::POST,
        headers: vec![],
        body: Bytes::from_static(b"{}"),
        limit: 1024,
        ipv6: None,
    }
}

#[tokio::test]
async fn runtime_https_uses_only_the_supplied_private_dialer_without_fallback() {
    for (failure, expected) in [
        (DialError::Refused, InternalHttpError::Connect),
        (DialError::Timeout, InternalHttpError::Timeout),
        (DialError::Cancelled, InternalHttpError::Cancelled),
    ] {
        let (http, dialer, _health, _cancel) = network(1, failure);
        assert_eq!(
            http.exchange(request(), &CancellationToken::new())
                .await
                .err(),
            Some(expected)
        );
        assert_eq!(
            *dialer.calls.lock().unwrap(),
            vec![("vpn-api.proton.me:443".into(), FlowClass::Business)]
        );
    }
}

#[tokio::test]
async fn stream_and_packet_reconnects_invalidate_the_runtime_api_lease() {
    for stream_generation in [0, 1] {
        let (http, dialer, health, cancel) = network(stream_generation, DialError::Refused);
        let original = http.generation();
        assert!(original.is_some());
        assert!(http.current(original));
        assert!(!http.current(None));
        if stream_generation == 0 {
            health.send_replace(connected(8));
        } else {
            dialer.generation.store(2, Ordering::Release);
        }
        assert!(!http.current(original));
        let replacement = http.generation();
        assert!(http.current(replacement));
        let RuntimeHealth::Connected { path, .. } = connected(8) else {
            unreachable!()
        };
        health.send_replace(RuntimeHealth::Reconnecting {
            last_path: path,
            attempt: 1,
            reconnect_count: 8,
            reason: "synthetic reconnect".into(),
            failure: usque_core::TransportFailure::new(
                usque_core::TransportFailureCode::H3ConnectionClosed,
                usque_core::TransportStage::SocketConnect,
            ),
        });
        assert!(!http.current(replacement));
        assert_eq!(
            http.exchange(request(), &CancellationToken::new())
                .await
                .err(),
            Some(InternalHttpError::Connect)
        );
        assert!(dialer.calls.lock().unwrap().is_empty());
        health.send_replace(connected(8));
        assert!(http.current(http.generation()));
        cancel.cancel();
        assert!(!http.current(http.generation()));
    }
}

#[tokio::test]
async fn cancelled_runtime_request_never_dials() {
    let (http, dialer, _health, _cancel) = network(1, DialError::Refused);
    let cancel = CancellationToken::new();
    cancel.cancel();
    assert_eq!(
        http.exchange(request(), &cancel).await.err(),
        Some(InternalHttpError::Cancelled)
    );
    assert!(dialer.calls.lock().unwrap().is_empty());
}
