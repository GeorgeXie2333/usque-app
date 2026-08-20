#ifndef RUNNER_WINDOW_FRAME_H_
#define RUNNER_WINDOW_FRAME_H_

#include <windows.h>

#include <optional>

namespace flutter {
class BinaryMessenger;
}

// Client-side decorated window frame.
//
// The caption is painted by Flutter. Hit-testing stays native so Windows
// keeps move, snap, resize, and the Win11 maximize flyout. Close still goes
// through WM_CLOSE so the tray "close to tray" path is unchanged.
namespace usque {

// Smallest usable window, in logical pixels. Below this the shell layout has
// nowhere left to collapse.
inline constexpr int kMinimumWindowWidth = 520;
inline constexpr int kMinimumWindowHeight = 600;

// Painted caption geometry, in logical pixels. Dart uses the same numbers.
inline constexpr int kCaptionHeightLogical = 40;
inline constexpr int kCaptionButtonWidthLogical = 46;
inline constexpr int kResizeBandLogical = 5;

// Handles the frame-related window messages. Returns the value the window
// procedure should return, or std::nullopt to continue normal handling.
std::optional<LRESULT> HandleCustomFrameMessage(HWND window, UINT message,
                                                WPARAM wparam, LPARAM lparam);

// Forces one frame recalculation after create, then extends the DWM frame so
// the drop shadow and rounded corners survive a client-area caption.
void ApplyCustomFrame(HWND window);

bool IsWindowMaximized(HWND window);

// Owns the dedicated window-frame method channel. Must run after the engine
// exists. Incoming Dart calls are read-only; clicks never come from Flutter.
void BindWindowFrameChannel(flutter::BinaryMessenger* messenger, HWND window);

// Pushes maximized / active / caption-hover state to Dart when it changes.
void PublishWindowFrameState(HWND window, bool force);

}  // namespace usque

#endif  // RUNNER_WINDOW_FRAME_H_
