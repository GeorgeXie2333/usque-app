#ifndef USQUE_LINUX_ENGINE_BRIDGE_H_
#define USQUE_LINUX_ENGINE_BRIDGE_H_

#include <flutter_linux/flutter_linux.h>

// Returns true when the bridge owns the asynchronous method response.
bool usque_engine_method(FlMethodCall* call);
void usque_register_engine_events(FlBinaryMessenger* messenger);

#endif
