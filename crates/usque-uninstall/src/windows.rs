use crate::{UninstallError, UninstallRequest};
use std::{
    os::windows::fs::OpenOptionsExt,
    path::{Path, PathBuf},
    process::Command,
    ptr,
};
use usque_platform::windows_authenticode::verify_same_signer;
use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_FILE_NOT_FOUND, ERROR_INVALID_PARAMETER, ERROR_PATH_NOT_FOUND,
    ERROR_SUCCESS, ERROR_SUCCESS_REBOOT_INITIATED, ERROR_SUCCESS_REBOOT_REQUIRED, GetLastError,
    HANDLE, WAIT_FAILED, WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use windows_sys::Win32::Globalization::{GetUserDefaultUILanguage, LCIDToLocaleName};
use windows_sys::Win32::Storage::FileSystem::FILE_SHARE_READ;
use windows_sys::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole};
use windows_sys::Win32::System::Registry::{
    HKEY, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_64KEY, REG_SZ, RegCloseKey, RegOpenKeyExW,
    RegQueryValueExW,
};
use windows_sys::Win32::System::SystemInformation::GetSystemDirectoryW;
use windows_sys::Win32::System::Threading::{
    OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW};
mod accessibility;
mod msi;
mod restart;
mod ui;
const PRODUCT_KEY: &str = r"Software\Usque";
const PRODUCT_VALUE: &str = "ProductCode";
const BUNDLE_PROVIDER_KEYS: [&str; 2] = ["Usque.Windows.x64-v2", "Usque.Windows.arm64"];
const DEPENDENCY_KEY_PREFIX: &str = r"Software\Classes\Installer\Dependencies";
const BUNDLE_UNINSTALL_KEY_PREFIX: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall";
const BUNDLE_PROVIDER_VALUE: &str = "BundleProviderKey";
const BUNDLE_CACHE_PATH_VALUE: &str = "BundleCachePath";
const PARENT_EXIT_TIMEOUT_MS: u32 = 60_000;
struct RegistryKey(HKEY);

impl Drop for RegistryKey {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: this wrapper owns the key returned by RegOpenKeyExW.
            unsafe {
                RegCloseKey(self.0);
            }
        }
    }
}

struct ProcessHandle(HANDLE);

impl Drop for ProcessHandle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: this wrapper owns the process handle returned by Win32.
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
}

pub(crate) fn attach_parent_console() {
    // SAFETY: AttachConsole only associates this process with an existing
    // parent console; failure means there is no console to attach.
    unsafe {
        AttachConsole(ATTACH_PARENT_PROCESS);
    }
}

pub(crate) fn show_error_message(_error: &UninstallError) {
    // Pre-window failures can contain ProductCodes, filesystem paths, or raw
    // signature/registry details. Keep those out of the graphical prompt.
    let locale = crate::l10n::setup_locale(&ui_locale_name());
    let text = wide(&format!(
        "{}\r\n\r\n{}",
        crate::l10n::setup_text(locale, "uninstall_launch_failed"),
        crate::l10n::setup_text(locale, "error_source_hint")
    ));
    let caption = wide("Usque");
    // SAFETY: both buffers are null-terminated wide strings that outlive the call.
    unsafe {
        MessageBoxW(
            ptr::null_mut(),
            text.as_ptr(),
            caption.as_ptr(),
            MB_OK | MB_ICONERROR,
        );
    }
}

pub(crate) fn read_installed_product_code() -> Result<String, UninstallError> {
    let key = open_machine_key(PRODUCT_KEY)?.ok_or(UninstallError::MissingProductCode)?;
    let product_code = read_registry_string(&key, Some(PRODUCT_VALUE))?
        .filter(|value| !value.is_empty())
        .ok_or(UninstallError::MissingProductCode)?;
    crate::normalize_product_code(&product_code)
}

pub(crate) fn run_interactive(
    product_code: Option<String>,
    wait_for_pid: Option<u32>,
) -> Result<i32, UninstallError> {
    if let Some(parent_pid) = wait_for_pid {
        wait_for_process(parent_pid)?;
    }
    if let Some(code) = relaunch_from_temp_if_needed(product_code.as_deref())? {
        return Ok(code);
    }
    let product_code = crate::resolve_product_code(product_code, read_installed_product_code)?;
    ui::run(Some(product_code), None, None, None)
}

