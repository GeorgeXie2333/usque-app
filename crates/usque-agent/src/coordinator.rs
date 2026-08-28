use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};

use async_trait::async_trait;
use thiserror::Error;
use tokio::sync::Mutex;
use tracing::warn;
use uuid::Uuid;

use crate::{
    AuthenticatedCaller,
    journal::{
        JournalError, JournalStore, MutationKind, MutationReceipt, MutationRecord, MutationState,
        OperationKind, RecoveryJournal, RecoveryPhase,
    },
    plan::ValidatedTunnelPlan,
};

pub const MIN_PACKET_RING_CAPACITY: u32 = 128 * 1024;
pub const MAX_PACKET_RING_CAPACITY: u32 = 64 * 1024 * 1024;
pub const PACKET_RING_LAYOUT_VERSION: u32 = 1;
pub const ORPHANED_TUNNEL_RECOVERY_GRACE: Duration = Duration::from_secs(30);
const SYSTEM_PROXY_RESTORE_ATTEMPTS: u32 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PacketSessionHandles {
    pub mapping_handle: u64,
    pub engine_to_agent_event_handle: u64,
    pub agent_to_engine_event_handle: u64,
    pub shutdown_event_handle: u64,
    pub ring_capacity: u32,
    pub layout_version: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepParameter {
    None,
    PacketRing { capacity: u32 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemProxySettings {
    pub proxy_uri: String,
    pub bypass_hosts: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct StepOutput {
    pub packet_session: Option<PacketSessionHandles>,
}

#[async_trait]
pub trait PrivilegedBackend: Send + Sync {
    /// Performs read-only discovery and creates deterministic resource
    /// identifiers. The returned receipt is persisted before any mutation.
    async fn plan_step(
        &self,
        kind: MutationKind,
        plan: &ValidatedTunnelPlan,
        caller: &AuthenticatedCaller,
        parameter: StepParameter,
    ) -> Result<MutationReceipt, BackendError>;

    /// Applies exactly the resource described by the write-ahead receipt. It
    /// may enrich fields learned from Windows, but must retain the same kind and
    /// identifiers so a crash can still recover from the original receipt.
    async fn apply_step(
        &self,
        receipt: MutationReceipt,
        plan: &ValidatedTunnelPlan,
        caller: &AuthenticatedCaller,
    ) -> Result<(MutationReceipt, StepOutput), BackendError>;

    /// Idempotently restores or removes only the resource named by the receipt.
    /// It must be safe for an `Intended` record whose mutation may or may not
    /// have reached Windows before a crash.
    async fn restore_step(&self, receipt: &MutationReceipt) -> Result<(), BackendError>;

    /// Recreates only volatile packet-session resources after an Agent or
    /// Engine restart. The adapter receipt must identify the exact already
    /// configured Wintun interface; this operation must not alter routes, DNS,
    /// or firewall policy.
    async fn resume_packet_session(
        &self,
        _adapter: &MutationReceipt,
        _session: &MutationReceipt,
        _plan: &ValidatedTunnelPlan,
        _caller: &AuthenticatedCaller,
    ) -> Result<PacketSessionHandles, BackendError> {
        Err(BackendError::Unavailable(
            "packet-session resume".to_owned(),
        ))
    }

    async fn plan_system_proxy(
        &self,
        _operation_id: Uuid,
        _caller: &AuthenticatedCaller,
        _settings: &SystemProxySettings,
    ) -> Result<MutationReceipt, BackendError> {
        Err(BackendError::Unavailable("system proxy".to_owned()))
    }

    async fn apply_system_proxy(
        &self,
        _receipt: MutationReceipt,
    ) -> Result<MutationReceipt, BackendError> {
        Err(BackendError::Unavailable("system proxy".to_owned()))
    }
}

pub struct AgentCoordinator<Backend> {
    backend: Arc<Backend>,
    store: JournalStore,
    journal: Mutex<RecoveryJournal>,
    packet_session_attached: AtomicBool,
    tunnel_lease_attached: AtomicBool,
    tunnel_lease_epoch: AtomicU64,
}

impl<Backend> AgentCoordinator<Backend>
where
    Backend: PrivilegedBackend,
{
    pub fn open(store: JournalStore, backend: Arc<Backend>) -> Result<Self, CoordinatorError> {
        let journal = store.load_or_clean()?;
        Ok(Self {
            backend,
            store,
            journal: Mutex::new(journal),
            // Packet mappings, events, and Wintun sessions are process-local.
            // A journal loaded by a fresh Agent can never imply a live session.
            packet_session_attached: AtomicBool::new(false),
            tunnel_lease_attached: AtomicBool::new(false),
            tunnel_lease_epoch: AtomicU64::new(0),
        })
    }

    pub async fn state(&self) -> RecoveryJournal {
        self.journal.lock().await.clone()
    }

    /// Finalizes journal entries whose exact Wintun adapter has already been
    /// removed by an earlier best-effort recovery pass. Interface addresses,
    /// MTU, and DNS state cannot survive removal of that same adapter, so these
    /// receipts no longer require another Windows mutation. This deliberately
    /// does not cover routes because a default-route receipt can also contain
    /// physical-interface split exclusions that still need explicit cleanup.
    pub async fn reconcile_removed_adapter_dependencies(&self) -> Result<bool, CoordinatorError> {
        let mut journal = self.journal.lock().await;
        if journal.phase != RecoveryPhase::RecoveryRequired {
            return Ok(false);
        }

        let original = journal.clone();
        let reconciled = (0..journal.steps.len())
            .filter(|index| dependency_satisfied_by_restored_wintun(&journal, *index))
            .collect::<Vec<_>>();
        if reconciled.is_empty() {
            return Ok(false);
        }
        for index in reconciled {
            journal.steps[index].state = MutationState::Restored;
        }
        if journal
            .steps
            .iter()
            .all(|step| step.state == MutationState::Restored)
        {
            let generation = journal.generation;
            *journal = RecoveryJournal::clean(generation);
        }
        if let Err(error) = self.store.save(&mut journal) {
            *journal = original;
            return Err(error.into());
        }
        Ok(true)
    }

    pub fn packet_session_attached(&self) -> bool {
        self.packet_session_attached.load(Ordering::Acquire)
    }

    pub fn tunnel_lease_attached(&self) -> bool {
        self.tunnel_lease_attached.load(Ordering::Acquire)
    }

    pub async fn recover_stale(&self) -> Result<(), CoordinatorError> {
        let mut journal = self.journal.lock().await;
        if journal.phase == RecoveryPhase::Clean {
            return Ok(());
        }
        self.recover_locked(&mut journal).await
    }

    pub async fn prepare(
        &self,
        operation_id: Uuid,
        plan: ValidatedTunnelPlan,
        caller: AuthenticatedCaller,
    ) -> Result<RecoveryJournal, CoordinatorError> {
        plan.validate()
            .map_err(|error| CoordinatorError::InvalidPlan(error.to_string()))?;
        validate_caller(&caller)?;
        let mut journal = self.journal.lock().await;
        ensure_clean(&journal)?;
        *journal = RecoveryJournal {
            schema_version: crate::journal::JOURNAL_SCHEMA_VERSION,
            generation: journal.generation,
            phase: RecoveryPhase::Preparing,
            operation_kind: Some(OperationKind::Tunnel),
            operation_id: Some(operation_id),
            owner_sid: Some(caller.user_sid.clone()),
            owner_process_id: Some(caller.process_id),
            plan: Some(plan.clone()),
            pause_deadline_unix_seconds: None,
            steps: Vec::new(),
        };
        self.store.save(&mut journal)?;

        // Complete every fallible, non-blocking interface preparation here.
        // The persistent WFP policy is deliberately deferred until commit,
        // after the packet session exists and immediately before default
        // routes are installed.
        let kinds = [
            MutationKind::WintunAdapter,
            MutationKind::EndpointBypass,
            MutationKind::InterfaceConfiguration,
            MutationKind::Dns,
        ];

        for kind in kinds {
            if let Err(error) = self
                .apply_new_step(&mut journal, kind, &plan, &caller, StepParameter::None)
                .await
            {
                let recovery = self.recover_locked(&mut journal).await;
                return match recovery {
                    Ok(()) => Err(error),
                    Err(recovery) => Err(CoordinatorError::ApplyAndRecovery {
                        apply: error.to_string(),
                        recovery: recovery.to_string(),
                    }),
                };
            }
        }
        journal.phase = RecoveryPhase::Prepared;
        if let Err(error) = self.store.save(&mut journal) {
            let recovery = self.recover_locked(&mut journal).await;
            return match recovery {
                Ok(()) => Err(error.into()),
                Err(recovery) => Err(CoordinatorError::ApplyAndRecovery {
                    apply: error.to_string(),
                    recovery: recovery.to_string(),
                }),
            };
        }
        Ok(journal.clone())
    }

    pub async fn open_packet_session(
        &self,
        operation_id: Uuid,
        capacity: u32,
        caller: &AuthenticatedCaller,
    ) -> Result<PacketSessionHandles, CoordinatorError> {
        if !(MIN_PACKET_RING_CAPACITY..=MAX_PACKET_RING_CAPACITY).contains(&capacity)
            || !capacity.is_power_of_two()
        {
            return Err(CoordinatorError::InvalidRingCapacity(capacity));
        }
        let mut journal = self.journal.lock().await;
        ensure_owner(&journal, operation_id, caller)?;
        ensure_operation_kind(&journal, OperationKind::Tunnel)?;
        if journal.phase != RecoveryPhase::Prepared {
            return Err(CoordinatorError::InvalidPhase {
                expected: "prepared",
                actual: journal.phase,
            });
        }
        journal.steps.retain(|step| {
            !(step.kind == MutationKind::PacketSession && step.state == MutationState::Restored)
        });
        if journal
            .steps
            .iter()
            .any(|step| step.kind == MutationKind::PacketSession)
        {
            return Err(CoordinatorError::DuplicatePacketSession);
        }
        let plan = journal.plan.clone().ok_or(CoordinatorError::MissingPlan)?;
        let output = match self
            .apply_new_step(
                &mut journal,
                MutationKind::PacketSession,
                &plan,
                caller,
                StepParameter::PacketRing { capacity },
            )
            .await
        {
            Ok(output) => output,
            Err(error) => {
                let recovery = self.recover_locked(&mut journal).await;
                return match recovery {
                    Ok(()) => Err(error),
                    Err(recovery) => Err(CoordinatorError::ApplyAndRecovery {
                        apply: error.to_string(),
                        recovery: recovery.to_string(),
                    }),
                };
            }
        };
        let handles = output
            .packet_session
            .ok_or(CoordinatorError::MissingPacketHandles)?;
        self.packet_session_attached.store(true, Ordering::Release);
        Ok(handles)
    }

    pub async fn close_packet_session(
        &self,
        operation_id: Uuid,
        caller: &AuthenticatedCaller,
    ) -> Result<RecoveryJournal, CoordinatorError> {
        let mut journal = self.journal.lock().await;
        ensure_owner(&journal, operation_id, caller)?;
        ensure_operation_kind(&journal, OperationKind::Tunnel)?;
        let active = journal.phase == RecoveryPhase::Active;
        if !matches!(
            journal.phase,
            RecoveryPhase::Prepared | RecoveryPhase::Active
        ) {
            return Err(CoordinatorError::InvalidPhase {
                expected: "prepared or active",
                actual: journal.phase,
            });
        }
        let Some(index) = journal
            .steps
            .iter()
            .position(|step| step.kind == MutationKind::PacketSession)
        else {
            self.packet_session_attached.store(false, Ordering::Release);
            return Ok(journal.clone());
        };
        if journal.steps[index].state != MutationState::Restored
            && let Err(error) = self
                .backend
                .restore_step(&journal.steps[index].receipt)
                .await
        {
            warn!(
                error = %error,
                "packet-session restore failed; keeping the tunnel phase unchanged"
            );
            return Err(error.into());
        }
        if active {
            // Active tunnel recovery keeps the logical packet step applied in
            // the journal while disposing only process-local handles/session.
            // Persistent routes, DNS, and WFP remain untouched and the same
            // step can be reattached by `resume_tunnel`.
            self.packet_session_attached.store(false, Ordering::Release);
            self.store.save(&mut journal)?;
            return Ok(journal.clone());
        }
        journal.steps.remove(index);
        self.packet_session_attached.store(false, Ordering::Release);
        self.store.save(&mut journal)?;
        Ok(journal.clone())
    }

    pub async fn resume_tunnel(
        &self,
        operation_id: Uuid,
        profile_id: Uuid,
        caller: &AuthenticatedCaller,
    ) -> Result<PacketSessionHandles, CoordinatorError> {
        validate_caller(caller)?;
        let mut journal = self.journal.lock().await;
        if journal.operation_id != Some(operation_id) {
            return Err(CoordinatorError::OperationMismatch);
        }
        ensure_operation_kind(&journal, OperationKind::Tunnel)?;
        if journal.phase != RecoveryPhase::Active {
            return Err(CoordinatorError::InvalidPhase {
                expected: "active",
                actual: journal.phase,
            });
        }
        if journal.owner_sid.as_deref() != Some(caller.user_sid.as_str()) {
            return Err(CoordinatorError::OwnerMismatch);
        }
        if self.packet_session_attached.load(Ordering::Acquire) {
            return Err(CoordinatorError::PacketSessionAlreadyAttached);
        }
        let plan = journal.plan.clone().ok_or(CoordinatorError::MissingPlan)?;
        if plan.profile_id != profile_id {
            return Err(CoordinatorError::ProfileMismatch {
                expected: plan.profile_id,
                actual: profile_id,
            });
        }
        let adapter = journal
            .steps
            .iter()
            .find(|step| {
                step.kind == MutationKind::WintunAdapter && step.state == MutationState::Applied
            })
            .map(|step| step.receipt.clone())
            .ok_or(CoordinatorError::MissingAppliedStep(
                MutationKind::WintunAdapter,
            ))?;
        let session = journal
            .steps
            .iter()
            .find(|step| {
                step.kind == MutationKind::PacketSession && step.state == MutationState::Applied
            })
            .map(|step| step.receipt.clone())
            .ok_or(CoordinatorError::MissingAppliedStep(
                MutationKind::PacketSession,
            ))?;

        // CallerPolicy has already authenticated the exact Engine image and
        // signer. Same-SID takeover is permitted here because the prior Engine
        // PID may have died; all other mutation APIs continue requiring the
        // exact owner PID. Persist the new owner before creating volatile
        // handles so a crash cannot leave an unowned resumed transaction.
        journal.owner_process_id = Some(caller.process_id);
        self.store.save(&mut journal)?;
        let handles = self
            .backend
            .resume_packet_session(&adapter, &session, &plan, caller)
            .await?;
        self.packet_session_attached.store(true, Ordering::Release);
        Ok(handles)
    }

    pub async fn acquire_tunnel_lease(
        &self,
        operation_id: Uuid,
        caller: &AuthenticatedCaller,
    ) -> Result<RecoveryJournal, CoordinatorError> {
        let journal = self.journal.lock().await;
        ensure_owner(&journal, operation_id, caller)?;
        ensure_operation_kind(&journal, OperationKind::Tunnel)?;
        if journal.phase != RecoveryPhase::Active {
            return Err(CoordinatorError::InvalidPhase {
                expected: "active",
                actual: journal.phase,
            });
        }
        if !self.packet_session_attached.load(Ordering::Acquire) {
            return Err(CoordinatorError::PacketSessionNotAttached);
        }
        if self.tunnel_lease_attached.swap(true, Ordering::AcqRel) {
            return Err(CoordinatorError::TunnelLeaseAlreadyAttached);
        }
        self.tunnel_lease_epoch.fetch_add(1, Ordering::AcqRel);
        Ok(journal.clone())
    }

    /// Releases the connection that owns tunnel setup before it is promoted
    /// to the active liveness lease. A later active lease changes the epoch and
    /// invalidates the corresponding startup watchdog.
    pub async fn release_startup_tunnel_lease(
        &self,
        operation_id: Uuid,
        caller: &AuthenticatedCaller,
    ) -> Result<Option<u64>, CoordinatorError> {
        validate_caller(caller)?;
        let journal = self.journal.lock().await;
        if !matches!(
            journal.phase,
            RecoveryPhase::Prepared | RecoveryPhase::Active
        ) || journal.operation_kind != Some(OperationKind::Tunnel)
            || journal.operation_id != Some(operation_id)
            || journal.owner_sid.as_deref() != Some(caller.user_sid.as_str())
            || journal.owner_process_id != Some(caller.process_id)
            || self.tunnel_lease_attached.load(Ordering::Acquire)
        {
            return Ok(None);
        }
        Ok(Some(
            self.tunnel_lease_epoch.fetch_add(1, Ordering::AcqRel) + 1,
        ))
    }

    pub async fn release_tunnel_lease(
        &self,
        operation_id: Uuid,
        caller: &AuthenticatedCaller,
    ) -> Result<Option<u64>, CoordinatorError> {
        validate_caller(caller)?;
        let mut journal = self.journal.lock().await;
        if journal.phase != RecoveryPhase::Active
            || journal.operation_kind != Some(OperationKind::Tunnel)
            || journal.operation_id != Some(operation_id)
            || journal.owner_sid.as_deref() != Some(caller.user_sid.as_str())
            || journal.owner_process_id != Some(caller.process_id)
            || !self.packet_session_attached.load(Ordering::Acquire)
            || !self.tunnel_lease_attached.load(Ordering::Acquire)
        {
            // A normal rollback may win the race with lease EOF. Never let a
            // stale lease mutate a newer transaction.
            return Ok(None);
        }
        let index = journal
            .steps
            .iter()
            .position(|step| {
                step.kind == MutationKind::PacketSession && step.state == MutationState::Applied
            })
            .ok_or(CoordinatorError::MissingAppliedStep(
                MutationKind::PacketSession,
            ))?;
        if let Err(error) = self
            .backend
            .restore_step(&journal.steps[index].receipt)
            .await
        {
            warn!(
                error = %error,
                "packet-session restore failed; keeping the tunnel phase unchanged"
            );
            return Err(error.into());
        }
        self.packet_session_attached.store(false, Ordering::Release);
        self.tunnel_lease_attached.store(false, Ordering::Release);
        self.store.save(&mut journal)?;
        Ok(Some(
            self.tunnel_lease_epoch.fetch_add(1, Ordering::AcqRel) + 1,
        ))
    }

    /// Recovers Prepared, or the narrow post-Commit/pre-lease Active window,
    /// when the Engine connection that owned startup disappears.
    pub async fn recover_orphaned_startup_tunnel(
        &self,
        operation_id: Uuid,
        lease_epoch: u64,
    ) -> Result<bool, CoordinatorError> {
        let mut journal = self.journal.lock().await;
        if !matches!(
            journal.phase,
            RecoveryPhase::Prepared | RecoveryPhase::Active
        ) || journal.operation_kind != Some(OperationKind::Tunnel)
            || journal.operation_id != Some(operation_id)
            || self.tunnel_lease_attached.load(Ordering::Acquire)
            || self.tunnel_lease_epoch.load(Ordering::Acquire) != lease_epoch
        {
            return Ok(false);
        }
        self.recover_locked(&mut journal).await?;
        Ok(true)
    }

    /// Recovers an active tunnel whose Engine lease disappeared and was not
    /// reattached during the bounded grace period. The operation ID prevents a
    /// stale watchdog from rolling back a newer transaction.
    pub async fn recover_orphaned_tunnel(
        &self,
        operation_id: Uuid,
        lease_epoch: u64,
    ) -> Result<bool, CoordinatorError> {
        let mut journal = self.journal.lock().await;
        if journal.phase != RecoveryPhase::Active
            || journal.operation_kind != Some(OperationKind::Tunnel)
            || journal.operation_id != Some(operation_id)
            || self.packet_session_attached.load(Ordering::Acquire)
            || self.tunnel_lease_attached.load(Ordering::Acquire)
            || self.tunnel_lease_epoch.load(Ordering::Acquire) != lease_epoch
        {
            return Ok(false);
        }
        self.recover_locked(&mut journal).await?;
        Ok(true)
    }

    pub async fn commit(
        &self,
        operation_id: Uuid,
        caller: &AuthenticatedCaller,
    ) -> Result<RecoveryJournal, CoordinatorError> {
        let mut journal = self.journal.lock().await;
        ensure_owner(&journal, operation_id, caller)?;
        ensure_operation_kind(&journal, OperationKind::Tunnel)?;
        if journal.phase != RecoveryPhase::Prepared {
            return Err(CoordinatorError::InvalidPhase {
                expected: "prepared",
                actual: journal.phase,
            });
        }
        if !journal.steps.iter().any(|step| {
            step.kind == MutationKind::PacketSession && step.state == MutationState::Applied
        }) {
            return Err(CoordinatorError::PacketSessionRequired);
        }
        let plan = journal.plan.clone().ok_or(CoordinatorError::MissingPlan)?;
        let mut commit_steps = Vec::with_capacity(2);
        if plan.kill_switch {
            commit_steps.push(MutationKind::KillSwitch);
        }
        commit_steps.push(MutationKind::DefaultRoutes);
        for kind in commit_steps {
            if let Err(error) = self
                .apply_new_step(&mut journal, kind, &plan, caller, StepParameter::None)
                .await
            {
                let recovery = self.recover_locked(&mut journal).await;
                return match recovery {
                    Ok(()) => Err(error),
                    Err(recovery) => Err(CoordinatorError::ApplyAndRecovery {
                        apply: error.to_string(),
                        recovery: recovery.to_string(),
                    }),
                };
            }
        }
        journal.phase = RecoveryPhase::Active;
        if let Err(error) = self.store.save(&mut journal) {
            let recovery = self.recover_locked(&mut journal).await;
            return match recovery {
                Ok(()) => Err(error.into()),
                Err(recovery) => Err(CoordinatorError::ApplyAndRecovery {
                    apply: error.to_string(),
                    recovery: recovery.to_string(),
                }),
            };
        }
        Ok(journal.clone())
    }

    pub async fn rollback(
        &self,
        operation_id: Uuid,
        caller: &AuthenticatedCaller,
    ) -> Result<RecoveryJournal, CoordinatorError> {
        let mut journal = self.journal.lock().await;
        ensure_owner(&journal, operation_id, caller)?;
        self.recover_locked(&mut journal).await?;
        Ok(journal.clone())
    }

    pub async fn apply_system_proxy(
        &self,
        operation_id: Uuid,
        settings: SystemProxySettings,
        caller: AuthenticatedCaller,
    ) -> Result<RecoveryJournal, CoordinatorError> {
        validate_caller(&caller)?;
        let mut journal = self.journal.lock().await;
        if journal.phase == RecoveryPhase::Active
            && journal.operation_kind == Some(OperationKind::Tunnel)
        {
            ensure_owner(&journal, operation_id, &caller)?;
            // Older two-save restores could leave a durable Restored step.
            // Treat those leftovers as absent, matching open_packet_session.
            journal.steps.retain(|step| {
                !(step.kind == MutationKind::SystemProxy && step.state == MutationState::Restored)
            });
            if journal
                .steps
                .iter()
                .any(|step| step.kind == MutationKind::SystemProxy)
            {
                return Err(CoordinatorError::DuplicateStep(MutationKind::SystemProxy));
            }
            let receipt = self
                .backend
                .plan_system_proxy(operation_id, &caller, &settings)
                .await?;
            if receipt.kind() != MutationKind::SystemProxy {
                return Err(CoordinatorError::BackendReceiptMismatch {
                    expected: MutationKind::SystemProxy,
                    actual: receipt.kind(),
                });
            }
            journal.steps.push(MutationRecord {
                kind: MutationKind::SystemProxy,
                state: MutationState::Intended,
                receipt,
            });
            self.store.save(&mut journal)?;
            let index = journal.steps.len() - 1;
            let applied = match self
                .backend
                .apply_system_proxy(journal.steps[index].receipt.clone())
                .await
            {
                Ok(receipt) if receipt.kind() == MutationKind::SystemProxy => receipt,
                Ok(receipt) => {
                    let error = CoordinatorError::BackendReceiptMismatch {
                        expected: MutationKind::SystemProxy,
                        actual: receipt.kind(),
                    };
                    return Err(self
                        .rollback_appended_system_proxy(&mut journal, index, error.to_string())
                        .await);
                }
                Err(error) => {
                    return Err(self
                        .rollback_appended_system_proxy(&mut journal, index, error.to_string())
                        .await);
                }
            };
            journal.steps[index].receipt = applied;
            journal.steps[index].state = MutationState::Applied;
            if let Err(error) = self.store.save(&mut journal) {
                return Err(self
                    .rollback_appended_system_proxy(&mut journal, index, error.to_string())
                    .await);
            }
            return Ok(journal.clone());
        }
        ensure_clean(&journal)?;
        *journal = RecoveryJournal {
            schema_version: crate::journal::JOURNAL_SCHEMA_VERSION,
            generation: journal.generation,
            phase: RecoveryPhase::Preparing,
            operation_kind: Some(OperationKind::SystemProxy),
            operation_id: Some(operation_id),
            owner_sid: Some(caller.user_sid.clone()),
            owner_process_id: Some(caller.process_id),
            plan: None,
            pause_deadline_unix_seconds: None,
            steps: Vec::new(),
        };
        self.store.save(&mut journal)?;

        let receipt = match self
            .backend
            .plan_system_proxy(operation_id, &caller, &settings)
            .await
        {
            Ok(receipt) if receipt.kind() == MutationKind::SystemProxy => receipt,
            Ok(receipt) => {
                let mismatch = CoordinatorError::BackendReceiptMismatch {
                    expected: MutationKind::SystemProxy,
                    actual: receipt.kind(),
                };
                self.recover_locked(&mut journal).await?;
                return Err(mismatch);
            }
            Err(error) => {
                self.recover_locked(&mut journal).await?;
                return Err(error.into());
            }
        };
        journal.steps.push(MutationRecord {
            kind: MutationKind::SystemProxy,
            state: MutationState::Intended,
            receipt,
        });
        self.store.save(&mut journal)?;

        let applied = match self
            .backend
            .apply_system_proxy(journal.steps[0].receipt.clone())
            .await
        {
            Ok(receipt) if receipt.kind() == MutationKind::SystemProxy => receipt,
            Ok(receipt) => {
                let mismatch = CoordinatorError::BackendReceiptMismatch {
                    expected: MutationKind::SystemProxy,
                    actual: receipt.kind(),
                };
                let recovery = self.recover_locked(&mut journal).await;
                return match recovery {
                    Ok(()) => Err(mismatch),
                    Err(recovery) => Err(CoordinatorError::ApplyAndRecovery {
                        apply: mismatch.to_string(),
                        recovery: recovery.to_string(),
                    }),
                };
            }
            Err(error) => {
                let recovery = self.recover_locked(&mut journal).await;
                return match recovery {
                    Ok(()) => Err(error.into()),
                    Err(recovery) => Err(CoordinatorError::ApplyAndRecovery {
                        apply: error.to_string(),
                        recovery: recovery.to_string(),
                    }),
                };
            }
        };
        journal.steps[0].receipt = applied;
        journal.steps[0].state = MutationState::Applied;
        journal.phase = RecoveryPhase::Active;
        if let Err(error) = self.store.save(&mut journal) {
            let recovery = self.recover_locked(&mut journal).await;
            return match recovery {
                Ok(()) => Err(error.into()),
                Err(recovery) => Err(CoordinatorError::ApplyAndRecovery {
                    apply: error.to_string(),
                    recovery: recovery.to_string(),
                }),
            };
        }
        Ok(journal.clone())
    }

    pub async fn restore_system_proxy(
        &self,
        operation_id: Uuid,
        caller: &AuthenticatedCaller,
    ) -> Result<RecoveryJournal, CoordinatorError> {
        let mut journal = self.journal.lock().await;
        ensure_owner(&journal, operation_id, caller)?;
        if journal.operation_kind == Some(OperationKind::Tunnel) {
            if journal.phase != RecoveryPhase::Active {
                return Err(CoordinatorError::InvalidPhase {
                    expected: "active",
                    actual: journal.phase,
                });
            }
            let Some(index) = journal
                .steps
                .iter()
                .position(|step| step.kind == MutationKind::SystemProxy)
            else {
                return Ok(journal.clone());
            };
            if journal.steps[index].state != MutationState::Restored
                && let Err(error) = self
                    .restore_system_proxy_step(&journal.steps[index].receipt)
                    .await
            {
                return Err(error.into());
            }
            journal.steps.remove(index);
            self.store.save(&mut journal)?;
        } else {
            ensure_operation_kind(&journal, OperationKind::SystemProxy)?;
            self.recover_locked(&mut journal).await?;
        }
        Ok(journal.clone())
    }

    async fn rollback_appended_system_proxy(
        &self,
        journal: &mut RecoveryJournal,
        index: usize,
        apply: String,
    ) -> CoordinatorError {
        let recovery = self
            .restore_system_proxy_step(&journal.steps[index].receipt)
            .await;
        match recovery {
            Ok(()) => {
                journal.steps.remove(index);
                match self.store.save(journal) {
                    Ok(()) => CoordinatorError::Backend(BackendError::Operation(apply)),
                    Err(error) => CoordinatorError::ApplyAndRecovery {
                        apply,
                        recovery: format!(
                            "persist active tunnel after system-proxy rollback: {error}"
                        ),
                    },
                }
            }
            Err(recovery) => CoordinatorError::ApplyAndRecovery {
                apply,
                recovery: recovery.to_string(),
            },
        }
    }

    async fn restore_system_proxy_step(
        &self,
        receipt: &MutationReceipt,
    ) -> Result<(), BackendError> {
        let mut last_error = None;
        for attempt in 1..=SYSTEM_PROXY_RESTORE_ATTEMPTS {
            match self.backend.restore_step(receipt).await {
                Ok(()) => return Ok(()),
                Err(error) => {
                    warn!(
                        attempt,
                        attempts = SYSTEM_PROXY_RESTORE_ATTEMPTS,
                        error = %error,
                        "system-proxy restore failed on an active tunnel; retrying without fail-open recovery"
                    );
                    last_error = Some(error);
                }
            }
        }
        Err(last_error.expect("system-proxy restore attempted"))
    }

    async fn apply_new_step(
        &self,
        journal: &mut RecoveryJournal,
        kind: MutationKind,
        plan: &ValidatedTunnelPlan,
        caller: &AuthenticatedCaller,
        parameter: StepParameter,
    ) -> Result<StepOutput, CoordinatorError> {
        if journal.steps.iter().any(|step| step.kind == kind) {
            return Err(CoordinatorError::DuplicateStep(kind));
        }
        let receipt = self
            .backend
            .plan_step(kind, plan, caller, parameter)
            .await?;
        if receipt.kind() != kind {
            return Err(CoordinatorError::BackendReceiptMismatch {
                expected: kind,
                actual: receipt.kind(),
            });
        }
        journal.steps.push(MutationRecord {
            kind,
            state: MutationState::Intended,
            receipt,
        });
        self.store.save(journal)?;

        let index = journal.steps.len() - 1;
        match self
            .backend
            .apply_step(journal.steps[index].receipt.clone(), plan, caller)
            .await
        {
            Ok((receipt, output)) => {
                if receipt.kind() != kind {
                    return Err(CoordinatorError::BackendReceiptMismatch {
                        expected: kind,
                        actual: receipt.kind(),
                    });
                }
                journal.steps[index].receipt = receipt;
                journal.steps[index].state = MutationState::Applied;
                self.store.save(journal)?;
                Ok(output)
            }
            Err(error) => {
                if let BackendError::PartialApply { receipt, .. } = &error
                    && receipt.kind() == kind
                {
                    journal.steps[index].receipt = receipt.as_ref().clone();
                    self.store.save(journal)?;
                }
                Err(error.into())
            }
        }
    }

    async fn recover_locked(&self, journal: &mut RecoveryJournal) -> Result<(), CoordinatorError> {
        self.tunnel_lease_attached.store(false, Ordering::Release);
        self.tunnel_lease_epoch.fetch_add(1, Ordering::AcqRel);
        journal.phase = RecoveryPhase::Recovering;
        journal.pause_deadline_unix_seconds = None;
        // Persistence failure must never prevent the actual cleanup. A final
        // clean save supersedes this transitional write when recovery succeeds.
        let recovering_save_error = self.store.save(journal).err();
        if let Err(error) = self.restore_steps_locked(journal).await {
            journal.phase = RecoveryPhase::RecoveryRequired;
            let required_save_error = self.store.save(journal).err();
            let mut failures = vec![error.to_string()];
            if let Some(save_error) = recovering_save_error {
                failures.push(format!("persist recovering phase: {save_error}"));
            }
            if let Some(save_error) = required_save_error {
                failures.push(format!("persist recovery-required phase: {save_error}"));
            }
            return Err(CoordinatorError::RecoveryFailures(failures.join("; ")));
        }
        let generation = journal.generation;
        *journal = RecoveryJournal::clean(generation);
        self.store.save(journal)?;
        Ok(())
    }

    async fn restore_steps_locked(
        &self,
        journal: &mut RecoveryJournal,
    ) -> Result<(), CoordinatorError> {
        // The Kill Switch is safety-critical in both directions: while an
        // active tunnel is running it prevents leaks, but once recovery has
        // started it must not be left behind merely because an unrelated
        // address, route, DNS, or adapter cleanup failed. Remove it first,
        // then best-effort every remaining step in normal reverse order.
        let mut order = journal
            .steps
            .iter()
            .enumerate()
            .filter_map(|(index, step)| {
                (step.kind == MutationKind::KillSwitch && step.state != MutationState::Restored)
                    .then_some(index)
            })
            .collect::<Vec<_>>();
        order.extend((0..journal.steps.len()).rev().filter(|index| {
            journal.steps[*index].kind != MutationKind::KillSwitch
                && journal.steps[*index].state != MutationState::Restored
        }));

        let mut failures = Vec::new();
        for index in order {
            if journal.steps[index].state == MutationState::Restored {
                continue;
            }
            let kind = journal.steps[index].kind;
            if dependency_satisfied_by_restored_wintun(journal, index) {
                journal.steps[index].state = MutationState::Restored;
                if let Err(error) = self.store.save(journal) {
                    failures.push(format!("persist {kind:?} recovery: {error}"));
                }
                continue;
            }
            match self
                .backend
                .restore_step(&journal.steps[index].receipt)
                .await
            {
                Ok(()) => {
                    journal.steps[index].state = MutationState::Restored;
                    if kind == MutationKind::PacketSession {
                        self.packet_session_attached.store(false, Ordering::Release);
                    }
                    if let Err(error) = self.store.save(journal) {
                        failures.push(format!("persist {kind:?} recovery: {error}"));
                    }
                }
                Err(error) => failures.push(format!("restore {kind:?}: {error}")),
            }
        }
        if failures.is_empty() {
            Ok(())
        } else {
            Err(CoordinatorError::RecoveryFailures(failures.join("; ")))
        }
    }
}

fn dependency_satisfied_by_restored_wintun(
    journal: &RecoveryJournal,
    dependency_index: usize,
) -> bool {
    let dependency = &journal.steps[dependency_index];
    if dependency.state == MutationState::Restored {
        return false;
    }

    journal.steps.iter().any(|adapter| {
        if adapter.kind != MutationKind::WintunAdapter || adapter.state != MutationState::Restored {
            return false;
        }
        let MutationReceipt::WintunAdapter {
            adapter_guid,
            interface_luid,
            ..
        } = &adapter.receipt
        else {
            return false;
        };
        match &dependency.receipt {
            MutationReceipt::InterfaceConfiguration {
                interface_luid: dependency_luid,
                ..
            } => dependency_luid == interface_luid,
            MutationReceipt::Dns { interface_guid, .. } => interface_guid == adapter_guid,
            _ => false,
        }
    })
}

fn ensure_clean(journal: &RecoveryJournal) -> Result<(), CoordinatorError> {
    if journal.phase == RecoveryPhase::Clean {
        Ok(())
    } else {
        Err(CoordinatorError::RecoveryRequired(journal.phase))
    }
}

fn ensure_operation_kind(
    journal: &RecoveryJournal,
    expected: OperationKind,
) -> Result<(), CoordinatorError> {
    if journal.operation_kind == Some(expected) {
        Ok(())
    } else {
        Err(CoordinatorError::OperationKind {
            expected,
            actual: journal.operation_kind,
        })
    }
}

fn ensure_owner(
    journal: &RecoveryJournal,
    operation_id: Uuid,
    caller: &AuthenticatedCaller,
) -> Result<(), CoordinatorError> {
    validate_caller(caller)?;
    if journal.operation_id != Some(operation_id) {
        return Err(CoordinatorError::OperationMismatch);
    }
    if journal.owner_sid.as_deref() != Some(caller.user_sid.as_str())
        || journal.owner_process_id != Some(caller.process_id)
    {
        return Err(CoordinatorError::OwnerMismatch);
    }
    Ok(())
}

fn validate_caller(caller: &AuthenticatedCaller) -> Result<(), CoordinatorError> {
    if caller.process_id == 0
        || !caller.user_sid.starts_with("S-")
        || caller.user_sid.len() > 256
        || !caller.user_sid[2..]
            .bytes()
            .all(|byte| byte.is_ascii_digit() || byte == b'-')
        || !caller.executable_path.is_absolute()
    {
        return Err(CoordinatorError::InvalidCaller);
    }
    Ok(())
}

#[derive(Debug, Error)]
pub enum BackendError {
    #[error("privileged backend operation failed: {0}")]
    Operation(String),
    #[error("privileged backend operation failed: {message}")]
    PartialApply {
        message: String,
        receipt: Box<MutationReceipt>,
    },
    #[error("privileged backend capability is unavailable: {0}")]
    Unavailable(String),
    #[error("no physical route to a configured WARP endpoint is available")]
    EndpointUnreachable,
    #[error("no physical route to an authenticated WARP control endpoint is available")]
    ControlApiUnreachable,
}

#[derive(Debug, Error)]
pub enum CoordinatorError {
    #[error("recovery journal failed: {0}")]
    Journal(#[from] JournalError),
    #[error(transparent)]
    Backend(#[from] BackendError),
    #[error("tunnel plan is invalid: {0}")]
    InvalidPlan(String),
    #[error("authenticated caller metadata is invalid")]
    InvalidCaller,
    #[error("agent is not clean; recovery is required from phase {0:?}")]
    RecoveryRequired(RecoveryPhase),
    #[error("agent operation ID does not match the active transaction")]
    OperationMismatch,
    #[error("agent operation kind mismatch: expected {expected:?}, got {actual:?}")]
    OperationKind {
        expected: OperationKind,
        actual: Option<OperationKind>,
    },
    #[error("only the authenticated owner process may mutate this transaction")]
    OwnerMismatch,
    #[error("agent operation requires phase {expected}, current phase is {actual:?}")]
    InvalidPhase {
        expected: &'static str,
        actual: RecoveryPhase,
    },
    #[error("packet ring capacity must be a power of two between 128 KiB and 64 MiB: {0}")]
    InvalidRingCapacity(u32),
    #[error("a packet session is already open")]
    DuplicatePacketSession,
    #[error("the active packet session is already attached to an Engine")]
    PacketSessionAlreadyAttached,
    #[error("the active packet session is not attached to an Engine")]
    PacketSessionNotAttached,
    #[error("the active tunnel already has an Engine liveness lease")]
    TunnelLeaseAlreadyAttached,
    #[error("a packet session must be open before default routes are committed")]
    PacketSessionRequired,
    #[error("privileged backend returned no duplicated packet handles")]
    MissingPacketHandles,
    #[error("journal is missing its validated tunnel plan")]
    MissingPlan,
    #[error("journal is missing applied mutation step {0:?}")]
    MissingAppliedStep(MutationKind),
    #[error("active tunnel belongs to Profile {expected}, not requested Profile {actual}")]
    ProfileMismatch { expected: Uuid, actual: Uuid },
    #[error("journal already contains mutation step {0:?}")]
    DuplicateStep(MutationKind),
    #[error("backend receipt kind mismatch: expected {expected:?}, got {actual:?}")]
    BackendReceiptMismatch {
        expected: MutationKind,
        actual: MutationKind,
    },
    #[error("apply failed ({apply}) and recovery also failed ({recovery})")]
    ApplyAndRecovery { apply: String, recovery: String },
    #[error("one or more recovery operations failed after all cleanup steps were attempted: {0}")]
    RecoveryFailures(String),
}

#[cfg(test)]
mod tests {
    use std::{
        collections::HashSet,
        fs,
        net::{Ipv4Addr, SocketAddrV4},
        path::PathBuf,
    };

    use ipnet::Ipv4Net;

    use crate::journal::RouteReceipt;

    use super::*;

    #[derive(Default)]
    struct MockBackend {
        applied: Mutex<Vec<MutationKind>>,
        restored: Mutex<Vec<MutationKind>>,
        fail_apply: Mutex<HashSet<MutationKind>>,
        fail_restore: Mutex<HashSet<MutationKind>>,
        unown_first_created_on_apply: Mutex<HashSet<MutationKind>>,
        deleted_owned_routes: Mutex<Vec<String>>,
    }

    #[async_trait]
    impl PrivilegedBackend for MockBackend {
        async fn plan_step(
            &self,
            kind: MutationKind,
            plan: &ValidatedTunnelPlan,
            _caller: &AuthenticatedCaller,
            parameter: StepParameter,
        ) -> Result<MutationReceipt, BackendError> {
            Ok(match kind {
                MutationKind::WintunAdapter => MutationReceipt::WintunAdapter {
                    adapter_name: format!("Usque-{}", &plan.profile_id.simple().to_string()[..12]),
                    adapter_guid: Uuid::new_v4(),
                    interface_luid: 7,
                },
                MutationKind::EndpointBypass => MutationReceipt::EndpointBypass {
                    created: plan
                        .endpoint_candidates
                        .iter()
                        .chain(plan.control_api_candidates.iter())
                        .map(|endpoint| {
                            route(format!(
                                "{}/{}",
                                endpoint.ip(),
                                if endpoint.is_ipv4() { 32 } else { 128 }
                            ))
                        })
                        .collect(),
                },
                MutationKind::KillSwitch => MutationReceipt::KillSwitch {
                    provider_key: Uuid::new_v4(),
                    sublayer_key: Uuid::new_v4(),
                    filter_keys: vec![Uuid::new_v4(), Uuid::new_v4()],
                    filter_ids: vec![1, 2],
                },
                MutationKind::InterfaceConfiguration => MutationReceipt::InterfaceConfiguration {
                    interface_luid: 7,
                    previous_ipv4_mtu: Some(1500),
                    previous_ipv6_mtu: Some(1500),
                    created_addresses: plan
                        .assigned_ipv4
                        .iter()
                        .chain(plan.assigned_ipv6.iter())
                        .map(|network| crate::journal::AddressReceipt {
                            address: network.to_string(),
                            owned: true,
                        })
                        .collect(),
                },
                MutationKind::Dns => MutationReceipt::Dns {
                    interface_guid: Uuid::new_v4(),
                    previous_automatic: true,
                    previous_servers: Vec::new(),
                },
                MutationKind::PacketSession => {
                    let StepParameter::PacketRing { capacity } = parameter else {
                        return Err(BackendError::Operation(
                            "packet capacity missing".to_owned(),
                        ));
                    };
                    MutationReceipt::PacketSession {
                        session_id: Uuid::new_v4(),
                        ring_capacity: capacity,
                    }
                }
                MutationKind::DefaultRoutes => MutationReceipt::DefaultRoutes {
                    created: vec![
                        RouteReceipt {
                            destination: "192.168.0.0/16".to_owned(),
                            next_hop: Some(Ipv4Addr::new(192, 0, 2, 1).into()),
                            next_hop_scope_id: 0,
                            interface_luid: 9,
                            metric: 1,
                            owned: true,
                        },
                        RouteReceipt {
                            destination: "0.0.0.0/0".to_owned(),
                            next_hop: None,
                            next_hop_scope_id: 0,
                            interface_luid: 7,
                            metric: 0,
                            owned: true,
                        },
                    ],
                    replaced: Vec::new(),
                },
                MutationKind::SystemProxy => MutationReceipt::SystemProxy {
                    user_sid: "S-1-5-21-1000".to_owned(),
                    operation_id: Uuid::new_v4(),
                    previous_proxy_enable: Some(0),
                    previous_proxy: None,
                    previous_bypass: None,
                    previous_auto_config_url: None,
                    previous_auto_detect: Some(1),
                    applied_proxy: "127.0.0.1:8080".to_owned(),
                    applied_bypass: "<local>".to_owned(),
                },
            })
        }

        async fn apply_step(
            &self,
            mut receipt: MutationReceipt,
            _plan: &ValidatedTunnelPlan,
            _caller: &AuthenticatedCaller,
        ) -> Result<(MutationReceipt, StepOutput), BackendError> {
            let kind = receipt.kind();
            if self
                .unown_first_created_on_apply
                .lock()
                .await
                .contains(&kind)
            {
                unown_first_created(&mut receipt);
            }
            if self.fail_apply.lock().await.contains(&kind) {
                return Err(BackendError::PartialApply {
                    message: format!("forced {kind:?} failure"),
                    receipt: Box::new(receipt),
                });
            }
            self.applied.lock().await.push(kind);
            let output = if let MutationReceipt::PacketSession { ring_capacity, .. } = &receipt {
                StepOutput {
                    packet_session: Some(PacketSessionHandles {
                        mapping_handle: 11,
                        engine_to_agent_event_handle: 12,
                        agent_to_engine_event_handle: 13,
                        shutdown_event_handle: 14,
                        ring_capacity: *ring_capacity,
                        layout_version: PACKET_RING_LAYOUT_VERSION,
                    }),
                }
            } else {
                StepOutput::default()
            };
            Ok((receipt, output))
        }

        async fn restore_step(&self, receipt: &MutationReceipt) -> Result<(), BackendError> {
            let kind = receipt.kind();
            self.restored.lock().await.push(kind);
            match receipt {
                MutationReceipt::EndpointBypass { created }
                | MutationReceipt::DefaultRoutes { created, .. } => {
                    self.deleted_owned_routes.lock().await.extend(
                        created
                            .iter()
                            .filter(|route| route.owned)
                            .map(|route| route.destination.clone()),
                    );
                }
                _ => {}
            }
            if self.fail_restore.lock().await.contains(&kind) {
                return Err(BackendError::Operation(format!(
                    "forced {kind:?} recovery failure"
                )));
            }
            Ok(())
        }

        async fn resume_packet_session(
            &self,
            adapter: &MutationReceipt,
            session: &MutationReceipt,
            _plan: &ValidatedTunnelPlan,
            _caller: &AuthenticatedCaller,
        ) -> Result<PacketSessionHandles, BackendError> {
            if adapter.kind() != MutationKind::WintunAdapter
                || session.kind() != MutationKind::PacketSession
            {
                return Err(BackendError::Operation(
                    "unexpected resume receipts".to_owned(),
                ));
            }
            let MutationReceipt::PacketSession { ring_capacity, .. } = session else {
                unreachable!("kind checked")
            };
            self.applied.lock().await.push(MutationKind::PacketSession);
            Ok(PacketSessionHandles {
                mapping_handle: 21,
                engine_to_agent_event_handle: 22,
                agent_to_engine_event_handle: 23,
                shutdown_event_handle: 24,
                ring_capacity: *ring_capacity,
                layout_version: PACKET_RING_LAYOUT_VERSION,
            })
        }

        async fn plan_system_proxy(
            &self,
            operation_id: Uuid,
            caller: &AuthenticatedCaller,
            settings: &SystemProxySettings,
        ) -> Result<MutationReceipt, BackendError> {
            Ok(MutationReceipt::SystemProxy {
                user_sid: caller.user_sid.clone(),
                operation_id,
                previous_proxy_enable: Some(0),
                previous_proxy: None,
                previous_bypass: Some("<local>".to_owned()),
                previous_auto_config_url: None,
                previous_auto_detect: Some(1),
                applied_proxy: settings.proxy_uri.clone(),
                applied_bypass: settings.bypass_hosts.join(";"),
            })
        }

        async fn apply_system_proxy(
            &self,
            receipt: MutationReceipt,
        ) -> Result<MutationReceipt, BackendError> {
            if self
                .fail_apply
                .lock()
                .await
                .contains(&MutationKind::SystemProxy)
            {
                return Err(BackendError::Operation(
                    "forced SystemProxy failure".to_owned(),
                ));
            }
            self.applied.lock().await.push(MutationKind::SystemProxy);
            Ok(receipt)
        }
    }

    fn unown_first_created(receipt: &mut MutationReceipt) {
        match receipt {
            MutationReceipt::EndpointBypass { created }
            | MutationReceipt::DefaultRoutes { created, .. } => {
                if let Some(route) = created.first_mut() {
                    route.owned = false;
                }
            }
            MutationReceipt::InterfaceConfiguration {
                created_addresses, ..
            } => {
                if let Some(address) = created_addresses.first_mut() {
                    address.owned = false;
                }
            }
            _ => {}
        }
    }

    fn route(destination: String) -> RouteReceipt {
        RouteReceipt {
            destination,
            next_hop: Some(Ipv4Addr::new(192, 0, 2, 1).into()),
            next_hop_scope_id: 0,
            interface_luid: 7,
            metric: 1,
            owned: true,
        }
    }

    fn plan() -> ValidatedTunnelPlan {
        ValidatedTunnelPlan {
            profile_id: Uuid::new_v4(),
            endpoint: SocketAddrV4::new(Ipv4Addr::new(162, 159, 198, 2), 443).into(),
            endpoint_candidates: vec![
                SocketAddrV4::new(Ipv4Addr::new(162, 159, 198, 2), 443).into(),
            ],
            control_api_candidates: vec![
                SocketAddrV4::new(Ipv4Addr::new(198, 51, 100, 10), 443).into(),
            ],
            mtu: 1280,
            dns_servers: vec![Ipv4Addr::new(1, 1, 1, 1).into()],
            split_exclusions: vec![
                Ipv4Net::new(Ipv4Addr::new(192, 168, 0, 0), 16)
                    .expect("network")
                    .into(),
            ],
            allow_lan: true,
            kill_switch: true,
            split_dns: false,
            assigned_ipv4: Some(
                Ipv4Net::new(Ipv4Addr::new(172, 16, 0, 2), 32)
                    .expect("assignment")
                    .into(),
            ),
            assigned_ipv6: None,
        }
    }

    fn caller() -> AuthenticatedCaller {
        #[cfg(windows)]
        let executable_path = PathBuf::from(r"C:\Program Files\Usque\usque-engine.exe");
        #[cfg(not(windows))]
        let executable_path = PathBuf::from("/opt/usque/usque-engine");

        AuthenticatedCaller {
            process_id: 42,
            user_sid: "S-1-5-21-1000".to_owned(),
            executable_path,
            process_handle: None,
        }
    }

    fn coordinator(
        backend: Arc<MockBackend>,
    ) -> (tempfile::TempDir, AgentCoordinator<MockBackend>) {
        let directory = tempfile::tempdir().expect("tempdir");
        let store = JournalStore::new(directory.path().join("recovery.json"));
        let coordinator = AgentCoordinator::open(store, backend).expect("coordinator");
        (directory, coordinator)
    }

    #[tokio::test]
    async fn two_phase_commit_removes_kill_switch_before_reverse_cleanup() {
        let backend = Arc::new(MockBackend::default());
        let (_directory, coordinator) = coordinator(Arc::clone(&backend));
        let operation = Uuid::new_v4();
        let owner = caller();

        let prepared = coordinator
            .prepare(operation, plan(), owner.clone())
            .await
            .expect("prepare");
        assert_eq!(prepared.phase, RecoveryPhase::Prepared);
        assert!(
            prepared
                .steps
                .iter()
                .all(|step| step.kind != MutationKind::KillSwitch),
            "preparation must not block physical traffic before a packet session exists"
        );
        assert!(matches!(
            coordinator.commit(operation, &owner).await,
            Err(CoordinatorError::PacketSessionRequired)
        ));
        let handles = coordinator
            .open_packet_session(operation, 1024 * 1024, &owner)
            .await
            .expect("packet session");
        assert_eq!(handles.mapping_handle, 11);
        let active = coordinator.commit(operation, &owner).await.expect("commit");
        assert_eq!(active.phase, RecoveryPhase::Active);

        coordinator
            .rollback(operation, &owner)
            .await
            .expect("rollback");
        let restored = backend.restored.lock().await.clone();
        let applied = backend.applied.lock().await.clone();
        assert_eq!(restored.first(), Some(&MutationKind::KillSwitch));
        let expected_tail = applied
            .into_iter()
            .rev()
            .filter(|kind| *kind != MutationKind::KillSwitch)
            .collect::<Vec<_>>();
        assert_eq!(restored[1..], expected_tail);
        assert_eq!(coordinator.state().await.phase, RecoveryPhase::Clean);
    }

    #[tokio::test]
    async fn already_existing_route_is_not_deleted_on_recovery() {
        let backend = Arc::new(MockBackend::default());
        backend
            .unown_first_created_on_apply
            .lock()
            .await
            .insert(MutationKind::DefaultRoutes);
        backend
            .fail_apply
            .lock()
            .await
            .insert(MutationKind::DefaultRoutes);
        let (_directory, coordinator) = coordinator(Arc::clone(&backend));
        let operation = Uuid::new_v4();
        let owner = caller();
        coordinator
            .prepare(operation, plan(), owner.clone())
            .await
            .expect("prepare");
        coordinator
            .open_packet_session(operation, 1024 * 1024, &owner)
            .await
            .expect("packet");

        assert!(matches!(
            coordinator.commit(operation, &owner).await,
            Err(CoordinatorError::Backend(_))
        ));
        assert_eq!(coordinator.state().await.phase, RecoveryPhase::Clean);
        let deleted = backend.deleted_owned_routes.lock().await.clone();
        assert!(
            !deleted
                .iter()
                .any(|destination| destination == "192.168.0.0/16"),
            "recovery must not delete a pre-existing route this generation did not create: {deleted:?}"
        );
        assert!(deleted.contains(&"0.0.0.0/0".to_owned()));
    }

    #[tokio::test]
    async fn failed_commit_recovers_every_write_ahead_step() {
        let backend = Arc::new(MockBackend::default());
        backend
            .fail_apply
            .lock()
            .await
            .insert(MutationKind::DefaultRoutes);
        let (_directory, coordinator) = coordinator(Arc::clone(&backend));
        let operation = Uuid::new_v4();
        let owner = caller();
        coordinator
            .prepare(operation, plan(), owner.clone())
            .await
            .expect("prepare");
        coordinator
            .open_packet_session(operation, 1024 * 1024, &owner)
            .await
            .expect("packet");

        assert!(matches!(
            coordinator.commit(operation, &owner).await,
            Err(CoordinatorError::Backend(_))
        ));
        assert_eq!(coordinator.state().await.phase, RecoveryPhase::Clean);
        assert_eq!(
            backend.restored.lock().await.first(),
            Some(&MutationKind::KillSwitch)
        );
    }

    #[tokio::test]
    async fn interface_failure_occurs_before_kill_switch_installation() {
        let backend = Arc::new(MockBackend::default());
        backend
            .fail_apply
            .lock()
            .await
            .insert(MutationKind::InterfaceConfiguration);
        let (_directory, coordinator) = coordinator(Arc::clone(&backend));

        assert!(
            coordinator
                .prepare(Uuid::new_v4(), plan(), caller())
                .await
                .is_err()
        );
        assert!(
            !backend
                .applied
                .lock()
                .await
                .contains(&MutationKind::KillSwitch)
        );
        assert!(
            !backend
                .restored
                .lock()
                .await
                .contains(&MutationKind::KillSwitch)
        );
        assert_eq!(coordinator.state().await.phase, RecoveryPhase::Clean);
    }

    #[tokio::test]
    async fn cleanup_failure_cannot_prevent_kill_switch_removal_or_later_attempts() {
        let backend = Arc::new(MockBackend::default());
        let (_directory, coordinator) = coordinator(Arc::clone(&backend));
        let operation = Uuid::new_v4();
        let owner = caller();
        coordinator
            .prepare(operation, plan(), owner.clone())
            .await
            .expect("prepare");
        coordinator
            .open_packet_session(operation, 1024 * 1024, &owner)
            .await
            .expect("packet");
        coordinator.commit(operation, &owner).await.expect("commit");
        backend
            .fail_restore
            .lock()
            .await
            .insert(MutationKind::InterfaceConfiguration);

        assert!(matches!(
            coordinator.rollback(operation, &owner).await,
            Err(CoordinatorError::RecoveryFailures(_))
        ));
        let restored = backend.restored.lock().await.clone();
        assert_eq!(restored.first(), Some(&MutationKind::KillSwitch));
        assert!(restored.contains(&MutationKind::WintunAdapter));
        let state = coordinator.state().await;
        assert_eq!(state.phase, RecoveryPhase::RecoveryRequired);
        assert_eq!(
            state
                .steps
                .iter()
                .find(|step| step.kind == MutationKind::KillSwitch)
                .map(|step| step.state),
            Some(MutationState::Restored)
        );
    }

    #[tokio::test]
    async fn removed_wintun_supersedes_its_stale_interface_receipt() {
        let backend = Arc::new(MockBackend::default());
        let (_directory, coordinator) = coordinator(Arc::clone(&backend));
        let operation = Uuid::new_v4();
        let owner = caller();
        coordinator
            .prepare(operation, plan(), owner.clone())
            .await
            .expect("prepare");
        backend
            .fail_restore
            .lock()
            .await
            .insert(MutationKind::InterfaceConfiguration);

        assert!(matches!(
            coordinator.rollback(operation, &owner).await,
            Err(CoordinatorError::RecoveryFailures(_))
        ));
        let state = coordinator.state().await;
        assert_eq!(state.phase, RecoveryPhase::RecoveryRequired);
        assert_eq!(
            state
                .steps
                .iter()
                .find(|step| step.kind == MutationKind::WintunAdapter)
                .map(|step| step.state),
            Some(MutationState::Restored)
        );
        let interface_attempts_before = backend
            .restored
            .lock()
            .await
            .iter()
            .filter(|kind| **kind == MutationKind::InterfaceConfiguration)
            .count();

        coordinator
            .recover_stale()
            .await
            .expect("removed adapter makes its interface receipt complete");

        assert_eq!(coordinator.state().await.phase, RecoveryPhase::Clean);
        let interface_attempts_after = backend
            .restored
            .lock()
            .await
            .iter()
            .filter(|kind| **kind == MutationKind::InterfaceConfiguration)
            .count();
        assert_eq!(interface_attempts_after, interface_attempts_before);
    }

    #[tokio::test]
    async fn startup_reconciliation_clears_journal_without_backend_mutation() {
        let backend = Arc::new(MockBackend::default());
        let (_directory, coordinator) = coordinator(Arc::clone(&backend));
        let operation = Uuid::new_v4();
        let owner = caller();
        coordinator
            .prepare(operation, plan(), owner.clone())
            .await
            .expect("prepare");
        backend
            .fail_restore
            .lock()
            .await
            .insert(MutationKind::InterfaceConfiguration);
        assert!(coordinator.rollback(operation, &owner).await.is_err());
        let restore_attempts_before = backend.restored.lock().await.len();

        assert!(
            coordinator
                .reconcile_removed_adapter_dependencies()
                .await
                .expect("journal-only reconciliation")
        );

        assert_eq!(coordinator.state().await.phase, RecoveryPhase::Clean);
        assert_eq!(backend.restored.lock().await.len(), restore_attempts_before);
    }

    #[tokio::test]
    async fn journal_write_failure_cannot_prevent_os_cleanup_attempts() {
        let backend = Arc::new(MockBackend::default());
        let (directory, coordinator) = coordinator(Arc::clone(&backend));
        let operation = Uuid::new_v4();
        let owner = caller();
        coordinator
            .prepare(operation, plan(), owner.clone())
            .await
            .expect("prepare");
        coordinator
            .open_packet_session(operation, 1024 * 1024, &owner)
            .await
            .expect("packet");
        coordinator.commit(operation, &owner).await.expect("commit");

        // Turn the journal target into a directory so every subsequent atomic
        // rename fails without affecting the temporary test filesystem.
        let journal_path = directory.path().join("recovery.json");
        fs::remove_file(&journal_path).expect("remove journal fixture");
        fs::create_dir(&journal_path).expect("block journal replacement");

        assert!(matches!(
            coordinator.rollback(operation, &owner).await,
            Err(CoordinatorError::RecoveryFailures(_))
        ));
        let restored = backend.restored.lock().await.clone();
        assert_eq!(restored.first(), Some(&MutationKind::KillSwitch));
        assert!(restored.contains(&MutationKind::WintunAdapter));
        assert!(restored.contains(&MutationKind::DefaultRoutes));
    }

    #[tokio::test]
    async fn a_new_process_cannot_take_over_an_active_transaction() {
        let backend = Arc::new(MockBackend::default());
        let (_directory, coordinator) = coordinator(backend);
        let operation = Uuid::new_v4();
        let owner = caller();
        coordinator
            .prepare(operation, plan(), owner.clone())
            .await
            .expect("prepare");
        let mut stranger = owner;
        stranger.process_id += 1;
        assert!(matches!(
            coordinator
                .open_packet_session(operation, 1024 * 1024, &stranger)
                .await,
            Err(CoordinatorError::OwnerMismatch)
        ));
    }

    #[tokio::test]
    async fn a_fresh_agent_reattaches_an_active_tunnel_for_the_same_user() {
        let directory = tempfile::tempdir().expect("tempdir");
        let journal_path = directory.path().join("recovery.json");
        let first_backend = Arc::new(MockBackend::default());
        let first =
            AgentCoordinator::open(JournalStore::new(&journal_path), Arc::clone(&first_backend))
                .expect("first coordinator");
        let operation = Uuid::new_v4();
        let tunnel_plan = plan();
        let profile_id = tunnel_plan.profile_id;
        let owner = caller();
        first
            .prepare(operation, tunnel_plan, owner.clone())
            .await
            .expect("prepare");
        first
            .open_packet_session(operation, 1024 * 1024, &owner)
            .await
            .expect("packet");
        first.commit(operation, &owner).await.expect("commit");
        assert!(first.packet_session_attached());
        drop(first);

        let restarted_backend = Arc::new(MockBackend::default());
        let restarted = AgentCoordinator::open(
            JournalStore::new(&journal_path),
            Arc::clone(&restarted_backend),
        )
        .expect("restarted coordinator");
        assert!(!restarted.packet_session_attached());
        let mut replacement = owner;
        replacement.process_id += 1;
        let handles = restarted
            .resume_tunnel(operation, profile_id, &replacement)
            .await
            .expect("resume");
        assert_eq!(handles.mapping_handle, 21);
        assert!(restarted.packet_session_attached());
        let state = restarted.state().await;
        assert_eq!(state.phase, RecoveryPhase::Active);
        assert_eq!(state.owner_process_id, Some(replacement.process_id));
        assert_eq!(
            restarted_backend.applied.lock().await.as_slice(),
            [MutationKind::PacketSession]
        );
    }

    #[tokio::test]
    async fn active_packet_detach_preserves_every_persistent_tunnel_step() {
        let backend = Arc::new(MockBackend::default());
        let (_directory, coordinator) = coordinator(Arc::clone(&backend));
        let operation = Uuid::new_v4();
        let tunnel_plan = plan();
        let profile_id = tunnel_plan.profile_id;
        let owner = caller();
        coordinator
            .prepare(operation, tunnel_plan, owner.clone())
            .await
            .expect("prepare");
        coordinator
            .open_packet_session(operation, 1024 * 1024, &owner)
            .await
            .expect("packet");
        let before = coordinator.commit(operation, &owner).await.expect("commit");

        let detached = coordinator
            .close_packet_session(operation, &owner)
            .await
            .expect("detach");
        assert_eq!(detached.phase, RecoveryPhase::Active);
        assert_eq!(detached.steps, before.steps);
        assert!(!coordinator.packet_session_attached());
        assert_eq!(
            backend.restored.lock().await.as_slice(),
            [MutationKind::PacketSession]
        );

        coordinator
            .resume_tunnel(operation, profile_id, &owner)
            .await
            .expect("reattach");
        assert!(coordinator.packet_session_attached());
    }

    #[tokio::test]
    async fn tunnel_lease_eof_detaches_only_the_volatile_packet_session() {
        let backend = Arc::new(MockBackend::default());
        let (_directory, coordinator) = coordinator(Arc::clone(&backend));
        let operation = Uuid::new_v4();
        let tunnel_plan = plan();
        let profile_id = tunnel_plan.profile_id;
        let owner = caller();
        coordinator
            .prepare(operation, tunnel_plan, owner.clone())
            .await
            .expect("prepare");
        coordinator
            .open_packet_session(operation, 1024 * 1024, &owner)
            .await
            .expect("packet");
        let active = coordinator.commit(operation, &owner).await.expect("commit");
        coordinator
            .acquire_tunnel_lease(operation, &owner)
            .await
            .expect("lease");

        coordinator
            .release_tunnel_lease(operation, &owner)
            .await
            .expect("lease EOF");
        let detached = coordinator.state().await;
        assert_eq!(detached.phase, RecoveryPhase::Active);
        assert_eq!(detached.steps, active.steps);
        assert!(!coordinator.packet_session_attached());
        assert_eq!(
            backend.restored.lock().await.as_slice(),
            [MutationKind::PacketSession]
        );
        coordinator
            .resume_tunnel(operation, profile_id, &owner)
            .await
            .expect("resume after EOF");
    }

    #[tokio::test]
    async fn startup_lease_eof_recovers_a_prepared_tunnel() {
        let backend = Arc::new(MockBackend::default());
        let (_directory, coordinator) = coordinator(Arc::clone(&backend));
        let operation = Uuid::new_v4();
        let owner = caller();
        coordinator
            .prepare(operation, plan(), owner.clone())
            .await
            .expect("prepare");

        let epoch = coordinator
            .release_startup_tunnel_lease(operation, &owner)
            .await
            .expect("startup lease EOF")
            .expect("startup watchdog armed");
        assert!(
            coordinator
                .recover_orphaned_startup_tunnel(operation, epoch)
                .await
                .expect("startup watchdog")
        );
        assert_eq!(coordinator.state().await.phase, RecoveryPhase::Clean);
    }

    #[tokio::test]
    async fn active_lease_promotion_invalidates_the_startup_watchdog() {
        let backend = Arc::new(MockBackend::default());
        let (_directory, coordinator) = coordinator(Arc::clone(&backend));
        let operation = Uuid::new_v4();
        let owner = caller();
        coordinator
            .prepare(operation, plan(), owner.clone())
            .await
            .expect("prepare");
        coordinator
            .open_packet_session(operation, 1024 * 1024, &owner)
            .await
            .expect("packet");
        coordinator.commit(operation, &owner).await.expect("commit");
        coordinator
            .acquire_tunnel_lease(operation, &owner)
            .await
            .expect("promote lease");

        let epoch = coordinator
            .release_startup_tunnel_lease(operation, &owner)
            .await
            .expect("stale startup EOF");
        assert!(epoch.is_none());
        assert_eq!(coordinator.state().await.phase, RecoveryPhase::Active);
    }

    #[tokio::test]
    async fn completed_rollback_does_not_arm_a_lease_watchdog() {
        let backend = Arc::new(MockBackend::default());
        let (_directory, coordinator) = coordinator(Arc::clone(&backend));
        let operation = Uuid::new_v4();
        let owner = caller();
        coordinator
            .prepare(operation, plan(), owner.clone())
            .await
            .expect("prepare");
        coordinator
            .open_packet_session(operation, 1024 * 1024, &owner)
            .await
            .expect("packet");
        coordinator.commit(operation, &owner).await.expect("commit");
        coordinator
            .acquire_tunnel_lease(operation, &owner)
            .await
            .expect("lease");
        coordinator
            .rollback(operation, &owner)
            .await
            .expect("rollback");

        assert!(
            coordinator
                .release_tunnel_lease(operation, &owner)
                .await
                .expect("lease EOF")
                .is_none()
        );
        assert!(
            coordinator
                .release_startup_tunnel_lease(operation, &owner)
                .await
                .expect("startup EOF")
                .is_none()
        );
    }

    #[tokio::test]
    async fn orphaned_tunnel_watchdog_recovers_only_its_lease_epoch() {
        let backend = Arc::new(MockBackend::default());
        let (_directory, coordinator) = coordinator(Arc::clone(&backend));
        let operation = Uuid::new_v4();
        let tunnel_plan = plan();
        let profile_id = tunnel_plan.profile_id;
        let owner = caller();
        coordinator
            .prepare(operation, tunnel_plan, owner.clone())
            .await
            .expect("prepare");
        coordinator
            .open_packet_session(operation, 1024 * 1024, &owner)
            .await
            .expect("packet");
        coordinator.commit(operation, &owner).await.expect("commit");
        coordinator
            .acquire_tunnel_lease(operation, &owner)
            .await
            .expect("first lease");
        let stale_epoch = coordinator
            .release_tunnel_lease(operation, &owner)
            .await
            .expect("first lease EOF")
            .expect("first watchdog armed");

        coordinator
            .resume_tunnel(operation, profile_id, &owner)
            .await
            .expect("reattach");
        coordinator
            .acquire_tunnel_lease(operation, &owner)
            .await
            .expect("replacement lease");
        let current_epoch = coordinator
            .release_tunnel_lease(operation, &owner)
            .await
            .expect("replacement lease EOF")
            .expect("replacement watchdog armed");
        assert_ne!(stale_epoch, current_epoch);
        assert!(
            !coordinator
                .recover_orphaned_tunnel(operation, stale_epoch)
                .await
                .expect("stale watchdog")
        );

        backend.restored.lock().await.clear();
        assert!(
            coordinator
                .recover_orphaned_tunnel(operation, current_epoch)
                .await
                .expect("current watchdog")
        );
        assert_eq!(
            backend.restored.lock().await.first(),
            Some(&MutationKind::KillSwitch)
        );
        assert_eq!(coordinator.state().await.phase, RecoveryPhase::Clean);
    }

    #[tokio::test]
    async fn resume_rejects_a_different_user_or_profile() {
        let directory = tempfile::tempdir().expect("tempdir");
        let journal_path = directory.path().join("recovery.json");
        let backend = Arc::new(MockBackend::default());
        let first = AgentCoordinator::open(JournalStore::new(&journal_path), Arc::clone(&backend))
            .expect("first coordinator");
        let operation = Uuid::new_v4();
        let tunnel_plan = plan();
        let profile_id = tunnel_plan.profile_id;
        let owner = caller();
        first
            .prepare(operation, tunnel_plan, owner.clone())
            .await
            .expect("prepare");
        first
            .open_packet_session(operation, 1024 * 1024, &owner)
            .await
            .expect("packet");
        first.commit(operation, &owner).await.expect("commit");
        drop(first);

        let restarted =
            AgentCoordinator::open(JournalStore::new(&journal_path), backend).expect("restart");
        let mut other_user = owner.clone();
        other_user.process_id += 1;
        other_user.user_sid = "S-1-5-21-2000".to_owned();
        assert!(matches!(
            restarted
                .resume_tunnel(operation, profile_id, &other_user)
                .await,
            Err(CoordinatorError::OwnerMismatch)
        ));
        let mut same_user = owner;
        same_user.process_id += 2;
        assert!(matches!(
            restarted
                .resume_tunnel(operation, Uuid::new_v4(), &same_user)
                .await,
            Err(CoordinatorError::ProfileMismatch { .. })
        ));
    }

    #[tokio::test]
    async fn legacy_paused_journal_is_recovered_on_startup() {
        let directory = tempfile::tempdir().expect("tempdir");
        let backend = Arc::new(MockBackend::default());
        let store = JournalStore::new(directory.path().join("journal.json"));
        let operation = Uuid::new_v4();
        let owner = caller();
        let mut legacy = RecoveryJournal {
            schema_version: crate::journal::JOURNAL_SCHEMA_VERSION,
            generation: 1,
            phase: RecoveryPhase::Paused,
            operation_kind: Some(OperationKind::Tunnel),
            operation_id: Some(operation),
            owner_sid: Some(owner.user_sid.clone()),
            owner_process_id: Some(owner.process_id),
            plan: Some(plan()),
            pause_deadline_unix_seconds: Some(1_700_000_000),
            steps: Vec::new(),
        };
        // Seed a restored-style journal like a pre-removal captive-portal pause.
        store.save(&mut legacy).expect("seed paused journal");
        let coordinator = AgentCoordinator::open(store, Arc::clone(&backend)).expect("coordinator");
        coordinator.recover_stale().await.expect("recover");
        let state = coordinator.state().await;
        assert_eq!(state.phase, RecoveryPhase::Clean);
        assert!(state.pause_deadline_unix_seconds.is_none());
    }

    #[tokio::test]
    async fn system_proxy_is_a_standalone_leased_recovery_transaction() {
        let directory = tempfile::tempdir().expect("tempdir");
        let backend = Arc::new(MockBackend::default());
        let coordinator = AgentCoordinator::open(
            JournalStore::new(directory.path().join("journal.json")),
            Arc::clone(&backend),
        )
        .expect("coordinator");
        let operation_id = Uuid::new_v4();
        let caller = caller();
        let active = coordinator
            .apply_system_proxy(
                operation_id,
                SystemProxySettings {
                    proxy_uri: "127.0.0.1:8080".to_owned(),
                    bypass_hosts: vec!["<local>".to_owned()],
                },
                caller.clone(),
            )
            .await
            .expect("apply");
        assert_eq!(active.phase, RecoveryPhase::Active);
        assert_eq!(active.operation_kind, Some(OperationKind::SystemProxy));
        assert!(active.plan.is_none());
        assert_eq!(
            backend.applied.lock().await.as_slice(),
            [MutationKind::SystemProxy]
        );

        let clean = coordinator
            .restore_system_proxy(operation_id, &caller)
            .await
            .expect("restore");
        assert_eq!(clean.phase, RecoveryPhase::Clean);
        assert_eq!(
            backend.restored.lock().await.as_slice(),
            [MutationKind::SystemProxy]
        );
    }

    #[tokio::test]
    async fn system_proxy_can_join_and_leave_an_active_tunnel_transaction() {
        let backend = Arc::new(MockBackend::default());
        let (_directory, coordinator) = coordinator(Arc::clone(&backend));
        let operation = Uuid::new_v4();
        let owner = caller();
        coordinator
            .prepare(operation, plan(), owner.clone())
            .await
            .expect("prepare");
        coordinator
            .open_packet_session(operation, 1024 * 1024, &owner)
            .await
            .expect("packet");
        coordinator.commit(operation, &owner).await.expect("commit");

        let with_proxy = coordinator
            .apply_system_proxy(
                operation,
                SystemProxySettings {
                    proxy_uri: "127.0.0.1:8080".to_owned(),
                    bypass_hosts: vec!["<local>".to_owned()],
                },
                owner.clone(),
            )
            .await
            .expect("apply system proxy");
        assert_eq!(with_proxy.phase, RecoveryPhase::Active);
        assert_eq!(with_proxy.operation_kind, Some(OperationKind::Tunnel));
        assert!(with_proxy.steps.iter().any(|step| {
            step.kind == MutationKind::SystemProxy && step.state == MutationState::Applied
        }));

        let without_proxy = coordinator
            .restore_system_proxy(operation, &owner)
            .await
            .expect("restore system proxy");
        assert_eq!(without_proxy.phase, RecoveryPhase::Active);
        assert_eq!(without_proxy.operation_kind, Some(OperationKind::Tunnel));
        assert!(
            without_proxy
                .steps
                .iter()
                .all(|step| step.kind != MutationKind::SystemProxy)
        );
        assert!(without_proxy.steps.iter().any(|step| {
            step.kind == MutationKind::KillSwitch && step.state == MutationState::Applied
        }));
    }

    #[tokio::test]
    async fn restoring_sidecar_system_proxy_is_one_journal_mutation() {
        let backend = Arc::new(MockBackend::default());
        let (_directory, coordinator, operation, owner) = active_tunnel(Arc::clone(&backend)).await;
        coordinator
            .apply_system_proxy(
                operation,
                SystemProxySettings {
                    proxy_uri: "127.0.0.1:8080".to_owned(),
                    bypass_hosts: vec!["<local>".to_owned()],
                },
                owner.clone(),
            )
            .await
            .expect("apply system proxy");
        let generation = coordinator.state().await.generation;

        let restored = coordinator
            .restore_system_proxy(operation, &owner)
            .await
            .expect("restore");
        assert_eq!(restored.phase, RecoveryPhase::Active);
        assert_eq!(restored.generation, generation + 1);
        assert!(
            restored
                .steps
                .iter()
                .all(|step| step.kind != MutationKind::SystemProxy)
        );
        coordinator
            .apply_system_proxy(
                operation,
                SystemProxySettings {
                    proxy_uri: "127.0.0.1:8080".to_owned(),
                    bypass_hosts: vec!["<local>".to_owned()],
                },
                owner.clone(),
            )
            .await
            .expect("re-apply after one-save restore");
    }

    #[tokio::test]
    async fn restored_sidecar_system_proxy_is_absent_for_reapply() {
        let directory = tempfile::tempdir().expect("tempdir");
        let journal_path = directory.path().join("recovery.json");
        let backend = Arc::new(MockBackend::default());
        let coordinator =
            AgentCoordinator::open(JournalStore::new(&journal_path), Arc::clone(&backend))
                .expect("coordinator");
        let operation = Uuid::new_v4();
        let owner = caller();
        coordinator
            .prepare(operation, plan(), owner.clone())
            .await
            .expect("prepare");
        coordinator
            .open_packet_session(operation, 1024 * 1024, &owner)
            .await
            .expect("packet");
        coordinator.commit(operation, &owner).await.expect("commit");
        coordinator
            .apply_system_proxy(
                operation,
                SystemProxySettings {
                    proxy_uri: "127.0.0.1:8080".to_owned(),
                    bypass_hosts: vec!["<local>".to_owned()],
                },
                owner.clone(),
            )
            .await
            .expect("apply system proxy");
        let mut journal = coordinator.state().await;
        journal
            .steps
            .iter_mut()
            .find(|step| step.kind == MutationKind::SystemProxy)
            .expect("proxy step")
            .state = MutationState::Restored;
        drop(coordinator);
        JournalStore::new(&journal_path)
            .save(&mut journal)
            .expect("seed restored system-proxy step");

        let coordinator =
            AgentCoordinator::open(JournalStore::new(&journal_path), Arc::clone(&backend))
                .expect("reload");
        let reapplied = coordinator
            .apply_system_proxy(
                operation,
                SystemProxySettings {
                    proxy_uri: "127.0.0.1:8080".to_owned(),
                    bypass_hosts: vec!["<local>".to_owned()],
                },
                owner.clone(),
            )
            .await
            .expect("restored sidecar steps are absent");
        assert_eq!(reapplied.phase, RecoveryPhase::Active);
        assert_eq!(
            reapplied
                .steps
                .iter()
                .filter(|step| step.kind == MutationKind::SystemProxy)
                .count(),
            1
        );
        assert_eq!(
            reapplied
                .steps
                .iter()
                .find(|step| step.kind == MutationKind::SystemProxy)
                .map(|step| step.state),
            Some(MutationState::Applied)
        );
    }

    async fn active_tunnel(
        backend: Arc<MockBackend>,
    ) -> (
        tempfile::TempDir,
        AgentCoordinator<MockBackend>,
        Uuid,
        AuthenticatedCaller,
    ) {
        let (directory, coordinator) = coordinator(backend);
        let operation = Uuid::new_v4();
        let owner = caller();
        coordinator
            .prepare(operation, plan(), owner.clone())
            .await
            .expect("prepare");
        coordinator
            .open_packet_session(operation, 1024 * 1024, &owner)
            .await
            .expect("packet");
        coordinator.commit(operation, &owner).await.expect("commit");
        (directory, coordinator, operation, owner)
    }

    #[tokio::test]
    async fn sidecar_system_proxy_restore_failure_keeps_the_tunnel_active() {
        let backend = Arc::new(MockBackend::default());
        let (_directory, coordinator, operation, owner) = active_tunnel(Arc::clone(&backend)).await;
        coordinator
            .apply_system_proxy(
                operation,
                SystemProxySettings {
                    proxy_uri: "127.0.0.1:8080".to_owned(),
                    bypass_hosts: vec!["<local>".to_owned()],
                },
                owner.clone(),
            )
            .await
            .expect("apply system proxy");
        backend
            .fail_restore
            .lock()
            .await
            .insert(MutationKind::SystemProxy);

        assert!(matches!(
            coordinator.restore_system_proxy(operation, &owner).await,
            Err(CoordinatorError::Backend(_))
        ));
        let state = coordinator.state().await;
        assert_eq!(state.phase, RecoveryPhase::Active);
        assert!(state.steps.iter().any(|step| {
            step.kind == MutationKind::KillSwitch && step.state == MutationState::Applied
        }));
        assert!(state.steps.iter().any(|step| {
            step.kind == MutationKind::SystemProxy && step.state == MutationState::Applied
        }));
        assert_eq!(
            backend
                .restored
                .lock()
                .await
                .iter()
                .filter(|kind| **kind == MutationKind::SystemProxy)
                .count(),
            SYSTEM_PROXY_RESTORE_ATTEMPTS as usize
        );
    }

    #[tokio::test]
    async fn sidecar_system_proxy_apply_rollback_failure_keeps_the_tunnel_active() {
        let backend = Arc::new(MockBackend::default());
        let (_directory, coordinator, operation, owner) = active_tunnel(Arc::clone(&backend)).await;
        backend
            .fail_apply
            .lock()
            .await
            .insert(MutationKind::SystemProxy);
        backend
            .fail_restore
            .lock()
            .await
            .insert(MutationKind::SystemProxy);

        assert!(matches!(
            coordinator
                .apply_system_proxy(
                    operation,
                    SystemProxySettings {
                        proxy_uri: "127.0.0.1:8080".to_owned(),
                        bypass_hosts: vec!["<local>".to_owned()],
                    },
                    owner.clone(),
                )
                .await,
            Err(CoordinatorError::ApplyAndRecovery { .. })
        ));
        let state = coordinator.state().await;
        assert_eq!(state.phase, RecoveryPhase::Active);
        assert!(state.steps.iter().any(|step| {
            step.kind == MutationKind::KillSwitch && step.state == MutationState::Applied
        }));
        assert!(
            state
                .steps
                .iter()
                .any(|step| step.kind == MutationKind::SystemProxy)
        );
    }

    #[tokio::test]
    async fn closing_a_prepared_packet_session_is_one_journal_mutation() {
        let backend = Arc::new(MockBackend::default());
        let (_directory, coordinator) = coordinator(Arc::clone(&backend));
        let operation = Uuid::new_v4();
        let owner = caller();
        coordinator
            .prepare(operation, plan(), owner.clone())
            .await
            .expect("prepare");
        coordinator
            .open_packet_session(operation, 1024 * 1024, &owner)
            .await
            .expect("packet");
        let generation = coordinator.state().await.generation;

        let closed = coordinator
            .close_packet_session(operation, &owner)
            .await
            .expect("close");
        assert_eq!(closed.phase, RecoveryPhase::Prepared);
        assert_eq!(closed.generation, generation + 1);
        assert!(
            closed
                .steps
                .iter()
                .all(|step| step.kind != MutationKind::PacketSession)
        );
        coordinator
            .open_packet_session(operation, 1024 * 1024, &owner)
            .await
            .expect("reopen");
        coordinator.commit(operation, &owner).await.expect("commit");
    }

    #[tokio::test]
    async fn restored_packet_session_is_absent_for_open_and_commit() {
        let directory = tempfile::tempdir().expect("tempdir");
        let journal_path = directory.path().join("recovery.json");
        let backend = Arc::new(MockBackend::default());
        let coordinator =
            AgentCoordinator::open(JournalStore::new(&journal_path), Arc::clone(&backend))
                .expect("coordinator");
        let operation = Uuid::new_v4();
        let owner = caller();
        coordinator
            .prepare(operation, plan(), owner.clone())
            .await
            .expect("prepare");
        coordinator
            .open_packet_session(operation, 1024 * 1024, &owner)
            .await
            .expect("packet");
        let mut journal = coordinator.state().await;
        journal
            .steps
            .iter_mut()
            .find(|step| step.kind == MutationKind::PacketSession)
            .expect("packet step")
            .state = MutationState::Restored;
        drop(coordinator);
        JournalStore::new(&journal_path)
            .save(&mut journal)
            .expect("seed restored packet step");

        let coordinator =
            AgentCoordinator::open(JournalStore::new(&journal_path), Arc::clone(&backend))
                .expect("reload");
        coordinator
            .open_packet_session(operation, 1024 * 1024, &owner)
            .await
            .expect("restored packet steps are absent");
        let committed = coordinator.commit(operation, &owner).await.expect("commit");
        assert_eq!(
            committed
                .steps
                .iter()
                .filter(|step| step.kind == MutationKind::PacketSession)
                .count(),
            1
        );
        assert_eq!(
            committed
                .steps
                .iter()
                .find(|step| step.kind == MutationKind::PacketSession)
                .map(|step| step.state),
            Some(MutationState::Applied)
        );
    }

    #[tokio::test]
    async fn packet_session_restore_failure_keeps_the_tunnel_active() {
        let backend = Arc::new(MockBackend::default());
        let (_directory, coordinator, operation, owner) = active_tunnel(Arc::clone(&backend)).await;
        backend
            .fail_restore
            .lock()
            .await
            .insert(MutationKind::PacketSession);

        assert!(matches!(
            coordinator.close_packet_session(operation, &owner).await,
            Err(CoordinatorError::Backend(_))
        ));
        let state = coordinator.state().await;
        assert_eq!(state.phase, RecoveryPhase::Active);
        assert!(state.steps.iter().any(|step| {
            step.kind == MutationKind::KillSwitch && step.state == MutationState::Applied
        }));
        assert!(coordinator.packet_session_attached());
    }
}
