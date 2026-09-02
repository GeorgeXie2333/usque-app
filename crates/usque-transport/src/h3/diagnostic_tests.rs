use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

use super::*;
use crate::NetworkProbeResult;
use crate::SocketHandle;
use usque_core::MasqueKeyPair;

struct Lease(Arc<AtomicUsize>);
impl Drop for Lease {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}
struct Protector {
    generation: AtomicU64,
    leases: Arc<AtomicUsize>,
}
impl Protector {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            generation: AtomicU64::new(7),
            leases: Arc::new(AtomicUsize::new(0)),
        })
    }
}
#[async_trait::async_trait]
impl SocketProtector for Protector {
    fn protect(&self, _: SocketHandle) -> Result<(), String> {
        panic!("probe requires exact generation")
    }
    async fn protect_for_target_generation(
        &self,
        _: SocketHandle,
        target: SocketAddr,
        protocol: DirectProtocol,
        generation: u64,
    ) -> Result<DirectEgressLease, String> {
        assert!(target.ip().is_loopback());
        assert_eq!(protocol, DirectProtocol::Udp);
        assert_eq!(generation, self.generation.load(Ordering::SeqCst));
        self.leases.fetch_add(1, Ordering::SeqCst);
        Ok(DirectEgressLease::hold_for_generation(
            Lease(self.leases.clone()),
            generation,
        ))
    }
    fn network_generation(&self) -> Option<u64> {
        Some(self.generation.load(Ordering::SeqCst))
    }
}

fn identities() -> (MasqueTlsIdentity, MasqueTlsIdentity) {
    let client = MasqueKeyPair::generate();
    let server = MasqueKeyPair::generate();
    let client_identity = MasqueTlsIdentity::new(
        client.private_sec1_der().unwrap(),
        &server.public_spki_der().unwrap(),
        Ipv4Addr::new(172, 16, 0, 2),
        "2001:db8::2".parse().unwrap(),
    )
    .unwrap();
    let server_identity = MasqueTlsIdentity::new(
        server.private_sec1_der().unwrap(),
        &client.public_spki_der().unwrap(),
        Ipv4Addr::new(172, 16, 0, 3),
        "2001:db8::3".parse().unwrap(),
    )
    .unwrap();
    (client_identity, server_identity)
}

#[tokio::test]
async fn deep_h3_handshake_has_no_http_streams_and_releases_exact_lease() {
    let (client_identity, server_identity) = identities();
    let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let address = socket.local_addr().unwrap();
    let server = AbortOnDropHandle::new(tokio::spawn(async move {
        let mut incoming = vec![0; 65536];
        let (length, peer) = socket.recv_from(&mut incoming).await.unwrap();
        let (mut config, _) = quic_config(&server_identity, INITIAL_SAFE_UDP_PAYLOAD).unwrap();
        let mut connection = quiche::accept_with_buf_factory::<H3BufferFactory>(
            &quiche::ConnectionId::from_ref(&[0x51; CONNECTION_ID_LENGTH]),
            None,
            address,
            peer,
            &mut config,
        )
        .unwrap();
        connection
            .recv(
                &mut incoming[..length],
                quiche::RecvInfo {
                    from: peer,
                    to: address,
                },
            )
            .unwrap();
        let mut outgoing = vec![0; 65536];
        loop {
            assert!(
                connection.readable().next().is_none(),
                "Doctor must not create CONNECT-IP or any HTTP stream"
            );
            assert_eq!(connection.dgram_recv_queue_len(), 0);
            if connection.is_established() {
                return;
            }
            while let Ok((length, info)) = connection.send(&mut outgoing) {
                socket.send_to(&outgoing[..length], info.to).await.unwrap();
            }
            let (length, from) = socket.recv_from(&mut incoming).await.unwrap();
            connection
                .recv(
                    &mut incoming[..length],
                    quiche::RecvInfo { from, to: address },
                )
                .unwrap();
        }
    }));
    let protector = Protector::new();
    let result = crate::probe_h3_handshake(
        address,
        "probe.test",
        &client_identity,
        protector.clone(),
        CancellationToken::new(),
    )
    .await;
    assert!(
        matches!(result, NetworkProbeResult::Passed { .. }),
        "{result:?}"
    );
    assert_eq!(protector.leases.load(Ordering::SeqCst), 0);
    timeout(Duration::from_secs(1), server)
        .await
        .unwrap()
        .unwrap();
}

#[tokio::test]
async fn deep_h3_cancel_and_generation_change_clean_the_socket_before_return() {
    for changed in [false, true] {
        let (identity, _) = identities();
        let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let address = socket.local_addr().unwrap();
        let protector = Protector::new();
        let cancel = CancellationToken::new();
        let task_protector = protector.clone();
        let task_cancel = cancel.clone();
        let task = AbortOnDropHandle::new(tokio::spawn(async move {
            crate::probe_h3_handshake(
                address,
                "probe.test",
                &identity,
                task_protector,
                task_cancel,
            )
            .await
        }));
        timeout(Duration::from_secs(1), socket.recv_from(&mut [0u8; 2048]))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(protector.leases.load(Ordering::SeqCst), 1);
        if changed {
            protector.generation.fetch_add(1, Ordering::SeqCst);
        } else {
            cancel.cancel();
        }
        let result = timeout(Duration::from_secs(1), task)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            result,
            if changed {
                NetworkProbeResult::NetworkChanged
            } else {
                NetworkProbeResult::Cancelled
            }
        );
        assert_eq!(protector.leases.load(Ordering::SeqCst), 0);
    }
}

#[tokio::test]
async fn deep_h3_timeout_is_bounded_and_does_not_leave_a_lease() {
    let (identity, _) = identities();
    let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let protector = Protector::new();
    let started = Instant::now();
    let result = crate::probe_h3_handshake(
        socket.local_addr().unwrap(),
        "probe.test",
        &identity,
        protector.clone(),
        CancellationToken::new(),
    )
    .await;
    assert_eq!(result, NetworkProbeResult::TimedOut);
    assert!(started.elapsed() < Duration::from_secs(4));
    assert_eq!(protector.leases.load(Ordering::SeqCst), 0);
}
