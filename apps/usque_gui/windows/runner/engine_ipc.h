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

EngineIpcResult ExchangeEngineFrame(const std::string& pipe_name,
                                    const std::vector<uint8_t>& request);

using EngineEventCallback = std::function<void(EngineIpcResult)>;

// Reconnects to the read-only event pipe until |active| becomes false. The
// callback is invoked for each complete length-prefixed protobuf frame, or once
// with a non-recoverable validation/access error.
void StreamEngineEvents(const std::string& pipe_name,
                        const std::shared_ptr<std::atomic_bool>& active,
                        EngineEventCallback callback);

#endif  // RUNNER_ENGINE_IPC_H_
