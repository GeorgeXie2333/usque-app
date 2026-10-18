use std::{io, os::fd::OwnedFd};

use uuid::Uuid;

use crate::plan::ValidatedTunnelPlan;

use super::journal::{
    Journal, JournalStore, Owner, Phase, Record, Replacement, Resources, Step, StepState,
};

/// Every backend mutation has an intent already on disk. Implementations may
/// only touch resources derived from the operation UUID and verified ownership.
pub trait Backend: Send {
    fn preflight(&self, plan: &ValidatedTunnelPlan) -> io::Result<()>;
    fn take_reusable_egress_mark(&mut self) -> Option<u16> {
        None
    }
    fn protection_active(&self, _resources: &Resources, _plan: &ValidatedTunnelPlan) -> bool {
        false
    }
    fn initialize_resources(&self, _resources: &mut Resources) -> io::Result<()> {
        Ok(())
    }
    fn apply(
        &mut self,
        step: Step,
        resources: &mut Resources,
        plan: &ValidatedTunnelPlan,
    ) -> io::Result<()>;
    fn restore(
        &mut self,
        step: Step,
        resources: &Resources,
        plan: &ValidatedTunnelPlan,
    ) -> io::Result<()>;
    fn duplicate_tun(&self, resources: &Resources) -> io::Result<OwnedFd>;
    fn install_bridge(
        &mut self,
        resources: &mut Resources,
        plan: &ValidatedTunnelPlan,
    ) -> io::Result<()> {
        self.apply(Step::Protection, resources, plan)
    }
    fn invalidate_network(&mut self, _resources: &Resources) -> io::Result<()> {
        Ok(())
    }
}

pub struct Coordinator<B> {
    backend: B,
    store: JournalStore,
    journal: Journal,
    persistence_uncertain: bool,
}

impl<B: Backend> Coordinator<B> {
    pub fn open(store: JournalStore, backend: B) -> io::Result<Self> {
        let journal = store.load()?;
        Ok(Self {
            backend,
            store,
            journal,
            persistence_uncertain: false,
        })
    }

    pub fn state(&self) -> &Journal {
        &self.journal
    }
    pub fn backend_mut(&mut self) -> &mut B {
        &mut self.backend
    }

    pub fn protection_active(&self) -> bool {
        if let Some(replacement) = &self.journal.replacement
            && self
                .backend
                .protection_active(&replacement.resources, &replacement.plan)
        {
            return true;
        }
        self.journal
            .resources
            .as_ref()
            .zip(self.journal.plan.as_ref())
            .is_some_and(|(resources, plan)| self.backend.protection_active(resources, plan))
    }

    pub fn abort_replacement(
        &mut self,
        owner: &Owner,
        target: Uuid,
        generation: u64,
    ) -> io::Result<()> {
        let operation = self
            .journal
            .operation_id
            .ok_or_else(|| denied("Linux replacement is absent"))?;
        self.check_owner(owner, operation)?;
        if generation != self.journal.generation
            || (operation != target
                && self
                    .journal
                    .replacement
                    .as_ref()
                    .is_none_or(|replacement| replacement.operation_id != target))
        {
            return Err(denied("Linux replacement abort identity changed"));
        }
        self.recover()
    }

    pub fn invalidate_physical_network(&mut self) -> io::Result<()> {
        if self.persistence_uncertain {
            return Err(denied("Linux recovery journal requires reconciliation"));
        }
        let resources = self
            .journal
            .resources
            .as_ref()
            .ok_or_else(|| denied("Linux network is absent"))?;
        self.backend.invalidate_network(resources)?;
        let mut next = self.journal.clone();
        let resources = next.resources.as_mut().expect("active resources");
        resources.network_generation = resources
            .network_generation
            .checked_add(1)
            .ok_or_else(|| denied("Linux network generation exhausted"))?;
        self.persist(next)
    }

    pub fn update_physical_snapshot(
        &mut self,
        interfaces: Vec<super::journal::PhysicalInterface>,
    ) -> io::Result<()> {
        if self.persistence_uncertain {
            return Err(denied("Linux recovery journal requires reconciliation"));
        }
        let Some(resources) = &self.journal.resources else {
            return Ok(());
        };
        if resources.physical_interfaces == interfaces {
            return Ok(());
        }
        self.backend.invalidate_network(resources)?;
        let mut next = self.journal.clone();
        let resources = next.resources.as_mut().expect("active resources");
        resources.network_generation = resources
            .network_generation
            .checked_add(1)
            .ok_or_else(|| denied("Linux network generation exhausted"))?;
        resources.physical_interfaces = interfaces;
        self.persist(next)
    }

