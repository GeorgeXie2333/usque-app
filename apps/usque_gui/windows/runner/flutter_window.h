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
#include <optional>
#include <string>
#include <string_view>

#include "win32_window.h"
#include "zero_trust_callback.h"

// A window that does nothing but host a Flutter view.
class FlutterWindow : public Win32Window {
 public:
  // Creates a new FlutterWindow hosting a Flutter view running |project|.
  // |start_maximized| applies to the first time the window becomes visible.
  explicit FlutterWindow(const flutter::DartProject& project,
                         bool start_hidden = false,
                         bool start_maximized = false);
  virtual ~FlutterWindow();

  void OfferZeroTrustCallback(std::string_view callback_uri);

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
  void ApplyTrayBadge(const std::string& badge);
  bool ShowTrayNotification(const std::wstring& title, const std::wstring& body,
                            DWORD level);
  void InvokeTrayCommand(const std::string& command, bool exit_on_success);
  void RequestDisconnectAndExit();
  void ShowAndActivate();
  void RememberNormalBounds();
  void SaveWindowPlacement();
  void NotifyZeroTrustCallbackArrived();
  bool ReleaseZeroTrustProtocol();
  bool HandleZeroTrustCopyData(const COPYDATASTRUCT* data);

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
  // Cleared by whichever path first makes the window visible.
  bool pending_maximize_ = false;
  // Last restored (not maximized or minimized) frame, in physical pixels.
  std::optional<RECT> normal_bounds_;
  bool chain_picker_busy_ = false;
  bool close_to_tray_ = true;
  bool force_exit_ = false;
  bool exit_pending_ = false;
  bool tray_icon_added_ = false;
  bool tray_connected_ = false;
  std::wstring tray_status_ = L"Disconnected";
  std::wstring tray_open_ = L"Open Usque";
  std::wstring tray_connect_ = L"Connect Active Profile";
  std::wstring tray_disconnect_ = L"Disconnect Active Profile";
  std::wstring tray_exit_ = L"Disconnect and Exit";
  // An empty label hides the corresponding output item from the tray menu.
  std::wstring tray_tunnel_label_;
  std::wstring tray_system_proxy_label_;
  bool tray_tunnel_ = false;
  bool tray_system_proxy_ = false;
  bool tray_outputs_enabled_ = false;
  bool tray_system_proxy_available_ = false;
  std::string tray_badge_ = "idle";
  HICON tray_base_icon_ = nullptr;
  HICON tray_badge_icon_ = nullptr;
  NOTIFYICONDATAW tray_icon_{};
  ZeroTrustCallbackSession zero_trust_session_;
};

#endif  // RUNNER_FLUTTER_WINDOW_H_
