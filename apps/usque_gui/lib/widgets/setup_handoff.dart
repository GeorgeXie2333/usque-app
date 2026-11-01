import 'package:flutter/material.dart';

import '../core/usque_motion.dart';

/// Phases of the first-run hand-off, as intervals of one [UsqueMotion.lock]
/// run. Setup closes its ring and clears away while its brand pane folds into
/// the rail; Home then rises in behind it.
abstract final class SetupHandoffTiming {
  /// Setup's form and step content.
  static const Interval form = Interval(0, 0.35, curve: UsqueMotion.exit);

  /// The brand ring sweeping closed, then fading with the rest of the motif.
  static const Interval ringClose = Interval(
    0,
    0.3,
    curve: UsqueMotion.standard,
  );
  static const Interval motif = Interval(0.2, 0.45, curve: UsqueMotion.exit);

  /// Setup's opaque canvas, revealing the shell beneath.
  static const Interval canvas = Interval(0.2, 0.55, curve: UsqueMotion.exit);

  /// The brand pane narrowing to the rail and its mark moving onto the rail's.
  static const Interval collapse = Interval(
    0.15,
    0.65,
    curve: UsqueMotion.emphasized,
  );

  /// The narrowed pane, then its mark, giving way to the rail they cover.
  static const Interval pane = Interval(0.55, 0.85, curve: UsqueMotion.exit);
  static const Interval mark = Interval(0.7, 0.95, curve: UsqueMotion.exit);

  /// Home's content arriving.
  static const Interval entrance = Interval(
    0.35,
    1,
    curve: UsqueMotion.emphasized,
  );
}

/// Switches between first-run setup and the shell. Completing setup plays a
/// single hand-off; every other switch, reduced motion and a hidden window
/// change screens directly.
///
/// The outgoing setup keeps its state for the hand-off but takes no input,
/// focus or semantics, so assistive technology and D-pad users only meet the
/// shell.
class SetupHandoff extends StatefulWidget {
  const SetupHandoff({
    required this.complete,
    required this.setup,
    required this.shell,
    super.key,
  });

  /// Whether setup has completed; true shows [shell].
  final bool complete;

  /// Builds setup. `exit` runs from 0 to 1 while setup hands over to the
  /// shell, and is null otherwise.
  final Widget Function(Animation<double>? exit) setup;
  final Widget shell;

  @override
  State<SetupHandoff> createState() => _SetupHandoffState();
}

class _SetupHandoffState extends State<SetupHandoff>
    with SingleTickerProviderStateMixin {
  late final AnimationController _motion;
  bool _leaving = false;

  @override
  void initState() {
    super.initState();
    _motion = AnimationController(
      vsync: this,
      duration: UsqueMotion.lock,
      value: 1,
    );
  }

  @override
  void didUpdateWidget(SetupHandoff oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.complete == widget.complete) return;
    if (!widget.complete || UsqueMotion.reduced(context)) {
      _settle();
    } else if (TickerMode.valuesOf(context).enabled) {
      _leaving = true;
      _motion.forward(from: 0).whenCompleteOrCancel(() {
        if (mounted && _leaving) setState(_settle);
      });
    }
  }

  void _settle() {
    _leaving = false;
    _motion.value = 1;
  }

  @override
  void dispose() {
    _motion.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    // One keyed stack in every state, so neither screen loses its state when
    // the other is added or removed.
    return Stack(
      fit: StackFit.expand,
      children: <Widget>[
        if (widget.complete)
          KeyedSubtree(
            key: const ValueKey<String>('setup-handoff-shell'),
            child: SetupEntrance(animation: _motion, child: widget.shell),
          ),
        if (!widget.complete || _leaving)
          KeyedSubtree(
            key: const ValueKey<String>('setup-handoff-setup'),
            child: ExcludeFocus(
              excluding: _leaving,
              child: ExcludeSemantics(
                excluding: _leaving,
                child: IgnorePointer(
                  ignoring: _leaving,
                  child: widget.setup(_leaving ? _motion : null),
                ),
              ),
            ),
          ),
      ],
    );
  }
}

/// Carries the hand-off to the shell's content. At rest, and anywhere outside
/// a [SetupHandoff], it is complete.
class SetupEntrance extends InheritedWidget {
  const SetupEntrance({
    required this.animation,
    required super.child,
    super.key,
  });

  final Animation<double> animation;

  static Animation<double> of(BuildContext context) =>
      context.dependOnInheritedWidgetOfExactType<SetupEntrance>()?.animation ??
      kAlwaysCompleteAnimation;

  @override
  bool updateShouldNotify(SetupEntrance oldWidget) =>
      animation != oldWidget.animation;
}

/// Fades and lifts [child] into place during the hand-off, optionally
/// growing it from [scale]. Draws [child] untouched at rest.
class SetupEntranceTransition extends StatelessWidget {
  const SetupEntranceTransition({
    required this.child,
    this.lift = 0.02,
    this.scale = 1,
    this.fade = true,
    super.key,
  });

  final Widget child;

  /// Upward travel as a fraction of the child's height.
  final double lift;

  /// Starting scale about the centre.
  final double scale;
  final bool fade;

  @override
  Widget build(BuildContext context) {
    final progress = SetupEntrance.of(
      context,
    ).drive(CurveTween(curve: SetupHandoffTiming.entrance));
    Widget result = child;
    if (scale != 1) {
      result = ScaleTransition(
        scale: progress.drive(Tween<double>(begin: scale, end: 1)),
        child: result,
      );
    }
    if (lift != 0) {
      result = SlideTransition(
        position: progress.drive(
          Tween<Offset>(begin: Offset(0, lift), end: Offset.zero),
        ),
        child: result,
      );
    }
    return fade ? FadeTransition(opacity: progress, child: result) : result;
  }
}
