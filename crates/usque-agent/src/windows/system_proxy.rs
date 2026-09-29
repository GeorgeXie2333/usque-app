use std::{collections::HashSet, ffi::c_void, io, net::SocketAddr, ptr, slice, str::FromStr};

use thiserror::Error;
use uuid::Uuid;
use windows_sys::Win32::{
    Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS},
    Networking::WinInet::{
        INTERNET_OPTION_REFRESH, INTERNET_OPTION_SETTINGS_CHANGED, InternetSetOptionW,
    },
    System::LibraryLoader::{GetModuleHandleW, GetProcAddress},
    System::Registry::{
        HKEY, HKEY_USERS, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_DWORD, REG_EXPAND_SZ, REG_SZ,
        RegCloseKey, RegDeleteValueW, RegFlushKey, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW,
    },
};

use crate::{
    AuthenticatedCaller,
    coordinator::SystemProxySettings,
    journal::{MutationReceipt, valid_sid},
};

const INTERNET_SETTINGS_PATH: &str = r"Software\Microsoft\Windows\CurrentVersion\Internet Settings";
const OWNER_VALUE: &str = "UsqueProxyOwnerV1";
const PROXY_ENABLE_VALUE: &str = "ProxyEnable";
const PROXY_SERVER_VALUE: &str = "ProxyServer";
const PROXY_OVERRIDE_VALUE: &str = "ProxyOverride";
const AUTO_CONFIG_URL_VALUE: &str = "AutoConfigURL";
const AUTO_DETECT_VALUE: &str = "AutoDetect";
const MAX_REGISTRY_VALUE_BYTES: u32 = 16 * 1024;
const MAX_BYPASS_HOSTS: usize = 64;
const MAX_TEXT_BYTES: usize = 4096;

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProxySnapshot {
    proxy_enable: Option<u32>,
    proxy: Option<String>,
    bypass: Option<String>,
    auto_config_url: Option<String>,
    auto_detect: Option<u32>,
}

trait InternetSettings {
    fn read_dword(&self, name: &str) -> Result<Option<u32>, SystemProxyError>;
    fn read_string(&self, name: &str) -> Result<Option<String>, SystemProxyError>;
    fn write_dword(&self, name: &str, value: u32) -> Result<(), SystemProxyError>;
    fn write_string(&self, name: &str, value: &str) -> Result<(), SystemProxyError>;
    fn delete_value(&self, name: &str) -> Result<(), SystemProxyError>;
    fn flush(&self) -> Result<(), SystemProxyError>;

    fn snapshot(&self) -> Result<ProxySnapshot, SystemProxyError> {
        Ok(ProxySnapshot {
            proxy_enable: self.read_dword(PROXY_ENABLE_VALUE)?,
            proxy: self.read_string(PROXY_SERVER_VALUE)?,
            bypass: self.read_string(PROXY_OVERRIDE_VALUE)?,
            auto_config_url: self.read_string(AUTO_CONFIG_URL_VALUE)?,
            auto_detect: self.read_dword(AUTO_DETECT_VALUE)?,
        })
    }
}

pub fn plan(
    operation_id: Uuid,
    caller: &AuthenticatedCaller,
    settings: &SystemProxySettings,
) -> Result<MutationReceipt, SystemProxyError> {
    if operation_id.is_nil() || !valid_sid(&caller.user_sid) {
        return Err(SystemProxyError::InvalidOwner);
    }
    let applied_proxy = normalize_proxy_uri(&settings.proxy_uri)?;
    let applied_bypass = normalize_bypass_hosts(&settings.bypass_hosts)?;
    let key = UserInternetSettings::open(&caller.user_sid)?;
    if key.read_string(OWNER_VALUE)?.is_some() {
        return Err(SystemProxyError::AlreadyOwned);
    }
    let previous = key.snapshot()?;
    Ok(MutationReceipt::SystemProxy {
        user_sid: caller.user_sid.clone(),
        operation_id,
        previous_proxy_enable: previous.proxy_enable,
        previous_proxy: previous.proxy,
        previous_bypass: previous.bypass,
        previous_auto_config_url: previous.auto_config_url,
        previous_auto_detect: previous.auto_detect,
        applied_proxy,
        applied_bypass,
    })
}

