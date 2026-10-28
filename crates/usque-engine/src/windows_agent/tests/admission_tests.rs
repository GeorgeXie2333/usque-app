use super::*;

fn admission_capabilities() -> AgentCapabilities {
    AgentCapabilities {
        protocol_version: AGENT_PROTOCOL_VERSION,
        reusable_tun_device: true,
        ..Default::default()
    }
}

fn capability_reply(capabilities: AgentCapabilities) -> AgentResponse {
    AgentResponse {
        payload: Some(agent_response::Payload::Capabilities(capabilities)),
        ..Default::default()
    }
}

fn clean_reply() -> AgentResponse {
    AgentResponse {
        payload: Some(agent_response::Payload::State(AgentState {
            phase: agent_v1::AgentPhase::Clean as i32,
            journal_generation: 1,
            device: Some(agent_v1::ManagedDeviceStatus {
                phase: agent_v1::ManagedDevicePhase::Absent as i32,
                ..Default::default()
            }),
            ..Default::default()
        })),
        ..Default::default()
    }
}

fn refusal(code: &str) -> AgentResponse {
    AgentResponse {
        error: Some(agent_v1::AgentError {
            code: code.into(),
            retryable: false,
            ..Default::default()
        }),
        ..Default::default()
    }
}

struct AdmissionServiceStatus(u32);

#[async_trait]
impl AgentServiceController for AdmissionServiceStatus {
    async fn ensure_started(
        &self,
        _: tokio::time::Instant,
    ) -> Result<(), AgentServiceControlError> {
        Ok(())
    }
    async fn status(&self) -> Result<AgentServiceStatus, AgentServiceControlError> {
        Ok(AgentServiceStatus {
            state: self.0,
            win32_exit_code: 0,
            service_exit_code: 0,
        })
    }
}

#[tokio::test]
async fn eof_is_transient_only_with_evidence_of_a_service_transition() {
    for service_state in [SERVICE_STOP_PENDING, SERVICE_RUNNING] {
        let name = format!("{AGENT_PIPE_NAME}.admission-eof-{}", Uuid::new_v4());
        let mut first = ServerOptions::new()
            .first_pipe_instance(true)
            .create(&name)
            .unwrap();
        let client = WindowsAgentClient::for_test_with_controller(
            name.clone(),
            Arc::new(AdmissionServiceStatus(service_state)),
        );
        let peer = tokio::spawn(async move {
            let mut next = ServerOptions::new().create(&name).unwrap();
            first.connect().await.unwrap();
            // Drop a complete request without sending even a header, as a
            // service stop between connection and GetState can do.
            let mut header = [0; 4];
            first.read_exact(&mut header).await.unwrap();
            let mut payload = vec![0; u32::from_be_bytes(header) as usize];
            first.read_exact(&mut payload).await.unwrap();
            drop(first);
            let mut requests = 1;
            if service_state == SERVICE_STOP_PENDING {
                for mut response in [capability_reply(admission_capabilities()), clean_reply()] {
                    next.connect().await.unwrap();
                    let mut pipe = next;
                    next = ServerOptions::new().create(&name).unwrap();
                    let mut header = [0; 4];
                    pipe.read_exact(&mut header).await.unwrap();
                    let mut payload = vec![0; u32::from_be_bytes(header) as usize];
                    pipe.read_exact(&mut payload).await.unwrap();
                    let mut frame = BytesMut::from(header.as_slice());
                    frame.extend_from_slice(&payload);
                    let request: AgentRequest = decode_frame(frame.freeze()).unwrap();
                    assert!(matches!(
                        request.payload,
                        Some(
                            agent_request::Payload::GetState(_)
                                | agent_request::Payload::GetCapabilities(_)
                        )
                    ));
                    response.request_id = request.request_id;
                    pipe.write_all(&encode_frame(&response).unwrap())
                        .await
                        .unwrap();
                    requests += 1;
                }
            }
            requests
        });
        let capabilities = admission_capabilities();
        let result = timeout(
            Duration::from_secs(3),
            client.get_admission_state(&capabilities),
        )
        .await
        .expect("admission did not settle");
        if service_state == SERVICE_STOP_PENDING {
            require_recovered_state(&result.unwrap()).unwrap();
            assert_eq!(peer.await.unwrap(), 3);
        } else {
            assert!(matches!(result, Err(WindowsVpnError::Io(_))));
            assert_eq!(peer.await.unwrap(), 1);
        }
    }
}

#[tokio::test]
async fn device_retirement_reconnects_with_fresh_capabilities_without_mutations() {
    let capabilities = admission_capabilities();
    let (client, task) = scripted_recovery_client(vec![
        refusal("AGENT_DEVICE_RETIRING"),
        capability_reply(capabilities.clone()),
        clean_reply(),
    ]);
    let state = client.get_admission_state(&capabilities).await.unwrap();
    require_recovered_state(&state).unwrap();
    let requests = task.await.unwrap();
    assert!(matches!(
        requests.as_slice(),
        [
            agent_request::Payload::GetState(_),
            agent_request::Payload::GetCapabilities(_),
            agent_request::Payload::GetState(_),
        ]
    ));
}

