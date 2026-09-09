//! Atomic L4 performance counters. No traffic, targets, or per-packet logging.
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use ts_netstack_smoltcp::netcore::TcpBufferMetrics;
use usque_core::{L4PerformanceSnapshot, L4QueueSnapshot, L4WaitSnapshot};

#[derive(Default)]
pub(crate) struct WaitHistogram {
    seen: AtomicU64,
    samples: AtomicU64,
    sum_us: AtomicU64,
    max_us: AtomicU64,
    buckets: [AtomicU64; 32],
}
impl WaitHistogram {
    pub(crate) fn begin(&self) -> Option<Instant> {
        (self.seen.fetch_add(1, Ordering::Relaxed) & 63 == 0).then(Instant::now)
    }
    pub(crate) fn finish(&self, started: Option<Instant>) {
        if let Some(started) = started {
            self.record(started.elapsed().as_micros().min(u128::from(u64::MAX)) as u64);
        }
    }
    fn record(&self, micros: u64) {
        let bucket = (64 - micros.leading_zeros()).min(31) as usize;
        self.buckets[bucket].fetch_add(1, Ordering::Relaxed);
        self.samples.fetch_add(1, Ordering::Relaxed);
        let _ = self
            .sum_us
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| {
                Some(n.saturating_add(micros))
            });
        self.max_us.fetch_max(micros, Ordering::Relaxed);
    }
    fn snapshot(&self) -> L4WaitSnapshot {
        L4WaitSnapshot {
            samples: self.samples.load(Ordering::Relaxed),
            sum_us: self.sum_us.load(Ordering::Relaxed),
            max_us: self.max_us.load(Ordering::Relaxed),
            buckets: std::array::from_fn(|i| self.buckets[i].load(Ordering::Relaxed)),
        }
    }
}

