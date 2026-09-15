use super::*;
use crate::journal::{DeviceState, ManagedDevice};

const DEVICE_RETIREMENT_TIMEOUT: Duration = Duration::from_secs(12);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeviceLeaseKey {
    pub id: Uuid,
    pub generation: u64,
}

pub(super) struct DeviceOwnerLease {
    key: DeviceLeaseKey,
    sid: String,
    process_id: u32,
    attached: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceRetirement {
    Complete,
    /// Network effects are gone and the remaining device intent is durable.
    Deferred,
}

impl<Backend: PrivilegedBackend + 'static> AgentCoordinator<Backend> {
    pub fn device_lease_attached(&self) -> bool {
        self.device_lease
            .lock()
            .expect("device lease lock")
            .as_ref()
            .is_some_and(|lease| lease.attached)
    }

    pub fn device_retirement_deferred(&self) -> bool {
        self.device_retirement_deferred.load(Ordering::Acquire)
    }

    pub fn device_retirement_finished(&self) -> bool {
        self.device_retirement_deferred()
            || self.device_retirement_completed.load(Ordering::Acquire)
    }

    pub fn device_retirement_retry_pending(&self) -> bool {
        self.device_retirement_retry_pending.load(Ordering::Acquire)
    }

    /// The service owns persistence retries even after the releasing pipe is gone.
    pub async fn retry_device_retirement_persistence(&self) -> Result<(), CoordinatorError> {
        let mut journal = self.journal.lock().await;
        if !self.device_retirement_retry_pending() {
            return Ok(());
        }
        if journal.phase != RecoveryPhase::Clean || self.device_lease_attached() {
            return Err(CoordinatorError::RecoveryBusy);
        }
        self.retire_device_locked(&mut journal).await.map(|_| ())
    }

    fn save_device_retirement(&self, journal: &mut RecoveryJournal) -> Result<(), JournalError> {
        let result = self.store.save(journal);
        self.device_retirement_retry_pending.store(
            matches!(&result, Err(JournalError::Io(_))),
            Ordering::Release,
        );
        result
    }

    pub async fn may_exit_idle(&self) -> bool {
        let journal = self.journal.lock().await;
        !self.device_lease_attached()
            && (journal.is_fully_clean()
                || (journal.phase == RecoveryPhase::Clean && self.device_retirement_deferred()))
    }

    /// Process shutdown owns all remaining cleanup; it does not grant reuse.
    pub async fn recover_for_process_exit(
        &self,
        revoke: impl std::future::Future<Output = ()> + Send,
    ) -> Result<DeviceRetirement, CoordinatorError> {
        let mut journal = self.journal.lock().await;
        if let Some(lease) = self
            .device_lease
            .lock()
            .expect("device lease lock")
            .as_mut()
        {
            lease.attached = false;
        }
        if journal.phase != RecoveryPhase::Clean {
            self.recover_locked_with_egress(&mut journal, revoke)
                .await?;
        } else {
            revoke.await;
        }
        self.retire_device_locked(&mut journal).await
    }

    pub(super) fn require_device_owner(
        &self,
        caller: &AuthenticatedCaller,
        key: Option<DeviceLeaseKey>,
    ) -> Result<(), CoordinatorError> {
        let lease = self.device_lease.lock().expect("device lease lock");
        if !lease.as_ref().is_some_and(|lease| {
            lease.attached
                && lease.sid == caller.user_sid
                && lease.process_id == caller.process_id
                && key.is_none_or(|key| key == lease.key)
        }) {
            return Err(CoordinatorError::DeviceLeaseRequired);
        }
        Ok(())
    }

