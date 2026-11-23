//! Read-only Linux recovery observations, separate from Windows receipts.
use crate::{ControlService, diagnostics::LinuxPlatformObservation, linux_agent::LinuxAgentClient};
use std::{sync::Arc, time::Duration};
use usque_core::{
    ConnectionError, ConnectionPhase, ConnectionSnapshot, ErrorCode, KillSwitchState,
};
use usque_ipc::agent_v1::AgentPhase;

const MAX_AGE: Duration = Duration::from_secs(10);

#[derive(Default)]
pub(crate) struct LinuxObservationCache {
    latest: Option<LinuxPlatformObservation>,
    // A shutdown owner may be waiting on the Agent or retained after failure.
    // Its observations are not evidence of a separate orphan transaction.
    cleaning: std::collections::HashSet<uuid::Uuid>,
    // A read can span both Agent status and physical-network requests. A
    // confirmed cleanup fences reads that were already in flight.
    cleanup_epoch: u64,
}

impl LinuxObservationCache {
    pub(super) fn begin_cleanup(&mut self, operations: [Option<uuid::Uuid>; 3]) {
        self.cleaning.extend(operations.into_iter().flatten());
    }

    fn publish(&mut self, epoch: u64, observation: LinuxPlatformObservation) -> bool {
        if self.cleanup_epoch != epoch {
            return false;
        }
        self.latest = Some(observation);
        true
    }

    pub(super) fn retire(&mut self, operations: [Option<uuid::Uuid>; 3]) {
        for id in operations.iter().flatten() {
            self.cleaning.remove(id);
        }
        self.cleanup_epoch += 1;
        if self.latest.as_ref().is_some_and(|observed| {
            uuid::Uuid::parse_str(&observed.state.operation_id)
                .is_ok_and(|id| operations.contains(&Some(id)))
        }) {
            self.latest = None;
        }
    }
}

impl crate::ActiveRuntime {
    pub(super) fn linux_cleanup_operations(&self) -> [Option<uuid::Uuid>; 3] {
        match self {
            crate::ActiveRuntime::Vpn(vpn) => vpn.cleanup_operations(),
            #[cfg(test)]
            crate::ActiveRuntime::Harness(runtime) if runtime.vpn => {
                [Some(runtime.operation_id), None, None]
            }
            _ => [None; 3],
        }
    }
}

impl ControlService {
    #[cfg(not(test))]
    pub(crate) async fn recover_linux_orphan_on_disconnect(
        &self,
    ) -> Result<(), crate::ControlServiceError> {
        let pending = self
            .linux_platform
            .lock()
            .await
            .latest
            .as_ref()
            .is_some_and(|v| v.state.phase != AgentPhase::Clean as i32);
        if !pending {
            return Ok(());
        }
        let attempt = async {
            let client = LinuxAgentClient::connect().await?;
            let state = client.state().await?;
            if state.phase != AgentPhase::Clean as i32 {
                client.recover(&state).await?;
            }
            let observed = capture(
                &client,
                self.session_generation
                    .load(std::sync::atomic::Ordering::Acquire),
            )
            .await?;
            if observed.state.phase != AgentPhase::Clean as i32 {
                return Err(std::io::Error::other("Linux recovery remains pending"));
            }
            let mut cache = self.linux_platform.lock().await;
            cache.cleanup_epoch += 1;
            cache.latest = Some(observed);
            Ok::<_, std::io::Error>(())
        }
        .await;
        attempt.map_err(|error| crate::ControlServiceError::PlatformVpn(error.to_string()))
    }

    pub(crate) async fn project_linux_active_recovery(
        &self,
        operation: uuid::Uuid,
        snapshot: &mut ConnectionSnapshot,
    ) {
        let observed = self.linux_platform.lock().await;
        let Some(observed) = observed
            .latest
            .as_ref()
            .filter(|v| v.state.operation_id == operation.to_string())
        else {
            return;
        };
        if matches!(
            AgentPhase::try_from(observed.state.phase),
            Ok(AgentPhase::RecoveryRequired | AgentPhase::Recovering)
        ) {
            project(observed, snapshot);
        }
    }

