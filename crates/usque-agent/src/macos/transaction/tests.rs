use super::*;
use std::{cell::RefCell, rc::Rc};

#[derive(Default)]
struct Evidence {
    persisted: Option<Transaction>,
    events: Vec<String>,
    fail_restore: Option<Step>,
    fail_save: bool,
    fail_apply: Option<Step>,
    stopped: usize,
    fail_stop: bool,
    fail_rebase: bool,
}
type Shared = Rc<RefCell<Evidence>>;
struct Disk(Shared);
struct Network(Shared);

impl Store for Disk {
    fn save(&mut self, value: Option<&Transaction>) -> io::Result<()> {
        let mut evidence = self.0.borrow_mut();
        if evidence.fail_save {
            return Err(io::Error::other("disk failure"));
        }
        evidence.persisted = value.cloned();
        evidence
            .events
            .push(if value.is_some() { "save" } else { "clean" }.into());
        Ok(())
    }
}
impl Backend for Network {
    fn stop_io(&mut self) -> io::Result<()> {
        self.0.borrow_mut().stopped += 1;
        if self.0.borrow().fail_stop {
            return Err(io::Error::from_raw_os_error(libc::ENOTCONN));
        }
        Ok(())
    }
    fn rebase(&mut self, previous: Fence, next: Fence) -> io::Result<()> {
        let mut e = self.0.borrow_mut();
        assert_eq!(e.persisted.as_ref().unwrap().fence, previous);
        assert_eq!(
            e.persisted
                .as_ref()
                .unwrap()
                .transition
                .as_ref()
                .unwrap()
                .fence,
            next
        );
        e.events.push("rebase".into());
        if e.fail_rebase {
            return Err(io::Error::other("rebase interrupted"));
        }
        Ok(())
    }
    fn apply_and_verify(&mut self, fence: Fence, step: Step) -> io::Result<()> {
        let mut e = self.0.borrow_mut();
        let saved = e
            .persisted
            .as_ref()
            .expect("intent persisted before mutation");
        assert_eq!(saved.fence, fence);
        assert!(
            saved
                .records
                .iter()
                .any(|r| r.step == step && r.state == StepState::Intended)
        );
        e.events.push(format!("apply {step:?}"));
        if e.fail_apply == Some(step) {
            return Err(io::Error::other("mutation uncertain"));
        }
        Ok(())
    }
    fn quiesce_and_revoke_egress(&mut self, _: Fence) -> io::Result<()> {
        self.0.borrow_mut().events.push("quiesce".into());
        Ok(())
    }
    fn restore_and_verify(&mut self, _: Fence, step: Step) -> io::Result<()> {
        let mut e = self.0.borrow_mut();
        assert!(matches!(
            e.persisted.as_ref().unwrap().phase,
            Phase::Recovering
        ));
        e.events.push(format!("restore {step:?}"));
        if e.fail_restore == Some(step) {
            return Err(io::Error::other("ownership conflict"));
        }
        Ok(())
    }
}
fn setup() -> (Coordinator<Disk, Network>, Shared, Owner, Fence) {
    let evidence = Shared::default();
    let coordinator =
        Coordinator::new(Disk(evidence.clone()), Network(evidence.clone()), None).unwrap();
    (
        coordinator,
        evidence,
        Owner {
            uid: 501,
            pid: 23,
            pid_version: 7,
        },
        Fence {
            session: Uuid::new_v4(),
            configuration: 1,
            network: 1,
        },
    )
}