pub(crate) fn run_preview(
    scenario: crate::state::Preview,
    locale: Option<&str>,
    theme: Option<crate::state::PreviewTheme>,
) -> Result<i32, UninstallError> {
    // Deliberately bypass every installed-product and staging operation.
    ui::run(None, Some(scenario), locale, theme)
}

pub(crate) fn run_quiet(
    product_code: Option<String>,
    remove_user_data: bool,
    wait_for_pid: Option<u32>,
) -> Result<i32, UninstallError> {
    // The registered quiet launcher lives in a system PowerShell process. It
    // stages and verifies this copy, lets the installed helper exit, then waits
    // for this worker's real exit code. Never detach a quiet caller or keep the
    // installed image mapped while Windows Installer tries to remove it.
    let current = std::env::current_exe().map_err(|error| {
        UninstallError::Detail(format!("failed to locate this helper: {error}"))
    })?;
    if !crate::is_temp_relaunch_path(&current, &std::env::temp_dir()) {
        return Err(UninstallError::InvalidExecutionContext);
    }
    if let Some(parent_pid) = wait_for_pid {
        wait_for_process(parent_pid)?;
    }
    let product_code = crate::resolve_product_code(product_code, read_installed_product_code)?;
    execute_uninstall(
        UninstallRequest {
            product_code,
            remove_user_data,
        },
        true,
    )
}

pub(crate) fn prepare_quiet_copy(pid: u32, verify_only: bool) -> Result<i32, UninstallError> {
    let current = std::env::current_exe().map_err(|error| {
        UninstallError::Detail(format!("failed to locate this helper: {error}"))
    })?;
    let destination = crate::temp_relaunch_path(&std::env::temp_dir(), pid);
    if verify_only {
        // The launcher holds a deny-write/deny-delete handle through this
        // verification and worker execution, closing the stage-to-launch race.
        verify_same_signer(&current, &destination).map_err(|error| {
            UninstallError::Detail(format!(
                "quiet uninstall helper verification failed: {error}"
            ))
        })?;
        return Ok(0);
    }
    let directory = destination
        .parent()
        .ok_or(UninstallError::InvalidExecutionContext)?;
    // Refuse an existing directory rather than trusting a stale or precreated
    // PID path. Only remove files that this staging operation created.
    std::fs::create_dir(directory).map_err(|error| {
        UninstallError::Detail(format!(
            "failed to create the quiet helper directory: {error}"
        ))
    })?;
    let mut target = match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&destination)
    {
        Ok(target) => target,
        Err(error) => {
            let _ = std::fs::remove_dir(directory);
            return Err(UninstallError::Detail(format!(
                "failed to create the quiet helper: {error}"
            )));
        }
    };
    let result = std::fs::File::open(&current)
        .and_then(|mut source| std::io::copy(&mut source, &mut target));
    drop(target);
    if let Err(error) = result {
        // A partially copied helper is never executed. Do not recursively
        // remove a directory that could contain a file we did not create.
        let _ = std::fs::remove_file(&destination);
        let _ = std::fs::remove_dir(directory);
        return Err(UninstallError::Detail(format!(
            "failed to stage the quiet helper: {error}"
        )));
    }
    if let Err(error) = verify_same_signer(&current, &destination) {
        let _ = std::fs::remove_file(&destination);
        let _ = std::fs::remove_dir(directory);
        return Err(UninstallError::Detail(format!(
            "quiet uninstall helper verification failed: {error}"
        )));
    }
    Ok(0)
}

fn execute_uninstall(request: UninstallRequest, quiet: bool) -> Result<i32, UninstallError> {
    let current = std::env::current_exe().map_err(|error| {
        UninstallError::Detail(format!("failed to locate this helper: {error}"))
    })?;
    if !crate::is_temp_relaunch_path(&current, &std::env::temp_dir()) {
        return Err(UninstallError::InvalidExecutionContext);
    }
    let bundle = find_registered_bundle(&current)?;
    let msi_code = run_msiexec(&request, quiet)?;
    if !successful_installer_exit(msi_code) {
        return Ok(msi_code);
    }

    let bundle_code = if let Some(bundle) = bundle {
        run_bundle_cleanup(&bundle)?
    } else {
        0
    };
    if !successful_installer_exit(bundle_code) {
        return Err(UninstallError::Detail(format!(
            "the hidden installer bundle cleanup failed with exit code {bundle_code}"
        )));
    }

    Ok(combine_success_codes(msi_code, bundle_code))
}

