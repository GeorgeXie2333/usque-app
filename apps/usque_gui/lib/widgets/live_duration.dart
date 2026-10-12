import 'dart:async';

import 'package:flutter/material.dart';

import '../core/usque_theme.dart';
import 'common.dart';

/// Formats [since] as a running clock. Rebuilds once a second while visible so
/// a parent that only listens to [since] itself does not freeze the readout.
class LiveDuration extends StatefulWidget {
  const LiveDuration({required this.since, this.now = DateTime.now, super.key});

  /// Start of the interval. Null renders an em dash and runs no timer.
  final DateTime? since;

  /// Clock used to format the interval. Tests inject a fake so fake-async
  /// pumps do not depend on wall time.
  final DateTime Function() now;

  @override
  State<LiveDuration> createState() => _LiveDurationState();
}

class _LiveDurationState extends State<LiveDuration>
    with WidgetsBindingObserver {
  Timer? _timer;
  bool _tickerEnabled = false;
  late bool _appVisible;

  bool get _shouldTick => widget.since != null && _tickerEnabled && _appVisible;

  @override
  void initState() {
    super.initState();
    _appVisible = _isVisible(WidgetsBinding.instance.lifecycleState);
    WidgetsBinding.instance.addObserver(this);
  }

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    // Section navigation and opaque routes mute tickers but do not cancel
    // ordinary Timers. Subscribe to the same visibility signal explicitly.
    _tickerEnabled = TickerMode.valuesOf(context).enabled;
    _syncTimer();
  }

  @override
  void didChangeAppLifecycleState(AppLifecycleState state) {
    if (!mounted) return;
    final visible = _isVisible(state);
    if (_appVisible == visible) return;
    _appVisible = visible;
    _syncTimer();
    if (mounted && _shouldTick) {
      // Recalculate from the clock immediately, including time spent hidden.
      setState(() {});
    }
  }

  static bool _isVisible(AppLifecycleState? state) => switch (state) {
    // No notification yet must not freeze the initial readout. Inactive can
    // still be visible, such as an unfocused desktop window or permission UI.
    null || AppLifecycleState.resumed || AppLifecycleState.inactive => true,
    AppLifecycleState.hidden ||
    AppLifecycleState.paused ||
    AppLifecycleState.detached => false,
  };

  @override
  void didUpdateWidget(covariant LiveDuration oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.since != widget.since) {
      _syncTimer();
    }
  }

  void _syncTimer() {
    if (!_shouldTick) {
      _timer?.cancel();
      _timer = null;
      return;
    }
    _timer ??= Timer.periodic(const Duration(seconds: 1), (_) {
      if (mounted && _shouldTick) {
        setState(() {});
      }
    });
  }

  @override
  void dispose() {
    WidgetsBinding.instance.removeObserver(this);
    _timer?.cancel();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final DateTime? since = widget.since;
    if (since == null) {
      return const EmptyValue(label: '—');
    }
    return Text(
      formatDuration(widget.now().difference(since)),
      style: UsqueTheme.readout(context),
    );
  }
}
