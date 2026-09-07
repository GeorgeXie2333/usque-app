import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:shared_preferences/shared_preferences.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/state/app_controller.dart';
import 'package:usque/state/network_quality_controller.dart';
import 'package:usque/widgets/sparkline.dart';

import 'app_test.dart' show EventEngineClient;
import 'quality_test_support.dart';
import 'ui_workflow_test.dart' show workflowHost;

final _base = DateTime.utc(2026, 9, 5);

EngineSnapshot _sample(
  int sourceMs,
  int total, {
  String id = 'sample-connection',
}) => EngineSnapshot(
  phase: ConnectionPhase.connected,
  downloadedBytes: total,
  uploadedBytes: total ~/ 2,
  downloadBytesPerSecond: 1000,
  uploadBytesPerSecond: 500,
  networkQuality: qualityFixture(
    _base.add(Duration(milliseconds: sourceMs)),
    id: id,
  ),
);

class _Events extends EventEngineClient {
  @override
  Future<EngineCapabilities?> getCapabilities() async =>
      const EngineCapabilities(networkQuality: true);
}

class _SlowSnapshot extends QualityEngineStub {
  var reply = Completer<EngineSnapshot>();
  var snapshotRequests = 0;
  @override
  Future<EngineSnapshot> snapshot() {
    snapshotRequests++;
    return reply.future;
  }
}

