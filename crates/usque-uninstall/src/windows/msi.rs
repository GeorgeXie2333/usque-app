//! Interactive MSI adapter. Quiet uninstall continues to use the original launcher.
use std::{
    ffi::c_void,
    path::PathBuf,
    ptr,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender},
    },
    thread,
};

use windows_sys::Win32::{
    Foundation::{ERROR_SUCCESS, HWND},
    System::ApplicationInstallationAndServicing::*,
    UI::WindowsAndMessaging::{
        IDABORT, IDCANCEL, IDNO, IDOK, IDRETRY, MB_ABORTRETRYIGNORE, MB_OKCANCEL, MB_RETRYCANCEL,
        MB_TYPEMASK, MB_YESNO, MB_YESNOCANCEL,
    },
};

use super::{
    combine_success_codes, find_registered_bundle, run_bundle_cleanup, successful_installer_exit,
    wide,
};
use crate::{
    UninstallRequest,
    state::{Lifecycle, Outcome, Stage},
};

#[derive(Clone, Debug)]
pub struct Completion {
    pub outcome: Outcome,
    pub code: u32,
    pub msi_code: u32,
    pub purge_started: bool,
    pub remove_user_data: bool,
    pub bundle: Option<PathBuf>,
}

pub struct Prompt {
    pub items: Vec<String>,
    pub accept: i32,
    pub decline: i32,
    pub files_in_use: bool,
    pub reply: Sender<i32>,
}

#[derive(Default)]
pub struct Shared {
    pub lifecycle: Mutex<Lifecycle>,
    pub prompt: Mutex<Option<Prompt>>,
    pub cancel: AtomicBool,
    pub error_code: Mutex<Option<u32>>,
}