fn open_machine_key(path: &str) -> Result<Option<RegistryKey>, UninstallError> {
    let mut key = ptr::null_mut();
    let subkey = wide(path);
    // SAFETY: subkey is null-terminated and key points to a writable HKEY slot.
    let status = unsafe {
        RegOpenKeyExW(
            HKEY_LOCAL_MACHINE,
            subkey.as_ptr(),
            0,
            KEY_READ | KEY_WOW64_64KEY,
            &mut key,
        )
    };
    if status == ERROR_FILE_NOT_FOUND || status == ERROR_PATH_NOT_FOUND {
        return Ok(None);
    }
    if status != ERROR_SUCCESS {
        return Err(UninstallError::Detail(format!(
            "failed to open HKLM\\{path} ({status})"
        )));
    }
    Ok(Some(RegistryKey(key)))
}

fn read_registry_string(
    key: &RegistryKey,
    name: Option<&str>,
) -> Result<Option<String>, UninstallError> {
    let name = name.map(wide);
    let name_pointer = name.as_ref().map_or(ptr::null(), |value| value.as_ptr());
    let mut data_type = 0_u32;
    let mut byte_len = 0_u32;
    // SAFETY: name_pointer is null for the default value or points to a live,
    // null-terminated buffer. The size query may pass a null data pointer.
    let status = unsafe {
        RegQueryValueExW(
            key.0,
            name_pointer,
            ptr::null_mut(),
            &mut data_type,
            ptr::null_mut(),
            &mut byte_len,
        )
    };
    if status == ERROR_FILE_NOT_FOUND || status == ERROR_PATH_NOT_FOUND {
        return Ok(None);
    }
    if status != ERROR_SUCCESS {
        return Err(UninstallError::Detail(format!(
            "failed to query registry string ({status})"
        )));
    }
    if data_type != REG_SZ || byte_len < 2 || !byte_len.is_multiple_of(2) {
        return Err(UninstallError::Detail(format!(
            "registry value is not a valid nonempty string (type {data_type}, size {byte_len})"
        )));
    }
    let unit_count = (byte_len as usize).div_ceil(2);
    let mut buffer = vec![0_u16; unit_count];
    let mut actual_len = byte_len;
    // SAFETY: buffer is writable for actual_len bytes reported by the registry.
    let status = unsafe {
        RegQueryValueExW(
            key.0,
            name_pointer,
            ptr::null_mut(),
            &mut data_type,
            buffer.as_mut_ptr().cast(),
            &mut actual_len,
        )
    };
    if status != ERROR_SUCCESS {
        return Err(UninstallError::Detail(format!(
            "failed to read registry string ({status})"
        )));
    }
    if actual_len > byte_len || actual_len < 2 || !actual_len.is_multiple_of(2) {
        return Err(UninstallError::Detail(
            "registry string changed to an invalid size while it was read".to_owned(),
        ));
    }
    let units = actual_len as usize / 2;
    let wide = &buffer[..units];
    if wide.last() != Some(&0) || wide[..wide.len() - 1].contains(&0) {
        return Err(UninstallError::Detail(
            "registry string has invalid null termination".to_owned(),
        ));
    }
    let text = String::from_utf16(&wide[..wide.len() - 1])
        .map_err(|_| UninstallError::Detail("registry string is not valid UTF-16".to_owned()))?;
    Ok(Some(text))
}

