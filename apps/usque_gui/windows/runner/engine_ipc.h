#ifndef RUNNER_ENGINE_IPC_H_
#define RUNNER_ENGINE_IPC_H_

#include <atomic>
#include <cstdint>
#include <functional>
#include <memory>
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

// Reconnects to the read-only event pipe until |active| becomes false. The
// callback is invoked only for complete length-prefixed protobuf frames.
// Recoverable pipe failures reconnect internally; fatal validation or Win32
// errors are returned through the callback so Dart can fall back to polling.
void StreamEngineEvents(const std::string& pipe_name,
                        const std::shared_ptr<std::atomic_bool>& active,
                        EngineEventCallback callback);

#endif  // RUNNER_ENGINE_IPC_H_
