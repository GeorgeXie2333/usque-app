use std::sync::{Arc, atomic::Ordering};

use usque_core::network_settings::{
    ApplyStatus, NetworkSettingsPatch, NetworkSettingsState, merge_patch, plan_application,
};
use usque_core::{ConnectionPhase, Profile, ReconfigureClass, storage::StoreError};
use usque_ipc::v1;

use crate::{
    ControlService, ControlServiceError, parse_profile_id, profile_from_proto, profile_to_proto,
};

impl ControlService {
    pub(crate) async fn network_settings_state(&self) -> v1::NetworkSettingsState {
        let _submission = self.settings_submission.lock().await;
        let config = self.config.read().await;
        let stored = config.active_profile();
        let shared = Some(
            config
                .network
                .hydrate(&usque_core::config::Account::default_account()),
        );
        drop(config);
        let mut state = self.settings.lock().await;
        if state.stored_profile != stored || state.shared_network_profile != shared {
            state.stored_profile = stored;
            state.shared_network_profile = shared;
            state.advance();
        }
        to_proto(&state)
    }

    pub(crate) async fn publish_settings_runtime(
        &self,
        profile: Option<Profile>,
        generation: Option<u64>,
    ) {
        #[cfg(windows)]
        let recovering = self.windows_recovery.lock().await.pending.is_some();
        #[cfg(not(windows))]
        let recovering = false;
        let mut state = self.settings.lock().await;
        state.applied_profile = profile;
        state.session_id = generation.map(|value| value.to_string());
        state.apply_status = if self.settings_applying.load(Ordering::SeqCst) || recovering {
            ApplyStatus::Applying
        } else if state.applied_profile.is_some() {
            ApplyStatus::Applied
        } else {
            ApplyStatus::Deferred
        };
        state.error_code = None;
        state.advance();
        self.settings_tx.send_replace(state.sequence);
    }