fn relaunch_from_temp_if_needed(product_code: Option<&str>) -> Result<Option<i32>, UninstallError> {
    let current = std::env::current_exe().map_err(|error| {
        UninstallError::Detail(format!("failed to locate this helper: {error}"))
    })?;
    let temp = std::env::temp_dir();
    if crate::is_temp_relaunch_path(&current, &temp) {
        return Ok(None);
    }
    let destination = crate::temp_relaunch_path(&temp, std::process::id());
    if let Some(parent) = destination.parent() {
        std::fs::create_dir_all(parent).map_err(|error| {
            UninstallError::Detail(format!(
                "failed to create a temporary helper directory: {error}"
            ))
        })?;
    }
    std::fs::copy(&current, &destination).map_err(|error| {
        UninstallError::Detail(format!(
            "failed to copy the helper to a temporary directory: {error}"
        ))
    })?;
    let executable_lock = lock_executable(&destination)?;
    verify_same_signer(&current, &destination).map_err(|error| {
        UninstallError::Detail(format!(
            "temporary uninstall helper verification failed: {error}"
        ))
    })?;

    let mut command = Command::new(&destination);
    if let Some(product_code) = product_code {
        command.arg("--product-code").arg(product_code);
    }
    command
        .arg("--wait-for-pid")
        .arg(std::process::id().to_string())
        .spawn()
        .map_err(|error| {
            UninstallError::Detail(format!("failed to start the temporary helper: {error}"))
        })?;
    drop(executable_lock);
    Ok(Some(0))
}

fn wait_for_process(process_id: u32) -> Result<(), UninstallError> {
    // SAFETY: this requests synchronization access only and does not inherit the handle.
    let handle = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, process_id) };
    if handle.is_null() {
        // SAFETY: read immediately after the failed Win32 call above.
        let error = unsafe { GetLastError() };
        if error == ERROR_INVALID_PARAMETER {
            return Ok(());
        }
        return Err(UninstallError::Detail(format!(
            "failed to wait for the installed uninstall helper ({error})"
        )));
    }
    let process = ProcessHandle(handle);
    // SAFETY: process owns a live synchronization handle.
    match unsafe { WaitForSingleObject(process.0, PARENT_EXIT_TIMEOUT_MS) } {
        WAIT_OBJECT_0 => Ok(()),
        WAIT_TIMEOUT => Err(UninstallError::Detail(
            "the installed uninstall helper did not exit within 60 seconds".to_owned(),
        )),
        WAIT_FAILED => Err(last_error(
            "waiting for the installed uninstall helper failed",
        )),
        result => Err(UninstallError::Detail(format!(
            "waiting for the installed uninstall helper returned {result}"
        ))),
    }
}

fn run_msiexec(request: &UninstallRequest, quiet: bool) -> Result<i32, UninstallError> {
    let msiexec = system_msiexec_path()?;
    let status = Command::new(msiexec)
        .args(request.arguments(quiet))
        .status()
        .map_err(|error| {
            UninstallError::Detail(format!("failed to start Windows Installer: {error}"))
        })?;
    status.code().ok_or_else(|| {
        UninstallError::Detail("Windows Installer exited without a status code".to_owned())
    })
}

fn system_msiexec_path() -> Result<PathBuf, UninstallError> {
    let mut buffer = vec![0_u16; 32_768];
    // SAFETY: buffer is writable for exactly the capacity passed to Kernel32.
    let length = unsafe { GetSystemDirectoryW(buffer.as_mut_ptr(), buffer.len() as u32) };
    if length == 0 || length as usize >= buffer.len() {
        return Err(last_error("failed to locate the Windows system directory"));
    }
    buffer.truncate(length as usize);
    let directory = PathBuf::from(String::from_utf16(&buffer).map_err(|_| {
        UninstallError::Detail("the Windows system directory is not valid UTF-16".to_owned())
    })?);
    let directory = directory.canonicalize().map_err(|error| {
        UninstallError::Detail(format!(
            "failed to resolve the Windows system directory: {error}"
        ))
    })?;
    let msiexec = directory
        .join("msiexec.exe")
        .canonicalize()
        .map_err(|error| {
            UninstallError::Detail(format!("failed to resolve system msiexec.exe: {error}"))
        })?;
    if !msiexec.is_file()
        || msiexec.parent() != Some(directory.as_path())
        || !msiexec
            .file_name()
            .is_some_and(|name| name.to_string_lossy().eq_ignore_ascii_case("msiexec.exe"))
    {
        return Err(UninstallError::Detail(
            "Windows returned an invalid system msiexec path".to_owned(),
        ));
    }
    Ok(msiexec)
}

