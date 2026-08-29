import 'dart:async';
import 'dart:io';

import 'package:flutter/services.dart';

import '../models/app_models.dart';
import '../state/app_controller.dart';

class PlatformShellBridge {
  PlatformShellBridge(this._controller) {
    _channel.setMethodCallHandler(_handleMethod);
    if (Platform.isWindows) {
      _controller.addListener(_publishTrayState);
      _publishTrayState();
    }
  }

  static const MethodChannel _channel = MethodChannel(
    'io.github.georgexie2333.usque/engine',
  );

  final AppController _controller;
  String? _lastTrayFingerprint;

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
    final connected =
        snapshot.phase != ConnectionPhase.disconnected &&
        snapshot.phase != ConnectionPhase.error;
    final fingerprint = '${snapshot.phase.name}:$connected';
    if (_lastTrayFingerprint == fingerprint) return;
    _lastTrayFingerprint = fingerprint;
    unawaited(
      _channel
          .invokeMethod<void>('updateTrayState', <String, Object>{
            'phase': snapshot.phase.name,
            'connected': connected,
          })
          .catchError((Object _) {}),
    );
  }

  void dispose() {
    if (Platform.isWindows) _controller.removeListener(_publishTrayState);
    _channel.setMethodCallHandler(null);
  }
}
