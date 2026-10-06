//! A single native window for confirmation, MSI progress, and the final result.
use super::{
    last_error,
    msi::{self, Completion, Operation, Prompt},
    ui_locale_name, wide,
};
use crate::{
    UninstallError, UninstallRequest, l10n,
    state::{Lifecycle, Outcome, Preview, PreviewTheme, RestartFlow, Stage, error_hint_key},
};
use std::{
    cell::{Cell, RefCell},
    mem::size_of,
    ptr,
    sync::mpsc::TryRecvError,
};
use windows_sys::Win32::{
    Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM},
    Graphics::{
        Dwm::{DWMWA_USE_IMMERSIVE_DARK_MODE, DwmSetWindowAttribute},
        Gdi::*,
    },
    System::{
        LibraryLoader::GetModuleHandleW,
        Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW},
    },
    UI::{
        Accessibility::{HCF_HIGHCONTRASTON, HIGHCONTRASTW, NotifyWinEvent},
        Controls::*,
        HiDpi::{AdjustWindowRectExForDpi, GetDpiForWindow},
        Input::KeyboardAndMouse::{
            EnableWindow, GetFocus, IsWindowEnabled, SetFocus, VK_F6, VK_F7, VK_F8,
        },
        Shell::{DefSubclassProc, SetWindowSubclass},
        WindowsAndMessaging::*,
    },
};

const CLASS: &str = "Usque.UninstallWizard";
const TITLE: i32 = 101;
const BODY: i32 = 102;
const CHECK: i32 = 103;
const DESCRIPTION: i32 = 104;
const WARNING: i32 = 105;
const STATUS: i32 = 106;
const PROGRESS: i32 = 107;
const DETAIL: i32 = 108;
const PRIMARY: i32 = 109;
const SECONDARY: i32 = 110;
const DETAILS: i32 = 111;
const BADGE: i32 = 112;
const SAVE: i32 = 113;
const VIEWPORT: i32 = 114;
const WARNING_ACCENT: i32 = 115;
const TIMER: usize = 1;
const WM_RESTART_ABORTED: u32 = WM_APP + 36;
const PAINT_PROPERTY: &str = "Usque.UninstallPaint";
const THEME_REQUEST_PROPERTY: &str = "Usque.UninstallThemeRequest";
// Winuser.h SS_TYPEMASK/SS_RIGHT/SS_OWNERDRAW. windows-sys groups these immutable
// control-style constants under the unrelated SystemServices feature.
const STATIC_TYPE_MASK: u32 = 0x1f;
const STATIC_RIGHT: u32 = 2;
const STATIC_OWNER_DRAW: u32 = 13;
const fn brand_accent(dark: bool) -> u32 {
    if dark { 0x005ca4ff } else { 0x000c50c2 }
}

const fn brand_on_accent(dark: bool) -> u32 {
    if dark { 0x00001844 } else { 0x00ffffff }
}

const fn brand_pressed(dark: bool) -> u32 {
    let color = brand_accent(dark);
    let red = ((color & 0xff) * 9 + 5) / 10;
    let green = (((color >> 8) & 0xff) * 9 + 5) / 10;
    let blue = (((color >> 16) & 0xff) * 9 + 5) / 10;
    red | (green << 8) | (blue << 16)
}

#[derive(Clone, Copy, Default)]
struct PaintStyle {
    dpi: u32,
    background: u32,
    foreground: u32,
    muted: u32,
    brush: HBRUSH,
    font: HFONT,
    dark: bool,
    high_contrast: bool,
    native_high_contrast: bool,
    accent_button: u32,
    rtl: bool,
}

struct NativeThemeRequest(HWND);

impl NativeThemeRequest {
    fn begin(hwnd: HWND) -> Option<Self> {
        // SAFETY: the property is private to this UI-thread window. Its opaque
        // non-null sentinel is only queried for presence, never dereferenced.
        if unsafe { SetPropW(hwnd, wide(THEME_REQUEST_PROPERTY).as_ptr(), 1usize as _) } == 0 {
            return None;
        }
        Some(Self(hwnd))
    }
}

impl Drop for NativeThemeRequest {
    fn drop(&mut self) {
        // SAFETY: this guard owns the property's lifetime on the UI thread.
        // Removing it restores normal handling of external OS theme messages.
        unsafe {
            RemovePropW(self.0, wide(THEME_REQUEST_PROPERTY).as_ptr());
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    Confirm,
    Running,
    Finished,
}

struct Palette {
    background: u32,
    foreground: u32,
    muted: u32,
    brush: HBRUSH,
    dark: bool,
    high_contrast: bool,
    native_high_contrast: bool,
}
impl Drop for Palette {
    fn drop(&mut self) {
        // SAFETY: this palette uniquely owns its solid background brush.
        unsafe {
            DeleteObject(self.brush);
        }
    }
}

struct Fonts {
    body: HFONT,
    secondary: HFONT,
    title: HFONT,
}
impl Drop for Fonts {
    fn drop(&mut self) {
        // SAFETY: no owned font remains selected into a DC or live control
        // when this value is dropped after font replacement/window destruction.
        unsafe {
            DeleteObject(self.body);
            DeleteObject(self.secondary);
            DeleteObject(self.title);
        }
    }
}

struct State {
    product: Option<String>,
    preview: Option<Preview>,
    preview_theme: Option<PreviewTheme>,
    locale: &'static str,
    page: Page,
    remove_data: bool,
    lifecycle: Lifecycle,
    restart: RestartFlow,
    operation: Option<Operation>,
    result: Option<Completion>,
    pending_registration: Option<Completion>,
    prompt: Option<Prompt>,
    details: bool,
    wait_on_close: bool,
    scroll: i32,
    max_scroll: i32,
    dpi: u32,
    palette: Palette,
    fonts: Fonts,
    preview_ticks: u32,
    save_code: Option<i32>,
    exit_code: i32,
    controls: Vec<HWND>,
    detail_accessible_name: &'static str,
    paint: Box<Cell<PaintStyle>>,
}

impl State {
    fn new(
        product: Option<String>,
        preview: Option<Preview>,
        preview_theme: Option<PreviewTheme>,
        locale: &'static str,
    ) -> Self {
        Self {
            product,
            preview,
            preview_theme,
            locale,
            page: Page::Confirm,
            remove_data: false,
            lifecycle: Lifecycle::default(),
            restart: RestartFlow::Idle,
            operation: None,
            result: None,
            pending_registration: None,
            prompt: None,
            details: false,
            wait_on_close: false,
            scroll: 0,
            max_scroll: 0,
            dpi: 96,
            palette: palette(preview.is_some(), preview_theme),
            fonts: fonts(96),
            preview_ticks: 0,
            save_code: None,
            exit_code: crate::ERROR_INSTALL_USEREXIT,
            controls: Vec::new(),
            detail_accessible_name: "",
            paint: Box::new(Cell::new(PaintStyle::default())),
        }
    }

    fn text(&self, key: &str) -> &'static str {
        l10n::setup_text(self.locale, key)
    }

    fn body_key(&self) -> &'static str {
        if let Some(key) = self.restart.message_key() {
            key
        } else if let Some(result) = &self.result {
            if result.outcome == Outcome::DataMayBeDeleted {
                "uninstall_failed"
            } else {
                result.outcome.key()
            }
        } else if self.page == Page::Running && self.wait_on_close {
            "wait_no_cancel"
        } else if self.prompt.as_ref().is_some_and(|p| p.files_in_use) {
            "uninstall_files_in_use"
        } else if self.prompt.is_some() {
            "uninstall_failed"
        } else if self.page == Page::Running {
            if self.lifecycle.stage == Stage::Registration {
                "uninstall_registration"
            } else {
                "uninstall_progress_note"
            }
        } else {
            "uninstall_body"
        }
    }