#[test]
fn prepare_persists_each_intent_and_disconnect_removes_protection_last() {
    let (mut c, e, owner, fence) = setup();
    c.prepare(owner, fence, true).unwrap();
    c.configure(owner, fence).unwrap();
    assert_eq!(c.state().unwrap().phase, Phase::Preparing);
    c.commit(owner, fence).unwrap();
    assert_eq!(e.borrow().persisted.as_ref().unwrap().phase, Phase::Active);
    c.disconnect(owner, fence).unwrap();
    let events: Vec<_> = e
        .borrow()
        .events
        .iter()
        .filter(|e| *e != "save")
        .cloned()
        .collect();
    assert_eq!(
        events,
        [
            "apply Protection",
            "apply Egress",
            "apply Utun",
            "apply Dns",
            "apply Routes",
            "quiesce",
            "restore Routes",
            "restore Dns",
            "restore Utun",
            "restore Egress",
            "restore Protection",
            "clean"
        ]
    );
    assert!(c.state().is_none());
}

#[test]
fn engine_loss_retains_guard_and_never_claims_clean() {
    let (mut c, e, owner, fence) = setup();
    c.prepare(owner, fence, true).unwrap();
    c.configure(owner, fence).unwrap();
    c.commit(owner, fence).unwrap();
    e.borrow_mut().events.clear();
    c.owner_lost(owner, fence).unwrap();
    assert_eq!(c.state().unwrap().phase, Phase::Retained);
    assert_eq!(e.borrow().events, ["save", "quiesce"]);
    assert!(e.borrow().persisted.is_some());
}

#[test]
fn another_user_stale_network_or_reused_pid_cannot_disconnect_or_commit() {
    let (mut c, e, owner, fence) = setup();
    c.prepare(owner, fence, true).unwrap();
    c.configure(owner, fence).unwrap();
    for (who, key) in [
        (Owner { uid: 502, ..owner }, fence),
        (
            Owner {
                pid_version: 8,
                ..owner
            },
            fence,
        ),
        (
            owner,
            Fence {
                network: 2,
                ..fence
            },
        ),
        (
            owner,
            Fence {
                configuration: 2,
                ..fence
            },
        ),
        (
            owner,
            Fence {
                session: Uuid::new_v4(),
                ..fence
            },
        ),
    ] {
        let before = e.borrow().events.len();
        assert_eq!(
            c.commit(who, key).unwrap_err().kind(),
            io::ErrorKind::PermissionDenied
        );
        assert_eq!(
            c.disconnect(who, key).unwrap_err().kind(),
            io::ErrorKind::PermissionDenied
        );
        assert_eq!(e.borrow().events.len(), before);
    }
}

#[test]
fn route_conflict_still_restores_dns_without_releasing_protection() {
    let (mut c, e, owner, fence) = setup();
    c.prepare(owner, fence, true).unwrap();
    c.configure(owner, fence).unwrap();
    c.commit(owner, fence).unwrap();
    e.borrow_mut().events.clear();
    e.borrow_mut().fail_restore = Some(Step::Routes);
    assert!(c.disconnect(owner, fence).is_err());
    assert_eq!(c.state().unwrap().phase, Phase::RecoveryRequired);
    let restored: Vec<_> = e
        .borrow()
        .events
        .iter()
        .filter(|s| s.starts_with("restore"))
        .cloned()
        .collect();
    assert_eq!(restored, ["restore Routes", "restore Dns"]);
    e.borrow_mut().fail_restore = None;
    c.recover_as_administrator().unwrap();
    assert!(c.state().is_none());
}

#[test]
fn failed_restore_does_not_remove_guard_and_can_be_retried_explicitly() {
    let (mut c, e, owner, fence) = setup();
    c.prepare(owner, fence, true).unwrap();
    c.configure(owner, fence).unwrap();
    e.borrow_mut().fail_restore = Some(Step::Dns);
    assert!(c.disconnect(owner, fence).is_err());
    assert_eq!(c.state().unwrap().phase, Phase::RecoveryRequired);
    assert!(!e.borrow().events.contains(&"restore Protection".into()));
    e.borrow_mut().fail_restore = None;
    c.recover_as_administrator().unwrap();
    assert!(c.state().is_none());
}

