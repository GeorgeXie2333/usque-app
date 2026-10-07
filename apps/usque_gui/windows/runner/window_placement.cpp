#include "window_placement.h"

#include <cstdint>

namespace usque {
namespace {

constexpr wchar_t kWindowPlacementValue[] = L"WindowPlacement";
constexpr uint32_t kWindowPlacementVersion = 1;
constexpr uint32_t kMaximizedFlag = 1;
constexpr LONG kMinimumStoredExtent = 64;
constexpr LONG kMaximumStoredExtent = 32768;

struct StoredWindowPlacement {
  uint32_t version;
  int32_t left;
  int32_t top;
  int32_t right;
  int32_t bottom;
  uint32_t dpi;
  uint32_t flags;
};

bool ValidExtent(LONG extent) {
  return extent >= kMinimumStoredExtent && extent <= kMaximumStoredExtent;
}

}  // namespace

std::optional<WindowPlacement> ReadWindowPlacement(HKEY root,
                                                   const wchar_t* key) {
  StoredWindowPlacement stored{};
  DWORD size = sizeof(stored);
  if (::RegGetValueW(root, key, kWindowPlacementValue, RRF_RT_REG_BINARY,
                     nullptr, &stored, &size) != ERROR_SUCCESS ||
      size != sizeof(stored) || stored.version != kWindowPlacementVersion ||
      stored.dpi < 48 || stored.dpi > 960 ||
      !ValidExtent(stored.right - stored.left) ||
      !ValidExtent(stored.bottom - stored.top)) {
    return std::nullopt;
  }
  WindowPlacement placement;
  placement.bounds = {stored.left, stored.top, stored.right, stored.bottom};
  placement.dpi = stored.dpi;
  placement.maximized = (stored.flags & kMaximizedFlag) != 0;
  return placement;
}

bool WriteWindowPlacement(HKEY root, const wchar_t* key,
                          const WindowPlacement& placement) {
  const RECT& bounds = placement.bounds;
  if (!ValidExtent(bounds.right - bounds.left) ||
      !ValidExtent(bounds.bottom - bounds.top)) {
    return false;
  }
  const StoredWindowPlacement stored{
      kWindowPlacementVersion,
      bounds.left,
      bounds.top,
      bounds.right,
      bounds.bottom,
      placement.dpi,
      placement.maximized ? kMaximizedFlag : 0,
  };
  HKEY handle = nullptr;
  if (::RegCreateKeyExW(root, key, 0, nullptr, 0, KEY_SET_VALUE, nullptr,
                        &handle, nullptr) != ERROR_SUCCESS) {
    return false;
  }
  const LSTATUS status = ::RegSetValueExW(
      handle, kWindowPlacementValue, 0, REG_BINARY,
      reinterpret_cast<const BYTE*>(&stored), sizeof(stored));
  ::RegCloseKey(handle);
  return status == ERROR_SUCCESS;
}

}  // namespace usque