    fn warning_key(&self) -> Option<&'static str> {
        if self.prompt.as_ref().is_some_and(|p| p.files_in_use) {
            Some("close_apps_save_work")
        } else if self.page == Page::Confirm {
            Some(if self.remove_data {
                "uninstall_delete_warning"
            } else {
                "retained_data"
            })
        } else if let Some(result) = &self.result {
            if result.outcome == Outcome::DataMayBeDeleted {
                Some("uninstall_partial_data")
            } else if matches!(result.outcome, Outcome::Success | Outcome::RebootRequired)
                && result.remove_user_data
            {
                Some("uninstall_data_removed")
            } else if !result.remove_user_data
                || (!result.purge_started && result.outcome == Outcome::MsiFailed)
            {
                Some("uninstall_data_kept")
            } else {
                None
            }
        } else {
            None
        }
    }

    fn critical_warning(&self) -> bool {
        (self.page == Page::Confirm && self.remove_data)
            || self
                .result
                .as_ref()
                .is_some_and(|result| result.outcome == Outcome::DataMayBeDeleted)
    }

    fn failure_hint(result: &Completion) -> Option<&'static str> {
        if matches!(
            result.outcome,
            Outcome::Success | Outcome::RebootRequired | Outcome::Cancelled
        ) {
            return None;
        }
        if result.outcome == Outcome::PermissionDenied {
            return Some("error_permission_hint");
        }
        let final_hint = error_hint_key(result.code);
        if result.outcome == Outcome::RegistrationFailed {
            // The MSI transaction already succeeded. Its historical record is
            // diagnostic context, never the cause of this Burn cleanup result.
            return Some(final_hint);
        }
        Some(result.record_code.map_or(final_hint, |code| {
            let hint = error_hint_key(code);
            if hint == "error_generic_hint" {
                final_hint
            } else {
                hint
            }
        }))
    }

    fn apply_preview(&mut self, scenario: Preview) {
        match scenario {
            Preview::Confirm => {}
            Preview::Progress | Preview::Rollback | Preview::FilesInUse => {
                self.page = Page::Running;
                self.lifecycle.progress_record([0, 100, 0, 0]);
                self.lifecycle.progress_record([2, 42, 0, 0]);
                self.lifecycle.common_data(2, 1);
                self.lifecycle.action(if scenario == Preview::Rollback {
                    "Rollback"
                } else {
                    "RecoverAgentState"
                });
                if scenario == Preview::FilesInUse {
                    let (reply, _rx) = std::sync::mpsc::channel();
                    self.prompt = Some(Prompt {
                        items: vec!["Usque".to_owned()],
                        accept: IDOK,
                        decline: IDCANCEL,
                        files_in_use: true,
                        reply,
                    });
                }
            }
            _ => {
                let (outcome, code) = match scenario {
                    Preview::Success => (Outcome::Success, 0),
                    Preview::Reboot | Preview::RestartFailure => (Outcome::RebootRequired, 3010),
                    Preview::Cancelled => (Outcome::Cancelled, 1602),
                    Preview::PartialData => (Outcome::DataMayBeDeleted, 1603),
                    Preview::RegistrationFailure => (Outcome::RegistrationFailed, 1603),
                    _ => (Outcome::MsiFailed, 1603),
                };
                self.finish(Completion {
                    outcome,
                    code,
                    msi_code: if scenario == Preview::RegistrationFailure {
                        0
                    } else {
                        code
                    },
                    record_code: (scenario == Preview::Failure).then_some(1618),
                    purge_started: scenario == Preview::PartialData,
                    remove_user_data: scenario == Preview::PartialData,
                    bundle: None,
                });
                if scenario == Preview::RestartFailure {
                    self.restart = RestartFlow::Failed(5);
                }
            }
        }
    }

    fn finish(&mut self, result: Completion) {
        self.restart = RestartFlow::Idle;
        self.wait_on_close = false;
        self.exit_code = result.code as i32;
        self.page = Page::Finished;
        self.result = Some(result);
        self.pending_registration = None;
        self.prompt = None;
        self.scroll = 0;
    }

    fn start(&mut self, hwnd: HWND) {
        self.restart = RestartFlow::Idle;
        self.wait_on_close = false;
        self.save_code = None;
        self.result = None;
        self.pending_registration = None;
        self.details = false;
        self.scroll = 0;
        self.page = Page::Running;
        self.lifecycle = Lifecycle::default();
        if self.preview.is_some() {
            self.preview_ticks = 1;
            self.lifecycle.common_data(2, 1);
        } else if let Some(product) = &self.product {
            self.operation = Some(msi::start(
                UninstallRequest {
                    product_code: product.clone(),
                    remove_user_data: self.remove_data,
                },
                hwnd,
            ));
        }
    }

    fn primary(&mut self, hwnd: HWND) {
        self.wait_on_close = false;
        if let Some(prompt) = self.prompt.take() {
            let _ = prompt.reply.send(prompt.accept);
            if self.preview.is_some() {
                self.preview_ticks = 1;
            }
        } else if self.page == Page::Confirm {
            self.start(hwnd);
        } else if self.page == Page::Finished
            && let Some(result) = self.result.clone()
        {
            if result.outcome == Outcome::RebootRequired {
                match self.restart {
                    RestartFlow::Confirming => self.restart.back(),
                    RestartFlow::Requested | RestartFlow::Previewed => close_window(hwnd),
                    _ => {
                        self.restart.begin(true);
                        self.details = false;
                        self.scroll = 0;
                    }
                }
            } else if result.outcome == Outcome::RegistrationFailed {
                self.result = None;
                self.pending_registration = Some(result.clone());
                self.page = Page::Running;
                self.lifecycle = Lifecycle::default();
                self.lifecycle.stage = Stage::Registration;
                self.lifecycle.purge_started = result.purge_started;
                self.details = false;
                self.save_code = None;
                self.scroll = 0;
                if self.preview.is_some() {
                    self.preview_ticks = 1;
                } else {
                    self.operation = Some(msi::retry_registration(result));
                }
            } else if result.outcome.can_return_to_confirmation() {
                // A completed MSI attempt can only return to confirmation.
                // Irreversible data deletion is never selected on a retry.
                self.page = Page::Confirm;
                self.remove_data = false;
                self.result = None;
                self.details = false;
                self.scroll = 0;
            } else {
                close_window(hwnd);
            }
        }
    }

    fn secondary(&mut self, hwnd: HWND) {
        if self.page == Page::Finished && self.restart == RestartFlow::Confirming {
            // The confirmation action occupies the other button position. A
            // double-click on the first Restart now button goes Back instead
            // of silently accepting this separate save-work confirmation.
            self.restart
                .confirm(self.preview.is_some(), super::restart::request);
            return;
        }
        if let Some(prompt) = self.prompt.take() {
            if self.lifecycle.purge_started || self.lifecycle.stage == Stage::RollingBack {
                self.wait_on_close = true;
                self.prompt = Some(prompt);
                return;
            }
            let _ = prompt.reply.send(prompt.decline);
            return;
        }
        if self.page != Page::Running {
            close_window(hwnd);
            return;
        }
        if !self.lifecycle.can_cancel() {
            self.wait_on_close = true;
            return;
        }
        if let Some(operation) = &self.operation {
            if !operation.shared.request_cancel() {
                self.wait_on_close = true;
            }
            self.lifecycle = operation.shared.snapshot();
        } else if self.preview.is_some() && self.lifecycle.can_cancel() {
            self.preview_ticks = 0;
            self.finish(Completion {
                outcome: Outcome::Cancelled,
                code: 1602,
                msi_code: 1602,
                record_code: None,
                purge_started: false,
                remove_user_data: self.remove_data,
                bundle: None,
            });
        }
    }

    fn tick(&mut self) {
        if let Some(operation) = &mut self.operation {
            self.lifecycle = operation.shared.snapshot();
            if self.lifecycle.can_cancel() {
                self.wait_on_close = false;
            }
            if self.pending_registration.is_none() {
                self.pending_registration = operation.shared.registration_context();
            }
            if self.prompt.is_none() {
                self.prompt = operation
                    .shared
                    .prompt
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .take();
            }
            match operation.completion.try_recv() {
                Ok(result) => {
                    if let Some(worker) = operation.worker.take() {
                        let _ = worker.join();
                    }
                    self.operation = None;
                    self.finish(result);
                }
                Err(TryRecvError::Disconnected) => {
                    // The worker may have completed MSI after this tick's
                    // first snapshot, then failed during Burn cleanup. Once
                    // disconnected, its last registration context is stable.
                    let previous = self
                        .pending_registration
                        .take()
                        .or_else(|| operation.shared.registration_context());
                    let record_code = *operation
                        .shared
                        .error_code
                        .lock()
                        .unwrap_or_else(|e| e.into_inner());
                    self.operation = None;
                    let result = if let Some(previous) = previous {
                        msi::registration_result(previous, 1)
                    } else {
                        Completion {
                            outcome: if self.lifecycle.purge_started {
                                Outcome::DataMayBeDeleted
                            } else {
                                Outcome::MsiFailed
                            },
                            code: 1,
                            msi_code: 1,
                            record_code,
                            purge_started: self.lifecycle.purge_started,
                            remove_user_data: self.remove_data,
                            bundle: None,
                        }
                    };
                    self.finish(result);
                }
                Err(TryRecvError::Empty) => {}
            }
        } else if self.preview.is_some() && self.preview_ticks > 0 {
            // This timer exists only in the explicitly inert preview. The real
            // installer obtains every progress value from MSI records.
            self.preview_ticks += 1;
            if self.pending_registration.is_none() {
                self.lifecycle.progress_record([0, 30, 0, 0]);
                self.lifecycle
                    .progress_record([2, self.preview_ticks as i32, 0, 0]);
                if self.preview_ticks == 10 {
                    self.lifecycle.action("RecoverAgentState");
                }
            }
            if self.preview_ticks >= 30 {
                self.preview_ticks = 0;
                let result = if let Some(previous) = self.pending_registration.take() {
                    msi::registration_result(previous, 0)
                } else {
                    Completion {
                        outcome: Outcome::Success,
                        code: 0,
                        msi_code: 0,
                        record_code: None,
                        purge_started: false,
                        remove_user_data: self.remove_data,
                        bundle: None,
                    }
                };
                self.finish(result);
            }
        }
    }

    fn details_text(&self) -> String {
        let mut text = format!(
            "Usque {}\r\n{}",
            env!("CARGO_PKG_VERSION"),
            self.text(self.lifecycle.stage.key())
        );
        if let Some(result) = &self.result {
            text.push_str(&format!(
                "\r\n{}\r\n{}: {}",
                self.text(result.outcome.key()),
                self.text("uninstall_error_code"),
                result.code
            ));
            if let Some(code) = result.record_code
                && (code != result.code || result.outcome == Outcome::RegistrationFailed)
            {
                text.push_str(&format!(
                    "\r\nMSI {}: {code}",
                    self.text("uninstall_error_code")
                ));
            }
            if let Some(key) = Self::failure_hint(result) {
                text.push_str(&format!("\r\n{}", self.text(key)));
            }
        }
        if let Some(operation) = &self.operation
            && let Some(code) = *operation
                .shared
                .error_code
                .lock()
                .unwrap_or_else(|e| e.into_inner())
        {
            text.push_str(&format!(
                "\r\nMSI {}: {code}",
                self.text("uninstall_error_code")
            ));
        }
        if let Some(key) = self.restart.message_key() {
            text.push_str(&format!("\r\n{}", self.text(key)));
        }
        if let RestartFlow::Failed(code) = self.restart {
            text.push_str(&format!(
                "\r\n{}: {code}",
                self.text("uninstall_error_code")
            ));
        }
        if let Some(code) = self.save_code {
            text.push_str(&format!(
                "\r\n{} {}",
                self.text("save_details"),
                if code == 0 {
                    self.text("done").to_owned()
                } else {
                    format!("{}: {code}", self.text("uninstall_error_code"))
                }
            ));
        }
        text
    }
}

