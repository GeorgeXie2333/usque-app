#include "shell_integration.h"

#include <windows.h>
#include <propkey.h>
#include <propvarutil.h>
#include <shlobj.h>
#include <shlwapi.h>
#include <wrl/client.h>

#include <algorithm>
#include <cstdint>
#include <utility>

namespace usque::shell {
namespace {

using Microsoft::WRL::ComPtr;
constexpr wchar_t kAppId[] = L"io.github.georgexie2333.usque";
constexpr wchar_t kRunKey[] =
    L"Software\\Microsoft\\Windows\\CurrentVersion\\Run";
constexpr wchar_t kRunValue[] = L"Usque";
constexpr DWORD kMaxLinkBytes = 64 * 1024;

class FileHandle {
 public:
  explicit FileHandle(HANDLE handle) : handle_(handle) {}
  ~FileHandle() {
    if (handle_ != INVALID_HANDLE_VALUE && handle_ != nullptr) {
      ::CloseHandle(handle_);
    }
  }
  FileHandle(const FileHandle&) = delete;
  FileHandle& operator=(const FileHandle&) = delete;
  HANDLE get() const { return handle_; }
 private:
  HANDLE handle_;
};

bool Failed(const ItemResult& result) {
  return result.status == ItemStatus::kConflict ||
         result.status == ItemStatus::kError;
}

ItemResult FromState(EntryState state, bool startup, bool keep = false) {
  switch (state) {
    case EntryState::kOwned:
    case EntryState::kSameTarget:
      return {keep ? ItemStatus::kKept
                   : startup ? ItemStatus::kEnabled : ItemStatus::kPresent,
              true};
    case EntryState::kAbsent:
      return {keep ? ItemStatus::kKept
                   : startup ? ItemStatus::kDisabled : ItemStatus::kAbsent,
              false};
    case EntryState::kForeign:
      return {keep ? ItemStatus::kKept : ItemStatus::kConflict, std::nullopt};
    case EntryState::kError:
      return {ItemStatus::kError, std::nullopt};
  }
  return {ItemStatus::kError, std::nullopt};
}

const char* StatusName(ItemStatus status) {
  switch (status) {
    case ItemStatus::kNotRequested: return "not_requested";
    case ItemStatus::kCreated: return "created";
    case ItemStatus::kPresent: return "present";
    case ItemStatus::kAbsent: return "absent";
    case ItemStatus::kEnabled: return "enabled";
    case ItemStatus::kDisabled: return "disabled";
    case ItemStatus::kUnchanged: return "unchanged";
    case ItemStatus::kKept: return "kept";
    case ItemStatus::kRemoved: return "removed";
    case ItemStatus::kConflict: return "conflict";
    case ItemStatus::kError: return "error";
  }
  return "error";
}

std::string ItemJson(const ItemResult& item) {
  return std::string("{\"status\":\"") + StatusName(item.status) +
         "\",\"enabled\":" +
         (item.enabled.has_value() ? (*item.enabled ? "true" : "false")
                                   : "null") + "}";
}

std::wstring ExecutablePath() {
  std::vector<wchar_t> path(32768);
  const DWORD count = ::GetModuleFileNameW(
      nullptr, path.data(), static_cast<DWORD>(path.size()));
  if (count == 0 || count >= path.size()) return {};
  return std::wstring(path.data(), count);
}

std::wstring DesktopShortcutPath() {
  PWSTR desktop = nullptr;
  // No CREATE flag: an unavailable/redirected Desktop is an item failure.
  const HRESULT result = ::SHGetKnownFolderPath(
      FOLDERID_Desktop, KF_FLAG_DEFAULT, nullptr, &desktop);
  if (FAILED(result) || desktop == nullptr) return {};
  std::wstring path(desktop);
  ::CoTaskMemFree(desktop);
  if (path.empty()) return {};
  if (path.back() != L'\\') path += L'\\';
  return path + L"Usque.lnk";
}

bool OriginalUserContext() {
  HANDLE raw_token = nullptr;
  // Setup must run in its original unelevated process, not through an
  // administrator or an impersonated identity. A non-elevated admin is valid.
  if (::OpenThreadToken(::GetCurrentThread(), TOKEN_QUERY, TRUE, &raw_token)) {
    ::CloseHandle(raw_token);
    return false;
  }
  if (::GetLastError() != ERROR_NO_TOKEN ||
      !::OpenProcessToken(::GetCurrentProcess(), TOKEN_QUERY, &raw_token)) {
    return false;
  }
  FileHandle token(raw_token);
  TOKEN_ELEVATION elevation{};
  DWORD size = 0;
  if (!::GetTokenInformation(token.get(), TokenElevation, &elevation,
                             sizeof(elevation), &size) ||
      elevation.TokenIsElevated != 0) return false;
  ::GetTokenInformation(token.get(), TokenUser, nullptr, 0, &size);
  if (size == 0 || size > 65536) return false;
  std::vector<BYTE> user_buffer(size);
  if (!::GetTokenInformation(token.get(), TokenUser, user_buffer.data(),
                             size, &size)) return false;
  const auto* user = reinterpret_cast<const TOKEN_USER*>(user_buffer.data());
  return !::IsWellKnownSid(user->User.Sid, WinLocalSystemSid) &&
         !::IsWellKnownSid(user->User.Sid, WinLocalServiceSid) &&
         !::IsWellKnownSid(user->User.Sid, WinNetworkServiceSid);
}

EntryState ReadStartup(const std::wstring& executable) {
  wchar_t command[32768]{};
  DWORD size = sizeof(command);
  const LSTATUS result = ::RegGetValueW(
      HKEY_CURRENT_USER, kRunKey, kRunValue, RRF_RT_REG_SZ | RRF_ZEROONFAILURE,
      nullptr, command, &size);
  if (result == ERROR_FILE_NOT_FOUND || result == ERROR_PATH_NOT_FOUND) {
    return EntryState::kAbsent;
  }
  if (result == ERROR_UNSUPPORTED_TYPE || result == ERROR_MORE_DATA) {
    return EntryState::kForeign;
  }
  if (result != ERROR_SUCCESS || size < sizeof(wchar_t) ||
      size % sizeof(wchar_t) != 0) return EntryState::kError;
  const size_t count = size / sizeof(wchar_t);
  if (command[count - 1] != L'\0' ||
      std::find(command, command + count - 1, L'\0') != command + count - 1) {
    return EntryState::kForeign;
  }
  return StartupCommandOwned(std::wstring(command, count - 1), executable)
             ? EntryState::kOwned : EntryState::kForeign;
}

ItemResult WriteStartup(const std::wstring& executable, bool enabled) {
  const EntryState state = ReadStartup(executable);
  if (state == EntryState::kError) return {ItemStatus::kError, std::nullopt};
  if (state == EntryState::kForeign) {
    return {enabled ? ItemStatus::kConflict : ItemStatus::kKept, std::nullopt};
  }
  if (enabled && state == EntryState::kOwned) {
    return {ItemStatus::kUnchanged, true};
  }
  if (!enabled && state == EntryState::kAbsent) {
    return {ItemStatus::kUnchanged, false};
  }
  HKEY key = nullptr;
  LSTATUS result = enabled
      ? ::RegCreateKeyExW(HKEY_CURRENT_USER, kRunKey, 0, nullptr, 0,
                         KEY_SET_VALUE, nullptr, &key, nullptr)
      : ::RegOpenKeyExW(HKEY_CURRENT_USER, kRunKey, 0, KEY_SET_VALUE, &key);
  if (!enabled && result == ERROR_FILE_NOT_FOUND) {
    return {ItemStatus::kUnchanged, false};
  }
  if (result != ERROR_SUCCESS) return {ItemStatus::kError, std::nullopt};
  if (enabled) {
    const std::wstring command = L"\"" + executable + L"\" --background";
    result = ::RegSetValueExW(
        key, kRunValue, 0, REG_SZ,
        reinterpret_cast<const BYTE*>(command.c_str()),
        static_cast<DWORD>((command.size() + 1) * sizeof(wchar_t)));
  } else {
    result = ::RegDeleteValueW(key, kRunValue);
    if (result == ERROR_FILE_NOT_FOUND) result = ERROR_SUCCESS;
  }
  ::RegCloseKey(key);
  return result == ERROR_SUCCESS
      ? ItemResult{enabled ? ItemStatus::kEnabled : ItemStatus::kDisabled, enabled}
      : ItemResult{ItemStatus::kError, std::nullopt};
}

EntryState ReadShortcutHandle(HANDLE file, const std::wstring& executable) {
  BY_HANDLE_FILE_INFORMATION info{};
  LARGE_INTEGER size{};
  if (!::GetFileInformationByHandle(file, &info) ||
      !::GetFileSizeEx(file, &size)) return EntryState::kError;
  if ((info.dwFileAttributes &
       (FILE_ATTRIBUTE_REPARSE_POINT | FILE_ATTRIBUTE_DIRECTORY)) != 0 ||
      size.QuadPart <= 0 || size.QuadPart > kMaxLinkBytes) {
    return EntryState::kForeign;
  }
  std::vector<BYTE> bytes(static_cast<size_t>(size.QuadPart));
  DWORD read = 0;
  if (!::ReadFile(file, bytes.data(), static_cast<DWORD>(bytes.size()),
                  &read, nullptr) || read != bytes.size()) return EntryState::kError;
  ComPtr<IStream> stream;
  stream.Attach(::SHCreateMemStream(bytes.data(), static_cast<UINT>(bytes.size())));
  ComPtr<IShellLinkW> link;
  ComPtr<IPersistStream> persistent;
  if (!stream || FAILED(::CoCreateInstance(CLSID_ShellLink, nullptr,
      CLSCTX_INPROC_SERVER, IID_PPV_ARGS(&link))) ||
      FAILED(link.As(&persistent)) || FAILED(persistent->Load(stream.Get()))) {
    return EntryState::kForeign;
  }
  wchar_t target[32768]{};
  wchar_t arguments[32768]{};
  // Never call Resolve: stale or moved shortcuts must not acquire a new owner.
  if (FAILED(link->GetPath(target, 32768, nullptr, SLGP_RAWPATH)) ||
      FAILED(link->GetArguments(arguments, 32768)) ||
      !ShortcutTargetsExecutable(target, arguments, executable)) {
    return EntryState::kForeign;
  }
  ComPtr<IPropertyStore> properties;
  PROPVARIANT app_id{};
  if (FAILED(link.As(&properties)) ||
      FAILED(properties->GetValue(PKEY_AppUserModel_ID, &app_id))) {
    ::PropVariantClear(&app_id);
    return EntryState::kForeign;
  }
  const bool unmarked = app_id.vt == VT_EMPTY;
  const bool owned = app_id.vt == VT_LPWSTR && app_id.pwszVal != nullptr &&
      ShortcutOwned(target, arguments, app_id.pwszVal, executable);
  ::PropVariantClear(&app_id);
  return owned ? EntryState::kOwned
               : unmarked ? EntryState::kSameTarget : EntryState::kForeign;
}

bool MarkForDeletion(HANDLE handle) {
  FILE_DISPOSITION_INFO disposition{TRUE};
  return ::SetFileInformationByHandle(handle, FileDispositionInfo,
      &disposition, sizeof(disposition)) != FALSE;
}

class WindowsPlatform final : public Platform {
 public:
  WindowsPlatform() = default;
  explicit WindowsPlatform(std::wstring executable)
      : executable_(std::move(executable)) {}
  bool IsOriginalUserContext() override { return OriginalUserContext(); }
  std::wstring ExecutablePath() override {
    return executable_.has_value() ? *executable_ : shell::ExecutablePath();
  }
  EntryState DesktopState(const std::wstring& executable) override {
    const auto path = DesktopShortcutPath();
    return path.empty() ? EntryState::kError : InspectShortcut(path, executable);
  }
  EntryState StartupState(const std::wstring& executable) override {
    return ReadStartup(executable);
  }
  ItemResult CreateDesktopLink(const std::wstring& executable) override {
    const auto path = DesktopShortcutPath();
    return path.empty() ? ItemResult{ItemStatus::kError, std::nullopt}
                        : CreateShortcut(path, executable);
  }
  ItemResult RemoveDesktopLink(const std::wstring& executable) override {
    const auto path = DesktopShortcutPath();
    return path.empty() ? ItemResult{ItemStatus::kError, std::nullopt}
                        : RemoveShortcut(path, executable);
  }
  ItemResult SetStartup(const std::wstring& executable, bool enabled) override {
    return WriteStartup(executable, enabled);
  }

