#include "preferences_writer.h"

#include <gio/gio.h>
#include <glib/gstdio.h>

#include <optional>
#include <utility>
#include <vector>

struct UsquePreferencesWriter::State : std::enable_shared_from_this<State> {
  Write write;
  unsigned debounce_ms;
  guint timer = 0;
  bool active = false;
  bool urgent = false;
  std::optional<std::string> pending;
  std::optional<std::string> requested;
  std::optional<std::string> saved;
  std::vector<Completion> pending_callbacks;
  std::vector<Completion> active_callbacks;
  std::vector<Completion> flush_callbacks;

  struct Job {
    std::shared_ptr<State> state;
    std::string contents;
  };

  void CancelTimer() {
    if (timer != 0) {
      g_source_remove(timer);
      timer = 0;
    }
  }

  void Schedule() {
    if (active || !pending) return;
    CancelTimer();
    if (urgent || !flush_callbacks.empty()) {
      Start();
      return;
    }
    auto* owner = new std::shared_ptr<State>(shared_from_this());
    timer = g_timeout_add_full(G_PRIORITY_DEFAULT, debounce_ms,
        [](gpointer data) -> gboolean {
          auto state = *static_cast<std::shared_ptr<State>*>(data);
          state->timer = 0;
          state->Start();
          return G_SOURCE_REMOVE;
        }, owner, [](gpointer data) {
          delete static_cast<std::shared_ptr<State>*>(data);
        });
  }

  void Start() {
    active = true;
    urgent = false;
    active_callbacks = std::move(pending_callbacks);
    auto* job = new Job{shared_from_this(), std::move(*pending)};
    pending.reset();
    g_autoptr(GTask) task = g_task_new(nullptr, nullptr,
        [](GObject*, GAsyncResult* result, gpointer) {
          auto* job = static_cast<Job*>(g_task_get_task_data(G_TASK(result)));
          auto state = job->state;
          const bool ok = g_task_propagate_boolean(G_TASK(result), nullptr);
          if (ok) state->saved = job->contents;
          auto callbacks = std::move(state->active_callbacks);
          state->active = false;
          // Publish the completed write before callbacks can queue newer work.
          for (auto& callback : callbacks) callback(ok);
          if (!state->active && state->pending) state->Schedule();
          if (!state->active && !state->pending) {
            auto flushed = std::move(state->flush_callbacks);
            const bool current = state->saved == state->requested;
            for (auto& callback : flushed) callback(current);
          }
        }, nullptr);
    g_task_set_task_data(task, job, [](gpointer data) {
      delete static_cast<Job*>(data);
    });
    g_task_run_in_thread(task, [](GTask* task, gpointer, gpointer data, GCancellable*) {
      auto* job = static_cast<Job*>(data);
      g_task_return_boolean(task, job->state->write(job->contents));
    });
  }
};

UsquePreferencesWriter::UsquePreferencesWriter(std::string path, Write write,
                                               unsigned debounce_ms)
    : state_(std::make_shared<State>()) {
  state_->debounce_ms = debounce_ms;
  state_->write = write ? std::move(write) : [path = std::move(path)](const std::string& contents) {
    g_autofree gchar* directory = g_path_get_dirname(path.c_str());
    return g_mkdir_with_parents(directory, 0700) == 0 &&
        g_file_set_contents_full(path.c_str(), contents.data(), contents.size(),
            static_cast<GFileSetContentsFlags>(G_FILE_SET_CONTENTS_CONSISTENT |
                                               G_FILE_SET_CONTENTS_DURABLE),
            0600, nullptr);
  };
}

UsquePreferencesWriter::~UsquePreferencesWriter() {
  // Jobs and timers retain their own state, never a Shell/GTK pointer.
  Flush([](bool) {});
}

void UsquePreferencesWriter::Save(std::string contents, Completion completion) {
  auto state = state_;
  if (state->requested == contents) {
    if (state->pending) {
      if (!completion) return;
      state->pending_callbacks.push_back(std::move(completion));
    } else if (state->active) {
      if (completion) state->active_callbacks.push_back(std::move(completion));
      return;
    } else if (state->saved == contents) {
      if (completion) completion(true);
      return;
    } else {
      state->pending = std::move(contents); // Retry a failed identical write.
      if (completion) state->pending_callbacks.push_back(std::move(completion));
    }
  } else {
    state->requested = contents;
    state->pending = std::move(contents);
    if (completion) state->pending_callbacks.push_back(std::move(completion));
  }
  // Explicit preference changes receive an acknowledgement after durable I/O.
  if (!state->pending_callbacks.empty()) state->urgent = true;
  state->Schedule();
}

void UsquePreferencesWriter::Flush(Completion completion) {
  auto state = state_;
  state->CancelTimer();
  if (!state->active && !state->pending) {
    completion(state->saved == state->requested);
    return;
  }
  state->flush_callbacks.push_back(std::move(completion));
  state->Schedule();
}
