use std::{
    ffi::c_void,
    fs::File,
    io::{self, Read},
    mem,
    os::windows::ffi::OsStrExt,
    path::{Path, PathBuf},
    ptr,
    sync::Arc,
};

use sha2::{Digest, Sha256};
use thiserror::Error;
use uuid::Uuid;
use windows_sys::{
    Win32::{
        Devices::DeviceAndDriverInstallation::{
            DI_REMOVEDEVICE_GLOBAL, DICS_FLAG_GLOBAL, DIF_REMOVE, DIREG_DRV, GUID_DEVCLASS_NET,
            HDEVINFO, SP_CLASSINSTALL_HEADER, SP_DEVINFO_DATA, SP_REMOVEDEVICE_PARAMS,
            SetupDiCallClassInstaller, SetupDiDestroyDeviceInfoList, SetupDiEnumDeviceInfo,
            SetupDiGetClassDevsW, SetupDiOpenDevRegKey, SetupDiSetClassInstallParamsW,
        },
        Foundation::{
            ERROR_FILE_NOT_FOUND, ERROR_NO_MORE_ITEMS, ERROR_NOT_FOUND, ERROR_SUCCESS, FreeLibrary,
            HANDLE, HMODULE, INVALID_HANDLE_VALUE,
        },
        NetworkManagement::{IpHelper::ConvertInterfaceLuidToGuid, Ndis::NET_LUID_LH},
        System::{
            LibraryLoader::{
                GetProcAddress, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR, LOAD_LIBRARY_SEARCH_SYSTEM32,
                LoadLibraryExW,
            },
            Registry::{HKEY, KEY_QUERY_VALUE, RRF_RT_REG_SZ, RegCloseKey, RegGetValueW},
        },
    },
    core::GUID,
};

