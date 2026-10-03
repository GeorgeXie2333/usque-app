//! First-run provisioning has its own durable intent and never repairs or
//! replaces an identity. An IPC deadline does not release its execution lease.

use std::{fs::File, future::Future};

use usque_core::{InitialIdentityOperation, InitialIdentityPhase};

use crate::*;

impl ControlService {
    pub(crate) async fn identity_execution_lease(&self) -> Result<File, ControlServiceError> {
        let store = self.store.clone();
        tokio::task::spawn_blocking(move || store.initial_identity_lease(true))
            .await
            .map_err(|error| ControlServiceError::PersistenceWorker(error.to_string()))??
            .ok_or_else(|| {
                ControlServiceError::InvalidRequest(
                    "identity execution lease is unavailable".into(),
                )
            })
    }

    async fn try_identity_execution_lease(&self) -> Result<Option<File>, ControlServiceError> {
        let store = self.store.clone();
        tokio::task::spawn_blocking(move || store.initial_identity_lease(false))
            .await
            .map_err(|error| ControlServiceError::PersistenceWorker(error.to_string()))?
            .map_err(Into::into)
    }

    async fn stable_initial_identity(
        &self,
        config: &AppConfig,
        profile_id: Uuid,
    ) -> Result<bool, ControlServiceError> {
        if config
            .pending_identity_replacements
            .contains_key(&profile_id)
            || config.pending_identity_creations.contains(&profile_id)
            || config.pending_identity_deletions.contains(&profile_id)
            || config
                .pending_identity_local_deletions
                .contains(&profile_id)
        {
            return Ok(false);
        }
        let identity = match self.load_warp_identity(profile_id).await {
            Ok(identity) => identity,
            Err(
                ControlServiceError::MissingCredential(_)
                | ControlServiceError::InvalidStoredIdentity
                | ControlServiceError::Identity(_),
            ) => return Ok(false),
            Err(error) => return Err(error),
        };
        if config
            .identity_bindings
            .get(&profile_id)
            .is_some_and(|provider| provider != identity.provider())
        {
            return Ok(false);
        }
        Ok(
            !matches!(identity.provider(), IdentityProvider::ZeroTrust { .. })
                || config
                    .account(profile_id)
                    .is_some_and(|account| account.managed_endpoint_ips.is_some()),
        )
    }

    async fn has_initial_identity_material(
        &self,
        config: &AppConfig,
        profile_id: Uuid,
    ) -> Result<bool, ControlServiceError> {
        if config.identity_bindings.contains_key(&profile_id)
            || config
                .pending_identity_replacements
                .contains_key(&profile_id)
        {
            return Ok(true);
        }
        for record in SecretRecord::ALL {
            if record != SecretRecord::ProxyPassword
                && self.vault.get(profile_id, record).await?.is_some()
            {
                return Ok(true);
            }
        }
        Ok(false)
    }

    async fn initial_state(
        &self,
        profile_id: Uuid,
        executing: bool,
    ) -> Result<v1::InitialIdentityState, ControlServiceError> {
        let config = self.store.load_or_default()?;
        if config.account(profile_id).is_none() {
            return Err(ControlServiceError::ProfileNotFound(profile_id));
        }
        let operation = config
            .initial_identity_operation
            .as_ref()
            .filter(|operation| operation.profile_id == profile_id);
        let mut state = v1::InitialIdentityState {
            profile_id: profile_id.to_string(),
            operation_id: operation
                .map(|operation| operation.operation_id.to_string())
                .unwrap_or_default(),
            phase: v1::InitialIdentityPhase::Idle as i32,
            ..Default::default()
        };
        // A writer may have produced readable credentials before its durable
        // endpoint/binding commit. Never expose that transient state as Ready.
        if executing {
            state.phase = v1::InitialIdentityPhase::Pending as i32;
        } else if self.stable_initial_identity(&config, profile_id).await? {
            state.phase = v1::InitialIdentityPhase::Completed as i32;
            state.reused = true;
        } else if let Some(operation) = operation {
            state.phase = match operation.phase {
                InitialIdentityPhase::Pending | InitialIdentityPhase::Interrupted => {
                    v1::InitialIdentityPhase::Interrupted
                }
                InitialIdentityPhase::Completed | InitialIdentityPhase::Failed => {
                    v1::InitialIdentityPhase::Failed
                }
            } as i32;
            state.error_code = operation.error_code.clone().unwrap_or_else(|| {
                match operation.phase {
                    InitialIdentityPhase::Completed => "INITIAL_IDENTITY_REPAIR_REQUIRED",
                    _ => "INITIAL_IDENTITY_INTERRUPTED",
                }
                .to_owned()
            });
        } else if self
            .has_initial_identity_material(&config, profile_id)
            .await?
        {
            state.phase = v1::InitialIdentityPhase::Failed as i32;
            state.error_code = "INITIAL_IDENTITY_REPAIR_REQUIRED".into();
        }
        Ok(state)
    }

