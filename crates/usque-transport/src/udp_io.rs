//! Safe UDP batch-I/O boundary used by the HTTP/3 actor.
//!
//! Linux and Android use `sendmmsg`/`recvmmsg`. Other targets, and a socket
//! that observes an explicit unsupported-syscall error, use the portable Tokio
//! readiness path. No raw descriptor or unsafe API escapes this module.

use std::collections::VecDeque;
use std::io;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Instant;

use tokio::io::Interest;
use tokio::net::UdpSocket;
use tokio_util::sync::CancellationToken;

use crate::network_quality::NetworkQualityTelemetry;

mod portable;
#[cfg(any(target_os = "android", target_os = "linux"))]
mod unix_batch;

/// Maximum accepted outer UDP payload. The portable path owns one additional
/// sentinel byte solely to turn otherwise-silent truncation into a hard error.
pub const UDP_RECEIVE_SLOT_SIZE: usize = 2_048;
const PORTABLE_RECEIVE_STORAGE_SIZE: usize = UDP_RECEIVE_SLOT_SIZE + 1;
pub(crate) const UDP_BATCH_SIZE: usize = 32;
pub(crate) const UDP_ACTOR_DRAIN_LIMIT: usize = 64;
// Audited for the PR-08 maximum of active, probing, and draining sockets. The
// current H3 actor owns one socket; the future socket set can share this pool.
const MAX_UDP_SOCKET_COUNT: usize = 3;
const RECEIVE_POOL_LIMIT: usize = UDP_BATCH_SIZE * MAX_UDP_SOCKET_COUNT * 2;
const RECEIVE_POOL_BYTE_BUDGET: usize = RECEIVE_POOL_LIMIT * PORTABLE_RECEIVE_STORAGE_SIZE;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum UdpBatchMode {
    Portable = 0,
    SendMmsgRecvMmsg = 1,
}

impl UdpBatchMode {
    fn from_u8(value: u8) -> Self {
        match value {
            1 => Self::SendMmsgRecvMmsg,
            _ => Self::Portable,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum UdpBatchFallbackReason {
    SyscallUnavailable = 1,
    OperationNotSupported = 2,
}

impl UdpBatchFallbackReason {
    fn code(self) -> &'static str {
        match self {
            Self::SyscallUnavailable => "syscall_unavailable",
            Self::OperationNotSupported => "operation_not_supported",
        }
    }
}

#[derive(Clone, Copy)]
pub struct SendDatagram<'a> {
    pub payload: &'a [u8],
    pub source: SocketAddr,
    pub destination: SocketAddr,
    pub due_at: Instant,
}

pub struct ReceivedDatagram {
    pub buffer: PooledUdpBuffer,
    pub length: usize,
    pub source: SocketAddr,
    pub destination: SocketAddr,
}

struct UdpBufferPool {
    free: Mutex<VecDeque<Box<[u8; PORTABLE_RECEIVE_STORAGE_SIZE]>>>,
}

impl UdpBufferPool {
    fn new() -> Self {
        Self {
            free: Mutex::new(VecDeque::with_capacity(RECEIVE_POOL_LIMIT)),
        }
    }

    fn free(&self) -> MutexGuard<'_, VecDeque<Box<[u8; PORTABLE_RECEIVE_STORAGE_SIZE]>>> {
        self.free
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn acquire(self: &Arc<Self>, quality: &NetworkQualityTelemetry) -> PooledUdpBuffer {
        let storage = match self.free().pop_back() {
            Some(storage) => {
                quality.record_packet_buffer_pool_hit();
                storage
            }
            None => {
                quality.record_packet_buffer_pool_miss();
                quality.record_fresh_allocation();
                Box::new([0; PORTABLE_RECEIVE_STORAGE_SIZE])
            }
        };
        PooledUdpBuffer {
            storage: Some(storage),
            pool: Arc::clone(self),
            quality: quality.clone(),
        }
    }

