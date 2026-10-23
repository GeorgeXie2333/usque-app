#ifndef USQUE_LINUX_STARTUP_OBSERVATION_H_
#define USQUE_LINUX_STARTUP_OBSERVATION_H_
#include <flutter_linux/flutter_linux.h>
void usque_startup_begin();
void usque_startup_mark(int stage, bool failed = false);
FlValue* usque_startup_snapshot();
#endif
