#include "flutter_window.h"

#include <flutter/event_stream_handler_functions.h>
#include <flutter/method_result_functions.h>
#include <flutter/standard_method_codec.h>
#include <shellapi.h>
#include <shobjidl.h>

#include <atomic>
#include <limits>
#include <optional>
#include <thread>
#include <variant>

#include "engine_ipc.h"
#include "flutter/generated_plugin_registrant.h"
#include "resource.h"
#include "utils.h"
#include "window_frame.h"
#include "zero_trust_protocol.h"

namespace {

constexpr UINT kEngineIpcComplete = WM_APP + 17;
constexpr UINT kEngineEventAvailable = WM_APP + 18;
constexpr UINT kTrayCallback = WM_APP + 19;
constexpr UINT kEngineReadyComplete = WM_APP + 20;
constexpr UINT kTrayOpen = 41001;
constexpr UINT kTrayToggle = 41002;
constexpr UINT kTrayDisconnectExit = 41003;
constexpr wchar_t kUsqueSettingsKey[] =
    L"Software\\io.github.georgexie2333\\Usque";
constexpr wchar_t kCloseToTrayValue[] = L"CloseToTray";
constexpr wchar_t kRunKey[] =
    L"Software\\Microsoft\\Windows\\CurrentVersion\\Run";
constexpr wchar_t kRunValue[] = L"Usque";
std::atomic<uint64_t> g_engine_event_generation = 0;
const UINT kTaskbarCreated = ::RegisterWindowMessageW(L"TaskbarCreated");

bool ReadCloseToTray() {
  DWORD value = 1;
  DWORD size = sizeof(value);
  const LSTATUS status = ::RegGetValueW(
      HKEY_CURRENT_USER, kUsqueSettingsKey, kCloseToTrayValue, RRF_RT_REG_DWORD,
      nullptr, &value, &size);
  return status != ERROR_SUCCESS || value != 0;
}

bool WriteCloseToTray(bool enabled) {
  HKEY key = nullptr;
  if (::RegCreateKeyExW(HKEY_CURRENT_USER, kUsqueSettingsKey, 0, nullptr, 0,
                        KEY_SET_VALUE, nullptr, &key, nullptr) !=
      ERROR_SUCCESS) {
    return false;
  }
  const DWORD value = enabled ? 1 : 0;
  const LSTATUS status = ::RegSetValueExW(
      key, kCloseToTrayValue, 0, REG_DWORD,
      reinterpret_cast<const BYTE*>(&value), sizeof(value));
  ::RegCloseKey(key);
  return status == ERROR_SUCCESS;
}

bool IsStartOnLoginEnabled() {
  wchar_t value[32768]{};
  DWORD size = sizeof(value);
  return ::RegGetValueW(HKEY_CURRENT_USER, kRunKey, kRunValue, RRF_RT_REG_SZ,
                        nullptr, value, &size) == ERROR_SUCCESS;
}

bool SetStartOnLogin(bool enabled) {
  HKEY key = nullptr;
  if (::RegCreateKeyExW(HKEY_CURRENT_USER, kRunKey, 0, nullptr, 0,
                        KEY_SET_VALUE, nullptr, &key, nullptr) !=
      ERROR_SUCCESS) {
    return false;
  }
  LSTATUS status = ERROR_SUCCESS;
  if (enabled) {
    wchar_t executable[MAX_PATH]{};
    const DWORD length = ::GetModuleFileNameW(nullptr, executable, MAX_PATH);
    if (length == 0 || length >= MAX_PATH) {
      ::RegCloseKey(key);
      return false;
    }
    const std::wstring command = L"\"" + std::wstring(executable, length) +
                                 L"\" --background";
    status = ::RegSetValueExW(
        key, kRunValue, 0, REG_SZ,
        reinterpret_cast<const BYTE*>(command.c_str()),
        static_cast<DWORD>((command.size() + 1) * sizeof(wchar_t)));
  } else {
    status = ::RegDeleteValueW(key, kRunValue);
    if (status == ERROR_FILE_NOT_FOUND) status = ERROR_SUCCESS;
  }
  ::RegCloseKey(key);
  return status == ERROR_SUCCESS;
}

struct PendingEngineReply {
  std::unique_ptr<flutter::MethodResult<flutter::EncodableValue>> result;
  EngineIpcResult ipc;
};

struct PendingEngineReadyReply {
  std::unique_ptr<flutter::MethodResult<flutter::EncodableValue>> result;
  std::string error;
};

struct PendingEngineEvent {
  uint64_t generation;
  EngineIpcResult ipc;
};

struct SaveDialogResult {
  std::optional<std::string> path;
  std::string error;
};

SaveDialogResult SelectDestination(HWND owner, const wchar_t* label,
                                   const wchar_t* pattern,
                                   const wchar_t* extension,
                                   const wchar_t* file_name) {
  IFileSaveDialog* dialog = nullptr;
  const HRESULT create_result =
      ::CoCreateInstance(CLSID_FileSaveDialog, nullptr, CLSCTX_INPROC_SERVER,
                         IID_PPV_ARGS(&dialog));
  if (FAILED(create_result)) {
    SaveDialogResult result;
    result.error = "Could not create the Windows save dialog (HRESULT " +
                   std::to_string(create_result) + ").";
    return result;
  }

  const COMDLG_FILTERSPEC filters[] = {
      {label, pattern},
  };
  dialog->SetFileTypes(1, filters);
  dialog->SetDefaultExtension(extension);
  dialog->SetFileName(file_name);
  const HRESULT show_result = dialog->Show(owner);
  if (show_result == HRESULT_FROM_WIN32(ERROR_CANCELLED)) {
    dialog->Release();
    return {};
  }
  if (FAILED(show_result)) {
    dialog->Release();
    SaveDialogResult result;
    result.error = "The Windows save dialog failed (HRESULT " +
                   std::to_string(show_result) + ").";
    return result;
  }

  IShellItem* item = nullptr;
  const HRESULT item_result = dialog->GetResult(&item);
  dialog->Release();
  if (FAILED(item_result) || item == nullptr) {
    SaveDialogResult result;
    result.error = "The Windows save dialog returned no destination.";
    return result;
  }
  wchar_t* path = nullptr;
  const HRESULT path_result = item->GetDisplayName(SIGDN_FILESYSPATH, &path);
  item->Release();
  if (FAILED(path_result) || path == nullptr) {
    SaveDialogResult result;
    result.error = "The selected diagnostic destination has no file path.";
    return result;
  }
  std::string utf8_path = Utf8FromUtf16(path);
  ::CoTaskMemFree(path);
  if (utf8_path.empty()) {
    SaveDialogResult result;
    result.error = "The selected diagnostic destination is not valid UTF-8.";
    return result;
  }
  SaveDialogResult result;
  result.path = std::move(utf8_path);
  return result;
}

}  // namespace

