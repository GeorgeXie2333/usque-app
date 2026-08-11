/// Per-socket TCP receive and transmit buffer sizes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TcpBufferTier {
    /// Receive buffer size in bytes.
    pub receive: usize,
    /// Transmit buffer size in bytes.
    pub transmit: usize,
}

impl TcpBufferTier {
    /// Total memory reserved by one socket in this tier.
    pub const fn total(self) -> Option<usize> {
        self.receive.checked_add(self.transmit)
    }
}

/// Bounded TCP buffer allocation policy.
///
/// New sockets use the preferred tier while both its own budget and the total
/// budget allow it. Under pressure they transparently fall back to the smaller
/// tier. Once the total budget is exhausted, creating another socket fails
/// without disturbing existing connections.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TcpBufferPolicy {
    /// Normal high-throughput socket buffers.
    pub preferred: TcpBufferTier,
    /// Reduced buffers used under memory pressure.
    pub fallback: TcpBufferTier,
    /// Maximum bytes assigned to preferred-tier sockets.
    pub preferred_budget: usize,
    /// Maximum bytes assigned to all TCP sockets.
    pub total_budget: usize,
}

/// Live counters for the bounded TCP buffer allocator.
#[derive(Clone, Default)]
pub struct TcpBufferMetrics {
    inner: Arc<TcpBufferMetricsInner>,
}

#[derive(Default)]
struct TcpBufferMetricsInner {
    preferred_bytes: AtomicUsize,
    total_bytes: AtomicUsize,
    preferred_sockets: AtomicUsize,
    fallback_sockets: AtomicUsize,
    rejected_sockets: AtomicUsize,
}

/// Point-in-time bounded TCP buffer usage.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct TcpBufferMetricsSnapshot {
    /// Bytes currently held by preferred-tier sockets.
    pub preferred_bytes: usize,
    /// Bytes currently held by all TCP sockets.
    pub total_bytes: usize,
    /// Number of current preferred-tier sockets.
    pub preferred_sockets: usize,
    /// Number of current fallback-tier sockets.
    pub fallback_sockets: usize,
    /// Number of socket creations rejected since the stack started.
    pub rejected_sockets: usize,
}

impl TcpBufferMetrics {
    /// Read a consistent-enough diagnostics snapshot. Each field is monotonic
    /// or advisory and does not participate in allocation decisions.
    pub fn snapshot(&self) -> TcpBufferMetricsSnapshot {
        TcpBufferMetricsSnapshot {
            preferred_bytes: self.inner.preferred_bytes.load(Ordering::Relaxed),
            total_bytes: self.inner.total_bytes.load(Ordering::Relaxed),
            preferred_sockets: self.inner.preferred_sockets.load(Ordering::Relaxed),
            fallback_sockets: self.inner.fallback_sockets.load(Ordering::Relaxed),
            rejected_sockets: self.inner.rejected_sockets.load(Ordering::Relaxed),
        }
    }

    pub(crate) fn update(
        &self,
        preferred_bytes: usize,
        total_bytes: usize,
        preferred_sockets: usize,
        fallback_sockets: usize,
        rejected_sockets: usize,
    ) {
        self.inner
            .preferred_bytes
            .store(preferred_bytes, Ordering::Relaxed);
        self.inner.total_bytes.store(total_bytes, Ordering::Relaxed);
        self.inner
            .preferred_sockets
            .store(preferred_sockets, Ordering::Relaxed);
        self.inner
            .fallback_sockets
            .store(fallback_sockets, Ordering::Relaxed);
        self.inner
            .rejected_sockets
            .store(rejected_sockets, Ordering::Relaxed);
    }
}

impl fmt::Debug for TcpBufferMetrics {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.snapshot().fmt(formatter)
    }
}

impl PartialEq for TcpBufferMetrics {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }
}

impl Eq for TcpBufferMetrics {}

impl Hash for TcpBufferMetrics {
    fn hash<H: Hasher>(&self, state: &mut H) {
        Arc::as_ptr(&self.inner).hash(state);
    }
}

/// Netstack configuration.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Config {
    /// Capacity of the command channel.
    ///
    /// If `None`, the channel is unbounded.
    pub command_channel_capacity: Option<usize>,

    /// Maximum transmission unit of the underlying net device.
    pub mtu: usize,

    /// Assign the IPv4 and IPv6 loopback addresses to the interface.
    pub loopback: bool,

    /// The default size of buffer allocated for each UDP socket created.
    pub udp_buffer_size: usize,
    /// The default number of pending messages supported for each UDP socket created.
    pub udp_message_count: usize,

    /// The default size of buffer allocated for each TCP socket created.
    pub tcp_buffer_size: usize,

    /// Optional bounded, asymmetric TCP buffer policy.
    ///
    /// When absent, [`Config::tcp_buffer_size`] is used for both directions
    /// with no explicit per-stack budget, preserving upstream behavior.
    pub tcp_buffer_policy: Option<TcpBufferPolicy>,

    /// Optional shared diagnostics observer for TCP buffer allocation.
    pub tcp_buffer_metrics: Option<TcpBufferMetrics>,

    /// Whether Nagle's algorithm is enabled on newly created TCP sockets.
    pub tcp_nagle_enabled: bool,

    /// The default size of buffer allocated for each raw socket.
    pub raw_buffer_size: usize,
    /// The default number of pending messages supported for each raw socket.
    pub raw_message_count: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            command_channel_capacity: Some(32),

            mtu: 1500,

            loopback: false,

            udp_buffer_size: 1024 * 4,
            udp_message_count: 32,

            tcp_buffer_size: 1024 * 16,
            tcp_buffer_policy: None,
            tcp_buffer_metrics: None,
            tcp_nagle_enabled: true,

            raw_buffer_size: 1024 * 4,
            raw_message_count: 32,
        }
    }
}
use alloc::sync::Arc;
use core::fmt;
use core::hash::{Hash, Hasher};
use core::sync::atomic::{AtomicUsize, Ordering};
