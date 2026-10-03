//! Fake named pipes exercise cleanup without changing Windows proxy settings.
use super::tests::scripted_recovery_client;
use super::*;
use tokio::net::windows::named_pipe::ServerOptions;

fn state_response(state: AgentState) -> AgentResponse {
    AgentResponse {
        payload: Some(agent_response::Payload::State(state)),
        ..Default::default()
    }
}

fn clean_state() -> AgentState {
    AgentState {
        phase: agent_v1::AgentPhase::Clean as i32,
        ..Default::default()
    }
}

fn owned_state(operation: Uuid, tunnel: bool, proxy_active: bool) -> AgentState {
    AgentState {
        phase: agent_v1::AgentPhase::Active as i32,
        operation_id: operation.to_string(),
        profile_id: if tunnel {
            operation.to_string()
        } else {
            String::new()
        },
        plan: tunnel.then(|| {
            Box::new(agent_v1::TunnelPlan {
                profile_id: operation.to_string(),
                ..Default::default()
            })
        }),
        system_proxy_active: proxy_active,
        ..Default::default()
    }
}

fn proxy_capabilities(supported: bool, version: u32) -> AgentResponse {
    AgentResponse {
        payload: Some(agent_response::Payload::Capabilities(AgentCapabilities {
            system_proxy: supported,
            protocol_version: version,
            ..Default::default()
        })),
        ..Default::default()
    }
}

fn restore_failure() -> AgentResponse {
    AgentResponse {
        error: Some(agent_v1::AgentError {
            code: "AGENT_RECOVERY_FAILED".into(),
            message: "inert restore failure".into(),
            retryable: true,
        }),
        ..Default::default()
    }
}

fn assert_restore_requests(requests: &[agent_request::Payload], operation: Uuid, count: usize) {
    let restore_ids = requests
        .iter()
        .filter_map(|request| match request {
            agent_request::Payload::RestoreSystemProxy(request) => {
                Some(request.operation_id.as_str())
            }
            agent_request::Payload::GetState(_) | agent_request::Payload::GetCapabilities(_) => {
                None
            }
            other => {
                panic!("cleanup must not apply a proxy or recover another transaction: {other:?}")
            }
        })
        .collect::<Vec<_>>();
    assert_eq!(restore_ids, vec![operation.to_string(); count]);
}

#[tokio::test]
async fn standalone_shutdown_retries_the_exact_owner_until_restore_is_confirmed() {
    let operation = Uuid::new_v4();
    let owned = || {
        state_response(AgentState {
            phase: agent_v1::AgentPhase::RecoveryRequired as i32,
            ..owned_state(operation, false, true)
        })
    };
    let (client, requests) = scripted_recovery_client(vec![
        restore_failure(),
        owned(),
        owned(),
        proxy_capabilities(true, AGENT_PROTOCOL_VERSION),
        restore_failure(),
        owned(),
        owned(),
        proxy_capabilities(true, AGENT_PROTOCOL_VERSION),
        state_response(clean_state()),
    ]);
    let pipe = client.open_pipe().await.unwrap();
    let mut guard = WindowsSystemProxyGuard::from_lease(client, operation, pipe, false);
    for _ in 0..2 {
        assert!(matches!(
            guard.shutdown().await,
            Err(WindowsVpnError::Remote { code, .. }) if code == "AGENT_RECOVERY_FAILED"
        ));
        assert!(guard.pipe.is_none());
        assert!(!guard.cleanup_confirmed);
        assert_eq!(guard.operation_id, operation);
    }
    guard.shutdown().await.unwrap();
    assert!(guard.cleanup_confirmed);
    // Confirmed shutdown is idempotent and does not depend on a live Agent.
    guard.shutdown().await.unwrap();
    assert_restore_requests(&requests.await.unwrap(), operation, 3);
}

#[tokio::test]
async fn shutdown_slot_preserves_failed_cleanup_until_the_original_operation_is_restored() {
    let operation = Uuid::new_v4();
    let owned = || state_response(owned_state(operation, false, true));
    let (client, requests) = scripted_recovery_client(vec![
        restore_failure(),
        owned(),
        owned(),
        proxy_capabilities(true, AGENT_PROTOCOL_VERSION),
        state_response(clean_state()),
    ]);
    let pipe = client.open_pipe().await.unwrap();
    let mut slot = Some(WindowsSystemProxyGuard::from_lease(
        client, operation, pipe, false,
    ));
    assert!(
        WindowsSystemProxyGuard::shutdown_slot(&mut slot)
            .await
            .is_err()
    );
    let retained = slot.as_ref().unwrap();
    assert_eq!(retained.operation_id, operation);
    assert!(!retained.cleanup_confirmed);
    WindowsSystemProxyGuard::shutdown_slot(&mut slot)
        .await
        .unwrap();
    assert!(slot.is_none());
    assert_restore_requests(&requests.await.unwrap(), operation, 2);
}

