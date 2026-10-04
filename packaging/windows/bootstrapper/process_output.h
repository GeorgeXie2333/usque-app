#pragma once

#include <windows.h>

#include <array>
#include <string>

namespace usque::setup {
struct ChildResult { DWORD code = ERROR_GEN_FAILURE; std::string output; };

// An exit signal does not consume the child's final pipe write. Drain after
// observing process exit, otherwise fast headless helpers intermittently lose
// their only JSON response. Caller owns handles and keeps stderr/stdout bounded.
inline ChildResult ReadChildOutput(HANDLE child, HANDLE read_pipe) {
  ChildResult result;
  const ULONGLONG deadline = GetTickCount64() + 30000;
  bool exited = false;
  for (;;) {
    DWORD available = 0;
    if (PeekNamedPipe(read_pipe, nullptr, 0, nullptr, &available, nullptr) && available) {
      std::array<char, 2048> buffer{};
      DWORD count = 0;
      if (ReadFile(read_pipe, buffer.data(), static_cast<DWORD>(buffer.size()), &count, nullptr)) {
        if (result.output.size() + count > 16384) { result.code = ERROR_INVALID_DATA; break; }
        result.output.append(buffer.data(), count);
      }
      continue;
    }
    if (exited) break;
    if (WaitForSingleObject(child, 20) == WAIT_OBJECT_0) {
      if (!GetExitCodeProcess(child, &result.code)) result.code = GetLastError();
      exited = true;
      continue;  // Re-check and drain after the exit signal.
    }
    if (GetTickCount64() >= deadline) { result.code = WAIT_TIMEOUT; break; }
  }
  return result;
}
}  // namespace usque::setup
