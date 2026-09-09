//! Directed interest registration. Register before retrying capacity; release
//! consumes each interest once. No waiters means no registry lock or wake.
use super::BufferBudget;
use std::sync::{
    Arc, Mutex, Weak,
    atomic::{AtomicBool, Ordering},
};
use std::task::Waker;
use tokio::sync::Notify;

pub(crate) struct BudgetWaiter {
    budget: Weak<BufferBudget>,
    armed: AtomicBool,
    notify: Option<Arc<Notify>>,
    waker: Mutex<Option<Waker>>,
}
impl BudgetWaiter {
    pub(crate) fn new(budget: &Arc<BufferBudget>, notify: Option<Arc<Notify>>) -> Arc<Self> {
        let waiter = Arc::new(Self {
            budget: Arc::downgrade(budget),
            armed: AtomicBool::new(false),
            notify,
            waker: Mutex::new(None),
        });
        let mut registry = budget.waiters.lock().unwrap_or_else(|e| e.into_inner());
        registry.retain(|v| v.strong_count() != 0);
        registry.push(Arc::downgrade(&waiter));
        waiter
    }
    pub(crate) fn arm(&self, waker: Option<&Waker>) {
        if let Some(waker) = waker {
            *self.waker.lock().unwrap_or_else(|e| e.into_inner()) = Some(waker.clone());
        }
        if let Some(budget) = self.budget.upgrade() {
            // Increment first: a concurrent release cannot underflow the count.
            budget.waiter_count.fetch_add(1, Ordering::AcqRel);
            if self.armed.swap(true, Ordering::AcqRel) {
                budget.waiter_count.fetch_sub(1, Ordering::AcqRel);
            }
        }
    }
    pub(crate) fn disarm(&self) -> bool {
        if self.armed.swap(false, Ordering::AcqRel) {
            if let Some(budget) = self.budget.upgrade() {
                budget.waiter_count.fetch_sub(1, Ordering::AcqRel);
            }
            true
        } else {
            false
        }
    }
    pub(crate) fn wake(&self) {
        if self.disarm() {
            if let Some(budget) = self.budget.upgrade() {
                budget
                    .metrics
                    .performance
                    .budget_wakeups
                    .fetch_add(1, Ordering::Relaxed);
            }
            if let Some(notify) = &self.notify {
                notify.notify_one();
            }
            let waker = self.waker.lock().unwrap_or_else(|e| e.into_inner()).clone();
            if let Some(waker) = waker {
                waker.wake();
            }
        }
    }
}
impl Drop for BudgetWaiter {
    fn drop(&mut self) {
        self.disarm();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        future::Future,
        pin::pin,
        task::{Context, Waker},
    };
    fn notified(notify: &Notify) -> bool {
        pin!(notify.notified())
            .poll(&mut Context::from_waker(Waker::noop()))
            .is_ready()
    }
    #[test]
    fn idle_return_and_cancelled_waiters_never_wake_an_actor() {
        let budget = Arc::new(BufferBudget::new(128 << 10, Arc::default(), Arc::default()));
        let notify = Arc::new(Notify::new());
        let waiter = BudgetWaiter::new(&budget, Some(notify.clone()));
        let pool = super::super::pool::ReceivePool::shared(&budget);
        drop(pool.take().unwrap());
        assert!(!notified(&notify));
        assert_eq!(budget.metrics.performance.sample().budget_wakeups, 0);
        waiter.arm(None);
        drop(waiter);
        pool.close();
        assert!(!notified(&notify));
        assert_eq!(budget.waiter_count.load(Ordering::Acquire), 0);
    }
    #[test]
    fn capacity_before_registration_and_release_during_registration_cannot_be_lost() {
        for release_before in [true, false] {
            for _ in 0..64 {
                let budget = Arc::new(BufferBudget::new(1, Arc::default(), Arc::default()));
                let notify = Arc::new(Notify::new());
                let waiter = BudgetWaiter::new(&budget, Some(notify.clone()));
                let lease = budget.reserve(1).unwrap();
                assert!(budget.reserve(1).is_none());
                if release_before {
                    drop(lease);
                    waiter.arm(None);
                } else {
                    std::thread::scope(|scope| {
                        scope.spawn(move || drop(lease));
                        waiter.arm(None);
                    });
                }
                let capacity = budget
                    .reserve(1)
                    .expect("register then retry sees returned capacity");
                waiter.disarm();
                drop(capacity);
                assert_eq!(budget.waiter_count.load(Ordering::Acquire), 0);
                assert!(budget.metrics.performance.sample().budget_wakeups <= 1);
            }
        }
    }
}