pub fn apply(receipt: MutationReceipt) -> Result<MutationReceipt, SystemProxyError> {
    let key = UserInternetSettings::open(validate_receipt_values(&receipt)?)?;
    apply_with(&key, receipt, notify_settings_changed)
}

fn apply_with(
    key: &impl InternetSettings,
    receipt: MutationReceipt,
    notify: impl FnOnce() -> Result<(), SystemProxyError>,
) -> Result<MutationReceipt, SystemProxyError> {
    let MutationReceipt::SystemProxy {
        user_sid: _,
        operation_id,
        previous_proxy_enable,
        previous_proxy,
        previous_bypass,
        previous_auto_config_url,
        previous_auto_detect,
        applied_proxy,
        applied_bypass,
    } = &receipt
    else {
        return Err(SystemProxyError::WrongReceipt);
    };
    validate_receipt_values(&receipt)?;
    let expected = ProxySnapshot {
        proxy_enable: *previous_proxy_enable,
        proxy: previous_proxy.clone(),
        bypass: previous_bypass.clone(),
        auto_config_url: previous_auto_config_url.clone(),
        auto_detect: *previous_auto_detect,
    };
    let owner = key.read_string(OWNER_VALUE)?;
    if owner.as_deref() != Some(&operation_id.to_string()) {
        if owner.is_some() {
            return Err(SystemProxyError::AlreadyOwned);
        }
        if key.snapshot()? != expected {
            return Err(SystemProxyError::SnapshotChanged);
        }
        key.write_string(OWNER_VALUE, &operation_id.to_string())?;
    }

    key.write_string(PROXY_SERVER_VALUE, applied_proxy)?;
    key.write_string(PROXY_OVERRIDE_VALUE, applied_bypass)?;
    key.delete_value(AUTO_CONFIG_URL_VALUE)?;
    key.write_dword(AUTO_DETECT_VALUE, 0)?;
    // Enable the proxy last. Recovery can identify ownership from the marker
    // even if the process is terminated between any preceding writes.
    key.write_dword(PROXY_ENABLE_VALUE, 1)?;
    key.flush()?;
    notify()?;
    Ok(receipt)
}

pub fn restore(receipt: &MutationReceipt) -> Result<(), SystemProxyError> {
    let key = UserInternetSettings::open(validate_receipt_values(receipt)?)?;
    restore_with(&key, receipt, notify_settings_changed)
}

fn restore_with(
    key: &impl InternetSettings,
    receipt: &MutationReceipt,
    notify: impl FnOnce() -> Result<(), SystemProxyError>,
) -> Result<(), SystemProxyError> {
    validate_receipt_values(receipt)?;
    let MutationReceipt::SystemProxy {
        user_sid: _,
        operation_id,
        previous_proxy_enable,
        previous_proxy,
        previous_bypass,
        previous_auto_config_url,
        previous_auto_detect,
        applied_proxy,
        applied_bypass,
    } = receipt
    else {
        return Err(SystemProxyError::WrongReceipt);
    };
    match key.read_string(OWNER_VALUE)? {
        // A prior attempt may have deleted the marker and then failed to
        // flush. The journal still requires both durability and notification;
        // absence of the marker must not turn that failure into success.
        None => {
            key.flush()?;
            return notify();
        }
        Some(owner) if owner == operation_id.to_string() => {}
        Some(_) => return Err(SystemProxyError::OwnerChanged),
    }

    let proxy = key.read_string(PROXY_SERVER_VALUE)?;
    // The server identifies which manual proxy the enable/PAC flags belong
    // to. Preserve a replacement proxy as a complete configuration instead
    // of disabling it or reintroducing a previous PAC. Accept the previous
    // server as well so interrupted apply/restore attempts remain recoverable.
    if proxy.as_deref() == Some(applied_proxy) || proxy.as_deref() == previous_proxy.as_deref() {
        restore_dword_if_unchanged(key, PROXY_ENABLE_VALUE, Some(1), *previous_proxy_enable)?;
        restore_string_if_unchanged(
            key,
            PROXY_SERVER_VALUE,
            Some(applied_proxy),
            previous_proxy.as_deref(),
        )?;
        restore_string_if_unchanged(
            key,
            PROXY_OVERRIDE_VALUE,
            Some(applied_bypass),
            previous_bypass.as_deref(),
        )?;
        restore_string_if_unchanged(
            key,
            AUTO_CONFIG_URL_VALUE,
            None,
            previous_auto_config_url.as_deref(),
        )?;
        restore_dword_if_unchanged(key, AUTO_DETECT_VALUE, Some(0), *previous_auto_detect)?;
    }
    // Keep ownership until the settings have been durably restored and the
    // notification has succeeded, allowing either failure to be retried.
    key.flush()?;
    notify()?;
    key.delete_value(OWNER_VALUE)?;
    key.flush()
}

