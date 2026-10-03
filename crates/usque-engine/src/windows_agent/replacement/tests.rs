//! Scripted named pipes only. No Agent service, TUN or platform mutation.
use super::super::tests::scripted_recovery_client;
use super::*;

fn request() -> agent_v1::ReplaceTunnelRequest {
    agent_v1::ReplaceTunnelRequest {
        source_operation_id: Uuid::new_v4().to_string(),
        operation_id: Uuid::new_v4().to_string(),
        expected_journal_generation: 42,
        plan: Some(agent_v1::TunnelPlan {
            profile_id: Uuid::new_v4().to_string(),
            control_api_candidates: vec!["198.51.100.10:443".into()],
            kill_switch: true,
            ..Default::default()
        }),
        device_lease_id: Uuid::new_v4().to_string(),
        device_lease_generation: 3,
    }
}

fn pending(request: &agent_v1::ReplaceTunnelRequest) -> AgentState {
    AgentState {
        phase: agent_v1::AgentPhase::Prepared as i32,
        operation_id: request.operation_id.clone(),
        journal_generation: 43,
        plan: request.plan.clone().map(Box::new),
        replacement: Some(Box::new(agent_v1::TunnelReplacementStatus {
            source_operation_id: request.source_operation_id.clone(),
            operation_id: request.operation_id.clone(),
            target_plan: request.plan.clone().map(Box::new),
            source_journal_generation: 42,
            phase: agent_v1::TunnelReplacementPhase::Prepared as i32,
            guard_active: true,
        })),
        ..Default::default()
    }
}
fn response(state: AgentState) -> AgentResponse {
    AgentResponse {
        payload: Some(agent_response::Payload::State(state)),
        ..Default::default()
    }
}
fn clean() -> AgentState {
    AgentState {
        phase: agent_v1::AgentPhase::Clean as i32,
        ..Default::default()
    }
}
fn runtime(agent: WindowsAgentClient, operation_id: Uuid) -> WindowsVpnRuntime {
    let lifetime = CancellationToken::new();
    let (pump_failure_tx, pump_failure) = watch::channel(None);
    WindowsVpnRuntime {
        agent,
        operation_id,
        monitor: WindowsVpnMonitor {
            tunnel: ManagedTunnelMonitor::failed(
                RuntimePath {
                    transport: usque_core::Transport::Http2,
                    endpoint_family: usque_core::AddressFamily::Ipv4,
                    ipv4_available: false,
                    ipv6_available: false,
                },
                &TransportError::TunnelClosed,
            ),
            pump_failure,
            agent_disconnected: watch::channel(false).1,
        },
        cancellation: lifetime.child_token(),
        lifetime,
        mapping: None,
        tasks: vec![],
        liveness: None,
        startup_lease: None,
        pump_failure_tx,
        listeners: vec![],
        socks5_listeners: vec![],
        http_listeners: vec![],
        system_proxy: None,
        transaction_open: true,
        held_failure: false,
        target_committed: false,
        startup_error: None,
        handoff_intent: false,
        shutdown_replacement: None,
        replacement_request: None,
        tunnel: None,
        bootstrap: None,
        socket_protector: None,
    }
}

#[tokio::test]
async fn pending_replacement_bypasses_ordinary_recovery_even_before_native_guard_confirmation() {
    for guard_active in [false, true] {
        let mut state = pending(&request());
        state.phase = agent_v1::AgentPhase::RecoveryRequired as i32;
        state.replacement.as_mut().unwrap().guard_active = guard_active;
        let (client, task) =
            scripted_recovery_client(vec![response(state.clone()), response(state.clone())]);
        let capabilities = AgentCapabilities {
            protected_tunnel_replacement: true,
            automatic_recovery: true,
            guarded_recovery: true,
            reusable_tun_device: true,
            ..Default::default()
        };
        assert!(
            client
                .automatic_recovery_preflight(&capabilities, true)
                .await
                .unwrap()
                .is_none()
        );
        assert_eq!(client.connection_state(&capabilities).await.unwrap(), state);
        assert!(
            task.await
                .unwrap()
                .iter()
                .all(|request| matches!(request, agent_request::Payload::GetState(_)))
        );
    }
}

