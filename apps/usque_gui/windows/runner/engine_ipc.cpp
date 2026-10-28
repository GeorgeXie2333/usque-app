#include "engine_ipc.h"

#include <windows.h>

#include <algorithm>
#include <chrono>
#include <limits>
#include <string_view>
#include <thread>

namespace {

constexpr size_t kMaximumFrameBytes = 4 * 1024 * 1024;
constexpr uint32_t kMaximumReadyTimeoutMs = 60 * 1000;
constexpr auto kMissingPipeRetryDelay = std::chrono::milliseconds(50);
constexpr auto kPipeWaitSlice = std::chrono::milliseconds(250);
constexpr std::string_view kPipePrefix =
    R"(\\.\pipe\io.github.georgexie2333.usque.engine.v1-ui-)";

class OwnedHandle {
 public:
  explicit OwnedHandle(HANDLE handle) : handle_(handle) {}
  ~OwnedHandle() {
    if (handle_ != INVALID_HANDLE_VALUE && handle_ != nullptr) {
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
                        const std::shared_ptr<EngineEventCancellation>&
                            cancellation,
                        std::string* error) {
  OwnedHandle completed(::CreateEventW(nullptr, TRUE, FALSE, nullptr));
  if (completed.get() == nullptr) {
    *error = WindowsError("CreateEventW");
    return false;
  }
  size_t offset = 0;
  while (offset < length && !cancellation->IsCancelled()) {
    OVERLAPPED operation{};
    operation.hEvent = completed.get();
    ::ResetEvent(operation.hEvent);
    const DWORD chunk = static_cast<DWORD>(std::min<size_t>(
        length - offset, std::numeric_limits<DWORD>::max()));
    DWORD read = 0;
    if (!::ReadFile(pipe, data + offset, chunk, nullptr, &operation)) {
      if (::GetLastError() != ERROR_IO_PENDING) {
        *error = WindowsError("ReadFile");
        return false;
      }
      const HANDLE waiting[]{cancellation->stop_event(), operation.hEvent};
      const DWORD ready = ::WaitForMultipleObjects(2, waiting, FALSE, INFINITE);
      if (ready != WAIT_OBJECT_0 + 1) {
        // Even ERROR_NOT_FOUND can mean completion won the cancellation race.
        // Drain it before either the buffer or OVERLAPPED leaves this scope.
        ::CancelIoEx(pipe, &operation);
        ::GetOverlappedResult(pipe, &operation, &read, TRUE);
        if (!cancellation->IsCancelled()) {
          *error = "Engine event read wait failed";
        }
        return false;
      }
    }
    if (!::GetOverlappedResult(pipe, &operation, &read, FALSE)) {
      *error = WindowsError("GetOverlappedResult");
      return false;
    }
    if (read == 0) {
      *error = "Engine event pipe reached EOF";
      return false;
    }
    if (cancellation->IsCancelled()) {
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

bool IsControlPipeName(const std::string& pipe_name) {
  return pipe_name.size() >= kPipePrefix.size() &&
         pipe_name.compare(0, kPipePrefix.size(), kPipePrefix) == 0;
}

}  // namespace

EngineEventCancellation::EngineEventCancellation()
    : stop_event_(::CreateEventW(nullptr, TRUE, FALSE, nullptr)) {}

EngineEventCancellation::~EngineEventCancellation() {
  if (stop_event_ != nullptr) ::CloseHandle(stop_event_);
}

void EngineEventCancellation::Cancel() {
  std::lock_guard<std::recursive_mutex> lock(callback_mutex_);
  cancelled_.store(true);
  if (stop_event_ != nullptr) ::SetEvent(stop_event_);
}

bool EngineEventCancellation::IsCancelled() const { return cancelled_.load(); }

void EngineEventCancellation::Deliver(EngineIpcResult result,
                                     const EngineEventCallback& callback) {
  std::lock_guard<std::recursive_mutex> lock(callback_mutex_);
  if (!IsCancelled()) callback(std::move(result));
}

std::string WaitForEnginePipe(const std::string& pipe_name,
                              uint32_t timeout_ms) {
  if (!IsControlPipeName(pipe_name)) {
    return "Named Pipe name is outside the Usque namespace";
  }
  if (timeout_ms == 0 || timeout_ms > kMaximumReadyTimeoutMs) {
    return "Named Pipe readiness timeout is invalid";
  }
  const std::wstring pipe_name_wide = Utf16FromUtf8(pipe_name);
  if (pipe_name_wide.empty()) {
    return "Named Pipe name is not valid UTF-8";
  }

  const auto deadline = std::chrono::steady_clock::now() +
                        std::chrono::milliseconds(timeout_ms);
  DWORD last_error = ERROR_FILE_NOT_FOUND;
  while (true) {
    const auto now = std::chrono::steady_clock::now();
    if (now >= deadline) {
      return "Usque Engine Named Pipe did not become ready before timeout "
             "(last Win32 error " +
             std::to_string(last_error) + ")";
    }
    const auto remaining =
        std::chrono::duration_cast<std::chrono::milliseconds>(deadline - now);
    const auto wait_slice = std::min(remaining, kPipeWaitSlice);
    const DWORD wait_ms = static_cast<DWORD>(
        std::max<int64_t>(1, static_cast<int64_t>(wait_slice.count())));
    if (::WaitNamedPipeW(pipe_name_wide.c_str(), wait_ms)) {
      return {};
    }

    last_error = ::GetLastError();
    if (!IsRecoverablePipeError(last_error)) {
      return "WaitNamedPipeW failed with Win32 error " +
             std::to_string(last_error);
    }
    // WaitNamedPipeW does not consume its timeout when no pipe instance exists.
    // Keep this bounded sleep to avoid a CPU spin during Engine cold start.
    if (last_error != ERROR_SEM_TIMEOUT) {
      const auto after_wait = std::chrono::steady_clock::now();
      if (after_wait < deadline) {
        const auto remaining_sleep =
            std::chrono::duration_cast<std::chrono::milliseconds>(deadline -
                                                                  after_wait);
        std::this_thread::sleep_for(
            std::min(kMissingPipeRetryDelay, remaining_sleep));
      }
    }
  }
}

EngineIpcResult ExchangeEngineFrame(const std::string& pipe_name,
                                    const std::vector<uint8_t>& request) {
  EngineIpcResult result;
  if (!IsControlPipeName(pipe_name)) {
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
  result.error = WaitForEnginePipe(pipe_name, 750);
  if (!result.error.empty()) {
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
                        const std::shared_ptr<EngineEventCancellation>&
                            cancellation,
                        EngineEventCallback callback) {
  const auto deliver = [&](EngineIpcResult event) {
    cancellation->Deliver(std::move(event), callback);
  };
  if (cancellation->IsCancelled()) return;
  if (cancellation->stop_event() == nullptr) {
    deliver(EngineIpcResult{{}, "Engine event cancellation event is unavailable"});
    return;
  }
  if (pipe_name.size() < kPipePrefix.size() + 8 ||
      pipe_name.compare(0, kPipePrefix.size(), kPipePrefix) != 0 ||
      pipe_name.compare(pipe_name.size() - 7, 7, ".events") != 0) {
    deliver(EngineIpcResult{
        {}, "Named Pipe event name is outside the Usque namespace"});
    return;
  }
  const std::wstring pipe_name_wide = Utf16FromUtf8(pipe_name);
  if (pipe_name_wide.empty()) {
    deliver(
        EngineIpcResult{{}, "Named Pipe event name is not valid UTF-8"});
    return;
  }

  while (!cancellation->IsCancelled()) {
    if (!::WaitNamedPipeW(pipe_name_wide.c_str(), 250)) {
      const DWORD error = ::GetLastError();
      if (!IsRecoverablePipeError(error)) {
        deliver(EngineIpcResult{
            {}, "WaitNamedPipeW failed with Win32 error " +
                    std::to_string(error)});
        return;
      }
      ::WaitForSingleObject(cancellation->stop_event(), 75);
      continue;
    }

    OwnedHandle pipe(::CreateFileW(
        pipe_name_wide.c_str(), GENERIC_READ, 0, nullptr, OPEN_EXISTING,
        FILE_ATTRIBUTE_NORMAL | FILE_FLAG_OVERLAPPED, nullptr));
    if (pipe.get() == INVALID_HANDLE_VALUE) {
      const DWORD error = ::GetLastError();
      if (!IsRecoverablePipeError(error)) {
        deliver(EngineIpcResult{
            {}, "CreateFileW failed with Win32 error " +
                    std::to_string(error)});
        return;
      }
      continue;
    }
    // CreateFile client handles already start in byte-read mode. Calling
    // SetNamedPipeHandleState here would require FILE_WRITE_ATTRIBUTES in
    // addition to GENERIC_READ and fails with ERROR_ACCESS_DENIED otherwise.

    while (!cancellation->IsCancelled()) {
      EngineIpcResult event;
      event.response.resize(4);
      if (!ReadAllCancellable(pipe.get(), event.response.data(), 4, cancellation,
                              &event.error)) {
        if (!cancellation->IsCancelled()) {
          deliver(EngineIpcResult{
              {}, "Engine event pipe closed during the frame header"});
        }
        return;
      }
      const uint32_t payload_length = BigEndianLength(event.response.data());
      if (payload_length > kMaximumFrameBytes) {
        deliver(EngineIpcResult{{}, "event frame exceeds 4 MiB"});
        return;
      }
      event.response.resize(static_cast<size_t>(payload_length) + 4);
      if (!ReadAllCancellable(pipe.get(), event.response.data() + 4,
                              payload_length, cancellation, &event.error)) {
        if (!cancellation->IsCancelled()) {
          deliver(EngineIpcResult{
              {}, "Engine event pipe closed during the frame body"});
        }
        return;
      }
      event.error.clear();
      deliver(std::move(event));
    }

    if (!cancellation->IsCancelled()) {
      ::WaitForSingleObject(cancellation->stop_event(), 75);
    }
  }
}