    /// Allocates ownership only. The first Prepare creates the native device.
    pub async fn acquire_device_lease(
        &self,
        caller: &AuthenticatedCaller,
    ) -> Result<DeviceLeaseKey, CoordinatorError> {
        validate_caller(caller)?;
        let journal = self.journal.lock().await;
        if let Some(device) = &journal.device {
            if device.owner_sid != caller.user_sid {
                return Err(CoordinatorError::OwnerMismatch);
            }
            let current_idle =
                device.state == DeviceState::Idle && device.agent_instance == self.agent_instance;
            let reattach =
                journal.phase == RecoveryPhase::Active && device.state == DeviceState::InUse;
            if !current_idle && !reattach {
                return Err(CoordinatorError::DeviceRecoveryRequired);
            }
        }
        if journal.phase != RecoveryPhase::Clean && journal.phase != RecoveryPhase::Active {
            return Err(CoordinatorError::RecoveryRequired(journal.phase));
        }
        if journal.phase == RecoveryPhase::Active
            && (self.packet_session_attached() || self.tunnel_lease_attached())
        {
            return Err(CoordinatorError::RecoveryBusy);
        }
        let mut lease = self.device_lease.lock().expect("device lease lock");
        if lease.as_ref().is_some_and(|lease| lease.attached) {
            return Err(CoordinatorError::DeviceLeaseRequired);
        }
        let key = DeviceLeaseKey {
            id: Uuid::new_v4(),
            generation: self.device_lease_epoch.fetch_add(1, Ordering::AcqRel) + 1,
        };
        *lease = Some(DeviceOwnerLease {
            key,
            sid: caller.user_sid.clone(),
            process_id: caller.process_id,
            attached: true,
        });
        self.device_retirement_deferred
            .store(false, Ordering::Release);
        self.device_retirement_completed
            .store(false, Ordering::Release);
        *self
            .device_retirement_result
            .lock()
            .expect("device retirement lock") = None;
        self.device_retirement_retry_pending
            .store(false, Ordering::Release);
        Ok(key)
    }

    pub fn detach_device_lease(&self, key: DeviceLeaseKey, caller: &AuthenticatedCaller) -> bool {
        let mut lease = self.device_lease.lock().expect("device lease lock");
        if let Some(lease) = lease.as_mut()
            && lease.key == key
            && lease.sid == caller.user_sid
            && lease.process_id == caller.process_id
            && lease.attached
        {
            lease.attached = false;
            return true;
        }
        false
    }

    pub async fn prepare_managed(
        &self,
        operation_id: Uuid,
        plan: ValidatedTunnelPlan,
        caller: AuthenticatedCaller,
        key: DeviceLeaseKey,
        expected_generation: u64,
    ) -> Result<RecoveryJournal, CoordinatorError> {
        validate_caller(&caller)?;
        plan.validate()
            .map_err(|error| CoordinatorError::InvalidPlan(error.to_string()))?;
        let mut journal = self.journal.lock().await;
        self.require_device_owner(&caller, Some(key))?;
        if journal.generation != expected_generation {
            return Err(CoordinatorError::RecoveryConflict);
        }
        if journal.phase != RecoveryPhase::Clean {
            return Err(CoordinatorError::RecoveryRequired(journal.phase));
        }
        if let Some(device) = &journal.device {
            if device.state != DeviceState::Idle
                || device.agent_instance != self.agent_instance
                || device.owner_sid != caller.user_sid
                || self.packet_session_attached()
            {
                return Err(CoordinatorError::DeviceRecoveryRequired);
            }
            match self.backend.inspect_idle_device(&device.receipt).await {
                Ok(true) => {}
                result => {
                    journal.device.as_mut().expect("device").state = DeviceState::RecoveryRequired;
                    self.store.save(&mut journal)?;
                    return Err(match result {
                        Err(error) => error.into(),
                        _ => CoordinatorError::DeviceRecoveryRequired,
                    });
                }
            }
        } else {
            let device_id = Uuid::new_v4();
            let receipt = MutationReceipt::WintunAdapter {
                adapter_name: ManagedDevice::name(device_id),
                adapter_guid: device_id,
                interface_luid: 0,
            };
            journal.device = Some(ManagedDevice {
                device_id,
                generation: journal.generation.saturating_add(1),
                agent_instance: self.agent_instance,
                owner_sid: caller.user_sid.clone(),
                owner_process_id: caller.process_id,
                state: DeviceState::Creating,
                receipt: receipt.clone(),
            });
            // No native work is allowed until the creation intent is durable.
            self.store.save(&mut journal)?;
            match self.backend.create_device(receipt).await {
                Ok(receipt) => {
                    let device = journal.device.as_mut().expect("creation intent");
                    if !matches!(&receipt, MutationReceipt::WintunAdapter { adapter_name, adapter_guid, interface_luid }
                        if *adapter_guid == device_id && *adapter_name == ManagedDevice::name(device_id) && *interface_luid != 0)
                    {
                        device.state = DeviceState::RecoveryRequired;
                        self.store.save(&mut journal)?;
                        return Err(BackendError::AdapterIdentity.into());
                    }
                    device.receipt = receipt;
                    device.state = DeviceState::Idle;
                    if let Err(error) = self.store.save(&mut journal) {
                        journal.device.as_mut().expect("device").state =
                            DeviceState::RecoveryRequired;
                        return Err(error.into());
                    }
                }
                Err(error) => {
                    journal.device.as_mut().expect("creation intent").state =
                        DeviceState::RecoveryRequired;
                    self.store.save(&mut journal)?;
                    return Err(error.into());
                }
            }
        }
        self.prepare_locked(&mut journal, operation_id, plan, caller)
            .await
    }

