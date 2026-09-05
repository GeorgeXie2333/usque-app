import 'dart:math' as math;

import 'package:flutter/material.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../core/connection_presentation.dart';
import '../core/usque_motion.dart';
import '../core/usque_theme.dart';
import '../models/app_models.dart';

/// The connection instrument.
///
/// A 60 tick bezel around a power control. Motion reports work, not health: the
/// bezel sweeps while the engine is mid-transition, closes once when the tunnel
/// comes up, and is otherwise completely still. Reduced motion skips straight
/// to the settled frame.
class ConnectionRing extends StatefulWidget {
  const ConnectionRing({
    required this.phase,
    required this.busy,
    required this.actionLabel,
    required this.onPressed,
    this.size = 232,
    this.semanticLabel,
    this.compactControl = false,
    super.key,
  });

  final ConnectionPhase phase;
  final bool busy;
  final String actionLabel;
  final VoidCallback? onPressed;
  final double size;
  final String? semanticLabel;
  final bool compactControl;

  @override
  State<ConnectionRing> createState() => _ConnectionRingState();
}

class _ConnectionRingState extends State<ConnectionRing>
    with SingleTickerProviderStateMixin {
  late final AnimationController _controller = AnimationController(
    vsync: this,
    duration: UsqueMotion.scan,
    value: 1,
  );

  RingMode? _applied;
  bool _reduced = false;

  ConnectionPresentation get _presentation =>
      ConnectionPresentation.of(widget.phase);

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    final bool reduced = UsqueMotion.reduced(context);
    if (_applied == null || reduced != _reduced) {
      _syncMotion();
    }
  }

  @override
  void didUpdateWidget(covariant ConnectionRing oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.phase != widget.phase) {
      _syncMotion();
    }
  }

  void _syncMotion() {
    final RingMode mode = _presentation.mode;
    final RingMode? previous = _applied;
    final bool reduced = UsqueMotion.reduced(context);
    _applied = mode;
    _reduced = reduced;
    if (mode == previous) {
      // Resume a scan after reduced-motion lifts; never restart one that
      // is already travelling.
      if (mode == RingMode.scan && !reduced && !_controller.isAnimating) {
        _controller.duration = UsqueMotion.scan;
        _controller.repeat();
      }
      return;
    }
    if (mode == RingMode.scan && !reduced) {
      _controller.duration = UsqueMotion.scan;
      _controller.repeat();
      return;
    }
    _controller.stop();
    // Every other mode is a still frame; only the transition into a live
    // tunnel is worth animating, and only once.
    if (mode == RingMode.steady &&
        previous != null &&
        previous != RingMode.steady &&
        !reduced) {
      _controller.duration = UsqueMotion.lock;
      _controller.forward(from: 0);
    } else {
      _controller.value = 1;
    }
  }

  @override
  void dispose() {
    _controller.dispose();
    super.dispose();
  }

  Color _accent(BuildContext context) {
    return _presentation.indicatorColor(
      UsqueTokens.of(context),
      Theme.of(context).colorScheme,
    );
  }

  @override
  Widget build(BuildContext context) {
    final UsqueTokens tokens = UsqueTokens.of(context);
    final Color accent = _accent(context);

    return Semantics(
      container: true,
      label: widget.semanticLabel,
      child: SizedBox.square(
        dimension: widget.size,
        child: Stack(
          alignment: Alignment.center,
          children: <Widget>[
            RepaintBoundary(
              child: AnimatedBuilder(
                animation: _controller,
                builder: (context, _) => CustomPaint(
                  size: Size.square(widget.size),
                  painter: _RingPainter(
                    t: _controller.value,
                    mode: _presentation.mode,
                    accent: accent,
                    track: tokens.ringTrack,
                  ),
                ),
              ),
            ),
            _PowerButton(
              diameter: widget.size * (widget.compactControl ? 0.68 : 0.47),
              label: widget.actionLabel,
              busy: widget.busy,
              engaged: _presentation.engaged,
              onPressed: widget.onPressed,
            ),
          ],
        ),
      ),
    );
  }
}

class _PowerButton extends StatefulWidget {
  const _PowerButton({
    required this.diameter,
    required this.label,
    required this.busy,
    required this.engaged,
    required this.onPressed,
  });

