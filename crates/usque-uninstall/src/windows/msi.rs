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
    Foundation::{ERROR_INVALID_PARAMETER, ERROR_SUCCESS, HWND},
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
    /// Only MSI's numeric error field survives completion; raw records do not.
    pub record_code: Option<u32>,
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
    registration: Mutex<Option<Completion>>,
}

impl Shared {
    pub(super) fn begin_registration(&self, completion: &Completion) {
        // Retain MSI's completed transaction before starting Burn. A worker
        // failure cannot turn registration-only retry into another uninstall.
        *self.registration.lock().unwrap_or_else(|e| e.into_inner()) = Some(completion.clone());
        let mut lifecycle = self.lifecycle.lock().unwrap_or_else(|e| e.into_inner());
        lifecycle.stage = Stage::Registration;
        lifecycle.purge_started = completion.purge_started;
    }

    pub fn registration_context(&self) -> Option<Completion> {
        self.registration
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

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
    shared.begin_registration(&previous);
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
        record_code: None,
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
    let _restore = match configure_msi_ui(owner) {
        Ok(restore) => restore,
        // A rejected UI configuration must never reach the MSI transaction.
        Err(code) => return failed(code),
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
        record_code: *shared.error_code.lock().unwrap_or_else(|e| e.into_inner()),
        purge_started: state.purge_started,
        remove_user_data: request.remove_user_data,
        bundle,
    };
    if !successful_installer_exit(code as i32) {
        return completion;
    }
    shared.begin_registration(&completion);
    finish_registration(completion)
}

fn finish_registration(completion: Completion) -> Completion {
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
    registration_result(completion, code)
}

pub(super) fn registration_result(mut completion: Completion, code: u32) -> Completion {
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

fn configure_msi_ui(owner: HWND) -> Result<RestoreMsiUi, u32> {
    configure_msi_ui_with(owner, |level, previous_owner| {
        // SAFETY: the UI keeps the borrowed owner window alive until this
        // synchronous operation and its callbacks return. The owner slot is
        // writable. No token change or runas is performed: MSI retains its
        // original-user impersonation contract for PurgeUserData.
        unsafe { MsiSetInternalUI(level, previous_owner) }
    })
}

fn configure_msi_ui_with(
    mut owner: HWND,
    set_ui: impl FnOnce(INSTALLUILEVEL, &mut HWND) -> INSTALLUILEVEL,
) -> Result<RestoreMsiUi, u32> {
    // Source resolution belongs to MSI, including its native source chooser.
    // Every other transaction prompt remains handled by the external UI.
    let previous_ui = set_ui(
        INSTALLUILEVEL_NONE | INSTALLUILEVEL_UACONLY | INSTALLUILEVEL_SOURCERESONLY,
        &mut owner,
    );
    if previous_ui == INSTALLUILEVEL_NOCHANGE {
        return Err(ERROR_INVALID_PARAMETER);
    }
    Ok(RestoreMsiUi {
        previous_ui,
        previous_owner: owner,
    })
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
    use windows_sys::Win32::{
        System::LibraryLoader::GetModuleHandleW,
        UI::WindowsAndMessaging::{CreateWindowExW, DestroyWindow, IsWindowVisible},
    };

    // MsiSetInternalUI and MsiSetExternalUIRecord change process-wide state.
    // Every test that changes those hooks must hold this lock for its guards.
    static MSI_UI_TEST_LOCK: Mutex<()> = Mutex::new(());

    #[link(name = "msi")]
    unsafe extern "system" {
        // windows-sys 0.61 models this output as a callback rather than a
        // pointer to a callback. Production passes null; the test needs the
        // SDK's actual output-pointer signature to inspect hook restoration.
        #[link_name = "MsiSetExternalUIRecord"]
        fn query_external_ui_record(
            handler: PINSTALLUI_HANDLER_RECORD,
            filter: u32,
            context: *const c_void,
            previous: *mut PINSTALLUI_HANDLER_RECORD,
        ) -> u32;
    }

    struct HiddenWindow(HWND);

    impl HiddenWindow {
        fn new() -> Self {
            // SAFETY: STATIC is a predefined class; all supplied strings and
            // arguments are valid. Without WS_VISIBLE this fixture is hidden.
            let hwnd = unsafe {
                CreateWindowExW(
                    0,
                    wide("STATIC").as_ptr(),
                    wide("Usque inert MSI owner fixture").as_ptr(),
                    0,
                    0,
                    0,
                    1,
                    1,
                    ptr::null_mut(),
                    ptr::null_mut(),
                    GetModuleHandleW(ptr::null()),
                    ptr::null(),
                )
            };
            assert!(!hwnd.is_null(), "create hidden owner fixture");
            // SAFETY: hwnd is the window just created on this test thread.
            assert_eq!(unsafe { IsWindowVisible(hwnd) }, 0);
            Self(hwnd)
        }
    }

    impl Drop for HiddenWindow {
        fn drop(&mut self) {
            // SAFETY: this fixture owns a window on the current test thread.
            unsafe { DestroyWindow(self.0) };
        }
    }

    struct Record(MSIHANDLE);

    impl Record {
        fn new(fields: u32) -> Self {
            // SAFETY: creating an in-memory record does not start an install.
            let record = unsafe { MsiCreateRecord(fields) };
            assert_ne!(record, 0, "create inert MSI record");
            Self(record)
        }

        fn integer(&self, field: u32, value: i32) {
            // SAFETY: this fixture owns the record; test fields are in range.
            assert_eq!(unsafe { MsiRecordSetInteger(self.0, field, value) }, 0);
        }

        fn string(&self, field: u32, value: &str) {
            // SAFETY: this fixture owns the record; the temporary wide string
            // outlives the synchronous call, which copies the string.
            assert_eq!(
                // SAFETY: the owned record and copied wide string are live.
                unsafe { MsiRecordSetStringW(self.0, field, wide(value).as_ptr()) },
                0
            );
        }

        fn send(&self, context: &mut CallbackContext, kind: u32) -> i32 {
            // SAFETY: context and this borrowed record remain alive for the
            // direct callback invocation. No MSI transaction is started.
            unsafe { callback(ptr::from_mut(context).cast(), kind, self.0) }
        }
    }

    impl Drop for Record {
        fn drop(&mut self) {
            // SAFETY: this fixture uniquely owns the MSI record handle.
            unsafe { MsiCloseHandle(self.0) };
        }
    }

    unsafe extern "system" fn inert_handler(
        _context: *mut c_void,
        _kind: u32,
        _record: MSIHANDLE,
    ) -> i32 {
        // The hook is only registered and queried, never used by an install.
        -1
    }

    #[test]
    fn interactive_msi_ui_accepts_source_resolution_and_restores_owner_and_hooks() {
        let _serial = MSI_UI_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let old_owner = HiddenWindow::new();
        let operation_owner = HiddenWindow::new();
        let mut original_owner = old_owner.0;
        // SAFETY: the fixture is a live hidden window; this only sets UI state.
        let original_ui = unsafe { MsiSetInternalUI(INSTALLUILEVEL_BASIC, &mut original_owner) };
        assert_ne!(original_ui, INSTALLUILEVEL_NOCHANGE);
        // Also restore the test's initial process state on assertion failure.
        // Both fixture windows outlive every restoration guard.
        let _restore_original = RestoreMsiUi {
            previous_ui: original_ui,
            previous_owner: original_owner,
        };
        let expected = INSTALLUILEVEL_NONE | INSTALLUILEVEL_UACONLY | INSTALLUILEVEL_SOURCERESONLY;
        let restore = configure_msi_ui_with(operation_owner.0, |level, owner| {
            assert_eq!(level, expected, "send both source and UAC modifiers to MSI");
            // SAFETY: owner is a writable slot containing the live hidden fixture.
            unsafe { MsiSetInternalUI(level, owner) }
        })
        .expect("source and UAC UI accepted");
        // Reapply the exact level while inspecting it: NOCHANGE alone clears
        // source/UAC modifier flags even though it retains the base UI level.
        assert_eq!(
            // SAFETY: this only reapplies the same UI configuration, with no transaction.
            unsafe { MsiSetInternalUI(expected, ptr::null_mut()) },
            expected
        );
        let mut owner = operation_owner.0;
        // SAFETY: setting the same live owner returns the configured owner.
        assert_eq!(
            // SAFETY: owner is the live hidden fixture window.
            unsafe { MsiSetInternalUI(expected, &mut owner) },
            expected
        );
        assert_eq!(owner, operation_owner.0);
        // SAFETY: this inert hook is never called by a transaction; it borrows
        // no context and is cleared by both production restoration guards.
        assert_eq!(
            // SAFETY: no transaction can invoke this context-free inert hook.
            unsafe {
                MsiSetExternalUIRecord(
                    Some(inert_handler),
                    INSTALLLOGMODE_RESOLVESOURCE as u32,
                    ptr::null(),
                    None,
                )
            },
            ERROR_SUCCESS
        );
        drop(restore);
        let mut restored_owner = old_owner.0;
        // SAFETY: query the restored level while retaining its live owner.
        assert_eq!(
            // SAFETY: the restored owner fixture remains live until guards drop.
            unsafe { MsiSetInternalUI(INSTALLUILEVEL_NOCHANGE, &mut restored_owner) },
            INSTALLUILEVEL_BASIC
        );
        assert_eq!(restored_owner, old_owner.0);
        let mut previous_handler = None;
        // SAFETY: this only queries/clears the hook, with a writable output
        // slot, while the process-wide test lock prevents concurrent changes.
        assert_eq!(
            // SAFETY: previous_handler is a writable SDK callback output slot.
            unsafe { query_external_ui_record(None, 0, ptr::null(), &mut previous_handler) },
            ERROR_SUCCESS
        );
        assert!(
            previous_handler.is_none(),
            "production guard cleared the hook"
        );
    }

    #[test]
    fn rejected_msi_ui_configuration_returns_an_error_without_a_guard() {
        let _serial = MSI_UI_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let result =
            configure_msi_ui_with(ptr::null_mut(), |_level, _owner| INSTALLUILEVEL_NOCHANGE);
        match result {
            Err(code) => assert_eq!(code, ERROR_INVALID_PARAMETER),
            Ok(_) => panic!("rejected MSI UI must not permit a transaction"),
        }
    }

    #[test]
    fn source_resolution_is_delegated_even_after_a_cancel_request() {
        let shared = Arc::new(Shared::default());
        let mut context = CallbackContext {
            shared: shared.clone(),
        };
        let common_data = Record::new(2);
        common_data.integer(1, 2);
        common_data.integer(2, 1);
        assert_eq!(
            common_data.send(&mut context, INSTALLMESSAGE_COMMONDATA as u32),
            IDOK
        );
        assert!(shared.request_cancel());
        let source = Record::new(1);
        source.string(1, r"C:\inert-source-fixture\missing.msi");
        assert_eq!(
            source.send(&mut context, INSTALLMESSAGE_RESOLVESOURCE as u32),
            0
        );
        assert_eq!(
            source.send(
                &mut context,
                INSTALLMESSAGE_RESOLVESOURCE as u32 | MB_RETRYCANCEL
            ),
            0
        );
        assert!(shared.snapshot().cancel_requested);
        assert!(shared.prompt.lock().unwrap().is_none());
        assert!(shared.error_code.lock().unwrap().is_none());
    }

    #[test]
    fn record_callbacks_keep_non_source_errors_fail_closed() {
        let shared = Arc::new(Shared::default());
        let mut context = CallbackContext {
            shared: shared.clone(),
        };
        let error = Record::new(2);
        error.integer(1, 1722);
        error.string(2, "RecoverAgentState");
        for (kind, buttons, expected) in [
            (INSTALLMESSAGE_ERROR, MB_ABORTRETRYIGNORE, IDABORT),
            (INSTALLMESSAGE_ERROR, MB_RETRYCANCEL, IDCANCEL),
            (INSTALLMESSAGE_WARNING, MB_YESNO, IDNO),
            (INSTALLMESSAGE_USER, MB_OKCANCEL, IDCANCEL),
        ] {
            assert_eq!(error.send(&mut context, kind as u32 | buttons), expected);
        }
        assert_eq!(*shared.error_code.lock().unwrap(), Some(1722));
        assert_eq!(shared.snapshot().finish(1603), Outcome::NetworkFailed);
        assert!(shared.prompt.lock().unwrap().is_none());
    }

    #[test]
    fn record_callbacks_cancel_execution_but_stop_before_data_deletion() {
        let shared = Arc::new(Shared::default());
        let mut context = CallbackContext {
            shared: shared.clone(),
        };
        let common_data = Record::new(2);
        common_data.integer(1, 2);
        common_data.integer(2, 1);
        assert_eq!(
            common_data.send(&mut context, INSTALLMESSAGE_COMMONDATA as u32),
            IDOK
        );
        assert!(shared.request_cancel());
        let progress = Record::new(4);
        for (field, value) in [(1, 0), (2, 100), (3, 0), (4, 0)] {
            progress.integer(field, value);
        }
        assert_eq!(
            progress.send(&mut context, INSTALLMESSAGE_PROGRESS as u32),
            IDCANCEL
        );
        let action = Record::new(1);
        action.string(1, "PurgeUserData");
        assert_eq!(
            action.send(&mut context, INSTALLMESSAGE_ACTIONSTART as u32),
            IDOK
        );
        assert_eq!(
            progress.send(&mut context, INSTALLMESSAGE_PROGRESS as u32),
            IDOK
        );
        assert!(shared.snapshot().purge_started);
        assert_eq!(shared.snapshot().finish(1602), Outcome::DataMayBeDeleted);
    }

    #[test]
    fn registration_retry_has_no_msi_or_data_deletion_entrypoint() {
        let original = Completion {
            outcome: Outcome::RegistrationFailed,
            code: 1,
            msi_code: 3010,
            record_code: Some(1722),
            purge_started: true,
            remove_user_data: true,
            bundle: None,
        };
        let completed = finish_registration(original);
        assert_eq!(completed.outcome, Outcome::RebootRequired);
        assert_eq!(completed.code, 3010);
        assert!(completed.purge_started);
    }

    #[test]
    fn registration_result_preserves_msi_identity_and_irreversible_data_flags() {
        for msi_code in [0, 3010, 1641] {
            for deleted_data in [false, true] {
                for registration_code in [0, 3010, 1641, 1603, 1602, 1] {
                    let previous = Completion {
                        outcome: Outcome::RegistrationFailed,
                        code: 1603,
                        msi_code,
                        record_code: Some(1706),
                        purge_started: deleted_data,
                        remove_user_data: deleted_data,
                        bundle: Some(PathBuf::from(r"C:\inert-fixture\{bundle-id}\setup.exe")),
                    };
                    let result = registration_result(previous.clone(), registration_code);
                    assert_eq!(result.msi_code, previous.msi_code);
                    assert_eq!(result.record_code, previous.record_code);
                    assert_eq!(result.purge_started, previous.purge_started);
                    assert_eq!(result.remove_user_data, previous.remove_user_data);
                    assert_eq!(result.bundle, previous.bundle);
                    let expected = if matches!(registration_code, 1603 | 1602 | 1) {
                        Outcome::RegistrationFailed
                    } else if msi_code == 0 && registration_code == 0 {
                        Outcome::Success
                    } else {
                        Outcome::RebootRequired
                    };
                    assert_eq!(result.outcome, expected);
                }
            }
        }
    }

    #[test]
    fn registration_context_is_saved_before_worker_execution_and_cannot_cancel() {
        let previous = Completion {
            outcome: Outcome::Success,
            code: 0,
            msi_code: 0,
            record_code: Some(1722),
            purge_started: true,
            remove_user_data: true,
            bundle: Some(PathBuf::from(r"C:\inert-fixture\{bundle-id}\setup.exe")),
        };
        let shared = Shared::default();
        shared.begin_registration(&previous);
        let retained = shared.registration_context().unwrap();
        assert_eq!(retained.msi_code, 0);
        assert_eq!(retained.bundle, previous.bundle);
        assert!(retained.purge_started && retained.remove_user_data);
        assert_eq!(shared.snapshot().stage, Stage::Registration);
        assert!(!shared.request_cancel());
    }
}