    pub async fn run_linux_observation_monitor(self: Arc<Self>) {
        let mut client = None;
        loop {
            let epoch = self.linux_platform.lock().await.cleanup_epoch;
            let result = tokio::time::timeout(Duration::from_secs(5), async {
                if client.is_none() {
                    client = Some(LinuxAgentClient::connect().await?);
                }
                capture(
                    client.as_ref().expect("connected client"),
                    self.session_generation
                        .load(std::sync::atomic::Ordering::Acquire),
                )
                .await
            })
            .await;
            if let Ok(Ok(observation)) = result {
                if !self.linux_platform.lock().await.publish(epoch, observation) {
                    // Cleanup overtook this sample. Read the current owner immediately.
                    continue;
                }
            } else {
                // Retain evidence when the service disappears. Old guard
                // observations become unknown instead of claiming safety.
                client = None;
            }
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    }

    pub(crate) async fn linux_diagnostic_observation(&self) -> Option<LinuxPlatformObservation> {
        #[cfg(not(test))]
        let epoch = self.linux_platform.lock().await.cleanup_epoch;
        #[cfg(not(test))]
        if let Ok(Ok(observation)) = tokio::time::timeout(Duration::from_secs(5), async {
            capture(
                &LinuxAgentClient::connect().await?,
                self.session_generation
                    .load(std::sync::atomic::Ordering::Acquire),
            )
            .await
        })
        .await
            && self.linux_platform.lock().await.cleanup_epoch == epoch
        {
            return Some(observation);
        }
        self.linux_platform.lock().await.latest.clone()
    }

    pub(crate) async fn project_linux_recovery(&self, snapshot: &mut ConnectionSnapshot) {
        if snapshot
            .error
            .as_ref()
            .is_some_and(|error| error.code == ErrorCode::DisconnectCleanupFailed)
        {
            // A different Agent transaction cannot discharge the local shutdown owner either.
            return;
        }
        let cache = self.linux_platform.lock().await;
        if let Some(observation) = &cache.latest
            && !uuid::Uuid::parse_str(&observation.state.operation_id)
                .is_ok_and(|id| cache.cleaning.contains(&id))
        {
            project(observation, snapshot);
        }
    }
}

#[cfg(not(test))]
pub(crate) async fn disconnected_probe_safe() -> bool {
    tokio::time::timeout(Duration::from_secs(3), async {
        match LinuxAgentClient::connect().await {
            Ok(client) => client
                .state()
                .await
                .is_ok_and(|state| state.phase == AgentPhase::Clean as i32),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                !std::path::Path::new("/etc/usque/install.json").exists()
            }
            Err(_) => false,
        }
    })
    .await
    .unwrap_or(false)
}

async fn capture(
    client: &LinuxAgentClient,
    session_generation: u64,
) -> std::io::Result<LinuxPlatformObservation> {
    let state = client.state().await?;
    let physical = match uuid::Uuid::parse_str(&state.operation_id) {
        Ok(id) => client.physical_network(id).await.ok(),
        Err(_) => None,
    };
    Ok(LinuxPlatformObservation {
        session_generation,
        state,
        physical,
        captured_at: tokio::time::Instant::now(),
    })
}

fn project(observation: &LinuxPlatformObservation, snapshot: &mut ConnectionSnapshot) {
    if observation.state.phase == AgentPhase::Clean as i32 {
        return;
    }
    snapshot.phase = ConnectionPhase::Error;
    snapshot.kill_switch_state =
        if observation.captured_at.elapsed() <= MAX_AGE && observation.state.kill_switch_active {
            KillSwitchState::Active
        } else {
            KillSwitchState::Error
        };
    snapshot.error = Some(ConnectionError {
        code: ErrorCode::PlatformSetupFailed,
        message: "Linux network recovery remains pending. Retry Disconnect or repair the background service.".into(),
        retryable: true,
    });
    snapshot.failure = Some(usque_core::TransportFailure::new(
        usque_core::TransportFailureCode::PlatformRecoveryPending,
        usque_core::TransportStage::PlatformRecovery,
    ));
}

