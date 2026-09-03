#ifndef RUNNER_MAINTENANCE_SHUTDOWN_H_
#define RUNNER_MAINTENANCE_SHUTDOWN_H_

#include <windows.h>

namespace usque {

enum class MaintenanceShutdownAction {
  kNone,
  kAllow,
  kCommit,
};

// Classifies confirmed session endings, including Restart Manager maintenance,
// ordinary shutdown/restart and sign-out. Query/cancellation must not disconnect.
MaintenanceShutdownAction ClassifyMaintenanceShutdownMessage(
    UINT message,
    WPARAM wparam,
    LPARAM lparam) noexcept;

}  // namespace usque

#endif  // RUNNER_MAINTENANCE_SHUTDOWN_H_
