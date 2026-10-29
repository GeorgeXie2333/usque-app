import 'dart:async';

import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';

import '../services/engine_client.dart';
import 'app_strings.dart';
import 'proton_strings.dart';

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

String userFacingFailure(
  AppStrings strings, {
  String? code,
  String? details,
  String? protonContext,
}) {
  final proton = protonConnectionFailure(strings, code);
  if (proton != null) {
    final context = protonContext;
    if (context != null &&
        RegExp(
          r'^proton api [0-9]{1,10} http [1-5][0-9]{2}$',
        ).hasMatch(context)) {
      final parts = context.split(' ');
      return '$proton\nProton API ${parts[2]} · HTTP ${parts[4]}';
    }
    if (context != null &&
        RegExp(r'^proton agent [0-9]{1,10}$').hasMatch(context)) {
      return '$proton\nProton agent ${context.split(' ').last}';
    }
    return proton;
  }
  final recovery = strings.windowsRecoveryError(code, details: details);
  if (recovery != null) return recovery;
  final key = switch (code) {
    'MACOS_INSTALL_REQUIRED' => 'macos_install_required',
    'MACOS_LOGIN_APPROVAL_REQUIRED' => 'macos_login_approval',
    'MACOS_LOGIN_ITEM_FAILED' => 'macos_login_failed',
    'SYSTEM_PROXY_PERMISSION_CANCELLED' || 'SYSTEM_PROXY_PERMISSION_REQUIRED'
        when defaultTargetPlatform == TargetPlatform.macOS =>
      'macos_proxy_permission',
    'SYSTEM_PROXY_RECOVERY_PENDING'
        when defaultTargetPlatform == TargetPlatform.macOS =>
      'macos_proxy_recovery',
    'SYSTEM_PROXY_CONFLICT' ||
    'SYSTEM_PROXY_NO_NETWORK_SERVICE' ||
    'SYSTEM_PROXY_UNAVAILABLE'
        when defaultTargetPlatform == TargetPlatform.macOS =>
      'macos_proxy_failed',
    'SECURE_STORAGE_FAILED'
        when defaultTargetPlatform == TargetPlatform.linux =>
      'linux_keyring_unavailable',
    'TRAY_UNAVAILABLE' => 'linux_tray_unavailable',
    'LINUX_PLATFORM_FAILED' => 'linux_platform_failed',
    'LINUX_SERVICE_DEPENDENCIES_MISSING' => 'linux_service_dependencies',
    'ENGINE_UNAVAILABLE' ||
    'ENGINE_IPC_UNAVAILABLE' ||
    'ENGINE_IPC_INVALID_RESPONSE' ||
    'ENGINE_CLOSED' ||
    'AGENT_UNREACHABLE' ||
    'VPN_SERVICE_UNAVAILABLE' => 'engine_unavailable',
    'ENGINE_REQUEST_TIMEOUT' || 'DIAGNOSTIC_TIMEOUT' => 'operation_timeout',
    'UPDATE_CHECK_FAILED' => 'update_check_failed',
    'UPDATE_CHECK_NETWORK' => 'update_check_network',
    'UPDATE_CHECK_RATE_LIMITED' => 'update_check_rate_limited',
    'UPDATE_CHECK_UNAVAILABLE' => 'update_check_unavailable',
    'UPDATE_CHECK_RESPONSE_TOO_LARGE' => 'update_check_response_too_large',
    'UPDATE_CHECK_INVALID_RESPONSE' => 'update_check_invalid_response',
    'INITIAL_IDENTITY_UNSUPPORTED' => 'onboarding_unavailable',
    'INITIAL_IDENTITY_INTERRUPTED' => 'onboarding_interrupted',
    'INITIAL_IDENTITY_CLEANUP_REQUIRED' => 'onboarding_cleanup_required',
    'VPN_PERMISSION_DENIED' => 'onboarding_vpn_required',
    'IDENTITY_SETUP_REQUIRED' ||
    'IDENTITY_INVALID' ||
    'AUTHENTICATION_FAILED' => 'diag_fix_refresh_or_replace_identity',
    'CONFIGURATION_INVALID' => 'diag_fix_review_configuration',
    'L4_UNSUPPORTED' => 'l4_unsupported',
    'AUTOMATIC_ENDPOINTS_UNSUPPORTED' => 'endpoint_unsupported',
    'ENDPOINT_PRESETS_UNSUPPORTED' => 'endpoint_preset_unsupported',
    'ENDPOINT_PRESET_REQUIRES_IPV6' => 'endpoint_preset_policy',
    'ENDPOINT_PRESET_REQUIRES_CUSTOM' => 'endpoint_custom',
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
