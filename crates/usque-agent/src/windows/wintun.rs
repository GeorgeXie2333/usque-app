use std::{
    ffi::c_void,
    fs::File,
    io::{self, Read},
    mem,
    os::windows::ffi::OsStrExt,
    path::{Path, PathBuf},
    ptr,
    sync::{Arc, OnceLock},
    thread,
    time::{Duration, Instant},
};

use sha2::{Digest, Sha256};
use thiserror::Error;
use usque_ipc::agent_v1::RecoveryTraceStage as TraceStage;
use uuid::Uuid;
use windows_sys::{
    Win32::{
        Devices::DeviceAndDriverInstallation::{
            CM_Get_DevNode_Status, DI_REMOVEDEVICE_GLOBAL, DIF_REMOVE, GUID_DEVCLASS_NET, HDEVINFO,
            SP_CLASSINSTALL_HEADER, SP_DEVINFO_DATA, SP_REMOVEDEVICE_PARAMS,
            SetupDiCallClassInstaller, SetupDiDestroyDeviceInfoList, SetupDiEnumDeviceInfo,
            SetupDiGetClassDevsW, SetupDiGetDeviceInstanceIdW, SetupDiSetClassInstallParamsW,
        },
        Foundation::{ERROR_NO_MORE_ITEMS, FreeLibrary, HANDLE, HMODULE, INVALID_HANDLE_VALUE},
        NetworkManagement::Ndis::NET_LUID_LH,
        System::LibraryLoader::{
            GetProcAddress, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR, LOAD_LIBRARY_SEARCH_SYSTEM32,
            LoadLibraryExW,
        },
    },
    core::GUID,
};

use super::network;
use crate::journal::MutationReceipt;
use crate::recovery_diagnostics::{
    AdapterRemovalDiagnostic, RecoveryApi, RemovalFailure, RemovalStage,
};
use crate::recovery_trace::{TraceResource, TraceSink};

const WINTUN_DLL_NAME: &str = "wintun.dll";
const WINTUN_MIN_RING_CAPACITY: u32 = 0x20_000;
const WINTUN_MAX_RING_CAPACITY: u32 = 0x400_0000;
const WINTUN_MAX_IP_PACKET_SIZE: usize = 0xffff;
const ADAPTER_REMOVAL_CONFIRM_TIMEOUT: Duration = Duration::from_secs(10);
const ADAPTER_REMOVAL_CLOSE_GRACE: Duration = Duration::from_secs(2);
const ADAPTER_REMOVAL_CONFIRM_INTERVAL: Duration = Duration::from_millis(100);

#[cfg(target_arch = "x86_64")]
const EXPECTED_DLL_SHA256: [u8; 32] = [
    0xe5, 0xda, 0x84, 0x47, 0xdc, 0x2c, 0x32, 0x0e, 0xdc, 0x0f, 0xc5, 0x2f, 0xa0, 0x18, 0x85, 0xc1,
    0x03, 0xde, 0x8c, 0x11, 0x84, 0x81, 0xf6, 0x83, 0x64, 0x3c, 0xac, 0xc3, 0x22, 0x0d, 0xaf, 0xce,
];

#[cfg(target_arch = "aarch64")]
const EXPECTED_DLL_SHA256: [u8; 32] = [
    0xf7, 0xba, 0x89, 0x00, 0x55, 0x44, 0xbe, 0x9d, 0x85, 0x23, 0x1a, 0x9e, 0x0d, 0x5f, 0x23, 0xb2,
    0xd1, 0x5b, 0x33, 0x11, 0x66, 0x7e, 0x2d, 0xad, 0x0d, 0xeb, 0xd3, 0x44, 0x91, 0x8a, 0x3f, 0x80,
];

type AdapterHandle = *mut c_void;
type SessionHandle = *mut c_void;
type CreateAdapter =
    unsafe extern "system" fn(*const u16, *const u16, *const GUID) -> AdapterHandle;
type OpenAdapter = unsafe extern "system" fn(*const u16) -> AdapterHandle;
type CloseAdapter = unsafe extern "system" fn(AdapterHandle);
type GetAdapterLuid = unsafe extern "system" fn(AdapterHandle, *mut NET_LUID_LH);
type GetRunningDriverVersion = unsafe extern "system" fn() -> u32;
type StartSession = unsafe extern "system" fn(AdapterHandle, u32) -> SessionHandle;
type EndSession = unsafe extern "system" fn(SessionHandle);
type GetReadWaitEvent = unsafe extern "system" fn(SessionHandle) -> HANDLE;
type ReceivePacket = unsafe extern "system" fn(SessionHandle, *mut u32) -> *mut u8;
type ReleaseReceivePacket = unsafe extern "system" fn(SessionHandle, *const u8);
type AllocateSendPacket = unsafe extern "system" fn(SessionHandle, u32) -> *mut u8;
type SendPacket = unsafe extern "system" fn(SessionHandle, *const u8);
type LoggerCallback = unsafe extern "system" fn(i32, u64, *const u16);
type SetLogger = unsafe extern "system" fn(Option<LoggerCallback>);

// Wintun's callback is global and may outlive the foreign call that queued it.
// Keep only the evidence producer alive; never associate it with the last VPN.
static NATIVE_TRACE: OnceLock<TraceSink> = OnceLock::new();

unsafe extern "system" fn native_logger(level: i32, timestamp: u64, message: *const u16) {
    // No panic may cross the FFI boundary. The callback performs bounded work,
    // uses try_send, and never obtains a journal/network lock or writes a file.
    let _ = std::panic::catch_unwind(|| {
        let Some(sink) = NATIVE_TRACE.get() else {
            return;
        };
        if message.is_null() {
            return;
        }
        let mut units = [0u16; 1024];
        let mut length = 0;
        while length < units.len() {
            // SAFETY: the Wintun callback contract supplies a live, terminated
            // UTF-16 string. Read sequentially and stop at the first terminator.
            let unit = unsafe { *message.add(length) };
            if unit == 0 {
                break;
            }
            units[length] = unit;
            length += 1;
        }
        let text = String::from_utf16_lossy(&units[..length]);
        let unix_ms = timestamp.saturating_sub(116_444_736_000_000_000) / 10_000;
        sink.native_message(
            u32::try_from(level.saturating_add(1)).unwrap_or(0),
            unix_ms,
            &text,
        );
    });
}

pub struct WintunLibrary {
    module: HMODULE,
    create_adapter: CreateAdapter,
    open_adapter: OpenAdapter,
    close_adapter: CloseAdapter,
    get_adapter_luid: GetAdapterLuid,
    get_running_driver_version: GetRunningDriverVersion,
    start_session: StartSession,
    end_session: EndSession,
    get_read_wait_event: GetReadWaitEvent,
    receive_packet: ReceivePacket,
    release_receive_packet: ReleaseReceivePacket,
    allocate_send_packet: AllocateSendPacket,
    send_packet: SendPacket,
    set_logger: SetLogger,
}

// SAFETY: a loaded module and immutable function table may be called
// concurrently; FreeLibrary runs only on unique drop.
unsafe impl Send for WintunLibrary {}
// SAFETY: `&WintunLibrary` is safe to share: function pointers and the module
// handle are immutable after load, and FreeLibrary runs only on exclusive Drop.
unsafe impl Sync for WintunLibrary {}

