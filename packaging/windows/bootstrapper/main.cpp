// The BA owns presentation and Burn orchestration. Privileged installation,
// recovery, rollback and data cleanup remain exclusively in the MSI/helper.
#include <windows.h>
#include <commctrl.h>
#include <dwmapi.h>
#include <richedit.h>
#include <shellapi.h>
#include <uxtheme.h>
#include <initguid.h>
#include <oleacc.h>
#include <msiquery.h>
#include <dutil.h>
#include <dictutil.h>

#include <BootstrapperApplication.h>
#include <BootstrapperApplicationBase.h>

#include "platform.h"
#include "options_result.h"
#include "setup_l10n.h"
#include "state.h"

#include <algorithm>
#include <array>
#include <atomic>
#include <filesystem>
#include <memory>
#include <string>
#include <thread>
#include <vector>

namespace usque::setup {
namespace {
constexpr UINT kEvent = WM_APP + 20;
constexpr UINT_PTR kPreviewTimer = 19;
enum Control {
  title = 100, subtitle, version, folder_label, folder_edit, browse, language_label,
  language, license, accept, scope, progress, status, details, desktop, launch_app,
  startup, more, license_text, folder_summary, change_folder, option_summary, save_details, primary, secondary, tertiary,
};
enum class EventType { detected, planned, applied, progress, status, failed, options, queried, files_in_use, cancel_permission };
enum class FinishIntent { normal, restart_later, restart_now };
struct Event {
  EventType type;
  HRESULT result = S_OK;
  DWORD number = 0;
  std::string key;
  ChildResult child;
  std::wstring text;
};
struct OptionResultRow {
  std::string key;
  bool done;
  bool include_status = true;
};

std::wstring ReadControl(HWND control) {
  const int length = GetWindowTextLengthW(control);
  std::wstring result(static_cast<size_t>(length) + 1, L'\0');
  GetWindowTextW(control, result.data(), length + 1);
  result.resize(length);
  return result;
}

std::wstring CultureForWindows() {
  wchar_t name[LOCALE_NAME_MAX_LENGTH]{};
  LCIDToLocaleName(MAKELCID(GetUserDefaultUILanguage(), SORT_DEFAULT), name, LOCALE_NAME_MAX_LENGTH, 0);
  const std::wstring locale(name);
  for (const auto& item : kLanguages) if (item.code == locale) return locale;
  const auto dash = locale.find(L'-');
  const auto prefix = locale.substr(0, dash);
  if (prefix == L"zh") {
    if (locale == L"zh-MO" || locale == L"zh-HK") return L"zh-HK";
    if (locale == L"zh-TW" || locale == L"zh-Hant") return L"zh-TW";
    return L"zh-CN";
  }
  for (const auto& item : kLanguages) {
    if (item.code.substr(0, item.code.find(L'-')) == prefix) return std::wstring(item.code);
  }
  return L"en-US";
}

bool IsKnownCulture(std::wstring_view culture) {
  for (const auto& item : kLanguages) if (item.code == culture) return true;
  return false;
}

DWORD ExitCode(HRESULT status) {
  return HRESULT_FACILITY(status) == FACILITY_WIN32 ? HRESULT_CODE(status) : static_cast<DWORD>(status);
}

struct RtfStream { std::string bytes; size_t offset = 0; };
DWORD CALLBACK ReadRtf(DWORD_PTR cookie, LPBYTE destination, LONG requested, LONG* written) {
  auto* stream = reinterpret_cast<RtfStream*>(cookie);
  const size_t count = (std::min)(static_cast<size_t>(requested), stream->bytes.size() - stream->offset);
  std::copy_n(stream->bytes.data() + stream->offset, count, destination);
  stream->offset += count;
  *written = static_cast<LONG>(count);
  return 0;
}
}  // namespace

class Application final : public CBootstrapperApplicationBase {
 public:
  explicit Application(bool preview = false, std::wstring preview_state = {})
      : preview_(preview), preview_state_(std::move(preview_state)) {}

  STDMETHODIMP OnCreate(IBootstrapperEngine* engine, BOOTSTRAPPER_COMMAND* command) override {
    const HRESULT result = CBootstrapperApplicationBase::OnCreate(engine, command);
    if (FAILED(result)) return result;
    BalInitialize(engine);
    requested_action_ = command->action;
    display_ = command->display;
    return S_OK;
  }

  STDMETHODIMP OnStartup() override {
    ui_thread_ = std::thread([this] { RunUi(); });
    return S_OK;
  }

  STDMETHODIMP OnDestroy(BOOL) override {
    if (ui_thread_.joinable()) ui_thread_.join();
    if (worker_.joinable()) worker_.join();
    BalUninitialize();
    return S_OK;
  }

  STDMETHODIMP OnShutdown(BOOTSTRAPPER_SHUTDOWN_ACTION* action) override {
    *action = restart_requested_ ? BOOTSTRAPPER_SHUTDOWN_ACTION_RESTART : BOOTSTRAPPER_SHUTDOWN_ACTION_NONE;
    return S_OK;
  }

  STDMETHODIMP OnDetectBegin(BOOL, BOOTSTRAPPER_REGISTRATION_TYPE registration,
                             DWORD, BOOL* cancel) override {
    detection_ = {};
    detection_.bundle_registered = registration != BOOTSTRAPPER_REGISTRATION_TYPE_NONE;
    *cancel |= CheckCanceled();
    return S_OK;
  }

  STDMETHODIMP OnDetectRelatedMsiPackage(LPCWSTR package, LPCWSTR, LPCWSTR,
                                       BOOL, LPCWSTR, BOOTSTRAPPER_RELATED_OPERATION operation,
                                       BOOL* cancel) override {
    if (std::wstring_view(package) == L"UsqueMsi") {
      detection_.related_product = true;
      detection_.newer_product |= operation == BOOTSTRAPPER_RELATED_OPERATION_DOWNGRADE;
    }
    *cancel |= CheckCanceled();
    return S_OK;
  }

  STDMETHODIMP OnDetectPackageComplete(LPCWSTR package, HRESULT result,
                                      BOOTSTRAPPER_PACKAGE_STATE state, BOOL) override {
    if (std::wstring_view(package) == L"UsqueMsi" && SUCCEEDED(result)) {
      detection_.exact_product = state == BOOTSTRAPPER_PACKAGE_STATE_PRESENT;
      detection_.newer_product |= state == BOOTSTRAPPER_PACKAGE_STATE_SUPERSEDED;
    }
    return S_OK;
  }

  STDMETHODIMP OnDetectComplete(HRESULT result, BOOL) override {
    Post({EventType::detected, result});
    return S_OK;
  }

  STDMETHODIMP OnPlanMsiPackage(LPCWSTR, BOOL execute, BOOTSTRAPPER_ACTION_STATE action,
                               BOOTSTRAPPER_MSI_FILE_VERSIONING, BOOL* cancel,
                               BURN_MSI_PROPERTY*, INSTALLUILEVEL* level, BOOL* disable_handler,
                               BOOTSTRAPPER_MSI_FILE_VERSIONING*) override {
    // Burn forwards real progress and errors. Directly opened MSIs keep their
    // existing compatibility UI because this affects only this BA's plan.
    *level = INSTALLUILEVEL_NONE;
    *disable_handler = FALSE;
    *cancel |= CheckCanceled();
    if (execute && (action == BOOTSTRAPPER_ACTION_STATE_REPAIR ||
                    action == BOOTSTRAPPER_ACTION_STATE_MODIFY ||
                    action == BOOTSTRAPPER_ACTION_STATE_MINOR_UPGRADE))
      return HRESULT_FROM_WIN32(ERROR_NOT_SUPPORTED);
    return S_OK;
  }

  STDMETHODIMP OnPlanComplete(HRESULT result) override {
    Post({EventType::planned, result});
    return S_OK;
  }

  STDMETHODIMP OnProgress(DWORD value, DWORD overall, BOOL* cancel) override {
    Post({EventType::progress, S_OK, overall});
    const HRESULT result = CBootstrapperApplicationBase::OnProgress(value, overall, cancel);
    if (!CallbackCancellationAllowed()) *cancel = FALSE;
    return result;
  }

  STDMETHODIMP OnExecuteProgress(LPCWSTR package, DWORD value, DWORD overall, BOOL* cancel) override {
    const HRESULT result = CBootstrapperApplicationBase::OnExecuteProgress(package, value, overall, cancel);
    if (!CallbackCancellationAllowed()) *cancel = FALSE;
    return result;
  }

  STDMETHODIMP OnExecutePackageBegin(LPCWSTR package, BOOL execute,
                                     BOOTSTRAPPER_ACTION_STATE action, INSTALLUILEVEL level,
                                     BOOL disabled, BOOL* cancel) override {
    if (!execute) cancel_stage_ = CancelStage::rollback;
    // Until MSI sends a valid COMMONDATA cancel-enable record, do not assume
    // its current operation is cancellable.
    native_cancel_enabled_ = false;
    Post({EventType::status, S_OK, 0, execute ?
      (action == BOOTSTRAPPER_ACTION_STATE_UNINSTALL ? "uninstalling" : "installing") : "rolling_back"});
    const HRESULT result = CBootstrapperApplicationBase::OnExecutePackageBegin(package, execute, action, level, disabled, cancel);
    if (!CallbackCancellationAllowed()) *cancel = FALSE;
    return result;
  }

  STDMETHODIMP OnError(BOOTSTRAPPER_ERROR_TYPE, LPCWSTR, DWORD code, LPCWSTR,
                       DWORD, DWORD, LPCWSTR*, int recommendation, int* result) override {
    // Do not surface raw MSI records: they can contain user paths or values.
    Post({EventType::status, HRESULT_FROM_WIN32(code), 0, "failed_description"});
    *result = CanReturnCancellation(cancel_stage_, native_cancel_enabled_, CheckCanceled()) ? IDCANCEL : recommendation;
    if (*result == IDCANCEL && !CallbackCancellationAllowed()) *result = IDNOACTION;
    return S_OK;
  }

  STDMETHODIMP OnApplyComplete(HRESULT result, BOOTSTRAPPER_APPLY_RESTART restart,
                               BOOTSTRAPPER_APPLYCOMPLETE_ACTION,
                               BOOTSTRAPPER_APPLYCOMPLETE_ACTION* action) override {
    // Never turn installation completion into an unsolicited machine restart.
    *action = BOOTSTRAPPER_APPLYCOMPLETE_ACTION_NONE;
    Post({EventType::applied, result, static_cast<DWORD>(restart)});
    return S_OK;
  }

  STDMETHODIMP OnExecuteMsiMessage(LPCWSTR, INSTALLMESSAGE type, DWORD, LPCWSTR,
                                   DWORD count, LPCWSTR* data, int recommendation, int* result) override {
    // WiX 5.0.2 wiutil.cpp forwards COMMONDATA through SendMsiMessage, and
    // Burn apply.cpp forwards its 1-based MSI fields as the 0-based data array.
    if (type == INSTALLMESSAGE_COMMONDATA && count && data && data[0]) {
      native_cancel_enabled_ = CommonDataCancelPermission(native_cancel_enabled_, data[0],
        count >= 2 && data[1] ? std::wstring_view(data[1]) : std::wstring_view{});
      Post({EventType::cancel_permission});
    }
    if (type == INSTALLMESSAGE_ACTIONSTART && count && data && data[0]) {
      const std::wstring_view action(data[0]);
      cancel_stage_ = ActionCancelStage(action, cancel_stage_.load());
      const std::string key = action == L"StopServices" ? "uninstall_closing_apps" :
        action == L"EmergencyRemoveKillSwitch" || action == L"RecoverAgentState" ? "uninstall_restoring_network" :
        action == L"PurgeUserData" ? "uninstall_deleting_data" :
        action == L"RemoveFiles" ? "uninstall_removing_files" :
        action == L"FinalizeAgentUninstall" ? "uninstall_registration" : "installing";
      if (!IsRollingBack()) Post({EventType::status, S_OK, 0, key});
    }
    *result = CanReturnCancellation(cancel_stage_, native_cancel_enabled_, CheckCanceled()) ? IDCANCEL : recommendation;
    if (*result == IDCANCEL && !CallbackCancellationAllowed()) *result = IDNOACTION;
    return S_OK;
  }

  STDMETHODIMP OnRollbackMsiTransactionBegin(LPCWSTR) override {
    cancel_stage_ = CancelStage::rollback;
    Post({EventType::status, S_OK, 0, "rolling_back"});
    return S_OK;
  }