#[tokio::test]
async fn replace_acknowledgement_binds_target_source_plan_and_real_guard_state() {
    for invalid in 0..5 {
        let request = request();
        let mut state = pending(&request);
        match invalid {
            1 => state.replacement.as_mut().unwrap().operation_id = Uuid::new_v4().to_string(),
            2 => {
                state.replacement.as_mut().unwrap().source_operation_id = Uuid::new_v4().to_string()
            }
            3 => {
                state
                    .replacement
                    .as_mut()
                    .unwrap()
                    .target_plan
                    .as_mut()
                    .unwrap()
                    .kill_switch = false
            }
            4 => state.replacement.as_mut().unwrap().guard_active = false,
            _ => {}
        }
        let (client, task) = scripted_recovery_client(vec![response(state)]);
        let result = client.replace_tunnel_lease(request.clone()).await;
        assert_eq!(result.is_ok(), invalid == 0);
        assert!(
            matches!(task.await.unwrap().as_slice(), [agent_request::Payload::ReplaceTunnel(sent)] if sent == &request)
        );
    }
}

#[tokio::test]
async fn explicit_shutdown_aborts_only_current_pending_target_and_rolls_back_completed_target() {
    for complete in [false, true] {
        let request = request();
        let mut state = pending(&request);
        if complete {
            state.phase = agent_v1::AgentPhase::Active as i32;
            let replacement = state.replacement.as_mut().unwrap();
            replacement.phase = agent_v1::TunnelReplacementPhase::Complete as i32;
            replacement.guard_active = false;
        }
        let (client, task) = scripted_recovery_client(vec![response(state), response(clean())]);
        let mut runtime = runtime(client, request.operation_id.parse().unwrap());
        runtime.replacement_request = Some(request.clone());
        runtime.shutdown().await.unwrap();
        let requests = task.await.unwrap();
        assert!(matches!(&requests[0], agent_request::Payload::GetState(_)));
        if complete {
            assert!(
                matches!(&requests[1], agent_request::Payload::RollbackTunnel(sent) if sent.operation_id == request.operation_id)
            );
        } else {
            assert!(
                matches!(&requests[1], agent_request::Payload::AbortReplacement(sent) if sent.operation_id == request.operation_id && sent.expected_journal_generation == 43)
            );
        }
        assert!(!runtime.transaction_open);
    }
}

#[tokio::test]
async fn stale_source_or_unknown_request_cannot_abort_a_newer_retarget() {
    let old = request();
    let mut new = request();
    new.source_operation_id = old.operation_id.clone();
    let (client, task) = scripted_recovery_client(vec![response(pending(&new))]);
    let mut runtime = runtime(client, old.operation_id.parse().unwrap());
    runtime.replacement_request = Some(old);
    assert!(matches!(
        runtime.shutdown().await,
        Err(WindowsVpnError::RecoveryConflict)
    ));
    assert!(matches!(
        task.await.unwrap().as_slice(),
        [agent_request::Payload::GetState(_)]
    ));
    assert!(runtime.transaction_open);
}

#[tokio::test]
async fn failed_gate_retains_lifetime_and_never_sends_cleanup_rpc() {
    let request = request();
    for confirmed in [false, true] {
        let mut state = pending(&request);
        state.replacement.as_mut().unwrap().guard_active = confirmed;
        let (client, task) = scripted_recovery_client(vec![response(state)]);
        let mut runtime = runtime(client, request.operation_id.parse().unwrap());
        runtime.replacement_request = Some(request.clone());
        assert_eq!(
            runtime
                .retain_failed_gate(usque_core::vpngate::GateFailure::Transport)
                .await
                .is_ok(),
            confirmed
        );
        assert!(runtime.failure_retained());
        assert!(!runtime.lifetime.is_cancelled());
        assert!(runtime.transaction_open);
        assert!(matches!(
            task.await.unwrap().as_slice(),
            [agent_request::Payload::GetState(_)]
        ));
    }
}

