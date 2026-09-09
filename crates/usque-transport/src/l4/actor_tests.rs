//! Offline interoperability with the locked quiche peer, no platform networking.
use super::actor::*;
use super::stream::{Flow, FlowState};
use super::{BufferBudget, L4Metrics, Limits};
use crate::h3::tests::{advance_test_pair, test_quic_pair_with_config};
use crate::h3_buffer::H3BufferFactory;
use crate::tcp::{DialError, TcpStream, TcpTarget};
use quiche::h3::NameValue;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::{Notify, Semaphore, oneshot};
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

struct Peer {
    client: quiche::Connection<H3BufferFactory>,
    server: quiche::Connection<H3BufferFactory>,
    h3: quiche::h3::Connection,
    peer: quiche::h3::Connection,
    actor: L4Actor,
}

#[test]
fn fixed_upstream_semantic_vectors_match_authority_and_identity_contracts() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("../../tests/fixtures/l4/contract.json")).unwrap();
    assert_eq!(
        fixture["source_commit"],
        "6aa03fc97d12848dce34eedbd187fb1077b5d1ea"
    );
    assert_eq!(fixture["consumer_sni"], usque_core::CONSUMER_L4_SNI);
    assert_eq!(fixture["zero_trust_sni"], usque_core::ZERO_TRUST_L4_SNI);
    for target in fixture["targets"].as_array().unwrap() {
        let actual = TcpTarget::new(
            target["host"].as_str().unwrap(),
            target["port"].as_u64().unwrap() as u16,
        )
        .unwrap();
        assert_eq!(actual.authority(), target["authority"].as_str().unwrap());
    }
    assert_eq!(fixture["datagrams_required"], false);
    assert_eq!(
        fixture["request_pseudo_headers"],
        serde_json::json!([":method", ":authority"])
    );
}

proptest::proptest! {
    #[test]
    fn externally_supplied_status_and_authority_are_panic_free(bytes in proptest::collection::vec(proptest::prelude::any::<u8>(), 0..2048)) {
        let _ = super::actor::response_status(&[quiche::h3::Header::new(b":status", &bytes)]);
        if let Ok(value) = std::str::from_utf8(&bytes) { let _ = TcpTarget::new(value, 443); }
        let _ = crate::split_dns::validate_query_bytes(&bytes);
        if let Some(meta) = crate::direct_gateway::NatPacket::parse(&bytes) {
            let _ = super::tun_wire::valid_transport(&bytes, &meta);
            let _ = super::tun_wire::TcpReset::from_packet(&bytes, &meta);
        }
    }
}

impl Peer {
    fn new() -> Self {
        let (mut client, mut server, _, _) = test_quic_pair_with_config(
            "127.0.0.1:12340".parse().unwrap(),
            "127.0.0.1:44330".parse().unwrap(),
            |client, server| {
                Limits::platform().configure(client);
                client.enable_pacing(false);
                server.enable_pacing(false);
                server.enable_dgram(false, 0, 0);
                server.set_initial_max_streams_bidi(16);
            },
        );
        for _ in 0..8 {
            advance_test_pair(&mut client, &mut server).unwrap();
        }
        assert!(client.is_established() && server.is_established());
        let config = quiche::h3::Config::new().unwrap();
        let h3 = quiche::h3::Connection::with_transport(&mut client, &config).unwrap();
        let peer = quiche::h3::Connection::with_transport(&mut server, &config).unwrap();
        let metrics = Arc::new(L4Metrics::default());
        let budget = Arc::new(BufferBudget::new(
            16 << 20,
            metrics.clone(),
            Arc::new(Notify::new()),
        ));
        Self {
            client,
            server,
            h3,
            peer,
            actor: L4Actor::new(budget, metrics, Arc::new(Notify::new())),
        }
    }

    fn pump(&mut self) {
        for _ in 0..8 {
            advance_test_pair(&mut self.client, &mut self.server).unwrap();
            self.actor
                .pump(&mut self.h3, &mut self.client, true)
                .unwrap();
        }
        advance_test_pair(&mut self.client, &mut self.server).unwrap();
    }