pub fn run(
    product: Option<String>,
    preview: Option<Preview>,
    requested_locale: Option<&str>,
    preview_theme: Option<PreviewTheme>,
) -> Result<i32, UninstallError> {
    let _accessibility_apartment = super::accessibility::Apartment::enter();
    let class = wide(CLASS);
    // SAFETY: null requests this process's executable module.
    let instance = unsafe { GetModuleHandleW(ptr::null()) };
    let locale = l10n::setup_locale(requested_locale.unwrap_or(&ui_locale_name()));
    let mut state = State::new(product, preview, preview_theme, locale);
    if let Some(scenario) = preview {
        state.apply_preview(scenario);
    }
    let state = RefCell::new(state);
    // SAFETY: the common-controls descriptor and registered class are complete;
    // state stays at this stack address for the entire window lifetime.
    let hwnd = unsafe {
        InitCommonControlsEx(&INITCOMMONCONTROLSEX {
            dwSize: size_of::<INITCOMMONCONTROLSEX>() as u32,
            dwICC: ICC_PROGRESS_CLASS,
        });
        let class_info = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(window_proc),
            hInstance: instance,
            hCursor: LoadCursorW(ptr::null_mut(), IDC_ARROW),
            hIcon: LoadIconW(instance, 1usize as _),
            lpszClassName: class.as_ptr(),
            ..Default::default()
        };
        if RegisterClassExW(&class_info) == 0 {
            return Err(last_error("register uninstall window"));
        }
        let rtl = matches!(locale, "ar-SA" | "fa-IR");
        CreateWindowExW(
            if rtl { WS_EX_LAYOUTRTL } else { 0 },
            class.as_ptr(),
            wide(state.borrow().text("uninstall_title")).as_ptr(),
            WS_OVERLAPPEDWINDOW | WS_CLIPCHILDREN | WS_VSCROLL,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            680,
            480,
            ptr::null_mut(),
            ptr::null_mut(),
            instance,
            ptr::from_ref(&state).cast(),
        )
    };
    if hwnd.is_null() {
        return Err(last_error("create uninstall window"));
    }
    {
        let mut state = state.borrow_mut();
        // SAFETY: hwnd is now live and supports per-monitor DPI queries.
        state.dpi = unsafe { GetDpiForWindow(hwnd) }.max(96);
        state.fonts = fonts(state.dpi);
        refresh_paint_style(&state);
        // SAFETY: the boxed Cell has a stable address until after the native
        // window and all children are destroyed. Paint callbacks only copy it.
        unsafe {
            SetPropW(
                hwnd,
                wide(PAINT_PROPERTY).as_ptr(),
                ptr::from_ref(state.paint.as_ref()).cast_mut().cast(),
            );
        }
        if let Err(error) = create_controls(hwnd, instance, &mut state) {
            // SAFETY: destroy all controls before the stack-owned state/fonts
            // can drop, including on partial control-creation failure.
            unsafe {
                DestroyWindow(hwnd);
                UnregisterClassW(class.as_ptr(), instance);
            }
            return Err(error);
        }
        set_fonts(hwnd, &state);
        apply_layout_direction(hwnd, &state);
        center(hwnd, state.dpi);
        apply_title_theme(hwnd, state.palette.dark);
        apply_progress_theme(hwnd, &state.palette);
        render(hwnd, &mut state);
    }
    // SAFETY: window and button handles belong to this UI thread.
    unsafe {
        SetTimer(hwnd, TIMER, 100, None);
        ShowWindow(hwnd, SW_SHOW);
        UpdateWindow(hwnd);
    }
    {
        let state = state.borrow();
        ensure_focus(hwnd, ptr::null_mut(), &state);
    }
    let mut message = MSG::default();
    loop {
        // SAFETY: message points at writable memory; this is the UI thread.
        let code = unsafe { GetMessageW(&mut message, ptr::null_mut(), 0, 0) };
        if code <= 0 {
            break;
        }
        if message.message == WM_KEYDOWN
            && matches!(message.wParam as u16, VK_F6 | VK_F7 | VK_F8)
            && state.borrow().preview.is_some()
        {
            let mut state = state.borrow_mut();
            preview_shortcut(hwnd, message.wParam as u16, &mut state);
            render(hwnd, &mut state);
            continue;
        }
        // SAFETY: messages are dispatched only while the window is alive.
        unsafe {
            if IsDialogMessageW(hwnd, &message) == 0 {
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
    }
    // SAFETY: WM_DESTROY ended the loop; no window still uses the class.
    unsafe {
        UnregisterClassW(class.as_ptr(), instance);
    }
    Ok(state.borrow().exit_code)
}

extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message == WM_THEMECHANGED {
        // SAFETY: the private property is an opaque presence flag. Filter only
        // synchronous theme-request reentry before touching the State RefCell.
        if !unsafe { GetPropW(hwnd, wide(THEME_REQUEST_PROPERTY).as_ptr()) }.is_null() {
            return 0;
        }
    }
    if message == WM_ENDSESSION && wparam == 0 {
        // SAFETY: queue the cancellation so it is observed after any reentrant
        // ExitWindowsEx call and its current UI-state borrow have returned.
        unsafe {
            PostMessageW(hwnd, WM_RESTART_ABORTED, 0, 0);
        }
        return 0;
    }
    if message == WM_DRAWITEM {
        // SAFETY: an owner-draw control supplies this structure for the call.
        let item = unsafe { &*(lparam as *const DRAWITEMSTRUCT) };
        if let Some(style) = paint_style(hwnd) {
            if item.CtlID == WARNING_ACCENT as u32 {
                draw_warning_accent(item, style);
            } else {
                draw_button(item, style);
            }
            return 1;
        }
    }
    if message == WM_ERASEBKGND
        && let Some(style) = paint_style(hwnd)
    {
        let mut rect = RECT::default();
        // SAFETY: the DC and writable client rect belong to this paint request.
        unsafe {
            GetClientRect(hwnd, &mut rect);
            FillRect(wparam as HDC, &rect, style.brush);
        }
        return 1;
    }
    if matches!(
        message,
        WM_CTLCOLORSTATIC | WM_CTLCOLOREDIT | WM_CTLCOLORBTN
    ) && let Some(style) = paint_style(hwnd)
    {
        // SAFETY: the supplied child DC is live. The independent paint Cell
        // remains readable even during synchronous child-window layout messages.
        unsafe {
            SetTextColor(
                wparam as HDC,
                if GetDlgCtrlID(lparam as HWND) == DESCRIPTION {
                    style.muted
                } else {
                    style.foreground
                },
            );
            SetBkColor(wparam as HDC, style.background);
            SetBkMode(wparam as HDC, TRANSPARENT as i32);
        }
        return style.brush as LRESULT;
    }
    if message == WM_DESTROY {
        // SAFETY: destruction can be reentrant while a button handler borrows
        // state. End the pump without taking a second mutable state borrow.
        unsafe {
            KillTimer(hwnd, TIMER);
            PostQuitMessage(0);
        }
        return 0;
    }
    if message == WM_NCCREATE {
        // SAFETY: WM_NCCREATE supplies this CREATESTRUCTW and its stable state pointer.
        unsafe {
            let created = &*(lparam as *const CREATESTRUCTW);
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, created.lpCreateParams as isize);
        }
    }
    // SAFETY: the pointer is either null or the RefCell kept alive by run().
    let raw = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *const RefCell<State>;
    if !raw.is_null() {
        // SAFETY: only this UI thread accesses the RefCell; try_borrow_mut also
        // protects synchronous reentrant messages from aliasing mutable state.
        let cell = unsafe { &*raw };
        if let Ok(mut state) = cell.try_borrow_mut() {
            match message {
                DM_GETDEFID => {
                    let safe_default = if state.restart == RestartFlow::Confirming {
                        PRIMARY
                    } else {
                        SECONDARY
                    };
                    return ((DC_HASDEFID as usize) << 16 | safe_default as usize) as LRESULT;
                }
                WM_RESTART_ABORTED => {
                    state.restart.cancelled_by_windows();
                    render(hwnd, &mut state);
                    return 0;
                }
                WM_COMMAND => {
                    let notification = ((wparam >> 16) & 0xffff) as u32;
                    if notification == BN_SETFOCUS || notification == EN_SETFOCUS {
                        reveal_focus(hwnd, lparam as HWND, &mut state);
                        return 0;
                    }
                    if notification != BN_CLICKED {
                        return 0;
                    }
                    match (wparam & 0xffff) as i32 {
                        PRIMARY | IDOK => state.primary(hwnd),
                        SECONDARY => state.secondary(hwnd),
                        IDCANCEL => {
                            if state.restart == RestartFlow::Confirming {
                                state.restart.back();
                            } else {
                                state.secondary(hwnd);
                            }
                        }
                        DETAILS => state.details = !state.details,
                        SAVE => state.save_code = save_details(hwnd, &state),
                        CHECK => {
                            // SAFETY: CHECK identifies the native checkbox child.
                            state.remove_data =
                                unsafe { SendMessageW(control(hwnd, CHECK), BM_GETCHECK, 0, 0) }
                                    as u32
                                    == BST_CHECKED;
                        }
                        _ => {}
                    }
                    render(hwnd, &mut state);
                    return 0;
                }
                WM_TIMER => {
                    state.tick();
                    render(hwnd, &mut state);
                    return 0;
                }
                WM_SIZE => {
                    layout(hwnd, &mut state);
                    return 0;
                }
                WM_DPICHANGED => {
                    state.dpi = (wparam & 0xffff) as u32;
                    let updated = fonts(state.dpi);
                    let previous = std::mem::replace(&mut state.fonts, updated);
                    set_fonts(hwnd, &state);
                    refresh_paint_style(&state);
                    drop(previous);
                    // SAFETY: WM_DPICHANGED supplies a suggested window RECT.
                    let rect = unsafe { *(lparam as *const RECT) };
                    // SAFETY: hwnd is live, and the suggested dimensions are bounded by Windows.
                    unsafe {
                        SetWindowPos(
                            hwnd,
                            ptr::null_mut(),
                            rect.left,
                            rect.top,
                            rect.right - rect.left,
                            rect.bottom - rect.top,
                            SWP_NOZORDER | SWP_NOACTIVATE,
                        );
                    }
                    layout(hwnd, &mut state);
                    return 0;
                }
                WM_GETMINMAXINFO => {
                    let work = work_area(hwnd);
                    let (width, height) = window_size(hwnd, state.dpi);
                    // SAFETY: Windows supplies a writable MINMAXINFO pointer.
                    let limits = unsafe { &mut *(lparam as *mut MINMAXINFO) };
                    limits.ptMinTrackSize.x = width.min(work.right - work.left);
                    limits.ptMinTrackSize.y = height.min(work.bottom - work.top);
                    return 0;
                }
                WM_SETTINGCHANGE | WM_SYSCOLORCHANGE | WM_THEMECHANGED => {
                    state.palette = palette(state.preview.is_some(), state.preview_theme);
                    refresh_paint_style(&state);
                    apply_title_theme(hwnd, state.palette.dark);
                    apply_progress_theme(hwnd, &state.palette);
                    // SAFETY: request repaint of this window and its children.
                    unsafe {
                        RedrawWindow(
                            hwnd,
                            ptr::null(),
                            ptr::null_mut(),
                            RDW_INVALIDATE | RDW_ALLCHILDREN,
                        );
                    }
                    return 0;
                }
                WM_VSCROLL => {
                    let code = (wparam & 0xffff) as i32;
                    state.scroll += match code {
                        SB_LINEUP => -28,
                        SB_LINEDOWN => 28,
                        SB_PAGEUP => -180,
                        SB_PAGEDOWN => 180,
                        _ => 0,
                    };
                    if matches!(code, SB_THUMBPOSITION | SB_THUMBTRACK) {
                        state.scroll = ((wparam >> 16) & 0xffff) as i32;
                    }
                    state.scroll = state.scroll.clamp(0, state.max_scroll);
                    layout(hwnd, &mut state);
                    return 0;
                }
                WM_MOUSEWHEEL => {
                    state.scroll = (state.scroll - (((wparam >> 16) as i16 as i32) / 120) * 60)
                        .clamp(0, state.max_scroll);
                    layout(hwnd, &mut state);
                    return 0;
                }
                WM_CTLCOLORSTATIC | WM_CTLCOLOREDIT | WM_CTLCOLORBTN => {
                    let dc = wparam as HDC;
                    // SAFETY: Windows supplies this live DC for child painting.
                    unsafe {
                        SetTextColor(
                            dc,
                            if lparam as HWND == control(hwnd, DESCRIPTION) {
                                state.palette.muted
                            } else {
                                state.palette.foreground
                            },
                        );
                        SetBkColor(dc, state.palette.background);
                        SetBkMode(dc, TRANSPARENT as i32);
                    }
                    return state.palette.brush as LRESULT;
                }
                WM_ERASEBKGND => {
                    let mut rect = RECT::default();
                    // SAFETY: the client RECT and supplied paint DC are valid.
                    unsafe {
                        GetClientRect(hwnd, &mut rect);
                        FillRect(wparam as HDC, &rect, state.palette.brush);
                    }
                    return 1;
                }
                WM_CLOSE => {
                    if state.restart == RestartFlow::Confirming {
                        state.restart.back();
                        close_window(hwnd);
                    } else {
                        state.secondary(hwnd);
                    }
                    render(hwnd, &mut state);
                    return 0;
                }
                WM_DESTROY => {
                    // SAFETY: only a completed operation or confirmation page
                    // may destroy this window; no worker still borrows HWND.
                    unsafe {
                        KillTimer(hwnd, TIMER);
                        PostQuitMessage(0);
                    }
                    return 0;
                }
                _ => {}
            }
        }
    }
    // SAFETY: standard processing of messages not handled by this native window.
    unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
}

