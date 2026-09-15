use super::*;

struct Peer {
    state: AgentState,
    acquired: usize,
    created: usize,
    ended: usize,
    retired: usize,
    retirement_pending: bool,
    network_pending: bool,
}

fn capabilities() -> AgentCapabilities {
    AgentCapabilities {
        protocol_version: AGENT_PROTOCOL_VERSION,
        reusable_tun_device: true,
        wintun: true,
        interface_addresses: true,
        interface_dns: true,
        shared_packet_ring: true,
        dynamic_direct_egress: true,
        physical_dns_snapshot: true,
        exact_generation_egress: true,
        automatic_recovery: true,
        guarded_recovery: true,
        wfp_kill_switch: true,
        ..Default::default()
    }
}

struct DeviceAgent {
    client: WindowsAgentClient,
    peer: Arc<tokio::sync::Mutex<Peer>>,
    stop: CancellationToken,
    task: JoinHandle<()>,
}

impl DeviceAgent {
    fn start() -> Self {
        let name = format!("{AGENT_PIPE_NAME}.device-{}", Uuid::new_v4());
        let mut next = ServerOptions::new()
            .first_pipe_instance(true)
            .create(&name)
            .unwrap();
        let client = WindowsAgentClient::for_test(name.clone());
        let peer = Arc::new(tokio::sync::Mutex::new(Peer {
            state: AgentState {
                phase: agent_v1::AgentPhase::Clean as i32,
                journal_generation: 19,
                device: Some(agent_v1::ManagedDeviceStatus {
                    phase: agent_v1::ManagedDevicePhase::Absent as i32,
                    ..Default::default()
                }),
                ..Default::default()
            },
            acquired: 0,
            created: 0,
            ended: 0,
            retired: 0,
            retirement_pending: false,
            network_pending: false,
        }));
        let stop = CancellationToken::new();
        let stopping = stop.clone();
        let shared = Arc::clone(&peer);
        let task = tokio::spawn(async move {
            let mut workers = tokio::task::JoinSet::new();
            loop {
                tokio::select! {
                    () = stopping.cancelled() => break,
                    result = next.connect() => result.unwrap(),
                }
                let mut pipe = next;
                next = ServerOptions::new().create(&name).unwrap();
                let state = Arc::clone(&shared);
                let stop = stopping.clone();
                workers.spawn(async move {
                    loop {
                        let mut header = [0; 4];
                        tokio::select! {
                            () = stop.cancelled() => break,
                            read = pipe.read_exact(&mut header) => if read.is_err() { break; },
                        }
                        let mut payload = vec![0; u32::from_be_bytes(header) as usize];
                        pipe.read_exact(&mut payload).await.unwrap();
                        let mut frame = BytesMut::from(header.as_slice());
                        frame.extend_from_slice(&payload);
                        let request: AgentRequest = decode_frame(frame.freeze()).unwrap();
                        let response = state.lock().await.respond(request);
                        if pipe
                            .write_all(&encode_frame(&response).unwrap())
                            .await
                            .is_err()
                        {
                            break;
                        }
                    }
                });
            }
            while let Some(result) = workers.join_next().await {
                result.unwrap();
            }
        });
        Self {
            client,
            peer,
            stop,
            task,
        }
    }

    async fn finish(self) {
        self.stop.cancel();
        self.task.await.unwrap();
    }
}

impl Peer {
    fn respond(&mut self, request: AgentRequest) -> AgentResponse {
        use agent_request::Payload as Request;
        use agent_response::Payload as Response;
        use agent_v1::{AgentPhase, ManagedDevicePhase};
        let payload = match request.payload.unwrap() {
            Request::GetCapabilities(_) => Response::Capabilities(capabilities()),
            Request::GetState(_) => Response::State(self.state.clone()),
            Request::AcquireDeviceLease(_) => {
                self.acquired += 1;
                assert_eq!(
                    self.acquired, 1,
                    "normal disconnect must retain the original lease"
                );
                self.state.device.as_mut().unwrap().lease_attached = true;
                Response::DeviceLease(test_device_lease())
            }
            Request::PrepareTunnel(prepare) => {
                assert_eq!(self.state.phase, AgentPhase::Clean as i32);
                assert_eq!(prepare.device_lease_id, test_device_lease().lease_id);
                assert_eq!(
                    prepare.device_lease_generation,
                    test_device_lease().lease_generation
                );
                assert_eq!(
                    prepare.expected_journal_generation,
                    self.state.journal_generation
                );
                if self.state.device.as_ref().unwrap().phase == ManagedDevicePhase::Absent as i32 {
                    self.created += 1;
                }
                self.state.device.as_mut().unwrap().phase = ManagedDevicePhase::InUse as i32;
                self.state.phase = AgentPhase::Prepared as i32;
                self.state.operation_id = prepare.operation_id;
                self.state.profile_id = prepare.plan.unwrap().profile_id;
                self.state.journal_generation += 1;
                Response::State(self.state.clone())
            }
            Request::OpenPacketSession(open) => {
                assert_eq!(open.operation_id, self.state.operation_id);
                assert!(!self.state.packet_session_active);
                self.state.packet_session_active = true;
                // Opaque fixtures only; these handles are never mapped or used.
                Response::PacketSession(PacketSessionHandles::default())
            }
            Request::CommitTunnel(commit) => {
                assert_eq!(commit.operation_id, self.state.operation_id);
                assert!(self.state.packet_session_active);
                self.state.phase = AgentPhase::Active as i32;
                Response::State(self.state.clone())
            }
            Request::RollbackTunnel(rollback) => {
                assert_eq!(rollback.operation_id, self.state.operation_id);
                self.ended += usize::from(self.state.packet_session_active);
                self.state.packet_session_active = false;
                self.state.operation_id.clear();
                self.state.profile_id.clear();
                self.state.phase = AgentPhase::Clean as i32;
                self.state.device.as_mut().unwrap().phase = ManagedDevicePhase::Idle as i32;
                self.state.journal_generation += 1;
                Response::State(self.state.clone())
            }
            Request::ReleaseDeviceLease(release) => {
                assert_eq!(release.lease_id, test_device_lease().lease_id);
                assert_eq!(
                    release.lease_generation,
                    test_device_lease().lease_generation
                );
                self.retired += 1;
                self.state.device.as_mut().unwrap().lease_attached = false;
                self.state.device.as_mut().unwrap().phase = if self.retirement_pending {
                    ManagedDevicePhase::RecoveryRequired
                } else {
                    ManagedDevicePhase::Absent
                } as i32;
                if self.network_pending {
                    self.state.phase = AgentPhase::RecoveryRequired as i32;
                }
                Response::State(self.state.clone())
            }
            other => panic!("unexpected device lifecycle request: {other:?}"),
        };
        AgentResponse {
            request_id: request.request_id,
            payload: Some(payload),
            error: None,
        }
    }
}