    /// Called while the service's mutation gate is held, after explicit release.
    pub async fn release_device_lease(
        &self,
        key: DeviceLeaseKey,
        caller: &AuthenticatedCaller,
    ) -> Result<DeviceRetirement, CoordinatorError> {
        self.release_device_lease_with_egress(key, caller, async {})
            .await
    }

    pub async fn release_device_lease_with_egress(
        &self,
        key: DeviceLeaseKey,
        caller: &AuthenticatedCaller,
        revoke: impl std::future::Future<Output = ()> + Send,
    ) -> Result<DeviceRetirement, CoordinatorError> {
        let mut journal = self.journal.lock().await;
        self.require_device_owner(caller, Some(key))?;
        self.detach_device_lease(key, caller);
        if journal.phase != RecoveryPhase::Clean {
            self.recover_locked_with_egress(&mut journal, revoke)
                .await?;
        } else {
            revoke.await;
        }
        self.retire_device_locked(&mut journal).await
    }

    /// A stale EOF timer must never retire a newer owner's device.
    pub async fn retire_orphaned_device(
        &self,
        key: DeviceLeaseKey,
    ) -> Result<bool, CoordinatorError> {
        self.retire_orphaned_device_with_egress(key, async {}).await
    }

    pub async fn retire_orphaned_device_with_egress(
        &self,
        key: DeviceLeaseKey,
        revoke: impl std::future::Future<Output = ()> + Send,
    ) -> Result<bool, CoordinatorError> {
        let mut journal = self.journal.lock().await;
        {
            let lease = self.device_lease.lock().expect("device lease lock");
            if !lease
                .as_ref()
                .is_some_and(|lease| lease.key == key && !lease.attached)
            {
                return Ok(false);
            }
        }
        if journal.phase != RecoveryPhase::Clean {
            self.recover_locked_with_egress(&mut journal, revoke)
                .await?;
        } else {
            revoke.await;
        }
        self.retire_device_locked(&mut journal).await?;
        Ok(true)
    }

    /// Startup/maintenance/shutdown entry; never part of ordinary disconnect.
    pub async fn retire_device(&self) -> Result<DeviceRetirement, CoordinatorError> {
        let mut journal = self.journal.lock().await;
        if journal.phase != RecoveryPhase::Clean
            || self.packet_session_attached()
            || self.device_lease_attached()
        {
            return Err(CoordinatorError::RecoveryBusy);
        }
        self.retire_device_locked(&mut journal).await
    }

