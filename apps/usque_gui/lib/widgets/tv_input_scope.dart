import 'dart:async';
import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';

import '../services/tv_input_platform.dart';
import 'desktop_shortcuts.dart';

/// Owns one editing session across root routes, nested navigators and dialogs.
class TvInputController with WidgetsBindingObserver {
  TvInputController({this.isTv, this.platform}) {
    platform?.onEditingEnded = (id, reason) {
      if (_active?.id == id) finish(restoreFocus: reason != 'lifecycle');
    };
    WidgetsBinding.instance.addObserver(this);
    FocusManager.instance.addListener(_invalidateFocusRestoration);
  }

  bool? isTv;
  final TvInputPlatform? platform;
  static int _nextOwner = 0;
  final String _owner =
      '${DateTime.now().microsecondsSinceEpoch}-${_nextOwner++}';
  int _sequence = 0;
  int _focusRevision = 0;
  _EditingSession? _active;
  final _consumedRoutes = <ModalRoute<dynamic>>{};
  bool _disposed = false;

  Future<void> initialize() async {
    if (isTv != null || platform == null) return;
    try {
      final value = await platform!.getEnvironment().timeout(
        const Duration(seconds: 2),
      );
      if (!_disposed) isTv = value;
    } on Object {
      // A missing/older host falls back to Android's navigation mode.
    }
  }

  bool owns(Object owner) => _active?.owner == owner;

  /// Deferred restoration is valid only until another session, focus change
  /// or navigation intent. An idle controller alone cannot prove that.
  int get focusRevision => _focusRevision;

  bool canRestoreFocus(int revision) =>
      !_disposed && _active == null && revision == _focusRevision;

  void _invalidateFocusRestoration() => _focusRevision++;

  /// Invalidate before the focus request settles, including edge traversal.
  void noteFocusNavigation() => _invalidateFocusRestoration();

  Future<bool> begin({
    required Object owner,
    required BuildContext context,
    required void Function(bool restoreFocus) onEnd,
  }) async {
    if (_disposed || !routeChainIsCurrent(context)) return false;
    finish(restoreFocus: false);
    _invalidateFocusRestoration();
    final routes = <ModalRoute<dynamic>>[];
    BuildContext? current = context;
    while (current != null) {
      final route = ModalRoute.of(current);
      if (route == null) break;
      routes.add(route);
      current = route.navigator?.context;
    }
    final session = _EditingSession(
      '$_owner-${++_sequence}',
      owner,
      context,
      routes,
      onEnd,
    );
    _active = session;
    for (final route in routes) {
      final entry = _EditingPopEntry(() => consumeRouteBack(route));
      session.entries[route] = entry;
      route.registerPopEntry(entry);
    }
    try {
      await platform
          ?.beginEditing(session.id)
          .timeout(const Duration(seconds: 2));
    } on Object {
      // Flutter's already-registered route protection remains effective.
    }
    if (_active != session ||
        _disposed ||
        !context.mounted ||
        !routeChainIsCurrent(context)) {
      if (_active == session) finish(restoreFocus: false);
      unawaited(_releaseNative(session.id));
      return false;
    }
    return true;
  }

  Future<void> _releaseNative(String id) async {
    try {
      await platform?.endEditing(id);
    } on Object {
      // Native lifecycle cleanup also drops any remaining registration.
    }
  }

  /// Shared by every return path; all callbacks for the same synchronous pop
  /// observe consumption even after the editor has returned to browsing.
  bool consumeBack(BuildContext context) =>
      consumeRouteBack(ModalRoute.of(context));

  bool consumeRouteBack(ModalRoute<dynamic>? route) {
    if (route == null) return false;
    if (_consumedRoutes.contains(route)) return true;
    final session = _active;
    if (session == null ||
        !session.routes.contains(route) ||
        !session.context.mounted ||
        !routeChainIsCurrent(session.context)) {
      return false;
    }
    _consumedRoutes.addAll(session.routes);
    scheduleMicrotask(_consumedRoutes.clear);
    finish();
    return true;
  }

