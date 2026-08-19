#ifndef RUNNER_ZERO_TRUST_CALLBACK_H_
#define RUNNER_ZERO_TRUST_CALLBACK_H_

#include <optional>
#include <string>
#include <string_view>
#include <vector>

#ifndef WIN32_LEAN_AND_MEAN
#define WIN32_LEAN_AND_MEAN
#endif
#include <windows.h>

// Matches Android ZeroTrustCallbackSession.MAX_CALLBACK_CHARS.
constexpr int kMaxZeroTrustCallbackChars = 64 * 1024;
constexpr ULONG_PTR kZeroTrustCallbackCopyData = 0x55535101ul;

std::optional<std::string> NormalizeZeroTrustTeam(std::string_view value);
bool IsValidZeroTrustCallback(std::string_view team,
                              std::string_view callback_uri);

// Process-local gate matching Android ZeroTrustCallbackSession.
class ZeroTrustCallbackSession {
 public:
  std::optional<std::string> Begin(std::string_view team);
  bool Accept(std::string_view callback_uri);
  std::optional<std::string> Consume();
  void Cancel();

 private:
  std::optional<std::string> expected_team_;
  std::optional<std::string> pending_callback_;
};

std::optional<std::string> ExtractZeroTrustCallbackArgument(
    const std::vector<std::string>& arguments);
bool ForwardZeroTrustCallback(HWND target, std::string_view callback_uri);

#endif  // RUNNER_ZERO_TRUST_CALLBACK_H_