#[tokio::test]
async fn stopping_service_can_finish_before_capability_negotiation() {
    let capabilities = admission_capabilities();
    let (client, task) = scripted_recovery_client(vec![
        refusal("AGENT_SHUTTING_DOWN"),
        capability_reply(capabilities.clone()),
    ]);
    assert_eq!(
        client.get_admission_capabilities().await.unwrap(),
        capabilities
    );
    assert!(
        task.await
            .unwrap()
            .iter()
            .all(|request| matches!(request, agent_request::Payload::GetCapabilities(_)))
    );
}

#[tokio::test]
async fn persistent_or_untrusted_admission_failures_are_not_retried() {
    for code in [
        "AGENT_DEVICE_RECOVERY_REQUIRED",
        "AGENT_RECOVERY_FAILED",
        "AGENT_RECOVERY_CONFLICT",
        "AGENT_OWNER_MISMATCH",
        "AGENT_PROTOCOL_MISMATCH",
    ] {
        let (client, task) = scripted_recovery_client(vec![refusal(code)]);
        assert!(matches!(
            client.get_admission_state(&admission_capabilities()).await,
            Err(WindowsVpnError::Remote { code: actual, .. }) if actual == code
        ));
        assert!(matches!(
            task.await.unwrap().as_slice(),
            [agent_request::Payload::GetState(_)]
        ));
    }
    let mut absent = clean_reply();
    if let Some(agent_response::Payload::State(state)) = &mut absent.payload {
        state.device = None;
    }
    let (client, task) = scripted_recovery_client(vec![absent]);
    assert!(matches!(
        client.connection_state(&admission_capabilities()).await,
        Err(WindowsVpnError::DeviceRecoveryRequired)
    ));
    assert_eq!(task.await.unwrap().len(), 1);
}

#[tokio::test]
async fn replacement_capability_downgrade_is_terminal_before_admission() {
    for protocol_version in [AGENT_PROTOCOL_VERSION, AGENT_PROTOCOL_VERSION + 1] {
        let mut changed = admission_capabilities();
        changed.reusable_tun_device = false;
        changed.protocol_version = protocol_version;
        let (client, task) = scripted_recovery_client(vec![
            refusal("AGENT_SHUTTING_DOWN"),
            capability_reply(changed),
        ]);
        let result = client.get_admission_state(&admission_capabilities()).await;
        if protocol_version == AGENT_PROTOCOL_VERSION {
            assert!(matches!(
                result,
                Err(WindowsVpnError::MissingCapabilities(_))
            ));
        } else {
            assert!(matches!(result, Err(WindowsVpnError::ProtocolVersion(_))));
        }
        assert!(matches!(
            task.await.unwrap().as_slice(),
            [
                agent_request::Payload::GetState(_),
                agent_request::Payload::GetCapabilities(_)
            ]
        ));
    }
}

#[tokio::test]
async fn retirement_wait_has_one_deadline_and_cancellation_closes_pending_read() {
    let (client, task) = scripted_recovery_client(vec![refusal("AGENT_DEVICE_RETIRING")]);
    assert!(matches!(
        client
            .get_admission_state_with_budget(&admission_capabilities(), Duration::from_millis(20))
            .await,
        Err(WindowsVpnError::RecoveryTimeout)
    ));
    assert_eq!(task.await.unwrap().len(), 1);

    // The actual RPC is cancelled rather than left detached on a retry task.
    let name = format!("{AGENT_PIPE_NAME}.admission-cancel-{}", Uuid::new_v4());
    let mut server = ServerOptions::new()
        .first_pipe_instance(true)
        .create(&name)
        .unwrap();
    let client = WindowsAgentClient::for_test(name);
    let (entered, ready) = tokio::sync::oneshot::channel();
    let peer = tokio::spawn(async move {
        server.connect().await.unwrap();
        let mut header = [0; 4];
        server.read_exact(&mut header).await.unwrap();
        let mut payload = vec![0; u32::from_be_bytes(header) as usize];
        server.read_exact(&mut payload).await.unwrap();
        entered.send(()).unwrap();
        let mut byte = [0];
        let result = timeout(Duration::from_secs(3), server.read(&mut byte))
            .await
            .unwrap();
        assert!(
            matches!(result, Ok(0))
                || result.is_err_and(|error| matches!(
                    error.kind(),
                    io::ErrorKind::BrokenPipe
                        | io::ErrorKind::UnexpectedEof
                        | io::ErrorKind::ConnectionReset
                ))
        );
    });
    let capabilities = admission_capabilities();
    let mut pending = Box::pin(client.get_admission_state(&capabilities));
    tokio::select! {
        _ = ready => {}
        result = &mut pending => panic!("read must remain pending: {result:?}"),
    }
    drop(pending);
    peer.await.unwrap();
}
