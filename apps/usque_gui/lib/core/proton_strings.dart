import 'app_strings.dart';
import 'l10n/proton.dart';

extension ProtonStrings on AppStrings {
  String proton(String key) {
    final catalog = kProtonCatalogs[catalogId] ?? kProtonEn;
    return catalog[key] ?? kProtonEn[key] ?? catalog['failed']!;
  }
}

String? protonConnectionFailure(AppStrings strings, String? code) {
  final key = switch (code) {
    'PROTON_AUTHENTICATION_FAILED' => 'connection_authentication',
    'PROTON_CERTIFICATE_FAILED' => 'connection_certificate',
    'PROTON_SERVER_RESTRICTED' => 'connection_restricted',
    'PROTON_SESSION_LIMIT' => 'connection_sessions',
    'PROTON_KEY_REUSED' => 'connection_key',
    'PROTON_POLICY_RESTRICTED' => 'connection_policy',
    'PROTON_SERVER_FAILED' => 'connection_server',
    'PROTON_RATE_LIMITED' => 'rate_limited',
    'PROTON_API_FAILED' => 'connection_api',
    'PROTON_PROTOCOL_FAILED' => 'connection_protocol',
    'PROTON_TRANSPORT_FAILED' => 'connection_transport',
    'PROTON_RECONNECT_REQUIRED' => 'connection_reconnect',
    _ => null,
  };
  return key == null ? null : strings.proton(key);
}

String? protonGateFailure(AppStrings strings, String? reason) =>
    protonConnectionFailure(strings, switch (reason) {
      'proton_authentication' => 'PROTON_AUTHENTICATION_FAILED',
      'proton_certificate' => 'PROTON_CERTIFICATE_FAILED',
      'proton_restricted' => 'PROTON_SERVER_RESTRICTED',
      'proton_max_sessions' => 'PROTON_SESSION_LIMIT',
      'proton_key_reused' => 'PROTON_KEY_REUSED',
      'proton_policy' => 'PROTON_POLICY_RESTRICTED',
      'proton_server' => 'PROTON_SERVER_FAILED',
      'proton_rate_limited' => 'PROTON_RATE_LIMITED',
      'proton_api' => 'PROTON_API_FAILED',
      'proton_protocol' => 'PROTON_PROTOCOL_FAILED',
      'proton_transport' => 'PROTON_TRANSPORT_FAILED',
      'proton_reconnect' => 'PROTON_RECONNECT_REQUIRED',
      _ => null,
    });
