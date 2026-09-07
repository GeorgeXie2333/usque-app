#ifndef RUNNER_WINDOW_GEOMETRY_H_
#define RUNNER_WINDOW_GEOMETRY_H_

#include <windows.h>

#include <algorithm>

namespace usque {

// Logical client size, including the Flutter-drawn caption.
inline constexpr int kDefaultWindowWidth = 1200;
inline constexpr int kDefaultWindowHeight = 840;

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

}  // namespace usque

#endif  // RUNNER_WINDOW_GEOMETRY_H_