    fn recycle(&self, mut storage: Box<[u8; PORTABLE_RECEIVE_STORAGE_SIZE]>) {
        let mut free = self.free();
        if free.len() < RECEIVE_POOL_LIMIT {
            free.push_back(storage);
        } else {
            storage.fill(0);
        }
    }

    #[cfg(test)]
    fn free_count(&self) -> usize {
        self.free().len()
    }
}

impl Drop for UdpBufferPool {
    fn drop(&mut self) {
        let free = self
            .free
            .get_mut()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        for storage in free {
            storage.fill(0);
        }
    }
}

pub struct PooledUdpBuffer {
    storage: Option<Box<[u8; PORTABLE_RECEIVE_STORAGE_SIZE]>>,
    pool: Arc<UdpBufferPool>,
    quality: NetworkQualityTelemetry,
}

impl PooledUdpBuffer {
    fn as_slice(&self, length: usize) -> &[u8] {
        &self
            .storage
            .as_deref()
            .expect("pooled UDP storage remains present until drop")[..length]
    }

    fn as_mut_slice(&mut self, length: usize) -> &mut [u8] {
        &mut self
            .storage
            .as_deref_mut()
            .expect("pooled UDP storage remains present until drop")[..length]
    }

    pub(crate) fn portable_storage_mut(&mut self) -> &mut [u8] {
        self.storage
            .as_deref_mut()
            .expect("pooled UDP storage remains present until drop")
    }

    #[cfg(any(target_os = "android", target_os = "linux"))]
    pub(crate) fn batch_storage_mut(&mut self) -> &mut [u8] {
        &mut self
            .storage
            .as_deref_mut()
            .expect("pooled UDP storage remains present until drop")[..UDP_RECEIVE_SLOT_SIZE]
    }
}

impl ReceivedDatagram {
    pub fn payload(&self) -> &[u8] {
        self.buffer.as_slice(self.length)
    }

    pub fn payload_mut(&mut self) -> &mut [u8] {
        self.buffer.as_mut_slice(self.length)
    }
}

impl Drop for PooledUdpBuffer {
    fn drop(&mut self) {
        if let Some(storage) = self.storage.take() {
            self.pool.recycle(storage);
            self.quality.record_buffer_recycle();
        }
    }
}

pub struct RecvBatch {
    pool: Arc<UdpBufferPool>,
    datagrams: Vec<ReceivedDatagram>,
}

impl RecvBatch {
    fn new(pool: Arc<UdpBufferPool>) -> Self {
        Self {
            pool,
            datagrams: Vec::with_capacity(UDP_ACTOR_DRAIN_LIMIT),
        }
    }

    pub fn len(&self) -> usize {
        self.datagrams.len()
    }

    pub fn is_empty(&self) -> bool {
        self.datagrams.is_empty()
    }

    pub fn drain(&mut self) -> std::vec::Drain<'_, ReceivedDatagram> {
        self.datagrams.drain(..)
    }

    fn clear(&mut self) {
        self.datagrams.clear();
    }

    pub(super) fn acquire_buffer(&self, quality: &NetworkQualityTelemetry) -> PooledUdpBuffer {
        self.pool.acquire(quality)
    }

    pub(super) fn push(&mut self, datagram: ReceivedDatagram) {
        debug_assert!(self.datagrams.len() < UDP_ACTOR_DRAIN_LIMIT);
        self.datagrams.push(datagram);
    }
}

pub struct UdpBatchIo {
    socket: UdpSocket,
    local_address: SocketAddr,
    mode: AtomicU8,
    fallback_reason: AtomicU8,
    pool: Arc<UdpBufferPool>,
    quality: NetworkQualityTelemetry,
}

impl std::fmt::Debug for SendDatagram<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SendDatagram")
            .field("payload_length", &self.payload.len())
            .field("source_family", &address_family(self.source))
            .field("destination_family", &address_family(self.destination))
            .finish_non_exhaustive()
    }
}

