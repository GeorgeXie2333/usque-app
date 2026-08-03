use std::{
    io,
    ptr::{self, NonNull},
    sync::{Arc, Mutex},
    thread::{self, JoinHandle},
};

use thiserror::Error;
use usque_platform::packet_ring::{
    PACKET_RING_LAYOUT_VERSION, PacketDirection, PacketRingError, SharedPacketRing,
};
use windows_sys::Win32::{
    Foundation::{
        CloseHandle, DUPLICATE_CLOSE_SOURCE, DUPLICATE_SAME_ACCESS, DuplicateHandle, HANDLE,
        INVALID_HANDLE_VALUE, WAIT_FAILED, WAIT_OBJECT_0,
    },
    System::{
        Memory::{
            CreateFileMappingW, FILE_MAP_ALL_ACCESS, MEMORY_MAPPED_VIEW_ADDRESS, MapViewOfFile,
            PAGE_READWRITE, UnmapViewOfFile,
        },
        Threading::{CreateEventW, GetCurrentProcess, INFINITE, SetEvent, WaitForMultipleObjects},
    },
};

use crate::{
    coordinator::PacketSessionHandles,
    windows::wintun::{WintunError, WintunSession},
};

pub struct PacketMapping {
    mapping: OwnedHandle,
    engine_to_agent_event: OwnedHandle,
    agent_to_engine_event: OwnedHandle,
    shutdown_event: OwnedHandle,
    view: MappedView,
    ring: SharedPacketRing,
    capacity: u32,
}

// SAFETY: all owned handles and the mapping view are process-scoped kernel
// objects; SharedPacketRing state is atomic and SPSC by protocol contract.
unsafe impl Send for PacketMapping {}
// SAFETY: `&PacketMapping` is safe to share: HANDLE fields are immutable after
// construction, kernel waits/signals are thread-safe, and the ring is SPSC with
// atomic indices (no thread-affine interior mutability).
unsafe impl Sync for PacketMapping {}

impl PacketMapping {
    pub fn create(
        capacity: u32,
        target_process: HANDLE,
    ) -> Result<(Arc<Self>, PacketSessionHandles), PacketSessionError> {
        if target_process.is_null() {
            return Err(PacketSessionError::InvalidTargetProcess);
        }
        let mapped_bytes = SharedPacketRing::mapped_bytes(capacity)?;
        let mapped_bytes_u64 =
            u64::try_from(mapped_bytes).map_err(|_| PacketSessionError::MappingSize)?;
        // SAFETY: INVALID_HANDLE_VALUE requests a page-file-backed unnamed
        // mapping; the size is validated and the returned handle is owned.
        let mapping = unsafe {
            CreateFileMappingW(
                INVALID_HANDLE_VALUE,
                ptr::null(),
                PAGE_READWRITE,
                (mapped_bytes_u64 >> 32) as u32,
                mapped_bytes_u64 as u32,
                ptr::null(),
            )
        };
        if mapping.is_null() {
            return Err(last_error("CreateFileMappingW"));
        }
        let mapping = OwnedHandle(mapping);
        // SAFETY: mapping is live and the requested view is within its size.
        let view = unsafe { MapViewOfFile(mapping.0, FILE_MAP_ALL_ACCESS, 0, 0, mapped_bytes) };
        let view = MappedView::new(view, mapped_bytes)?;
        // SAFETY: a Windows mapping is page aligned, uniquely initialized here,
        // writable for mapped_bytes, and retained by this object.
        let ring = unsafe { SharedPacketRing::initialize(view.pointer(), mapped_bytes, capacity) }?;
        let engine_to_agent_event = create_event(false)?;
        let agent_to_engine_event = create_event(false)?;
        let shutdown_event = create_event(true)?;

        let mapping = Arc::new(Self {
            mapping,
            engine_to_agent_event,
            agent_to_engine_event,
            shutdown_event,
            view,
            ring,
            capacity,
        });
        let handles = mapping.duplicate_into(target_process)?;
        Ok((mapping, handles))
    }

    pub fn ring(&self) -> SharedPacketRing {
        debug_assert!(self.view.length >= 64 + self.capacity as usize * 2);
        self.ring
    }

    pub fn engine_to_agent_event(&self) -> HANDLE {
        self.engine_to_agent_event.0
    }

