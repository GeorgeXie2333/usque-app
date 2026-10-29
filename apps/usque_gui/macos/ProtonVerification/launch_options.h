#ifndef USQUE_PROTON_VERIFICATION_LAUNCH_OPTIONS_H_
#define USQUE_PROTON_VERIFICATION_LAUNCH_OPTIONS_H_

#include <string_view>

namespace usque::proton::macos {

enum class LaunchMode { kInvalid, kStdio, kSmoke, kInteractiveDisabled };

struct LaunchOptions {
  LaunchMode mode = LaunchMode::kInvalid;
  bool no_interactive = true;
};

// Keep this check independent of Cocoa/CEF so automated tests can prove that a
// browser-free invocation is rejected before loading the browser framework.
inline LaunchOptions ParseLaunchOptions(int argc, const char* const* argv) {
  if (argc < 2 || argc > 3 || !argv || !argv[1]) return {};
  const std::string_view command(argv[1]);
  if (command == "--stdio") {
    if (argc == 2) return {LaunchMode::kStdio, false};
    if (argv[2] && std::string_view(argv[2]) == "--no-interactive") {
      return {LaunchMode::kStdio, true};
    }
  } else if (command == "--smoke-test") {
    if (argc == 2 || (argv[2] && std::string_view(argv[2]) == "--no-interactive")) {
      return {LaunchMode::kInteractiveDisabled, true};
    }
    if (argv[2] && std::string_view(argv[2]) == "--allow-interactive") {
      return {LaunchMode::kSmoke, false};
    }
  }
  return {};
}

}  // namespace usque::proton::macos

#endif  // USQUE_PROTON_VERIFICATION_LAUNCH_OPTIONS_H_