  void finish({bool restoreFocus = true, Object? owner}) {
    final session = _active;
    if (session == null || owner != null && session.owner != owner) return;
    _active = null;
    _invalidateFocusRestoration();
    // Do not mutate ModalRoute's PopEntry collection during its dispatch loop.
    for (final entry in session.entries.values) {
      entry.canPopNotifier.value = true;
    }
    scheduleMicrotask(() {
      for (final pair in session.entries.entries) {
        pair.key.unregisterPopEntry(pair.value);
        pair.value.canPopNotifier.dispose();
      }
    });
    unawaited(_releaseNative(session.id));
    session.onEnd(restoreFocus);
  }

  @override
  Future<bool> didPopRoute() async {
    final session = _active;
    return session != null && consumeRouteBack(session.routes.firstOrNull);
  }

  @override
  void didChangeAppLifecycleState(AppLifecycleState state) {
    if (state == AppLifecycleState.hidden ||
        state == AppLifecycleState.paused ||
        state == AppLifecycleState.detached) {
      _invalidateFocusRestoration();
      finish(restoreFocus: false);
    }
  }

  void dispose() {
    _disposed = true;
    finish(restoreFocus: false);
    WidgetsBinding.instance.removeObserver(this);
    FocusManager.instance.removeListener(_invalidateFocusRestoration);
    platform?.dispose();
  }
}

class _EditingSession {
  _EditingSession(this.id, this.owner, this.context, this.routes, this.onEnd);
  final String id;
  final Object owner;
  final BuildContext context;
  final List<ModalRoute<dynamic>> routes;
  final void Function(bool) onEnd;
  final entries = <ModalRoute<dynamic>, _EditingPopEntry>{};
}

class _EditingPopEntry extends PopEntry<Object?> {
  _EditingPopEntry(this.onBack);
  final VoidCallback onBack;
  @override
  final ValueNotifier<bool> canPopNotifier = ValueNotifier(false);
  @override
  void onPopInvokedWithResult(bool didPop, Object? result) {
    if (!didPop) onBack();
  }
}

class TvInputScope extends InheritedWidget {
  const TvInputScope({
    required this.controller,
    required super.child,
    super.key,
  });
  final TvInputController controller;
  static TvInputController? maybeOf(BuildContext context) =>
      context.dependOnInheritedWidgetOfExactType<TvInputScope>()?.controller;
  static bool enabled(BuildContext context) =>
      defaultTargetPlatform == TargetPlatform.android &&
      (maybeOf(context)?.isTv ??
          MediaQuery.maybeNavigationModeOf(context) ==
              NavigationMode.directional);
  @override
  bool updateShouldNotify(TvInputScope oldWidget) =>
      controller != oldWidget.controller;
}

/// Sits outside MaterialApp so its legacy Back observer runs before navigation.
class TvInputHost extends StatefulWidget {
  const TvInputHost({required this.child, this.controller, super.key});
  final Widget child;
  final TvInputController? controller;
  @override
  State<TvInputHost> createState() => _TvInputHostState();
}

class _TvInputHostState extends State<TvInputHost> {
  late final TvInputController _controller;
  bool _ready = false;
  @override
  void initState() {
    super.initState();
    _controller =
        widget.controller ??
        TvInputController(
          isTv: defaultTargetPlatform == TargetPlatform.android ? null : false,
          // A simulated Android target on a desktop has no Android messenger.
          // Tests can inject a platform explicitly; never wait for a native host
          // that is not running on this operating system.
          platform: !kIsWeb && Platform.isAndroid
              ? MethodChannelTvInputPlatform()
              : null,
        );
    if (_controller.isTv != null || _controller.platform == null) {
      _ready = true;
    } else {
      unawaited(
        _controller.initialize().then((_) {
          if (mounted) setState(() => _ready = true);
        }),
      );
    }
  }

  @override
  void dispose() {
    if (widget.controller == null) _controller.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) => TvInputScope(
    controller: _controller,
    child: _ready ? widget.child : const SizedBox.shrink(),
  );
}