    pub fn prepare(
        &mut self,
        owner: Owner,
        operation_id: Uuid,
        generation: u64,
        plan: ValidatedTunnelPlan,
    ) -> io::Result<()> {
        if self.persistence_uncertain
            || operation_id.is_nil()
            || generation != self.journal.generation
        {
            return Err(denied("stale Linux operation generation"));
        }
        if self.journal.phase != Phase::Clean {
            return Err(denied("another Linux tunnel or recovery owns this host"));
        }
        plan.validate()
            .map_err(|_| denied("invalid Linux tunnel plan"))?;
        self.backend.preflight(&plan)?;
        let mut resources = Resources::for_operation(operation_id);
        resources.network_generation = self
            .journal
            .generation
            .checked_add(1)
            .ok_or_else(|| denied("Linux network generation exhausted"))?;
        self.backend.initialize_resources(&mut resources)?;
        let next = Journal {
            schema_version: super::journal::SCHEMA_VERSION,
            generation: self.journal.generation,
            phase: Phase::Preparing,
            operation_id: Some(operation_id),
            owner: Some(owner),
            plan: Some(plan),
            resources: Some(resources),
            records: Vec::new(),
            replacement: None,
            replacement_origin: None,
            replacement_initial_plan: None,
            automatic_recovery: Default::default(),
        };
        self.persist(next)?;
        let deferred = self
            .journal
            .plan
            .as_ref()
            .expect("validated plan")
            .defer_network_configuration;
        for step in [Step::Protection, Step::Tun, Step::Addresses, Step::Dns]
            .into_iter()
            .take(if deferred { 2 } else { 4 })
        {
            if let Err(error) = self.apply(step) {
                // Keep the original operation evidence even if a subsequent
                // persistence or cleanup operation also fails.
                self.record_failure();
                return Err(error);
            }
        }
        let mut next = self.journal.clone();
        next.phase = Phase::Prepared;
        self.persist(next)
    }

    pub fn commit(&mut self, owner: &Owner, operation_id: Uuid) -> io::Result<()> {
        self.check_owner(owner, operation_id)?;
        if self.journal.phase == Phase::Active {
            return Ok(());
        }
        if self.journal.phase != Phase::Prepared {
            return Err(denied("Linux tunnel is not prepared"));
        }
        if self
            .journal
            .plan
            .as_ref()
            .is_some_and(|plan| plan.defer_network_configuration)
        {
            return Err(denied(
                "Linux chain network must be finalized before commit",
            ));
        }
        if let Err(error) = self.apply(Step::Routes) {
            self.record_failure();
            return Err(error);
        }
        let mut next = self.journal.clone();
        next.phase = Phase::Active;
        self.persist(next)
    }

    pub fn packet_descriptor(&self, owner: &Owner, operation_id: Uuid) -> io::Result<OwnedFd> {
        self.check_owner(owner, operation_id)?;
        if !matches!(self.journal.phase, Phase::Prepared | Phase::Active) {
            return Err(denied("Linux TUN is not ready"));
        }
        self.backend.duplicate_tun(
            self.journal
                .resources
                .as_ref()
                .expect("validated resources"),
        )
    }

    pub fn finalize(
        &mut self,
        owner: &Owner,
        operation_id: Uuid,
        plan: ValidatedTunnelPlan,
    ) -> io::Result<()> {
        self.check_owner(owner, operation_id)?;
        plan.validate()
            .map_err(|_| denied("invalid Linux final network"))?;
        if self.journal.phase != Phase::Prepared {
            return Err(denied("Linux chain is not prepared"));
        }
        let bootstrap = self.journal.plan.as_ref().expect("validated plan");
        if !bootstrap.defer_network_configuration {
            return if bootstrap == &plan {
                Ok(())
            } else {
                Err(denied("Linux network is already finalized"))
            };
        }
        let mut allowed = bootstrap.clone();
        allowed.assigned_ipv4 = plan.assigned_ipv4;
        allowed.assigned_ipv6 = plan.assigned_ipv6;
        allowed.dns_servers.clone_from(&plan.dns_servers);
        allowed.mtu = plan.mtu;
        allowed.defer_network_configuration = false;
        if allowed != plan {
            return Err(denied(
                "Linux chain bootstrap policy cannot change during finalization",
            ));
        }
        let mut next = self.journal.clone();
        next.plan = Some(plan);
        next.phase = Phase::Preparing;
        self.persist(next)?;
        for step in [Step::Addresses, Step::Dns] {
            if let Err(error) = self.apply(step) {
                self.record_failure();
                return Err(error);
            }
        }
        let mut next = self.journal.clone();
        next.phase = Phase::Prepared;
        self.persist(next)
    }

    pub fn rollback(&mut self, owner: &Owner, operation_id: Uuid) -> io::Result<()> {
        self.check_owner(owner, operation_id)?;
        self.recover()
    }

    pub fn begin_chain_transition(&mut self, owner: &Owner, operation_id: Uuid) -> io::Result<()> {
        self.check_owner(owner, operation_id)?;
        if !matches!(self.journal.phase, Phase::Prepared | Phase::Active)
            || self
                .journal
                .plan
                .as_ref()
                .is_none_or(|plan| !plan.vpn_chain)
        {
            return Err(denied("Linux chain transition is unavailable"));
        }
        if self
            .journal
            .plan
            .as_ref()
            .is_some_and(|plan| plan.defer_network_configuration)
        {
            return Ok(());
        }
        let mut next = self.journal.clone();
        next.phase = Phase::Recovering;
        self.persist(next)?;
        for index in (2..self.journal.records.len()).rev() {
            let record = &self.journal.records[index];
            if record.state == StepState::Restored {
                continue;
            }
            if let Err(error) = self.backend.restore(
                record.step,
                self.journal.resources.as_ref().expect("active resources"),
                self.journal.plan.as_ref().expect("active plan"),
            ) {
                let mut next = self.journal.clone();
                next.phase = Phase::RecoveryRequired;
                let _ = self.persist(next);
                return Err(error);
            }
            let mut next = self.journal.clone();
            next.records[index].state = StepState::Restored;
            self.persist(next)?;
        }
        let mut next = self.journal.clone();
        next.records.truncate(2);
        next.phase = Phase::Prepared;
        next.plan
            .as_mut()
            .expect("active plan")
            .defer_network_configuration = true;
        let resources = next.resources.as_mut().expect("active resources");
        let previous = resources.physical_interfaces.clone();
        self.backend.initialize_resources(resources)?;
        if previous != resources.physical_interfaces {
            self.backend.invalidate_network(resources)?;
            resources.network_generation = resources
                .network_generation
                .checked_add(1)
                .ok_or_else(|| denied("Linux network generation exhausted"))?;
        }
        self.persist(next)
    }