impl std::fmt::Debug for ReceivedDatagram {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ReceivedDatagram")
            .field("length", &self.length)
            .field("source_family", &address_family(self.source))
            .field("destination_family", &address_family(self.destination))
            .finish_non_exhaustive()
    }
}

impl std::fmt::Debug for PooledUdpBuffer {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PooledUdpBuffer")
            .field("capacity", &UDP_RECEIVE_SLOT_SIZE)
            .finish_non_exhaustive()
    }
}

impl std::fmt::Debug for RecvBatch {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RecvBatch")
            .field("datagram_count", &self.datagrams.len())
            .finish_non_exhaustive()
    }
}

impl std::fmt::Debug for UdpBatchIo {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("UdpBatchIo")
            .field("mode", &self.mode())
            .field("fallback_reason", &self.fallback_reason())
            .field("receive_pool_byte_budget", &RECEIVE_POOL_BYTE_BUDGET)
            .finish_non_exhaustive()
    }
}

impl UdpBatchIo {
    pub fn new(socket: UdpSocket, quality: NetworkQualityTelemetry) -> io::Result<Self> {
        Self::with_mode(socket, default_batch_mode(), quality)
    }

    pub fn with_mode(
        socket: UdpSocket,
        mode: UdpBatchMode,
        quality: NetworkQualityTelemetry,
    ) -> io::Result<Self> {
        let local_address = socket.local_addr()?;
        Ok(Self {
            socket,
            local_address,
            mode: AtomicU8::new(mode as u8),
            fallback_reason: AtomicU8::new(0),
            pool: Arc::new(UdpBufferPool::new()),
            quality,
        })
    }

    pub fn mode(&self) -> UdpBatchMode {
        UdpBatchMode::from_u8(self.mode.load(Ordering::Acquire))
    }

    pub fn local_addr(&self) -> SocketAddr {
        self.local_address
    }

    pub fn new_recv_batch(&self) -> RecvBatch {
        RecvBatch::new(Arc::clone(&self.pool))
    }

    pub async fn recv_batch(
        &self,
        output: &mut RecvBatch,
        cancel: &CancellationToken,
    ) -> io::Result<usize> {
        output.clear();
        loop {
            tokio::select! {
                biased;
                _ = cancel.cancelled() => return Err(cancelled_error()),
                ready = self.socket.readable() => ready?,
            }
            let mode = self.mode();
            let result = self.socket.try_io(Interest::READABLE, || match mode {
                UdpBatchMode::Portable => portable::try_recv_batch(
                    &self.socket,
                    self.local_address,
                    output,
                    &self.quality,
                ),
                UdpBatchMode::SendMmsgRecvMmsg => self.try_recv_batch_unix(output),
            });
            match result {
                Ok(count) => return Ok(count),
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => continue,
                Err(error) if mode == UdpBatchMode::SendMmsgRecvMmsg => {
                    if let Some(reason) = batch_unavailable_reason(&error) {
                        self.switch_to_portable(reason);
                        continue;
                    }
                    return Err(error);
                }
                Err(error) => return Err(error),
            }
        }
    }

