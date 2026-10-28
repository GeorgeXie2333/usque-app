use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use windows_sys::Win32::{Foundation::WAIT_TIMEOUT, System::Threading::WaitForSingleObject};

#[derive(Default)]
struct IoStats {
    started: AtomicUsize,
    pending: AtomicUsize,
    received: AtomicUsize,
    dropped: AtomicUsize,
}

struct PendingSend(Arc<IoStats>);

impl Drop for PendingSend {
    fn drop(&mut self) {
        self.0.pending.fetch_sub(1, Ordering::SeqCst);
    }
}

struct FakeIo {
    outgoing: mpsc::Sender<Bytes>,
    incoming: mpsc::Receiver<Result<Bytes, TransportError>>,
    stats: Arc<IoStats>,
}

impl Drop for FakeIo {
    fn drop(&mut self) {
        self.stats.dropped.fetch_add(1, Ordering::SeqCst);
    }
}

impl PacketTransport for FakeIo {
    fn start_send(
        &self,
        packet: Bytes,
    ) -> impl Future<Output = Result<(), TransportError>> + Send + 'static + use<> {
        let outgoing = self.outgoing.clone();
        self.stats.started.fetch_add(1, Ordering::SeqCst);
        self.stats.pending.fetch_add(1, Ordering::SeqCst);
        let pending = PendingSend(Arc::clone(&self.stats));
        async move {
            let _pending = pending;
            outgoing
                .send(packet)
                .await
                .map_err(|_| TransportError::TunnelClosed)
        }
    }

    async fn receive(&mut self) -> Result<Bytes, TransportError> {
        let result = self
            .incoming
            .recv()
            .await
            .ok_or(TransportError::TunnelClosed)?;
        self.stats.received.fetch_add(1, Ordering::SeqCst);
        result
    }

    fn try_receive(&mut self) -> Result<Option<Bytes>, TransportError> {
        match self.incoming.try_recv() {
            Ok(result) => {
                self.stats.received.fetch_add(1, Ordering::SeqCst);
                result.map(Some)
            }
            Err(mpsc::error::TryRecvError::Empty) => Ok(None),
            Err(mpsc::error::TryRecvError::Disconnected) => Err(TransportError::TunnelClosed),
        }
    }
}

struct FakeChannels {
    io: FakeIo,
    sent: mpsc::Receiver<Bytes>,
    incoming: mpsc::Sender<Result<Bytes, TransportError>>,
    stats: Arc<IoStats>,
}

fn fake_io() -> FakeChannels {
    let (outgoing, sent) = mpsc::channel(1);
    let (received, incoming) = mpsc::channel(128);
    let stats = Arc::new(IoStats::default());
    FakeChannels {
        io: FakeIo {
            outgoing,
            incoming,
            stats: Arc::clone(&stats),
        },
        sent,
        incoming: received,
        stats,
    }
}

fn test_path() -> RuntimePath {
    RuntimePath {
        transport: usque_core::Transport::Http3,
        endpoint_family: usque_core::AddressFamily::Ipv4,
        ipv4_available: true,
        ipv6_available: true,
    }
}

// This guard is also effective if an assertion panics while a real blocking
// event waiter is alive. The fixture never starts an Agent or a TUN session.
struct PumpFixture {
    mapping: Arc<PacketSessionMapping>,
    cancellation: CancellationToken,
    tasks: Vec<JoinHandle<()>>,
    failure: watch::Receiver<Option<WindowsPumpFailure>>,
}

impl PumpFixture {
    fn start(io: FakeIo, mapping: Arc<PacketSessionMapping>) -> Self {
        let cancellation = CancellationToken::new();
        let (failure, observed) = watch::channel(None);
        let tasks = start_with_io(
            io,
            Arc::clone(&mapping),
            test_path,
            cancellation.clone(),
            failure,
        );
        Self {
            mapping,
            cancellation,
            tasks,
            failure: observed,
        }
    }

    async fn stop(&mut self) {
        self.cancellation.cancel();
        timeout(Duration::from_secs(3), stop_tasks(&mut self.tasks))
            .await
            .expect("cancellation must wake and join the actual native waiter");
        assert_eq!(Arc::strong_count(&self.mapping), 1);
    }
}

impl Drop for PumpFixture {
    fn drop(&mut self) {
        self.cancellation.cancel();
        self.mapping.signal_shutdown();
    }
}

fn mapping() -> Arc<PacketSessionMapping> {
    crate::windows_agent::tests::packet_mapping_fixture()
}

