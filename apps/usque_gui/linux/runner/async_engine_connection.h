#ifndef USQUE_ASYNC_ENGINE_CONNECTION_H_
#define USQUE_ASYNC_ENGINE_CONNECTION_H_
#include <functional>
#include <memory>
#include <string>
#include <sys/types.h>

// Main-context API. Connect/authentication runs on a worker. A successful
// callback owns the FD; cancellation or destruction closes an obsolete result.
class UsqueAsyncEngineConnection {
 public:
  using Connector = std::function<int(const std::string&, pid_t)>;
  explicit UsqueAsyncEngineConnection(Connector connector = {});
  ~UsqueAsyncEngineConnection();
  void Start(std::string path, pid_t pid, std::function<void(int)> completed);
  void Cancel();
 private:
  struct State;
  std::shared_ptr<State> state_;
};
#endif
