//! Fail-closed transaction ordering for Darwin. Disconnect is explicit; losing
//! the Engine connection must never schedule removal of an active guard.

use serde::{Deserialize, Serialize};
use std::io;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Owner {
    pub uid: u32,
    pub pid: u32,
    pub pid_version: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Fence {
    pub session: Uuid,
    pub configuration: u64,
    pub network: u64,
}
impl TryFrom<usque_ipc::agent_v1::MacosFence> for Fence {
    type Error = io::Error;
    fn try_from(value: usque_ipc::agent_v1::MacosFence) -> io::Result<Self> {
        let session =
            Uuid::parse_str(&value.session_id).map_err(|_| denied("invalid session fence"))?;
        if session.is_nil() || value.configuration == 0 || value.network == 0 {
            return Err(denied("invalid session fence"));
        }
        Ok(Self {
            session,
            configuration: value.configuration,
            network: value.network,
        })
    }
}
impl From<Fence> for usque_ipc::agent_v1::MacosFence {
    fn from(value: Fence) -> Self {
        Self {
            session_id: value.session.to_string(),
            configuration: value.configuration,
            network: value.network,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Step {
    Protection,
    Egress,
    Utun,
    Dns,
    Routes,
}
const STEPS: [Step; 5] = [
    Step::Protection,
    Step::Egress,
    Step::Utun,
    Step::Dns,
    Step::Routes,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Preparing,
    Active,
    Retained,
    Recovering,
    RecoveryRequired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepState {
    Intended,
    Applied,
    Restored,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Record {
    pub step: Step,
    pub state: StepState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Transaction {
    pub owner: Owner,
    pub fence: Fence,
    pub kill_switch: bool,
    pub phase: Phase,
    pub records: Vec<Record>,
    #[serde(default)]
    pub transition: Option<Transition>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Transition {
    pub kill_switch: bool,
    pub owner: Owner,
    pub fence: Fence,
}

/// The store must persist each complete intent durably before returning.
/// Native receipts belong in the same durable store; a backend may not return
/// success until actual state has been read back and matches those receipts.
pub trait Store {
    fn save(&mut self, transaction: Option<&Transaction>) -> io::Result<()>;
}

/// Implementations compare receipts before restoring. No step may remove
/// resources belonging to a different transaction or another application.
pub trait Backend {
    /// Stop shared file descriptions without relying on disk or PF availability.
    fn stop_io(&mut self) -> io::Result<()> {
        Ok(())
    }
    fn apply_and_verify(&mut self, fence: Fence, step: Step) -> io::Result<()>;
    fn quiesce_and_revoke_egress(&mut self, fence: Fence) -> io::Result<()>;
    fn restore_and_verify(&mut self, fence: Fence, step: Step) -> io::Result<()>;
    fn rebase(&mut self, _previous: Fence, _next: Fence) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "guarded resume unavailable",
        ))
    }
}

pub struct Coordinator<S, B> {
    store: S,
    backend: B,
    transaction: Option<Transaction>,
    persistence_uncertain: bool,
}

impl<S: Store, B: Backend> Coordinator<S, B> {
    pub fn new(store: S, backend: B, transaction: Option<Transaction>) -> io::Result<Self> {
        if let Some(value) = &transaction {
            value.validate()?;
        }
        Ok(Self {
            store,
            backend,
            transaction,
            persistence_uncertain: false,
        })
    }
    pub fn state(&self) -> Option<&Transaction> {
        self.transaction.as_ref()
    }
    pub fn persistence_uncertain(&self) -> bool {
        self.persistence_uncertain
    }
    pub fn backend(&self) -> &B {
        &self.backend
    }
    pub fn backend_mut(&mut self) -> &mut B {
        &mut self.backend
    }
    /// Establish the guard and physical-egress boundary before the Engine
    /// creates its single remote data plane. System routes/DNS are deferred.
    pub fn prepare(&mut self, owner: Owner, fence: Fence, kill_switch: bool) -> io::Result<()> {
        self.require_certain()?;
        if self.transaction.is_some() {
            return Err(denied("system TUN already owned"));
        }
        let next = Transaction {
            owner,
            fence,
            kill_switch,
            phase: Phase::Preparing,
            records: Vec::new(),
            transition: None,
        };
        next.validate()?;
        self.transaction = Some(next);
        self.persist()?;
        for step in &STEPS[..2] {
            self.apply(*step)?;
        }
        Ok(())
    }
    /// Called only after remote negotiation has supplied validated interface
    /// settings. The backend must already hold that validated configuration.
    pub fn configure(&mut self, owner: Owner, fence: Fence) -> io::Result<()> {
        self.authorize(owner, fence)?;
        let t = self.transaction.as_ref().expect("authorized transaction");
        if t.phase != Phase::Preparing
            || t.records.len() != 2
            || t.records.iter().any(|r| r.state != StepState::Applied)
        {
            return Err(denied("tunnel is not ready for interface configuration"));
        }
        for step in &STEPS[2..] {
            self.apply(*step)?;
        }
        Ok(())
    }
    /// Engine acknowledgment that its packet pump is ready. Applying settings
    /// alone is never evidence that the data plane is connected.
    pub fn commit(&mut self, owner: Owner, fence: Fence) -> io::Result<()> {
        self.authorize(owner, fence)?;
        let t = self.transaction.as_mut().expect("authorized transaction");
        if t.phase != Phase::Preparing || !t.fully_applied() {
            return Err(denied("incomplete tunnel"));
        }
        t.phase = Phase::Active;
        self.persist()
    }
    pub fn owner_lost(&mut self, owner: Owner, fence: Fence) -> io::Result<()> {
        if self
            .transaction
            .as_ref()
            .is_none_or(|t| t.owner != owner || t.fence != fence)
        {
            return Err(denied("lost owner mismatch"));
        }
        let _ = self.backend.stop_io();
        self.authorize(owner, fence)?;
        let Some(t) = self.transaction.as_mut() else {
            return Ok(());
        };
        if !t.kill_switch && t.transition.is_none() {
            return self.recover_as_administrator();
        }
        let fence = t.fence;
        if !matches!(t.phase, Phase::Recovering | Phase::RecoveryRequired) {
            t.phase = Phase::Retained;
        }
        self.persist()?;
        if let Err(error) = self.backend.quiesce_and_revoke_egress(fence) {
            self.failed();
            return Err(error);
        }
        Ok(())
    }
    pub fn disconnect(&mut self, owner: Owner, fence: Fence) -> io::Result<()> {
        self.authorize(owner, fence)?;
        self.recover_as_administrator()
    }
    pub fn suspend(&mut self, owner: Owner, fence: Fence) -> io::Result<()> {
        self.authorize(owner, fence)?;
        let _ = self.backend.stop_io();
        let t = self.transaction.as_mut().expect("authorized transaction");
        if !matches!(t.phase, Phase::Active | Phase::Preparing) {
            return Err(denied("session cannot be suspended"));
        }
        t.phase = Phase::Retained;
        self.persist()?;
        if let Err(error) = self.backend.quiesce_and_revoke_egress(fence) {
            self.failed();
            return Err(error);
        }
        Ok(())
    }
    /// A newly authenticated installed Engine may complete its user's explicit
    /// recovery, but cannot disconnect another user's or still-owned session.
    pub fn disconnect_retained(&mut self, owner: Owner, fence: Fence) -> io::Result<()> {
        self.require_certain()?;
        let t = self
            .transaction
            .as_ref()
            .ok_or_else(|| denied("no retained session"))?;
        if t.owner.uid != owner.uid
            || t.fence != fence
            || !matches!(
                t.phase,
                Phase::Retained | Phase::RecoveryRequired | Phase::Recovering
            )
        {
            return Err(denied("retained owner or fence mismatch"));
        }
        self.recover_as_administrator()
    }
    /// Adopt a retained session for the same user without removing its guard.
    /// The pending fence makes both sides of a two-journal handoff recoverable.
    pub fn resume(
        &mut self,
        owner: Owner,
        previous: Fence,
        next: Fence,
        kill_switch: bool,
    ) -> io::Result<()> {
        self.require_certain()?;
        let t = self
            .transaction
            .as_mut()
            .ok_or_else(|| denied("no retained session"))?;
        if t.owner.uid != owner.uid
            || t.fence != previous
            || !matches!(t.phase, Phase::Retained | Phase::RecoveryRequired)
            || next.session != previous.session
            || next.configuration <= previous.configuration
            || next.network < previous.network
            || t.records
                .first()
                .is_none_or(|r| r.state != StepState::Applied)
        {
            return Err(denied("retained owner or fence mismatch"));
        }
        t.phase = Phase::Recovering;
        t.transition = Some(Transition {
            owner,
            fence: next,
            kill_switch,
        });
        self.persist()?;
        if let Err(error) = self.backend.quiesce_and_revoke_egress(previous) {
            self.failed();
            return Err(error);
        }
        let count = self.transaction.as_ref().unwrap().records.len();
        for index in (1..count).rev() {
            let record = self.transaction.as_ref().unwrap().records[index].clone();
            if record.state != StepState::Restored {
                if let Err(error) = self.backend.restore_and_verify(previous, record.step) {
                    self.failed();
                    return Err(error);
                }
                self.transaction.as_mut().unwrap().records[index].state = StepState::Restored;
                self.persist()?;
            }
        }
        if let Err(error) = self.backend.rebase(previous, next) {
            self.failed();
            return Err(error);
        }
        let t = self.transaction.as_mut().unwrap();
        t.fence = next;
        t.kill_switch = kill_switch;
        t.owner = owner;
        t.transition = None;
        t.phase = Phase::Preparing;
        t.records.truncate(1);
        self.persist()?;
        self.apply(Step::Egress)
    }
    /// Only the authenticated owner's explicit Disconnect or an administrator
    /// maintenance entry point may call this. Never call from service Drop,
    /// a disconnect timer or an unprivileged IPC recovery request.
    pub fn recover_as_administrator(&mut self) -> io::Result<()> {
        let _ = self.backend.stop_io();
        self.require_certain()?;
        let Some(t) = self.transaction.as_mut() else {
            return Ok(());
        };
        let fence = t.fence;
        t.phase = Phase::Recovering;
        self.persist()?;
        if let Err(error) = self.backend.quiesce_and_revoke_egress(fence) {
            self.failed();
            return Err(error);
        }
        let count = self
            .transaction
            .as_ref()
            .expect("recovering transaction")
            .records
            .len();
        for index in (0..count).rev() {
            let record = &self
                .transaction
                .as_ref()
                .expect("recovering transaction")
                .records[index];
            if record.state == StepState::Restored {
                continue;
            }
            if let Err(error) = self.backend.restore_and_verify(fence, record.step) {
                if record.step == Step::Routes {
                    // DNS receipts are independent and record each verified service.
                    // Keep the route transaction pending and the guard in place.
                    // An explicit retry will finish the normal reverse order.
                    if let Err(dns_error) = self.backend.restore_and_verify(fence, Step::Dns) {
                        self.failed();
                        return Err(io::Error::new(
                            error.kind(),
                            format!("route recovery: {error}; DNS recovery: {dns_error}"),
                        ));
                    }
                }
                self.failed();
                return Err(error);
            }
            self.transaction
                .as_mut()
                .expect("recovering transaction")
                .records[index]
                .state = StepState::Restored;
            self.persist()?;
        }
        // Keep the in-memory recovery record if publishing Clean is uncertain.
        if let Err(error) = self.store.save(None) {
            self.persistence_uncertain = true;
            self.transaction
                .as_mut()
                .expect("recovering transaction")
                .phase = Phase::RecoveryRequired;
            return Err(error);
        }
        self.transaction = None;
        Ok(())
    }
    fn apply(&mut self, step: Step) -> io::Result<()> {
        let t = self.transaction.as_mut().expect("preparing transaction");
        let fence = t.fence;
        t.records.push(Record {
            step,
            state: StepState::Intended,
        });
        self.persist()?;
        if let Err(error) = self.backend.apply_and_verify(fence, step) {
            self.failed();
            return Err(error);
        }
        self.transaction
            .as_mut()
            .expect("preparing transaction")
            .records
            .last_mut()
            .expect("intent")
            .state = StepState::Applied;
        self.persist()
    }
    fn failed(&mut self) {
        if let Some(t) = self.transaction.as_mut() {
            t.phase = Phase::RecoveryRequired;
        }
        // The original operation error remains primary, but persistence status
        // independently prevents any subsequent mutation until durable reload.
        let _ = self.persist();
    }
    fn persist(&mut self) -> io::Result<()> {
        if let Some(t) = &self.transaction {
            t.validate()?;
        }
        if let Err(error) = self.store.save(self.transaction.as_ref()) {
            self.persistence_uncertain = true;
            if let Some(t) = self.transaction.as_mut() {
                t.phase = Phase::RecoveryRequired;
            }
            return Err(error);
        }
        Ok(())
    }
    fn require_certain(&self) -> io::Result<()> {
        if self.persistence_uncertain {
            return Err(denied("recovery journal requires durable reload"));
        }
        Ok(())
    }
    fn authorize(&self, owner: Owner, fence: Fence) -> io::Result<()> {
        self.require_certain()?;
        if !self
            .transaction
            .as_ref()
            .is_some_and(|t| t.owner == owner && t.fence == fence)
        {
            return Err(denied("owner or session generation mismatch"));
        }
        Ok(())
    }
}

impl Transaction {
    fn fully_applied(&self) -> bool {
        self.records.len() == STEPS.len()
            && self.records.iter().all(|r| r.state == StepState::Applied)
    }
    pub fn validate(&self) -> io::Result<()> {
        if self.owner.uid == 0
            || self.owner.pid == 0
            || self.owner.pid_version == 0
            || self.fence.session.is_nil()
            || self.fence.configuration == 0
            || self.fence.network == 0
            || self.records.len() > STEPS.len()
            || self.records.iter().zip(STEPS).any(|(r, s)| r.step != s)
            || (self.phase == Phase::Active && !self.fully_applied())
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid macOS transaction",
            ));
        }
        if let Some(next) = &self.transition
            && (next.owner.uid != self.owner.uid
                || next.owner.pid == 0
                || next.owner.pid_version == 0
                || next.fence.session != self.fence.session
                || next.fence.configuration <= self.fence.configuration
                || next.fence.network < self.fence.network
                || !matches!(self.phase, Phase::Recovering | Phase::RecoveryRequired))
        {
            return Err(denied("invalid pending fence transition"));
        }
        let mut incomplete = false;
        for record in &self.records {
            if incomplete && record.state != StepState::Restored {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "invalid macOS step ordering",
                ));
            }
            incomplete |= record.state != StepState::Applied;
        }
        if matches!(
            self.phase,
            Phase::Active | Phase::Preparing | Phase::Retained
        ) && self.records.iter().any(|r| r.state == StepState::Restored)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "restored macOS transaction cannot be active",
            ));
        }
        Ok(())
    }
}

fn denied(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::PermissionDenied, message)
}

#[cfg(test)]
mod tests;
