#include "zero_trust_callback.h"

#include <utility>

namespace {

char AsciiLower(char value) {
  if (value >= 'A' && value <= 'Z') {
    return static_cast<char>(value - 'A' + 'a');
  }
  return value;
}

std::string AsciiLowerCopy(std::string_view value) {
  std::string lowered(value);
  for (char& unit : lowered) {
    unit = AsciiLower(unit);
  }
  return lowered;
}

bool IsAsciiAlphanumeric(char value) {
  return (value >= '0' && value <= '9') || (value >= 'A' && value <= 'Z') ||
         (value >= 'a' && value <= 'z');
}

bool IsUriScheme(std::string_view scheme) {
  if (scheme.empty() || scheme.front() < 'a' || scheme.front() > 'z') {
    return false;
  }
  for (char unit : scheme) {
    if ((unit >= 'a' && unit <= 'z') || (unit >= '0' && unit <= '9') ||
        unit == '+' || unit == '-' || unit == '.') {
      continue;
    }
    return false;
  }
  return true;
}

bool ParsePort(std::string_view text, int* port) {
  if (text.empty() || text.size() > 5) return false;
  int value = 0;
  for (char unit : text) {
    if (unit < '0' || unit > '9') return false;
    value = value * 10 + (unit - '0');
    if (value > 65535) return false;
  }
  *port = value;
  return true;
}

struct RawUri {
  std::string scheme;
  std::optional<std::string> host;
  std::optional<std::string> raw_user_info;
  int port = -1;
  std::string raw_path;
  std::optional<std::string> raw_query;
  std::optional<std::string> raw_fragment;
};

std::optional<RawUri> ParseRawUri(std::string_view input) {
  for (unsigned char unit : input) {
    if (unit <= 0x20 || unit == 0x7f) return std::nullopt;
  }
  const auto scheme_end = input.find(':');
  if (scheme_end == std::string_view::npos || scheme_end == 0) {
    return std::nullopt;
  }
  RawUri parsed;
  parsed.scheme = AsciiLowerCopy(input.substr(0, scheme_end));
  if (!IsUriScheme(parsed.scheme)) return std::nullopt;
  if (scheme_end + 3 > input.size() || input[scheme_end + 1] != '/' ||
      input[scheme_end + 2] != '/') {
    return std::nullopt;
  }

  std::string_view rest = input.substr(scheme_end + 3);
  const auto hash = rest.find('#');
  if (hash != std::string_view::npos) {
    parsed.raw_fragment = std::string(rest.substr(hash + 1));
    rest = rest.substr(0, hash);
  }
  const auto query = rest.find('?');
  if (query != std::string_view::npos) {
    parsed.raw_query = std::string(rest.substr(query + 1));
    rest = rest.substr(0, query);
  }

  const auto path_start = rest.find('/');
  const std::string_view authority =
      path_start == std::string_view::npos ? rest : rest.substr(0, path_start);
  parsed.raw_path = path_start == std::string_view::npos
                        ? std::string()
                        : std::string(rest.substr(path_start));

  std::string_view host_port = authority;
  const auto at = authority.find('@');
  if (at != std::string_view::npos) {
    parsed.raw_user_info = std::string(authority.substr(0, at));
    host_port = authority.substr(at + 1);
  }

  if (host_port.empty()) {
    parsed.host.reset();
  } else if (host_port.front() == '[') {
    const auto close = host_port.find(']');
    if (close == std::string_view::npos) return std::nullopt;
    parsed.host = AsciiLowerCopy(host_port.substr(1, close - 1));
    const auto after = host_port.substr(close + 1);
    if (!after.empty()) {
      if (after.front() != ':') return std::nullopt;
      int port = -1;
      if (!ParsePort(after.substr(1), &port)) return std::nullopt;
      parsed.port = port;
    }
  } else {
    const auto colon = host_port.rfind(':');
    if (colon != std::string_view::npos) {
      parsed.host = AsciiLowerCopy(host_port.substr(0, colon));
      int port = -1;
      if (!ParsePort(host_port.substr(colon + 1), &port)) return std::nullopt;
      parsed.port = port;
    } else {
      parsed.host = AsciiLowerCopy(host_port);
    }
    if (parsed.host.has_value() && parsed.host->empty()) {
      parsed.host.reset();
    }
  }
  return parsed;
}

bool MatchesExpectedTeam(std::string_view expected_team,
                         std::string_view callback_uri) {
  if (callback_uri.empty() ||
      callback_uri.size() > static_cast<size_t>(kMaxZeroTrustCallbackChars)) {
    return false;
  }
  const auto callback = ParseRawUri(callback_uri);
  if (!callback.has_value() || !callback->raw_query.has_value()) {
    return false;
  }
  const std::string& query = *callback->raw_query;
  if (query.find('&') != std::string::npos) return false;
  const auto separator = query.find('=');
  if (separator == std::string::npos) return false;
  const std::string token_name = query.substr(0, separator);
  const std::string token_value = query.substr(separator + 1);
  const std::string expected_host =
      std::string(expected_team) + ".cloudflareaccess.com";
  const std::string host =
      callback->host.has_value() ? AsciiLowerCopy(*callback->host) : "";
  return callback->scheme == "com.cloudflare.warp" && host == expected_host &&
         callback->raw_path == "/auth" && !callback->raw_user_info.has_value() &&
         callback->port == -1 && !callback->raw_fragment.has_value() &&
         token_name == "token" && !token_value.empty();
}

}  // namespace