fn validate_receipt_values(receipt: &MutationReceipt) -> Result<&str, SystemProxyError> {
    let MutationReceipt::SystemProxy {
        user_sid,
        operation_id,
        previous_proxy_enable,
        previous_proxy,
        previous_bypass,
        previous_auto_config_url,
        previous_auto_detect,
        applied_proxy,
        applied_bypass,
    } = receipt
    else {
        return Err(SystemProxyError::WrongReceipt);
    };
    if !valid_sid(user_sid)
        || operation_id.is_nil()
        || previous_proxy_enable.is_some_and(|value| value > 1)
        || previous_auto_detect.is_some_and(|value| value > 1)
        || [
            previous_proxy.as_deref(),
            previous_bypass.as_deref(),
            previous_auto_config_url.as_deref(),
            Some(applied_proxy),
            Some(applied_bypass),
        ]
        .into_iter()
        .flatten()
        .any(|value| value.len() > MAX_TEXT_BYTES || value.contains('\0'))
        || applied_proxy.is_empty()
    {
        return Err(SystemProxyError::InvalidReceipt);
    }
    Ok(user_sid)
}

fn normalize_proxy_uri(value: &str) -> Result<String, SystemProxyError> {
    let authority = value
        .strip_prefix("http://")
        .ok_or(SystemProxyError::InvalidProxyUri)?;
    if authority.is_empty()
        || authority
            .bytes()
            .any(|byte| matches!(byte, b'/' | b'?' | b'#' | b'@' | b';'))
    {
        return Err(SystemProxyError::InvalidProxyUri);
    }
    let address = SocketAddr::from_str(authority).map_err(|_| SystemProxyError::InvalidProxyUri)?;
    if !address.ip().is_loopback() || address.port() == 0 {
        return Err(SystemProxyError::NonLoopbackProxy);
    }
    Ok(address.to_string())
}

fn normalize_bypass_hosts(values: &[String]) -> Result<String, SystemProxyError> {
    if values.len() > MAX_BYPASS_HOSTS {
        return Err(SystemProxyError::TooManyBypassHosts(values.len()));
    }
    let mut output = Vec::with_capacity(values.len() + 1);
    let mut unique = HashSet::new();
    for value in values {
        let value = value.trim();
        if value.is_empty()
            || value.len() > 255
            || !value.bytes().all(|byte| {
                byte.is_ascii_alphanumeric()
                    || matches!(
                        byte,
                        b'.' | b'*' | b':' | b'_' | b'-' | b'[' | b']' | b'<' | b'>' | b'/'
                    )
            })
        {
            return Err(SystemProxyError::InvalidBypassHost(value.to_owned()));
        }
        if value.starts_with('<') && value != "<local>" {
            return Err(SystemProxyError::InvalidBypassHost(value.to_owned()));
        }
        if unique.insert(value.to_ascii_lowercase()) {
            output.push(value.to_owned());
        }
    }
    if unique.insert("<local>".to_owned()) {
        output.push("<local>".to_owned());
    }
    let joined = output.join(";");
    if joined.len() > MAX_TEXT_BYTES {
        return Err(SystemProxyError::BypassListTooLong);
    }
    Ok(joined)
}

