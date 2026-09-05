import 'dart:async';
import 'dart:collection';
import 'dart:math' as math;

import 'package:flutter/foundation.dart';

import '../models/app_models.dart';
import '../models/network_quality_models.dart';
import '../services/engine_client.dart';

/// Process-local observations, not UI-timer samples. Nothing is persisted.
class NetworkQualityController extends ChangeNotifier {
  NetworkQualityController(
    this._engine, {
    DateTime Function()? now,
    this.autoTick = true,
  }) : _now = now ?? DateTime.now;

  static const int historyCapacity = 300;
  static const Duration staleAfter = Duration(seconds: 3);
  final EngineClient _engine;
  final DateTime Function() _now;
  final bool autoTick;
  final _quality = SplayTreeMap<int, ({int? rtt, int? loss})>();
  final _counters = SplayTreeMap<int, _CounterReading>();
  final _retiredIds = ListQueue<String>();
  Timer? _timer;
  bool _disposed = false;
  bool _enabled = false;
  bool _refreshing = false;
  bool _streamUnavailable = false;
  DateTime? _receivedAt;
  DateTime? _origin;
  DateTime? _counterOrigin;
  DateTime? _acceptAfter;
  int _counterStartSlot = 0;
  int _lastSourceSequence = 0;
  int? _lastSourceMonotonic;
  bool _sourceHistoryActive = false;
  DateTime? _pausedAt;
  int? _pausedSlot;
  DateTime? _lastSnapshotAt;
  EngineSnapshot? _lastSnapshot;
  String? _connectionId;
  int _epoch = 0;
  int _counterEpoch = 0;
  NetworkQualitySnapshot? latest;
  EngineSnapshot connection = const EngineSnapshot();

  bool get enabled => _enabled;
  bool get refreshing => _refreshing;
  bool get paused => _pausedAt != null;
  DateTime get windowEnd => _pausedAt ?? _now();
  Duration? get sampleAge {
    final sampled = latest?.sampledAt;
    if (sampled == null) return null;
    final age = _now().difference(sampled);
    return age.isNegative ? Duration.zero : age;
  }

  bool get stale {
    final sampled = latest?.sampledAt;
    final received = _receivedAt;
    if (_streamUnavailable || sampled == null || received == null) return true;
    final now = _now();
    return now.difference(received) > staleAfter ||
        now.difference(sampled) > staleAfter ||
        sampled.difference(now) > staleAfter;
  }

  List<NetworkQualityPoint> get history {
    if (_origin == null) return const [];
    final slots = <int>{..._quality.keys, ..._counters.keys}.toList()..sort();
    return List.unmodifiable(
      slots.map((slot) {
        final metrics = _quality[slot];
        return NetworkQualityPoint(
          at: _origin!.add(Duration(seconds: slot)),
          rttMilliseconds: metrics?.rtt,
          lossBasisPoints: metrics?.loss,
          downloadBytesPerSecond: _intervalRate(slot, true),
          uploadBytesPerSecond: _intervalRate(slot, false),
        );
      }),
    );
  }

  void setEnabled(bool value) {
    if (_disposed || value == _enabled) return;
    _enabled = value;
    if (!value) _clear();
    _syncTimer();
    notifyListeners();
  }

  /// Legacy state counters are sampled only from full state events. Source
  /// history already owns its counters, including in quality-only replies.
  void updateConnection(EngineSnapshot value) {
    if (_disposed) return;
    final now = _now();
    final id = value.networkQuality?.connectionInstanceId;
    if (value.isConnected && id != null && _retiredIds.contains(id)) return;
    final previousSourceAt = _lastSnapshot?.networkQuality?.sampledAt;
    final sourceAt = value.networkQuality?.sampledAt;
    final clockRollback =
        _lastSnapshotAt != null && now.isBefore(_lastSnapshotAt!);
    if (!clockRollback &&
        id == _connectionId &&
        sourceAt != null &&
        previousSourceAt != null &&
        sourceAt.isBefore(previousSourceAt)) {
      return;
    }
    connection = value;
    if (value.phase == ConnectionPhase.disconnected ||
        value.phase == ConnectionPhase.error) {
      _clear(retire: true);
    } else if (_enabled) {
      if (clockRollback) {
        _clear(); // Wall-clock rollback cannot form a negative rate interval.
      }
      if (value.networkQuality case final quality?) _acceptQuality(quality);
      if (!_sourceHistoryActive &&
          !paused &&
          value.isConnected &&
          !stale &&
          _origin != null &&
          (value != _lastSnapshot ||
              value.networkQuality?.sampledAt !=
                  _lastSnapshot?.networkQuality?.sampledAt)) {
        // Legacy engines lack source counter timestamps. Give the receipt
        // stream its own phase so a fixed delivery delay cannot straddle the
        // quality stream's half-second boundary.
        if (_counterOrigin == null) {
          _counterOrigin = now;
          _counterStartSlot = _slot(sourceAt ?? now);
        }
        final slot =
            _counterStartSlot +
            (now.difference(_counterOrigin!).inMicroseconds / 1000000).round();
        _counters[slot] = _CounterReading(
          now,
          value.downloadedBytes,
          value.uploadedBytes,
          _counterEpoch,
        );
        _trim();
      }
      _lastSnapshot = value;
      _lastSnapshotAt = now;
      if (!value.isConnected) {
        _counterEpoch++;
        _acceptAfter = now;
      }
    }
    _syncTimer();
    notifyListeners();
  }