#[test]
fn only_an_active_exact_target_with_completed_handoff_changes_applied_preference() {
    let request = request();
    let target = request.operation_id.parse().unwrap();
    let mut state = pending(&request);
    state.phase = agent_v1::AgentPhase::Active as i32;
    assert!(!target_is_committed(&state, target));
    state.replacement.as_mut().unwrap().phase = agent_v1::TunnelReplacementPhase::Complete as i32;
    assert!(target_is_committed(&state, target));
    assert!(!target_is_committed(&state, Uuid::new_v4()));
    state.replacement.as_mut().unwrap().phase = 999;
    assert!(pending_replacement(&state).is_err());
    assert!(!target_is_committed(&state, target));
}

#[test]
fn retained_control_candidates_are_numeric_fixed_port_and_do_not_accept_partial_parsing() {
    let request = request();
    let mut state = pending(&request);
    assert_eq!(
        state_bootstrap_candidates(&state).unwrap(),
        vec!["198.51.100.10:443".parse().unwrap()]
    );
    for value in [
        "api.cloudflareclient.com:443",
        "198.51.100.10:53",
        "0.0.0.0:443",
        "127.0.0.1:443",
    ] {
        state
            .replacement
            .as_mut()
            .unwrap()
            .target_plan
            .as_mut()
            .unwrap()
            .control_api_candidates
            .push(value.into());
        assert!(state_bootstrap_candidates(&state).is_err());
        state
            .replacement
            .as_mut()
            .unwrap()
            .target_plan
            .as_mut()
            .unwrap()
            .control_api_candidates
            .pop();
    }
}

#[tokio::test]
async fn source_sidecar_is_retired_after_replace_and_lost_reply_abort_owns_its_cleanup() {
    for lost_reply in [false, true] {
        let request = request();
        let old: Uuid = request.source_operation_id.parse().unwrap();
        let target: Uuid = request.operation_id.parse().unwrap();
        let (old_client, old_task) = scripted_recovery_client(vec![response(AgentState {
            phase: agent_v1::AgentPhase::Active as i32,
            operation_id: old.to_string(),
            system_proxy_active: true,
            ..Default::default()
        })]);
        let mut old_pipe = old_client.open_pipe().await.unwrap();
        old_client
            .exchange(
                &mut old_pipe,
                agent_request::Payload::GetState(GetStateRequest {}),
            )
            .await
            .unwrap();
        let (client, task) =
            scripted_recovery_client(vec![response(pending(&request)), response(clean())]);
        let mut runtime = runtime(client, if lost_reply { old } else { target });
        runtime.replacement_request = Some(request.clone());
        runtime.system_proxy = Some(WindowsSystemProxyGuard::from_lease(
            old_client, old, old_pipe, true,
        ));
        if !lost_reply {
            runtime.discard_retired_sidecar(&pending(&request));
            assert!(
                runtime.system_proxy.is_none(),
                "finalize must recreate the target sidecar"
            );
        }
        runtime.shutdown().await.unwrap();
        assert!(runtime.system_proxy.is_none());
        assert!(matches!(
            old_task.await.unwrap().as_slice(),
            [agent_request::Payload::GetState(_)]
        ));
        assert!(
            matches!(task.await.unwrap().as_slice(), [agent_request::Payload::GetState(_), agent_request::Payload::AbortReplacement(abort)] if abort.operation_id == request.operation_id)
        );
    }
}

#[test]
fn startup_capability_error_remains_actionable_without_releasing_its_runtime() {
    let mut runtime = runtime(
        WindowsAgentClient::for_test("unused-test-pipe".into()),
        Uuid::new_v4(),
    );
    runtime.held_failure = true;
    runtime.startup_error = Some(WindowsVpnError::MissingCapabilities(
        "protected_tunnel_replacement".into(),
    ));
    assert!(
        matches!(runtime.take_startup_error(), Some(WindowsVpnError::MissingCapabilities(value)) if value == "protected_tunnel_replacement")
    );
    assert!(runtime.take_startup_error().is_none());
    assert!(runtime.transaction_open && runtime.failure_retained());
    assert!(!runtime.lifetime.is_cancelled());
}