fn find_registered_bundle(reference_helper: &Path) -> Result<Option<PathBuf>, UninstallError> {
    let mut found = None;
    for provider_key in BUNDLE_PROVIDER_KEYS {
        let dependency_path = format!(r"{DEPENDENCY_KEY_PREFIX}\{provider_key}");
        let Some(dependency) = open_machine_key(&dependency_path)? else {
            continue;
        };
        let bundle_id = read_registry_string(&dependency, None)?
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                UninstallError::Detail(format!(
                    "the Burn dependency provider {provider_key} has no bundle id"
                ))
            })?;
        let bundle_id = crate::normalize_product_code(&bundle_id).map_err(|_| {
            UninstallError::Detail(format!(
                "the Burn dependency provider {provider_key} has an invalid bundle id"
            ))
        })?;
        let registration_path = format!(r"{BUNDLE_UNINSTALL_KEY_PREFIX}\{bundle_id}");
        let registration = open_machine_key(&registration_path)?.ok_or_else(|| {
            UninstallError::Detail(format!(
                "the Burn bundle registration {bundle_id} is missing"
            ))
        })?;
        let registered_provider = read_registry_string(&registration, Some(BUNDLE_PROVIDER_VALUE))?
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                UninstallError::Detail(format!(
                    "the Burn bundle registration {bundle_id} has no provider key"
                ))
            })?;
        if !registered_provider.eq_ignore_ascii_case(provider_key) {
            return Err(UninstallError::Detail(format!(
                "the Burn bundle registration {bundle_id} belongs to an unexpected provider"
            )));
        }
        let cache_path = read_registry_string(&registration, Some(BUNDLE_CACHE_PATH_VALUE))?
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                UninstallError::Detail(format!(
                    "the Burn bundle registration {bundle_id} has no cache path"
                ))
            })?;
        let cache_path = validate_bundle_cache_path(&cache_path, &bundle_id)?;
        verify_same_signer(reference_helper, &cache_path).map_err(|error| {
            UninstallError::Detail(format!(
                "the cached installer bundle does not match the Usque signer: {error}"
            ))
        })?;
        if found.replace(cache_path).is_some() {
            return Err(UninstallError::Detail(
                "multiple Usque installer bundles are registered".to_owned(),
            ));
        }
    }
    Ok(found)
}

fn validate_bundle_cache_path(value: &str, bundle_id: &str) -> Result<PathBuf, UninstallError> {
    let path = PathBuf::from(value);
    if !path.is_absolute() {
        return Err(UninstallError::Detail(
            "the cached installer bundle path is not absolute".to_owned(),
        ));
    }
    let path = path.canonicalize().map_err(|error| {
        UninstallError::Detail(format!(
            "failed to resolve the cached installer bundle: {error}"
        ))
    })?;
    let bundle_directory = path.parent().ok_or_else(|| {
        UninstallError::Detail("the cached installer bundle has no parent directory".to_owned())
    })?;
    if !path.is_file()
        || !path
            .extension()
            .is_some_and(|extension| extension.to_string_lossy().eq_ignore_ascii_case("exe"))
        || !bundle_directory
            .file_name()
            .is_some_and(|name| name.to_string_lossy().eq_ignore_ascii_case(bundle_id))
    {
        return Err(UninstallError::Detail(
            "the cached installer bundle path does not match its Burn bundle id".to_owned(),
        ));
    }
    Ok(path)
}

fn run_bundle_cleanup(bundle: &Path) -> Result<i32, UninstallError> {
    let executable_lock = lock_executable(bundle)?;
    let current = std::env::current_exe().map_err(|error| {
        UninstallError::Detail(format!("failed to locate this helper: {error}"))
    })?;
    verify_same_signer(&current, bundle).map_err(|error| {
        UninstallError::Detail(format!("cached installer verification failed: {error}"))
    })?;
    let mut child = Command::new(bundle)
        .args(["/uninstall", "/quiet", "/norestart"])
        .spawn()
        .map_err(|error| {
            UninstallError::Detail(format!("failed to start installer bundle cleanup: {error}"))
        })?;
    // CreateProcess has loaded the verified image. Release our extra lock so
    // Burn can remove its cache after exit; the mapped image protects itself.
    drop(executable_lock);
    let status = child.wait().map_err(|error| {
        UninstallError::Detail(format!(
            "failed to wait for installer bundle cleanup: {error}"
        ))
    })?;
    status.code().ok_or_else(|| {
        UninstallError::Detail("installer bundle cleanup exited without a status code".to_owned())
    })
}

fn lock_executable(path: &Path) -> Result<std::fs::File, UninstallError> {
    std::fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ)
        .open(path)
        .map_err(|error| {
            UninstallError::Detail(format!("failed to lock the verified executable: {error}"))
        })
}

