import 'dart:io';

/// Offline syntax checks; core performs authoritative IDNA normalization.
class BypassTargets {
  const BypassTargets(this.cidrs, this.domains, this.domainLines);

  final List<String> cidrs;
  final List<String> domains;
  final List<int> domainLines;

  static BypassTargets parse(String text) {
    final cidrs = <String>{};
    final domains = <String>{};
    final domainLines = <int>[];
    final lines = text.split('\n');
    for (var index = 0; index < lines.length; index++) {
      final value = lines[index].trim();
      if (value.isEmpty) continue;
      final parts = value.split('/');
      final address = InternetAddress.tryParse(parts.first);
      if (address != null && !value.contains('%')) {
        final bits = address.rawAddress.length * 8;
        final prefix = parts.length == 1 ? bits : int.tryParse(parts.last);
        if (parts.length > 2 || prefix == null || prefix < 0 || prefix > bits) {
          throw BypassTargetError(index + 1, 'invalid_cidr');
        }
        final bytes = address.rawAddress;
        for (var byte = 0; byte < bytes.length; byte++) {
          final keep = (prefix - byte * 8).clamp(0, 8);
          bytes[byte] &= (0xff << (8 - keep)) & 0xff;
        }
        cidrs.add('${InternetAddress.fromRawAddress(bytes).address}/$prefix');
      } else {
        final domain = value.toLowerCase().replaceFirst(RegExp(r'\.$'), '');
        if (domain.isEmpty ||
            domain.length > 253 ||
            RegExp(r'[\s\x00-\x20:/\\?#@%*\[\]]').hasMatch(domain) ||
            RegExp(r'^[\d.]+$').hasMatch(domain) ||
            domain
                .split('.')
                .any(
                  (label) =>
                      label.isEmpty ||
                      label.length > 63 ||
                      label.startsWith('-') ||
                      label.endsWith('-') ||
                      label.runes.any(
                        (rune) =>
                            rune < 128 &&
                            !RegExp(
                              r'[a-z0-9-]',
                            ).hasMatch(String.fromCharCode(rune)),
                      ),
                )) {
          throw BypassTargetError(index + 1, 'invalid_dns_name');
        }
        if (domains.add(domain)) domainLines.add(index + 1);
      }
      if (cidrs.length > 256 || domains.length > 256) {
        throw BypassTargetError(index + 1, 'bypass_limit');
      }
    }
    return BypassTargets(cidrs.toList(), domains.toList(), domainLines);
  }
}

class BypassTargetError implements Exception {
  const BypassTargetError(this.line, this.messageKey);
  final int line;
  final String messageKey;
}
