use super::*;
use crate::netstack::bounded_piped;
use crate::stack_tcp::{OnceListener, StackTcpStream};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::oneshot;
use tokio::time::timeout;
use tokio_util::task::AbortOnDropHandle;
use ts_netstack_smoltcp::netcore::{Config, HasChannel, NetstackControl};

#[tokio::test]
async fn live_tcp_recovers_without_icmp_after_budget_shrinks_and_ignores_old_commands() {
    for (client_ip, server_ip, initial, reduced) in [
        ("10.0.0.1", "10.0.0.2", 1280, 1100),
        ("2001:db8::1", "2001:db8::2", 1500, 1280),
    ] {
        let config = Config {
            mtu: initial,
            tcp_buffer_size: 64 * 1024,
            command_channel_capacity: Some(4),
            ..Config::default()
        };
        let (client, mut left) = bounded_piped(config.clone());
        let (server, mut right) = bounded_piped(config);
        let channel = client.command_channel();
        let server_channel = server.command_channel();
        let _client = AbortOnDropHandle::new(client.spawn_tokio());
        let _server = AbortOnDropHandle::new(server.spawn_tokio());
        let local_ip = client_ip.parse().unwrap();
        let remote_ip = server_ip.parse().unwrap();
        channel.set_ips([local_ip]).await.unwrap();
        server_channel.set_ips([remote_ip]).await.unwrap();
        let (publisher, updates) = BudgetPublisher::new();
        publisher.publish(Some(H3PacketBudget {
            limit: initial,
            confirmed: true,
        }));
        let cancellation = CancellationToken::new();
        let updater = attach(&channel, Some(updates.clone()), &cancellation)
            .await
            .unwrap()
            .unwrap();
        let _updater = AbortOnDropHandle::new(updater);
        let bound = Arc::new(AtomicUsize::new(reduced));
        let maximum = Arc::new(AtomicUsize::new(0));
        let link_bound = bound.clone();
        let link_maximum = maximum.clone();
        let (dropped_tx, dropped_rx) = oneshot::channel();
        let _link = AbortOnDropHandle::new(tokio::spawn(async move {
            let mut dropped = Some(dropped_tx);
            loop {
                tokio::select! {
                    Some(packet) = left.rx.recv_async() => {
                        if packet.len() > link_bound.load(Ordering::Relaxed) {
                            if let Some(signal) = dropped.take() { let _ = signal.send(packet.len()); }
                            continue; // A small-path blackhole: deliberately no ICMP.
                        }
                        link_maximum.fetch_max(packet.len(), Ordering::Relaxed);
                        right.tx.send_owned_async(packet).await;
                    },
                    Some(packet) = right.rx.recv_async() => { left.tx.send_owned_async(packet).await; },
                    else => break,
                }
            }
        }));
        let local = std::net::SocketAddr::new(local_ip, 40000);
        let remote = std::net::SocketAddr::new(remote_ip, 443);
        let listener = OnceListener::bind(server_channel.clone(), remote)
            .await
            .unwrap();
        let (outgoing, accepted) = timeout(Duration::from_secs(3), async {
            tokio::join!(
                StackTcpStream::connect(channel.clone(), local, remote),
                listener.accept(local)
            )
        })
        .await
        .unwrap();
        let mut outgoing = outgoing.unwrap();
        let mut accepted = accepted.unwrap();
        let data: Vec<u8> = (0..32_768).map(|i| (i % 251) as u8).collect();
        let write = outgoing.write_all(&data);
        tokio::pin!(write);
        let mut received = vec![0; data.len()];
        let read = accepted.read_exact(&mut received);
        let change = async {
            assert!(
                timeout(Duration::from_secs(3), dropped_rx)
                    .await
                    .unwrap()
                    .unwrap()
                    > reduced
            );
            publisher.publish(Some(H3PacketBudget {
                limit: reduced,
                confirmed: true,
            }));
        };
        let (write_result, read_result, ()) = timeout(Duration::from_secs(10), async {
            tokio::join!(write, read, change)
        })
        .await
        .expect("TCP must resegment and recover even when every ICMP is lost");
        write_result.unwrap();
        read_result.unwrap();
        assert_eq!(received, data);
        assert!(maximum.load(Ordering::Relaxed) <= reduced);
        // An old-session command must not remove the current limit.
        apply(
            &channel,
            TcpPacketBudget {
                revision: 1,
                ipv4: None,
                ipv6: None,
            },
        )
        .await
        .unwrap();
        let mut second = vec![0; data.len()];
        let (written, read) = timeout(Duration::from_secs(5), async {
            tokio::join!(outgoing.write_all(&data), accepted.read_exact(&mut second))
        })
        .await
        .unwrap();
        written.unwrap();
        read.unwrap();
        assert_eq!(second, data);
        // Future sockets inherit the current budget, including their SYN MSS.
        let next_local = std::net::SocketAddr::new(local_ip, 40001);
        let next_remote = std::net::SocketAddr::new(remote_ip, 444);
        let listener = OnceListener::bind(server_channel.clone(), next_remote)
            .await
            .unwrap();
        let (next, next_accepted) = timeout(Duration::from_secs(3), async {
            tokio::join!(
                StackTcpStream::connect(channel.clone(), next_local, next_remote),
                listener.accept(next_local)
            )
        })
        .await
        .unwrap();
        let mut next = next.unwrap();
        let mut next_accepted = next_accepted.unwrap();
        let (written, read) = timeout(Duration::from_secs(5), async {
            tokio::join!(next.write_all(&data), next_accepted.read_exact(&mut second))
        })
        .await
        .unwrap();
        written.unwrap();
        read.unwrap();
        assert_eq!(second, data);
        // H2 replacement removes the DATAGRAM cap without resetting the TCP flow.
        bound.store(initial, Ordering::Relaxed);
        publisher.publish(None);
        let latest = *updates.borrow();
        apply(&channel, latest).await.unwrap();
        maximum.store(0, Ordering::Relaxed);
        let (written, read) = timeout(Duration::from_secs(5), async {
            tokio::join!(outgoing.write_all(&data), accepted.read_exact(&mut second))
        })
        .await
        .unwrap();
        written.unwrap();
        read.unwrap();
        assert_eq!(second, data);
        assert!(maximum.load(Ordering::Relaxed) > reduced);
        outgoing.shutdown().await.unwrap();
        assert_eq!(
            timeout(Duration::from_secs(3), accepted.read_u8())
                .await
                .unwrap()
                .unwrap_err()
                .kind(),
            std::io::ErrorKind::UnexpectedEof
        );
        cancellation.cancel();
    }
}