#[test]
fn uncertain_apply_remains_recoverable_with_intent_in_journal() {
    let (mut c, e, owner, fence) = setup();
    e.borrow_mut().fail_apply = Some(Step::Dns);
    c.prepare(owner, fence, true).unwrap();
    assert!(c.configure(owner, fence).is_err());
    assert_eq!(c.state().unwrap().phase, Phase::RecoveryRequired);
    assert!(!e.borrow().events.contains(&"apply Routes".into()));
    assert!(c.commit(owner, fence).is_err());
    c.disconnect(owner, fence).unwrap();
    assert!(e.borrow().events.contains(&"restore Dns".into()));
}

#[test]
fn failed_persistence_prevents_mutation_and_further_connections() {
    let (mut c, e, owner, fence) = setup();
    e.borrow_mut().fail_save = true;
    assert!(c.prepare(owner, fence, true).is_err());
    assert!(c.persistence_uncertain());
    assert!(e.borrow().events.is_empty());
    e.borrow_mut().fail_save = false;
    assert!(c.prepare(owner, fence, true).is_err());
}

#[test]
fn rejects_incomplete_active_state_and_duplicate_steps_on_restart() {
    let (mut c, _, owner, fence) = setup();
    c.prepare(owner, fence, true).unwrap();
    c.configure(owner, fence).unwrap();
    let mut t = c.state().unwrap().clone();
    t.phase = Phase::Active;
    t.records.pop();
    assert!(t.validate().is_err());
    let mut t = c.state().unwrap().clone();
    t.records.push(t.records[0].clone());
    assert!(t.validate().is_err());
    let mut t = c.state().unwrap().clone();
    t.fence.network = 0;
    assert!(t.validate().is_err());
}

#[test]
fn engine_loss_during_failed_recovery_does_not_downgrade_recovery_required() {
    let (mut c, e, owner, fence) = setup();
    c.prepare(owner, fence, true).unwrap();
    c.configure(owner, fence).unwrap();
    e.borrow_mut().fail_restore = Some(Step::Dns);
    assert!(c.disconnect(owner, fence).is_err());
    c.owner_lost(owner, fence).unwrap();
    assert_eq!(c.state().unwrap().phase, Phase::RecoveryRequired);
    c.state().unwrap().validate().unwrap();
}

#[test]
fn configuration_cannot_start_before_remote_prepare_or_be_replayed() {
    let (mut c, e, owner, fence) = setup();
    assert!(c.configure(owner, fence).is_err());
    c.prepare(owner, fence, true).unwrap();
    assert!(!e.borrow().events.contains(&"apply Utun".into()));
    assert!(c.commit(owner, fence).is_err());
    c.configure(owner, fence).unwrap();
    assert!(c.configure(owner, fence).is_err());
    c.commit(owner, fence).unwrap();
    assert!(c.commit(owner, fence).is_err());
}

#[test]
fn delayed_eof_from_previous_session_cannot_stop_new_session() {
    let (mut c, e, owner, old) = setup();
    c.prepare(owner, old, false).unwrap();
    c.disconnect(owner, old).unwrap();
    let new = Fence {
        session: Uuid::new_v4(),
        ..old
    };
    c.prepare(owner, new, false).unwrap();
    e.borrow_mut().events.clear();
    assert_eq!(
        c.owner_lost(owner, old).unwrap_err().kind(),
        io::ErrorKind::PermissionDenied
    );
    assert!(e.borrow().events.is_empty());
    assert_eq!(c.state().unwrap().fence, new);
}

#[test]
fn guarded_resume_changes_owner_and_fence_without_removing_protection() {
    let (mut c, e, owner, fence) = setup();
    c.prepare(owner, fence, true).unwrap();
    c.configure(owner, fence).unwrap();
    c.commit(owner, fence).unwrap();
    c.owner_lost(owner, fence).unwrap();
    let next = Fence {
        configuration: 2,
        network: 2,
        ..fence
    };
    let next_owner = Owner {
        pid: 42,
        pid_version: 10,
        ..owner
    };
    assert!(
        c.resume(
            Owner {
                uid: 502,
                ..next_owner
            },
            fence,
            next,
            true
        )
        .is_err()
    );
    c.resume(next_owner, fence, next, true).unwrap();
    assert!(!e.borrow().events.contains(&"restore Protection".into()));
    assert!(!e.borrow().events.contains(&"clean".into()));
    assert!(c.configure(owner, fence).is_err());
    c.configure(next_owner, next).unwrap();
    c.commit(next_owner, next).unwrap();
    c.disconnect(next_owner, next).unwrap();
}