    pub fn agent_to_engine_event(&self) -> HANDLE {
        self.agent_to_engine_event.0
    }

    pub fn shutdown_event(&self) -> HANDLE {
        self.shutdown_event.0
    }

    fn duplicate_into(
        &self,
        target_process: HANDLE,
    ) -> Result<PacketSessionHandles, PacketSessionError> {
        let mut remote = RemoteHandleGuard::new(target_process);
        let mapping_handle = remote.duplicate(self.mapping.0)?;
        let engine_to_agent_event_handle = remote.duplicate(self.engine_to_agent_event.0)?;
        let agent_to_engine_event_handle = remote.duplicate(self.agent_to_engine_event.0)?;
        let shutdown_event_handle = remote.duplicate(self.shutdown_event.0)?;
        remote.disarm();
        Ok(PacketSessionHandles {
            mapping_handle: handle_value(mapping_handle),
            engine_to_agent_event_handle: handle_value(engine_to_agent_event_handle),
            agent_to_engine_event_handle: handle_value(agent_to_engine_event_handle),
            shutdown_event_handle: handle_value(shutdown_event_handle),
            ring_capacity: self.capacity,
            layout_version: PACKET_RING_LAYOUT_VERSION,
        })
    }
}

/// Closes the exact packet-session handles previously duplicated into a live
/// Engine process. Used when setup fails after duplication but before the
/// handles can be returned to the Engine.
pub fn close_remote_packet_handles(target_process: HANDLE, handles: &PacketSessionHandles) {
    for value in [
        handles.mapping_handle,
        handles.engine_to_agent_event_handle,
        handles.agent_to_engine_event_handle,
        handles.shutdown_event_handle,
    ] {
        close_remote_handle(target_process, value as usize as HANDLE);
    }
}

impl Drop for PacketMapping {
    fn drop(&mut self) {
        // Wake both local and remote consumers before originals are closed.
        // SAFETY: event handle remains live during Drop.
        unsafe {
            SetEvent(self.shutdown_event.0);
        }
    }
}

pub struct PacketPump {
    mapping: Arc<PacketMapping>,
    thread: Option<JoinHandle<Result<(), PacketSessionError>>>,
    terminal_error: Arc<Mutex<Option<String>>>,
}

impl PacketPump {
    pub fn start(
        session: WintunSession,
        mapping: Arc<PacketMapping>,
    ) -> Result<Self, PacketSessionError> {
        let thread_mapping = Arc::clone(&mapping);
        let terminal_error = Arc::new(Mutex::new(None));
        let thread_error = Arc::clone(&terminal_error);
        let thread = thread::Builder::new()
            .name("usque-wintun-pump".to_owned())
            .spawn(move || {
                let result = run_packet_pump(session, &thread_mapping);
                if let Err(error) = &result
                    && let Ok(mut slot) = thread_error.lock()
                {
                    *slot = Some(error.to_string());
                }
                result
            })
            .map_err(PacketSessionError::ThreadSpawn)?;
        Ok(Self {
            mapping,
            thread: Some(thread),
            terminal_error,
        })
    }

    pub fn terminal_error(&self) -> Option<String> {
        self.terminal_error
            .lock()
            .ok()
            .and_then(|value| value.clone())
    }

    pub fn stop(mut self) -> Result<(), PacketSessionError> {
        self.signal_shutdown();
        self.join()
    }

    fn signal_shutdown(&self) {
        // SAFETY: mapping owns this live event handle.
        unsafe {
            SetEvent(self.mapping.shutdown_event());
        }
    }

    fn join(&mut self) -> Result<(), PacketSessionError> {
        let Some(thread) = self.thread.take() else {
            return Ok(());
        };
        thread
            .join()
            .map_err(|_| PacketSessionError::PumpPanicked)?
    }
}

impl Drop for PacketPump {
    fn drop(&mut self) {
        self.signal_shutdown();
        let _ = self.join();
    }
}

