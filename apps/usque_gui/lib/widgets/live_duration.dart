import 'dart:async';

import 'package:flutter/material.dart';

import 'common.dart';

/// Formats [since] as a running clock. Rebuilds once a second so a parent
/// that only listens to [since] itself does not freeze the readout.
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

class _LiveDurationState extends State<LiveDuration> {
  Timer? _timer;

  @override
  void initState() {
    super.initState();
    _syncTimer();
  }

  @override
  void didUpdateWidget(covariant LiveDuration oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.since != widget.since) {
      _syncTimer();
    }
  }

  void _syncTimer() {
    _timer?.cancel();
    _timer = null;
    if (widget.since == null) {
      return;
    }
    _timer = Timer.periodic(const Duration(seconds: 1), (_) {
      if (mounted) {
        setState(() {});
      }
    });
  }

  @override
  void dispose() {
    _timer?.cancel();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final DateTime? since = widget.since;
    if (since == null) {
      return const EmptyValue(label: '—');
    }
    return MonoValue(value: formatDuration(widget.now().difference(since)));
  }
}