    pub(super) async fn retire_device_locked(
        &self,
        journal: &mut RecoveryJournal,
    ) -> Result<DeviceRetirement, CoordinatorError> {
        if self.device_retirement_deferred() {
            return Ok(DeviceRetirement::Deferred);
        }
        let Some(device) = journal.device.as_mut() else {
            return Ok(DeviceRetirement::Complete);
        };
        if journal.phase != RecoveryPhase::Clean || self.packet_session_attached() {
            return Err(CoordinatorError::RecoveryBusy);
        }
        let previous_result = *self
            .device_retirement_result
            .lock()
            .expect("device retirement lock");
        let retirement = match previous_result {
            Some(result) => result,
            None => {
                device.state = DeviceState::Retiring;
                let receipt = device.receipt.clone();
                // A failed intent write cannot start native work or allow exit.
                self.save_device_retirement(journal)?;
                let started = Instant::now();
                let backend = Arc::clone(&self.backend);
                // Remember the attempt before spawning. A timed-out worker owns
                // its handles until completion/process exit; saving its pending
                // result must never start a second native worker.
                *self
                    .device_retirement_result
                    .lock()
                    .expect("device retirement lock") = Some(DeviceRetirement::Deferred);
                let mut worker = tokio::spawn(async move { backend.restore_step(&receipt).await });
                let result =
                    match tokio::time::timeout(DEVICE_RETIREMENT_TIMEOUT, &mut worker).await {
                        Ok(Ok(result)) => result,
                        Ok(Err(_)) => Err(BackendError::Operation(
                            "device retirement worker failed".into(),
                        )),
                        Err(_) => Err(BackendError::AdapterRemovalPending),
                    };
                self.record_step_result(
                    journal,
                    MutationKind::WintunAdapter,
                    started.elapsed(),
                    &result,
                );
                let retirement = match result {
                    Ok(()) => DeviceRetirement::Complete,
                    Err(error) => {
                        warn!(summary = %CoordinatorError::Backend(error).sanitized_recovery_summary(), "device retirement remains pending; retaining its recovery record");
                        DeviceRetirement::Deferred
                    }
                };
                *self
                    .device_retirement_result
                    .lock()
                    .expect("device retirement lock") = Some(retirement);
                retirement
            }
        };
        if retirement == DeviceRetirement::Deferred {
            journal.device.as_mut().expect("device").state = DeviceState::RecoveryRequired;
            self.save_device_retirement(journal)?;
            self.device_retirement_deferred
                .store(true, Ordering::Release);
            return Ok(DeviceRetirement::Deferred);
        }
        let mut clean = RecoveryJournal::clean(journal.generation);
        if let Err(error) = self.save_device_retirement(&mut clean) {
            journal.generation = clean.generation;
            journal.device.as_mut().expect("device").state = DeviceState::RecoveryRequired;
            return Err(error.into());
        }
        *journal = clean;
        self.device_retirement_completed
            .store(true, Ordering::Release);
        Ok(DeviceRetirement::Complete)
    }

    pub fn device_status(&self, journal: &RecoveryJournal) -> agent_v1::ManagedDeviceStatus {
        use agent_v1::ManagedDevicePhase as Phase;
        let phase = match journal.device.as_ref().map(|device| device.state) {
            None => Phase::Absent,
            Some(DeviceState::Creating) => Phase::Creating,
            Some(DeviceState::Idle) => Phase::Idle,
            Some(DeviceState::InUse) => Phase::InUse,
            Some(DeviceState::Retiring) => Phase::Retiring,
            Some(DeviceState::RecoveryRequired) => Phase::RecoveryRequired,
        };
        agent_v1::ManagedDeviceStatus {
            phase: phase as i32,
            lease_attached: self.device_lease_attached(),
            device_generation: journal
                .device
                .as_ref()
                .map_or(0, |device| device.generation),
        }
    }
}
