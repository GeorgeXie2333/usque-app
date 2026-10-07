#include "flutter_window.h"

#include <flutter/event_stream_handler_functions.h>
#include <flutter/method_result_functions.h>
#include <flutter/standard_method_codec.h>
#include <flutter_windows.h>
#include <shellapi.h>
#include <shobjidl.h>

#include <algorithm>
#include <atomic>
#include <cmath>
#include <cstring>
#include <limits>
#include <optional>
#include <thread>
#include <variant>
#include <vector>

#include "engine_ipc.h"
#include "flutter/generated_plugin_registrant.h"
#include "maintenance_shutdown.h"
#include "resource.h"
#include "shell_integration.h"
#include "utils.h"
#include "window_frame.h"
#include "window_placement.h"
#include "zero_trust_protocol.h"

namespace {

constexpr size_t kMaxChainFiles = 128;
constexpr DWORD kMaxChainFileBytes = 128 * 1024;

flutter::EncodableMap ReadChainFile(IShellItem* item) {
  using flutter::EncodableValue;
  flutter::EncodableMap result;
  PWSTR path = nullptr;
  if (item != nullptr) item->GetDisplayName(SIGDN_FILESYSPATH, &path);
  if (path == nullptr) {
    result[EncodableValue("name")] = EncodableValue("");
    result[EncodableValue("error")] = EncodableValue("CHAIN_FILE_READ_FAILED");
    return result;
  }
  const std::wstring full_path(path);
  const auto separator = full_path.find_last_of(L"\\/");
  result[EncodableValue("name")] = EncodableValue(Utf8FromUtf16(
      full_path.substr(separator == std::wstring::npos ? 0 : separator + 1).c_str()));
  HANDLE file = CreateFileW(path, GENERIC_READ, FILE_SHARE_READ, nullptr,
                            OPEN_EXISTING, FILE_ATTRIBUTE_NORMAL, nullptr);
  CoTaskMemFree(path);
  if (file == INVALID_HANDLE_VALUE) {
    result[EncodableValue("error")] = EncodableValue("CHAIN_FILE_READ_FAILED");
    return result;
  }
  std::vector<uint8_t> bytes(kMaxChainFileBytes + 1);
  DWORD count = 0;
  bool ok = true;
  while (count < bytes.size()) {
    DWORD read = 0;
    if (!ReadFile(file, bytes.data() + count,
                  static_cast<DWORD>(bytes.size()) - count, &read, nullptr)) {
      ok = false;
      break;
    }
    if (read == 0) break;
    count += read;
  }
  CloseHandle(file);
  if (!ok || count == 0 || count > kMaxChainFileBytes) {
    result[EncodableValue("error")] = EncodableValue(
        count > kMaxChainFileBytes ? "CHAIN_FILE_TOO_LARGE" : "CHAIN_FILE_READ_FAILED");
    SecureZeroMemory(bytes.data(), bytes.size());
  } else {
    bytes.resize(count);
    result[EncodableValue("bytes")] = EncodableValue(std::move(bytes));
  }
  return result;
}

std::optional<flutter::EncodableList> ReadChainConfigurations(
    HWND owner, bool& cancelled, std::string& error) {
  cancelled = false;
  error = "CHAIN_FILE_READ_FAILED";
  IFileOpenDialog* dialog = nullptr;
  if (FAILED(CoCreateInstance(CLSID_FileOpenDialog, nullptr, CLSCTX_INPROC_SERVER,
                              IID_PPV_ARGS(&dialog)))) {
    error = "CHAIN_FILE_UNAVAILABLE";
    return std::nullopt;
  }
  const COMDLG_FILTERSPEC filters[] = {
      {L"VPN configuration", L"*.ovpn;*.conf"}, {L"All files", L"*.*"}};
  dialog->SetFileTypes(2, filters);
  dialog->SetOptions(FOS_FILEMUSTEXIST | FOS_PATHMUSTEXIST |
                    FOS_FORCEFILESYSTEM | FOS_ALLOWMULTISELECT);
  const HRESULT shown = dialog->Show(owner);
  if (shown == HRESULT_FROM_WIN32(ERROR_CANCELLED)) cancelled = true;
  IShellItemArray* items = nullptr;
  if (SUCCEEDED(shown)) dialog->GetResults(&items);
  dialog->Release();
  if (items == nullptr) return std::nullopt;
  DWORD count = 0;
  const HRESULT counted = items->GetCount(&count);
  if (FAILED(counted) || count > kMaxChainFiles) {
    if (count > kMaxChainFiles) error = "CHAIN_FILE_COUNT_LIMIT";
    items->Release();
    return std::nullopt;
  }
  flutter::EncodableList files;
  files.reserve(count);
  for (DWORD i = 0; i < count; ++i) {
    IShellItem* item = nullptr;
    items->GetItemAt(i, &item);
    files.emplace_back(ReadChainFile(item));
    if (item != nullptr) item->Release();
  }
  items->Release();
  return files;
}

constexpr UINT kEngineIpcComplete = WM_APP + 17;
constexpr UINT kEngineEventAvailable = WM_APP + 18;
constexpr UINT kTrayCallback = WM_APP + 19;
constexpr UINT kEngineReadyComplete = WM_APP + 20;
constexpr UINT_PTR kZeroTrustLoginTimer = 41004;
constexpr UINT kZeroTrustLoginTimeoutMs = 10 * 60 * 1000;
constexpr UINT kZeroTrustCleanupRetryMs = 10 * 1000;
constexpr UINT kTrayOpen = 41001;
constexpr UINT kTrayToggle = 41002;
constexpr UINT kTrayDisconnectExit = 41003;
constexpr UINT kTrayTunnel = 41005;
constexpr UINT kTraySystemProxy = 41006;
constexpr wchar_t kCloseToTrayValue[] = L"CloseToTray";
std::atomic<uint64_t> g_engine_event_generation = 0;
const UINT kTaskbarCreated = ::RegisterWindowMessageW(L"TaskbarCreated");

bool ReadCloseToTray() {
  DWORD value = 1;
  DWORD size = sizeof(value);
  const LSTATUS status = ::RegGetValueW(
      HKEY_CURRENT_USER, usque::kUsqueSettingsKey, kCloseToTrayValue,
      RRF_RT_REG_DWORD, nullptr, &value, &size);
  return status != ERROR_SUCCESS || value != 0;
}

bool WriteCloseToTray(bool enabled) {
  HKEY key = nullptr;
  if (::RegCreateKeyExW(HKEY_CURRENT_USER, usque::kUsqueSettingsKey, 0,
                        nullptr, 0, KEY_SET_VALUE, nullptr, &key, nullptr) !=
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

std::optional<COLORREF> TrayBadgeColor(const std::string& badge) {
  if (badge == "connected") return RGB(0x4A, 0xDE, 0x9C);
  if (badge == "busy" || badge == "warning") return RGB(0xF2, 0xB2, 0x4C);
  if (badge == "error") return RGB(0xE5, 0x53, 0x4B);
  return std::nullopt;
}

// Paints a status dot into the bottom-right corner of straight-alpha BGRA
// |pixels|, cutting a transparent ring so the dot stays legible on the
// artwork and on any taskbar colour.
void PaintTrayBadge(std::vector<uint32_t>& pixels, int width, int height,
                    COLORREF color) {
  const double size = static_cast<double>(std::min(width, height));
  const double radius = size * 0.23;
  const double ring = std::max(1.0, size / 16.0);
  const double center_x = width - radius;
  const double center_y = height - radius;
  const double fill_r = GetRValue(color);
  const double fill_g = GetGValue(color);
  const double fill_b = GetBValue(color);
  for (int y = 0; y < height; ++y) {
    for (int x = 0; x < width; ++x) {
      const double distance =
          std::hypot(x + 0.5 - center_x, y + 0.5 - center_y);
      const double cleared =
          std::clamp(radius + ring - distance + 0.5, 0.0, 1.0);
      if (cleared <= 0) continue;
      const double fill = std::clamp(radius - distance + 0.5, 0.0, 1.0);
      uint32_t& pixel = pixels[static_cast<size_t>(y) * width + x];
      const double alpha = ((pixel >> 24) & 0xFF) / 255.0 * (1 - cleared);
      const double out_alpha = fill + alpha * (1 - fill);
      if (out_alpha <= 0) {
        pixel = 0;
        continue;
      }
      const auto blend = [&](double top, int shift) {
        const double bottom = (pixel >> shift) & 0xFF;
        return static_cast<uint32_t>(std::lround(
            (top * fill + bottom * alpha * (1 - fill)) / out_alpha));
      };
      pixel = (static_cast<uint32_t>(std::lround(out_alpha * 255)) << 24) |
              (blend(fill_r, 16) << 16) | (blend(fill_g, 8) << 8) |
              blend(fill_b, 0);
    }
  }
}

// Returns nullptr when |base| has no alpha channel to composite against; the
// caller then keeps the plain icon and relies on the tooltip.
HICON CreateBadgedTrayIcon(HICON base, COLORREF color) {
  ICONINFO info{};
  if (base == nullptr || !::GetIconInfo(base, &info)) return nullptr;
  HICON result = nullptr;
  BITMAP bitmap{};
  if (info.hbmColor != nullptr &&
      ::GetObjectW(info.hbmColor, sizeof(bitmap), &bitmap) != 0 &&
      bitmap.bmWidth > 0 && bitmap.bmHeight > 0) {
    const int width = bitmap.bmWidth;
    const int height = bitmap.bmHeight;
    BITMAPINFO bitmap_info{};
    bitmap_info.bmiHeader.biSize = sizeof(BITMAPINFOHEADER);
    bitmap_info.bmiHeader.biWidth = width;
    bitmap_info.bmiHeader.biHeight = -height;
    bitmap_info.bmiHeader.biPlanes = 1;
    bitmap_info.bmiHeader.biBitCount = 32;
    bitmap_info.bmiHeader.biCompression = BI_RGB;
    std::vector<uint32_t> pixels(static_cast<size_t>(width) * height);
    HDC screen = ::GetDC(nullptr);
    const bool read =
        screen != nullptr &&
        ::GetDIBits(screen, info.hbmColor, 0, height, pixels.data(),
                    &bitmap_info, DIB_RGB_COLORS) == height;
    const bool has_alpha =
        read && std::any_of(pixels.begin(), pixels.end(),
                            [](uint32_t pixel) { return (pixel >> 24) != 0; });
    if (has_alpha) {
      PaintTrayBadge(pixels, width, height, color);
      void* bits = nullptr;
      HBITMAP color_bitmap = ::CreateDIBSection(
          screen, &bitmap_info, DIB_RGB_COLORS, &bits, nullptr, 0);
      const std::vector<uint8_t> mask_bits(
          static_cast<size_t>((width + 15) / 16) * 2 * height, 0);
      HBITMAP mask = ::CreateBitmap(width, height, 1, 1, mask_bits.data());
      if (color_bitmap != nullptr && bits != nullptr && mask != nullptr) {
        std::memcpy(bits, pixels.data(), pixels.size() * sizeof(uint32_t));
        ICONINFO badged{TRUE, 0, 0, mask, color_bitmap};
        result = ::CreateIconIndirect(&badged);
      }
      if (mask != nullptr) ::DeleteObject(mask);
      if (color_bitmap != nullptr) ::DeleteObject(color_bitmap);
    }
    if (screen != nullptr) ::ReleaseDC(nullptr, screen);
  }
  if (info.hbmColor != nullptr) ::DeleteObject(info.hbmColor);
  if (info.hbmMask != nullptr) ::DeleteObject(info.hbmMask);
  return result;
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
                             bool start_hidden, bool start_maximized)
    : project_(project),
      start_hidden_(start_hidden),
      pending_maximize_(start_maximized) {}

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
  // Recover a previous interrupted login or migrate the old persistent toggle.
  ReleaseZeroTrustProtocol();
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
        if (call.method_name() == "readChainConfigurations") {
          if (chain_picker_busy_) {
            result->Error("CHAIN_FILE_BUSY", "A file picker is already open.");
            return;
          }
          chain_picker_busy_ = true;
          bool cancelled = false;
          std::string error;
          auto files = ReadChainConfigurations(GetHandle(), cancelled, error);
          chain_picker_busy_ = false;
          if (files) {
            flutter::EncodableValue value(std::move(*files));
            result->Success(value);
            for (auto& entry : std::get<flutter::EncodableList>(value)) {
              auto& fields = std::get<flutter::EncodableMap>(entry);
              const auto found = fields.find(flutter::EncodableValue("bytes"));
              if (found != fields.end()) {
                auto& data = std::get<std::vector<uint8_t>>(found->second);
                SecureZeroMemory(data.data(), data.size());
              }
            }
          } else if (cancelled) {
            result->Success();
          } else {
            result->Error(error, "Configuration file could not be read.");
          }
          return;
        }
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
              flutter::EncodableValue(usque::shell::IsStartOnLoginEnabled());
          preferences[flutter::EncodableValue("close_to_tray")] =
              flutter::EncodableValue(close_to_tray_);
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
          if (!ReleaseZeroTrustProtocol() ||
              !SetCurrentUserWarpProtocolAssociation(true) ||
              ::SetTimer(GetHandle(), kZeroTrustLoginTimer,
                         kZeroTrustLoginTimeoutMs, nullptr) == 0) {
            zero_trust_session_.Cancel();
            ReleaseZeroTrustProtocol();
            result->Error("ZERO_TRUST_PROTOCOL_FAILED",
                          "Windows could not prepare the Access callback handler.");
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
          if (!ReleaseZeroTrustProtocol()) {
            result->Error("ZERO_TRUST_PROTOCOL_FAILED",
                          "Windows could not restore the Access callback handler.");
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
                                 ? usque::shell::SetStartOnLogin(enabled)
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
          const auto read_utf8 = [arguments](const char* key) -> std::string {
            const auto iterator =
                arguments->find(flutter::EncodableValue(key));
            if (iterator == arguments->end() ||
                !std::holds_alternative<std::string>(iterator->second)) {
              return {};
            }
            return std::get<std::string>(iterator->second);
          };
          const auto connected_it =
              arguments->find(flutter::EncodableValue("connected"));
          std::string status = read_utf8("status");
          if (status.empty()) {
            status = read_utf8("phase");
          }
          if (status.empty() || connected_it == arguments->end() ||
              !std::holds_alternative<bool>(connected_it->second)) {
            result->Error("INVALID_ARGUMENT", "Tray state is malformed.");
            return;
          }
          auto assign_label = [&](std::wstring& target, const char* key) {
            const std::string value = read_utf8(key);
            if (!value.empty()) {
              target = Utf16FromUtf8(value);
            }
          };
          const auto read_bool = [arguments](const char* key) {
            const auto iterator =
                arguments->find(flutter::EncodableValue(key));
            return iterator != arguments->end() &&
                   std::holds_alternative<bool>(iterator->second) &&
                   std::get<bool>(iterator->second);
          };
          assign_label(tray_open_, "open");
          assign_label(tray_connect_, "connect");
          assign_label(tray_disconnect_, "disconnect");
          assign_label(tray_exit_, "disconnect_exit");
          tray_tunnel_label_ = Utf16FromUtf8(read_utf8("tunnel_label"));
          tray_system_proxy_label_ =
              Utf16FromUtf8(read_utf8("system_proxy_label"));
          tray_tunnel_ = read_bool("tunnel");
          tray_system_proxy_ = read_bool("system_proxy");
          tray_outputs_enabled_ = read_bool("outputs_enabled");
          tray_system_proxy_available_ = read_bool("system_proxy_available");
          UpdateTrayState(status, std::get<bool>(connected_it->second));
          ApplyTrayBadge(read_utf8("badge"));
          result->Success();
          return;
        }
        if (call.method_name() == "showTrayNotification") {
          const auto* arguments =
              std::get_if<flutter::EncodableMap>(call.arguments());
          const auto read_utf8 = [arguments](const char* key) -> std::string {
            if (arguments == nullptr) return {};
            const auto iterator =
                arguments->find(flutter::EncodableValue(key));
            if (iterator == arguments->end() ||
                !std::holds_alternative<std::string>(iterator->second)) {
              return {};
            }
            return std::get<std::string>(iterator->second);
          };
          const std::string title = read_utf8("title");
          const std::string body = read_utf8("body");
          const std::string level = read_utf8("level");
          if (title.empty() || body.empty()) {
            result->Error("INVALID_ARGUMENT", "Tray notification is malformed.");
            return;
          }
          const DWORD flags = level == "error"     ? NIIF_ERROR
                              : level == "warning" ? NIIF_WARNING
                                                   : NIIF_INFO;
          result->Success(flutter::EncodableValue(ShowTrayNotification(
              Utf16FromUtf8(title), Utf16FromUtf8(body), flags)));
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

  RememberNormalBounds();
  flutter_controller_->engine()->SetNextFrameCallback([&]() {
    if (start_hidden_) return;
    if (pending_maximize_) {
      pending_maximize_ = false;
      ::ShowWindow(GetHandle(), SW_SHOWMAXIMIZED);
    } else {
      this->Show();
    }
  });

  // Flutter can complete the first frame before the "show window" callback is
  // registered. The following call ensures a frame is pending to ensure the
  // window is shown. It is a no-op if the first frame hasn't completed yet.
  flutter_controller_->ForceRedraw();

  return true;
}

void FlutterWindow::OnDestroy() {
  zero_trust_session_.Cancel();
  ReleaseZeroTrustProtocol();
  ::KillTimer(GetHandle(), kZeroTrustLoginTimer);
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
  if (tray_base_icon_ == nullptr) {
    tray_base_icon_ = static_cast<HICON>(::LoadImageW(
        ::GetModuleHandleW(nullptr), MAKEINTRESOURCEW(IDI_APP_ICON),
        IMAGE_ICON, ::GetSystemMetrics(SM_CXSMICON),
        ::GetSystemMetrics(SM_CYSMICON), LR_DEFAULTCOLOR));
  }
  tray_icon_ = {};
  tray_icon_.cbSize = sizeof(tray_icon_);
  tray_icon_.hWnd = GetHandle();
  tray_icon_.uID = 1;
  // NOTIFYICON_VERSION_4 suppresses the standard tooltip without NIF_SHOWTIP.
  tray_icon_.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP | NIF_SHOWTIP;
  tray_icon_.uCallbackMessage = kTrayCallback;
  tray_icon_.hIcon =
      tray_badge_icon_ != nullptr ? tray_badge_icon_ : tray_base_icon_;
  const std::wstring tooltip = L"Usque - " + tray_status_;
  wcsncpy_s(tray_icon_.szTip, tooltip.c_str(), _TRUNCATE);
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
  tray_icon_.hIcon = nullptr;
  if (tray_badge_icon_ != nullptr) {
    ::DestroyIcon(tray_badge_icon_);
    tray_badge_icon_ = nullptr;
  }
  if (tray_base_icon_ != nullptr) {
    ::DestroyIcon(tray_base_icon_);
    tray_base_icon_ = nullptr;
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
  tray_icon_.uFlags = NIF_TIP | NIF_SHOWTIP;
  ::Shell_NotifyIconW(NIM_MODIFY, &tray_icon_);
}

void FlutterWindow::ApplyTrayBadge(const std::string& badge) {
  if (badge == tray_badge_) return;
  tray_badge_ = badge;
  HICON previous = tray_badge_icon_;
  const std::optional<COLORREF> color = TrayBadgeColor(badge);
  tray_badge_icon_ = color.has_value()
                         ? CreateBadgedTrayIcon(tray_base_icon_, *color)
                         : nullptr;
  tray_icon_.hIcon =
      tray_badge_icon_ != nullptr ? tray_badge_icon_ : tray_base_icon_;
  if (tray_icon_added_) {
    tray_icon_.uFlags = NIF_ICON;
    ::Shell_NotifyIconW(NIM_MODIFY, &tray_icon_);
  }
  if (previous != nullptr) ::DestroyIcon(previous);
}

bool FlutterWindow::ShowTrayNotification(const std::wstring& title,
                                         const std::wstring& body,
                                         DWORD level) {
  if (!tray_icon_added_) return false;
  const HWND window = GetHandle();
  if (::IsWindowVisible(window) && !::IsIconic(window) &&
      ::GetForegroundWindow() == window) {
    return false;
  }
  NOTIFYICONDATAW notice{};
  notice.cbSize = sizeof(notice);
  notice.hWnd = window;
  notice.uID = tray_icon_.uID;
  notice.uFlags = NIF_INFO;
  notice.dwInfoFlags = level | NIIF_RESPECT_QUIET_TIME;
  wcsncpy_s(notice.szInfoTitle, title.c_str(), _TRUNCATE);
  wcsncpy_s(notice.szInfo, body.c_str(), _TRUNCATE);
  return ::Shell_NotifyIconW(NIM_MODIFY, &notice) == TRUE;
}

void FlutterWindow::ShowAndActivate() {
  const HWND window = GetHandle();
  if (::IsIconic(window)) {
    ::ShowWindow(window, SW_RESTORE);
  } else if (!::IsWindowVisible(window)) {
    // SW_RESTORE would un-maximize a window that was hidden to the tray.
    const bool maximize = pending_maximize_;
    pending_maximize_ = false;
    ::ShowWindow(window, maximize ? SW_SHOWMAXIMIZED : SW_SHOW);
  }
  ::SetForegroundWindow(window);
}

void FlutterWindow::RememberNormalBounds() {
  const HWND window = GetHandle();
  if (window == nullptr || ::IsIconic(window) || ::IsZoomed(window)) return;
  RECT bounds{};
  if (::GetWindowRect(window, &bounds)) normal_bounds_ = bounds;
}

void FlutterWindow::SaveWindowPlacement() {
  const HWND window = GetHandle();
  if (window == nullptr || !normal_bounds_.has_value() ||
      !::IsWindowVisible(window) || ::IsIconic(window)) {
    return;
  }
  usque::WindowPlacement placement;
  placement.bounds = *normal_bounds_;
  placement.dpi = FlutterDesktopGetDpiForHWND(window);
  placement.maximized = ::IsZoomed(window) != FALSE;
  usque::WriteWindowPlacement(HKEY_CURRENT_USER, usque::kUsqueSettingsKey,
                              placement);
}

void FlutterWindow::NotifyZeroTrustCallbackArrived() {
  if (!engine_channel_) return;
  engine_channel_->InvokeMethod("zeroTrustCallbackArrived", nullptr);
}

bool FlutterWindow::ReleaseZeroTrustProtocol() {
  if (!SetCurrentUserWarpProtocolAssociation(false)) {
    ::SetTimer(GetHandle(), kZeroTrustLoginTimer, kZeroTrustCleanupRetryMs,
               nullptr);
    return false;
  }
  ::KillTimer(GetHandle(), kZeroTrustLoginTimer);
  return true;
}

void FlutterWindow::OfferZeroTrustCallback(std::string_view callback_uri) {
  if (!zero_trust_session_.Accept(callback_uri)) return;
  ReleaseZeroTrustProtocol();
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
  ::AppendMenuW(menu, MF_STRING, kTrayOpen, tray_open_.c_str());
  ::AppendMenuW(menu, MF_STRING, kTrayToggle,
                tray_connected_ ? tray_disconnect_.c_str()
                                : tray_connect_.c_str());
  if (!tray_tunnel_label_.empty() || !tray_system_proxy_label_.empty()) {
    ::AppendMenuW(menu, MF_SEPARATOR, 0, nullptr);
  }
  if (!tray_tunnel_label_.empty()) {
    ::AppendMenuW(menu,
                  MF_STRING | (tray_tunnel_ ? MF_CHECKED : MF_UNCHECKED) |
                      (tray_outputs_enabled_ ? MF_ENABLED : MF_GRAYED),
                  kTrayTunnel, tray_tunnel_label_.c_str());
  }
  if (!tray_system_proxy_label_.empty()) {
    // Turning system proxy on needs the HTTP local proxy; turning it off never
    // does.
    const bool available = tray_outputs_enabled_ &&
                           (tray_system_proxy_ || tray_system_proxy_available_);
    ::AppendMenuW(menu,
                  MF_STRING | (tray_system_proxy_ ? MF_CHECKED : MF_UNCHECKED) |
                      (available ? MF_ENABLED : MF_GRAYED),
                  kTraySystemProxy, tray_system_proxy_label_.c_str());
  }
  ::AppendMenuW(menu, MF_SEPARATOR, 0, nullptr);
  ::AppendMenuW(menu, MF_STRING, kTrayDisconnectExit, tray_exit_.c_str());
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
  } else if (command == kTrayTunnel) {
    InvokeTrayCommand("toggleTunnel", false);
  } else if (command == kTraySystemProxy) {
    InvokeTrayCommand("toggleSystemProxy", false);
  } else if (command == kTrayDisconnectExit) {
    RequestDisconnectAndExit();
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

void FlutterWindow::RequestDisconnectAndExit() {
  if (force_exit_ || exit_pending_) return;
  exit_pending_ = true;
  if (engine_channel_) {
    InvokeTrayCommand("disconnectAndExit", true);
    return;
  }

  // A maintenance request can arrive while the Flutter engine is still being
  // created. No tunnel can be owned at that point, so release the executable
  // immediately instead of making Restart Manager wait for its force timeout.
  force_exit_ = true;
  ::PostMessageW(GetHandle(), WM_CLOSE, 0, 0);
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
  if (message == WM_TIMER && wparam == kZeroTrustLoginTimer) {
    zero_trust_session_.Cancel();
    ReleaseZeroTrustProtocol();
    return 0;
  }
  if (message == WM_COPYDATA) {
    return HandleZeroTrustCopyData(
               reinterpret_cast<const COPYDATASTRUCT*>(lparam))
               ? TRUE
               : FALSE;
  }

  switch (usque::ClassifyMaintenanceShutdownMessage(message, wparam, lparam)) {
    case usque::MaintenanceShutdownAction::kAllow:
      // Both Windows shutdown and Restart Manager confirm with WM_ENDSESSION.
      // Do not start cleanup during the cancellable query.
      return TRUE;
    case usque::MaintenanceShutdownAction::kCommit:
      RequestDisconnectAndExit();
      return 0;
    case usque::MaintenanceShutdownAction::kNone:
      break;
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
    // Explorer restarted and forgot the icon; the cached images stay valid.
    tray_icon_added_ = false;
    AddTrayIcon();
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
      } else {
        engine_event_sink_->Error("ENGINE_EVENT_UNAVAILABLE",
                                  pending->ipc.error);
        engine_event_sink_->EndOfStream();
        StopEngineEventStream();
      }
      return 0;
    }
    case kTrayCallback: {
      const UINT event = LOWORD(lparam);
      if (event == WM_LBUTTONUP || event == WM_LBUTTONDBLCLK ||
          event == NIN_KEYSELECT || event == NIN_BALLOONUSERCLICK) {
        ShowAndActivate();
      } else if (event == WM_RBUTTONUP || event == WM_CONTEXTMENU) {
        ShowTrayMenu();
      }
      return 0;
    }
    case WM_SIZE:
      RememberNormalBounds();
      usque::PublishWindowFrameState(hwnd, false);
      break;
    case WM_ACTIVATE:
      usque::PublishWindowFrameState(hwnd, false);
      break;
    case WM_EXITSIZEMOVE:
      SaveWindowPlacement();
      break;
    case WM_SHOWWINDOW:
      // A path that showed the window without consulting the saved state,
      // such as a second instance asking it to come forward.
      if (wparam == TRUE && pending_maximize_) {
        pending_maximize_ = false;
        ::PostMessageW(hwnd, WM_SYSCOMMAND, SC_MAXIMIZE, 0);
      }
      break;
    case WM_CLOSE:
      SaveWindowPlacement();
      if (force_exit_) {
        break;
      }
      if (close_to_tray_) {
        ::ShowWindow(hwnd, SW_HIDE);
        return 0;
      }
      if (!exit_pending_) {
        RequestDisconnectAndExit();
      }
      return 0;
    case WM_FONTCHANGE:
      flutter_controller_->engine()->ReloadSystemFonts();
      break;
  }

  return Win32Window::MessageHandler(hwnd, message, wparam, lparam);
}
