use super::*;
use tokio::net::windows::named_pipe::NamedPipeClient;
use tokio::time::timeout;

const TEST_WAIT: Duration = Duration::from_secs(5);

async fn request(pipe: &mut NamedPipeClient, payload: agent_request::Payload) -> AgentResponse {
    let request = AgentRequest {
        request_id: Uuid::new_v4().to_string(),
        protocol_version: AGENT_PROTOCOL_VERSION,
        payload: Some(payload),
    };
    timeout(TEST_WAIT, async {
        pipe.write_all(&encode_frame(&request).unwrap())
            .await
            .unwrap();
        let mut header = [0; 4];
        pipe.read_exact(&mut header).await.unwrap();
        let size = u32::from_be_bytes(header) as usize;
        assert!(size <= MAX_AGENT_FRAME_BYTES);
        let mut payload = vec![0; size];
        pipe.read_exact(&mut payload).await.unwrap();
        let mut frame = BytesMut::from(header.as_slice());
        frame.extend_from_slice(&payload);
        let response: AgentResponse = decode_frame(frame.freeze()).unwrap();
        assert_eq!(response.request_id, request.request_id);
        response
    })
    .await
    .expect("authenticated read-only request timed out")
}

#[tokio::test]
async fn authenticated_read_only_connection_reuse_does_not_own_or_revoke_egress_leases() {
    let directory = tempfile::tempdir().unwrap();
    let coordinator = Arc::new(
        AgentCoordinator::open(
            JournalStore::new(directory.path().join("journal.json")),
            Arc::new(RejectingBackend),
        )
        .unwrap(),
    );
    let service = Arc::new(AgentService::new(
        coordinator,
        AgentCapabilities {
            protocol_version: AGENT_PROTOCOL_VERSION,
            ..Default::default()
        },
    ));
    let key = DirectEgressKey {
        operation_id: Uuid::new_v4(),
        remote: "198.51.100.8:443".parse().unwrap(),
        protocol: 6,
        interface_luid: 17,
        network_generation: 7,
        purpose: EgressPurpose::Generic,
    };
    // Memory-only registry: no WFP permit, routes, interface lookup or TUN.
    service.direct_egress.lock().await.entries.insert(
        key,
        DirectEgressEntry {
            references: 2,
            _permit: None,
        },
    );
    let name = format!("{AGENT_PIPE_NAME}.readonly-reuse-{}", Uuid::new_v4());
    let pipe = create_agent_pipe(&name, true).unwrap();
    let policy =
        Arc::new(CallerPolicy::new(vec![std::env::current_exe().unwrap()], None, true).unwrap());
    let shared = service.clone();
    let server = tokio::spawn(async move {
        pipe.connect().await.unwrap();
        handle_connected_pipe(pipe, shared, policy).await.unwrap();
    });
    let mut client = ClientOptions::new().open(&name).unwrap();
    for _ in 0..3 {
        let caps = request(
            &mut client,
            agent_request::Payload::GetCapabilities(GetCapabilitiesRequest {}),
        )
        .await;
        assert!(caps.error.is_none());
        assert!(matches!(
            caps.payload,
            Some(agent_response::Payload::Capabilities(_))
        ));
        let state = request(
            &mut client,
            agent_request::Payload::GetState(agent_v1::GetStateRequest {}),
        )
        .await;
        assert!(state.error.is_none());
        assert!(matches!(
            state.payload,
            Some(agent_response::Payload::State(_))
        ));
        assert_eq!(
            service.direct_egress.lock().await.entries[&key].references,
            2
        );
    }
    drop(client);
    timeout(TEST_WAIT, server).await.unwrap().unwrap();
    assert_eq!(
        service.direct_egress.lock().await.entries[&key].references,
        2
    );
    // Explicit release still respects each independent lease reference.
    service.release_direct_egress(key).await;
    assert_eq!(
        service.direct_egress.lock().await.entries[&key].references,
        1
    );
    service.release_direct_egress(key).await;
    assert!(service.direct_egress.lock().await.entries.is_empty());
}

#[tokio::test]
async fn each_replacement_pipe_still_authenticates_the_callers_image() {
    let directory = tempfile::tempdir().unwrap();
    let coordinator = Arc::new(
        AgentCoordinator::open(
            JournalStore::new(directory.path().join("journal.json")),
            Arc::new(RejectingBackend),
        )
        .unwrap(),
    );
    let service = Arc::new(AgentService::new(coordinator, AgentCapabilities::default()));
    let other_image = directory.path().join("other-engine.bin");
    std::fs::write(&other_image, b"inert authentication fixture").unwrap();
    for accepted in [true, false, false] {
        let name = format!("{AGENT_PIPE_NAME}.rejected-reuse-{}", Uuid::new_v4());
        let pipe = create_agent_pipe(&name, true).unwrap();
        // The real test executable must not inherit authentication merely
        // because another connection from the same PID was accepted earlier.
        let executable = std::env::current_exe().unwrap();
        let allowed_path = if accepted {
            executable
        } else {
            other_image.clone()
        };
        let policy = Arc::new(CallerPolicy::new(vec![allowed_path], None, true).unwrap());
        let shared = service.clone();
        let server = tokio::spawn(async move {
            pipe.connect().await.unwrap();
            handle_connected_pipe(pipe, shared, policy).await
        });
        let mut client = ClientOptions::new().open(&name).unwrap();
        if accepted {
            let response = request(
                &mut client,
                agent_request::Payload::GetCapabilities(GetCapabilitiesRequest {}),
            )
            .await;
            assert!(response.error.is_none());
            drop(client);
            timeout(TEST_WAIT, server).await.unwrap().unwrap().unwrap();
        } else {
            // Supply a message so the real Named Pipe impersonation path can
            // inspect this process, rather than failing only for lack of data.
            let _ = client
                .write_all(
                    &encode_frame(&AgentRequest {
                        request_id: Uuid::new_v4().to_string(),
                        protocol_version: AGENT_PROTOCOL_VERSION,
                        payload: Some(agent_request::Payload::GetCapabilities(
                            GetCapabilitiesRequest {},
                        )),
                    })
                    .unwrap(),
                )
                .await;
            let result = timeout(TEST_WAIT, server).await.unwrap().unwrap();
            assert!(matches!(
                result,
                Err(ServerError::Authentication(
                    AuthenticationError::UnexpectedExecutable(_)
                ))
            ));
            drop(client);
        }
    }
    assert!(service.direct_egress.lock().await.entries.is_empty());
}
