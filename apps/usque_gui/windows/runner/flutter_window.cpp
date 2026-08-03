#include "flutter_window.h"

#include <flutter/event_stream_handler_functions.h>
#include <flutter/standard_method_codec.h>
#include <shobjidl.h>

#include <atomic>
#include <optional>
#include <thread>
#include <variant>

#include "engine_ipc.h"
#include "flutter/generated_plugin_registrant.h"
#include "utils.h"

namespace {

constexpr UINT kEngineIpcComplete = WM_APP + 17;
constexpr UINT kEngineEventAvailable = WM_APP + 18;
std::atomic<uint64_t> g_engine_event_generation = 0;

struct PendingEngineReply {
  std::unique_ptr<flutter::MethodResult<flutter::EncodableValue>> result;
  EngineIpcResult ipc;
};

struct PendingEngineEvent {
  uint64_t generation;
  EngineIpcResult ipc;
};

struct SaveDialogResult {
  std::optional<std::string> path;
  std::string error;
};

SaveDialogResult SelectDiagnosticsDestination(HWND owner) {
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
      {L"ZIP archive (*.zip)", L"*.zip"},
  };
  dialog->SetFileTypes(1, filters);
  dialog->SetDefaultExtension(L"zip");
  dialog->SetFileName(L"usque-diagnostics.zip");
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

FlutterWindow::FlutterWindow(const flutter::DartProject& project)
    : project_(project) {}

FlutterWindow::~FlutterWindow() {}

bool FlutterWindow::OnCreate() {
  if (!Win32Window::OnCreate()) {
    return false;
  }

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
        if (call.method_name() == "selectDiagnosticsDestination") {
          const SaveDialogResult selection =
              SelectDiagnosticsDestination(GetHandle());
          if (!selection.error.empty()) {
            result->Error("DIAGNOSTICS_DESTINATION_FAILED", selection.error);
          } else if (selection.path.has_value()) {
            result->Success(flutter::EncodableValue(*selection.path));
          } else {
            result->Success(flutter::EncodableValue());
          }
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
  SetChildContent(flutter_controller_->view()->GetNativeWindow());

  flutter_controller_->engine()->SetNextFrameCallback([&]() {
    this->Show();
  });

  // Flutter can complete the first frame before the "show window" callback is
  // registered. The following call ensures a frame is pending to ensure the
  // window is shown. It is a no-op if the first frame hasn't completed yet.
  flutter_controller_->ForceRedraw();

  return true;
}

void FlutterWindow::OnDestroy() {
  StopEngineEventStream();
  if (flutter_controller_) {
    engine_event_channel_.reset();
    engine_channel_.reset();
    flutter_controller_ = nullptr;
  }

  Win32Window::OnDestroy();
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
  // Give Flutter, including plugins, an opportunity to handle window messages.
  if (flutter_controller_) {
    std::optional<LRESULT> result =
        flutter_controller_->HandleTopLevelWindowProc(hwnd, message, wparam,
                                                      lparam);
    if (result) {
      return *result;
    }
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
    case WM_FONTCHANGE:
      flutter_controller_->engine()->ReloadSystemFonts();
      break;
  }

  return Win32Window::MessageHandler(hwnd, message, wparam, lparam);
}
