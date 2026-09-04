//! Bounded HTTP Datagram buffer ownership for quiche.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use bytes::Bytes;

use crate::network_quality::NetworkQualityTelemetry;

pub(crate) const HTTP_DATAGRAM_ENCODE_POOL_LIMIT: usize = 128;
pub(crate) const HTTP_DATAGRAM_BUFFER_CAPACITY: usize = 2_048;
pub(crate) const HTTP_DATAGRAM_ENCODE_POOL_BYTE_BUDGET: usize =
    HTTP_DATAGRAM_ENCODE_POOL_LIMIT * HTTP_DATAGRAM_BUFFER_CAPACITY;
pub(crate) const H3_DATAGRAM_ENCODER_STRATEGY: &str = "ownership_transfer";

#[derive(Clone, Debug, Default)]
pub(crate) struct H3BufferFactory;

impl quiche::BufFactory for H3BufferFactory {
    type Buf = Bytes;
    type DgramBuf = PooledDatagramBuffer;

    fn buf_from_slice(buffer: &[u8]) -> Self::Buf {
        Bytes::copy_from_slice(buffer)
    }

    fn dgram_buf_from_slice(buffer: &[u8]) -> Self::DgramBuf {
        PooledDatagramBuffer::from(buffer.to_vec())
    }
}

#[derive(Clone)]
pub(crate) struct DatagramEncodePool {
    inner: Arc<DatagramEncodePoolInner>,
}

struct DatagramEncodePoolInner {
    free: Mutex<Vec<Vec<u8>>>,
    allocated: AtomicUsize,
    quality: NetworkQualityTelemetry,
}

impl DatagramEncodePool {
    pub(crate) fn new(quality: NetworkQualityTelemetry) -> Self {
        Self {
            inner: Arc::new(DatagramEncodePoolInner {
                free: Mutex::new(Vec::with_capacity(HTTP_DATAGRAM_ENCODE_POOL_LIMIT)),
                allocated: AtomicUsize::new(0),
                quality,
            }),
        }
    }

    pub(crate) fn take(&self) -> Option<PooledDatagramBuffer> {
        if let Some(bytes) = self.inner.free().pop() {
            self.inner.quality.record_packet_buffer_pool_hit();
            self.inner.quality.record_encode_buffer_reuse();
            return Some(PooledDatagramBuffer {
                bytes: Some(bytes),
                pool: Some(Arc::clone(&self.inner)),
            });
        }
        self.inner.quality.record_packet_buffer_pool_miss();
        let reserved = self
            .inner
            .allocated
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |allocated| {
                (allocated < HTTP_DATAGRAM_ENCODE_POOL_LIMIT).then_some(allocated + 1)
            })
            .is_ok();
        if !reserved {
            return None;
        }
        self.inner.quality.record_fresh_allocation();
        Some(PooledDatagramBuffer {
            bytes: Some(Vec::with_capacity(HTTP_DATAGRAM_BUFFER_CAPACITY)),
            pool: Some(Arc::clone(&self.inner)),
        })
    }

    #[cfg(test)]
    fn allocated(&self) -> usize {
        self.inner.allocated.load(Ordering::Acquire)
    }

    #[cfg(test)]
    fn free_count(&self) -> usize {
        self.inner.free().len()
    }
}

impl std::fmt::Debug for DatagramEncodePool {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("DatagramEncodePool")
            .field("strategy", &H3_DATAGRAM_ENCODER_STRATEGY)
            .field("limit", &HTTP_DATAGRAM_ENCODE_POOL_LIMIT)
            .field("byte_budget", &HTTP_DATAGRAM_ENCODE_POOL_BYTE_BUDGET)
            .field("allocated", &self.inner.allocated.load(Ordering::Relaxed))
            .finish_non_exhaustive()
    }
}

impl DatagramEncodePoolInner {
    fn free(&self) -> MutexGuard<'_, Vec<Vec<u8>>> {
        self.free
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn recycle(&self, mut bytes: Vec<u8>) {
        if bytes.capacity() > HTTP_DATAGRAM_BUFFER_CAPACITY {
            zero_buffer(&mut bytes);
            self.allocated.fetch_sub(1, Ordering::AcqRel);
            return;
        }
        bytes.clear();
        let mut free = self.free();
        if free.len() < HTTP_DATAGRAM_ENCODE_POOL_LIMIT {
            free.push(bytes);
            self.quality.record_buffer_recycle();
        } else {
            zero_buffer(&mut bytes);
            self.allocated.fetch_sub(1, Ordering::AcqRel);
        }
    }
}

