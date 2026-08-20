import 'package:flutter/material.dart';

import '../core/app_strings.dart';
import '../core/connection_presentation.dart';
import '../core/usque_motion.dart';
import '../core/usque_theme.dart';
import '../models/app_models.dart';
import '../state/window_frame.dart';

/// Height of the Flutter-drawn Windows caption, in logical pixels.
/// Keep in sync with `kCaptionHeightLogical` in the runner.
const double kWindowTitleBarHeight = 40;

/// Width of one caption button, in logical pixels.
/// Keep in sync with `kCaptionButtonWidthLogical` in the runner.
const double kWindowCaptionButtonWidth = 46;

/// The Windows caption, drawn by Flutter.
///
/// Display-only: native `WM_NCHITTEST` owns drag, snap, resize, and the three
/// window commands. Closing still goes through `WM_CLOSE` so the tray
/// preference keeps deciding what a close actually means.
class WindowTitleBar extends StatelessWidget {
  const WindowTitleBar({required this.strings, required this.phase, super.key});

  final AppStrings strings;
  final ConnectionPhase phase;

  @override
  Widget build(BuildContext context) {
    final ThemeData theme = Theme.of(context);
    final UsqueTokens tokens = UsqueTokens.of(context);
    final WindowFrame frame = WindowFrame.instance;

    return ListenableBuilder(
      listenable: frame,
      builder: (context, _) {
        return AnimatedOpacity(
          duration: UsqueMotion.of(context, UsqueMotion.base),
          opacity: frame.active ? 1 : 0.62,
          child: DecoratedBox(
            decoration: BoxDecoration(
              color: tokens.canvas,
              border: Border(
                bottom: BorderSide(
                  color: tokens.hairline.withValues(alpha: 0.7),
                ),
              ),
            ),
            child: SizedBox(
              height: kWindowTitleBarHeight,
              child: Row(
                children: <Widget>[
                  Expanded(
                    child: Padding(
                      padding: const EdgeInsets.only(left: 12),
                      child: Row(
                        children: <Widget>[
                          Image.asset(
                            'assets/branding/usque-ui-icon.png',
                            width: 17,
                            height: 17,
                            filterQuality: FilterQuality.medium,
                          ),
                          const SizedBox(width: 9),
                          Text(
                            strings.get('app_name'),
                            style: theme.textTheme.labelMedium?.copyWith(
                              color: theme.colorScheme.onSurfaceVariant,
                              letterSpacing: 0.4,
                            ),
                          ),
                          const SizedBox(width: 10),
                          _ConnectionLamp(phase: phase),
                        ],
                      ),
                    ),
                  ),
                  _WindowButton(
                    label: strings.get('window_minimize'),
                    hovered: frame.captionHover == CaptionHover.min,
                    glyph: _Glyph.minimize,
                  ),
                  _WindowButton(
                    label: strings.get(
                      frame.maximized ? 'window_restore' : 'window_maximize',
                    ),
                    hovered: frame.captionHover == CaptionHover.max,
                    glyph: frame.maximized ? _Glyph.restore : _Glyph.maximize,
                  ),
                  _WindowButton(
                    label: strings.get('window_close'),
                    hovered: frame.captionHover == CaptionHover.close,
                    danger: true,
                    glyph: _Glyph.close,
                  ),
                ],
              ),
            ),
          ),
        );
      },
    );
  }
}

/// Small tinted dot mirroring the connection ring, so the window state is
/// readable while the app is a thin strip at the top of the screen.
class _ConnectionLamp extends StatelessWidget {
  const _ConnectionLamp({required this.phase});

  final ConnectionPhase phase;

  @override
  Widget build(BuildContext context) {
    final ThemeData theme = Theme.of(context);
    final Color color = ConnectionPresentation.of(
      phase,
    ).indicatorColor(UsqueTokens.of(context), theme.colorScheme);
    return AnimatedContainer(
      duration: UsqueMotion.of(context, UsqueMotion.base),
      width: 6,
      height: 6,
      decoration: BoxDecoration(color: color, shape: BoxShape.circle),
    );
  }
}

enum _Glyph { minimize, maximize, restore, close }

class _WindowButton extends StatelessWidget {
  const _WindowButton({
    required this.label,
    required this.hovered,
    required this.glyph,
    this.danger = false,
  });

  final String label;
  final bool hovered;
  final _Glyph glyph;
  final bool danger;

  @override
  Widget build(BuildContext context) {
    final ThemeData theme = Theme.of(context);
    final Color background = danger
        ? const Color(0xFFC42B1C)
        : theme.colorScheme.onSurface.withValues(alpha: 0.08);
    final Color foreground = hovered && danger
        ? Colors.white
        : theme.colorScheme.onSurfaceVariant;

    // No Tooltip here on purpose: the caption is built by MaterialApp.builder,
    // above the navigator, so there is no Overlay for one to live in. The
    // semantics label carries the same text to assistive tech.
    return Semantics(
      button: true,
      label: label,
      child: AnimatedContainer(
        duration: UsqueMotion.of(context, UsqueMotion.fast),
        width: kWindowCaptionButtonWidth,
        height: kWindowTitleBarHeight,
        color: hovered ? background : Colors.transparent,
        child: CustomPaint(painter: _GlyphPainter(glyph, foreground)),
      ),
    );
  }
}

/// Draws a caption glyph centred in the button, on Windows' 10px grid.
class _GlyphPainter extends CustomPainter {
  const _GlyphPainter(this.glyph, this.color);

  final _Glyph glyph;
  final Color color;

  @override
  void paint(Canvas canvas, Size size) {
    const double box = 10;
    final Paint paint = Paint()
      ..style = PaintingStyle.stroke
      ..strokeWidth = 1
      ..color = color;
    canvas.save();
    canvas.translate(
      ((size.width - box) / 2).roundToDouble(),
      ((size.height - box) / 2).roundToDouble(),
    );
    switch (glyph) {
      case _Glyph.minimize:
        canvas.drawLine(const Offset(0, 5.5), const Offset(10, 5.5), paint);
      case _Glyph.maximize:
        canvas.drawRect(const Rect.fromLTWH(0.5, 0.5, 9, 9), paint);
      case _Glyph.restore:
        canvas.drawRect(const Rect.fromLTWH(0.5, 2.5, 7, 7), paint);
        canvas.drawPath(
          Path()
            ..moveTo(2.5, 2)
            ..lineTo(2.5, 0.5)
            ..lineTo(9.5, 0.5)
            ..lineTo(9.5, 7.5)
            ..lineTo(8, 7.5),
          paint,
        );
      case _Glyph.close:
        canvas.drawLine(Offset.zero, const Offset(10, 10), paint);
        canvas.drawLine(const Offset(10, 0), const Offset(0, 10), paint);
    }
    canvas.restore();
  }

  @override
  bool shouldRepaint(covariant _GlyphPainter oldDelegate) =>
      oldDelegate.color != color || oldDelegate.glyph != glyph;
}

/// Wraps the application in the Windows caption.
class WindowFrameScaffold extends StatelessWidget {
  const WindowFrameScaffold({
    required this.strings,
    required this.phase,
    required this.child,
    super.key,
  });

  final AppStrings strings;
  final ConnectionPhase phase;
  final Widget child;

  @override
  Widget build(BuildContext context) {
    if (!WindowFrame.instance.enabled) {
      return child;
    }
    return ColoredBox(
      color: UsqueTokens.of(context).canvas,
      child: Column(
        children: <Widget>[
          IgnorePointer(
            child: WindowTitleBar(strings: strings, phase: phase),
          ),
          Expanded(child: child),
        ],
      ),
    );
  }
}