    /// Install and journal the target guard before removing any source state.
    /// Failure deliberately retains its guard until an explicit rollback or
    /// service recovery; it never falls back to an unprotected prepare.
    pub fn replace(
        &mut self,
        owner: &Owner,
        source: Uuid,
        operation_id: Uuid,
        generation: u64,
        plan: ValidatedTunnelPlan,
    ) -> io::Result<()> {
        if self.journal.operation_id == Some(operation_id) {
            self.check_owner(owner, operation_id)?;
            if self.journal.replacement_origin != Some(source)
                || self.journal.replacement_initial_plan.as_ref() != Some(&plan)
            {
                return Err(denied("Linux replacement target policy changed"));
            }
            if matches!(self.journal.phase, Phase::Prepared | Phase::Active) {
                return Ok(());
            }
            if generation != self.journal.generation
                || !matches!(
                    self.journal.phase,
                    Phase::Preparing | Phase::Recovering | Phase::RecoveryRequired
                )
            {
                return Err(denied("Linux replacement retry generation changed"));
            }
            self.backend
                .invalidate_network(self.journal.resources.as_ref().expect("target resources"))?;
            let mut next = self.journal.clone();
            next.phase = Phase::Recovering;
            self.persist(next)?;
            for index in (1..self.journal.records.len()).rev() {
                let record = &self.journal.records[index];
                if record.state == StepState::Restored {
                    continue;
                }
                self.backend.restore(
                    record.step,
                    self.journal.resources.as_ref().expect("target resources"),
                    self.journal.plan.as_ref().expect("target plan"),
                )?;
                let mut next = self.journal.clone();
                next.records[index].state = StepState::Restored;
                self.persist(next)?;
            }
            let mut next = self.journal.clone();
            next.phase = Phase::Preparing;
            next.plan = Some(plan.clone());
            next.records.truncate(1);
            let resources = next.resources.as_mut().expect("target resources");
            resources.interface_index = None;
            self.backend.initialize_resources(resources)?;
            resources.network_generation = resources
                .network_generation
                .checked_add(1)
                .ok_or_else(|| denied("Linux network generation exhausted"))?;
            self.persist(next)?;
            return self.prepare_target_steps(&plan);
        }
        self.check_owner(owner, source)?;
        if operation_id.is_nil() || operation_id == source || generation != self.journal.generation
        {
            return Err(denied("stale Linux replacement identity or generation"));
        }
        plan.validate()
            .map_err(|_| denied("invalid Linux replacement plan"))?;
        self.backend.preflight(&plan)?;
        if let Some(replacement) = &self.journal.replacement {
            if replacement.operation_id != operation_id || replacement.plan != plan {
                return Err(denied("another Linux replacement already owns protection"));
            }
        } else {
            if !matches!(self.journal.phase, Phase::Prepared | Phase::Active) {
                return Err(denied("Linux replacement source is not ready"));
            }
            let mut resources = Resources::for_operation(operation_id);
            resources.network_generation = self
                .journal
                .resources
                .as_ref()
                .expect("source resources")
                .network_generation
                .checked_add(1)
                .ok_or_else(|| denied("Linux network generation exhausted"))?;
            let mut next = self.journal.clone();
            next.phase = Phase::Recovering;
            next.replacement = Some(Replacement {
                operation_id,
                plan: plan.clone(),
                resources,
                state: StepState::Intended,
            });
            self.persist(next)?;
        }
        let mut replacement = self
            .journal
            .replacement
            .clone()
            .expect("replacement intent");
        if replacement.state != StepState::Applied {
            self.backend
                .install_bridge(&mut replacement.resources, &replacement.plan)?;
            replacement.state = StepState::Applied;
            let mut next = self.journal.clone();
            next.replacement = Some(replacement.clone());
            self.persist(next)?;
        }
        if let Some(resources) = &self.journal.resources {
            self.backend.invalidate_network(resources)?;
        }
        for index in (0..self.journal.records.len()).rev() {
            let record = &self.journal.records[index];
            if record.state == StepState::Restored {
                continue;
            }
            if let Err(error) = self.backend.restore(
                record.step,
                self.journal.resources.as_ref().expect("source resources"),
                self.journal.plan.as_ref().expect("source plan"),
            ) {
                let mut next = self.journal.clone();
                next.phase = Phase::RecoveryRequired;
                let _ = self.persist(next);
                return Err(error);
            }
            let mut next = self.journal.clone();
            next.records[index].state = StepState::Restored;
            self.persist(next)?;
        }
        self.backend
            .initialize_resources(&mut replacement.resources)?;
        self.persist(Journal {
            generation: self.journal.generation,
            phase: Phase::Preparing,
            operation_id: Some(operation_id),
            owner: Some(owner.clone()),
            plan: Some(plan.clone()),
            resources: Some(replacement.resources),
            records: vec![Record {
                step: Step::Protection,
                state: StepState::Applied,
            }],
            replacement_origin: Some(source),
            replacement_initial_plan: Some(plan.clone()),
            ..Journal::default()
        })?;
        self.prepare_target_steps(&plan)
    }