    fn open(
        &mut self,
        authority: &str,
    ) -> (Arc<Flow>, oneshot::Receiver<Result<TcpStream, DialError>>) {
        let (reply, response) = oneshot::channel();
        let target = if authority == "v6" {
            TcpTarget::address("[2001:db8::1]:443".parse().unwrap())
        } else {
            TcpTarget::new(authority, 443).unwrap()
        };
        let flow = Arc::new(Flow {
            session_generation: 1,
            state: Mutex::new(FlowState::default()),
            target,
            deadline: Instant::now() + Duration::from_secs(10),
            cancellation: CancellationToken::new(),
            budget: self.actor.budget.clone(),
            wake: self.actor.handle.wake.clone(),
            metrics: self.actor.budget.metrics.clone(),
            counters: Arc::default(),
            reply: Mutex::new(Some(reply)),
            started: Instant::now(),
            _active: Arc::new(Semaphore::new(1)).try_acquire_owned().unwrap(),
            _relay: self
                .actor
                .budget
                .reserve(2 * Limits::platform().relay)
                .unwrap(),
            _pending: Mutex::new(Some(
                Arc::new(Semaphore::new(1)).try_acquire_owned().unwrap(),
            )),
            delivered: AtomicBool::new(false),
            stream_id: AtomicU64::new(u64::MAX),
        });
        self.actor.budget.metrics.update(|m| m.active_flows += 1);
        assert!(self.actor.handle.enqueue(flow.clone()).is_ok());
        self.pump();
        (flow, response)
    }

    fn request(&mut self) -> u64 {
        loop {
            match self.peer.poll(&mut self.server).unwrap() {
                (id, quiche::h3::Event::Headers { list, .. }) => {
                    assert_eq!(
                        list.len(),
                        2,
                        "classic CONNECT has no capsule/protocol/path/auth headers"
                    );
                    assert_eq!(list[0].name(), b":method");
                    assert_eq!(list[0].value(), b"CONNECT");
                    assert_eq!(list[1].name(), b":authority");
                    return id;
                }
                (_, quiche::h3::Event::PriorityUpdate) => {}
                event => panic!("unexpected peer event {event:?}"),
            }
        }
    }

    fn respond(&mut self, id: u64, code: &[u8]) {
        self.peer
            .send_response(
                &mut self.server,
                id,
                &[quiche::h3::Header::new(b":status", code)],
                false,
            )
            .unwrap();
    }
}

#[tokio::test]
async fn headers_data_fin_same_flight_preserves_bytes_and_ipv6_binding() {
    let mut peer = Peer::new();
    let (_, response) = peer.open("v6");
    let id = peer.request();
    peer.respond(id, b"103");
    peer.peer
        .send_additional_headers(
            &mut peer.server,
            id,
            &[quiche::h3::Header::new(b":status", b"200")],
            false,
            false,
        )
        .unwrap();
    peer.peer
        .send_body(&mut peer.server, id, b"early data", true)
        .unwrap();
    peer.pump();
    let mut stream = response.await.unwrap().unwrap();
    assert_eq!(stream.local_addr().unwrap(), "[::]:0".parse().unwrap());
    assert!(peer.actor.handle.verified.load(Ordering::Acquire));
    let mut received = Vec::new();
    tokio::time::timeout(Duration::from_secs(1), stream.read_to_end(&mut received))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(received, b"early data");
}

#[tokio::test]
async fn rejection_and_cancellation_are_stream_local() {
    let mut peer = Peer::new();
    let (_, refused) = peer.open("refused.test");
    let first = peer.request();
    let (good, accepted) = peer.open("allowed.test");
    let second = peer.request();
    peer.respond(first, b"403");
    peer.respond(second, b"200");
    peer.pump();
    assert!(matches!(
        refused.await.unwrap(),
        Err(DialError::Rejected(403))
    ));
    let mut stream = accepted.await.unwrap().unwrap();
    stream.write_all(b"hello").await.unwrap();
    peer.pump();
    let mut buf = [0; 5];
    while let Ok((id, event)) = peer.peer.poll(&mut peer.server) {
        if id == second && matches!(event, quiche::h3::Event::Data) {
            assert_eq!(
                peer.peer.recv_body(&mut peer.server, id, &mut buf).unwrap(),
                5
            );
        }
    }
    assert_eq!(&buf, b"hello");
    good.cancellation.cancel();
    peer.pump();
    assert!(stream.write_all(b"never replay").await.is_err());
    assert!(!peer.actor.handle.closed.load(Ordering::Acquire));
}

