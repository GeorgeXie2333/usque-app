use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use tokio::sync::{OwnedSemaphorePermit, Semaphore, mpsc};
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum QueueKind {
    TunToTransport,
    ProxyToTransport,
    TransportOutgoingPackets,
    H3DatagramSend,
    H3WireSend,
    TransportToTun,
    TransportToProxy,
    DirectDnsRequests,
}

pub const ALL_QUEUE_KINDS: [QueueKind; 8] = [
    QueueKind::TunToTransport,
    QueueKind::ProxyToTransport,
    QueueKind::TransportOutgoingPackets,
    QueueKind::H3DatagramSend,
    QueueKind::H3WireSend,
    QueueKind::TransportToTun,
    QueueKind::TransportToProxy,
    QueueKind::DirectDnsRequests,
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueueMetricsSnapshot {
    pub kind: QueueKind,
    pub registered: bool,
    pub current_items: u64,
    pub current_bytes: u64,
    pub item_capacity: u64,
    pub byte_capacity: u64,
    pub items_high_water: u64,
    pub bytes_high_water: u64,
    pub enqueue_count: u64,
    pub dequeue_count: u64,
    pub drop_items: u64,
    pub drop_bytes: u64,
    pub oldest_age: Option<Duration>,
    pub closed: bool,
    pub cancelled: bool,
}

/// Lock-free accounting for one bounded queue.
///
/// Each queued item carries its enqueue timestamp and the single receiver
/// publishes the next FIFO head after a dequeue. Packet-path updates therefore
/// never allocate and snapshotting never inspects a payload.
#[derive(Debug)]
pub struct QueueMetrics {
    kind: QueueKind,
    registered: bool,
    item_capacity: u64,
    byte_capacity: u64,
    epoch: Instant,
    oldest_timestamp: AtomicU64,
    current_items: AtomicU64,
    current_bytes: AtomicU64,
    items_high_water: AtomicU64,
    bytes_high_water: AtomicU64,
    enqueue_count: AtomicU64,
    dequeue_count: AtomicU64,
    drop_items: AtomicU64,
    drop_bytes: AtomicU64,
    closed: AtomicBool,
    cancelled: AtomicBool,
    unordered_timestamps: Option<Mutex<BTreeMap<u64, u64>>>,
}

impl QueueMetrics {
    pub fn unregistered(kind: QueueKind) -> Arc<Self> {
        Arc::new(Self {
            kind,
            registered: false,
            item_capacity: 0,
            byte_capacity: 0,
            epoch: Instant::now(),
            oldest_timestamp: AtomicU64::new(0),
            current_items: AtomicU64::new(0),
            current_bytes: AtomicU64::new(0),
            items_high_water: AtomicU64::new(0),
            bytes_high_water: AtomicU64::new(0),
            enqueue_count: AtomicU64::new(0),
            dequeue_count: AtomicU64::new(0),
            drop_items: AtomicU64::new(0),
            drop_bytes: AtomicU64::new(0),
            closed: AtomicBool::new(false),
            cancelled: AtomicBool::new(false),
            unordered_timestamps: None,
        })
    }

    pub fn new(kind: QueueKind, item_capacity: usize, byte_capacity: usize) -> Arc<Self> {
        Self::new_inner(kind, item_capacity, byte_capacity, false)
    }

    pub(crate) fn new_unordered(
        kind: QueueKind,
        item_capacity: usize,
        byte_capacity: usize,
    ) -> Arc<Self> {
        Self::new_inner(kind, item_capacity, byte_capacity, true)
    }

    fn new_inner(
        kind: QueueKind,
        item_capacity: usize,
        byte_capacity: usize,
        track_unordered_timestamps: bool,
    ) -> Arc<Self> {
        assert!(
            item_capacity > 0,
            "tracked queue item capacity must be non-zero"
        );
        assert!(
            byte_capacity > 0,
            "tracked queue byte capacity must be non-zero"
        );
        Arc::new(Self {
            kind,
            registered: true,
            item_capacity: item_capacity as u64,
            byte_capacity: byte_capacity as u64,
            epoch: Instant::now(),
            oldest_timestamp: AtomicU64::new(0),
            current_items: AtomicU64::new(0),
            current_bytes: AtomicU64::new(0),
            items_high_water: AtomicU64::new(0),
            bytes_high_water: AtomicU64::new(0),
            enqueue_count: AtomicU64::new(0),
            dequeue_count: AtomicU64::new(0),
            drop_items: AtomicU64::new(0),
            drop_bytes: AtomicU64::new(0),
            closed: AtomicBool::new(false),
            cancelled: AtomicBool::new(false),
            unordered_timestamps: track_unordered_timestamps.then(|| Mutex::new(BTreeMap::new())),
        })
    }

    pub fn kind(&self) -> QueueKind {
        self.kind
    }

    pub fn start_entry(self: &Arc<Self>, bytes: usize) -> QueueEntry {
        debug_assert!(self.registered, "cannot enqueue into an unregistered queue");
        let enqueued_at = Instant::now();
        saturating_increment(&self.enqueue_count, 1);
        let timestamp = self.timestamp(enqueued_at);
        let previous_items = self.current_items.fetch_add(1, Ordering::AcqRel);
        let current_items = previous_items.saturating_add(1);
        if previous_items == 0 {
            self.oldest_timestamp.store(timestamp, Ordering::Release);
        }
        if let Some(timestamps) = &self.unordered_timestamps {
            let mut timestamps = lock_unpoisoned(timestamps);
            let count = timestamps.entry(timestamp).or_default();
            *count = count.saturating_add(1);
            if let Some((&oldest, _)) = timestamps.first_key_value() {
                self.oldest_timestamp.store(oldest, Ordering::Release);
            }
        }
        let current_bytes = self
            .current_bytes
            .fetch_add(bytes as u64, Ordering::AcqRel)
            .saturating_add(bytes as u64);
        self.items_high_water
            .fetch_max(current_items, Ordering::Relaxed);
        self.bytes_high_water
            .fetch_max(current_bytes, Ordering::Relaxed);

        QueueEntry {
            metrics: Arc::clone(self),
            timestamp,
            bytes: bytes as u64,
            active: true,
        }
    }

    pub fn record_rejected(&self, bytes: usize) {
        saturating_increment(&self.drop_items, 1);
        saturating_increment(&self.drop_bytes, bytes as u64);
    }

    pub fn mark_closed(&self) {
        self.closed.store(true, Ordering::Release);
    }

    pub fn mark_cancelled(&self) {
        self.cancelled.store(true, Ordering::Release);
        self.mark_closed();
    }

    pub fn observe_oldest_entry(&self, entry: &QueueEntry) {
        self.observe_oldest_timestamp(entry.timestamp);
    }

    pub fn snapshot(&self, now: Instant) -> QueueMetricsSnapshot {
        let current_items = self.current_items.load(Ordering::Acquire);
        let timestamp = self.oldest_timestamp.load(Ordering::Acquire);
        let oldest_age = if current_items == 0 || timestamp == 0 {
            None
        } else {
            let enqueued = self.epoch + Duration::from_nanos(timestamp - 1);
            Some(now.saturating_duration_since(enqueued))
        };
        QueueMetricsSnapshot {
            kind: self.kind,
            registered: self.registered,
            current_items,
            current_bytes: self.current_bytes.load(Ordering::Acquire),
            item_capacity: self.item_capacity,
            byte_capacity: self.byte_capacity,
            items_high_water: self.items_high_water.load(Ordering::Relaxed),
            bytes_high_water: self.bytes_high_water.load(Ordering::Relaxed),
            enqueue_count: self.enqueue_count.load(Ordering::Relaxed),
            dequeue_count: self.dequeue_count.load(Ordering::Relaxed),
            drop_items: self.drop_items.load(Ordering::Relaxed),
            drop_bytes: self.drop_bytes.load(Ordering::Relaxed),
            oldest_age,
            closed: self.closed.load(Ordering::Acquire),
            cancelled: self.cancelled.load(Ordering::Acquire),
        }
    }

    fn timestamp(&self, instant: Instant) -> u64 {
        u64::try_from(instant.saturating_duration_since(self.epoch).as_nanos())
            .unwrap_or(u64::MAX - 1)
            .saturating_add(1)
    }

    fn finish(&self, timestamp: u64, bytes: u64, dropped: bool) {
        if !dropped {
            saturating_increment(&self.dequeue_count, 1);
        }
        let previous_items = saturating_subtract(&self.current_items, 1);
        saturating_subtract(&self.current_bytes, bytes);
        if let Some(timestamps) = &self.unordered_timestamps {
            let mut timestamps = lock_unpoisoned(timestamps);
            if let Some(count) = timestamps.get_mut(&timestamp) {
                *count = count.saturating_sub(1);
                if *count == 0 {
                    timestamps.remove(&timestamp);
                }
            }
            self.oldest_timestamp.store(
                timestamps
                    .first_key_value()
                    .map_or(0, |(&oldest, _)| oldest),
                Ordering::Release,
            );
        } else if previous_items <= 1 || self.oldest_timestamp.load(Ordering::Acquire) == timestamp
        {
            // The receiver installs the next FIFO item's timestamp after the
            // dequeue. A concurrent first enqueue sees current_items == 0 and
            // installs its own timestamp, so the zero window is self-healing.
            let _ = self.oldest_timestamp.compare_exchange(
                timestamp,
                0,
                Ordering::AcqRel,
                Ordering::Acquire,
            );
        }
        if dropped {
            saturating_increment(&self.drop_items, 1);
            saturating_increment(&self.drop_bytes, bytes);
        }
    }

    fn observe_oldest_timestamp(&self, timestamp: u64) {
        if self.unordered_timestamps.is_some() {
            return;
        }
        if self.current_items.load(Ordering::Acquire) != 0 {
            self.oldest_timestamp.store(timestamp, Ordering::Release);
        }
    }
}

pub struct QueueEntry {
    metrics: Arc<QueueMetrics>,
    timestamp: u64,
    bytes: u64,
    active: bool,
}

impl QueueEntry {
    pub fn complete(mut self) {
        self.metrics.finish(self.timestamp, self.bytes, false);
        self.active = false;
    }

    pub fn drop_item(mut self) {
        self.metrics.finish(self.timestamp, self.bytes, true);
        self.active = false;
    }
}

impl Drop for QueueEntry {
    fn drop(&mut self) {
        if self.active {
            self.metrics.finish(self.timestamp, self.bytes, true);
            self.active = false;
        }
    }
}

struct TrackedItem<T> {
    value: Option<T>,
    entry: Option<QueueEntry>,
    _byte_permit: OwnedSemaphorePermit,
    _item_permit: OwnedSemaphorePermit,
}

impl<T> TrackedItem<T> {
    fn timestamp(&self) -> u64 {
        self.entry
            .as_ref()
            .expect("tracked item has queue entry")
            .timestamp
    }

    fn receive(mut self) -> T {
        self.entry
            .take()
            .expect("tracked item has queue entry")
            .complete();
        self.value.take().expect("tracked item has payload")
    }
}

pub struct TrackedSender<T> {
    inner: mpsc::Sender<TrackedItem<T>>,
    byte_budget: Arc<Semaphore>,
    item_budget: Arc<Semaphore>,
    metrics: Arc<QueueMetrics>,
    lifetime: Arc<TrackedSenderLifetime>,
}

struct TrackedSenderLifetime {
    metrics: Arc<QueueMetrics>,
}

impl Drop for TrackedSenderLifetime {
    fn drop(&mut self) {
        self.metrics.mark_closed();
    }
}

impl<T> Clone for TrackedSender<T> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            byte_budget: Arc::clone(&self.byte_budget),
            item_budget: Arc::clone(&self.item_budget),
            metrics: Arc::clone(&self.metrics),
            lifetime: Arc::clone(&self.lifetime),
        }
    }
}

