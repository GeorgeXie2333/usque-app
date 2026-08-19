#include <cstdio>
#include <cwchar>
#include <string>

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

}  // namespace

int main() {
  matchingCallbackIsConsumedOnlyOnce();
  callbackRequiresAnActiveSameTeamLogin();
  cancellationAndProcessReplacementDiscardState();
  malformedCallbacksAndTeamsAreRejected();
  unregisterDeletesOnlyAssociationPointingAtThisExe();
  if (g_failures != 0) {
    std::fprintf(stderr, "%d Zero Trust Windows tests failed\n", g_failures);
    return 1;
  }
  std::printf("zero_trust_windows_test: ok\n");
  return 0;
}