#[derive(Default)]
pub(crate) struct QueuePerformance {
    packets: AtomicU64,
    bytes: AtomicU64,
    high_water_packets: AtomicU64,
    high_water_bytes: AtomicU64,
    wait: WaitHistogram,
}
impl QueuePerformance {
    fn ticket(self: &Arc<Self>, bytes: usize) -> QueueTicket {
        let packets = self.packets.fetch_add(1, Ordering::Relaxed) + 1;
        let total = self.bytes.fetch_add(bytes as u64, Ordering::Relaxed) + bytes as u64;
        self.high_water_packets
            .fetch_max(packets, Ordering::Relaxed);
        self.high_water_bytes.fetch_max(total, Ordering::Relaxed);
        QueueTicket {
            meter: self.clone(),
            bytes: bytes as u64,
            started: self.wait.begin(),
        }
    }
    fn snapshot(&self) -> L4QueueSnapshot {
        L4QueueSnapshot {
            packets: self.packets.load(Ordering::Relaxed),
            bytes: self.bytes.load(Ordering::Relaxed),
            high_water_packets: self.high_water_packets.load(Ordering::Relaxed),
            high_water_bytes: self.high_water_bytes.load(Ordering::Relaxed),
            wait: self.wait.snapshot(),
        }
    }
}
struct QueueTicket {
    meter: Arc<QueuePerformance>,
    bytes: u64,
    started: Option<Instant>,
}
impl Drop for QueueTicket {
    fn drop(&mut self) {
        self.meter.packets.fetch_sub(1, Ordering::Relaxed);
        self.meter.bytes.fetch_sub(self.bytes, Ordering::Relaxed);
        self.meter.wait.finish(self.started);
    }
}
pub(crate) struct QueuedPacket {
    bytes: bytes::Bytes,
    _ticket: Option<QueueTicket>,
}
impl QueuedPacket {
    pub(crate) fn new(bytes: bytes::Bytes, meter: Option<&Arc<QueuePerformance>>) -> Self {
        let ticket = meter.map(|meter| meter.ticket(bytes.len()));
        Self {
            bytes,
            _ticket: ticket,
        }
    }
    pub(crate) fn into_bytes(self) -> bytes::Bytes {
        self.bytes
    }
}
#[derive(Clone)]
pub(crate) struct MeasuredSender {
    sender: tokio::sync::mpsc::Sender<QueuedPacket>,
    meter: Arc<QueuePerformance>,
}
impl MeasuredSender {
    pub(crate) fn new(
        sender: tokio::sync::mpsc::Sender<QueuedPacket>,
        meter: Arc<QueuePerformance>,
    ) -> Self {
        Self { sender, meter }
    }
    pub(crate) async fn send(&self, packet: bytes::Bytes) -> Result<(), ()> {
        let permit = self.sender.reserve().await.map_err(|_| ())?;
        permit.send(QueuedPacket::new(packet, Some(&self.meter)));
        Ok(())
    }
    pub(crate) fn try_send(&self, packet: bytes::Bytes) -> Result<(), ()> {
        self.sender
            .try_reserve()
            .map_err(|_| ())?
            .send(QueuedPacket::new(packet, Some(&self.meter)));
        Ok(())
    }
}
#[derive(Default)]
struct Metadata {
    udp: Option<(u64, Option<u64>, Option<u64>)>,
    tun: Option<(u32, TcpBufferMetrics)>,
}
#[derive(Default)]
pub(crate) struct Performance {
    pub(crate) h3_read_calls: AtomicU64,
    pub(crate) h3_read_bytes: AtomicU64,
    pub(crate) h3_empty_reads: AtomicU64,
    pub(crate) receive_pool_allocations: AtomicU64,
    pub(crate) receive_pool_hits: AtomicU64,
    pub(crate) receive_pool_evictions: AtomicU64,
    pub(crate) receive_pool_idle_bytes: AtomicU64,
    pub(crate) receive_pool_idle_high_watermark: AtomicU64,
    pub(crate) receive_pool_live_bytes: AtomicU64,
    pub(crate) receive_pool_live_high_watermark: AtomicU64,
    pub(crate) adapter_copied_bytes: AtomicU64,
    pub(crate) tcp_accepted_bytes: AtomicU64,
    pub(crate) tcp_write_calls: AtomicU64,
    pub(crate) tcp_partial_writes: AtomicU64,
    pub(crate) actor_wakeups: AtomicU64,
    pub(crate) actor_polls: AtomicU64,
    pub(crate) actor_no_progress_polls: AtomicU64,
    pub(crate) actor_no_progress_wakeups: AtomicU64,
    pub(crate) budget_wakeups: AtomicU64,
    pub(crate) tun_ingress_packets: AtomicU64,
    pub(crate) tun_ingress_bytes: AtomicU64,
    pub(crate) tun_egress_packets: AtomicU64,
    pub(crate) tun_egress_bytes: AtomicU64,
    pub(crate) tun_write_calls: AtomicU64,
    pub(crate) tun_write_would_block: AtomicU64,
    pub(crate) command_wait: WaitHistogram,
    tun_write_wait: WaitHistogram,
    pub(crate) tun_ingress: Arc<QueuePerformance>,
    pub(crate) tun_egress: Arc<QueuePerformance>,
    pub(crate) stack_ingress: Arc<QueuePerformance>,
    pub(crate) stack_egress: Arc<QueuePerformance>,
    pub(crate) active_epoch: AtomicU64,
    platform_writer: AtomicBool,
    metadata: Mutex<Metadata>,
    cached: Mutex<(u64, L4PerformanceSnapshot)>,
}
impl Performance {
    pub(crate) fn observe_udp(&self, epoch: u64, sizes: (Option<u64>, Option<u64>)) {
        if epoch != 0 && epoch == self.active_epoch.load(Ordering::Acquire) {
            self.metadata.lock().unwrap_or_else(|e| e.into_inner()).udp =
                Some((epoch, sizes.0, sizes.1));
        }
    }
    pub(crate) fn observe_tun(&self, mtu: u16, tcp: TcpBufferMetrics) {
        self.metadata.lock().unwrap_or_else(|e| e.into_inner()).tun = Some((u32::from(mtu), tcp));
    }
    /// Called by the existing once-per-second runtime maintenance task.
    pub(crate) fn sample(&self) -> L4PerformanceSnapshot {
        let mut value = L4PerformanceSnapshot {
            h3_read_calls: self.h3_read_calls.load(Ordering::Relaxed),
            h3_read_bytes: self.h3_read_bytes.load(Ordering::Relaxed),
            h3_empty_reads: self.h3_empty_reads.load(Ordering::Relaxed),
            receive_pool_allocations: self.receive_pool_allocations.load(Ordering::Relaxed),
            receive_pool_hits: self.receive_pool_hits.load(Ordering::Relaxed),
            receive_pool_evictions: self.receive_pool_evictions.load(Ordering::Relaxed),
            receive_pool_idle_bytes: self.receive_pool_idle_bytes.load(Ordering::Relaxed),
            receive_pool_idle_high_watermark: self
                .receive_pool_idle_high_watermark
                .load(Ordering::Relaxed),
            receive_pool_live_bytes: self.receive_pool_live_bytes.load(Ordering::Relaxed),
            receive_pool_live_high_watermark: self
                .receive_pool_live_high_watermark
                .load(Ordering::Relaxed),
            adapter_copied_bytes: self.adapter_copied_bytes.load(Ordering::Relaxed),
            tcp_accepted_bytes: self.tcp_accepted_bytes.load(Ordering::Relaxed),
            tcp_write_calls: self.tcp_write_calls.load(Ordering::Relaxed),
            tcp_partial_writes: self.tcp_partial_writes.load(Ordering::Relaxed),
            actor_wakeups: self.actor_wakeups.load(Ordering::Relaxed),
            actor_polls: self.actor_polls.load(Ordering::Relaxed),
            actor_no_progress_polls: self.actor_no_progress_polls.load(Ordering::Relaxed),
            actor_no_progress_wakeups: self.actor_no_progress_wakeups.load(Ordering::Relaxed),
            budget_wakeups: self.budget_wakeups.load(Ordering::Relaxed),
            tun_ingress_packets: self.tun_ingress_packets.load(Ordering::Relaxed),
            tun_ingress_bytes: self.tun_ingress_bytes.load(Ordering::Relaxed),
            tun_egress_packets: self.tun_egress_packets.load(Ordering::Relaxed),
            tun_egress_bytes: self.tun_egress_bytes.load(Ordering::Relaxed),
            tun_write_calls: self
                .platform_writer
                .load(Ordering::Relaxed)
                .then(|| self.tun_write_calls.load(Ordering::Relaxed)),
            tun_write_would_block: self
                .platform_writer
                .load(Ordering::Relaxed)
                .then(|| self.tun_write_would_block.load(Ordering::Relaxed)),
            command_wait: self.command_wait.snapshot(),
            tun_write_wait: self.tun_write_wait.snapshot(),
            ..L4PerformanceSnapshot::default()
        };
        let epoch = self.active_epoch.load(Ordering::Acquire);
        let meta = self.metadata.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((observed, receive, send)) = meta.udp
            && observed == epoch
            && epoch != 0
        {
            value.udp_receive_buffer_bytes = receive;
            value.udp_send_buffer_bytes = send;
            value.udp_buffer_source = Some("getsockopt_raw".to_owned());
        }
        if let Some((mtu, tcp)) = &meta.tun {
            let tcp = tcp.snapshot();
            value.tun_mtu = Some(*mtu);
            value.tun_mtu_source = Some("applied_profile".to_owned());
            value.tcp_preferred_sockets = Some(tcp.preferred_sockets as u64);
            value.tcp_fallback_sockets = Some(tcp.fallback_sockets as u64);
            value.tcp_buffer_bytes = Some(tcp.total_bytes as u64);
            value.tun_ingress_queue = Some(self.tun_ingress.snapshot());
            value.tun_egress_queue = Some(self.tun_egress.snapshot());
            value.stack_ingress_queue = Some(self.stack_ingress.snapshot());
            value.stack_egress_queue = Some(self.stack_egress.snapshot());
        }
        drop(meta);
        *self.cached.lock().unwrap_or_else(|e| e.into_inner()) = (epoch, value.clone());
        value
    }
    pub(crate) fn snapshot(&self) -> L4PerformanceSnapshot {
        let (epoch, mut value) = self
            .cached
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        if epoch != self.active_epoch.load(Ordering::Acquire) {
            value.udp_receive_buffer_bytes = None;
            value.udp_send_buffer_bytes = None;
            value.udp_buffer_source = None;
        }
        value
    }
}