fn restore_dword_if_unchanged(
    key: &impl InternetSettings,
    name: &str,
    applied: Option<u32>,
    previous: Option<u32>,
) -> Result<(), SystemProxyError> {
    if key.read_dword(name)? == applied {
        match previous {
            Some(value) => key.write_dword(name, value),
            None => key.delete_value(name),
        }
    } else {
        Ok(())
    }
}

fn restore_string_if_unchanged(
    key: &impl InternetSettings,
    name: &str,
    applied: Option<&str>,
    previous: Option<&str>,
) -> Result<(), SystemProxyError> {
    if key.read_string(name)?.as_deref() == applied {
        match previous {
            Some(value) => key.write_string(name, value),
            None => key.delete_value(name),
        }
    } else {
        Ok(())
    }
}

struct UserInternetSettings {
    key: HKEY,
}

impl UserInternetSettings {
    fn open(user_sid: &str) -> Result<Self, SystemProxyError> {
        if !valid_sid(user_sid) {
            return Err(SystemProxyError::InvalidOwner);
        }
        let path = wide(&format!("{user_sid}\\{INTERNET_SETTINGS_PATH}"));
        let mut key: HKEY = ptr::null_mut();
        // SAFETY: path is a live null-terminated UTF-16 buffer and key is
        // writable. HKEY_USERS is a predefined process-wide handle.
        let status = unsafe {
            RegOpenKeyExW(
                HKEY_USERS,
                path.as_ptr(),
                0,
                KEY_QUERY_VALUE | KEY_SET_VALUE,
                &mut key,
            )
        };
        win32_status(status, "RegOpenKeyExW(user Internet Settings)")?;
        if key.is_null() {
            return Err(SystemProxyError::Registry(io::Error::other(
                "RegOpenKeyExW returned a null key",
            )));
        }
        let settings = Self { key };
        // Check the resolved object, not the requested name: a link anywhere
        // in the user-controlled parent chain can redirect a SYSTEM open.
        // All operations below retain this same handle, so changing a link
        // after verification cannot redirect a later read or write.
        verify_registry_key_path(user_sid, &registry_key_path(settings.key)?)?;
        Ok(settings)
    }

    fn read_dword(&self, name: &str) -> Result<Option<u32>, SystemProxyError> {
        let Some((kind, bytes)) = self.read_raw(name)? else {
            return Ok(None);
        };
        if kind != REG_DWORD || bytes.len() != size_of::<u32>() {
            return Err(SystemProxyError::UnexpectedRegistryType(name.to_owned()));
        }
        Ok(Some(u32::from_le_bytes(
            bytes.try_into().expect("validated DWORD length"),
        )))
    }

    fn read_string(&self, name: &str) -> Result<Option<String>, SystemProxyError> {
        let Some((kind, bytes)) = self.read_raw(name)? else {
            return Ok(None);
        };
        if !matches!(kind, REG_SZ | REG_EXPAND_SZ) || bytes.len() % 2 != 0 {
            return Err(SystemProxyError::UnexpectedRegistryType(name.to_owned()));
        }
        let mut units = bytes
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect::<Vec<_>>();
        while units.last() == Some(&0) {
            units.pop();
        }
        if units.contains(&0) {
            return Err(SystemProxyError::UnexpectedRegistryType(name.to_owned()));
        }
        String::from_utf16(&units)
            .map(Some)
            .map_err(|_| SystemProxyError::UnexpectedRegistryType(name.to_owned()))
    }

    fn read_raw(&self, name: &str) -> Result<Option<(u32, Vec<u8>)>, SystemProxyError> {
        let name = wide(name);
        let mut kind = 0_u32;
        let mut bytes = 0_u32;
        // SAFETY: name is null-terminated and size/type outputs are writable.
        let status = unsafe {
            RegQueryValueExW(
                self.key,
                name.as_ptr(),
                ptr::null(),
                &mut kind,
                ptr::null_mut(),
                &mut bytes,
            )
        };
        if status == ERROR_FILE_NOT_FOUND {
            return Ok(None);
        }
        win32_status(status, "RegQueryValueExW(size)")?;
        if bytes > MAX_REGISTRY_VALUE_BYTES {
            return Err(SystemProxyError::RegistryValueTooLarge(bytes));
        }
        let mut data = vec![0_u8; bytes as usize];
        // SAFETY: data has exactly the byte capacity requested by Windows.
        let status = unsafe {
            RegQueryValueExW(
                self.key,
                name.as_ptr(),
                ptr::null(),
                &mut kind,
                data.as_mut_ptr(),
                &mut bytes,
            )
        };
        win32_status(status, "RegQueryValueExW(data)")?;
        data.truncate(bytes as usize);
        Ok(Some((kind, data)))
    }

