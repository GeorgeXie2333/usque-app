#include "engine_ipc.h"

#include <windows.h>

#include <algorithm>
#include <chrono>
#include <limits>
#include <string_view>
#include <thread>

namespace {

constexpr size_t kMaximumFrameBytes = 4 * 1024 * 1024;
constexpr std::string_view kPipePrefix =
    R"(\\.\pipe\io.github.georgexie2333.usque.engine.v1-ui-)";

class OwnedHandle {
 public:
  explicit OwnedHandle(HANDLE handle) : handle_(handle) {}
  ~OwnedHandle() {
    if (handle_ != INVALID_HANDLE_VALUE) {
      ::CloseHandle(handle_);
    }
  }
  HANDLE get() const { return handle_; }

 private:
  HANDLE handle_;
};

std::wstring Utf16FromUtf8(const std::string& value) {
  if (value.empty() ||
      value.size() > static_cast<size_t>(std::numeric_limits<int>::max())) {
    return {};
  }
  const int length = ::MultiByteToWideChar(
      CP_UTF8, MB_ERR_INVALID_CHARS, value.data(), static_cast<int>(value.size()),
      nullptr, 0);
  if (length <= 0) {
    return {};
  }
  std::wstring output(static_cast<size_t>(length), L'\0');
  if (::MultiByteToWideChar(CP_UTF8, MB_ERR_INVALID_CHARS, value.data(),
                            static_cast<int>(value.size()), output.data(),
                            length) != length) {
    return {};
  }
  return output;
}

std::string WindowsError(const char* operation) {
  return std::string(operation) + " failed with Win32 error " +
         std::to_string(::GetLastError());
}

bool WriteAll(HANDLE pipe, const uint8_t* data, size_t length,
              std::string* error) {
  size_t offset = 0;
  while (offset < length) {
    const DWORD chunk = static_cast<DWORD>(std::min<size_t>(
        length - offset, std::numeric_limits<DWORD>::max()));
    DWORD written = 0;
    if (!::WriteFile(pipe, data + offset, chunk, &written, nullptr) ||
        written == 0) {
      *error = WindowsError("WriteFile");
      return false;
    }
    offset += written;
  }
  return true;
}

bool ReadAll(HANDLE pipe, uint8_t* data, size_t length, std::string* error) {
  size_t offset = 0;
  while (offset < length) {
    const DWORD chunk = static_cast<DWORD>(std::min<size_t>(
        length - offset, std::numeric_limits<DWORD>::max()));
    DWORD read = 0;
    if (!::ReadFile(pipe, data + offset, chunk, &read, nullptr) || read == 0) {
      *error = WindowsError("ReadFile");
      return false;
    }
    offset += read;
  }
  return true;
}

bool ReadAllCancellable(HANDLE pipe, uint8_t* data, size_t length,
                        const std::shared_ptr<std::atomic_bool>& active,
                        std::string* error) {
  size_t offset = 0;
  while (offset < length && active->load()) {
    DWORD available = 0;
    if (!::PeekNamedPipe(pipe, nullptr, 0, nullptr, &available, nullptr)) {
      *error = WindowsError("PeekNamedPipe");
      return false;
    }
    if (available == 0) {
      std::this_thread::sleep_for(std::chrono::milliseconds(25));
      continue;
    }
    const DWORD chunk = static_cast<DWORD>(std::min<size_t>(
        std::min<size_t>(length - offset, available),
        std::numeric_limits<DWORD>::max()));
    DWORD read = 0;
    if (!::ReadFile(pipe, data + offset, chunk, &read, nullptr) || read == 0) {
      *error = WindowsError("ReadFile");
      return false;
    }
    offset += read;
  }
  return offset == length;
}

uint32_t BigEndianLength(const uint8_t* bytes) {
  return (static_cast<uint32_t>(bytes[0]) << 24) |
         (static_cast<uint32_t>(bytes[1]) << 16) |
         (static_cast<uint32_t>(bytes[2]) << 8) |
         static_cast<uint32_t>(bytes[3]);
}

bool IsRecoverablePipeError(DWORD error) {
  return error == ERROR_FILE_NOT_FOUND || error == ERROR_PIPE_BUSY ||
         error == ERROR_SEM_TIMEOUT || error == ERROR_BROKEN_PIPE ||
         error == ERROR_PIPE_NOT_CONNECTED || error == ERROR_NO_DATA;
}

}  // namespace