impl<T> TrackedSender<T> {
    pub async fn send(&self, value: T, bytes: usize) -> Result<(), TrackedSendError> {
        self.send_inner(value, bytes, None).await
    }

    pub async fn send_cancellable(
        &self,
        value: T,
        bytes: usize,
        cancellation: &CancellationToken,
    ) -> Result<(), TrackedSendError> {
        self.send_inner(value, bytes, Some(cancellation)).await
    }

    pub fn try_send(&self, value: T, bytes: usize) -> Result<(), TrackedTrySendError<T>> {
        let permits = match self.permit_count(bytes) {
            Ok(permits) => permits,
            Err(kind) => {
                self.metrics.record_rejected(bytes);
                return Err(TrackedTrySendError { kind, value });
            }
        };
        let byte_permit = match Arc::clone(&self.byte_budget).try_acquire_many_owned(permits) {
            Ok(permit) => permit,
            Err(tokio::sync::TryAcquireError::NoPermits) => {
                self.metrics.record_rejected(bytes);
                return Err(TrackedTrySendError {
                    kind: TrackedSendErrorKind::Full,
                    value,
                });
            }
            Err(tokio::sync::TryAcquireError::Closed) => {
                self.metrics.mark_closed();
                self.metrics.record_rejected(bytes);
                return Err(TrackedTrySendError {
                    kind: TrackedSendErrorKind::Closed,
                    value,
                });
            }
        };
        let slot = match Arc::clone(&self.item_budget).try_acquire_owned() {
            Ok(permit) => permit,
            Err(error) => {
                self.metrics.record_rejected(bytes);
                let kind = match error {
                    tokio::sync::TryAcquireError::NoPermits => TrackedSendErrorKind::Full,
                    tokio::sync::TryAcquireError::Closed => TrackedSendErrorKind::Closed,
                };
                return Err(TrackedTrySendError { kind, value });
            }
        };
        let item_permit = match self.inner.clone().try_reserve_owned() {
            Ok(permit) => permit,
            Err(mpsc::error::TrySendError::Full(_)) => {
                self.metrics.record_rejected(bytes);
                return Err(TrackedTrySendError {
                    kind: TrackedSendErrorKind::Full,
                    value,
                });
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                self.metrics.mark_closed();
                self.metrics.record_rejected(bytes);
                return Err(TrackedTrySendError {
                    kind: TrackedSendErrorKind::Closed,
                    value,
                });
            }
        };
        item_permit.send(TrackedItem {
            value: Some(value),
            entry: Some(self.metrics.start_entry(bytes)),
            _byte_permit: byte_permit,
            _item_permit: slot,
        });
        Ok(())
    }

