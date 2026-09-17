use super::*;
use crate::{Config, HasChannel, Pipe, PipeDev, TcpBufferMetrics, TcpBufferPolicy, TcpBufferTier};
use alloc::{boxed::Box, vec};
use core::task::{Context, Poll, Waker};
use smoltcp::{phy::Medium, time::Instant};

extern crate std;

#[test]
fn concurrent_response_handoff_never_reclaims_the_received_socket() {
    for _ in 0..256 {
        let (mut stack, _) = stack();
        let (resp, receiver) = flume::bounded(1);
        stack.process_one_cmd(crate::Request {
            handle: None,
            command: command(1),
            resp,
        });
        let received = std::thread::spawn(move || receiver.recv().unwrap());
        for _ in 0..8 {
            stack.reap_unclaimed_creations();
            std::thread::yield_now();
        }
        let response = received.join().unwrap();
        stack.reap_unclaimed_creations();
        assert_eq!(stack.socket_set.iter().count(), 1);
        stack.reclaim_creation(CreatedSocket::from_response(&response).unwrap());
        assert_eq!(stack.socket_set.iter().count(), 0);
    }
}

fn stack() -> (Netstack, TcpBufferMetrics) {
    let metrics = TcpBufferMetrics::default();
    let tier = TcpBufferTier {
        receive: 16384,
        transmit: 16384,
    };
    let mut stack = Netstack::new(
        Config {
            command_channel_capacity: Some(1),
            tcp_buffer_policy: Some(TcpBufferPolicy {
                preferred: tier,
                fallback: tier,
                preferred_budget: 32768,
                total_budget: 32768,
            }),
            tcp_buffer_metrics: Some(metrics.clone()),
            ..Config::default()
        },
        Instant::from_millis(0),
    );
    assert!(stack.direct_set_ips(["172.16.0.2".parse().unwrap()]));
    (stack, metrics)
}

fn command(kind: usize) -> Command {
    let local = "172.16.0.2:50000".parse().unwrap();
    match kind {
        0 => tcp::stream::Command::Connect {
            local_endpoint: local,
            remote_endpoint: "172.16.0.3:443".parse().unwrap(),
        }
        .into(),
        1 => udp::Command::Bind { endpoint: local }.into(),
        _ => tcp::listen::Command::ListenOnce {
            local_endpoint: local,
        }
        .into(),
    }
}

#[test]
fn cancellation_before_admission_after_allocation_and_with_a_full_queue_reclaims_creations() {
    for kind in 0..3 {
        for stage in 0..3 {
            let (mut stack, metrics) = stack();
            let channel = stack.command_channel();
            let mut request = Box::pin(channel.request(None, command(kind)));
            assert!(
                request
                    .as_mut()
                    .poll(&mut Context::from_waker(Waker::noop()))
                    .is_pending()
            );
            if stage != 0 {
                stack.process_cmds();
                assert_eq!(stack.socket_set.iter().count(), 1);
            }
            if stage == 2 {
                crate::try_request_nonblocking(
                    &channel,
                    None,
                    crate::stack_control::Command::SetIps {
                        new_ips: vec!["172.16.0.2".parse().unwrap()],
                    },
                )
                .unwrap();
            }
            drop(request);
            stack.process_cmds();
            stack.pump_waiters();
            assert_eq!(
                metrics.snapshot().total_bytes,
                0,
                "kind {kind}, stage {stage}"
            );
            assert_eq!(
                stack.socket_set.iter().count(),
                0,
                "kind {kind}, stage {stage}"
            );
            assert!(stack.blocked_commands.is_empty());
            assert!(stack.unclaimed_creations.is_empty());
            assert!(stack.tcp_listeners.is_empty());
        }
    }
}

#[test]
fn received_response_survives_tracking_cleanup_and_slot_reuse() {
    for kind in [1, 2] {
        let (mut stack, _) = stack();
        let channel = stack.command_channel();
        let mut request = Box::pin(channel.request(None, command(kind)));
        let mut cx = Context::from_waker(Waker::noop());
        assert!(request.as_mut().poll(&mut cx).is_pending());
        stack.process_cmds();
        let Poll::Ready(Ok(response)) = request.as_mut().poll(&mut cx) else {
            panic!("response")
        };
        let owner = CreatedSocket::from_response(&response).unwrap();
        drop(request);
        stack.pump_waiters();
        assert_eq!(stack.socket_set.iter().count(), 1);
        assert!(stack.unclaimed_creations.is_empty());
        stack.reclaim_creation(owner);
        assert_eq!(stack.socket_set.iter().count(), 0);
        let mut replacement = Box::pin(channel.request(None, command(kind)));
        assert!(replacement.as_mut().poll(&mut cx).is_pending());
        stack.process_cmds();
        stack.pump_waiters();
        assert_eq!(stack.socket_set.iter().count(), 1);
        drop(replacement);
        stack.process_cmds();
        assert_eq!(stack.socket_set.iter().count(), 0);
    }
}

#[test]
fn cancelled_connected_response_is_reclaimed_without_touching_a_received_winner() {
    for consume in [false, true] {
        let (mut client, metrics) = stack();
        let (mut server, _) = stack();
        assert!(server.direct_set_ips(["172.16.0.3".parse().unwrap()]));
        let (a, b) = Pipe::unbounded();
        let mut a = PipeDev {
            pipe: a,
            medium: Medium::Ip,
            mtu: 1280,
        };
        let mut b = PipeDev {
            pipe: b,
            medium: Medium::Ip,
            mtu: 1280,
        };
        let listener = server.process_tcp_listen(
            tcp::listen::Command::ListenOnce {
                local_endpoint: "172.16.0.3:443".parse().unwrap(),
            },
            None,
        );
        assert!(matches!(listener, Response::TcpListen(_)));
        let channel = client.command_channel();
        let mut request = Box::pin(channel.request(None, command(0)));
        let mut cx = Context::from_waker(Waker::noop());
        assert!(request.as_mut().poll(&mut cx).is_pending());
        client.process_cmds();
        for n in 0..8 {
            client.poll_device_io(Instant::from_millis(n), &mut a);
            server.poll_device_io(Instant::from_millis(n), &mut b);
        }
        assert_eq!(client.unclaimed_creations.len(), 1, "handshake completed");
        let owner = if consume {
            let Poll::Ready(Ok(response)) = request.as_mut().poll(&mut cx) else {
                panic!("connected")
            };
            Some(CreatedSocket::from_response(&response).unwrap())
        } else {
            None
        };
        drop(request);
        client.process_cmds();
        client.pump_waiters();
        assert_eq!(
            metrics.snapshot().total_bytes,
            if consume { 32768 } else { 0 }
        );
        assert!(client.unclaimed_creations.is_empty());
        if let Some(owner) = owner {
            client.reclaim_creation(owner);
            assert_eq!(metrics.snapshot().total_bytes, 0);
        }
    }
}
