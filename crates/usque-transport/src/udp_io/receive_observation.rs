//! Bounded QUIC receive-buffer policy and socket-local observations.
//! No addresses, payloads, file descriptors, connection identifiers,
//! force-buffer options or system-wide settings.
use super::UdpBatchMode;
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
use tokio::net::UdpSocket;
use usque_core::L4ReceiveSnapshot;

pub(crate) const RECEIVE_BUFFER_TARGET: usize = 2 * 1024 * 1024;

pub(crate) const fn production_target() -> Option<usize> {
    if cfg!(any(target_os = "windows", target_os = "android")) {
        Some(RECEIVE_BUFFER_TARGET)
    } else {
        None
    }
}

static NEXT_SOCKET: AtomicU64 = AtomicU64::new(1);
const UNKNOWN_DROPS: u64 = u64::MAX;

pub(crate) struct ReceiveObservation {
    pub(crate) id: u64,
    requested: Option<u64>,
    target: Option<u64>,
    request_status: &'static str,
    overflow_enabled: bool,
    drops: AtomicU64,
    reports: AtomicU64,
    ancillary_errors: AtomicU64,
    syscalls: AtomicU64,
    datagrams: AtomicU64,
    empty: AtomicU64,
}

#[derive(Clone)]
pub(crate) struct ReceiveSource {
    pub(crate) observation: Arc<ReceiveObservation>,
    pub(crate) receive_mode: UdpBatchMode,
    pub(crate) send_mode: UdpBatchMode,
}
impl std::fmt::Debug for ReceiveSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ReceiveSource")
            .field("receive_mode", &self.receive_mode)
            .field("send_mode", &self.send_mode)
            .finish_non_exhaustive()
    }
}
/// Read-only metadata; it owns counters, never the socket or egress lease.
#[derive(Debug, Clone)]
pub(crate) struct SocketReceiveState {
    pub(crate) sizes: (Option<u64>, Option<u64>),
    pub(crate) source: ReceiveSource,
}
impl SocketReceiveState {
    pub(crate) fn snapshot(&self) -> crate::network_quality::SocketReceiveQuality {
        crate::network_quality::SocketReceiveQuality {
            receive_buffer_bytes: self.sizes.0,
            send_buffer_bytes: self.sizes.1,
            observation: self.source.snapshot(),
        }
    }
}
impl ReceiveSource {
    pub(crate) fn snapshot(&self) -> L4ReceiveSnapshot {
        let observation = &self.observation;
        let native = self.receive_mode == UdpBatchMode::SendMmsgRecvMmsg;
        let drops = observation.drops.load(Ordering::Relaxed);
        L4ReceiveSnapshot {
            buffer_target_bytes: observation.target,
            requested_buffer_bytes: observation.requested,
            buffer_request_status: Some(observation.request_status.to_owned()),
            overflow_monitoring: Some(
                if !native {
                    "unavailable_backend"
                } else if observation.overflow_enabled {
                    "enabled"
                } else {
                    "unavailable"
                }
                .to_owned(),
            ),
            socket_drops_reported: (native
                && observation.overflow_enabled
                && drops != UNKNOWN_DROPS)
                .then_some(drops),
            overflow_reports: observation.reports.load(Ordering::Relaxed),
            ancillary_errors: observation.ancillary_errors.load(Ordering::Relaxed),
            recv_syscalls: observation.syscalls.load(Ordering::Relaxed),
            received_datagrams: observation.datagrams.load(Ordering::Relaxed),
            empty_recv_syscalls: observation.empty.load(Ordering::Relaxed),
            receive_backend: Some(if native { "recvmmsg" } else { "portable" }.to_owned()),
            send_backend: Some(
                if self.send_mode == UdpBatchMode::SendMmsgRecvMmsg {
                    "sendmmsg"
                } else {
                    "portable"
                }
                .to_owned(),
            ),
            ..L4ReceiveSnapshot::default()
        }
    }
}