  final double diameter;
  final String label;
  final bool busy;

  /// True while the tunnel carries traffic, which makes the control quiet
  /// rather than a call to action.
  final bool engaged;
  final VoidCallback? onPressed;

  @override
  State<_PowerButton> createState() => _PowerButtonState();
}

class _PowerButtonState extends State<_PowerButton> {
  bool _pressed = false;
  bool _hovered = false;

  @override
  Widget build(BuildContext context) {
    final ThemeData theme = Theme.of(context);
    final UsqueTokens tokens = UsqueTokens.of(context);
    final bool enabled = widget.onPressed != null;
    final Color background = widget.engaged
        ? theme.colorScheme.surfaceContainerHigh
        : theme.colorScheme.primary;
    final Color foreground = widget.engaged
        ? theme.colorScheme.onSurface
        : theme.colorScheme.onPrimary;

    return AnimatedScale(
      scale: _pressed ? 0.97 : 1,
      duration: UsqueMotion.of(context, UsqueMotion.fast),
      curve: UsqueMotion.standard,
      child: Material(
        color: enabled
            ? background
            : theme.colorScheme.surfaceContainerHigh.withValues(alpha: 0.6),
        shape: CircleBorder(
          side: BorderSide(
            color: widget.engaged ? tokens.hairlineStrong : Colors.transparent,
          ),
        ),
        clipBehavior: Clip.antiAlias,
        child: InkWell(
          onTap: widget.onPressed,
          onHighlightChanged: (value) => setState(() => _pressed = value),
          onHover: (value) => setState(() => _hovered = value),
          overlayColor: WidgetStatePropertyAll<Color>(
            foreground.withValues(alpha: _hovered ? 0.06 : 0.10),
          ),
          child: SizedBox.square(
            dimension: widget.diameter,
            child: Padding(
              padding: EdgeInsets.symmetric(
                horizontal: widget.diameter * 0.13,
                vertical: widget.diameter * 0.18,
              ),
              // The label has to survive a 200% text scale inside a circle, so
              // it shrinks rather than pushing the icon out of the button.
              child: FittedBox(
                fit: BoxFit.scaleDown,
                child: Column(
                  mainAxisSize: MainAxisSize.min,
                  children: <Widget>[
                    if (widget.busy)
                      SizedBox(
                        width: 22,
                        height: 22,
                        child: CircularProgressIndicator(
                          strokeWidth: 2.4,
                          color: enabled ? foreground : theme.disabledColor,
                        ),
                      )
                    else
                      Icon(
                        widget.engaged
                            ? LucideIcons.powerOff
                            : LucideIcons.power,
                        size: 24,
                        color: enabled ? foreground : theme.disabledColor,
                      ),
                    const SizedBox(height: 7),
                    Text(
                      widget.label,
                      maxLines: 1,
                      textAlign: TextAlign.center,
                      style: theme.textTheme.labelLarge?.copyWith(
                        color: enabled ? foreground : theme.disabledColor,
                        fontWeight: FontWeight.w700,
                      ),
                    ),
                  ],
                ),
              ),
            ),
          ),
        ),
      ),
    );
  }
}

class _RingPainter extends CustomPainter {
  _RingPainter({
    required this.t,
    required this.mode,
    required this.accent,
    required this.track,
  });

  /// In [RingMode.scan] this is the position of the travelling head; elsewhere
  /// it is how far the bezel has closed, and rests at 1.
  final double t;
  final RingMode mode;
  final Color accent;
  final Color track;

  static const int _tickCount = 60;

  /// Length of the lit tail behind a scanning head, as a fraction of the turn.
  static const double _tail = 0.24;

  /// Half the opening left in the bezel when the tunnel is broken, in radians.
  static const double _faultGap = 0.22;

  @override
  void paint(Canvas canvas, Size size) {
    final Offset center = size.center(Offset.zero);
    final double outer = size.shortestSide / 2 - 1;
    final double radius = outer - 17;
    final double closed = mode == RingMode.steady
        ? Curves.easeOutCubic.transform(t.clamp(0, 1).toDouble())
        : 1;

    _paintGlow(canvas, center, radius, closed);
    _paintRing(canvas, center, radius, closed);
    _paintTicks(canvas, center, outer, closed);
  }

