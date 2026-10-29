//! Narrow C boundary used by native providers. Native callers authenticate XPC
//! peers before submitting secrets. All native memory is copied immediately.
use crate::{
    Controller,
    backend::{
        DataPlaneBackend, MAX_PACKET_BYTES, PacketEndpoint, PlatformAdapter, PlatformRequest,
        SessionPacket,
    },
    credential_rpc::{
        CredentialRequest, CredentialResponse, CredentialTransport, RpcCredentialBroker,
    },
    credentials::{CredentialBroker, UnavailableCredentialBroker},
    policy::PolicyEvaluator,
    protocol::*,
};
use bytes::Bytes;
use serde::Serialize;
use std::{
    ffi::c_void,
    ptr,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};
use usque_transport::GeoRoute;
use zeroize::Zeroize;

#[repr(C)]
pub struct Buffer {
    pub data: *mut u8,
    pub len: usize,
}
impl Default for Buffer {
    fn default() -> Self {
        Self {
            data: ptr::null_mut(),
            len: 0,
        }
    }
}
pub type PlatformCallback = unsafe extern "C" fn(*mut c_void, *const u8, usize) -> i32;
pub type CredentialCallback =
    unsafe extern "C" fn(*mut c_void, *const u8, usize, *mut u8, usize, *mut usize) -> i32;
struct CallbackLifetime {
    active: AtomicBool,
    guard: Mutex<()>,
}
struct CallbackCredentialTransport {
    callback: CredentialCallback,
    context: usize,
    lifetime: Arc<CallbackLifetime>,
}
impl CredentialTransport for CallbackCredentialTransport {
    fn exchange(&self, request: &CredentialRequest) -> Result<CredentialResponse, String> {
        let _guard = self
            .lifetime
            .guard
            .lock()
            .map_err(|_| "credential_callback_unavailable")?;
        if !self.lifetime.active.load(Ordering::Acquire) {
            return Err("credential_callback_closed".into());
        }
        let encoded = zeroize::Zeroizing::new(
            serde_json::to_vec(request).map_err(|_| "invalid_credential_request")?,
        );
        let mut response = zeroize::Zeroizing::new(vec![0u8; MAX_CONTROL_BYTES]);
        let mut response_len = 0usize;
        // SAFETY: Context/callback remain valid until destroy, which serializes
        // with this guard to prevent a late detached lease-release callback.
        let status = unsafe {
            (self.callback)(
                self.context as *mut c_void,
                encoded.as_ptr(),
                encoded.len(),
                response.as_mut_ptr(),
                response.len(),
                &mut response_len,
            )
        };
        if status != 0 || response_len == 0 || response_len > response.len() {
            return Err("credential_callback_failed".into());
        }
        serde_json::from_slice(&response[..response_len])
            .map_err(|_| "invalid_credential_response".into())
    }
}
struct CallbackAdapter {
    callback: PlatformCallback,
    context: usize,
}
// SAFETY: The C ABI requires a context whose lifetime spans the handle and a
// callback that is safe on concurrent provider workers. The integer is never
// dereferenced by Rust; the caller retains ownership of the opaque context.
unsafe impl Send for CallbackAdapter {}
// SAFETY: Same callback contract as Send. Rust only passes the opaque context.
unsafe impl Sync for CallbackAdapter {}
impl PlatformAdapter for CallbackAdapter {
    fn request(&self, request: &PlatformRequest) -> Result<(), String> {
        let bytes = serde_json::to_vec(request).map_err(|_| "platform_request_invalid")?;
        // SAFETY: Callback and context are valid per runtime_create contract;
        // the borrowed request allocation outlives the synchronous invocation.
        let result =
            unsafe { (self.callback)(self.context as *mut c_void, bytes.as_ptr(), bytes.len()) };
        if result == 0 {
            Ok(())
        } else {
            Err("native_platform_action_rejected".into())
        }
    }
}
pub struct RuntimeHandle {
    controller: Controller,
    packets: Mutex<PacketEndpoint>,
    _runtime: tokio::runtime::Runtime,
    callback_lifetime: Arc<CallbackLifetime>,
}
impl Drop for RuntimeHandle {
    fn drop(&mut self) {
        let _guard = self
            .callback_lifetime
            .guard
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        self.callback_lifetime
            .active
            .store(false, Ordering::Release);
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn usque_macos_protocol_version() -> u32 {
    PROTOCOL_VERSION
}

/// Create a provider-owned runtime. A missing callback returns null.
///
/// # Safety
/// `context` must remain valid until destroy returns. Callback must accept calls
/// on Rust worker threads, complete OS operations before returning, and return
/// nonzero for unavailable/unverified protection. It must not call destroy or
/// await another operation on this same handle from inside a callback.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn usque_macos_runtime_create(
    callback: Option<PlatformCallback>,
    context: *mut c_void,
) -> *mut RuntimeHandle {
    create_runtime(callback, None, context)
}
/// Create a runtime with an authenticated, bounded credential RPC callback.
///
/// # Safety
/// Same context lifetime contract as runtime_create. Credential callback writes
/// at most response_capacity bytes and sets response_len, enforces a deadline,
/// and never retains input or output pointers after returning.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn usque_macos_runtime_create_with_broker(
    callback: Option<PlatformCallback>,
    credential: Option<CredentialCallback>,
    context: *mut c_void,
) -> *mut RuntimeHandle {
    if credential.is_none() {
        return ptr::null_mut();
    }
    create_runtime(callback, credential, context)
}
fn create_runtime(
    callback: Option<PlatformCallback>,
    credential: Option<CredentialCallback>,
    context: *mut c_void,
) -> *mut RuntimeHandle {
    let Some(callback) = callback else {
        return ptr::null_mut();
    };
    let Ok(runtime) = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
    else {
        return ptr::null_mut();
    };
    let adapter: Arc<dyn PlatformAdapter> = Arc::new(CallbackAdapter {
        callback,
        context: context as usize,
    });
    let callback_lifetime = Arc::new(CallbackLifetime {
        active: AtomicBool::new(true),
        guard: Mutex::new(()),
    });
    let broker: Arc<dyn CredentialBroker> = match credential {
        Some(callback) => Arc::new(RpcCredentialBroker::new(Arc::new(
            CallbackCredentialTransport {
                callback,
                context: context as usize,
                lifetime: callback_lifetime.clone(),
            },
        ))),
        None => Arc::new(UnavailableCredentialBroker),
    };
    let (backend, packets) = DataPlaneBackend::new(adapter, broker);
    let controller = {
        let _guard = runtime.enter();
        Controller::spawn(Box::new(backend), 8)
    };
    Box::into_raw(Box::new(RuntimeHandle {
        controller,
        packets: Mutex::new(packets),
        _runtime: runtime,
        callback_lifetime,
    }))
}