std::optional<std::string> NormalizeZeroTrustTeam(std::string_view value) {
  size_t start = 0;
  size_t end = value.size();
  while (start < end &&
         (value[start] == ' ' || value[start] == '\t' || value[start] == '\r' ||
          value[start] == '\n')) {
    ++start;
  }
  while (end > start && (value[end - 1] == ' ' || value[end - 1] == '\t' ||
                         value[end - 1] == '\r' || value[end - 1] == '\n')) {
    --end;
  }
  if (end <= start || end - start > 63) return std::nullopt;
  std::string team = AsciiLowerCopy(value.substr(start, end - start));
  if (!IsAsciiAlphanumeric(team.front()) || !IsAsciiAlphanumeric(team.back())) {
    return std::nullopt;
  }
  for (char unit : team) {
    if (!IsAsciiAlphanumeric(unit) && unit != '-') return std::nullopt;
  }
  return team;
}

bool IsValidZeroTrustCallback(std::string_view team,
                              std::string_view callback_uri) {
  const auto expected = NormalizeZeroTrustTeam(team);
  if (!expected.has_value()) return false;
  return MatchesExpectedTeam(*expected, callback_uri);
}

std::optional<std::string> ZeroTrustCallbackSession::Begin(
    std::string_view team) {
  const auto normalized = NormalizeZeroTrustTeam(team);
  if (!normalized.has_value()) return std::nullopt;
  expected_team_ = normalized;
  pending_callback_.reset();
  return "https://" + *normalized + ".cloudflareaccess.com/warp";
}

bool ZeroTrustCallbackSession::Accept(std::string_view callback_uri) {
  if (!expected_team_.has_value()) return false;
  if (!MatchesExpectedTeam(*expected_team_, callback_uri)) return false;
  pending_callback_ = std::string(callback_uri);
  expected_team_.reset();
  return true;
}

std::optional<std::string> ZeroTrustCallbackSession::Consume() {
  std::optional<std::string> pending = std::move(pending_callback_);
  pending_callback_.reset();
  return pending;
}

void ZeroTrustCallbackSession::Cancel() {
  expected_team_.reset();
  pending_callback_.reset();
}

std::optional<std::string> ExtractZeroTrustCallbackArgument(
    const std::vector<std::string>& arguments) {
  for (const std::string& argument : arguments) {
    const auto colon = argument.find(':');
    if (colon == std::string::npos || colon == 0) continue;
    if (AsciiLowerCopy(argument.substr(0, colon)) == "com.cloudflare.warp") {
      return argument;
    }
  }
  return std::nullopt;
}

bool ForwardZeroTrustCallback(HWND target, std::string_view callback_uri) {
  if (target == nullptr || callback_uri.empty() ||
      callback_uri.size() >
          static_cast<size_t>(kMaxZeroTrustCallbackChars)) {
    return false;
  }
  COPYDATASTRUCT data{};
  data.dwData = kZeroTrustCallbackCopyData;
  data.cbData = static_cast<DWORD>(callback_uri.size());
  data.lpData = const_cast<char*>(callback_uri.data());
  DWORD_PTR accepted = 0;
  return ::SendMessageTimeoutW(target, WM_COPYDATA, 0,
                               reinterpret_cast<LPARAM>(&data),
                               SMTO_ABORTIFHUNG, 5000, &accepted) != 0;
}
