//! Bounded connection evidence whose lifetime is independent of runtime cleanup.

use std::time::SystemTime;

use usque_core::ConnectionSnapshot;
use usque_transport::{ConnectionTimelineSnapshot, NetworkQualitySnapshot};

use crate::{ActiveDataPlane, ControlService};

#[derive(Clone, serde::Serialize)]
pub(crate) struct StartupFailureEvidence {
    pub detail: Option<usque_ipc::agent_v1::LinuxFailureDetail>,
    pub cleanup_detail: Option<usque_ipc::agent_v1::LinuxFailureDetail>,
    pub cleanup_status: &'static str,
    #[serde(skip)]
    #[cfg(target_os = "linux")]
    pub cleanup_owner: Option<StartupCleanup>,
}

#[derive(Clone)]
#[cfg(target_os = "linux")]
pub(crate) struct StartupCleanup(
    std::sync::Arc<
        std::sync::Mutex<(
            &'static str,
            Option<usque_ipc::agent_v1::LinuxFailureDetail>,
        )>,
    >,
);

#[cfg(target_os = "linux")]
impl Default for StartupCleanup {
    fn default() -> Self {
        Self(std::sync::Arc::new(std::sync::Mutex::new((
            "not_requested",
            None,
        ))))
    }
}

#[cfg(target_os = "linux")]
impl StartupCleanup {
    pub fn pending(&self) {
        *self.0.lock().unwrap_or_else(|e| e.into_inner()) = ("pending", None);
    }
    #[cfg(target_os = "linux")]
    pub fn complete(&self, result: &std::io::Result<()>) {
        *self.0.lock().unwrap_or_else(|e| e.into_inner()) = match result {
            Ok(()) => ("completed", None),
            Err(error) => ("failed", crate::linux_agent::failure_detail(error)),
        };
    }
}

impl StartupFailureEvidence {
    pub fn resolved(&self) -> Self {
        let mut value = self.clone();
        #[cfg(target_os = "linux")]
        if let Some(owner) = &value.cleanup_owner {
            let (status, detail) = owner.0.lock().unwrap_or_else(|e| e.into_inner()).clone();
            value.cleanup_status = status;
            if detail.is_some() {
                value.cleanup_detail = detail;
            }
        }
        value.detail = value.detail.filter(usque_ipc::linux_failure::valid);
        value.cleanup_detail = value.cleanup_detail.filter(usque_ipc::linux_failure::valid);
        value
    }
}

#[cfg(target_os = "linux")]
pub(crate) fn linux_failure_stage(stage: i32) -> usque_core::TransportStage {
    use usque_core::TransportStage as T;
    use usque_ipc::agent_v1::LinuxFailureStage as S;
    match S::try_from(stage).ok() {
        Some(S::DnsProbe | S::DnsApply) => T::DnsApply,
        Some(S::Protection) => T::KillSwitchApply,
        Some(S::RouteApply) => T::RouteApply,
        Some(S::AddressApply) => T::AddressAssignment,
        Some(S::EgressPermission) => T::SocketProtection,
        Some(S::Recovery) => T::PlatformRecovery,
        _ => T::TunnelStartup,
    }
}

#[derive(serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CaptureScope {
    ActiveConnection,
    RetainedConnection,
    NotObserved,
}

#[derive(serde::Serialize)]
pub(crate) struct CaptureMetadata {
    schema_version: u32,
    scope: CaptureScope,
    captured_at: chrono::DateTime<chrono::Utc>,
    current_phase: usque_core::ConnectionPhase,
    captured_phase: usque_core::ConnectionPhase,
    pub(crate) session_generation: Option<u64>,
    connection_instance_id: Option<uuid::Uuid>,
    quality_age_milliseconds: Option<u64>,
    cleanup_status: &'static str,
    connection_stable_during_capture: bool,
    platform_observation_included: bool,
    timeline_dropped_events: u64,
    configuration_scope: &'static str,
}

