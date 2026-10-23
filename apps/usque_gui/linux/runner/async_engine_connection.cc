#include "async_engine_connection.h"
#include "unix_engine_socket.h"
#include <gio/gio.h>
#include <unistd.h>
#include <utility>

struct UsqueAsyncEngineConnection::State {
  Connector connector;
  uint64_t generation = 0;
};
UsqueAsyncEngineConnection::UsqueAsyncEngineConnection(Connector connector)
    : state_(std::make_shared<State>()) {
  state_->connector = connector ? std::move(connector) : [](const std::string& path, pid_t pid) {
    return usque_connect_engine_socket(path, pid);
  };
}
UsqueAsyncEngineConnection::~UsqueAsyncEngineConnection() { Cancel(); }
void UsqueAsyncEngineConnection::Cancel() { ++state_->generation; }
void UsqueAsyncEngineConnection::Start(std::string path, pid_t pid,
                                      std::function<void(int)> completed) {
  struct Job {
    std::shared_ptr<State> state;
    std::string path;
    pid_t pid;
    uint64_t generation;
    std::function<void(int)> completed;
    int fd = -1;
    ~Job() { if (fd >= 0) close(fd); }
  };
  auto* job = new Job{state_, std::move(path), pid, ++state_->generation, std::move(completed)};
  g_autoptr(GTask) task = g_task_new(nullptr, nullptr,
      [](GObject*, GAsyncResult* result, gpointer) {
        auto* job = static_cast<Job*>(g_task_get_task_data(G_TASK(result)));
        g_task_propagate_boolean(G_TASK(result), nullptr);
        if (job->state->generation == job->generation)
          job->completed(std::exchange(job->fd, -1));
      }, nullptr);
  g_task_set_task_data(task, job, [](gpointer p) { delete static_cast<Job*>(p); });
  g_task_run_in_thread(task, [](GTask* task, gpointer, gpointer p, GCancellable*) {
    auto* job = static_cast<Job*>(p);
    job->fd = job->state->connector(job->path, job->pid);
    g_task_return_boolean(task, TRUE);
  });
}
