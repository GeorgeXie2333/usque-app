//! Local scene transactions. A durable selection is not a runtime receipt.
use std::sync::{Arc, atomic::Ordering};

use serde_json::{Value, json};
use usque_core::network_settings::ApplyStatus;
use usque_core::scenarios::{self, ScenarioCommand, ScenarioContext};
use usque_core::{ConnectionPhase, Profile, ReconfigureClass, storage::StoreError};
use usque_ipc::v1;

use crate::{ControlService, ControlServiceError};

/// Clears the scene/settings application flags even if the owned worker
/// unwinds, so later settings saves are not refused indefinitely.
struct ApplyingFlags(ControlService);

impl ApplyingFlags {
    fn set(service: &ControlService) -> Self {
        service.scenario_applying.store(true, Ordering::SeqCst);
        service.settings_applying.store(true, Ordering::SeqCst);
        Self(service.clone())
    }
}

impl Drop for ApplyingFlags {
    fn drop(&mut self) {
        self.0.settings_applying.store(false, Ordering::SeqCst);
        self.0.scenario_applying.store(false, Ordering::SeqCst);
    }
}

impl ControlService {
    async fn scenario_context(&self) -> ScenarioContext {
        let phase = self.state.lock().await.snapshot().phase;
        let state = self.settings.lock().await;
        let connected = matches!(
            phase,
            ConnectionPhase::Connected | ConnectionPhase::Degraded
        );
        ScenarioContext {
            source_epoch: state.source_epoch,
            session_id: state.session_id.clone(),
            applied_profile: state.applied_profile.clone().filter(|_| connected),
            apply_status: state.apply_status,
            dependency_fingerprint: String::new(),
        }
    }

    fn validate_scenario_resources_at(
        parent: &std::path::Path,
        cache: &std::path::Path,
        cipher: Option<&dyn usque_core::chain_exit::store::ProfileCipher>,
        profile: &Profile,
    ) -> Result<String, ControlServiceError> {
        Self::validate_chain_selection_at(parent, profile, cipher).map_err(|_| {
            ControlServiceError::ScenarioUnavailable("SCENARIO_DEPENDENCY_UNAVAILABLE")
        })?;
        let policy = crate::load_geo_direct_policy(profile, cache)
            .map_err(|_| ControlServiceError::ScenarioUnavailable("SCENARIO_RULES_UNAVAILABLE"))?;
        if profile.routing.ads_enabled && policy.ads_revision().is_none() {
            return Err(ControlServiceError::ScenarioUnavailable(
                "SCENARIO_RULES_UNAVAILABLE",
            ));
        }
        let mut revisions = Vec::new();
        if let Some(selection) = profile.custom_chain() {
            if selection.profile_id.is_some() {
                let (summary, _, _) = usque_core::chain_exit::store::ChainProfileStore::new(
                    parent,
                    cipher.ok_or(ControlServiceError::ScenarioUnavailable(
                        "SCENARIO_DEPENDENCY_UNAVAILABLE",
                    ))?,
                )
                .load(selection)
                .map_err(|_| {
                    ControlServiceError::ScenarioUnavailable("SCENARIO_DEPENDENCY_UNAVAILABLE")
                })?;
                revisions.push(format!("{}:{}", summary.revision, summary.edit_revision));
            }
        } else if profile.chain_enabled() {
            let selection = profile.vpn_gate.selection.as_ref().ok_or(
                ControlServiceError::ScenarioUnavailable("SCENARIO_DEPENDENCY_UNAVAILABLE"),
            )?;
            usque_core::vpngate::CatalogueStore::new(cache)
                .load_selection(selection)
                .map_err(|_| {
                    ControlServiceError::ScenarioUnavailable("SCENARIO_DEPENDENCY_UNAVAILABLE")
                })?;
            revisions.push(selection.config_sha256.clone());
        }
        for code in &profile.geo_direct_countries {
            let country =
                usque_geo::CountryCode::parse(code).map_err(ControlServiceError::configuration)?;
            revisions.push(file_revision(&usque_geo::geoip_cache_path(cache, &country)));
            revisions.push(file_revision(&usque_geo::geosite_cache_path(
                cache, &country,
            )));
        }
        if profile.routing.ads_enabled {
            revisions.push(policy.ads_revision().unwrap_or_default().to_owned());
        }
        if profile.routing.ads_enabled || !profile.geo_direct_countries.is_empty() {
            revisions.push(file_revision(&usque_geo::global_geosite_cache_path(cache)));
        }
        serde_json::to_string(&revisions).map_err(ControlServiceError::configuration)
    }