unsafe fn bytes<'a>(data: *const u8, len: usize, maximum: usize) -> Option<&'a [u8]> {
    if data.is_null() || len == 0 || len > maximum {
        return None;
    }
    // SAFETY: Caller guarantees one readable allocation of at least len bytes.
    Some(unsafe { std::slice::from_raw_parts(data, len) })
}
unsafe fn output(out: *mut Buffer, bytes: Vec<u8>) -> i32 {
    if out.is_null() {
        return -1;
    }
    let mut allocation = bytes.into_boxed_slice();
    let value = Buffer {
        data: allocation.as_mut_ptr(),
        len: allocation.len(),
    };
    std::mem::forget(allocation);
    // SAFETY: Caller provides a valid writable Buffer and releases prior data.
    unsafe { ptr::write(out, value) };
    0
}
unsafe fn json_output(out: *mut Buffer, value: &impl Serialize) -> i32 {
    match serde_json::to_vec(value) {
        // SAFETY: Forward the caller's writable output contract.
        Ok(bytes) => unsafe { output(out, bytes) },
        Err(_) => -1,
    }
}

/// Copy and admit one JSON request; return immediately with acceptance/status.
///
/// # Safety
/// `handle` is a live pointer from create. `data` spans len readable bytes;
/// `out` is writable and does not alias the input or handle. No concurrent
/// destroy is permitted. Release every returned Buffer with buffer_free.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn usque_macos_runtime_command(
    handle: *mut RuntimeHandle,
    data: *const u8,
    len: usize,
    out: *mut Buffer,
) -> i32 {
    // SAFETY: Pointer validity is the caller's ABI contract; null is rejected.
    let Some(handle) = (unsafe { handle.as_ref() }) else {
        return -1;
    };
    // SAFETY: Borrow the caller's input only for this synchronous invocation.
    let Some(input) = (unsafe { bytes(data, len, MAX_CONTROL_BYTES) }) else {
        return -1;
    };
    let result = handle.controller.submit_json(input);
    match result {
        // SAFETY: Forward the writable output contract.
        Ok(reply) => unsafe { json_output(out, &reply) },
        Err(error) => {
            // SAFETY: Forward the writable output contract; never echo secrets.
            let status =
                unsafe { json_output(out, &serde_json::json!({"error_code":error.to_string()})) };
            if status == 0 { -2 } else { status }
        }
    }
}
/// Read the current state without creating a replay-journal entry.
///
/// # Safety
/// Same live handle and writable Buffer contract as runtime_command.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn usque_macos_runtime_snapshot(
    handle: *mut RuntimeHandle,
    out: *mut Buffer,
) -> i32 {
    // SAFETY: Validate null; caller guarantees pointer lifetime.
    let Some(handle) = (unsafe { handle.as_ref() }) else {
        return -1;
    };
    // SAFETY: Forward the writable output contract.
    unsafe { json_output(out, &handle.controller.snapshot()) }
}
/// Queue one complete IPv4/IPv6 packet. -2 means bounded backpressure;
/// -3 means no admitted healthy session. Never retry a packet across sessions.
///
/// # Safety
/// Handle/input lifetime rules match runtime_command.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn usque_macos_runtime_send_packet(
    handle: *mut RuntimeHandle,
    data: *const u8,
    len: usize,
) -> i32 {
    // SAFETY: Caller guarantees live allocation; null rejected.
    let Some(handle) = (unsafe { handle.as_ref() }) else {
        return -1;
    };
    // SAFETY: Caller guarantees len readable bytes; max enforces bounded memory.
    let Some(packet) = (unsafe { bytes(data, len, MAX_PACKET_BYTES) }) else {
        return -1;
    };
    if !matches!(packet[0] >> 4, 4 | 6) {
        return -1;
    }
    queue_packet(handle, packet, None)
}
/// Queue a packet captured under an exact session/revision/network generation.
/// Native backpressure must never relabel an old packet as a new session.
///
/// # Safety
/// Handle/input lifetime rules match runtime_command. Session is bounded JSON.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn usque_macos_runtime_send_session_packet(
    handle: *mut RuntimeHandle,
    data: *const u8,
    len: usize,
    session: *const u8,
    session_len: usize,
) -> i32 {
    // SAFETY: Caller guarantees live allocations; null/lengths are checked.
    let Some(handle) = (unsafe { handle.as_ref() }) else {
        return -1;
    };
    // SAFETY: Read only the bounded input allocations guaranteed by the caller.
    let Some(packet) = (unsafe { bytes(data, len, MAX_PACKET_BYTES) }) else {
        return -1;
    };
    // SAFETY: Same bounded allocation contract for the session JSON.
    let Some(session) = (unsafe { bytes(session, session_len, 512) }) else {
        return -1;
    };
    let Ok(expected) = serde_json::from_slice::<SessionKey>(session) else {
        return -1;
    };
    if !matches!(packet[0] >> 4, 4 | 6) {
        return -1;
    }
    queue_packet(handle, packet, Some(&expected))
}
fn queue_packet(handle: &RuntimeHandle, packet: &[u8], expected: Option<&SessionKey>) -> i32 {
    enqueue_packet(
        handle.controller.snapshot(),
        &handle.packets,
        packet,
        expected,
    )
}
fn enqueue_packet(
    snapshot: Snapshot,
    endpoint: &Mutex<PacketEndpoint>,
    packet: &[u8],
    expected: Option<&SessionKey>,
) -> i32 {
    if snapshot.phase != Phase::Connected {
        return -3;
    }
    let Some(session) = snapshot.session else {
        return -3;
    };
    if expected.is_some_and(|expected| expected != &session) {
        return -3;
    }
    let Ok(packets) = endpoint.lock() else {
        return -1;
    };
    match packets.outgoing.try_send(SessionPacket {
        session,
        bytes: Bytes::copy_from_slice(packet),
    }) {
        Ok(()) => 0,
        Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => -2,
        Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => -3,
    }
}
/// Receive a packet: 1=packet, 0=empty/not connected, negative=error.
///
/// # Safety
/// Same live handle and writable Buffer contract as runtime_command.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn usque_macos_runtime_receive_packet(
    handle: *mut RuntimeHandle,
    out: *mut Buffer,
) -> i32 {
    // SAFETY: Caller guarantees live handle; null rejected.
    let Some(handle) = (unsafe { handle.as_ref() }) else {
        return -1;
    };
    let snapshot = handle.controller.snapshot();
    if snapshot.phase != Phase::Connected {
        return 0;
    }
    let Ok(mut packets) = handle.packets.lock() else {
        return -1;
    };
    while let Ok(packet) = packets.incoming.try_recv() {
        if Some(&packet.session) != snapshot.session.as_ref() {
            continue;
        }
        // SAFETY: Forward writable output contract.
        return if unsafe { output(out, packet.bytes.to_vec()) } == 0 {
            1
        } else {
            -1
        };
    }
    0
}
/// Release exactly one returned Buffer, clearing its contents first.
///
/// # Safety
/// Pass an unmodified Buffer from this library exactly once. Empty/null is OK.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn usque_macos_buffer_free(buffer: Buffer) {
    if buffer.data.is_null() {
        return;
    }
    // SAFETY: Buffer points to the Box<[u8]> allocation returned by output.
    let mut bytes =
        unsafe { Box::from_raw(ptr::slice_from_raw_parts_mut(buffer.data, buffer.len)) };
    bytes.zeroize();
}
/// Destroy the provider runtime. This is provider teardown, not XPC disconnect.
/// Explicit Stop must finish before intentional teardown to restore networking;
/// otherwise independent native filtering must remain active.
///
/// # Safety
/// Live handle from create, passed once; no concurrent operations/callbacks.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn usque_macos_runtime_destroy(handle: *mut RuntimeHandle) {
    if !handle.is_null() {
        // SAFETY: Ownership of create's Box is returned exactly once.
        drop(unsafe { Box::from_raw(handle) });
    }
}
/// Build the shared policy from SessionConfiguration JSON. Null means fail shut.
///
/// # Safety
/// Input spans len readable bytes and remains valid through this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn usque_macos_policy_new_json(
    data: *const u8,
    len: usize,
) -> *mut PolicyEvaluator {
    // SAFETY: Borrow caller's bounded input only for this invocation.
    let Some(input) = (unsafe { bytes(data, len, MAX_CONTROL_BYTES) }) else {
        return ptr::null_mut();
    };
    let Ok(configuration) = serde_json::from_slice(input) else {
        return ptr::null_mut();
    };
    match PolicyEvaluator::load(&configuration) {
        Ok(policy) => Box::into_raw(Box::new(policy)),
        Err(_) => ptr::null_mut(),
    }
}
fn route_code(route: GeoRoute) -> i32 {
    match route {
        GeoRoute::Tunnel => 0,
        GeoRoute::Direct => 1,
        GeoRoute::Reject => 2,
    }
}
/// Evaluate a domain with the shared rule precedence. -1 must be rejected.
///
/// # Safety
/// Live policy pointer; host spans len readable UTF-8 bytes. No concurrent free.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn usque_macos_policy_route_host(
    policy: *const PolicyEvaluator,
    host: *const u8,
    len: usize,
) -> i32 {
    // SAFETY: Caller guarantees a live policy; null rejected.
    let Some(policy) = (unsafe { policy.as_ref() }) else {
        return -1;
    };
    // SAFETY: Caller guarantees readable bounded host bytes.
    let Some(input) = (unsafe { bytes(host, len, 253) }) else {
        return -1;
    };
    let Ok(host) = std::str::from_utf8(input) else {
        return -1;
    };
    if host.chars().any(char::is_control) {
        return -1;
    }
    route_code(policy.route_host(host))
}
/// Evaluate an IPv4/IPv6 textual address. -1 must be rejected.
///
/// # Safety
/// Same pointer/input lifetime rules as policy_route_host.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn usque_macos_policy_route_ip(
    policy: *const PolicyEvaluator,
    address: *const u8,
    len: usize,
) -> i32 {
    // SAFETY: Caller guarantees live policy; null rejected.
    let Some(policy) = (unsafe { policy.as_ref() }) else {
        return -1;
    };
    // SAFETY: Caller guarantees readable address bytes.
    let Some(input) = (unsafe { bytes(address, len, 45) }) else {
        return -1;
    };
    let Ok(address) = std::str::from_utf8(input) else {
        return -1;
    };
    let Ok(address) = address.parse() else {
        return -1;
    };
    route_code(policy.route_ip(address))
}
/// Evaluate one hostname/IP pair with the existing shared rule precedence.
///
/// # Safety
/// Live policy, nullable host/IP with zero length or bounded readable UTF-8.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn usque_macos_policy_route_flow(
    policy: *const PolicyEvaluator,
    host: *const u8,
    host_len: usize,
    ip: *const u8,
    ip_len: usize,
) -> i32 {
    // SAFETY: The caller guarantees the policy's lifetime; null is rejected.
    let Some(policy) = (unsafe { policy.as_ref() }) else {
        return -1;
    };
    let host = if host_len == 0 {
        None
    } else {
        // SAFETY: Read only the bounded allocation guaranteed by the caller.
        let Some(data) = (unsafe { bytes(host, host_len, 253) }) else {
            return -1;
        };
        let Ok(value) = std::str::from_utf8(data) else {
            return -1;
        };
        if value.chars().any(char::is_control) {
            return -1;
        }
        Some(value)
    };
    let ip = if ip_len == 0 {
        None
    } else {
        // SAFETY: Read only the bounded allocation guaranteed by the caller.
        let Some(data) = (unsafe { bytes(ip, ip_len, 45) }) else {
            return -1;
        };
        let Ok(text) = std::str::from_utf8(data) else {
            return -1;
        };
        let Ok(value) = text.parse() else {
            return -1;
        };
        Some(value)
    };
    route_code(policy.route_flow(host, ip))
}
/// Release one policy handle after all concurrent readers finish.
///
/// # Safety
/// Pointer was returned by policy_new_json and is passed at most once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn usque_macos_policy_free(policy: *mut PolicyEvaluator) {
    if !policy.is_null() {
        // SAFETY: Return ownership of policy_new_json's Box exactly once.
        drop(unsafe { Box::from_raw(policy) });
    }
}

