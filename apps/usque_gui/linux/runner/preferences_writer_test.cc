#include "preferences_writer.h"

#include <gio/gio.h>
#include <glib/gstdio.h>
#include <sys/stat.h>

#include <atomic>
#include <condition_variable>
#include <cstdio>
#include <cstdlib>
#include <mutex>
#include <string>
#include <vector>

namespace {
void Check(bool condition, const char* message) {
  if (!condition) {
    fprintf(stderr, "FAIL: %s\n", message);
    exit(1);
  }
}

template <typename Predicate>
void Until(Predicate done) {
  const gint64 deadline = g_get_monotonic_time() + 5 * G_TIME_SPAN_SECOND;
  while (!done()) {
    Check(g_get_monotonic_time() < deadline, "main-loop fixture deadline");
    while (g_main_context_iteration(nullptr, FALSE)) {}
    g_usleep(1000);
  }
}

void BurstAndDuplicateEvents() {
  std::vector<std::string> writes;
  UsquePreferencesWriter writer("unused", [&](const std::string& bytes) {
    writes.push_back(bytes);
    return true;
  });
  for (int i = 0; i < 100; ++i) writer.Save(std::to_string(i));
  bool flushed = false;
  writer.Flush([&](bool ok) { Check(ok, "burst saved"); flushed = true; });
  Until([&] { return flushed; });
  Check(writes == std::vector<std::string>{"99"}, "coalesce resize burst to latest state");
  writer.Save("99");
  writer.Flush([&](bool ok) { Check(ok, "identical snapshot remains saved"); });
  Check(writes.size() == 1, "unchanged window events do not write again");
}

void SlowDiskAndExit() {
  std::atomic<bool> entered{false};
  std::mutex mutex;
  std::condition_variable released;
  bool release = false;
  std::vector<std::string> writes;
  GThread* owner = g_thread_self();
  UsquePreferencesWriter writer("unused", [&](const std::string& bytes) {
    Check(g_thread_self() != owner, "disk I/O stays off the GTK thread");
    if (bytes == "first") {
      entered.store(true);
      std::unique_lock<std::mutex> lock(mutex);
      released.wait(lock, [&] { return release; });
    }
    writes.push_back(bytes);
    return true;
  });
  bool acknowledged = false;
  writer.Save("first", [&](bool ok) {
    Check(ok && g_thread_self() == owner, "durable acknowledgement on main thread");
    acknowledged = true;
  });
  Until([&] { return entered.load(); });
  bool responsive = false;
  g_idle_add([](gpointer data) -> gboolean {
    *static_cast<bool*>(data) = true;
    return G_SOURCE_REMOVE;
  }, &responsive);
  Until([&] { return responsive; });
  Check(!acknowledged, "slow disk must not acknowledge an unfinished write");
  writer.Save("intermediate");
  writer.Save("latest");
  bool flushed = false;
  writer.Flush([&](bool ok) { Check(ok, "exit drains latest state"); flushed = true; });
  Check(!flushed, "exit waits asynchronously for the active and pending writes");
  {
    std::lock_guard<std::mutex> lock(mutex);
    release = true;
  }
  released.notify_one();
  Until([&] { return flushed; });
  Check(acknowledged, "explicit preference acknowledged");
  Check(writes == std::vector<std::string>({"first", "latest"}), "single writer preserves order");
}

void FailureAndOwnerLifetime() {
  int attempts = 0;
  UsquePreferencesWriter writer("unused", [&](const std::string&) { return ++attempts > 1; });
  bool replied = false;
  writer.Save("same", [&](bool ok) { Check(!ok, "save failure reaches caller"); replied = true; });
  Until([&] { return replied; });
  replied = false;
  writer.Save("same", [&](bool ok) { Check(ok, "identical failed snapshot can retry"); replied = true; });
  Until([&] { return replied; });
  Check(attempts == 2, "one failed write and one retry");
  std::atomic<bool> saved{false};
  {
    UsquePreferencesWriter temporary("unused", [&](const std::string& bytes) {
      Check(bytes == "last", "owned immutable snapshot survives destruction");
      saved.store(true);
      return true;
    });
    temporary.Save("last");
  }
  Until([&] { return saved.load(); });
  // Drain the completion too, before the referenced fixture state goes away.
  while (g_main_context_iteration(nullptr, FALSE)) {}
}

void RealPrivateFile() {
  g_autofree gchar* directory = g_dir_make_tmp("usque-preferences-test-XXXXXX", nullptr);
  Check(directory != nullptr, "create private temporary directory");
  const std::string path = std::string(directory) + "/nested/desktop.ini";
  {
    UsquePreferencesWriter writer(path);
    bool done = false;
    writer.Save("[Window]\nwidth=1280\n", [&](bool ok) { Check(ok, "real durable save"); done = true; });
    Until([&] { return done; });
  }
  struct stat metadata {};
  Check(stat(path.c_str(), &metadata) == 0 && (metadata.st_mode & 0777) == 0600,
        "preferences remain private");
  g_autofree gchar* contents = nullptr;
  Check(g_file_get_contents(path.c_str(), &contents, nullptr, nullptr) &&
        std::string(contents) == "[Window]\nwidth=1280\n", "complete serialized contents");
  Check(g_unlink(path.c_str()) == 0, "remove fixture file");
  Check(g_rmdir((std::string(directory) + "/nested").c_str()) == 0, "remove nested fixture directory");
  Check(g_rmdir(directory) == 0, "remove fixture directory");
}
}  // namespace

int main() {
  BurstAndDuplicateEvents();
  SlowDiskAndExit();
  FailureAndOwnerLifetime();
  RealPrivateFile();
  puts("Linux asynchronous preferences tests passed.");
}