    pub fn capacity(&self) -> usize {
        self.inner
            .capacity()
            .min(self.item_budget.available_permits())
    }

    pub fn max_capacity(&self) -> usize {
        self.inner.max_capacity()
    }

    async fn send_inner(
        &self,
        value: T,
        bytes: usize,
        cancellation: Option<&CancellationToken>,
    ) -> Result<(), TrackedSendError> {
        let permits = match self.permit_count(bytes) {
            Ok(permits) => permits,
            Err(kind) => {
                self.metrics.record_rejected(bytes);
                drop(value);
                return Err(TrackedSendError { kind });
            }
        };
        let reserve = self.inner.clone().reserve_owned();
        tokio::pin!(reserve);
        let item_permit = if let Some(cancellation) = cancellation {
            tokio::select! {
                _ = cancellation.cancelled() => {
                    self.metrics.record_rejected(bytes);
                    drop(value);
                    return Err(TrackedSendError { kind: TrackedSendErrorKind::Cancelled });
                }
                permit = &mut reserve => permit,
            }
        } else {
            reserve.await
        };
        let item_permit = match item_permit {
            Ok(permit) => permit,
            Err(_) => {
                self.metrics.mark_closed();
                self.metrics.record_rejected(bytes);
                drop(value);
                return Err(TrackedSendError {
                    kind: TrackedSendErrorKind::Closed,
                });
            }
        };

        // Retain an item permit until consumption, including while the
        // receiver prefetches an entry out of Tokio's bounded channel.
        let slot = Arc::clone(&self.item_budget).acquire_owned();
        tokio::pin!(slot);
        let slot = if let Some(cancellation) = cancellation {
            tokio::select! {
                _ = cancellation.cancelled() => {
                    self.metrics.record_rejected(bytes);
                    drop(value);
                    return Err(TrackedSendError { kind: TrackedSendErrorKind::Cancelled });
                }
                permit = &mut slot => permit,
            }
        } else {
            slot.await
        };
        let slot = slot.map_err(|_| {
            self.metrics.mark_closed();
            self.metrics.record_rejected(bytes);
            TrackedSendError {
                kind: TrackedSendErrorKind::Closed,
            }
        })?;

        let acquire = Arc::clone(&self.byte_budget).acquire_many_owned(permits);
        tokio::pin!(acquire);
        let byte_permit = if let Some(cancellation) = cancellation {
            tokio::select! {
                _ = cancellation.cancelled() => {
                    self.metrics.record_rejected(bytes);
                    drop(value);
                    return Err(TrackedSendError { kind: TrackedSendErrorKind::Cancelled });
                }
                permit = &mut acquire => permit,
            }
        } else {
            acquire.await
        };
        let byte_permit = match byte_permit {
            Ok(permit) => permit,
            Err(_) => {
                self.metrics.mark_closed();
                self.metrics.record_rejected(bytes);
                drop(value);
                return Err(TrackedSendError {
                    kind: TrackedSendErrorKind::Closed,
                });
            }
        };
        item_permit.send(TrackedItem {
            value: Some(value),
            entry: Some(self.metrics.start_entry(bytes)),
            _byte_permit: byte_permit,
            _item_permit: slot,
        });
        Ok(())
    }