void main() {
  test(
    'fallback polling stays single-flight and cannot overwrite newer state',
    () async {
      final engine = _SlowSnapshot();
      final app = AppController(engine);
      addTearDown(app.dispose);
      final request = app.refreshSnapshot();
      for (var i = 0; i < 20; i++) {
        expect(identical(app.refreshSnapshot(silent: true), request), isTrue);
      }
      expect(engine.snapshotRequests, 1);
      final newer = _sample(2000, 2000);
      app.snapshot = newer;
      engine.reply.complete(_sample(0, 0));
      await request;
      expect(app.snapshot, newer);
      engine.reply = Completer<EngineSnapshot>()..complete(newer);
      await app.refreshSnapshot();
      expect(engine.snapshotRequests, 2);
    },
  );

  test('counter reset and local clock rollback start new rate baselines', () {
    var now = _base;
    final controller = NetworkQualityController(
      QualityEngineStub(),
      now: () => now,
      autoTick: false,
    )..setEnabled(true);
    addTearDown(controller.dispose);
    controller.updateConnection(_sample(0, 10000));
    now = _base.add(const Duration(seconds: 1));
    controller.updateConnection(_sample(1000, 1));
    expect(controller.history.last.downloadBytesPerSecond, isNull);
    now = _base.add(const Duration(seconds: 2));
    controller.updateConnection(_sample(2000, 1001));
    expect(controller.history.last.downloadBytesPerSecond, 1000);
    now = _base;
    controller.updateConnection(_sample(0, 1002));
    expect(controller.history.length, 1);
    expect(controller.history.single.downloadBytesPerSecond, isNull);
  });
  test(
    'source jitter, delivery jitter and repaint ticks retain every real beat',
    () {
      var now = _base;
      final controller = NetworkQualityController(
        QualityEngineStub(),
        now: () => now,
        autoTick: false,
      )..setEnabled(true);
      addTearDown(controller.dispose);
      for (var second = 0; second < 400; second++) {
        final jitter = [0, 8, -11, 15, -6][second % 5];
        final source = second * 1000 + jitter;
        final received = source + [80, 250, 180, 90][second % 4];
        now = _base.add(Duration(milliseconds: received));
        final value = _sample(source, received * 2);
        controller.updateConnection(value);
        controller.accept(
          value.networkQuality!,
        ); // Windows quality-only duplicate.
        now = now.add(const Duration(milliseconds: 150));
        controller.tick();
        controller.updateConnection(value); // Cached full-snapshot duplicate.
      }
      expect(controller.history.length, 300);
      expect(
        controller.trace((p) => p.rttMilliseconds).whereType<int>().length,
        60,
      );
      expect(
        controller.trace((p) => p.downloadBytesPerSecond),
        everyElement(2000),
      );
      expect(controller.rateAverage(download: true, seconds: 5), 2000);
    },
  );

  test(
    'quality-only events never sample old counters and can precede a full snapshot',
    () {
      var now = _base;
      final controller = NetworkQualityController(
        QualityEngineStub(),
        now: () => now,
        autoTick: false,
      )..setEnabled(true);
      addTearDown(controller.dispose);
      controller.accept(_sample(0, 0).networkQuality!);
      controller.updateConnection(_sample(0, 0));
      now = _base.add(const Duration(seconds: 1));
      controller.accept(_sample(1000, 1000).networkQuality!);
      expect(controller.history.last.downloadBytesPerSecond, isNull);
      controller.updateConnection(_sample(1000, 1000));
      expect(controller.history.last.downloadBytesPerSecond, 1000);
      now = _base.add(const Duration(milliseconds: 1450));
      controller.tick();
      expect(controller.history.length, 2);
      expect(controller.trace((p) => p.rttMilliseconds).last, 42);
      now = _base.add(const Duration(milliseconds: 2600));
      controller.tick();
      expect(controller.trace((p) => p.rttMilliseconds).last, isNull);
      expect(controller.history.length, 2);
    },
  );

  test(
    'window average weights actual elapsed time; genuine missing beats remain gaps',
    () {
      var now = _base;
      final controller = NetworkQualityController(
        QualityEngineStub(),
        now: () => now,
        autoTick: false,
      )..setEnabled(true);
      addTearDown(controller.dispose);
      final times = [0, 1005, 1990, 3000, 4000, 5090];
      final bytes = [0, 1000, 4000, 4000, 5000, 9000];
      for (var i = 0; i < times.length; i++) {
        now = _base.add(Duration(milliseconds: times[i]));
        controller.updateConnection(_sample(times[i], bytes[i]));
      }
      expect(
        controller.rateAverage(download: true, seconds: 5),
        (9000 / 5.09).round(),
      );
      now = _base.add(const Duration(seconds: 7));
      controller.updateConnection(_sample(7000, 10000));
      expect(controller.trace((p) => p.rttMilliseconds)[58], isNull);
      expect(controller.history.last.downloadBytesPerSecond, isNull);
      expect(controller.rateAverage(download: true, seconds: 5), isNull);
    },
  );

  test(
    'duplicate, reordered and retired connection data do not rewrite history',
    () {
      var now = _base;
      final controller = NetworkQualityController(
        QualityEngineStub(),
        now: () => now,
        autoTick: false,
      )..setEnabled(true);
      addTearDown(controller.dispose);
      controller.updateConnection(_sample(0, 0));
      now = _base.add(const Duration(seconds: 1));
      controller.updateConnection(_sample(1000, 1000));
      controller.accept(_sample(0, 0).networkQuality!);
      expect(controller.latest!.sampledAt, now);
      controller.updateConnection(_sample(0, 0));
      expect(controller.connection.downloadedBytes, 1000);
      expect(controller.history.last.downloadBytesPerSecond, 1000);
      controller.updateConnection(_sample(1000, 1, id: 'next-connection'));
      controller.accept(_sample(1000, 1000).networkQuality!);
      expect(controller.latest!.connectionInstanceId, 'next-connection');
      expect(controller.history.length, 1);
      expect(controller.history.single.downloadBytesPerSecond, isNull);
      now = _base.add(const Duration(seconds: 2));
      controller.updateConnection(_sample(2000, 1001, id: 'next-connection'));
      expect(controller.history.last.downloadBytesPerSecond, 1000);
      controller.togglePaused();
      final frozen = controller.trace((p) => p.rttMilliseconds);
      now = _base.add(const Duration(seconds: 8));
      controller.updateConnection(_sample(8000, 8001, id: 'next-connection'));
      expect(controller.trace((p) => p.rttMilliseconds), frozen);
      controller.togglePaused();
      now = _base.add(const Duration(seconds: 9));
      controller.updateConnection(_sample(9000, 9001, id: 'next-connection'));
      expect(controller.history.last.downloadBytesPerSecond, isNull);
    },
  );

  for (final platform in [TargetPlatform.windows, TargetPlatform.android]) {
    testWidgets(
      'event ingestion and home charts use one history on $platform',
      (tester) async {
        SharedPreferences.setMockInitialValues({});
        var now = _base;
        final engine = _Events();
        final quality = NetworkQualityController(
          engine,
          now: () => now,
          autoTick: false,
        );
        final app = AppController(engine, qualityController: quality);
        try {
          await app.initialize();
          await tester.pump();
          app.engineCapabilities = const EngineCapabilities(
            networkQuality: true,
          );
          for (var second = 0; second <= 65; second++) {
            final source = second * 1000 + (second.isEven ? 0 : 10);
            now = _base.add(Duration(milliseconds: source + 100));
            // Equal values at new source timestamps remain real samples.
            final sample = _sample(source, second > 59 ? 59010 : source);
            if (platform == TargetPlatform.windows) {
              engine.emitNetworkQuality(sample.networkQuality!);
            }
            engine.emitSnapshot(sample);
            await tester.pump();
          }
          expect(
            quality.trace((p) => p.downloadBytesPerSecond).take(54),
            everyElement(1000),
          );
          expect(
            quality.trace((p) => p.downloadBytesPerSecond).skip(54),
            everyElement(0),
          );
          expect(quality.rateAverage(download: true, seconds: 5), 0);
          expect(quality.trace((p) => p.rttMilliseconds), everyElement(42));
          tester.view.devicePixelRatio = 1;
          tester.view.physicalSize = platform == TargetPlatform.windows
              ? const Size(1280, 900)
              : const Size(375, 812);
          addTearDown(tester.view.resetDevicePixelRatio);
          addTearDown(tester.view.resetPhysicalSize);
          await tester.pumpWidget(workflowHost(app));
          await tester.pumpAndSettle();
          final traces = tester
              .widgetList<Sparkline>(find.byType(Sparkline))
              .toList();
          expect(traces.length, 2);
          expect(
            traces.first.samples,
            quality.trace((p) => p.downloadBytesPerSecond),
          );
          await tester.pumpWidget(workflowHost(app, dark: true));
          await tester.pumpAndSettle();
          expect(quality.history.length, 66);
          engine.eventControllers.last.addError(
            StateError('test event stream unavailable'),
          );
          await tester.pump();
          expect(app.snapshotStreamDegraded, isTrue);
          expect(quality.stale, isTrue);
          now = _base.add(const Duration(milliseconds: 66100));
          engine.current = _sample(66000, 60010);
          await app.refreshSnapshot(silent: true);
          await tester.pump();
          expect(app.snapshotStreamDegraded, isTrue);
          expect(quality.stale, isFalse);
          expect(quality.history.last.downloadBytesPerSecond, isNull);
          now = _base.add(const Duration(milliseconds: 67100));
          engine.current = _sample(67000, 61010);
          await app.refreshSnapshot(silent: true);
          await tester.pump();
          expect(quality.history.last.downloadBytesPerSecond, 1000);
          expect(tester.takeException(), isNull);
        } finally {
          await tester.pumpWidget(const SizedBox.shrink());
          app.dispose();
        }
      },
      variant: TargetPlatformVariant.only(platform),
    );
  }
}
