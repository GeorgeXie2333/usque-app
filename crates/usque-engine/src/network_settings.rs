use std::sync::{Arc, atomic::Ordering};

use usque_core::network_settings::{
    ApplyStatus, NetworkSettingsPatch, NetworkSettingsState, merge_patch, plan_application,
};
use usque_core::{ConnectionPhase, Profile, ReconfigureClass, storage::StoreError};
use usque_ipc::v1;

use crate::{
    ControlService, ControlServiceError, parse_profile_id, profile_from_proto, profile_to_proto,
};

const SYSTEM_PROXY_EXECUTOR_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

/// One bounded executor and one latest, session-scoped system-proxy intent.
/// Other saved settings never enter this queue.
#[derive(Default)]
pub(crate) struct SystemProxyApplications {
    pending: Option<SystemProxyApplication>,
    worker_running: bool,
}

struct SystemProxyApplication {
    operation_id: uuid::Uuid,
    account_id: uuid::Uuid,
    generation: u64,
    intent: u64,
    enabled: bool,
}

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
        let system_proxy_pending = self
            .system_proxy_applications
            .lock()
            .await
            .pending
            .is_some();
        let mut state = self.settings.lock().await;
        state.applied_profile = profile;
        state.session_id = generation.map(|value| value.to_string());
        state.apply_status = if self.settings_applying.load(Ordering::SeqCst)
            || recovering
            || system_proxy_pending
        {
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
        // Saving never joins the long lifecycle queue. Only a confirmed,
        // session-scoped system-proxy switch can reserve a bounded follow-up.
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
        let observed_phase = self
            .state
            .try_lock()
            .map(|state| state.snapshot().phase)
            .ok();
        let phase = observed_phase.unwrap_or(ConnectionPhase::Reconnecting);
        let (confirmed, confirmed_session) = {
            let state = self.settings.lock().await;
            (
                state.applied_profile.is_some(),
                state.applied_profile.as_ref().and_then(|profile| {
                    Some((profile.id, state.session_id.as_ref()?.parse::<u64>().ok()?))
                }),
            )
        };
        let mut values = request.values.ok_or_else(|| {
            ControlServiceError::InvalidRequest("network settings values are missing".into())
        })?;
        if request
            .changed_fields
            .iter()
            .any(|field| field == "frontends.http")
            && values
                .frontends
                .as_ref()
                .is_some_and(|frontends| !frontends.http)
            && let Some(proxy) = &mut values.proxy
        {
            // The GUI carries the old dependent flag when switching HTTP off.
            // Apply the same dependency normalization as merge_patch before
            // validating its wire values; unrelated masks remain fail-closed.
            proxy.system_proxy = false;
        }
        let patch = NetworkSettingsPatch {
            operation_id: parse_profile_id(&request.operation_id)?,
            account_id: parse_profile_id(&request.account_id)?,
            values: profile_from_proto(values)?,
            changed_fields: request.changed_fields,
        };
        if patch.values.custom_chain().is_none()
            && patch.changed_fields.iter().any(|field| field == "vpn_gate")
        {
            self.pin_gate_settings(&patch.values.vpn_gate).await?;
        }
        if patch
            .changed_fields
            .iter()
            .any(|field| field == "chain_exit" || field == "data_plane")
        {
            self.validate_chain_selection(&patch.values)?;
        }
        // Only local read/modify/write work holds the configuration guard.
        let mut config = self.config.write().await;
        let store = self.store.clone();
        let edit = patch.clone();
        let chain_parent = self.cache_dir.clone();
        let commit = tokio::task::spawn_blocking(move || {
            store.update(|latest| {
                let merged = merge_patch(latest, &edit)
                    .map_err(|error| StoreError::NetworkSettings(error.to_string()))?;
                // Validate the committed combination, including fields omitted
                // by old clients, while holding the configuration transaction.
                ControlService::validate_chain_selection_at(&chain_parent, &merged)
                    .map_err(|error| StoreError::NetworkSettings(error.to_string()))?;
                Ok(merged)
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
        let followup = if cfg!(windows)
            && matches!(&plan, Ok(plan) if plan.status == ApplyStatus::Deferred)
            && (patch.changed_fields.as_slice() == ["proxy.system_proxy"]
                || patch.changed_fields.as_slice() == ["frontends.http"] && !stored.frontends.http)
            && observed_phase.is_none_or(|phase| {
                matches!(
                    phase,
                    ConnectionPhase::Connected | ConnectionPhase::Degraded
                )
            })
            && runtime.as_ref().is_none_or(|(_, _, health)| {
                matches!(health, usque_transport::RuntimeHealth::Connected { .. })
            }) {
            confirmed_session
                .filter(|(id, generation)| {
                    *id == patch.account_id
                        && runtime
                            .as_ref()
                            .is_none_or(|(_, current, _)| current == generation)
                        && self.settings_intent.load(Ordering::SeqCst) == intent
                })
                .map(|(_, generation)| SystemProxyApplication {
                    operation_id: patch.operation_id,
                    account_id: patch.account_id,
                    generation,
                    intent,
                    enabled: stored.proxy.system_proxy,
                })
        } else {
            None
        };
        let queued = followup.is_some();
        let start_followup = {
            let mut applications = self.system_proxy_applications.lock().await;
            if let Some(followup) = followup {
                applications.pending = Some(followup);
                !std::mem::replace(&mut applications.worker_running, true)
            } else {
                if patch
                    .changed_fields
                    .iter()
                    .any(|field| field == "proxy.system_proxy")
                {
                    applications.pending = None;
                } else if patch
                    .changed_fields
                    .iter()
                    .any(|field| field == "frontends.http")
                    && !stored.frontends.http
                    && let Some(pending) = &mut applications.pending
                {
                    // HTTP shutdown normalizes the latest proxy intent to off.
                    // Retire its lease without pulling deferred HTTP/listener
                    // changes into the existing session.
                    pending.enabled = false;
                    pending.operation_id = patch.operation_id;
                }
                false
            }
        };
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
        state.apply_status = if queued {
            ApplyStatus::Applying
        } else {
            plan.status
        };
        state.advance();
        let response = to_proto(&state);
        self.settings_tx.send_replace(state.sequence);
        drop(state);
        if start_followup {
            let service = self.clone();
            tokio::spawn(async move {
                service
                    .run_system_proxy_applications(SYSTEM_PROXY_EXECUTOR_TIMEOUT)
                    .await;
            });
        }
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
                            if !service.protected_chain_present().await {
                                let _ = service.disconnect_locked().await;
                            }
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
                                let recovery_target = if target.vpn_gate == previous.vpn_gate
                                    && target.chain_exit == previous.chain_exit
                                {
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

    async fn run_system_proxy_applications(&self, timeout: std::time::Duration) {
        let Ok(_lifecycle) = tokio::time::timeout(timeout, self.mutation_lock.lock()).await else {
            let pending = {
                let mut applications = self.system_proxy_applications.lock().await;
                applications.worker_running = false;
                applications.pending.take()
            };
            if let Some(pending) = pending {
                self.finish_system_proxy_application_error(
                    pending.operation_id,
                    "NETWORK_SETTINGS_SYSTEM_PROXY_BUSY",
                    ApplyStatus::Deferred,
                    false,
                )
                .await;
            }
            return;
        };
        loop {
            let pending = {
                let mut applications = self.system_proxy_applications.lock().await;
                let Some(pending) = applications.pending.take() else {
                    applications.worker_running = false;
                    return;
                };
                pending
            };
            let target = self.system_proxy_application_target(&pending).await;
            let Some(target) = target else {
                self.finish_system_proxy_application_error(
                    pending.operation_id,
                    "NETWORK_SETTINGS_CANCELLED",
                    ApplyStatus::Deferred,
                    false,
                )
                .await;
                continue;
            };
            self.settings_applying.store(true, Ordering::SeqCst);
            let result = self
                .execute_settings_plan(
                    &target,
                    &target,
                    ReconfigureClass::HotSystemProxy,
                    pending.intent,
                )
                .await;
            self.settings_applying.store(false, Ordering::SeqCst);
            if result.is_err() {
                let newest = {
                    let mut applications = self.system_proxy_applications.lock().await;
                    applications.worker_running = false;
                    applications.pending.take().unwrap_or(pending)
                };
                self.finish_system_proxy_application_error(
                    newest.operation_id,
                    "NETWORK_SETTINGS_APPLY_FAILED",
                    ApplyStatus::Failed,
                    true,
                )
                .await;
                return;
            }
            if self.settings_intent.load(Ordering::SeqCst) != pending.intent {
                if !self.protected_chain_present().await {
                    let _ = self.disconnect_locked().await;
                }
                continue;
            }
            let confirmed = {
                let mut active = self.data_plane.lock().await;
                active.as_mut().is_some_and(|active| {
                    if active.session_generation != pending.generation
                        || active.profile_id != pending.account_id
                        || !matches!(
                            active.runtime.health(),
                            usque_transport::RuntimeHealth::Connected { .. }
                        )
                    {
                        return false;
                    }
                    active.profile = target.clone();
                    true
                })
            };
            if confirmed {
                *self.session_profile.lock().await = Some(target.clone());
                self.publish_settings_runtime(Some(target), Some(pending.generation))
                    .await;
            } else {
                self.finish_system_proxy_application_error(
                    pending.operation_id,
                    "NETWORK_SETTINGS_RUNTIME_PENDING",
                    ApplyStatus::Failed,
                    true,
                )
                .await;
            }
        }
    }

    async fn system_proxy_application_target(
        &self,
        pending: &SystemProxyApplication,
    ) -> Option<Profile> {
        if self.settings_intent.load(Ordering::SeqCst) != pending.intent {
            return None;
        }
        {
            let config = self.config.read().await;
            if config.active_profile_id != Some(pending.account_id)
                || config.network.proxy.system_proxy != pending.enabled
            {
                return None;
            }
        }
        {
            let state = self.settings.lock().await;
            if state.persisted != Some(true)
                || state.applied_profile.as_ref()?.id != pending.account_id
                || state.session_id.as_ref()?.parse::<u64>().ok()? != pending.generation
            {
                return None;
            }
        }
        if !matches!(
            self.state.lock().await.snapshot().phase,
            ConnectionPhase::Connected | ConnectionPhase::Degraded
        ) {
            return None;
        }
        let mut target = {
            let active = self.data_plane.lock().await;
            let active = active.as_ref()?;
            if active.session_generation != pending.generation
                || active.profile_id != pending.account_id
                || !matches!(
                    active.runtime.health(),
                    usque_transport::RuntimeHealth::Connected { .. }
                )
            {
                return None;
            }
            active.profile.clone()
        };
        target.proxy.system_proxy = pending.enabled;
        target.validate().ok()?;
        if self.settings_intent.load(Ordering::SeqCst) != pending.intent {
            return None;
        }
        Some(target)
    }

    async fn finish_system_proxy_application_error(
        &self,
        operation_id: uuid::Uuid,
        code: &str,
        status: ApplyStatus,
        clear_confirmation: bool,
    ) {
        let mut state = self.settings.lock().await;
        // A timed-out or cancelled worker cannot overwrite a newer save.
        if state.operation_id != Some(operation_id) {
            return;
        }
        state.apply_status = status;
        state.error_code = Some(code.into());
        if clear_confirmation {
            state.applied_profile = None;
        }
        state.advance();
        self.settings_tx.send_replace(state.sequence);
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
                #[cfg(windows)]
                {
                    let cancellation = self.gate_startup_cancel.lock().await.clone();
                    if Box::pin(self.reconnect_protected_chain(target, &cancellation))
                        .await?
                        .is_some()
                    {
                        return Ok(());
                    }
                }
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
                        && (previous.vpn_gate == target.vpn_gate
                            && previous.chain_exit == target.chain_exit)
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
    async fn http_shutdown_normalizes_its_old_wire_system_proxy_flag_before_validation() {
        let (_directory, service) = service();
        let mut profile = service.config_snapshot().await.active_profile().unwrap();
        profile.frontends.http = false;
        profile.proxy.system_proxy = true;
        let response = service
            .save_network_settings(request(&profile, &["frontends.http"]))
            .await
            .unwrap();
        let stored = response.stored_profile.unwrap();
        assert!(!stored.frontends.unwrap().http);
        assert!(!stored.proxy.unwrap().system_proxy);

        let rejected = service
            .save_network_settings(request(&profile, &["proxy.system_proxy"]))
            .await;
        assert!(matches!(
            rejected,
            Err(ControlServiceError::InvalidConfiguration(_))
        ));
        assert!(!service.store.load().unwrap().network.proxy.system_proxy);
    }

    #[cfg(windows)]
    async fn proxy_session(service: &ControlService) -> Profile {
        let mut profile = service.config_snapshot().await.active_profile().unwrap();
        profile.proxy.system_proxy = true;
        service
            .save_network_settings(request(&profile, &["proxy.system_proxy"]))
            .await
            .unwrap();
        let profile = service.config_snapshot().await.active_profile().unwrap();
        service
            .install_test_session(profile.clone(), true, 3)
            .await
            .unwrap();
        profile
    }

    #[cfg(windows)]
    async fn wait_proxy_applications(service: &ControlService) {
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            while service
                .system_proxy_applications
                .lock()
                .await
                .worker_running
            {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn busy_system_proxy_switches_coalesce_without_applying_other_saved_fields() {
        let (_directory, service) = service();
        let mut profile = proxy_session(&service).await;
        let generation = service
            .data_plane
            .lock()
            .await
            .as_ref()
            .unwrap()
            .session_generation;
        let lifecycle = service.mutation_lock.lock().await;
        profile.mtu = 1400;
        let deferred = service
            .save_network_settings(request(&profile, &["mtu"]))
            .await
            .unwrap();
        assert_eq!(deferred.apply_status, 4);
        let mut last_operation = String::new();
        for enabled in [false, true, false, true, false] {
            profile.proxy.system_proxy = enabled;
            let response = service
                .save_network_settings(request(&profile, &["proxy.system_proxy"]))
                .await
                .unwrap();
            assert_eq!(response.persisted, Some(true));
            assert_eq!(response.apply_status, 2);
            last_operation = response.operation_id;
            assert!(
                service
                    .system_proxy_applications
                    .lock()
                    .await
                    .worker_running
            );
        }
        drop(lifecycle);
        wait_proxy_applications(&service).await;
        let state = service.network_settings_state().await;
        assert_eq!(state.operation_id, last_operation);
        assert_eq!(state.apply_status, 4);
        assert_eq!(state.deferred_fields, ["mtu"]);
        let applied = state.applied_profile.unwrap();
        assert!(!applied.proxy.unwrap().system_proxy);
        assert_eq!(applied.mtu, u32::from(Profile::default().mtu));
        let active = service.data_plane.lock().await;
        let active = active.as_ref().unwrap();
        assert_eq!(active.session_generation, generation);
        let crate::active_runtime::ActiveRuntime::Harness(harness) = &active.runtime else {
            panic!("harness")
        };
        assert_eq!(harness.system_proxy_apply_count, 1);
        assert_eq!(harness.reconfigure_count, 0);
        assert_eq!(harness.reconnect_count, 3);
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn system_proxy_off_follows_an_application_holding_the_runtime_lock() {
        let (_directory, service) = service();
        let profile = proxy_session(&service).await;
        let lifecycle = service.mutation_lock.lock().await;
        let runtime = service.data_plane.lock().await;
        let mut off = profile;
        off.proxy.system_proxy = false;
        let response = service
            .save_network_settings(request(&off, &["proxy.system_proxy"]))
            .await
            .unwrap();
        assert_eq!(response.apply_status, 2);
        drop(runtime);
        drop(lifecycle);
        wait_proxy_applications(&service).await;
        let state = service.network_settings_state().await;
        assert_eq!(state.apply_status, 3);
        assert!(!state.applied_profile.unwrap().proxy.unwrap().system_proxy);
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn system_proxy_off_rechecks_connection_phase_after_a_busy_state_read() {
        let (_directory, service) = service();
        let mut profile = proxy_session(&service).await;
        let lifecycle = service.mutation_lock.lock().await;
        let phase = service.state.lock().await;
        profile.proxy.system_proxy = false;
        let response = service
            .save_network_settings(request(&profile, &["proxy.system_proxy"]))
            .await
            .unwrap();
        assert_eq!(response.apply_status, 2);
        drop(phase);
        drop(lifecycle);
        wait_proxy_applications(&service).await;
        let state = service.network_settings_state().await;
        assert_eq!(state.operation_id, response.operation_id);
        assert_eq!(state.apply_status, 3);
        assert!(!state.applied_profile.unwrap().proxy.unwrap().system_proxy);
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn pending_proxy_enable_cannot_survive_a_saved_http_shutdown() {
        let (_directory, service) = service();
        let mut profile = proxy_session(&service).await;
        let lifecycle = service.mutation_lock.lock().await;
        service
            .save_network_settings(request(&profile, &["proxy.system_proxy"]))
            .await
            .unwrap();
        profile.frontends.http = false;
        let response = service
            .save_network_settings(request(&profile, &["frontends.http"]))
            .await
            .unwrap();
        assert!(!response.stored_profile.unwrap().proxy.unwrap().system_proxy);
        drop(lifecycle);
        wait_proxy_applications(&service).await;
        let state = service.network_settings_state().await;
        assert_eq!(state.operation_id, response.operation_id);
        assert_eq!(state.apply_status, 4);
        let applied = state.applied_profile.unwrap();
        assert!(applied.frontends.unwrap().http);
        assert!(!applied.proxy.unwrap().system_proxy);
        assert_eq!(state.deferred_fields, ["frontends.http"]);
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn http_shutdown_follows_a_proxy_application_that_already_took_its_pending_intent() {
        let (_directory, service) = service();
        let mut profile = proxy_session(&service).await;
        let lifecycle = service.mutation_lock.lock().await;
        let runtime = service.data_plane.lock().await;
        service
            .save_network_settings(request(&profile, &["proxy.system_proxy"]))
            .await
            .unwrap();
        drop(lifecycle);
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            loop {
                let applications = service.system_proxy_applications.lock().await;
                if applications.worker_running && applications.pending.is_none() {
                    break;
                }
                drop(applications);
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        profile.frontends.http = false;
        let response = service
            .save_network_settings(request(&profile, &["frontends.http"]))
            .await
            .unwrap();
        assert_eq!(response.apply_status, 2);
        drop(runtime);
        wait_proxy_applications(&service).await;
        let state = service.network_settings_state().await;
        assert_eq!(state.operation_id, response.operation_id);
        assert_eq!(state.apply_status, 4);
        let applied = state.applied_profile.unwrap();
        assert!(applied.frontends.unwrap().http);
        assert!(!applied.proxy.unwrap().system_proxy);
        assert_eq!(state.deferred_fields, ["frontends.http"]);
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn pending_system_proxy_application_rejects_retired_or_unconfirmed_sessions() {
        for change in [
            "intent",
            "session",
            "account",
            "phase",
            "confirmation",
            "health",
        ] {
            let (_directory, service) = service();
            let mut profile = proxy_session(&service).await;
            let lifecycle = service.mutation_lock.lock().await;
            profile.proxy.system_proxy = false;
            service
                .save_network_settings(request(&profile, &["proxy.system_proxy"]))
                .await
                .unwrap();
            match change {
                "intent" => {
                    service.settings_intent.fetch_add(1, Ordering::SeqCst);
                }
                "session" => {
                    service
                        .data_plane
                        .lock()
                        .await
                        .as_mut()
                        .unwrap()
                        .session_generation += 1;
                }
                "account" => {
                    service.config.write().await.active_profile_id = Some(uuid::Uuid::new_v4());
                }
                "phase" => {
                    service
                        .state
                        .lock()
                        .await
                        .transition(ConnectionPhase::Disconnecting)
                        .unwrap();
                    service
                        .state
                        .lock()
                        .await
                        .transition(ConnectionPhase::Disconnected)
                        .unwrap();
                }
                "confirmation" => {
                    service.settings.lock().await.applied_profile = None;
                }
                "health" => {
                    let mut active = service.data_plane.lock().await;
                    let crate::active_runtime::ActiveRuntime::Harness(harness) =
                        &mut active.as_mut().unwrap().runtime
                    else {
                        panic!("harness")
                    };
                    harness.gate_status.failure = Some(usque_core::vpngate::GateFailure::Transport);
                }
                _ => unreachable!(),
            }
            drop(lifecycle);
            wait_proxy_applications(&service).await;
            assert_ne!(
                service.network_settings_state().await.apply_status,
                3,
                "{change}"
            );
            let active = service.data_plane.lock().await;
            let crate::active_runtime::ActiveRuntime::Harness(harness) =
                &active.as_ref().unwrap().runtime
            else {
                panic!("harness")
            };
            assert_eq!(harness.system_proxy_apply_count, 0, "{change}");
        }
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn system_proxy_executor_timeout_is_bounded_and_cannot_overwrite_a_new_save() {
        let (_directory, service) = service();
        let mut profile = proxy_session(&service).await;
        let lifecycle = service.mutation_lock.lock().await;
        profile.proxy.system_proxy = false;
        let response = service
            .save_network_settings(request(&profile, &["proxy.system_proxy"]))
            .await
            .unwrap();
        tokio::time::pause();
        tokio::task::yield_now().await;
        tokio::time::advance(SYSTEM_PROXY_EXECUTOR_TIMEOUT + std::time::Duration::from_secs(1))
            .await;
        wait_proxy_applications(&service).await;
        let state = service.network_settings_state().await;
        assert_eq!(state.operation_id, response.operation_id);
        assert_eq!(state.apply_status, 4);
        assert_eq!(state.error_code, "NETWORK_SETTINGS_SYSTEM_PROXY_BUSY");
        assert!(state.applied_profile.unwrap().proxy.unwrap().system_proxy);
        drop(lifecycle);
        service
            .finish_system_proxy_application_error(
                uuid::Uuid::new_v4(),
                "OLD_WORKER",
                ApplyStatus::Failed,
                true,
            )
            .await;
        assert_eq!(
            service.network_settings_state().await.error_code,
            "NETWORK_SETTINGS_SYSTEM_PROXY_BUSY"
        );
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn failed_system_proxy_followup_never_confirms_the_saved_switch() {
        for failures in [1, 2] {
            let (_directory, service) = service();
            let mut profile = proxy_session(&service).await;
            let lifecycle = service.mutation_lock.lock().await;
            {
                let mut active = service.data_plane.lock().await;
                let crate::active_runtime::ActiveRuntime::Harness(harness) =
                    &mut active.as_mut().unwrap().runtime
                else {
                    panic!("harness")
                };
                harness.system_proxy_failures = failures;
            }
            profile.proxy.system_proxy = false;
            let response = service
                .save_network_settings(request(&profile, &["proxy.system_proxy"]))
                .await
                .unwrap();
            drop(lifecycle);
            wait_proxy_applications(&service).await;
            let state = service.network_settings_state().await;
            assert_eq!(state.operation_id, response.operation_id);
            assert_eq!(state.persisted, Some(true));
            assert_eq!(state.apply_status, 5);
            assert!(state.applied_profile.is_none());
            assert!(!state.stored_profile.unwrap().proxy.unwrap().system_proxy);
            let active = service.data_plane.lock().await;
            if failures == 1 {
                let active = active.as_ref().unwrap();
                assert!(active.profile.proxy.system_proxy);
                assert!(
                    active
                        .runtime
                        .frontend_statuses(active.frontends)
                        .iter()
                        .any(|status| {
                            status.kind == usque_core::FrontendKind::SystemProxy
                                && status.phase == usque_core::FrontendPhase::Active
                        })
                );
            } else {
                assert!(active.is_none());
                assert_eq!(
                    service.state.lock().await.snapshot().phase,
                    ConnectionPhase::Error
                );
            }
        }
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn configuration_selection_and_library_deletion_share_one_transaction() {
        use usque_core::chain_exit::{
            ChainSource, ImportSecrets,
            store::{ChainProfileStore, WindowsProfileCipher},
        };
        for select_first in [false, true] {
            let (_directory, service) = service();
            let summary = ChainProfileStore::new(&service.cache_dir, &WindowsProfileCipher).import(
                ChainSource::OpenvpnCustom, "Race fixture", ImportSecrets::new("client\ndev tun\nproto tcp\nremote vpn.example 1194\nauth-user-pass\n<ca>\nTEST\n</ca>\n".into())
            ).unwrap();
            let mut profile = service.config_snapshot().await.active_profile().unwrap();
            let mut selection = summary.selection();
            selection.enabled = false;
            profile.chain_exit = Some(selection);
            let save = request(&profile, &["chain_exit"]);
            let remove = v1::ChainProfileRequest {
                action: "remove".into(),
                profile_id: summary.id.to_string(),
                revision: summary.edit_revision.to_string(),
                ..Default::default()
            };
            let transaction = service.store.lock_exclusive().unwrap();
            let first = service.clone();
            let saving;
            let deleting;
            if select_first {
                saving = tokio::spawn(async move { first.save_network_settings(save).await });
                tokio::time::timeout(std::time::Duration::from_secs(2), async {
                    while service.config.try_read().is_ok() {
                        tokio::task::yield_now().await;
                    }
                })
                .await
                .unwrap();
                let second = service.clone();
                deleting = tokio::spawn(async move { second.chain_profile_command(remove).await });
            } else {
                deleting = tokio::spawn(async move { first.chain_profile_command(remove).await });
                tokio::time::timeout(std::time::Duration::from_secs(2), async {
                    while service.config.try_read().is_ok() {
                        tokio::task::yield_now().await;
                    }
                })
                .await
                .unwrap();
                let second = service.clone();
                saving = tokio::spawn(async move { second.save_network_settings(save).await });
            }
            drop(transaction);
            let (saved, removed) = tokio::time::timeout(std::time::Duration::from_secs(3), async {
                tokio::join!(saving, deleting)
            })
            .await
            .unwrap();
            let saved = saved.unwrap();
            let removed: serde_json::Value =
                serde_json::from_str(&removed.unwrap().unwrap().metadata_json).unwrap();
            let library = ChainProfileStore::new(&service.cache_dir, &WindowsProfileCipher)
                .list()
                .unwrap();
            if select_first {
                assert_eq!(saved.unwrap().persisted, Some(true));
                assert_eq!(removed["error"]["reason"], "profile_in_use");
                assert_eq!(library.len(), 1);
            } else {
                assert!(saved.is_err());
                assert!(removed["error"].is_null());
                assert!(library.is_empty());
            }
            let latest = service.store.load().unwrap();
            if let Some(id) = latest
                .network
                .chain_exit
                .as_ref()
                .and_then(|value| value.profile_id)
            {
                assert!(library.iter().any(|item| item.id == id));
            }
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
