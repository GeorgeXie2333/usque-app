#include "async_engine_connection.h"
#include "linux_preferences_store.h"
#include <gio/gio.h>
#include <glib/gstdio.h>
#include <fcntl.h>
#include <sys/stat.h>
#include <unistd.h>
#include <atomic>
#include <cstdio>
#include <cstdlib>
#include <memory>
#include <string>

namespace {
void Check(bool ok, const char* message) {
  if (!ok) { fprintf(stderr, "FAIL: %s\n", message); exit(1); }
}
template<typename Predicate> void Until(Predicate done) {
  auto deadline = g_get_monotonic_time() + 5 * G_TIME_SPAN_SECOND;
  while (!done()) {
    Check(g_get_monotonic_time() < deadline, "fixture deadline");
    while (g_main_context_iteration(nullptr, FALSE)) {}
    g_usleep(1000);
  }
}
void Preferences() {
  g_autofree gchar* directory = g_dir_make_tmp("usque-startup-io-XXXXXX", nullptr);
  Check(directory != nullptr, "private test directory");
  std::string legacy = std::string(directory) + "/usque";
  Check(g_mkdir(legacy.c_str(), 0700) == 0, "legacy directory");
  const auto path = legacy + "/shared_preferences.json";
  Check(g_file_set_contents(path.c_str(), "{\"flutter.theme\":\"dark\"}", -1, nullptr), "legacy preferences");
  GThread* main = g_thread_self();
  bool loaded = false;
  int ticks = 0;
  const guint timer = g_timeout_add(10, [](gpointer p) -> gboolean {
    ++*static_cast<int*>(p); return G_SOURCE_CONTINUE;
  }, &ticks);
  UsqueLinuxPreferencesStore store(directory, "application.id", "usque", [main] {
    Check(g_thread_self() != main, "preference read executes off main thread");
    g_usleep(2 * G_TIME_SPAN_SECOND);
  });
  store.Read([&](bool ok, const std::string& value) {
    Check(ok && value == "{\"flutter.theme\":\"dark\"}", "legacy contents retained");
    Check(g_thread_self() == main, "read completion on main context"); loaded = true;
  });
  Until([&] { return loaded; });
  g_source_remove(timer);
  Check(ticks >= 10, "main context continues during two-second storage read");
  bool saved = false, flushed = false;
  store.Write("{\"flutter.theme\":\"light\"}", [&](bool ok) { Check(ok, "save ack"); saved = true; });
  store.Flush([&](bool ok) { Check(ok, "flush ack"); flushed = true; });
  Until([&] { return flushed; });
  Check(saved, "flush follows write acknowledgement");
  struct stat statbuf{};
  Check(stat(path.c_str(), &statbuf) == 0 && (statbuf.st_mode & 0777) == 0600, "private file mode");
  g_autofree gchar* contents = nullptr;
  Check(g_file_get_contents(path.c_str(), &contents, nullptr, nullptr) &&
      std::string(contents) == "{\"flutter.theme\":\"light\"}", "atomic update of original legacy file");
  Check(!g_file_test((std::string(directory) + "/application.id").c_str(), G_FILE_TEST_EXISTS), "no migration to a new empty directory");
  g_unlink(path.c_str()); g_rmdir(legacy.c_str()); g_rmdir(directory);
}
void Connections() {
  GThread* main = g_thread_self();
  std::atomic<int> issued{-1};
  bool completed = false;
  int ticks = 0;
  const guint timer = g_timeout_add(10, [](gpointer p) -> gboolean {
    ++*static_cast<int*>(p); return G_SOURCE_CONTINUE;
  }, &ticks);
  auto connector = std::make_unique<UsqueAsyncEngineConnection>([&](const std::string&, pid_t) {
    Check(g_thread_self() != main, "socket connect executes off main thread");
    g_usleep(2 * G_TIME_SPAN_SECOND);
    int fd = open("/dev/null", O_RDONLY | O_CLOEXEC); issued.store(fd); return fd;
  });
  connector->Start("synthetic", getpid(), [&](int fd) { close(fd); completed = true; });
  connector->Cancel();
  Until([&] { return issued.load() >= 0 && fcntl(issued.load(), F_GETFD) < 0; });
  g_source_remove(timer);
  Check(!completed && ticks >= 10, "cancelled connection closes FD without blocking main context");
  issued.store(-1);
  connector->Start("synthetic", getpid(), [&](int fd) { close(fd); completed = true; });
  connector.reset();
  Until([&] { return issued.load() >= 0 && fcntl(issued.load(), F_GETFD) < 0; });
  Check(!completed, "destroyed owner cannot publish a late connection");
  std::atomic<int> old_fd{-1};
  UsqueAsyncEngineConnection overlap([&](const std::string& path, pid_t) {
    if (path == "old") g_usleep(50000);
    int fd = open("/dev/null", O_RDONLY | O_CLOEXEC);
    if (path == "old") old_fd.store(fd);
    return fd;
  });
  bool old = false, newest = false;
  overlap.Start("old", getpid(), [&](int fd) { close(fd); old = true; });
  overlap.Start("new", getpid(), [&](int fd) { close(fd); newest = true; });
  Until([&] { return newest; });
  Until([&] { return old_fd.load() >= 0 && fcntl(old_fd.load(), F_GETFD) < 0; });
  Check(!old, "latest connection wins");
}
}
int main() { Preferences(); Connections(); puts("startup I/O fixtures passed"); }
