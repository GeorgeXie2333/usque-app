import 'package:flutter/foundation.dart';
import 'package:flutter/widgets.dart';

import '../state/app_controller.dart';

/// Rebuilds only when the selected value changes, for pages that read more
/// than the app controller (network quality, diagnostics).
///
/// [ControllerSelector] covers the app controller alone; this variant merges
/// several listenables so a section can follow, for example, the connection
/// and its quality samples without repainting on unrelated notifications.
class ListenableSelector<T> extends StatefulWidget {
  const ListenableSelector({
    required this.listenables,
    required this.selector,
    required this.builder,
    this.equals,
    super.key,
  });

  final List<Listenable> listenables;
  final T Function() selector;
  final Widget Function(BuildContext context, T value) builder;

  /// Value comparison; defaults to `==`. Use it for selections holding lists.
  final bool Function(T previous, T next)? equals;

  @override
  State<ListenableSelector<T>> createState() => _ListenableSelectorState<T>();
}

class _ListenableSelectorState<T> extends State<ListenableSelector<T>> {
  late T _value;

  @override
  void initState() {
    super.initState();
    _value = widget.selector();
    for (final listenable in widget.listenables) {
      listenable.addListener(_changed);
    }
  }

  @override
  void didUpdateWidget(covariant ListenableSelector<T> oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (!listEquals(oldWidget.listenables, widget.listenables)) {
      for (final listenable in oldWidget.listenables) {
        listenable.removeListener(_changed);
      }
      for (final listenable in widget.listenables) {
        listenable.addListener(_changed);
      }
    }
    _value = widget.selector();
  }

  void _changed() {
    if (!mounted) return;
    if (widget.listenables.any(
      (value) => value is AppController && !value.uiVisible,
    )) {
      return;
    }
    final next = widget.selector();
    final same = widget.equals?.call(_value, next) ?? next == _value;
    if (same) return;
    setState(() => _value = next);
  }

  @override
  void dispose() {
    for (final listenable in widget.listenables) {
      listenable.removeListener(_changed);
    }
    super.dispose();
  }

  @override
  Widget build(BuildContext context) => widget.builder(context, _value);
}