FlutterWindow::FlutterWindow(const flutter::DartProject& project,
                             bool start_hidden)
    : project_(project), start_hidden_(start_hidden) {}

FlutterWindow::~FlutterWindow() {}

bool FlutterWindow::OnCreate() {
  if (!Win32Window::OnCreate()) {
    return false;
  }

  // Before the client area is measured: the view is created at that size, and
  // the caption inset would otherwise stay until the first user resize.
  usque::ApplyCustomFrame(GetHandle());

  RECT frame = GetClientArea();

  // The size here must match the window dimensions to avoid unnecessary surface
  // creation / destruction in the startup path.
  flutter_controller_ = std::make_unique<flutter::FlutterViewController>(
      frame.right - frame.left, frame.bottom - frame.top, project_);
  // Ensure that basic setup of the controller was successful.
  if (!flutter_controller_->engine() || !flutter_controller_->view()) {
    return false;
  }
  RegisterPlugins(flutter_controller_->engine());
  usque::BindWindowFrameChannel(flutter_controller_->engine()->messenger(),
                                GetHandle());
  close_to_tray_ = ReadCloseToTray();
  AddTrayIcon();
  engine_channel_ =
      std::make_unique<flutter::MethodChannel<flutter::EncodableValue>>(
          flutter_controller_->engine()->messenger(),
          "io.github.georgexie2333.usque/engine",
          &flutter::StandardMethodCodec::GetInstance());
  engine_channel_->SetMethodCallHandler(
      [this](const flutter::MethodCall<flutter::EncodableValue>& call,
             std::unique_ptr<flutter::MethodResult<flutter::EncodableValue>>
                 result) {
        if (call.method_name() == "exchangeFrame") {
          const auto* arguments =
              std::get_if<flutter::EncodableMap>(call.arguments());
          if (arguments == nullptr) {
            result->Error("ENGINE_IPC_INVALID_ARGUMENT",
                          "Named Pipe arguments are missing.");
            return;
          }
          const auto pipe_iterator =
              arguments->find(flutter::EncodableValue("pipe_name"));
          const auto request_iterator =
              arguments->find(flutter::EncodableValue("request"));
          if (pipe_iterator == arguments->end() ||
              request_iterator == arguments->end()) {
            result->Error("ENGINE_IPC_INVALID_ARGUMENT",
                          "Named Pipe name or request frame is missing.");
            return;
          }
          const auto* pipe_name =
              std::get_if<std::string>(&pipe_iterator->second);
          const auto* request =
              std::get_if<std::vector<uint8_t>>(&request_iterator->second);
          if (pipe_name == nullptr || request == nullptr) {
            result->Error("ENGINE_IPC_INVALID_ARGUMENT",
                          "Named Pipe arguments have invalid types.");
            return;
          }
          const HWND window = GetHandle();
          std::thread([window, pipe_name = *pipe_name, request = *request,
                       result = std::move(result)]() mutable {
            auto* pending = new PendingEngineReply{
                std::move(result), ExchangeEngineFrame(pipe_name, request)};
            if (!::PostMessageW(window, kEngineIpcComplete, 0,
                                reinterpret_cast<LPARAM>(pending))) {
              delete pending;
            }
          }).detach();
          return;
        }
        if (call.method_name() == "waitForEnginePipe") {
          const auto* arguments =
              std::get_if<flutter::EncodableMap>(call.arguments());
          if (arguments == nullptr) {
            result->Error("ENGINE_IPC_INVALID_ARGUMENT",
                          "Named Pipe readiness arguments are missing.");
            return;
          }
          const auto pipe_iterator =
              arguments->find(flutter::EncodableValue("pipe_name"));
          const auto timeout_iterator =
              arguments->find(flutter::EncodableValue("timeout_ms"));
          if (pipe_iterator == arguments->end() ||
              timeout_iterator == arguments->end()) {
            result->Error(
                "ENGINE_IPC_INVALID_ARGUMENT",
                "Named Pipe readiness name or timeout is missing.");
            return;
          }
          const auto* pipe_name =
              std::get_if<std::string>(&pipe_iterator->second);
          int64_t timeout_ms = 0;
          if (const auto* timeout_32 =
                  std::get_if<int32_t>(&timeout_iterator->second)) {
            timeout_ms = *timeout_32;
          } else if (const auto* timeout_64 =
                         std::get_if<int64_t>(&timeout_iterator->second)) {
            timeout_ms = *timeout_64;
          }
          if (pipe_name == nullptr || timeout_ms <= 0 ||
              timeout_ms > std::numeric_limits<uint32_t>::max()) {
            result->Error("ENGINE_IPC_INVALID_ARGUMENT",
                          "Named Pipe readiness arguments are invalid.");
            return;
          }
          const HWND window = GetHandle();
          std::thread([window, pipe_name = *pipe_name,
                       timeout_ms = static_cast<uint32_t>(timeout_ms),
                       result = std::move(result)]() mutable {
            auto* pending = new PendingEngineReadyReply{
                std::move(result), WaitForEnginePipe(pipe_name, timeout_ms)};
            if (!::PostMessageW(window, kEngineReadyComplete, 0,
                                reinterpret_cast<LPARAM>(pending))) {
              delete pending;
            }
          }).detach();
          return;
        }
        if (call.method_name() == "selectDiagnosticsDestination") {
          const SaveDialogResult selection =
              SelectDestination(GetHandle(), L"ZIP archive (*.zip)", L"*.zip",
                                L"zip", L"usque-diagnostics.zip");
          if (!selection.error.empty()) {
            result->Error("DIAGNOSTICS_DESTINATION_FAILED", selection.error);
          } else if (selection.path.has_value()) {
            result->Success(flutter::EncodableValue(*selection.path));
          } else {
            result->Success(flutter::EncodableValue());
          }
          return;
        }
        if (call.method_name() == "selectWarpSecretDestination") {
          const SaveDialogResult selection = SelectDestination(
              GetHandle(), L"JSON file (*.json)", L"*.json", L"json",
              L"usque-warp-secret.json");
          if (!selection.error.empty()) {
            result->Error("WARP_SECRET_DESTINATION_FAILED", selection.error);
          } else if (selection.path.has_value()) {
            result->Success(flutter::EncodableValue(*selection.path));
          } else {
            result->Success(flutter::EncodableValue());
          }
          return;
        }
        if (call.method_name() == "platformPreferences") {
          flutter::EncodableMap preferences;
          preferences[flutter::EncodableValue("start_on_boot")] =
              flutter::EncodableValue(IsStartOnLoginEnabled());
          preferences[flutter::EncodableValue("close_to_tray")] =
              flutter::EncodableValue(close_to_tray_);
          preferences[flutter::EncodableValue("warp_protocol_association")] =
              flutter::EncodableValue(IsCurrentUserWarpProtocolAssociated());
          result->Success(flutter::EncodableValue(preferences));
          return;
        }
        if (call.method_name() == "beginZeroTrustLogin") {
          const auto* arguments =
              std::get_if<flutter::EncodableMap>(call.arguments());
          const auto iterator =
              arguments == nullptr
                  ? flutter::EncodableMap::const_iterator{}
                  : arguments->find(flutter::EncodableValue("team_name"));
          const auto* team =
              arguments != nullptr && iterator != arguments->end()
                  ? std::get_if<std::string>(&iterator->second)
                  : nullptr;
          if (team == nullptr) {
            result->Error("ZERO_TRUST_TEAM_INVALID",
                          "The organization name is missing.");
            return;
          }
          const auto login = zero_trust_session_.Begin(*team);
          if (!login.has_value()) {
            result->Error("ZERO_TRUST_TEAM_INVALID",
                          "Enter one Cloudflare Zero Trust team name.");
            return;
          }
          result->Success(flutter::EncodableValue(*login));
          return;
        }
        if (call.method_name() == "consumeZeroTrustCallback") {
          const auto pending = zero_trust_session_.Consume();
          if (pending.has_value()) {
            result->Success(flutter::EncodableValue(*pending));
          } else {
            result->Success(flutter::EncodableValue());
          }
          return;
        }
        if (call.method_name() == "cancelZeroTrustLogin") {
          zero_trust_session_.Cancel();
          result->Success();
          return;
        }
        if (call.method_name() == "setWarpProtocolAssociation") {
          const auto* arguments =
              std::get_if<flutter::EncodableMap>(call.arguments());
          const auto iterator =
              arguments == nullptr
                  ? flutter::EncodableMap::const_iterator{}
                  : arguments->find(flutter::EncodableValue("enabled"));
          const bool valid = arguments != nullptr &&
                             iterator != arguments->end() &&
                             std::holds_alternative<bool>(iterator->second);
          if (!valid) {
            result->Error("INVALID_ARGUMENT",
                          "The Windows shell setting is malformed.");
            return;
          }
          if (!SetCurrentUserWarpProtocolAssociation(
                  std::get<bool>(iterator->second))) {
            result->Error("WINDOWS_SHELL_SETTING_FAILED",
                          "Windows could not save the shell integration setting.");
            return;
          }
          result->Success();
          return;
        }
        if (call.method_name() == "setStartOnBoot" ||
            call.method_name() == "setCloseToTray") {
          const auto* arguments =
              std::get_if<flutter::EncodableMap>(call.arguments());
          const auto iterator =
              arguments == nullptr
                  ? flutter::EncodableMap::const_iterator{}
                  : arguments->find(flutter::EncodableValue("enabled"));
          const bool valid = arguments != nullptr &&
                             iterator != arguments->end() &&
                             std::holds_alternative<bool>(iterator->second);
          if (!valid) {
            result->Error("INVALID_ARGUMENT",
                          "The Windows shell setting is malformed.");
            return;
          }
          const bool enabled = std::get<bool>(iterator->second);
          const bool saved = call.method_name() == "setStartOnBoot"
                                 ? SetStartOnLogin(enabled)
                                 : WriteCloseToTray(enabled);
          if (!saved) {
            result->Error("WINDOWS_SHELL_SETTING_FAILED",
                          "Windows could not save the shell integration setting.");
            return;
          }
          if (call.method_name() == "setCloseToTray") {
            close_to_tray_ = enabled;
          }
          result->Success();
          return;
        }
        if (call.method_name() == "updateTrayState") {
          const auto* arguments =
              std::get_if<flutter::EncodableMap>(call.arguments());
          if (arguments == nullptr) {
            result->Error("INVALID_ARGUMENT", "Tray state is missing.");
            return;
          }
          const auto phase_it =
              arguments->find(flutter::EncodableValue("phase"));
          const auto connected_it =
              arguments->find(flutter::EncodableValue("connected"));
          if (phase_it == arguments->end() ||
              connected_it == arguments->end() ||
              !std::holds_alternative<std::string>(phase_it->second) ||
              !std::holds_alternative<bool>(connected_it->second)) {
            result->Error("INVALID_ARGUMENT", "Tray state is malformed.");
            return;
          }
          UpdateTrayState(std::get<std::string>(phase_it->second),
                          std::get<bool>(connected_it->second));
          result->Success();
          return;
        }
        if (call.method_name() == "exitApplication") {
          force_exit_ = true;
          result->Success();
          ::PostMessageW(GetHandle(), WM_CLOSE, 0, 0);
          return;
        }
        result->NotImplemented();
      });
  engine_event_channel_ =
      std::make_unique<flutter::EventChannel<flutter::EncodableValue>>(
          flutter_controller_->engine()->messenger(),
          "io.github.georgexie2333.usque/engine_events",
          &flutter::StandardMethodCodec::GetInstance());
  engine_event_channel_->SetStreamHandler(
      std::make_unique<
          flutter::StreamHandlerFunctions<flutter::EncodableValue>>(
          [this](
              const flutter::EncodableValue* arguments,
              std::unique_ptr<flutter::EventSink<flutter::EncodableValue>>&&
                  events)
              -> std::unique_ptr<
                  flutter::StreamHandlerError<flutter::EncodableValue>> {
            const auto* map =
                arguments == nullptr
                    ? nullptr
                    : std::get_if<flutter::EncodableMap>(arguments);
            if (map == nullptr) {
              return std::make_unique<
                  flutter::StreamHandlerError<flutter::EncodableValue>>(
                  "ENGINE_EVENT_INVALID_ARGUMENT",
                  "Named Pipe event arguments are missing.", nullptr);
            }
            const auto iterator =
                map->find(flutter::EncodableValue("pipe_name"));
            if (iterator == map->end()) {
              return std::make_unique<
                  flutter::StreamHandlerError<flutter::EncodableValue>>(
                  "ENGINE_EVENT_INVALID_ARGUMENT",
                  "Named Pipe event name is missing.", nullptr);
            }
            const auto* pipe_name =
                std::get_if<std::string>(&iterator->second);
            if (pipe_name == nullptr) {
              return std::make_unique<
                  flutter::StreamHandlerError<flutter::EncodableValue>>(
                  "ENGINE_EVENT_INVALID_ARGUMENT",
                  "Named Pipe event name has an invalid type.", nullptr);
            }

            StopEngineEventStream();
            engine_event_sink_ = std::move(events);
            engine_event_active_ = std::make_shared<std::atomic_bool>(true);
            engine_event_generation_ =
                g_engine_event_generation.fetch_add(1) + 1;
            const HWND window = GetHandle();
            const uint64_t generation = engine_event_generation_;
            const auto active = engine_event_active_;
            std::thread([window, generation, active,
                         pipe_name = *pipe_name]() {
              StreamEngineEvents(
                  pipe_name, active,
                  [window, generation](EngineIpcResult event) {
                    auto* pending = new PendingEngineEvent{
                        generation, std::move(event)};
                    if (!::PostMessageW(
                            window, kEngineEventAvailable, 0,
                            reinterpret_cast<LPARAM>(pending))) {
                      delete pending;
                    }
                  });
            }).detach();
            return nullptr;
          },
          [this](const flutter::EncodableValue*)
              -> std::unique_ptr<
                  flutter::StreamHandlerError<flutter::EncodableValue>> {
            StopEngineEventStream();
            return nullptr;
          }));
  HWND flutter_view = flutter_controller_->view()->GetNativeWindow();
  SetChildContent(flutter_view);
  usque::AttachFlutterView(GetHandle(), flutter_view);

  flutter_controller_->engine()->SetNextFrameCallback([&]() {
    if (!start_hidden_) this->Show();
  });

  // Flutter can complete the first frame before the "show window" callback is
  // registered. The following call ensures a frame is pending to ensure the
  // window is shown. It is a no-op if the first frame hasn't completed yet.
  flutter_controller_->ForceRedraw();

  return true;
}