#[test]
fn ipv6_minimum_and_h2_reset_are_independent_of_ipv4_budget() {
    let (publisher, updates) = BudgetPublisher::new();
    publisher.publish(Some(H3PacketBudget {
        limit: 1100,
        confirmed: false,
    }));
    let first = *updates.borrow();
    assert_eq!((first.ipv4, first.ipv6), (Some(1100), Some(1280)));
    publisher.publish(Some(H3PacketBudget {
        limit: 1100,
        confirmed: true,
    }));
    assert_eq!(updates.borrow().revision, first.revision);
    publisher.publish(None);
    assert!(updates.borrow().revision > first.revision);
    assert_eq!((updates.borrow().ipv4, updates.borrow().ipv6), (None, None));
}

#[tokio::test]
async fn budget_consumer_coalesces_updates_and_cancels_a_full_command_queue() {
    use ts_netstack_smoltcp::netcore::{Command, Request, Response, flume};
    let (tx, rx) = flume::bounded::<Request>(1);
    let channel = tx.downgrade();
    let (publisher, updates) = BudgetPublisher::new();
    publisher.publish(Some(H3PacketBudget {
        limit: 1280,
        confirmed: true,
    }));
    let cancel = CancellationToken::new();
    let (consumer, ()) = tokio::join!(attach(&channel, Some(updates.clone()), &cancel), async {
        let request = rx.recv_async().await.unwrap();
        request.resp.send(Response::Ok).unwrap();
    });
    let consumer = consumer.unwrap().unwrap();
    publisher.publish(Some(H3PacketBudget {
        limit: 1200,
        confirmed: true,
    }));
    let pending = rx.recv_async().await.unwrap();
    for limit in (900..1200).rev() {
        publisher.publish(Some(H3PacketBudget {
            limit,
            confirmed: true,
        }));
    }
    assert_eq!(rx.len(), 0, "only one in-flight command is permitted");
    pending.resp.send(Response::Ok).unwrap();
    let latest = rx.recv_async().await.unwrap();
    assert!(
        matches!(latest.command, Command::StackControl(stack_control::Command::SetTcpPacketBudget { revision, ipv4: Some(900), .. }) if revision == updates.borrow().revision)
    );
    latest.resp.send(Response::Ok).unwrap();
    let (response, _response_rx) = flume::bounded(1);
    tx.send(Request {
        handle: None,
        command: Command::ReapCancelled,
        resp: response,
    })
    .unwrap();
    publisher.publish(None);
    tokio::task::yield_now().await;
    cancel.cancel();
    timeout(Duration::from_secs(1), consumer)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(rx.len(), 1);
    assert!(matches!(rx.recv().unwrap().command, Command::ReapCancelled));
    assert!(rx.try_recv().is_err(), "cancelled updates must not replay");
}
