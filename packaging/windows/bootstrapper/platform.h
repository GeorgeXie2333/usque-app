#pragma once

#include <windows.h>
#include "process_output.h"

#include <string>
#include <vector>

namespace usque::setup {

std::wstring ModulePath();
std::wstring RegistryString(HKEY root, const wchar_t* key, const wchar_t* name);
bool UnelevatedInteractiveUser();
bool SameVerifiedSigner(const std::wstring& file);
bool Launch(const std::wstring& file, const std::wstring& arguments, DWORD* error);
ChildResult RunInstallerOptions(const std::wstring& file, bool query, bool desktop, int startup);
std::string ReadLicense();
std::wstring SelectFolder(HWND owner, const std::wstring& initial);
HRESULT SaveDetails(HWND owner, const std::string& contents);

}  // namespace usque::setup
