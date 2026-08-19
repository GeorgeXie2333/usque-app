/// Access callback rules shared by the Windows/Dart Zero Trust enrollment UI.
///
/// Field checks match Android [ZeroTrustCallbackSession]: scheme
/// `com.cloudflare.warp`, host `<team>.cloudflareaccess.com`, path `/auth`,
/// no userinfo/port/fragment, and a single nonempty `token` query parameter.
class ZeroTrustCallbackSession {
  static const int maxCallbackChars = 64 * 1024;

  String? _expectedTeam;
  String? _pendingCallback;

  /// Returns the Access login URL after recording the expected team.
  String begin(String team) {
    final normalized = normalizeTeam(team);
    _expectedTeam = normalized;
    _pendingCallback = null;
    return 'https://$normalized.cloudflareaccess.com/warp';
  }

  bool accept(String callbackUri) {
    final expected = _expectedTeam;
    if (expected == null) return false;
    if (!isValidCallback(expected, callbackUri)) return false;
    _pendingCallback = callbackUri;
    _expectedTeam = null;
    return true;
  }

  String? consume() {
    final pending = _pendingCallback;
    _pendingCallback = null;
    return pending;
  }

  void cancel() {
    _expectedTeam = null;
    _pendingCallback = null;
  }

  /// DNS-label team used by Android `normalizeTeam`. Throws on an invalid name.
  static String normalizeTeam(String value) {
    final team = _tryNormalizeTeam(value);
    if (team == null) {
      throw ArgumentError.value(value, 'value', 'invalid Zero Trust team');
    }
    return team;
  }

  static String? tryNormalizeTeam(String value) => _tryNormalizeTeam(value);

  static bool isValidCallback(String team, String callbackUri) {
    final expected = _tryNormalizeTeam(team);
    if (expected == null) return false;
    return _matchesExpectedTeam(expected, callbackUri);
  }

  static String? _tryNormalizeTeam(String value) {
    final team = value.trim().toLowerCase();
    if (team.isEmpty || team.length > 63) return null;
    final first = team.codeUnitAt(0);
    final last = team.codeUnitAt(team.length - 1);
    if (!_isAsciiAlphanumeric(first) || !_isAsciiAlphanumeric(last)) {
      return null;
    }
    for (final unit in team.codeUnits) {
      final isLetter = unit >= 0x61 && unit <= 0x7a;
      final isDigit = unit >= 0x30 && unit <= 0x39;
      if (!isLetter && !isDigit && unit != 0x2d) return null;
    }
    return team;
  }

  static bool _matchesExpectedTeam(String expectedTeam, String callbackUri) {
    if (callbackUri.isEmpty || callbackUri.length > maxCallbackChars) {
      return false;
    }
    final callback = _parseRawUri(callbackUri);
    if (callback == null || callback.rawQuery == null) return false;
    final queryParts = callback.rawQuery!.split('&');
    if (queryParts.length != 1) return false;
    final separator = queryParts.single.indexOf('=');
    if (separator < 0) return false;
    final tokenName = queryParts.single.substring(0, separator);
    final tokenValue = queryParts.single.substring(separator + 1);
    final host = callback.host?.toLowerCase();
    return callback.scheme == 'com.cloudflare.warp' &&
        host == '$expectedTeam.cloudflareaccess.com' &&
        callback.rawPath == '/auth' &&
        callback.rawUserInfo == null &&
        callback.port == -1 &&
        callback.rawFragment == null &&
        tokenName == 'token' &&
        tokenValue.isNotEmpty;
  }

  static bool _isAsciiAlphanumeric(int unit) {
    final isLetter =
        (unit >= 0x41 && unit <= 0x5a) || (unit >= 0x61 && unit <= 0x7a);
    final isDigit = unit >= 0x30 && unit <= 0x39;
    return isLetter || isDigit;
  }
}

class _RawUri {
  const _RawUri({
    required this.scheme,
    required this.rawPath,
    required this.port,
    this.host,
    this.rawUserInfo,
    this.rawQuery,
    this.rawFragment,
  });

  final String scheme;
  final String? host;
  final String? rawUserInfo;
  final int port;
  final String rawPath;
  final String? rawQuery;
  final String? rawFragment;
}

/// Parses the same hierarchical fields Android reads from `java.net.URI`.
_RawUri? _parseRawUri(String input) {
  for (final unit in input.codeUnits) {
    if (unit <= 0x20 || unit == 0x7f) return null;
  }
  final schemeEnd = input.indexOf(':');
  if (schemeEnd <= 0) return null;
  final scheme = input.substring(0, schemeEnd).toLowerCase();
  if (!_isUriScheme(scheme)) return null;
  if (schemeEnd + 3 > input.length ||
      input[schemeEnd + 1] != '/' ||
      input[schemeEnd + 2] != '/') {
    return null;
  }

  var rest = input.substring(schemeEnd + 3);
  String? rawFragment;
  final hash = rest.indexOf('#');
  if (hash >= 0) {
    rawFragment = rest.substring(hash + 1);
    rest = rest.substring(0, hash);
  }
  String? rawQuery;
  final queryStart = rest.indexOf('?');
  if (queryStart >= 0) {
    rawQuery = rest.substring(queryStart + 1);
    rest = rest.substring(0, queryStart);
  }

  final pathStart = rest.indexOf('/');
  final authority = pathStart < 0 ? rest : rest.substring(0, pathStart);
  final rawPath = pathStart < 0 ? '' : rest.substring(pathStart);

  String? rawUserInfo;
  var hostPort = authority;
  final at = authority.indexOf('@');
  if (at >= 0) {
    rawUserInfo = authority.substring(0, at);
    hostPort = authority.substring(at + 1);
  }

  String? host;
  var port = -1;
  if (hostPort.isEmpty) {
    host = null;
  } else if (hostPort.startsWith('[')) {
    final close = hostPort.indexOf(']');
    if (close < 0) return null;
    host = hostPort.substring(1, close).toLowerCase();
    final after = hostPort.substring(close + 1);
    if (after.isNotEmpty) {
      if (!after.startsWith(':')) return null;
      final parsedPort = int.tryParse(after.substring(1));
      if (parsedPort == null || parsedPort < 0 || parsedPort > 65535) {
        return null;
      }
      port = parsedPort;
    }
  } else {
    final colon = hostPort.lastIndexOf(':');
    if (colon >= 0) {
      host = hostPort.substring(0, colon).toLowerCase();
      final parsedPort = int.tryParse(hostPort.substring(colon + 1));
      if (parsedPort == null || parsedPort < 0 || parsedPort > 65535) {
        return null;
      }
      port = parsedPort;
    } else {
      host = hostPort.toLowerCase();
    }
    if (host.isEmpty) host = null;
  }

  return _RawUri(
    scheme: scheme,
    host: host,
    rawUserInfo: rawUserInfo,
    port: port,
    rawPath: rawPath,
    rawQuery: rawQuery,
    rawFragment: rawFragment,
  );
}

bool _isUriScheme(String scheme) {
  if (scheme.isEmpty) return false;
  final first = scheme.codeUnitAt(0);
  if (first < 0x61 || first > 0x7a) return false;
  for (final unit in scheme.codeUnits) {
    final letter = unit >= 0x61 && unit <= 0x7a;
    final digit = unit >= 0x30 && unit <= 0x39;
    if (!letter && !digit && unit != 0x2b && unit != 0x2d && unit != 0x2e) {
      return false;
    }
  }
  return true;
}
