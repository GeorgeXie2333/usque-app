import 'dart:async';

import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';

/// Which caption button the pointer is over. Pushed from native hit-testing
/// because the painted title bar is not interactive.
enum CaptionHover { none, min, max, close }

/// State of the Flutter-drawn Windows title bar.
///
/// The window is a process singleton, so this is one too. It stays disabled
/// until [enable] is called from `main`, which keeps tests and the Android
/// build on the platform's own decorations.
///
/// Dart only paints and receives state. Move, resize, min/max/close, and
/// snap layouts are handled by the native window procedure.
class WindowFrame extends ChangeNotifier {
  WindowFrame._();

  static final WindowFrame instance = WindowFrame._();

  static const MethodChannel _channel = MethodChannel(
    'io.github.georgexie2333.usque/window_frame',
  );

  bool _enabled = false;
  bool _maximized = false;
  bool _active = true;
  CaptionHover _captionHover = CaptionHover.none;

  /// True when Flutter owns the caption and the shell must draw a title bar.
  bool get enabled => _enabled;

  bool get maximized => _maximized;

  /// False while another window holds focus, which dims the title bar.
  bool get active => _active;

  CaptionHover get captionHover => _captionHover;

  void enable() {
    if (_enabled) return;
    _enabled = true;
    _channel.setMethodCallHandler(_onMethod);
    notifyListeners();
    unawaited(refresh());
  }

  @visibleForTesting
  void debugEnable({
    bool maximized = false,
    bool active = true,
    CaptionHover captionHover = CaptionHover.none,
  }) {
    _enabled = true;
    _maximized = maximized;
    _active = active;
    _captionHover = captionHover;
    notifyListeners();
  }

  @visibleForTesting
  void debugReset() {
    _enabled = false;
    _maximized = false;
    _active = true;
    _captionHover = CaptionHover.none;
    notifyListeners();
  }

  /// Applies a state push from the shell.
  void apply({
    required bool maximized,
    required bool active,
    CaptionHover captionHover = CaptionHover.none,
  }) {
    if (_maximized == maximized &&
        _active == active &&
        _captionHover == captionHover) {
      return;
    }
    _maximized = maximized;
    _active = active;
    _captionHover = captionHover;
    notifyListeners();
  }

  Future<void> refresh() async {
    if (!_enabled) return;
    final Map<Object?, Object?>? state = await _invoke<Map<Object?, Object?>>(
      'windowFrameState',
    );
    if (state == null) return;
    _applyMap(state);
  }

  Future<Object?> _onMethod(MethodCall call) async {
    if (call.method == 'windowFrameChanged') {
      final Object? arguments = call.arguments;
      if (arguments is Map) {
        _applyMap(arguments);
      }
    }
    return null;
  }

  void _applyMap(Map<dynamic, dynamic> state) {
    apply(
      maximized: state['maximized'] == true,
      active: state['active'] != false,
      captionHover: _parseHover(state['captionHover']),
    );
  }

  static CaptionHover _parseHover(Object? value) {
    return switch (value) {
      'min' => CaptionHover.min,
      'max' => CaptionHover.max,
      'close' => CaptionHover.close,
      _ => CaptionHover.none,
    };
  }

  Future<T?> _invoke<T>(String method, [Object? arguments]) async {
    try {
      return await _channel.invokeMethod<T>(method, arguments);
    } on PlatformException catch (error) {
      debugPrint('Usque window frame: ${error.code} ${error.message}');
      return null;
    } on MissingPluginException {
      return null;
    }
  }
}