fn signal_upload(mapping: &PacketSessionMapping) {
    // SAFETY: this anonymous fixture owns the live notification event.
    assert_ne!(unsafe { SetEvent(mapping.agent_to_engine_event.0) }, 0);
}

fn upload(mapping: &PacketSessionMapping, packet: &[u8]) {
    assert!(
        mapping
            .ring()
            .try_push_preserving(PacketDirection::AgentToEngine, packet)
            .unwrap()
    );
    signal_upload(mapping);
}

fn fill_download(mapping: &PacketSessionMapping) -> usize {
    // An aligned 4096-byte record fills the ring exactly, so even a small next
    // packet must wait. The test transfers sole producer ownership to the pump.
    let packet = [0x22; 4092];
    let mut count = 0;
    while mapping
        .ring()
        .try_push_preserving(PacketDirection::EngineToAgent, &packet)
        .unwrap()
    {
        count += 1;
    }
    assert_eq!(count * 4096, mapping.ring().capacity() as usize);
    count
}

async fn wait_until(mut condition: impl FnMut() -> bool) {
    timeout(Duration::from_secs(3), async {
        while !condition() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("packet pump made no progress");
}

async fn next_download(mapping: &PacketSessionMapping) -> Vec<u8> {
    timeout(Duration::from_secs(3), async {
        loop {
            if let Some(packet) = mapping
                .ring()
                .try_pop(PacketDirection::EngineToAgent)
                .unwrap()
            {
                return packet;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("download must progress independently")
}

#[tokio::test]
async fn blocked_upload_keeps_downloading_and_retains_only_one_pending_packet() {
    let mapping = mapping();
    let FakeChannels {
        io,
        sent: _sent,
        incoming,
        stats,
    } = fake_io();
    io.outgoing
        .try_send(Bytes::from_static(b"occupied"))
        .unwrap();
    upload(&mapping, b"first upload");
    upload(&mapping, b"second upload");
    let mut pump = PumpFixture::start(io, mapping);
    wait_until(|| stats.pending.load(Ordering::SeqCst) == 1).await;
    incoming
        .send(Ok(Bytes::from_static(b"independent download")))
        .await
        .unwrap();
    assert_eq!(next_download(&pump.mapping).await, b"independent download");
    assert_eq!(stats.started.load(Ordering::SeqCst), 1);
    pump.stop().await;
    assert_eq!(stats.pending.load(Ordering::SeqCst), 0);
    assert_eq!(stats.dropped.load(Ordering::SeqCst), 1);
    assert!(pump.failure.borrow().is_none());
    // Consumer ownership transfers back only after the real pump has joined.
    assert_eq!(
        pump.mapping
            .ring()
            .try_pop(PacketDirection::AgentToEngine)
            .unwrap()
            .unwrap(),
        b"second upload"
    );
}

#[tokio::test]
async fn full_download_ring_keeps_uploading_then_retries_without_losing_order() {
    let mapping = mapping();
    let fillers = fill_download(&mapping);
    let FakeChannels {
        io,
        mut sent,
        incoming,
        stats,
    } = fake_io();
    let mut pump = PumpFixture::start(io, mapping);
    incoming
        .send(Ok(Bytes::from_static(b"pending download")))
        .await
        .unwrap();
    incoming
        .send(Ok(Bytes::from_static(b"next download")))
        .await
        .unwrap();
    wait_until(|| stats.received.load(Ordering::SeqCst) == 1).await;
    upload(&pump.mapping, b"independent upload");
    assert_eq!(
        timeout(Duration::from_secs(3), sent.recv())
            .await
            .unwrap()
            .unwrap(),
        b"independent upload"[..]
    );
    assert_eq!(
        stats.received.load(Ordering::SeqCst),
        1,
        "full ring must bound download admission"
    );
    for _ in 0..fillers {
        assert_eq!(next_download(&pump.mapping).await, [0x22; 4092]);
    }
    assert_eq!(next_download(&pump.mapping).await, b"pending download");
    assert_eq!(next_download(&pump.mapping).await, b"next download");
    pump.stop().await;
    assert!(pump.failure.borrow().is_none());
}

#[tokio::test]
async fn cancellation_drops_both_pending_owners_and_joins_the_mapping_waiter() {
    let mapping = mapping();
    let weak = Arc::downgrade(&mapping);
    fill_download(&mapping);
    let FakeChannels {
        io,
        sent: _sent,
        incoming,
        stats,
    } = fake_io();
    io.outgoing
        .try_send(Bytes::from_static(b"occupied"))
        .unwrap();
    upload(&mapping, b"pending upload");
    let mut pump = PumpFixture::start(io, mapping);
    incoming
        .send(Ok(Bytes::from_static(b"pending download")))
        .await
        .unwrap();
    wait_until(|| {
        stats.pending.load(Ordering::SeqCst) == 1 && stats.received.load(Ordering::SeqCst) == 1
    })
    .await;
    pump.stop().await;
    assert_eq!(stats.pending.load(Ordering::SeqCst), 0);
    assert_eq!(stats.dropped.load(Ordering::SeqCst), 1);
    assert!(
        incoming.is_closed(),
        "drop must release the transport receive half"
    );
    assert!(pump.failure.borrow().is_none());
    drop(pump);
    assert!(
        weak.upgrade().is_none(),
        "joined workers must release their mapping and events"
    );
}

#[tokio::test]
async fn abort_before_the_async_pump_is_polled_still_releases_native_waiter() {
    let FakeChannels {
        io,
        sent: _sent,
        incoming: _incoming,
        stats,
    } = fake_io();
    let mut pump = PumpFixture::start(io, mapping());
    // On this current-thread runtime no spawn has been polled yet. The shutdown
    // owner must already be inside the async future's arguments.
    pump.tasks.last().unwrap().abort();
    timeout(Duration::from_secs(3), stop_tasks(&mut pump.tasks))
        .await
        .unwrap();
    assert_eq!(Arc::strong_count(&pump.mapping), 1);
    assert_eq!(stats.dropped.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn transport_send_error_receive_eof_and_invalid_ring_packet_fail_and_clean_up() {
    for fault in ["send", "eof", "packet"] {
        let mapping = mapping();
        let weak = Arc::downgrade(&mapping);
        let FakeChannels {
            io,
            sent,
            incoming,
            stats,
        } = fake_io();
        let mut pump = PumpFixture::start(io, mapping);
        match fault {
            "send" => {
                drop(sent);
                upload(&pump.mapping, b"closed sink");
                // Keep the other direction open so only the send can fail.
                wait_until(|| pump.cancellation.is_cancelled()).await;
                drop(incoming);
            }
            "eof" => drop(incoming),
            "packet" => {
                incoming.send(Ok(Bytes::new())).await.unwrap();
                wait_until(|| pump.cancellation.is_cancelled()).await;
                drop(incoming);
            }
            _ => unreachable!(),
        }
        wait_until(|| pump.cancellation.is_cancelled()).await;
        pump.stop().await;
        let failure = pump
            .failure
            .borrow()
            .clone()
            .expect("failure must reach runtime health");
        if fault == "packet" {
            assert_eq!(failure.failure.code, TransportFailureCode::AgentUnreachable);
            assert!(
                failure
                    .message
                    .contains("Engine-to-Agent packet ring failed")
            );
        } else {
            assert_eq!(
                failure.failure,
                TransportError::TunnelClosed.failure(
                    Some(test_path().transport),
                    Some(test_path().endpoint_family)
                )
            );
            assert!(failure.message.contains(if fault == "send" {
                "failed to send"
            } else {
                "failed to receive"
            }));
        }
        assert_eq!(stats.pending.load(Ordering::SeqCst), 0);
        assert_eq!(stats.dropped.load(Ordering::SeqCst), 1);
        drop(pump);
        assert!(weak.upgrade().is_none());
    }
}

fn publish(
    io: &mut FakeIo,
    mapping: &PacketSessionMapping,
    pending: &mut Option<Bytes>,
) -> PublishState {
    publish_engine_packet_batch(
        io,
        mapping,
        test_path(),
        &CancellationToken::new(),
        &mut 0,
        pending,
    )
    .unwrap_or_else(|failure| panic!("{}", failure.message))
}

fn assert_one_notification(mapping: &PacketSessionMapping) {
    // SAFETY: the anonymous fixture owns this auto-reset event throughout both
    // nonblocking waits; they do not touch a network interface or service.
    let signaled = unsafe { WaitForSingleObject(mapping.engine_to_agent_event.0, 0) };
    assert_eq!(signaled, WAIT_OBJECT_0);
    // SAFETY: same live event; consuming its signal above leaves it reset.
    let reset = unsafe { WaitForSingleObject(mapping.engine_to_agent_event.0, 0) };
    assert_eq!(reset, WAIT_TIMEOUT);
}

#[test]
fn downlink_batches_obey_packet_and_quarter_ring_limits_and_signal_after_publication() {
    for bytes in [1, usque_platform::packet_ring::MAX_PACKET_BYTES] {
        let mapping = mapping();
        let FakeChannels {
            mut io,
            sent: _sent,
            incoming,
            stats: _stats,
        } = fake_io();
        for _ in 0..PACKET_WAKE_BATCH {
            incoming
                .try_send(Ok(Bytes::from(vec![0x44; bytes])))
                .unwrap();
        }
        let mut pending = Some(Bytes::from(vec![0x44; bytes]));
        assert_eq!(
            publish(&mut io, &mapping, &mut pending),
            PublishState::Ready
        );
        assert!(pending.is_none());
        let mut count = 0;
        while let Some(packet) = mapping
            .ring()
            .try_pop(PacketDirection::EngineToAgent)
            .unwrap()
        {
            assert_eq!(packet, vec![0x44; bytes]);
            count += 1;
        }
        let wake_bytes = mapping.ring().capacity() as usize / 4;
        let expected = PACKET_WAKE_BATCH.min(wake_bytes.div_ceil(bytes));
        assert_eq!(count, expected);
        assert_eq!(io.incoming.len(), PACKET_WAKE_BATCH + 1 - expected);
        assert_one_notification(&mapping);
    }
}

#[test]
fn full_ring_preserves_pending_without_drops_and_receive_error_signals_published_prefix() {
    let mapping = mapping();
    fill_download(&mapping);
    let FakeChannels {
        mut io,
        sent: _sent,
        incoming: _incoming,
        stats: _stats,
    } = fake_io();
    let packet = Bytes::from_static(b"retained");
    let mut pending = Some(packet.clone());
    assert_eq!(
        publish(&mut io, &mapping, &mut pending),
        PublishState::Blocked
    );
    assert_eq!(pending, Some(packet));
    assert_eq!(mapping.ring().dropped(PacketDirection::EngineToAgent), 0);

    let mapping = self::mapping();
    let FakeChannels {
        mut io,
        sent: _sent,
        incoming,
        stats: _stats,
    } = fake_io();
    incoming
        .try_send(Err(TransportError::TunnelClosed))
        .unwrap();
    let mut pending = Some(Bytes::from_static(b"published before error"));
    let failure = match publish_engine_packet_batch(
        &mut io,
        &mapping,
        test_path(),
        &CancellationToken::new(),
        &mut 0,
        &mut pending,
    ) {
        Err(failure) => failure,
        Ok(state) => panic!("expected receive error, got {state:?}"),
    };
    assert!(failure.message.contains("failed to receive"));
    assert_eq!(
        mapping
            .ring()
            .try_pop(PacketDirection::EngineToAgent)
            .unwrap()
            .unwrap(),
        b"published before error"
    );
    assert_one_notification(&mapping);
}

#[test]
fn full_ring_signals_published_prefix_and_retries_the_retained_suffix_once() {
    let mapping = mapping();
    let fillers = fill_download(&mapping);
    mapping
        .ring()
        .try_pop(PacketDirection::EngineToAgent)
        .unwrap()
        .unwrap();
    let FakeChannels {
        mut io,
        sent: _sent,
        incoming,
        stats: _stats,
    } = fake_io();
    incoming
        .try_send(Ok(Bytes::from_static(b"retained suffix")))
        .unwrap();
    let prefix = Bytes::from(vec![0x55; 4092]);
    let mut pending = Some(prefix.clone());
    assert_eq!(
        publish(&mut io, &mapping, &mut pending),
        PublishState::Blocked
    );
    assert_eq!(pending, Some(Bytes::from_static(b"retained suffix")));
    assert_eq!(mapping.ring().dropped(PacketDirection::EngineToAgent), 0);
    assert_one_notification(&mapping);

    for _ in 1..fillers {
        assert_eq!(
            mapping
                .ring()
                .try_pop(PacketDirection::EngineToAgent)
                .unwrap()
                .unwrap(),
            [0x22; 4092]
        );
    }
    assert_eq!(
        mapping
            .ring()
            .try_pop(PacketDirection::EngineToAgent)
            .unwrap()
            .unwrap(),
        prefix
    );
    assert_eq!(
        publish(&mut io, &mapping, &mut pending),
        PublishState::Ready
    );
    assert!(pending.is_none());
    assert_eq!(
        mapping
            .ring()
            .try_pop(PacketDirection::EngineToAgent)
            .unwrap()
            .unwrap(),
        b"retained suffix"
    );
    assert!(
        mapping
            .ring()
            .try_pop(PacketDirection::EngineToAgent)
            .unwrap()
            .is_none()
    );
    assert_one_notification(&mapping);
}