impl WintunLibrary {
    pub fn load(path: &Path) -> Result<Arc<Self>, WintunError> {
        if !path.is_absolute()
            || !path
                .file_name()
                .is_some_and(|name| name.eq_ignore_ascii_case(WINTUN_DLL_NAME))
        {
            return Err(WintunError::InvalidPath(path.to_path_buf()));
        }
        verify_hash(path)?;
        let path_wide = wide(path.as_os_str());
        // SAFETY: the path is absolute and null-terminated. Search is limited
        // to the DLL directory and System32, preventing current-directory DLL
        // preloading.
        let module = unsafe {
            LoadLibraryExW(
                path_wide.as_ptr(),
                ptr::null_mut(),
                LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32,
            )
        };
        if module.is_null() {
            return Err(WintunError::Windows(
                "LoadLibraryExW",
                io::Error::last_os_error(),
            ));
        }

        let library = (|| {
            // SAFETY: every symbol name and function signature is copied
            // verbatim from the pinned 0.14.1 wintun.h; module remains loaded
            // for the entire resolve sequence.
            unsafe {
                Ok(Self {
                    module,
                    create_adapter: resolve(module, b"WintunCreateAdapter\0")?,
                    open_adapter: resolve(module, b"WintunOpenAdapter\0")?,
                    close_adapter: resolve(module, b"WintunCloseAdapter\0")?,
                    get_adapter_luid: resolve(module, b"WintunGetAdapterLUID\0")?,
                    get_running_driver_version: resolve(
                        module,
                        b"WintunGetRunningDriverVersion\0",
                    )?,
                    start_session: resolve(module, b"WintunStartSession\0")?,
                    end_session: resolve(module, b"WintunEndSession\0")?,
                    get_read_wait_event: resolve(module, b"WintunGetReadWaitEvent\0")?,
                    receive_packet: resolve(module, b"WintunReceivePacket\0")?,
                    release_receive_packet: resolve(module, b"WintunReleaseReceivePacket\0")?,
                    allocate_send_packet: resolve(module, b"WintunAllocateSendPacket\0")?,
                    send_packet: resolve(module, b"WintunSendPacket\0")?,
                    set_logger: resolve(module, b"WintunSetLogger\0")?,
                })
            }
        })();
        match library {
            Ok(library) => Ok(Arc::new(library)),
            Err(error) => {
                // SAFETY: module was loaded successfully and ownership was not
                // transferred into WintunLibrary.
                unsafe {
                    FreeLibrary(module);
                }
                Err(error)
            }
        }
    }

    pub fn create_adapter(
        self: &Arc<Self>,
        name: &str,
        requested_guid: Uuid,
        trace: TraceResource,
    ) -> Result<WintunAdapter, WintunError> {
        let name = wide_name(name)?;
        let tunnel_type = wide_name("Usque")?;
        let guid = GUID::from_u128(requested_guid.as_u128());
        // SAFETY: names and GUID are valid for the complete call; the returned
        // handle is uniquely owned by AdapterInner.
        let handle = unsafe { (self.create_adapter)(name.as_ptr(), tunnel_type.as_ptr(), &guid) };
        if handle.is_null() {
            return Err(WintunError::Windows(
                "WintunCreateAdapter",
                io::Error::last_os_error(),
            ));
        }
        Ok(WintunAdapter(Arc::new(AdapterInner {
            library: Arc::clone(self),
            handle,
            name: name_to_string(&name),
            trace,
        })))
    }

    pub fn open_adapter(
        self: &Arc<Self>,
        name: &str,
        trace: TraceResource,
    ) -> Result<WintunAdapter, WintunError> {
        let name = wide_name(name)?;
        // SAFETY: name is valid and null-terminated; returned handle ownership
        // is transferred into AdapterInner.
        let handle = unsafe { (self.open_adapter)(name.as_ptr()) };
        if handle.is_null() {
            return Err(WintunError::Windows(
                "WintunOpenAdapter",
                io::Error::last_os_error(),
            ));
        }
        Ok(WintunAdapter(Arc::new(AdapterInner {
            library: Arc::clone(self),
            handle,
            name: name_to_string(&name),
            trace,
        })))
    }

    pub fn running_driver_version(&self) -> Result<u32, WintunError> {
        // SAFETY: function pointer belongs to the live module.
        let version = unsafe { (self.get_running_driver_version)() };
        if version == 0 {
            Err(WintunError::Windows(
                "WintunGetRunningDriverVersion",
                io::Error::last_os_error(),
            ))
        } else {
            Ok(version)
        }
    }

    pub fn enable_recovery_trace(&self, sink: TraceSink) {
        if sink.enabled() && NATIVE_TRACE.set(sink).is_ok() {
            // SAFETY: the pinned function signature matches wintun.h. The
            // callback and its producer live for the rest of this process.
            unsafe {
                (self.set_logger)(Some(native_logger));
            }
        }
    }
}

impl Drop for WintunLibrary {
    fn drop(&mut self) {
        if !self.module.is_null() {
            // SAFETY: this object uniquely owns the module and all adapters and
            // sessions retain an Arc, so no function pointer remains in use.
            unsafe {
                FreeLibrary(self.module);
            }
        }
    }
}

#[derive(Clone)]
pub struct WintunAdapter(Arc<AdapterInner>);

impl WintunAdapter {
    pub fn name(&self) -> &str {
        &self.0.name
    }

    pub fn luid(&self) -> u64 {
        let mut luid = NET_LUID_LH::default();
        // SAFETY: adapter handle and output pointer are valid.
        unsafe {
            (self.0.library.get_adapter_luid)(self.0.handle, &mut luid);
            luid.Value
        }
    }

    pub fn start_session(
        &self,
        capacity: u32,
        trace: TraceResource,
    ) -> Result<WintunSession, WintunError> {
        if !(WINTUN_MIN_RING_CAPACITY..=WINTUN_MAX_RING_CAPACITY).contains(&capacity)
            || !capacity.is_power_of_two()
        {
            return Err(WintunError::InvalidRingCapacity(capacity));
        }
        // SAFETY: adapter remains live through the clone stored in the session.
        let handle = unsafe { (self.0.library.start_session)(self.0.handle, capacity) };
        if handle.is_null() {
            return Err(WintunError::Windows(
                "WintunStartSession",
                io::Error::last_os_error(),
            ));
        }
        Ok(WintunSession {
            adapter: self.clone(),
            handle,
            trace,
        })
    }

    pub fn release_requested(&self, generation: u64) {
        self.0.trace.set_generation(generation);
        let mut event = self.0.trace.event(TraceStage::AdapterReleaseRequested);
        // This is diagnostic evidence only, never a liveness/safety decision.
        event.reference_count = u32::try_from(Arc::strong_count(&self.0)).ok();
        self.0.trace.emit(event);
    }
}

struct AdapterInner {
    library: Arc<WintunLibrary>,
    handle: AdapterHandle,
    name: String,
    trace: TraceResource,
}

// SAFETY: adapter handle is owned uniquely; WintunLibrary is already Send.
unsafe impl Send for AdapterInner {}
// SAFETY: `&AdapterInner` is safe to share: the adapter handle is an opaque
// immutable ID after open, library is Sync, and close runs only on exclusive Drop.
unsafe impl Sync for AdapterInner {}