#[tokio::test]
async fn inactive_tunnel_queries_and_failed_restore_readbacks_never_confirm_an_intended_receipt() {
    let operation = Uuid::new_v4();
    let inactive = || state_response(owned_state(operation, true, false));
    let (client, requests) = scripted_recovery_client(vec![
        inactive(),
        proxy_capabilities(true, AGENT_PROTOCOL_VERSION),
        restore_failure(),
        inactive(),
        inactive(),
        proxy_capabilities(true, AGENT_PROTOCOL_VERSION),
        inactive(),
    ]);
    assert!(
        client
            .restore_owned_system_proxy(operation, true)
            .await
            .is_err()
    );
    client
        .restore_owned_system_proxy(operation, true)
        .await
        .unwrap();
    assert_restore_requests(&requests.await.unwrap(), operation, 2);
}

#[tokio::test]
async fn lost_restore_reply_accepts_only_complete_global_clean() {
    let operation = Uuid::new_v4();
    // The fake Agent drops the original reply. Successful lease EOF recovery
    // has already retired the standalone transaction before the fresh query.
    let (client, requests) = timed_client(vec![
        TimedReply::DropReply,
        TimedReply::Reply(Box::new(state_response(clean_state()))),
    ]);
    let pipe = client.open_pipe().await.unwrap();
    let mut guard = WindowsSystemProxyGuard::from_lease(client, operation, pipe, false);
    guard.shutdown().await.unwrap();
    assert!(guard.cleanup_confirmed);
    assert_restore_requests(&requests.await.unwrap(), operation, 1);
}

#[tokio::test]
async fn tunnel_retry_accepts_global_clean_after_whole_tunnel_rollback() {
    let operation = Uuid::new_v4();
    let (client, requests) = scripted_recovery_client(vec![
        restore_failure(),
        state_response(owned_state(operation, true, true)),
        state_response(clean_state()),
    ]);
    let pipe = client.open_pipe().await.unwrap();
    let mut guard = WindowsSystemProxyGuard::from_lease(client, operation, pipe, true);
    assert!(guard.shutdown().await.is_err());
    assert!(!guard.cleanup_confirmed);
    guard.shutdown().await.unwrap();
    assert!(guard.cleanup_confirmed);
    assert_restore_requests(&requests.await.unwrap(), operation, 1);
}

#[tokio::test]
async fn cleanup_queries_reject_successors_and_the_other_operation_kind_without_mutating_them() {
    let operation = Uuid::new_v4();
    for tunnel in [false, true] {
        let mut pending = owned_state(operation, tunnel, true);
        pending.replacement = Some(Box::new(agent_v1::TunnelReplacementStatus {
            source_operation_id: operation.to_string(),
            operation_id: Uuid::new_v4().to_string(),
            phase: agent_v1::TunnelReplacementPhase::Prepared as i32,
            guard_active: true,
            target_plan: Some(Box::new(agent_v1::TunnelPlan::default())),
            ..Default::default()
        }));
        for state in [
            owned_state(Uuid::new_v4(), tunnel, true),
            owned_state(operation, !tunnel, true),
            AgentState {
                phase: agent_v1::AgentPhase::Prepared as i32,
                ..owned_state(operation, tunnel, true)
            },
            AgentState {
                phase: agent_v1::AgentPhase::Preparing as i32,
                ..owned_state(operation, tunnel, true)
            },
            AgentState {
                phase: agent_v1::AgentPhase::Recovering as i32,
                ..owned_state(operation, tunnel, true)
            },
            pending,
        ] {
            let (client, requests) = scripted_recovery_client(vec![state_response(state)]);
            assert!(matches!(
                client.restore_owned_system_proxy(operation, tunnel).await,
                Err(WindowsVpnError::RecoveryConflict)
            ));
            assert_restore_requests(&requests.await.unwrap(), operation, 0);
        }
    }
}

