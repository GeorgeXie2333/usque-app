//! Runtime-scoped receive storage. A lease follows the allocation through all
//! Bytes slices and the idle cache, so caching cannot hide resident buffers.
use super::{BufferBudget, performance::Performance, stream::BufferLease};
use bytes::Bytes;
use std::sync::{
    Arc, Weak,
    atomic::{AtomicBool, Ordering},
};
use ts_netstack_smoltcp::netcore::flume;
use zeroize::Zeroize;

pub(super) const BLOCK: usize = 32 << 10;
const IDLE_BYTES: usize = if cfg!(all(target_os = "android", target_pointer_width = "32")) {
    512 << 10
} else {
    2 << 20
};

pub(crate) struct ReceivePool {
    budget: Arc<BufferBudget>,
    tx: flume::Sender<Allocation>,
    rx: flume::Receiver<Allocation>,
    closed: AtomicBool,
}
struct Allocation {
    bytes: Vec<u8>,
    _lease: BufferLease,
    metrics: Arc<Performance>,
}
impl Drop for Allocation {
    fn drop(&mut self) {
        self.bytes.zeroize();
        self.metrics
            .receive_pool_live_bytes
            .fetch_sub(BLOCK as u64, Ordering::Relaxed);
    }
}
pub(super) struct ReceiveBuffer {
    allocation: Option<Allocation>,
    pool: Weak<ReceivePool>,
    length: usize,
}
impl ReceiveBuffer {
    pub(super) fn as_mut(&mut self) -> &mut [u8] {
        &mut self.allocation.as_mut().expect("owned allocation").bytes
    }
    pub(super) fn freeze(mut self, length: usize) -> Bytes {
        assert!(length <= BLOCK);
        self.length = length;
        Bytes::from_owner(self)
    }
}
impl AsRef<[u8]> for ReceiveBuffer {
    fn as_ref(&self) -> &[u8] {
        &self.allocation.as_ref().expect("owned allocation").bytes[..self.length]
    }
}
impl Drop for ReceiveBuffer {
    fn drop(&mut self) {
        let Some(allocation) = self.allocation.take() else {
            return;
        };
        if let Some(pool) = self.pool.upgrade() {
            pool.recycle(allocation);
        }
    }
}
impl ReceivePool {
    pub(crate) fn shared(budget: &Arc<BufferBudget>) -> Arc<Self> {
        let mut slot = budget.pool.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(pool) = slot.upgrade() {
            return pool;
        }
        let (tx, rx) = flume::bounded(IDLE_BYTES / BLOCK);
        let pool = Arc::new(Self {
            budget: budget.clone(),
            tx,
            rx,
            closed: AtomicBool::new(false),
        });
        *slot = Arc::downgrade(&pool);
        pool
    }
    pub(super) fn take(self: &Arc<Self>) -> Option<ReceiveBuffer> {
        if self.closed.load(Ordering::Acquire) {
            return None;
        }
        let p = &self.budget.metrics.performance;
        let allocation = if let Ok(allocation) = self.rx.try_recv() {
            p.receive_pool_idle_bytes
                .fetch_sub(BLOCK as u64, Ordering::Relaxed);
            p.receive_pool_hits.fetch_add(1, Ordering::Relaxed);
            allocation
        } else {
            let lease = self.budget.reserve(BLOCK)?;
            let live = p
                .receive_pool_live_bytes
                .fetch_add(BLOCK as u64, Ordering::Relaxed)
                + BLOCK as u64;
            p.receive_pool_live_high_watermark
                .fetch_max(live, Ordering::Relaxed);
            p.receive_pool_allocations.fetch_add(1, Ordering::Relaxed);
            Allocation {
                bytes: vec![0; BLOCK],
                _lease: lease,
                metrics: p.clone(),
            }
        };
        Some(ReceiveBuffer {
            allocation: Some(allocation),
            pool: Arc::downgrade(self),
            length: BLOCK,
        })
    }
    fn recycle(&self, allocation: Allocation) {
        if self.closed.load(Ordering::Acquire) {
            return;
        }
        let p = &self.budget.metrics.performance;
        let idle = p
            .receive_pool_idle_bytes
            .fetch_add(BLOCK as u64, Ordering::Relaxed)
            + BLOCK as u64;
        match self.tx.try_send(allocation) {
            Ok(()) => {
                p.receive_pool_idle_high_watermark
                    .fetch_max(idle.min(IDLE_BYTES as u64), Ordering::Relaxed);
                // Closing may race with returning a previously active chunk.
                if self.closed.load(Ordering::Acquire) {
                    self.trim();
                }
                self.budget.notify_capacity();
            }
            Err(_) => {
                p.receive_pool_idle_bytes
                    .fetch_sub(BLOCK as u64, Ordering::Relaxed);
            }
        }
    }
    pub(crate) fn trim(&self) {
        while let Ok(allocation) = self.rx.try_recv() {
            let p = &self.budget.metrics.performance;
            p.receive_pool_idle_bytes
                .fetch_sub(BLOCK as u64, Ordering::Relaxed);
            p.receive_pool_evictions.fetch_add(1, Ordering::Relaxed);
            drop(allocation);
        }
    }
    pub(crate) fn close(&self) {
        self.closed.store(true, Ordering::Release);
        self.trim();
    }
}
impl Drop for ReceivePool {
    fn drop(&mut self) {
        self.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn setup(size: usize) -> (Arc<BufferBudget>, Arc<ReceivePool>) {
        let budget = Arc::new(BufferBudget::new(size, Arc::default(), Arc::default()));
        let pool = ReceivePool::shared(&budget);
        (budget, pool)
    }
    #[test]
    fn warm_reuse_keeps_one_lease_until_close_and_late_return() {
        let (budget, pool) = setup(BLOCK * 4);
        let first = pool.take().unwrap().freeze(BLOCK);
        let pointer = first.as_ptr();
        drop(first);
        assert_eq!(budget.available(), BLOCK * 3);
        let second = pool.take().unwrap().freeze(123);
        assert_eq!(second.as_ptr(), pointer);
        let retained = second.slice(1..);
        drop(second);
        pool.close();
        assert_eq!(budget.available(), BLOCK * 3);
        drop(retained);
        assert_eq!(budget.available(), BLOCK * 4);
        let p = budget.metrics.performance.sample();
        assert_eq!((p.receive_pool_allocations, p.receive_pool_hits), (1, 1));
        assert_eq!(
            (p.receive_pool_idle_bytes, p.receive_pool_live_bytes),
            (0, 0)
        );
        assert!(pool.take().is_none());
    }
    #[test]
    fn cached_bytes_are_trimmed_before_admission_rejection() {
        let (budget, pool) = setup(BLOCK * 2);
        drop(pool.take().unwrap());
        let lease = budget
            .reserve_admission(BLOCK + BLOCK / 2)
            .expect("cache must not reduce admission");
        assert_eq!(
            budget.metrics.performance.sample().receive_pool_idle_bytes,
            0
        );
        drop(lease);
        assert_eq!(budget.available(), BLOCK * 2);
    }

    #[test]
    fn idle_cap_and_concurrent_close_account_all_live_and_late_returns() {
        let (budget, pool) = setup(IDLE_BYTES * 2);
        let buffers: Vec<_> = (0..IDLE_BYTES / BLOCK + 8)
            .map(|_| pool.take().unwrap().freeze(1))
            .collect();
        assert_eq!(
            budget.metrics.performance.sample().receive_pool_live_bytes,
            buffers.len() as u64 * BLOCK as u64
        );
        drop(buffers);
        assert_eq!(
            budget.metrics.performance.sample().receive_pool_idle_bytes,
            IDLE_BYTES as u64
        );
        assert_eq!(budget.available(), IDLE_BYTES);
        let late: Vec<_> = (0..8).map(|_| pool.take().unwrap().freeze(BLOCK)).collect();
        std::thread::scope(|scope| {
            scope.spawn(move || drop(late));
            pool.close();
        });
        assert_eq!(budget.available(), IDLE_BYTES * 2);
        assert_eq!(
            budget.metrics.performance.sample().receive_pool_live_bytes,
            0
        );
        let (new_budget, new_pool) = setup(BLOCK * 2);
        drop(new_pool.take().unwrap());
        assert_eq!(new_budget.available(), BLOCK);
        assert_eq!(budget.available(), IDLE_BYTES * 2);
    }
}
