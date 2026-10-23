#include "linux_preferences_store.h"
#include "preferences_writer.h"
#include <gio/gio.h>
#include <fcntl.h>
#include <sys/stat.h>
#include <unistd.h>
#include <cerrno>
#include <utility>
#include <vector>

namespace {
constexpr size_t kMaximumPreferences = 8 * 1024 * 1024;
struct Loaded {
  bool ok = false;
  std::string path;
  std::string contents;
};
Loaded Load(const std::string& directory, const std::string& id,
            const std::string& executable) {
  Loaded result;
  auto parent = directory + "/" + id;
  struct stat metadata {};
  if (stat(parent.c_str(), &metadata) != 0) {
    if (errno != ENOENT) return result;
    auto legacy = directory + "/" + executable;
    if (stat(legacy.c_str(), &metadata) == 0) {
      if (!S_ISDIR(metadata.st_mode)) return result;
      parent = std::move(legacy);
    } else if (errno != ENOENT) return result;
  } else if (!S_ISDIR(metadata.st_mode)) return result;
  result.path = parent + "/shared_preferences.json";
  const int fd = open(result.path.c_str(), O_RDONLY | O_CLOEXEC | O_NONBLOCK | O_NOFOLLOW);
  if (fd < 0) {
    if (errno == ENOENT) { result.ok = true; result.contents = "{}"; }
    return result;
  }
  bool ok = fstat(fd, &metadata) == 0 && S_ISREG(metadata.st_mode) &&
      metadata.st_size >= 0 && static_cast<uint64_t>(metadata.st_size) <= kMaximumPreferences;
  char buffer[8192];
  while (ok) {
    const auto count = read(fd, buffer, sizeof(buffer));
    if (count < 0 && errno == EINTR) continue;
    if (count < 0) { ok = false; break; }
    if (count == 0) break;
    result.contents.append(buffer, count);
    if (result.contents.size() > kMaximumPreferences) ok = false;
  }
  close(fd);
  result.ok = ok && g_utf8_validate(result.contents.data(), result.contents.size(), nullptr);
  // An existing empty/truncated file is not a new installation.
  if (result.contents.empty()) result.ok = false;
  return result;
}
}  // namespace

struct UsqueLinuxPreferencesStore::State {
  std::string directory, id, executable, path;
  std::unique_ptr<UsquePreferencesWriter> writer;
  std::vector<ReadCompletion> readers;
  bool loading = false;
  std::function<void()> before_read;
};
UsqueLinuxPreferencesStore::UsqueLinuxPreferencesStore(std::string directory,
    std::string id, std::string executable, std::function<void()> before_read) : state_(std::make_shared<State>()) {
  state_->directory = std::move(directory);
  state_->id = std::move(id);
  state_->executable = std::move(executable);
  state_->before_read = std::move(before_read);
}
UsqueLinuxPreferencesStore::~UsqueLinuxPreferencesStore() = default;
void UsqueLinuxPreferencesStore::Read(ReadCompletion completion) {
  auto state = state_;
  state->readers.push_back(std::move(completion));
  if (state->loading) return;
  state->loading = true;
  auto* owner = new std::shared_ptr<State>(state);
  g_autoptr(GTask) task = g_task_new(nullptr, nullptr,
      [](GObject*, GAsyncResult* response, gpointer) {
        auto state = *static_cast<std::shared_ptr<State>*>(g_task_get_task_data(G_TASK(response)));
        std::unique_ptr<Loaded> loaded(static_cast<Loaded*>(g_task_propagate_pointer(G_TASK(response), nullptr)));
        state->loading = false;
        if (loaded->ok && !state->writer) {
          state->path = loaded->path;
          state->writer = std::make_unique<UsquePreferencesWriter>(state->path);
        }
        const bool ok = loaded->ok && loaded->path == state->path;
        auto callbacks = std::move(state->readers);
        for (auto& callback : callbacks) callback(ok, ok ? loaded->contents : "");
      }, nullptr);
  g_task_set_task_data(task, owner, [](gpointer p) { delete static_cast<std::shared_ptr<State>*>(p); });
  g_task_run_in_thread(task, [](GTask* task, gpointer, gpointer p, GCancellable*) {
    auto state = *static_cast<std::shared_ptr<State>*>(p);
    if (state->before_read) state->before_read();
    auto* result = new Loaded(Load(state->directory, state->id, state->executable));
    g_task_return_pointer(task, result, [](gpointer p) { delete static_cast<Loaded*>(p); });
  });
}
void UsqueLinuxPreferencesStore::Write(std::string contents, std::function<void(bool)> completion) {
  if (!state_->writer || contents.size() > kMaximumPreferences ||
      !g_utf8_validate(contents.data(), contents.size(), nullptr)) {
    completion(false); return;
  }
  state_->writer->Save(std::move(contents), std::move(completion));
}
void UsqueLinuxPreferencesStore::Flush(std::function<void(bool)> completion) {
  if (state_->writer) state_->writer->Flush(std::move(completion));
  else completion(true);
}