impl Drop for AdapterInner {
    fn drop(&mut self) {
        if !self.handle.is_null() {
            self.trace.call(
                TraceStage::CloseAdapterStarted,
                TraceStage::CloseAdapterReturned,
                || {
                    // SAFETY: this object uniquely owns the adapter handle. A VOID
                    // return records completion of the call, not device absence.
                    unsafe {
                        (self.library.close_adapter)(self.handle);
                    }
                },
            );
        }
    }
}

pub struct WintunSession {
    adapter: WintunAdapter,
    handle: SessionHandle,
    trace: TraceResource,
}

// SAFETY: session handle is uniquely owned; adapter is Send and Sync.
unsafe impl Send for WintunSession {}
// SAFETY: `&WintunSession` is safe to share: the session handle is an opaque
// immutable ID for its lifetime, adapter is Sync, and end_session runs only on
// exclusive Drop (Wintun allows concurrent packet APIs under single ownership).
unsafe impl Sync for WintunSession {}

impl WintunSession {
    pub fn trace(&self) -> TraceResource {
        self.trace.clone()
    }
    pub fn adapter(&self) -> &WintunAdapter {
        &self.adapter
    }

    pub fn read_wait_event(&self) -> HANDLE {
        // SAFETY: session remains live and Wintun owns the returned event.
        unsafe { (self.adapter.0.library.get_read_wait_event)(self.handle) }
    }

    pub fn receive(&self) -> Result<Option<Vec<u8>>, WintunError> {
        let mut packet = Vec::new();
        if self.receive_into(&mut packet)? {
            Ok(Some(packet))
        } else {
            Ok(None)
        }
    }

    /// Receives into a caller-owned buffer so the packet pump can reuse its
    /// allocation. The buffer is unchanged when no packet is available or the
    /// Wintun record fails validation.
    pub fn receive_into(&self, output: &mut Vec<u8>) -> Result<bool, WintunError> {
        let mut size = 0_u32;
        // SAFETY: session is valid and size is writable.
        let packet = unsafe { (self.adapter.0.library.receive_packet)(self.handle, &mut size) };
        if packet.is_null() {
            let error = io::Error::last_os_error();
            return if error.raw_os_error() == Some(259) {
                Ok(false)
            } else {
                Err(WintunError::Windows("WintunReceivePacket", error))
            };
        }
        if size == 0 || size as usize > WINTUN_MAX_IP_PACKET_SIZE {
            // SAFETY: packet was returned by this session and must always be
            // released, including malformed-size failures.
            unsafe {
                (self.adapter.0.library.release_receive_packet)(self.handle, packet);
            }
            return Err(WintunError::InvalidPacketSize(size));
        }
        // SAFETY: Wintun guarantees `size` readable bytes until release.
        let source = unsafe { std::slice::from_raw_parts(packet, size as usize) };
        output.clear();
        output.extend_from_slice(source);
        // SAFETY: packet belongs to this session and is released exactly once.
        unsafe {
            (self.adapter.0.library.release_receive_packet)(self.handle, packet);
        }
        Ok(true)
    }

    pub fn send(&self, packet: &[u8]) -> Result<(), WintunError> {
        if packet.is_empty() || packet.len() > WINTUN_MAX_IP_PACKET_SIZE {
            return Err(WintunError::InvalidPacketSize(
                u32::try_from(packet.len()).unwrap_or(u32::MAX),
            ));
        }
        let length = u32::try_from(packet.len()).expect("Wintun packet bound fits u32");
        // SAFETY: session is valid and length passed the Wintun API bound.
        let destination =
            unsafe { (self.adapter.0.library.allocate_send_packet)(self.handle, length) };
        if destination.is_null() {
            return Err(WintunError::Windows(
                "WintunAllocateSendPacket",
                io::Error::last_os_error(),
            ));
        }
        // SAFETY: Wintun allocated exactly `length` writable bytes and the
        // non-overlapping source slice has the same length.
        unsafe {
            ptr::copy_nonoverlapping(packet.as_ptr(), destination, packet.len());
            (self.adapter.0.library.send_packet)(self.handle, destination);
        }
        Ok(())
    }
}

impl Drop for WintunSession {
    fn drop(&mut self) {
        if !self.handle.is_null() {
            self.trace.call(
                TraceStage::EndSessionStarted,
                TraceStage::EndSessionReturned,
                || {
                    // SAFETY: this object uniquely owns the session handle.
                    unsafe {
                        (self.adapter.0.library.end_session)(self.handle);
                    }
                },
            );
        }
    }
}

/// Recovery never opens Wintun: OpenAdapter/CloseAdapter can enqueue unrelated
/// orphan cleanup. The journal stores the RequestedGUID passed to pinned
/// Wintun 0.14.1, which uses it as SWD\Wintun's software-device instance ID.
#[derive(Debug, Default)]
pub struct AdapterRemovalState {
    request_accepted: bool,
}

struct AdapterObservation {
    interface_present: Option<bool>,
    device_present: Option<bool>,
    error: Option<WintunError>,
    interface: usque_ipc::agent_v1::RecoveryResourceObservation,
    pnp_device: usque_ipc::agent_v1::RecoveryResourceObservation,
}

impl AdapterObservation {
    fn absent(&self) -> bool {
        self.error.is_none()
            && self.interface_present == Some(false)
            && self.device_present == Some(false)
    }
}

fn observe_adapter(receipt: &MutationReceipt, guid: Uuid) -> AdapterObservation {
    // Observe both independently so an error is never reported as absence.
    let interface_state = network::inspect_adapter_state(receipt).map_err(interface_error);
    let mut details = resource_observation_ref(interface_state.as_ref().map(|s| s.is_some()));
    if details.api == 0 {
        details.api = usque_ipc::agent_v1::RecoveryDiagnosticApi::GetIfTable2 as i32;
    }
    if let Ok(Some(state)) = &interface_state {
        details.interface_oper_status = Some(state.oper_status);
        details.interface_admin_status = Some(state.admin_status);
        details.media_connect_state = Some(state.media_connect_state);
    }
    let interface = interface_state.map(|state| state.is_some());
    let device = device_instance_present(guid);
    let mut pnp_device = resource_observation_ref(device.as_ref().copied());
    if pnp_device.api == 0 {
        pnp_device.api = usque_ipc::agent_v1::RecoveryDiagnosticApi::SetupDiEnumDeviceInfo as i32;
    }
    AdapterObservation {
        interface_present: interface.as_ref().ok().copied(),
        device_present: device.as_ref().ok().copied(),
        error: interface.err().or_else(|| device.err()),
        interface: details,
        pnp_device,
    }
}