void FlutterWindow::OnDestroy() {
  usque::UnbindWindowFrameChannel();
  usque::DetachFlutterView();
  StopEngineEventStream();
  RemoveTrayIcon();
  if (flutter_controller_) {
    engine_event_channel_.reset();
    engine_channel_.reset();
    flutter_controller_ = nullptr;
  }

  Win32Window::OnDestroy();
}

void FlutterWindow::AddTrayIcon() {
  tray_icon_ = {};
  tray_icon_.cbSize = sizeof(tray_icon_);
  tray_icon_.hWnd = GetHandle();
  tray_icon_.uID = 1;
  tray_icon_.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
  tray_icon_.uCallbackMessage = kTrayCallback;
  tray_icon_.hIcon = static_cast<HICON>(::LoadImageW(
      ::GetModuleHandleW(nullptr), MAKEINTRESOURCEW(IDI_APP_ICON), IMAGE_ICON,
      ::GetSystemMetrics(SM_CXSMICON), ::GetSystemMetrics(SM_CYSMICON),
      LR_DEFAULTCOLOR));
  wcscpy_s(tray_icon_.szTip, L"Usque - Disconnected");
  tray_icon_added_ = ::Shell_NotifyIconW(NIM_ADD, &tray_icon_) == TRUE;
  if (tray_icon_added_) {
    tray_icon_.uVersion = NOTIFYICON_VERSION_4;
    ::Shell_NotifyIconW(NIM_SETVERSION, &tray_icon_);
  }
}