#[tokio::test]
async fn owned_reads_reuse_pool_without_adapter_copy_and_keep_final_data_before_eof() {
    let mut peer = Peer::new();
    let (_, response) = peer.open("download.test");
    let id = peer.request();
    peer.respond(id, b"200");
    peer.pump();
    let mut stream = response.await.unwrap().unwrap();
    assert!(stream.has_owned_read());
    peer.actor.record_wakeup();
    peer.pump();
    assert_eq!(
        peer.actor
            .budget
            .metrics
            .performance
            .sample()
            .actor_no_progress_wakeups,
        1
    );
    let payload = vec![0x53; 1024];
    let perf = peer.actor.budget.metrics.performance.clone();
    for index in 0..64 {
        peer.peer
            .send_body(&mut peer.server, id, &payload, index == 63)
            .unwrap();
        peer.pump();
        let bytes = std::future::poll_fn(|cx| stream.poll_read_owned(cx))
            .await
            .unwrap();
        assert_eq!(bytes, payload);
        drop(bytes);
    }
    peer.pump();
    assert!(
        std::future::poll_fn(|cx| stream.poll_read_owned(cx))
            .await
            .unwrap()
            .is_empty()
    );
    let value = perf.sample();
    assert_eq!(value.h3_read_bytes, 64 * 1024);
    assert_eq!(value.adapter_copied_bytes, 0);
    assert!(value.receive_pool_hits >= 63);
    assert!(value.receive_pool_allocations <= 2);
    drop(stream);
    drop(peer);
    assert_eq!(perf.sample().receive_pool_live_bytes, 0);
}

#[tokio::test]
async fn zero_copy_remainder_fin_and_drop_release_budget() {
    let mut peer = Peer::new();
    let (flow, response) = peer.open("bulk.test");
    let id = peer.request();
    peer.respond(id, b"200");
    peer.pump();
    let mut stream = response.await.unwrap().unwrap();
    let payload = vec![0xa7; super::FLOW_BUFFER];
    stream.write_all(&payload).await.unwrap();
    // Request FIN before all partial writes have been submitted.
    flow.lock().fin_requested = true;
    let mut actual = Vec::new();
    let mut finished = false;
    for _ in 0..128 {
        peer.pump();
        while let Ok((sid, event)) = peer.peer.poll(&mut peer.server) {
            assert_eq!(sid, id);
            match event {
                quiche::h3::Event::Data => {
                    let mut bytes = [0u8; 4096];
                    while let Ok(n) = peer.peer.recv_body(&mut peer.server, id, &mut bytes) {
                        if n == 0 {
                            break;
                        }
                        actual.extend_from_slice(&bytes[..n]);
                    }
                }
                quiche::h3::Event::Finished => finished = true,
                _ => {}
            }
        }
        if finished {
            break;
        }
    }
    assert_eq!(actual, payload);
    assert!(finished);
    let metrics = peer.actor.budget.metrics.clone();
    drop(stream);
    drop(flow);
    drop(peer);
    assert_eq!(metrics.snapshot().buffer_bytes, 0);
    assert_eq!(metrics.snapshot().active_flows, 0);
}