    pub async fn send_batch(
        &self,
        batch: &[SendDatagram<'_>],
        cancel: &CancellationToken,
    ) -> io::Result<usize> {
        let eligible = eligible_send_prefix(batch, self.local_address, Instant::now())?;
        if eligible == 0 {
            return Ok(0);
        }
        let batch = &batch[..eligible];
        loop {
            tokio::select! {
                biased;
                _ = cancel.cancelled() => return Err(cancelled_error()),
                ready = self.socket.writable() => ready?,
            }
            let mode = self.mode();
            let result = self.socket.try_io(Interest::WRITABLE, || match mode {
                UdpBatchMode::Portable => {
                    portable::try_send_batch(&self.socket, batch, &self.quality)
                }
                UdpBatchMode::SendMmsgRecvMmsg => self.try_send_batch_unix(batch),
            });
            match result {
                Ok(count) => return Ok(count),
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => continue,
                Err(error) if mode == UdpBatchMode::SendMmsgRecvMmsg => {
                    if let Some(reason) = batch_unavailable_reason(&error) {
                        self.switch_to_portable(reason);
                        continue;
                    }
                    return Err(error);
                }
                Err(error) => return Err(error),
            }
        }
    }

    #[cfg(any(target_os = "android", target_os = "linux"))]
    fn try_recv_batch_unix(&self, output: &mut RecvBatch) -> io::Result<usize> {
        unix_batch::try_recv_batch(&self.socket, self.local_address, output, &self.quality)
    }

    #[cfg(not(any(target_os = "android", target_os = "linux")))]
    fn try_recv_batch_unix(&self, _output: &mut RecvBatch) -> io::Result<usize> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "UDP batch receive is unavailable on this platform",
        ))
    }

    #[cfg(any(target_os = "android", target_os = "linux"))]
    fn try_send_batch_unix(&self, batch: &[SendDatagram<'_>]) -> io::Result<usize> {
        unix_batch::try_send_batch(&self.socket, batch, &self.quality)
    }

    #[cfg(not(any(target_os = "android", target_os = "linux")))]
    fn try_send_batch_unix(&self, _batch: &[SendDatagram<'_>]) -> io::Result<usize> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "UDP batch send is unavailable on this platform",
        ))
    }

    fn switch_to_portable(&self, reason: UdpBatchFallbackReason) {
        if self
            .mode
            .compare_exchange(
                UdpBatchMode::SendMmsgRecvMmsg as u8,
                UdpBatchMode::Portable as u8,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_ok()
        {
            self.fallback_reason.store(reason as u8, Ordering::Release);
            self.quality.record_udp_batch_fallback();
            tracing::warn!(
                reason_code = reason.code(),
                "UDP batch I/O is unavailable; this socket will use portable I/O"
            );
        }
    }

    pub fn fallback_reason(&self) -> Option<UdpBatchFallbackReason> {
        match self.fallback_reason.load(Ordering::Acquire) {
            1 => Some(UdpBatchFallbackReason::SyscallUnavailable),
            2 => Some(UdpBatchFallbackReason::OperationNotSupported),
            _ => None,
        }
    }
}

fn default_batch_mode() -> UdpBatchMode {
    if cfg!(any(target_os = "android", target_os = "linux")) {
        UdpBatchMode::SendMmsgRecvMmsg
    } else {
        UdpBatchMode::Portable
    }
}

fn address_family(address: SocketAddr) -> &'static str {
    if address.is_ipv4() { "ipv4" } else { "ipv6" }
}

fn cancelled_error() -> io::Error {
    io::Error::new(io::ErrorKind::Interrupted, "UDP batch I/O was cancelled")
}

fn batch_unavailable_reason(error: &io::Error) -> Option<UdpBatchFallbackReason> {
    #[cfg(any(target_os = "android", target_os = "linux"))]
    if let Some(code) = error.raw_os_error() {
        if code == libc::ENOSYS {
            return Some(UdpBatchFallbackReason::SyscallUnavailable);
        }
        if code == libc::EOPNOTSUPP || code == libc::ENOTSUP {
            return Some(UdpBatchFallbackReason::OperationNotSupported);
        }
    }
    if error.kind() == io::ErrorKind::Unsupported {
        return Some(UdpBatchFallbackReason::OperationNotSupported);
    }
    None
}

pub(crate) fn eligible_send_prefix(
    batch: &[SendDatagram<'_>],
    local_address: SocketAddr,
    now: Instant,
) -> io::Result<usize> {
    let Some(first) = batch.first() else {
        return Ok(0);
    };
    if first.source != local_address {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "UDP batch source does not match its socket path",
        ));
    }
    if first.due_at > now {
        return Ok(0);
    }
    let mut count = 0;
    for datagram in batch.iter().take(UDP_ACTOR_DRAIN_LIMIT) {
        if datagram.source != first.source
            || datagram.destination != first.destination
            || datagram.due_at > now
        {
            break;
        }
        count += 1;
    }
    Ok(count)
}

