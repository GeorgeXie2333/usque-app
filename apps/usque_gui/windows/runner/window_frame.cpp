#include "window_frame.h"

#include <commctrl.h>
#include <dwmapi.h>
#include <flutter/binary_messenger.h>
#include <flutter/encodable_value.h>
#include <flutter/method_channel.h>
#include <flutter/standard_method_codec.h>
#include <shellapi.h>
#include <windowsx.h>

#include <memory>
#include <string>

namespace usque {

namespace {

UINT WindowDpi(HWND window) {
  using GetDpiForWindowProc = UINT(WINAPI*)(HWND);
  static const auto* module = ::GetModuleHandleW(L"user32.dll");
  static const auto get_dpi_for_window =
      module == nullptr
          ? nullptr
          : reinterpret_cast<GetDpiForWindowProc>(::GetProcAddress(
                const_cast<HMODULE>(module), "GetDpiForWindow"));
  if (get_dpi_for_window != nullptr) {
    const UINT dpi = get_dpi_for_window(window);
    if (dpi != 0) {
      return dpi;
    }
  }
  return USER_DEFAULT_SCREEN_DPI;
}

int MetricForDpi(int index, UINT dpi) {
  using GetSystemMetricsForDpiProc = int(WINAPI*)(int, UINT);
  static const auto* module = ::GetModuleHandleW(L"user32.dll");
  static const auto get_metrics_for_dpi =
      module == nullptr
          ? nullptr
          : reinterpret_cast<GetSystemMetricsForDpiProc>(::GetProcAddress(
                const_cast<HMODULE>(module), "GetSystemMetricsForDpi"));
  if (get_metrics_for_dpi != nullptr) {
    return get_metrics_for_dpi(index, dpi);
  }
  return ::GetSystemMetrics(index);
}

int ScaleForDpi(int logical, UINT dpi) {
  return ::MulDiv(logical, static_cast<int>(dpi), USER_DEFAULT_SCREEN_DPI);
}

// Returns the screen edge occupied by an auto-hide taskbar, or -1 when there is
// none. A maximized borderless window has to leave one pixel on that edge, or
// the taskbar can no longer be revealed.
int AutoHideTaskbarEdge() {
  APPBARDATA state{};
  state.cbSize = sizeof(state);
  const UINT flags =
      static_cast<UINT>(::SHAppBarMessage(ABM_GETSTATE, &state));
  if ((flags & ABS_AUTOHIDE) == 0) {
    return -1;
  }
  for (const UINT edge : {ABE_BOTTOM, ABE_TOP, ABE_LEFT, ABE_RIGHT}) {
    APPBARDATA probe{};
    probe.cbSize = sizeof(probe);
    probe.uEdge = edge;
    if (::SHAppBarMessage(ABM_GETAUTOHIDEBAR, &probe) != 0) {
      return static_cast<int>(edge);
    }
  }
  return -1;
}

const char* HoverName(LRESULT hit) {
  switch (hit) {
    case HTMINBUTTON:
      return "min";
    case HTMAXBUTTON:
      return "max";
    case HTCLOSE:
      return "close";
    default:
      return "none";
  }
}

flutter::EncodableMap EncodeWindowFrameState(HWND window, const char* hover) {
  flutter::EncodableMap state;
  state[flutter::EncodableValue("maximized")] =
      flutter::EncodableValue(IsWindowMaximized(window));
  state[flutter::EncodableValue("active")] =
      flutter::EncodableValue(::GetActiveWindow() == window);
  state[flutter::EncodableValue("captionHover")] =
      flutter::EncodableValue(std::string(hover));
  return state;
}

std::unique_ptr<flutter::MethodChannel<flutter::EncodableValue>> g_channel;
bool g_published = false;
bool g_maximized = false;
bool g_active = true;
std::string g_hover = "none";
bool g_tracking_leave = false;
HWND g_top_level = nullptr;
HWND g_flutter_view = nullptr;

constexpr UINT_PTR kFlutterViewSubclassId = 1;

LRESULT HitTest(HWND window, LPARAM lparam);

bool IsResizeHit(LRESULT hit) {
  switch (hit) {
    case HTLEFT:
    case HTRIGHT:
    case HTTOP:
    case HTBOTTOM:
    case HTTOPLEFT:
    case HTTOPRIGHT:
    case HTBOTTOMLEFT:
    case HTBOTTOMRIGHT:
      return true;
    default:
      return false;
  }
}

bool ApplyCaptionButton(HWND window, LRESULT hit, bool double_click) {
  switch (hit) {
    case HTMINBUTTON:
      if (!double_click) {
        ::ShowWindow(window, SW_MINIMIZE);
      }
      return true;
    case HTMAXBUTTON:
      if (!double_click) {
        ::ShowWindow(window, IsWindowMaximized(window) ? SW_RESTORE : SW_MAXIMIZE);
      }
      return true;
    case HTCLOSE:
      if (!double_click) {
        ::PostMessageW(window, WM_CLOSE, 0, 0);
      }
      return true;
    default:
      return false;
  }
}

void PublishHover(HWND window, const char* hover) {
  if (g_hover == hover && g_published) {
    return;
  }
  g_hover = hover;
  g_published = false;
  PublishWindowFrameState(window, true);
}

bool IsCaptionButtonHit(LRESULT hit) {
  return hit == HTMINBUTTON || hit == HTMAXBUTTON || hit == HTCLOSE;
}

LRESULT CALLBACK FlutterViewSubclassProc(HWND hwnd, UINT message, WPARAM wparam,
                                         LPARAM lparam, UINT_PTR subclass_id,
                                         DWORD_PTR) {
  if (g_top_level != nullptr && message == WM_NCHITTEST) {
    // Caption, caption buttons, and resize must reach the top-level window
    // so HandleCustomFrameMessage owns hit-test, hover, and clicks.
    // HTMAXBUTTON has to land on the parent for the Win11 snap flyout.
    const LRESULT hit = HitTest(g_top_level, lparam);
    if (hit == HTCAPTION || IsResizeHit(hit) || IsCaptionButtonHit(hit)) {
      return HTTRANSPARENT;
    }
  }
  if (message == WM_NCDESTROY) {
    ::RemoveWindowSubclass(hwnd, FlutterViewSubclassProc, subclass_id);
    g_flutter_view = nullptr;
    g_top_level = nullptr;
  }
  return ::DefSubclassProc(hwnd, message, wparam, lparam);
}

void TrackPointerLeave(HWND window) {
  if (g_tracking_leave) {
    return;
  }
  TRACKMOUSEEVENT track{};
  track.cbSize = sizeof(track);
  track.dwFlags = TME_LEAVE | TME_NONCLIENT;
  track.hwndTrack = window;
  if (::TrackMouseEvent(&track)) {
    g_tracking_leave = true;
  }
}

LRESULT HitTest(HWND window, LPARAM lparam) {
  POINT point{GET_X_LPARAM(lparam), GET_Y_LPARAM(lparam)};
  ::ScreenToClient(window, &point);

  RECT client{};
  ::GetClientRect(window, &client);
  const int width = client.right - client.left;
  const int height = client.bottom - client.top;
  const UINT dpi = WindowDpi(window);
  const int band = ScaleForDpi(kResizeBandLogical, dpi);
  const int corner = ScaleForDpi(static_cast<int>(kResizeBandLogical * 2.4), dpi);
  const int caption = ScaleForDpi(kCaptionHeightLogical, dpi);
  const int button = ScaleForDpi(kCaptionButtonWidthLogical, dpi);

  if (!IsWindowMaximized(window)) {
    const bool left = point.x < band;
    const bool right = point.x >= width - band;
    const bool top = point.y < band;
    const bool bottom = point.y >= height - band;
    const bool left_corner = point.x < corner;
    const bool right_corner = point.x >= width - corner;
    const bool top_corner = point.y < corner;
    const bool bottom_corner = point.y >= height - corner;
    if (left_corner && top_corner) {
      return HTTOPLEFT;
    }
    if (right_corner && top_corner) {
      return HTTOPRIGHT;
    }
    if (left_corner && bottom_corner) {
      return HTBOTTOMLEFT;
    }
    if (right_corner && bottom_corner) {
      return HTBOTTOMRIGHT;
    }
    if (left) {
      return HTLEFT;
    }
    if (right) {
      return HTRIGHT;
    }
    if (top) {
      return HTTOP;
    }
    if (bottom) {
      return HTBOTTOM;
    }
  }

  if (point.y >= 0 && point.y < caption && point.x >= 0 && point.x < width) {
    if (point.x >= width - button) {
      return HTCLOSE;
    }
    if (point.x >= width - 2 * button) {
      return HTMAXBUTTON;
    }
    if (point.x >= width - 3 * button) {
      return HTMINBUTTON;
    }
    return HTCAPTION;
  }
  return HTCLIENT;
}

}  // namespace

bool IsWindowMaximized(HWND window) {
  WINDOWPLACEMENT placement{};
  placement.length = sizeof(placement);
  if (!::GetWindowPlacement(window, &placement)) {
    return false;
  }
  return placement.showCmd == SW_SHOWMAXIMIZED;
}

void PublishWindowFrameState(HWND window, bool force) {
  if (window == nullptr) {
    return;
  }
  const bool maximized = IsWindowMaximized(window);
  const bool active = ::GetActiveWindow() == window;
  if (!force && g_published && maximized == g_maximized && active == g_active) {
    return;
  }
  g_maximized = maximized;
  g_active = active;
  g_published = true;
  if (!g_channel) {
    return;
  }
  g_channel->InvokeMethod(
      "windowFrameChanged",
      std::make_unique<flutter::EncodableValue>(
          EncodeWindowFrameState(window, g_hover.c_str())));
}

std::optional<LRESULT> HandleCustomFrameMessage(HWND window, UINT message,
                                                WPARAM wparam, LPARAM lparam) {
  switch (message) {
    case WM_NCCALCSIZE: {
      if (wparam != TRUE) {
        return std::nullopt;
      }
      // Non-maximized: the client area is the whole window. Resize comes from
      // WM_NCHITTEST edge hits, not from a leftover non-client frame.
      // Maximized: inset by the invisible frame plus one pixel on an
      // auto-hide taskbar edge so the bar can still be revealed.
      auto* params = reinterpret_cast<NCCALCSIZE_PARAMS*>(lparam);
      RECT& client = params->rgrc[0];
      if (IsWindowMaximized(window)) {
        const UINT dpi = WindowDpi(window);
        const int padding = MetricForDpi(SM_CXPADDEDBORDER, dpi);
        const int frame_x = MetricForDpi(SM_CXFRAME, dpi) + padding;
        const int frame_y = MetricForDpi(SM_CYFRAME, dpi) + padding;
        client.left += frame_x;
        client.right -= frame_x;
        client.top += frame_y;
        client.bottom -= frame_y;
        switch (AutoHideTaskbarEdge()) {
          case ABE_TOP:
            client.top += 1;
            break;
          case ABE_BOTTOM:
            client.bottom -= 1;
            break;
          case ABE_LEFT:
            client.left += 1;
            break;
          case ABE_RIGHT:
            client.right -= 1;
            break;
          default:
            break;
        }
      }
      return 0;
    }
    case WM_GETMINMAXINFO: {
      const UINT dpi = WindowDpi(window);
      auto* info = reinterpret_cast<MINMAXINFO*>(lparam);
      info->ptMinTrackSize.x = ScaleForDpi(kMinimumWindowWidth, dpi);
      info->ptMinTrackSize.y = ScaleForDpi(kMinimumWindowHeight, dpi);
      return 0;
    }
    case WM_NCHITTEST: {
      const LRESULT hit = HitTest(window, lparam);
      PublishHover(window, HoverName(hit));
      if (hit == HTMINBUTTON || hit == HTMAXBUTTON || hit == HTCLOSE ||
          hit == HTCAPTION) {
        TrackPointerLeave(window);
      }
      if (hit == HTCLIENT) {
        return std::nullopt;
      }
      return hit;
    }
    case WM_NCLBUTTONDOWN:
    case WM_NCLBUTTONDBLCLK:
      // The Flutter child returns HTTRANSPARENT for caption buttons, so
      // clicks arrive here as non-client messages.
      if (ApplyCaptionButton(window, static_cast<LRESULT>(wparam),
                             message == WM_NCLBUTTONDBLCLK)) {
        return 0;
      }
      return std::nullopt;
    case WM_MOUSELEAVE:
    case WM_NCMOUSELEAVE:
      g_tracking_leave = false;
      PublishHover(window, "none");
      return std::nullopt;
    default:
      return std::nullopt;
  }
}

void ApplyCustomFrame(HWND window) {
  const MARGINS margins{0, 0, 0, 1};
  ::DwmExtendFrameIntoClientArea(window, &margins);
  ::SetWindowPos(window, nullptr, 0, 0, 0, 0,
                 SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER |
                     SWP_NOACTIVATE | SWP_NOOWNERZORDER);
}

void BindWindowFrameChannel(flutter::BinaryMessenger* messenger, HWND window) {
  g_channel =
      std::make_unique<flutter::MethodChannel<flutter::EncodableValue>>(
          messenger, "io.github.georgexie2333.usque/window_frame",
          &flutter::StandardMethodCodec::GetInstance());
  g_channel->SetMethodCallHandler(
      [window](const flutter::MethodCall<flutter::EncodableValue>& call,
               std::unique_ptr<flutter::MethodResult<flutter::EncodableValue>>
                   result) {
        if (call.method_name() == "windowFrameState") {
          result->Success(flutter::EncodableValue(
              EncodeWindowFrameState(window, g_hover.c_str())));
          return;
        }
        result->NotImplemented();
      });
  PublishWindowFrameState(window, true);
}

void AttachFlutterView(HWND top_level, HWND flutter_view) {
  DetachFlutterView();
  if (top_level == nullptr || flutter_view == nullptr) {
    return;
  }
  if (!::SetWindowSubclass(flutter_view, FlutterViewSubclassProc,
                           kFlutterViewSubclassId, 0)) {
    return;
  }
  g_top_level = top_level;
  g_flutter_view = flutter_view;
}

void DetachFlutterView() {
  if (g_flutter_view != nullptr) {
    ::RemoveWindowSubclass(g_flutter_view, FlutterViewSubclassProc,
                           kFlutterViewSubclassId);
  }
  g_flutter_view = nullptr;
  g_top_level = nullptr;
}

}  // namespace usque