fn create_controls(
    hwnd: HWND,
    instance: HINSTANCE,
    state: &mut State,
) -> Result<(), UninstallError> {
    // SAFETY: this native container clips scrolled content above the fixed
    // footer. The subclass forwards child notifications to the owning window.
    let viewport = unsafe {
        let viewport = CreateWindowExW(
            WS_EX_CONTROLPARENT,
            wide("STATIC").as_ptr(),
            wide("").as_ptr(),
            WS_CHILD | WS_VISIBLE | WS_CLIPCHILDREN,
            0,
            0,
            1,
            1,
            hwnd,
            VIEWPORT as isize as _,
            instance,
            ptr::null(),
        );
        if !viewport.is_null() {
            SetWindowSubclass(viewport, Some(viewport_proc), 1, 0);
        }
        viewport
    };
    if viewport.is_null() {
        return Err(last_error("create uninstall content area"));
    }
    for (id, class, style) in [
        (TITLE, "STATIC", 0),
        (BODY, "STATIC", 0),
        (DESCRIPTION, "STATIC", 0),
        (WARNING, "STATIC", 0),
        // An empty native owner-drawn static is decoration only. WARNING keeps
        // its native text, print handling, and accessible name independently.
        (WARNING_ACCENT, "STATIC", STATIC_OWNER_DRAW),
        (STATUS, "STATIC", 0),
        (BADGE, "STATIC", 0),
        (
            CHECK,
            "BUTTON",
            WS_TABSTOP | BS_AUTOCHECKBOX as u32 | BS_MULTILINE as u32 | BS_NOTIFY as u32,
        ),
        (PRIMARY, "BUTTON", WS_TABSTOP | BS_OWNERDRAW as u32),
        (SECONDARY, "BUTTON", WS_TABSTOP | BS_OWNERDRAW as u32),
        (DETAILS, "BUTTON", WS_TABSTOP | BS_OWNERDRAW as u32),
        (SAVE, "BUTTON", WS_TABSTOP | BS_OWNERDRAW as u32),
        (PROGRESS, "msctls_progress32", PBS_SMOOTH),
        (
            DETAIL,
            "EDIT",
            WS_TABSTOP
                | WS_VSCROLL
                | ES_MULTILINE as u32
                | ES_READONLY as u32
                | ES_AUTOVSCROLL as u32,
        ),
    ] {
        // SAFETY: strings outlive the call; IDs are unique within this parent.
        let child = unsafe {
            CreateWindowExW(
                0,
                wide(class).as_ptr(),
                wide("").as_ptr(),
                WS_CHILD | WS_VISIBLE | style,
                0,
                0,
                1,
                1,
                if matches!(id, PRIMARY | SECONDARY | DETAILS) {
                    hwnd
                } else {
                    viewport
                },
                id as isize as _,
                instance,
                ptr::null(),
            )
        };
        if child.is_null() {
            return Err(last_error("create uninstall control"));
        }
        state.controls.push(child);
        if id == CHECK {
            // SAFETY: the stable paint Cell lives longer than this checkbox.
            unsafe {
                SetWindowSubclass(
                    child,
                    Some(checkbox_proc),
                    2,
                    ptr::from_ref(state.paint.as_ref()) as usize,
                );
            }
        } else if id == PROGRESS {
            // SAFETY: only the native progress border is overpainted; its
            // value, animation, messages, and accessibility remain native.
            unsafe {
                SetWindowSubclass(
                    child,
                    Some(progress_proc),
                    3,
                    ptr::from_ref(state.paint.as_ref()) as usize,
                );
            }
        }
    }
    Ok(())
}

fn render(hwnd: HWND, state: &mut State) {
    // Keep a valid existing focus across state updates; disabling a focused
    // Cancel button must not strand keyboard input on a null/disabled HWND.
    // SAFETY: GetFocus returns this UI thread's borrowed focus handle.
    let previous_focus = unsafe { GetFocus() };
    refresh_paint_style(state);
    if state.preview.is_some() {
        let caption = format!("{} — Preview · F6 / F7 / F8", state.text("uninstall_title"));
        if window_text(hwnd) != caption {
            // SAFETY: caption is nul-terminated for the live preview window.
            unsafe {
                SetWindowTextW(hwnd, wide(&caption).as_ptr());
            }
        }
    }
    let confirming = state.page == Page::Confirm;
    let running = state.page == Page::Running;
    let success = state
        .result
        .as_ref()
        .is_some_and(|r| matches!(r.outcome, Outcome::Success | Outcome::RebootRequired));
    let reboot = state
        .result
        .as_ref()
        .is_some_and(|r| r.outcome == Outcome::RebootRequired);
    let restart_terminal = matches!(
        state.restart,
        RestartFlow::Requested | RestartFlow::Previewed
    );
    let title_key = if let Some(result) = &state.result {
        result.outcome.title_key()
    } else if running {
        "uninstall_running_title"
    } else {
        "uninstall_title"
    };
    set_text(hwnd, TITLE, state.text(title_key));
    set_text(hwnd, PROGRESS, state.text(state.lifecycle.stage.key()));
    set_text(
        hwnd,
        BADGE,
        if state.preview.is_some() {
            state.text("uninstall_preview")
        } else {
            "Usque"
        },
    );
    let mut body = state.text(state.body_key()).to_owned();
    if let Some(result) = &state.result
        && let Some(key) = State::failure_hint(result)
    {
        body.push_str("\r\n\r\n");
        body.push_str(state.text(key));
    }
    set_text(hwnd, BODY, &body);
    set_text(hwnd, CHECK, state.text("uninstall_delete_data"));
    set_text(hwnd, DESCRIPTION, state.text("uninstall_data_description"));
    let warning = state.warning_key().map_or("", |key| state.text(key));
    set_text(hwnd, WARNING, warning);
    set_text(
        hwnd,
        STATUS,
        state.text(if state.lifecycle.cancel_requested {
            "uninstall_stopping"
        } else {
            state.lifecycle.stage.key()
        }),
    );
    let primary = if reboot {
        if state.restart == RestartFlow::Confirming {
            "back"
        } else if restart_terminal {
            "close"
        } else {
            "restart_now"
        }
    } else if let Some(prompt) = &state.prompt {
        if prompt.files_in_use && prompt.accept == IDOK {
            "uninstall_close_apps"
        } else {
            "uninstall_check_again"
        }
    } else if confirming {
        if state.remove_data {
            "uninstall_delete_action"
        } else {
            "uninstall_action"
        }
    } else if state
        .result
        .as_ref()
        .is_some_and(|r| r.outcome == Outcome::RegistrationFailed)
    {
        "retry"
    } else if state
        .result
        .as_ref()
        .is_some_and(|r| r.outcome.can_return_to_confirmation())
    {
        "back"
    } else {
        "close"
    };
    set_text(hwnd, PRIMARY, state.text(primary));
    set_text(
        hwnd,
        SECONDARY,
        state.text(if state.restart == RestartFlow::Confirming {
            "restart_now"
        } else if reboot {
            "restart_later"
        } else if state.page == Page::Finished {
            "close"
        } else {
            "cancel"
        }),
    );
    set_text(
        hwnd,
        DETAILS,
        state.text(if state.details {
            "hide_details"
        } else {
            "details"
        }),
    );
    set_text(hwnd, SAVE, state.text("save_details"));
    let detail = if let Some(prompt) = &state.prompt {
        prompt.items.join("\r\n")
    } else {
        state.details_text()
    };
    set_text(hwnd, DETAIL, &detail);
    let detail_name = state.text(
        if state
            .prompt
            .as_ref()
            .is_some_and(|prompt| prompt.files_in_use)
        {
            "uninstall_files_in_use"
        } else {
            "details"
        },
    );
    if state.detail_accessible_name != detail_name {
        super::accessibility::name(control(hwnd, DETAIL), detail_name);
        state.detail_accessible_name = detail_name;
        // SAFETY: the readonly edit is a live child of this UI window. Notify
        // only on an actual label/locale change, not on every progress tick.
        unsafe {
            NotifyWinEvent(
                EVENT_OBJECT_NAMECHANGE,
                control(hwnd, DETAIL),
                OBJID_CLIENT,
                0,
            );
        }
    }
    // SAFETY: each child ID is owned by hwnd. Progress values are bounded by the
    // pure decoder, and indeterminate progress has no fabricated percentage.
    unsafe {
        SendMessageW(
            control(hwnd, CHECK),
            BM_SETCHECK,
            if state.remove_data {
                BST_CHECKED
            } else {
                BST_UNCHECKED
            } as usize,
            0,
        );
        for id in [CHECK, DESCRIPTION] {
            ShowWindow(
                control(hwnd, id),
                if confirming { SW_SHOW } else { SW_HIDE },
            );
        }
        ShowWindow(
            control(hwnd, WARNING_ACCENT),
            if state.critical_warning() {
                SW_SHOW
            } else {
                SW_HIDE
            },
        );
        ShowWindow(
            control(hwnd, STATUS),
            if running { SW_SHOW } else { SW_HIDE },
        );
        ShowWindow(
            control(hwnd, PROGRESS),
            if running && state.prompt.is_none() {
                SW_SHOW
            } else {
                SW_HIDE
            },
        );
        ShowWindow(
            control(hwnd, PRIMARY),
            if !running || state.prompt.is_some() {
                SW_SHOW
            } else {
                SW_HIDE
            },
        );
        ShowWindow(
            control(hwnd, SECONDARY),
            if (success && !reboot) || restart_terminal {
                SW_HIDE
            } else {
                SW_SHOW
            },
        );
        EnableWindow(
            control(hwnd, SECONDARY),
            i32::from(
                !running
                    || (state.prompt.is_some()
                        && !state.lifecycle.purge_started
                        && state.lifecycle.stage != Stage::RollingBack)
                    || state.lifecycle.can_cancel(),
            ),
        );
        ShowWindow(
            control(hwnd, DETAILS),
            if confirming || state.prompt.is_some() {
                SW_HIDE
            } else {
                SW_SHOW
            },
        );
        ShowWindow(
            control(hwnd, SAVE),
            if state.details && !confirming && state.preview.is_none() {
                SW_SHOW
            } else {
                SW_HIDE
            },
        );
        ShowWindow(
            control(hwnd, DETAIL),
            if state.details || state.prompt.is_some() {
                SW_SHOW
            } else {
                SW_HIDE
            },
        );
        let bar = control(hwnd, PROGRESS);
        let percent = if state.lifecycle.stage == Stage::Registration {
            None
        } else {
            state.lifecycle.progress.percent()
        };
        let style = GetWindowLongW(bar, GWL_STYLE) as u32;
        SetWindowLongW(
            bar,
            GWL_STYLE,
            if percent.is_some() {
                style & !PBS_MARQUEE
            } else {
                style | PBS_MARQUEE
            } as i32,
        );
        SendMessageW(
            bar,
            PBM_SETMARQUEE,
            usize::from(running && state.prompt.is_none() && percent.is_none()),
            35,
        );
        SendMessageW(bar, PBM_SETRANGE32, 0, 100);
        if let Some(percent) = percent {
            SendMessageW(bar, PBM_SETPOS, percent as usize, 0);
        }
    }
    layout(hwnd, state);
    ensure_focus(hwnd, previous_focus, state);
}

fn ensure_focus(hwnd: HWND, previous: HWND, state: &State) {
    // SAFETY: all candidates are native controls owned by this live window.
    // Background timer updates must never activate the app or steal focus.
    unsafe {
        if GetForegroundWindow() != hwnd || IsWindowVisible(hwnd) == 0 {
            return;
        }
        let valid = |candidate: HWND| {
            !candidate.is_null()
                && (candidate == hwnd || IsChild(hwnd, candidate) != 0)
                && IsWindowVisible(candidate) != 0
                && IsWindowEnabled(candidate) != 0
        };
        if valid(previous) {
            if GetFocus() != previous {
                SetFocus(previous);
            }
            return;
        }
        if valid(GetFocus()) {
            return;
        }
        let candidates = if state.prompt.is_some() {
            [PRIMARY, SECONDARY, DETAIL]
        } else {
            match state.page {
                Page::Confirm => [SECONDARY, CHECK, PRIMARY],
                Page::Running => [DETAILS, SECONDARY, PRIMARY],
                Page::Finished => [PRIMARY, DETAILS, SECONDARY],
            }
        };
        for id in candidates {
            let candidate = control(hwnd, id);
            if valid(candidate) {
                SetFocus(candidate);
                return;
            }
        }
        SetFocus(hwnd);
    }
}

#[derive(Debug)]
struct FooterButton {
    id: i32,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
}

