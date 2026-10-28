#ifndef RUNNER_ENGINE_IPC_H_
#define RUNNER_ENGINE_IPC_H_

#include <atomic>
#include <cstdint>
#include <functional>
#include <memory>
#include <mutex>
#include <string>
#include <vector>

struct EngineIpcResult {
  std::vector<uint8_t> response;
  std::string error;
};

// Waits until the Engine has created a connectable control-pipe instance. The
// timeout is an overall deadline: ERROR_FILE_NOT_FOUND is retried explicitly
// because WaitNamedPipeW otherwise returns immediately when no instance exists.
// Returns an empty string on success or a user-facing diagnostic on failure.
std::string WaitForEnginePipe(const std::string& pipe_name,
                              uint32_t timeout_ms);

EngineIpcResult ExchangeEngineFrame(const std::string& pipe_name,
                                     const std::vector<uint8_t>& request);

using EngineEventCallback = std::function<void(EngineIpcResult)>;

// Owns the wakeup used by overlapped event reads. Cancellation also serializes
// callback delivery, so no callback can begin after Cancel returns.
class EngineEventCancellation {
 public:
  EngineEventCancellation();
  ~EngineEventCancellation();
  EngineEventCancellation(const EngineEventCancellation&) = delete;
  EngineEventCancellation& operator=(const EngineEventCancellation&) = delete;

  void Cancel();
  bool IsCancelled() const;
  void* stop_event() const { return stop_event_; }
  void Deliver(EngineIpcResult result, const EngineEventCallback& callback);

 private:
  void* stop_event_;
  std::atomic_bool cancelled_{false};
  std::recursive_mutex callback_mutex_;
};

// Reconnects to the read-only event pipe until |cancellation| is cancelled. The
// callback carries complete frames or a terminal stream error. Once connected,
// pipe EOF and truncated frames are reported so Dart can fall back to status
// polling even when quality telemetry is paused. Intentional cancellation is quiet.
void StreamEngineEvents(const std::string& pipe_name,
                        const std::shared_ptr<EngineEventCancellation>&
                            cancellation,
                        EngineEventCallback callback);

#endif  // RUNNER_ENGINE_IPC_H_
