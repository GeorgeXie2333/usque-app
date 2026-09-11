use std::{
    path::{Path, PathBuf},
    process::Command,
    ptr,
};

use usque_platform::windows_authenticode::verify_same_signer;
use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_FILE_NOT_FOUND, ERROR_INVALID_PARAMETER, ERROR_PATH_NOT_FOUND,
    ERROR_SUCCESS, ERROR_SUCCESS_REBOOT_INITIATED, ERROR_SUCCESS_REBOOT_REQUIRED, GetLastError,
    HANDLE, HWND, LPARAM, LRESULT, WAIT_FAILED, WAIT_OBJECT_0, WAIT_TIMEOUT, WPARAM,
};
use windows_sys::Win32::Globalization::{GetUserDefaultUILanguage, LCIDToLocaleName};
use windows_sys::Win32::Graphics::Gdi::{
    COLOR_WINDOW, DEFAULT_GUI_FONT, GetStockObject, UpdateWindow,
};
use windows_sys::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Registry::{
    HKEY, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_64KEY, REG_SZ, RegCloseKey, RegOpenKeyExW,
    RegQueryValueExW,
};
use windows_sys::Win32::System::SystemInformation::GetSystemDirectoryW;
use windows_sys::Win32::System::Threading::{
    OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject,
};
use windows_sys::Win32::UI::Controls::{BST_CHECKED, IsDlgButtonChecked};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::SetFocus;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    BS_AUTOCHECKBOX, BS_DEFPUSHBUTTON, BS_PUSHBUTTON, CREATESTRUCTW, CW_USEDEFAULT,
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GWLP_USERDATA, GetDlgItem,
    GetMessageW, GetSystemMetrics, GetWindowLongPtrW, IDC_ARROW, IDCANCEL, IDOK, IsDialogMessageW,
    LoadCursorW, MB_ICONERROR, MB_OK, MSG, MessageBoxW, PostQuitMessage, RegisterClassExW,
    SM_CXSCREEN, SM_CYSCREEN, SW_SHOW, SWP_NOZORDER, SendMessageW, SetWindowLongPtrW, SetWindowPos,
    ShowWindow, TranslateMessage, UnregisterClassW, WM_CLOSE, WM_COMMAND, WM_CREATE, WM_DESTROY,
    WM_SETFONT, WNDCLASSEXW, WS_CAPTION, WS_CHILD, WS_OVERLAPPED, WS_SYSMENU, WS_TABSTOP,
    WS_VISIBLE,
};

use crate::l10n::{self, UninstallCopy};
use crate::{ERROR_INSTALL_USEREXIT, UninstallError, UninstallRequest};

const PRODUCT_KEY: &str = r"Software\Usque";
const PRODUCT_VALUE: &str = "ProductCode";
const BUNDLE_PROVIDER_KEYS: [&str; 2] = ["Usque.Windows.x64-v2", "Usque.Windows.arm64"];
const DEPENDENCY_KEY_PREFIX: &str = r"Software\Classes\Installer\Dependencies";
const BUNDLE_UNINSTALL_KEY_PREFIX: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall";
const BUNDLE_PROVIDER_VALUE: &str = "BundleProviderKey";
const BUNDLE_CACHE_PATH_VALUE: &str = "BundleCachePath";
const PARENT_EXIT_TIMEOUT_MS: u32 = 60_000;
const CLASS_NAME: &str = "Usque.UninstallConfirm";
const IDC_BODY: i32 = 1001;
const IDC_CHECK: i32 = 1002;
const IDC_WARNING: i32 = 1003;
const IDC_UNINSTALL: i32 = 1004;
const IDC_CANCEL: i32 = 1005;

#[derive(Clone, Copy)]
enum Confirm {
    Cancel,
    Uninstall { remove_user_data: bool },
}

struct DialogState {
    outcome: Confirm,
    copy: UninstallCopy,
}

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