/// Keep actions inside the work area and reserve their measured text height.
/// The primary button stays in the last position when restart confirmation
/// swaps button meanings, preserving the distinct-click safety boundary.
fn footer_positions(
    (width, height): (i32, i32),
    margin: i32,
    gap: i32,
    padding: i32,
    stacked: bool,
    captions: [(i32, bool, i32); 3],
) -> (i32, Vec<FooterButton>) {
    let action_height = captions[1].2.max(captions[2].2);
    let row_height = captions[0].2.max(action_height);
    let visible_count = captions.iter().filter(|(_, visible, _)| *visible).count() as i32;
    let footer_height = if stacked {
        captions
            .iter()
            .filter(|(_, visible, _)| *visible)
            .map(|(id, _, h)| if *id == DETAILS { *h } else { action_height })
            .sum::<i32>()
            + gap * (visible_count - 1).max(0)
            + padding * 2
    } else {
        row_height + padding * 2
    };
    let button_width = if stacked {
        width - margin * 2
    } else {
        (width - margin * 2 - gap * 2) / 3
    }
    .max(1);
    let mut y = height - footer_height + padding;
    let buttons = captions
        .into_iter()
        .enumerate()
        .filter_map(|(index, (id, visible, h))| {
            if !visible {
                return None;
            }
            let h = if id == DETAILS { h } else { action_height };
            let button = FooterButton {
                id,
                x: if stacked {
                    margin
                } else {
                    margin + index as i32 * (button_width + gap)
                },
                y: if stacked {
                    y
                } else {
                    height - padding - row_height + (row_height - h) / 2
                },
                width: button_width,
                height: h,
            };
            if stacked {
                y += h + gap;
            }
            Some(button)
        })
        .collect();
    (footer_height, buttons)
}

fn layout(hwnd: HWND, state: &mut State) {
    let mut rect = RECT::default();
    // SAFETY: hwnd is a live window and rect is writable.
    unsafe {
        GetClientRect(hwnd, &mut rect);
    }
    let scale = |value: i32| ((i64::from(value) * i64::from(state.dpi)) / 96) as i32;
    let width = rect.right.max(1);
    let height = rect.bottom.max(1);
    let margin = scale(32).min(width / 8);
    let body_width = (width - 2 * margin).max(40);
    let stacked = body_width < scale(520);
    let button_width = if stacked {
        body_width
    } else {
        (body_width - scale(20)) / 3
    };
    let captions = [DETAILS, SECONDARY, PRIMARY].map(|id| {
        let child = control(hwnd, id);
        // SAFETY: these are the live native footer controls.
        let visible = unsafe { GetWindowLongW(child, GWL_STYLE) } as u32 & WS_VISIBLE != 0;
        let minimum = scale(if id == DETAILS { 36 } else { 48 });
        (
            id,
            visible,
            (measure(
                hwnd,
                child,
                state.fonts.body,
                (button_width - scale(20)).max(1),
            ) + scale(12))
            .max(minimum),
        )
    });
    let (footer, footer_buttons) = footer_positions(
        (width, height),
        margin,
        scale(10),
        scale(20),
        stacked,
        captions,
    );
    let viewport = (height - footer).max(1);
    let mut y = scale(24);
    let mut items = Vec::new();
    for id in [
        BADGE,
        TITLE,
        BODY,
        CHECK,
        DESCRIPTION,
        WARNING,
        STATUS,
        PROGRESS,
        DETAIL,
        SAVE,
    ] {
        // SAFETY: a child handle exists for each ID.
        let child = control(hwnd, id);
        // SAFETY: visibility query does not mutate the native control.
        if unsafe { GetWindowLongW(child, GWL_STYLE) } as u32 & WS_VISIBLE == 0 {
            continue;
        }
        let h = match id {
            PROGRESS => scale(8),
            DETAIL => scale(112),
            SAVE => scale(36),
            CHECK => measure(
                hwnd,
                child,
                state.fonts.body,
                (body_width - scale(34)).max(30),
            )
            .max(scale(44)),
            WARNING if state.critical_warning() => {
                measure(
                    hwnd,
                    child,
                    state.fonts.body,
                    (body_width - scale(24)).max(1),
                ) + scale(16)
            }
            _ => measure(
                hwnd,
                child,
                if id == TITLE {
                    state.fonts.title
                } else if id == DESCRIPTION {
                    state.fonts.secondary
                } else {
                    state.fonts.body
                },
                body_width,
            )
            .max(scale(18)),
        };
        let child_width = if id == SAVE {
            (text_width(hwnd, child, state.fonts.body) + scale(24)).min(body_width)
        } else if id == WARNING && state.critical_warning() {
            (body_width - scale(24)).max(1)
        } else {
            body_width
        };
        if id == WARNING && state.critical_warning() {
            // Both controls use parent coordinates, so Windows mirrors their
            // geometry together for RTL. Text itself remains a native STATIC.
            items.push((
                child,
                margin + scale(16),
                y + scale(8),
                (h - scale(16)).max(1),
                child_width,
            ));
            items.push((control(hwnd, WARNING_ACCENT), margin, y, h, scale(3).max(1)));
        } else {
            items.push((child, margin, y, h, child_width));
        }
        y += h + scale(match id {
            TITLE | BODY => 20,
            CHECK => 8,
            _ => 12,
        });
    }
    state.max_scroll = (y - viewport).max(0);
    state.scroll = state.scroll.clamp(0, state.max_scroll);
    // SAFETY: native scrollbar state and child dimensions are bounded integers.
    unsafe {
        let info = SCROLLINFO {
            cbSize: size_of::<SCROLLINFO>() as u32,
            fMask: SIF_RANGE | SIF_PAGE | SIF_POS,
            nMin: 0,
            nMax: y.max(1) - 1,
            nPage: viewport as u32,
            nPos: state.scroll,
            nTrackPos: 0,
        };
        SetScrollInfo(hwnd, SB_VERT, &info, 1);
        MoveWindow(GetDlgItem(hwnd, VIEWPORT), 0, 0, width, viewport, 1);
        for (child, left, top, h, child_width) in items {
            MoveWindow(child, left, top - state.scroll, child_width, h, 1);
        }
        for button in footer_buttons {
            MoveWindow(
                control(hwnd, button.id),
                button.x,
                button.y,
                button.width,
                button.height,
                1,
            );
        }
    }
}

fn control(hwnd: HWND, id: i32) -> HWND {
    // SAFETY: every requested ID belongs either to the fixed footer or to the
    // content container. GetDlgItem returns null if creation has not finished.
    unsafe {
        if matches!(id, PRIMARY | SECONDARY | DETAILS | VIEWPORT) {
            GetDlgItem(hwnd, id)
        } else {
            GetDlgItem(GetDlgItem(hwnd, VIEWPORT), id)
        }
    }
}

fn apply_layout_direction(hwnd: HWND, state: &State) {
    let rtl = matches!(state.locale, "ar-SA" | "fa-IR");
    // SAFETY: every HWND belongs to this UI. Changing a parent's layout does
    // not update children created earlier, so update each existing child too.
    unsafe {
        for window in [hwnd, control(hwnd, VIEWPORT)]
            .into_iter()
            .chain(state.controls.iter().copied())
        {
            let id = GetDlgCtrlID(window);
            let static_text = matches!(id, TITLE | BODY | DESCRIPTION | WARNING | STATUS | BADGE);
            let old = GetWindowLongW(window, GWL_EXSTYLE) as u32;
            let mut extended = old & !(WS_EX_LAYOUTRTL | WS_EX_RTLREADING | WS_EX_RIGHT);
            if rtl && id != DETAIL {
                extended |= WS_EX_RTLREADING;
                // Static text uses an unmirrored DC with explicit right
                // alignment. This avoids reversing its alignment twice.
                if !static_text {
                    extended |= WS_EX_LAYOUTRTL;
                }
            }
            SetWindowLongW(window, GWL_EXSTYLE, extended as i32);
            if static_text {
                let style = GetWindowLongW(window, GWL_STYLE) as u32;
                SetWindowLongW(
                    window,
                    GWL_STYLE,
                    ((style & !STATIC_TYPE_MASK) | if rtl { STATIC_RIGHT } else { 0 }) as i32,
                );
            } else if id == DETAIL {
                // Diagnostic codes and file paths retain their LTR ordering.
                let style = GetWindowLongW(window, GWL_STYLE) as u32;
                SetWindowLongW(
                    window,
                    GWL_STYLE,
                    (style & !((ES_RIGHT | ES_CENTER) as u32)) as i32,
                );
            }
            SetWindowPos(
                window,
                ptr::null_mut(),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED,
            );
            InvalidateRect(window, ptr::null(), 1);
        }
    }
}

fn reveal_focus(hwnd: HWND, child: HWND, state: &mut State) {
    let viewport = control(hwnd, VIEWPORT);
    // SAFETY: focus notifications supply the notifying child handle.
    unsafe {
        if GetParent(child) != viewport {
            return;
        }
        let mut rect = RECT::default();
        let mut view = RECT::default();
        GetWindowRect(child, &mut rect);
        GetWindowRect(viewport, &mut view);
        if rect.top < view.top {
            state.scroll = (state.scroll - (view.top - rect.top)).max(0);
        }
        if rect.bottom > view.bottom {
            state.scroll = (state.scroll + rect.bottom - view.bottom).min(state.max_scroll);
        }
    }
    layout(hwnd, state);
}

unsafe extern "system" fn viewport_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    _data: usize,
) -> LRESULT {
    if matches!(
        message,
        WM_COMMAND
            | WM_DRAWITEM
            | WM_CTLCOLORSTATIC
            | WM_CTLCOLOREDIT
            | WM_CTLCOLORBTN
            | WM_ERASEBKGND
            | WM_MOUSEWHEEL
    ) {
        // SAFETY: the viewport is a child of the wizard for its full lifetime.
        unsafe { SendMessageW(GetParent(hwnd), message, wparam, lparam) }
    } else {
        // SAFETY: pass unhandled native-container messages to the subclass chain.
        unsafe { DefSubclassProc(hwnd, message, wparam, lparam) }
    }
}

fn set_text(parent: HWND, id: i32, text: &str) {
    // SAFETY: child is owned by this parent. Only changed accessible text is
    // announced, preventing a screen reader from repeating every timer tick.
    unsafe {
        let child = control(parent, id);
        if window_text(child) != text {
            SetWindowTextW(child, wide(text).as_ptr());
            if id == PROGRESS {
                super::accessibility::name(child, text);
            }
            if matches!(id, TITLE | BODY | STATUS | PROGRESS) {
                NotifyWinEvent(EVENT_OBJECT_NAMECHANGE, child, OBJID_CLIENT, 0);
            }
        }
    }
}

fn window_text(hwnd: HWND) -> String {
    // SAFETY: hwnd is a UI-thread native control; reads use a bounded buffer.
    unsafe {
        let len = GetWindowTextLengthW(hwnd).clamp(0, 8192);
        let mut text = vec![0u16; len as usize + 1];
        let copied = GetWindowTextW(hwnd, text.as_mut_ptr(), text.len() as i32);
        String::from_utf16_lossy(&text[..copied.max(0) as usize])
    }
}