    fn prepare_target_steps(&mut self, plan: &ValidatedTunnelPlan) -> io::Result<()> {
        for step in [Step::Tun, Step::Addresses, Step::Dns].into_iter().take(
            if plan.defer_network_configuration {
                1
            } else {
                3
            },
        ) {
            if let Err(error) = self.apply(step) {
                self.record_failure();
                return Err(error);
            }
        }
        let mut next = self.journal.clone();
        next.phase = Phase::Prepared;
        self.persist(next)
    }

    fn record_failure(&mut self) {
        if self.journal.replacement_origin.is_some() || self.journal.replacement.is_some() {
            let mut next = self.journal.clone();
            next.phase = Phase::RecoveryRequired;
            let _ = self.persist(next);
        } else {
            let _ = self.recover();
        }
    }

    pub fn arm_automatic_recovery(&mut self) -> io::Result<bool> {
        use super::journal::AutomaticPhase;
        if self.persistence_uncertain || self.journal.phase != Phase::RecoveryRequired {
            return Ok(false);
        }
        if matches!(
            self.journal.automatic_recovery.phase,
            AutomaticPhase::Exhausted | AutomaticPhase::Blocked
        ) {
            return Ok(false);
        }
        if self.journal.automatic_recovery.phase != AutomaticPhase::Waiting {
            let mut next = self.journal.clone();
            next.automatic_recovery.phase =
                if next.automatic_recovery.attempts >= super::journal::RECOVERY_ATTEMPT_LIMIT {
                    AutomaticPhase::Exhausted
                } else {
                    AutomaticPhase::Waiting
                };
            self.persist(next)?;
        }
        Ok(self.journal.automatic_recovery.phase == AutomaticPhase::Waiting)
    }

    pub fn automatic_recover(&mut self, operation: Uuid, generation: u64) -> io::Result<()> {
        use super::journal::AutomaticPhase;
        if self.persistence_uncertain
            || self.journal.operation_id != Some(operation)
            || self.journal.generation != generation
            || self.journal.phase != Phase::RecoveryRequired
            || self.journal.automatic_recovery.phase != AutomaticPhase::Waiting
            || self.journal.automatic_recovery.attempts >= super::journal::RECOVERY_ATTEMPT_LIMIT
        {
            return Err(denied(
                "Linux automatic recovery identity or budget changed",
            ));
        }
        let mut next = self.journal.clone();
        next.automatic_recovery.attempts += 1;
        next.automatic_recovery.phase = AutomaticPhase::Running;
        self.persist(next)?;
        match self.recover() {
            Ok(()) => Ok(()),
            Err(error) => {
                // Journal uncertainty is reconciled only by recovery, never by
                // saving possibly stale bookkeeping over the durable evidence.
                if !self.persistence_uncertain {
                    let mut next = self.journal.clone();
                    next.phase = Phase::RecoveryRequired;
                    next.automatic_recovery.phase = if matches!(
                        error.kind(),
                        io::ErrorKind::PermissionDenied
                            | io::ErrorKind::InvalidData
                            | io::ErrorKind::Unsupported
                    ) {
                        AutomaticPhase::Blocked
                    } else if next.automatic_recovery.attempts
                        >= super::journal::RECOVERY_ATTEMPT_LIMIT
                    {
                        AutomaticPhase::Exhausted
                    } else {
                        AutomaticPhase::Waiting
                    };
                    let _ = self.persist(next);
                }
                Err(error)
            }
        }
    }

    pub fn restart_automatic_recovery(
        &mut self,
        uid: u32,
        operation: Uuid,
        generation: u64,
        owner_alive: bool,
    ) -> io::Result<()> {
        if self.persistence_uncertain
            || self.journal.operation_id != Some(operation)
            || self.journal.generation != generation
            || self.journal.phase != Phase::RecoveryRequired
            || self
                .journal
                .owner
                .as_ref()
                .is_none_or(|owner| owner.uid != uid)
            || owner_alive
        {
            return Err(denied("Linux recovery restart identity or owner changed"));
        }
        let mut next = self.journal.clone();
        next.automatic_recovery = super::journal::AutomaticRecovery {
            phase: super::journal::AutomaticPhase::Waiting,
            attempts: 0,
        };
        self.persist(next)
    }

    pub fn recover_orphaned(
        &mut self,
        uid: u32,
        operation_id: Uuid,
        generation: u64,
        owner_alive: bool,
    ) -> io::Result<()> {
        if self.journal.operation_id != Some(operation_id)
            || self.journal.generation != generation
            || self
                .journal
                .owner
                .as_ref()
                .is_none_or(|owner| owner.uid != uid)
            || owner_alive
        {
            return Err(denied("Linux recovery identity or generation changed"));
        }
        self.recover()
    }