impl ReceiveObservation {
    pub(crate) fn attach(socket: &UdpSocket, requested: Option<usize>) -> Arc<Self> {
        let current = socket2::SockRef::from(socket).recv_buffer_size().ok();
        let (called, request_status) = apply_buffer_target(
            requested,
            current,
            cfg!(any(target_os = "android", target_os = "linux")),
            |bytes| socket2::SockRef::from(socket).set_recv_buffer_size(bytes),
        );
        Arc::new(Self {
            id: NEXT_SOCKET.fetch_add(1, Ordering::Relaxed),
            requested: called.map(|n| n as u64),
            target: requested.map(|n| n as u64),
            request_status,
            overflow_enabled: enable_overflow(socket),
            drops: AtomicU64::new(UNKNOWN_DROPS),
            reports: AtomicU64::new(0),
            ancillary_errors: AtomicU64::new(0),
            syscalls: AtomicU64::new(0),
            datagrams: AtomicU64::new(0),
            empty: AtomicU64::new(0),
        })
    }
    pub(super) fn record_recv(&self, count: usize) {
        self.syscalls.fetch_add(1, Ordering::Relaxed);
        self.datagrams.fetch_add(count as u64, Ordering::Relaxed);
        if count == 0 {
            self.empty.fetch_add(1, Ordering::Relaxed);
        }
    }
    #[cfg(any(test, target_os = "android", target_os = "linux"))]
    pub(super) fn overflow_enabled(&self) -> bool {
        self.overflow_enabled
    }

    #[cfg(any(test, target_os = "android", target_os = "linux"))]
    pub(super) fn record_control(&self, control: ControlResult) {
        match control {
            ControlResult::Absent => {} // absence is NOT proof of zero socket loss
            ControlResult::Invalid => {
                self.ancillary_errors.fetch_add(1, Ordering::Relaxed);
            }
            ControlResult::Drops(raw) => {
                self.reports.fetch_add(1, Ordering::Relaxed);
                let old = self.drops.load(Ordering::Relaxed);
                let total = if old == UNKNOWN_DROPS {
                    u64::from(raw)
                } else {
                    let delta = raw.wrapping_sub(old as u32);
                    if delta > u32::MAX / 2 {
                        self.ancillary_errors.fetch_add(1, Ordering::Relaxed);
                        return;
                    }
                    old.saturating_add(u64::from(delta)).min(UNKNOWN_DROPS - 1)
                };
                // Exactly one receive task owns a socket; sampling is read-only.
                self.drops.store(total, Ordering::Relaxed);
            }
        }
    }
}

fn apply_buffer_target(
    target: Option<usize>,
    current_raw: Option<usize>,
    linux_accounting: bool,
    set: impl FnOnce(usize) -> std::io::Result<()>,
) -> (Option<usize>, &'static str) {
    let Some(target) = target else {
        return (None, "not_requested");
    };
    let threshold = target.saturating_mul(if linux_accounting { 2 } else { 1 });
    if current_raw.is_some_and(|bytes| bytes >= threshold) {
        return (None, "already_sufficient");
    }
    (
        Some(target),
        if set(target).is_ok() {
            "accepted"
        } else {
            "rejected"
        },
    )
}

#[cfg(any(target_os = "android", target_os = "linux"))]
fn enable_overflow(socket: &UdpSocket) -> bool {
    use std::{mem, os::fd::AsRawFd};
    let enabled: libc::c_int = 1;
    // SAFETY: the socket remains owned; enabled is initialized and lives for
    // this synchronous call. This option adds counters, not network authority.
    let result = unsafe {
        libc::setsockopt(
            socket.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_RXQ_OVFL,
            std::ptr::from_ref(&enabled).cast(),
            mem::size_of_val(&enabled) as libc::socklen_t,
        )
    };
    result == 0
}
#[cfg(not(any(target_os = "android", target_os = "linux")))]
fn enable_overflow(_socket: &UdpSocket) -> bool {
    false
}

#[cfg(any(test, target_os = "android", target_os = "linux"))]
#[derive(Clone, Copy)]
#[repr(C, align(8))]
pub(super) struct ControlBuffer(pub(super) [u8; 32]);
#[cfg(any(test, target_os = "android", target_os = "linux"))]
#[derive(Debug, PartialEq, Eq)]
pub(super) enum ControlResult {
    Absent,
    Drops(u32),
    Invalid,
}