fn measure(hwnd: HWND, child: HWND, font: HFONT, width: i32) -> i32 {
    let text = wide(&window_text(child));
    let mut rect = RECT {
        left: 0,
        top: 0,
        right: width,
        bottom: 0,
    };
    // SAFETY: select only a live font, then restore the borrowed DC before release.
    unsafe {
        let dc = GetDC(hwnd);
        let old = SelectObject(dc, font);
        let reading = if GetWindowLongW(child, GWL_EXSTYLE) as u32 & WS_EX_RTLREADING != 0 {
            DT_RTLREADING
        } else {
            0
        };
        DrawTextW(
            dc,
            text.as_ptr(),
            -1,
            &mut rect,
            DT_CALCRECT | DT_WORDBREAK | DT_NOPREFIX | reading,
        );
        SelectObject(dc, old);
        ReleaseDC(hwnd, dc);
    }
    rect.bottom.max(1)
}

fn text_width(hwnd: HWND, child: HWND, font: HFONT) -> i32 {
    let text = wide(&window_text(child));
    let mut rect = RECT::default();
    // SAFETY: measure a single line with the live font, restoring the borrowed
    // DC before release. The caller bounds the result to the available width.
    unsafe {
        let dc = GetDC(hwnd);
        let old = SelectObject(dc, font);
        DrawTextW(
            dc,
            text.as_ptr(),
            -1,
            &mut rect,
            DT_CALCRECT | DT_SINGLELINE | DT_NOPREFIX,
        );
        SelectObject(dc, old);
        ReleaseDC(hwnd, dc);
    }
    rect.right.max(1)
}

fn fonts(dpi: u32) -> Fonts {
    let make = |height: i32, weight: u32| {
        // SAFETY: the face name is nul-terminated and CreateFontW returns an owned font.
        unsafe {
            CreateFontW(
                -(height * dpi as i32 / 96),
                0,
                0,
                0,
                weight as i32,
                0,
                0,
                0,
                DEFAULT_CHARSET as u32,
                OUT_DEFAULT_PRECIS as u32,
                CLIP_DEFAULT_PRECIS as u32,
                CLEARTYPE_QUALITY as u32,
                DEFAULT_PITCH as u32,
                wide("Segoe UI").as_ptr(),
            )
        }
    };
    Fonts {
        body: make(15, FW_NORMAL),
        secondary: make(13, FW_NORMAL),
        title: make(28, FW_SEMIBOLD),
    }
}

fn set_fonts(hwnd: HWND, state: &State) {
    // SAFETY: all handles are UI-thread controls; fonts outlive their selection.
    unsafe {
        for child in &state.controls {
            SendMessageW(
                *child,
                WM_SETFONT,
                if *child == control(hwnd, TITLE) {
                    state.fonts.title
                } else if *child == control(hwnd, DESCRIPTION) {
                    state.fonts.secondary
                } else {
                    state.fonts.body
                } as usize,
                1,
            );
        }
    }
}

fn palette(preview: bool, preview_theme: Option<PreviewTheme>) -> Palette {
    let mut high = HIGHCONTRASTW {
        cbSize: size_of::<HIGHCONTRASTW>() as u32,
        ..Default::default()
    };
    // SAFETY: HIGHCONTRASTW has the required size and writable storage.
    unsafe {
        SystemParametersInfoW(
            SPI_GETHIGHCONTRAST,
            high.cbSize,
            ptr::from_mut(&mut high).cast(),
            0,
        );
    }
    let high_contrast = high.dwFlags & HCF_HIGHCONTRASTON != 0;
    let mut light = 1u32;
    if !preview && !high_contrast {
        let mut size = size_of::<u32>() as u32;
        // SAFETY: read only the OS app-theme DWORD into its exact-size buffer.
        // Preview deliberately never reads the registry or installed state.
        unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                wide("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize").as_ptr(),
                wide("AppsUseLightTheme").as_ptr(),
                RRF_RT_REG_DWORD,
                ptr::null_mut(),
                ptr::from_mut(&mut light).cast(),
                &mut size,
            );
        }
    }
    let dark = !high_contrast && (preview_theme == Some(PreviewTheme::Dark) || light == 0);
    let (background, foreground, muted) = if preview_theme == Some(PreviewTheme::HighContrast) {
        (0, 0x00ffffff, 0x00ffffff)
    } else if high_contrast {
        // SAFETY: color indices are documented system-color constants.
        unsafe {
            (
                GetSysColor(COLOR_WINDOW),
                GetSysColor(COLOR_WINDOWTEXT),
                GetSysColor(COLOR_WINDOWTEXT),
            )
        }
    } else if dark {
        (0x00100e0e, 0x00e9e8e8, 0x009fa5a7)
    } else {
        (0x00f1f4f5, 0x00181b1c, 0x0061686b)
    };
    // SAFETY: creates an owned brush for the chosen COLORREF.
    let brush = unsafe { CreateSolidBrush(background) };
    Palette {
        background,
        foreground,
        muted,
        brush,
        dark,
        high_contrast: high_contrast || preview_theme == Some(PreviewTheme::HighContrast),
        native_high_contrast: high_contrast,
    }
}

fn apply_title_theme(hwnd: HWND, dark: bool) {
    let value = i32::from(dark);
    // SAFETY: a BOOL-sized value is passed for this documented DWM attribute.
    unsafe {
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_USE_IMMERSIVE_DARK_MODE as u32,
            ptr::from_ref(&value).cast(),
            size_of::<i32>() as u32,
        );
    }
}

fn apply_progress_theme(hwnd: HWND, palette: &Palette) {
    let bar = control(hwnd, PROGRESS);
    let empty = [0u16];
    let native_theme = wide(if palette.dark {
        "DarkMode_Explorer"
    } else {
        "Explorer"
    });
    // SetWindowTheme sends WM_THEMECHANGED synchronously. This guard is
    // independent of State and is removed even if this scope unwinds. If the
    // property cannot be installed, leave the main window's system theme alone.
    let theme_request = NativeThemeRequest::begin(hwnd);
    // SAFETY: selecting the main window and edit/scrollbar themes affects only
    // appearance. High contrast retains its system theme and all native input
    // semantics stay unchanged. Rejected theme requests restore OS defaults.
    unsafe {
        for window in [hwnd, control(hwnd, DETAIL)] {
            if window == hwnd && theme_request.is_none() {
                continue;
            }
            if SetWindowTheme(
                window,
                if palette.high_contrast {
                    ptr::null()
                } else {
                    native_theme.as_ptr()
                },
                ptr::null(),
            ) < 0
            {
                SetWindowTheme(window, ptr::null(), ptr::null());
            }
        }
    }
    // SAFETY: only the native progress control's colors/theme are changed.
    // Visual styles otherwise ignore PBM_SETBARCOLOR/PBM_SETBKCOLOR. The
    // native control continues to own its range, position and marquee timer.
    unsafe {
        if palette.native_high_contrast {
            SetWindowTheme(bar, ptr::null(), ptr::null());
            SendMessageW(bar, PBM_SETBARCOLOR, 0, CLR_DEFAULT as isize);
            SendMessageW(bar, PBM_SETBKCOLOR, 0, CLR_DEFAULT as isize);
        } else {
            SetWindowTheme(bar, empty.as_ptr(), empty.as_ptr());
            let fill = if palette.high_contrast {
                GetSysColor(COLOR_HIGHLIGHT)
            } else {
                brand_accent(palette.dark)
            };
            let track = if palette.high_contrast {
                palette.background
            } else if palette.dark {
                0x00262424
            } else {
                0x00e9eced
            };
            SendMessageW(bar, PBM_SETBARCOLOR, 0, fill as isize);
            SendMessageW(bar, PBM_SETBKCOLOR, 0, track as isize);
        }
        InvalidateRect(bar, ptr::null(), 1);
    }
}

unsafe extern "system" fn progress_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    data: usize,
) -> LRESULT {
    // SAFETY: every message first reaches the original native progress
    // implementation. The stable paint Cell outlives this child window.
    let result = unsafe { DefSubclassProc(hwnd, message, wparam, lparam) };
    // SAFETY: create_controls supplied this Cell, owned until window teardown.
    let style = unsafe { (*(data as *const Cell<PaintStyle>)).get() };
    if message == WM_PAINT && !style.high_contrast {
        // SAFETY: paint only a subtle frame after the native control has drawn
        // its real position or marquee; no progress value is synthesized.
        unsafe {
            let dc = GetDC(hwnd);
            let mut rect = RECT::default();
            GetClientRect(hwnd, &mut rect);
            let border = CreateSolidBrush(if style.dark { 0x00434141 } else { 0x00d0d4d6 });
            for _ in 0..(style.dpi / 96).max(1) {
                FrameRect(dc, &rect, border);
                InflateRect(&mut rect, -1, -1);
            }
            DeleteObject(border);
            ReleaseDC(hwnd, dc);
        }
    }
    result
}

fn center(hwnd: HWND, dpi: u32) {
    let work = work_area(hwnd);
    let (width, height) = window_size(hwnd, dpi);
    // SAFETY: window geometry is limited to the current monitor's work area.
    unsafe {
        let width = width.min((work.right - work.left).max(1));
        let height = height.min((work.bottom - work.top).max(1));
        SetWindowPos(
            hwnd,
            ptr::null_mut(),
            work.left + (work.right - work.left - width) / 2,
            work.top + (work.bottom - work.top - height) / 2,
            width,
            height,
            SWP_NOZORDER | SWP_NOACTIVATE,
        );
    }
}

fn window_size(hwnd: HWND, dpi: u32) -> (i32, i32) {
    let mut rect = RECT {
        right: 680 * dpi as i32 / 96,
        bottom: 480 * dpi as i32 / 96,
        ..Default::default()
    };
    // SAFETY: calculate the native frame around a 680 by 480 logical client
    // area. Callers then clamp it to the monitor work area; content can scroll.
    unsafe {
        AdjustWindowRectExForDpi(
            &mut rect,
            GetWindowLongW(hwnd, GWL_STYLE) as u32,
            0,
            GetWindowLongW(hwnd, GWL_EXSTYLE) as u32,
            dpi,
        );
    }
    (rect.right - rect.left, rect.bottom - rect.top)
}

fn work_area(hwnd: HWND) -> RECT {
    let mut monitor = MONITORINFO {
        cbSize: size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    // SAFETY: nearest-monitor query and output structure are valid.
    unsafe {
        if GetMonitorInfoW(
            MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST),
            &mut monitor,
        ) != 0
        {
            return monitor.rcWork;
        }
    }
    RECT {
        left: 0,
        top: 0,
        right: 680,
        bottom: 480,
    }
}

fn save_details(hwnd: HWND, state: &State) -> Option<i32> {
    if state.preview.is_some() {
        return None;
    }
    use windows_sys::Win32::UI::Controls::Dialogs::{
        GetSaveFileNameW, OFN_NOCHANGEDIR, OFN_OVERWRITEPROMPT, OFN_PATHMUSTEXIST, OPENFILENAMEW,
    };
    let mut path = [0u16; 32_768];
    let name = wide("Usque-uninstall-details.txt");
    path[..name.len()].copy_from_slice(&name);
    let filter = wide("Text files (*.txt)\0*.txt\0\0");
    let extension = wide("txt");
    let mut dialog = OPENFILENAMEW {
        lStructSize: size_of::<OPENFILENAMEW>() as u32,
        hwndOwner: hwnd,
        lpstrFilter: filter.as_ptr(),
        lpstrFile: path.as_mut_ptr(),
        nMaxFile: path.len() as u32,
        lpstrDefExt: extension.as_ptr(),
        Flags: OFN_NOCHANGEDIR | OFN_OVERWRITEPROMPT | OFN_PATHMUSTEXIST,
        ..Default::default()
    };
    // SAFETY: all dialog string buffers outlive the synchronous call.
    if unsafe { GetSaveFileNameW(&mut dialog) } != 0 {
        let length = path.iter().position(|unit| *unit == 0).unwrap_or(0);
        if let Ok(destination) = String::from_utf16(&path[..length]) {
            // Only fixed labels, version and numeric status are exported. No
            // product paths, account data, registry values, or MSI records.
            return Some(match std::fs::write(destination, state.details_text()) {
                Ok(()) => 0,
                Err(error) => error.raw_os_error().unwrap_or(1),
            });
        }
    }
    None
}

