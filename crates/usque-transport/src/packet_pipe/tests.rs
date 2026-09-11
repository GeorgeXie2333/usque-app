use super::*;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use std::task::{Wake, Waker};
use std::time::Duration;
use ts_netstack_smoltcp::netcore::smoltcp::phy::{RxToken, TxToken};
use ts_netstack_smoltcp::netcore::{Config, Netstack, Request, flume, tcp};

#[derive(Default)]
struct WakeCounter(AtomicUsize);
impl Wake for WakeCounter {
    fn wake(self: Arc<Self>) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }
    fn wake_by_ref(self: &Arc<Self>) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }
}

#[test]
fn tokens_reserve_capacity_before_tcp_can_accept_bytes() {
    let (pipe, mut peer) = PacketPipe::bounded(1);
    let mut device = PacketDevice::new(pipe, 1280);
    let token = device.transmit(Instant::from_millis(0)).unwrap();
    assert!(device.transmit(Instant::from_millis(0)).is_none());
    drop(token);
    device
        .transmit(Instant::from_millis(0))
        .unwrap()
        .consume(3, |p| p.copy_from_slice(b"one"));
    assert!(device.transmit(Instant::from_millis(0)).is_none());
    assert_eq!(peer.rx.try_recv().unwrap().as_ref(), b"one");
    assert!(peer.rx.try_recv().is_none());
    assert!(device.transmit(Instant::from_millis(0)).is_some());
}

#[test]
fn capacity_and_receive_notifications_resume_without_another_packet() {
    let (pipe, mut peer) = PacketPipe::bounded(1);
    let mut device = PacketDevice::new(pipe, 1280);
    let wake = Arc::new(WakeCounter::default());
    let waker = Waker::from(wake.clone());
    let mut cx = Context::from_waker(&waker);
    device
        .transmit(Instant::from_millis(0))
        .unwrap()
        .consume(1, |p| p[0] = 7);
    assert!(Pin::new(&mut device).poll_tx(&mut cx).is_pending());
    assert!(Pin::new(&mut device).poll_rx(&mut cx).is_pending());
    assert!(peer.tx.try_send(b"request"));
    assert!(Pin::new(&mut device).poll_rx(&mut cx).is_pending());
    let before = wake.0.load(Ordering::Relaxed);
    peer.rx.try_recv().unwrap();
    assert!(wake.0.load(Ordering::Relaxed) > before);
    assert!(Pin::new(&mut device).poll_rx(&mut cx).is_ready());
    let (rx, tx) = device.receive(Instant::from_millis(0)).unwrap();
    rx.consume(|packet| assert_eq!(packet, b"request"));
    tx.consume(5, |packet| packet.copy_from_slice(b"reply"));
    assert_eq!(peer.rx.try_recv().unwrap().as_ref(), b"reply");
    drop(peer);
    assert!(device.transmit(Instant::from_millis(0)).is_none());
}

#[test]
fn real_tcp_egress_stops_at_full_pipe_and_retains_the_next_syn() {
    // This exact 63/64 queue + two socket burst blocked in the old device.
    // An OS-thread watchdog releases the peer even on regression, so a failed
    // test cannot leave a Tokio worker permanently blocked in CI.
    let (pipe, mut peer) = PacketPipe::bounded(64);
    let mut device = PacketDevice::new(pipe, 1280);
    for _ in 0..63 {
        device
            .transmit(Instant::from_millis(0))
            .unwrap()
            .consume(1, |p| p[0] = 0);
    }
    let mut stack = Netstack::new(
        Config {
            mtu: 1280,
            loopback: true,
            ..Config::default()
        },
        Instant::from_millis(0),
    );
    let mut responses = Vec::new();
    for port in [40001, 40002] {
        let (resp, response) = flume::bounded(1);
        responses.push(response);
        stack.process_one_cmd(Request {
            handle: None,
            command: tcp::stream::Command::Connect {
                local_endpoint: ([127, 0, 0, 1], port).into(),
                remote_endpoint: ([127, 0, 0, 2], 443).into(),
            }
            .into(),
            resp,
        });
    }
    let (done, result) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        let mut cx = Context::from_waker(Waker::noop());
        let polled =
            stack.poll_device_egress_async(&mut cx, Instant::from_millis(0), Pin::new(&mut device));
        let _ = done.send((polled, stack, device, responses));
    });
    let completed = result.recv_timeout(Duration::from_secs(2));
    if completed.is_err() {
        drop(peer);
        worker.join().unwrap();
        panic!("TCP egress blocked on a full packet pipe");
    }
    let (polled, mut stack, mut device, _responses) = completed.unwrap();
    worker.join().unwrap();
    assert_eq!(polled, Poll::Ready(true));
    let mut packets = Vec::new();
    while let Some(packet) = peer.rx.try_recv() {
        packets.push(packet);
    }
    assert_eq!(packets.len(), 64);
    let mut cx = Context::from_waker(Waker::noop());
    assert_eq!(
        stack.poll_device_egress_async(&mut cx, Instant::from_millis(0), Pin::new(&mut device)),
        Poll::Ready(true)
    );
    let second = peer.rx.try_recv().unwrap();
    assert_ne!(&packets[63][20..22], &second[20..22]);
    assert!(peer.rx.try_recv().is_none());
}

#[tokio::test]
async fn full_pipe_task_is_cancellable_and_can_be_recreated() {
    for _ in 0..4 {
        let (pipe, _peer) = PacketPipe::bounded(1);
        let mut device = PacketDevice::new(pipe, 1280);
        device
            .transmit(Instant::from_millis(0))
            .unwrap()
            .consume(1, |p| p[0] = 1);
        let task = tokio::spawn(async move {
            std::future::poll_fn(|cx| Pin::new(&mut device).poll_tx(cx)).await;
        });
        tokio::task::yield_now().await;
        task.abort();
        assert!(
            tokio::time::timeout(Duration::from_secs(1), task)
                .await
                .unwrap()
                .unwrap_err()
                .is_cancelled()
        );
    }
}