pub(super) fn receive_too_large_error() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "UDP datagram exceeded the 2048-byte receive bound",
    )
}

pub(crate) fn is_message_too_long(error: &io::Error) -> bool {
    #[cfg(unix)]
    if error.raw_os_error() == Some(libc::EMSGSIZE) {
        return true;
    }
    #[cfg(windows)]
    if error.raw_os_error() == Some(10_040) {
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::network_quality::NetworkQualitySampler;
    #[cfg(target_os = "linux")]
    use crate::network_quality::UdpIoQuality;
    use std::time::Duration;
    use tokio::time::timeout;

    fn datagram<'a>(
        payload: &'a [u8],
        source: SocketAddr,
        destination: SocketAddr,
        due_at: Instant,
    ) -> SendDatagram<'a> {
        SendDatagram {
            payload,
            source,
            destination,
            due_at,
        }
    }

    #[test]
    fn send_prefix_never_crosses_path_destination_or_future_deadline() {
        let source: SocketAddr = "127.0.0.1:10000".parse().unwrap();
        let first: SocketAddr = "127.0.0.1:20000".parse().unwrap();
        let second: SocketAddr = "127.0.0.1:30000".parse().unwrap();
        let now = Instant::now();
        let payload = [1_u8];
        let batch = [
            datagram(&payload, source, first, now),
            datagram(&payload, source, first, now),
            datagram(&payload, source, second, now),
            datagram(&payload, source, first, now + Duration::from_secs(1)),
        ];

        assert_eq!(eligible_send_prefix(&batch, source, now).unwrap(), 2);
        assert_eq!(eligible_send_prefix(&batch[3..], source, now).unwrap(), 0);
        assert!(eligible_send_prefix(&batch, second, now).is_err());
    }

    #[test]
    fn platform_message_too_large_error_is_classified_for_pmtu_revalidation() {
        #[cfg(windows)]
        let error = io::Error::from_raw_os_error(10_040);
        #[cfg(unix)]
        let error = io::Error::from_raw_os_error(libc::EMSGSIZE);

        assert!(is_message_too_long(&error));
        assert!(!is_message_too_long(&io::Error::from_raw_os_error(22)));
    }

    #[test]
    fn debug_output_never_contains_payloads_or_socket_addresses() {
        let source: SocketAddr = "127.0.0.1:10000".parse().unwrap();
        let destination: SocketAddr = "127.0.0.2:20000".parse().unwrap();
        let value = datagram(b"sensitive-payload", source, destination, Instant::now());

        let debug = format!("{value:?}");

        assert!(debug.contains("payload_length"));
        assert!(debug.contains("ipv4"));
        assert!(!debug.contains("sensitive-payload"));
        assert!(!debug.contains("127.0.0"));
        assert!(!debug.contains("10000"));
        assert!(!debug.contains("20000"));
    }

    #[tokio::test]
    async fn portable_send_and_receive_preserve_datagram_order() {
        let quality = NetworkQualityTelemetry::default();
        let receiver = UdpBatchIo::with_mode(
            UdpSocket::bind("127.0.0.1:0").await.unwrap(),
            UdpBatchMode::Portable,
            quality.clone(),
        )
        .unwrap();
        let sender = UdpBatchIo::with_mode(
            UdpSocket::bind("127.0.0.1:0").await.unwrap(),
            UdpBatchMode::Portable,
            quality,
        )
        .unwrap();
        let now = Instant::now();
        let payloads: [&[u8]; 3] = [b"one", b"two", b"three"];
        let batch: Vec<_> = payloads
            .iter()
            .map(|payload| datagram(payload, sender.local_addr(), receiver.local_addr(), now))
            .collect();
        let cancel = CancellationToken::new();

        assert_eq!(sender.send_batch(&batch, &cancel).await.unwrap(), 3);
        let mut received = receiver.new_recv_batch();
        assert_eq!(
            receiver.recv_batch(&mut received, &cancel).await.unwrap(),
            3
        );
        let actual: Vec<_> = received
            .drain()
            .map(|datagram| datagram.payload().to_vec())
            .collect();
        assert_eq!(actual, payloads);
    }

    #[tokio::test]
    async fn readiness_wait_is_released_immediately_by_cancellation() {
        let io = UdpBatchIo::with_mode(
            UdpSocket::bind("127.0.0.1:0").await.unwrap(),
            UdpBatchMode::Portable,
            NetworkQualityTelemetry::default(),
        )
        .unwrap();
        let mut output = io.new_recv_batch();
        let cancel = CancellationToken::new();
        let trigger = cancel.clone();
        tokio::spawn(async move {
            tokio::task::yield_now().await;
            trigger.cancel();
        });

        let error = timeout(
            Duration::from_millis(100),
            io.recv_batch(&mut output, &cancel),
        )
        .await
        .unwrap()
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::Interrupted);
    }

    #[test]
    fn invalid_argument_never_masquerades_as_batch_unavailable() {
        assert_eq!(
            batch_unavailable_reason(&io::Error::from_raw_os_error(libc::EINVAL)),
            None
        );
    }

    #[cfg(any(target_os = "android", target_os = "linux"))]
    #[test]
    fn only_explicit_unsupported_errors_enable_unix_fallback() {
        assert_eq!(
            batch_unavailable_reason(&io::Error::from_raw_os_error(libc::ENOSYS)),
            Some(UdpBatchFallbackReason::SyscallUnavailable)
        );
        assert_eq!(
            batch_unavailable_reason(&io::Error::from_raw_os_error(libc::EOPNOTSUPP)),
            Some(UdpBatchFallbackReason::OperationNotSupported)
        );
    }

    #[tokio::test]
    async fn oversized_receive_is_counted_and_never_silently_truncated() {
        let quality = NetworkQualityTelemetry::default();
        let receiver = UdpBatchIo::with_mode(
            UdpSocket::bind("127.0.0.1:0").await.unwrap(),
            UdpBatchMode::Portable,
            quality.clone(),
        )
        .unwrap();
        let sender = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        sender
            .send_to(
                &vec![7_u8; UDP_RECEIVE_SLOT_SIZE + 512],
                receiver.local_addr(),
            )
            .await
            .unwrap();
        let mut output = receiver.new_recv_batch();

        let error = receiver
            .recv_batch(&mut output, &CancellationToken::new())
            .await
            .unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert!(output.is_empty());
        assert_eq!(
            NetworkQualitySampler::new(quality)
                .sample()
                .udp_io
                .receive_truncations,
            1
        );
    }

    #[tokio::test]
    async fn fallback_transition_and_reason_are_recorded_only_once() {
        let quality = NetworkQualityTelemetry::default();
        let io = UdpBatchIo::with_mode(
            UdpSocket::bind("127.0.0.1:0").await.unwrap(),
            UdpBatchMode::SendMmsgRecvMmsg,
            quality.clone(),
        )
        .unwrap();

        io.switch_to_portable(UdpBatchFallbackReason::SyscallUnavailable);
        io.switch_to_portable(UdpBatchFallbackReason::OperationNotSupported);

        assert_eq!(io.mode(), UdpBatchMode::Portable);
        assert_eq!(
            io.fallback_reason(),
            Some(UdpBatchFallbackReason::SyscallUnavailable)
        );
        assert_eq!(
            NetworkQualitySampler::new(quality)
                .sample()
                .udp_io
                .batch_fallbacks,
            1
        );
    }

    #[cfg(not(any(target_os = "android", target_os = "linux")))]
    #[tokio::test]
    async fn unavailable_batch_backend_falls_back_once_for_the_socket_lifetime() {
        let quality = NetworkQualityTelemetry::default();
        let receiver = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let sender = UdpBatchIo::with_mode(
            UdpSocket::bind("127.0.0.1:0").await.unwrap(),
            UdpBatchMode::SendMmsgRecvMmsg,
            quality.clone(),
        )
        .unwrap();
        let now = Instant::now();
        let payload = [1_u8];
        let item = datagram(
            &payload,
            sender.local_addr(),
            receiver.local_addr().unwrap(),
            now,
        );
        let cancel = CancellationToken::new();

        assert_eq!(sender.send_batch(&[item], &cancel).await.unwrap(), 1);
        assert_eq!(sender.send_batch(&[item], &cancel).await.unwrap(), 1);

        assert_eq!(sender.mode(), UdpBatchMode::Portable);
        assert_eq!(
            sender.fallback_reason(),
            Some(UdpBatchFallbackReason::OperationNotSupported)
        );
        assert_eq!(
            NetworkQualitySampler::new(quality)
                .sample()
                .udp_io
                .batch_fallbacks,
            1
        );
    }

    #[tokio::test]
    async fn pooled_buffers_are_all_reclaimed_when_the_socket_closes() {
        assert_eq!(RECEIVE_POOL_LIMIT, 192);
        let quality = NetworkQualityTelemetry::default();
        let io = UdpBatchIo::with_mode(
            UdpSocket::bind("127.0.0.1:0").await.unwrap(),
            UdpBatchMode::Portable,
            quality.clone(),
        )
        .unwrap();
        let pool = Arc::clone(&io.pool);
        let buffers: Vec<_> = (0..UDP_ACTOR_DRAIN_LIMIT)
            .map(|_| pool.acquire(&quality))
            .collect();
        assert_eq!(pool.free_count(), 0);
        drop(io);
        drop(buffers);
        assert_eq!(pool.free_count(), UDP_ACTOR_DRAIN_LIMIT);
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn linux_batch_loopback_matches_portable_order() {
        async fn round_trip(mode: UdpBatchMode) -> (Vec<Vec<u8>>, UdpIoQuality) {
            let quality = NetworkQualityTelemetry::default();
            let receiver = UdpBatchIo::with_mode(
                UdpSocket::bind("127.0.0.1:0").await.unwrap(),
                mode,
                quality.clone(),
            )
            .unwrap();
            let sender = UdpBatchIo::with_mode(
                UdpSocket::bind("127.0.0.1:0").await.unwrap(),
                mode,
                quality.clone(),
            )
            .unwrap();
            let payloads: [&[u8]; 4] = [b"one", b"two", b"three", b"four"];
            let now = Instant::now();
            let batch: Vec<_> = payloads
                .iter()
                .map(|payload| datagram(payload, sender.local_addr(), receiver.local_addr(), now))
                .collect();
            let cancel = CancellationToken::new();
            assert_eq!(sender.send_batch(&batch, &cancel).await.unwrap(), 4);
            let mut received = receiver.new_recv_batch();
            assert_eq!(
                receiver.recv_batch(&mut received, &cancel).await.unwrap(),
                4
            );
            let payloads = received
                .drain()
                .map(|datagram| datagram.payload().to_vec())
                .collect();
            let udp_io = NetworkQualitySampler::new(quality).sample().udp_io;
            (payloads, udp_io)
        }

        let (portable, _) = round_trip(UdpBatchMode::Portable).await;
        let (batched, counters) = round_trip(UdpBatchMode::SendMmsgRecvMmsg).await;
        assert_eq!(portable, batched);
        assert!(
            counters.send_syscalls + counters.recv_syscalls
                <= (counters.sent_datagrams + counters.received_datagrams) / 2
        );
    }
}