#[cfg(test)]
mod packet_tests {
    use super::*;
    use tokio::sync::mpsc;
    use uuid::Uuid;

    #[test]
    fn delayed_native_packets_cannot_enter_a_replacement_session() {
        let old = SessionKey {
            session_id: Uuid::new_v4(),
            config_revision: 1,
            network_generation: 7,
        };
        let current = SessionKey {
            config_revision: 2,
            ..old.clone()
        };
        let (outgoing, mut receiver) = mpsc::channel(1);
        let (_, incoming) = mpsc::channel(1);
        let endpoint = Mutex::new(PacketEndpoint { outgoing, incoming });
        let connected = Snapshot {
            phase: Phase::Connected,
            session: Some(current.clone()),
            ..Snapshot::default()
        };
        let packet = [0x45, 0, 0, 20];
        for stale in [
            old,
            SessionKey {
                network_generation: 8,
                ..current.clone()
            },
            SessionKey {
                session_id: Uuid::new_v4(),
                ..current.clone()
            },
        ] {
            assert_eq!(
                enqueue_packet(connected.clone(), &endpoint, &packet, Some(&stale)),
                -3
            );
            assert!(receiver.try_recv().is_err());
        }
        assert_eq!(
            enqueue_packet(connected.clone(), &endpoint, &packet, Some(&current)),
            0
        );
        assert_eq!(
            enqueue_packet(connected.clone(), &endpoint, &packet, Some(&current)),
            -2
        );
        let queued = receiver.try_recv().unwrap();
        assert_eq!(queued.session, current);
        assert_eq!(queued.bytes.as_ref(), packet);
        let protected = Snapshot {
            phase: Phase::Protected,
            ..connected
        };
        assert_eq!(
            enqueue_packet(protected, &endpoint, &packet, Some(&current)),
            -3
        );
        assert!(receiver.try_recv().is_err());
    }
}