  STDMETHODIMP OnUnregisterBegin(BOOTSTRAPPER_REGISTRATION_TYPE registration, BOOTSTRAPPER_REGISTRATION_TYPE*) override {
    // Burn also ends ordinary installation sessions here while retaining their
    // registration. Keep that presentation distinct from removal and rollback.
    const bool rolling_back = IsRollingBack() || cancel_stage_.load() == CancelStage::rollback;
    const std::string key = rolling_back ? "rolling_back" :
      action_ == BOOTSTRAPPER_ACTION_UNINSTALL || registration == BOOTSTRAPPER_REGISTRATION_TYPE_NONE ?
        "uninstall_registration" : "installing";
    cancel_stage_ = CancelStage::registration_cleanup;
    Post({EventType::status, S_OK, 0, key});
    return S_OK;
  }

  STDMETHODIMP OnExecuteFilesInUse(LPCWSTR, DWORD count, LPCWSTR* files, int,
                                   BOOTSTRAPPER_FILES_IN_USE_TYPE source, int* result) override {
    if (CanReturnCancellation(cancel_stage_, native_cancel_enabled_, CheckCanceled())) { *result = IDCANCEL; return S_OK; }
    // Match WiX 5.0.2's documented silent MSI policy. Interactive installations
    // wait for a choice and never substitute an implicit Ignore/reboot choice.
    if (!Interactive()) { *result = IDOK; return S_OK; }
    if (!choice_event_) return E_OUTOFMEMORY;
    ResetEvent(choice_event_);
    choice_result_ = IDCANCEL;
    Event event{EventType::files_in_use, S_OK, static_cast<DWORD>(source)};
    for (DWORD index = 1; files && index < count && index < 33; index += 2) {
      if (!files[index]) continue;
      std::wstring name(files[index], wcsnlen_s(files[index], 160));
      std::replace_if(name.begin(), name.end(), [](wchar_t ch) { return ch < 32; }, L' ');
      event.text += name + L"\r\n";
    }
    Post(std::move(event));
    const DWORD wait = WaitForSingleObject(choice_event_, INFINITE);
    *result = wait == WAIT_OBJECT_0 ? choice_result_.load() : IDCANCEL;
    if (*result == IDCANCEL && !CallbackCancellationAllowed()) *result = IDNOACTION;
    return S_OK;
  }

  int Preview() { RunUi(); return 0; }

 private:
  ~Application() override {
    if (worker_.joinable()) worker_.join();
    if (ui_thread_.joinable()) ui_thread_.join();
    if (choice_event_) CloseHandle(choice_event_);
  }

  void Post(Event event) {
    auto message = std::make_unique<Event>(std::move(event));
    if (PostMessageW(window_.load(), kEvent, 0, reinterpret_cast<LPARAM>(message.get())))
      message.release();
  }

  std::wstring L(std::string_view key) const { return std::wstring(Lookup(culture_, key)); }
  HWND C(int id) const { return controls_[static_cast<size_t>(id - title)]; }
  void Text(int id, std::string_view key) { SetWindowTextW(C(id), L(key).c_str()); }
  void Show(int id, bool visible = true) { ShowWindow(C(id), visible ? SW_SHOW : SW_HIDE); }
  bool Checked(int id) const { return SendMessageW(C(id), BM_GETCHECK, 0, 0) == BST_CHECKED; }
  void Check(int id, bool value) { SendMessageW(C(id), BM_SETCHECK, value ? BST_CHECKED : BST_UNCHECKED, 0); }
  bool Interactive() const { return preview_ || display_ == BOOTSTRAPPER_DISPLAY_FULL; }
  bool CallbackCancellationAllowed() const { return native_cancel_enabled_ && CanRequestCancellation(cancel_stage_); }

  std::wstring Variable(const wchar_t* name) {
    if (!m_pEngine) return {};
    SIZE_T size = 0;
    HRESULT hr = m_pEngine->GetVariableString(name, nullptr, &size);
    if (hr != HRESULT_FROM_WIN32(ERROR_MORE_DATA) || size > 32768) return {};
    std::wstring value(size + 1, L'\0');
    ++size;
    hr = m_pEngine->GetVariableString(name, value.data(), &size);
    if (FAILED(hr)) return {};
    value.resize(wcslen(value.c_str()));
    return value;
  }

  void RunUi() {
    CoInitializeEx(nullptr, COINIT_APARTMENTTHREADED);
    INITCOMMONCONTROLSEX common{sizeof(common), ICC_PROGRESS_CLASS | ICC_STANDARD_CLASSES};
    InitCommonControlsEx(&common);
    rich_edit_ = LoadLibraryW(L"Msftedit.dll");
    culture_ = CultureForWindows();
    if (!preview_) {
      const auto persisted = Variable(L"UsqueMsiTransform");
      if (persisted.starts_with(L"transforms\\") && persisted.ends_with(L".mst")) {
        const auto candidate = persisted.substr(11, persisted.size() - 15);
        if (IsKnownCulture(candidate)) culture_ = candidate;
      }
      installed_folder_ = RegistryString(HKEY_LOCAL_MACHINE, L"Software\\Usque", L"InstallLocation");
      current_version_ = RegistryString(HKEY_LOCAL_MACHINE, L"Software\\Usque", L"DisplayVersion");
      target_version_ = Variable(L"UsqueDisplayVersion");
    }
    folder_ = installed_folder_;
    if (folder_.empty()) folder_ = Variable(L"UsqueInstallFolder");
    if (folder_.empty()) {
      wchar_t program_files[MAX_PATH]{};
      GetEnvironmentVariableW(L"ProgramW6432", program_files, MAX_PATH);
      folder_ = std::wstring(program_files) + L"\\Usque";
      if (!ValidInstallFolder(folder_)) folder_ = L"C:\\Program Files\\Usque";
    }
    WNDCLASSEXW cls{sizeof(cls)};
    cls.hInstance = GetModuleHandleW(nullptr);
    cls.lpfnWndProc = WindowProc;
    cls.hCursor = LoadCursorW(nullptr, IDC_ARROW);
    cls.hIcon = LoadIconW(cls.hInstance, MAKEINTRESOURCEW(1));
    cls.hIconSm = cls.hIcon;
    cls.lpszClassName = L"UsqueBootstrapperWindow";
    RegisterClassExW(&cls);
    const HWND hwnd = CreateWindowExW(WS_EX_CONTROLPARENT, cls.lpszClassName,
      L("setup_title").c_str(), WS_OVERLAPPEDWINDOW | WS_CLIPCHILDREN | WS_VSCROLL,
      CW_USEDEFAULT, CW_USEDEFAULT, 680, 480, nullptr, nullptr, cls.hInstance, this);
    if (!hwnd) {
      if (m_pEngine) m_pEngine->Quit(GetLastError());
      CoUninitialize(); return;
    }
    dpi_ = GetDpiForWindow(hwnd);
    SizeToWorkspace();
    SetTheme();
    CreateControls();
    Render();
    if (display_ >= BOOTSTRAPPER_DISPLAY_PASSIVE || preview_) ShowWindow(hwnd, SW_SHOW);
    if (m_pEngine) {
      m_pEngine->CloseSplashScreen();
      const HRESULT result = m_pEngine->Detect(hwnd);
      if (FAILED(result)) Fail(result);
    } else {
      SetupPreview();
    }
    MSG message{};
    while (GetMessageW(&message, nullptr, 0, 0) > 0) {
      if (preview_ && message.message == WM_KEYDOWN && message.wParam == VK_F6) {
        constexpr std::wstring_view scenes[] = {L"install", L"upgrade", L"maintenance", L"residual", L"complete", L"partial", L"failed", L"reboot", L"files", L"failed-reboot"};
        preview_scene_ = (preview_scene_ + 1) % std::size(scenes);
        preview_state_ = scenes[preview_scene_];
        KillTimer(hwnd, kPreviewTimer); SetupPreview(); continue;
      }
      if (preview_ && message.message == WM_KEYDOWN && message.wParam == VK_F7) {
        constexpr std::wstring_view cultures[] = {L"en-US", L"zh-CN", L"ar-SA"};
        preview_language_ = (preview_language_ + 1) % std::size(cultures);
        culture_ = cultures[preview_language_];
        for (size_t index = 0; index < std::size(kLanguages); ++index)
          if (kLanguages[index].code == culture_) SendMessageW(C(language), CB_SETCURSEL, index, 0);
        Render(); continue;
      }
      if (preview_ && message.message == WM_KEYDOWN && message.wParam == VK_F8) {
        preview_theme_ = (preview_theme_ + 1) % 3;
        SetTheme(); Render(); continue;
      }
      if (preview_ && message.message == WM_KEYDOWN && message.wParam == VK_F9) {
        preview_scale_ = preview_scale_ == 1 ? 2 : 1;
        SizeToWorkspace();
        scroll_ = 0; SetTheme(); Render(); continue;
      }
      if (!IsDialogMessageW(hwnd, &message)) {
        TranslateMessage(&message); DispatchMessageW(&message);
      }
      if (message.message == WM_KEYDOWN && message.wParam == VK_TAB) EnsureFocusVisible();
    }
    if (worker_.joinable()) worker_.join();
    if (font_) DeleteObject(font_);
    if (title_font_) DeleteObject(title_font_);
    if (secondary_font_) DeleteObject(secondary_font_);
    if (strong_font_) DeleteObject(strong_font_);
    if (background_) DeleteObject(background_);
    if (rich_edit_) FreeLibrary(rich_edit_);
    CoUninitialize();
  }

  void SetupPreview() {
    action_ = BOOTSTRAPPER_ACTION_INSTALL; finish_ = {};
    finish_intent_ = FinishIntent::normal; reconfirm_restart_ = false; launch_failed_ = false;
    Check(desktop, false); Check(launch_app, true); Check(startup, false);
    options_busy_ = false; cancel_requested_ = false; reboot_confirmation_ = false;
    cancel_stage_ = CancelStage::before_start;
    native_cancel_enabled_ = true;
    option_rows_.clear(); scroll_ = 0; details_expanded_ = false;
    mode_ = preview_state_ == L"maintenance" ? Mode::maintenance :
      preview_state_ == L"upgrade" ? Mode::upgrade :
      preview_state_ == L"residual" ? Mode::residual : Mode::install;
    page_ = Page::ready;
    target_version_ = L"0.2.9";
    if (mode_ == Mode::maintenance || mode_ == Mode::upgrade) current_version_ = L"0.2.8";
    if (preview_state_ == L"complete" || preview_state_ == L"reboot") {
      page_ = Page::complete; finish_.reboot = preview_state_ == L"reboot";
      if (finish_.reboot) { finish_.launch = false; Check(launch_app, false); }
    } else if (preview_state_ == L"failed" || preview_state_ == L"failed-reboot") {
      page_ = Page::failed; result_ = HRESULT_FROM_WIN32(ERROR_INSTALL_FAILURE);
      finish_.reboot = preview_state_ == L"failed-reboot";
    } else if (preview_state_ == L"partial") {
      page_ = Page::options_failed; result_ = HRESULT_FROM_WIN32(ERROR_ACCESS_DENIED);
      finish_.desktop = true; finish_.desktop_done = true; Check(desktop, true);
      finish_.startup = true; Check(startup, true);
      option_rows_ = {{"desktop_shortcut", true}, {"start_on_login", false}};
    }
    else if (preview_state_ == L"files") {
      page_ = Page::files_in_use; files_source_ = BOOTSTRAPPER_FILES_IN_USE_TYPE_MSI_RM;
      files_in_use_ = L"Usque";
    }
    Render();
  }

