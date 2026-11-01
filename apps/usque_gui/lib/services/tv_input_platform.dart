import 'package:flutter/services.dart';

/// UI-only bridge. Text and credentials never cross this channel.
abstract class TvInputPlatform {
  void Function(String sessionId, String reason)? onEditingEnded;
  Future<bool?> getEnvironment();
  Future<void> beginEditing(String sessionId);
  Future<void> endEditing(String sessionId);
  void dispose();
}

class MethodChannelTvInputPlatform extends TvInputPlatform {
  MethodChannelTvInputPlatform() {
    _channel.setMethodCallHandler((call) async {
      if (call.method != 'editingEnded') return;
      final arguments = call.arguments;
      if (arguments is! Map) return;
      final session = arguments['sessionId'];
      final reason = arguments['reason'];
      if (session is String && reason is String) {
        onEditingEnded?.call(session, reason);
      }
    });
  }

  static const _channel = MethodChannel(
    'io.github.georgexie2333.usque/tv_input',
  );

  @override
  Future<bool?> getEnvironment() async {
    final value = await _channel.invokeMapMethod<String, Object?>(
      'getEnvironment',
    );
    return value?['isTv'] as bool?;
  }

  @override
  Future<void> beginEditing(String sessionId) => _channel.invokeMethod<void>(
    'beginEditing',
    <String, Object>{'sessionId': sessionId},
  );

  @override
  Future<void> endEditing(String sessionId) => _channel.invokeMethod<void>(
    'endEditing',
    <String, Object>{'sessionId': sessionId},
  );

  @override
  void dispose() {
    onEditingEnded = null;
    _channel.setMethodCallHandler(null);
  }
}