    fn write_dword(&self, name: &str, value: u32) -> Result<(), SystemProxyError> {
        let name = wide(name);
        // SAFETY: all pointers reference live fixed-size buffers.
        let status = unsafe {
            RegSetValueExW(
                self.key,
                name.as_ptr(),
                0,
                REG_DWORD,
                value.to_le_bytes().as_ptr(),
                size_of::<u32>() as u32,
            )
        };
        win32_status(status, "RegSetValueExW(DWORD)")
    }

    fn write_string(&self, name: &str, value: &str) -> Result<(), SystemProxyError> {
        if value.len() > MAX_TEXT_BYTES || value.contains('\0') {
            return Err(SystemProxyError::InvalidReceipt);
        }
        let name = wide(name);
        let value = wide(value);
        let byte_length = u32::try_from(value.len() * size_of::<u16>())
            .map_err(|_| SystemProxyError::RegistryValueTooLarge(u32::MAX))?;
        // SAFETY: value is a live null-terminated UTF-16 buffer and its byte
        // length includes the terminator.
        let bytes =
            unsafe { slice::from_raw_parts(value.as_ptr().cast::<u8>(), byte_length as usize) };
        // SAFETY: all pointers and lengths reference the live buffers above.
        let status = unsafe {
            RegSetValueExW(
                self.key,
                name.as_ptr(),
                0,
                REG_SZ,
                bytes.as_ptr(),
                byte_length,
            )
        };
        win32_status(status, "RegSetValueExW(REG_SZ)")
    }

    fn delete_value(&self, name: &str) -> Result<(), SystemProxyError> {
        let name = wide(name);
        // SAFETY: name is a live null-terminated UTF-16 buffer.
        let status = unsafe { RegDeleteValueW(self.key, name.as_ptr()) };
        if status == ERROR_FILE_NOT_FOUND {
            Ok(())
        } else {
            win32_status(status, "RegDeleteValueW")
        }
    }

    fn flush(&self) -> Result<(), SystemProxyError> {
        // SAFETY: key remains owned and live.
        win32_status(unsafe { RegFlushKey(self.key) }, "RegFlushKey")
    }
}

impl InternetSettings for UserInternetSettings {
    fn read_dword(&self, name: &str) -> Result<Option<u32>, SystemProxyError> {
        Self::read_dword(self, name)
    }

    fn read_string(&self, name: &str) -> Result<Option<String>, SystemProxyError> {
        Self::read_string(self, name)
    }

    fn write_dword(&self, name: &str, value: u32) -> Result<(), SystemProxyError> {
        Self::write_dword(self, name, value)
    }

    fn write_string(&self, name: &str, value: &str) -> Result<(), SystemProxyError> {
        Self::write_string(self, name, value)
    }

    fn delete_value(&self, name: &str) -> Result<(), SystemProxyError> {
        Self::delete_value(self, name)
    }

    fn flush(&self) -> Result<(), SystemProxyError> {
        Self::flush(self)
    }
}

fn verify_registry_key_path(user_sid: &str, actual: &str) -> Result<(), SystemProxyError> {
    if !valid_sid(user_sid) {
        return Err(SystemProxyError::InvalidOwner);
    }
    let expected = format!(r"\REGISTRY\USER\{user_sid}\{INTERNET_SETTINGS_PATH}");
    if !actual.eq_ignore_ascii_case(&expected) {
        // Do not include the resolved path in diagnostics: it can disclose
        // another user's SID or a deliberately supplied sensitive target.
        return Err(SystemProxyError::RegistryKeyIdentity);
    }
    Ok(())
}

