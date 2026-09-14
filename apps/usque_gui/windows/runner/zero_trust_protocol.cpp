#include "zero_trust_protocol.h"

#include <string>
#include <vector>

namespace {

std::wstring SkipLeadingSpaces(std::wstring_view value) {
  size_t index = 0;
  while (index < value.size() && value[index] == L' ') {
    ++index;
  }
  return std::wstring(value.substr(index));
}

std::wstring CommandExecutable(std::wstring_view command) {
  const std::wstring trimmed = SkipLeadingSpaces(command);
  if (trimmed.empty()) return {};
  if (trimmed.front() == L'"') {
    const auto end = trimmed.find(L'"', 1);
    if (end == std::wstring::npos) return {};
    return trimmed.substr(1, end - 1);
  }
  const auto end = trimmed.find(L' ');
  if (end == std::wstring::npos) return trimmed;
  return trimmed.substr(0, end);
}

std::wstring NormalizeComparablePath(std::wstring path) {
  for (wchar_t& unit : path) {
    if (unit == L'/') unit = L'\\';
  }
  std::vector<wchar_t> full(32768);
  const DWORD length =
      ::GetFullPathNameW(path.c_str(), static_cast<DWORD>(full.size()),
                         full.data(), nullptr);
  if (length > 0 && length < full.size()) {
    path.assign(full.data(), length);
  }
  for (wchar_t& unit : path) {
    if (unit >= L'A' && unit <= L'Z') {
      unit = static_cast<wchar_t>(unit - L'A' + L'a');
    }
  }
  while (path.size() > 3 && path.back() == L'\\') {
    path.pop_back();
  }
  return path;
}

bool PathsReferToSameExe(const std::wstring& left, const std::wstring& right) {
  if (left.empty() || right.empty()) return false;
  return NormalizeComparablePath(left) == NormalizeComparablePath(right);
}

std::wstring ReadCommandValue(HKEY root, const wchar_t* protocol_key) {
  const std::wstring command_key =
      std::wstring(protocol_key) + L"\\shell\\open\\command";
  wchar_t value[32768]{};
  DWORD size = sizeof(value);
  const LSTATUS status =
      ::RegGetValueW(root, command_key.c_str(), nullptr, RRF_RT_REG_SZ, nullptr,
                     value, &size);
  if (status != ERROR_SUCCESS) return {};
  return value;
}

bool WriteStringValue(HKEY key, const wchar_t* name, const std::wstring& value) {
  return ::RegSetValueExW(key, name, 0, REG_SZ,
                          reinterpret_cast<const BYTE*>(value.c_str()),
                          static_cast<DWORD>((value.size() + 1) * sizeof(wchar_t))) ==
         ERROR_SUCCESS;
}

}  // namespace

std::wstring CurrentExecutablePath() {
  wchar_t executable[MAX_PATH]{};
  const DWORD length = ::GetModuleFileNameW(nullptr, executable, MAX_PATH);
  if (length == 0 || length >= MAX_PATH) return {};
  return std::wstring(executable, length);
}

bool WarpProtocolAssociationPointsAtExe(HKEY root, const wchar_t* protocol_key,
                                        const wchar_t* exe_path) {
  if (protocol_key == nullptr || exe_path == nullptr || exe_path[0] == L'\0') {
    return false;
  }
  const std::wstring command = ReadCommandValue(root, protocol_key);
  if (command.empty()) return false;
  return PathsReferToSameExe(CommandExecutable(command), exe_path);
}

