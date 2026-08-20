#ifndef RUNNER_WINDOW_FRAME_H_
#define RUNNER_WINDOW_FRAME_H_

#include <windows.h>

#include <optional>
#include <string>

// Client-side decorated window frame.
//
// The caption and its buttons are removed so Flutter paints the whole window,
// while resizing, snapping, maximizing, and the DWM shadow stay native. The
// Dart title bar asks the shell to start a native move or resize loop instead
// of moving the window itself, so Windows keeps ownership of snap layouts and
// drag-to-edge behaviour.
namespace usque {

// Smallest usable window, in logical pixels. Below this the shell layout has
// nowhere left to collapse.
inline constexpr int kMinimumWindowWidth = 520;
inline constexpr int kMinimumWindowHeight = 600;

// Handles the frame-related window messages. Returns the value the window
// procedure should return, or std::nullopt to continue normal handling.
std::optional<LRESULT> HandleCustomFrameMessage(HWND window, UINT message,
                                                WPARAM wparam, LPARAM lparam);

// Forces one frame recalculation, which must run once after the window is
// created: the WM_NCCALCSIZE sent during CreateWindow is answered with the
// cached standard frame, so without this the client area keeps the caption
// inset and Windows draws its own title bar over the Flutter one.
void ApplyCustomFrame(HWND window);

bool IsWindowMaximized(HWND window);

// Hands the pointer to the native move loop. Must be called while the primary
// mouse button is still down.
void BeginWindowDrag(HWND window);

// Hands the pointer to the native resize loop for one of "left", "top",
// "right", "bottom", "topLeft", "topRight", "bottomLeft", "bottomRight".
// Returns false when the edge name is not recognised.
bool BeginWindowResize(HWND window, const std::string& edge);

void MinimizeWindow(HWND window);

void ToggleWindowMaximize(HWND window);

}  // namespace usque

#endif  // RUNNER_WINDOW_FRAME_H_
