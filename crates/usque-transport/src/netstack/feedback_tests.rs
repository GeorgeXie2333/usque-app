//! Real packet pump + one-slot packet device, without sockets or platform I/O.
use std::future::poll_fn;
use std::task::Poll;

use ts_netstack_smoltcp::netcore::smoltcp::{
    phy::{Device, TxToken},
    time::Instant as StackInstant,
};

use super::*;
use crate::h3::regression_tests::scripted_tunnel;

async fn drive_pending(future: &mut Pin<&mut impl Future<Output = ActiveOutcome>>) {
    for _ in 0..4 {
        poll_fn(|cx| {
            assert!(
                future.as_mut().poll(cx).is_pending(),
                "pump ended unexpectedly"
            );
            Poll::Ready(())
        })
        .await;
        tokio::task::yield_now().await;
    }
}

#[tokio::test]
async fn icmp_feedback_does_not_stop_outgoing_when_both_packet_directions_are_full() {
    feedback_saturation(1, false).await;
}

#[tokio::test]
async fn feedback_overflow_preserves_progress_and_cancellation() {
    feedback_saturation(100, false).await;
}

#[tokio::test]
async fn full_feedback_queue_does_not_mask_ipv6_minimum_mtu_failure() {
    feedback_saturation(100, true).await;
}

async fn feedback_saturation(count: usize, terminate_ipv6: bool) {
    let (tunnel, mut actor) = scripted_tunnel();
    let (stack_pipe, pipe) = WakingPipe::bounded(1);
    assert!(pipe.tx.try_send(&super::tests::test_ipv4_packet(1)));
    let mut device = PacketDevice::new(stack_pipe, 1280);
    let mut packet_io = PacketIo::from_pipe(pipe);
    let cancellation = CancellationToken::new();
    let key = usque_core::MasqueKeyPair::generate();
    let identity = Arc::new(
        MasqueTlsIdentity::new(
            key.private_sec1_der().unwrap(),
            &key.public_spki_der().unwrap(),
            "172.16.0.2".parse().unwrap(),
            "2001:db8::2".parse().unwrap(),
        )
        .unwrap(),
    );
    let path = runtime_path(Transport::Http3, AddressFamily::Ipv4);
    let (control_tx, _control_rx) = watch::channel(PeerNetworkState::default());
    let (health_tx, _health_rx) = watch::channel(RuntimeHealth::Connected {
        path,
        reconnect_count: 0,
    });
    let profile = Profile::default();
    let telemetry = ConnectionTelemetry::default();
    let (publisher, updates) = crate::tcp_packet_budget::BudgetPublisher::new();
    let pump = pump_active_tunnel(
        MasqueTunnel::Http3(Box::new(tunnel)),
        path,
        0,
        &mut packet_io,
        &profile,
        identity,
        crate::socket::noop_socket_protector(),
        &cancellation,
        Arc::new(TrafficCounters::default()),
        &control_tx,
        &health_tx,
        &telemetry,
        0,
        None,
        Some(&publisher),
    );
    tokio::pin!(pump);
    actor
        .incoming
        .send(PacketBatch::single(super::tests::test_ipv4_packet(2)))
        .await
        .unwrap();
    drive_pending(&mut pump).await;
    assert_eq!(updates.borrow().ipv4, Some(1280));
    actor
        .budget
        .send_replace(Some(crate::tcp_packet_budget::H3PacketBudget {
            limit: 1150,
            confirmed: true,
        }));
    drive_pending(&mut pump).await;
    assert_eq!(
        (updates.borrow().ipv4, updates.borrow().ipv6),
        (Some(1150), Some(1280))
    );
    assert_eq!(
        actor.incoming.capacity(),
        1,
        "real incoming delivery must be blocked on the full pipe"
    );

    let mut large = super::tests::test_ipv4_packet(4).to_vec();
    large.resize(1280, 0);
    large[2..4].copy_from_slice(&1280_u16.to_be_bytes());
    large[10..12].fill(0);
    let checksum = ipv4_header_checksum(&large);
    large[10..12].copy_from_slice(&checksum.to_be_bytes());
    for _ in 0..count {
        device
            .transmit(StackInstant::from_millis(0))
            .unwrap()
            .consume(large.len(), |out| out.copy_from_slice(&large));
        drive_pending(&mut pump).await;
        let (mut batch, completion) = actor.take_outgoing().unwrap();
        completion
            .send(PacketBatchResult {
                accepted_bytes: 0,
                oversized: vec![(batch.pop_front().unwrap(), 1150)],
            })
            .unwrap();
        drive_pending(&mut pump).await;
    }
    let small = super::tests::test_ipv4_packet(3);
    device
        .transmit(StackInstant::from_millis(0))
        .unwrap()
        .consume(small.len(), |out| out.copy_from_slice(&small));
    assert!(
        device.receive(StackInstant::from_millis(0)).is_none(),
        "the real device needs TX capacity before consuming RX"
    );
    drive_pending(&mut pump).await;
    let (batch, completion) = actor
        .take_outgoing()
        .expect("queued ICMP must not prevent draining the full outgoing pipe");
    assert_eq!(batch.len(), 1);
    completion
        .send(PacketBatchResult {
            accepted_bytes: batch.bytes(),
            oversized: vec![],
        })
        .unwrap();
    drive_pending(&mut pump).await;
    if terminate_ipv6 {
        let mut packet = vec![0; 1280];
        packet[0] = 0x60;
        packet[4..6].copy_from_slice(&1240u16.to_be_bytes());
        packet[6] = 17;
        packet[7] = 64;
        device
            .transmit(StackInstant::from_millis(0))
            .unwrap()
            .consume(packet.len(), |out| out.copy_from_slice(&packet));
        drive_pending(&mut pump).await;
        let (mut batch, completion) = actor.take_outgoing().unwrap();
        completion
            .send(PacketBatchResult {
                accepted_bytes: 0,
                oversized: vec![(batch.pop_front().unwrap(), 1150)],
            })
            .unwrap();
        assert!(
            matches!(pump.await, ActiveOutcome::Terminal(failure) if failure.code == TransportFailureCode::PacketSendFailed)
        );
        return;
    }
    assert!(
        device.receive(StackInstant::from_millis(0)).is_some(),
        "draining outgoing must unblock the device"
    );
    cancellation.cancel();
    assert!(matches!(pump.await, ActiveOutcome::Shutdown));
    let old = *updates.borrow();
    actor
        .budget
        .send_replace(Some(crate::tcp_packet_budget::H3PacketBudget {
            limit: 1500,
            confirmed: true,
        }));
    tokio::task::yield_now().await;
    assert_eq!(
        *updates.borrow(),
        old,
        "a closed session cannot publish a late budget"
    );
}