void FlutterWindow::RemoveTrayIcon() {
  if (tray_icon_added_) {
    ::Shell_NotifyIconW(NIM_DELETE, &tray_icon_);
    tray_icon_added_ = false;
  }
  if (tray_icon_.hIcon != nullptr) {
    ::DestroyIcon(tray_icon_.hIcon);
    tray_icon_.hIcon = nullptr;
  }
}

void FlutterWindow::UpdateTrayState(const std::string& phase,
                                    bool connected) {
  tray_connected_ = connected;
  tray_status_ = Utf16FromUtf8(phase);
  if (tray_status_.empty()) tray_status_ = L"Disconnected";
  if (!tray_icon_added_) return;
  const std::wstring tooltip = L"Usque - " + tray_status_;
  wcsncpy_s(tray_icon_.szTip, tooltip.c_str(), _TRUNCATE);
  tray_icon_.uFlags = NIF_TIP;
  ::Shell_NotifyIconW(NIM_MODIFY, &tray_icon_);
}

void FlutterWindow::ShowAndActivate() {
  ::ShowWindow(GetHandle(), SW_RESTORE);
  ::SetForegroundWindow(GetHandle());
}

void FlutterWindow::NotifyZeroTrustCallbackArrived() {
  if (!engine_channel_) return;
  engine_channel_->InvokeMethod("zeroTrustCallbackArrived", nullptr);
}

