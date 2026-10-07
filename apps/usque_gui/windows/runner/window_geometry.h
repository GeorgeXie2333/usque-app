#ifndef RUNNER_WINDOW_GEOMETRY_H_
#define RUNNER_WINDOW_GEOMETRY_H_

#include <windows.h>

#include <algorithm>

namespace usque {

// Logical client size, including the Flutter-drawn caption.
inline constexpr int kDefaultWindowWidth = 1020;
inline constexpr int kDefaultWindowHeight = 728;

// Physical pixels. Keep the initial window and its minimum tracking size
// inside the monitor's usable area, including at high DPI or above a taskbar.
inline RECT FitWindowBounds(RECT bounds, const RECT& work_area) {
  const LONG work_width = work_area.right - work_area.left;
  const LONG work_height = work_area.bottom - work_area.top;
  if (work_width <= 0 || work_height <= 0) return bounds;

  const LONG width =
      std::clamp<LONG>(bounds.right - bounds.left, 1, work_width);
  const LONG height =
      std::clamp<LONG>(bounds.bottom - bounds.top, 1, work_height);
  const LONG left =
      std::clamp(bounds.left, work_area.left, work_area.right - width);
  const LONG top =
      std::clamp(bounds.top, work_area.top, work_area.bottom - height);
  return {left, top, left + width, top + height};
}

// Physical pixels. Centres a window of |width| x |height| in |work_area|.
inline RECT CenterWindowBounds(LONG width, LONG height, const RECT& work_area) {
  const LONG left = work_area.left + (work_area.right - work_area.left - width) / 2;
  const LONG top = work_area.top + (work_area.bottom - work_area.top - height) / 2;
  return FitWindowBounds({left, top, left + width, top + height}, work_area);
}

// Physical pixels. Keeps a saved window's top-left corner and logical size
// when its monitor's DPI changed between sessions.
inline RECT RestoreWindowBounds(const RECT& saved, UINT saved_dpi, UINT dpi,
                                const RECT& work_area) {
  LONG width = saved.right - saved.left;
  LONG height = saved.bottom - saved.top;
  if (saved_dpi != 0 && dpi != 0 && saved_dpi != dpi) {
    width = ::MulDiv(width, static_cast<int>(dpi), static_cast<int>(saved_dpi));
    height =
        ::MulDiv(height, static_cast<int>(dpi), static_cast<int>(saved_dpi));
  }
  return FitWindowBounds(
      {saved.left, saved.top, saved.left + width, saved.top + height},
      work_area);
}

}  // namespace usque

#endif  // RUNNER_WINDOW_GEOMETRY_H_