    /// Only service startup/shutdown or the separately authorized maintenance
    /// process may use unguarded recovery. Never expose this directly over IPC.
    pub fn recover(&mut self) -> io::Result<()> {
        if self.persistence_uncertain {
            self.journal = self.store.load()?;
            self.persistence_uncertain = false;
        }
        if self.journal.phase == Phase::Clean {
            return Ok(());
        }
        let mut next = self.journal.clone();
        next.phase = Phase::Recovering;
        self.persist(next)?;
        for index in (0..self.journal.records.len()).rev() {
            let record = &self.journal.records[index];
            if record.state == StepState::Restored {
                continue;
            }
            let result = self.backend.restore(
                record.step,
                self.journal
                    .resources
                    .as_ref()
                    .expect("validated resources"),
                self.journal.plan.as_ref().expect("validated plan"),
            );
            if let Err(error) = result {
                let mut next = self.journal.clone();
                next.phase = Phase::RecoveryRequired;
                let _ = self.persist(next);
                // Never continue to remove the guard after a failed dependent
                // cleanup, including Intended records from an interrupted call.
                return Err(error);
            }
            let mut next = self.journal.clone();
            next.records[index].state = StepState::Restored;
            self.persist(next)?;
        }
        if let Some(replacement) = &self.journal.replacement {
            self.backend
                .restore(Step::Protection, &replacement.resources, &replacement.plan)?;
        }
        self.persist(Journal {
            generation: self.journal.generation,
            ..Journal::default()
        })
    }

    pub fn check_owner(&self, owner: &Owner, operation_id: Uuid) -> io::Result<()> {
        if self.persistence_uncertain
            || self.journal.owner.as_ref() != Some(owner)
            || self.journal.operation_id != Some(operation_id)
        {
            return Err(denied(
                "Linux operation is owned by another process or user",
            ));
        }
        Ok(())
    }

    fn apply(&mut self, step: Step) -> io::Result<()> {
        let mut next = self.journal.clone();
        next.phase = Phase::Preparing;
        next.records.push(Record {
            step,
            state: StepState::Intended,
        });
        self.persist(next)?;
        let mut resources = self.journal.resources.clone().expect("validated resources");
        self.backend.apply(
            step,
            &mut resources,
            self.journal.plan.as_ref().expect("validated plan"),
        )?;
        let mut next = self.journal.clone();
        next.resources = Some(resources);
        next.records.last_mut().expect("step persisted").state = StepState::Applied;
        self.persist(next)
    }

    fn persist(&mut self, mut next: Journal) -> io::Result<()> {
        if let Err(error) = self.store.save(&mut next) {
            self.persistence_uncertain = true;
            return Err(error);
        }
        self.journal = next;
        Ok(())
    }

    pub fn reserve_egress_mark(
        &mut self,
        owner: &Owner,
        operation_id: Uuid,
        generation: u64,
    ) -> io::Result<(Resources, u32)> {
        self.check_owner(owner, operation_id)?;
        if self
            .journal
            .resources
            .as_ref()
            .is_none_or(|resources| resources.network_generation != generation)
            || !matches!(self.journal.phase, Phase::Prepared | Phase::Active)
        {
            return Err(denied("Linux egress generation or phase changed"));
        }
        let resources = self
            .journal
            .resources
            .as_ref()
            .expect("validated resources");
        if let Some(slot) = self.backend.take_reusable_egress_mark() {
            if slot == 0 || slot > resources.egress_mark_highwater {
                return Err(denied("invalid recycled Linux mark"));
            }
            return Ok((resources.clone(), resources.socket_mark | u32::from(slot)));
        }
        let mut next = self.journal.clone();
        let resources = next.resources.as_mut().expect("validated resources");
        resources.egress_mark_highwater = resources
            .egress_mark_highwater
            .checked_add(1)
            .ok_or_else(|| denied("Linux egress marks exhausted"))?;
        let allocated = resources.clone();
        self.persist(next)?;
        let mark = allocated.socket_mark | u32::from(allocated.egress_mark_highwater);
        Ok((allocated, mark))
    }
}

