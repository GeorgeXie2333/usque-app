use super::*;

fn local_pair() -> (MacAgentClient, UnixStream) {
    let (socket, server) = UnixStream::pair().unwrap();
    let peer = darwin_fd::peer_identity(&socket).unwrap();
    let client = MacAgentClient {
        socket: Arc::new(socket),
        connection: Arc::new(Mutex::new(Some(Connection { peer, sequence: 0 }))),
    };
    (client, server)
}

fn receive_request(server: &UnixStream, sequence: u64) -> (String, Operation) {
    let peer = darwin_fd::peer_identity(server).unwrap();
    let frame = darwin_fd::receive_frame(server, peer).unwrap();
    assert!(frame.descriptor.is_none());
    let request: p::AgentRequest = decode_frame(frame.bytes.into()).unwrap();
    let Some(agent_request::Payload::Macos(body)) = request.payload else {
        panic!("expected macOS request");
    };
    assert_eq!(body.sequence, sequence);
    (request.request_id, body.operation.unwrap())
}

fn send_reply(
    server: &UnixStream,
    request_id: String,
    reply: p::MacosAgentResponse,
    fd: Option<&OwnedFd>,
) {
    let response = p::AgentResponse {
        request_id,
        error: None,
        payload: Some(agent_response::Payload::Macos(reply)),
    };
    darwin_fd::send_frame(server, &encode_frame(&response).unwrap(), fd).unwrap();
}

fn respond(server: &UnixStream, sequence: u64, reply: p::MacosAgentResponse) -> Operation {
    let (request_id, operation) = receive_request(server, sequence);
    send_reply(server, request_id, reply, None);
    operation
}

fn dial() -> Operation {
    Operation::CreateSocket(p::MacosSocketRequest {
        fence: Some(p::MacosFence::default()),
        remote_endpoint: "[2001:db8::1]:443".into(),
        protocol: 6,
        purpose: p::MacosSocketPurpose::Masque as i32,
        ..Default::default()
    })
}

#[tokio::test]
async fn abandoning_startup_interrupts_an_unanswered_agent_dial() {
    let (client, server) = local_pair();
    let request = tokio::spawn({
        let client = client.clone();
        async move { client.request(dial()).await }
    });
    let server = tokio::task::spawn_blocking(move || {
        assert_eq!(receive_request(&server, 1).1, dial());
        server
    })
    .await
    .unwrap();
    // The server keeps the socket open without replying, like a stalled permit update.
    let abandon = tokio::task::spawn_blocking(move || client.abandon());
    let stopped = tokio::time::timeout(Duration::from_secs(1), request).await;
    drop(server);
    abandon.await.unwrap();
    assert!(
        stopped
            .expect("abandoned dial still waiting for reply")
            .unwrap()
            .is_err()
    );
}

#[tokio::test]
async fn malformed_descriptor_success_closes_the_owner_connection() {
    let fence = p::MacosFence::default();
    let different = p::MacosFence {
        network: 2,
        ..fence.clone()
    };
    for (operation, reply_fence, index, lease, descriptor) in [
        (dial(), None, 1, 1, true),
        (dial(), Some(different), 1, 1, true),
        (dial(), Some(fence.clone()), 0, 1, true),
        (dial(), Some(fence.clone()), 1, 0, true),
        (
            Operation::OpenTun(fence.clone()),
            Some(fence.clone()),
            1,
            1,
            true,
        ),
        (dial(), Some(fence.clone()), 1, 1, false),
        (Operation::OpenTun(fence.clone()), Some(fence), 1, 0, false),
    ] {
        let (client, socket) = local_pair();
        let server = std::thread::spawn(move || {
            let (id, _) = receive_request(&socket, 1);
            let payload = if descriptor {
                Payload::Descriptor(p::MacosDescriptor {
                    fence: reply_fence,
                    interface_index: index,
                    lease_id: lease,
                    ..Default::default()
                })
            } else {
                Payload::Empty(p::Empty {})
            };
            let fd: OwnedFd = fs::File::open("/dev/null").unwrap().into();
            send_reply(
                &socket,
                id,
                p::MacosAgentResponse {
                    failure: 0,
                    payload: Some(payload),
                },
                descriptor.then_some(&fd),
            );
            socket
        });
        assert_eq!(
            client.request(operation).await.unwrap_err().kind(),
            io::ErrorKind::PermissionDenied
        );
        assert!(client.connection.lock().unwrap().is_none());
        server.join().unwrap();
    }
}