fn run_packet_pump(
    session: WintunSession,
    mapping: &PacketMapping,
) -> Result<(), PacketSessionError> {
    let handles = [
        mapping.shutdown_event(),
        mapping.engine_to_agent_event(),
        session.read_wait_event(),
    ];
    loop {
        // SAFETY: all three handles remain live for this thread and the array is
        // valid for the complete wait call.
        let wait =
            unsafe { WaitForMultipleObjects(handles.len() as u32, handles.as_ptr(), 0, INFINITE) };
        match wait {
            value if value == WAIT_OBJECT_0 => return Ok(()),
            value if value == WAIT_OBJECT_0 + 1 => {
                while let Some(packet) = mapping.ring().try_pop(PacketDirection::EngineToAgent)? {
                    match session.send(&packet) {
                        Ok(()) => {}
                        Err(error) if error.raw_os_error() == Some(111) => {
                            // Wintun transmit ring full: drop this packet. The
                            // Engine-side shared ring remains healthy.
                        }
                        Err(error) => return Err(error.into()),
                    }
                }
            }
            value if value == WAIT_OBJECT_0 + 2 => {
                while let Some(packet) = session.receive()? {
                    if mapping
                        .ring()
                        .try_push(PacketDirection::AgentToEngine, &packet)?
                    {
                        // SAFETY: mapping owns this live auto-reset event.
                        if unsafe { SetEvent(mapping.agent_to_engine_event()) } == 0 {
                            return Err(last_error("SetEvent(agent_to_engine)"));
                        }
                    }
                }
            }
            WAIT_FAILED => return Err(last_error("WaitForMultipleObjects")),
            value => return Err(PacketSessionError::UnexpectedWait(value)),
        }
    }
}

fn create_event(manual_reset: bool) -> Result<OwnedHandle, PacketSessionError> {
    // SAFETY: creates an unnamed event with default security.
    let event = unsafe { CreateEventW(ptr::null(), i32::from(manual_reset), 0, ptr::null()) };
    if event.is_null() {
        Err(last_error("CreateEventW"))
    } else {
        Ok(OwnedHandle(event))
    }
}

fn handle_value(handle: HANDLE) -> u64 {
    handle as usize as u64
}

struct RemoteHandleGuard {
    target_process: HANDLE,
    handles: Vec<HANDLE>,
    armed: bool,
}

impl RemoteHandleGuard {
    fn new(target_process: HANDLE) -> Self {
        Self {
            target_process,
            handles: Vec::with_capacity(4),
            armed: true,
        }
    }

    fn duplicate(&mut self, source: HANDLE) -> Result<HANDLE, PacketSessionError> {
        let mut duplicated: HANDLE = ptr::null_mut();
        // SAFETY: both process handles are live, source belongs to the current
        // process, and duplicated points to writable storage.
        if unsafe {
            DuplicateHandle(
                GetCurrentProcess(),
                source,
                self.target_process,
                &mut duplicated,
                0,
                0,
                DUPLICATE_SAME_ACCESS,
            )
        } == 0
        {
            return Err(last_error("DuplicateHandle"));
        }
        self.handles.push(duplicated);
        Ok(duplicated)
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for RemoteHandleGuard {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        for remote in self.handles.drain(..) {
            close_remote_handle(self.target_process, remote);
        }
    }
}

fn close_remote_handle(target_process: HANDLE, remote: HANDLE) {
    let mut temporary: HANDLE = ptr::null_mut();
    // SAFETY: duplicating with CLOSE_SOURCE closes the exact remote handle. A
    // temporary local duplicate is closed immediately if creation succeeds.
    if unsafe {
        DuplicateHandle(
            target_process,
            remote,
            GetCurrentProcess(),
            &mut temporary,
            0,
            0,
            DUPLICATE_CLOSE_SOURCE | DUPLICATE_SAME_ACCESS,
        )
    } != 0
        && !temporary.is_null()
    {
        // SAFETY: temporary is now uniquely owned in the current process.
        unsafe {
            CloseHandle(temporary);
        }
    }
}

struct OwnedHandle(HANDLE);

// SAFETY: uniquely owned Windows kernel handle; CloseHandle is thread-safe.
unsafe impl Send for OwnedHandle {}
// SAFETY: `&OwnedHandle` is safe to share: the HANDLE value is immutable after
// construction, kernel object ops are thread-safe, and Drop still closes once.
unsafe impl Sync for OwnedHandle {}

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: uniquely owned kernel handle.
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
}

struct MappedView {
    address: MEMORY_MAPPED_VIEW_ADDRESS,
    length: usize,
}