fn registry_key_path(key: HKEY) -> Result<String, SystemProxyError> {
    type NtQueryKey = unsafe extern "system" fn(HKEY, i32, *mut c_void, u32, *mut u32) -> i32;
    // NtQueryKey is the documented user-mode spelling of ZwQueryKey.
    // KeyNameInformation (3) reports the resolved name for this live handle.
    // https://learn.microsoft.com/windows-hardware/drivers/ddi/wdm/nf-wdm-zwquerykey
    const KEY_NAME_INFORMATION: i32 = 3;
    let module_name = wide("ntdll.dll");
    // SAFETY: the name is terminated; ntdll is already loaded by Windows and
    // the borrowed module remains loaded for the process lifetime.
    let module = unsafe { GetModuleHandleW(module_name.as_ptr()) };
    if module.is_null() {
        return Err(SystemProxyError::RegistryKeyIdentity);
    }
    // SAFETY: the module is live and the export name is terminated.
    let address = unsafe { GetProcAddress(module, c"NtQueryKey".as_ptr().cast()) }
        .ok_or(SystemProxyError::RegistryKeyIdentity)?;
    // SAFETY: this exact ntdll export has the documented NtQueryKey system
    // ABI and signature above on both supported Windows architectures.
    let query =
        unsafe { std::mem::transmute::<unsafe extern "system" fn() -> isize, NtQueryKey>(address) };
    query_registry_key_path_with(|buffer, required| {
        let data = if buffer.is_empty() {
            ptr::null_mut()
        } else {
            buffer.as_mut_ptr().cast()
        };
        // SAFETY: key remains live, data is null or an aligned writable buffer
        // of buffer.len() bytes, and required is a writable size output.
        unsafe {
            query(
                key,
                KEY_NAME_INFORMATION,
                data,
                buffer.len() as u32,
                required,
            )
        }
    })
}

fn query_registry_key_path_with(
    mut query: impl FnMut(&mut [u8], &mut u32) -> i32,
) -> Result<String, SystemProxyError> {
    const STATUS_BUFFER_OVERFLOW: i32 = 0x8000_0005_u32 as i32;
    const STATUS_BUFFER_TOO_SMALL: i32 = 0xc000_0023_u32 as i32;
    let mut required = 0;
    let status = query(&mut [], &mut required);
    if !matches!(status, STATUS_BUFFER_OVERFLOW | STATUS_BUFFER_TOO_SMALL)
        || required < size_of::<u32>() as u32
        || required > MAX_REGISTRY_VALUE_BYTES
    {
        return Err(SystemProxyError::RegistryKeyIdentity);
    }
    let byte_length = required as usize;
    let mut words = vec![0_u32; byte_length.div_ceil(size_of::<u32>())];
    // SAFETY: the zeroed allocation is DWORD-aligned and has at least
    // byte_length bytes. It is accessed only through this slice until it ends.
    let buffer = unsafe { slice::from_raw_parts_mut(words.as_mut_ptr().cast::<u8>(), byte_length) };
    let status = query(buffer, &mut required);
    if status != 0 || required as usize > buffer.len() || required < size_of::<u32>() as u32 {
        return Err(SystemProxyError::RegistryKeyIdentity);
    }
    let buffer = &buffer[..required as usize];
    let name_length =
        u32::from_le_bytes(buffer[..4].try_into().expect("validated header")) as usize;
    if !name_length.is_multiple_of(2) || name_length > buffer.len() - 4 {
        return Err(SystemProxyError::RegistryKeyIdentity);
    }
    let name = buffer[4..4 + name_length]
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .collect::<Vec<_>>();
    String::from_utf16(&name).map_err(|_| SystemProxyError::RegistryKeyIdentity)
}

impl Drop for UserInternetSettings {
    fn drop(&mut self) {
        if !self.key.is_null() {
            // SAFETY: this object uniquely owns the opened registry key.
            unsafe {
                RegCloseKey(self.key);
            }
        }
    }
}

