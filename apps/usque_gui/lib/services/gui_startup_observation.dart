import 'dart:async';

import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';

import 'control_codec.dart';

/// Fixed, process-local evidence; never credentials or platform health.
class GuiStartupObservation {
  GuiStartupObservation({
    @visibleForTesting int Function()? elapsedMilliseconds,
  }) : _readElapsed = elapsedMilliseconds;
  static final instance = GuiStartupObservation();
  static const _channel = MethodChannel('io.github.georgexie2333.usque/engine');
  final _clock = Stopwatch();
  final int Function()? _readElapsed;
  int get _elapsed => _readElapsed?.call() ?? _clock.elapsedMilliseconds;
  final _stages = <int, ({int status, int? elapsed})>{};
  Timer? _timer;
  int _lastTick = 0;
  int _maxGap = 0;
  bool get enabled => _clock.isRunning;

  void start() {
    if (enabled) return;
    _clock.start();
    for (var stage = 2; stage <= 7; stage++) {
      _stages[stage] = (status: 1, elapsed: null);
    }
    _timer = Timer.periodic(const Duration(milliseconds: 100), (_) {
      final now = _elapsed;
      _maxGap = (now - _lastTick).clamp(_maxGap, 60000);
      _lastTick = now;
      if (now >= 60000) _timer?.cancel();
    });
  }

  void mark(int stage, {bool failed = false}) {
    if (!enabled || _elapsed > 60000 || stage < 2 || stage > 7) return;
    if (_stages[stage]?.status == 2) return;
    _stages[stage] = (status: failed ? 3 : 2, elapsed: _elapsed);
  }

  Future<Uint8List?> encode() async {
    if (!enabled) return null;
    final elapsed = _elapsed.clamp(0, 60000);
    final writer = ControlPayloadWriter()
      ..optionalUnsigned(1, 1)
      ..optionalUnsigned(2, elapsed)
      ..boolean(6, elapsed == 60000);
    if (_lastTick > 0) writer.optionalUnsigned(3, _maxGap);
    for (final entry in _stages.entries) {
      final timedOut = elapsed == 60000 && entry.value.status == 1;
      final value = ControlPayloadWriter()
        ..optionalUnsigned(1, entry.key)
        ..optionalUnsigned(2, 1)
        ..optionalUnsigned(3, timedOut ? 4 : entry.value.status);
      final time = timedOut ? 60000 : entry.value.elapsed;
      if (time != null) value.optionalUnsigned(4, time);
      writer.message(5, value.takeBytes());
    }
    try {
      final native = await _channel
          .invokeMapMethod<String, Object?>('guiStartupObservation')
          .timeout(const Duration(seconds: 1));
      final gap = native?['max_gap_ms'];
      if (gap is int && gap >= 0 && gap <= 60000) {
        writer.optionalUnsigned(4, gap);
      }
      final stages = native?['stages'];
      if (stages is List && stages.length <= 2) {
        for (final raw in stages) {
          if (raw is! Map) continue;
          final stage = raw['stage'];
          final status = raw['status'];
          final time = raw['elapsed_ms'];
          if (stage is! int ||
              stage < 1 ||
              stage > 2 ||
              status is! int ||
              status < 1 ||
              status > 4 ||
              (time != null && (time is! int || time < 0 || time > 60000)) ||
              (status == 1) != (time == null)) {
            continue;
          }
          final value = ControlPayloadWriter()
            ..optionalUnsigned(1, stage)
            ..optionalUnsigned(2, 2)
            ..optionalUnsigned(3, status);
          if (time is int) value.optionalUnsigned(4, time);
          writer.message(5, value.takeBytes());
        }
      }
    } on Object {
      // Older native shells contribute no GTK evidence, not a zero pause.
    }
    return writer.takeBytes();
  }

  @visibleForTesting
  void dispose() => _timer?.cancel();
}
