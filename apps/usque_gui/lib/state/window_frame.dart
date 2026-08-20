import 'dart:async';

import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';

/// Which edge a resize gesture grabs.
enum WindowEdge {
  left,
  top,
  right,
  bottom,
  topLeft,
  topRight,
  bottomLeft,
  bottomRight,
}

/// State of the Flutter-drawn Windows title bar.
///
/// The window is a process singleton, so this is one too. It stays disabled
/// until [enable] is called from `main`, which keeps tests and the Android
/// build on the platform's own decorations.
class WindowFrame extends ChangeNotifier {
  WindowFrame._();

  static final WindowFrame instance = WindowFrame._();

  static const MethodChannel _channel = MethodChannel(
    'io.github.georgexie2333.usque/engine',
  );

  bool _enabled = false;
  bool _maximized = false;
  bool _active = true;

  /// True when Flutter owns the caption and the shell must draw a title bar.
  bool get enabled => _enabled;

  bool get maximized => _maximized;

  /// False while another window holds focus, which dims the title bar.
  bool get active => _active;

  void enable() {
    if (_enabled) return;
    _enabled = true;
    notifyListeners();
    unawaited(refresh());
  }

  @visibleForTesting
  void debugEnable({bool maximized = false, bool active = true}) {
    _enabled = true;
    _maximized = maximized;
    _active = active;
    notifyListeners();
  }

  @visibleForTesting
  void debugReset() {
    _enabled = false;
    _maximized = false;
    _active = true;
    notifyListeners();
  }

  /// Applies a state push from the shell.
  void apply({required bool maximized, required bool active}) {
    if (_maximized == maximized && _active == active) return;
    _maximized = maximized;
    _active = active;
    notifyListeners();
  }

  Future<void> refresh() async {
    if (!_enabled) return;
    final Map<Object?, Object?>? state = await _invoke<Map<Object?, Object?>>(
      'windowFrameState',
    );
    if (state == null) return;
    apply(
      maximized: state['maximized'] == true,
      active: state['active'] != false,
    );
  }

  Future<void> minimize() => _send('windowMinimize');

  Future<void> toggleMaximize() => _send('windowToggleMaximize');

  Future<void> close() => _send('windowClose');

  Future<void> startDrag() => _send('windowStartDrag');

  Future<void> startResize(WindowEdge edge) =>
      _send('windowStartResize', edge.name);

  Future<void> _send(String method, [Object? arguments]) async {
    if (!_enabled) return;
    await _invoke<void>(method, arguments);
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
