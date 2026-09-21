//! OpenVPN 3 without OS sockets or TUN devices. The caller transports framed
//! packets over its own TCP stream and owns all platform network configuration.
#[cfg(all(feature = "interop-test", not(debug_assertions), not(test)))]
compile_error!("the memory test peer must not be included in release binaries");
#[cfg(all(test, feature = "interop-test"))]
mod interop_tests;
use bytes::Bytes;
use std::ffi::{CString, c_char, c_void};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::ptr::NonNull;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tokio::sync::Notify;
use tokio::task::JoinHandle;

const MAX_PACKET: usize = u16::MAX as usize;
const MAX_CONFIG: usize = 128 * 1024;
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum Error {
    #[error("OpenVPN configuration is invalid or unsupported")]
    InvalidConfig,
    #[error("OpenVPN session is closed or belongs to another connection generation")]
    Closed,
    #[error("invalid OpenVPN or IP packet")]
    InvalidPacket,
    #[error("OpenVPN worker failed")]
    Worker,
    #[error("OpenVPN worker is still stopping")]
    ShutdownTimeout,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetworkConfig {
    pub ipv4: Option<Ipv4Addr>,
    pub ipv6: Option<Ipv6Addr>,
    pub mtu: u16,
    pub dns_servers: Vec<IpAddr>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    Dial {
        generation: u64,
    },
    TransportPacket {
        generation: u64,
        packet: Bytes,
    },
    IpPacket {
        generation: u64,
        packet: Bytes,
    },
    Network {
        generation: u64,
        config: NetworkConfig,
    },
    State {
        generation: u64,
        name: String,
        error: bool,
        fatal: bool,
    },
    Stopped,
}

#[repr(C)]
struct RawEvent {
    kind: u32,
    code: u32,
    generation: u64,
    length: u32,
    mtu: u32,
    ipv4: [c_char; 48],
    ipv6: [c_char; 48],
    dns: [[c_char; 48]; 8],
}

unsafe extern "C" {
    fn usque_ovpn_create(
        config: *const u8,
        length: usize,
        remote: *const c_char,
        port: u16,
        username: *const c_char,
        password: *const c_char,
        key_password: *const c_char,
        notify: extern "C" fn(*mut c_void),
        context: *mut c_void,
    ) -> *mut c_void;
    fn usque_ovpn_run(session: *mut c_void) -> i32;
    fn usque_ovpn_stop(session: *mut c_void);
    fn usque_ovpn_destroy(session: *mut c_void);
    fn usque_ovpn_push(
        session: *mut c_void,
        kind: u32,
        generation: u64,
        data: *const u8,
        length: usize,
    ) -> i32;
    fn usque_ovpn_pop(
        session: *mut c_void,
        event: *mut RawEvent,
        data: *mut u8,
        capacity: usize,
    ) -> i32;
    #[cfg(test)]
    fn usque_ovpn_event_size() -> usize;
}

struct Native {
    pointer: NonNull<c_void>,
    // The stable allocation is the callback context; it outlives destroy().
    notify: Box<Notify>,
    stopped: AtomicBool,
}

// SAFETY: The native ABI synchronizes input/output and stop. run() is invoked
// once by the worker; the core's protocol objects stay on that worker thread.
unsafe impl Send for Native {}
// SAFETY: Concurrent Rust callers can only invoke the native synchronized
// push/pop/stop entry points. Arc keeps the allocation and callback alive.
unsafe impl Sync for Native {}

impl Native {
    fn stop(&self) {
        self.stopped.store(true, Ordering::Release);
        // Wake backpressured inputs even if the native worker cannot make
        // progress. Register-before-check in push() prevents a lost wakeup.
        self.notify.notify_waiters();
        // SAFETY: Arc retains the allocation. stop is safe during/before run.
        unsafe { usque_ovpn_stop(self.pointer.as_ptr()) };
    }
    async fn push(&self, kind: u32, generation: u64, data: &[u8]) -> Result<(), Error> {
        loop {
            let notified = self.notify.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if self.stopped.load(Ordering::Acquire) {
                return Err(Error::Closed);
            }
            // SAFETY: The live native session copies this bounded slice before
            // return. The generation is checked before publishing any input.
            let result = unsafe {
                usque_ovpn_push(
                    self.pointer.as_ptr(),
                    kind,
                    generation,
                    data.as_ptr(),
                    data.len(),
                )
            };
            match result {
                1 => return Ok(()),
                0 => notified.await,
                _ => return Err(Error::Closed),
            }
        }
    }
}
impl Drop for Native {
    fn drop(&mut self) {
        // SAFETY: The worker holds its own Arc through run(), so the last Arc
        // can disappear only after run and all concurrent callers have ended.
        // The callback's Notify allocation is destroyed after this Drop body.
        unsafe { usque_ovpn_destroy(self.pointer.as_ptr()) };
    }
}

extern "C" fn wake(context: *mut c_void) {
    // SAFETY: create receives a pointer to Native's stable Notify allocation.
    // Native and its allocation outlive the worker and native destroy().
    let notify = unsafe { &*context.cast::<Notify>() };
    notify.notify_waiters();
}

/// Cloneable input handle. It cannot open a socket or change network settings.
#[derive(Clone)]
pub struct Input {
    native: Arc<Native>,
}
impl Input {
    pub async fn transport_connected(&self, generation: u64) -> Result<(), Error> {
        self.native.push(3, generation, &[]).await
    }
    pub async fn transport_failed(&self, generation: u64) -> Result<(), Error> {
        self.native.push(4, generation, &[]).await
    }
    /// Accepts one OpenVPN protocol packet, without the TCP two-byte length.
    pub async fn receive_transport(&self, generation: u64, packet: &[u8]) -> Result<(), Error> {
        if packet.is_empty() || packet.len() > MAX_PACKET {
            return Err(Error::InvalidPacket);
        }
        self.native.push(1, generation, packet).await
    }
    pub async fn send_ip(&self, generation: u64, packet: &[u8]) -> Result<(), Error> {
        validate_ip_packet(packet)?;
        self.native.push(2, generation, packet).await
    }
    pub fn stop(&self) {
        self.native.stop();
    }
}

/// One protocol worker with bounded native input and output queues. Dropping
/// the session requests stop; its native memory remains owned by the worker
/// until the core exits. shutdown() waits up to five seconds and reports a
/// timeout without claiming that a pending worker has exited.
pub struct Session {
    native: Arc<Native>,
    worker: Option<JoinHandle<Result<(), Error>>>,
}
impl Session {
    pub fn start(config: &str, remote: SocketAddr) -> Result<Self, Error> {
        Self::start_with_credentials(config, remote, "", "", "")
    }
    pub fn start_with_credentials(
        config: &str,
        remote: SocketAddr,
        username: &str,
        password: &str,
        key_password: &str,
    ) -> Result<Self, Error> {
        if config.is_empty()
            || config.len() > MAX_CONFIG
            || config.contains('\0')
            || remote.port() == 0
        {
            return Err(Error::InvalidConfig);
        }
        let notify = Box::new(Notify::new());
        let context = (&*notify as *const Notify).cast_mut().cast::<c_void>();
        let address = CString::new(remote.ip().to_string()).map_err(|_| Error::InvalidConfig)?;
        // Zeroizing byte buffers avoid leaving temporary CString copies of credentials.
        fn credential(value: &str) -> Result<zeroize::Zeroizing<Vec<u8>>, Error> {
            if value.len() > 2048 || value.contains('\0') {
                return Err(Error::InvalidConfig);
            }
            let mut bytes = zeroize::Zeroizing::new(value.as_bytes().to_vec());
            bytes.push(0);
            Ok(bytes)
        }
        let username = credential(username)?;
        let password = credential(password)?;
        let key_password = credential(key_password)?;
        // SAFETY: create copies the configuration and remote before returning.
        // The callback points at a stable allocation retained by Native.
        let pointer = unsafe {
            usque_ovpn_create(
                config.as_ptr(),
                config.len(),
                address.as_ptr(),
                remote.port(),
                username.as_ptr().cast(),
                password.as_ptr().cast(),
                key_password.as_ptr().cast(),
                wake,
                context,
            )
        };
        let pointer = NonNull::new(pointer).ok_or(Error::InvalidConfig)?;
        let native = Arc::new(Native {
            pointer,
            notify,
            stopped: AtomicBool::new(false),
        });
        let worker_native = Arc::clone(&native);
        let worker = tokio::task::spawn_blocking(move || {
            // SAFETY: Exactly this worker invokes run. Its Arc retains the
            // native session and notification context for the full call.
            let result = unsafe { usque_ovpn_run(worker_native.pointer.as_ptr()) };
            if result == 0 {
                Ok(())
            } else {
                Err(Error::Worker)
            }
        });
        Ok(Self {
            native,
            worker: Some(worker),
        })
    }
    pub fn input(&self) -> Input {
        Input {
            native: Arc::clone(&self.native),
        }
    }
    pub async fn next_event(&mut self) -> Result<Event, Error> {
        let mut data = vec![0; MAX_PACKET];
        loop {
            let notified = self.native.notify.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            let mut event = std::mem::MaybeUninit::<RawEvent>::uninit();
            // SAFETY: Output buffers have their declared capacity. pop writes
            // a complete RawEvent only when it returns 1; mutexes serialize it.
            let result = unsafe {
                usque_ovpn_pop(
                    self.native.pointer.as_ptr(),
                    event.as_mut_ptr(),
                    data.as_mut_ptr(),
                    data.len(),
                )
            };
            if result == 0 {
                notified.await;
                continue;
            }
            if result != 1 {
                return Err(Error::Closed);
            }
            // SAFETY: A successful pop initialized the full repr(C) value.
            let event = unsafe { event.assume_init() };
            let length = usize::try_from(event.length).map_err(|_| Error::InvalidPacket)?;
            if length > data.len() {
                return Err(Error::InvalidPacket);
            }
            data.truncate(length);
            return decode_event(event, data);
        }
    }
    pub async fn shutdown(&mut self) -> Result<(), Error> {
        self.native.stop();
        join_worker(&mut self.worker, SHUTDOWN_TIMEOUT).await
    }
}

async fn join_worker(
    worker: &mut Option<JoinHandle<Result<(), Error>>>,
    timeout: Duration,
) -> Result<(), Error> {
    let Some(task) = worker.as_mut() else {
        return Ok(());
    };
    // Keep ownership on timeout or cancellation. A spawn_blocking worker cannot
    // be aborted; its Arc retains native memory even if Session is then dropped.
    let result = tokio::time::timeout(timeout, task)
        .await
        .map_err(|_| Error::ShutdownTimeout)?;
    worker.take();
    result.map_err(|_| Error::Worker)?
}
impl Drop for Session {
    fn drop(&mut self) {
        self.native.stop();
    }
}

fn address_text(value: &[c_char; 48]) -> Result<String, Error> {
    let end = value
        .iter()
        .position(|byte| *byte == 0)
        .ok_or(Error::InvalidPacket)?;
    let bytes: Vec<_> = value[..end]
        .iter()
        .map(|byte| byte.to_ne_bytes()[0])
        .collect();
    String::from_utf8(bytes).map_err(|_| Error::InvalidPacket)
}
fn optional_address<T: std::str::FromStr>(value: &[c_char; 48]) -> Result<Option<T>, Error> {
    let text = address_text(value)?;
    if text.is_empty() {
        Ok(None)
    } else {
        text.parse().map(Some).map_err(|_| Error::InvalidPacket)
    }
}
fn decode_event(raw: RawEvent, data: Vec<u8>) -> Result<Event, Error> {
    let generation = raw.generation;
    Ok(match raw.kind {
        1 => Event::Dial { generation },
        2 => Event::TransportPacket {
            generation,
            packet: data.into(),
        },
        3 => {
            validate_ip_packet(&data)?;
            Event::IpPacket {
                generation,
                packet: data.into(),
            }
        }
        4 => {
            let ipv4 = optional_address(&raw.ipv4)?;
            let ipv6 = optional_address(&raw.ipv6)?;
            let mtu = u16::try_from(raw.mtu).map_err(|_| Error::InvalidPacket)?;
            if ipv4.is_none() && ipv6.is_none() || mtu < 576 {
                return Err(Error::InvalidPacket);
            }
            let dns_servers = raw
                .dns
                .iter()
                .map(optional_address)
                .collect::<Result<Vec<_>, _>>()?
                .into_iter()
                .flatten()
                .collect();
            Event::Network {
                generation,
                config: NetworkConfig {
                    ipv4,
                    ipv6,
                    mtu,
                    dns_servers,
                },
            }
        }
        5 => Event::State {
            generation,
            name: String::from_utf8(data).map_err(|_| Error::InvalidPacket)?,
            error: raw.code > 0,
            fatal: raw.code > 1,
        },
        6 => Event::Stopped,
        _ => return Err(Error::InvalidPacket),
    })
}

fn validate_ip_packet(packet: &[u8]) -> Result<(), Error> {
    let valid = match packet.first().map(|b| b >> 4) {
        Some(4) if packet.len() >= 20 => {
            let header = usize::from(packet[0] & 15) * 4;
            header >= 20
                && header <= packet.len()
                && usize::from(u16::from_be_bytes([packet[2], packet[3]])) == packet.len()
        }
        Some(6) if packet.len() >= 40 => {
            usize::from(u16::from_be_bytes([packet[4], packet[5]])) + 40 == packet.len()
        }
        _ => false,
    };
    if valid && packet.len() <= MAX_PACKET {
        Ok(())
    } else {
        Err(Error::InvalidPacket)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn shutdown_timeout_retains_worker_until_it_really_exits() {
        let (release, pending) = std::sync::mpsc::channel();
        let mut worker = Some(tokio::task::spawn_blocking(move || {
            pending.recv().map_err(|_| Error::Worker)
        }));
        assert_eq!(
            join_worker(&mut worker, Duration::ZERO).await,
            Err(Error::ShutdownTimeout)
        );
        assert!(worker.as_ref().is_some_and(|task| !task.is_finished()));
        release.send(()).unwrap();
        assert_eq!(
            join_worker(&mut worker, Duration::from_secs(3)).await,
            Ok(())
        );
        assert!(worker.is_none());
    }

    #[tokio::test]
    async fn cancelled_shutdown_keeps_the_same_worker_joinable() {
        let (release, pending) = tokio::sync::oneshot::channel::<()>();
        let mut worker = Some(tokio::spawn(async move {
            pending.await.map_err(|_| Error::Worker)
        }));
        let id = worker.as_ref().unwrap().id();
        assert!(
            tokio::time::timeout(
                Duration::ZERO,
                join_worker(&mut worker, Duration::from_secs(60))
            )
            .await
            .is_err()
        );
        assert_eq!(worker.as_ref().unwrap().id(), id);
        release.send(()).unwrap();
        assert_eq!(
            join_worker(&mut worker, Duration::from_secs(3)).await,
            Ok(())
        );
    }
    #[test]
    fn native_event_layout_matches() {
        assert_eq!(
            // SAFETY: This function returns a constant and accesses no pointers.
            unsafe { usque_ovpn_event_size() },
            std::mem::size_of::<RawEvent>()
        );
    }
    #[tokio::test]
    async fn rejects_invalid_configuration_without_starting_worker() {
        let remote = "192.0.2.1:1194".parse().unwrap();
        assert!(matches!(
            Session::start("", remote),
            Err(Error::InvalidConfig)
        ));
        assert!(matches!(
            Session::start("client\0", remote),
            Err(Error::InvalidConfig)
        ));
    }
    #[tokio::test]
    async fn unusable_tls_configuration_finishes_without_opening_a_socket() {
        // The memory peer changes OpenVPN's process-global CryptoAlgs table.
        // Even a configuration that fails before dialing must not initialize
        // another native client concurrently with that test-only setup.
        #[cfg(feature = "interop-test")]
        let _native_fixture = crate::interop_tests::SERIAL.lock().await;
        let remote = "192.0.2.1:1194".parse().unwrap();
        let mut session = Session::start(
            "client\ndev tun\nproto tcp\nremote 192.0.2.1 1194\n",
            remote,
        )
        .unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(3), async {
            loop {
                match session.next_event().await.unwrap() {
                    Event::Dial { .. } => panic!("invalid TLS setup must fail before transport"),
                    Event::Stopped => break,
                    _ => {}
                }
            }
            assert_eq!(session.shutdown().await, Err(Error::Worker));
        })
        .await
        .unwrap();
    }
    #[test]
    fn validates_ip_lengths_and_rejects_ipv6_jumbograms() {
        let mut packet = vec![0; 20];
        packet[0] = 0x45;
        packet[3] = 20;
        assert!(validate_ip_packet(&packet).is_ok());
        packet[0] = 0x4f;
        assert_eq!(validate_ip_packet(&packet), Err(Error::InvalidPacket));
        let mut packet = vec![0; 40];
        packet[0] = 0x60;
        assert!(validate_ip_packet(&packet).is_ok());
        packet.push(0);
        assert_eq!(validate_ip_packet(&packet), Err(Error::InvalidPacket));
    }
}