bool SetWarpProtocolAssociation(HKEY root, const wchar_t* protocol_key,
                                const wchar_t* exe_path, bool enabled) {
  if (protocol_key == nullptr || exe_path == nullptr || exe_path[0] == L'\0') {
    return false;
  }
  if (!enabled) {
    if (!WarpProtocolAssociationPointsAtExe(root, protocol_key, exe_path)) {
      return true;
    }
    const LSTATUS status = ::RegDeleteTreeW(root, protocol_key);
    return status == ERROR_SUCCESS || status == ERROR_FILE_NOT_FOUND;
  }

  HKEY key = nullptr;
  if (::RegCreateKeyExW(root, protocol_key, 0, nullptr, 0, KEY_SET_VALUE, nullptr,
                        &key, nullptr) != ERROR_SUCCESS) {
    return false;
  }
  const bool wrote_protocol =
      WriteStringValue(key, nullptr, L"URL:Cloudflare WARP") &&
      WriteStringValue(key, L"URL Protocol", L"");
  ::RegCloseKey(key);
  if (!wrote_protocol) return false;

  const std::wstring command_key =
      std::wstring(protocol_key) + L"\\shell\\open\\command";
  if (::RegCreateKeyExW(root, command_key.c_str(), 0, nullptr, 0, KEY_SET_VALUE,
                        nullptr, &key, nullptr) != ERROR_SUCCESS) {
    return false;
  }
  const std::wstring command =
      L"\"" + std::wstring(exe_path) + L"\" \"%1\"";
  const bool wrote_command = WriteStringValue(key, nullptr, command);
  ::RegCloseKey(key);
  return wrote_command;
}

bool SetTemporaryWarpProtocolAssociation(HKEY root, const wchar_t* protocol_key,
                                         const wchar_t* exe_path, bool enabled) {
  if (protocol_key == nullptr || exe_path == nullptr || exe_path[0] == L'\0') {
    return false;
  }
  const std::wstring path(protocol_key);
  const auto separator = path.find_last_of(L'\\');
  if (separator == std::wstring::npos) return false;
  HKEY parent = nullptr;
  if (::RegCreateKeyExW(root, path.substr(0, separator).c_str(), 0, nullptr, 0,
                        KEY_ALL_ACCESS, nullptr, &parent, nullptr) !=
      ERROR_SUCCESS) {
    return false;
  }
  const std::wstring name = path.substr(separator + 1);
  const std::wstring backup = name + L".UsqueBackup";
  const std::wstring pending = name + L".UsquePending";
  const auto exists = [parent](const std::wstring& key) {
    HKEY opened = nullptr;
    const LSTATUS status = ::RegOpenKeyExW(parent, key.c_str(), 0, KEY_READ,
                                          &opened);
    if (opened != nullptr) ::RegCloseKey(opened);
    // Access failures must never be mistaken for an absent key.
    return status != ERROR_FILE_NOT_FOUND;
  };
  const auto owned = [parent, exe_path](const std::wstring& key) {
    return WarpProtocolAssociationPointsAtExe(parent, key.c_str(), exe_path);
  };
  const auto restore = [&]() {
    if (exists(pending)) {
      if (!owned(pending) ||
          ::RegDeleteTreeW(parent, pending.c_str()) != ERROR_SUCCESS) {
        return false;
      }
    }
    if (owned(name) &&
        ::RegDeleteTreeW(parent, name.c_str()) != ERROR_SUCCESS) {
      return false;
    }
    if (!exists(name) && exists(backup)) {
      return ::RegRenameKey(parent, backup.c_str(), name.c_str()) ==
             ERROR_SUCCESS;
    }
    return true;
  };
  bool success = restore();
  if (enabled && success) {
    // A third party may have claimed the protocol while we were active. Keep
    // its registration and the saved original intact; require recovery first.
    success = !exists(backup);
    if (success) {
      success = SetWarpProtocolAssociation(parent, pending.c_str(), exe_path,
                                            true);
    }
    if (success && exists(name)) {
      success = ::RegRenameKey(parent, name.c_str(), backup.c_str()) ==
                ERROR_SUCCESS;
    }
    if (success) {
      success = ::RegRenameKey(parent, pending.c_str(), name.c_str()) ==
                ERROR_SUCCESS;
    }
    if (!success) restore();
  }
  ::RegCloseKey(parent);
  return success;
}

bool SetCurrentUserWarpProtocolAssociation(bool enabled) {
  const std::wstring exe = CurrentExecutablePath();
  if (exe.empty()) return false;
  return SetTemporaryWarpProtocolAssociation(
      HKEY_CURRENT_USER, kUsqueWarpProtocolKey, exe.c_str(), enabled);
}