    async fn validate_scenario_resources(
        &self,
        profile: &Profile,
    ) -> Result<String, ControlServiceError> {
        let parent = self.profile_dir.clone();
        let cache = self.cache_dir.clone();
        let cipher = self.chain_cipher.clone();
        let profile = profile.clone();
        // Library reads and decryption are blocking file work.
        tokio::task::spawn_blocking(move || {
            Self::validate_scenario_resources_at(&parent, &cache, cipher.as_deref(), &profile)
        })
        .await
        .map_err(|_| ControlServiceError::ScenarioUnavailable("SCENARIO_DEPENDENCY_UNAVAILABLE"))?
    }

    /// Publish a configuration observed after an uncertain commit.
    async fn observe_scenario_config(&self, config: &usque_core::AppConfig) {
        let mut state = self.settings.lock().await;
        state.stored_profile = config.active_profile();
        state.shared_network_profile = Some(
            config
                .network
                .hydrate(&usque_core::config::Account::default_account()),
        );
        state.advance();
        self.settings_tx.send_replace(state.sequence);
    }

    /// Settle an uncertain commit from durable storage before later scene
    /// commands. A recorded target is acknowledged only by a successful durable
    /// write; a different record proves that the operation never committed.
    /// The lost runtime application is never replayed.
    async fn resolve_unconfirmed_scenario(&self) {
        let Some(operation) = *self.scenario_unconfirmed_operation.lock().await else {
            return;
        };
        let store = self.store.clone();
        let Ok(Ok((observed, _))) = tokio::task::spawn_blocking(move || {
            store.update::<_, StoreError>(|latest| {
                Ok(scenarios::acknowledge_operation(latest, operation))
            })
        })
        .await
        else {
            return;
        };
        *self.config.write().await = observed.clone();
        *self.scenario_unconfirmed_operation.lock().await = None;
        self.observe_scenario_config(&observed).await;
    }

