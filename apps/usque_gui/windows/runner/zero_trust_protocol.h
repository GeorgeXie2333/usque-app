#ifndef RUNNER_ZERO_TRUST_PROTOCOL_H_
#define RUNNER_ZERO_TRUST_PROTOCOL_H_

#ifndef WIN32_LEAN_AND_MEAN
#define WIN32_LEAN_AND_MEAN
#endif
#include <windows.h>

#include <string>

constexpr wchar_t kUsqueWarpProtocolKey[] =
    L"Software\\Classes\\com.cloudflare.warp";

std::wstring CurrentExecutablePath();

bool WarpProtocolAssociationPointsAtExe(HKEY root, const wchar_t* protocol_key,
                                        const wchar_t* exe_path);

// Registers or removes an HKCU protocol association. Unregister deletes the
// key only when the open command already points at |exe_path|.
bool SetWarpProtocolAssociation(HKEY root, const wchar_t* protocol_key,
                                const wchar_t* exe_path, bool enabled);

bool IsCurrentUserWarpProtocolAssociated();
bool SetCurrentUserWarpProtocolAssociation(bool enabled);

#endif  // RUNNER_ZERO_TRUST_PROTOCOL_H_