    fn permit_count(&self, bytes: usize) -> Result<u32, TrackedSendErrorKind> {
        if bytes as u64 > self.metrics.byte_capacity {
            return Err(TrackedSendErrorKind::ByteLimit);
        }
        u32::try_from(bytes).map_err(|_| TrackedSendErrorKind::ByteLimit)
    }
}

pub struct TrackedReceiver<T> {
    inner: mpsc::Receiver<TrackedItem<T>>,
    metrics: Arc<QueueMetrics>,
    prefetched: Option<TrackedItem<T>>,
}

impl<T> TrackedReceiver<T> {
    pub async fn recv(&mut self) -> Option<T> {
        let item = match self.prefetched.take() {
            Some(item) => Some(item),
            None => self.inner.recv().await,
        };
        match item {
            Some(item) => {
                let value = item.receive();
                self.prefetch();
                Some(value)
            }
            None => {
                self.metrics.mark_closed();
                None
            }
        }
    }

    pub fn try_recv(&mut self) -> Result<T, mpsc::error::TryRecvError> {
        let item = match self.prefetched.take() {
            Some(item) => Ok(item),
            None => self.inner.try_recv(),
        };
        match item {
            Ok(item) => {
                let value = item.receive();
                self.prefetch();
                Ok(value)
            }
            Err(mpsc::error::TryRecvError::Disconnected) => {
                self.metrics.mark_closed();
                Err(mpsc::error::TryRecvError::Disconnected)
            }
            Err(error) => Err(error),
        }
    }