fn successful_installer_exit(code: i32) -> bool {
    matches!(
        code as u32,
        ERROR_SUCCESS | ERROR_SUCCESS_REBOOT_REQUIRED | ERROR_SUCCESS_REBOOT_INITIATED
    )
}

fn combine_success_codes(first: i32, second: i32) -> i32 {
    if [first, second]
        .into_iter()
        .any(|code| code as u32 == ERROR_SUCCESS_REBOOT_INITIATED)
    {
        ERROR_SUCCESS_REBOOT_INITIATED as i32
    } else if [first, second]
        .into_iter()
        .any(|code| code as u32 == ERROR_SUCCESS_REBOOT_REQUIRED)
    {
        ERROR_SUCCESS_REBOOT_REQUIRED as i32
    } else {
        ERROR_SUCCESS as i32
    }
}

fn ui_locale_name() -> String {
    let mut buffer = [0u16; 85];
    // SAFETY: this call has no pointer arguments.
    let language = unsafe { GetUserDefaultUILanguage() };
    // SAFETY: this receives a writable LOCALE_NAME_MAX_LENGTH buffer and a
    // valid UI-language LCID.
    let count = unsafe {
        LCIDToLocaleName(
            u32::from(language),
            buffer.as_mut_ptr(),
            buffer.len() as i32,
            0,
        )
    };
    if count <= 1 {
        return "en".to_owned();
    }
    String::from_utf16_lossy(&buffer[..count as usize - 1])
}

fn last_error(operation: &str) -> UninstallError {
    // SAFETY: called immediately after a failing Win32 call.
    let code = unsafe { GetLastError() };
    UninstallError::Detail(format!("{operation} ({code})"))
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    #[test]
    fn quiet_mode_refuses_to_detach_from_an_installed_path() {
        // The unit-test binary is not the staged helper. This must fail before
        // registry resolution, spawning a copy, or any Windows Installer call.
        assert!(matches!(
            run_quiet(None, false, None),
            Err(UninstallError::InvalidExecutionContext)
        ));
    }

    #[test]
    fn installer_success_codes_preserve_reboot_requirements() {
        assert!(successful_installer_exit(0));
        assert!(successful_installer_exit(1641));
        assert!(successful_installer_exit(3010));
        assert!(!successful_installer_exit(1602));
        assert_eq!(combine_success_codes(0, 0), 0);
        assert_eq!(combine_success_codes(3010, 0), 3010);
        assert_eq!(combine_success_codes(0, 1641), 1641);
    }

    #[test]
    fn executable_verification_lock_prevents_replacement_until_launch() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let fixture = std::env::temp_dir().join(format!(
            "UsqueExecutableLockTest-{}-{unique}.exe",
            std::process::id()
        ));
        std::fs::write(&fixture, b"inert fixture").expect("write fixture");
        let lock = lock_executable(&fixture).expect("read-only image lock");
        assert!(
            std::fs::OpenOptions::new()
                .write(true)
                .open(&fixture)
                .is_err()
        );
        assert!(std::fs::remove_file(&fixture).is_err());
        drop(lock);
        std::fs::remove_file(&fixture).expect("remove unlocked fixture");
    }

    #[test]
    fn bundle_cleanup_requires_the_registered_bundle_id_directory() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "UsqueBundleCachePathTest-{}-{unique}",
            std::process::id()
        ));
        let bundle_id = "{11111111-1111-1111-1111-111111111111}";
        let bundle_directory = root.join("Redirected Burn Cache").join(bundle_id);
        std::fs::create_dir_all(&bundle_directory).expect("cache fixture");
        let bundle = bundle_directory.join("renamed installer.exe");
        std::fs::write(&bundle, b"fixture").expect("bundle fixture");

        assert_eq!(
            validate_bundle_cache_path(&bundle.to_string_lossy(), bundle_id).expect("valid path"),
            bundle.canonicalize().expect("canonical bundle")
        );
        assert!(
            validate_bundle_cache_path(
                &bundle.to_string_lossy(),
                "{22222222-2222-2222-2222-222222222222}"
            )
            .is_err()
        );

        std::fs::remove_dir_all(&root).expect("remove cache fixture");
    }
}
