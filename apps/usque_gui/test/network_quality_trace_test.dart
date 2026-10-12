import 'package:flutter_test/flutter_test.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/models/network_quality_models.dart';
import 'package:usque/state/network_quality_controller.dart';

import 'quality_test_support.dart';

final _base = DateTime.utc(2026, 10, 10);

EngineSnapshot _legacy(int second, {int rtt = 42, String id = 'connection'}) =>
    EngineSnapshot(
      phase: ConnectionPhase.connected,
      downloadedBytes: second * 1000,
      uploadedBytes: second * 200,
      networkQuality: qualityFixture(
        _base.add(Duration(seconds: second)),
        id: id,
        rtt: rtt,
      ),
    );

void _expectTraces(NetworkQualityController controller) {
  for (final metric in NetworkQualityTrace.values) {
    expect(
      controller.traceFor(metric),
      controller.trace(
        (point) => switch (metric) {
          NetworkQualityTrace.rtt => point.rttMilliseconds,
          NetworkQualityTrace.loss => point.lossBasisPoints,
          NetworkQualityTrace.download => point.downloadBytesPerSecond,
          NetworkQualityTrace.upload => point.uploadBytesPerSecond,
        },
      ),
      reason: metric.name,
    );
  }
}

class _TraceOnlyController extends NetworkQualityController {
  _TraceOnlyController(super.engine, {super.now}) : super(autoTick: false);

  @override
  List<NetworkQualityPoint> get history =>
      throw StateError('A trace must not materialize the full history');
}