#[test]
fn fresh_engine_handoff_uses_chain_scope_even_when_kill_switch_preference_is_off() {
    let profile = Uuid::new_v4();
    let mut state = AgentState {
        phase: agent_v1::AgentPhase::Active as i32,
        profile_id: profile.to_string(),
        kill_switch_active: false,
        plan: Some(Box::new(agent_v1::TunnelPlan {
            vpn_chain: true,
            kill_switch: false,
            ..Default::default()
        })),
        ..Default::default()
    };
    assert!(active_chain_needs_handoff(&state, profile));
    assert!(active_chain_needs_handoff(&state, Uuid::new_v4()));
    state.plan.as_mut().unwrap().vpn_chain = false;
    assert!(!active_chain_needs_handoff(&state, profile));
    assert!(!active_chain_needs_handoff(&state, Uuid::new_v4()));
    state.kill_switch_active = true;
    assert!(active_chain_needs_handoff(&state, Uuid::new_v4()));
    state.phase = agent_v1::AgentPhase::Prepared as i32;
    assert!(!active_chain_needs_handoff(&state, Uuid::new_v4()));
}

struct UnusedPinRefresher;
#[async_trait::async_trait]
impl EndpointPinRefresher for UnusedPinRefresher {
    async fn refresh(
        &self,
        _: Arc<dyn SocketProtector>,
    ) -> Result<MasqueTlsIdentity, TransportError> {
        panic!("a missing Agent capability must not start network work")
    }
}

#[tokio::test]
async fn missing_handoff_capability_retains_a_live_ks_off_source_until_explicit_cleanup() {
    let operation = Uuid::new_v4();
    let profile = Profile {
        kill_switch: false,
        ..Default::default()
    };
    let (client, task) = scripted_recovery_client(vec![
        AgentResponse {
            payload: Some(agent_response::Payload::Capabilities(
                AgentCapabilities::default(),
            )),
            ..Default::default()
        },
        response(AgentState {
            phase: agent_v1::AgentPhase::Active as i32,
            operation_id: operation.to_string(),
            kill_switch_active: false,
            plan: Some(Box::new(agent_v1::TunnelPlan {
                vpn_chain: true,
                kill_switch: false,
                ..Default::default()
            })),
            ..Default::default()
        }),
        response(clean()),
    ]);
    let mut runtime = runtime(client, operation);
    let error = Box::pin(runtime.replace_protected_connection(
        &profile,
        super::super::tests::identity(),
        Arc::new(UnusedPinRefresher),
        Arc::new(GeoDirectPolicy::disabled()),
        None,
        watch::channel(Default::default()).0,
        &CancellationToken::new(),
        &WindowsDeviceOwner::default(),
    ))
    .await
    .unwrap_err();
    assert!(matches!(error, WindowsVpnError::MissingCapabilities(_)));
    assert!(runtime.replacement_pending());
    assert!(!runtime.target_committed());
    assert!(!runtime.lifetime.is_cancelled());
    assert!(runtime.transaction_open);
    runtime.shutdown().await.unwrap();
    assert!(!runtime.replacement_pending());
    assert!(
        matches!(task.await.unwrap().as_slice(), [agent_request::Payload::GetCapabilities(_),
        agent_request::Payload::GetState(_), agent_request::Payload::RollbackTunnel(rollback)]
        if rollback.operation_id == operation.to_string())
    );
}

fn aborted(request: &agent_v1::ReplaceTunnelRequest) -> AgentState {
    let mut state = clean();
    state.journal_generation = 60;
    let mut receipt = pending(request).replacement.unwrap();
    receipt.phase = agent_v1::TunnelReplacementPhase::Aborted as i32;
    receipt.guard_active = false;
    state.replacement = Some(receipt);
    state
}

fn lost_response_client(
    script: Vec<Option<AgentResponse>>,
) -> (WindowsAgentClient, JoinHandle<Vec<agent_request::Payload>>) {
    use tokio::net::windows::named_pipe::ServerOptions;
    let name = format!("{AGENT_PIPE_NAME}.abort-test-{}", Uuid::new_v4());
    let mut next = ServerOptions::new()
        .first_pipe_instance(true)
        .create(&name)
        .unwrap();
    let client = WindowsAgentClient::for_test(name.clone());
    let task = tokio::spawn(async move {
        let mut requests = vec![];
        for response in script {
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
            requests.push(request.payload.unwrap());
            if let Some(mut response) = response {
                response.request_id = request.request_id;
                pipe.write_all(&encode_frame(&response).unwrap())
                    .await
                    .unwrap();
            }
            // None models a completed server mutation whose response pipe is
            // lost before any acknowledgment reaches the client.
        }
        requests
    });
    (client, task)
}

