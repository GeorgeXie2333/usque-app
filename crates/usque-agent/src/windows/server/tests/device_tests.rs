use super::*;
use tokio::net::windows::named_pipe::NamedPipeClient;

async fn exchange(pipe: &mut NamedPipeClient, payload: agent_request::Payload) -> AgentResponse {
    let request = AgentRequest {
        request_id: Uuid::new_v4().to_string(),
        protocol_version: AGENT_PROTOCOL_VERSION,
        payload: Some(payload),
    };
    pipe.write_all(&encode_frame(&request).unwrap())
        .await
        .unwrap();
    let mut header = [0; 4];
    pipe.read_exact(&mut header).await.unwrap();
    let mut payload = vec![0; u32::from_be_bytes(header) as usize];
    pipe.read_exact(&mut payload).await.unwrap();
    let mut frame = BytesMut::from(header.as_slice());
    frame.extend_from_slice(&payload);
    decode_frame(frame.freeze()).unwrap()
}

async fn client(
    service: Arc<AgentService<RejectingBackend>>,
) -> (NamedPipeClient, tokio::task::JoinHandle<()>) {
    let name = format!("{AGENT_PIPE_NAME}.device-test-{}", Uuid::new_v4());
    let pipe = create_agent_pipe(&name, true).unwrap();
    let policy =
        Arc::new(CallerPolicy::new(vec![std::env::current_exe().unwrap()], None, true).unwrap());
    let task = tokio::spawn(async move {
        pipe.connect().await.unwrap();
        handle_connected_pipe(pipe, service, policy).await.unwrap();
    });
    (ClientOptions::new().open(&name).unwrap(), task)
}

async fn acquire(pipe: &mut NamedPipeClient) -> agent_v1::DeviceLease {
    let response = exchange(
        pipe,
        agent_request::Payload::AcquireDeviceLease(agent_v1::AcquireDeviceLeaseRequest {}),
    )
    .await;
    assert!(response.error.is_none(), "{:?}", response.error);
    let Some(agent_response::Payload::DeviceLease(lease)) = response.payload else {
        panic!("missing lease");
    };
    lease
}

async fn release(pipe: &mut NamedPipeClient, lease: &agent_v1::DeviceLease) {
    let response = exchange(
        pipe,
        agent_request::Payload::ReleaseDeviceLease(agent_v1::ReleaseDeviceLeaseRequest {
            lease_id: lease.lease_id.clone(),
            lease_generation: lease.lease_generation,
        }),
    )
    .await;
    assert!(response.error.is_none(), "{:?}", response.error);
}

#[tokio::test]
async fn authenticated_device_lease_survives_an_old_pipes_eof_and_is_lazy() {
    let directory = tempfile::tempdir().unwrap();
    let coordinator = Arc::new(
        AgentCoordinator::open(
            JournalStore::new(directory.path().join("recovery.json")),
            Arc::new(RejectingBackend),
        )
        .unwrap(),
    );
    let service = Arc::new(AgentService::new(
        Arc::clone(&coordinator),
        AgentCapabilities {
            reusable_tun_device: true,
            ..Default::default()
        },
    ));
    let (mut first, first_task) = client(Arc::clone(&service)).await;
    let old = acquire(&mut first).await;
    assert!(coordinator.device_lease_attached());
    assert!(coordinator.state().await.device.is_none());
    let (mut second, second_task) = client(service).await;
    let rejected = exchange(
        &mut second,
        agent_request::Payload::AcquireDeviceLease(agent_v1::AcquireDeviceLeaseRequest {}),
    )
    .await;
    assert_eq!(rejected.error.unwrap().code, "AGENT_DEVICE_LEASE_REQUIRED");
    release(&mut second, &old).await;
    let current = acquire(&mut second).await;
    assert_ne!(current.lease_id, old.lease_id);
    let stale_release = exchange(
        &mut second,
        agent_request::Payload::ReleaseDeviceLease(agent_v1::ReleaseDeviceLeaseRequest {
            lease_id: old.lease_id.clone(),
            lease_generation: old.lease_generation,
        }),
    )
    .await;
    assert_eq!(
        stale_release.error.unwrap().code,
        "AGENT_DEVICE_LEASE_REQUIRED"
    );
    drop(first);
    tokio::time::timeout(Duration::from_secs(2), first_task)
        .await
        .unwrap()
        .unwrap();
    assert!(coordinator.device_lease_attached());
    assert!(!coordinator.may_exit_idle().await);
    release(&mut second, &current).await;
    drop(second);
    tokio::time::timeout(Duration::from_secs(2), second_task)
        .await
        .unwrap()
        .unwrap();
    assert!(coordinator.may_exit_idle().await);
}

#[tokio::test]
async fn old_prepare_shape_is_rejected_before_any_native_work() {
    let directory = tempfile::tempdir().unwrap();
    let coordinator = Arc::new(
        AgentCoordinator::open(
            JournalStore::new(directory.path().join("recovery.json")),
            Arc::new(RejectingBackend),
        )
        .unwrap(),
    );
    let service = AgentService::new(Arc::clone(&coordinator), AgentCapabilities::default());
    let response = service
        .handle(
            AgentRequest {
                request_id: "old-prepare".into(),
                protocol_version: AGENT_PROTOCOL_VERSION,
                payload: Some(agent_request::Payload::PrepareTunnel(
                    agent_v1::PrepareTunnelRequest {
                        operation_id: Uuid::new_v4().to_string(),
                        plan: Some(egress_plan().to_proto()),
                        ..Default::default()
                    },
                )),
            },
            &test_caller(),
        )
        .await;
    assert_eq!(response.error.unwrap().code, "AGENT_DEVICE_LEASE_REQUIRED");
    assert!(coordinator.state().await.is_fully_clean());
}
