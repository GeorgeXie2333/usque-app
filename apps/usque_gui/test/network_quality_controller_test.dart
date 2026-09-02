import 'dart:async';
import 'dart:typed_data';

import 'package:flutter_test/flutter_test.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/models/network_quality_models.dart';
import 'package:usque/services/control_codec.dart';
import 'package:usque/state/network_quality_controller.dart';

import 'quality_test_support.dart';

void main() {
  test('300-point 1 Hz bound, 60-second view and counter-derived rates', () {
    var now = DateTime.utc(2026, 9, 2);
    final engine = QualityEngineStub();
    final controller = NetworkQualityController(
      engine,
      now: () => now,
      autoTick: false,
    )..setEnabled(true);
    for (var second = 0; second < 400; second++) {
      now = now.add(const Duration(seconds: 1));
      controller.updateConnection(
        EngineSnapshot(
          phase: ConnectionPhase.connected,
          downloadedBytes: second * 1000,
          uploadedBytes: second * 200,
          networkQuality: qualityFixture(now),
        ),
      );
      controller.accept(qualityFixture(now));
    }
    expect(controller.history, hasLength(300));
    expect(controller.trace((point) => point.rttMilliseconds), hasLength(60));
    expect(controller.rateAverage(download: true, seconds: 1), 1000);
    expect(controller.rateAverage(download: false, seconds: 5), 200);
    expect(() => controller.history.clear(), throwsUnsupportedError);
    controller.dispose();
  });

  test('stale/paused intervals stay gaps; new connection resets counters', () {
    var now = DateTime.utc(2026, 9, 2);
    final controller = NetworkQualityController(
      QualityEngineStub(),
      now: () => now,
      autoTick: false,
    )..setEnabled(true);
    controller.updateConnection(
      EngineSnapshot(
        phase: ConnectionPhase.connected,
        networkQuality: qualityFixture(now),
      ),
    );
    expect(controller.stale, isFalse);
    controller.togglePaused();
    final paused = controller.windowEnd;
    now = now.add(const Duration(seconds: 10));
    controller.tick();
    expect(controller.windowEnd, paused);
    expect(controller.history, hasLength(1));
    controller.togglePaused();
    expect(controller.stale, isTrue);
    expect(controller.trace((point) => point.rttMilliseconds).last, isNull);
    controller.accept(qualityFixture(now));
    expect(controller.history.last.downloadBytesPerSecond, isNull);
    controller.accept(qualityFixture(now, id: 'new-connection'));
    expect(controller.history, hasLength(1));
    expect(controller.rateAverage(download: true, seconds: 5), isNull);
    controller.markStreamUnavailable(true);
    expect(controller.stale, isTrue);
    controller.markStreamUnavailable(false);
    expect(controller.stale, isFalse);
    controller.dispose();
  });

  test(
    'a stalled refresh remains single-flight and cannot restore an old connection',
    () async {
      final now = DateTime.utc(2026, 9, 2);
      final engine = QualityEngineStub()
        ..pendingQuality = Completer<NetworkQualitySnapshot?>();
      final controller = NetworkQualityController(
        engine,
        now: () => now,
        autoTick: false,
      )..setEnabled(true);
      controller.updateConnection(
        EngineSnapshot(
          phase: ConnectionPhase.connected,
          networkQuality: qualityFixture(now),
        ),
      );
      final first = controller.refresh();
      for (var i = 0; i < 30; i++) {
        await controller.refresh();
      }
      expect(engine.qualityRequests, 1);
      controller.updateConnection(const EngineSnapshot());
      engine.pendingQuality!.complete(qualityFixture(now));
      await first;
      expect(controller.latest, isNull);
      expect(controller.history, isEmpty);
      controller.dispose();
    },
  );

  test(
    'out-of-order samples and negative or malformed map values are not measurements',
    () {
      final now = DateTime.utc(2026, 9, 2);
      final controller = NetworkQualityController(
        QualityEngineStub(),
        now: () => now,
        autoTick: false,
      )..setEnabled(true);
      controller.updateConnection(
        EngineSnapshot(
          phase: ConnectionPhase.connected,
          networkQuality: qualityFixture(now),
        ),
      );
      controller.accept(
        qualityFixture(now.subtract(const Duration(seconds: 1)), rtt: 99),
      );
      expect(controller.latest!.metrics.latestRttMilliseconds, 42);
      final decoded = NetworkQualitySnapshot.fromMap(<Object?, Object?>{
        'sampled_at_unix_ms': 9223372036854775807,
        'level': 99,
        'connection_instance_id': 7,
        'queues': 'wrong',
        'metrics': <String, Object?>{
          'latest_rtt_milliseconds': -1,
          'smoothed_rtt_milliseconds': 'wrong',
          'interval_loss_basis_points': double.nan,
        },
      });
      expect(decoded.sampledAt, isNull);
      expect(decoded.metrics.latestRttMilliseconds, isNull);
      expect(decoded.metrics.smoothedRttMilliseconds, isNull);
      expect(decoded.queues, isEmpty);
      expect(decoded.connectionInstanceId, isNull);
      controller.dispose();
    },
  );

  test('queue pressure uses either budget and clamps; unknown is not zero', () {
    expect(queuePressure(const NetworkQueueQuality()), isNull);
    expect(
      queuePressure(
        const NetworkQueueQuality(
          availability: MetricAvailability.available,
          currentItems: 5,
          capacityItems: 100,
          currentBytes: 800,
          capacityBytes: 1000,
        ),
      ),
      .8,
    );
    expect(
      queuePressure(
        const NetworkQueueQuality(
          availability: MetricAvailability.available,
          currentItems: 11,
          capacityItems: 10,
        ),
      ),
      1,
    );
    expect(availableMetric(0, MetricAvailability.unsupported), isNull);
    expect(availableMetric(0, MetricAvailability.available), 0);
  });

  test(
    'latest RTT uses append-only fields 64–66 independently of smoothed',
    () {
      final metrics = ControlPayloadWriter()
        ..unsigned(4, 42)
        ..boolean(5, true)
        ..unsigned(64, 7)
        ..boolean(65, true)
        ..enumeration(66, 1);
      expect(metrics.takeBytes(), <int>[
        0x20,
        42,
        0x28,
        1,
        0x80,
        4,
        7,
        0x88,
        4,
        1,
        0x90,
        4,
        1,
      ]);
      final values = ControlPayloadWriter()
        ..unsigned(4, 42)
        ..boolean(5, true)
        ..unsigned(64, 7)
        ..boolean(65, true)
        ..enumeration(66, 1)
        ..unsigned(99, 123);
      final quality = ControlPayloadWriter()..message(4, values.takeBytes());
      final body = ControlPayloadWriter()
        ..string(1, 'nq')
        ..message(21, quality.takeBytes());
      final bytes = body.takeBytes();
      final prefix = ByteData(4)..setUint32(0, bytes.length);
      final decoded = debugDecodeNetworkQualityFrame(
        Uint8List.fromList(<int>[...prefix.buffer.asUint8List(), ...bytes]),
        'nq',
      )!;
      expect(decoded.metrics.latestRttMilliseconds, 7);
      expect(decoded.metrics.smoothedRttMilliseconds, 42);
      expect(
        decoded.metrics.latestRttAvailability,
        MetricAvailability.available,
      );
    },
  );
}
