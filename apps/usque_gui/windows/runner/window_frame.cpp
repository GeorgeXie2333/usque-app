#include "window_frame.h"

#include <shellapi.h>

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

}  // namespace

bool IsWindowMaximized(HWND window) {
  WINDOWPLACEMENT placement{};
  placement.length = sizeof(placement);
  if (!::GetWindowPlacement(window, &placement)) {
    return false;
  }
  return placement.showCmd == SW_SHOWMAXIMIZED;
}

std::optional<LRESULT> HandleCustomFrameMessage(HWND window, UINT message,
                                                WPARAM wparam, LPARAM lparam) {
  switch (message) {
    case WM_NCCALCSIZE: {
      if (wparam != TRUE) {
        return std::nullopt;
      }
      // Give the whole window to the client area so Flutter paints the title
      // bar. The resize borders live outside the visible bounds and stay
      // functional because the window keeps WS_THICKFRAME.
      auto* params = reinterpret_cast<NCCALCSIZE_PARAMS*>(lparam);
      RECT& client = params->rgrc[0];
      if (IsWindowMaximized(window)) {
        // A maximized window is sized to the work area plus the invisible
        // frame; without this inset the top and sides would be clipped.
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
    default:
      return std::nullopt;
  }
}

void ApplyCustomFrame(HWND window) {
  ::SetWindowPos(window, nullptr, 0, 0, 0, 0,
                 SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER |
                     SWP_NOACTIVATE | SWP_NOOWNERZORDER);
}

void BeginWindowDrag(HWND window) {
  ::ReleaseCapture();
  ::SendMessageW(window, WM_NCLBUTTONDOWN, HTCAPTION, 0);
}

bool BeginWindowResize(HWND window, const std::string& edge) {
  WPARAM hit = 0;
  if (edge == "left") {
    hit = HTLEFT;
  } else if (edge == "top") {
    hit = HTTOP;
  } else if (edge == "right") {
    hit = HTRIGHT;
  } else if (edge == "bottom") {
    hit = HTBOTTOM;
  } else if (edge == "topLeft") {
    hit = HTTOPLEFT;
  } else if (edge == "topRight") {
    hit = HTTOPRIGHT;
  } else if (edge == "bottomLeft") {
    hit = HTBOTTOMLEFT;
  } else if (edge == "bottomRight") {
    hit = HTBOTTOMRIGHT;
  } else {
    return false;
  }
  if (IsWindowMaximized(window)) {
    return true;
  }
  ::ReleaseCapture();
  ::SendMessageW(window, WM_NCLBUTTONDOWN, hit, 0);
  return true;
}

void MinimizeWindow(HWND window) {
  ::ShowWindow(window, SW_MINIMIZE);
}

void ToggleWindowMaximize(HWND window) {
  ::ShowWindow(window, IsWindowMaximized(window) ? SW_RESTORE : SW_MAXIMIZE);
}

}  // namespace usque
