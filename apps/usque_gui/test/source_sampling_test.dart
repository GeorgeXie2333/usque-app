import 'dart:typed_data';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:shared_preferences/shared_preferences.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/services/control_codec.dart';
import 'package:usque/state/app_controller.dart';
import 'package:usque/state/network_quality_controller.dart';
import 'package:usque/widgets/sparkline.dart';

import 'app_test.dart' show EventEngineClient;
import 'quality_test_support.dart';
import 'ui_workflow_test.dart' show workflowHost;

final _base = DateTime.utc(2026, 9, 5);

NetworkQualitySample _point(int sequence, int millis, {int? total}) =>
    NetworkQualitySample(
      sequence: sequence,
      sampledAt: _base.add(Duration(milliseconds: millis)),
      monotonicMillis: millis,
      downloadedBytes: total ?? millis * 2,
      uploadedBytes: total ?? millis,
      rttMilliseconds: 42,
      lossBasisPoints: 0,
    );

NetworkQualitySnapshot _batch(
  List<NetworkQualitySample> samples, {
  String id = 'sample-connection',
}) => NetworkQualitySnapshot(
  connectionInstanceId: id,
  sampledAt: samples.last.sampledAt,
  samples: List.unmodifiable(
    samples.length > 16 ? samples.sublist(samples.length - 16) : samples,
  ),
);

EngineSnapshot _state(NetworkQualitySnapshot quality) => EngineSnapshot(
  phase: ConnectionPhase.connected,
  // Deliberately unrelated cached state counters. Source samples own rates.
  downloadedBytes: 999999,
  uploadedBytes: 123456,
  networkQuality: quality,
);

class _SourceEvents extends EventEngineClient {
  @override
  Future<EngineCapabilities?> getCapabilities() async =>
      const EngineCapabilities(networkQuality: true);
}