#[tokio::test]
async fn lost_abort_reply_is_confirmed_only_by_the_exact_durable_completed_fingerprint() {
    for invalid in 0..7 {
        let request = request();
        let expected = pending(&request).replacement.unwrap();
        let mut completed = aborted(&request);
        match invalid {
            1 => completed.replacement.as_mut().unwrap().operation_id = Uuid::new_v4().to_string(),
            2 => {
                completed.replacement.as_mut().unwrap().source_operation_id =
                    Uuid::new_v4().to_string()
            }
            3 => {
                completed
                    .replacement
                    .as_mut()
                    .unwrap()
                    .target_plan
                    .as_mut()
                    .unwrap()
                    .kill_switch = false
            }
            4 => completed.replacement.as_mut().unwrap().guard_active = true,
            5 => completed.journal_generation = 1,
            6 => completed.replacement = None,
            _ => {}
        }
        let (client, task) = lost_response_client(vec![None, Some(response(completed))]);
        assert_eq!(
            client.abort_replacement(&expected, 43).await.is_ok(),
            invalid == 0
        );
        assert!(
            matches!(task.await.unwrap().as_slice(), [agent_request::Payload::AbortReplacement(abort),
            agent_request::Payload::GetState(_)] if abort.operation_id == request.operation_id)
        );
    }
}

#[tokio::test]
async fn repeated_runtime_shutdown_recognizes_completed_abort_after_both_initial_replies_are_lost()
{
    let request = request();
    let (client, task) = lost_response_client(vec![
        Some(response(pending(&request))),
        None,
        None,
        Some(response(aborted(&request))),
    ]);
    let mut runtime = runtime(client, request.operation_id.parse().unwrap());
    runtime.replacement_request = Some(request.clone());
    assert!(runtime.shutdown().await.is_err());
    assert!(runtime.transaction_open && runtime.shutdown_replacement.is_some());
    runtime.shutdown().await.unwrap();
    assert!(!runtime.transaction_open);
    assert!(
        matches!(task.await.unwrap().as_slice(), [agent_request::Payload::GetState(_),
        agent_request::Payload::AbortReplacement(abort), agent_request::Payload::GetState(_),
        agent_request::Payload::GetState(_)] if abort.operation_id == request.operation_id)
    );
}

#[tokio::test]
async fn failed_abort_then_new_disconnect_retries_that_target_and_never_a_retarget() {
    for retarget in [false, true] {
        let request = request();
        let mut current = pending(&request);
        current.replacement.as_mut().unwrap().phase =
            agent_v1::TunnelReplacementPhase::Aborting as i32;
        current.journal_generation = 50;
        let mut second = current.clone();
        if retarget {
            let replacement = second.replacement.as_mut().unwrap();
            replacement.source_operation_id = request.operation_id.clone();
            replacement.operation_id = Uuid::new_v4().to_string();
            second.operation_id = replacement.operation_id.clone();
        }
        let failure = AgentResponse {
            error: Some(agent_v1::AgentError {
                code: "AGENT_RECOVERY_FAILED".into(),
                message: "cleanup unavailable".into(),
                retryable: true,
            }),
            ..Default::default()
        };
        let mut script = vec![
            response(pending(&request)),
            failure,
            response(current),
            response(second),
        ];
        if !retarget {
            script.push(response(aborted(&request)));
        }
        let (client, task) = scripted_recovery_client(script);
        let mut runtime = runtime(client, request.operation_id.parse().unwrap());
        runtime.replacement_request = Some(request.clone());
        assert!(runtime.shutdown().await.is_err());
        assert_eq!(runtime.shutdown().await.is_ok(), !retarget);
        let requests = task.await.unwrap();
        let aborts: Vec<_> = requests
            .iter()
            .filter_map(|request| match request {
                agent_request::Payload::AbortReplacement(abort) => Some(abort),
                _ => None,
            })
            .collect();
        assert_eq!(aborts.len(), if retarget { 1 } else { 2 });
        assert!(
            aborts
                .iter()
                .all(|abort| abort.operation_id == request.operation_id)
        );
    }
}
