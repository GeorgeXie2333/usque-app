#pragma once

#include <windows.h>

namespace usque::setup {
// COLORREF uses BGR byte order. Match the Flutter primary/onPrimary pairs.
constexpr COLORREF BrandAccent(bool dark) {
  return dark ? RGB(255, 164, 92) : RGB(194, 80, 12);
}

constexpr COLORREF BrandOnAccent(bool dark) {
  return dark ? RGB(68, 24, 0) : RGB(255, 255, 255);
}

constexpr COLORREF BrandPressed(bool dark) {
  const COLORREF color = BrandAccent(dark);
  return RGB((GetRValue(color) * 9 + 5) / 10,
             (GetGValue(color) * 9 + 5) / 10,
             (GetBValue(color) * 9 + 5) / 10);
}
}  // namespace usque::setup