  void CreateControls() {
    body_ = CreateWindowExW(WS_EX_CONTROLPARENT, L"STATIC", L"", WS_CHILD | WS_VISIBLE | WS_CLIPCHILDREN,
      0, 0, 0, 0, window_.load(), nullptr, GetModuleHandleW(nullptr), nullptr);
    SetWindowSubclass(body_, BodyProc, 1, reinterpret_cast<DWORD_PTR>(this));
    const auto add = [this](int id, const wchar_t* type, DWORD style) {
      const bool fixed = id == title || id == primary || id == secondary || id == tertiary;
      const HWND child = CreateWindowExW(type == std::wstring_view(L"EDIT") ? WS_EX_CLIENTEDGE : 0,
        type, L"", WS_CHILD | style, 0, 0, 0, 0, fixed ? window_.load() : body_,
        reinterpret_cast<HMENU>(static_cast<INT_PTR>(id)), GetModuleHandleW(nullptr), nullptr);
      controls_[static_cast<size_t>(id - title)] = child;
      SendMessageW(child, WM_SETFONT, reinterpret_cast<WPARAM>(font_), TRUE);
      return child;
    };
    for (int id : {title, subtitle, version, folder_label})
      add(id, L"STATIC", SS_LEFT | SS_NOPREFIX);
    add(folder_edit, L"EDIT", WS_TABSTOP | ES_AUTOHSCROLL);
    add(language_label, L"STATIC", SS_LEFT | SS_NOPREFIX);
    add(language, WC_COMBOBOXW, WS_TABSTOP | CBS_DROPDOWNLIST | CBS_OWNERDRAWFIXED | CBS_HASSTRINGS | WS_VSCROLL);
    SetWindowSubclass(C(language), ComboProc, 1, reinterpret_cast<DWORD_PTR>(this));
    for (int id : {scope, status, option_summary}) add(id, L"STATIC", SS_LEFT | SS_NOPREFIX);
    SetWindowSubclass(C(option_summary), NoticeProc, 1, reinterpret_cast<DWORD_PTR>(this));
    add(folder_summary, L"STATIC", SS_LEFT | SS_NOPREFIX | SS_PATHELLIPSIS);
    for (int id : {browse, change_folder, license, more, save_details, primary, secondary, tertiary})
      add(id, L"BUTTON", WS_TABSTOP | BS_OWNERDRAW);
    for (int id : {accept, desktop, launch_app, startup}) {
      const HWND checkbox = add(id, L"BUTTON", WS_TABSTOP | BS_AUTOCHECKBOX | BS_MULTILINE);
      SetWindowSubclass(checkbox, CheckboxProc, 1, reinterpret_cast<DWORD_PTR>(this));
    }
    add(progress, PROGRESS_CLASSW, PBS_SMOOTH);
    add(details, L"EDIT", WS_TABSTOP | ES_READONLY | ES_MULTILINE | ES_AUTOVSCROLL | WS_VSCROLL);
    add(license_text, MSFTEDIT_CLASS, WS_TABSTOP | ES_READONLY | ES_MULTILINE | ES_AUTOVSCROLL | WS_VSCROLL);
    SetWindowTextW(C(folder_edit), folder_.c_str());
    SendMessageW(C(folder_edit), EM_SETLIMITTEXT, 240, 0);
    for (size_t index = 0; index < std::size(kLanguages); ++index) {
      const std::wstring name(kLanguages[index].name);
      SendMessageW(C(language), CB_ADDSTRING, 0, reinterpret_cast<LPARAM>(name.c_str()));
      if (kLanguages[index].code == culture_) SendMessageW(C(language), CB_SETCURSEL, index, 0);
    }
    SendMessageW(C(language), CB_SETITEMHEIGHT, static_cast<WPARAM>(-1), Scale(30));
    SendMessageW(C(language), CB_SETITEMHEIGHT, 0, Scale(32));
    Check(desktop, false); Check(launch_app, true); Check(startup, false);
    SendMessageW(C(title), WM_SETFONT, reinterpret_cast<WPARAM>(title_font_), TRUE);
    SetTheme();
  }

  void SetTheme() {
    HIGHCONTRASTW contrast{sizeof(contrast)};
    SystemParametersInfoW(SPI_GETHIGHCONTRAST, sizeof(contrast), &contrast, 0);
    high_contrast_ = (contrast.dwFlags & HCF_HIGHCONTRASTON) != 0;
    DWORD light = 1, bytes = sizeof(light);
    RegGetValueW(HKEY_CURRENT_USER, L"Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize",
      L"AppsUseLightTheme", RRF_RT_REG_DWORD, nullptr, &light, &bytes);
    dark_ = !high_contrast_ && !light;
    if (preview_ && preview_theme_ >= 0) {
      high_contrast_ = preview_theme_ == 2; dark_ = preview_theme_ == 1;
    }
    background_color_ = high_contrast_ ? SystemColor(COLOR_WINDOW) : dark_ ? RGB(24, 29, 37) : RGB(250, 251, 253);
    text_color_ = high_contrast_ ? SystemColor(COLOR_WINDOWTEXT) : dark_ ? RGB(235, 239, 247) : RGB(28, 36, 47);
    if (background_) DeleteObject(background_);
    background_ = CreateSolidBrush(background_color_);
    const BOOL use_dark = dark_;
    DwmSetWindowAttribute(window_.load(), DWMWA_USE_IMMERSIVE_DARK_MODE, &use_dark, sizeof(use_dark));
    if (font_) DeleteObject(font_);
    if (title_font_) DeleteObject(title_font_);
    if (secondary_font_) DeleteObject(secondary_font_);
    if (strong_font_) DeleteObject(strong_font_);
    NONCLIENTMETRICSW metrics{sizeof(metrics)};
    SystemParametersInfoForDpi(SPI_GETNONCLIENTMETRICS, sizeof(metrics), &metrics, 0, EffectiveDpi());
    metrics.lfMessageFont.lfHeight = -Scale(15);
    font_ = CreateFontIndirectW(&metrics.lfMessageFont);
    metrics.lfMessageFont.lfWeight = FW_SEMIBOLD;
    strong_font_ = CreateFontIndirectW(&metrics.lfMessageFont);
    metrics.lfMessageFont.lfWeight = FW_NORMAL;
    metrics.lfMessageFont.lfHeight = -Scale(13);
    secondary_font_ = CreateFontIndirectW(&metrics.lfMessageFont);
    metrics.lfMessageFont.lfHeight = -Scale(29);
    metrics.lfMessageFont.lfWeight = FW_SEMIBOLD;
    title_font_ = CreateFontIndirectW(&metrics.lfMessageFont);
    for (int id = title; id <= tertiary; ++id) if (C(id)) {
      const HFONT font = id == title ? title_font_ : IsSecondaryText(id) ? secondary_font_ : font_;
      SendMessageW(C(id), WM_SETFONT, reinterpret_cast<WPARAM>(font), TRUE);
      SetWindowTheme(C(id), dark_ || PreviewContrast() ? L"" : L"Explorer", nullptr);
    }
    if (C(license_text)) SendMessageW(C(license_text), EM_SETBKGNDCOLOR, 0, background_color_);
    if (C(language)) {
      SendMessageW(C(language), CB_SHOWDROPDOWN, FALSE, 0);
      SendMessageW(C(language), CB_SETITEMHEIGHT, static_cast<WPARAM>(-1), Scale(30));
      SendMessageW(C(language), CB_SETITEMHEIGHT, 0, Scale(32));
    }
    if (C(progress)) {
      if (!high_contrast_) SetWindowTheme(C(progress), L"", nullptr);
      SendMessageW(C(progress), PBM_SETBARCOLOR, 0, high_contrast_ ? SystemColor(COLOR_HIGHLIGHT) : RGB(244, 129, 32));
      SendMessageW(C(progress), PBM_SETBKCOLOR, 0, high_contrast_ ? SystemColor(COLOR_WINDOW) :
        dark_ ? RGB(40, 47, 58) : RGB(224, 231, 239));
    }
    // The viewport is a separate clipped child window. Invalidating only the
    // frame leaves its old pixels behind across theme/RTL/DPI transitions.
    RedrawWindow(window_.load(), nullptr, nullptr,
      RDW_INVALIDATE | RDW_ERASE | RDW_ALLCHILDREN | RDW_FRAME);
  }

  UINT EffectiveDpi() const { return dpi_ * (preview_ ? preview_scale_ : 1); }
  int Scale(int value) const { return MulDiv(value, static_cast<int>(EffectiveDpi()), 96); }
  bool PreviewContrast() const { return preview_ && preview_theme_ == 2; }

  static bool IsSecondaryText(int id) {
    return id == version || id == scope || id == folder_label || id == language_label;
  }

  COLORREF MutedColor() const {
    return high_contrast_ ? SystemColor(COLOR_WINDOWTEXT) : dark_ ? RGB(155, 166, 183) : RGB(98, 111, 129);
  }

  COLORREF FieldColor() const {
    return high_contrast_ ? SystemColor(COLOR_WINDOW) : dark_ ? RGB(34, 41, 52) : RGB(255, 255, 255);
  }

  void SizeToWorkspace() {
    const HWND hwnd = window_.load();
    MONITORINFO work{sizeof(work)};
    GetMonitorInfoW(MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST), &work);
    RECT outer{0, 0, Scale(680), Scale(480)};
    AdjustWindowRectExForDpi(&outer, static_cast<DWORD>(GetWindowLongPtrW(hwnd, GWL_STYLE)), FALSE,
      static_cast<DWORD>(GetWindowLongPtrW(hwnd, GWL_EXSTYLE)), dpi_);
    const int width = (std::min)(outer.right - outer.left, work.rcWork.right - work.rcWork.left);
    const int height = (std::min)(outer.bottom - outer.top, work.rcWork.bottom - work.rcWork.top);
    SetWindowPos(hwnd, nullptr, work.rcWork.left + (work.rcWork.right - work.rcWork.left - width) / 2,
      work.rcWork.top + (work.rcWork.bottom - work.rcWork.top - height) / 2, width, height,
      SWP_NOZORDER | SWP_NOACTIVATE);
  }

  int TextHeight(std::wstring_view text, int width, HFONT font) const {
    const HDC dc = GetDC(window_.load());
    const auto previous = SelectObject(dc, font);
    RECT bounds{0, 0, Scale((std::max)(1, width)), 0};
    DrawTextW(dc, text.data(), static_cast<int>(text.size()), &bounds, DT_CALCRECT | DT_WORDBREAK | DT_NOPREFIX);
    SelectObject(dc, previous); ReleaseDC(window_.load(), dc);
    return MulDiv(bounds.bottom, 96, static_cast<int>(EffectiveDpi())) + 1;
  }

  int NaturalWidth(int id, int padding = 24) const {
    const HDC dc = GetDC(window_.load());
    const auto previous = SelectObject(dc, font_);
    RECT bounds{};
    const auto text = ReadControl(C(id));
    DrawTextW(dc, text.c_str(), -1, &bounds, DT_CALCRECT | DT_SINGLELINE | DT_NOPREFIX);
    SelectObject(dc, previous); ReleaseDC(window_.load(), dc);
    return MulDiv(bounds.right, 96, static_cast<int>(EffectiveDpi())) + padding;
  }

  std::wstring OptionRowText(const OptionResultRow& row) const {
    return L(row.key) + (row.include_status ? L": " + L(row.done ? "option_done" : "option_failed") : L"");
  }

  std::wstring OptionSummary() const {
    std::wstring result;
    for (const auto& row : option_rows_) { if (!result.empty()) result += L"\r\n"; result += OptionRowText(row); }
    return result;
  }

  int NoticeHeight(int width) const {
    int height = 24;
    for (const auto& row : option_rows_)
      height += (std::max)(22, TextHeight(OptionRowText(row), width - 32, row.done ? secondary_font_ : strong_font_)) + 4;
    return height - 4;
  }

  COLORREF SystemColor(int color) const {
    if (!PreviewContrast()) return GetSysColor(color);
    switch (color) {
      case COLOR_WINDOW: case COLOR_BTNFACE: case COLOR_HIGHLIGHTTEXT: return RGB(0, 0, 0);
      case COLOR_HIGHLIGHT: return RGB(255, 255, 0);
      case COLOR_GRAYTEXT: return RGB(145, 145, 145);
      default: return RGB(255, 255, 255);
    }
  }

  void AccessibleName(HWND control, std::string_view key) {
    IAccPropServices* service = nullptr;
    if (SUCCEEDED(CoCreateInstance(CLSID_AccPropServices, nullptr, CLSCTX_INPROC_SERVER,
                                    IID_PPV_ARGS(&service)))) {
      service->SetHwndPropStr(control, static_cast<DWORD>(OBJID_CLIENT), CHILDID_SELF, PROPID_ACC_NAME, L(key).c_str());
      service->Release();
    }
  }

  void Place(int id, int x, int y, int width, int height, bool body = true) {
    MoveWindow(C(id), Scale(x), Scale(y - (body ? scroll_ + 72 : 0)), Scale(width), Scale(height), TRUE);
  }

