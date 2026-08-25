#include <windows.h>

#include <chrono>
#include <cstdio>
#include <cwchar>
#include <string>
#include <thread>

#include "engine_ipc.h"
#include "zero_trust_callback.h"
#include "zero_trust_protocol.h"

namespace {

int g_failures = 0;

void Expect(bool condition, const char* name) {
  if (condition) return;
  std::fprintf(stderr, "FAIL %s\n", name);
  ++g_failures;
}

void matchingCallbackIsConsumedOnlyOnce() {
  ZeroTrustCallbackSession session;
  const auto login = session.Begin(" Example-Team ");
  Expect(login.has_value() &&
             *login == "https://example-team.cloudflareaccess.com/warp",
         "matchingCallbackIsConsumedOnlyOnce.login");
  const char* callback =
      "com.cloudflare.warp://example-team.cloudflareaccess.com/auth?token="
      "assertion";
  Expect(session.Accept(callback), "matchingCallbackIsConsumedOnlyOnce.accept");
  const auto first = session.Consume();
  Expect(first.has_value() && *first == callback,
         "matchingCallbackIsConsumedOnlyOnce.consume");
  Expect(!session.Consume().has_value(),
         "matchingCallbackIsConsumedOnlyOnce.secondConsume");
  Expect(!session.Accept(callback),
         "matchingCallbackIsConsumedOnlyOnce.secondAccept");
}

void callbackRequiresAnActiveSameTeamLogin() {
  ZeroTrustCallbackSession session;
  const char* callback =
      "com.cloudflare.warp://example-team.cloudflareaccess.com/auth?token="
      "assertion";
  Expect(!session.Accept(callback),
         "callbackRequiresAnActiveSameTeamLogin.noLogin");
  Expect(session.Begin("other-team").has_value(),
         "callbackRequiresAnActiveSameTeamLogin.begin");
  Expect(!session.Accept(callback),
         "callbackRequiresAnActiveSameTeamLogin.otherTeam");
  Expect(!session.Consume().has_value(),
         "callbackRequiresAnActiveSameTeamLogin.consume");
}

void cancellationAndProcessReplacementDiscardState() {
  ZeroTrustCallbackSession session;
  Expect(session.Begin("example-team").has_value(),
         "cancellationAndProcessReplacementDiscardState.begin");
  session.Cancel();
  Expect(!session.Accept(
             "com.cloudflare.warp://example-team.cloudflareaccess.com/auth?"
             "token=assertion"),
         "cancellationAndProcessReplacementDiscardState.afterCancel");
  ZeroTrustCallbackSession replacement;
  Expect(!replacement.Consume().has_value(),
         "cancellationAndProcessReplacementDiscardState.replacement");
}

void malformedCallbacksAndTeamsAreRejected() {
  Expect(!NormalizeZeroTrustTeam("team.example").has_value(),
         "malformedCallbacksAndTeamsAreRejected.team");
  const char* invalid_callbacks[] = {
      "https://example-team.cloudflareaccess.com/auth?token=x",
      "com.cloudflare.warp://example-team.cloudflareaccess.com/warp?token=x",
      "com.cloudflare.warp://example-team.cloudflareaccess.com/auth?token=x&"
      "token=y",
      "com.cloudflare.warp://example-team.cloudflareaccess.com/auth?state=x",
      "com.cloudflare.warp://other.cloudflareaccess.com/auth?token=x",
      "com.cloudflare.warp://user@example-team.cloudflareaccess.com/auth?token=x",
      "com.cloudflare.warp://example-team.cloudflareaccess.com:443/auth?token=x",
      "com.cloudflare.warp://example-team.cloudflareaccess.com/auth?token=x#"
      "fragment",
      "com.cloudflare.warp://example-team.cloudflareaccess.com/auth",
      "com.cloudflare.warp://example-team.cloudflareaccess.com/auth?token=",
  };
  for (const char* callback : invalid_callbacks) {
    ZeroTrustCallbackSession session;
    session.Begin("example-team");
    if (session.Accept(callback)) {
      std::fprintf(stderr, "FAIL malformedCallbacksAndTeamsAreRejected: %s\n",
                   callback);
      ++g_failures;
    }
  }
  const char* good =
      "com.cloudflare.warp://example-team.cloudflareaccess.com/auth?token="
      "assertion";
  Expect(IsValidZeroTrustCallback("example-team", good),
         "malformedCallbacksAndTeamsAreRejected.good");
}

void unregisterDeletesOnlyAssociationPointingAtThisExe() {
  wchar_t key[160]{};
  swprintf_s(key, L"Software\\io.github.georgexie2333\\Usque\\zt-test-%lu",
             ::GetCurrentProcessId());
  const wchar_t* ours = L"C:\\Usque\\usque.exe";
  const wchar_t* other = L"C:\\Program Files\\Cloudflare\\Cloudflare WARP\\"
                         L"Cloudflare WARP.exe";
  Expect(SetWarpProtocolAssociation(HKEY_CURRENT_USER, key, other, true),
         "unregisterDeletesOnlyAssociationPointingAtThisExe.registerOther");
  Expect(WarpProtocolAssociationPointsAtExe(HKEY_CURRENT_USER, key, other),
         "unregisterDeletesOnlyAssociationPointingAtThisExe.otherOwned");
  Expect(!WarpProtocolAssociationPointsAtExe(HKEY_CURRENT_USER, key, ours),
         "unregisterDeletesOnlyAssociationPointingAtThisExe.oursNotOwner");
  Expect(SetWarpProtocolAssociation(HKEY_CURRENT_USER, key, ours, false),
         "unregisterDeletesOnlyAssociationPointingAtThisExe.leaveOther");
  Expect(WarpProtocolAssociationPointsAtExe(HKEY_CURRENT_USER, key, other),
         "unregisterDeletesOnlyAssociationPointingAtThisExe.otherRemains");

  Expect(SetWarpProtocolAssociation(HKEY_CURRENT_USER, key, ours, true),
         "unregisterDeletesOnlyAssociationPointingAtThisExe.registerOurs");
  Expect(WarpProtocolAssociationPointsAtExe(HKEY_CURRENT_USER, key, ours),
         "unregisterDeletesOnlyAssociationPointingAtThisExe.oursOwned");
  Expect(SetWarpProtocolAssociation(HKEY_CURRENT_USER, key, ours, false),
         "unregisterDeletesOnlyAssociationPointingAtThisExe.deleteOurs");
  Expect(!WarpProtocolAssociationPointsAtExe(HKEY_CURRENT_USER, key, ours),
         "unregisterDeletesOnlyAssociationPointingAtThisExe.oursGone");
  Expect(!WarpProtocolAssociationPointsAtExe(HKEY_CURRENT_USER, key, other),
         "unregisterDeletesOnlyAssociationPointingAtThisExe.otherGoneToo");
  ::RegDeleteTreeW(HKEY_CURRENT_USER, key);
}

std::string TestPipeName(const char* suffix) {
  return R"(\\.\pipe\io.github.georgexie2333.usque.engine.v1-ui-test-)" +
         std::to_string(::GetCurrentProcessId()) + "-" + suffix;
}

std::wstring Wide(const std::string& value) {
  return std::wstring(value.begin(), value.end());
}

void enginePipeReadinessRetriesAInitiallyMissingPipe() {
  const std::string pipe_name = TestPipeName("delayed");
  const std::wstring pipe_name_wide = Wide(pipe_name);
  HANDLE server = INVALID_HANDLE_VALUE;
  std::thread creator([&]() {
    std::this_thread::sleep_for(std::chrono::milliseconds(150));
    server = ::CreateNamedPipeW(
        pipe_name_wide.c_str(), PIPE_ACCESS_DUPLEX,
        PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT, 1, 4096, 4096, 0,
        nullptr);
    if (server != INVALID_HANDLE_VALUE) {
      const BOOL connected = ::ConnectNamedPipe(server, nullptr);
      if (!connected && ::GetLastError() != ERROR_PIPE_CONNECTED) {
        ::CloseHandle(server);
        server = INVALID_HANDLE_VALUE;
      }
    }
  });

  const auto started = std::chrono::steady_clock::now();
  const std::string error = WaitForEnginePipe(pipe_name, 2000);
  const auto elapsed = std::chrono::steady_clock::now() - started;
  Expect(error.empty(),
         "enginePipeReadinessRetriesAInitiallyMissingPipe.ready");
  Expect(elapsed >= std::chrono::milliseconds(100),
         "enginePipeReadinessRetriesAInitiallyMissingPipe.waited");

  HANDLE client = INVALID_HANDLE_VALUE;
  const auto connect_deadline =
      std::chrono::steady_clock::now() + std::chrono::seconds(2);
  do {
    client = ::CreateFileW(pipe_name_wide.c_str(),
                           GENERIC_READ | GENERIC_WRITE, 0, nullptr,
                           OPEN_EXISTING, FILE_ATTRIBUTE_NORMAL, nullptr);
    if (client != INVALID_HANDLE_VALUE) break;
    std::this_thread::sleep_for(std::chrono::milliseconds(25));
  } while (std::chrono::steady_clock::now() < connect_deadline);
  Expect(client != INVALID_HANDLE_VALUE,
         "enginePipeReadinessRetriesAInitiallyMissingPipe.connect");
  if (client != INVALID_HANDLE_VALUE) ::CloseHandle(client);
  creator.join();
  if (server != INVALID_HANDLE_VALUE) ::CloseHandle(server);
}

void enginePipeReadinessUsesAnOverallDeadline() {
  const std::string pipe_name = TestPipeName("absent");
  const auto started = std::chrono::steady_clock::now();
  const std::string error = WaitForEnginePipe(pipe_name, 150);
  const auto elapsed = std::chrono::steady_clock::now() - started;
  Expect(error.find("before timeout") != std::string::npos,
         "enginePipeReadinessUsesAnOverallDeadline.error");
  Expect(elapsed >= std::chrono::milliseconds(100),
         "enginePipeReadinessUsesAnOverallDeadline.waited");
  Expect(elapsed < std::chrono::seconds(2),
         "enginePipeReadinessUsesAnOverallDeadline.bounded");
}

}  // namespace

int main() {
  matchingCallbackIsConsumedOnlyOnce();
  callbackRequiresAnActiveSameTeamLogin();
  cancellationAndProcessReplacementDiscardState();
  malformedCallbacksAndTeamsAreRejected();
  unregisterDeletesOnlyAssociationPointingAtThisExe();
  enginePipeReadinessRetriesAInitiallyMissingPipe();
  enginePipeReadinessUsesAnOverallDeadline();
  if (g_failures != 0) {
    std::fprintf(stderr, "%d Zero Trust Windows tests failed\n", g_failures);
    return 1;
  }
  std::printf("zero_trust_windows_test: ok\n");
  return 0;
}
