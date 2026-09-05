use std::{
    ffi::c_void,
    fs::File,
    io::{self, Read},
    mem,
    os::windows::ffi::OsStrExt,
    path::{Path, PathBuf},
    ptr,
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

use sha2::{Digest, Sha256};
use thiserror::Error;
use uuid::Uuid;
use windows_sys::{
    Win32::{
        Devices::DeviceAndDriverInstallation::{
            DI_REMOVEDEVICE_GLOBAL, DIF_REMOVE, GUID_DEVCLASS_NET, HDEVINFO,
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

const WINTUN_DLL_NAME: &str = "wintun.dll";
const WINTUN_MIN_RING_CAPACITY: u32 = 0x20_000;
const WINTUN_MAX_RING_CAPACITY: u32 = 0x400_0000;
const WINTUN_MAX_IP_PACKET_SIZE: usize = 0xffff;
const ADAPTER_REMOVAL_CONFIRM_TIMEOUT: Duration = Duration::from_secs(2);
const ADAPTER_REMOVAL_CONFIRM_INTERVAL: Duration = Duration::from_millis(25);

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
        })))
    }

    pub fn open_adapter(self: &Arc<Self>, name: &str) -> Result<WintunAdapter, WintunError> {
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

    pub fn start_session(&self, capacity: u32) -> Result<WintunSession, WintunError> {
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
        })
    }
}

struct AdapterInner {
    library: Arc<WintunLibrary>,
    handle: AdapterHandle,
    name: String,
}

// SAFETY: adapter handle is owned uniquely; WintunLibrary is already Send.
unsafe impl Send for AdapterInner {}
// SAFETY: `&AdapterInner` is safe to share: the adapter handle is an opaque
// immutable ID after open, library is Sync, and close runs only on exclusive Drop.
unsafe impl Sync for AdapterInner {}

impl Drop for AdapterInner {
    fn drop(&mut self) {
        if !self.handle.is_null() {
            // SAFETY: this object uniquely owns the adapter handle.
            unsafe {
                (self.library.close_adapter)(self.handle);
            }
        }
    }
}

pub struct WintunSession {
    adapter: WintunAdapter,
    handle: SessionHandle,
}

// SAFETY: session handle is uniquely owned; adapter is Send and Sync.
unsafe impl Send for WintunSession {}
// SAFETY: `&WintunSession` is safe to share: the session handle is an opaque
// immutable ID for its lifetime, adapter is Sync, and end_session runs only on
// exclusive Drop (Wintun allows concurrent packet APIs under single ownership).
unsafe impl Sync for WintunSession {}

impl WintunSession {
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
            // SAFETY: this object uniquely owns the session handle.
            unsafe {
                (self.adapter.0.library.end_session)(self.handle);
            }
        }
    }
}

/// Recovery never opens Wintun: OpenAdapter/CloseAdapter can enqueue unrelated
/// orphan cleanup. The journal stores the RequestedGUID passed to pinned
/// Wintun 0.14.1, which uses it as SWD\Wintun's software-device instance ID.
pub fn remove_adapter_if_present(receipt: &MutationReceipt) -> Result<(), WintunError> {
    let MutationReceipt::WintunAdapter {
        adapter_name,
        adapter_guid,
        ..
    } = receipt
    else {
        return Err(WintunError::InvalidRecoveryIdentity);
    };
    wide_name(adapter_name)?;
    // Reject a renamed/replaced live interface BEFORE any SetupAPI mutation.
    interface_instance_present(receipt)?;
    let removal = remove_device_instance(*adapter_guid);
    let confirmation = wait_for_device_instance_removal(
        adapter_name,
        ADAPTER_REMOVAL_CONFIRM_TIMEOUT,
        ADAPTER_REMOVAL_CONFIRM_INTERVAL,
        || adapter_resources_present(receipt),
    );
    match (removal, confirmation) {
        (_, Ok(())) => Ok(()),
        (Err(error), Err(_)) | (Ok(()), Err(error)) => Err(error),
    }
}

/// False means BOTH exact PnP and IP Helper absence, never a missing name,
/// stale LUID, or an unreadable registry value. This function is read-only.
pub fn adapter_resources_present(receipt: &MutationReceipt) -> Result<bool, WintunError> {
    adapter_resources_present_with(receipt, interface_instance_present, device_instance_present)
}

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

fn interface_instance_present(receipt: &MutationReceipt) -> Result<bool, WintunError> {
    network::inspect_adapter_identity(receipt).map_err(|error| match error {
        network::NetworkError::Windows { operation, code } => {
            WintunError::Windows(operation, io::Error::from_raw_os_error(code as i32))
        }
        _ => WintunError::InvalidRecoveryIdentity,
    })
}

fn remove_device_instance(expected_guid: Uuid) -> Result<(), WintunError> {
    let Some((device_info, device)) = find_device_instance(expected_guid)? else {
        return Ok(());
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
    Ok(())
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
