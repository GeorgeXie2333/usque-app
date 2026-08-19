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

bool IsCurrentUserWarpProtocolAssociated() {
  const std::wstring exe = CurrentExecutablePath();
  if (exe.empty()) return false;
  return WarpProtocolAssociationPointsAtExe(HKEY_CURRENT_USER,
                                            kUsqueWarpProtocolKey, exe.c_str());
}

bool SetCurrentUserWarpProtocolAssociation(bool enabled) {
  const std::wstring exe = CurrentExecutablePath();
  if (exe.empty()) return false;
  return SetWarpProtocolAssociation(HKEY_CURRENT_USER, kUsqueWarpProtocolKey,
                                    exe.c_str(), enabled);
}
