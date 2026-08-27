#include "maintenance_shutdown.h"

namespace usque {

MaintenanceShutdownAction ClassifyMaintenanceShutdownMessage(
    UINT message,
    WPARAM wparam,
    LPARAM lparam) noexcept {
  if ((static_cast<ULONG_PTR>(lparam) & ENDSESSION_CLOSEAPP) == 0) {
    return MaintenanceShutdownAction::kNone;
  }
  if (message == WM_QUERYENDSESSION) {
    return MaintenanceShutdownAction::kAllow;
  }
  if (message == WM_ENDSESSION && wparam != FALSE) {
    return MaintenanceShutdownAction::kCommit;
  }
  return MaintenanceShutdownAction::kNone;
}

}  // namespace usque
