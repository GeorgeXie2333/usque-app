import 'dart:async';
import 'dart:collection';

import 'package:flutter/foundation.dart';

import '../models/app_models.dart';
import '../models/network_quality_models.dart';
import '../services/engine_client.dart';

/// Process-local, bounded read model. No preferences, persistence or upload.
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
  final ListQueue<NetworkQualityPoint> _history =
      ListQueue<NetworkQualityPoint>();
  Timer? _timer;
  bool _disposed = false;
  bool _enabled = false;
  bool _refreshing = false;
  bool _streamUnavailable = false;
  DateTime? _receivedAt;
  DateTime? _lastPointAt;
  DateTime? _pausedAt;
  int? _lastDownloaded;
  int? _lastUploaded;
  String? _connectionId;
  int _epoch = 0;
  NetworkQualitySnapshot? latest;
  EngineSnapshot connection = const EngineSnapshot();

  bool get enabled => _enabled;
  bool get refreshing => _refreshing;
  bool get paused => _pausedAt != null;
  List<NetworkQualityPoint> get history =>
      List<NetworkQualityPoint>.unmodifiable(_history);
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

  void setEnabled(bool value) {
    if (_disposed || value == _enabled) return;
    _enabled = value;
    if (!value) _clear();
    _syncTimer();
    notifyListeners();
  }

  void updateConnection(EngineSnapshot value) {
    if (_disposed) return;
    connection = value;
    if (value.phase == ConnectionPhase.disconnected ||
        value.phase == ConnectionPhase.error) {
      _clear();
    } else if (value.networkQuality != null) {
      accept(value.networkQuality!);
    }
    _syncTimer();
  }

  void accept(NetworkQualitySnapshot value) {
    if (_disposed) return;
    if (value.connectionInstanceId == _connectionId &&
        value.sampledAt != null &&
        latest?.sampledAt != null &&
        value.sampledAt!.isBefore(latest!.sampledAt!)) {
      return;
    }
    if (value.connectionInstanceId != _connectionId) {
      _clear();
      _connectionId = value.connectionInstanceId;
    }
    latest = value;
    _receivedAt = _now();
    _record();
    notifyListeners();
  }

  void markStreamUnavailable(bool value) {
    if (_disposed || value == _streamUnavailable) return;
    _streamUnavailable = value;
    notifyListeners();
  }

  void togglePaused() {
    _pausedAt = paused ? null : _now();
    notifyListeners();
  }

  void _clear() {
    _epoch++;
    latest = null;
    _history.clear();
    _lastPointAt = null;
    _lastDownloaded = null;
    _lastUploaded = null;
    _receivedAt = null;
    _connectionId = null;
    _pausedAt = null;
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
    _record();
    if (connection.isConnected &&
        (_receivedAt == null ||
            _now().difference(_receivedAt!) >= const Duration(seconds: 2))) {
      unawaited(refresh());
    }
    notifyListeners();
  }

  /// Keep exactly one outstanding request, even if an underlying IPC future
  /// is slow. A timeout wrapper must not orphan it and enqueue more requests.
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
      // Existing readings age into Stale; no raw transport error reaches UI.
    } finally {
      _refreshing = false;
      if (!_disposed) notifyListeners();
    }
  }

  void _record() {
    final snapshot = latest;
    final now = _now();
    if (!_enabled ||
        paused ||
        !connection.isConnected ||
        snapshot == null ||
        _connectionId == null ||
        _connectionId!.isEmpty ||
        stale ||
        (_lastPointAt != null &&
            now.difference(_lastPointAt!) < const Duration(seconds: 1))) {
      return;
    }
    final elapsed = _lastPointAt == null
        ? null
        : now.difference(_lastPointAt!).inMilliseconds;
    int? rate(int total, int? previous) =>
        previous == null ||
            total < previous ||
            elapsed == null ||
            elapsed < 1 ||
            elapsed > 2000
        ? null
        : ((total - previous) / elapsed * 1000).round();
    final metrics = snapshot.metrics;
    _history.add(
      NetworkQualityPoint(
        at: now,
        rttMilliseconds:
            availableMetric(
              metrics.latestRttMilliseconds,
              metrics.latestRttAvailability,
            ) ??
            availableMetric(
              metrics.smoothedRttMilliseconds,
              metrics.smoothedRttAvailability,
            ),
        lossBasisPoints: availableMetric(
          metrics.intervalLossBasisPoints,
          metrics.intervalLossAvailability,
        ),
        downloadBytesPerSecond: rate(
          connection.downloadedBytes,
          _lastDownloaded,
        ),
        uploadBytesPerSecond: rate(connection.uploadedBytes, _lastUploaded),
      ),
    );
    while (_history.length > historyCapacity) {
      _history.removeFirst();
    }
    _lastPointAt = now;
    _lastDownloaded = connection.downloadedBytes;
    _lastUploaded = connection.uploadedBytes;
  }

  List<int?> trace(int? Function(NetworkQualityPoint point) value) {
    final end = windowEnd.millisecondsSinceEpoch ~/ 1000;
    final samples = List<int?>.filled(60, null);
    for (final point in _history) {
      final index = point.at.millisecondsSinceEpoch ~/ 1000 - (end - 59);
      if (index >= 0 && index < samples.length) samples[index] = value(point);
    }
    return List<int?>.unmodifiable(samples);
  }

  int? rateAverage({required bool download, required int seconds}) {
    assert(seconds > 0 && seconds <= 60);
    final values = trace(
      (point) =>
          download ? point.downloadBytesPerSecond : point.uploadBytesPerSecond,
    );
    final recent = values.sublist(values.length - seconds);
    if (stale || recent.any((value) => value == null)) return null;
    return (recent.whereType<int>().reduce((a, b) => a + b) / seconds).round();
  }

  @override
  void dispose() {
    _disposed = true;
    _timer?.cancel();
    _history.clear();
    super.dispose();
  }
}
