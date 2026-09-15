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

async fn client<Backend: PrivilegedBackend + 'static>(
    service: Arc<AgentService<Backend>>,
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

#[tokio::test]
async fn device_persistence_retry_finishes_without_a_client_or_connection_budget() {
    for (successful_saves, native_failure) in [(0, false), (1, false), (1, true)] {
        let directory = tempfile::tempdir().unwrap();
        let store = JournalStore::new(directory.path().join("recovery.json"));
        let backend = Arc::new(ScriptedRecoveryBackend::transient(usize::from(
            native_failure,
        )));
        let coordinator =
            Arc::new(AgentCoordinator::open(store.clone(), Arc::clone(&backend)).unwrap());
        let service = Arc::new(AgentService::new(
            Arc::clone(&coordinator),
            automatic_recovery_capabilities(),
        ));
        let (mut pipe, pipe_task) = client(Arc::clone(&service)).await;
        let lease = acquire(&mut pipe).await;
        // The memory backend creates a device, then rejects network planning.
        // Prepare rolls back the empty connection and retains the idle device.
        let prepared = exchange(
            &mut pipe,
            agent_request::Payload::PrepareTunnel(agent_v1::PrepareTunnelRequest {
                operation_id: Uuid::new_v4().to_string(),
                plan: Some(egress_plan().to_proto()),
                device_lease_id: lease.lease_id.clone(),
                device_lease_generation: lease.lease_generation,
                expected_journal_generation: lease.journal_generation,
            }),
        )
        .await;
        assert!(prepared.error.is_some());
        assert_eq!(
            coordinator.state().await.device.unwrap().state,
            crate::journal::DeviceState::Idle
        );
        let worker = tokio::spawn(Arc::clone(&service).run_automatic_recovery());
        tokio::task::yield_now().await;
        store.fail_clean_save_after(successful_saves);
        let released = exchange(
            &mut pipe,
            agent_request::Payload::ReleaseDeviceLease(agent_v1::ReleaseDeviceLeaseRequest {
                lease_id: lease.lease_id,
                lease_generation: lease.lease_generation,
            }),
        )
        .await;
        assert!(released.error.is_some());
        assert!(!coordinator.may_exit_idle().await);
        drop(pipe);
        pipe_task.await.unwrap();
        // Keep real pipe I/O outside virtual time: auto-advancing past its
        // readiness notification could finish the retry before the error reply.
        tokio::time::pause();
        tokio::time::timeout(Duration::from_secs(30), async {
            while !coordinator.may_exit_idle().await {
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        })
        .await
        .expect("disk recovery must wake the supervisor without another client request");
        assert_eq!(backend.restore_calls.load(Ordering::Acquire), 1);
        assert_eq!(
            store.load_or_clean().unwrap().is_fully_clean(),
            !native_failure
        );
        assert_eq!(
            service.automatic_recovery.lock().await.stage,
            AutomaticRecoveryStage::Inactive
        );
        assert_eq!(
            service.automatic_recovery.lock().await.attempts_completed,
            0
        );
        service.begin_shutdown();
        worker.await.unwrap();
        tokio::time::resume();
    }
}