impl Drop for DatagramEncodePoolInner {
    fn drop(&mut self) {
        let free = self
            .free
            .get_mut()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        for bytes in free {
            zero_buffer(bytes);
        }
    }
}

fn zero_buffer(bytes: &mut Vec<u8>) {
    bytes.resize(bytes.capacity(), 0);
    bytes.fill(0);
    bytes.clear();
}

pub(crate) struct PooledDatagramBuffer {
    bytes: Option<Vec<u8>>,
    pool: Option<Arc<DatagramEncodePoolInner>>,
}

impl PooledDatagramBuffer {
    pub(crate) fn bytes_mut(&mut self) -> &mut Vec<u8> {
        self.bytes
            .as_mut()
            .expect("HTTP Datagram buffer remains present until drop")
    }

    pub(crate) fn into_bytes(mut self) -> Bytes {
        if self.pool.is_some() {
            return Bytes::copy_from_slice(self.as_ref());
        }
        Bytes::from(
            self.bytes
                .take()
                .expect("unpooled HTTP Datagram buffer remains present"),
        )
    }
}

impl AsRef<[u8]> for PooledDatagramBuffer {
    fn as_ref(&self) -> &[u8] {
        self.bytes
            .as_deref()
            .expect("HTTP Datagram buffer remains present until drop")
    }
}

impl From<Vec<u8>> for PooledDatagramBuffer {
    fn from(bytes: Vec<u8>) -> Self {
        Self {
            bytes: Some(bytes),
            pool: None,
        }
    }
}

impl std::fmt::Debug for PooledDatagramBuffer {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PooledDatagramBuffer")
            .field("length", &self.as_ref().len())
            .field("pooled", &self.pool.is_some())
            .finish_non_exhaustive()
    }
}