#[tokio::test]
async fn goaway_drains_existing_stream_and_rejects_new_without_extending_deadline() {
    let mut peer = Peer::new();
    let (_, response) = peer.open("old.test");
    let id = peer.request();
    peer.respond(id, b"200");
    peer.pump();
    let _stream = response.await.unwrap().unwrap();
    peer.peer.send_goaway(&mut peer.server, id + 4).unwrap();
    peer.pump();
    let deadline = peer.actor.drain_deadline.unwrap();
    assert!(peer.actor.handle.draining.load(Ordering::Acquire));
    peer.peer.send_goaway(&mut peer.server, id + 4).unwrap();
    peer.pump();
    assert_eq!(peer.actor.drain_deadline, Some(deadline));
    assert!(peer.actor.handle.verified.load(Ordering::Acquire));
}

#[tokio::test]
async fn shared_budget_release_wakes_both_actors_and_counts_ack_ownership() {
    let metrics = Arc::new(L4Metrics::default());
    let budget = Arc::new(BufferBudget::new(
        16,
        metrics.clone(),
        Arc::new(Notify::new()),
    ));
    let bytes = budget.copy(&[3; 16]).unwrap();
    let retained_by_quic = bytes.clone();
    drop(bytes);
    assert_eq!(budget.available(), 0);
    let first_notify = Arc::new(Notify::new());
    let second_notify = Arc::new(Notify::new());
    let first_waiter = super::budget_wait::BudgetWaiter::new(&budget, Some(first_notify.clone()));
    let second_waiter = super::budget_wait::BudgetWaiter::new(&budget, Some(second_notify.clone()));
    first_waiter.arm(None);
    second_waiter.arm(None);
    assert_eq!(budget.available(), 0);
    let first = first_notify.notified();
    let second = second_notify.notified();
    tokio::pin!(first, second);
    first.as_mut().enable();
    second.as_mut().enable();
    drop(retained_by_quic);
    tokio::time::timeout(Duration::from_secs(1), async {
        first.await;
        second.await;
    })
    .await
    .unwrap();
    assert_eq!(budget.available(), 16);
    assert_eq!(metrics.snapshot().buffer_bytes, 0);
}

#[test]
fn full_admission_retains_working_space_for_data_progress() {
    let metrics = Arc::new(L4Metrics::default());
    let budget = Arc::new(BufferBudget::new(1 << 20, metrics, Arc::new(Notify::new())));
    let mut admitted = Vec::new();
    while let Some(lease) = budget.reserve_admission(64 << 10) {
        admitted.push(lease);
    }
    assert_eq!(admitted.len(), 14);
    assert_eq!(budget.available(), 128 << 10);
    let bytes = budget
        .copy(&vec![0; 64 << 10])
        .expect("DATA retains progress capacity");
    assert!(budget.reserve_admission(1).is_none());
    drop(bytes);
    drop(admitted);
    assert_eq!(budget.available(), 1 << 20);
}

#[tokio::test]
async fn a_budget_blocked_stream_writer_resumes_without_a_network_packet() {
    use std::task::{Context, Wake, Waker};
    use tokio::io::AsyncWrite;
    struct Counter(std::sync::atomic::AtomicUsize);
    impl Wake for Counter {
        fn wake(self: Arc<Self>) {
            self.0.fetch_add(1, Ordering::Relaxed);
        }
    }
    let mut peer = Peer::new();
    let (_, response) = peer.open("blocked.test");
    let id = peer.request();
    peer.respond(id, b"200");
    peer.pump();
    let mut stream = response.await.unwrap().unwrap();
    let budget = peer.actor.budget.clone();
    let hold = budget.reserve(budget.available()).unwrap();
    let counter = Arc::new(Counter(std::sync::atomic::AtomicUsize::new(0)));
    let waker = Waker::from(counter.clone());
    assert!(
        std::pin::Pin::new(&mut stream)
            .poll_write(&mut Context::from_waker(&waker), b"resume")
            .is_pending()
    );
    drop(hold);
    assert!(counter.0.load(Ordering::Relaxed) > 0);
    // Deliberately do not pump QUIC between capacity release and this write.
    assert!(matches!(
        std::pin::Pin::new(&mut stream).poll_write(&mut Context::from_waker(&waker), b"resume"),
        std::task::Poll::Ready(Ok(6))
    ));
}