  void accept(NetworkQualitySnapshot value) {
    if (_disposed) return;
    _acceptQuality(value);
    notifyListeners();
  }

  void _acceptQuality(NetworkQualitySnapshot value) {
    final at = value.sampledAt;
    final id = value.connectionInstanceId;
    final now = _now();
    if (at == null || id == null || id.isEmpty) {
      latest ??= value;
      return;
    }
    if (_retiredIds.contains(id) ||
        now.difference(at) > staleAfter ||
        at.isAfter(now)) {
      return;
    }
    if (id == _connectionId && latest?.sampledAt != null) {
      if (at.isBefore(latest!.sampledAt!)) return;
      if (at == latest!.sampledAt &&
          (_origin != null || paused || !connection.isConnected)) {
        return;
      }
    }
    if (id != _connectionId) {
      _clear(retire: true);
      _connectionId = id;
    }
    latest = value;
    _receivedAt = now;
    // A fresh, validated fallback reply is data even while the app continues
    // to report the event pipe as degraded. It does not heal the pipe itself.
    _streamUnavailable = false;
    if (_enabled && !paused && connection.isConnected && !_streamUnavailable) {
      if (value.samples.isNotEmpty) {
        _acceptSourceSamples(value);
        return;
      }
      if (_sourceHistoryActive) return;
      _origin ??= at;
      _quality[_slot(at)] = (
        rtt:
            availableMetric(
              value.metrics.latestRttMilliseconds,
              value.metrics.latestRttAvailability,
            ) ??
            availableMetric(
              value.metrics.smoothedRttMilliseconds,
              value.metrics.smoothedRttAvailability,
            ),
        loss: availableMetric(
          value.metrics.intervalLossBasisPoints,
          value.metrics.intervalLossAvailability,
        ),
      );
      _trim();
    }
  }

  void _acceptSourceSamples(NetworkQualitySnapshot value) {
    _sourceHistoryActive = true;
    // The compact history crosses coalescing boundaries intact. Duplicate
    // frames are immutable; no timer or cached full snapshot adds a reading.
    for (final sample in value.samples.take(16)) {
      if (sample.sequence <= _lastSourceSequence ||
          sample.sequence <= 0 ||
          sample.monotonicMillis < 0 ||
          sample.monotonicMillis > 8640000000000000 ||
          (_lastSourceMonotonic != null &&
              sample.monotonicMillis <= _lastSourceMonotonic!) ||
          sample.sampledAt.isAfter(value.sampledAt!) ||
          value.sampledAt!.difference(sample.sampledAt) >
              const Duration(seconds: 30)) {
        continue;
      }
      _lastSourceSequence = sample.sequence;
      _lastSourceMonotonic = sample.monotonicMillis;
      if (_acceptAfter != null && !sample.sampledAt.isAfter(_acceptAfter!)) {
        continue;
      }
      _origin ??= sample.sampledAt.subtract(
        Duration(milliseconds: sample.monotonicMillis),
      );
      final slot = (sample.monotonicMillis / 1000).round();
      _quality[slot] = (
        rtt: sample.rttMilliseconds,
        loss: sample.lossBasisPoints,
      );
      final down = sample.downloadedBytes;
      final up = sample.uploadedBytes;
      if (down != null && up != null && down >= 0 && up >= 0) {
        _counters[slot] = _CounterReading(
          // Rates use source monotonic elapsed time, independent of delivery
          // batching, wall-clock corrections and presentation slot rounding.
          DateTime.fromMillisecondsSinceEpoch(
            sample.monotonicMillis,
            isUtc: true,
          ),
          down,
          up,
          _counterEpoch,
          sequence: sample.sequence,
        );
      }
    }
    _trim();
  }

  void markStreamUnavailable(bool value) {
    if (_disposed || value == _streamUnavailable) return;
    _streamUnavailable = value;
    if (value) _acceptAfter = _now();
    _counterEpoch++; // Do not compute an interval across a known stream outage.
    notifyListeners();
  }

  void togglePaused() {
    _pausedSlot = paused || _origin == null ? null : _windowLastSlot;
    _pausedAt = paused ? null : _now();
    if (!paused) _acceptAfter = _now();
    _counterEpoch++;
    _lastSnapshot = null;
    notifyListeners();
  }

  void _clear({bool retire = false}) {
    if (retire && _connectionId != null) {
      _retiredIds.add(_connectionId!);
      while (_retiredIds.length > 8) {
        _retiredIds.removeFirst();
      }
    }
    _epoch++;
    _counterEpoch++;
    latest = null;
    _quality.clear();
    _counters.clear();
    _origin = null;
    _counterOrigin = null;
    _acceptAfter = null;
    _counterStartSlot = 0;
    _lastSourceSequence = 0;
    _lastSourceMonotonic = null;
    _sourceHistoryActive = false;
    _receivedAt = null;
    _lastSnapshot = null;
    _lastSnapshotAt = null;
    _connectionId = null;
    _pausedAt = null;
    _pausedSlot = null;
  }

