//! Read-only Linux recovery observations, separate from Windows receipts.
use crate::{ControlService, diagnostics::LinuxPlatformObservation, linux_agent::LinuxAgentClient};
use std::{sync::Arc, time::Duration};
use usque_core::{
    ConnectionError, ConnectionPhase, ConnectionSnapshot, ErrorCode, KillSwitchState,
};
use usque_ipc::agent_v1::AgentPhase;

const MAX_AGE: Duration = Duration::from_secs(10);

impl ControlService {
    #[cfg(not(test))]
    pub(crate) async fn recover_linux_orphan_on_disconnect(
        &self,
    ) -> Result<(), crate::ControlServiceError> {
        let pending = self
            .linux_platform
            .lock()
            .await
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
            *self.linux_platform.lock().await = Some(observed);
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
                *self.linux_platform.lock().await = Some(observation);
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
        if let Ok(Ok(observation)) = tokio::time::timeout(Duration::from_secs(5), async {
            capture(
                &LinuxAgentClient::connect().await?,
                self.session_generation
                    .load(std::sync::atomic::Ordering::Acquire),
            )
            .await
        })
        .await
        {
            return Some(observation);
        }
        self.linux_platform.lock().await.clone()
    }

    pub(crate) async fn project_linux_recovery(&self, snapshot: &mut ConnectionSnapshot) {
        if let Some(observation) = self.linux_platform.lock().await.as_ref() {
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
}