    pub(crate) async fn save_network_settings(
        &self,
        request: v1::SaveNetworkSettingsRequest,
    ) -> Result<v1::NetworkSettingsState, ControlServiceError> {
        let _submission = self.settings_submission.lock().await;
        let intent = self.settings_intent.load(Ordering::SeqCst);
        // Reserve only an immediately available executor. Saving never joins
        // the long lifecycle queue, nor creates a delayed apply operation.
        let lifecycle = Arc::clone(&self.mutation_lock).try_lock_owned().ok();
        let runtime = self.data_plane.try_lock().ok().and_then(|active| {
            active.as_ref().map(|active| {
                (
                    active.profile.clone(),
                    active.session_generation,
                    active.runtime.health(),
                )
            })
        });
        let phase = self
            .state
            .try_lock()
            .map(|state| state.snapshot().phase)
            .unwrap_or(ConnectionPhase::Reconnecting);
        let confirmed = self.settings.lock().await.applied_profile.is_some();
        let patch = NetworkSettingsPatch {
            operation_id: parse_profile_id(&request.operation_id)?,
            account_id: parse_profile_id(&request.account_id)?,
            values: profile_from_proto(request.values.ok_or_else(|| {
                ControlServiceError::InvalidRequest("network settings values are missing".into())
            })?)?,
            changed_fields: request.changed_fields,
        };
        if patch.changed_fields.iter().any(|field| field == "vpn_gate") {
            self.pin_gate_settings(&patch.values.vpn_gate).await?;
        }
        // Only local read/modify/write work holds the configuration guard.
        let mut config = self.config.write().await;
        let store = self.store.clone();
        let edit = patch.clone();
        let commit = tokio::task::spawn_blocking(move || {
            store.update(|latest| {
                merge_patch(latest, &edit)
                    .map_err(|error| StoreError::NetworkSettings(error.to_string()))
            })
        })
        .await
        .map_err(|error| ControlServiceError::PersistenceWorker(error.to_string()))?;
        let (next, stored) = match commit {
            Ok(result) => result,
            Err(StoreError::CommitUncertain(_)) => {
                let store = self.store.clone();
                let observed = tokio::task::spawn_blocking(move || store.load()).await;
                let mut state = self.settings.lock().await;
                if let Ok(Ok(next)) = observed {
                    state.stored_profile = next.active_profile();
                    state.shared_network_profile = Some(
                        next.network
                            .hydrate(&usque_core::config::Account::default_account()),
                    );
                    *config = next;
                }
                state.operation_id = Some(patch.operation_id);
                state.persisted = None;
                state.apply_status = ApplyStatus::Unknown;
                state.error_code = Some("NETWORK_SETTINGS_COMMIT_UNCONFIRMED".into());
                state.advance();
                self.settings_tx.send_replace(state.sequence);
                return Ok(to_proto(&state));
            }
            Err(error) => return Err(error.into()),
        };
        let shared = next
            .network
            .hydrate(&usque_core::config::Account::default_account());
        *config = next;
        drop(config);

        let stable = runtime.as_ref().is_some_and(|(_, _, health)| {
            matches!(health, usque_transport::RuntimeHealth::Connected { .. })
        });
        let plan = plan_application(
            runtime
                .as_ref()
                .filter(|_| confirmed)
                .map(|(profile, _, _)| profile),
            &stored,
            &patch.changed_fields,
            phase,
            lifecycle.is_some() && stable && self.settings_intent.load(Ordering::SeqCst) == intent,
        );
        let mut state = self.settings.lock().await;
        state.operation_id = Some(patch.operation_id);
        state.persisted = Some(true);
        state.stored_profile = Some(stored);
        state.shared_network_profile = Some(shared);
        state.error_code = None;
        if let Some((profile, generation, _)) = &runtime
            && confirmed
        {
            state.applied_profile = Some(profile.clone());
            state.session_id = Some(generation.to_string());
        }
        let plan = match plan {
            Ok(plan) => plan,
            Err(_) => {
                state.apply_status = ApplyStatus::Failed;
                state.error_code = Some("NETWORK_SETTINGS_APPLY_INVALID".into());
                state.advance();
                self.settings_tx.send_replace(state.sequence);
                return Ok(to_proto(&state));
            }
        };
        state.apply_status = plan.status;
        state.advance();
        let response = to_proto(&state);
        self.settings_tx.send_replace(state.sequence);
        drop(state);
        if let (Some(target), Some(lifecycle), Some((previous, generation, _))) =
            (plan.target, lifecycle, runtime)
        {
            self.settings_applying.store(true, Ordering::SeqCst);
            let service = self.clone();
            tokio::spawn(async move {
                let _lifecycle = lifecycle;
                let current = service
                    .data_plane
                    .lock()
                    .await
                    .as_ref()
                    .is_some_and(|active| active.session_generation == generation);
                if !current || service.settings_intent.load(Ordering::SeqCst) != intent {
                    service.settings_applying.store(false, Ordering::SeqCst);
                    service
                        .finish_settings_error(
                            patch.operation_id,
                            "NETWORK_SETTINGS_CANCELLED",
                            ApplyStatus::Deferred,
                            false,
                        )
                        .await;
                    return;
                }
                let result = service
                    .execute_settings_plan(&target, &previous, plan.class, intent)
                    .await;
                match result {
                    Ok(()) => {
                        service.settings_applying.store(false, Ordering::SeqCst);
                        if service.settings_intent.load(Ordering::SeqCst) != intent {
                            let _ = service.disconnect_locked().await;
                            return;
                        }
                        let generation = {
                            let mut active = service.data_plane.lock().await;
                            active
                                .as_mut()
                                .filter(|active| {
                                    matches!(
                                        active.runtime.health(),
                                        usque_transport::RuntimeHealth::Connected { .. }
                                    )
                                })
                                .map(|active| {
                                    active.profile = target.clone();
                                    active.session_generation
                                })
                        };
                        if generation.is_none() {
                            service
                                .finish_settings_error(
                                    patch.operation_id,
                                    "NETWORK_SETTINGS_RUNTIME_PENDING",
                                    ApplyStatus::Applying,
                                    true,
                                )
                                .await;
                            return;
                        }
                        *service.session_profile.lock().await = Some(target.clone());
                        service
                            .publish_settings_runtime(Some(target), generation)
                            .await;
                    }
                    Err(error) => {
                        service.settings_applying.store(false, Ordering::SeqCst);
                        let restored = plan.class == ReconfigureClass::ColdReconnect
                            && service
                                .data_plane
                                .lock()
                                .await
                                .as_ref()
                                .is_some_and(|active| {
                                    active.profile == previous
                                        && matches!(
                                            active.runtime.health(),
                                            usque_transport::RuntimeHealth::Connected { .. }
                                        )
                                });
                        let status = match &error {
                            #[cfg(windows)]
                            ControlServiceError::PlatformRecoveryPending {
                                operation_id,
                                journal_generation,
                            } => {
                                let recovery_target = if target.vpn_gate == previous.vpn_gate {
                                    &previous
                                } else {
                                    &target
                                };
                                *service.session_profile.lock().await =
                                    Some(recovery_target.clone());
                                if service.settings_intent.load(Ordering::SeqCst) == intent
                                    && service
                                        .enter_windows_automatic_recovery(
                                            previous.id,
                                            operation_id.clone(),
                                            *journal_generation,
                                        )
                                        .await
                                        .is_ok()
                                {
                                    ApplyStatus::Applying
                                } else {
                                    ApplyStatus::Failed
                                }
                            }
                            _ => ApplyStatus::Failed,
                        };
                        service
                            .finish_settings_error(
                                patch.operation_id,
                                if status == ApplyStatus::Applying {
                                    "NETWORK_SETTINGS_PLATFORM_RECOVERY_PENDING"
                                } else {
                                    "NETWORK_SETTINGS_APPLY_FAILED"
                                },
                                status,
                                !restored,
                            )
                            .await
                    }
                }
            });
        }
        Ok(response)
    }

