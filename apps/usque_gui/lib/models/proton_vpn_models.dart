import 'dart:convert';
import 'dart:typed_data';

/// Own mutable bytes before clearing or passing on a sensitive native reply.
/// Flutter platform messages may borrow an unmodifiable engine buffer. That
/// source can only be released; writable transferred sources are cleared here.
Uint8List takeProtonSensitiveBytes(Uint8List source) {
  final owned = Uint8List.fromList(source);
  try {
    source.fillRange(0, source.length, 0);
  } on UnsupportedError {
    // The platform owns this read-only buffer; do not treat it as our storage.
  }
  return owned;
}

String encodeProtonVpnCommand(Map<String, Object?> command) {
  const keys = {
    'action',
    'job_id',
    'profile_id',
    'challenge_id',
    'node_id',
    'name',
    'offset',
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
    'servers',
    'choose_server',
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
    : sensitiveHandoff = sensitiveHandoff == null
          ? Uint8List(0)
          : takeProtonSensitiveBytes(sensitiveHandoff);

  final Map<Object?, Object?> metadata;
  final Uint8List sensitiveHandoff;

  void clearHandoff() =>
      sensitiveHandoff.fillRange(0, sensitiveHandoff.length, 0);

  @override
  String toString() => 'ProtonVpnResult([redacted])';
}
