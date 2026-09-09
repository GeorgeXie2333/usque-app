import 'dart:async';
import 'dart:io';

import 'package:flutter/services.dart';

import '../core/connection_presentation.dart';
import '../models/app_models.dart';
import '../state/app_controller.dart';

class PlatformShellBridge {
  PlatformShellBridge(this._controller) {
    _channel.setMethodCallHandler(_handleMethod);
    if (Platform.isWindows) {
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
    final connected =
        snapshot.phase != ConnectionPhase.disconnected &&
        snapshot.phase != ConnectionPhase.error;
    final status = strings.get(
      ConnectionPresentation.of(snapshot.phase).labelKey,
    );
    final fingerprint =
        '${snapshot.phase.name}:$connected:${strings.catalogId}:$status';
    if (_lastTrayFingerprint == fingerprint) return;
    _lastTrayFingerprint = fingerprint;
    unawaited(
      _channel
          .invokeMethod<void>('updateTrayState', <String, Object>{
            'phase': snapshot.phase.name,
            'status': status,
            'connected': connected,
            'open': strings.get('tray_open'),
            'connect': strings.get('tray_connect_profile'),
            'disconnect': strings.get('tray_disconnect_profile'),
            'disconnect_exit': strings.get('tray_disconnect_exit'),
          })
          .catchError((Object _) {}),
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
    _channel.setMethodCallHandler(null);
  }
}