fn notify_settings_changed() -> Result<(), SystemProxyError> {
    // SAFETY: these two global notification options require null handles and
    // no data buffer.
    if unsafe {
        InternetSetOptionW(
            ptr::null(),
            INTERNET_OPTION_SETTINGS_CHANGED,
            ptr::null::<c_void>(),
            0,
        )
    } == 0
    {
        return Err(SystemProxyError::last_windows_error(
            "InternetSetOptionW(SETTINGS_CHANGED)",
        ));
    }
    // SAFETY: same contract as above.
    if unsafe {
        InternetSetOptionW(
            ptr::null(),
            INTERNET_OPTION_REFRESH,
            ptr::null::<c_void>(),
            0,
        )
    } == 0
    {
        return Err(SystemProxyError::last_windows_error(
            "InternetSetOptionW(REFRESH)",
        ));
    }
    Ok(())
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

fn win32_status(status: u32, operation: &'static str) -> Result<(), SystemProxyError> {
    if status == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(SystemProxyError::Windows {
            operation,
            code: status,
        })
    }
}

#[derive(Debug, Error)]
pub enum SystemProxyError {
    #[error("system-proxy owner metadata is invalid")]
    InvalidOwner,
    #[error("system-proxy receipt type is invalid")]
    WrongReceipt,
    #[error("system-proxy receipt is malformed")]
    InvalidReceipt,
    #[error("system proxy must be an http:// URI with a numeric socket address")]
    InvalidProxyUri,
    #[error("Windows system proxy may point only to a Loopback listener")]
    NonLoopbackProxy,
    #[error("system-proxy bypass list contains too many entries: {0}")]
    TooManyBypassHosts(usize),
    #[error("system-proxy bypass entry is invalid: {0}")]
    InvalidBypassHost(String),
    #[error("system-proxy bypass list exceeds the safety limit")]
    BypassListTooLong,
    #[error("another Usque operation already owns this user's system proxy")]
    AlreadyOwned,
    #[error("system-proxy settings changed before the write-ahead operation could apply")]
    SnapshotChanged,
    #[error("system-proxy ownership marker belongs to another operation")]
    OwnerChanged,
    #[error("system-proxy registry key identity could not be verified")]
    RegistryKeyIdentity,
    #[error("registry value has an unexpected type: {0}")]
    UnexpectedRegistryType(String),
    #[error("registry value exceeds the safety limit: {0} bytes")]
    RegistryValueTooLarge(u32),
    #[error("{operation} failed with Windows error {code}")]
    Windows { operation: &'static str, code: u32 },
    #[error("Windows system-proxy registry operation failed: {0}")]
    Registry(#[from] io::Error),
}

impl SystemProxyError {
    fn last_windows_error(operation: &'static str) -> Self {
        Self::Windows {
            operation,
            code: io::Error::last_os_error()
                .raw_os_error()
                .unwrap_or_default() as u32,
        }
    }
}

#[cfg(test)]
mod recovery_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_only_numeric_loopback_http_proxy_uris() {
        assert_eq!(
            normalize_proxy_uri("http://127.0.0.1:8080").expect("IPv4"),
            "127.0.0.1:8080"
        );
        assert_eq!(
            normalize_proxy_uri("http://[::1]:8080").expect("IPv6"),
            "[::1]:8080"
        );
        for rejected in [
            "https://127.0.0.1:8080",
            "http://0.0.0.0:8080",
            "http://192.168.1.2:8080",
            "http://localhost:8080",
            "http://127.0.0.1:8080/path",
        ] {
            assert!(normalize_proxy_uri(rejected).is_err(), "{rejected}");
        }
    }

    #[test]
    fn bypass_list_is_deduplicated_and_always_keeps_local_hosts_direct() {
        let normalized = normalize_bypass_hosts(&[
            "localhost".to_owned(),
            "LOCALHOST".to_owned(),
            "127.*".to_owned(),
        ])
        .expect("bypass");
        assert_eq!(normalized, "localhost;127.*;<local>");
        assert!(normalize_bypass_hosts(&["host;injected".to_owned()]).is_err());
    }
}