/// A platform-only observer; it never controls packet forwarding or policy.
#[derive(Clone)]
pub struct TunWriteObserver(pub(crate) Arc<Performance>);
pub struct TunWriteSample(Option<Instant>);
impl TunWriteObserver {
    pub(crate) fn new(perf: Arc<Performance>) -> Self {
        perf.platform_writer.store(true, Ordering::Relaxed);
        Self(perf)
    }
    pub fn begin(&self) -> TunWriteSample {
        TunWriteSample(self.0.tun_write_wait.begin())
    }
    pub fn finish(&self, sample: TunWriteSample) {
        self.0.tun_write_wait.finish(sample.0);
    }
    pub fn syscall(&self, would_block: bool) {
        self.0.tun_write_calls.fetch_add(1, Ordering::Relaxed);
        if would_block {
            self.0.tun_write_would_block.fetch_add(1, Ordering::Relaxed);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn histogram_is_bounded_sampled_and_never_invents_missing_metadata() {
        let histogram = WaitHistogram::default();
        assert_eq!((0..128).filter(|_| histogram.begin().is_some()).count(), 2);
        for value in [0, 1, 2, 3, u64::MAX] {
            histogram.record(value);
        }
        let sampled = histogram.snapshot();
        assert_eq!(sampled.samples, 5);
        assert_eq!(sampled.buckets.iter().sum::<u64>(), 5);
        let value = Performance::default().sample();
        assert!(value.udp_receive_buffer_bytes.is_none());
        assert!(value.tun_mtu.is_none());
        assert!(value.tun_write_calls.is_none());
    }
    #[test]
    fn queue_ticket_is_reclaimed_on_delivery_or_channel_drop() {
        let meter = Arc::new(QueuePerformance::default());
        let bytes = bytes::Bytes::from(vec![7; 8]);
        let pointer = bytes.as_ptr();
        let packet = QueuedPacket::new(bytes, Some(&meter));
        assert_eq!(meter.snapshot().bytes, 8);
        let bytes = packet.into_bytes();
        assert_eq!(bytes.as_ptr(), pointer);
        assert!(bytes.try_into_mut().is_ok());
        assert_eq!(meter.snapshot().packets, 0);
        drop(QueuedPacket::new(
            bytes::Bytes::from_static(b"closed"),
            Some(&meter),
        ));
        assert_eq!(meter.snapshot().bytes, 0);
    }
    #[test]
    fn stale_session_cannot_publish_udp_buffer_observations() {
        let perf = Performance::default();
        perf.active_epoch.store(1, Ordering::Release);
        perf.observe_udp(1, (Some(123), Some(456)));
        assert_eq!(perf.sample().udp_receive_buffer_bytes, Some(123));
        perf.active_epoch.store(2, Ordering::Release);
        assert!(perf.snapshot().udp_receive_buffer_bytes.is_none());
        perf.observe_udp(1, (Some(999), Some(999)));
        assert!(perf.sample().udp_receive_buffer_bytes.is_none());
    }
}