#[derive(Clone)]
pub(crate) struct ConnectionEvidence {
    pub startup_failure: Option<StartupFailureEvidence>,
    pub connection: ConnectionSnapshot,
    pub timeline: ConnectionTimelineSnapshot,
    pub quality: Option<NetworkQualitySnapshot>,
    pub session_generation: u64,
    pub retained: bool,
    pub cleanup_status: &'static str,
    pub captured_at: SystemTime,
}

impl ConnectionEvidence {
    pub(crate) fn active(active: &ActiveDataPlane, mut connection: ConnectionSnapshot) -> Self {
        // Project actual health into this read-only copy. A delayed status tick
        // must not make a failed or reconnecting runtime appear ready; the
        // authoritative StateMachine and platform state remain untouched.
        match active.runtime.health() {
            usque_transport::RuntimeHealth::Failed { failure, .. } => {
                connection.phase = usque_core::ConnectionPhase::Error;
                let retryable = failure.retryable;
                connection.failure = Some(failure);
                connection.error = Some(usque_core::ConnectionError {
                    code: usque_core::ErrorCode::TransportUnavailable,
                    message: "The current connection runtime failed.".into(),
                    retryable,
                });
            }
            usque_transport::RuntimeHealth::Reconnecting { failure, .. } => {
                connection.phase = usque_core::ConnectionPhase::Reconnecting;
                connection.failure = Some(failure);
            }
            _ => {}
        }
        let updates = active.runtime.subscribe_network_quality();
        let quality = updates.borrow().clone();
        Self {
            startup_failure: {
                #[cfg(target_os = "linux")]
                {
                    match &active.runtime {
                        crate::ActiveRuntime::Vpn(runtime) => runtime.startup_failure(),
                        _ => None,
                    }
                }
                #[cfg(not(target_os = "linux"))]
                {
                    None
                }
            },
            connection,
            timeline: active.runtime.connection_timeline(),
            quality: Some(quality),
            session_generation: active.session_generation,
            retained: false,
            cleanup_status: "not_requested",
            captured_at: SystemTime::now(),
        }
    }

    pub(crate) fn terminal(mut self) -> Self {
        self.retained = true;
        self.cleanup_status = "pending";
        self
    }

    pub(crate) fn capture_metadata(
        &self,
        current: &ConnectionSnapshot,
        stable: bool,
        platform_included: bool,
    ) -> CaptureMetadata {
        CaptureMetadata {
            schema_version: 1,
            scope: if self.retained {
                CaptureScope::RetainedConnection
            } else if self.session_generation != 0 {
                CaptureScope::ActiveConnection
            } else {
                CaptureScope::NotObserved
            },
            captured_at: self.captured_at.into(),
            current_phase: current.phase,
            captured_phase: self.connection.phase,
            session_generation: (self.session_generation != 0).then_some(self.session_generation),
            connection_instance_id: self
                .quality
                .as_ref()
                .and_then(|quality| quality.connection_id)
                .map(|id| id.0),
            quality_age_milliseconds: self.quality.as_ref().map(|quality| {
                quality
                    .sampled_at
                    .elapsed()
                    .as_millis()
                    .min(u128::from(u64::MAX)) as u64
            }),
            cleanup_status: self
                .startup_failure
                .as_ref()
                .map(|f| f.resolved().cleanup_status)
                .unwrap_or(self.cleanup_status),
            connection_stable_during_capture: stable,
            platform_observation_included: platform_included,
            timeline_dropped_events: self.timeline.dropped_event_count,
            configuration_scope: "saved_configuration",
        }
    }
}