    #[cfg(test)]
    pub fn close(&mut self) {
        self.metrics.mark_closed();
        self.inner.close();
    }

    pub fn cancel(&mut self) {
        self.metrics.mark_cancelled();
        self.inner.close();
    }

    fn prefetch(&mut self) {
        debug_assert!(self.prefetched.is_none());
        match self.inner.try_recv() {
            Ok(item) => {
                self.metrics.observe_oldest_timestamp(item.timestamp());
                self.prefetched = Some(item);
            }
            Err(mpsc::error::TryRecvError::Empty) => {}
            Err(mpsc::error::TryRecvError::Disconnected) => self.metrics.mark_closed(),
        }
    }
}

impl<T> Drop for TrackedReceiver<T> {
    fn drop(&mut self) {
        self.metrics.mark_closed();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrackedSendErrorKind {
    Closed,
    Cancelled,
    Full,
    ByteLimit,
}

#[derive(Debug)]
pub struct TrackedSendError {
    pub kind: TrackedSendErrorKind,
}

#[derive(Debug)]
pub struct TrackedTrySendError<T> {
    pub kind: TrackedSendErrorKind,
    pub value: T,
}

pub fn tracked_channel<T>(metrics: Arc<QueueMetrics>) -> (TrackedSender<T>, TrackedReceiver<T>) {
    assert!(
        metrics.registered,
        "tracked channel requires registered metrics"
    );
    let item_capacity =
        usize::try_from(metrics.item_capacity).expect("tracked queue item capacity fits usize");
    let byte_capacity =
        usize::try_from(metrics.byte_capacity).expect("tracked queue byte capacity fits usize");
    assert!(
        byte_capacity <= u32::MAX as usize,
        "tracked queue byte capacity must fit Tokio semaphore permits"
    );
    let (sender, receiver) = mpsc::channel(item_capacity);
    let byte_budget = Arc::new(Semaphore::new(byte_capacity));
    let item_budget = Arc::new(Semaphore::new(item_capacity));
    let lifetime = Arc::new(TrackedSenderLifetime {
        metrics: Arc::clone(&metrics),
    });
    (
        TrackedSender {
            inner: sender,
            byte_budget,
            item_budget,
            metrics: Arc::clone(&metrics),
            lifetime,
        },
        TrackedReceiver {
            inner: receiver,
            metrics,
            prefetched: None,
        },
    )
}

fn lock_unpoisoned<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn saturating_increment(counter: &AtomicU64, value: u64) {
    let _ = counter.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
        Some(current.saturating_add(value))
    });
}