  // Presentation only. Source rates use monotonic sample timestamps; legacy
  // receipt counters have an independent phase. Never interpolate absent beats.
  int _slot(DateTime at) =>
      (at.difference(_origin!).inMicroseconds / 1000000).round();

  void _trim() {
    final newest = math.max(_quality.lastKey() ?? 0, _counters.lastKey() ?? 0);
    final oldest = newest - historyCapacity + 1;
    _quality.removeWhere((slot, _) => slot < oldest);
    _counters.removeWhere((slot, _) => slot < oldest);
  }

  int get _windowLastSlot {
    if (paused && _pausedSlot != null) return _pausedSlot!;
    final newest = math.max(_quality.lastKey() ?? 0, _counters.lastKey() ?? 0);
    // Render the most recent complete source frame while the next delivery is
    // in flight. Transport latency is not a missing future measurement.
    if (_sourceHistoryActive &&
        !stale &&
        _receivedAt != null &&
        windowEnd.difference(_receivedAt!) <
            const Duration(milliseconds: 1500)) {
      return newest;
    }
    // An in-flight current beat is not yet a missing sample. After a complete
    // beat passes, advance the window even with no events so real gaps appear.
    return math.max(newest, _slot(windowEnd) - 1);
  }

  bool _validInterval(int slot) {
    final a = _counters[slot - 1];
    final b = _counters[slot];
    if (a == null || b == null || a.epoch != b.epoch) return false;
    if (a.sequence != null && b.sequence != a.sequence! + 1) return false;
    final elapsed = b.at.difference(a.at).inMicroseconds;
    return a.down >= 0 &&
        a.up >= 0 &&
        elapsed > 0 &&
        elapsed <= 2000000 &&
        b.down >= a.down &&
        b.up >= a.up;
  }

  int? _intervalRate(int slot, bool download) {
    if (!_validInterval(slot)) return null;
    final a = _counters[slot - 1]!;
    final b = _counters[slot]!;
    final bytes = download ? b.down - a.down : b.up - a.up;
    return (bytes * 1000000 / b.at.difference(a.at).inMicroseconds).round();
  }

  List<int?> trace(int? Function(NetworkQualityPoint point) value) {
    final samples = List<int?>.filled(60, null);
    if (_origin == null) return List.unmodifiable(samples);
    final end = _windowLastSlot;
    for (final point in history) {
      final index = _slot(point.at) - (end - 59);
      if (index >= 0 && index < samples.length) samples[index] = value(point);
    }
    return List.unmodifiable(samples);
  }

  int? rateAverage({required bool download, required int seconds}) {
    assert(seconds > 0 && seconds <= 60);
    if (stale || paused || _origin == null) return null;
    final end = _windowLastSlot;
    for (var slot = end - seconds + 1; slot <= end; slot++) {
      if (!_validInterval(slot)) return null;
    }
    final a = _counters[end - seconds]!;
    final b = _counters[end]!;
    final bytes = download ? b.down - a.down : b.up - a.up;
    return (bytes * 1000000 / b.at.difference(a.at).inMicroseconds).round();
  }

  void _syncTimer() {
    if (!autoTick || !_enabled || !connection.isConnected) {
      _timer?.cancel();
      _timer = null;
    } else {
      _timer ??= Timer.periodic(const Duration(seconds: 1), (_) => tick());
    }
  }

  @visibleForTesting
  void tick() {
    if (_disposed || !_enabled) return;
    // Only age/repaint existing observations. A timer is not a measurement.
    if (!paused &&
        connection.isConnected &&
        (_receivedAt == null ||
            _now().difference(_receivedAt!) >= const Duration(seconds: 2))) {
      unawaited(refresh());
    }
    notifyListeners();
  }

  /// Exactly one outstanding IPC request. Epoch guards reject late replies.
  Future<void> refresh() async {
    if (_disposed || !_enabled || _refreshing) return;
    _refreshing = true;
    final epoch = _epoch;
    notifyListeners();
    try {
      final snapshot = await _engine.getNetworkQuality();
      if (!_disposed && _enabled && epoch == _epoch && snapshot != null) {
        accept(snapshot);
      }
    } on Object {
      // Existing readings age into Stale; raw transport errors stay out of UI.
    } finally {
      _refreshing = false;
      if (!_disposed) notifyListeners();
    }
  }

  @override
  void dispose() {
    _disposed = true;
    _timer?.cancel();
    _quality.clear();
    _counters.clear();
    super.dispose();
  }
}

class _CounterReading {
  const _CounterReading(
    this.at,
    this.down,
    this.up,
    this.epoch, {
    this.sequence,
  });
  final DateTime at;
  final int down;
  final int up;
  final int epoch;
  final int? sequence;
}