void FlutterWindow::OfferZeroTrustCallback(std::string_view callback_uri) {
  if (!zero_trust_session_.Accept(callback_uri)) return;
  NotifyZeroTrustCallbackArrived();
}

bool FlutterWindow::HandleZeroTrustCopyData(const COPYDATASTRUCT* data) {
  if (data == nullptr || data->dwData != kZeroTrustCallbackCopyData ||
      data->lpData == nullptr || data->cbData == 0 ||
      data->cbData > static_cast<DWORD>(kMaxZeroTrustCallbackChars)) {
    return false;
  }
  const auto* bytes = static_cast<const char*>(data->lpData);
  std::string uri(bytes, data->cbData);
  if (!uri.empty() && uri.back() == '\0') {
    uri.pop_back();
  }
  ShowAndActivate();
  OfferZeroTrustCallback(uri);
  return true;
}

void FlutterWindow::ShowTrayMenu() {
  HMENU menu = ::CreatePopupMenu();
  if (menu == nullptr) return;
  ::AppendMenuW(menu, MF_STRING | MF_DISABLED, 0, tray_status_.c_str());
  ::AppendMenuW(menu, MF_SEPARATOR, 0, nullptr);
  ::AppendMenuW(menu, MF_STRING, kTrayOpen, L"Open Usque");
  ::AppendMenuW(menu, MF_STRING, kTrayToggle,
                tray_connected_ ? L"Disconnect Active Profile"
                                : L"Connect Active Profile");
  ::AppendMenuW(menu, MF_SEPARATOR, 0, nullptr);
  ::AppendMenuW(menu, MF_STRING, kTrayDisconnectExit,
                L"Disconnect and Exit");
  POINT point{};
  ::GetCursorPos(&point);
  ::SetForegroundWindow(GetHandle());
  const UINT command = ::TrackPopupMenu(
      menu, TPM_RETURNCMD | TPM_NONOTIFY | TPM_RIGHTBUTTON, point.x, point.y, 0,
      GetHandle(), nullptr);
  ::DestroyMenu(menu);
  if (command == kTrayOpen) {
    ShowAndActivate();
  } else if (command == kTrayToggle) {
    InvokeTrayCommand("toggle", false);
  } else if (command == kTrayDisconnectExit) {
    InvokeTrayCommand("disconnectAndExit", true);
  }
  ::PostMessageW(GetHandle(), WM_NULL, 0, 0);
}

