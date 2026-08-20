import 'package:flutter/material.dart';

import '../core/app_strings.dart';
import '../core/usque_motion.dart';
import '../core/usque_theme.dart';
import '../models/app_models.dart';
import '../state/window_frame.dart';

/// Height of the Flutter-drawn Windows caption, in logical pixels.
const double kWindowTitleBarHeight = 40;

/// Thickness of the invisible strips that start a native resize loop.
const double _kResizeEdge = 5;

/// The Windows caption, drawn by Flutter.
///
/// Holds the brand mark, a live connection lamp, the drag region, and the three
/// window commands. Closing goes through the shell so the tray "close to tray"
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
                    child: GestureDetector(
                      behavior: HitTestBehavior.opaque,
                      onPanStart: (_) => frame.startDrag(),
                      onDoubleTap: frame.toggleMaximize,
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
                  ),
                  _WindowButton(
                    label: strings.get('window_minimize'),
                    onPressed: frame.minimize,
                    glyph: _Glyph.minimize,
                  ),
                  _WindowButton(
                    label: strings.get(
                      frame.maximized ? 'window_restore' : 'window_maximize',
                    ),
                    onPressed: frame.toggleMaximize,
                    glyph: frame.maximized ? _Glyph.restore : _Glyph.maximize,
                  ),
                  _WindowButton(
                    label: strings.get('window_close'),
                    onPressed: frame.close,
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
    final UsqueTokens tokens = UsqueTokens.of(context);
    final Color color = switch (phase) {
      ConnectionPhase.connected => tokens.success,
      ConnectionPhase.degraded ||
      ConnectionPhase.reconnecting => tokens.caution,
      ConnectionPhase.error => tokens.danger,
      ConnectionPhase.preparing ||
      ConnectionPhase.connectingH3 ||
      ConnectionPhase.connectingH2 ||
      ConnectionPhase.disconnecting => tokens.brand,
      ConnectionPhase.disconnected => tokens.hairlineStrong,
    };
    return AnimatedContainer(
      duration: UsqueMotion.of(context, UsqueMotion.base),
      width: 6,
      height: 6,
      decoration: BoxDecoration(color: color, shape: BoxShape.circle),
    );
  }
}

enum _Glyph { minimize, maximize, restore, close }

class _WindowButton extends StatefulWidget {
  const _WindowButton({
    required this.label,
    required this.onPressed,
    required this.glyph,
    this.danger = false,
  });

  final String label;
  final VoidCallback onPressed;
  final _Glyph glyph;
  final bool danger;

  @override
  State<_WindowButton> createState() => _WindowButtonState();
}

class _WindowButtonState extends State<_WindowButton> {
  bool _hovered = false;

  @override
  Widget build(BuildContext context) {
    final ThemeData theme = Theme.of(context);
    final Color background = widget.danger
        ? const Color(0xFFC42B1C)
        : theme.colorScheme.onSurface.withValues(alpha: 0.08);
    final Color foreground = _hovered && widget.danger
        ? Colors.white
        : theme.colorScheme.onSurfaceVariant;

    // No Tooltip here on purpose: the caption is built by MaterialApp.builder,
    // above the navigator, so there is no Overlay for one to live in. The
    // semantics label carries the same text to assistive tech.
    return Semantics(
      button: true,
      label: widget.label,
      child: MouseRegion(
        onEnter: (_) => setState(() => _hovered = true),
        onExit: (_) => setState(() => _hovered = false),
        child: GestureDetector(
          behavior: HitTestBehavior.opaque,
          onTap: widget.onPressed,
          child: AnimatedContainer(
            duration: UsqueMotion.of(context, UsqueMotion.fast),
            width: 46,
            height: kWindowTitleBarHeight,
            color: _hovered ? background : Colors.transparent,
            child: CustomPaint(
              painter: _GlyphPainter(widget.glyph, foreground),
            ),
          ),
        ),
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

/// Invisible strips along the window edges that start a native resize loop.
///
/// The Flutter view covers the whole window once the caption is removed, so
/// Windows never sees the pointer near the frame; these strips hand it back.
class WindowResizeEdges extends StatelessWidget {
  const WindowResizeEdges({super.key});

  @override
  Widget build(BuildContext context) {
    final WindowFrame frame = WindowFrame.instance;
    return ListenableBuilder(
      listenable: frame,
      builder: (context, _) {
        if (frame.maximized) {
          return const SizedBox.shrink();
        }
        return Stack(
          children: <Widget>[
            _edge(frame, WindowEdge.top, top: 0, left: 0, right: 0),
            _edge(frame, WindowEdge.bottom, bottom: 0, left: 0, right: 0),
            _edge(frame, WindowEdge.left, top: 0, bottom: 0, left: 0),
            _edge(frame, WindowEdge.right, top: 0, bottom: 0, right: 0),
            _corner(frame, WindowEdge.topLeft, top: 0, left: 0),
            _corner(frame, WindowEdge.topRight, top: 0, right: 0),
            _corner(frame, WindowEdge.bottomLeft, bottom: 0, left: 0),
            _corner(frame, WindowEdge.bottomRight, bottom: 0, right: 0),
          ],
        );
      },
    );
  }

  Widget _edge(
    WindowFrame frame,
    WindowEdge edge, {
    double? top,
    double? bottom,
    double? left,
    double? right,
  }) {
    final bool horizontal = edge == WindowEdge.top || edge == WindowEdge.bottom;
    return Positioned(
      top: top,
      bottom: bottom,
      left: left,
      right: right,
      width: horizontal ? null : _kResizeEdge,
      height: horizontal ? _kResizeEdge : null,
      child: _ResizeHandle(frame: frame, edge: edge),
    );
  }

  Widget _corner(
    WindowFrame frame,
    WindowEdge edge, {
    double? top,
    double? bottom,
    double? left,
    double? right,
  }) {
    return Positioned(
      top: top,
      bottom: bottom,
      left: left,
      right: right,
      width: _kResizeEdge * 2.4,
      height: _kResizeEdge * 2.4,
      child: _ResizeHandle(frame: frame, edge: edge),
    );
  }
}

class _ResizeHandle extends StatelessWidget {
  const _ResizeHandle({required this.frame, required this.edge});

  final WindowFrame frame;
  final WindowEdge edge;

  @override
  Widget build(BuildContext context) {
    return MouseRegion(
      cursor: switch (edge) {
        WindowEdge.left ||
        WindowEdge.right => SystemMouseCursors.resizeLeftRight,
        WindowEdge.top || WindowEdge.bottom => SystemMouseCursors.resizeUpDown,
        WindowEdge.topLeft ||
        WindowEdge.bottomRight => SystemMouseCursors.resizeUpLeftDownRight,
        WindowEdge.topRight ||
        WindowEdge.bottomLeft => SystemMouseCursors.resizeUpRightDownLeft,
      },
      child: Listener(
        behavior: HitTestBehavior.opaque,
        onPointerDown: (_) => frame.startResize(edge),
      ),
    );
  }
}

/// Wraps the application in the Windows caption and its resize strips.
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
      child: Stack(
        children: <Widget>[
          Column(
            children: <Widget>[
              WindowTitleBar(strings: strings, phase: phase),
              Expanded(child: child),
            ],
          ),
          const Positioned.fill(child: WindowResizeEdges()),
        ],
      ),
    );
  }
}