pub fn inspect_adapter_diagnostics(
    receipt: &MutationReceipt,
) -> (
    usque_ipc::agent_v1::RecoveryResourceObservation,
    usque_ipc::agent_v1::RecoveryResourceObservation,
) {
    use usque_ipc::agent_v1::{RecoveryIdentityCheck as Identity, RecoveryResourceObservation};
    let MutationReceipt::WintunAdapter {
        adapter_guid,
        adapter_name,
        ..
    } = receipt
    else {
        let invalid = RecoveryResourceObservation {
            identity_check: Identity::InvalidReceipt as i32,
            ..Default::default()
        };
        return (invalid, invalid);
    };
    if adapter_guid.is_nil() || adapter_name.is_empty() {
        let invalid = RecoveryResourceObservation {
            identity_check: Identity::InvalidReceipt as i32,
            ..Default::default()
        };
        return (invalid, invalid);
    }
    // Independent probes retain both errors. No library load, OpenAdapter,
    // service action, or cleanup action is performed here.
    let interface = match network::inspect_adapter_state(receipt).map_err(interface_error) {
        Ok(state) => {
            let mut observation = resource_observation(Ok(state.is_some()));
            if let Some(state) = state {
                observation.interface_oper_status = Some(state.oper_status);
                observation.interface_admin_status = Some(state.admin_status);
                observation.media_connect_state = Some(state.media_connect_state);
            }
            observation
        }
        Err(error) => resource_observation(Err(error)),
    };
    let device = match find_device_instance(*adapter_guid) {
        Ok(Some((_set, device))) => {
            let mut observation = resource_observation(Ok(true));
            let mut flags = 0;
            let mut problem = 0;
            // SAFETY: SetupAPI returned this devnode; writable outputs remain
            // valid for the read-only CONFIGRET call. No device is opened.
            let result =
                unsafe { CM_Get_DevNode_Status(&mut flags, &mut problem, device.DevInst, 0) };
            if result == 0 {
                observation.devnode_status = Some(flags);
                observation.problem_code = Some(problem);
            } else {
                observation.presence = 0;
                observation.api =
                    usque_ipc::agent_v1::RecoveryDiagnosticApi::CmGetDevNodeStatus as i32;
                observation.configret_code = Some(result);
            }
            observation
        }
        Ok(None) => resource_observation(Ok(false)),
        Err(error) => resource_observation(Err(error)),
    };
    (interface, device)
}

fn resource_observation(
    result: Result<bool, WintunError>,
) -> usque_ipc::agent_v1::RecoveryResourceObservation {
    resource_observation_ref(result.as_ref().copied())
}

fn resource_observation_ref(
    result: Result<bool, &WintunError>,
) -> usque_ipc::agent_v1::RecoveryResourceObservation {
    use usque_ipc::agent_v1::{RecoveryIdentityCheck as Identity, RecoveryResourceObservation};
    let mut observation = RecoveryResourceObservation::default();
    match result {
        Ok(present) => {
            observation.presence = crate::recovery_diagnostics::presence(Some(present));
            observation.identity_check = Identity::Verified as i32;
        }
        Err(WintunError::InvalidRecoveryIdentity | WintunError::AdapterIdentityMismatch(_)) => {
            observation.identity_check = Identity::Conflict as i32;
        }
        Err(WintunError::Windows(api, error)) => {
            observation.api =
                crate::recovery_diagnostics::diagnostic_api(Some(RecoveryApi::from_name(api)));
            observation.win32_code = error.raw_os_error().map(|code| code as u32);
        }
        Err(_) => {}
    }
    observation
}

pub fn remove_adapter_if_present(
    receipt: &MutationReceipt,
    state: &mut AdapterRemovalState,
    trace: &TraceResource,
) -> Result<(), WintunError> {
    let MutationReceipt::WintunAdapter {
        adapter_name,
        adapter_guid,
        ..
    } = receipt
    else {
        return Err(WintunError::InvalidRecoveryIdentity);
    };
    wide_name(adapter_name)?;
    let started = Instant::now();
    trace_adapter_removal(
        state,
        ADAPTER_REMOVAL_CONFIRM_TIMEOUT,
        (trace, || observe_adapter(receipt, *adapter_guid)),
        || remove_device_instance(*adapter_guid),
        || started.elapsed(),
        || thread::sleep(ADAPTER_REMOVAL_CONFIRM_INTERVAL),
    )
}

/// Record the checks cleanup already performs. This adds no native probe,
/// worker, wait, or change to the conditions that authorize removal/Clean.
fn trace_adapter_removal(
    state: &mut AdapterRemovalState,
    timeout: Duration,
    (trace, mut observe): (&TraceResource, impl FnMut() -> AdapterObservation),
    remove: impl FnMut() -> Result<bool, WintunError>,
    elapsed: impl FnMut() -> Duration,
    wait: impl FnMut(),
) -> Result<(), WintunError> {
    use usque_ipc::agent_v1::{RecoveryObservation, RecoverySampleStatus};
    trace.record(TraceStage::RemovalAttemptStarted);
    let started = Instant::now();
    let mut last: Option<RecoveryObservation> = None;
    let mut changes = 0;
    let result = remove_adapter_observed(
        state,
        timeout,
        || {
            let observed = observe();
            let sample = RecoveryObservation {
                sampled_at_unix_ms: crate::recovery_diagnostics::unix_ms(),
                journal_generation: trace.generation(),
                status: RecoverySampleStatus::Complete as i32,
                interface: Some(crate::recovery_trace::sanitize_resource(observed.interface)),
                pnp_device: Some(crate::recovery_trace::sanitize_resource(
                    observed.pnp_device,
                )),
            };
            if changes < 8
                && last.as_ref().is_none_or(|previous| {
                    previous.interface != sample.interface
                        || previous.pnp_device != sample.pnp_device
                })
            {
                changes += 1;
                let mut event = trace.event(TraceStage::ObservationChanged);
                event.observation = Some(sample);
                trace.emit(event);
            }
            last = Some(sample);
            observed
        },
        remove,
        elapsed,
        wait,
    );
    let mut event = trace.event(TraceStage::RemovalAttemptReturned);
    event.elapsed_ms = Some(crate::recovery_trace::milliseconds(started.elapsed()));
    event.succeeded = Some(result.is_ok());
    event.observation = last;
    trace.emit(event);
    result
}

fn remove_adapter_observed(
    state: &mut AdapterRemovalState,
    timeout: Duration,
    mut observe: impl FnMut() -> AdapterObservation,
    mut remove: impl FnMut() -> Result<bool, WintunError>,
    mut elapsed: impl FnMut() -> Duration,
    mut wait: impl FnMut(),
) -> Result<(), WintunError> {
    loop {
        let mut observation = observe();
        if observation.absent() {
            state.request_accepted = false;
            return Ok(());
        }
        if let Some(error) = observation.error.take() {
            return Err(removal_failure(
                state,
                &observation,
                RemovalStage::Observe,
                elapsed(),
                Some(error),
            ));
        }
        // WintunCloseAdapter starts asynchronous removal. Give it a bounded
        // observation window before requesting exact-device uninstallation.
        // A successful request is retained across automatic recovery passes;
        // waiting for PnP must not repeatedly issue DIF_REMOVE.
        if !state.request_accepted
            && observation.device_present == Some(true)
            && elapsed() >= ADAPTER_REMOVAL_CLOSE_GRACE
        {
            match remove() {
                Ok(accepted) => state.request_accepted = accepted,
                Err(error) => {
                    let after = observe();
                    if after.absent() {
                        state.request_accepted = false;
                        return Ok(());
                    }
                    return Err(removal_failure(
                        state,
                        &after,
                        RemovalStage::Request,
                        elapsed(),
                        Some(error),
                    ));
                }
            }
            observation = observe();
            if observation.absent() {
                state.request_accepted = false;
                return Ok(());
            }
            if let Some(error) = observation.error.take() {
                return Err(removal_failure(
                    state,
                    &observation,
                    RemovalStage::Confirm,
                    elapsed(),
                    Some(error),
                ));
            }
        }
        if elapsed() >= timeout {
            return Err(removal_failure(
                state,
                &observation,
                RemovalStage::Confirm,
                elapsed(),
                None,
            ));
        }
        wait();
    }
}