  void _paintGlow(Canvas canvas, Offset center, double radius, double closed) {
    if (mode != RingMode.steady) {
      return;
    }
    canvas.drawCircle(
      center,
      radius,
      Paint()
        ..style = PaintingStyle.stroke
        ..strokeWidth = 8
        ..color = accent.withValues(alpha: 0.16 * closed)
        ..maskFilter = const MaskFilter.blur(BlurStyle.normal, 9),
    );
    canvas.drawCircle(
      center,
      radius,
      Paint()
        ..shader = RadialGradient(
          colors: <Color>[
            accent.withValues(alpha: 0.07 * closed),
            accent.withValues(alpha: 0),
          ],
        ).createShader(Rect.fromCircle(center: center, radius: radius)),
    );
  }

  void _paintRing(Canvas canvas, Offset center, double radius, double closed) {
    final Rect bounds = Rect.fromCircle(center: center, radius: radius);
    final Paint base = Paint()
      ..style = PaintingStyle.stroke
      ..strokeWidth = 2
      ..strokeCap = StrokeCap.round
      ..color = track.withValues(alpha: 0.9);
    final Paint lit = Paint()
      ..style = PaintingStyle.stroke
      ..strokeWidth = 2.8
      ..strokeCap = StrokeCap.round
      ..color = accent;

    switch (mode) {
      case RingMode.idle:
        canvas.drawCircle(center, radius, base);
      case RingMode.scan:
        canvas.drawCircle(center, radius, base);
        final double head = -math.pi / 2 + t * 2 * math.pi;
        canvas.drawCircle(
          center,
          radius,
          Paint()
            ..style = PaintingStyle.stroke
            ..strokeWidth = 2.8
            ..strokeCap = StrokeCap.round
            ..shader = SweepGradient(
              colors: <Color>[
                accent.withValues(alpha: 0),
                accent.withValues(alpha: 0),
                accent,
              ],
              stops: const <double>[0, 1 - _tail, 1],
              transform: GradientRotation(head),
            ).createShader(bounds),
        );
      case RingMode.steady:
        canvas.drawCircle(center, radius, base);
        canvas.drawArc(bounds, -math.pi / 2, 2 * math.pi * closed, false, lit);
      case RingMode.fault:
        canvas.drawArc(
          bounds,
          -math.pi / 2 + _faultGap,
          2 * math.pi - 2 * _faultGap,
          false,
          lit,
        );
    }
  }

  void _paintTicks(Canvas canvas, Offset center, double outer, double closed) {
    final Paint paint = Paint()
      ..strokeWidth = 1.5
      ..strokeCap = StrokeCap.round;
    final Color dim = track.withValues(alpha: 0.85);

    for (int i = 0; i < _tickCount; i += 1) {
      final double fraction = i / _tickCount;
      final double angle = -math.pi / 2 + fraction * 2 * math.pi;
      final bool major = i % 5 == 0;
      final double length = major ? 10 : 5.5;
      final double intensity = _tickIntensity(fraction, closed);

      paint.color = intensity <= 0
          ? dim
          : Color.lerp(dim, accent, intensity.clamp(0, 1).toDouble())!;
      final Offset direction = Offset(math.cos(angle), math.sin(angle));
      canvas.drawLine(
        center + direction * (outer - length),
        center + direction * outer,
        paint,
      );
    }
  }

  double _tickIntensity(double fraction, double closed) {
    switch (mode) {
      case RingMode.idle:
        return 0;
      case RingMode.scan:
        final double behind = (t - fraction) % 1.0;
        return behind < _tail ? 1 - behind / _tail : 0;
      case RingMode.steady:
        return fraction <= closed ? 0.8 : 0;
      case RingMode.fault:
        final double angle = fraction * 2 * math.pi;
        final bool inGap = angle < _faultGap || angle > 2 * math.pi - _faultGap;
        return inGap ? 0 : 0.7;
    }
  }

  @override
  bool shouldRepaint(covariant _RingPainter oldDelegate) {
    return oldDelegate.t != t ||
        oldDelegate.mode != mode ||
        oldDelegate.accent != accent ||
        oldDelegate.track != track;
  }
}