    async fn execute_settings_plan(
        &self,
        target: &Profile,
        previous: &Profile,
        class: ReconfigureClass,
        intent: u64,
    ) -> Result<(), ControlServiceError> {
        match class {
            ReconfigureClass::HotTrafficPolicy => {}
            ReconfigureClass::HotFrontends => self.hot_reconfigure_frontends(target).await?,
            ReconfigureClass::HotSystemProxy => self.hot_apply_system_proxy(target).await?,
            ReconfigureClass::HotTunnelAttach => self.hot_tunnel_attach(target).await?,
            ReconfigureClass::HotVpnGate => self.hot_replace_gate(target).await?,
            ReconfigureClass::ColdReconnect => {
                self.disconnect_locked().await?;
                self.await_disconnect_cleanup().await?;
                if self.settings_intent.load(Ordering::SeqCst) != intent {
                    return Err(ControlServiceError::InvalidRequest(
                        "network settings application was cancelled".into(),
                    ));
                }
                *self.session_profile.lock().await = Some(target.clone());
                if let Err(error) = self.connect_locked(target.id).await {
                    if self.settings_intent.load(Ordering::SeqCst) == intent
                        && previous.vpn_gate == target.vpn_gate
                    {
                        *self.session_profile.lock().await = Some(previous.clone());
                        let _ = self.connect_locked(previous.id).await;
                    }
                    return Err(error);
                }
            }
            ReconfigureClass::PersistOnly | ReconfigureClass::Reject => {}
        }
        self.hot_update_traffic_policy(target).await?;
        self.apply_hot_profile_state(target).await;
        Ok(())
    }

    async fn finish_settings_error(
        &self,
        operation_id: uuid::Uuid,
        code: &str,
        status: ApplyStatus,
        clear_confirmation: bool,
    ) {
        let mut state = self.settings.lock().await;
        state.apply_status = if state.operation_id == Some(operation_id) {
            status
        } else if clear_confirmation {
            ApplyStatus::Unknown
        } else {
            ApplyStatus::Deferred
        };
        state.error_code = Some(code.into());
        if clear_confirmation {
            state.applied_profile = None;
        }
        state.advance();
        self.settings_tx.send_replace(state.sequence);
    }
}