    pub(crate) async fn scenario_command(
        &self,
        request: v1::ScenarioRequest,
    ) -> Result<v1::ScenarioResponse, ControlServiceError> {
        let command = ScenarioCommand::parse(&request.command_json)?;
        let applying = matches!(
            command,
            ScenarioCommand::Activate { .. } | ScenarioCommand::Restore { .. }
        );
        let operation_id = match &command {
            ScenarioCommand::Activate { operation_id, .. }
            | ScenarioCommand::Restore { operation_id, .. } => Some(*operation_id),
            _ => None,
        };
        let _submission = self.settings_submission.lock().await;
        self.resolve_unconfirmed_scenario().await;
        // Queries and template edits remain available during connection
        // lifecycle work. A saved VPN Gate pin is serialized with catalogue
        // cleanup, which runs under the lifecycle lock.
        let pins_gate = matches!(
            &command,
            ScenarioCommand::Save { settings, .. } if settings.vpn_gate.selection.is_some()
        );
        let lifecycle = if applying || pins_gate {
            Some(
                Arc::clone(&self.mutation_lock)
                    .try_lock_owned()
                    .map_err(|_| ControlServiceError::ScenarioUnavailable("SCENARIO_BUSY"))?,
            )
        } else {
            None
        };
        let mut context = self.scenario_context().await;
        let mut candidate = self.config.read().await.clone();
        if let Some(target) = scenarios::candidate_profile(&candidate, &command)? {
            context.dependency_fingerprint = self.validate_scenario_resources(&target).await?;
            if let Some(previous) = &context.applied_profile {
                let runtime_target = scenarios::runtime_target(&target, previous);
                let cold = previous.id != runtime_target.id
                    || usque_core::reconfigure::platform_class(usque_core::classify_reconfigure(
                        previous,
                        &runtime_target,
                    )) == ReconfigureClass::ColdReconnect;
                if cold
                    && previous.frontends.tunnel
                    && previous.kill_switch
                    && target.frontends.tunnel
                    && !self.protected_chain_present().await
                {
                    return Err(ControlServiceError::ScenarioUnavailable(
                        "SCENARIO_PROTECTION_UNAVAILABLE",
                    ));
                }
            }
            // Reading the existing vault does not register or renew an account.
            self.load_warp_identity(target.id).await?;
        }
        let preview = scenarios::command(&mut candidate, command.clone(), &context)?;
        if !command.is_mutation() {
            let mut response = preview.response;
            self.scenario_receipt(&mut response).await;
            return scenario_response(response);
        }
        let provider = if applying {
            self.cancel_warp_jobs().await?;
            self.cancel_gate_refresh().await;
            Some(
                usque_core::proton_vpn::lifecycle_lease(&self.profile_dir)
                    .map_err(ControlServiceError::configuration)?,
            )
        } else {
            None
        };
        // Public confirmation snapshots strip listener passwords; private
        // execution and rollback retain the actual owner's authentication.
        let previous = self
            .data_plane
            .lock()
            .await
            .as_ref()
            .map(|active| active.profile.clone());
        let phase = self.state.lock().await.snapshot().phase;
        // A changing/unknown owner must finish before another explicit switch.
        if applying
            && !matches!(
                phase,
                ConnectionPhase::Disconnected
                    | ConnectionPhase::Connected
                    | ConnectionPhase::Degraded
                    | ConnectionPhase::Error
            )
        {
            return Err(ControlServiceError::ScenarioUnavailable("SCENARIO_BUSY"));
        }
        let running = self.data_plane.lock().await.is_some();
        if applying && running && context.applied_profile.is_none() {
            return Err(ControlServiceError::ScenarioUnavailable(
                "SCENARIO_RUNTIME_UNCONFIRMED",
            ));
        }
        let mut config = self.config.write().await;
        let store = self.store.clone();
        let parent = self.profile_dir.clone();
        let cache = self.cache_dir.clone();
        let cipher = self.chain_cipher.clone();
        let commit_context = context.clone();
        let commit = tokio::task::spawn_blocking(move || {
            store.update::<_, ControlServiceError>(|latest| {
                if let ScenarioCommand::Save { settings, .. } = &command
                    && let Some(selection) = &settings.vpn_gate.selection
                {
                    usque_core::vpngate::CatalogueStore::new(&cache)
                        .pin(selection)
                        .map_err(|_| {
                            ControlServiceError::ScenarioUnavailable(
                                "SCENARIO_DEPENDENCY_UNAVAILABLE",
                            )
                        })?;
                }
                let result = scenarios::command(latest, command, &commit_context)?;
                if let Some(target) = &result.target {
                    let revision = Self::validate_scenario_resources_at(
                        &parent,
                        &cache,
                        cipher.as_deref(),
                        target,
                    )?;
                    if revision != commit_context.dependency_fingerprint {
                        return Err(usque_core::scenarios::ScenarioError::PreviewStale.into());
                    }
                }
                Ok(result)
            })
        })
        .await
        // A worker that unwound may have stopped after the durable write.
        .unwrap_or(Err(ControlServiceError::Persistence(
            StoreError::CommitUncertain(std::io::Error::other("scene worker stopped")),
        )));
        let (next, mut result) = match commit {
            Ok(value) => value,
            Err(ControlServiceError::Persistence(StoreError::CommitUncertain(_))) => {
                // A template save cannot retire an older unresolved activation.
                if operation_id.is_some() {
                    *self.scenario_unconfirmed_operation.lock().await = operation_id;
                }
                let store = self.store.clone();
                if let Ok(Ok(observed)) = tokio::task::spawn_blocking(move || store.load()).await {
                    *config = observed;
                }
                self.observe_scenario_config(&config).await;
                let mut state = self.settings.lock().await;
                state.apply_status = ApplyStatus::Unknown;
                state.operation_id = operation_id;
                state.persisted = None;
                state.error_code = Some("SCENARIO_COMMIT_UNCONFIRMED".into());
                state.advance();
                self.settings_tx.send_replace(state.sequence);
                return Err(ControlServiceError::ScenarioUnavailable(
                    "SCENARIO_COMMIT_UNCONFIRMED",
                ));
            }
            Err(error) => return Err(error),
        };
        *config = next;
        if let Some(operation) = operation_id {
            // Only written after the target transaction acknowledged durability.
            // A failed marker write cannot invalidate that first receipt; it
            // only means a later process may be unable to confirm it.
            let store = self.store.clone();
            if let Ok(Ok((acknowledged, _))) = tokio::task::spawn_blocking(move || {
                store.update::<_, StoreError>(|latest| {
                    Ok(scenarios::acknowledge_operation(latest, operation))
                })
            })
            .await
            {
                *config = acknowledged;
            }
        }
        let stored = config.active_profile();
        let shared = config
            .network
            .hydrate(&usque_core::config::Account::default_account());
        drop(config);
        result.response["persisted"] = json!(true);
        if let (Some(mut target), Some(operation_id), Some(lifecycle)) =
            (result.target, operation_id, lifecycle)
        {
            *self.scenario_unconfirmed_operation.lock().await = None;
            result.response["persisted_operation_id"] = json!(operation_id);
            let intent = self.settings_intent.fetch_add(1, Ordering::SeqCst) + 1;
            let mut state = self.settings.lock().await;
            state.operation_id = Some(operation_id);
            state.persisted = Some(true);
            state.record_persisted_operation(operation_id);
            state.stored_profile = stored;
            state.shared_network_profile = Some(shared);
            state.error_code = None;
            state.apply_status = if running {
                ApplyStatus::Applying
            } else {
                ApplyStatus::Deferred
            };
            state.advance();
            self.settings_tx.send_replace(state.sequence);
            result.response["state"]["apply_status"] = json!(state.apply_status);
            // The pre-commit comparison describes the old owner, never success.
            result.response["state"]["applied_matches"] = Value::Null;
            drop(state);
            if running {
                let applying = ApplyingFlags::set(self);
                let service = self.clone();
                tokio::spawn(async move {
                    let _lifecycle = lifecycle;
                    let _provider = provider;
                    if let Some(previous) = &previous {
                        target = scenarios::runtime_target(&target, previous);
                    }
                    let result = async {
                        service.attach_proxy_auth(&mut target).await?;
                        if service.settings_intent.load(Ordering::SeqCst) != intent {
                            return Err(ControlServiceError::ScenarioUnavailable(
                                "SCENARIO_CANCELLED",
                            ));
                        }
                        let previous =
                            previous
                                .as_ref()
                                .ok_or(ControlServiceError::ScenarioUnavailable(
                                    "SCENARIO_RUNTIME_UNCONFIRMED",
                                ))?;
                        let class = if previous.id != target.id {
                            ReconfigureClass::ColdReconnect
                        } else {
                            usque_core::reconfigure::platform_class(
                                usque_core::classify_reconfigure(previous, &target),
                            )
                        };
                        if class == ReconfigureClass::PersistOnly {
                            return Ok(class);
                        }
                        service
                            .execute_settings_plan(&target, previous, class, intent)
                            .await?;
                        Ok(class)
                    }
                    .await;
                    drop(applying);
                    if service.settings_intent.load(Ordering::SeqCst) != intent {
                        return;
                    }
                    match result {
                        Ok(class) => {
                            let confirmed =
                                service.data_plane.lock().await.as_mut().and_then(|active| {
                                    if active.profile.id != target.id
                                        || !matches!(
                                            active.runtime.health(),
                                            usque_transport::RuntimeHealth::Connected { .. }
                                        )
                                    {
                                        return None;
                                    }
                                    if class != ReconfigureClass::ColdReconnect {
                                        let actual_algorithm = active.profile.congestion_control;
                                        active.profile = target.clone();
                                        active.profile.congestion_control = actual_algorithm;
                                    }
                                    Some((active.profile.clone(), active.session_generation))
                                });
                            if let Some((profile, generation)) = confirmed {
                                *service.session_profile.lock().await = Some(profile.clone());
                                service
                                    .publish_settings_runtime(Some(profile), Some(generation))
                                    .await;
                            } else {
                                service
                                    .finish_settings_error(
                                        operation_id,
                                        "SCENARIO_RUNTIME_PENDING",
                                        ApplyStatus::Applying,
                                        true,
                                    )
                                    .await;
                            }
                        }
                        Err(_) => {
                            service
                                .finish_settings_error(
                                    operation_id,
                                    "SCENARIO_APPLY_FAILED",
                                    ApplyStatus::Failed,
                                    true,
                                )
                                .await
                        }
                    }
                });
            }
        }
        scenario_response(result.response)
    }