/// Byte-based CMSG parsing avoids unaligned reads and unchecked pointer walks.
#[cfg(any(test, target_os = "android", target_os = "linux"))]
pub(super) fn parse_control(
    bytes: &[u8],
    returned: usize,
    truncated: bool,
    word: usize,
    socket_level: i32,
    drop_type: i32,
) -> ControlResult {
    if truncated || returned > bytes.len() || ![4, 8].contains(&word) {
        return ControlResult::Invalid;
    }
    let bytes = &bytes[..returned];
    let header = word + 8;
    let mut offset = 0;
    let mut result = ControlResult::Absent;
    while offset < bytes.len() {
        if bytes.len() - offset < header {
            return ControlResult::Invalid;
        }
        let len = if word == 8 {
            u64::from_ne_bytes(
                bytes[offset..offset + 8]
                    .try_into()
                    .expect("checked header"),
            )
        } else {
            u64::from(u32::from_ne_bytes(
                bytes[offset..offset + 4]
                    .try_into()
                    .expect("checked header"),
            ))
        };
        let Ok(len) = usize::try_from(len) else {
            return ControlResult::Invalid;
        };
        if len < header || len > bytes.len() - offset {
            return ControlResult::Invalid;
        }
        let level = i32::from_ne_bytes(
            bytes[offset + word..offset + word + 4]
                .try_into()
                .expect("checked header"),
        );
        let kind = i32::from_ne_bytes(
            bytes[offset + word + 4..offset + header]
                .try_into()
                .expect("checked header"),
        );
        if level == socket_level && kind == drop_type {
            if len != header + 4 || result != ControlResult::Absent {
                return ControlResult::Invalid;
            }
            result = ControlResult::Drops(u32::from_ne_bytes(
                bytes[offset + header..offset + len]
                    .try_into()
                    .expect("checked payload"),
            ));
        }
        let Some(aligned) = len.checked_add(word - 1).map(|n| n & !(word - 1)) else {
            return ControlResult::Invalid;
        };
        if aligned > bytes.len() - offset {
            break;
        } // final CMSG may omit alignment padding
        offset += aligned;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn production_target_is_platform_scoped() {
        let expected = if cfg!(any(target_os = "windows", target_os = "android")) {
            Some(2 << 20)
        } else {
            None
        };
        assert_eq!(production_target(), expected);
    }
    #[test]
    fn larger_buffers_are_preserved_and_request_failure_is_observable() {
        for linux in [false, true] {
            let threshold = if linux { 4 << 20 } else { 2 << 20 };
            assert_eq!(
                apply_buffer_target(Some(2 << 20), Some(threshold), linux, |_| panic!(
                    "must not shrink"
                )),
                (None, "already_sufficient")
            );
            assert_eq!(
                apply_buffer_target(Some(2 << 20), Some(threshold * 2), linux, |_| panic!(
                    "must not shrink"
                )),
                (None, "already_sufficient")
            );
            assert_eq!(
                apply_buffer_target(Some(2 << 20), Some(224 << 10), linux, |_| Err(
                    std::io::ErrorKind::PermissionDenied.into()
                )),
                (Some(2 << 20), "rejected")
            );
            assert_eq!(
                apply_buffer_target(Some(2 << 20), None, linux, |size| {
                    assert_eq!(size, 2 << 20);
                    Ok(())
                }),
                (Some(2 << 20), "accepted")
            );
        }
        assert_eq!(
            apply_buffer_target(None, None, false, |_| panic!("no explicit request")),
            (None, "not_requested")
        );
    }
    #[test]
    fn ordinary_request_never_retries_with_a_force_option() {
        let mut calls = 0;
        let result = apply_buffer_target(Some(2 << 20), Some(224 << 10), true, |_| {
            calls += 1;
            Err(std::io::ErrorKind::PermissionDenied.into())
        });
        assert_eq!(calls, 1);
        assert_eq!(result, (Some(2 << 20), "rejected"));
    }
    fn message(word: usize, drops: u32) -> ControlBuffer {
        let mut buffer = ControlBuffer([0; 32]);
        let length = word + 12;
        if word == 8 {
            buffer.0[..8].copy_from_slice(&(length as u64).to_ne_bytes());
        } else {
            buffer.0[..4].copy_from_slice(&(length as u32).to_ne_bytes());
        }
        buffer.0[word..word + 4].copy_from_slice(&1i32.to_ne_bytes());
        buffer.0[word + 4..word + 8].copy_from_slice(&40i32.to_ne_bytes());
        buffer.0[word + 8..length].copy_from_slice(&drops.to_ne_bytes());
        buffer
    }
    #[test]
    fn parses_native_32_and_64_bit_headers_without_unaligned_access() {
        for word in [4, 8] {
            let buffer = message(word, 42);
            let length = word + 12;
            assert_eq!(
                parse_control(&buffer.0, length, false, word, 1, 40),
                ControlResult::Drops(42)
            );
            assert_eq!(
                parse_control(&buffer.0, length, true, word, 1, 40),
                ControlResult::Invalid
            );
            assert_eq!(
                parse_control(&buffer.0, 33, false, word, 1, 40),
                ControlResult::Invalid
            );
            assert_eq!(
                parse_control(&[], 0, false, word, 1, 40),
                ControlResult::Absent
            );
            for size in 1..length {
                assert_eq!(
                    parse_control(&buffer.0, size, false, word, 1, 40),
                    ControlResult::Invalid
                );
            }
            assert_eq!(
                parse_control(&buffer.0, length, false, word, 2, 40),
                ControlResult::Absent
            );
        }
    }
    #[tokio::test]
    async fn counters_keep_missing_loss_unknown_and_handle_wrap_and_socket_reset() {
        let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let mut observation = ReceiveObservation::attach(&socket, None);
        Arc::get_mut(&mut observation).unwrap().overflow_enabled = true;
        let source = ReceiveSource {
            observation: observation.clone(),
            receive_mode: UdpBatchMode::SendMmsgRecvMmsg,
            send_mode: UdpBatchMode::SendMmsgRecvMmsg,
        };
        assert!(observation.overflow_enabled());
        observation.record_control(ControlResult::Absent);
        assert!(source.snapshot().socket_drops_reported.is_none());
        observation.record_control(ControlResult::Drops(u32::MAX));
        observation.record_control(ControlResult::Drops(3));
        observation.record_control(ControlResult::Invalid);
        observation.record_recv(32);
        observation.record_recv(0);
        let snapshot = source.snapshot();
        assert_eq!(
            snapshot.socket_drops_reported,
            Some(u64::from(u32::MAX) + 4)
        );
        assert_eq!(
            (
                snapshot.recv_syscalls,
                snapshot.received_datagrams,
                snapshot.empty_recv_syscalls
            ),
            (2, 32, 1)
        );
        assert_eq!(snapshot.ancillary_errors, 1);
        let replacement = ReceiveObservation::attach(&socket, None);
        assert_ne!(replacement.id, observation.id);
        assert_eq!(replacement.drops.load(Ordering::Relaxed), UNKNOWN_DROPS);
        let portable = ReceiveSource {
            receive_mode: UdpBatchMode::Portable,
            ..source
        };
        assert!(portable.snapshot().socket_drops_reported.is_none());
    }
    #[tokio::test]
    async fn two_megabyte_request_is_separate_from_actual_os_buffer_and_send_buffer() {
        let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let send = socket2::SockRef::from(&socket).send_buffer_size().unwrap();
        let observation = ReceiveObservation::attach(&socket, Some(2 << 20));
        let source = ReceiveSource {
            observation,
            receive_mode: UdpBatchMode::SendMmsgRecvMmsg,
            send_mode: UdpBatchMode::SendMmsgRecvMmsg,
        };
        let snapshot = source.snapshot();
        assert_eq!(snapshot.buffer_target_bytes, Some(2 << 20));
        assert!(matches!(
            snapshot.buffer_request_status.as_deref(),
            Some("accepted" | "already_sufficient")
        ));
        assert_eq!(
            snapshot.requested_buffer_bytes,
            (snapshot.buffer_request_status.as_deref() == Some("accepted")).then_some(2 << 20)
        );
        assert!(socket2::SockRef::from(&socket).recv_buffer_size().unwrap() > 0);
        assert_eq!(
            socket2::SockRef::from(&socket).send_buffer_size().unwrap(),
            send
        );
        assert_eq!(source.snapshot().send_backend.as_deref(), Some("sendmmsg"));
    }
    proptest::proptest! {
        #[test]
        fn malformed_control_bytes_are_bounded_and_panic_free(bytes in proptest::collection::vec(proptest::prelude::any::<u8>(), 0..128), length in 0usize..160, word in proptest::sample::select(vec![4usize, 8])) {
            let _ = parse_control(&bytes, length, false, word, 1, 40);
        }
    }
}