#[tokio::test]
async fn restoration_racing_with_an_owner_change_accepts_only_global_clean() {
    let operation = Uuid::new_v4();
    let successor = Uuid::new_v4();
    for completed in [false, true] {
        let (client, requests) = scripted_recovery_client(vec![
            state_response(owned_state(operation, false, true)),
            proxy_capabilities(true, AGENT_PROTOCOL_VERSION),
            AgentResponse {
                error: Some(agent_v1::AgentError {
                    code: "AGENT_OPERATION_MISMATCH".into(),
                    message: "inert ownership race".into(),
                    retryable: false,
                }),
                ..Default::default()
            },
            state_response(if completed {
                clean_state()
            } else {
                owned_state(successor, false, true)
            }),
        ]);
        assert_eq!(
            client
                .restore_owned_system_proxy(operation, false)
                .await
                .is_ok(),
            completed,
        );
        assert_restore_requests(&requests.await.unwrap(), operation, 1);
    }
}

#[tokio::test]
async fn cleanup_retry_checks_capabilities_even_for_an_inactive_owned_proxy() {
    let operation = Uuid::new_v4();
    for tunnel in [false, true] {
        for (supported, version) in [
            (false, AGENT_PROTOCOL_VERSION),
            (true, AGENT_PROTOCOL_VERSION + 1),
        ] {
            let (client, requests) = scripted_recovery_client(vec![
                state_response(owned_state(operation, tunnel, false)),
                proxy_capabilities(supported, version),
            ]);
            let result = client.restore_owned_system_proxy(operation, tunnel).await;
            if supported {
                assert!(matches!(result, Err(WindowsVpnError::ProtocolVersion(_))));
            } else {
                assert!(matches!(
                    result,
                    Err(WindowsVpnError::MissingCapabilities(_))
                ));
            }
            assert_restore_requests(&requests.await.unwrap(), operation, 0);
        }
    }
}

#[test]
fn clean_confirmation_rejects_dirty_fields_and_pending_replacement() {
    let operation = Uuid::new_v4();
    let mut dirty = Vec::new();
    for field in [
        "operation",
        "profile",
        "proxy",
        "packet",
        "kill",
        "plan",
        "device",
        "replacement",
    ] {
        let mut state = clean_state();
        match field {
            "operation" => state.operation_id = operation.to_string(),
            "profile" => state.profile_id = operation.to_string(),
            "proxy" => state.system_proxy_active = true,
            "packet" => state.packet_session_active = true,
            "kill" => state.kill_switch_active = true,
            "plan" => state.plan = Some(Box::new(agent_v1::TunnelPlan::default())),
            "device" => {
                state.device = Some(agent_v1::ManagedDeviceStatus {
                    phase: agent_v1::ManagedDevicePhase::InUse as i32,
                    ..Default::default()
                })
            }
            "replacement" => {
                state.replacement = Some(Box::new(agent_v1::TunnelReplacementStatus {
                    source_operation_id: operation.to_string(),
                    operation_id: Uuid::new_v4().to_string(),
                    phase: agent_v1::TunnelReplacementPhase::Prepared as i32,
                    guard_active: true,
                    target_plan: Some(Box::new(agent_v1::TunnelPlan::default())),
                    ..Default::default()
                }))
            }
            _ => unreachable!(),
        }
        dirty.push((field, state));
    }
    for (field, state) in dirty {
        assert!(
            !system_proxy_clean(&state),
            "{field} must remain unconfirmed"
        );
        for tunnel in [false, true] {
            assert!(!system_proxy_restore_succeeded(tunnel, operation, &state));
        }
    }
    assert!(system_proxy_clean(&clean_state()));
    assert!(system_proxy_restore_succeeded(
        false,
        operation,
        &clean_state()
    ));
    assert!(!system_proxy_restore_succeeded(
        true,
        operation,
        &clean_state()
    ));
}

#[tokio::test]
async fn resumed_tunnel_cleanup_rejects_clean_when_it_requires_the_tunnel_to_remain_active() {
    let operation = Uuid::new_v4();
    let (client, requests) = scripted_recovery_client(vec![
        state_response(owned_state(operation, true, true)),
        proxy_capabilities(true, AGENT_PROTOCOL_VERSION),
        state_response(clean_state()),
    ]);
    assert!(
        client
            .restore_retained_system_proxy(operation)
            .await
            .is_err()
    );
    assert_restore_requests(&requests.await.unwrap(), operation, 1);
}

enum TimedReply {
    Reply(Box<AgentResponse>),
    DropReply,
    UntilLeaseCloses(Arc<tokio::sync::Notify>),
}

