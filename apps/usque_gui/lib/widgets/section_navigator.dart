import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter/scheduler.dart';

import 'unsaved_changes_guard.dart';

/// Keeps a shell section's subpages inside the shell, so desktop navigation
/// stays visible and survives rail breakpoints.
///
/// Subpages use the ordinary [Navigator.of] API from the root page. Leaving the
/// section goes through [SectionNavigatorState.closeSubpages], which asks every
/// mounted [UnsavedChangesGuard] before returning to the root page.
class SectionNavigator extends StatefulWidget {
  const SectionNavigator({
    required this.active,
    required this.rootName,
    required this.builder,
    this.onSubpageChanged,
    super.key,
  });

  final bool active;
  final String rootName;
  final WidgetBuilder builder;
  final ValueChanged<bool>? onSubpageChanged;

  static SectionNavigatorState? maybeOf(BuildContext context) =>
      context.findAncestorStateOfType<SectionNavigatorState>();

  @override
  State<SectionNavigator> createState() => SectionNavigatorState();
}

class SectionNavigatorState extends State<SectionNavigator> {
  final _navigator = GlobalKey<NavigatorState>();
  late final _observer = _PageObserver(_pagesChanged);
  final _guards = <UnsavedChangesGuardState>[];
  bool _closing = false;
  bool _subpageOpen = false;
  Future<void>? _exit;

  NavigatorState? get navigator => _navigator.currentState;

  bool get closing => _closing;

  void registerGuard(UnsavedChangesGuardState guard) {
    if (!_guards.contains(guard)) _guards.add(guard);
  }

  void unregisterGuard(UnsavedChangesGuardState guard) => _guards.remove(guard);

  void _pagesChanged(Route<dynamic>? exiting) {
    final open = _observer.pages.length > 1;
    if (open) {
      _report(true);
      return;
    }
    // Keep the subpage state until its exit animation finishes, while the
    // section is still visible, so its TickerMode cannot delay disposal.
    final exit = exiting is TransitionRoute ? exiting.completed : null;
    if (exit == null) {
      _report(false);
      return;
    }
    final pending = exit.then<void>((_) {});
    _exit = pending;
    unawaited(
      pending.then((_) {
        if (_exit == pending) _exit = null;
        if (mounted && _observer.pages.length <= 1) _report(false);
      }),
    );
  }

  void _report(bool open) {
    if (open == _subpageOpen) return;
    void apply() {
      if (!mounted || open == _subpageOpen) return;
      _subpageOpen = open;
      widget.onSubpageChanged?.call(open);
    }

    if (SchedulerBinding.instance.schedulerPhase ==
        SchedulerPhase.persistentCallbacks) {
      SchedulerBinding.instance.addPostFrameCallback((_) => apply());
    } else {
      apply();
    }
  }

  /// Returns to the root page after every guarded subpage agrees to leave.
  Future<bool> closeSubpages() async {
    final navigator = _navigator.currentState;
    if (navigator == null) return true;
    if (_closing) return false;
    _closing = true;
    try {
      final pages = _observer.pages;
      if (pages.length <= 1) {
        await _exit;
        return mounted;
      }
      // Dismiss local popups such as dropdown menus before asking, without
      // closing any page until every guard agrees.
      navigator.popUntil((route) => route is PageRoute);
      for (final guard in _guards.reversed.toList()) {
        if (!guard.mounted) continue;
        final route = ModalRoute.of(guard.context);
        if (route == null || !route.isActive || route.isFirst) continue;
        if (!await guard.confirmLeave()) return false;
        if (!mounted) return false;
      }
      final top = _observer.pages.last;
      navigator.popUntil((route) => route.isFirst);
      if (top is TransitionRoute) await top.completed;
      return mounted;
    } finally {
      _closing = false;
    }
  }

  @override
  Widget build(BuildContext context) => NavigatorPopHandler<void>(
    enabled: widget.active,
    onPopWithResult: (_) {
      if (widget.active) unawaited(_navigator.currentState!.maybePop());
    },
    child: Navigator(
      key: _navigator,
      observers: <NavigatorObserver>[_observer],
      onGenerateRoute: (_) => MaterialPageRoute<void>(
        settings: RouteSettings(name: widget.rootName),
        builder: widget.builder,
      ),
    ),
  );
}

/// Tracks page routes, ignoring dialogs and popup menus.
class _PageObserver extends NavigatorObserver {
  _PageObserver(this.onChanged);

  final void Function(Route<dynamic>? exiting) onChanged;
  final pages = <Route<dynamic>>[];

  void _add(Route<dynamic>? route) {
    if (route is PageRoute) {
      pages.add(route);
      onChanged(null);
    }
  }

  void _remove(Route<dynamic>? route) {
    if (route is PageRoute && pages.remove(route)) onChanged(route);
  }

  @override
  void didPush(Route<dynamic> route, Route<dynamic>? previousRoute) =>
      _add(route);

  @override
  void didPop(Route<dynamic> route, Route<dynamic>? previousRoute) =>
      _remove(route);

  @override
  void didRemove(Route<dynamic> route, Route<dynamic>? previousRoute) =>
      _remove(route);

  @override
  void didReplace({Route<dynamic>? newRoute, Route<dynamic>? oldRoute}) {
    _remove(oldRoute);
    _add(newRoute);
  }
}
