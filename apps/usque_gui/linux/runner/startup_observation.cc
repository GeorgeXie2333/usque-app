#include "startup_observation.h"
#include <algorithm>
#include <array>
namespace {
gint64 started = 0, last_tick = 0, max_gap = 0;
std::array<gint64, 2> stages = {-1, -1};
std::array<int, 2> statuses = {1, 1};
gint64 elapsed() { return (g_get_monotonic_time() - started) / 1000; }
}
void usque_startup_begin() {
  if (started != 0) return;
  started = g_get_monotonic_time();
  g_timeout_add(100, [](gpointer) -> gboolean {
    const auto now = elapsed();
    max_gap = std::max(max_gap, std::min<gint64>(now - last_tick, 60000));
    last_tick = now;
    return now < 60000 ? G_SOURCE_CONTINUE : G_SOURCE_REMOVE;
  }, nullptr);
}
void usque_startup_mark(int stage, bool failed) {
  if (started && stage >= 1 && stage <= 2 && elapsed() <= 60000 && stages[stage - 1] < 0) {
    stages[stage - 1] = elapsed();
    statuses[stage - 1] = failed ? 3 : 2;
  }
}
FlValue* usque_startup_snapshot() {
  auto* result = fl_value_new_map();
  if (!started) return result;
  if (last_tick > 0) fl_value_set_string_take(result, "max_gap_ms", fl_value_new_int(max_gap));
  auto* values = fl_value_new_list();
  for (size_t i = 0; i < stages.size(); ++i) {
    auto* stage = fl_value_new_map();
    const bool timeout = stages[i] < 0 && elapsed() >= 60000;
    fl_value_set_string_take(stage, "stage", fl_value_new_int(i + 1));
    fl_value_set_string_take(stage, "status", fl_value_new_int(stages[i] >= 0 ? statuses[i] : timeout ? 4 : 1));
    if (stages[i] >= 0 || timeout)
      fl_value_set_string_take(stage, "elapsed_ms", fl_value_new_int(timeout ? 60000 : stages[i]));
    fl_value_append_take(values, stage);
  }
  fl_value_set_string_take(result, "stages", values);
  return result;
}