fn timed_client(
    script: Vec<TimedReply>,
) -> (WindowsAgentClient, JoinHandle<Vec<agent_request::Payload>>) {
    let pipe_name = format!("{AGENT_PIPE_NAME}.test-{}", Uuid::new_v4());
    let mut next = ServerOptions::new()
        .first_pipe_instance(true)
        .create(&pipe_name)
        .unwrap();
    let client = WindowsAgentClient::for_test(pipe_name.clone());
    let task = tokio::spawn(async move {
        let mut requests = Vec::new();
        for step in script {
            next.connect().await.unwrap();
            let mut pipe = next;
            next = ServerOptions::new().create(&pipe_name).unwrap();
            let mut header = [0; 4];
            pipe.read_exact(&mut header).await.unwrap();
            let mut payload = vec![0; u32::from_be_bytes(header) as usize];
            pipe.read_exact(&mut payload).await.unwrap();
            let mut frame = BytesMut::from(header.as_slice());
            frame.extend_from_slice(&payload);
            let request: AgentRequest = decode_frame(frame.freeze()).unwrap();
            requests.push(request.payload.unwrap());
            match step {
                TimedReply::Reply(mut response) => {
                    response.request_id = request.request_id;
                    pipe.write_all(&encode_frame(response.as_ref()).unwrap())
                        .await
                        .unwrap();
                }
                TimedReply::DropReply => {}
                TimedReply::UntilLeaseCloses(started) => {
                    started.notify_one();
                    match pipe.read(&mut [0; 1]).await {
                        Ok(0) => {}
                        Err(error) if error.kind() == io::ErrorKind::BrokenPipe => {}
                        result => {
                            panic!("the cancelled attempt must release its old lease: {result:?}")
                        }
                    }
                }
            }
        }
        requests
    });
    (client, task)
}

#[tokio::test]
async fn shutdown_timeout_retains_unconfirmed_identity_and_retries_on_a_fresh_pipe() {
    let operation = Uuid::new_v4();
    let started = Arc::new(tokio::sync::Notify::new());
    let (client, requests) = timed_client(vec![
        TimedReply::UntilLeaseCloses(started.clone()),
        TimedReply::Reply(Box::new(state_response(owned_state(
            operation, false, true,
        )))),
        TimedReply::Reply(Box::new(proxy_capabilities(true, AGENT_PROTOCOL_VERSION))),
        TimedReply::Reply(Box::new(state_response(clean_state()))),
    ]);
    let pipe = client.open_pipe().await.unwrap();
    let mut guard = WindowsSystemProxyGuard::from_lease(client, operation, pipe, false);
    let mut attempt = Box::pin(guard.shutdown_with_timeout(Duration::from_secs(1)));
    tokio::select! {
        _ = started.notified() => {}
        result = &mut attempt => panic!("the original Restore must reach the fake Agent: {result:?}"),
    }
    assert!(matches!(attempt.await, Err(WindowsVpnError::RpcTimeout)));
    assert!(!guard.cleanup_confirmed);
    assert!(guard.pipe.is_none());
    guard.shutdown().await.unwrap();
    assert!(guard.cleanup_confirmed);
    assert_restore_requests(&requests.await.unwrap(), operation, 2);
}

#[tokio::test]
async fn cancelling_shutdown_slot_keeps_the_guard_for_an_exact_operation_retry() {
    let operation = Uuid::new_v4();
    let started = Arc::new(tokio::sync::Notify::new());
    let (client, requests) = timed_client(vec![
        TimedReply::UntilLeaseCloses(started.clone()),
        TimedReply::Reply(Box::new(state_response(owned_state(
            operation, false, true,
        )))),
        TimedReply::Reply(Box::new(proxy_capabilities(true, AGENT_PROTOCOL_VERSION))),
        TimedReply::Reply(Box::new(state_response(clean_state()))),
    ]);
    let pipe = client.open_pipe().await.unwrap();
    let mut slot = Some(WindowsSystemProxyGuard::from_lease(
        client, operation, pipe, false,
    ));
    let mut attempt = Box::pin(WindowsSystemProxyGuard::shutdown_slot(&mut slot));
    tokio::select! {
        _ = started.notified() => {}
        result = &mut attempt => panic!("the original Restore must still be waiting: {result:?}"),
    }
    drop(attempt);
    assert!(!slot.as_ref().unwrap().cleanup_confirmed);
    assert_eq!(slot.as_ref().unwrap().operation_id, operation);
    WindowsSystemProxyGuard::shutdown_slot(&mut slot)
        .await
        .unwrap();
    assert!(slot.is_none());
    assert_restore_requests(&requests.await.unwrap(), operation, 2);
}
