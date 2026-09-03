#include "maintenance_shutdown.h"

namespace usque {

MaintenanceShutdownAction ClassifyMaintenanceShutdownMessage(
    UINT message,
    WPARAM wparam,
    LPARAM /*lparam*/) noexcept {
  if (message == WM_QUERYENDSESSION) {
    return MaintenanceShutdownAction::kAllow;
  }
  if (message == WM_ENDSESSION && wparam != FALSE) {
    return MaintenanceShutdownAction::kCommit;
  }
  return MaintenanceShutdownAction::kNone;
}

}  // namespace usque