pub(crate) fn show_error_message(error: &UninstallError) {
    let text = wide(&error.to_string());
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
    if let Some(code) = relaunch_from_temp_if_needed(product_code.as_deref())? {
        return Ok(code);
    }
    if let Some(parent_pid) = wait_for_pid {
        wait_for_process(parent_pid)?;
    }
    let product_code = crate::resolve_product_code(product_code, read_installed_product_code)?;
    match confirm_uninstall()? {
        Confirm::Cancel => Ok(ERROR_INSTALL_USEREXIT),
        Confirm::Uninstall { remove_user_data } => execute_uninstall(
            UninstallRequest {
                product_code,
                remove_user_data,
            },
            false,
        ),
    }
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
    let status = Command::new(bundle)
        .args(["/uninstall", "/quiet", "/norestart"])
        .status()
        .map_err(|error| {
            UninstallError::Detail(format!("failed to start installer bundle cleanup: {error}"))
        })?;
    status.code().ok_or_else(|| {
        UninstallError::Detail("installer bundle cleanup exited without a status code".to_owned())
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

fn confirm_uninstall() -> Result<Confirm, UninstallError> {
    let class = wide(CLASS_NAME);
    let instance = {
        // SAFETY: a null module name returns the handle of this executable.
        unsafe { GetModuleHandleW(ptr::null()) }
    };
    if instance.is_null() {
        return Err(last_error("failed to get the helper module handle"));
    }

    let cursor = {
        // SAFETY: IDC_ARROW is a predefined cursor identifier.
        unsafe { LoadCursorW(ptr::null_mut(), IDC_ARROW) }
    };
    let class_info = WNDCLASSEXW {
        cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
        style: 0,
        lpfnWndProc: Some(dialog_proc),
        cbClsExtra: 0,
        cbWndExtra: 0,
        hInstance: instance,
        hIcon: ptr::null_mut(),
        hCursor: cursor,
        hbrBackground: (COLOR_WINDOW + 1) as _,
        lpszMenuName: ptr::null(),
        lpszClassName: class.as_ptr(),
        hIconSm: ptr::null_mut(),
    };
    // SAFETY: class_info points at a complete WNDCLASSEXW that outlives registration.
    let atom = unsafe { RegisterClassExW(&class_info) };
    if atom == 0 {
        return Err(last_error("failed to register the uninstall dialog class"));
    }

    let copy = l10n::copy_for_locale(&ui_locale_name());
    let mut state = DialogState {
        outcome: Confirm::Cancel,
        copy,
    };
    let title = wide(copy.title);
    let hwnd = {
        // SAFETY: the class was registered above; lpParam borrows state for WM_CREATE.
        unsafe {
            CreateWindowExW(
                0,
                class.as_ptr(),
                title.as_ptr(),
                WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_VISIBLE,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                520,
                280,
                ptr::null_mut(),
                ptr::null_mut(),
                instance,
                ptr::from_mut(&mut state).cast(),
            )
        }
    };
    if hwnd.is_null() {
        // SAFETY: the class was registered by this function.
        unsafe {
            UnregisterClassW(class.as_ptr(), instance);
        }
        return Err(last_error("failed to create the uninstall dialog"));
    }

    center_window(hwnd);
    // SAFETY: hwnd is a window created by this function.
    unsafe {
        ShowWindow(hwnd, SW_SHOW);
        UpdateWindow(hwnd);
    }

    let mut message = MSG::default();
    loop {
        // SAFETY: message is a writable MSG used only for this pump.
        let result = unsafe { GetMessageW(&mut message, ptr::null_mut(), 0, 0) };
        if result == 0 || result == -1 {
            break;
        }
        // SAFETY: hwnd is still valid until WM_DESTROY posts the quit message.
        if unsafe { IsDialogMessageW(hwnd, &message) } == 0 {
            // SAFETY: message was filled by GetMessageW on this thread.
            unsafe {
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
    }

    // SAFETY: no windows remain that use this class.
    unsafe {
        UnregisterClassW(class.as_ptr(), instance);
    }
    Ok(state.outcome)
}

extern "system" fn dialog_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_CREATE => {
            store_state_on_create(hwnd, lparam);
            if create_children(hwnd).is_err() {
                // SAFETY: this is the window currently being created.
                unsafe {
                    DestroyWindow(hwnd);
                }
            }
            0
        }
        WM_COMMAND => {
            let control_id = (wparam & 0xffff) as i32;
            if control_id == IDC_UNINSTALL || control_id == IDOK {
                finish_dialog(
                    hwnd,
                    Confirm::Uninstall {
                        remove_user_data: checkbox_checked(hwnd),
                    },
                );
            } else if control_id == IDC_CANCEL || control_id == IDCANCEL {
                finish_dialog(hwnd, Confirm::Cancel);
            }
            0
        }
        WM_CLOSE => {
            finish_dialog(hwnd, Confirm::Cancel);
            0
        }
        WM_DESTROY => {
            // SAFETY: posted from the dialog thread to end the local message pump.
            unsafe {
                PostQuitMessage(0);
            }
            0
        }
        _ => {
            // SAFETY: default processing for an application-owned top-level window.
            unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
        }
    }
}

fn create_children(hwnd: HWND) -> Result<(), UninstallError> {
    let copy = dialog_state(hwnd)
        .map(|state| state.copy)
        .unwrap_or(l10n::EN);
    let instance = {
        // SAFETY: a null module name returns the handle of this executable.
        unsafe { GetModuleHandleW(ptr::null()) }
    };
    let font = {
        // SAFETY: DEFAULT_GUI_FONT is a predefined stock object.
        unsafe { GetStockObject(DEFAULT_GUI_FONT) }
    };

    create_control(
        hwnd,
        instance,
        ControlSpec {
            class_name: "STATIC",
            text: copy.body,
            style: WS_CHILD | WS_VISIBLE,
            id: IDC_BODY,
            x: 20,
            y: 16,
            width: 460,
            height: 40,
        },
    )?;
    create_control(
        hwnd,
        instance,
        ControlSpec {
            class_name: "BUTTON",
            text: copy.delete_data,
            style: WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_AUTOCHECKBOX as u32,
            id: IDC_CHECK,
            x: 20,
            y: 64,
            width: 460,
            height: 40,
        },
    )?;
    create_control(
        hwnd,
        instance,
        ControlSpec {
            class_name: "STATIC",
            text: copy.warning,
            style: WS_CHILD | WS_VISIBLE,
            id: IDC_WARNING,
            x: 40,
            y: 108,
            width: 440,
            height: 48,
        },
    )?;
    create_control(
        hwnd,
        instance,
        ControlSpec {
            class_name: "BUTTON",
            text: copy.uninstall,
            style: WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_PUSHBUTTON as u32,
            id: IDC_UNINSTALL,
            x: 236,
            y: 180,
            width: 110,
            height: 28,
        },
    )?;
    let cancel = create_control(
        hwnd,
        instance,
        ControlSpec {
            class_name: "BUTTON",
            text: copy.cancel,
            style: WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_DEFPUSHBUTTON as u32,
            id: IDC_CANCEL,
            x: 356,
            y: 180,
            width: 110,
            height: 28,
        },
    )?;

    if !font.is_null() {
        for child in [IDC_BODY, IDC_CHECK, IDC_WARNING, IDC_UNINSTALL, IDC_CANCEL] {
            if let Some(handle) = child_from_id(hwnd, child) {
                // SAFETY: handle is a child of hwnd and font is a stock object.
                unsafe {
                    SendMessageW(handle, WM_SETFONT, font as WPARAM, 1);
                }
            }
        }
    }
    // SAFETY: cancel is a child button created above.
    unsafe {
        SetFocus(cancel);
    }
    let _ = instance;
    Ok(())
}

struct ControlSpec {
    class_name: &'static str,
    text: &'static str,
    style: u32,
    id: i32,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
}

fn create_control(
    parent: HWND,
    instance: windows_sys::Win32::Foundation::HINSTANCE,
    spec: ControlSpec,
) -> Result<HWND, UninstallError> {
    let class = wide(spec.class_name);
    let caption = wide(spec.text);
    // SAFETY: class and caption are null-terminated; parent is a live window.
    let handle = unsafe {
        CreateWindowExW(
            0,
            class.as_ptr(),
            caption.as_ptr(),
            spec.style,
            spec.x,
            spec.y,
            spec.width,
            spec.height,
            parent,
            spec.id as isize as _,
            instance,
            ptr::null(),
        )
    };
    if handle.is_null() {
        Err(last_error("failed to create an uninstall dialog control"))
    } else {
        Ok(handle)
    }
}

fn child_from_id(parent: HWND, id: i32) -> Option<HWND> {
    // SAFETY: parent is a live owner of the child id.
    let handle = unsafe { GetDlgItem(parent, id) };
    if handle.is_null() { None } else { Some(handle) }
}

fn checkbox_checked(hwnd: HWND) -> bool {
    // SAFETY: IDC_CHECK is a checkbox child of hwnd.
    unsafe { IsDlgButtonChecked(hwnd, IDC_CHECK) == BST_CHECKED }
}

fn finish_dialog(hwnd: HWND, outcome: Confirm) {
    if let Some(state) = dialog_state(hwnd) {
        state.outcome = outcome;
    }
    // SAFETY: hwnd is the top-level dialog owned by this helper.
    unsafe {
        DestroyWindow(hwnd);
    }
}

fn dialog_state<'a>(hwnd: HWND) -> Option<&'a mut DialogState> {
    // SAFETY: GWLP_USERDATA is set to the DialogState pointer in WM_CREATE
    // and remains valid until the stack frame in confirm_uninstall returns,
    // which is after the message pump ends.
    let pointer = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut DialogState;
    if pointer.is_null() {
        None
    } else {
        // SAFETY: pointer refers to the confirm_uninstall stack value.
        Some(unsafe { &mut *pointer })
    }
}

fn center_window(hwnd: HWND) {
    let width = 520;
    let height = 280;
    // SAFETY: SM_CXSCREEN and SM_CYSCREEN are predefined system metrics.
    let screen_width = unsafe { GetSystemMetrics(SM_CXSCREEN) };
    // SAFETY: SM_CYSCREEN is a predefined system metric.
    let screen_height = unsafe { GetSystemMetrics(SM_CYSCREEN) };
    let x = (screen_width - width).max(0) / 2;
    let y = (screen_height - height).max(0) / 2;
    // SAFETY: hwnd is a live top-level window.
    unsafe {
        SetWindowPos(hwnd, ptr::null_mut(), x, y, width, height, SWP_NOZORDER);
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

// Store the dialog state pointer when the window is created.
// CreateWindowExW delivers WM_CREATE before returning; retrieve lpCreateParams.
fn store_state_on_create(hwnd: HWND, lparam: LPARAM) {
    // SAFETY: WM_CREATE lParam points at CREATESTRUCTW supplied by CreateWindowExW.
    let created = unsafe { &*(lparam as *const CREATESTRUCTW) };
    // SAFETY: lpCreateParams is the DialogState pointer passed by confirm_uninstall.
    unsafe {
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, created.lpCreateParams as isize);
    }
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
