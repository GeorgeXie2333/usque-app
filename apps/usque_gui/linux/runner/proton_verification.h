#ifndef USQUE_LINUX_PROTON_VERIFICATION_H_
#define USQUE_LINUX_PROTON_VERIFICATION_H_
#include <flutter_linux/flutter_linux.h>
#include <gtk/gtk.h>
bool usque_proton_verification_method(GtkWindow* parent, FlMethodCall* call);
void usque_proton_verification_cancel();
#endif