fn removal_failure(
    state: &AdapterRemovalState,
    observation: &AdapterObservation,
    stage: RemovalStage,
    elapsed: Duration,
    error: Option<WintunError>,
) -> WintunError {
    let (failure, api, win32_code) = match error {
        Some(WintunError::Windows(api, error)) => (
            RemovalFailure::Native,
            Some(RecoveryApi::from_name(api)),
            error.raw_os_error().map(|value| value as u32),
        ),
        Some(_) => (RemovalFailure::Identity, None, None),
        None => (RemovalFailure::Pending, None, None),
    };
    WintunError::Removal(AdapterRemovalDiagnostic {
        stage,
        failure,
        api,
        win32_code,
        interface_present: observation.interface_present,
        device_present: observation.device_present,
        request_accepted: state.request_accepted,
        elapsed_ms: elapsed.as_millis().min(u128::from(u64::MAX)) as u64,
    })
}

/// False means BOTH exact PnP and IP Helper absence, never a missing name,
/// stale LUID, or an unreadable registry value. This function is read-only.
pub fn adapter_resources_present(receipt: &MutationReceipt) -> Result<bool, WintunError> {
    let MutationReceipt::WintunAdapter { adapter_guid, .. } = receipt else {
        return Err(WintunError::InvalidRecoveryIdentity);
    };
    let started = Instant::now();
    let mut observation = observe_adapter(receipt, *adapter_guid);
    if let Some(error) = observation.error.take() {
        return Err(removal_failure(
            &AdapterRemovalState::default(),
            &observation,
            RemovalStage::Observe,
            started.elapsed(),
            Some(error),
        ));
    }
    Ok(!observation.absent())
}

#[cfg(test)]
fn adapter_resources_present_with(
    receipt: &MutationReceipt,
    inspect_interface: impl FnOnce(&MutationReceipt) -> Result<bool, WintunError>,
    inspect_device: impl FnOnce(Uuid) -> Result<bool, WintunError>,
) -> Result<bool, WintunError> {
    let MutationReceipt::WintunAdapter { adapter_guid, .. } = receipt else {
        return Err(WintunError::InvalidRecoveryIdentity);
    };
    let interface = inspect_interface(receipt)?;
    let device = inspect_device(*adapter_guid)?;
    Ok(interface || device)
}

fn interface_error(error: network::NetworkError) -> WintunError {
    match error {
        network::NetworkError::Windows { operation, code } => {
            WintunError::Windows(operation, io::Error::from_raw_os_error(code as i32))
        }
        _ => WintunError::InvalidRecoveryIdentity,
    }
}

fn remove_device_instance(expected_guid: Uuid) -> Result<bool, WintunError> {
    let Some((device_info, device)) = find_device_instance(expected_guid)? else {
        return Ok(false);
    };
    let parameters = SP_REMOVEDEVICE_PARAMS {
        ClassInstallHeader: SP_CLASSINSTALL_HEADER {
            cbSize: u32::try_from(mem::size_of::<SP_CLASSINSTALL_HEADER>())
                .expect("SP_CLASSINSTALL_HEADER size fits u32"),
            InstallFunction: DIF_REMOVE,
        },
        Scope: DI_REMOVEDEVICE_GLOBAL,
        HwProfile: 0,
    };
    // SAFETY: this set/device pair names the exact journaled software device.
    // The class-install buffer has the required DIF_REMOVE size and layout.
    if unsafe {
        SetupDiSetClassInstallParamsW(
            device_info.0,
            &device,
            &parameters.ClassInstallHeader,
            u32::try_from(mem::size_of::<SP_REMOVEDEVICE_PARAMS>())
                .expect("SP_REMOVEDEVICE_PARAMS size fits u32"),
        )
    } == 0
    {
        return Err(WintunError::Windows(
            "SetupDiSetClassInstallParamsW",
            io::Error::last_os_error(),
        ));
    }
    // SAFETY: the parameters above select global removal of this exact device.
    if unsafe { SetupDiCallClassInstaller(DIF_REMOVE, device_info.0, &device) } == 0 {
        return Err(WintunError::Windows(
            "SetupDiCallClassInstaller(DIF_REMOVE)",
            io::Error::last_os_error(),
        ));
    }
    Ok(true)
}

pub(super) fn device_instance_present(expected_guid: Uuid) -> Result<bool, WintunError> {
    find_device_instance(expected_guid).map(|device| device.is_some())
}

fn find_device_instance(
    expected_guid: Uuid,
) -> Result<Option<(DeviceInfoSet, SP_DEVINFO_DATA)>, WintunError> {
    if expected_guid.is_nil() {
        return Err(WintunError::InvalidRecoveryIdentity);
    }
    let device_info = DeviceInfoSet::network_adapters()?;
    for index in 0..4_096 {
        let mut device = SP_DEVINFO_DATA {
            cbSize: u32::try_from(mem::size_of::<SP_DEVINFO_DATA>())
                .expect("SP_DEVINFO_DATA size fits u32"),
            ..Default::default()
        };
        // SAFETY: the device set is live; the output has its required size.
        if unsafe { SetupDiEnumDeviceInfo(device_info.0, index, &mut device) } == 0 {
            let error = io::Error::last_os_error();
            if error.raw_os_error() == Some(ERROR_NO_MORE_ITEMS as i32) {
                return Ok(None);
            }
            return Err(WintunError::Windows("SetupDiEnumDeviceInfo", error));
        }
        // MAX_DEVICE_ID_LEN is 200 UTF-16 units, including the terminator.
        let mut id = [0_u16; 200];
        // SAFETY: the set/device pair was enumerated above and the writable
        // buffer length exactly matches its declared capacity.
        if unsafe {
            SetupDiGetDeviceInstanceIdW(
                device_info.0,
                &device,
                id.as_mut_ptr(),
                id.len() as u32,
                ptr::null_mut(),
            )
        } == 0
        {
            return Err(WintunError::Windows(
                "SetupDiGetDeviceInstanceIdW",
                io::Error::last_os_error(),
            ));
        }
        if device_instance_matches(expected_guid, &id)? {
            return Ok(Some((device_info, device)));
        }
    }
    Err(WintunError::InvalidRecoveryIdentity)
}

fn device_instance_matches(expected_guid: Uuid, id: &[u16]) -> Result<bool, WintunError> {
    if expected_guid.is_nil() {
        return Err(WintunError::InvalidRecoveryIdentity);
    }
    let end = id
        .iter()
        .position(|unit| *unit == 0)
        .ok_or(WintunError::InvalidRecoveryIdentity)?;
    let id = String::from_utf16(&id[..end]).map_err(|_| WintunError::InvalidRecoveryIdentity)?;
    Ok(id.eq_ignore_ascii_case(&format!(r"SWD\Wintun\{{{expected_guid}}}")))
}

#[cfg(test)]
fn wait_for_device_instance_removal<Probe>(
    adapter_name: &str,
    timeout: Duration,
    poll_interval: Duration,
    mut is_present: Probe,
) -> Result<(), WintunError>
where
    Probe: FnMut() -> Result<bool, WintunError>,
{
    let deadline = Instant::now() + timeout;
    loop {
        if !is_present()? {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(WintunError::AdapterRemovalIncomplete(
                adapter_name.to_owned(),
            ));
        }
        thread::sleep(poll_interval);
    }
}

struct DeviceInfoSet(HDEVINFO);