#[tokio::test]
async fn application_owner_survives_hundred_connections_and_disconnects_until_shutdown() {
    let agent = DeviceAgent::start();
    let (_dir, service, _) = recovery_entry_service().await;
    for _ in 0..100 {
        let state = agent
            .client
            .connection_state(&capabilities())
            .await
            .unwrap();
        let grant = service
            .windows_device
            .acquire(&agent.client, &capabilities())
            .await
            .unwrap();
        let operation = Uuid::new_v4();
        let startup = agent
            .client
            .prepare(
                operation,
                agent_v1::TunnelPlan {
                    profile_id: Uuid::new_v4().to_string(),
                    ..Default::default()
                },
                &grant,
                state.journal_generation,
            )
            .await
            .unwrap();
        agent
            .client
            .open_packet_session(operation, DEFAULT_PACKET_RING_CAPACITY)
            .await
            .unwrap();
        agent.client.commit(operation).await.unwrap();
        agent
            .client
            .rollback_for_disconnect(operation)
            .await
            .unwrap();
        drop(startup);
        service.disconnect().await.unwrap();
        assert_eq!(agent.peer.lock().await.retired, 0);
    }
    assert_eq!(agent.peer.lock().await.created, 1);
    assert_eq!(agent.peer.lock().await.ended, 100);
    service.shutdown().await.unwrap();
    assert_eq!(agent.peer.lock().await.retired, 1);
    assert!(
        service
            .windows_device
            .acquire(&agent.client, &capabilities())
            .await
            .is_err()
    );
    agent.finish().await;
}

#[tokio::test]
async fn application_exit_accepts_only_durable_device_retirement_pending() {
    for network_pending in [false, true] {
        let agent = DeviceAgent::start();
        let owner = WindowsDeviceOwner::default();
        owner.acquire(&agent.client, &capabilities()).await.unwrap();
        {
            let mut peer = agent.peer.lock().await;
            peer.retirement_pending = true;
            peer.network_pending = network_pending;
        }
        assert_eq!(owner.shutdown().await.is_ok(), !network_pending);
        assert!(owner.acquire(&agent.client, &capabilities()).await.is_err());
        agent.finish().await;
    }
}

#[tokio::test]
async fn capable_agent_must_supply_device_state_before_admission() {
    let agent = DeviceAgent::start();
    agent.peer.lock().await.state.device = None;
    assert!(matches!(
        agent.client.connection_state(&capabilities()).await,
        Err(WindowsVpnError::DeviceRecoveryRequired)
    ));
    assert_eq!(agent.peer.lock().await.acquired, 0);
    agent.finish().await;
}

#[tokio::test]
async fn unused_owner_never_contacts_agent_and_old_capability_cannot_fall_back() {
    let owner = WindowsDeviceOwner::default();
    let client =
        WindowsAgentClient::for_test(format!("{AGENT_PIPE_NAME}.missing-{}", Uuid::new_v4()));
    assert!(matches!(
        owner.acquire(&client, &AgentCapabilities::default()).await,
        Err(WindowsVpnError::DeviceReuseUnsupported)
    ));
    owner.shutdown().await.unwrap();
}

#[test]
fn a_clean_connection_with_an_unusable_device_cannot_admit_a_new_session() {
    for phase in [
        agent_v1::ManagedDevicePhase::Creating,
        agent_v1::ManagedDevicePhase::InUse,
        agent_v1::ManagedDevicePhase::Retiring,
        agent_v1::ManagedDevicePhase::RecoveryRequired,
        agent_v1::ManagedDevicePhase::Unspecified,
    ] {
        let state = AgentState {
            phase: agent_v1::AgentPhase::Clean as i32,
            device: Some(agent_v1::ManagedDeviceStatus {
                phase: phase as i32,
                ..Default::default()
            }),
            ..Default::default()
        };
        assert!(matches!(
            require_recovered_state(&state),
            Err(WindowsVpnError::DeviceRecoveryRequired)
        ));
    }
}