    pub(crate) async fn get_initial_identity_state(
        &self,
        profile_id: Uuid,
    ) -> Result<v1::InitialIdentityState, ControlServiceError> {
        let lease = self.try_identity_execution_lease().await?;
        self.initial_state(profile_id, lease.is_none()).await
    }

    pub(crate) async fn initial_identity(
        &self,
        request: v1::InitialIdentityRequest,
    ) -> Result<v1::InitialIdentityState, ControlServiceError> {
        self.initial_identity_with(request, |provisioning| {
            self.register_initial_identity(provisioning)
        })
        .await
    }

    async fn initial_identity_with<F, R>(
        &self,
        request: v1::InitialIdentityRequest,
        register: F,
    ) -> Result<v1::InitialIdentityState, ControlServiceError>
    where
        F: FnOnce(v1::IdentityProvisioning) -> R,
        R: Future<Output = Result<ProvisionedIdentity, ControlServiceError>>,
    {
        let profile_id = parse_profile_id(&request.profile_id)?;
        let operation_id = parse_profile_id(&request.operation_id)?;
        if operation_id.is_nil() {
            return Err(ControlServiceError::InvalidRequest(
                "initial operation ID is missing".into(),
            ));
        }
        let Some(_lease) = self.try_identity_execution_lease().await? else {
            return self.initial_state(profile_id, true).await;
        };
        let state = self.initial_state(profile_id, false).await?;
        let config = self.store.load_or_default()?;
        if state.phase == v1::InitialIdentityPhase::Completed as i32 {
            // Local secure writes can have completed before the final journal
            // acknowledgement was received. Finalize only this exact intent;
            // reusing a different Ready identity never changes its old journal.
            if config.active_profile_id == Some(profile_id)
                && config
                    .initial_identity_operation
                    .as_ref()
                    .is_some_and(|operation| {
                        operation.operation_id == operation_id
                            && operation.profile_id == profile_id
                            && operation.phase == InitialIdentityPhase::Pending
                    })
            {
                self.update_config(move |config| {
                    config
                        .finish_initial_identity(
                            operation_id,
                            profile_id,
                            InitialIdentityPhase::Completed,
                            None,
                        )
                        .map_err(ControlServiceError::configuration)?;
                    Ok(())
                })
                .await?;
            }
            return Ok(state);
        }
        if request.resume_only {
            // Desktop has no encrypted Android candidate. An interrupted or
            // partial result remains observable without reusing credentials or
            // starting another remote registration.
            return Ok(state);
        }
        if self
            .has_initial_identity_material(&config, profile_id)
            .await?
        {
            return Ok(v1::InitialIdentityState {
                phase: v1::InitialIdentityPhase::Failed as i32,
                error_code: "INITIAL_IDENTITY_REPAIR_REQUIRED".into(),
                ..state
            });
        }
        if config
            .initial_identity_operation
            .as_ref()
            .is_some_and(|operation| operation.operation_id == operation_id)
        {
            return Ok(state);
        }
        let provisioning = request.provisioning.ok_or_else(|| {
            ControlServiceError::InvalidRequest("initial provisioning is missing".into())
        })?;
        if !provisioning.terms_accepted {
            return Err(ControlServiceError::TermsNotAccepted);
        }
        let (method, organization) =
            match v1::IdentityProvisioningMethod::try_from(provisioning.method) {
                Ok(v1::IdentityProvisioningMethod::Register) => ("register", None),
                Ok(v1::IdentityProvisioningMethod::RegisterWithLicense) => {
                    ("registerWithLicense", None)
                }
                Ok(v1::IdentityProvisioningMethod::RegisterZeroTrust) => (
                    "zeroTrust",
                    Some(normalize_zero_trust_team(
                        &provisioning
                            .zero_trust
                            .as_ref()
                            .ok_or_else(|| {
                                ControlServiceError::InvalidRequest("enrollment is missing".into())
                            })?
                            .team_name,
                    )?),
                ),
                _ => {
                    return Err(ControlServiceError::InvalidRequest(
                        "initial provisioning method is invalid".into(),
                    ));
                }
            };
        let operation = InitialIdentityOperation::new(
            operation_id,
            profile_id,
            method,
            organization,
            Uuid::new_v4(),
        )
        .map_err(ControlServiceError::configuration)?;
        self.update_config(move |config| {
            if let Some(previous) = &mut config.initial_identity_operation
                && previous.phase == InitialIdentityPhase::Pending
            {
                // This caller owns the lease: the previous worker is gone.
                previous.phase = InitialIdentityPhase::Interrupted;
                previous.error_code = Some("INITIAL_IDENTITY_INTERRUPTED".into());
            }
            config
                .begin_initial_identity(operation)
                .map_err(ControlServiceError::configuration)?;
            Ok(())
        })
        .await?;
        let result = async {
            let provisioned = register(provisioning).await?;
            let _mutation = self.mutation_lock.lock().await;
            let mut write_attempted = false;
            let commit = async {
                let latest = self.store.load_or_default()?;
                if latest.active_profile_id != Some(profile_id)
                    || !latest
                        .initial_identity_operation
                        .as_ref()
                        .is_some_and(|operation| {
                            operation.operation_id == operation_id
                                && operation.profile_id == profile_id
                                && operation.phase == InitialIdentityPhase::Pending
                        })
                {
                    return Err(ControlServiceError::InvalidRequest(
                        "initial identity operation was superseded".into(),
                    ));
                }
                if self
                    .has_initial_identity_material(&latest, profile_id)
                    .await?
                {
                    return Err(ControlServiceError::InvalidRequest(
                        "initial identity cannot replace stored credentials".into(),
                    ));
                }
                // Reuse rollback-safe secure writes without giving this entry
                // point permission to replace an existing identity.
                *self.config.write().await = latest;
                write_attempted = true;
                self.replace_identity_records_locked(
                    profile_id,
                    provisioned.identity(),
                    provisioned.managed_endpoint_ips().cloned(),
                )
                .await?;
                self.update_config(move |config| {
                    config
                        .finish_initial_identity(
                            operation_id,
                            profile_id,
                            InitialIdentityPhase::Completed,
                            None,
                        )
                        .map_err(ControlServiceError::configuration)
                })
                .await
            }
            .await;
            if let Err(error) = commit {
                // A commit-uncertain result may already be readable and durable.
                // Preserve its entitlement; clean up only an uncommitted device.
                let committed_here = match self.store.load_or_default() {
                    Ok(config) => {
                        self.stable_initial_identity(&config, profile_id)
                            .await
                            .unwrap_or(false)
                            && self
                                .load_warp_identity(profile_id)
                                .await
                                .is_ok_and(|identity| {
                                    identity.device_id() == provisioned.identity().device_id()
                                        && identity.access_token()
                                            == provisioned.identity().access_token()
                                })
                    }
                    // A failed read after secure writes cannot prove the device
                    // is uncommitted. Do not revoke a possibly live entitlement.
                    Err(_) => write_attempted,
                };
                if !committed_here {
                    self.cleanup_uncommitted_initial_identity(provisioned.identity())
                        .await?;
                }
                return Err(Self::after_zero_trust_registration(
                    error,
                    provisioned.is_zero_trust(),
                ));
            }
            Ok(())
        }
        .await;
        if let Err(error) = result {
            // Keep the original failure if clearing/deletion already fenced the
            // operation. Never recreate its journal from a late result.
            let _ = self
                .update_config(move |config| {
                    config
                        .finish_initial_identity(
                            operation_id,
                            profile_id,
                            InitialIdentityPhase::Failed,
                            Some("REGISTRATION_FAILED".into()),
                        )
                        .map_err(ControlServiceError::configuration)
                })
                .await;
            return Err(error);
        }
        let mut state = self.initial_state(profile_id, false).await?;
        state.reused = false;
        Ok(state)
    }