void FlutterWindow::InvokeTrayCommand(const std::string& command,
                                      bool exit_on_success) {
  if (!engine_channel_) return;
  engine_channel_->InvokeMethod(
      "trayCommand", std::make_unique<flutter::EncodableValue>(command),
      std::make_unique<flutter::MethodResultFunctions<flutter::EncodableValue>>(
          [this, exit_on_success](const flutter::EncodableValue*) {
            if (exit_on_success) {
              force_exit_ = true;
              ::PostMessageW(GetHandle(), WM_CLOSE, 0, 0);
            }
          },
          [this](const std::string&, const std::string&,
                 const flutter::EncodableValue*) { exit_pending_ = false; },
          [this]() { exit_pending_ = false; }));
}

void FlutterWindow::StopEngineEventStream() {
  if (engine_event_active_) {
    engine_event_active_->store(false);
    engine_event_active_.reset();
  }
  engine_event_generation_ = 0;
  engine_event_sink_.reset();
}

LRESULT
FlutterWindow::MessageHandler(HWND hwnd, UINT const message,
                              WPARAM const wparam,
                              LPARAM const lparam) noexcept {
  if (message == WM_COPYDATA) {
    return HandleZeroTrustCopyData(
               reinterpret_cast<const COPYDATASTRUCT*>(lparam))
               ? TRUE
               : FALSE;
  }

  // The caption is drawn by Flutter, so the frame messages are answered before
  // anything else; WM_NCCALCSIZE in particular arrives while the window is
  // still being created and no engine exists yet.
  if (const std::optional<LRESULT> framed =
          usque::HandleCustomFrameMessage(hwnd, message, wparam, lparam)) {
    return *framed;
  }

  // Give Flutter, including plugins, an opportunity to handle window messages.
  if (flutter_controller_) {
    std::optional<LRESULT> result =
        flutter_controller_->HandleTopLevelWindowProc(hwnd, message, wparam,
                                                      lparam);
    if (result) {
      return *result;
    }
  }

  if (message == kTaskbarCreated) {
    tray_icon_added_ = false;
    if (tray_icon_.hIcon != nullptr) {
      ::DestroyIcon(tray_icon_.hIcon);
      tray_icon_.hIcon = nullptr;
    }
    AddTrayIcon();
    UpdateTrayState(Utf8FromUtf16(tray_status_.c_str()), tray_connected_);
    return 0;
  }

  switch (message) {
    case kEngineIpcComplete: {
      std::unique_ptr<PendingEngineReply> pending(
          reinterpret_cast<PendingEngineReply*>(lparam));
      if (pending->ipc.error.empty()) {
        pending->result->Success(
            flutter::EncodableValue(pending->ipc.response));
      } else {
        pending->result->Error("ENGINE_IPC_UNAVAILABLE", pending->ipc.error);
      }
      return 0;
    }
    case kEngineReadyComplete: {
      std::unique_ptr<PendingEngineReadyReply> pending(
          reinterpret_cast<PendingEngineReadyReply*>(lparam));
      if (pending->error.empty()) {
        pending->result->Success();
      } else {
        pending->result->Error("ENGINE_START_UNAVAILABLE", pending->error);
      }
      return 0;
    }
    case kEngineEventAvailable: {
      std::unique_ptr<PendingEngineEvent> pending(
          reinterpret_cast<PendingEngineEvent*>(lparam));
      if (engine_event_sink_ == nullptr ||
          pending->generation != engine_event_generation_) {
        return 0;
      }
      if (pending->ipc.error.empty()) {
        engine_event_sink_->Success(
            flutter::EncodableValue(pending->ipc.response));
      }
      // Pipe and validation failures reconnect in C++. EndOfStream only on
      // window destroy or Dart onCancel via StopEngineEventStream.
      return 0;
    }
    case kTrayCallback: {
      const UINT event = LOWORD(lparam);
      if (event == WM_LBUTTONUP || event == WM_LBUTTONDBLCLK) {
        ShowAndActivate();
      } else if (event == WM_RBUTTONUP || event == WM_CONTEXTMENU) {
        ShowTrayMenu();
      }
      return 0;
    }
    case WM_SIZE:
    case WM_ACTIVATE:
      usque::PublishWindowFrameState(hwnd, false);
      break;
    case WM_CLOSE:
      if (force_exit_) {
        break;
      }
      if (close_to_tray_) {
        ::ShowWindow(hwnd, SW_HIDE);
        return 0;
      }
      if (!exit_pending_) {
        exit_pending_ = true;
        InvokeTrayCommand("disconnectAndExit", true);
      }
      return 0;
    case WM_FONTCHANGE:
      flutter_controller_->engine()->ReloadSystemFonts();
      break;
  }

  return Win32Window::MessageHandler(hwnd, message, wparam, lparam);
}