impl ControlService {
    #[cfg(target_os = "linux")]
    pub(crate) async fn retain_linux_startup(
        &self,
        generation: u64,
        telemetry: &usque_transport::ConnectionTelemetry,
        failure: Option<StartupFailureEvidence>,
    ) {
        // Startup is serialized by mutation_lock, but asynchronous old cleanup
        // can finish later. Never publish its evidence over a newer attempt.
        let connection = self.state.lock().await.snapshot().clone();
        telemetry.record(
            if connection.failure.is_some() {
                usque_transport::ConnectionEventType::Failed
            } else {
                usque_transport::ConnectionEventType::Disconnected
            },
            connection.failure.as_ref().map(|f| f.stage),
            Default::default(),
            None,
            connection.failure.clone(),
        );
        let mut evidence = self.retained_connection_evidence.lock().await;
        if self
            .session_generation
            .load(std::sync::atomic::Ordering::Relaxed)
            != generation
        {
            return;
        }
        *evidence = Some(ConnectionEvidence {
            connection,
            timeline: telemetry.snapshot(),
            quality: None,
            session_generation: generation,
            retained: true,
            cleanup_status: failure.as_ref().map_or("cancelled", |e| e.cleanup_status),
            startup_failure: failure,
            captured_at: SystemTime::now(),
        });
    }
    pub(crate) async fn capture_connection_evidence(&self) -> ConnectionEvidence {
        // Use the same lock ordering as status reads. This does not reconcile
        // runtime state or create observations, sockets or sampling timers.
        let plane = self.data_plane.lock().await;
        let connection = self.state.lock().await.snapshot().clone();
        if let Some(active) = plane.as_ref() {
            return ConnectionEvidence::active(active, connection);
        }
        if let Some(retained) = self.retained_connection_evidence.lock().await.as_ref() {
            return retained.clone();
        }
        ConnectionEvidence {
            startup_failure: None,
            connection,
            timeline: ConnectionTimelineSnapshot::default(),
            quality: None,
            session_generation: 0,
            retained: false,
            cleanup_status: "not_sampled",
            captured_at: SystemTime::now(),
        }
    }

