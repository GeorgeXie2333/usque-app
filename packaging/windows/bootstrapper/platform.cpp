#include "platform.h"
#include "shell_integration.h"

#include <shlobj.h>
#include <softpub.h>
#include <wincrypt.h>
#include <wintrust.h>

#include <array>
#include <filesystem>
#include <fstream>
#include <iterator>

namespace usque::setup {
namespace {
struct Handle {
  HANDLE value = nullptr;
  ~Handle() { if (value && value != INVALID_HANDLE_VALUE) CloseHandle(value); }
};

std::vector<BYTE> VerifiedCertificate(const std::wstring& path) {
  WINTRUST_FILE_INFO file{sizeof(file)};
  file.pcwszFilePath = path.c_str();
  WINTRUST_DATA trust{sizeof(trust)};
  trust.dwUIChoice = WTD_UI_NONE;
  trust.fdwRevocationChecks = WTD_REVOKE_NONE;
  trust.dwUnionChoice = WTD_CHOICE_FILE;
  trust.pFile = &file;
  trust.dwStateAction = WTD_STATEACTION_VERIFY;
  trust.dwProvFlags = WTD_CACHE_ONLY_URL_RETRIEVAL;
  GUID action = WINTRUST_ACTION_GENERIC_VERIFY_V2;
  const LONG status = WinVerifyTrust(nullptr, &action, &trust);
  std::vector<BYTE> certificate;
  // Usque's fixed pre-1.0 signer is self-signed. An untrusted root is allowed
  // only after Authenticode validation and exact certificate equality below.
  if (status == ERROR_SUCCESS || status == CERT_E_UNTRUSTEDROOT) {
    auto* provider = WTHelperProvDataFromStateData(trust.hWVTStateData);
    auto* signer = provider ? WTHelperGetProvSignerFromChain(provider, 0, FALSE, 0) : nullptr;
    if (signer && signer->csCertChain && signer->pasCertChain[0].pCert) {
      auto* cert = signer->pasCertChain[0].pCert;
      certificate.assign(cert->pbCertEncoded, cert->pbCertEncoded + cert->cbCertEncoded);
    }
  }
  trust.dwStateAction = WTD_STATEACTION_CLOSE;
  WinVerifyTrust(nullptr, &action, &trust);
  return certificate;
}

bool Start(const std::wstring& path, const std::wstring& args, STARTUPINFOW& startup,
           PROCESS_INFORMATION& process, bool inherit, DWORD flags = 0) {
  std::wstring command = L"\"" + path + L"\"";
  if (!args.empty()) command += L" " + args;
  const auto directory = std::filesystem::path(path).parent_path().wstring();
  return CreateProcessW(path.c_str(), command.data(), nullptr, nullptr,
                        inherit, CREATE_NO_WINDOW | flags, nullptr, directory.c_str(),
                        &startup, &process) != FALSE;
}
}  // namespace

std::wstring ModulePath() {
  std::wstring path(32768, L'\0');
  const DWORD length = GetModuleFileNameW(nullptr, path.data(), static_cast<DWORD>(path.size()));
  if (!length || length >= path.size()) return {};
  path.resize(length);
  return path;
}

std::wstring RegistryString(HKEY root, const wchar_t* key, const wchar_t* name) {
  DWORD size = 0;
  if (RegGetValueW(root, key, name, RRF_RT_REG_SZ | RRF_SUBKEY_WOW6464KEY,
                   nullptr, nullptr, &size) != ERROR_SUCCESS || size < sizeof(wchar_t) ||
      size > 65536) return {};
  std::wstring value(size / sizeof(wchar_t), L'\0');
  if (RegGetValueW(root, key, name, RRF_RT_REG_SZ | RRF_SUBKEY_WOW6464KEY,
                   nullptr, value.data(), &size) != ERROR_SUCCESS) return {};
  while (!value.empty() && value.back() == L'\0') value.pop_back();
  if (value.find(L'\0') != std::wstring::npos) return {};
  return value;
}

bool UnelevatedInteractiveUser() {
  Handle token;
  if (!OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &token.value)) return false;
  TOKEN_ELEVATION elevation{};
  DWORD size = 0;
  if (!GetTokenInformation(token.value, TokenElevation, &elevation, sizeof(elevation), &size) ||
      elevation.TokenIsElevated) return false;
  DWORD session = 0;
  return ProcessIdToSessionId(GetCurrentProcessId(), &session) && session != 0;
}

bool SameVerifiedSigner(const std::wstring& file) {
  const auto own = VerifiedCertificate(ModulePath());
  return !own.empty() && own == VerifiedCertificate(file);
}