impl Shared {
    pub fn snapshot(&self) -> Lifecycle {
        self.lifecycle
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    pub fn request_cancel(&self) -> bool {
        let mut state = self.lifecycle.lock().unwrap_or_else(|e| e.into_inner());
        if state.request_cancel() {
            self.cancel.store(true, Ordering::Release);
            true
        } else {
            false
        }
    }
}

pub struct Operation {
    pub shared: Arc<Shared>,
    pub completion: Receiver<Completion>,
    pub worker: Option<thread::JoinHandle<()>>,
}

pub fn start(request: UninstallRequest, owner: HWND) -> Operation {
    let shared = Arc::new(Shared::default());
    let worker_shared = shared.clone();
    let (tx, rx) = mpsc::channel();
    // HWND is borrowed only while the UI owns this operation and cannot close.
    let owner = owner as usize;
    let worker = thread::spawn(move || {
        let result = execute(request, owner as HWND, &worker_shared);
        let _ = tx.send(result);
    });
    Operation {
        shared,
        completion: rx,
        worker: Some(worker),
    }
}

pub fn retry_registration(previous: Completion) -> Operation {
    let shared = Arc::new(Shared::default());
    shared
        .lifecycle
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .stage = Stage::Registration;
    let (tx, rx) = mpsc::channel();
    let worker = thread::spawn(move || {
        // This entry point cannot call MSI or replay user-data deletion.
        let result = finish_registration(previous);
        let _ = tx.send(result);
    });
    Operation {
        shared,
        completion: rx,
        worker: Some(worker),
    }
}

fn execute(request: UninstallRequest, owner: HWND, shared: &Arc<Shared>) -> Completion {
    let failed = |code| Completion {
        outcome: Outcome::MsiFailed,
        code,
        msi_code: code,
        purge_started: false,
        remove_user_data: request.remove_user_data,
        bundle: None,
    };
    let Ok(current) = std::env::current_exe() else {
        return failed(2);
    };
    if !crate::is_temp_relaunch_path(&current, &std::env::temp_dir()) {
        return failed(5);
    }
    // Resolve and verify Burn before any mutation, because MSI removes its own
    // registration and files. Registry text is never executable shell source.
    let bundle = match find_registered_bundle(&current) {
        Ok(bundle) => bundle,
        Err(_) => return failed(13),
    };
    let product = wide(&request.product_code);
    // SAFETY: product is a live nul-terminated GUID. This is a read-only query.
    let installed = unsafe { MsiQueryProductStateW(product.as_ptr()) };
    if installed != INSTALLSTATE_DEFAULT {
        return failed(1605);
    }
    let properties = wide(&format!(
        "USQUE_REMOVE_USER_DATA={} REBOOT=ReallySuppress",
        if request.remove_user_data { "1" } else { "0" }
    ));
    let mut context = CallbackContext {
        shared: shared.clone(),
    };
    let mut owner = owner;
    // SAFETY: the live window remains owned by the UI until this synchronous
    // call and all callbacks return. No token change or runas is performed:
    // MSI retains its original-user impersonation contract for PurgeUserData.
    let previous_ui =
        unsafe { MsiSetInternalUI(INSTALLUILEVEL_NONE | INSTALLUILEVEL_UACONLY, &mut owner) };
    let _restore = RestoreMsiUi {
        previous_ui,
        previous_owner: owner,
    };
    let filter = INSTALLLOGMODE_ACTIONSTART
        | INSTALLLOGMODE_ACTIONDATA
        | INSTALLLOGMODE_PROGRESS
        | INSTALLLOGMODE_COMMONDATA
        | INSTALLLOGMODE_ERROR
        | INSTALLLOGMODE_WARNING
        | INSTALLLOGMODE_FATALEXIT
        | INSTALLLOGMODE_USER
        | INSTALLLOGMODE_FILESINUSE
        | INSTALLLOGMODE_RMFILESINUSE
        | INSTALLLOGMODE_OUTOFDISKSPACE
        | INSTALLLOGMODE_RESOLVESOURCE;
    // SAFETY: context remains at this stack address through the synchronous
    // MSI call. The callback copies record fields; it never retains MSI handles.
    let registered = unsafe {
        MsiSetExternalUIRecord(
            Some(callback),
            filter as u32,
            ptr::from_mut(&mut context).cast(),
            None,
        )
    };
    if registered != ERROR_SUCCESS {
        return failed(registered);
    }
    // SAFETY: both strings outlive the synchronous MSI call. ABSENT invokes the
    // authored uninstall sequence; this does not implement a separate cleanup.
    let code = unsafe {
        MsiConfigureProductExW(
            product.as_ptr(),
            INSTALLLEVEL_DEFAULT,
            INSTALLSTATE_ABSENT,
            properties.as_ptr(),
        )
    };
    let state = shared.snapshot();
    let completion = Completion {
        outcome: state.finish(code),
        code,
        msi_code: code,
        purge_started: state.purge_started,
        remove_user_data: request.remove_user_data,
        bundle,
    };
    if !successful_installer_exit(code as i32) {
        return completion;
    }
    shared
        .lifecycle
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .stage = Stage::Registration;
    finish_registration(completion)
}

fn finish_registration(mut completion: Completion) -> Completion {
    let code = if let Some(bundle) = &completion.bundle {
        // Reverify every attempt. The original window owns this exact trusted
        // cache path; retry never discovers a substitute or re-enters MSI.
        let verified = std::env::current_exe().ok().is_some_and(|current| {
            usque_platform::windows_authenticode::verify_same_signer(&current, bundle).is_ok()
        });
        if !verified {
            13
        } else {
            run_bundle_cleanup(bundle).unwrap_or(1) as u32
        }
    } else {
        0
    };
    if successful_installer_exit(code as i32) {
        completion.code = combine_success_codes(completion.msi_code as i32, code as i32) as u32;
        completion.outcome = if completion.code == 0 {
            Outcome::Success
        } else {
            Outcome::RebootRequired
        };
    } else {
        completion.code = code;
        completion.outcome = Outcome::RegistrationFailed;
    }
    completion
}

struct RestoreMsiUi {
    previous_ui: INSTALLUILEVEL,
    previous_owner: HWND,
}

impl Drop for RestoreMsiUi {
    fn drop(&mut self) {
        // SAFETY: this dedicated helper owns its process-wide MSI UI hooks.
        // The synchronous operation has returned; there can be no late callback.
        unsafe {
            MsiSetExternalUIRecord(None, 0, ptr::null(), None);
            MsiSetInternalUI(self.previous_ui, &mut self.previous_owner);
        }
    }
}

struct CallbackContext {
    shared: Arc<Shared>,
}

unsafe extern "system" fn callback(context: *mut c_void, kind: u32, record: MSIHANDLE) -> i32 {
    // No Rust unwind may cross the system ABI, even after a malformed record.
    std::panic::catch_unwind(|| {
        if context.is_null() {
            return -1;
        }
        // SAFETY: execute registers this pointer only while its stack frame and
        // CallbackContext remain alive. MSI invokes callbacks synchronously.
        let context = unsafe { &*(context.cast::<CallbackContext>()) };
        handle_record(context, kind, record)
    })
    .unwrap_or(-1)
}

fn handle_record(context: &CallbackContext, kind: u32, record: MSIHANDLE) -> i32 {
    let message = (kind & 0xff00_0000) as i32;
    let shared = &context.shared;
    match message {
        INSTALLMESSAGE_ACTIONSTART => {
            if let Some(action) = string_field(record, 1) {
                shared
                    .lifecycle
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .action(&action);
            }
        }
        INSTALLMESSAGE_ACTIONDATA => {
            shared
                .lifecycle
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .progress
                .action_data();
        }
        INSTALLMESSAGE_PROGRESS => {
            let fields = [
                integer_field(record, 1),
                integer_field(record, 2),
                integer_field(record, 3),
                integer_field(record, 4),
            ];
            shared
                .lifecycle
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .progress_record(fields);
        }
        INSTALLMESSAGE_COMMONDATA => {
            shared
                .lifecycle
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .common_data(integer_field(record, 1), integer_field(record, 2));
        }
        INSTALLMESSAGE_FILESINUSE | INSTALLMESSAGE_RMFILESINUSE => {
            // MSI owns the record. Keep a bounded local copy, not raw INFO logs
            // or arbitrary property data; it is shown only in the current UI.
            // SAFETY: record is a live borrowed callback record.
            let count = unsafe { MsiRecordGetFieldCount(record) }.min(32);
            let items = (1..=count)
                .filter_map(|field| string_field(record, field))
                .filter(|item| !item.is_empty() && !item.bytes().all(|byte| byte.is_ascii_digit()))
                .collect();
            return ask(
                shared,
                items,
                if message == INSTALLMESSAGE_RMFILESINUSE {
                    IDOK
                } else {
                    IDRETRY
                },
                IDCANCEL,
                true,
            );
        }
        INSTALLMESSAGE_ERROR
        | INSTALLMESSAGE_FATALEXIT
        | INSTALLMESSAGE_OUTOFDISKSPACE
        | INSTALLMESSAGE_USER
        | INSTALLMESSAGE_WARNING => {
            let error = integer_field(record, 1);
            if error > 0 {
                *shared.error_code.lock().unwrap_or_else(|e| e.into_inner()) = Some(error as u32);
            }
            if error == 1722
                && let Some(action) = string_field(record, 2)
            {
                shared
                    .lifecycle
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .action_failed(&action);
            }
            return match kind & MB_TYPEMASK {
                MB_ABORTRETRYIGNORE => IDABORT,
                MB_RETRYCANCEL | MB_OKCANCEL => IDCANCEL,
                MB_YESNO | MB_YESNOCANCEL => IDNO,
                _ => return IDOK,
            };
        }
        INSTALLMESSAGE_RESOLVESOURCE => return 0,
        _ => return IDOK,
    }
    // Cancellation is acknowledged only at MSI's documented cancellation
    // message points. Never stop a child or bypass its cleanup/rollback.
    if matches!(message, INSTALLMESSAGE_PROGRESS | INSTALLMESSAGE_ACTIONDATA)
        && shared.cancel.load(Ordering::Acquire)
    {
        let state = shared.snapshot();
        if state.cancel_at_callback() {
            return IDCANCEL;
        }
    }
    IDOK
}

fn ask(shared: &Shared, items: Vec<String>, accept: i32, decline: i32, files_in_use: bool) -> i32 {
    let (tx, rx) = mpsc::channel();
    *shared.prompt.lock().unwrap_or_else(|e| e.into_inner()) = Some(Prompt {
        items,
        accept,
        decline,
        files_in_use,
        reply: tx,
    });
    // The UI pumps messages on a different thread. Closing the prompt channel
    // fails closed and can never synthesize permission to continue.
    let response = rx.recv().unwrap_or(decline);
    if response == IDCANCEL {
        shared
            .lifecycle
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .user_declined_prompt();
    }
    response
}

fn integer_field(record: MSIHANDLE, field: u32) -> i32 {
    // SAFETY: record is borrowed from MSI for this callback only.
    unsafe { MsiRecordGetInteger(record, field) }
}

fn string_field(record: MSIHANDLE, field: u32) -> Option<String> {
    let mut buffer = [0_u16; 512];
    let mut length = (buffer.len() - 1) as u32;
    // SAFETY: the record is live and the length matches the writable buffer.
    let status = unsafe { MsiRecordGetStringW(record, field, buffer.as_mut_ptr(), &mut length) };
    if status != ERROR_SUCCESS || length as usize >= buffer.len() {
        return None;
    }
    let text = String::from_utf16(&buffer[..length as usize]).ok()?;
    Some(text.chars().filter(|ch| !ch.is_control()).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registration_retry_has_no_msi_or_data_deletion_entrypoint() {
        let original = Completion {
            outcome: Outcome::RegistrationFailed,
            code: 1,
            msi_code: 3010,
            purge_started: true,
            remove_user_data: true,
            bundle: None,
        };
        let completed = finish_registration(original);
        assert_eq!(completed.outcome, Outcome::RebootRequired);
        assert_eq!(completed.code, 3010);
        assert!(completed.purge_started);
    }
}