  void Layout() {
    if (layout_active_) return;
    layout_active_ = true;
    RECT area{}; GetClientRect(window_.load(), &area);
    const int width = MulDiv(area.right, 96, static_cast<int>(EffectiveDpi()));
    const int height = MulDiv(area.bottom, 96, static_cast<int>(EffectiveDpi()));
    const int content = (std::max)(200, width - 64);
    const int title_height = (std::max)(42, TextHeight(ReadControl(C(title)), content, title_font_));
    const int header_extra = title_height - 42;
    const bool second = IsWindowVisible(C(secondary)) != FALSE;
    const bool third = IsWindowVisible(C(tertiary)) != FALSE;
    const bool stack_footer = content < 360;
    const int third_width = third ? (stack_footer ? content : (std::min)(150, (std::max)(92, NaturalWidth(tertiary)))) : 0;
    const int available = content - (third ? third_width + 12 : 0) - (second ? 12 : 0);
    const int button_width = stack_footer ? content : (std::max)(80, (std::min)(200, available / (second ? 2 : 1)));
    int button_height = (std::max)(48, TextHeight(ReadControl(C(primary)), button_width - 24, font_) + 16);
    if (second) button_height = (std::max)(button_height, TextHeight(ReadControl(C(secondary)), button_width - 24, font_) + 16);
    if (third) button_height = (std::max)(button_height, TextHeight(ReadControl(C(tertiary)), third_width - 24, font_) + 16);
    const int footer_rows = stack_footer ? 1 + (second ? 1 : 0) + (third ? 1 : 0) : 1;
    const int footer_height = button_height * footer_rows + 12 * (footer_rows - 1) + 30;
    const int footer = height - footer_height;
    MoveWindow(body_, 0, Scale(72 + header_extra), area.right,
      Scale((std::max)(1, footer - 72 - header_extra)), TRUE);
    Place(title, 32, 27, content, title_height, false);
    const int subtitle_height = (std::max)(24, TextHeight(ReadControl(C(subtitle)), content, font_));
    Place(subtitle, 32, 79, content, subtitle_height);
    int end = 79 + subtitle_height;
    if (page_ == Page::ready) {
      int y = end + 12;
      const int version_height = (std::max)(22, TextHeight(ReadControl(C(version)), content, secondary_font_));
      Place(version, 32, y, content, version_height); y += version_height + 16;
      const int label_height = (std::max)(18, TextHeight(ReadControl(C(folder_label)), content, secondary_font_));
      Place(folder_label, 32, y, content, label_height); y += label_height + 4;
      const int change_width = IsWindowVisible(C(change_folder)) ? (std::min)(content / 2, NaturalWidth(change_folder)) : 0;
      const int browse_width = folder_expanded_ ? (std::min)(130, NaturalWidth(browse)) : 0;
      const int path_width = content - (change_width ? change_width + 12 : 0) - (browse_width ? browse_width + 8 : 0);
      Place(folder_summary, 32, y + 8, path_width, 24);
      Place(folder_edit, 32, y, path_width, 36);
      Place(browse, 32 + path_width + 8, y, browse_width, 36);
      Place(change_folder, 32 + content - change_width, y, change_width, 36); y += 52;
      const int language_height = (std::max)(18, TextHeight(ReadControl(C(language_label)), content, secondary_font_));
      Place(language_label, 32, y, content, language_height); y += language_height + 4;
      Place(language, 32, y, (std::min)(260, content), 280);
      // CB_SETITEMHEIGHT sizes the selection, not its native border. Measure
      // the actual chrome so the closed field has a 36 logical-pixel target.
      RECT combo{}; GetWindowRect(C(language), &combo);
      const int selection = static_cast<int>(SendMessageW(C(language), CB_GETITEMHEIGHT, static_cast<WPARAM>(-1), 0));
      const int chrome = combo.bottom - combo.top - selection;
      SendMessageW(C(language), CB_SETITEMHEIGHT, static_cast<WPARAM>(-1), (std::max)(Scale(20), Scale(36) - chrome));
      y += 52;
      const int license_width = (std::min)(content / 2, NaturalWidth(license));
      const int accept_width = content - license_width - 20;
      const int consent_height = (std::max)(44, TextHeight(ReadControl(C(accept)), accept_width - 36, font_) + 12);
      Place(license, 32, y + (consent_height - 36) / 2, license_width, 36);
      Place(accept, 32 + license_width + 20, y, accept_width, consent_height); y += consent_height + 12;
      const int scope_height = (std::max)(26, TextHeight(ReadControl(C(scope)), content, secondary_font_));
      Place(scope, 32, y, content, scope_height);
      end = (mode_ == Mode::install || mode_ == Mode::upgrade) ? y + scope_height : 79 + subtitle_height + 12 + version_height;
    } else if (page_ == Page::complete || page_ == Page::options_failed) {
      int y = end + 12;
      const int scope_height = (std::max)(22, TextHeight(ReadControl(C(scope)), content, secondary_font_));
      Place(scope, 32, y, content, scope_height); y += scope_height + 10;
      const int option_width = (std::min)(content, (std::max)({280, NaturalWidth(desktop, 44), NaturalWidth(launch_app, 44), NaturalWidth(startup, 44)}));
      for (int id : {desktop, launch_app}) {
        const int row = (std::max)(44, TextHeight(ReadControl(C(id)), option_width - 36, font_) + 12);
        Place(id, 32, y, option_width, row); y += row;
      }
      Place(more, 32, y, (std::min)(content, NaturalWidth(more, 42)), 36);
      Place(startup, 32, y + 36, option_width, (std::max)(44, TextHeight(ReadControl(C(startup)), option_width - 36, font_) + 12));
      end = y + 36 + (expanded_ ? (std::max)(44, TextHeight(ReadControl(C(startup)), option_width - 36, font_) + 12) : 0);
      if (page_ == Page::options_failed) {
        const int notice_height = NoticeHeight(content);
        Place(option_summary, 32, y + 14, content, notice_height);
        end = y + 14 + notice_height;
        Place(details, 32, end + 12, content, 70);
        Place(save_details, 32, end + 94, (std::min)(content, NaturalWidth(save_details)), 36);
        if (details_expanded_) end += 130;
      }
    } else if (page_ == Page::license) {
      const int license_height = (std::max)(90, footer - header_extra - end - 28);
      Place(license_text, 32, end + 12, content, license_height);
      end += 12 + license_height;
    } else if (page_ == Page::working || page_ == Page::detecting) {
      Place(progress, 32, end + 32, content, 12);
      const int status_height = (std::max)(44, TextHeight(ReadControl(C(status)), content, font_));
      Place(status, 32, end + 60, content, status_height); end += 60 + status_height;
    } else {
      Place(details, 32, end + 24, content, 120);
      Place(save_details, 32, end + 156, (std::min)(content, NaturalWidth(save_details)), 36);
      if (page_ == Page::files_in_use) end += 144;
      else if (details_expanded_) end += 192;
    }
    const int needed = end + 16 + header_extra + footer_height;
    SCROLLINFO scrolling{sizeof(scrolling), SIF_RANGE | SIF_PAGE | SIF_POS};
    scrolling.nMax = needed - 1; scrolling.nPage = (std::max)(1, height);
    const int previous_scroll = scroll_;
    scroll_ = (std::max)(0, (std::min)(scroll_, needed - height));
    scrolling.nPos = scroll_;
    SetScrollInfo(window_.load(), SB_VERT, &scrolling, TRUE);
    Place(primary, width - 32 - button_width, height - button_height - 20, button_width, button_height, false);
    Place(secondary, stack_footer ? 32 : width - 44 - button_width * 2,
      stack_footer ? height - button_height * 2 - 32 : footer + 10, button_width, button_height, false);
    Place(tertiary, 32, footer + 10, third_width, button_height, false);
    layout_active_ = false;
    if (previous_scroll != scroll_) Layout();
  }

  void Render() {
    ApplyDirection();
    for (int id = title; id <= tertiary; ++id) Show(id, false);
    EnableWindow(C(secondary), !options_busy_);
    EnableWindow(C(tertiary), !options_busy_);
    Show(title); Show(subtitle); Show(primary);
    Text(primary, "close"); Text(secondary, "cancel");
    const std::string_view heading = page_ == Page::detecting ? "detecting" :
      page_ == Page::license ? "license" : page_ == Page::files_in_use ? "uninstall_files_in_use" : page_ == Page::working ?
        (action_ == BOOTSTRAPPER_ACTION_UNINSTALL ? "uninstalling" : "installing") :
      page_ == Page::complete ? (finish_.reboot ? "reboot_title" : "complete_title") :
      page_ == Page::failed ? "failed_title" : page_ == Page::options_failed ? "complete_title" :
      mode_ == Mode::maintenance ? "maintenance_title" : mode_ == Mode::residual ? "residual_title" :
      mode_ == Mode::upgrade ? (current_version_ == target_version_ ? "reinstall" : "upgrade_title") : "install_title";
    Text(title, heading);
    Text(subtitle, page_ == Page::complete ? (finish_.reboot ? "reboot_description" : "complete_description") :
      page_ == Page::options_failed ? "options_failed" : page_ == Page::failed ? failure_key_ :
      page_ == Page::working ? "install_scope" : page_ == Page::detecting ? "detecting" :
      mode_ == Mode::maintenance ? "maintenance_description" : mode_ == Mode::residual ? "residual_description" :
      mode_ == Mode::upgrade ? "upgrade_description" : "install_description");
    if (preview_) SetWindowTextW(window_.load(), (L("setup_title") + L" — " + L("preview")).c_str());
    else SetWindowTextW(window_.load(), L("setup_title").c_str());
    if (page_ == Page::ready) {
      Show(version);
      std::wstring versions = current_version_.empty() ? L"" : L("current_version") + L": " + current_version_ + L"   ";
      versions += L("target_version") + L": " + target_version_;
      SetWindowTextW(C(version), versions.c_str());
      if (mode_ == Mode::install || mode_ == Mode::upgrade) {
        for (int id : {folder_label, language_label, language, license, accept, scope, secondary}) Show(id);
        Show(folder_summary, !folder_expanded_); Show(folder_edit, folder_expanded_);
        Show(browse, folder_expanded_); Show(change_folder, mode_ == Mode::install);
        SetWindowTextW(C(folder_summary), ReadControl(C(folder_edit)).c_str());
        Text(change_folder, folder_expanded_ ? "back" : "change_folder");
        Text(folder_label, "install_folder"); Text(browse, "browse"); Text(language_label, "language");
        AccessibleName(C(folder_edit), "install_folder"); AccessibleName(C(language), "language");
        Text(license, "license"); Text(accept, "license_accept"); Text(scope, "install_scope");
        Text(primary, mode_ == Mode::upgrade ? (current_version_ == target_version_ ? "reinstall" : "upgrade") : "install");
        // Moving an existing product during upgrade breaks the recovery bridge.
        EnableWindow(C(folder_edit), mode_ == Mode::install);
        EnableWindow(C(browse), mode_ == Mode::install);
        EnableWindow(C(primary), CanInstall(mode_, Checked(accept), ValidInstallFolder(ReadControl(C(folder_edit)))));
      } else {
        EnableWindow(C(primary), TRUE);
        if (mode_ == Mode::maintenance) {
          Text(primary, "uninstall"); Show(secondary); Text(secondary, "open_app");
          Show(tertiary); Text(tertiary, "close");
        }
        if (mode_ == Mode::residual) { Text(primary, "cleanup"); Show(secondary); Text(secondary, "close"); }
        if (mode_ == Mode::blocked) Text(subtitle, "downgrade_blocked");
      }
    } else if (page_ == Page::license) {
      Show(license_text); Text(primary, "back"); EnableWindow(C(primary), TRUE);
      AccessibleName(C(license_text), "license");
    } else if (page_ == Page::files_in_use) {
      Text(subtitle, "close_apps_save_work"); Show(details); SetWindowTextW(C(details), files_in_use_.c_str());
      AccessibleName(C(details), "uninstall_files_in_use");
      Text(primary, files_source_ == BOOTSTRAPPER_FILES_IN_USE_TYPE_MSI_RM ?
        "uninstall_close_apps" : "uninstall_check_again");
      Show(secondary); Text(secondary, "cancel"); EnableWindow(C(primary), TRUE);
      EnableWindow(C(secondary), ChooseCloseAction(page_, cancel_stage_, cancel_requested_, options_busy_, native_cancel_enabled_) == CloseAction::request_cancel);
    } else if (page_ == Page::working || page_ == Page::detecting) {
      Show(progress); Show(status); Text(status, status_key_); Text(primary, "cancel");
      AccessibleName(C(progress), status_key_);
      EnableWindow(C(primary), ChooseCloseAction(page_, cancel_stage_, cancel_requested_, options_busy_, native_cancel_enabled_) == CloseAction::request_cancel);
      SendMessageW(C(progress), PBM_SETPOS, percent_, 0);
    } else if (page_ == Page::complete || page_ == Page::options_failed) {
      EnableWindow(C(primary), !options_busy_);
      if (finish_.reboot && action_ == BOOTSTRAPPER_ACTION_UNINSTALL) {
        Text(primary, reboot_confirmation_ ? "back" : "restart_now");
        Show(secondary); Text(secondary, reboot_confirmation_ ? "restart_now" : "restart_later");
        if (reboot_confirmation_) Text(subtitle, "save_work");
      } else if (action_ == BOOTSTRAPPER_ACTION_UNINSTALL) {
        Text(title, "done"); Text(subtitle, "retained_data"); Text(primary, "close");
      } else {
        Show(scope); Text(scope, "current_user");
        Show(desktop); Show(launch_app); Show(more);
        Text(desktop, "desktop_shortcut"); Text(launch_app, "launch_app"); Text(more, "more_options");
        Show(startup, expanded_); Text(startup, "start_on_login");
        Text(primary, Checked(launch_app) ? "finish_open" : "finish");
        EnableWindow(C(desktop), !options_busy_ && !finish_.desktop_done);
        EnableWindow(C(launch_app), !options_busy_ && !finish_.reboot);
        EnableWindow(C(startup), !options_busy_ && startup_known_ && !finish_.startup_done);
        if (page_ == Page::options_failed) {
          Text(primary, "options_retry"); Show(secondary); Text(secondary, "skip_finish");
          Show(option_summary); SetWindowTextW(C(option_summary), OptionSummary().c_str());
          Show(tertiary); Text(tertiary, "details");
          Show(details, details_expanded_); SetErrorText();
          Show(save_details, details_expanded_); Text(save_details, "save_details");
          // Results replace the optional section, avoiding overlapping controls.
          Show(startup, false); Show(more, false);
          if (finish_.reboot) SetWindowTextW(C(subtitle), (L("options_failed") + L"\r\n" + L("reboot_title")).c_str());
        } else if (finish_.reboot) {
          Text(primary, reboot_confirmation_ ? "back" : "restart_now");
          Show(secondary); Text(secondary, reboot_confirmation_ ? "restart_now" : "restart_later");
          if (reboot_confirmation_) Text(subtitle, "save_work");
        }
      }
    } else if (page_ == Page::failed) {
      Show(tertiary); Text(tertiary, "details");
      Show(details, details_expanded_); SetErrorText();
      Show(save_details, details_expanded_); Text(save_details, "save_details");
      if (finish_.reboot) {
        SetWindowTextW(C(subtitle), (reboot_confirmation_ ? L("save_work") :
          L("reboot_title") + L"\r\n" + L("save_work")).c_str());
        Text(primary, reboot_confirmation_ ? "back" : "restart_now");
        Show(secondary); Text(secondary, reboot_confirmation_ ? "restart_now" : "restart_later");
      } else if (CanRetryFailure(terminal_failure_, finish_.reboot)) {
        Text(primary, "retry"); Show(secondary); Text(secondary, "close");
      }
      EnableWindow(C(primary), TRUE);
    }
    Layout();
    ReorderTabControls();
    InvalidateRect(window_.load(), nullptr, TRUE);
  }

