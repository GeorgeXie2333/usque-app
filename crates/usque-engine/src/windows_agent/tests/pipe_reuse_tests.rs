use super::*;
use tokio::net::windows::named_pipe::NamedPipeServer;

const TEST_WAIT: Duration = Duration::from_secs(5);

async fn read_request(pipe: &mut NamedPipeServer) -> AgentRequest {
    timeout(TEST_WAIT, async {
        let mut header = [0; 4];
        pipe.read_exact(&mut header).await.unwrap();
        let size = u32::from_be_bytes(header) as usize;
        assert!(size <= MAX_AGENT_FRAME_BYTES);
        let mut payload = vec![0; size];
        pipe.read_exact(&mut payload).await.unwrap();
        let mut frame = BytesMut::from(header.as_slice());
        frame.extend_from_slice(&payload);
        let request: AgentRequest = decode_frame(frame.freeze()).unwrap();
        assert_eq!(request.protocol_version, AGENT_PROTOCOL_VERSION);
        request
    })
    .await
    .expect("client request timed out")
}

async fn write_response(
    pipe: &mut NamedPipeServer,
    request_id: String,
    payload: agent_response::Payload,
) {
    pipe.write_all(
        &encode_frame(&AgentResponse {
            request_id,
            payload: Some(payload),
            error: None,
        })
        .unwrap(),
    )
    .await
    .unwrap();
}

async fn assert_eof(pipe: &mut NamedPipeServer) {
    let mut byte = [0];
    match timeout(TEST_WAIT, pipe.read(&mut byte))
        .await
        .expect("client retained an obsolete pipe")
    {
        Ok(0) => {}
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::BrokenPipe
                    | io::ErrorKind::ConnectionReset
                    | io::ErrorKind::UnexpectedEof
            ) => {}
        other => panic!("expected client EOF, got {other:?}"),
    }
}

fn physical_info(generation: u64) -> PhysicalNetworkInfo {
    PhysicalNetworkInfo {
        generation,
        interfaces: vec![agent_v1::PhysicalInterface {
            interface_luid: 17,
            interface_index: 23,
            dns_servers: vec!["192.0.2.53".into()],
            address_family_mask: 1,
        }],
    }
}

fn fixture() -> (WindowsAgentClient, NamedPipeServer, String) {
    let name = format!("{AGENT_PIPE_NAME}.reuse-{}", Uuid::new_v4());
    let pipe = ServerOptions::new()
        .first_pipe_instance(true)
        .create(&name)
        .unwrap();
    (WindowsAgentClient::for_test(name.clone()), pipe, name)
}

fn protector(client: WindowsAgentClient, operation_id: Uuid) -> Arc<WindowsVpnSocketProtector> {
    Arc::new(WindowsVpnSocketProtector {
        registration_api: Vec::new(),
        agent: client,
        operation_id,
        physical: RwLock::new(WindowsPhysicalState {
            generation: 10,
            agent_generation: Some(7),
            dns_servers: vec!["192.0.2.53:53".parse().unwrap()],
            family_mask: 1,
        }),
        physical_watch: watch::channel(PhysicalNetworkSnapshot::default()).0,
        monitor_cancel: CancellationToken::new(),
        proxy_mode: AtomicBool::new(false),
        automatic_endpoints: false,
        preset_hostname: None,
    })
}