void main() {
  for (final platform in [TargetPlatform.android, TargetPlatform.windows]) {
    testWidgets(
      'buffered source frames reach Home on $platform',
      (tester) async {
        SharedPreferences.setMockInitialValues({});
        var now = _base;
        final engine = _SourceEvents();
        final quality = NetworkQualityController(
          engine,
          now: () => now,
          autoTick: false,
        );
        final app = AppController(engine, qualityController: quality);
        addTearDown(app.dispose);
        await app.initialize();
        await tester.pump();
        final points = <NetworkQualitySample>[];
        for (var i = 0; i < 80; i++) {
          points.add(_point(i + 1, i * 1000));
          if (i % 5 == 2) continue;
          now = _base.add(Duration(milliseconds: i * 1000 + 510));
          final batch = _batch(points);
          if (platform == TargetPlatform.windows) {
            engine.emitNetworkQuality(batch);
          }
          engine.emitSnapshot(_state(batch));
          await tester.pump();
        }
        expect(
          quality.trace((p) => p.downloadBytesPerSecond),
          everyElement(2000),
        );
        tester.view.devicePixelRatio = 1;
        tester.view.physicalSize = platform == TargetPlatform.windows
            ? const Size(1280, 900)
            : const Size(375, 812);
        addTearDown(tester.view.resetDevicePixelRatio);
        addTearDown(tester.view.resetPhysicalSize);
        await tester.pumpWidget(workflowHost(app));
        await tester.pumpAndSettle();
        final charts = tester
            .widgetList<Sparkline>(find.byType(Sparkline))
            .toList();
        expect(charts.length, 2);
        expect(charts.first.samples, everyElement(2000));
        expect(charts.last.samples, everyElement(1000));
        expect(tester.takeException(), isNull);
        await tester.pumpWidget(const SizedBox.shrink());
      },
      variant: TargetPlatformVariant.only(platform),
    );
  }

  test(
    'pause freezes a delayed window even after its source becomes stale',
    () {
      var now = _base.add(const Duration(milliseconds: 2900));
      final controller = NetworkQualityController(
        QualityEngineStub(),
        now: () => now,
        autoTick: false,
      )..setEnabled(true);
      addTearDown(controller.dispose);
      controller.updateConnection(
        _state(_batch([_point(1, 0), _point(2, 1000)])),
      );
      final frozen = controller.trace((p) => p.downloadBytesPerSecond);
      controller.togglePaused();
      now = now.add(const Duration(seconds: 10));
      controller.tick();
      expect(controller.stale, isTrue);
      expect(controller.trace((p) => p.downloadBytesPerSecond), frozen);
    },
  );

  test('delivery beyond ring capacity never invents the lost observations', () {
    var now = _base;
    final controller = NetworkQualityController(
      QualityEngineStub(),
      now: () => now,
      autoTick: false,
    )..setEnabled(true);
    addTearDown(controller.dispose);
    controller.updateConnection(_state(_batch([_point(1, 0)])));
    now = _base.add(const Duration(milliseconds: 40100));
    controller.accept(
      _batch([for (var i = 25; i <= 40; i++) _point(i + 1, i * 1000)]),
    );
    expect(
      controller.trace((p) => p.rttMilliseconds).whereType<int>().length,
      17,
    );
    expect(
      controller.trace((p) => p.downloadBytesPerSecond).whereType<int>().length,
      15,
    );
    expect(controller.history.last.downloadBytesPerSecond, 2000);
  });

  test(
    'legacy receipt phase does not collide at the source half-second boundary',
    () {
      var now = _base;
      final controller = NetworkQualityController(
        QualityEngineStub(),
        now: () => now,
        autoTick: false,
      )..setEnabled(true);
      addTearDown(controller.dispose);
      for (var i = 0; i < 80; i++) {
        now = _base.add(
          Duration(milliseconds: i * 1000 + [480, 490, 510, 505, 485][i % 5]),
        );
        controller.updateConnection(
          EngineSnapshot(
            phase: ConnectionPhase.connected,
            networkQuality: qualityFixture(_base.add(Duration(seconds: i))),
          ),
        );
      }
      expect(controller.trace((p) => p.rttMilliseconds), everyElement(42));
      expect(
        controller.trace((p) => p.downloadBytesPerSecond),
        everyElement(0),
      );
    },
  );

  test(
    'same-instance reconnect does not replay transitional counter intervals',
    () {
      var now = _base.add(const Duration(milliseconds: 1100));
      final controller = NetworkQualityController(
        QualityEngineStub(),
        now: () => now,
        autoTick: false,
      )..setEnabled(true);
      addTearDown(controller.dispose);
      final points = [_point(1, 0), _point(2, 1000)];
      controller.updateConnection(_state(_batch(points)));
      now = _base.add(const Duration(milliseconds: 3100));
      controller.updateConnection(
        EngineSnapshot(
          phase: ConnectionPhase.reconnecting,
          networkQuality: _batch(points),
        ),
      );
      points.addAll([for (var i = 2; i <= 5; i++) _point(i + 1, i * 1000)]);
      now = _base.add(const Duration(milliseconds: 5100));
      controller.updateConnection(_state(_batch(points)));
      expect(controller.history.length, 4);
      expect(controller.trace((p) => p.rttMilliseconds)[56], isNull);
      expect(controller.trace((p) => p.rttMilliseconds)[57], isNull);
      expect(controller.history[2].downloadBytesPerSecond, isNull);
      expect(controller.history.last.downloadBytesPerSecond, 2000);
    },
  );

  for (final lag in [30, 480, 510, 1490, 1510, 1900]) {
    test('source history survives coalesced delivery at ${lag}ms phase', () {
      var now = _base;
      final controller = NetworkQualityController(
        QualityEngineStub(),
        now: () => now,
        autoTick: false,
      )..setEnabled(true);
      addTearDown(controller.dispose);
      final samples = <NetworkQualitySample>[];
      for (var i = 0; i < 400; i++) {
        final source = i * 1000 + [0, 5, 20, 5, 0][i % 5];
        samples.add(_point(i + 1, source));
        // An intermediate latest-only reader misses every fifth source update.
        // The next snapshot must carry that real measurement, not interpolate.
        if (i % 5 == 2) continue;
        now = _base.add(Duration(milliseconds: source + lag + (i % 2) * 20));
        final quality = _batch(samples);
        controller.updateConnection(_state(quality));
        controller.accept(
          quality,
        ); // Windows quality event/full-state duplicate.
        controller.tick();
      }
      expect(controller.history.length, 300);
      expect(controller.trace((p) => p.rttMilliseconds), everyElement(42));
      expect(
        controller.trace((p) => p.downloadBytesPerSecond),
        everyElement(2000),
      );
      expect(controller.rateAverage(download: false, seconds: 5), 1000);
      expect(controller.stale, isFalse);
    });
  }

  test(
    'actual source holes and sequence loss stay gaps, counters reset safely',
    () {
      var now = _base;
      final controller = NetworkQualityController(
        QualityEngineStub(),
        now: () => now,
        autoTick: false,
      )..setEnabled(true);
      addTearDown(controller.dispose);
      final samples = [_point(1, 0), _point(2, 1000), _point(3, 3000)];
      now = _base.add(const Duration(milliseconds: 3100));
      controller.updateConnection(_state(_batch(samples)));
      expect(controller.trace((p) => p.rttMilliseconds)[58], isNull);
      expect(controller.history.last.downloadBytesPerSecond, isNull);
      samples.add(_point(4, 4000, total: 1));
      now = _base.add(const Duration(milliseconds: 4100));
      controller.accept(_batch(samples));
      expect(controller.history.last.downloadBytesPerSecond, isNull);
      samples.add(_point(5, 5000, total: 1001));
      now = _base.add(const Duration(milliseconds: 5100));
      controller.accept(_batch(samples));
      expect(controller.history.last.downloadBytesPerSecond, 1000);
      samples.add(_point(7, 6000, total: 2001));
      now = _base.add(const Duration(milliseconds: 6100));
      controller.accept(_batch(samples));
      expect(controller.history.last.downloadBytesPerSecond, isNull);
    },
  );

  test(
    'paused and outage periods are not backfilled; reconnect retires old batches',
    () {
      var now = _base;
      final controller = NetworkQualityController(
        QualityEngineStub(),
        now: () => now,
        autoTick: false,
      )..setEnabled(true);
      addTearDown(controller.dispose);
      final samples = [_point(1, 0)];
      controller.updateConnection(_state(_batch(samples)));
      controller.togglePaused();
      final frozen = controller.trace((p) => p.downloadBytesPerSecond);
      for (var i = 1; i <= 5; i++) {
        samples.add(_point(i + 1, i * 1000));
      }
      now = _base.add(const Duration(milliseconds: 5100));
      controller.accept(_batch(samples));
      expect(controller.trace((p) => p.downloadBytesPerSecond), frozen);
      controller.togglePaused();
      samples.add(_point(7, 6000));
      now = _base.add(const Duration(milliseconds: 6100));
      controller.accept(_batch(samples));
      expect(controller.history.length, 2);
      expect(controller.history.last.downloadBytesPerSecond, isNull);
      controller.markStreamUnavailable(true);
      samples.add(_point(8, 7000));
      now = _base.add(const Duration(milliseconds: 7100));
      controller.accept(_batch(samples));
      expect(controller.history.last.downloadBytesPerSecond, isNull);
      controller.updateConnection(_state(_batch([_point(1, 7100)], id: 'new')));
      controller.accept(_batch(samples));
      expect(controller.latest!.connectionInstanceId, 'new');
      expect(controller.history.length, 1);
    },
  );

  test(
    'malformed map samples remain absent, known zero is preserved and input is bounded',
    () {
      final map = <Object?, Object?>{
        'sequence': 1,
        'sampled_at_unix_ms': 1234,
        'monotonic_millis': 0,
        'downloaded_bytes': 0,
        'uploaded_bytes': -1,
        'loss_basis_points': 10001,
      };
      final sample = NetworkQualitySample.fromMap(map)!;
      expect(sample.downloadedBytes, 0);
      expect(sample.uploadedBytes, isNull);
      expect(sample.rttMilliseconds, isNull);
      expect(sample.lossBasisPoints, isNull);
      expect(
        NetworkQualitySample.fromMap({...map, 'monotonic_millis': -1}),
        isNull,
      );
      expect(NetworkQualitySample.fromMap({...map, 'sequence': 0}), isNull);
      expect(
        NetworkQualitySnapshot.fromMap({
          'samples': List.filled(100, map),
        }).samples.length,
        16,
      );
    },
  );

  test(
    'append-only source sample fixture decodes optional zero and unknown fields',
    () {
      // Shared with usque-ipc Rust test: sequence=1, UTC=1234, monotonic=0,
      // downloaded=Some(0), uploaded=None, RTT=42, loss=None.
      const sampleBytes = [8, 1, 16, 210, 9, 32, 0, 48, 42];
      final quality = ControlPayloadWriter();
      for (var i = 0; i < 20; i++) {
        quality.message(9, Uint8List.fromList(sampleBytes));
      }
      quality.unsigned(99, 1);
      final body = ControlPayloadWriter()
        ..string(1, 'nq')
        ..message(21, quality.takeBytes());
      final bytes = body.takeBytes();
      final prefix = ByteData(4)..setUint32(0, bytes.length);
      final decoded = debugDecodeNetworkQualityFrame(
        Uint8List.fromList([...prefix.buffer.asUint8List(), ...bytes]),
        'nq',
      )!;
      expect(decoded.samples.length, 16);
      final sample = decoded.samples.first;
      expect(sample.monotonicMillis, 0);
      expect(sample.downloadedBytes, 0);
      expect(sample.uploadedBytes, isNull);
      expect(sample.rttMilliseconds, 42);
    },
  );
}