  void ApplyDirection() {
    const bool rtl = culture_ == L"ar-SA" || culture_ == L"fa-IR";
    if (rtl == rtl_) return;
    rtl_ = rtl;
    const auto direction = [rtl](HWND handle, bool preserve_ltr = false) {
      const LONG_PTR style = GetWindowLongPtrW(handle, GWL_EXSTYLE);
      SetWindowLongPtrW(handle, GWL_EXSTYLE, rtl && !preserve_ltr ?
        style | WS_EX_LAYOUTRTL : style & ~static_cast<LONG_PTR>(WS_EX_LAYOUTRTL));
      SetWindowPos(handle, nullptr, 0, 0, 0, 0,
        SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED);
    };
    direction(window_.load()); direction(body_);
    for (int id = title; id <= tertiary; ++id)
      direction(C(id), id == folder_edit || id == folder_summary || id == license_text);
  }

  void ReorderTabControls() {
    HWND after = HWND_TOP;
    HWND first = nullptr;
    // Native dialog traversal follows sibling z-order. Keep it aligned with
    // visual order even as folded sections and whole pages become visible.
    for (int id : {folder_edit, browse, change_folder, language, license, accept,
                   desktop, launch_app, more, startup, license_text, details, save_details}) {
      const HWND child = C(id);
      if (!IsWindowVisible(child)) continue;
      SetWindowPos(child, after, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
      after = child;
      if (!first && IsWindowEnabled(child)) first = child;
    }
    if (!first) for (int id : {primary, secondary, tertiary})
      if (IsWindowVisible(C(id)) && IsWindowEnabled(C(id))) { first = C(id); break; }
    const HWND focused = GetFocus();
    if (first && (last_page_ != page_ || !focused || !IsWindowVisible(focused) || !IsWindowEnabled(focused))) SetFocus(first);
    last_page_ = page_;
    EnsureFocusVisible();
  }

  void EnsureFocusVisible() {
    const HWND focused = GetFocus();
    if (!focused || GetParent(focused) != body_) return;
    RECT control{}, visible{};
    GetWindowRect(focused, &control); GetWindowRect(body_, &visible);
    int delta = 0;
    if (control.top < visible.top) delta = control.top - visible.top;
    else if (control.bottom > visible.bottom) delta = control.bottom - visible.bottom;
    if (delta) {
      SCROLLINFO info{sizeof(info), SIF_ALL}; GetScrollInfo(window_.load(), SB_VERT, &info);
      scroll_ = (std::max)(0, (std::min)(scroll_ + MulDiv(delta, 96, static_cast<int>(EffectiveDpi())),
        info.nMax - static_cast<int>(info.nPage) + 1));
      Layout();
    }
  }

  void SetErrorText() {
    AccessibleName(C(details), "details");
    wchar_t code[32]{};
    swprintf_s(code, L"0x%08lX", static_cast<unsigned long>(result_));
    SetWindowTextW(C(details), (L("uninstall_error_code") + L": " + code).c_str());
  }

  void Fail(HRESULT result, std::string key = "failed_description", bool terminal = false) {
    result_ = result; failure_key_ = std::move(key); terminal_failure_ = terminal;
    details_expanded_ = false;
    page_ = Page::failed; cancel_requested_ = false; options_busy_ = false;
    if (!Interactive()) { Quit(ExitCode(result)); return; }
    Render();
  }

  void BeginPlan(BOOTSTRAPPER_ACTION action) {
    action_ = action; page_ = Page::working; status_key_ = "planning";
    cancel_requested_ = false; percent_ = 0; scroll_ = 0;
    cancel_stage_ = CancelStage::running; native_cancel_enabled_ = true; detail_stage_ = "planning";
    EnterCriticalSection(&m_csCanceled); m_fCanceled = FALSE; LeaveCriticalSection(&m_csCanceled);
    Render();
    if (preview_) { SetTimer(window_.load(), kPreviewTimer, 90, nullptr); return; }
    if (action == BOOTSTRAPPER_ACTION_INSTALL) {
      folder_ = ReadControl(C(folder_edit));
      if (!ValidInstallFolder(folder_)) { Fail(E_INVALIDARG, "invalid_folder"); return; }
      HRESULT hr = m_pEngine->SetVariableString(L"UsqueInstallFolder", folder_.c_str(), FALSE);
      if (FAILED(hr)) { Fail(hr); return; }
      const auto transform = culture_ == L"en-US" ? L"" : L"transforms\\" + culture_ + L".mst";
      hr = m_pEngine->SetVariableString(L"UsqueMsiTransform", transform.c_str(), FALSE);
      if (FAILED(hr)) { Fail(hr); return; }
    }
    const HRESULT result = m_pEngine->Plan(action);
    if (FAILED(result)) Fail(result);
  }

  void Detected(HRESULT result) {
    if (FAILED(result)) { Fail(result); return; }
    const auto code = RegistryString(HKEY_LOCAL_MACHINE, L"Software\\Usque", L"ProductCode");
    detection_.installed_product = !code.empty() && MsiQueryProductStateW(code.c_str()) == INSTALLSTATE_DEFAULT;
    mode_ = SelectMode(detection_);
    if (requested_action_ == BOOTSTRAPPER_ACTION_REPAIR || requested_action_ == BOOTSTRAPPER_ACTION_MODIFY) {
      // Burn may label an ARP maintenance launch Modify; the EXE's explicit
      // repair command is never allowed to reach the MSI's service actions.
      if (requested_action_ == BOOTSTRAPPER_ACTION_REPAIR || !Interactive()) {
        Fail(HRESULT_FROM_WIN32(ERROR_NOT_SUPPORTED), "unsupported_action", true); return;
      }
    }
    if (requested_action_ == BOOTSTRAPPER_ACTION_UNINSTALL) {
      if (Interactive() && detection_.installed_product) { DelegateUninstall(); return; }
      BeginPlan(BOOTSTRAPPER_ACTION_UNINSTALL); return;
    }
    if (requested_action_ != BOOTSTRAPPER_ACTION_INSTALL &&
        requested_action_ != BOOTSTRAPPER_ACTION_MODIFY && requested_action_ != BOOTSTRAPPER_ACTION_UNKNOWN) {
      Fail(HRESULT_FROM_WIN32(ERROR_NOT_SUPPORTED), "unsupported_action", true); return;
    }
    if (mode_ == Mode::blocked) {
      Fail(HRESULT_FROM_WIN32(ERROR_PRODUCT_VERSION), "downgrade_blocked", true); return;
    }
    if (!Interactive()) { BeginPlan(BOOTSTRAPPER_ACTION_INSTALL); return; }
    page_ = Page::ready; Render();
  }

  void DelegateUninstall() {
    if (preview_) { page_ = Page::complete; action_ = BOOTSTRAPPER_ACTION_UNINSTALL; Render(); return; }
    if (!UnelevatedInteractiveUser()) { Fail(E_ACCESSDENIED, "elevated_options", true); return; }
    const auto folder = RegistryString(HKEY_LOCAL_MACHINE, L"Software\\Usque", L"InstallLocation");
    if (!ValidInstallFolder(folder)) { Fail(E_INVALIDARG, "uninstall_helper_failed", true); return; }
    DWORD error = 0;
    const auto helper = (std::filesystem::path(folder) / L"usque-uninstall.exe").wstring();
    if (!Launch(helper, L"--wait-for-pid " + std::to_wstring(GetCurrentProcessId()), &error)) {
      Fail(HRESULT_FROM_WIN32(error), "uninstall_helper_failed"); return;
    }
    Quit(0);
  }

  void Completed(const Event& event) {
    result_ = event.result;
    finish_.reboot = event.number != BOOTSTRAPPER_APPLY_RESTART_NONE;
    if (FAILED(event.result)) { Fail(event.result); return; }
    if (!Interactive()) { Quit(finish_.reboot ? ERROR_SUCCESS_REBOOT_REQUIRED : ERROR_SUCCESS); return; }
    page_ = Page::complete; percent_ = 100; cancel_requested_ = false;
    if (finish_.reboot) { finish_.launch = false; Check(launch_app, false); }
    if (action_ == BOOTSTRAPPER_ACTION_INSTALL && !preview_) QueryOptions();
    Render();
  }

  std::wstring GuiPath() const { return (std::filesystem::path(folder_) / L"usque.exe").wstring(); }

  void QueryOptions() {
    if (worker_.joinable()) worker_.join();
    options_busy_ = true;
    const auto file = GuiPath();
    worker_ = std::thread([this, file] {
      Event event{EventType::queried};
      event.child = RunInstallerOptions(file, true, false, -1);
      Post(std::move(event));
    });
  }

  void FinishOptions(FinishIntent intent = FinishIntent::normal) {
    if (options_busy_) return;
    if (page_ != Page::options_failed || intent != FinishIntent::normal) finish_intent_ = intent;
    finish_.desktop = Checked(desktop); finish_.startup = Checked(startup); finish_.launch = Checked(launch_app);
    if (preview_) {
      if (page_ == Page::options_failed) {
        if (finish_.NeedDesktop()) finish_.desktop_done = true;
        if (finish_.NeedStartup()) finish_.startup_done = true;
        page_ = Page::complete; details_expanded_ = false; Render();
      } else Quit(0);
      return;
    }
    if ((finish_.OptionsPending() || finish_.NeedLaunch()) && !UnelevatedInteractiveUser()) {
      result_ = E_ACCESSDENIED; page_ = Page::options_failed; failure_key_ = "elevated_options";
      reconfirm_restart_ = finish_.reboot && finish_intent_ == FinishIntent::restart_now;
      reboot_confirmation_ = false;
      option_rows_ = {{"elevated_options", false, false}}; details_expanded_ = false; Render(); return;
    }
    if (!finish_.OptionsPending()) { FinishLaunch(); return; }
    if (worker_.joinable()) worker_.join();
    options_busy_ = true; reboot_confirmation_ = false; Render();
    const auto file = GuiPath();
    const bool create_desktop = finish_.NeedDesktop();
    const int startup_choice = finish_.NeedStartup() ? (finish_.startup ? 1 : 0) : -1;
    worker_ = std::thread([this, file, create_desktop, startup_choice] {
      Event event{EventType::options};
      event.child = RunInstallerOptions(file, false, create_desktop, startup_choice);
      Post(std::move(event));
    });
  }

  void FinishLaunch() {
    if (finish_.reboot) {
      const auto action = ResolveRebootFinish(finish_intent_ == FinishIntent::restart_now, reconfirm_restart_);
      if (action == RebootFinishAction::confirm_again) {
        reconfirm_restart_ = false; finish_intent_ = FinishIntent::normal;
        reboot_confirmation_ = false; page_ = Page::complete; Render(); return;
      }
      restart_requested_ = action == RebootFinishAction::restart && !preview_;
      Quit(ERROR_SUCCESS_REBOOT_REQUIRED); return;
    }
    if (finish_.NeedLaunch()) {
      if (!UnelevatedInteractiveUser()) {
        launch_failed_ = true; result_ = E_ACCESSDENIED;
        option_rows_ = {{"elevated_options", false, false}};
        page_ = Page::options_failed; details_expanded_ = false; Render(); return;
      }
      DWORD error = 0;
      if (!Launch(GuiPath(), L"", &error)) {
        launch_failed_ = true;
        result_ = HRESULT_FROM_WIN32(error); failure_key_ = "launch_failed";
        option_rows_ = {{"launch_app", false}};
        page_ = Page::options_failed; details_expanded_ = false; Render(); return;
      }
      finish_.launch_done = true;
      launch_failed_ = false;
    }
    Quit(finish_.reboot ? ERROR_SUCCESS_REBOOT_REQUIRED : ERROR_SUCCESS);
  }

  void SkipFailedOptions() {
    if (options_busy_) return;
    // This explicit completion action differs from closing the window: honor
    // the launch choice, but never silently retry a launch that already failed.
    finish_.launch = LaunchAfterSkip(Checked(launch_app), launch_failed_, finish_.reboot);
    if (!finish_.desktop_done) { finish_.desktop = false; Check(desktop, false); }
    if (!finish_.startup_done) { finish_.startup = finish_.initial_startup; Check(startup, finish_.startup); }
    finish_intent_ = FinishIntent::restart_later;
    reconfirm_restart_ = false; reboot_confirmation_ = false;
    if (preview_) { Quit(0); return; }
    FinishLaunch();
  }

  void OnEvent(Event& event) {
    switch (event.type) {
      case EventType::detected: Detected(event.result); break;
      case EventType::planned:
        if (FAILED(event.result)) Fail(event.result);
        else { status_key_ = action_ == BOOTSTRAPPER_ACTION_UNINSTALL ? "uninstalling" : "installing";
          Render(); const HRESULT result = m_pEngine->Apply(window_.load()); if (FAILED(result)) Fail(result); }
        break;
      case EventType::applied: Completed(event); break;
      case EventType::progress:
        percent_ = (std::min)(99UL, event.number);
        SendMessageW(C(progress), PBM_SETPOS, percent_, 0); break;
      case EventType::status:
        if (!cancel_requested_) { status_key_ = event.key; Text(status, status_key_); }
        AccessibleName(C(progress), status_key_);
        if (event.key != "failed_description") detail_stage_ = event.key;
        if (page_ == Page::working || page_ == Page::detecting)
          EnableWindow(C(primary), ChooseCloseAction(page_, cancel_stage_, cancel_requested_, options_busy_, native_cancel_enabled_) == CloseAction::request_cancel);
        if (FAILED(event.result)) result_ = event.result;
        break;
      case EventType::failed: Fail(event.result); break;
      case EventType::cancel_permission:
        if (page_ == Page::working || page_ == Page::detecting)
          EnableWindow(C(primary), ChooseCloseAction(page_, cancel_stage_, cancel_requested_, options_busy_, native_cancel_enabled_) == CloseAction::request_cancel);
        if (page_ == Page::files_in_use)
          EnableWindow(C(secondary), ChooseCloseAction(page_, cancel_stage_, cancel_requested_, options_busy_, native_cancel_enabled_) == CloseAction::request_cancel);
        break;
      case EventType::files_in_use:
        files_source_ = static_cast<BOOTSTRAPPER_FILES_IN_USE_TYPE>(event.number);
        files_in_use_ = std::move(event.text); page_ = Page::files_in_use; Render(); break;
      case EventType::queried:
        options_busy_ = false;
        // Only an explicit successful query may initialize startup state.
        {
          const auto report = ResultParser(event.child.output).Parse();
          startup_known_ = report.valid && report.startup.Success() && report.startup.enabled.has_value();
          if (startup_known_) {
            finish_.initial_startup = *report.startup.enabled;
            finish_.startup = finish_.initial_startup; Check(startup, finish_.startup);
          }
        }
        Render(); break;
      case EventType::options:
        options_busy_ = false;
        // Per-option result parsing is deliberately separate from process exit:
        // a failed sibling must never cause a completed option to be repeated.
        {
          const auto report = ResultParser(event.child.output).Parse();
          if (report.valid && (report.status == "ok" || report.status == "partial")) {
            if (finish_.NeedDesktop() && report.desktop.Success() && report.desktop.enabled == true)
              finish_.desktop_done = true;
            if (finish_.NeedStartup() && report.startup.Success() && report.startup.enabled == finish_.startup)
              finish_.startup_done = true;
          }
          option_rows_.clear();
          if (finish_.desktop)
            option_rows_.push_back({"desktop_shortcut", finish_.desktop_done});
          if (finish_.startup != finish_.initial_startup)
            option_rows_.push_back({"start_on_login", finish_.startup_done});
          if (!finish_.OptionsPending() && report.valid) {
          FinishLaunch();
          } else {
            result_ = HRESULT_FROM_WIN32(event.child.code == 0 ? ERROR_INVALID_DATA : event.child.code);
            reconfirm_restart_ = finish_.reboot && finish_intent_ == FinishIntent::restart_now;
            reboot_confirmation_ = false;
            page_ = Page::options_failed; details_expanded_ = false; Render();
          }
        }
        break;
    }
  }

  void ShowLicense() {
    const auto license_data = ReadLicense();
    if (license_data.empty()) { Fail(HRESULT_FROM_WIN32(ERROR_FILE_NOT_FOUND), "license_missing"); return; }
    RtfStream data{license_data};
    EDITSTREAM stream{reinterpret_cast<DWORD_PTR>(&data), 0, ReadRtf};
    SendMessageW(C(license_text), EM_STREAMIN, SF_RTF, reinterpret_cast<LPARAM>(&stream));
    if (stream.dwError) { Fail(HRESULT_FROM_WIN32(stream.dwError), "license_missing"); return; }
    page_ = Page::license; scroll_ = 0; Render(); SetFocus(C(license_text));
  }

  void Command(int id, int notification) {
    if (id == IDCANCEL) { Close(); return; }
    if (id == IDOK) id = primary;
    if (id == primary && !IsWindowEnabled(C(primary))) return;
    if (id == folder_edit && notification == EN_CHANGE && page_ == Page::ready) {
      EnableWindow(C(primary), CanInstall(mode_, Checked(accept), ValidInstallFolder(ReadControl(C(folder_edit)))));
      return;
    }
    if (id == language && notification == CBN_SELENDCANCEL) {
      for (size_t index = 0; index < std::size(kLanguages); ++index)
        if (kLanguages[index].code == culture_) SendMessageW(C(language), CB_SETCURSEL, index, 0);
      return;
    }
    if (id == language && (notification == CBN_SELENDOK ||
        (notification == CBN_SELCHANGE && !SendMessageW(C(language), CB_GETDROPPEDSTATE, 0, 0)))) {
      const auto index = static_cast<size_t>(SendMessageW(C(language), CB_GETCURSEL, 0, 0));
      if (index < std::size(kLanguages) && kLanguages[index].code != culture_) {
        culture_ = kLanguages[index].code; Render(); SetFocus(C(language)); EnsureFocusVisible();
      }
      return;
    }
    if (notification != BN_CLICKED) return;
    if (id == accept || id == launch_app) { Render(); return; }
    if (id == more) { expanded_ = !expanded_; Render(); return; }
    if (id == change_folder) { folder_expanded_ = !folder_expanded_; Render(); if (folder_expanded_) SetFocus(C(folder_edit)); return; }
    if (id == browse) {
      const auto selected = SelectFolder(window_.load(), ReadControl(C(folder_edit)));
      if (!selected.empty()) SetWindowTextW(C(folder_edit), selected.c_str());
      return;
    }
    if (id == license) { ShowLicense(); return; }
    if (id == save_details) {
      if (preview_) { Text(save_details, "done"); return; }
      const std::wstring phase(detail_stage_.begin(), detail_stage_.end());
      const HRESULT saved = SaveDetails(window_.load(), SanitizedDetails(target_version_, phase, static_cast<unsigned>(result_)));
      if (SUCCEEDED(saved)) Text(save_details, "done");
      else if (saved != HRESULT_FROM_WIN32(ERROR_CANCELLED)) {
        wchar_t code[32]{}; swprintf_s(code, L"0x%08lX", static_cast<unsigned long>(saved));
        SetWindowTextW(C(details), (ReadControl(C(details)) + L"\r\n" + L("save_details") + L" " + code).c_str());
      }
      return;
    }
    if (id == secondary && reboot_confirmation_ && finish_.reboot &&
        (page_ == Page::complete || page_ == Page::failed)) {
      if (page_ == Page::complete && action_ == BOOTSTRAPPER_ACTION_INSTALL) FinishOptions(FinishIntent::restart_now);
      else { restart_requested_ = !preview_; Quit(TerminalExitCode(page_, ExitCode(result_), true)); }
      return;
    }
    if (id == secondary && page_ == Page::ready && mode_ == Mode::maintenance) {
      if (preview_) { Quit(0); return; }
      if (!UnelevatedInteractiveUser()) { Fail(E_ACCESSDENIED, "elevated_options"); return; }
      DWORD error = 0;
      if (Launch(GuiPath(), L"", &error)) Quit(0);
      else Fail(HRESULT_FROM_WIN32(error), "launch_failed");
      return;
    }
    if (id == tertiary && (page_ == Page::failed || page_ == Page::options_failed)) {
      details_expanded_ = !details_expanded_; Render(); return;
    }
    if (id == secondary && page_ == Page::options_failed) { SkipFailedOptions(); return; }
    if (id == secondary || id == tertiary) { Close(); return; }
    if (id != primary) return;
    if (page_ == Page::license) { page_ = Page::ready; Render(); return; }
    if (page_ == Page::files_in_use) {
      choice_result_ = files_source_ == BOOTSTRAPPER_FILES_IN_USE_TYPE_MSI_RM ? IDOK : IDRETRY;
      page_ = Page::working; Render(); SetEvent(choice_event_); return;
    }
    if (page_ == Page::detecting || page_ == Page::working) { Close(); return; }
    if (finish_.reboot && (page_ == Page::complete || page_ == Page::failed)) {
      reboot_confirmation_ = !reboot_confirmation_; Render(); SetFocus(C(primary));
      return;
    }
    if (page_ == Page::complete || page_ == Page::options_failed) {
      if (action_ == BOOTSTRAPPER_ACTION_UNINSTALL) Quit(0);
      else FinishOptions();
      return;
    }
    if (page_ == Page::failed) {
      if (!CanRetryFailure(terminal_failure_, finish_.reboot)) Quit(ExitCode(result_));
      else if (preview_) SetupPreview();
      else {
        EnterCriticalSection(&m_csCanceled); m_fCanceled = FALSE; LeaveCriticalSection(&m_csCanceled);
        cancel_stage_ = CancelStage::before_start;
        native_cancel_enabled_ = true;
        page_ = Page::detecting; status_key_ = "detecting"; Render();
        const HRESULT hr = m_pEngine->Detect(window_.load()); if (FAILED(hr)) Fail(hr);
      }
      return;
    }
    if (mode_ == Mode::maintenance) DelegateUninstall();
    else if (mode_ == Mode::residual) BeginPlan(BOOTSTRAPPER_ACTION_UNINSTALL);
    else if (mode_ == Mode::blocked) Quit(ERROR_PRODUCT_VERSION);
    else if (CanInstall(mode_, Checked(accept), ValidInstallFolder(ReadControl(C(folder_edit))))) BeginPlan(BOOTSTRAPPER_ACTION_INSTALL);
  }

  void Close() {
    if (options_busy_) return;
    if (reboot_confirmation_ && finish_.reboot) {
      reboot_confirmation_ = false; Render(); SetFocus(C(primary)); return;
    }
    if (page_ == Page::complete && finish_.reboot && action_ == BOOTSTRAPPER_ACTION_INSTALL) {
      FinishOptions(FinishIntent::restart_later); return;
    }
    const auto action = ChooseCloseAction(page_, cancel_stage_, cancel_requested_, options_busy_, native_cancel_enabled_);
    if (action == CloseAction::wait) return;
    if (page_ == Page::files_in_use) {
      choice_result_ = IDCANCEL; page_ = Page::working; SetEvent(choice_event_);
    }
    if (action == CloseAction::request_cancel) {
      cancel_requested_ = true; status_key_ = "cancel_requested";
      PromptCancel(window_.load(), TRUE, nullptr, nullptr);
      if (preview_) { KillTimer(window_.load(), kPreviewTimer); Fail(HRESULT_FROM_WIN32(ERROR_INSTALL_USEREXIT)); }
      else Render();
      return;
    }
    Quit(TerminalExitCode(page_, ExitCode(result_), finish_.reboot));
  }

  void Quit(DWORD code) {
    if (m_pEngine) m_pEngine->Quit(code);
    DestroyWindow(window_.load());
  }

  static LRESULT CALLBACK BodyProc(HWND hwnd, UINT message, WPARAM wparam, LPARAM lparam,
                                   UINT_PTR, DWORD_PTR reference) {
    auto* app = reinterpret_cast<Application*>(reference);
    switch (message) {
      case WM_COMMAND: case WM_DRAWITEM: case WM_MEASUREITEM: case WM_CTLCOLORSTATIC: case WM_CTLCOLORBTN:
      case WM_CTLCOLOREDIT: case WM_CTLCOLORLISTBOX:
        return SendMessageW(app->window_.load(), message, wparam, lparam);
      case WM_ERASEBKGND: {
        RECT area{}; GetClientRect(hwnd, &area);
        FillRect(reinterpret_cast<HDC>(wparam), &area, app->background_);
        return 1;
      }
      case WM_PAINT: {
        PAINTSTRUCT paint{};
        const HDC dc = BeginPaint(hwnd, &paint);
        // WS_CLIPCHILDREN preserves control pixels while repainting every
        // uncovered part of this scrollable viewport with the current theme.
        FillRect(dc, &paint.rcPaint, app->background_);
        EndPaint(hwnd, &paint);
        return 0;
      }
      default: return DefSubclassProc(hwnd, message, wparam, lparam);
    }
  }

  void DrawButton(const DRAWITEMSTRUCT& draw) {
    const bool enabled = (draw.itemState & ODS_DISABLED) == 0;
    const bool accent = draw.CtlID == primary && enabled;
    const bool disclosure = draw.CtlID == more;
    COLORREF fill = high_contrast_ ? SystemColor(accent ? COLOR_HIGHLIGHT : COLOR_BTNFACE) :
      accent ? RGB(244, 129, 32) : disclosure ? background_color_ : dark_ ? RGB(40, 47, 58) : RGB(238, 241, 245);
    if ((draw.itemState & ODS_SELECTED) && !high_contrast_) fill = accent ? RGB(220, 104, 41) : dark_ ? RGB(59, 68, 81) : RGB(222, 228, 235);
    const COLORREF text = high_contrast_ ? SystemColor(enabled ? (accent ? COLOR_HIGHLIGHTTEXT : COLOR_BTNTEXT) : COLOR_GRAYTEXT) :
      !enabled ? (dark_ ? RGB(132, 141, 153) : RGB(118, 126, 137)) : accent ? RGB(28, 27, 24) : text_color_;
    const HBRUSH brush = CreateSolidBrush(fill);
    const HPEN pen = CreatePen(PS_SOLID, Scale(1), high_contrast_ ? SystemColor(COLOR_WINDOWTEXT) : dark_ ? RGB(75, 84, 98) : RGB(210, 218, 229));
    const auto old_brush = SelectObject(draw.hDC, brush); const auto old_pen = SelectObject(draw.hDC, pen);
    if (disclosure && !high_contrast_) FillRect(draw.hDC, &draw.rcItem, brush);
    else RoundRect(draw.hDC, draw.rcItem.left, draw.rcItem.top, draw.rcItem.right, draw.rcItem.bottom, Scale(8), Scale(8));
    SelectObject(draw.hDC, old_brush); SelectObject(draw.hDC, old_pen); DeleteObject(brush); DeleteObject(pen);
    const auto old_font = SelectObject(draw.hDC, font_);
    SetBkMode(draw.hDC, TRANSPARENT); SetTextColor(draw.hDC, text);
    RECT bounds = draw.rcItem; InflateRect(&bounds, -Scale(10), -Scale(4));
    if (disclosure) bounds.left += Scale(18);
    const auto caption = ReadControl(draw.hwndItem);
    RECT measured{0, 0, bounds.right - bounds.left, 0};
    const UINT alignment = disclosure ? DT_LEFT : DT_CENTER;
    DrawTextW(draw.hDC, caption.c_str(), -1, &measured, DT_CALCRECT | DT_WORDBREAK | alignment | DT_NOPREFIX);
    bounds.top += (std::max)(0L, (bounds.bottom - bounds.top - measured.bottom) / 2);
    DrawTextW(draw.hDC, caption.c_str(), -1, &bounds, DT_WORDBREAK | alignment | DT_NOPREFIX);
    if (disclosure) {
      const HPEN chevron = CreatePen(PS_SOLID, Scale(2), text);
      const auto previous = SelectObject(draw.hDC, chevron);
      const int x = draw.rcItem.left + Scale(10), y = (draw.rcItem.top + draw.rcItem.bottom) / 2;
      MoveToEx(draw.hDC, x, y + (expanded_ ? Scale(2) : -Scale(2)), nullptr);
      LineTo(draw.hDC, x + Scale(4), y + (expanded_ ? -Scale(2) : Scale(2)));
      LineTo(draw.hDC, x + Scale(8), y + (expanded_ ? Scale(2) : -Scale(2)));
      SelectObject(draw.hDC, previous); DeleteObject(chevron);
    }
    if ((draw.itemState & ODS_FOCUS) && !(draw.itemState & ODS_NOFOCUSRECT)) {
      RECT focus = draw.rcItem; InflateRect(&focus, -Scale(4), -Scale(4)); DrawFocusRect(draw.hDC, &focus);
    }
    SelectObject(draw.hDC, old_font);
  }

  void DrawComboItem(const DRAWITEMSTRUCT& draw) {
    const int saved = SaveDC(draw.hDC);
    const bool selected = (draw.itemState & ODS_SELECTED) != 0 && !(draw.itemState & ODS_COMBOBOXEDIT);
    const bool disabled = (draw.itemState & ODS_DISABLED) != 0;
    const COLORREF fill = selected ? (high_contrast_ ? SystemColor(COLOR_HIGHLIGHT) :
      dark_ ? RGB(58, 69, 85) : RGB(227, 234, 243)) : FieldColor();
    const COLORREF ink = disabled ? (high_contrast_ ? SystemColor(COLOR_GRAYTEXT) : MutedColor()) :
      selected && high_contrast_ ? SystemColor(COLOR_HIGHLIGHTTEXT) : text_color_;
    const HBRUSH brush = CreateSolidBrush(fill);
    FillRect(draw.hDC, &draw.rcItem, brush); DeleteObject(brush);
    if (draw.itemID != static_cast<UINT>(-1)) {
      const auto length = SendMessageW(draw.hwndItem, CB_GETLBTEXTLEN, draw.itemID, 0);
      if (length >= 0 && length < 1024) {
        std::wstring caption(static_cast<size_t>(length) + 1, L'\0');
        SendMessageW(draw.hwndItem, CB_GETLBTEXT, draw.itemID, reinterpret_cast<LPARAM>(caption.data()));
        SelectObject(draw.hDC, font_); SetTextColor(draw.hDC, ink); SetBkMode(draw.hDC, TRANSPARENT);
        RECT bounds = draw.rcItem; bounds.left += Scale(12); bounds.right -= Scale(8);
        DrawTextW(draw.hDC, caption.c_str(), -1, &bounds, DT_SINGLELINE | DT_VCENTER | DT_NOPREFIX | DT_END_ELLIPSIS);
      }
    }
    if ((draw.itemState & ODS_FOCUS) && !(draw.itemState & ODS_NOFOCUSRECT)) {
      RECT focus = draw.rcItem; InflateRect(&focus, -Scale(2), -Scale(2)); DrawFocusRect(draw.hDC, &focus);
    }
    RestoreDC(draw.hDC, saved);
  }

  static LRESULT CALLBACK ComboProc(HWND hwnd, UINT message, WPARAM wparam, LPARAM lparam,
                                    UINT_PTR, DWORD_PTR reference) {
    auto* app = reinterpret_cast<Application*>(reference);
    // Retain the native combo, its listbox, strings, keyboard and accessibility.
    // Only its closed-face pixels need replacement; owner-drawn popup rows are
    // handled separately so selecting a language never relies on a fake menu.
    if (message == WM_PAINT && (!app->high_contrast_ || app->PreviewContrast())) {
      PAINTSTRUCT paint{}; const HDC dc = BeginPaint(hwnd, &paint);
      RECT area{}; GetClientRect(hwnd, &area);
      const int saved = SaveDC(dc);
      const HBRUSH fill = CreateSolidBrush(app->FieldColor());
      FillRect(dc, &area, fill); DeleteObject(fill);
      const COLORREF ink = IsWindowEnabled(hwnd) ? app->text_color_ : app->MutedColor();
      const HPEN pen = CreatePen(PS_SOLID, app->Scale(1), app->high_contrast_ ? app->SystemColor(COLOR_WINDOWTEXT) :
        app->dark_ ? RGB(83, 95, 113) : RGB(190, 201, 216));
      SelectObject(dc, pen); SelectObject(dc, GetStockObject(NULL_BRUSH));
      Rectangle(dc, area.left, area.top, area.right, area.bottom);
      COMBOBOXINFO info{sizeof(info)}; GetComboBoxInfo(hwnd, &info);
      RECT bounds = info.rcItem;
      bounds.left += app->Scale(10); bounds.right -= app->Scale(8);
      SelectObject(dc, app->font_); SetBkMode(dc, TRANSPARENT); SetTextColor(dc, ink);
      const auto caption = ReadControl(hwnd);
      DrawTextW(dc, caption.c_str(), -1, &bounds, DT_SINGLELINE | DT_VCENTER | DT_NOPREFIX | DT_END_ELLIPSIS);
      const int x = (info.rcButton.left + info.rcButton.right) / 2;
      const int y = (info.rcButton.top + info.rcButton.bottom) / 2;
      const HPEN arrow = CreatePen(PS_SOLID, app->Scale(2), ink);
      SelectObject(dc, arrow);
      MoveToEx(dc, x - app->Scale(4), y - app->Scale(2), nullptr);
      LineTo(dc, x, y + app->Scale(2)); LineTo(dc, x + app->Scale(4), y - app->Scale(2));
      if (GetFocus() == hwnd && !(SendMessageW(hwnd, WM_QUERYUISTATE, 0, 0) & UISF_HIDEFOCUS)) {
        RECT focus = area; InflateRect(&focus, -app->Scale(3), -app->Scale(3)); DrawFocusRect(dc, &focus);
      }
      RestoreDC(dc, saved); DeleteObject(pen); DeleteObject(arrow);
      EndPaint(hwnd, &paint); return 0;
    }
    if (message == WM_ERASEBKGND && (!app->high_contrast_ || app->PreviewContrast())) return 1;
    const LRESULT result = DefSubclassProc(hwnd, message, wparam, lparam);
    if (message == WM_SETFOCUS || message == WM_KILLFOCUS || message == WM_ENABLE ||
        message == CB_SETCURSEL || message == WM_KEYDOWN || message == WM_LBUTTONUP)
      InvalidateRect(hwnd, nullptr, TRUE);
    return result;
  }

  static LRESULT CALLBACK NoticeProc(HWND hwnd, UINT message, WPARAM wparam, LPARAM lparam,
                                     UINT_PTR, DWORD_PTR reference) {
    auto* app = reinterpret_cast<Application*>(reference);
    if (message == WM_PAINT) {
      PAINTSTRUCT paint{}; const HDC dc = BeginPaint(hwnd, &paint);
      const int saved = SaveDC(dc);
      RECT area{}; GetClientRect(hwnd, &area);
      const COLORREF amber = app->high_contrast_ ? app->SystemColor(COLOR_WINDOWTEXT) :
        app->dark_ ? RGB(242, 178, 76) : RGB(161, 92, 0);
      const HBRUSH fill = CreateSolidBrush(app->high_contrast_ ? app->background_color_ :
        app->dark_ ? RGB(43, 39, 32) : RGB(255, 246, 229));
      FillRect(dc, &area, fill); DeleteObject(fill);
      RECT marker = area; marker.right = marker.left + app->Scale(3);
      const HBRUSH accent = CreateSolidBrush(amber); FillRect(dc, &marker, accent); DeleteObject(accent);
      int y = app->Scale(12);
      SetBkMode(dc, TRANSPARENT);
      for (const auto& row : app->option_rows_) {
        SelectObject(dc, row.done ? app->secondary_font_ : app->strong_font_);
        SetTextColor(dc, row.done ? app->MutedColor() : amber);
        RECT bounds{app->Scale(16), y, area.right - app->Scale(16), area.bottom};
        const auto text = app->OptionRowText(row);
        RECT measured = bounds;
        DrawTextW(dc, text.c_str(), -1, &measured, DT_CALCRECT | DT_WORDBREAK | DT_NOPREFIX);
        DrawTextW(dc, text.c_str(), -1, &bounds, DT_WORDBREAK | DT_NOPREFIX);
        y += (std::max)(app->Scale(22), static_cast<int>(measured.bottom - measured.top)) + app->Scale(4);
      }
      RestoreDC(dc, saved); EndPaint(hwnd, &paint); return 0;
    }
    if (message == WM_ERASEBKGND) return 1;
    return DefSubclassProc(hwnd, message, wparam, lparam);
  }

  static LRESULT CALLBACK CheckboxProc(HWND hwnd, UINT message, WPARAM wparam, LPARAM lparam,
                                       UINT_PTR, DWORD_PTR reference) {
    auto* app = reinterpret_cast<Application*>(reference);
    // Keep BS_AUTOCHECKBOX and its default input/UIA implementation. Only the
    // pixels are custom; high-contrast mode uses the native system rendering.
    if (message == WM_PAINT && (!app->high_contrast_ || app->PreviewContrast())) {
      PAINTSTRUCT paint{}; const HDC dc = BeginPaint(hwnd, &paint);
      RECT rect{}; GetClientRect(hwnd, &rect); FillRect(dc, &rect, app->background_);
      const bool checked = SendMessageW(hwnd, BM_GETCHECK, 0, 0) == BST_CHECKED;
      const bool enabled = IsWindowEnabled(hwnd) != FALSE;
      const int box = app->Scale(20), left = app->Scale(2), top = (rect.bottom - box) / 2;
      const COLORREF ink = enabled ? app->text_color_ : app->high_contrast_ ? app->SystemColor(COLOR_GRAYTEXT) :
        app->dark_ ? RGB(130, 139, 151) : RGB(121, 128, 138);
      const COLORREF accent = app->high_contrast_ ? app->SystemColor(COLOR_HIGHLIGHT) : RGB(244, 129, 32);
      const HBRUSH brush = CreateSolidBrush(checked ? accent : app->background_color_);
      const HPEN pen = CreatePen(PS_SOLID, app->Scale(1), checked ? accent : ink);
      auto old_brush = SelectObject(dc, brush); auto old_pen = SelectObject(dc, pen);
      RoundRect(dc, left, top, left + box, top + box, app->Scale(4), app->Scale(4));
      SelectObject(dc, old_brush); SelectObject(dc, old_pen); DeleteObject(brush); DeleteObject(pen);
      if (checked) {
        const HPEN check_pen = CreatePen(PS_SOLID, app->Scale(2), app->high_contrast_ ?
          app->SystemColor(COLOR_HIGHLIGHTTEXT) : RGB(24, 27, 32));
        old_pen = SelectObject(dc, check_pen);
        MoveToEx(dc, left + box / 5, top + box / 2, nullptr);
        LineTo(dc, left + box * 2 / 5, top + box * 3 / 4); LineTo(dc, left + box * 4 / 5, top + box / 4);
        SelectObject(dc, old_pen); DeleteObject(check_pen);
      }
      const auto old_font = SelectObject(dc, app->font_);
      SetBkMode(dc, TRANSPARENT); SetTextColor(dc, ink);
      RECT text = rect; text.left = left + box + app->Scale(12);
      const auto caption = ReadControl(hwnd);
      RECT measured{0, 0, text.right - text.left, 0};
      DrawTextW(dc, caption.c_str(), -1, &measured, DT_CALCRECT | DT_WORDBREAK | DT_NOPREFIX);
      text.top += (std::max)(0L, (text.bottom - text.top - measured.bottom) / 2);
      DrawTextW(dc, caption.c_str(), -1, &text, DT_WORDBREAK | DT_NOPREFIX);
      if (GetFocus() == hwnd && !(SendMessageW(hwnd, WM_QUERYUISTATE, 0, 0) & UISF_HIDEFOCUS)) {
        RECT focus = rect;
        focus.right = (std::min)(focus.right, text.left + measured.right + app->Scale(6));
        InflateRect(&focus, -app->Scale(1), -app->Scale(1)); DrawFocusRect(dc, &focus);
      }
      SelectObject(dc, old_font); EndPaint(hwnd, &paint); return 0;
    }
    if (message == WM_ERASEBKGND && (!app->high_contrast_ || app->PreviewContrast())) return 1;
    return DefSubclassProc(hwnd, message, wparam, lparam);
  }

  static LRESULT CALLBACK WindowProc(HWND hwnd, UINT message, WPARAM wparam, LPARAM lparam) {
    auto* app = reinterpret_cast<Application*>(GetWindowLongPtrW(hwnd, GWLP_USERDATA));
    if (message == WM_NCCREATE) {
      const auto* create = reinterpret_cast<CREATESTRUCTW*>(lparam);
      app = static_cast<Application*>(create->lpCreateParams);
      app->window_ = hwnd; SetWindowLongPtrW(hwnd, GWLP_USERDATA, reinterpret_cast<LONG_PTR>(app));
    }
    if (!app) return DefWindowProcW(hwnd, message, wparam, lparam);
    switch (message) {
      case WM_COMMAND: app->Command(LOWORD(wparam), HIWORD(wparam)); return 0;
      case WM_DRAWITEM:
        if (reinterpret_cast<DRAWITEMSTRUCT*>(lparam)->CtlType == ODT_COMBOBOX)
          app->DrawComboItem(*reinterpret_cast<DRAWITEMSTRUCT*>(lparam));
        else app->DrawButton(*reinterpret_cast<DRAWITEMSTRUCT*>(lparam));
        return TRUE;
      case WM_MEASUREITEM:
        if (reinterpret_cast<MEASUREITEMSTRUCT*>(lparam)->CtlID == language) {
          reinterpret_cast<MEASUREITEMSTRUCT*>(lparam)->itemHeight = app->Scale(32); return TRUE;
        }
        return DefWindowProcW(hwnd, message, wparam, lparam);
      case kEvent: {
        std::unique_ptr<Event> event(reinterpret_cast<Event*>(lparam)); app->OnEvent(*event); return 0;
      }
      case WM_SIZE: if (app->C(title)) app->Layout(); return 0;
      case WM_GETMINMAXINFO: {
        MONITORINFO work{sizeof(work)};
        GetMonitorInfoW(MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST), &work);
        RECT outer{0, 0, app->Scale(680), app->Scale(480)};
        AdjustWindowRectExForDpi(&outer, static_cast<DWORD>(GetWindowLongPtrW(hwnd, GWL_STYLE)), FALSE,
          static_cast<DWORD>(GetWindowLongPtrW(hwnd, GWL_EXSTYLE)), app->dpi_);
        auto* limits = reinterpret_cast<MINMAXINFO*>(lparam);
        limits->ptMinTrackSize.x = (std::min)(outer.right - outer.left, work.rcWork.right - work.rcWork.left);
        limits->ptMinTrackSize.y = (std::min)(outer.bottom - outer.top, work.rcWork.bottom - work.rcWork.top);
        return 0;
      }
      case WM_DPICHANGED: {
        app->dpi_ = HIWORD(wparam);
        const auto* rectangle = reinterpret_cast<RECT*>(lparam);
        MONITORINFO info{sizeof(info)};
        GetMonitorInfoW(MonitorFromRect(rectangle, MONITOR_DEFAULTTONEAREST), &info);
        const int width = (std::min)(rectangle->right - rectangle->left, info.rcWork.right - info.rcWork.left);
        const int height = (std::min)(rectangle->bottom - rectangle->top, info.rcWork.bottom - info.rcWork.top);
        SetWindowPos(hwnd, nullptr, (std::max)(info.rcWork.left, (std::min)(rectangle->left, info.rcWork.right - width)),
          (std::max)(info.rcWork.top, (std::min)(rectangle->top, info.rcWork.bottom - height)), width, height, SWP_NOZORDER);
        app->SetTheme(); app->Layout(); return 0;
      }
      case WM_SETTINGCHANGE: case WM_THEMECHANGED:
        app->SetTheme(); if (app->C(title)) app->Layout(); return 0;
      case WM_VSCROLL: {
        SCROLLINFO info{sizeof(info), SIF_ALL}; GetScrollInfo(hwnd, SB_VERT, &info);
        int next = info.nPos;
        switch (LOWORD(wparam)) {
          case SB_LINEUP: next -= 25; break; case SB_LINEDOWN: next += 25; break;
          case SB_PAGEUP: next -= 150; break; case SB_PAGEDOWN: next += 150; break;
          case SB_THUMBTRACK: next = info.nTrackPos; break; default: break;
        }
        app->scroll_ = (std::max)(0, (std::min)(next, info.nMax - static_cast<int>(info.nPage) + 1));
        app->Layout(); return 0;
      }
      case WM_MOUSEWHEEL: {
        SCROLLINFO info{sizeof(info), SIF_ALL}; GetScrollInfo(hwnd, SB_VERT, &info);
        app->scroll_ = (std::max)(0, (std::min)(app->scroll_ - GET_WHEEL_DELTA_WPARAM(wparam) / WHEEL_DELTA * 40,
          info.nMax - static_cast<int>(info.nPage) + 1));
        app->Layout(); return 0;
      }
      case WM_TIMER:
        if (wparam == kPreviewTimer) {
          app->percent_ += 4; SendMessageW(app->C(progress), PBM_SETPOS, app->percent_, 0);
          if (app->percent_ >= 100) { KillTimer(hwnd, kPreviewTimer); app->Completed({EventType::applied}); }
        }
        return 0;
      case WM_CTLCOLORSTATIC: case WM_CTLCOLORBTN: case WM_CTLCOLOREDIT: case WM_CTLCOLORLISTBOX: {
        const HDC dc = reinterpret_cast<HDC>(wparam);
        const int id = GetDlgCtrlID(reinterpret_cast<HWND>(lparam));
        SetTextColor(dc, IsSecondaryText(id) ? app->MutedColor() : app->text_color_); SetBkColor(dc, app->background_color_);
        return reinterpret_cast<LRESULT>(app->background_);
      }
      case WM_ERASEBKGND: {
        RECT area{}; GetClientRect(hwnd, &area);
        FillRect(reinterpret_cast<HDC>(wparam), &area, app->background_); return 1;
      }
      case WM_CLOSE: app->Close(); return 0;
      case WM_DESTROY: PostQuitMessage(0); return 0;
      default: return DefWindowProcW(hwnd, message, wparam, lparam);
    }
  }