const WINTUN_DLL_NAME: &str = "wintun.dll";
const WINTUN_MIN_RING_CAPACITY: u32 = 0x20_000;
const WINTUN_MAX_RING_CAPACITY: u32 = 0x400_0000;
const WINTUN_MAX_IP_PACKET_SIZE: usize = 0xffff;

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

    /// Removes the exact journal-owned adapter after a process crash.
    ///
    /// `WintunCloseAdapter` removes adapters only when the handle came from
    /// `WintunCreateAdapter`. A fresh recovery process can obtain only an
    /// `WintunOpenAdapter` handle, so closing that handle is not sufficient.
    /// Verify the journaled name, interface GUID, and (when known) LUID before
    /// asking SetupAPI to remove the matching device instance.
    pub fn remove_adapter_if_present(
        self: &Arc<Self>,
        name: &str,
        expected_guid: Uuid,
        expected_luid: u64,
    ) -> Result<(), WintunError> {
        if expected_guid.is_nil() {
            return Err(WintunError::InvalidRecoveryIdentity);
        }
        let adapter = match self.open_adapter(name) {
            Ok(adapter) => adapter,
            Err(error)
                if matches!(
                    error.raw_os_error(),
                    Some(code) if code == ERROR_NOT_FOUND as i32
                        || code == ERROR_FILE_NOT_FOUND as i32
                ) =>
            {
                return Ok(());
            }
            Err(error) => return Err(error),
        };

        let actual_luid = adapter.luid();
        if actual_luid == 0 || expected_luid != 0 && actual_luid != expected_luid {
            return Err(WintunError::AdapterIdentityMismatch(name.to_owned()));
        }
        let mut luid = NET_LUID_LH::default();
        luid.Value = actual_luid;
        let mut actual_guid = GUID::default();
        // SAFETY: both structures are initialized and remain live for the call.
        let status = unsafe { ConvertInterfaceLuidToGuid(&luid, &mut actual_guid) };
        if status != ERROR_SUCCESS {
            return Err(WintunError::Windows(
                "ConvertInterfaceLuidToGuid",
                io::Error::from_raw_os_error(status as i32),
            ));
        }
        if !guid_equals(&actual_guid, &GUID::from_u128(expected_guid.as_u128())) {
            return Err(WintunError::AdapterIdentityMismatch(name.to_owned()));
        }

        // This handle was opened rather than created, so dropping it releases
        // resources but intentionally does not remove the device instance.
        drop(adapter);
        remove_device_instance(expected_guid)?;

        // Verify the named adapter is no longer available. If the name was
        // reused concurrently, fail safely rather than targeting the new one.
        match self.open_adapter(name) {
            Err(error)
                if matches!(
                    error.raw_os_error(),
                    Some(code) if code == ERROR_NOT_FOUND as i32
                        || code == ERROR_FILE_NOT_FOUND as i32
                ) =>
            {
                Ok(())
            }
            Ok(adapter) => {
                let remaining_luid = adapter.luid();
                drop(adapter);
                if remaining_luid == actual_luid {
                    Err(WintunError::AdapterRemovalIncomplete(name.to_owned()))
                } else {
                    Err(WintunError::AdapterIdentityMismatch(name.to_owned()))
                }
            }
            Err(error) => Err(error),
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
        let mut size = 0_u32;
        // SAFETY: session is valid and size is writable.
        let packet = unsafe { (self.adapter.0.library.receive_packet)(self.handle, &mut size) };
        if packet.is_null() {
            let error = io::Error::last_os_error();
            return if error.raw_os_error() == Some(259) {
                Ok(None)
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
        let output = unsafe { std::slice::from_raw_parts(packet, size as usize) }.to_vec();
        // SAFETY: packet belongs to this session and is released exactly once.
        unsafe {
            (self.adapter.0.library.release_receive_packet)(self.handle, packet);
        }
        Ok(Some(output))
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

fn remove_device_instance(expected_guid: Uuid) -> Result<(), WintunError> {
    let device_info = DeviceInfoSet::network_adapters()?;
    let mut index = 0_u32;
    loop {
        let mut device = SP_DEVINFO_DATA {
            cbSize: u32::try_from(mem::size_of::<SP_DEVINFO_DATA>())
                .expect("SP_DEVINFO_DATA size fits u32"),
            ..Default::default()
        };
        // SAFETY: the device-info set is live and `device` has the required size.
        if unsafe { SetupDiEnumDeviceInfo(device_info.0, index, &mut device) } == 0 {
            let error = io::Error::last_os_error();
            if error.raw_os_error() == Some(ERROR_NO_MORE_ITEMS as i32) {
                return Ok(());
            }
            return Err(WintunError::Windows("SetupDiEnumDeviceInfo", error));
        }
        index = index.saturating_add(1);

        let Some(instance_id) =
            read_device_registry_string(device_info.0, &device, "NetCfgInstanceId")
        else {
            continue;
        };
        if parse_registry_guid(&instance_id) != Some(expected_guid) {
            continue;
        }

        let parameters = SP_REMOVEDEVICE_PARAMS {
            ClassInstallHeader: SP_CLASSINSTALL_HEADER {
                cbSize: u32::try_from(mem::size_of::<SP_CLASSINSTALL_HEADER>())
                    .expect("SP_CLASSINSTALL_HEADER size fits u32"),
                InstallFunction: DIF_REMOVE,
            },
            Scope: DI_REMOVEDEVICE_GLOBAL,
            HwProfile: 0,
        };
        // SAFETY: the set/device pair came from SetupAPI and the class-install
        // buffer has the exact structure and byte size required for DIF_REMOVE.
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
        // SAFETY: class-install parameters above describe a global removal for
        // this exact device information element.
        if unsafe { SetupDiCallClassInstaller(DIF_REMOVE, device_info.0, &device) } == 0 {
            return Err(WintunError::Windows(
                "SetupDiCallClassInstaller(DIF_REMOVE)",
                io::Error::last_os_error(),
            ));
        }
        return Ok(());
    }
}

fn read_device_registry_string(
    device_info: HDEVINFO,
    device: &SP_DEVINFO_DATA,
    value_name: &str,
) -> Option<String> {
    // SAFETY: the set/device pair came from SetupAPI. Read-only driver-key
    // access is sufficient for NetCfgInstanceId.
    let key = unsafe {
        SetupDiOpenDevRegKey(
            device_info,
            device,
            DICS_FLAG_GLOBAL,
            0,
            DIREG_DRV,
            KEY_QUERY_VALUE,
        )
    };
    if ptr::eq(key, INVALID_HANDLE_VALUE) {
        return None;
    }
    let key = RegistryKey(key);
    let name = value_name
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let mut byte_count = 0_u32;
    // SAFETY: key and value name are valid; the first call requests size only.
    let status = unsafe {
        RegGetValueW(
            key.0,
            ptr::null(),
            name.as_ptr(),
            RRF_RT_REG_SZ,
            ptr::null_mut(),
            ptr::null_mut(),
            &mut byte_count,
        )
    };
    if status != ERROR_SUCCESS
        || byte_count == 0
        || byte_count > 512
        || !byte_count.is_multiple_of(2)
    {
        return None;
    }
    let mut value = vec![0_u16; byte_count as usize / 2];
    // SAFETY: the UTF-16 buffer has exactly the byte count returned above.
    let status = unsafe {
        RegGetValueW(
            key.0,
            ptr::null(),
            name.as_ptr(),
            RRF_RT_REG_SZ,
            ptr::null_mut(),
            value.as_mut_ptr().cast(),
            &mut byte_count,
        )
    };
    if status != ERROR_SUCCESS {
        return None;
    }
    let length = value
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(value.len());
    String::from_utf16(&value[..length]).ok()
}

fn parse_registry_guid(value: &str) -> Option<Uuid> {
    Uuid::parse_str(value.trim().trim_start_matches('{').trim_end_matches('}')).ok()
}

fn guid_equals(left: &GUID, right: &GUID) -> bool {
    left.data1 == right.data1
        && left.data2 == right.data2
        && left.data3 == right.data3
        && left.data4 == right.data4
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

struct RegistryKey(HKEY);

impl Drop for RegistryKey {
    fn drop(&mut self) {
        // SAFETY: this wrapper uniquely owns the registry handle.
        unsafe {
            RegCloseKey(self.0);
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
    fn windows_registry_guid_parser_is_strict_but_accepts_braces_and_case() {
        let expected = Uuid::parse_str("d2f0aa15-fb6b-4d89-8fa9-58cf825086f9").expect("guid");
        assert_eq!(
            parse_registry_guid(" {D2F0AA15-FB6B-4D89-8FA9-58CF825086F9} "),
            Some(expected)
        );
        assert_eq!(parse_registry_guid("not-a-guid"), None);
    }
}
