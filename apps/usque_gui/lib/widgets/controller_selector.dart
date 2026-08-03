import 'package:flutter/material.dart';

import '../state/app_controller.dart';

typedef ControllerSelection<T> = T Function(AppController controller);
typedef ControllerSelectionBuilder<T> =
    Widget Function(BuildContext context, T value);

/// Rebuilds only when the selected controller value changes.
///
/// An optional [active] predicate lets an IndexedStack page stay completely
/// quiet while another tab receives high-frequency engine statistics.
class ControllerSelector<T> extends StatefulWidget {
  const ControllerSelector({
    required this.controller,
    required this.selector,
    required this.builder,
    this.active,
    super.key,
  });

  final AppController controller;
  final ControllerSelection<T> selector;
  final ControllerSelectionBuilder<T> builder;
  final bool Function(AppController controller)? active;

  @override
  State<ControllerSelector<T>> createState() => _ControllerSelectorState<T>();
}

class _ControllerSelectorState<T> extends State<ControllerSelector<T>> {
  late T _value;

  @override
  void initState() {
    super.initState();
    _value = widget.selector(widget.controller);
    widget.controller.addListener(_handleControllerChanged);
  }

  @override
  void didUpdateWidget(covariant ControllerSelector<T> oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.controller != widget.controller) {
      oldWidget.controller.removeListener(_handleControllerChanged);
      widget.controller.addListener(_handleControllerChanged);
    }
    _value = widget.selector(widget.controller);
  }

  void _handleControllerChanged() {
    if (widget.active case final active? when !active(widget.controller)) {
      return;
    }
    final next = widget.selector(widget.controller);
    if (next == _value || !mounted) {
      return;
    }
    setState(() => _value = next);
  }

  @override
  void dispose() {
    widget.controller.removeListener(_handleControllerChanged);
    super.dispose();
  }

  @override
  Widget build(BuildContext context) => widget.builder(context, _value);
}