#[tokio::test]
async fn cancelling_a_sent_dial_releases_the_late_lease_and_preserves_the_owner() {
    use tokio::sync::oneshot;
    use usque_transport::{PhysicalSocketPurpose, SocketProtector};
    let (client, socket) = local_pair();
    let fence = p::MacosFence {
        session_id: Uuid::new_v4().to_string(),
        configuration: 1,
        network: 1,
    };
    let protector = crate::macos_socket::MacSocketProtector::new(
        client.clone(),
        fence.clone(),
        &p::PhysicalNetworkInfo {
            generation: 1,
            interfaces: vec![p::PhysicalInterface {
                address_family_mask: 1,
                dns_servers: vec!["127.0.0.1:53".into()],
                ..Default::default()
            }],
        },
        vec![],
    )
    .unwrap();
    let (received, sent) = oneshot::channel();
    let (grant, cancelled) = std::sync::mpsc::channel();
    let (released, release) = oneshot::channel();
    let server = std::thread::spawn(move || {
        let (request_id, operation) = receive_request(&socket, 1);
        let Operation::CreateSocket(dial) = operation else {
            panic!("expected a dial")
        };
        received.send(()).unwrap();
        cancelled.recv().unwrap();

        let udp = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        udp.set_nonblocking(true).unwrap();
        // SAFETY: a terminated interface name, and a live UDP socket with a bounded index value.
        let index = unsafe { libc::if_nametoindex(c"lo0".as_ptr()) };
        assert_ne!(index, 0);
        assert_eq!(
            unsafe {
                libc::setsockopt(
                    udp.as_raw_fd(),
                    libc::IPPROTO_IP,
                    libc::IP_BOUND_IF,
                    (&index as *const u32).cast(),
                    std::mem::size_of_val(&index) as _,
                )
            },
            0
        );
        let remote = dial.remote_endpoint.parse().unwrap();
        udp.connect(remote).unwrap();
        let info = p::MacosDescriptor {
            fence: Some(fence.clone()),
            interface_index: index,
            interface_name: "lo0".into(),
            protocol: 17,
            local_endpoint: udp.local_addr().unwrap().to_string(),
            remote_endpoint: dial.remote_endpoint,
            lease_id: 7,
        };
        let fd: OwnedFd = udp.into();
        verify_socket(&fd, &info, remote, 17).unwrap();
        send_reply(
            &socket,
            request_id,
            p::MacosAgentResponse {
                failure: 0,
                payload: Some(Payload::Descriptor(info)),
            },
            Some(&fd),
        );
        let op = respond(
            &socket,
            2,
            p::MacosAgentResponse {
                failure: 0,
                payload: Some(Payload::Empty(p::Empty {})),
            },
        );
        assert_eq!(
            op,
            Operation::ReleaseSocket(p::MacosReleaseSocket {
                fence: Some(fence),
                lease_id: 7
            })
        );
        released.send(()).unwrap();
        let status = p::MacosPlatformStatus {
            protocol_version: PROTOCOL_VERSION,
            agent_version: env!("CARGO_PKG_VERSION").into(),
            architecture: std::env::consts::ARCH.into(),
            ..Default::default()
        };
        assert!(matches!(
            respond(
                &socket,
                3,
                p::MacosAgentResponse {
                    failure: 0,
                    payload: Some(Payload::Status(status))
                }
            ),
            Operation::Status(_)
        ));
        socket
    });
    let pending = tokio::spawn(async move {
        protector
            .prepare_udp(
                "127.0.0.1:443".parse().unwrap(),
                PhysicalSocketPurpose::Masque,
                1,
            )
            .await
    });
    sent.await.unwrap();
    pending.abort();
    assert!(pending.await.unwrap_err().is_cancelled());
    grant.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(3), release)
        .await
        .expect("late lease was not revoked")
        .unwrap();
    client.status().await.unwrap();
    server.join().unwrap();
}

#[tokio::test]
async fn failed_endpoint_dial_preserves_owner_for_status_and_disconnect() {
    let (client, socket) = local_pair();
    let server = std::thread::spawn(move || {
        assert!(matches!(
            respond(
                &socket,
                1,
                p::MacosAgentResponse {
                    failure: p::MacosFailure::EndpointUnreachable as i32,
                    payload: None,
                }
            ),
            Operation::CreateSocket(_)
        ));
        let status = p::MacosPlatformStatus {
            protocol_version: PROTOCOL_VERSION,
            agent_version: env!("CARGO_PKG_VERSION").into(),
            architecture: std::env::consts::ARCH.into(),
            ..Default::default()
        };
        assert!(matches!(
            respond(
                &socket,
                2,
                p::MacosAgentResponse {
                    failure: 0,
                    payload: Some(Payload::Status(status)),
                }
            ),
            Operation::Status(_)
        ));
        assert!(matches!(
            respond(
                &socket,
                3,
                p::MacosAgentResponse {
                    failure: 0,
                    payload: Some(Payload::Empty(p::Empty {})),
                }
            ),
            Operation::Disconnect(_)
        ));
        // Keep the peer's audit token available until the client reads the last reply.
        socket
    });

    let err = client.clone().request(dial()).await.unwrap_err();
    assert_eq!(
        err.get_ref()
            .and_then(|e| e.downcast_ref::<AgentFailure>())
            .map(|e| e.0),
        Some(p::MacosFailure::EndpointUnreachable)
    );
    client.status().await.unwrap();
    client
        .empty(Operation::Disconnect(p::MacosFence::default()))
        .await
        .unwrap();
    server.join().unwrap();
}

#[tokio::test]
async fn protocol_and_guard_failures_still_close_the_owner_connection() {
    for (operation, failure, payload) in [
        (dial(), p::MacosFailure::RecoveryIncomplete, None),
        (dial(), p::MacosFailure::StaleFence, None),
        (
            Operation::Status(p::Empty {}),
            p::MacosFailure::EndpointUnreachable,
            None,
        ),
        (
            dial(),
            p::MacosFailure::EndpointUnreachable,
            Some(Payload::Empty(p::Empty {})),
        ),
    ] {
        let (client, socket) = local_pair();
        let malformed = payload.is_some();
        let server = std::thread::spawn(move || {
            respond(
                &socket,
                1,
                p::MacosAgentResponse {
                    failure: failure as i32,
                    payload,
                },
            );
            socket
        });
        let error = client.request(operation).await.unwrap_err();
        if malformed {
            assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
        } else {
            assert_eq!(
                error
                    .get_ref()
                    .and_then(|e| e.downcast_ref::<AgentFailure>())
                    .map(|e| e.0),
                Some(failure)
            );
        }
        assert!(client.connection.lock().unwrap().is_none());
        server.join().unwrap();
    }
}
