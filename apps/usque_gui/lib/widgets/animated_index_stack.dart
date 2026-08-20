import 'package:flutter/material.dart';

import '../core/usque_motion.dart';

/// [IndexedStack] with a fade-through between sections.
///
/// Every child stays mounted, so scroll offsets and half-typed fields survive a
/// section switch exactly as they did with a plain [IndexedStack]. Hidden
/// children are taken out of paint, hit testing, and semantics, and their
/// tickers are muted so background pages cannot animate.
class AnimatedIndexStack extends StatefulWidget {
  const AnimatedIndexStack({
    required this.index,
    required this.children,
    this.duration = UsqueMotion.gentle,
    super.key,
  });

  final int index;
  final List<Widget> children;
  final Duration duration;

  @override
  State<AnimatedIndexStack> createState() => _AnimatedIndexStackState();
}

class _AnimatedIndexStackState extends State<AnimatedIndexStack>
    with SingleTickerProviderStateMixin {
  late final AnimationController _controller = AnimationController(
    vsync: this,
    duration: widget.duration,
    value: 1,
  );
  late final CurvedAnimation _enter = CurvedAnimation(
    parent: _controller,
    curve: const Interval(0.25, 1, curve: UsqueMotion.emphasized),
  );
  late final CurvedAnimation _leave = CurvedAnimation(
    parent: _controller,
    curve: const Interval(0, 0.45, curve: UsqueMotion.exit),
  );
  late final Animation<double> _leaveOpacity = Tween<double>(
    begin: 1,
    end: 0,
  ).animate(_leave);
  late final Animation<Offset> _enterOffset = Tween<Offset>(
    begin: const Offset(0, 0.014),
    end: Offset.zero,
  ).animate(_enter);

  late int _incoming = widget.index;
  int? _outgoing;

  @override
  void didUpdateWidget(covariant AnimatedIndexStack oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.index == widget.index) {
      return;
    }
    _outgoing = _incoming;
    _incoming = widget.index;
    _controller
      ..duration = UsqueMotion.of(context, widget.duration)
      ..forward(from: 0).whenComplete(() {
        if (mounted) {
          setState(() => _outgoing = null);
        }
      });
  }

  @override
  void dispose() {
    _enter.dispose();
    _leave.dispose();
    _controller.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return Stack(
      fit: StackFit.expand,
      children: <Widget>[
        for (int i = 0; i < widget.children.length; i += 1)
          _Section(
            key: ValueKey<int>(i),
            visible: i == _incoming || i == _outgoing,
            interactive: i == _incoming,
            opacity: i == _incoming ? _enter : _leaveOpacity,
            offset: i == _incoming ? _enterOffset : _Section.still,
            child: widget.children[i],
          ),
      ],
    );
  }
}

class _Section extends StatelessWidget {
  const _Section({
    required this.visible,
    required this.interactive,
    required this.opacity,
    required this.offset,
    required this.child,
    super.key,
  });

  final bool visible;
  final bool interactive;
  final Animation<double> opacity;
  final Animation<Offset> offset;
  final Widget child;

  /// Stand-in for the leaving section. Every section is wrapped in the same
  /// widgets whatever its role, because a wrapper that comes and goes would
  /// reparent the page and throw away its state on every switch.
  static const Animation<Offset> still = AlwaysStoppedAnimation<Offset>(
    Offset.zero,
  );

  @override
  Widget build(BuildContext context) {
    // Offstage keeps the subtree alive and findable while removing it from
    // paint, hit testing, and semantics, matching IndexedStack semantics.
    return Offstage(
      offstage: !visible,
      child: TickerMode(
        enabled: interactive,
        child: IgnorePointer(
          ignoring: !interactive,
          child: FadeTransition(
            opacity: opacity,
            child: SlideTransition(position: offset, child: child),
          ),
        ),
      ),
    );
  }
}