#[test]
fn retained_disconnect_requires_same_user_and_exact_fence() {
    let (mut c, e, owner, fence) = setup();
    c.prepare(owner, fence, true).unwrap();
    let restarted = Owner {
        pid: 99,
        pid_version: 4,
        ..owner
    };
    assert!(c.disconnect_retained(restarted, fence).is_err());
    c.owner_lost(owner, fence).unwrap();
    assert!(
        c.disconnect_retained(
            Owner {
                uid: 900,
                ..restarted
            },
            fence
        )
        .is_err()
    );
    assert!(
        c.disconnect_retained(
            restarted,
            Fence {
                configuration: 2,
                ..fence
            }
        )
        .is_err()
    );
    c.disconnect_retained(restarted, fence).unwrap();
    assert!(c.state().is_none());
    assert_eq!(e.borrow().events.last().unwrap(), "clean");
}
#[test]
fn explicit_replacement_keeps_guard_even_without_kill_switch() {
    let (mut c, e, owner, fence) = setup();
    c.prepare(owner, fence, false).unwrap();
    c.suspend(owner, fence).unwrap();
    assert_eq!(c.state().unwrap().phase, Phase::Retained);
    c.resume(
        owner,
        fence,
        Fence {
            configuration: 2,
            ..fence
        },
        true,
    )
    .unwrap();
    assert!(c.state().unwrap().kill_switch);
    assert!(!e.borrow().events.contains(&"restore Protection".into()));
}

#[test]
fn ownership_loss_stops_io_even_if_recovery_storage_is_unavailable() {
    let (mut c, e, owner, fence) = setup();
    c.prepare(owner, fence, true).unwrap();
    e.borrow_mut().fail_save = true;
    assert!(c.owner_lost(owner, fence).is_err());
    assert_eq!(e.borrow().stopped, 1);
    assert!(c.persistence_uncertain());
    assert!(c.owner_lost(owner, fence).is_err());
    assert_eq!(e.borrow().stopped, 2);
    assert!(!e.borrow().events.contains(&"restore Protection".into()));
}

#[test]
fn shutdown_failure_still_attempts_verified_pf_revocation_and_explicit_recovery() {
    let (mut c, e, owner, fence) = setup();
    c.prepare(owner, fence, true).unwrap();
    e.borrow_mut().fail_stop = true;
    c.owner_lost(owner, fence).unwrap();
    assert_eq!(c.state().unwrap().phase, Phase::Retained);
    assert!(e.borrow().events.contains(&"quiesce".into()));
    c.recover_as_administrator().unwrap();
    assert!(c.state().is_none());
}
#[test]
fn interrupted_handoff_to_kill_switch_keeps_guard_before_native_rebase() {
    let (mut c, e, owner, fence) = setup();
    c.prepare(owner, fence, false).unwrap();
    c.suspend(owner, fence).unwrap();
    e.borrow_mut().fail_rebase = true;
    assert!(
        c.resume(
            owner,
            fence,
            Fence {
                configuration: 2,
                ..fence
            },
            true
        )
        .is_err()
    );
    assert!(
        e.borrow()
            .persisted
            .as_ref()
            .unwrap()
            .transition
            .as_ref()
            .unwrap()
            .kill_switch
    );
    c.owner_lost(owner, fence).unwrap();
    assert!(c.state().is_some());
    assert!(!e.borrow().events.contains(&"restore Protection".into()));
}