fn to_proto(state: &NetworkSettingsState) -> v1::NetworkSettingsState {
    v1::NetworkSettingsState {
        source_epoch: state.source_epoch.to_string(),
        sequence: state.sequence,
        operation_id: state
            .operation_id
            .map(|id| id.to_string())
            .unwrap_or_default(),
        session_id: state.session_id.clone().unwrap_or_default(),
        stored_profile: state.stored_profile.as_ref().map(profile_to_proto),
        shared_network_profile: state.shared_network_profile.as_ref().map(profile_to_proto),
        applied_profile: state.applied_profile.as_ref().map(profile_to_proto),
        apply_status: match state.apply_status {
            ApplyStatus::NotRequired => 1,
            ApplyStatus::Applying => 2,
            ApplyStatus::Applied => 3,
            ApplyStatus::Deferred => 4,
            ApplyStatus::Failed => 5,
            ApplyStatus::Unknown => 6,
        },
        deferred_fields: state.deferred_fields.clone(),
        error_code: state.error_code.clone().unwrap_or_default(),
        persisted: state.persisted,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use usque_core::{CongestionControlAlgorithm, storage::ConfigStore};

    fn service() -> (tempfile::TempDir, ControlService) {
        let directory = tempfile::tempdir().unwrap();
        let service = ControlService::open_with_vault(
            ConfigStore::new(directory.path().join("config.json")),
            Arc::new(crate::tests::MemoryVault::default()),
        )
        .unwrap();
        (directory, service)
    }

    fn request(profile: &Profile, fields: &[&str]) -> v1::SaveNetworkSettingsRequest {
        v1::SaveNetworkSettingsRequest {
            operation_id: uuid::Uuid::new_v4().to_string(),
            account_id: profile.id.to_string(),
            values: Some(profile_to_proto(profile)),
            changed_fields: fields.iter().map(|field| (*field).into()).collect(),
        }
    }

    #[tokio::test]
    async fn saving_does_not_join_the_connection_executor() {
        let (_directory, service) = service();
        let mut profile = service.config_snapshot().await.active_profile().unwrap();
        profile.mtu = 1400;
        let lifecycle = service.mutation_lock.lock().await;
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            service.save_network_settings(request(&profile, &["mtu"])),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(result.persisted, Some(true));
        assert_eq!(result.apply_status, 4);
        assert!(service.data_plane.lock().await.is_none());
        assert_eq!(service.store.load().unwrap().network.mtu, 1400);
        drop(lifecycle);
    }

    #[tokio::test]
    async fn quic_hot_save_keeps_session_listeners_and_platform_leases() {
        let (_directory, service) = service();
        let country = usque_geo::CountryCode::parse("CN").unwrap();
        let geoip = usque_geo::geoip_cache_path(&service.cache_dir, &country);
        let geosite = usque_geo::geosite_cache_path(&service.cache_dir, &country);
        std::fs::create_dir_all(geoip.parent().unwrap()).unwrap();
        std::fs::create_dir_all(geosite.parent().unwrap()).unwrap();
        std::fs::write(
            geoip,
            include_bytes!("../../usque-geo/tests/fixtures/geoip-cn.dat"),
        )
        .unwrap();
        std::fs::write(
            geosite,
            include_bytes!("../../usque-geo/tests/fixtures/geosite-cn.txt"),
        )
        .unwrap();
        let mut profile = service.config_snapshot().await.active_profile().unwrap();
        profile.geo_direct_countries = vec!["CN".into()];
        service
            .install_test_session(profile.clone(), true, 3)
            .await
            .unwrap();
        let (generation, connected_at) = {
            let active = service.data_plane.lock().await;
            let active = active.as_ref().unwrap();
            (active.session_generation, active.connected_at)
        };
        for enabled in [true, false, true] {
            profile.disable_quic = enabled;
            let response = service
                .save_network_settings(request(&profile, &["disable_quic"]))
                .await
                .unwrap();
            assert_eq!(response.persisted, Some(true));
            let _finished = service.mutation_lock.lock().await;
            let state = service.network_settings_state().await;
            assert_eq!(state.apply_status, 3);
            assert_eq!(state.applied_profile.unwrap().disable_quic, enabled);
            assert_eq!(service.store.load().unwrap().network.disable_quic, enabled);
            let active = service.data_plane.lock().await;
            let active = active.as_ref().unwrap();
            assert_eq!(active.session_generation, generation);
            assert_eq!(active.connected_at, connected_at);
            assert_eq!(active.profile.geo_direct_countries, ["CN"]);
            let crate::active_runtime::ActiveRuntime::Harness(harness) = &active.runtime else {
                panic!("harness")
            };
            assert_eq!(harness.disable_quic, enabled);
            assert_eq!(
                (
                    harness.reconnect_count,
                    harness.reconfigure_count,
                    harness.attach_count,
                    harness.detach_count,
                    harness.system_proxy_apply_count
                ),
                (3, 0, 0, 0, 0)
            );
        }
    }

    #[tokio::test]
    async fn mixed_hot_edit_keeps_algorithm_and_previously_deferred_mtu() {
        let (_directory, service) = service();
        let profile = service.config_snapshot().await.active_profile().unwrap();
        service
            .install_test_session(profile.clone(), true, 0)
            .await
            .unwrap();
        let mut next = profile.clone();
        next.mtu = 1400;
        {
            let _busy = service.mutation_lock.lock().await;
            service
                .save_network_settings(request(&next, &["mtu"]))
                .await
                .unwrap();
        }
        next.congestion_control = CongestionControlAlgorithm::Reno;
        next.proxy.http_listeners[0].set_port(9090);
        let result = service
            .save_network_settings(request(
                &next,
                &["congestion_control", "proxy.http_listeners"],
            ))
            .await
            .unwrap();
        assert_eq!(result.apply_status, 2);
        let _finished = service.mutation_lock.lock().await;
        let state = service.network_settings_state().await;
        let applied = state.applied_profile.unwrap();
        assert_eq!(applied.mtu, u32::from(profile.mtu));
        assert_eq!(
            applied.congestion_control,
            profile_to_proto(&profile).congestion_control
        );
        assert!(state.deferred_fields.contains(&"mtu".into()));
        assert!(state.deferred_fields.contains(&"congestion_control".into()));
        assert_eq!(service.store.load().unwrap().network.mtu, 1400);
        assert_eq!(service.test_harness_counts().await.unwrap().1, 1);
    }

    #[tokio::test]
    async fn detach_failure_retains_saved_values_and_invalidates_confirmation() {
        let (_directory, service) = service();
        let mut profile = service.config_snapshot().await.active_profile().unwrap();
        service
            .install_test_session(profile.clone(), true, 0)
            .await
            .unwrap();
        if let Some(active) = service.data_plane.lock().await.as_mut()
            && let crate::active_runtime::ActiveRuntime::Harness(harness) = &mut active.runtime
        {
            harness.fail_after_detach = true;
        }
        profile.frontends.tunnel = false;
        service
            .save_network_settings(request(&profile, &["frontends.tunnel"]))
            .await
            .unwrap();
        let _finished = service.mutation_lock.lock().await;
        let state = service.network_settings_state().await;
        assert_eq!(state.persisted, Some(true));
        assert_eq!(state.apply_status, 5);
        assert!(state.applied_profile.is_none());
        assert!(!service.store.load().unwrap().network.frontends.tunnel);
    }

    #[tokio::test]
    async fn account_commit_cannot_restore_an_old_network_snapshot() {
        let (_directory, service) = service();
        let mut account_edit = service.config_snapshot().await;
        account_edit.profiles[0].name = "Renamed".into();
        let mut profile = account_edit.active_profile().unwrap();
        profile.mtu = 1400;
        service
            .save_network_settings(request(&profile, &["mtu"]))
            .await
            .unwrap();
        service.persist(account_edit).await.unwrap();
        let config = service.store.load().unwrap();
        assert_eq!(config.network.mtu, 1400);
        assert_eq!(config.profiles[0].name, "Renamed");
    }
}
