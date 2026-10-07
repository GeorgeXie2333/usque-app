import 'dart:async';
import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';

import '../core/app_strings.dart';
import '../core/connection_presentation.dart';
import '../models/app_models.dart';
import '../state/app_controller.dart';
import '../state/connection_notice_tracker.dart';
import '../widgets/common.dart';

class PlatformShellBridge {
  PlatformShellBridge(this._controller) {
    _channel.setMethodCallHandler(_handleMethod);
    if (Platform.isWindows) {
      _notices = ConnectionNoticeTracker(onNotice: _showNotice);
      _controller.addListener(_publishTrayState);
      _publishTrayState();
    } else if (Platform.isAndroid) {
      _controller.addListener(_publishAndroidLocale);
      _publishAndroidLocale();
    }
  }

  static const MethodChannel _channel = MethodChannel(
    'io.github.georgexie2333.usque/engine',
  );

  final AppController _controller;
  ConnectionNoticeTracker? _notices;
  String? _lastTrayFingerprint;
  String? _lastAndroidLocale;

  Future<Object?> _handleMethod(MethodCall call) async {
    if (call.method == 'zeroTrustCallbackArrived') {
      _controller.noteZeroTrustCallbackArrived();
      return null;
    }
    if (call.method == 'updateInstallFinished') {
      final arguments = call.arguments;
      if (arguments is Map) {
        _controller.noteUpdateInstallFinished(
          success: arguments['success'] == true,
          message: arguments['message'] as String?,
        );
      }
      return null;
    }
    if (call.method != 'trayCommand') return null;
    switch (call.arguments) {
      case 'toggle':
        await _controller.connectOrDisconnect();
      case 'disconnectAndExit':
        await _controller.disconnectForExit();
      case 'toggleTunnel':
        await toggleTunnelShortcut(_controller);
      case 'toggleSystemProxy':
        await toggleSystemProxyShortcut(_controller);
      default:
        throw PlatformException(
          code: 'INVALID_TRAY_COMMAND',
          message: 'The Windows tray command is not supported.',
        );
    }
    return null;
  }

  void _publishTrayState() {
    final snapshot = _controller.snapshot;
    final strings = _controller.strings;
    final profile = _controller.activeProfile;
    _notices?.update(
      snapshot,
      killSwitchArmed: profile.frontends.tunnel && profile.killSwitch,
    );
    final connected =
        snapshot.phase != ConnectionPhase.disconnected &&
        snapshot.phase != ConnectionPhase.error;
    final status = strings.get(
      ConnectionPresentation.of(snapshot.phase).labelKey,
    );
    final showOutputs =
        _controller.initialized && _controller.networkSettings.supported;
    final state = <String, Object>{
      'phase': snapshot.phase.name,
      'status': status,
      'connected': connected,
      'badge': trayBadge(snapshot.phase),
      'open': strings.get('tray_open'),
      'connect': strings.get('tray_connect_profile'),
      'disconnect': strings.get('tray_disconnect_profile'),
      'disconnect_exit': strings.get('tray_disconnect_exit'),
      'tunnel_label': showOutputs
          ? strings.tunnelOutputLabel(TargetPlatform.windows)
          : '',
      'system_proxy_label': showOutputs ? strings.get('home_system_proxy') : '',
      'tunnel': profile.frontends.tunnel,
      'system_proxy': profile.proxy.systemProxy,
      'outputs_enabled': !_controller.networkShortcutsLocked,
      'system_proxy_available': profile.frontends.http,
    };
    final fingerprint = state.entries
        .map((entry) => '${entry.key}=${entry.value}')
        .join('\u0000');
    if (_lastTrayFingerprint == fingerprint) return;
    _lastTrayFingerprint = fingerprint;
    unawaited(
      _channel
          .invokeMethod<void>('updateTrayState', state)
          .catchError((Object _) {}),
    );
  }

  void _showNotice(ConnectionNotice notice) {
    final strings = _controller.strings;
    final (title, body, level) = connectionNoticeText(strings, notice);
    unawaited(
      _channel
          .invokeMethod<bool>('showTrayNotification', <String, Object>{
            'title': title,
            'body': body,
            'level': level,
          })
          .catchError((Object _) => false),
    );
  }

  void _publishAndroidLocale() {
    if (!_controller.initialized) return;
    final catalogId = _controller.localePreference == LocalePreference.system
        ? 'system'
        : _controller.strings.catalogId;
    if (_lastAndroidLocale == catalogId) return;
    _lastAndroidLocale = catalogId;
    unawaited(
      _channel
          .invokeMethod<void>('updatePlatformLocale', <String, Object>{
            'catalog_id': catalogId,
          })
          .catchError((Object _) {}),
    );
  }

  void dispose() {
    if (Platform.isWindows) _controller.removeListener(_publishTrayState);
    if (Platform.isAndroid) _controller.removeListener(_publishAndroidLocale);
    _notices?.dispose();
    _channel.setMethodCallHandler(null);
  }
}

/// Tray status dot for [phase]; `idle` keeps the plain application icon.
@visibleForTesting
String trayBadge(ConnectionPhase phase) {
  final presentation = ConnectionPresentation.of(phase);
  if (presentation.mode == RingMode.idle) return 'idle';
  return switch (presentation.tone) {
    StatusTone.success => 'connected',
    StatusTone.warning => 'warning',
    StatusTone.danger => 'error',
    StatusTone.brand || StatusTone.neutral => 'busy',
  };
}

/// Title, body, and Windows notification level for [notice].
///
/// The text is fixed copy: engine error details can carry addresses and stay
/// inside the app instead of the system notification history.
@visibleForTesting
(String, String, String) connectionNoticeText(
  AppStrings strings,
  ConnectionNotice notice,
) {
  String withKillSwitch(String body) => notice.killSwitchBlocking
      ? '$body\n${strings.get('notice_kill_switch_blocking')}'
      : body;
  return switch (notice.kind) {
    ConnectionNoticeKind.interrupted => (
      strings.get('reconnecting'),
      withKillSwitch(strings.get('notice_connection_interrupted')),
      'warning',
    ),
    ConnectionNoticeKind.failed => (
      strings.get('error'),
      withKillSwitch(strings.get('notice_connection_failed')),
      'error',
    ),
    ConnectionNoticeKind.restored => (
      strings.get('connected'),
      strings.get('notice_connection_restored'),
      'info',
    ),
  };
}

/// Flips the saved TUN output through the same apply lifecycle as Home.
Future<bool> toggleTunnelShortcut(AppController app) async {
  if (app.networkShortcutsLocked) return false;
  final profile = app.activeProfile;
  return app.saveNetwork(
    profile.copyWith(
      frontends: profile.frontends.copyWith(tunnel: !profile.frontends.tunnel),
    ),
    changedFields: const ['frontends.tunnel'],
  );
}

/// Flips the saved system proxy; enabling it requires the HTTP local proxy.
Future<bool> toggleSystemProxyShortcut(AppController app) async {
  if (app.networkShortcutsLocked) return false;
  final profile = app.activeProfile;
  final enabled = !profile.proxy.systemProxy;
  if (enabled && !profile.frontends.http) return false;
  return app.saveNetwork(
    profile.copyWith(proxy: profile.proxy.copyWith(systemProxy: enabled)),
    changedFields: const ['proxy.system_proxy'],
  );
}
