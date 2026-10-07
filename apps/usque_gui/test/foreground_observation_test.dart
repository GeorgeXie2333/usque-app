import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:shared_preferences/shared_preferences.dart';
import 'package:usque/app.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/models/diagnostics_models.dart';
import 'package:usque/state/app_controller.dart';
import 'package:usque/state/diagnostics_controller.dart';
import 'package:usque/state/network_quality_controller.dart';

import 'app_test.dart' show EventEngineClient;
import 'diagnostics_controller_test.dart'
    show DiagnosticsEngineStub, runningSession;
import 'quality_test_support.dart';

class _ObservationEngine extends EventEngineClient {
  int snapshotReads = 0;
  Completer<EngineSnapshot>? pendingSnapshot;

  @override
  Future<EngineSnapshot> snapshot() {
    snapshotReads++;
    return pendingSnapshot?.future ?? Future.value(current);
  }
}

class _PollingObservationEngine extends _ObservationEngine {
  @override
  bool get supportsSnapshotEvents => false;
}

void main() {
  testWidgets(
    'unpaused connected quality stops automatic reads hidden and restarts on resume',
    (tester) async {
      var now = DateTime.utc(2026, 10, 7);
      final engine = QualityEngineStub();
      final controller = NetworkQualityController(engine, now: () => now)
        ..setEnabled(true);
      controller.updateConnection(
        EngineSnapshot(
          phase: ConnectionPhase.connected,
          networkQuality: qualityFixture(now),
        ),
      );
      expect(controller.paused, isFalse);
      controller.setObservationVisible(false);
      now = now.add(const Duration(minutes: 10));
      await tester.pump(const Duration(minutes: 10));
      expect(engine.qualityRequests, 0);
      expect(controller.history, hasLength(1));
      controller.setObservationVisible(true);
      await tester.pump();
      expect(engine.qualityRequests, 1);
      now = now.add(const Duration(seconds: 3));
      await tester.pump(const Duration(seconds: 3));
      await tester.pump();
      expect(engine.qualityRequests, greaterThan(1));
      expect(controller.history, hasLength(1));
      controller.dispose();
    },
  );

  testWidgets(
    'hidden quality observation preserves history and user Pause without IPC',
    (tester) async {
      var now = DateTime.utc(2026, 10, 7);
      final engine = QualityEngineStub();
      final controller = NetworkQualityController(engine, now: () => now)
        ..setEnabled(true);
      controller.updateConnection(
        EngineSnapshot(
          phase: ConnectionPhase.connected,
          networkQuality: qualityFixture(now),
        ),
      );
      controller.togglePaused();
      final history = controller.history;
      final pausedAt = controller.windowEnd;
      controller.setObservationVisible(false);
      now = now.add(const Duration(minutes: 10));
      await tester.pump(const Duration(minutes: 10));
      expect(engine.qualityRequests, 0);
      expect(controller.history.length, history.length);
      expect(controller.paused, isTrue);
      expect(controller.windowEnd, pausedAt);
      controller.setObservationVisible(true);
      await tester.pump();
      expect(engine.qualityRequests, 1);
      expect(controller.paused, isTrue);
      expect(controller.history.length, history.length);
      controller.dispose();
    },
  );

  testWidgets(
    'late hidden quality reply cannot replace fresh authority or bridge legacy rates',
    (tester) async {
      var now = DateTime.utc(2026, 10, 7);
      final engine = QualityEngineStub()
        ..pendingQuality = Completer<NetworkQualitySnapshot?>();
      final controller = NetworkQualityController(
        engine,
        now: () => now,
        autoTick: false,
      )..setEnabled(true);
      void sample(int down) => controller.updateConnection(
        EngineSnapshot(
          phase: ConnectionPhase.connected,
          downloadedBytes: down,
          networkQuality: qualityFixture(now),
        ),
      );
      sample(0);
      now = now.add(const Duration(seconds: 1));
      sample(1000);
      expect(controller.rateAverage(download: true, seconds: 1), 1000);
      final previous = controller.latest;
      final pending = engine.pendingQuality!;
      final reading = controller.refresh();
      controller.setObservationVisible(false);
      now = now.add(const Duration(milliseconds: 100));
      controller.setObservationVisible(true);
      expect(engine.qualityRequests, 1);
      now = now.add(const Duration(milliseconds: 900));
      final fresh = qualityFixture(now, rtt: 8);
      engine.pendingQuality = null;
      engine.current = EngineSnapshot(
        phase: ConnectionPhase.connected,
        networkQuality: fresh,
      );
      pending.complete(previous);
      await reading;
      await tester.pump();
      expect(engine.qualityRequests, 2);
      expect(controller.latest, same(fresh));
      sample(2000);
      expect(controller.rateAverage(download: true, seconds: 1), isNull);
      now = now.add(const Duration(seconds: 1));
      sample(3000);
      expect(controller.rateAverage(download: true, seconds: 1), 1000);
      controller.dispose();
    },
  );

  test(
    'visibility cutoff does not suppress fresh legacy data after clock rollback',
    () {
      var now = DateTime.utc(2026, 10, 7);
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
      controller.setObservationVisible(false);
      now = now.add(const Duration(hours: 1));
      controller.setObservationVisible(true);
      now = now.subtract(const Duration(hours: 2));
      final fresh = qualityFixture(now, rtt: 11);
      controller.updateConnection(
        EngineSnapshot(phase: ConnectionPhase.connected, networkQuality: fresh),
      );
      expect(controller.latest, same(fresh));
      expect(controller.history, hasLength(1));
      controller.dispose();
    },
  );

  test(
    'resume can recover actual compact source samples recorded while hidden',
    () {
      var now = DateTime.utc(2026, 10, 7);
      final controller = NetworkQualityController(
        QualityEngineStub(),
        now: () => now,
        autoTick: false,
      )..setEnabled(true);
      NetworkQualitySnapshot source(List<int> seconds) =>
          NetworkQualitySnapshot(
            connectionInstanceId: 'source-connection',
            sampledAt: now,
            samples: [
              for (final second in seconds)
                NetworkQualitySample(
                  sequence: second + 1,
                  sampledAt: DateTime.utc(
                    2026,
                    10,
                    7,
                  ).add(Duration(seconds: second)),
                  monotonicMillis: second * 1000,
                  downloadedBytes: second * 1000,
                  uploadedBytes: second * 100,
                  rttMilliseconds: 10,
                ),
            ],
          );
      controller.updateConnection(
        EngineSnapshot(
          phase: ConnectionPhase.connected,
          networkQuality: source([0]),
        ),
      );
      controller.setObservationVisible(false);
      now = now.add(const Duration(seconds: 30));
      controller.setObservationVisible(true);
      controller.updateConnection(
        EngineSnapshot(
          phase: ConnectionPhase.connected,
          networkQuality: source([28, 29, 30]),
        ),
      );
      expect(controller.history, hasLength(4));
      expect(controller.rateAverage(download: true, seconds: 1), 1000);
      expect(controller.rateAverage(download: true, seconds: 5), isNull);
      controller.dispose();
    },
  );

  testWidgets(
    'hidden diagnostics stops read timers but preserves explicit start cancel and export',
    (tester) async {
      final engine = DiagnosticsEngineStub()
        ..pendingStart = Completer<DiagnosticSession>();
      final controller = DiagnosticsController(engine)..beginTimelineUpdates();
      final starting = controller.start(DiagnosticMode.standard);
      controller.setObservationVisible(false);
      final running = runningSession();
      engine.recovered = running;
      engine.pendingStart!.complete(running);
      await starting;
      await tester.pump(const Duration(minutes: 10));
      expect(controller.session, same(running));
      expect(engine.restoreCalls, 0);
      expect(engine.timelineCalls, 0);
      expect(engine.cancelCalls, 0);
      expect(await controller.export(), 'test-diagnostics.zip');
      await controller.cancel();
      expect(engine.cancelCalls, 1);
      expect(controller.session!.state, DiagnosticSessionState.cancelled);
      controller.setObservationVisible(true);
      await tester.pump();
      expect(engine.restoreCalls, 1);
      expect(engine.timelineCalls, 1);
      controller.dispose();
    },
  );

  testWidgets(
    'late diagnostic reads are ignored and resume waits for native read ownership',
    (tester) async {
      final engine = DiagnosticsEngineStub()
        ..pendingRestore = Completer<DiagnosticSession?>()
        ..pendingTimeline = Completer<ConnectionTimeline>();
      final controller = DiagnosticsController(engine)..beginTimelineUpdates();
      final restore = controller.restore(refreshTimeline: false);
      final timeline = controller.loadTimeline(silent: true);
      controller.setObservationVisible(false);
      controller.setObservationVisible(true);
      await tester.pump(const Duration(milliseconds: 800));
      expect(engine.restoreCalls, 1);
      expect(engine.timelineCalls, 1);
      final previousRestore = engine.pendingRestore!;
      final previousTimeline = engine.pendingTimeline!;
      engine.pendingRestore = null;
      engine.pendingTimeline = null;
      engine.recovered = runningSession(id: 'fresh-session');
      engine.timeline = const ConnectionTimeline(droppedEventCount: 2);
      previousRestore.complete(runningSession(id: 'old-session'));
      previousTimeline.complete(
        const ConnectionTimeline(droppedEventCount: 99),
      );
      await Future.wait([restore, timeline]);
      await tester.pump();
      expect(controller.session!.sessionId, 'fresh-session');
      expect(controller.timeline.droppedEventCount, 2);
      expect(engine.restoreCalls, 2);
      expect(engine.timelineCalls, 2);
      controller.dispose();
    },
  );

  testWidgets(
    'hidden degraded snapshot fallback and reconnect recover on resume',
    (tester) async {
      SharedPreferences.setMockInitialValues({
        'onboarding_complete': true,
        'update_checks_enabled': false,
      });
      final engine = _ObservationEngine();
      final controller = AppController(engine);
      await controller.initialize();
      engine.current = const EngineSnapshot(phase: ConnectionPhase.connected);
      engine.emitSnapshot(engine.current);
      await tester.pump();
      engine.eventControllers.single.addError(
        StateError('observation pipe lost'),
      );
      await tester.pump();
      expect(controller.snapshotStreamDegraded, isTrue);
      controller.setObservationVisible(false);
      final reads = engine.snapshotReads;
      await tester.pump(const Duration(minutes: 10));
      expect(engine.snapshotReads, reads);
      expect(engine.eventControllers, hasLength(1));
      engine.current = const EngineSnapshot(
        phase: ConnectionPhase.degraded,
        transport: 'HTTP/2',
      );
      controller.setObservationVisible(true);
      await tester.pump();
      expect(engine.eventControllers, hasLength(2));
      expect(controller.snapshot.transport, 'HTTP/2');
      final resumedReads = engine.snapshotReads;
      await tester.pump(const Duration(seconds: 1));
      await tester.pump();
      expect(engine.snapshotReads, greaterThan(resumedReads));
      engine.emitSnapshot(engine.current);
      await tester.pump();
      expect(controller.snapshotStreamDegraded, isFalse);
      final healedReads = engine.snapshotReads;
      await tester.pump(const Duration(seconds: 5));
      expect(engine.snapshotReads, healedReads);
      controller.dispose();
      await tester.pump();
    },
  );

  testWidgets(
    'rapid hide resume waits for asynchronous event cancellation before replacing its listener',
    (tester) async {
      SharedPreferences.setMockInitialValues({
        'onboarding_complete': true,
        'update_checks_enabled': false,
      });
      final engine = _ObservationEngine();
      final controller = AppController(engine);
      await controller.initialize();
      final cancellation = Completer<void>();
      engine.delayCancel = cancellation;
      controller.setObservationVisible(false);
      controller.setObservationVisible(true);
      controller.setObservationVisible(false);
      controller.setObservationVisible(true);
      await tester.pump(const Duration(seconds: 1));
      expect(engine.eventControllers, hasLength(1));
      cancellation.complete();
      await tester.pump();
      expect(engine.eventControllers, hasLength(2));
      engine.emitSnapshot(
        const EngineSnapshot(
          phase: ConnectionPhase.connected,
          transport: 'HTTP/3',
        ),
      );
      await tester.pump();
      expect(controller.snapshot.transport, 'HTTP/3');
      controller.dispose();
      await tester.pump();
    },
  );

  testWidgets('snapshot polling resumes for engines without event support', (
    tester,
  ) async {
    SharedPreferences.setMockInitialValues({
      'onboarding_complete': true,
      'update_checks_enabled': false,
    });
    final engine = _PollingObservationEngine();
    final controller = AppController(engine);
    await controller.initialize();
    controller.setObservationVisible(false);
    final reads = engine.snapshotReads;
    await tester.pump(const Duration(minutes: 10));
    expect(engine.snapshotReads, reads);
    engine.current = const EngineSnapshot(phase: ConnectionPhase.connected);
    controller.setObservationVisible(true);
    await tester.pump();
    final resumed = engine.snapshotReads;
    await tester.pump(const Duration(seconds: 2));
    await tester.pump();
    expect(engine.snapshotReads, greaterThan(resumed));
    engine.current = const EngineSnapshot();
    await tester.pump(const Duration(seconds: 1));
    await tester.pump();
    final disconnected = engine.snapshotReads;
    await tester.pump(const Duration(seconds: 5));
    expect(engine.snapshotReads, disconnected);
    controller.dispose();
    await tester.pump();
  });

  testWidgets(
    'late snapshot from before hiding cannot overwrite the authority read after resume',
    (tester) async {
      SharedPreferences.setMockInitialValues({
        'onboarding_complete': true,
        'update_checks_enabled': false,
      });
      final engine = _ObservationEngine();
      final controller = AppController(engine);
      await controller.initialize();
      final pending = Completer<EngineSnapshot>();
      engine.pendingSnapshot = pending;
      final reading = controller.refreshSnapshot(silent: true);
      controller.setObservationVisible(false);
      controller.setObservationVisible(true);
      engine.pendingSnapshot = null;
      engine.current = const EngineSnapshot(
        phase: ConnectionPhase.connected,
        transport: 'HTTP/2',
      );
      pending.complete(
        const EngineSnapshot(
          phase: ConnectionPhase.connected,
          transport: 'HTTP/3',
        ),
      );
      await reading;
      await tester.pump();
      expect(controller.snapshot.transport, 'HTTP/2');
      controller.dispose();
      await tester.pump();
    },
  );

  testWidgets(
    'initially hidden bootstrap finishes setup without automatic observation retries',
    (tester) async {
      SharedPreferences.setMockInitialValues({
        'onboarding_complete': true,
        'update_checks_enabled': false,
      });
      final engine = _ObservationEngine();
      final controller = AppController(engine)..setObservationVisible(false);
      await controller.initialize();
      expect(controller.initialized, isTrue);
      await tester.pump(const Duration(minutes: 10));
      expect(engine.snapshotReads, 0);
      expect(engine.eventControllers, isEmpty);
      controller.setObservationVisible(true);
      await tester.pump();
      expect(engine.snapshotReads, 1);
      expect(engine.eventControllers, hasLength(1));
      controller.dispose();
      await tester.pump();
    },
  );

  for (final platform in [TargetPlatform.android, TargetPlatform.windows]) {
    testWidgets(
      'bootstrap lifecycle observation applies to Android and preserves desktop tray semantics on $platform',
      (tester) async {
        SharedPreferences.setMockInitialValues({
          'onboarding_complete': true,
          'update_checks_enabled': false,
        });
        final engine = _ObservationEngine();
        tester.binding.handleAppLifecycleStateChanged(
          AppLifecycleState.resumed,
        );
        await tester.pumpWidget(
          UsqueBootstrap(engine: engine, builder: (_, _) => const SizedBox()),
        );
        await tester.pumpAndSettle();
        expect(engine.eventControllers, hasLength(1));
        tester.binding.handleAppLifecycleStateChanged(
          AppLifecycleState.inactive,
        );
        await tester.pump();
        expect(engine.eventControllers.single.hasListener, isTrue);
        tester.binding.handleAppLifecycleStateChanged(AppLifecycleState.hidden);
        await tester.pump();
        expect(
          engine.eventControllers.single.hasListener,
          platform == TargetPlatform.windows,
        );
        tester.binding.handleAppLifecycleStateChanged(AppLifecycleState.paused);
        tester.binding.handleAppLifecycleStateChanged(
          AppLifecycleState.resumed,
        );
        await tester.pump();
        expect(
          engine.eventControllers.length,
          platform == TargetPlatform.android ? 2 : 1,
        );
        await tester.pumpWidget(const SizedBox());
        await tester.pump();
      },
      variant: TargetPlatformVariant({platform}),
    );
  }
}