    async fn scenario_receipt(&self, response: &mut Value) {
        let record = self.config.read().await.last_scenario_switch.clone();
        let unconfirmed = *self.scenario_unconfirmed_operation.lock().await;
        // The GUI keeps its connection barrier only while the owner itself
        // cannot settle this operation from storage.
        response["unresolved_operation_id"] = json!(unconfirmed);
        if let Some(record) = record.filter(|record| Some(record.operation_id) != unconfirmed) {
            let acknowledged = record.acknowledged
                || self
                    .settings
                    .lock()
                    .await
                    .persisted_operation_ids
                    .contains(&record.operation_id);
            if acknowledged {
                response["persisted_operation_id"] = json!(record.operation_id);
            }
        }
    }
}

fn scenario_response(response: Value) -> Result<v1::ScenarioResponse, ControlServiceError> {
    Ok(v1::ScenarioResponse {
        metadata_json: serde_json::to_string(&response)
            .map_err(ControlServiceError::configuration)?,
    })
}

fn file_revision(path: &std::path::Path) -> String {
    std::fs::metadata(path)
        .ok()
        .map(|metadata| {
            let modified = metadata
                .modified()
                .ok()
                .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|duration| duration.as_nanos());
            format!("{}:{modified:?}", metadata.len())
        })
        .unwrap_or_else(|| "absent".into())
}

#[cfg(test)]
mod tests;