bool Launch(const std::wstring& file, const std::wstring& arguments, DWORD* error) {
  // Keep a deny-write/deny-delete handle through process creation to close the
  // verification-to-launch replacement race. Never use ShellExecute/runas.
  Handle locked{CreateFileW(file.c_str(), GENERIC_READ, FILE_SHARE_READ, nullptr,
                            OPEN_EXISTING, FILE_ATTRIBUTE_NORMAL, nullptr)};
  if (locked.value == INVALID_HANDLE_VALUE) { *error = GetLastError(); return false; }
  if (!SameVerifiedSigner(file)) { *error = ERROR_INVALID_DATA; return false; }
  STARTUPINFOW startup{sizeof(startup)};
  PROCESS_INFORMATION process{};
  if (!Start(file, arguments, startup, process, false)) { *error = GetLastError(); return false; }
  CloseHandle(process.hThread);
  CloseHandle(process.hProcess);
  *error = ERROR_SUCCESS;
  return true;
}

ChildResult RunInstallerOptions(const std::wstring& file, bool query, bool desktop, int startup) {
  // Called only after successful MSI completion for the known install target.
  // Do not load or execute it: a reboot can still be required to replace it.
  // The shared shell API validates the path, user context and item ownership.
  usque::shell::Command command;
  command.mode = query ? usque::shell::CommandMode::kQuery : usque::shell::CommandMode::kApply;
  command.desktop = desktop ? usque::shell::DesktopChoice::kCreate : usque::shell::DesktopChoice::kKeep;
  command.startup = startup < 0 ? usque::shell::StartupChoice::kKeep : startup ?
    usque::shell::StartupChoice::kEnable : usque::shell::StartupChoice::kDisable;
  const auto result = usque::shell::ExecuteInstallerOptions(command, file);
  return {static_cast<DWORD>(result.exit_code), usque::shell::ResultJson(result)};
}

std::string ReadLicense() {
  const auto path = std::filesystem::path(ModulePath()).parent_path() / L"license.rtf";
  std::ifstream stream(path, std::ios::binary);
  if (!stream) return {};
  std::string bytes(std::istreambuf_iterator<char>{stream}, {});
  if (bytes.size() > 2 * 1024 * 1024) return {};
  // RTF is ASCII markup; the RichEdit streaming callback receives these bytes.
  return bytes;
}

std::wstring SelectFolder(HWND owner, const std::wstring& initial) {
  IFileOpenDialog* dialog = nullptr;
  if (FAILED(CoCreateInstance(CLSID_FileOpenDialog, nullptr, CLSCTX_INPROC_SERVER,
                               IID_PPV_ARGS(&dialog)))) return {};
  DWORD flags = 0;
  dialog->GetOptions(&flags);
  dialog->SetOptions(flags | FOS_PICKFOLDERS | FOS_FORCEFILESYSTEM);
  IShellItem* folder = nullptr;
  if (SUCCEEDED(SHCreateItemFromParsingName(initial.c_str(), nullptr, IID_PPV_ARGS(&folder)))) {
    dialog->SetFolder(folder); folder->Release();
  }
  std::wstring result;
  if (SUCCEEDED(dialog->Show(owner))) {
    IShellItem* selected = nullptr;
    if (SUCCEEDED(dialog->GetResult(&selected))) {
      PWSTR path = nullptr;
      if (SUCCEEDED(selected->GetDisplayName(SIGDN_FILESYSPATH, &path))) {
        result = path; CoTaskMemFree(path);
      }
      selected->Release();
    }
  }
  dialog->Release();
  return result;
}

HRESULT SaveDetails(HWND owner, const std::string& contents) {
  IFileSaveDialog* dialog = nullptr;
  HRESULT result = CoCreateInstance(CLSID_FileSaveDialog, nullptr, CLSCTX_INPROC_SERVER, IID_PPV_ARGS(&dialog));
  if (FAILED(result)) return result;
  DWORD flags = 0;
  dialog->GetOptions(&flags);
  dialog->SetOptions(flags | FOS_FORCEFILESYSTEM | FOS_OVERWRITEPROMPT | FOS_PATHMUSTEXIST);
  dialog->SetFileName(L"usque-setup-details.txt");
  dialog->SetDefaultExtension(L"txt");
  result = dialog->Show(owner);
  if (SUCCEEDED(result)) {
    IShellItem* item = nullptr;
    result = dialog->GetResult(&item);
    if (SUCCEEDED(result)) {
      PWSTR path = nullptr;
      result = item->GetDisplayName(SIGDN_FILESYSPATH, &path);
      if (SUCCEEDED(result)) {
        Handle file{CreateFileW(path, GENERIC_WRITE, 0, nullptr, CREATE_ALWAYS, FILE_ATTRIBUTE_NORMAL, nullptr)};
        if (file.value == INVALID_HANDLE_VALUE) result = HRESULT_FROM_WIN32(GetLastError());
        else {
          DWORD written = 0;
          if (!WriteFile(file.value, contents.data(), static_cast<DWORD>(contents.size()), &written, nullptr) ||
              written != contents.size()) result = HRESULT_FROM_WIN32(ERROR_WRITE_FAULT);
        }
        CoTaskMemFree(path);
      }
      item->Release();
    }
  }
  dialog->Release();
  return result;
}

}  // namespace usque::setup
