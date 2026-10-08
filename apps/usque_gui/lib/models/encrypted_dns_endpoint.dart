import 'dart:io';

const cloudflareDohUrl = 'https://cloudflare-dns.com/dns-query';
const cloudflareDotServer = 'one.one.one.one';
const cloudflareDnsBootstrapIps = <String>[
  '1.1.1.1',
  '1.0.0.1',
  '2606:4700:4700::1111',
  '2606:4700:4700::1001',
];

bool validDirectDnsName(String value) {
  if (value.isEmpty ||
      value.runes.length > 253 ||
      value.trim() != value ||
      InternetAddress.tryParse(value) != null) {
    return false;
  }
  if (value.runes.any((rune) => rune <= 32 || rune == 127) ||
      RegExp(r'[:/\\?#@%*\[\]\s]').hasMatch(value)) {
    return false;
  }
  return value
      .split('.')
      .every(
        (label) =>
            label.isNotEmpty &&
            label.runes.length <= 63 &&
            !label.startsWith('-') &&
            !label.endsWith('-') &&
            label.runes.every(
              (rune) =>
                  rune > 127 ||
                  RegExp(r'[a-zA-Z0-9-]').hasMatch(String.fromCharCode(rune)),
            ),
      );
}

bool validDirectDnsPath(String value) =>
    value.startsWith('/') &&
    !value.startsWith('//') &&
    value.length <= 256 &&
    !value.contains('://') &&
    !RegExp(r'[?#\\\s]').hasMatch(value) &&
    value.codeUnits.every((unit) => unit >= 33 && unit <= 126);

/// UI adapter for the existing persisted server_name, port and doh_path fields.
/// Keep the raw path rather than normalizing/decoding it through Uri, so legacy
/// percent escapes, repeated slashes and custom paths retain their meaning.
class DohEndpoint {
  const DohEndpoint(this.serverName, this.port, this.path);

  final String serverName;
  final int port;
  final String path;

  String get url =>
      'https://$serverName'
      '${port == 0 || port == 443 ? '' : ':$port'}'
      '${path.isEmpty ? '/dns-query' : path}';

  static DohEndpoint? tryParse(String value) {
    final match = RegExp(
      r'^https://([^/:?#]+)(?::([0-9]+))?(/[^?#]*)?$',
      caseSensitive: false,
    ).firstMatch(value);
    if (match == null || match.end != value.length) return null;
    final name = match[1]!;
    final port = match[2] == null ? 443 : int.tryParse(match[2]!);
    final path = match[3] ?? '/dns-query';
    if (!validDirectDnsName(name) ||
        port == null ||
        port < 1 ||
        port > 65535 ||
        !validDirectDnsPath(path)) {
      return null;
    }
    return DohEndpoint(name, port, path);
  }
}