    pub(crate) async fn evidence_capture_is_current(&self, captured: &ConnectionEvidence) -> bool {
        let plane = self.data_plane.lock().await;
        if captured.retained {
            plane.is_none()
                && matches!(
                    self.state.lock().await.snapshot().phase,
                    usque_core::ConnectionPhase::Disconnected | usque_core::ConnectionPhase::Error
                )
                && self
                    .retained_connection_evidence
                    .lock()
                    .await
                    .as_ref()
                    .is_some_and(|current| {
                        current.session_generation == captured.session_generation
                    })
        } else {
            plane
                .as_ref()
                .map(|active| active.session_generation)
                .unwrap_or(0)
                == captured.session_generation
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn startup_owner_retains_timeline_and_old_cleanup_cannot_replace_new_attempt() {
        let directory = tempfile::tempdir().unwrap();
        let service =
            ControlService::open(ConfigStore::new(directory.path().join("config.json"))).unwrap();
        let first = service.next_session_generation();
        let attempt = crate::linux_runtime::StartupAttempt::default();
        attempt.cleanup.pending();
        let failure = attempt.evidence(&std::io::Error::from_raw_os_error(1));
        service
            .retain_linux_startup(first, &attempt.telemetry, Some(failure.clone()))
            .await;
        assert!(
            !service
                .capture_connection_evidence()
                .await
                .timeline
                .events
                .is_empty()
        );
        let second = service.next_session_generation();
        let next = crate::linux_runtime::StartupAttempt::default();
        service
            .retain_linux_startup(
                second,
                &next.telemetry,
                Some(next.evidence(&std::io::Error::from_raw_os_error(2))),
            )
            .await;
        let cleanup = usque_ipc::linux_failure::wrap(
            usque_ipc::agent_v1::LinuxFailureStage::Recovery,
            std::io::Error::from_raw_os_error(13),
        );
        attempt.cleanup.complete(&Err(cleanup));
        assert_eq!(failure.resolved().cleanup_status, "failed");
        assert_eq!(
            failure.resolved().cleanup_detail.unwrap().os_errno,
            Some(13)
        );
        service
            .retain_linux_startup(first, &attempt.telemetry, Some(failure))
            .await;
        let captured = service.capture_connection_evidence().await;
        assert_eq!(captured.session_generation, second);
        assert_eq!(
            captured.startup_failure.unwrap().resolved().cleanup_status,
            "not_requested"
        );
    }

    use usque_core::storage::ConfigStore;
    use usque_transport::{ConnectionEventType, ConnectionTelemetry};

    #[tokio::test]
    async fn capture_projects_failed_runtime_without_mutating_connection_state() {
        let directory = tempfile::tempdir().unwrap();
        let service =
            ControlService::open(ConfigStore::new(directory.path().join("config.json"))).unwrap();
        let profile = service.config_snapshot().await.runtime_profiles()[0].clone();
        service
            .install_test_session(profile, false, 0)
            .await
            .unwrap();
        {
            let mut plane = service.data_plane.lock().await;
            let crate::ActiveRuntime::Harness(runtime) = &mut plane.as_mut().unwrap().runtime
            else {
                panic!("harness");
            };
            runtime.gate_status.failure = Some(usque_core::vpngate::GateFailure::Certificate);
        }
        let before = service.state.lock().await.snapshot().clone();
        let captured = service.capture_connection_evidence().await;
        assert_eq!(
            captured.connection.phase,
            usque_core::ConnectionPhase::Error
        );
        assert!(captured.connection.failure.is_some());
        assert!(!captured.connection.error.unwrap().retryable);
        let after = service.state.lock().await.snapshot().clone();
        assert_eq!(after.phase, before.phase);
        assert_eq!(after.failure, before.failure);
    }

    #[tokio::test]
    async fn terminal_timeline_survives_without_a_runtime_and_new_runtime_invalidates_capture() {
        let directory = tempfile::tempdir().unwrap();
        let service =
            ControlService::open(ConfigStore::new(directory.path().join("config.json"))).unwrap();
        let telemetry = ConnectionTelemetry::default();
        telemetry.record(
            ConnectionEventType::Failed,
            None,
            Default::default(),
            None,
            None,
        );
        let terminal = ConnectionEvidence {
            startup_failure: None,
            connection: ConnectionSnapshot::default(),
            timeline: telemetry.snapshot(),
            quality: None,
            session_generation: 7,
            retained: true,
            cleanup_status: "shutdown_failed",
            captured_at: SystemTime::now(),
        };
        *service.retained_connection_evidence.lock().await = Some(terminal);
        let captured = service.capture_connection_evidence().await;
        assert_eq!(captured.timeline.events.len(), 1);
        assert!(captured.retained);
        assert!(service.evidence_capture_is_current(&captured).await);
        service
            .retained_connection_evidence
            .lock()
            .await
            .as_mut()
            .unwrap()
            .session_generation = 8;
        assert!(!service.evidence_capture_is_current(&captured).await);
        *service.retained_connection_evidence.lock().await = None;
        assert!(
            service
                .capture_connection_evidence()
                .await
                .timeline
                .events
                .is_empty()
        );
    }

    #[tokio::test]
    async fn disconnect_freezes_and_completes_the_actual_runtime_timeline() {
        let directory = tempfile::tempdir().unwrap();
        let service =
            ControlService::open(ConfigStore::new(directory.path().join("config.json"))).unwrap();
        let profile = service.config_snapshot().await.runtime_profiles()[0].clone();
        service
            .install_test_session(profile.clone(), false, 0)
            .await
            .unwrap();
        {
            let mut plane = service.data_plane.lock().await;
            let crate::ActiveRuntime::Harness(runtime) = &mut plane.as_mut().unwrap().runtime
            else {
                panic!("harness");
            };
            runtime.timeline.record_attempt(
                usque_core::Transport::Http3,
                usque_core::AddressFamily::Ipv4,
            );
        }
        let active = service.capture_connection_evidence().await;
        assert!(!active.retained);
        service.disconnect().await.unwrap();
        service.await_disconnect_cleanup().await.unwrap();
        let terminal = service.capture_connection_evidence().await;
        assert!(terminal.retained);
        assert_eq!(terminal.session_generation, active.session_generation);
        assert_eq!(terminal.timeline.events.len(), 2);
        assert_eq!(
            terminal.timeline.events.last().unwrap().event_type,
            ConnectionEventType::Disconnected
        );
        assert_eq!(terminal.cleanup_status, "shutdown_returned");
        service
            .install_test_session(profile, false, 0)
            .await
            .unwrap();
        assert!(!service.evidence_capture_is_current(&terminal).await);
        assert!(!service.capture_connection_evidence().await.retained);
    }
}