impl DeviceInfoSet {
    fn network_adapters() -> Result<Self, WintunError> {
        // Include non-present devices so a stale software adapter cannot hide
        // from uninstall merely because its interface is currently disabled.
        // SAFETY: GUID is static, optional pointers are null, and flags are valid.
        let handle =
            unsafe { SetupDiGetClassDevsW(&GUID_DEVCLASS_NET, ptr::null(), ptr::null_mut(), 0) };
        if handle == INVALID_HANDLE_VALUE as HDEVINFO {
            Err(WintunError::Windows(
                "SetupDiGetClassDevsW",
                io::Error::last_os_error(),
            ))
        } else {
            Ok(Self(handle))
        }
    }
}

impl Drop for DeviceInfoSet {
    fn drop(&mut self) {
        // SAFETY: this wrapper uniquely owns the SetupAPI device-info set.
        unsafe {
            SetupDiDestroyDeviceInfoList(self.0);
        }
    }
}

fn verify_hash(path: &Path) -> Result<(), WintunError> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    let actual: [u8; 32] = hasher.finalize().into();
    if actual != EXPECTED_DLL_SHA256 {
        return Err(WintunError::HashMismatch);
    }
    Ok(())
}

unsafe fn resolve<Function: Copy>(
    module: HMODULE,
    name: &'static [u8],
) -> Result<Function, WintunError> {
    // SAFETY: caller guarantees module is live and name is null-terminated.
    let function = unsafe { GetProcAddress(module, name.as_ptr()) }.ok_or_else(|| {
        WintunError::MissingExport(
            String::from_utf8_lossy(&name[..name.len().saturating_sub(1)]).into_owned(),
        )
    })?;
    if mem::size_of::<Function>() != mem::size_of_val(&function) {
        return Err(WintunError::InvalidFunctionPointer);
    }
    // SAFETY: the symbol's ABI/signature is fixed by pinned wintun.h; sizes
    // were checked above and Function is Copy.
    Ok(unsafe { mem::transmute_copy(&function) })
}

fn wide(value: &std::ffi::OsStr) -> Vec<u16> {
    value.encode_wide().chain(std::iter::once(0)).collect()
}

fn wide_name(value: &str) -> Result<Vec<u16>, WintunError> {
    if value.is_empty() || value.encode_utf16().count() >= 128 || value.contains('\0') {
        return Err(WintunError::InvalidAdapterName);
    }
    Ok(value.encode_utf16().chain(std::iter::once(0)).collect())
}

fn name_to_string(name: &[u16]) -> String {
    String::from_utf16_lossy(&name[..name.len().saturating_sub(1)])
}

