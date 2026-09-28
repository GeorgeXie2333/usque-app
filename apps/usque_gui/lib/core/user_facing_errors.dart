import 'dart:async';

import 'package:flutter/services.dart';

import '../services/engine_client.dart';
import 'app_strings.dart';

/// Translate structured failure codes, never backend messages or exception text.
/// Raw diagnostics may contain paths, addresses or credentials. The existing
/// diagnostic view and sanitized export provide structured technical evidence.
String userFacingError(AppStrings strings, Object error) {
  if (error is TimeoutException) return strings.get('operation_timeout');
  return switch (error) {
    EngineException() => userFacingFailure(
      strings,
      code: error.code,
      details: error.message,
    ),
    PlatformException() => userFacingFailure(
      strings,
      code: error.code,
      details: error.message,
    ),
    _ => strings.get('operation_failed'),
  };
}

String userFacingFailure(AppStrings strings, {String? code, String? details}) {
  final recovery = strings.windowsRecoveryError(code, details: details);
  if (recovery != null) return recovery;
  final key = switch (code) {
    'ENGINE_UNAVAILABLE' ||
    'ENGINE_IPC_UNAVAILABLE' ||
    'ENGINE_IPC_INVALID_RESPONSE' ||
    'ENGINE_CLOSED' ||
    'AGENT_UNREACHABLE' ||
    'VPN_SERVICE_UNAVAILABLE' => 'engine_unavailable',
    'ENGINE_REQUEST_TIMEOUT' || 'DIAGNOSTIC_TIMEOUT' => 'operation_timeout',
    'IDENTITY_SETUP_REQUIRED' ||
    'IDENTITY_INVALID' ||
    'AUTHENTICATION_FAILED' => 'diag_fix_refresh_or_replace_identity',
    'CONFIGURATION_INVALID' => 'diag_fix_review_configuration',
    'L4_UNSUPPORTED' => 'l4_unsupported',
    'NETWORK_SETTINGS_UNSUPPORTED' ||
    'PROXY_AUTH_UNSUPPORTED' => 'settings_unsupported',
    'PROXY_AUTH_APPLY_FAILED' => 'settings_failed',
    'PROXY_AUTH_SAVE_FAILED' => 'settings_save_failed',
    _ => null,
  };
  if (key != null) return strings.get(key);
  if (code == 'VPN_GATE_UNSUPPORTED') return strings.vpnGateUnsupported;
  // Only translate known diagnostic codes. Never turn an unknown code into
  // English prose or append it to the user's message.
  if (code != null) {
    final titleKey = 'diag_fail_$code';
    final title = strings.get(titleKey);
    if (title != titleKey) {
      return '$title\n${strings.get('diag_fix_default')}';
    }
  }
  return strings.get('operation_failed');
}