fn close_window(hwnd: HWND) {
    // SAFETY: callers only close after an operation returns or before it starts.
    unsafe {
        DestroyWindow(hwnd);
    }
}

fn refresh_paint_style(state: &State) {
    state.paint.set(PaintStyle {
        dpi: state.dpi,
        background: state.palette.background,
        foreground: state.palette.foreground,
        muted: state.palette.muted,
        brush: state.palette.brush,
        font: state.fonts.body,
        dark: state.palette.dark,
        high_contrast: state.palette.high_contrast,
        native_high_contrast: state.palette.native_high_contrast,
        accent_button: if state.restart == RestartFlow::Confirming {
            SECONDARY as u32
        } else {
            PRIMARY as u32
        },
        rtl: matches!(state.locale, "ar-SA" | "fa-IR"),
    });
}

fn paint_style(hwnd: HWND) -> Option<PaintStyle> {
    // SAFETY: run installs this property using a stable boxed Cell and keeps it
    // alive until this window and its children have been destroyed.
    let pointer =
        unsafe { GetPropW(hwnd, wide(PAINT_PROPERTY).as_ptr()) } as *const Cell<PaintStyle>;
    if pointer.is_null() {
        None
    } else {
        // SAFETY: the property is private to this window, on its UI thread.
        Some(unsafe { (*pointer).get() })
    }
}

fn draw_button(item: &DRAWITEMSTRUCT, style: PaintStyle) {
    let enabled = item.itemState & ODS_DISABLED == 0;
    let accent = item.CtlID == style.accent_button && enabled;
    let auxiliary = matches!(item.CtlID as i32, DETAILS | SAVE);
    let scale = |value: i32| value * style.dpi as i32 / 96;
    let reading = if style.rtl { DT_RTLREADING } else { 0 };
    // SAFETY: owner-draw supplies this DC and rectangle for the callback. All
    // temporary selected objects are restored before their handles are deleted.
    unsafe {
        let mut fill = if style.high_contrast {
            GetSysColor(if accent {
                COLOR_HIGHLIGHT
            } else {
                COLOR_BTNFACE
            })
        } else if accent {
            brand_accent(style.dark)
        } else if auxiliary {
            style.background
        } else if style.dark {
            0x00262424
        } else {
            0x00e9eced
        };
        if item.itemState & ODS_SELECTED != 0 && !style.high_contrast {
            fill = if accent {
                brand_pressed(style.dark)
            } else if style.dark {
                0x00343436
            } else {
                0x00dddfdf
            };
        }
        let ink = if style.high_contrast {
            GetSysColor(if !enabled {
                COLOR_GRAYTEXT
            } else if accent {
                COLOR_HIGHLIGHTTEXT
            } else {
                COLOR_BTNTEXT
            })
        } else if !enabled {
            if style.dark { 0x00797777 } else { 0x006f7476 }
        } else if accent {
            brand_on_accent(style.dark)
        } else {
            style.foreground
        };
        let brush = CreateSolidBrush(fill);
        let pen = CreatePen(
            PS_SOLID,
            scale(1).max(1),
            if style.high_contrast {
                GetSysColor(COLOR_WINDOWTEXT)
            } else if auxiliary {
                fill
            } else if style.dark {
                0x00434141
            } else {
                0x00d0d4d6
            },
        );
        FillRect(item.hDC, &item.rcItem, style.brush);
        let old_brush = SelectObject(item.hDC, brush);
        let old_pen = SelectObject(item.hDC, pen);
        RoundRect(
            item.hDC,
            item.rcItem.left,
            item.rcItem.top,
            item.rcItem.right,
            item.rcItem.bottom,
            scale(8),
            scale(8),
        );
        SelectObject(item.hDC, old_brush);
        SelectObject(item.hDC, old_pen);
        DeleteObject(brush);
        DeleteObject(pen);
        let old_font = SelectObject(item.hDC, style.font);
        SetBkMode(item.hDC, TRANSPARENT as i32);
        SetTextColor(item.hDC, ink);
        let mut bounds = item.rcItem;
        InflateRect(&mut bounds, -scale(10), -scale(4));
        let caption = wide(&window_text(item.hwndItem));
        let mut measured = RECT {
            right: bounds.right - bounds.left,
            ..Default::default()
        };
        DrawTextW(
            item.hDC,
            caption.as_ptr(),
            -1,
            &mut measured,
            DT_CALCRECT | DT_WORDBREAK | DT_CENTER | DT_NOPREFIX | reading,
        );
        bounds.top += ((bounds.bottom - bounds.top - measured.bottom) / 2).max(0);
        DrawTextW(
            item.hDC,
            caption.as_ptr(),
            -1,
            &mut bounds,
            DT_WORDBREAK | DT_CENTER | DT_NOPREFIX | reading,
        );
        if item.itemState & ODS_FOCUS != 0 && item.itemState & ODS_NOFOCUSRECT == 0 {
            let mut focus = item.rcItem;
            InflateRect(&mut focus, -scale(4), -scale(4));
            DrawFocusRect(item.hDC, &focus);
        }
        SelectObject(item.hDC, old_font);
    }
}

fn draw_warning_accent(item: &DRAWITEMSTRUCT, style: PaintStyle) {
    // SAFETY: the empty owner-drawn STATIC supplies its live DC and bounds for
    // paint/print requests. The temporary brush is never selected into a DC.
    unsafe {
        let accent = CreateSolidBrush(if style.high_contrast {
            style.foreground
        } else {
            brand_accent(style.dark)
        });
        FillRect(item.hDC, &item.rcItem, accent);
        DeleteObject(accent);
    }
}

unsafe extern "system" fn checkbox_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    data: usize,
) -> LRESULT {
    // SAFETY: create_controls passes a stable boxed Cell, retained until after
    // this checkbox is destroyed. Copying it does not borrow the UI state.
    let style = unsafe { (*(data as *const Cell<PaintStyle>)).get() };
    if message == WM_PAINT && !style.native_high_contrast {
        let scale = |value: i32| value * style.dpi as i32 / 96;
        let reading = if style.rtl { DT_RTLREADING } else { 0 };
        let mut paint = PAINTSTRUCT::default();
        // SAFETY: this is the checkbox's WM_PAINT, with a matched Begin/EndPaint.
        // Only pixels are replaced; BS_AUTOCHECKBOX input and UIA stay native.
        unsafe {
            let dc = BeginPaint(hwnd, &mut paint);
            let mut rect = RECT::default();
            GetClientRect(hwnd, &mut rect);
            FillRect(dc, &rect, style.brush);
            let checked = SendMessageW(hwnd, BM_GETCHECK, 0, 0) as u32 == BST_CHECKED;
            let box_size = scale(20);
            let left = scale(2);
            let top = (rect.bottom - box_size) / 2;
            let ink = style.foreground;
            let checked_fill = if style.high_contrast {
                GetSysColor(COLOR_HIGHLIGHT)
            } else {
                brand_accent(style.dark)
            };
            let brush = CreateSolidBrush(if checked {
                checked_fill
            } else {
                style.background
            });
            let pen = CreatePen(
                PS_SOLID,
                scale(1).max(1),
                if checked { checked_fill } else { ink },
            );
            let old_brush = SelectObject(dc, brush);
            let old_pen = SelectObject(dc, pen);
            RoundRect(
                dc,
                left,
                top,
                left + box_size,
                top + box_size,
                scale(4),
                scale(4),
            );
            SelectObject(dc, old_brush);
            SelectObject(dc, old_pen);
            DeleteObject(brush);
            DeleteObject(pen);
            if checked {
                let pen = CreatePen(
                    PS_SOLID,
                    scale(2).max(1),
                    if style.high_contrast {
                        GetSysColor(COLOR_HIGHLIGHTTEXT)
                    } else {
                        brand_on_accent(style.dark)
                    },
                );
                let previous = SelectObject(dc, pen);
                MoveToEx(dc, left + box_size / 5, top + box_size / 2, ptr::null_mut());
                LineTo(dc, left + box_size * 2 / 5, top + box_size * 3 / 4);
                LineTo(dc, left + box_size * 4 / 5, top + box_size / 4);
                SelectObject(dc, previous);
                DeleteObject(pen);
            }
            let old_font = SelectObject(dc, style.font);
            SetBkMode(dc, TRANSPARENT as i32);
            SetTextColor(dc, ink);
            let mut text = rect;
            text.left = left + box_size + scale(12);
            let caption = wide(&window_text(hwnd));
            let mut measured = RECT {
                right: text.right - text.left,
                ..Default::default()
            };
            DrawTextW(
                dc,
                caption.as_ptr(),
                -1,
                &mut measured,
                DT_CALCRECT | DT_WORDBREAK | DT_NOPREFIX | reading,
            );
            text.top += ((text.bottom - text.top - measured.bottom) / 2).max(0);
            DrawTextW(
                dc,
                caption.as_ptr(),
                -1,
                &mut text,
                DT_WORDBREAK | DT_NOPREFIX | reading,
            );
            if GetFocus() == hwnd
                && SendMessageW(hwnd, WM_QUERYUISTATE, 0, 0) & UISF_HIDEFOCUS as isize == 0
            {
                let focus = RECT {
                    left: scale(1),
                    top: (top.min(text.top) - scale(4)).max(scale(1)),
                    right: (text.left + measured.right + scale(6)).min(rect.right - scale(1)),
                    bottom: ((top + box_size).max(text.top + measured.bottom) + scale(4))
                        .min(rect.bottom - scale(1)),
                };
                DrawFocusRect(dc, &focus);
            }
            SelectObject(dc, old_font);
            EndPaint(hwnd, &paint);
        }
        return 0;
    }
    // SAFETY: all non-paint messages retain the native checkbox implementation.
    unsafe { DefSubclassProc(hwnd, message, wparam, lparam) }
}

