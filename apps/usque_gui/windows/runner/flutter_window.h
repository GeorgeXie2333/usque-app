#ifndef RUNNER_FLUTTER_WINDOW_H_
#define RUNNER_FLUTTER_WINDOW_H_

#include <flutter/dart_project.h>
#include <flutter/event_channel.h>
#include <flutter/event_sink.h>
#include <flutter/flutter_view_controller.h>
#include <flutter/method_channel.h>

#include <atomic>
#include <cstdint>
#include <memory>
#include <string>

#include "win32_window.h"

// A window that does nothing but host a Flutter view.
class FlutterWindow : public Win32Window {
 public:
  // Creates a new FlutterWindow hosting a Flutter view running |project|.
  explicit FlutterWindow(const flutter::DartProject& project,
                         bool start_hidden = false);
  virtual ~FlutterWindow();

 protected:
  // Win32Window:
  bool OnCreate() override;
  void OnDestroy() override;
  LRESULT MessageHandler(HWND window, UINT const message, WPARAM const wparam,
                         LPARAM const lparam) noexcept override;

 private:
  void StopEngineEventStream();
  void AddTrayIcon();
  void RemoveTrayIcon();
  void ShowTrayMenu();
  void UpdateTrayState(const std::string& phase, bool connected);
  void InvokeTrayCommand(const std::string& command, bool exit_on_success);
  void ShowAndActivate();

  // The project to run.
  flutter::DartProject project_;

  // The Flutter instance hosted by this window.
  std::unique_ptr<flutter::FlutterViewController> flutter_controller_;
  std::unique_ptr<flutter::MethodChannel<flutter::EncodableValue>>
      engine_channel_;
  std::unique_ptr<flutter::EventChannel<flutter::EncodableValue>>
      engine_event_channel_;
  std::unique_ptr<flutter::EventSink<flutter::EncodableValue>>
      engine_event_sink_;
  std::shared_ptr<std::atomic_bool> engine_event_active_;
  uint64_t engine_event_generation_ = 0;
  bool start_hidden_ = false;
  bool close_to_tray_ = true;
  bool force_exit_ = false;
  bool exit_pending_ = false;
  bool tray_icon_added_ = false;
  bool tray_connected_ = false;
  std::wstring tray_status_ = L"Disconnected";
  NOTIFYICONDATAW tray_icon_{};
};

#endif  // RUNNER_FLUTTER_WINDOW_H_
