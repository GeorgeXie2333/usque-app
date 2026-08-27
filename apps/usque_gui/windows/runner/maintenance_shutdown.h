#ifndef RUNNER_MAINTENANCE_SHUTDOWN_H_
#define RUNNER_MAINTENANCE_SHUTDOWN_H_

#include <windows.h>

namespace usque {

enum class MaintenanceShutdownAction {
  kNone,
  kAllow,
  kCommit,
};

// Classifies the Restart Manager messages used when Windows Installer needs
// Usque to release installed files. Ordinary sign-out and system-shutdown
// messages remain on the default Win32 path.
MaintenanceShutdownAction ClassifyMaintenanceShutdownMessage(
    UINT message,
    WPARAM wparam,
    LPARAM lparam) noexcept;

}  // namespace usque

#endif  // RUNNER_MAINTENANCE_SHUTDOWN_H_