    async fn cleanup_uncommitted_initial_identity(
        &self,
        identity: &WarpIdentity,
    ) -> Result<(), ControlServiceError> {
        if !should_unbind_remote_license(identity) {
            return Ok(());
        }
        let cleanup_id = Uuid::new_v4();
        if self
            .unbind_remote_consumer_license(cleanup_id, identity)
            .await
            .is_ok()
        {
            return Ok(());
        }
        // Retain a private cleanup identity and the existing durable deletion
        // marker when remote revocation is unavailable. It is never an account.
        self.update_config(move |config| {
            config.pending_identity_deletions.push(cleanup_id);
            Ok(())
        })
        .await?;
        self.persist_identity(cleanup_id, identity, None).await?;
        tracing::warn!(
            error_code = "INITIAL_IDENTITY_CLEANUP_PENDING",
            "uncommitted WARP license cleanup was queued"
        );
        Ok(())
    }

    pub(crate) async fn register_initial_identity(
        &self,
        provisioning: v1::IdentityProvisioning,
    ) -> Result<ProvisionedIdentity, ControlServiceError> {
        let secret = Zeroizing::new(provisioning.warp_secret);
        let license = Zeroizing::new(provisioning.license_key);
        if !secret.is_empty() {
            return Err(ControlServiceError::FeatureRemoved("WARP Secret import"));
        }
        let options = registration_options(provisioning.device_name, provisioning.locale);
        let client = ConsumerRegistrationClient::new()?;
        match v1::IdentityProvisioningMethod::try_from(provisioning.method) {
            Ok(v1::IdentityProvisioningMethod::Register) if license.is_empty() => Ok(
                ProvisionedIdentity::consumer(client.register(&options).await?),
            ),
            Ok(v1::IdentityProvisioningMethod::RegisterWithLicense) => {
                let license = std::str::from_utf8(&license)
                    .map_err(|_| ControlServiceError::InvalidLicenseEncoding)?;
                Ok(ProvisionedIdentity::consumer(
                    client.register_with_license(&options, license).await?,
                ))
            }
            Ok(v1::IdentityProvisioningMethod::RegisterZeroTrust) if license.is_empty() => {
                let enrollment = provisioning.zero_trust.ok_or_else(|| {
                    ControlServiceError::InvalidRequest("enrollment is missing".into())
                })?;
                let callback = Zeroizing::new(enrollment.callback_uri);
                let callback = std::str::from_utf8(&callback)
                    .map_err(|_| RegistrationError::InvalidZeroTrustCallback)?;
                let result = client
                    .register_zero_trust(&options, &enrollment.team_name, callback)
                    .await?;
                Ok(ProvisionedIdentity::zero_trust(
                    result.identity,
                    &result.endpoint,
                ))
            }
            Ok(v1::IdentityProvisioningMethod::ImportSecret) => {
                Err(ControlServiceError::FeatureRemoved("WARP Secret import"))
            }
            _ => Err(ControlServiceError::InvalidRequest(
                "initial provisioning method or credentials are invalid".into(),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::{MemoryVault, test_identity};

    fn request(profile_id: Uuid) -> v1::InitialIdentityRequest {
        v1::InitialIdentityRequest {
            operation_id: Uuid::new_v4().to_string(),
            profile_id: profile_id.to_string(),
            resume_only: false,
            provisioning: Some(v1::IdentityProvisioning {
                terms_accepted: true,
                method: v1::IdentityProvisioningMethod::Register as i32,
                ..Default::default()
            }),
        }
    }

    #[tokio::test]
    async fn resume_only_without_a_journal_never_reserves_or_registers() {
        let directory = tempfile::tempdir().unwrap();
        let store = ConfigStore::new(directory.path().join("config.json"));
        let service =
            ControlService::open_with_vault(store.clone(), Arc::new(MemoryVault::default()))
                .unwrap();
        let profile_id = service.config_snapshot().await.profiles[0].id;
        let before = std::fs::read(store.path()).unwrap();
        let mut resume = request(profile_id);
        resume.resume_only = true;
        resume.provisioning = None;
        let state = service
            .initial_identity_with(resume, |_| async {
                panic!("local recovery must never register remotely")
            })
            .await
            .unwrap();
        assert_eq!(state.phase, v1::InitialIdentityPhase::Idle as i32);
        assert!(store.load().unwrap().initial_identity_operation.is_none());
        assert_eq!(std::fs::read(store.path()).unwrap(), before);
        assert!(service.load_warp_identity(profile_id).await.is_err());
    }

    #[tokio::test]
    async fn resume_only_finalizes_a_matching_pending_ready_identity() {
        let directory = tempfile::tempdir().unwrap();
        let service = ControlService::open_with_vault(
            ConfigStore::new(directory.path().join("config.json")),
            Arc::new(MemoryVault::default()),
        )
        .unwrap();
        let profile_id = service.config_snapshot().await.profiles[0].id;
        service
            .replace_identity_locked(
                profile_id,
                test_identity(IdentityProvider::Consumer, Some("retained-license")),
                None,
            )
            .await
            .unwrap();
        let mut resume = request(profile_id);
        resume.resume_only = true;
        resume.provisioning = None;
        let operation_id = resume.operation_id.parse().unwrap();
        service
            .update_config(move |config| {
                config.initial_identity_operation = Some(
                    InitialIdentityOperation::new(
                        operation_id,
                        profile_id,
                        "registerWithLicense",
                        None,
                        Uuid::new_v4(),
                    )
                    .map_err(ControlServiceError::configuration)?,
                );
                Ok(())
            })
            .await
            .unwrap();
        let state = service
            .initial_identity_with(resume, |_| async {
                panic!("Ready recovery must never register remotely")
            })
            .await
            .unwrap();
        assert!(state.reused);
        assert_eq!(state.phase, v1::InitialIdentityPhase::Completed as i32);
        assert_eq!(
            service
                .config_snapshot()
                .await
                .initial_identity_operation
                .unwrap()
                .phase,
            InitialIdentityPhase::Completed
        );
        assert_eq!(
            service
                .load_warp_identity(profile_id)
                .await
                .unwrap()
                .license(),
            Some("retained-license")
        );
    }

    #[tokio::test]
    async fn ready_identity_is_reused_without_registration_or_license_loss() {
        let directory = tempfile::tempdir().unwrap();
        let service = ControlService::open_with_vault(
            ConfigStore::new(directory.path().join("config.json")),
            Arc::new(MemoryVault::default()),
        )
        .unwrap();
        let profile_id = service.config_snapshot().await.profiles[0].id;
        let identity = test_identity(IdentityProvider::Consumer, Some("keep-license"));
        service
            .replace_identity_locked(profile_id, identity, None)
            .await
            .unwrap();
        let before = service.load_warp_identity(profile_id).await.unwrap();
        let state = service
            .initial_identity_with(request(profile_id), |_| async {
                panic!("a Ready identity must never be registered again")
            })
            .await
            .unwrap();
        assert_eq!(state.phase, v1::InitialIdentityPhase::Completed as i32);
        assert!(state.reused);
        let after = service.load_warp_identity(profile_id).await.unwrap();
        assert_eq!(before.device_id(), after.device_id());
        assert_eq!(after.license(), Some("keep-license"));
    }

    #[tokio::test]
    async fn pending_lease_blocks_duplicate_operations_and_survives_caller_timeout() {
        let directory = tempfile::tempdir().unwrap();
        let service = ControlService::open_with_vault(
            ConfigStore::new(directory.path().join("config.json")),
            Arc::new(MemoryVault::default()),
        )
        .unwrap();
        let profile_id = service.config_snapshot().await.profiles[0].id;
        let first = request(profile_id);
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (finish_tx, finish_rx) = tokio::sync::oneshot::channel();
        let worker_service = service.clone();
        let mut worker = tokio::spawn(async move {
            worker_service
                .initial_identity_with(first, |_| async move {
                    started_tx.send(()).unwrap();
                    finish_rx.await.unwrap();
                    Ok(ProvisionedIdentity::consumer(test_identity(
                        IdentityProvider::Consumer,
                        None,
                    )))
                })
                .await
        });
        started_rx.await.unwrap();
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(1), &mut worker)
                .await
                .is_err()
        );
        let pending = service
            .get_initial_identity_state(profile_id)
            .await
            .unwrap();
        assert_eq!(pending.phase, v1::InitialIdentityPhase::Pending as i32);
        let duplicate = service
            .initial_identity_with(request(profile_id), |_| async {
                panic!("pending execution must not be replayed")
            })
            .await
            .unwrap();
        assert_eq!(duplicate.phase, v1::InitialIdentityPhase::Pending as i32);
        finish_tx.send(()).unwrap();
        assert_eq!(
            worker.await.unwrap().unwrap().phase,
            v1::InitialIdentityPhase::Completed as i32
        );
    }

    #[tokio::test]
    async fn lost_owner_is_interrupted_and_clear_fences_late_result() {
        let directory = tempfile::tempdir().unwrap();
        let store = ConfigStore::new(directory.path().join("config.json"));
        let service =
            ControlService::open_with_vault(store.clone(), Arc::new(MemoryVault::default()))
                .unwrap();
        let profile_id = service.config_snapshot().await.profiles[0].id;
        let operation = request(profile_id);
        service
            .update_config(move |config| {
                config.initial_identity_operation = Some(
                    InitialIdentityOperation::new(
                        operation.operation_id.parse().unwrap(),
                        profile_id,
                        "register",
                        None,
                        Uuid::new_v4(),
                    )
                    .map_err(ControlServiceError::configuration)?,
                );
                Ok(())
            })
            .await
            .unwrap();
        let before = std::fs::read(store.path()).unwrap();
        assert_eq!(
            service
                .get_initial_identity_state(profile_id)
                .await
                .unwrap()
                .phase,
            v1::InitialIdentityPhase::Interrupted as i32
        );
        assert_eq!(std::fs::read(store.path()).unwrap(), before);
        let late_service = service.clone();
        let result = service
            .initial_identity_with(request(profile_id), |_| async move {
                late_service
                    .update_config(|config| {
                        *config = AppConfig::default();
                        Ok(())
                    })
                    .await
                    .unwrap();
                Ok(ProvisionedIdentity::consumer(test_identity(
                    IdentityProvider::Consumer,
                    Some("uncommitted-license"),
                )))
            })
            .await;
        assert!(result.is_err());
        assert!(service.load_warp_identity(profile_id).await.is_err());
        assert!(store.load().unwrap().initial_identity_operation.is_none());
        assert_eq!(service.remote_license_unbinds.lock().await.len(), 1);
    }

    #[tokio::test]
    async fn partial_credentials_require_repair_and_are_not_overwritten() {
        let directory = tempfile::tempdir().unwrap();
        let vault = Arc::new(MemoryVault::default());
        let service = ControlService::open_with_vault(
            ConfigStore::new(directory.path().join("config.json")),
            vault.clone(),
        )
        .unwrap();
        let profile_id = service.config_snapshot().await.profiles[0].id;
        vault
            .put(profile_id, SecretRecord::WarpSecret, b"partial")
            .await
            .unwrap();
        let state = service
            .initial_identity_with(request(profile_id), |_| async {
                panic!("partial identity material must not be replaced by onboarding")
            })
            .await
            .unwrap();
        assert_eq!(state.phase, v1::InitialIdentityPhase::Failed as i32);
        assert_eq!(state.error_code, "INITIAL_IDENTITY_REPAIR_REQUIRED");
        assert_eq!(
            vault
                .get(profile_id, SecretRecord::WarpSecret)
                .await
                .unwrap()
                .unwrap()
                .as_slice(),
            b"partial"
        );
    }

    #[tokio::test]
    async fn zero_trust_ready_identity_is_reused_by_default_consumer_request() {
        let directory = tempfile::tempdir().unwrap();
        let service = ControlService::open_with_vault(
            ConfigStore::new(directory.path().join("config.json")),
            Arc::new(MemoryVault::default()),
        )
        .unwrap();
        let profile_id = service.config_snapshot().await.profiles[0].id;
        let provider = IdentityProvider::zero_trust("example-team").unwrap();
        service
            .replace_identity_locked(
                profile_id,
                test_identity(provider.clone(), None),
                Some(ManagedEndpointIps {
                    ipv4: "162.159.197.2".parse().unwrap(),
                    ipv6: "2606:4700:102::2".parse().unwrap(),
                }),
            )
            .await
            .unwrap();
        let state = service
            .initial_identity_with(request(profile_id), |_| async {
                panic!("onboarding must not change Zero Trust provider")
            })
            .await
            .unwrap();
        assert!(state.reused);
        assert_eq!(state.phase, v1::InitialIdentityPhase::Completed as i32);
        assert_eq!(
            service
                .load_warp_identity(profile_id)
                .await
                .unwrap()
                .provider(),
            &provider
        );
    }
}