#[cfg(test)]
mod tests {
    use super::*;
    struct LockedVault(tokio::sync::Notify);
    #[async_trait::async_trait]
    impl usque_platform::SecretVault for LockedVault {
        async fn put(
            &self,
            _: uuid::Uuid,
            _: usque_platform::SecretRecord,
            _: &[u8],
        ) -> Result<(), usque_platform::VaultError> {
            unreachable!()
        }
        async fn get(
            &self,
            _: uuid::Uuid,
            _: usque_platform::SecretRecord,
        ) -> Result<Option<zeroize::Zeroizing<Vec<u8>>>, usque_platform::VaultError> {
            self.0.notify_one();
            std::future::pending().await
        }
        async fn delete(
            &self,
            _: uuid::Uuid,
            _: usque_platform::SecretRecord,
        ) -> Result<(), usque_platform::VaultError> {
            unreachable!()
        }
    }
    #[tokio::test]
    async fn keyring_unlock_wait_does_not_block_control_status_or_config() {
        let dir = tempfile::tempdir().unwrap();
        let vault = Arc::new(LockedVault(tokio::sync::Notify::new()));
        let service = ControlService::open_with_vault(
            usque_core::storage::ConfigStore::new(dir.path().join("config.json")),
            vault.clone(),
        )
        .unwrap();
        let startup = tokio_util::task::AbortOnDropHandle::new(tokio::spawn({
            let service = service.clone();
            async move { service.initialize_linux_user_state().await }
        }));
        tokio::time::timeout(Duration::from_secs(1), vault.0.notified())
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(1), async {
            let _ = service.config_snapshot().await;
            assert_eq!(
                service.status_snapshot().await.phase,
                ConnectionPhase::Disconnected
            );
        })
        .await
        .unwrap();
        drop(startup);
    }

    #[tokio::test]
    async fn startup_metadata_bypasses_locked_keyring_and_migration_lease() {
        use usque_ipc::v1::{self, control_request, control_response};
        let dir = tempfile::tempdir().unwrap();
        let vault = Arc::new(LockedVault(tokio::sync::Notify::new()));
        let service = ControlService::open_with_vault(
            usque_core::storage::ConfigStore::new(dir.path().join("config.json")),
            vault.clone(),
        )
        .unwrap();
        let before = service.config_snapshot().await;
        let startup = tokio_util::task::AbortOnDropHandle::new(tokio::spawn({
            let service = service.clone();
            async move { service.initialize_linux_user_state().await }
        }));
        tokio::time::timeout(Duration::from_secs(1), vault.0.notified())
            .await
            .unwrap();
        let response = tokio::time::timeout(
            Duration::from_secs(1),
            service.handle(v1::ControlRequest {
                request_id: "startup-metadata".into(),
                payload: Some(control_request::Payload::ListProfiles(
                    v1::ListProfilesRequest {
                        metadata_only: true,
                    },
                )),
            }),
        )
        .await
        .expect("metadata must not wait for the migration lease or vault");
        assert!(response.error.is_none());
        let Some(control_response::Payload::ProfileList(catalog)) = response.payload else {
            panic!("expected profile metadata");
        };
        assert_eq!(catalog.profiles.len(), before.profiles.len());
        assert_eq!(
            catalog.active_profile_id,
            before.active_profile_id.unwrap().to_string()
        );
        assert!(
            catalog.identity_statuses.is_empty(),
            "metadata must never grant readiness"
        );
        assert_eq!(service.config_snapshot().await, before);
        // Full validation still waits for the vault instead of claiming Ready.
        assert!(
            tokio::time::timeout(
                Duration::from_millis(20),
                service.handle(v1::ControlRequest {
                    request_id: "validated-catalog".into(),
                    payload: Some(control_request::Payload::ListProfiles(
                        v1::ListProfilesRequest::default()
                    )),
                })
            )
            .await
            .is_err()
        );
        drop(startup);
    }

    #[tokio::test]
    async fn completed_flutter_migration_read_does_not_take_mutation_lease() {
        use usque_ipc::v1;
        let dir = tempfile::tempdir().unwrap();
        let service = ControlService::open_with_vault(
            usque_core::storage::ConfigStore::new(dir.path().join("config.json")),
            Arc::new(LockedVault(tokio::sync::Notify::new())),
        )
        .unwrap();
        service
            .config
            .write()
            .await
            .preferences
            .profiles_migrated_from_flutter = true;
        let _mutation = service.mutation_lock.lock().await;
        let catalog = tokio::time::timeout(
            Duration::from_secs(1),
            service.import_legacy_profiles(v1::ImportLegacyProfilesRequest::default()),
        )
        .await
        .expect("already migrated reads must not wait for keyring recovery")
        .unwrap();
        assert!(!catalog.profiles.is_empty());
    }
    #[test]
    fn cold_recovery_and_stale_protection_do_not_appear_disconnected_or_safe() {
        let mut observation = LinuxPlatformObservation {
            session_generation: 0,
            state: usque_ipc::agent_v1::AgentState {
                phase: AgentPhase::RecoveryRequired as i32,
                kill_switch_active: true,
                ..Default::default()
            },
            physical: None,
            captured_at: tokio::time::Instant::now(),
        };
        let mut snapshot = ConnectionSnapshot::default();
        project(&observation, &mut snapshot);
        assert_eq!(snapshot.phase, ConnectionPhase::Error);
        assert_eq!(snapshot.kill_switch_state, KillSwitchState::Active);
        observation.captured_at -= MAX_AGE + Duration::from_secs(1);
        project(&observation, &mut snapshot);
        assert_eq!(snapshot.kill_switch_state, KillSwitchState::Error);
        observation.state.phase = AgentPhase::Clean as i32;
        let mut clean = ConnectionSnapshot::default();
        project(&observation, &mut clean);
        assert_eq!(clean.phase, ConnectionPhase::Disconnected);
    }

    #[tokio::test]
    async fn confirmed_vpn_cleanup_retires_cached_and_in_flight_agent_state() {
        let dir = tempfile::tempdir().unwrap();
        let service = ControlService::open_with_vault(
            usque_core::storage::ConfigStore::new(dir.path().join("config.json")),
            Arc::new(crate::tests::MemoryVault::default()),
        )
        .unwrap();
        let mut profile = service.config_snapshot().await.active_profile().unwrap();
        profile.frontends.tunnel = true;
        profile.kill_switch = true;
        profile.canonicalize_mode();
        service
            .install_test_session(profile, true, 0)
            .await
            .unwrap();
        let release = tokio_util::sync::CancellationToken::new();
        let operation = {
            let mut active = service.data_plane.lock().await;
            let crate::ActiveRuntime::Harness(runtime) = &mut active.as_mut().unwrap().runtime
            else {
                unreachable!()
            };
            runtime.shutdown_failures = 1;
            runtime.shutdown_release = Some(release.clone());
            runtime.operation_id
        };
        let sample = LinuxPlatformObservation {
            state: usque_ipc::agent_v1::AgentState {
                phase: AgentPhase::Active as i32,
                operation_id: operation.to_string(),
                kill_switch_active: true,
                ..Default::default()
            },
            physical: None,
            captured_at: tokio::time::Instant::now(),
            session_generation: service
                .session_generation
                .load(std::sync::atomic::Ordering::Acquire),
        };
        let initial_epoch = {
            let mut cache = service.linux_platform.lock().await;
            let epoch = cache.cleanup_epoch;
            cache.publish(epoch, sample.clone());
            epoch
        };
        let _lifecycle = service.mutation_lock.lock().await;
        service.teardown_for_reconnect_locked().await.unwrap();
        assert_eq!(
            service.status_snapshot().await.phase,
            ConnectionPhase::Reconnecting
        );
        release.cancel();
        assert!(service.await_disconnect_cleanup().await.is_err());
        assert_eq!(
            service.linux_platform.lock().await.cleanup_epoch,
            initial_epoch
        );
        assert!(service.linux_platform.lock().await.latest.is_some());
        let failed = service.event_snapshot().await;
        assert_eq!(failed.error.unwrap().code, "DISCONNECT_CLEANUP_FAILED");
        assert_eq!(
            failed.kill_switch_state,
            usque_ipc::v1::KillSwitchState::Error as i32
        );
        assert!(service.retry_disconnect_cleanup().await);
        service.await_disconnect_cleanup().await.unwrap();
        assert!(service.disconnect_owners.lock().await.is_empty());
        assert!(service.linux_platform.lock().await.latest.is_none());
        service
            .linux_platform
            .lock()
            .await
            .publish(initial_epoch, sample.clone());
        assert_eq!(
            service.event_snapshot().await.kill_switch_state,
            usque_ipc::v1::KillSwitchState::NotApplicable as i32
        );

        // A distinct Agent transaction still projects a real recovery state,
        // even if an earlier sample arrives after the new one.
        let mut other = sample.clone();
        other.state.operation_id = uuid::Uuid::new_v4().to_string();
        other.state.phase = AgentPhase::RecoveryRequired as i32;
        {
            let mut cache = service.linux_platform.lock().await;
            let epoch = cache.cleanup_epoch;
            cache.publish(epoch, other);
            cache.publish(initial_epoch, sample);
            cache.retire([Some(operation), None, None]);
        }
        let status = service.status_snapshot().await;
        assert_eq!(status.phase, ConnectionPhase::Error);
        assert_eq!(status.kill_switch_state, KillSwitchState::Active);
    }
}
