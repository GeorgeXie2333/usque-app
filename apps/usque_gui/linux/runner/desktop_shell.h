#ifndef USQUE_LINUX_DESKTOP_SHELL_H_
#define USQUE_LINUX_DESKTOP_SHELL_H_

#include <flutter_linux/flutter_linux.h>
#include <gtk/gtk.h>

// First-launch size; a window still at this size has not been resized.
inline constexpr int kUsqueDefaultWindowWidth = 1280;
inline constexpr int kUsqueDefaultWindowHeight = 800;

void usque_register_desktop_shell(GtkApplication* application, GtkWindow* window,
                                   FlBinaryMessenger* messenger);
void usque_desktop_open_uri(const char* uri);

#endif
