use async_trait::async_trait;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use tokio_util::sync::CancellationToken;
use usque_core::{AddressFamily, Transport};
use usque_macos_runtime::{
    backend::{SessionBackend, SessionHealth},
    credentials::SessionCredentials,
    *,
};
use uuid::Uuid;
use zeroize::Zeroizing;

#[derive(Default)]
struct Evidence {
    events: Mutex<Vec<&'static str>>,
    live: AtomicUsize,
    maximum: AtomicUsize,
    hold_start: AtomicBool,
    failed: AtomicBool,
    fail_release: AtomicBool,
}
impl Evidence {
    fn event(&self, value: &'static str) {
        self.events.lock().unwrap().push(value);
    }
}
struct TestBackend {
    evidence: Arc<Evidence>,
    active: bool,
}
#[async_trait]
impl SessionBackend for TestBackend {
    async fn protect(&mut self, _: &SessionKey) -> Result<(), String> {
        self.evidence.event("protect");
        Ok(())
    }
    async fn configure_dns_routes(
        &mut self,
        _: &SessionKey,
        _: &SessionConfiguration,
        _: &SessionCredentials,
    ) -> Result<(), String> {
        self.evidence.event("dns_routes");
        Ok(())
    }
    async fn connect(
        &mut self,
        _: &SessionKey,
        _: SessionConfiguration,
        _: SessionCredentials,
        cancellation: CancellationToken,
    ) -> Result<(), String> {
        self.evidence.event("connect");
        if self.evidence.hold_start.load(Ordering::Acquire) {
            cancellation.cancelled().await;
            return Err("operation_cancelled".into());
        }
        if self.active {
            return Err("second_session".into());
        }
        self.active = true;
        let count = self.evidence.live.fetch_add(1, Ordering::AcqRel) + 1;
        self.evidence.maximum.fetch_max(count, Ordering::AcqRel);
        Ok(())
    }
    fn health(&self) -> SessionHealth {
        if self.evidence.failed.load(Ordering::Acquire) {
            SessionHealth::Failed
        } else if self.active {
            SessionHealth::Connected(ProviderPath {
                transport: Transport::Http2,
                endpoint_family: AddressFamily::Ipv4,
                ipv4_available: true,
                ipv6_available: true,
            })
        } else {
            SessionHealth::Starting
        }
    }
    async fn allow(&mut self, _: &SessionKey) -> Result<(), String> {
        self.evidence.event("allow");
        Ok(())
    }
    async fn block(&mut self, _: &SessionKey) -> Result<(), String> {
        self.evidence.event("block");
        Ok(())
    }
    async fn disconnect(&mut self) -> Result<(), String> {
        self.evidence.event("disconnect");
        if self.active {
            self.active = false;
            self.evidence.live.fetch_sub(1, Ordering::AcqRel);
        }
        Ok(())
    }
    async fn clear_dns_routes(&mut self, _: &SessionKey) -> Result<(), String> {
        self.evidence.event("clear_dns_routes");
        Ok(())
    }
    async fn release_protection(&mut self, _: &SessionKey) -> Result<(), String> {
        self.evidence.event("release");
        if self.evidence.fail_release.load(Ordering::Acquire) {
            Err("release_failed".into())
        } else {
            Ok(())
        }
    }
}
fn setup(capacity: usize) -> (Controller, Arc<Evidence>, SessionKey) {
    let evidence = Arc::new(Evidence::default());
    let controller = Controller::spawn(
        Box::new(TestBackend {
            evidence: evidence.clone(),
            active: false,
        }),
        capacity,
    );
    let key = SessionKey {
        session_id: Uuid::new_v4(),
        config_revision: 1,
        network_generation: 7,
    };
    (controller, evidence, key)
}
fn envelope(key: &SessionKey, command: Command) -> Envelope {
    Envelope {
        version: PROTOCOL_VERSION,
        operation_id: Uuid::new_v4(),
        session: key.clone(),
        command,
    }
}
fn start_command(replace: bool) -> Command {
    let configuration = SessionConfiguration {
        profile: usque_core::Profile::default(),
        geo_cache_dir: "/nonexistent-read-only".into(),
        policy_digest: None,
    };
    let credentials = SessionCredentials {
        warp_identity: Zeroizing::new("test-only-material".into()),
        chain: None,
    };
    if replace {
        Command::Replace {
            configuration,
            credentials,
        }
    } else {
        Command::Start {
            configuration,
            credentials,
        }
    }
}
async fn wait_phase(controller: &Controller, expected: Phase) -> Snapshot {
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        loop {
            let snapshot = controller.snapshot();
            if snapshot.phase == expected {
                return snapshot;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("lifecycle reached expected phase")
}

#[tokio::test]
async fn acceptance_is_not_connection_and_stop_reverses_order() {
    let (controller, evidence, key) = setup(4);
    let accepted = controller
        .submit(envelope(&key, start_command(false)))
        .unwrap();
    assert_eq!(accepted.acceptance, Acceptance::Accepted);
    assert_ne!(accepted.snapshot.phase, Phase::Connected);
    assert_eq!(accepted.snapshot.selected_path, None);
    assert_eq!(accepted.snapshot.session.as_ref(), Some(&key));
    assert_eq!(accepted.snapshot.phase, Phase::Starting);
    assert!(!accepted.snapshot.protection_active);
    let connected = wait_phase(&controller, Phase::Connected).await;
    assert!(connected.protection_active);
    assert!(!connected.production_capable);
    assert!(connected.selected_path.is_some());
    controller.submit(envelope(&key, Command::Stop)).unwrap();
    assert!(
        !wait_phase(&controller, Phase::Stopped)
            .await
            .protection_active
    );
    assert_eq!(
        *evidence.events.lock().unwrap(),
        [
            "protect",
            "dns_routes",
            "connect",
            "allow",
            "block",
            "disconnect",
            "clear_dns_routes",
            "release"
        ]
    );
}

#[tokio::test]
async fn replacement_never_owns_two_sessions_and_old_generations_are_rejected() {
    let (controller, evidence, key) = setup(4);
    controller
        .submit(envelope(&key, start_command(false)))
        .unwrap();
    wait_phase(&controller, Phase::Connected).await;
    assert_eq!(
        controller.submit(envelope(&key, start_command(false))),
        Err(ControlError::StaleSession)
    );
    let mut replacement = key.clone();
    replacement.config_revision += 1;
    replacement.network_generation += 1;
    controller
        .submit(envelope(&replacement, start_command(true)))
        .unwrap();
    // An immediate old Stop must not cancel the newly admitted configuration.
    assert_eq!(
        controller.submit(envelope(&key, Command::Stop)),
        Err(ControlError::StaleSession)
    );
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        while controller.snapshot().session.as_ref() != Some(&replacement)
            || controller.snapshot().phase != Phase::Connected
        {
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(evidence.maximum.load(Ordering::Acquire), 1);
    assert_eq!(evidence.live.load(Ordering::Acquire), 1);
}

#[tokio::test]
async fn duplicate_payload_is_idempotent_but_reused_operation_is_rejected() {
    let (controller, evidence, key) = setup(4);
    let request = envelope(&key, start_command(false));
    controller.submit(request.clone()).unwrap();
    assert_eq!(
        controller.submit(request.clone()).unwrap().acceptance,
        Acceptance::Duplicate
    );
    let mut altered = request;
    altered.command = Command::Stop;
    assert_eq!(
        controller.submit(altered),
        Err(ControlError::ReplayMismatch)
    );
    wait_phase(&controller, Phase::Connected).await;
    assert_eq!(evidence.maximum.load(Ordering::Acquire), 1);
}

#[tokio::test]
async fn wrong_version_is_rejected_before_side_effects() {
    let (controller, evidence, key) = setup(1);
    let mut request = envelope(&key, start_command(false));
    request.version = 2;
    assert_eq!(
        controller.submit(request),
        Err(ControlError::UnsupportedVersion)
    );
    assert_eq!(evidence.live.load(Ordering::Acquire), 0);
    assert!(evidence.events.lock().unwrap().is_empty());
}

#[tokio::test]
async fn cancellation_interrupts_inflight_start_and_keeps_protection() {
    let (controller, evidence, key) = setup(1);
    evidence.hold_start.store(true, Ordering::Release);
    let start = envelope(&key, start_command(false));
    controller.submit(start.clone()).unwrap();
    wait_phase(&controller, Phase::Starting).await;
    controller
        .submit(envelope(
            &key,
            Command::Cancel {
                operation_id: start.operation_id,
            },
        ))
        .unwrap();
    let failed = wait_phase(&controller, Phase::Failed).await;
    assert!(failed.protection_active);
    assert_eq!(failed.error_code.as_deref(), Some("operation_cancelled"));
    assert!(!evidence.events.lock().unwrap().contains(&"release"));
}

#[tokio::test]
async fn queue_backpressure_does_not_supersede_the_session() {
    let (controller, evidence, key) = setup(1);
    evidence.hold_start.store(true, Ordering::Release);
    let start = envelope(&key, start_command(false));
    controller.submit(start.clone()).unwrap();
    wait_phase(&controller, Phase::Starting).await;
    let mut second = key.clone();
    second.config_revision = 2;
    controller
        .submit(envelope(&second, start_command(true)))
        .unwrap();
    let mut rejected = second.clone();
    rejected.config_revision = 3;
    assert_eq!(
        controller.submit(envelope(&rejected, start_command(true))),
        Err(ControlError::Backpressure)
    );
    // Second revision is still head: an exact Stop is admitted once room exists.
    controller
        .submit(envelope(
            &second,
            Command::Cancel {
                operation_id: start.operation_id,
            },
        ))
        .unwrap();
}

#[tokio::test]
async fn process_loss_and_transport_failure_never_release_protection() {
    let (controller, evidence, key) = setup(2);
    controller
        .submit(envelope(&key, start_command(false)))
        .unwrap();
    wait_phase(&controller, Phase::Connected).await;
    evidence.failed.store(true, Ordering::Release);
    assert!(
        wait_phase(&controller, Phase::Failed)
            .await
            .protection_active
    );
    drop(controller);
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    assert_eq!(evidence.live.load(Ordering::Acquire), 1);
    assert!(!evidence.events.lock().unwrap().contains(&"release"));
}

#[tokio::test]
async fn protect_barrier_can_precede_engine_mode_switch() {
    let (controller, evidence, key) = setup(2);
    controller.submit(envelope(&key, Command::Protect)).unwrap();
    assert!(
        wait_phase(&controller, Phase::Protected)
            .await
            .protection_active
    );
    assert_eq!(evidence.live.load(Ordering::Acquire), 0);
    controller
        .submit(envelope(&key, start_command(false)))
        .unwrap();
    wait_phase(&controller, Phase::Connected).await;
}

#[tokio::test]
async fn saturated_replay_journal_keeps_snapshot_and_terminal_stop_available() {
    let (controller, evidence, key) = setup(1);
    evidence.hold_start.store(true, Ordering::Release);
    let start = envelope(&key, start_command(false));
    controller.submit(start.clone()).unwrap();
    wait_phase(&controller, Phase::Starting).await;
    // Fill the bounded journal without yielding to the cancelled startup.
    for _ in 1..4096 {
        controller
            .submit(envelope(
                &key,
                Command::Cancel {
                    operation_id: start.operation_id,
                },
            ))
            .unwrap();
    }
    assert_eq!(
        controller.submit(envelope(&key, Command::Protect)),
        Err(ControlError::ReplayWindowFull)
    );
    controller
        .submit(envelope(&key, Command::Snapshot))
        .unwrap();
    evidence.fail_release.store(true, Ordering::Release);
    let stop = envelope(&key, Command::Stop);
    controller.submit(stop.clone()).unwrap();
    // Additional Stop UUIDs coalesce instead of consuming another safety slot.
    for _ in 0..4096 {
        controller.submit(envelope(&key, Command::Stop)).unwrap();
    }
    let failed = wait_phase(&controller, Phase::Failed).await;
    assert!(failed.protection_active);
    assert_eq!(failed.error_code.as_deref(), Some("release_failed"));
    evidence.fail_release.store(false, Ordering::Release);
    controller.submit(envelope(&key, Command::Stop)).unwrap();
    assert_eq!(
        controller.submit(stop.clone()).unwrap().acceptance,
        Acceptance::Duplicate
    );
    wait_phase(&controller, Phase::Stopped).await;
    let event_count = evidence.events.lock().unwrap().len();
    for _ in 0..4096 {
        controller.submit(envelope(&key, Command::Stop)).unwrap();
        controller
            .submit(envelope(&key, Command::Snapshot))
            .unwrap();
    }
    let mut mismatch = stop;
    mismatch.command = Command::Protect;
    assert_eq!(
        controller.submit(mismatch),
        Err(ControlError::ReplayMismatch)
    );
    tokio::time::sleep(std::time::Duration::from_millis(75)).await;
    assert_eq!(evidence.events.lock().unwrap().len(), event_count);
}

#[tokio::test]
async fn priority_stop_discards_queued_replacement_without_reopening_protection() {
    let (controller, evidence, key) = setup(1);
    evidence.hold_start.store(true, Ordering::Release);
    controller
        .submit(envelope(&key, start_command(false)))
        .unwrap();
    wait_phase(&controller, Phase::Starting).await;
    let mut replacement = key.clone();
    replacement.config_revision += 1;
    controller
        .submit(envelope(&replacement, start_command(true)))
        .unwrap();
    // The ordinary queue is full, but Stop still cancels both accepted starts.
    controller
        .submit(envelope(&replacement, Command::Stop))
        .unwrap();
    assert_eq!(
        controller.submit(envelope(&replacement, start_command(true))),
        Err(ControlError::StaleSession)
    );
    let stopped = wait_phase(&controller, Phase::Stopped).await;
    assert_eq!(stopped.session.as_ref(), Some(&replacement));
    tokio::time::sleep(std::time::Duration::from_millis(75)).await;
    assert_eq!(controller.snapshot().phase, Phase::Stopped);
    assert_eq!(
        evidence
            .events
            .lock()
            .unwrap()
            .iter()
            .filter(|event| **event == "protect")
            .count(),
        1
    );
    assert_eq!(evidence.live.load(Ordering::Acquire), 0);
    assert_eq!(evidence.events.lock().unwrap().last(), Some(&"release"));
}

#[tokio::test]
async fn stopped_session_uuid_cannot_be_reused_after_a_later_session() {
    let (controller, _, key) = setup(2);
    controller.submit(envelope(&key, Command::Protect)).unwrap();
    wait_phase(&controller, Phase::Protected).await;
    controller.submit(envelope(&key, Command::Stop)).unwrap();
    wait_phase(&controller, Phase::Stopped).await;
    for command in [Command::Protect, start_command(false)] {
        assert_eq!(
            controller.submit(envelope(&key, command)),
            Err(ControlError::StaleSession)
        );
    }
    let mut next = key.clone();
    next.session_id = Uuid::new_v4();
    controller
        .submit(envelope(&next, start_command(false)))
        .unwrap();
    wait_phase(&controller, Phase::Connected).await;
    controller.submit(envelope(&next, Command::Stop)).unwrap();
    wait_phase(&controller, Phase::Stopped).await;
    let mut retired = key;
    retired.config_revision += 1;
    retired.network_generation += 1;
    assert_eq!(
        controller.submit(envelope(&retired, start_command(false))),
        Err(ControlError::StaleSession)
    );
}

#[tokio::test]
async fn stop_before_start_runs_does_not_rearm_and_allows_a_fresh_session() {
    let (controller, evidence, key) = setup(1);
    controller
        .submit(envelope(&key, start_command(false)))
        .unwrap();
    controller.submit(envelope(&key, Command::Stop)).unwrap();
    // The initial default snapshot is also Stopped, without a session. Await
    // the acknowledged Stop rather than mistaking that initial state for it.
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        while controller.snapshot().session.as_ref() != Some(&key) {
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    let stopped = wait_phase(&controller, Phase::Stopped).await;
    assert_eq!(stopped.session.as_ref(), Some(&key));
    assert_eq!(
        *evidence.events.lock().unwrap(),
        ["block", "disconnect", "clear_dns_routes", "release"]
    );
    let mut fresh = key;
    fresh.session_id = Uuid::new_v4();
    controller
        .submit(envelope(&fresh, start_command(false)))
        .unwrap();
    wait_phase(&controller, Phase::Connected).await;
    assert_eq!(
        controller.submit(envelope(&fresh, start_command(false))),
        Err(ControlError::StaleSession)
    );
    assert_eq!(evidence.maximum.load(Ordering::Acquire), 1);
}