EngineIpcResult ExchangeEngineFrame(const std::string& pipe_name,
                                    const std::vector<uint8_t>& request) {
  EngineIpcResult result;
  if (pipe_name.size() < kPipePrefix.size() ||
      pipe_name.compare(0, kPipePrefix.size(), kPipePrefix) != 0) {
    result.error = "Named Pipe name is outside the Usque namespace";
    return result;
  }
  if (request.size() < 4 || request.size() > kMaximumFrameBytes + 4 ||
      static_cast<size_t>(BigEndianLength(request.data())) !=
          request.size() - 4) {
    result.error = "request frame length is invalid";
    return result;
  }
  const std::wstring pipe_name_wide = Utf16FromUtf8(pipe_name);
  if (pipe_name_wide.empty()) {
    result.error = "Named Pipe name is not valid UTF-8";
    return result;
  }
  if (!::WaitNamedPipeW(pipe_name_wide.c_str(), 750)) {
    result.error = WindowsError("WaitNamedPipeW");
    return result;
  }
  OwnedHandle pipe(::CreateFileW(pipe_name_wide.c_str(),
                                 GENERIC_READ | GENERIC_WRITE, 0, nullptr,
                                 OPEN_EXISTING, FILE_ATTRIBUTE_NORMAL, nullptr));
  if (pipe.get() == INVALID_HANDLE_VALUE) {
    result.error = WindowsError("CreateFileW");
    return result;
  }
  DWORD mode = PIPE_READMODE_BYTE;
  if (!::SetNamedPipeHandleState(pipe.get(), &mode, nullptr, nullptr)) {
    result.error = WindowsError("SetNamedPipeHandleState");
    return result;
  }
  if (!WriteAll(pipe.get(), request.data(), request.size(), &result.error)) {
    return result;
  }

  result.response.resize(4);
  if (!ReadAll(pipe.get(), result.response.data(), 4, &result.error)) {
    result.response.clear();
    return result;
  }
  const uint32_t payload_length = BigEndianLength(result.response.data());
  if (payload_length > kMaximumFrameBytes) {
    result.response.clear();
    result.error = "response frame exceeds 4 MiB";
    return result;
  }
  result.response.resize(static_cast<size_t>(payload_length) + 4);
  if (!ReadAll(pipe.get(), result.response.data() + 4, payload_length,
               &result.error)) {
    result.response.clear();
  }
  return result;
}

void StreamEngineEvents(const std::string& pipe_name,
                        const std::shared_ptr<std::atomic_bool>& active,
                        EngineEventCallback callback) {
  if (pipe_name.size() < kPipePrefix.size() + 8 ||
      pipe_name.compare(0, kPipePrefix.size(), kPipePrefix) != 0 ||
      pipe_name.compare(pipe_name.size() - 7, 7, ".events") != 0) {
    callback(EngineIpcResult{
        {}, "Named Pipe event name is outside the Usque namespace"});
    return;
  }
  const std::wstring pipe_name_wide = Utf16FromUtf8(pipe_name);
  if (pipe_name_wide.empty()) {
    callback(
        EngineIpcResult{{}, "Named Pipe event name is not valid UTF-8"});
    return;
  }

  while (active->load()) {
    if (!::WaitNamedPipeW(pipe_name_wide.c_str(), 250)) {
      const DWORD error = ::GetLastError();
      if (!IsRecoverablePipeError(error)) {
        callback(EngineIpcResult{
            {}, "WaitNamedPipeW failed with Win32 error " +
                    std::to_string(error)});
        return;
      }
      std::this_thread::sleep_for(std::chrono::milliseconds(75));
      continue;
    }

    OwnedHandle pipe(::CreateFileW(pipe_name_wide.c_str(), GENERIC_READ, 0,
                                   nullptr, OPEN_EXISTING,
                                   FILE_ATTRIBUTE_NORMAL, nullptr));
    if (pipe.get() == INVALID_HANDLE_VALUE) {
      const DWORD error = ::GetLastError();
      if (!IsRecoverablePipeError(error)) {
        callback(EngineIpcResult{
            {}, "CreateFileW failed with Win32 error " +
                    std::to_string(error)});
        return;
      }
      continue;
    }
    DWORD mode = PIPE_READMODE_BYTE;
    if (!::SetNamedPipeHandleState(pipe.get(), &mode, nullptr, nullptr)) {
      const DWORD error = ::GetLastError();
      if (!IsRecoverablePipeError(error)) {
        callback(EngineIpcResult{
            {}, "SetNamedPipeHandleState failed with Win32 error " +
                    std::to_string(error)});
        return;
      }
      continue;
    }

    while (active->load()) {
      EngineIpcResult event;
      event.response.resize(4);
      if (!ReadAllCancellable(pipe.get(), event.response.data(), 4, active,
                              &event.error)) {
        break;
      }
      const uint32_t payload_length = BigEndianLength(event.response.data());
      if (payload_length > kMaximumFrameBytes) {
        callback(EngineIpcResult{{}, "event frame exceeds 4 MiB"});
        return;
      }
      event.response.resize(static_cast<size_t>(payload_length) + 4);
      if (!ReadAllCancellable(pipe.get(), event.response.data() + 4,
                              payload_length, active, &event.error)) {
        break;
      }
      event.error.clear();
      callback(std::move(event));
    }

    if (active->load()) {
      std::this_thread::sleep_for(std::chrono::milliseconds(75));
    }
  }
}
