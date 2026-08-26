#include <flutter/dart_project.h>
#include <flutter/flutter_view_controller.h>
#include <sddl.h>
#include <windows.h>

#include <algorithm>
#include <string>
#include <vector>

#include "flutter_window.h"
#include "utils.h"
#include "zero_trust_callback.h"

namespace {

std::wstring CurrentUserSid() {
  HANDLE token = nullptr;
  if (!::OpenProcessToken(::GetCurrentProcess(), TOKEN_QUERY, &token)) {
    return {};
  }
  DWORD size = 0;
  ::GetTokenInformation(token, TokenUser, nullptr, 0, &size);
  std::vector<BYTE> buffer(size);
  if (size == 0 || !::GetTokenInformation(token, TokenUser, buffer.data(),
                                           size, &size)) {
    ::CloseHandle(token);
    return {};
  }
  ::CloseHandle(token);
  const auto* user = reinterpret_cast<const TOKEN_USER*>(buffer.data());
  wchar_t* sid = nullptr;
  if (!::ConvertSidToStringSidW(user->User.Sid, &sid) || sid == nullptr) {
    return {};
  }
  std::wstring value(sid);
  ::LocalFree(sid);
  return value;
}

bool HasArgument(const std::vector<std::string>& arguments,
                 const std::string& expected) {
  return std::find(arguments.begin(), arguments.end(), expected) !=
         arguments.end();
}

void RemoveStartupEntry() {
  HKEY key = nullptr;
  if (::RegOpenKeyExW(
          HKEY_CURRENT_USER,
          L"Software\\Microsoft\\Windows\\CurrentVersion\\Run", 0,
          KEY_SET_VALUE, &key) == ERROR_SUCCESS) {
    ::RegDeleteValueW(key, L"Usque");
    ::RegCloseKey(key);
  }
}

}  // namespace

int APIENTRY wWinMain(_In_ HINSTANCE instance, _In_opt_ HINSTANCE prev,
                      _In_ wchar_t *command_line, _In_ int show_command) {
  // Attach to console when present (e.g., 'flutter run') or create a
  // new console when running with a debugger.
  if (!::AttachConsole(ATTACH_PARENT_PROCESS) && ::IsDebuggerPresent()) {
    CreateAndAttachConsole();
  }

  // Initialize COM, so that it is available for use in the library and/or
  // plugins.
  ::CoInitializeEx(nullptr, COINIT_APARTMENTTHREADED);

  flutter::DartProject project(L"data");

  std::vector<std::string> command_line_arguments =
      GetCommandLineArguments();

  if (HasArgument(command_line_arguments, "--remove-startup")) {
    RemoveStartupEntry();
    ::CoUninitialize();
    return EXIT_SUCCESS;
  }

  const std::wstring sid = CurrentUserSid();
  if (sid.empty()) {
    ::CoUninitialize();
    return EXIT_FAILURE;
  }
  const std::wstring mutex_name =
      L"Local\\io.github.georgexie2333.usque.ui." + sid;
  HANDLE instance_mutex = ::CreateMutexW(nullptr, FALSE, mutex_name.c_str());
  if (instance_mutex == nullptr) {
    ::CoUninitialize();
    return EXIT_FAILURE;
  }
  const DWORD mutex_status = ::GetLastError();
  const auto forwarded_callback =
      ExtractZeroTrustCallbackArgument(command_line_arguments);
  if (mutex_status == ERROR_ALREADY_EXISTS) {
    HWND existing =
        ::FindWindowW(L"FLUTTER_RUNNER_WIN32_WINDOW", L"Usque");
    if (existing != nullptr) {
      ::ShowWindow(existing, SW_RESTORE);
      ::SetForegroundWindow(existing);
      if (forwarded_callback.has_value()) {
        ForwardZeroTrustCallback(existing, *forwarded_callback);
      }
    }
    ::CloseHandle(instance_mutex);
    ::CoUninitialize();
    return EXIT_SUCCESS;
  }

  const bool start_hidden =
      HasArgument(command_line_arguments, "--background");

  project.set_dart_entrypoint_arguments(std::move(command_line_arguments));

  FlutterWindow window(project, start_hidden);
  Win32Window::Point origin(10, 10);
  // The caption is drawn by Flutter, so this is the usable client size.
  Win32Window::Size size(1200, 760);
  if (!window.Create(L"Usque", origin, size)) {
    ::CloseHandle(instance_mutex);
    return EXIT_FAILURE;
  }
  window.SetQuitOnClose(true);
  if (forwarded_callback.has_value()) {
    window.OfferZeroTrustCallback(*forwarded_callback);
  }

  ::MSG msg;
  while (::GetMessage(&msg, nullptr, 0, 0)) {
    ::TranslateMessage(&msg);
    ::DispatchMessage(&msg);
  }

  ::CoUninitialize();
  ::CloseHandle(instance_mutex);
  return EXIT_SUCCESS;
}