fn denied(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::PermissionDenied, message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        os::unix::fs::PermissionsExt,
        sync::{Arc, Mutex},
    };

    struct Fake {
        calls: Arc<Mutex<Vec<(Step, bool)>>>,
        fail_apply: Option<Step>,
        fail_restore: Option<Step>,
        restore_error: io::ErrorKind,
        path: std::path::PathBuf,
        reusable_marks: Vec<u16>,
    }

    impl Backend for Fake {
        fn take_reusable_egress_mark(&mut self) -> Option<u16> {
            self.reusable_marks.pop()
        }
        fn preflight(&self, _: &ValidatedTunnelPlan) -> io::Result<()> {
            Ok(())
        }
        fn apply(
            &mut self,
            step: Step,
            resources: &mut Resources,
            _: &ValidatedTunnelPlan,
        ) -> io::Result<()> {
            let persisted = JournalStore::for_test(&self.path).load().unwrap();
            assert_eq!(
                persisted.records.last(),
                Some(&Record {
                    step,
                    state: StepState::Intended
                })
            );
            self.calls.lock().unwrap().push((step, true));
            if self.fail_apply == Some(step) {
                return Err(io::Error::other("injected apply failure"));
            }
            if step == Step::Tun {
                resources.interface_index = Some(42);
            }
            Ok(())
        }
        fn restore(
            &mut self,
            step: Step,
            _: &Resources,
            _: &ValidatedTunnelPlan,
        ) -> io::Result<()> {
            self.calls.lock().unwrap().push((step, false));
            if self.fail_restore == Some(step) {
                return Err(io::Error::new(
                    self.restore_error,
                    "injected restore failure",
                ));
            }
            Ok(())
        }
        fn install_bridge(
            &mut self,
            resources: &mut Resources,
            _: &ValidatedTunnelPlan,
        ) -> io::Result<()> {
            let persisted = JournalStore::for_test(&self.path).load().unwrap();
            let replacement = persisted
                .replacement
                .as_ref()
                .expect("durable bridge intent");
            assert_eq!(&replacement.resources, resources);
            assert_eq!(replacement.state, StepState::Intended);
            self.calls.lock().unwrap().push((Step::Protection, true));
            if self.fail_apply == Some(Step::Protection) {
                return Err(io::Error::other("injected bridge failure"));
            }
            Ok(())
        }
        fn duplicate_tun(&self, _: &Resources) -> io::Result<OwnedFd> {
            Err(io::Error::other("inert backend"))
        }
    }

    fn owner() -> Owner {
        Owner {
            uid: 1000,
            process_id: 100,
            process_start_time: 123,
        }
    }
    fn plan() -> ValidatedTunnelPlan {
        ValidatedTunnelPlan {
            profile_id: Uuid::new_v4(),
            endpoint: "162.159.192.1:443".parse().unwrap(),
            endpoint_candidates: vec!["162.159.192.1:443".parse().unwrap()],
            control_api_candidates: Vec::new(),
            mtu: 1280,
            dns_servers: vec!["1.1.1.1".parse().unwrap()],
            split_exclusions: Vec::new(),
            allow_lan: false,
            kill_switch: true,
            split_dns: false,
            assigned_ipv4: Some("172.16.0.2/32".parse().unwrap()),
            assigned_ipv6: None,
            vpn_chain: false,
            defer_network_configuration: false,
            automatic_endpoint_policy: None,
            preset_endpoint_policy: None,
        }
    }

    fn setup(
        fail_apply: Option<Step>,
        fail_restore: Option<Step>,
    ) -> (tempfile::TempDir, Coordinator<Fake>) {
        let directory = tempfile::tempdir().unwrap();
        fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let path = directory.path().join("journal.json");
        let backend = Fake {
            calls: Arc::default(),
            fail_apply,
            fail_restore,
            restore_error: io::ErrorKind::Other,
            path: path.clone(),
            reusable_marks: Vec::new(),
        };
        let coordinator = Coordinator::open(JournalStore::for_test(path), backend).unwrap();
        (directory, coordinator)
    }

    #[test]
    fn every_partial_apply_rolls_back_in_reverse_after_durable_intent() {
        for failed in [
            Step::Protection,
            Step::Tun,
            Step::Addresses,
            Step::Dns,
            Step::Routes,
        ] {
            let (_directory, mut coordinator) = setup(Some(failed), None);
            let id = Uuid::new_v4();
            let result = coordinator.prepare(owner(), id, 0, plan());
            if failed == Step::Routes {
                result.unwrap();
                assert!(coordinator.commit(&owner(), id).is_err());
            } else {
                assert!(result.is_err());
            }
            assert_eq!(coordinator.state().phase, Phase::Clean);
            let calls = coordinator.backend.calls.lock().unwrap();
            let applied: Vec<_> = calls
                .iter()
                .filter(|(_, apply)| *apply)
                .map(|(step, _)| *step)
                .collect();
            let restored: Vec<_> = calls
                .iter()
                .filter(|(_, apply)| !apply)
                .map(|(step, _)| *step)
                .collect();
            assert_eq!(restored, applied.into_iter().rev().collect::<Vec<_>>());
        }
    }

    #[test]
    fn failed_cleanup_keeps_protection_and_retry_evidence() {
        let (_directory, mut coordinator) = setup(Some(Step::Dns), Some(Step::Addresses));
        assert!(
            coordinator
                .prepare(owner(), Uuid::new_v4(), 0, plan())
                .is_err()
        );
        assert_eq!(coordinator.state().phase, Phase::RecoveryRequired);
        assert!(
            !coordinator
                .backend
                .calls
                .lock()
                .unwrap()
                .contains(&(Step::Protection, false))
        );
        coordinator.backend.fail_restore = None;
        coordinator.recover().unwrap();
        assert_eq!(coordinator.state().phase, Phase::Clean);
    }

    #[test]
    fn other_user_stale_generation_and_live_owner_cannot_take_over() {
        let (_directory, mut coordinator) = setup(None, None);
        let id = Uuid::new_v4();
        coordinator.prepare(owner(), id, 0, plan()).unwrap();
        let mut other = owner();
        other.uid += 1;
        assert!(coordinator.rollback(&other, id).is_err());
        assert!(
            coordinator
                .prepare(
                    other,
                    Uuid::new_v4(),
                    coordinator.state().generation,
                    plan()
                )
                .is_err()
        );
        assert!(coordinator.recover_orphaned(1000, id, 0, false).is_err());
        assert!(
            coordinator
                .recover_orphaned(1001, id, coordinator.state().generation, false)
                .is_err()
        );
        assert!(
            coordinator
                .recover_orphaned(1000, id, coordinator.state().generation, true)
                .is_err()
        );
        coordinator.commit(&owner(), id).unwrap();
        coordinator.rollback(&owner(), id).unwrap();
        assert_eq!(coordinator.state().phase, Phase::Clean);
    }
    #[test]
    fn deferred_chain_finalizes_exact_policy_and_transitions_under_guard() {
        let (_directory, mut coordinator) = setup(None, None);
        let id = Uuid::new_v4();
        let mut bootstrap = plan();
        bootstrap.vpn_chain = true;
        bootstrap.defer_network_configuration = true;
        coordinator
            .prepare(owner(), id, 0, bootstrap.clone())
            .unwrap();
        assert_eq!(coordinator.state().records.len(), 2);
        assert!(coordinator.commit(&owner(), id).is_err());
        let mut final_plan = bootstrap.clone();
        final_plan.defer_network_configuration = false;
        final_plan.assigned_ipv4 = Some("10.80.0.2/24".parse().unwrap());
        let mut changed = final_plan.clone();
        changed.allow_lan = true;
        assert!(coordinator.finalize(&owner(), id, changed).is_err());
        coordinator
            .finalize(&owner(), id, final_plan.clone())
            .unwrap();
        coordinator.finalize(&owner(), id, final_plan).unwrap();
        coordinator.commit(&owner(), id).unwrap();
        coordinator.begin_chain_transition(&owner(), id).unwrap();
        assert_eq!(coordinator.state().phase, Phase::Prepared);
        assert_eq!(coordinator.state().records.len(), 2);
        assert!(
            !coordinator
                .backend
                .calls
                .lock()
                .unwrap()
                .iter()
                .any(|(step, apply)| !apply && matches!(step, Step::Protection | Step::Tun))
        );
    }

    #[test]
    fn physical_generation_is_independent_of_permit_journal_writes() {
        let (directory, mut coordinator) = setup(None, None);
        let id = Uuid::new_v4();
        coordinator.prepare(owner(), id, 0, plan()).unwrap();
        let generation = coordinator
            .state()
            .resources
            .as_ref()
            .unwrap()
            .network_generation;
        for highwater in 1..=3 {
            let (resources, _) = coordinator
                .reserve_egress_mark(&owner(), id, generation)
                .unwrap();
            assert_eq!(resources.egress_mark_highwater, highwater);
            assert_eq!(resources.network_generation, generation);
        }
        let saved = JournalStore::for_test(directory.path().join("journal.json"))
            .load()
            .unwrap();
        assert_eq!(saved.resources.unwrap().egress_mark_highwater, 3);
        coordinator.invalidate_physical_network().unwrap();
        assert!(
            coordinator
                .reserve_egress_mark(&owner(), id, generation)
                .is_err()
        );
        assert_eq!(
            coordinator
                .reserve_egress_mark(&owner(), id, generation + 1)
                .unwrap()
                .0
                .egress_mark_highwater,
            4
        );
    }

    #[test]
    fn replacement_installs_durable_bridge_before_source_cleanup() {
        let (_directory, mut coordinator) = setup(None, None);
        let source = Uuid::new_v4();
        let target = Uuid::new_v4();
        coordinator.prepare(owner(), source, 0, plan()).unwrap();
        coordinator.commit(&owner(), source).unwrap();
        coordinator.backend.calls.lock().unwrap().clear();
        let target_plan = plan();
        coordinator
            .replace(
                &owner(),
                source,
                target,
                coordinator.state().generation,
                target_plan.clone(),
            )
            .unwrap();
        let calls = coordinator.backend.calls.lock().unwrap().clone();
        assert_eq!(calls[0], (Step::Protection, true));
        assert_eq!(calls[1], (Step::Routes, false));
        assert_eq!(
            calls
                .iter()
                .filter(|call| **call == (Step::Protection, false))
                .count(),
            1
        );
        assert_eq!(coordinator.state().operation_id, Some(target));
        coordinator
            .replace(&owner(), source, target, 0, target_plan.clone())
            .unwrap();
        coordinator.commit(&owner(), target).unwrap();
        coordinator
            .replace(&owner(), source, target, 0, target_plan)
            .unwrap();
        coordinator.rollback(&owner(), target).unwrap();
        assert_eq!(coordinator.state().phase, Phase::Clean);
    }

    #[test]
    fn every_replacement_failure_retains_a_guard_until_retry_or_abort() {
        for failed in [
            Step::Protection,
            Step::Tun,
            Step::Addresses,
            Step::Dns,
            Step::Routes,
        ] {
            let (_directory, mut coordinator) = setup(None, None);
            let source = Uuid::new_v4();
            let target = Uuid::new_v4();
            coordinator.prepare(owner(), source, 0, plan()).unwrap();
            coordinator.commit(&owner(), source).unwrap();
            coordinator.backend.calls.lock().unwrap().clear();
            coordinator.backend.fail_apply = Some(failed);
            let target_plan = plan();
            let result = coordinator.replace(
                &owner(),
                source,
                target,
                coordinator.state().generation,
                target_plan.clone(),
            );
            if failed == Step::Routes {
                result.unwrap();
                assert!(coordinator.commit(&owner(), target).is_err());
            } else {
                assert!(result.is_err());
            }
            assert_ne!(coordinator.state().phase, Phase::Clean);
            let restores = coordinator
                .backend
                .calls
                .lock()
                .unwrap()
                .iter()
                .filter(|call| **call == (Step::Protection, false))
                .count();
            assert_eq!(restores, usize::from(failed != Step::Protection));
            coordinator.backend.fail_apply = None;
            coordinator
                .replace(
                    &owner(),
                    source,
                    target,
                    coordinator.state().generation,
                    target_plan,
                )
                .unwrap();
            assert_eq!(coordinator.state().phase, Phase::Prepared);
            coordinator
                .abort_replacement(&owner(), target, coordinator.state().generation)
                .unwrap();
            assert_eq!(coordinator.state().phase, Phase::Clean);
        }
    }

    #[test]
    fn replacement_source_cleanup_failure_is_owned_and_explicitly_abortable() {
        let (_directory, mut coordinator) = setup(None, None);
        let source = Uuid::new_v4();
        let target = Uuid::new_v4();
        coordinator.prepare(owner(), source, 0, plan()).unwrap();
        coordinator.commit(&owner(), source).unwrap();
        coordinator.backend.fail_restore = Some(Step::Dns);
        assert!(
            coordinator
                .replace(
                    &owner(),
                    source,
                    target,
                    coordinator.state().generation,
                    plan()
                )
                .is_err()
        );
        assert!(coordinator.state().replacement.is_some());
        assert_eq!(coordinator.state().operation_id, Some(source));
        let mut other = owner();
        other.uid += 1;
        assert!(
            coordinator
                .abort_replacement(&other, target, coordinator.state().generation)
                .is_err()
        );
        assert!(coordinator.abort_replacement(&owner(), target, 0).is_err());
        coordinator.backend.fail_restore = None;
        coordinator
            .abort_replacement(&owner(), target, coordinator.state().generation)
            .unwrap();
        assert_eq!(coordinator.state().phase, Phase::Clean);
    }
    #[test]
    fn recycled_marks_keep_durable_highwater_and_do_not_spend_new_slots() {
        let (_directory, mut coordinator) = setup(None, None);
        let id = Uuid::new_v4();
        coordinator.prepare(owner(), id, 0, plan()).unwrap();
        let network_generation = coordinator
            .state()
            .resources
            .as_ref()
            .unwrap()
            .network_generation;
        let (resources, first_mark) = coordinator
            .reserve_egress_mark(&owner(), id, network_generation)
            .unwrap();
        assert_eq!(resources.egress_mark_highwater, 1);
        let generation = coordinator.state().generation;
        for _ in 0..100_000 {
            coordinator.backend.reusable_marks.push(1);
            let (resources, reused) = coordinator
                .reserve_egress_mark(&owner(), id, network_generation)
                .unwrap();
            assert_eq!(reused, first_mark);
            assert_eq!(resources.egress_mark_highwater, 1);
        }
        assert_eq!(coordinator.state().generation, generation);
        coordinator.backend.reusable_marks.push(2);
        assert!(
            coordinator
                .reserve_egress_mark(&owner(), id, network_generation)
                .is_err()
        );
    }

    #[test]
    fn automatic_recovery_is_generation_scoped_bounded_and_keeps_failed_guard() {
        use super::super::journal::AutomaticPhase;
        let (_directory, mut coordinator) = setup(Some(Step::Dns), Some(Step::Addresses));
        let id = Uuid::new_v4();
        assert!(coordinator.prepare(owner(), id, 0, plan()).is_err());
        assert!(coordinator.arm_automatic_recovery().unwrap());
        let generation = coordinator.state().generation;
        assert!(
            coordinator
                .automatic_recover(Uuid::new_v4(), generation)
                .is_err()
        );
        assert!(coordinator.automatic_recover(id, generation - 1).is_err());
        assert_eq!(coordinator.state().automatic_recovery.attempts, 0);
        for attempt in 1..=3 {
            let generation = coordinator.state().generation;
            assert!(coordinator.automatic_recover(id, generation).is_err());
            assert_eq!(coordinator.state().automatic_recovery.attempts, attempt);
            assert_eq!(coordinator.state().phase, Phase::RecoveryRequired);
            assert!(
                !coordinator
                    .backend
                    .calls
                    .lock()
                    .unwrap()
                    .contains(&(Step::Protection, false))
            );
        }
        assert_eq!(
            coordinator.state().automatic_recovery.phase,
            AutomaticPhase::Exhausted
        );
        assert!(!coordinator.arm_automatic_recovery().unwrap());
        let generation = coordinator.state().generation;
        assert!(coordinator.automatic_recover(id, generation).is_err());
        assert!(
            coordinator
                .restart_automatic_recovery(1001, id, generation, false)
                .is_err()
        );
        assert!(
            coordinator
                .restart_automatic_recovery(1000, id, generation, true)
                .is_err()
        );
        coordinator
            .restart_automatic_recovery(1000, id, generation, false)
            .unwrap();
        coordinator.backend.fail_restore = None;
        coordinator
            .automatic_recover(id, coordinator.state().generation)
            .unwrap();
        assert_eq!(coordinator.state().phase, Phase::Clean);
        assert_eq!(
            coordinator.state().automatic_recovery.phase,
            AutomaticPhase::Inactive
        );
    }
    #[test]
    fn automatic_recovery_blocks_ownership_errors_without_spending_more_attempts() {
        use super::super::journal::AutomaticPhase;
        let (_directory, mut coordinator) = setup(Some(Step::Dns), Some(Step::Addresses));
        let id = Uuid::new_v4();
        assert!(coordinator.prepare(owner(), id, 0, plan()).is_err());
        coordinator.backend.restore_error = io::ErrorKind::PermissionDenied;
        coordinator.arm_automatic_recovery().unwrap();
        assert!(
            coordinator
                .automatic_recover(id, coordinator.state().generation)
                .is_err()
        );
        assert_eq!(
            coordinator.state().automatic_recovery.phase,
            AutomaticPhase::Blocked
        );
        assert_eq!(coordinator.state().automatic_recovery.attempts, 1);
        assert!(!coordinator.arm_automatic_recovery().unwrap());
    }
}