#[tokio::test]
async fn monitor_ticks_share_one_pipe_and_cancel_releases_the_actual_connection() {
    let (client, mut pipe, name) = fixture();
    let operation = Uuid::new_v4();
    let accepts = Arc::new(AtomicUsize::new(0));
    let observed_accepts = accepts.clone();
    let (ready, completed) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        pipe.connect().await.unwrap();
        observed_accepts.fetch_add(1, Ordering::SeqCst);
        let unexpected = ServerOptions::new().create(&name).unwrap();
        for generation in [7, 8, 8] {
            let request = tokio::select! {
                request = read_request(&mut pipe) => request,
                result = unexpected.connect() => {
                    result.unwrap();
                    observed_accepts.fetch_add(1, Ordering::SeqCst);
                    panic!("monitor opened a pipe for another tick");
                }
            };
            assert!(matches!(request.payload,
                Some(agent_request::Payload::GetPhysicalNetworkInfo(ref request))
                if request.operation_id == operation.to_string()));
            write_response(
                &mut pipe,
                request.request_id,
                agent_response::Payload::PhysicalNetworkInfo(physical_info(generation)),
            )
            .await;
        }
        ready.send(()).unwrap();
        assert_eof(&mut pipe).await;
    });
    let protector = protector(client, operation);
    let cancellation = protector.monitor_cancel.clone();
    start_physical_network_monitor(&protector);
    timeout(TEST_WAIT, completed).await.unwrap().unwrap();
    assert_eq!(accepts.load(Ordering::SeqCst), 1);
    // A changed Agent generation is still published by the reused connection.
    timeout(TEST_WAIT, async {
        loop {
            if protector.physical.read().unwrap().agent_generation == Some(8) {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(protector.physical.read().unwrap().generation, 11);
    drop(protector);
    timeout(TEST_WAIT, cancellation.cancelled()).await.unwrap();
    timeout(TEST_WAIT, server).await.unwrap().unwrap();
}

#[tokio::test]
async fn complete_offline_observations_keep_the_read_only_pipe() {
    let (client, mut pipe, _) = fixture();
    let operation = Uuid::new_v4();
    let server = tokio::spawn(async move {
        pipe.connect().await.unwrap();
        for code in [
            "AGENT_PHYSICAL_NETWORK_OFFLINE",
            "AGENT_PHYSICAL_NETWORK_UNAVAILABLE",
        ] {
            let request = read_request(&mut pipe).await;
            assert!(matches!(request.payload,
                Some(agent_request::Payload::GetPhysicalNetworkInfo(ref request))
                if request.operation_id == operation.to_string()));
            pipe.write_all(
                &encode_frame(&AgentResponse {
                    request_id: request.request_id,
                    payload: None,
                    error: Some(agent_v1::AgentError {
                        code: code.into(),
                        retryable: true,
                        ..Default::default()
                    }),
                })
                .unwrap(),
            )
            .await
            .unwrap();
        }
        let request = read_request(&mut pipe).await;
        write_response(
            &mut pipe,
            request.request_id,
            agent_response::Payload::PhysicalNetworkInfo(physical_info(9)),
        )
        .await;
        assert_eof(&mut pipe).await;
    });
    let mut connection = None;
    for code in [
        "AGENT_PHYSICAL_NETWORK_OFFLINE",
        "AGENT_PHYSICAL_NETWORK_UNAVAILABLE",
    ] {
        let result = client
            .get_physical_network_info_reusing_pipe(&mut connection, operation, TEST_WAIT)
            .await;
        assert!(
            matches!(result, Err(WindowsVpnError::Remote { code: observed, .. }) if observed == code)
        );
        assert!(connection.is_some());
    }
    assert_eq!(
        client
            .get_physical_network_info_reusing_pipe(&mut connection, operation, TEST_WAIT)
            .await
            .unwrap()
            .generation,
        9
    );
    drop(connection);
    timeout(TEST_WAIT, server).await.unwrap().unwrap();
}

#[derive(Clone, Copy, Debug)]
enum FailedReply {
    Eof,
    WrongRequestId,
    Oversized,
    PartialTimeout,
    UnexpectedPayload,
    OwnerMismatch,
    ShuttingDown,
}

#[tokio::test]
async fn broken_or_untrusted_readbacks_drop_the_pipe_and_reauthenticate_next_query() {
    for fault in [
        FailedReply::Eof,
        FailedReply::WrongRequestId,
        FailedReply::Oversized,
        FailedReply::PartialTimeout,
        FailedReply::UnexpectedPayload,
        FailedReply::OwnerMismatch,
        FailedReply::ShuttingDown,
    ] {
        let (client, mut first, name) = fixture();
        let operation = Uuid::new_v4();
        let accepts = Arc::new(AtomicUsize::new(0));
        let observed = accepts.clone();
        let server = tokio::spawn(async move {
            first.connect().await.unwrap();
            observed.fetch_add(1, Ordering::SeqCst);
            let mut second = ServerOptions::new().create(&name).unwrap();
            let request = read_request(&mut first).await;
            assert!(matches!(
                request.payload,
                Some(agent_request::Payload::GetPhysicalNetworkInfo(_))
            ));
            match fault {
                FailedReply::Eof => {}
                FailedReply::WrongRequestId => {
                    write_response(
                        &mut first,
                        "stale-request-id".into(),
                        agent_response::Payload::PhysicalNetworkInfo(physical_info(7)),
                    )
                    .await;
                    assert_eof(&mut first).await;
                }
                FailedReply::Oversized => {
                    first
                        .write_all(&((MAX_AGENT_FRAME_BYTES + 1) as u32).to_be_bytes())
                        .await
                        .unwrap();
                    assert_eof(&mut first).await;
                }
                FailedReply::PartialTimeout => {
                    first.write_all(&[0, 0, 0, 64, 1]).await.unwrap();
                    assert_eof(&mut first).await;
                }
                FailedReply::UnexpectedPayload => {
                    write_response(
                        &mut first,
                        request.request_id,
                        agent_response::Payload::Empty(agent_v1::Empty {}),
                    )
                    .await;
                    assert_eof(&mut first).await;
                }
                FailedReply::OwnerMismatch | FailedReply::ShuttingDown => {
                    first
                        .write_all(
                            &encode_frame(&AgentResponse {
                                request_id: request.request_id,
                                payload: None,
                                error: Some(agent_v1::AgentError {
                                    code: if matches!(fault, FailedReply::OwnerMismatch) {
                                        "AGENT_OWNER_MISMATCH"
                                    } else {
                                        "AGENT_SHUTTING_DOWN"
                                    }
                                    .into(),
                                    ..Default::default()
                                }),
                            })
                            .unwrap(),
                        )
                        .await
                        .unwrap();
                    assert_eof(&mut first).await;
                }
            }
            drop(first);
            second.connect().await.unwrap();
            observed.fetch_add(1, Ordering::SeqCst);
            let request = read_request(&mut second).await;
            assert!(matches!(request.payload,
                Some(agent_request::Payload::GetPhysicalNetworkInfo(ref request))
                if request.operation_id == operation.to_string()));
            write_response(
                &mut second,
                request.request_id,
                agent_response::Payload::PhysicalNetworkInfo(physical_info(8)),
            )
            .await;
            assert_eof(&mut second).await;
        });
        let mut connection = None;
        let budget = if matches!(fault, FailedReply::PartialTimeout) {
            Duration::from_millis(500)
        } else {
            TEST_WAIT
        };
        let failed = client
            .get_physical_network_info_reusing_pipe(&mut connection, operation, budget)
            .await;
        match fault {
            FailedReply::WrongRequestId => {
                assert!(matches!(failed, Err(WindowsVpnError::ResponseIdMismatch)))
            }
            FailedReply::Oversized => {
                assert!(matches!(failed, Err(WindowsVpnError::FrameTooLarge(_))))
            }
            FailedReply::PartialTimeout => {
                assert!(matches!(failed, Err(WindowsVpnError::RpcTimeout)))
            }
            FailedReply::UnexpectedPayload => assert!(matches!(
                failed,
                Err(WindowsVpnError::UnexpectedResponse(_))
            )),
            FailedReply::OwnerMismatch | FailedReply::ShuttingDown => {
                assert!(matches!(failed, Err(WindowsVpnError::Remote { .. })))
            }
            FailedReply::Eof => assert!(matches!(failed, Err(WindowsVpnError::Io(_)))),
        }
        assert!(connection.is_none(), "retained a failed stream: {fault:?}");
        assert_eq!(
            client
                .get_physical_network_info_reusing_pipe(&mut connection, operation, TEST_WAIT)
                .await
                .unwrap()
                .generation,
            8
        );
        assert_eq!(accepts.load(Ordering::SeqCst), 2);
        drop(connection);
        timeout(TEST_WAIT, server).await.unwrap().unwrap();
    }
}

#[tokio::test]
async fn cancelling_an_inflight_reused_query_closes_its_partial_pipe() {
    let (client, mut pipe, _) = fixture();
    let operation = Uuid::new_v4();
    let (entered, observed) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        pipe.connect().await.unwrap();
        let first = read_request(&mut pipe).await;
        write_response(
            &mut pipe,
            first.request_id,
            agent_response::Payload::PhysicalNetworkInfo(physical_info(7)),
        )
        .await;
        let _ = read_request(&mut pipe).await;
        pipe.write_all(&[0, 0, 0, 64, 1]).await.unwrap();
        entered.send(()).unwrap();
        assert_eof(&mut pipe).await;
    });
    let mut connection = None;
    client
        .get_physical_network_info_reusing_pipe(&mut connection, operation, TEST_WAIT)
        .await
        .unwrap();
    let mut query = Box::pin(client.get_physical_network_info_reusing_pipe(
        &mut connection,
        operation,
        TEST_WAIT,
    ));
    tokio::select! {
        result = &mut query => panic!("query unexpectedly completed: {result:?}"),
        ready = observed => ready.unwrap(),
    }
    drop(query);
    assert!(connection.is_none());
    timeout(TEST_WAIT, server).await.unwrap().unwrap();
}

#[tokio::test]
async fn lease_readback_reuses_acquisition_pipe_and_holds_permit_until_drop() {
    for (snapshot_generation, interface_luid, succeeds) in
        [(7, 17, true), (8, 17, false), (7, 18, false)]
    {
        let (client, mut pipe, _) = fixture();
        let operation = Uuid::new_v4();
        let remote: SocketAddr = "198.51.100.8:443".parse().unwrap();
        let (readback, seen) = tokio::sync::oneshot::channel();
        let (released, ended) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(async move {
            pipe.connect().await.unwrap();
            let acquire = read_request(&mut pipe).await;
            assert!(
                matches!(acquire.payload, Some(agent_request::Payload::AcquireDirectEgress(ref request))
                if request.expected_generation == 7 && request.operation_id == operation.to_string()
                    && request.remote_endpoint == remote.to_string() && request.protocol == 6)
            );
            write_response(
                &mut pipe,
                acquire.request_id,
                agent_response::Payload::DirectEgressLease(AgentDirectEgressLease {
                    remote_endpoint: remote.to_string(),
                    protocol: 6,
                    interface_luid: 17,
                    interface_index: 23,
                    network_generation: 7,
                    ..Default::default()
                }),
            )
            .await;
            let snapshot = read_request(&mut pipe).await;
            assert!(
                matches!(snapshot.payload, Some(agent_request::Payload::GetPhysicalNetworkInfo(ref request))
                if request.operation_id == operation.to_string())
            );
            let mut info = physical_info(snapshot_generation);
            info.interfaces[0].interface_luid = interface_luid;
            write_response(
                &mut pipe,
                snapshot.request_id,
                agent_response::Payload::PhysicalNetworkInfo(info),
            )
            .await;
            readback.send(()).unwrap();
            assert_eof(&mut pipe).await;
            released.send(()).unwrap();
        });
        let protector = protector(client.clone(), operation);
        protector.verify_generation(10, 7).unwrap();
        let (pipe, lease) = client
            .acquire_direct_egress(
                operation,
                remote,
                DirectProtocol::Tcp,
                7,
                agent_v1::DirectEgressPurpose::Unspecified,
            )
            .await
            .unwrap();
        // The production post-bind path is exercised without performing a
        // native interface bind or changing any host networking.
        let result = protector
            .finish_direct_egress(
                pipe,
                DirectEgressReadback {
                    lease: &lease,
                    remote,
                    expected_generation: 10,
                    agent_generation: 7,
                    purpose: agent_v1::DirectEgressPurpose::Unspecified,
                },
            )
            .await;
        seen.await.unwrap();
        let mut ended = Box::pin(ended);
        if succeeds {
            let held = result.unwrap();
            assert_eq!(held.generation(), Some(10));
            assert!(
                timeout(Duration::from_millis(30), &mut ended)
                    .await
                    .is_err()
            );
            drop(held);
        } else {
            assert_eq!(result.unwrap_err(), STALE_GENERATION_REASON);
        }
        timeout(TEST_WAIT, &mut ended).await.unwrap().unwrap();
        timeout(TEST_WAIT, server).await.unwrap().unwrap();
    }
}

#[tokio::test]
async fn cancelling_lease_readback_closes_the_acquired_lease_pipe() {
    let (client, mut pipe, _) = fixture();
    let operation = Uuid::new_v4();
    let remote: SocketAddr = "198.51.100.8:443".parse().unwrap();
    let (entered, observed) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        pipe.connect().await.unwrap();
        let request = read_request(&mut pipe).await;
        assert!(matches!(
            request.payload,
            Some(agent_request::Payload::AcquireDirectEgress(_))
        ));
        write_response(
            &mut pipe,
            request.request_id,
            agent_response::Payload::DirectEgressLease(AgentDirectEgressLease {
                remote_endpoint: remote.to_string(),
                protocol: 6,
                interface_luid: 17,
                interface_index: 23,
                network_generation: 7,
                ..Default::default()
            }),
        )
        .await;
        let readback = read_request(&mut pipe).await;
        assert!(matches!(
            readback.payload,
            Some(agent_request::Payload::GetPhysicalNetworkInfo(_))
        ));
        pipe.write_all(&[0, 0, 0, 64, 1]).await.unwrap();
        entered.send(()).unwrap();
        assert_eof(&mut pipe).await;
    });
    let protector = protector(client.clone(), operation);
    let (pipe, lease) = client
        .acquire_direct_egress(
            operation,
            remote,
            DirectProtocol::Tcp,
            7,
            agent_v1::DirectEgressPurpose::Unspecified,
        )
        .await
        .unwrap();
    let mut readback = Box::pin(protector.finish_direct_egress(
        pipe,
        DirectEgressReadback {
            lease: &lease,
            remote,
            expected_generation: 10,
            agent_generation: 7,
            purpose: agent_v1::DirectEgressPurpose::Unspecified,
        },
    ));
    tokio::select! {
        result = &mut readback => panic!("partial response unexpectedly completed: {result:?}"),
        ready = observed => ready.unwrap(),
    }
    drop(readback);
    timeout(TEST_WAIT, server).await.unwrap().unwrap();
}
