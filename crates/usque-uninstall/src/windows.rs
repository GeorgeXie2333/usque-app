use std::os::windows::process::CommandExt;
use std::process::Command;
use std::ptr;

use windows_sys::Win32::Foundation::{
    ERROR_FILE_NOT_FOUND, ERROR_SUCCESS, GetLastError, HWND, LPARAM, LRESULT, WPARAM,
};
use windows_sys::Win32::Graphics::Gdi::{
    COLOR_WINDOW, DEFAULT_GUI_FONT, GetStockObject, UpdateWindow,
};
use windows_sys::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Registry::{
    HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_64KEY, REG_SZ, RegCloseKey, RegOpenKeyExW,
    RegQueryValueExW,
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

use crate::{ERROR_INSTALL_USEREXIT, UninstallError, UninstallRequest};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const PRODUCT_KEY: &str = r"Software\Usque";
const PRODUCT_VALUE: &str = "ProductCode";
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
    let mut key = ptr::null_mut();
    let subkey = wide(PRODUCT_KEY);
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
    if status == ERROR_FILE_NOT_FOUND {
        return Err(UninstallError::MissingProductCode);
    }
    if status != ERROR_SUCCESS {
        return Err(UninstallError::Detail(format!(
            "failed to open HKLM\\{PRODUCT_KEY} ({status})"
        )));
    }
    let result = read_product_code_value(key);
    // SAFETY: key was opened successfully above and is not used after this call.
    unsafe {
        RegCloseKey(key);
    }
    result
}

pub(crate) fn run_interactive(product_code: Option<String>) -> Result<i32, UninstallError> {
    if let Some(code) = relaunch_from_temp_if_needed()? {
        return Ok(code);
    }
    let product_code = crate::resolve_product_code(product_code, read_installed_product_code)?;
    match confirm_uninstall()? {
        Confirm::Cancel => Ok(ERROR_INSTALL_USEREXIT),
        Confirm::Uninstall { remove_user_data } => {
            start_delayed_msiexec(&UninstallRequest {
                product_code,
                remove_user_data,
            })?;
            Ok(0)
        }
    }
}

fn read_product_code_value(
    key: windows_sys::Win32::System::Registry::HKEY,
) -> Result<String, UninstallError> {
    let name = wide(PRODUCT_VALUE);
    let mut data_type = 0_u32;
    let mut byte_len = 0_u32;
    // SAFETY: name is null-terminated; the size query may pass a null data pointer.
    let status = unsafe {
        RegQueryValueExW(
            key,
            name.as_ptr(),
            ptr::null_mut(),
            &mut data_type,
            ptr::null_mut(),
            &mut byte_len,
        )
    };
    if status == ERROR_FILE_NOT_FOUND || byte_len < 2 {
        return Err(UninstallError::MissingProductCode);
    }
    if status != ERROR_SUCCESS {
        return Err(UninstallError::Detail(format!(
            "failed to read {PRODUCT_VALUE} ({status})"
        )));
    }
    if data_type != REG_SZ {
        return Err(UninstallError::Detail(format!(
            "{PRODUCT_VALUE} is not a string value"
        )));
    }
    let unit_count = (byte_len as usize).div_ceil(2);
    let mut buffer = vec![0_u16; unit_count];
    let mut actual_len = byte_len;
    // SAFETY: buffer is writable for actual_len bytes reported by the registry.
    let status = unsafe {
        RegQueryValueExW(
            key,
            name.as_ptr(),
            ptr::null_mut(),
            &mut data_type,
            buffer.as_mut_ptr().cast(),
            &mut actual_len,
        )
    };
    if status != ERROR_SUCCESS {
        return Err(UninstallError::Detail(format!(
            "failed to read {PRODUCT_VALUE} ({status})"
        )));
    }
    let units = actual_len as usize / 2;
    let wide = buffer.get(..units).unwrap_or(&buffer);
    let end = wide
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(wide.len());
    let text = String::from_utf16(&wide[..end])
        .map_err(|_| UninstallError::Detail(format!("{PRODUCT_VALUE} is not valid UTF-16")))?;
    if text.is_empty() {
        return Err(UninstallError::MissingProductCode);
    }
    Ok(text)
}

fn relaunch_from_temp_if_needed() -> Result<Option<i32>, UninstallError> {
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
    let forwarded: Vec<String> = std::env::args().skip(1).collect();
    let status = Command::new(&destination)
        .args(forwarded)
        .status()
        .map_err(|error| {
            UninstallError::Detail(format!("failed to start the temporary helper: {error}"))
        })?;
    Ok(Some(status.code().unwrap_or(1)))
}

fn start_delayed_msiexec(request: &UninstallRequest) -> Result<(), UninstallError> {
    Command::new("cmd.exe")
        .args(["/D", "/C", &request.delayed_cmd_line()])
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map_err(|error| {
            UninstallError::Detail(format!("failed to start Windows Installer: {error}"))
        })?;
    Ok(())
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

    let mut state = DialogState {
        outcome: Confirm::Cancel,
    };
    let title = wide("Uninstall Usque");
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
            text: "This will remove the Usque application and the Usque Agent service.",
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
            text: "Delete profiles, settings, logs, caches, and WARP identities for this Windows user.",
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
            text: "This cannot be undone. Leave this option unchecked to keep local data. Other Windows users are not affected.",
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
            text: "Uninstall",
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
            text: "Cancel",
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