// SAFETY: mapped view is process memory with no thread-affine state; unique
// ownership unmaps exactly once on drop.
unsafe impl Send for MappedView {}
// SAFETY: `&MappedView` is safe to share: the base address is immutable after
// MapViewOfFile, and concurrent byte access is coordinated by SharedPacketRing.
unsafe impl Sync for MappedView {}

impl MappedView {
    fn new(address: MEMORY_MAPPED_VIEW_ADDRESS, length: usize) -> Result<Self, PacketSessionError> {
        if address.Value.is_null() {
            Err(last_error("MapViewOfFile"))
        } else {
            Ok(Self { address, length })
        }
    }

    fn pointer(&self) -> NonNull<u8> {
        NonNull::new(self.address.Value.cast()).expect("validated mapping")
    }
}

impl Drop for MappedView {
    fn drop(&mut self) {
        if !self.address.Value.is_null() {
            // SAFETY: this object uniquely owns the mapped view.
            unsafe {
                UnmapViewOfFile(self.address);
            }
        }
    }
}

fn last_error(operation: &'static str) -> PacketSessionError {
    PacketSessionError::Windows(operation, io::Error::last_os_error())
}

#[derive(Debug, Error)]
pub enum PacketSessionError {
    #[error("packet-ring layout failed: {0}")]
    Ring(#[from] PacketRingError),
    #[error("Wintun packet session failed: {0}")]
    Wintun(#[from] WintunError),
    #[error("target Engine process handle is invalid")]
    InvalidTargetProcess,
    #[error("packet mapping size is unsupported")]
    MappingSize,
    #[error("Windows {0} failed: {1}")]
    Windows(&'static str, io::Error),
    #[error("packet pump wait returned unexpected status {0}")]
    UnexpectedWait(u32),
    #[error("packet pump thread panicked")]
    PumpPanicked,
    #[error("packet pump thread could not be created: {0}")]
    ThreadSpawn(io::Error),
}

#[cfg(test)]
mod tests {
    use super::*;
    use usque_platform::packet_ring::MIN_RING_CAPACITY;

    #[test]
    fn mapping_handles_are_duplicated_and_share_packet_bytes() {
        // SAFETY: returns the current-process pseudo-handle.
        let process = unsafe { GetCurrentProcess() };
        let (mapping, handles) =
            PacketMapping::create(MIN_RING_CAPACITY, process).expect("mapping");
        assert_eq!(handles.layout_version, PACKET_RING_LAYOUT_VERSION);
        let duplicate_mapping = OwnedHandle(handles.mapping_handle as usize as HANDLE);
        let duplicate_engine_event =
            OwnedHandle(handles.engine_to_agent_event_handle as usize as HANDLE);
        let duplicate_agent_event =
            OwnedHandle(handles.agent_to_engine_event_handle as usize as HANDLE);
        let duplicate_shutdown = OwnedHandle(handles.shutdown_event_handle as usize as HANDLE);
        let bytes = SharedPacketRing::mapped_bytes(handles.ring_capacity).expect("bytes");
        // SAFETY: duplicated mapping handle is valid in this test process.
        let view = unsafe { MapViewOfFile(duplicate_mapping.0, FILE_MAP_ALL_ACCESS, 0, 0, bytes) };
        let duplicate_view = MappedView::new(view, bytes).expect("view");
        // SAFETY: the duplicated view points to the same initialized mapping
        // and all views remain live for this test.
        let engine_ring =
            unsafe { SharedPacketRing::attach(duplicate_view.pointer(), bytes) }.expect("attach");

        assert!(
            engine_ring
                .try_push(PacketDirection::EngineToAgent, b"outbound")
                .expect("push")
        );
        assert_eq!(
            mapping
                .ring()
                .try_pop(PacketDirection::EngineToAgent)
                .expect("pop"),
            Some(b"outbound".to_vec())
        );
        assert!(
            mapping
                .ring()
                .try_push(PacketDirection::AgentToEngine, b"inbound")
                .expect("push")
        );
        assert_eq!(
            engine_ring
                .try_pop(PacketDirection::AgentToEngine)
                .expect("pop"),
            Some(b"inbound".to_vec())
        );

        drop(duplicate_engine_event);
        drop(duplicate_agent_event);
        drop(duplicate_shutdown);
        assert_eq!(duplicate_view.length, bytes);
    }
}
