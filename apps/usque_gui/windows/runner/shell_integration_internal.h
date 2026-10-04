#pragma once

#include "shell_integration.h"

namespace usque::shell::detail {

// Internal command-line dispatch seam. Tests supply a fake platform and COM
// state without touching current-user shell settings or initializing COM.
CommandResult ExecuteCommandWithComState(const Command& command, Platform& platform,
                                        bool com_available);

}  // namespace usque::shell::detail
