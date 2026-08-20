import 'package:flutter/material.dart';

/// Motion tokens for the whole client.
///
/// Usque is a status instrument: motion exists to make a state change legible,
/// never to decorate. Four durations cover every animation in the app and the
/// ambient loop is the only one that repeats.
class UsqueMotion {
  const UsqueMotion._();

  /// Pointer feedback: hover, press, focus.
  static const Duration fast = Duration(milliseconds: 120);

  /// State changes inside a surface: colours, pills, counters.
  static const Duration base = Duration(milliseconds: 200);

  /// Layout level changes: section switches, banners, dialogs.
  static const Duration gentle = Duration(milliseconds: 300);

  /// The connection ring closing once the tunnel is up.
  static const Duration lock = Duration(milliseconds: 720);

  /// One full rotation of a scanning arc. Repeats only while the engine is
  /// mid-transition, so the app is never animating at rest.
  static const Duration scan = Duration(milliseconds: 1600);

  static const Curve standard = Curves.easeOutCubic;
  static const Curve emphasized = Cubic(0.2, 0.0, 0.0, 1.0);
  static const Curve exit = Curves.easeInCubic;

  /// True when the platform asks for reduced motion.
  static bool reduced(BuildContext context) =>
      MediaQuery.maybeOf(context)?.disableAnimations ?? false;

  /// Collapses [value] to zero when the platform asks for reduced motion.
  static Duration of(BuildContext context, Duration value) =>
      reduced(context) ? Duration.zero : value;
}

/// Cross-fades between children while nudging the incoming one upward.
///
/// Material calls this a fade-through; it reads as "a different thing" rather
/// than "the same thing moved", which is what a section switch is.
class FadeThroughSwitcher extends StatelessWidget {
  const FadeThroughSwitcher({
    required this.child,
    this.duration = UsqueMotion.gentle,
    this.offset = 0.012,
    this.alignment = Alignment.topCenter,
    super.key,
  });

  final Widget child;
  final Duration duration;

  /// Vertical travel of the incoming child, as a fraction of its height.
  final double offset;
  final AlignmentGeometry alignment;

  @override
  Widget build(BuildContext context) {
    return AnimatedSwitcher(
      duration: UsqueMotion.of(context, duration),
      switchInCurve: UsqueMotion.emphasized,
      switchOutCurve: UsqueMotion.exit,
      layoutBuilder: (currentChild, previousChildren) => Stack(
        alignment: alignment,
        children: <Widget>[...previousChildren, ?currentChild],
      ),
      transitionBuilder: (child, animation) => FadeTransition(
        opacity: animation,
        child: SlideTransition(
          position: Tween<Offset>(
            begin: Offset(0, offset),
            end: Offset.zero,
          ).animate(animation),
          child: child,
        ),
      ),
      child: child,
    );
  }
}