#[derive(Debug, Error)]
pub enum WintunError {
    #[error("{0}")]
    Removal(AdapterRemovalDiagnostic),
    #[error("Wintun DLL path must be an absolute path ending in wintun.dll: {0}")]
    InvalidPath(PathBuf),
    #[error("Wintun DLL SHA-256 does not match the pinned official 0.14.1 binary")]
    HashMismatch,
    #[error("Wintun DLL is missing export {0}")]
    MissingExport(String),
    #[error("Wintun export has an unexpected function-pointer representation")]
    InvalidFunctionPointer,
    #[error("Wintun adapter name is empty, overlong, or contains NUL")]
    InvalidAdapterName,
    #[error("Wintun ring capacity must be a power of two between 128 KiB and 64 MiB: {0}")]
    InvalidRingCapacity(u32),
    #[error("Wintun returned an invalid IP packet size: {0}")]
    InvalidPacketSize(u32),
    #[error("Wintun recovery receipt has an invalid adapter identity")]
    InvalidRecoveryIdentity,
    #[error("Wintun adapter identity no longer matches the recovery journal: {0}")]
    AdapterIdentityMismatch(String),
    #[error("Windows reported success but the Wintun adapter still exists: {0}")]
    AdapterRemovalIncomplete(String),
    #[error("Wintun file I/O failed: {0}")]
    Io(#[from] io::Error),
    #[error("Windows {0} failed: {1}")]
    Windows(&'static str, io::Error),
}

impl WintunError {
    pub fn raw_os_error(&self) -> Option<i32> {
        match self {
            Self::Io(error) | Self::Windows(_, error) => error.raw_os_error(),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn memory_library() -> Arc<WintunLibrary> {
        unsafe extern "system" fn create(
            _: *const u16,
            _: *const u16,
            _: *const GUID,
        ) -> AdapterHandle {
            ptr::dangling_mut()
        }
        unsafe extern "system" fn open(_: *const u16) -> AdapterHandle {
            ptr::dangling_mut()
        }
        unsafe extern "system" fn close(_: AdapterHandle) {}
        unsafe extern "system" fn luid(_: AdapterHandle, _: *mut NET_LUID_LH) {}
        unsafe extern "system" fn version() -> u32 {
            1
        }
        unsafe extern "system" fn start(_: AdapterHandle, _: u32) -> SessionHandle {
            ptr::dangling_mut()
        }
        unsafe extern "system" fn event(_: SessionHandle) -> HANDLE {
            ptr::null_mut()
        }
        unsafe extern "system" fn receive(_: SessionHandle, _: *mut u32) -> *mut u8 {
            ptr::null_mut()
        }
        unsafe extern "system" fn release(_: SessionHandle, _: *const u8) {}
        unsafe extern "system" fn allocate(_: SessionHandle, _: u32) -> *mut u8 {
            ptr::null_mut()
        }
        unsafe extern "system" fn logger(_: Option<LoggerCallback>) {}
        Arc::new(WintunLibrary {
            module: ptr::null_mut(),
            create_adapter: create,
            open_adapter: open,
            close_adapter: close,
            get_adapter_luid: luid,
            get_running_driver_version: version,
            start_session: start,
            end_session: close,
            get_read_wait_event: event,
            receive_packet: receive,
            release_receive_packet: release,
            allocate_send_packet: allocate,
            send_packet: release,
            set_logger: logger,
        })
    }

    #[test]
    fn session_and_last_adapter_reference_emit_the_actual_foreign_call_boundaries() {
        let (sink, receiver) = TraceSink::channel(16);
        let adapter = memory_library()
            .create_adapter("memory-only", Uuid::new_v4(), sink.resource(8))
            .unwrap();
        let session = adapter
            .start_session(WINTUN_MIN_RING_CAPACITY, sink.resource(11))
            .unwrap();
        let retained = adapter.clone();
        adapter.release_requested(20);
        drop(adapter);
        let rows: Vec<_> = receiver.try_iter().collect();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].stage, TraceStage::AdapterReleaseRequested as i32);
        assert_eq!(rows[0].reference_count, Some(3));
        session.trace().set_generation(19);
        drop(session);
        let rows: Vec<_> = receiver.try_iter().collect();
        assert_eq!(
            rows.iter().map(|e| e.stage).collect::<Vec<_>>(),
            [
                TraceStage::EndSessionStarted as i32,
                TraceStage::EndSessionReturned as i32
            ]
        );
        assert!(
            rows.iter()
                .all(|e| e.journal_generation == 19 && e.succeeded.is_none())
        );
        drop(retained);
        let rows: Vec<_> = receiver.try_iter().collect();
        assert_eq!(
            rows.iter().map(|e| e.stage).collect::<Vec<_>>(),
            [
                TraceStage::CloseAdapterStarted as i32,
                TraceStage::CloseAdapterReturned as i32
            ]
        );
        assert!(
            rows.iter()
                .all(|e| e.journal_generation == 20 && e.succeeded.is_none())
        );
    }

    fn observation(interface: bool, device: bool) -> AdapterObservation {
        AdapterObservation {
            interface_present: Some(interface),
            device_present: Some(device),
            error: None,
            interface: resource_observation(Ok(interface)),
            pnp_device: resource_observation(Ok(device)),
        }
    }

    #[test]
    fn every_removal_attempt_records_final_interface_state_including_eventual_success() {
        let (sink, receiver) = TraceSink::channel(64);
        let mut state = AdapterRemovalState::default();
        for (generation, absent) in [(25, false), (27, false), (29, false), (31, true)] {
            let clock = std::cell::Cell::new(Duration::ZERO);
            let calls = std::cell::Cell::new(0);
            let trace = sink.resource(generation);
            let result = trace_adapter_removal(
                &mut state,
                Duration::from_secs(10),
                (&trace, || {
                    calls.set(calls.get() + 1);
                    let mut sample = observation(!absent, false);
                    sample.interface.interface_oper_status = Some(2);
                    sample.interface.interface_admin_status = Some(1);
                    sample.interface.media_connect_state = Some(2);
                    sample
                }),
                || panic!("PnP is absent: no removal request"),
                || clock.get(),
                || clock.set(clock.get() + Duration::from_secs(1)),
            );
            assert_eq!(result.is_ok(), absent);
            assert_eq!(
                calls.get(),
                if absent { 1 } else { 11 },
                "trace adds no probe"
            );
            let rows: Vec<_> = receiver
                .try_iter()
                .filter_map(crate::recovery_trace::sanitize_event)
                .collect();
            assert_eq!(rows.len(), 3);
            assert_eq!(rows[0].stage, TraceStage::RemovalAttemptStarted as i32);
            assert!(rows.iter().all(|row| row.journal_generation == generation));
            let last = rows.last().unwrap();
            assert_eq!(last.stage, TraceStage::RemovalAttemptReturned as i32);
            assert_eq!(last.succeeded, Some(absent));
            let sample = last.observation.as_ref().unwrap();
            assert_ne!(sample.sampled_at_unix_ms, 0);
            assert_eq!(
                sample.interface.as_ref().unwrap().interface_oper_status,
                (!absent).then_some(2)
            );
            assert_eq!(
                sample.pnp_device.as_ref().unwrap().presence,
                usque_ipc::agent_v1::RecoveryPresence::Absent as i32
            );
        }
    }

    #[test]
    fn tracing_identity_failure_and_full_writer_queue_cannot_change_cleanup_result() {
        for capacity in [0, 16] {
            let (sink, receiver) = TraceSink::channel(capacity);
            let trace = sink.resource(25);
            let error = trace_adapter_removal(
                &mut AdapterRemovalState::default(),
                Duration::from_secs(10),
                (&trace, || {
                    let error =
                        WintunError::Windows("GetIfTable2", io::Error::from_raw_os_error(5));
                    let interface = resource_observation_ref(Err(&error));
                    AdapterObservation {
                        interface_present: None,
                        device_present: Some(false),
                        error: Some(error),
                        interface,
                        pnp_device: resource_observation(Ok(false)),
                    }
                }),
                || panic!("unknown cannot authorize removal"),
                || Duration::ZERO,
                || panic!("identity failure does not wait"),
            )
            .unwrap_err();
            assert!(matches!(
                error,
                WintunError::Removal(AdapterRemovalDiagnostic {
                    win32_code: Some(5),
                    ..
                })
            ));
            let rows: Vec<_> = receiver
                .try_iter()
                .filter_map(crate::recovery_trace::sanitize_event)
                .collect();
            if capacity == 0 {
                assert!(rows.is_empty());
                assert_eq!(sink.losses().0, 3);
            } else {
                let last = rows.last().unwrap();
                assert_eq!(last.succeeded, Some(false));
                let interface = last
                    .observation
                    .as_ref()
                    .unwrap()
                    .interface
                    .as_ref()
                    .unwrap();
                assert_eq!(interface.presence, 0);
                assert_eq!(interface.win32_code, Some(5));
            }
        }
    }

    #[test]
    fn absent_adapter_is_idempotent_without_a_removal_request() {
        remove_adapter_observed(
            &mut AdapterRemovalState::default(),
            Duration::from_secs(10),
            || observation(false, false),
            || panic!("must not remove an absent device"),
            || Duration::ZERO,
            || panic!("must not wait"),
        )
        .unwrap();
    }

    #[test]
    fn asynchronous_close_can_finish_during_observation_without_dif_remove() {
        let clock = std::cell::Cell::new(Duration::ZERO);
        remove_adapter_observed(
            &mut AdapterRemovalState::default(),
            Duration::from_secs(10),
            || observation(clock.get() < Duration::from_secs(1), false),
            || panic!("PnP is already absent"),
            || clock.get(),
            || clock.set(clock.get() + Duration::from_millis(100)),
        )
        .unwrap();
        assert_eq!(clock.get(), Duration::from_secs(1));
    }

    #[test]
    fn accepted_removal_is_not_reissued_across_bounded_recovery_passes() {
        let mut state = AdapterRemovalState::default();
        let calls = std::cell::Cell::new(0);
        for _ in 0..2 {
            let clock = std::cell::Cell::new(Duration::ZERO);
            let error = remove_adapter_observed(
                &mut state,
                Duration::from_secs(10),
                || observation(false, true),
                || {
                    calls.set(calls.get() + 1);
                    Ok(true)
                },
                || clock.get(),
                || clock.set(clock.get() + Duration::from_secs(1)),
            )
            .unwrap_err();
            let WintunError::Removal(diagnostic) = error else {
                panic!("typed pending required")
            };
            assert_eq!(diagnostic.failure, RemovalFailure::Pending);
            assert_eq!(diagnostic.interface_present, Some(false));
            assert_eq!(diagnostic.device_present, Some(true));
            assert!(diagnostic.request_accepted);
            assert_eq!(diagnostic.elapsed_ms, 10_000);
        }
        assert_eq!(calls.get(), 1);
        remove_adapter_observed(
            &mut state,
            Duration::from_secs(10),
            || observation(false, false),
            || panic!("already removed"),
            || Duration::ZERO,
            || panic!("already removed"),
        )
        .unwrap();
        assert!(!state.request_accepted);
    }

    #[test]
    fn disappearance_after_a_failed_request_is_still_successful_cleanup() {
        let mut snapshots = [observation(true, true), observation(false, false)].into_iter();
        remove_adapter_observed(
            &mut AdapterRemovalState::default(),
            Duration::from_secs(10),
            || snapshots.next().unwrap(),
            || {
                Err(WintunError::Windows(
                    "SetupDiCallClassInstaller(DIF_REMOVE)",
                    io::Error::from_raw_os_error(170),
                ))
            },
            || Duration::from_secs(2),
            || panic!("absence was proved"),
        )
        .unwrap();
    }

    #[test]
    fn probe_errors_and_identity_conflicts_never_authorize_removal() {
        for native in [false, true] {
            let error = remove_adapter_observed(
                &mut AdapterRemovalState::default(),
                Duration::from_secs(10),
                || AdapterObservation {
                    interface_present: None,
                    device_present: Some(false),
                    error: Some(if native {
                        WintunError::Windows("GetIfTable2", io::Error::from_raw_os_error(5))
                    } else {
                        WintunError::InvalidRecoveryIdentity
                    }),
                    interface: Default::default(),
                    pnp_device: resource_observation(Ok(false)),
                },
                || panic!("unverified identity must not be deleted"),
                || Duration::ZERO,
                || panic!("must fail closed"),
            )
            .unwrap_err();
            let WintunError::Removal(diagnostic) = error else {
                panic!("typed failure required")
            };
            assert_eq!(diagnostic.interface_present, None);
            assert_eq!(diagnostic.device_present, Some(false));
            assert_eq!(diagnostic.win32_code, native.then_some(5));
            assert!(!diagnostic.request_accepted);
        }
    }

    #[test]
    fn failed_native_removal_keeps_its_api_code_and_observation() {
        let error = remove_adapter_observed(
            &mut AdapterRemovalState::default(),
            Duration::from_secs(10),
            || observation(false, true),
            || {
                Err(WintunError::Windows(
                    "SetupDiCallClassInstaller(DIF_REMOVE)",
                    io::Error::from_raw_os_error(5),
                ))
            },
            || Duration::from_secs(2),
            || panic!("must report the real error"),
        )
        .unwrap_err();
        let WintunError::Removal(diagnostic) = error else {
            panic!("typed failure required")
        };
        assert_eq!(diagnostic.stage, RemovalStage::Request);
        assert_eq!(diagnostic.failure, RemovalFailure::Native);
        assert_eq!(diagnostic.api, Some(RecoveryApi::SetupDiCallClassInstaller));
        assert_eq!(diagnostic.win32_code, Some(5));
    }

    #[test]
    fn removal_waits_for_both_pnp_and_ip_helper_without_reopening_wintun() {
        let receipt = MutationReceipt::WintunAdapter {
            adapter_name: "Usque-0123456789ab".to_owned(),
            adapter_guid: Uuid::new_v4(),
            interface_luid: 7,
        };
        // PnP has disappeared but IP Helper still has its old row. This is
        // pending removal, not a permanent identity conflict or early success.
        let mut observations = [(true, true), (true, false), (false, false)].into_iter();
        wait_for_device_instance_removal(
            "Usque-0123456789ab",
            Duration::from_secs(1),
            Duration::ZERO,
            || {
                let (interface, device) = observations.next().expect("bounded probe");
                adapter_resources_present_with(&receipt, |_| Ok(interface), |_| Ok(device))
            },
        )
        .unwrap();
        assert!(observations.next().is_none());
        assert!(
            adapter_resources_present_with(&receipt, |_| Ok(false), |_| Ok(true)).unwrap(),
            "a disabled/partial device still exists"
        );
        assert!(
            adapter_resources_present_with(
                &receipt,
                |_| Ok(false),
                |_| Err(WintunError::Windows(
                    "PnP probe",
                    io::Error::from_raw_os_error(5)
                ))
            )
            .is_err()
        );
    }

    fn official_dll() -> PathBuf {
        let architecture = if cfg!(target_arch = "aarch64") {
            "arm64"
        } else {
            "amd64"
        };
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../third_party/wintun-0.14.1/wintun/bin")
            .join(architecture)
            .join("wintun.dll")
            .canonicalize()
            .expect("official Wintun dependency")
    }

    #[test]
    fn pinned_official_library_loads_all_required_exports_without_installing_driver() {
        let library = WintunLibrary::load(&official_dll()).expect("load function table");
        assert!(Arc::strong_count(&library) == 1);
    }

    #[test]
    fn modified_library_is_rejected_before_load() {
        let directory = tempfile::tempdir().expect("tempdir");
        let path = directory.path().join("wintun.dll");
        let mut bytes = std::fs::read(official_dll()).expect("read");
        bytes[0] ^= 0xff;
        std::fs::write(&path, bytes).expect("fixture");
        let path = path.canonicalize().expect("path");
        assert!(matches!(
            WintunLibrary::load(&path),
            Err(WintunError::HashMismatch)
        ));
    }

    #[test]
    fn adapter_and_packet_bounds_are_checked_without_driver_calls() {
        assert!(wide_name("").is_err());
        assert!(wide_name(&"x".repeat(128)).is_err());
        assert!(wide_name("Usque").is_ok());
        assert_eq!(WINTUN_MIN_RING_CAPACITY, 128 * 1024);
        assert_eq!(WINTUN_MAX_RING_CAPACITY, 64 * 1024 * 1024);
    }

    #[test]
    fn exact_software_device_identity_is_required_without_registry_reads() {
        let expected = Uuid::parse_str("d2f0aa15-fb6b-4d89-8fa9-58cf825086f9").expect("guid");
        for (id, matches) in [
            (r"SWD\WINTUN\{D2F0AA15-FB6B-4D89-8FA9-58CF825086F9}", true),
            (r"swd\wintun\{d2f0aa15-fb6b-4d89-8fa9-58cf825086f9}", true),
            (r"SWD\OTHER\{D2F0AA15-FB6B-4D89-8FA9-58CF825086F9}", false),
            (r"SWD\WINTUN\{00000000-0000-4000-8000-000000000002}", false),
            (
                r"SWD\WINTUN\{D2F0AA15-FB6B-4D89-8FA9-58CF825086F9}\extra",
                false,
            ),
        ] {
            let id = id.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
            assert_eq!(device_instance_matches(expected, &id).unwrap(), matches);
        }
        assert!(device_instance_matches(expected, &[0xd800, 0]).is_err());
        assert!(device_instance_matches(expected, &[b'x' as u16]).is_err());
        assert!(device_instance_matches(Uuid::nil(), &[0]).is_err());
    }

    #[test]
    fn adapter_removal_confirmation_accepts_eventual_exact_device_absence() {
        let mut observations = [true, true, false].into_iter();
        wait_for_device_instance_removal(
            "Usque-0123456789ab",
            Duration::from_secs(1),
            Duration::ZERO,
            || Ok(observations.next().expect("bounded observations")),
        )
        .expect("eventual removal");
        assert!(observations.next().is_none());
    }

    #[test]
    fn adapter_removal_confirmation_fails_closed_while_device_remains() {
        assert!(matches!(
            wait_for_device_instance_removal(
                "Usque-0123456789ab",
                Duration::ZERO,
                Duration::ZERO,
                || Ok(true),
            ),
            Err(WintunError::AdapterRemovalIncomplete(name))
                if name == "Usque-0123456789ab"
        ));
    }

    #[test]
    fn adapter_removal_confirmation_preserves_probe_failures() {
        assert!(matches!(
            wait_for_device_instance_removal(
                "Usque-0123456789ab",
                Duration::from_secs(1),
                Duration::ZERO,
                || Err(WintunError::InvalidRecoveryIdentity),
            ),
            Err(WintunError::InvalidRecoveryIdentity)
        ));
        for code in [5, 13, 170] {
            let result = wait_for_device_instance_removal(
                "Usque-0123456789ab",
                Duration::ZERO,
                Duration::ZERO,
                || {
                    Err(WintunError::Windows(
                        "SetupDiGetDeviceInstanceIdW",
                        io::Error::from_raw_os_error(code),
                    ))
                },
            );
            assert_eq!(result.unwrap_err().raw_os_error(), Some(code));
        }
    }
}