fn preview_shortcut(hwnd: HWND, key: u16, state: &mut State) {
    if state.preview.is_none() {
        return;
    }
    match key {
        VK_F6 => {
            let scenes = [
                Preview::Confirm,
                Preview::Progress,
                Preview::FilesInUse,
                Preview::Rollback,
                Preview::Success,
                Preview::Reboot,
                Preview::Cancelled,
                Preview::Failure,
                Preview::PartialData,
                Preview::RegistrationFailure,
                Preview::RestartFailure,
            ];
            let index = scenes
                .iter()
                .position(|scene| Some(*scene) == state.preview)
                .unwrap_or(0);
            let next = scenes[(index + 1) % scenes.len()];
            state.page = Page::Confirm;
            state.result = None;
            state.pending_registration = None;
            state.prompt = None;
            state.lifecycle = Lifecycle::default();
            state.restart = RestartFlow::Idle;
            state.remove_data = false;
            state.details = false;
            state.preview_ticks = 0;
            state.scroll = 0;
            state.preview = Some(next);
            state.apply_preview(next);
        }
        VK_F7 => {
            let locales = [
                "zh-CN", "en-US", "ar-SA", "fa-IR", "de-DE", "ja-JP", "ko-KR", "es-ES", "fr-FR",
                "pt-BR", "nl-NL", "it-IT", "pl-PL", "ru-RU", "uk-UA", "tr-TR", "id-ID", "vi-VN",
                "th-TH", "zh-HK", "zh-TW",
            ];
            let index = locales
                .iter()
                .position(|locale| *locale == state.locale)
                .unwrap_or(0);
            state.locale = locales[(index + 1) % locales.len()];
            state.scroll = 0;
            refresh_paint_style(state);
            apply_layout_direction(hwnd, state);
        }
        VK_F8 => {
            state.preview_theme = Some(match state.preview_theme {
                Some(PreviewTheme::Dark) => PreviewTheme::Light,
                Some(PreviewTheme::Light) => PreviewTheme::HighContrast,
                _ => PreviewTheme::Dark,
            });
            state.palette = palette(true, state.preview_theme);
            refresh_paint_style(state);
            apply_title_theme(hwnd, state.palette.dark);
            apply_progress_theme(hwnd, &state.palette);
        }
        _ => {}
    }
    refresh_paint_style(state);
    // SAFETY: update only the preview window and its native children.
    unsafe {
        RedrawWindow(
            hwnd,
            ptr::null(),
            ptr::null_mut(),
            RDW_INVALIDATE | RDW_ALLCHILDREN | RDW_ERASE,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{path::PathBuf, sync::Arc, sync::mpsc};

    #[test]
    fn brand_palette_matches_flutter_in_colorref_byte_order() {
        assert_eq!(brand_accent(false), 0x000c50c2);
        assert_eq!(brand_accent(true), 0x005ca4ff);
        assert_eq!(brand_on_accent(false), 0x00ffffff);
        assert_eq!(brand_on_accent(true), 0x00001844);
        assert_eq!(brand_pressed(false), 0x000b48af);
        assert_eq!(brand_pressed(true), 0x005394e6);
    }

    #[test]
    fn registration_guidance_uses_current_cleanup_code_and_retains_msi_context() {
        let cleanup_failure = Completion {
            outcome: Outcome::RegistrationFailed,
            code: 5,
            msi_code: 0,
            record_code: Some(1618),
            purge_started: true,
            remove_user_data: true,
            bundle: None,
        };
        assert_eq!(
            State::failure_hint(&cleanup_failure),
            Some("error_permission_hint")
        );
        assert_eq!(cleanup_failure.record_code, Some(1618));
        let msi_failure = Completion {
            outcome: Outcome::MsiFailed,
            code: 1603,
            msi_code: 1603,
            ..cleanup_failure
        };
        assert_eq!(State::failure_hint(&msi_failure), Some("error_busy_hint"));
    }

    fn registration_failure(msi_code: u32, deleted_data: bool) -> Completion {
        Completion {
            outcome: Outcome::RegistrationFailed,
            code: 1603,
            msi_code,
            record_code: Some(1722),
            purge_started: deleted_data,
            remove_user_data: deleted_data,
            // This path is only retained/compared. These tests never execute it.
            bundle: Some(PathBuf::from(r"C:\inert-fixture\{bundle-id}\setup.exe")),
        }
    }

    fn preview() -> State {
        State::new(
            None,
            Some(Preview::RegistrationFailure),
            Some(PreviewTheme::Light),
            "en-US",
        )
    }

    fn assert_context(result: &Completion, original: &Completion) {
        assert_eq!(result.msi_code, original.msi_code);
        assert_eq!(result.record_code, original.record_code);
        assert_eq!(result.purge_started, original.purge_started);
        assert_eq!(result.remove_user_data, original.remove_user_data);
        assert_eq!(result.bundle, original.bundle);
    }

    #[test]
    fn completed_failure_retains_specific_numeric_msi_error_and_guidance() {
        let mut state = preview();
        let mut result = registration_failure(1603, false);
        result.outcome = Outcome::MsiFailed;
        result.record_code = Some(1706);
        state.finish(result);
        let details = state.details_text();
        assert!(details.contains("1603"));
        assert!(details.contains("1706"));
        assert!(details.contains(state.text("error_source_hint")));
        assert!(!details.contains("inert-fixture"));
        assert!(!details.contains("bundle-id"));
    }

    #[test]
    fn disconnected_msi_worker_preserves_numeric_error_without_raw_record_data() {
        let shared = Arc::new(msi::Shared::default());
        *shared.error_code.lock().unwrap() = Some(1618);
        let (tx, rx) = mpsc::channel();
        drop(tx);
        let mut state = preview();
        state.page = Page::Running;
        state.operation = Some(Operation {
            shared,
            completion: rx,
            worker: None,
        });
        state.tick();
        assert_eq!(state.result.as_ref().unwrap().record_code, Some(1618));
        assert!(state.details_text().contains(state.text("error_busy_hint")));
    }

    #[test]
    fn unavailable_cancel_explains_waiting_without_changing_the_transaction() {
        for action in ["PurgeUserData", "Rollback"] {
            let mut state = preview();
            state.apply_preview(Preview::Progress);
            state.lifecycle.action(action);
            let stage = state.lifecycle.stage;
            let purge_started = state.lifecycle.purge_started;
            state.secondary(ptr::null_mut());
            assert!(state.page == Page::Running);
            assert!(state.result.is_none());
            assert_eq!(state.lifecycle.stage, stage);
            assert_eq!(state.lifecycle.purge_started, purge_started);
            assert!(!state.lifecycle.cancel_requested);
            assert_eq!(state.body_key(), "wait_no_cancel");
        }
    }

    #[test]
    fn returning_from_partial_deletion_never_reselects_data_deletion() {
        let mut state = preview();
        state.apply_preview(Preview::PartialData);
        state.remove_data = true;
        assert!(state.critical_warning());
        assert_eq!(state.warning_key(), Some("uninstall_partial_data"));
        state.primary(ptr::null_mut());
        assert!(state.page == Page::Confirm);
        assert!(!state.remove_data);
        assert!(!state.critical_warning());
        assert!(state.result.is_none());
    }

    #[test]
    fn measured_footer_captions_fit_and_restart_confirmation_keeps_button_positions() {
        for (scale, width, height, stacked) in [
            (1, 680, 480, false),
            (1, 320, 480, true),
            (2, 1360, 960, false),
            (2, 900, 800, true),
        ] {
            let captions = [
                (DETAILS, true, 36 * scale),
                (SECONDARY, true, 48 * scale),
                (PRIMARY, true, 70 * scale),
            ];
            let (_, buttons) = footer_positions(
                (width, height),
                32 * scale,
                10 * scale,
                20 * scale,
                stacked,
                captions,
            );
            assert_eq!(buttons.len(), 3);
            for (index, button) in buttons.iter().enumerate() {
                assert!(button.x >= 0 && button.y >= 0);
                assert!(button.x + button.width <= width);
                assert!(button.y + button.height <= height);
                assert!(button.height >= captions[index].2);
                for other in &buttons[index + 1..] {
                    assert!(
                        button.x + button.width <= other.x
                            || other.x + other.width <= button.x
                            || button.y + button.height <= other.y
                            || other.y + other.height <= button.y
                    );
                }
            }
            let swapped = [
                (DETAILS, true, 36 * scale),
                (SECONDARY, true, 70 * scale),
                (PRIMARY, true, 48 * scale),
            ];
            let (_, confirmed) = footer_positions(
                (width, height),
                32 * scale,
                10 * scale,
                20 * scale,
                stacked,
                swapped,
            );
            let primary = &buttons[2];
            assert_eq!(
                (primary.x, primary.y, primary.width, primary.height),
                (
                    confirmed[2].x,
                    confirmed[2].y,
                    confirmed[2].width,
                    confirmed[2].height
                )
            );
        }
    }

    #[test]
    fn registration_retry_clears_old_failure_and_preserves_msi_result_in_preview() {
        for msi_code in [0, 3010, 1641] {
            for deleted_data in [false, true] {
                let original = registration_failure(msi_code, deleted_data);
                let mut state = preview();
                state.finish(original.clone());
                state.primary(ptr::null_mut());
                assert!(state.result.is_none());
                assert!(state.page == Page::Running);
                assert_eq!(state.lifecycle.stage, Stage::Registration);
                assert_eq!(state.body_key(), "uninstall_registration");
                assert!(!state.lifecycle.can_cancel());
                assert_context(state.pending_registration.as_ref().unwrap(), &original);
                assert!(state.operation.is_none(), "preview never starts a worker");
                for _ in 0..30 {
                    state.tick();
                }
                assert!(state.page == Page::Finished);
                assert!(state.pending_registration.is_none());
                let result = state.result.as_ref().unwrap();
                assert_context(result, &original);
                assert_eq!(
                    result.outcome,
                    if msi_code == 0 {
                        Outcome::Success
                    } else {
                        Outcome::RebootRequired
                    }
                );
                assert_eq!(
                    state.warning_key(),
                    Some(if deleted_data {
                        "uninstall_data_removed"
                    } else {
                        "uninstall_data_kept"
                    })
                );
                assert_eq!(state.exit_code, msi_code as i32);
            }
        }
    }

    #[test]
    fn registration_worker_results_and_disconnection_cannot_restart_msi() {
        for disconnected in [false, true] {
            let original = registration_failure(3010, true);
            let mut state = preview();
            state.finish(original.clone());
            state.primary(ptr::null_mut());
            let shared = Arc::new(msi::Shared::default());
            let (tx, rx) = mpsc::channel();
            if !disconnected {
                tx.send(msi::registration_result(original.clone(), 1603))
                    .unwrap();
            }
            drop(tx);
            state.operation = Some(Operation {
                shared,
                completion: rx,
                worker: None,
            });
            state.tick();
            let failed = state.result.as_ref().unwrap();
            assert_eq!(failed.outcome, Outcome::RegistrationFailed);
            assert!(!failed.outcome.can_return_to_confirmation());
            assert_context(failed, &original);
            state.primary(ptr::null_mut());
            assert_eq!(state.body_key(), "uninstall_registration");
            assert!(!state.lifecycle.can_cancel());
            assert_context(state.pending_registration.as_ref().unwrap(), &original);
            for _ in 0..30 {
                state.tick();
            }
            assert_eq!(
                state.result.as_ref().unwrap().outcome,
                Outcome::RebootRequired
            );
            assert_eq!(state.warning_key(), Some("uninstall_data_removed"));
        }
    }

    #[test]
    fn registration_failure_preview_starts_after_a_successful_msi() {
        let mut state = preview();
        state.apply_preview(Preview::RegistrationFailure);
        let result = state.result.as_ref().unwrap();
        assert_eq!(result.outcome, Outcome::RegistrationFailed);
        assert_eq!(result.msi_code, 0);
        assert_eq!(result.code, 1603);
        state.primary(ptr::null_mut());
        for _ in 0..30 {
            state.tick();
        }
        assert_eq!(state.result.as_ref().unwrap().outcome, Outcome::Success);
        assert_eq!(state.exit_code, 0);
    }

    #[test]
    fn initial_registration_worker_disconnection_retains_completed_msi_context() {
        let original = registration_failure(1641, true);
        let shared = Arc::new(msi::Shared::default());
        shared.begin_registration(&original);
        let (tx, rx) = mpsc::channel();
        drop(tx);
        let mut state = preview();
        state.page = Page::Running;
        state.operation = Some(Operation {
            shared,
            completion: rx,
            worker: None,
        });
        assert!(state.pending_registration.is_none());
        state.tick();
        let result = state.result.as_ref().unwrap();
        assert_eq!(result.outcome, Outcome::RegistrationFailed);
        assert_context(result, &original);
        assert!(!result.outcome.can_return_to_confirmation());
    }
}