 private:
  std::optional<std::wstring> executable_;
};

}  // namespace

Command ParseCommand(const std::vector<std::string>& arguments) {
  const auto reserved = [](const std::string& value) {
    for (const auto* prefix : {"--setup-options", "--query-setup-options",
                              "--desktop-shortcut", "--start-on-login",
                              "--remove-startup"}) {
      if (value.rfind(prefix, 0) == 0) return true;
    }
    return false;
  };
  if (std::none_of(arguments.begin(), arguments.end(), reserved)) return {};
  Command result{CommandMode::kInvalid};
  if (arguments.size() == 1 && arguments[0] == "--query-setup-options") {
    result.mode = CommandMode::kQuery;
    return result;
  }
  if (arguments.size() == 1 && arguments[0] == "--remove-startup") {
    result.mode = CommandMode::kRemove;
    return result;
  }
  if (arguments.size() != 3) return result;
  bool apply = false;
  bool desktop = false;
  bool startup = false;
  for (const auto& argument : arguments) {
    if (argument == "--setup-options" && !apply) {
      apply = true;
    } else if (!desktop && (argument == "--desktop-shortcut=create" ||
                           argument == "--desktop-shortcut=keep")) {
      desktop = true;
      result.desktop = argument == "--desktop-shortcut=create"
                           ? DesktopChoice::kCreate : DesktopChoice::kKeep;
    } else if (!startup && (argument == "--start-on-login=enable" ||
                           argument == "--start-on-login=disable" ||
                           argument == "--start-on-login=keep")) {
      startup = true;
      result.startup = argument == "--start-on-login=enable"
          ? StartupChoice::kEnable : argument == "--start-on-login=disable"
          ? StartupChoice::kDisable : StartupChoice::kKeep;
    } else {
      return {CommandMode::kInvalid};
    }
  }
  if (apply && desktop && startup) result.mode = CommandMode::kApply;
  return result;
}

std::optional<std::wstring> NormalizeAbsolutePath(const std::wstring& input) {
  if (input.empty() || input.size() > 32767 ||
      input.find_first_of(L"\"<>|?*\r\n") != std::wstring::npos ||
      input.find(L'\0') != std::wstring::npos) return std::nullopt;
  std::wstring path = input;
  std::replace(path.begin(), path.end(), L'/', L'\\');
  size_t root_length = 0;
  if (path.size() >= 3 && ((path[0] >= L'A' && path[0] <= L'Z') ||
                          (path[0] >= L'a' && path[0] <= L'z')) &&
      path[1] == L':' && path[2] == L'\\') {
    root_length = 3;
  } else if (path.rfind(L"\\\\", 0) == 0 && path.size() > 4 && path[2] != L'.') {
    const auto server_end = path.find(L'\\', 2);
    if (server_end == std::wstring::npos || server_end == 2) return std::nullopt;
    const auto share_end = path.find(L'\\', server_end + 1);
    if (share_end == std::wstring::npos || share_end == server_end + 1) return std::nullopt;
    root_length = share_end + 1;
  } else {
    return std::nullopt;
  }
  if (path.find(L':', root_length) != std::wstring::npos) return std::nullopt;
  std::vector<std::wstring> parts;
  size_t offset = root_length;
  while (offset < path.size()) {
    const auto end = path.find(L'\\', offset);
    const std::wstring part = path.substr(offset, end == std::wstring::npos
        ? std::wstring::npos : end - offset);
    if (part == L"..") {
      if (parts.empty()) return std::nullopt;
      parts.pop_back();
    } else if (!part.empty() && part != L".") {
      if (part.back() == L'.' || part.back() == L' ') return std::nullopt;
      parts.push_back(part);
    }
    if (end == std::wstring::npos) break;
    offset = end + 1;
  }
  if (parts.empty()) return std::nullopt;
  std::wstring result = path.substr(0, root_length);
  for (const auto& part : parts) {
    if (result.back() != L'\\') result += L'\\';
    result += part;
  }
  return result;
}

bool SameExecutable(const std::wstring& left, const std::wstring& right) {
  const auto normalized_left = NormalizeAbsolutePath(left);
  const auto normalized_right = NormalizeAbsolutePath(right);
  return normalized_left && normalized_right &&
      ::CompareStringOrdinal(normalized_left->c_str(), -1,
                             normalized_right->c_str(), -1, TRUE) == CSTR_EQUAL;
}

bool StartupCommandOwned(const std::wstring& command,
                         const std::wstring& executable) {
  if (command.empty() || command.front() != L'"') return false;
  const auto end = command.find(L'"', 1);
  return end != std::wstring::npos &&
      command.substr(end + 1) == L" --background" &&
      SameExecutable(command.substr(1, end - 1), executable);
}

bool ShortcutTargetsExecutable(const std::wstring& target,
                                const std::wstring& arguments,
                                const std::wstring& executable) {
  return arguments.empty() && SameExecutable(target, executable);
}

bool ShortcutOwned(const std::wstring& target, const std::wstring& arguments,
                   const std::wstring& app_id, const std::wstring& executable) {
  return app_id == kAppId &&
      ShortcutTargetsExecutable(target, arguments, executable);
}

namespace {

CommandResult ExecuteCommandImpl(const Command& command, Platform& platform,
                                 bool installer_options) {
  CommandResult result;
  if (command.mode == CommandMode::kInvalid || command.mode == CommandMode::kNone ||
      (installer_options && command.mode != CommandMode::kQuery &&
       command.mode != CommandMode::kApply)) {
    result.exit_code = 2;
    result.status = "invalid_arguments";
    return result;
  }
  // Legacy MSI Impersonate=yes cleanup retains its current-user behavior.
  // Only new setup/query commands require the unelevated user agent.
  if (command.mode != CommandMode::kRemove && !platform.IsOriginalUserContext()) {
    result.exit_code = 3;
    result.status = "user_context_required";
    return result;
  }
  auto executable = platform.ExecutablePath();
  const auto normalized = NormalizeAbsolutePath(executable);
  if (installer_options) {
    const auto name = normalized
        ? normalized->substr(normalized->find_last_of(L'\\') + 1)
        : std::wstring();
    if (!normalized ||
        ::CompareStringOrdinal(name.c_str(), -1, L"usque.exe", -1, TRUE) != CSTR_EQUAL) {
      result.exit_code = 2;
      result.status = "invalid_arguments";
      return result;
    }
    executable = *normalized;
  }
  if (!normalized) {
    result.exit_code = 1;
    result.status = "partial";
    result.desktop = result.startup = {ItemStatus::kError, std::nullopt};
    return result;
  }
  if (command.mode == CommandMode::kQuery) {
    result.desktop = FromState(platform.DesktopState(executable), false);
    result.startup = FromState(platform.StartupState(executable), true);
  } else if (command.mode == CommandMode::kRemove) {
    result.desktop = platform.RemoveDesktopLink(executable);
    result.startup = platform.SetStartup(executable, false);
  } else {
    result.desktop = command.desktop == DesktopChoice::kCreate
        ? platform.CreateDesktopLink(executable)
        : FromState(platform.DesktopState(executable), false, true);
    result.startup = command.startup == StartupChoice::kKeep
        ? FromState(platform.StartupState(executable), true, true)
        : platform.SetStartup(executable, command.startup == StartupChoice::kEnable);
  }
  if (Failed(result.desktop) || Failed(result.startup)) {
    result.exit_code = 1;
    result.status = "partial";
  }
  return result;
}

}  // namespace

CommandResult ExecuteCommand(const Command& command, Platform& platform) {
  return ExecuteCommandImpl(command, platform, false);
}

CommandResult ExecuteInstallerOptions(const Command& command, Platform& platform) {
  return ExecuteCommandImpl(command, platform, true);
}

CommandResult ExecuteInstallerOptions(const Command& command,
                                     const std::wstring& installed_usque_path) {
  const HRESULT com = ::CoInitializeEx(nullptr, COINIT_APARTMENTTHREADED);
  if (FAILED(com)) {
    CommandResult result;
    result.exit_code = 1;
    result.status = "partial";
    result.desktop = result.startup = {ItemStatus::kError, std::nullopt};
    return result;
  }
  WindowsPlatform platform(installed_usque_path);
  const auto result = ExecuteInstallerOptions(command, platform);
  ::CoUninitialize();
  return result;
}

std::string ResultJson(const CommandResult& result) {
  // Only closed-set status values and booleans leave the process. No file
  // paths, registry content, account identity, or arbitrary error text.
  const std::string status = result.status == "ok" || result.status == "partial" ||
      result.status == "invalid_arguments" || result.status == "user_context_required"
          ? result.status : "partial";
  return "{\"schema\":1,\"status\":\"" + status +
      "\",\"desktopShortcut\":" + ItemJson(result.desktop) +
      ",\"startOnLogin\":" + ItemJson(result.startup) + "}\n";
}

EntryState InspectShortcut(const std::wstring& path,
                           const std::wstring& executable) {
  FileHandle file(::CreateFileW(path.c_str(), GENERIC_READ, FILE_SHARE_READ,
      nullptr, OPEN_EXISTING, FILE_FLAG_OPEN_REPARSE_POINT, nullptr));
  if (file.get() == INVALID_HANDLE_VALUE) {
    const DWORD error = ::GetLastError();
    return error == ERROR_FILE_NOT_FOUND || error == ERROR_PATH_NOT_FOUND
        ? EntryState::kAbsent : EntryState::kError;
  }
  return ReadShortcutHandle(file.get(), executable);
}

ItemResult CreateShortcut(const std::wstring& path,
                          const std::wstring& executable) {
  const EntryState state = InspectShortcut(path, executable);
  if (state == EntryState::kOwned || state == EntryState::kSameTarget) {
    return {ItemStatus::kUnchanged, true};
  }
  if (state != EntryState::kAbsent) return FromState(state, false);
  if (!NormalizeAbsolutePath(executable)) return {ItemStatus::kError, std::nullopt};
  ComPtr<IShellLinkW> link;
  ComPtr<IPropertyStore> properties;
  ComPtr<IPersistStream> persistent;
  ComPtr<IStream> stream;
  if (FAILED(::CoCreateInstance(CLSID_ShellLink, nullptr, CLSCTX_INPROC_SERVER,
      IID_PPV_ARGS(&link))) || FAILED(link->SetPath(executable.c_str())) ||
      FAILED(link->SetArguments(L"")) || FAILED(link->SetDescription(L"Usque")) ||
      FAILED(link->SetWorkingDirectory(executable.substr(
          0, executable.find_last_of(L"\\/")).c_str())) ||
      FAILED(link->SetIconLocation(executable.c_str(), 0)) ||
      FAILED(link.As(&properties)) || FAILED(link.As(&persistent)) ||
      FAILED(::CreateStreamOnHGlobal(nullptr, TRUE, &stream))) {
    return {ItemStatus::kError, std::nullopt};
  }
  PROPVARIANT app_id{};
  HRESULT result = ::InitPropVariantFromString(kAppId, &app_id);
  if (SUCCEEDED(result)) result = properties->SetValue(PKEY_AppUserModel_ID, app_id);
  ::PropVariantClear(&app_id);
  if (FAILED(result) || FAILED(properties->Commit()) ||
      FAILED(persistent->Save(stream.Get(), TRUE))) {
    return {ItemStatus::kError, std::nullopt};
  }
  STATSTG stat{};
  HGLOBAL memory = nullptr;
  if (FAILED(stream->Stat(&stat, STATFLAG_NONAME)) ||
      stat.cbSize.QuadPart == 0 || stat.cbSize.QuadPart > kMaxLinkBytes ||
      FAILED(::GetHGlobalFromStream(stream.Get(), &memory))) {
    return {ItemStatus::kError, std::nullopt};
  }
  const void* bytes = ::GlobalLock(memory);
  if (bytes == nullptr) return {ItemStatus::kError, std::nullopt};
  // CREATE_NEW is the final ownership boundary: a racing same-name file is
  // never overwritten. On failure, remove only the file held by this handle.
  FileHandle file(::CreateFileW(path.c_str(), GENERIC_WRITE | DELETE, 0, nullptr,
      CREATE_NEW, FILE_ATTRIBUTE_NORMAL, nullptr));
  if (file.get() == INVALID_HANDLE_VALUE) {
    const DWORD error = ::GetLastError();
    ::GlobalUnlock(memory);
    if (error == ERROR_FILE_EXISTS || error == ERROR_ALREADY_EXISTS) {
      const EntryState raced = InspectShortcut(path, executable);
      if (raced == EntryState::kOwned || raced == EntryState::kSameTarget) {
        return {ItemStatus::kUnchanged, true};
      }
      return {ItemStatus::kConflict, std::nullopt};
    }
    return {ItemStatus::kError, std::nullopt};
  }
  DWORD written = 0;
  const DWORD count = static_cast<DWORD>(stat.cbSize.QuadPart);
  const bool saved = ::WriteFile(file.get(), bytes, count, &written, nullptr) &&
      written == count && ::FlushFileBuffers(file.get());
  ::GlobalUnlock(memory);
  if (!saved) {
    MarkForDeletion(file.get());
    return {ItemStatus::kError, std::nullopt};
  }
  return {ItemStatus::kCreated, true};
}

ItemResult RemoveShortcut(const std::wstring& path,
                          const std::wstring& executable) {
  // Hold a non-share-write/non-share-delete handle through the ownership
  // check and deletion, preventing a same-name replacement from being removed.
  FileHandle file(::CreateFileW(path.c_str(), GENERIC_READ | DELETE,
      FILE_SHARE_READ, nullptr, OPEN_EXISTING, FILE_FLAG_OPEN_REPARSE_POINT, nullptr));
  if (file.get() == INVALID_HANDLE_VALUE) {
    const DWORD error = ::GetLastError();
    return error == ERROR_FILE_NOT_FOUND || error == ERROR_PATH_NOT_FOUND
        ? ItemResult{ItemStatus::kAbsent, false}
        : ItemResult{ItemStatus::kError, std::nullopt};
  }
  const EntryState state = ReadShortcutHandle(file.get(), executable);
  if (state == EntryState::kError) return {ItemStatus::kError, std::nullopt};
  if (state != EntryState::kOwned) return FromState(state, false, true);
  return MarkForDeletion(file.get())
      ? ItemResult{ItemStatus::kRemoved, false}
      : ItemResult{ItemStatus::kError, true};
}

bool IsStartOnLoginEnabled() {
  const auto path = ExecutablePath();
  return !path.empty() && ReadStartup(path) == EntryState::kOwned;
}

bool SetStartOnLogin(bool enabled) {
  const auto path = ExecutablePath();
  return !path.empty() && !Failed(WriteStartup(path, enabled));
}

std::optional<int> HandleCommandLine(const std::vector<std::string>& arguments) {
  const Command command = ParseCommand(arguments);
  if (command.mode == CommandMode::kNone) return std::nullopt;
  const HRESULT com = ::CoInitializeEx(nullptr, COINIT_APARTMENTTHREADED);
  WindowsPlatform platform;
  CommandResult result;
  if (FAILED(com)) {
    result.exit_code = 1;
    result.status = "partial";
    result.desktop = result.startup = {ItemStatus::kError, std::nullopt};
  } else {
    result = ExecuteCommand(command, platform);
  }
  const std::string output = ResultJson(result);
  DWORD written = 0;
  const HANDLE out = ::GetStdHandle(STD_OUTPUT_HANDLE);
  const bool output_ok = out != nullptr && out != INVALID_HANDLE_VALUE &&
      ::WriteFile(out, output.data(), static_cast<DWORD>(output.size()),
                   &written, nullptr) && written == output.size();
  if (SUCCEEDED(com)) ::CoUninitialize();
  // The legacy MSI cleanup has no output pipe. New callers must capture the
  // bounded JSON; failure to deliver it must not be reported as full success.
  if (!output_ok && command.mode != CommandMode::kRemove) return 1;
  return result.exit_code;
}

}  // namespace usque::shell
