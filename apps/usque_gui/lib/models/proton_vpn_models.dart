import 'dart:convert';
import 'dart:typed_data';

String encodeProtonVpnCommand(Map<String, Object?> command) {
  const keys = {
    'action',
    'job_id',
    'profile_id',
    'challenge_id',
    'node_id',
    'name',
  };
  const actions = {
    'guest',
    'login',
    'get',
    'cancel',
    'totp',
    'handoff',
    'verify',
    'select_node',
    'refresh',
    'logout',
    'forget',
    'list',
  };
  if (command.keys.any((key) => !keys.contains(key)) ||
      !actions.contains(command['action'])) {
    throw const FormatException('Invalid Proton command metadata');
  }
  final encoded = jsonEncode(command);
  if (utf8.encode(encoded).length > 4096) {
    throw const FormatException('Proton command exceeds bound');
  }
  return encoded;
}

/// An explicit native response. Handoff bytes never become settings or events.
class ProtonVpnResult {
  ProtonVpnResult({required this.metadata, Uint8List? sensitiveHandoff})
    : sensitiveHandoff = sensitiveHandoff ?? Uint8List(0);

  final Map<Object?, Object?> metadata;
  final Uint8List sensitiveHandoff;

  void clearHandoff() =>
      sensitiveHandoff.fillRange(0, sensitiveHandoff.length, 0);

  @override
  String toString() => 'ProtonVpnResult([redacted])';
}
