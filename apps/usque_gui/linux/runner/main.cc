#include "my_application.h"
#include "startup_observation.h"

int main(int argc, char** argv) {
  usque_startup_begin();
  g_autoptr(MyApplication) app = my_application_new();
  return g_application_run(G_APPLICATION(app), argc, argv);
}
