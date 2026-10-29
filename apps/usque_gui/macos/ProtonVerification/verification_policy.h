#ifndef USQUE_PROTON_VERIFICATION_POLICY_H_
#define USQUE_PROTON_VERIFICATION_POLICY_H_

#include <algorithm>
#include <initializer_list>
#include <string_view>

namespace usque::proton::macos {

// Adapter boundaries are kept independent of CEF so they can be checked
// without launching a browser or passing real credentials to one.
inline bool OfficialResource(std::string_view url) {
  if (url.empty() || url.size() > 16384 ||
      !std::all_of(url.begin(), url.end(), [](unsigned char value) {
        return value >= 0x21 && value <= 0x7e && value != '\\';
      })) return false;
  for (const std::string_view origin : {"https://verify.proton.me",
      "https://verify-api.proton.me", "https://account.proton.me"}) {
    if (url.substr(0, origin.size()) == origin &&
        (url.size() == origin.size() || url[origin.size()] == '/' ||
         url[origin.size()] == '?' || url[origin.size()] == '#')) return true;
  }
  return false;
}

inline bool ReadyProofBound(bool current_browser, bool main_frame, unsigned stage,
    std::string_view candidate, std::string_view expected) {
  return current_browser && main_frame && stage == 2 && !expected.empty() && candidate == expected;
}

inline bool MainDocumentHttpFailed(bool main_frame, int status) {
  return main_frame && status >= 400;
}

inline bool ProxyCredentialsAllowed(bool finished, bool is_proxy,
    std::string_view host, int port, std::string_view realm, std::string_view scheme,
    unsigned expected_port, std::string_view expected_realm) {
  return !finished && is_proxy && expected_port > 0 && expected_port <= 65535 &&
      !expected_realm.empty() && host == "127.0.0.1" &&
      port == static_cast<int>(expected_port) && realm == expected_realm && scheme == "basic";
}

}  // namespace usque::proton::macos

#endif  // USQUE_PROTON_VERIFICATION_POLICY_H_