  bool preview_ = false;
  std::wstring preview_state_;
  size_t preview_scene_ = 0;
  size_t preview_language_ = 2;
  int preview_theme_ = -1;
  UINT preview_scale_ = 1;
  std::thread ui_thread_, worker_;
  std::atomic<HWND> window_{nullptr};
  HWND body_ = nullptr;
  std::array<HWND, tertiary - title + 1> controls_{};
  BOOTSTRAPPER_DISPLAY display_ = BOOTSTRAPPER_DISPLAY_FULL;
  BOOTSTRAPPER_ACTION requested_action_ = BOOTSTRAPPER_ACTION_INSTALL;
  BOOTSTRAPPER_ACTION action_ = BOOTSTRAPPER_ACTION_INSTALL;
  Detection detection_;
  Mode mode_ = Mode::install;
  Page page_ = Page::detecting;
  Page last_page_ = Page::detecting;
  FinishState finish_;
  FinishIntent finish_intent_ = FinishIntent::normal;
  bool reconfirm_restart_ = false;
  bool launch_failed_ = false;
  std::wstring culture_, folder_, installed_folder_, current_version_, target_version_;
  std::string status_key_ = "detecting", failure_key_ = "failed_description";
  std::string detail_stage_ = "detecting";
  HRESULT result_ = S_OK;
  DWORD percent_ = 0;
  bool cancel_requested_ = false, terminal_failure_ = false, options_busy_ = false, expanded_ = false;
  bool startup_known_ = true, reboot_confirmation_ = false, folder_expanded_ = false;
  bool details_expanded_ = false;
  std::atomic<bool> restart_requested_{false};
  std::atomic<CancelStage> cancel_stage_{CancelStage::before_start};
  std::atomic<bool> native_cancel_enabled_{true};
  HANDLE choice_event_ = CreateEventW(nullptr, TRUE, FALSE, nullptr);
  std::atomic<int> choice_result_{IDCANCEL};
  BOOTSTRAPPER_FILES_IN_USE_TYPE files_source_ = BOOTSTRAPPER_FILES_IN_USE_TYPE_MSI;
  std::wstring files_in_use_;
  std::vector<OptionResultRow> option_rows_;
  bool high_contrast_ = false, dark_ = false, rtl_ = false;
  UINT dpi_ = 96;
  int scroll_ = 0;
  bool layout_active_ = false;
  HFONT font_ = nullptr, title_font_ = nullptr, secondary_font_ = nullptr, strong_font_ = nullptr;
  HBRUSH background_ = nullptr;
  HMODULE rich_edit_ = nullptr;
  COLORREF background_color_ = RGB(250, 251, 253), text_color_ = RGB(28, 36, 47);
};
}  // namespace usque::setup

int WINAPI wWinMain(HINSTANCE, HINSTANCE, LPWSTR, int) {
  using namespace usque::setup;
  int count = 0;
  LPWSTR* args = CommandLineToArgvW(GetCommandLineW(), &count);
#ifdef USQUE_PREVIEW_ONLY
  const bool preview = true;
#else
  const bool preview = count >= 2 && std::wstring_view(args[1]) == L"--preview";
#endif
  std::wstring preview_state;
  if (preview && count == 3) preview_state = args[2];
  if (args) LocalFree(args);
  auto* application = new Application(preview, preview_state);
  const HRESULT result = preview ? application->Preview() : BootstrapperApplicationRun(application);
  application->Release();
  return FAILED(result) ? static_cast<int>(result) : 0;
}
