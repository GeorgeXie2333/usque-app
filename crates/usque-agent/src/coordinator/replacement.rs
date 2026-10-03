use super::*;
use crate::journal::{
    DeviceState, ManagedDevice, ReplacementGuardPlan, ReplacementPhase, TunnelReplacement,
    replacement::{effective_exclusions, intersect_exclusions},
};

impl<Backend: PrivilegedBackend + 'static> AgentCoordinator<Backend> {
    pub async fn replacement_status(
        &self,
        snapshot: &RecoveryJournal,
    ) -> Option<agent_v1::TunnelReplacementStatus> {
        let replacement = snapshot.replacement.as_ref()?;
        let guard_active = replacement.pending()
            && self
                .backend
                .inspect_replacement_guard(&replacement.guard.receipt)
                .await
                .unwrap_or(false);
        // A newer operation may have replaced this snapshot during inspection.
        let current = self.journal.lock().await;
        let guard_active = guard_active && current.generation == snapshot.generation;
        Some(agent_v1::TunnelReplacementStatus {
            source_operation_id: replacement.source_operation_id.to_string(),
            operation_id: replacement.operation_id.to_string(),
            phase: replacement.phase.to_proto(),
            guard_active,
            source_journal_generation: replacement.source_journal_generation,
            target_plan: Some(Box::new(replacement.guard_plan.target.to_proto())),
        })
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "matches the authenticated replacement request"
    )]
    pub async fn replace_tunnel(
        &self,
        source_operation_id: Uuid,
        expected_generation: u64,
        operation_id: Uuid,
        plan: ValidatedTunnelPlan,
        caller: AuthenticatedCaller,
        key: DeviceLeaseKey,
        revoke_egress: impl std::future::Future<Output = ()> + Send,
    ) -> Result<RecoveryJournal, CoordinatorError> {
        validate_caller(&caller)?;
        plan.validate()
            .map_err(|error| CoordinatorError::InvalidPlan(error.to_string()))?;
        if source_operation_id.is_nil()
            || operation_id.is_nil()
            || source_operation_id == operation_id
        {
            return Err(CoordinatorError::RecoveryConflict);
        }
        let mut journal = self.journal.lock().await;
        self.require_device_owner(&caller, Some(key))?;
        let previous = journal.replacement.clone();
        let same_request = previous.as_ref().is_some_and(|replacement| {
            replacement.operation_id == operation_id
                && replacement.source_operation_id == source_operation_id
                && replacement.guard_plan.target == plan
        });
        if let Some(replacement) = &previous
            && replacement.owner_sid != caller.user_sid
        {
            return Err(CoordinatorError::OwnerMismatch);
        }
        if previous
            .as_ref()
            .is_some_and(|replacement| replacement.operation_id == operation_id)
            && !same_request
        {
            return Err(CoordinatorError::RecoveryConflict);
        }
        if same_request {
            let replacement = previous.as_ref().expect("same request");
            if expected_generation != journal.generation
                && expected_generation != replacement.source_journal_generation
            {
                return Err(CoordinatorError::RecoveryConflict);
            }
            if replacement.phase == ReplacementPhase::Aborted {
                return Err(CoordinatorError::RecoveryConflict);
            }
            if replacement.owner_process_id != caller.process_id {
                if self.packet_session_attached() || self.tunnel_lease_attached() {
                    return Err(CoordinatorError::RecoveryBusy);
                }
                journal.owner_process_id = Some(caller.process_id);
                journal
                    .replacement
                    .as_mut()
                    .expect("replacement")
                    .owner_process_id = caller.process_id;
                if let Some(device) = journal.device.as_mut() {
                    device.owner_process_id = caller.process_id;
                }
                self.store.save(&mut journal)?;
            }
            if replacement.phase == ReplacementPhase::Complete {
                if journal.operation_id != Some(operation_id)
                    || journal.phase != RecoveryPhase::Active
                {
                    return Err(CoordinatorError::RecoveryConflict);
                }
                return Ok(journal.clone());
            }
        } else {
            if journal.generation != expected_generation {
                return Err(CoordinatorError::RecoveryConflict);
            }
            let (original_source_operation_id, original_source_plan, allowed) =
                if let Some(replacement) = previous
                    .as_ref()
                    .filter(|replacement| replacement.pending())
                {
                    if replacement.operation_id != source_operation_id
                        || operation_id == replacement.original_source_operation_id
                        || operation_id == replacement.cleanup_operation_id
                        || operation_id == replacement.source_operation_id
                        || replacement.phase == ReplacementPhase::Aborting
                        || replacement.owner_process_id != caller.process_id
                            && (self.packet_session_attached() || self.tunnel_lease_attached())
                    {
                        return Err(CoordinatorError::RecoveryConflict);
                    }
                    if !self
                        .backend
                        .inspect_replacement_guard(&replacement.guard.receipt)
                        .await?
                    {
                        let source_policy = journal.steps.iter().find(|step| {
                            step.kind == MutationKind::KillSwitch
                                && step.state == MutationState::Applied
                        });
                        let source_still_protected = replacement.phase
                            == ReplacementPhase::InstallingGuard
                            && journal
                                .plan
                                .as_ref()
                                .is_some_and(|plan| plan.kill_switch || plan.vpn_chain)
                            && source_policy.is_some()
                            && self
                                .backend
                                .inspect_guard_policy(
                                    &source_policy.expect("checked").receipt,
                                    journal.plan.as_ref().expect("checked").kill_switch,
                                )
                                .await?;
                        // A commit removal intent is enough to repair protection:
                        // the old device may no longer exist after service restart.
                        // The new guard is installed/read back before any cleanup.
                        let committed_target = replacement.phase == ReplacementPhase::Committing
                            && journal.phase == RecoveryPhase::Active;
                        if !source_still_protected && !committed_target {
                            return Err(CoordinatorError::ReplacementGuardUnavailable);
                        }
                    }
                    (
                        replacement.original_source_operation_id,
                        replacement.original_source_plan.clone(),
                        replacement.guard_plan.exclusions.clone(),
                    )
                } else {
                    ensure_owner(&journal, source_operation_id, &caller)?;
                    ensure_operation_kind(&journal, OperationKind::Tunnel)?;
                    let source_plan = journal.plan.clone().ok_or(CoordinatorError::MissingPlan)?;
                    let policy = journal.steps.iter().find(|step| {
                        step.kind == MutationKind::KillSwitch
                            && step.state == MutationState::Applied
                    });
                    if !(source_plan.kill_switch || source_plan.vpn_chain)
                        || policy.is_none()
                        || !self
                            .backend
                            .inspect_guard_policy(
                                &policy.expect("checked").receipt,
                                source_plan.kill_switch,
                            )
                            .await?
                    {
                        return Err(CoordinatorError::ReplacementGuardUnavailable);
                    }
                    (
                        source_operation_id,
                        source_plan.clone(),
                        effective_exclusions(&source_plan),
                    )
                };
            let guard_plan = ReplacementGuardPlan {
                exclusions: intersect_exclusions(&allowed, &plan),
                target: plan.clone(),
            };
            let receipt = self.backend.plan_replacement_guard(&guard_plan).await?;
            let replacement = TunnelReplacement {
                original_source_operation_id,
                original_source_plan,
                source_operation_id,
                source_journal_generation: expected_generation,
                cleanup_operation_id: journal
                    .operation_id
                    .ok_or(CoordinatorError::OperationMismatch)?,
                operation_id,
                owner_sid: caller.user_sid.clone(),
                owner_process_id: caller.process_id,
                guard_plan,
                guard: MutationRecord {
                    kind: MutationKind::KillSwitch,
                    state: MutationState::Intended,
                    receipt,
                },
                phase: ReplacementPhase::InstallingGuard,
                abort_generation: None,
            };
            replacement.validate()?;
            journal.owner_process_id = Some(caller.process_id);
            journal.replacement = Some(replacement);
            self.store.save(&mut journal)?;
        }

        let replacement = journal.replacement.as_ref().expect("replacement").clone();
        if replacement.phase == ReplacementPhase::Committing
            && journal.phase == RecoveryPhase::Active
            && self.packet_session_attached()
        {
            self.complete_replacement_locked(&mut journal).await?;
            return Ok(journal.clone());
        }
        if replacement.phase == ReplacementPhase::Aborting {
            return Err(CoordinatorError::ReplacementPending);
        }
        if replacement.guard.state == MutationState::Intended {
            // An earlier save may have failed after updating this process's
            // in-memory journal. Retry the intent write before any native work.
            self.store.save(&mut journal)?;
            // Retarget uses one native transaction: failure leaves the old guard intact.
            let receipt = self
                .backend
                .apply_replacement_guard(
                    replacement.guard.receipt,
                    &replacement.guard_plan,
                    &caller,
                )
                .await?;
            let entry = journal.replacement.as_mut().expect("replacement");
            entry.guard.receipt = receipt;
            entry.guard.state = MutationState::Applied;
            entry.phase = ReplacementPhase::SourceCleanup;
            self.store.save(&mut journal)?;
        }
        let entry = journal.replacement.as_ref().expect("replacement").clone();
        if !self
            .backend
            .inspect_replacement_guard(&entry.guard.receipt)
            .await?
        {
            // Commit may have removed the guard immediately before a crash lost
            // its completion write. Reinstall it before rebuilding a detached
            // target; never treat a journal claim as native protection.
            if entry.phase != ReplacementPhase::Committing || journal.phase != RecoveryPhase::Active
            {
                return Err(CoordinatorError::ReplacementGuardUnavailable);
            }
            let receipt = self
                .backend
                .apply_replacement_guard(entry.guard.receipt, &entry.guard_plan, &caller)
                .await?;
            let replacement = journal.replacement.as_mut().expect("replacement");
            replacement.guard.receipt = receipt;
            replacement.guard.state = MutationState::Applied;
            replacement.phase = ReplacementPhase::SourceCleanup;
            self.store.save(&mut journal)?;
            if !self
                .backend
                .inspect_replacement_guard(
                    &journal
                        .replacement
                        .as_ref()
                        .expect("replacement")
                        .guard
                        .receipt,
                )
                .await?
            {
                return Err(CoordinatorError::ReplacementGuardUnavailable);
            }
        }
        if journal.operation_id == Some(operation_id)
            && journal.phase == RecoveryPhase::Prepared
            && journal.replacement.as_ref().expect("replacement").phase
                == ReplacementPhase::Prepared
            && journal
                .device
                .as_ref()
                .is_some_and(|device| device.agent_instance == self.agent_instance)
        {
            return Ok(journal.clone());
        }
        self.restore_replacement_normal_locked(&mut journal, revoke_egress)
            .await?;
        self.prepare_replacement_device_locked(&mut journal, &caller)
            .await?;
        journal.replacement.as_mut().expect("replacement").phase =
            ReplacementPhase::PreparingTarget;
        self.store.save(&mut journal)?;
        self.prepare_locked(&mut journal, operation_id, plan, caller)
            .await?;
        journal.replacement.as_mut().expect("replacement").phase = ReplacementPhase::Prepared;
        self.store.save(&mut journal)?;
        Ok(journal.clone())
    }

    async fn restore_replacement_normal_locked(
        &self,
        journal: &mut RecoveryJournal,
        revoke: impl std::future::Future<Output = ()> + Send,
    ) -> Result<(), CoordinatorError> {
        self.tunnel_lease_attached.store(false, Ordering::Release);
        self.tunnel_lease_epoch.fetch_add(1, Ordering::AcqRel);
        journal.phase = RecoveryPhase::Recovering;
        self.store.save(journal)?;
        let failures = self.restore_steps_locked(journal, revoke).await;
        journal.phase = RecoveryPhase::RecoveryRequired;
        self.store.save(journal)?;
        if !failures.is_empty() {
            return Err(recovery_error(&failures));
        }
        // Every restore and the completed cleanup phase are durable. Retire
        // these receipts before a replacement device acquires a different LUID.
        journal.steps.clear();
        self.store.save(journal)?;
        Ok(())
    }

    async fn prepare_replacement_device_locked(
        &self,
        journal: &mut RecoveryJournal,
        caller: &AuthenticatedCaller,
    ) -> Result<(), CoordinatorError> {
        if let Some(device) = &journal.device {
            if device.agent_instance == self.agent_instance
                && self.backend.inspect_idle_device(&device.receipt).await?
            {
                return Ok(());
            }
            // The creator handle cannot cross Agent processes. Keep guard protection
            // while retiring the exact old identity and journaling a fresh device.
            let receipt = device.receipt.clone();
            journal.device.as_mut().expect("device").state = DeviceState::RecoveryRequired;
            self.store.save(journal)?;
            self.backend.restore_step(&receipt).await?;
            journal.device = None;
            journal.device_binding = None;
            self.store.save(journal)?;
        }
        let id = Uuid::new_v4();
        let receipt = MutationReceipt::WintunAdapter {
            adapter_name: ManagedDevice::name(id),
            adapter_guid: id,
            interface_luid: 0,
        };
        let device = ManagedDevice {
            device_id: id,
            generation: journal.generation.saturating_add(1),
            agent_instance: self.agent_instance,
            owner_sid: caller.user_sid.clone(),
            owner_process_id: caller.process_id,
            state: DeviceState::Creating,
            receipt: receipt.clone(),
        };
        journal.device_binding = Some(device.binding());
        journal.device = Some(device);
        self.store.save(journal)?;
        let receipt = self.backend.create_device(receipt).await?;
        if !matches!(&receipt, MutationReceipt::WintunAdapter { adapter_name, adapter_guid, interface_luid } if *adapter_guid == id && *adapter_name == ManagedDevice::name(id) && *interface_luid != 0)
        {
            return Err(BackendError::AdapterIdentity.into());
        }
        let device = journal.device.as_mut().expect("creation intent");
        device.receipt = receipt;
        device.state = DeviceState::InUse;
        self.store.save(journal)?;
        Ok(())
    }

    pub(super) async fn complete_replacement_locked(
        &self,
        journal: &mut RecoveryJournal,
    ) -> Result<(), CoordinatorError> {
        if !journal.replacement_pending() {
            return Ok(());
        }
        let replacement = journal.replacement.as_ref().expect("replacement");
        if journal.operation_id != Some(replacement.operation_id)
            || !self.packet_session_attached()
            || self.backend.inspect_tunnel(journal).await? != TunnelInspection::Reattachable
        {
            return Err(CoordinatorError::ReplacementGuardUnavailable);
        }
        journal.replacement.as_mut().expect("replacement").phase = ReplacementPhase::Committing;
        self.store.save(journal)?;
        let receipt = &journal
            .replacement
            .as_ref()
            .expect("replacement")
            .guard
            .receipt;
        self.backend.restore_replacement_guard(receipt).await?;
        if self.backend.inspect_replacement_guard(receipt).await? {
            return Err(CoordinatorError::ReplacementGuardUnavailable);
        }
        let mut completed = journal.clone();
        let replacement = completed.replacement.as_mut().expect("replacement");
        replacement.guard.state = MutationState::Restored;
        replacement.phase = ReplacementPhase::Complete;
        if let Err(error) = self.store.save(&mut completed) {
            journal.generation = completed.generation;
            return Err(error.into());
        }
        *journal = completed;
        Ok(())
    }

    pub async fn abort_replacement(
        &self,
        operation_id: Uuid,
        expected_generation: u64,
        caller: &AuthenticatedCaller,
        revoke: impl std::future::Future<Output = ()> + Send,
    ) -> Result<RecoveryJournal, CoordinatorError> {
        validate_caller(caller)?;
        let mut journal = self.journal.lock().await;
        let replacement = journal
            .replacement
            .as_ref()
            .ok_or(CoordinatorError::RecoveryConflict)?;
        if replacement.operation_id != operation_id || replacement.owner_sid != caller.user_sid {
            return Err(CoordinatorError::OwnerMismatch);
        }
        if replacement.owner_process_id != caller.process_id
            && (self.packet_session_attached()
                || self.tunnel_lease_attached()
                || self.device_lease_attached() && self.require_device_owner(caller, None).is_err())
        {
            return Err(CoordinatorError::RecoveryBusy);
        }
        if replacement.phase == ReplacementPhase::Aborted {
            return Ok(journal.clone());
        }
        if replacement.phase == ReplacementPhase::Complete
            || journal.generation != expected_generation
                && replacement.abort_generation != Some(expected_generation)
        {
            return Err(CoordinatorError::RecoveryConflict);
        }
        let replacement = journal.replacement.as_mut().expect("replacement");
        replacement
            .abort_generation
            .get_or_insert(expected_generation);
        replacement.phase = ReplacementPhase::Aborting;
        replacement.owner_process_id = caller.process_id;
        journal.owner_process_id = Some(caller.process_id);
        if let Some(device) = journal.device.as_mut() {
            device.owner_process_id = caller.process_id;
        }
        self.store.save(&mut journal)?;
        self.restore_replacement_normal_locked(&mut journal, revoke)
            .await?;
        if let Some(device) = &journal.device
            && device.agent_instance != self.agent_instance
        {
            // A restarted Agent cannot retain a usable creator handle for the
            // next connection. Retire this exact identity under the guard.
            let receipt = device.receipt.clone();
            journal.device.as_mut().expect("device").state = DeviceState::RecoveryRequired;
            self.store.save(&mut journal)?;
            self.backend.restore_step(&receipt).await?;
            journal.device = None;
            journal.device_binding = None;
            self.store.save(&mut journal)?;
        }
        let receipt = &journal
            .replacement
            .as_ref()
            .expect("replacement")
            .guard
            .receipt;
        self.backend.restore_replacement_guard(receipt).await?;
        if self.backend.inspect_replacement_guard(receipt).await? {
            return Err(CoordinatorError::ReplacementGuardUnavailable);
        }
        let mut clean = journal.disconnected();
        let replacement = clean.replacement.as_mut().expect("replacement");
        replacement.guard.state = MutationState::Restored;
        replacement.phase = ReplacementPhase::Aborted;
        if let Err(error) = self.store.save(&mut clean) {
            journal.generation = clean.generation;
            return Err(error.into());
        }
        *journal = clean;
        Ok(journal.clone())
    }

    /// Elevated uninstall/upgrade maintenance is an explicit release boundary.
    /// It uses the same guarded cleanup ordering without impersonating an Engine.
    pub async fn abort_replacement_for_maintenance(&self) -> Result<(), CoordinatorError> {
        let state = self.state().await;
        let Some(replacement) = state.replacement.filter(TunnelReplacement::pending) else {
            return Ok(());
        };
        let caller = AuthenticatedCaller {
            process_id: replacement.owner_process_id,
            user_sid: replacement.owner_sid,
            executable_path: std::env::current_exe()
                .map_err(|error| BackendError::Operation(error.to_string()))?,
            process_handle: None,
        };
        self.abort_replacement(replacement.operation_id, state.generation, &caller, async {
        })
        .await?;
        Ok(())
    }
}