void main() {
  test('both trace APIs read only their 60-slot display window', () {
    var now = _base;
    final controller = _TraceOnlyController(QualityEngineStub(), now: () => now)
      ..setEnabled(true);
    addTearDown(controller.dispose);
    for (var second = 0; second < 400; second++) {
      now = _base.add(Duration(seconds: second));
      controller.updateConnection(_legacy(second, rtt: second));
    }
    var projected = 0;
    expect(
      controller.trace((point) {
        projected++;
        return point.rttMilliseconds;
      }),
      [for (var second = 340; second < 400; second++) second],
    );
    expect(projected, 60);
    expect(
      controller.traceFor(NetworkQualityTrace.download),
      everyElement(1000),
    );
    _expectTraces(controller);
  });

  test(
    'duplicate notifications reuse immutable traces but retain new beats',
    () {
      var now = _base;
      final controller = NetworkQualityController(
        QualityEngineStub(),
        now: () => now,
        autoTick: false,
      )..setEnabled(true);
      addTearDown(controller.dispose);
      for (var second = 0; second <= 65; second++) {
        now = _base.add(Duration(seconds: second));
        controller.updateConnection(_legacy(second));
      }
      final traces = {
        for (final metric in NetworkQualityTrace.values)
          metric: controller.traceFor(metric),
      };
      controller.updateConnection(_legacy(65));
      controller.accept(_legacy(65).networkQuality!);
      now = now.add(const Duration(milliseconds: 300));
      controller.tick();
      for (final metric in NetworkQualityTrace.values) {
        expect(controller.traceFor(metric), same(traces[metric]));
        expect(() => traces[metric]![0] = 99, throwsUnsupportedError);
      }
      expect(controller.history, hasLength(66));
      now = _base.add(const Duration(seconds: 66));
      controller.updateConnection(_legacy(66));
      expect(controller.history, hasLength(67));
      for (final metric in NetworkQualityTrace.values) {
        // Equal-valued new beats do not need a different painted curve.
        expect(controller.traceFor(metric), same(traces[metric]));
      }
      _expectTraces(controller);
    },
  );

  test('later counters invalidate a cached window without moving its end', () {
    var now = _base;
    final controller = NetworkQualityController(
      QualityEngineStub(),
      now: () => now,
      autoTick: false,
    )..setEnabled(true);
    addTearDown(controller.dispose);
    controller.updateConnection(_legacy(0));
    now = _base.add(const Duration(seconds: 1));
    controller.accept(_legacy(1).networkQuality!);
    final waiting = controller.traceFor(NetworkQualityTrace.download);
    final rtt = controller.traceFor(NetworkQualityTrace.rtt);
    expect(waiting.last, isNull);
    controller.updateConnection(_legacy(1));
    final ready = controller.traceFor(NetworkQualityTrace.download);
    expect(ready, isNot(same(waiting)));
    expect(ready.last, 1000);
    expect(controller.traceFor(NetworkQualityTrace.rtt), same(rtt));
    _expectTraces(controller);
  });

  test(
    'source delivery grace, stale gaps and Pause use the same cached window',
    () {
      var now = _base.add(const Duration(milliseconds: 2500));
      final controller = NetworkQualityController(
        QualityEngineStub(),
        now: () => now,
        autoTick: false,
      )..setEnabled(true);
      addTearDown(controller.dispose);
      controller.updateConnection(
        EngineSnapshot(
          phase: ConnectionPhase.connected,
          networkQuality: NetworkQualitySnapshot(
            connectionInstanceId: 'source',
            sampledAt: _base.add(const Duration(seconds: 1)),
            samples: [
              for (var second = 0; second <= 1; second++)
                NetworkQualitySample(
                  sequence: second + 1,
                  sampledAt: _base.add(Duration(seconds: second)),
                  monotonicMillis: second * 1000,
                  downloadedBytes: second * 1000,
                  uploadedBytes: second * 200,
                  rttMilliseconds: 42,
                ),
            ],
          ),
        ),
      );
      final received = controller.traceFor(NetworkQualityTrace.download);
      expect(received.last, 1000);
      now = _base.add(const Duration(milliseconds: 3999));
      expect(controller.traceFor(NetworkQualityTrace.download), same(received));
      now = _base.add(const Duration(milliseconds: 4000));
      final advanced = controller.traceFor(NetworkQualityTrace.download);
      expect(advanced[57], 1000);
      expect(advanced.skip(58), everyElement(isNull));
      _expectTraces(controller);
      controller.togglePaused();
      now = now.add(const Duration(minutes: 1));
      expect(controller.stale, isTrue);
      expect(controller.traceFor(NetworkQualityTrace.download), same(advanced));
      _expectTraces(controller);
      controller.togglePaused();
      expect(
        controller.traceFor(NetworkQualityTrace.download),
        everyElement(isNull),
      );
      _expectTraces(controller);
    },
  );

  test(
    'outages, hiding, connection changes and reset do not reuse old values',
    () {
      var now = _base;
      final controller = NetworkQualityController(
        QualityEngineStub(),
        now: () => now,
        autoTick: false,
      )..setEnabled(true);
      addTearDown(controller.dispose);
      void sample(int second, {String id = 'connection'}) {
        now = _base.add(Duration(seconds: second));
        controller.updateConnection(_legacy(second, id: id));
        _expectTraces(controller);
      }

      sample(0);
      sample(1);
      expect(controller.traceFor(NetworkQualityTrace.download).last, 1000);
      controller.markStreamUnavailable(true);
      sample(2);
      expect(controller.traceFor(NetworkQualityTrace.download).last, isNull);
      sample(3);
      expect(controller.traceFor(NetworkQualityTrace.download).last, 1000);
      controller.setObservationVisible(false);
      sample(4);
      controller.setObservationVisible(true);
      sample(5);
      expect(controller.traceFor(NetworkQualityTrace.download).last, isNull);
      sample(6);
      expect(controller.traceFor(NetworkQualityTrace.download).last, 1000);
      sample(7, id: 'replacement');
      expect(
        controller.traceFor(NetworkQualityTrace.download),
        everyElement(isNull),
      );
      expect(controller.traceFor(NetworkQualityTrace.rtt).whereType<int>(), [
        42,
      ]);
      controller.accept(_legacy(7).networkQuality!);
      expect(controller.latest!.connectionInstanceId, 'replacement');
      _expectTraces(controller);
      controller.reset();
      for (final metric in NetworkQualityTrace.values) {
        expect(controller.traceFor(metric), everyElement(isNull));
      }
      _expectTraces(controller);
    },
  );
}