fn saturating_subtract(counter: &AtomicU64, value: u64) -> u64 {
    counter
        .fetch_update(Ordering::AcqRel, Ordering::Acquire, |current| {
            Some(current.saturating_sub(value))
        })
        .unwrap_or_else(|current| current)
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicUsize;

    use tokio::time::advance;

    use super::*;

    #[tokio::test]
    async fn enqueue_dequeue_drop_and_close_zero_items_and_bytes() {
        let metrics = QueueMetrics::new(QueueKind::TunToTransport, 2, 32);
        let (sender, mut receiver) = tracked_channel(Arc::clone(&metrics));
        sender.send(vec![1_u8; 7], 7).await.unwrap();
        sender.send(vec![2_u8; 5], 5).await.unwrap();
        let snapshot = metrics.snapshot(Instant::now());
        assert_eq!(snapshot.current_items, 2);
        assert_eq!(snapshot.current_bytes, 12);
        assert_eq!(snapshot.items_high_water, 2);
        assert_eq!(snapshot.bytes_high_water, 12);

        assert_eq!(receiver.recv().await.unwrap().len(), 7);
        drop(receiver);
        drop(sender);
        tokio::task::yield_now().await;

        let snapshot = metrics.snapshot(Instant::now());
        assert_eq!(snapshot.current_items, 0);
        assert_eq!(snapshot.current_bytes, 0);
        assert_eq!(snapshot.dequeue_count, 1);
        assert_eq!(snapshot.drop_items, 1);
        assert_eq!(snapshot.drop_bytes, 5);
        assert!(snapshot.closed);
    }

    #[tokio::test(start_paused = true)]
    async fn prefetched_entry_keeps_its_item_capacity_and_cancellation_releases_reservations() {
        let metrics = QueueMetrics::new(QueueKind::TunToTransport, 2, 32);
        let (sender, mut receiver) = tracked_channel(Arc::clone(&metrics));
        sender.try_send(1_u8, 1).unwrap();
        sender.try_send(2_u8, 1).unwrap();
        assert_eq!(receiver.recv().await, Some(1));
        sender.try_send(3_u8, 1).unwrap();
        assert_eq!(sender.capacity(), 0);
        assert_eq!(
            sender.try_send(4_u8, 1).unwrap_err().kind,
            TrackedSendErrorKind::Full
        );
        let cancel = CancellationToken::new();
        let pending = sender.send_cancellable(4_u8, 1, &cancel);
        tokio::pin!(pending);
        tokio::select! {
            result = &mut pending => panic!("full queue completed unexpectedly: {result:?}"),
            _ = tokio::task::yield_now() => {}
        }
        cancel.cancel();
        assert_eq!(
            pending.await.unwrap_err().kind,
            TrackedSendErrorKind::Cancelled
        );
        assert_eq!(receiver.recv().await, Some(2));
        sender.send(5_u8, 1).await.unwrap();
        assert_eq!(receiver.recv().await, Some(3));
        assert_eq!(receiver.recv().await, Some(5));
        let snapshot = metrics.snapshot(Instant::now());
        assert_eq!(snapshot.items_high_water, 2);
        assert_eq!(snapshot.current_items, 0);
        assert_eq!(sender.capacity(), 2);
    }

    #[tokio::test(start_paused = true)]
    async fn oldest_age_uses_the_fifo_head_under_paused_time() {
        let metrics = QueueMetrics::new(QueueKind::TransportToTun, 2, 32);
        let (sender, mut receiver) = tracked_channel(Arc::clone(&metrics));
        sender.send(1_u8, 1).await.unwrap();
        advance(Duration::from_secs(2)).await;
        sender.send(2_u8, 1).await.unwrap();
        advance(Duration::from_secs(3)).await;
        assert_eq!(
            metrics.snapshot(Instant::now()).oldest_age,
            Some(Duration::from_secs(5))
        );
        assert_eq!(receiver.recv().await, Some(1));
        assert_eq!(
            metrics.snapshot(Instant::now()).oldest_age,
            Some(Duration::from_secs(3))
        );
    }

    #[test]
    fn high_water_updates_are_monotonic_across_threads() {
        let metrics = QueueMetrics::new(QueueKind::H3WireSend, 64, 64);
        std::thread::scope(|scope| {
            for _ in 0..32 {
                let metrics = Arc::clone(&metrics);
                scope.spawn(move || {
                    metrics.items_high_water.fetch_max(17, Ordering::Relaxed);
                    metrics.bytes_high_water.fetch_max(31, Ordering::Relaxed);
                });
            }
        });
        let snapshot = metrics.snapshot(Instant::now());
        assert_eq!(snapshot.items_high_water, 17);
        assert_eq!(snapshot.bytes_high_water, 31);
        assert_eq!(snapshot.current_items, 0);
        assert_eq!(snapshot.current_bytes, 0);
    }

    #[tokio::test]
    async fn dropping_a_tracked_receiver_releases_every_payload() {
        struct DropProbe(Arc<AtomicUsize>);

        impl Drop for DropProbe {
            fn drop(&mut self) {
                self.0.fetch_add(1, Ordering::Relaxed);
            }
        }

        let drops = Arc::new(AtomicUsize::new(0));
        let metrics = QueueMetrics::new(QueueKind::ProxyToTransport, 4, 4);
        let (sender, receiver) = tracked_channel(Arc::clone(&metrics));
        for _ in 0..4 {
            assert!(sender.send(DropProbe(Arc::clone(&drops)), 1).await.is_ok());
        }
        drop(receiver);
        drop(sender);
        tokio::task::yield_now().await;
        assert_eq!(drops.load(Ordering::Relaxed), 4);
        assert_eq!(metrics.snapshot(Instant::now()).current_items, 0);
    }

    #[tokio::test]
    async fn cancellation_is_bounded_and_does_not_enqueue() {
        let metrics = QueueMetrics::new(QueueKind::DirectDnsRequests, 1, 1);
        let (sender, _receiver) = tracked_channel::<u8>(Arc::clone(&metrics));
        sender.send(1_u8, 1).await.unwrap();
        let cancellation = CancellationToken::new();
        cancellation.cancel();
        let error = sender
            .send_cancellable(2_u8, 1, &cancellation)
            .await
            .unwrap_err();
        assert_eq!(error.kind, TrackedSendErrorKind::Cancelled);
        let snapshot = metrics.snapshot(Instant::now());
        assert_eq!(snapshot.current_items, 1);
        assert_eq!(snapshot.drop_items, 1);
    }

    #[tokio::test]
    async fn rejected_try_send_is_not_counted_as_an_enqueue() {
        let metrics = QueueMetrics::new(QueueKind::TransportToProxy, 1, 2);
        let (sender, _receiver) = tracked_channel(Arc::clone(&metrics));
        sender.try_send(1_u8, 1).unwrap();
        let error = sender.try_send(2_u8, 1).unwrap_err();
        assert_eq!(error.kind, TrackedSendErrorKind::Full);

        let snapshot = metrics.snapshot(Instant::now());
        assert_eq!(snapshot.enqueue_count, 1);
        assert_eq!(snapshot.drop_items, 1);
        assert_eq!(snapshot.current_items, 1);
        assert_eq!(snapshot.current_bytes, 1);
    }

    #[tokio::test(start_paused = true)]
    async fn unordered_entries_publish_the_next_oldest_age() {
        let metrics = QueueMetrics::new_unordered(QueueKind::DirectDnsRequests, 3, 3);
        let first = metrics.start_entry(1);
        advance(Duration::from_secs(2)).await;
        let second = metrics.start_entry(1);
        advance(Duration::from_secs(3)).await;

        first.complete();
        assert_eq!(
            metrics.snapshot(Instant::now()).oldest_age,
            Some(Duration::from_secs(3))
        );
        second.complete();
        assert_eq!(metrics.snapshot(Instant::now()).oldest_age, None);
    }

    #[tokio::test]
    async fn last_sender_drop_marks_the_queue_closed() {
        let metrics = QueueMetrics::new(QueueKind::TransportOutgoingPackets, 1, 1);
        let (sender, _receiver) = tracked_channel::<u8>(Arc::clone(&metrics));
        let clone = sender.clone();
        drop(sender);
        assert!(!metrics.snapshot(Instant::now()).closed);
        drop(clone);
        assert!(metrics.snapshot(Instant::now()).closed);
    }
}
