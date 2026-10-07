#ifndef RUNNER_WINDOW_PLACEMENT_H_
#define RUNNER_WINDOW_PLACEMENT_H_

#include <windows.h>

#include <optional>

namespace usque {

inline constexpr wchar_t kUsqueSettingsKey[] =
    L"Software\\io.github.georgexie2333\\Usque";

// Last restored (not maximized) window rectangle, in physical pixels at |dpi|.
struct WindowPlacement {
  RECT bounds{};
  UINT dpi = 96;
  bool maximized = false;
};

// Returns std::nullopt for a missing, foreign-format, or degenerate value, so
// the caller falls back to a centred default window.
std::optional<WindowPlacement> ReadWindowPlacement(HKEY root,
                                                   const wchar_t* key);

bool WriteWindowPlacement(HKEY root, const wchar_t* key,
                          const WindowPlacement& placement);

}  // namespace usque

#endif  // RUNNER_WINDOW_PLACEMENT_H_