impl Drop for PooledDatagramBuffer {
    fn drop(&mut self) {
        if let (Some(bytes), Some(pool)) = (self.bytes.take(), self.pool.take()) {
            pool.recycle(bytes);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::network_quality::NetworkQualitySampler;
    use quiche::BufFactory as _;
    use std::path::Path;

    #[derive(Debug, PartialEq, Eq)]
    struct EncoderCost {
        fresh_buffer_allocations: u64,
        downstream_copy_allocations: u64,
        total_allocations: u64,
        copied_bytes: u64,
    }

    fn benchmark_encoder(
        iterations: usize,
        payload_bytes: usize,
        downstream_copy: bool,
    ) -> EncoderCost {
        let quality = NetworkQualityTelemetry::default();
        let pool = DatagramEncodePool::new(quality.clone());
        let payload = vec![0x5a_u8; payload_bytes];
        let mut copied_bytes = 0_u64;
        let mut downstream_copy_allocations = 0_u64;
        for _ in 0..iterations {
            let mut buffer = pool
                .take()
                .expect("sequential benchmark recycles its buffer");
            buffer.bytes_mut().extend_from_slice(&[0, 0]);
            buffer.bytes_mut().extend_from_slice(&payload);
            copied_bytes = copied_bytes.saturating_add(buffer.as_ref().len() as u64);
            if downstream_copy {
                let copied = std::hint::black_box(buffer.as_ref().to_vec());
                copied_bytes = copied_bytes.saturating_add(copied.len() as u64);
                downstream_copy_allocations = downstream_copy_allocations.saturating_add(1);
                std::hint::black_box(copied);
            }
            drop(buffer);
        }
        let fresh_buffer_allocations = NetworkQualitySampler::new(quality)
            .sample()
            .allocations
            .fresh_allocations;
        EncoderCost {
            fresh_buffer_allocations,
            downstream_copy_allocations,
            total_allocations: fresh_buffer_allocations.saturating_add(downstream_copy_allocations),
            copied_bytes,
        }
    }

    #[test]
    fn pool_reuses_success_error_and_drop_paths() {
        let quality = NetworkQualityTelemetry::default();
        let pool = DatagramEncodePool::new(quality.clone());
        let allocation = {
            let mut buffer = pool.take().unwrap();
            buffer.bytes_mut().extend_from_slice(b"packet");
            buffer.as_ref().as_ptr()
        };
        assert_eq!(pool.free_count(), 1);

        let reused = pool.take().unwrap();
        assert_eq!(reused.as_ref().as_ptr(), allocation);
        assert!(reused.as_ref().is_empty());
        drop(reused);

        let snapshot = NetworkQualitySampler::new(quality).sample();
        assert_eq!(snapshot.allocations.packet_buffer_pool_misses, 1);
        assert_eq!(snapshot.allocations.packet_buffer_pool_hits, 1);
        assert_eq!(snapshot.allocations.encode_buffer_reuses, 1);
        assert_eq!(snapshot.allocations.fresh_allocations, 1);
        assert_eq!(snapshot.allocations.buffer_recycles, 2);
    }

    #[test]
    fn pool_limit_is_hard_and_recovery_is_nonblocking() {
        assert_eq!(HTTP_DATAGRAM_ENCODE_POOL_BYTE_BUDGET, 256 * 1024);
        let quality = NetworkQualityTelemetry::default();
        let pool = DatagramEncodePool::new(quality);
        let buffers: Vec<_> = (0..HTTP_DATAGRAM_ENCODE_POOL_LIMIT)
            .map(|_| pool.take().unwrap())
            .collect();
        assert_eq!(pool.allocated(), HTTP_DATAGRAM_ENCODE_POOL_LIMIT);
        assert!(pool.take().is_none());
        drop(buffers);
        assert_eq!(pool.free_count(), HTTP_DATAGRAM_ENCODE_POOL_LIMIT);
        assert!(pool.take().is_some());
    }

    #[test]
    fn connection_close_waits_for_owned_buffers_then_releases_the_pool() {
        let pool = DatagramEncodePool::new(NetworkQualityTelemetry::default());
        let weak = Arc::downgrade(&pool.inner);
        let buffers: Vec<_> = (0..8).map(|_| pool.take().unwrap()).collect();

        drop(pool);
        assert!(weak.upgrade().is_some());
        drop(buffers);
        assert!(weak.upgrade().is_none());
    }

    #[test]
    fn bytes_clones_cannot_create_mutable_aliases() {
        let bytes = H3BufferFactory::buf_from_slice(b"immutable");
        let clone = bytes.clone();
        assert!(bytes.try_into_mut().is_err());
        assert_eq!(clone, Bytes::from_static(b"immutable"));
    }

    #[test]
    fn inbound_factory_buffer_transfers_to_bytes_without_another_copy() {
        let buffer = H3BufferFactory::dgram_buf_from_slice(b"received");
        let allocation = buffer.as_ref().as_ptr();
        let bytes = buffer.into_bytes();
        assert_eq!(bytes.as_ptr(), allocation);
        assert_eq!(bytes, Bytes::from_static(b"received"));
    }

    #[test]
    fn debug_never_exposes_datagram_bytes() {
        let buffer = PooledDatagramBuffer::from(b"secret-payload".to_vec());
        let debug = format!("{buffer:?}");
        assert!(debug.contains("length"));
        assert!(!debug.contains("secret-payload"));
    }

    #[test]
    fn deterministic_strategy_benchmark_reproduces_the_checked_in_fixture() {
        let fixture_path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tool/fixtures/performance/h3-encoder-strategies.json");
        let fixture: serde_json::Value =
            serde_json::from_slice(&std::fs::read(fixture_path).unwrap()).unwrap();
        let iterations = fixture["iterations"].as_u64().unwrap() as usize;
        let payload_bytes = fixture["payload_bytes"].as_u64().unwrap() as usize;
        let header_bytes = fixture["header_bytes"].as_u64().unwrap() as usize;
        assert_eq!(header_bytes, 2);
        let ownership = benchmark_encoder(iterations, payload_bytes, false);
        let staging = benchmark_encoder(iterations, payload_bytes, true);

        for (name, actual) in [
            ("ownership_transfer", ownership),
            ("pooled_staging_copy", staging),
        ] {
            let expected = &fixture["results"][name];
            assert_eq!(
                actual,
                EncoderCost {
                    fresh_buffer_allocations: expected["fresh_buffer_allocations"]
                        .as_u64()
                        .unwrap(),
                    downstream_copy_allocations: expected["downstream_copy_allocations"]
                        .as_u64()
                        .unwrap(),
                    total_allocations: expected["total_allocations"].as_u64().unwrap(),
                    copied_bytes: expected["copied_bytes"].as_u64().unwrap(),
                }
            );
            assert_eq!(
                expected["allocations_per_packet"].as_f64().unwrap(),
                actual.total_allocations as f64 / iterations as f64
            );
            assert_eq!(
                expected["copy_bytes_per_packet"].as_u64().unwrap(),
                actual.copied_bytes / iterations as u64
            );
        }
        assert_eq!(fixture["selected"], H3_DATAGRAM_ENCODER_STRATEGY);
    }
}
