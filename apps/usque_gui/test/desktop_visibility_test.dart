import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:shared_preferences/shared_preferences.dart';
import 'package:usque/app.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/models/diagnostics_models.dart';
import 'package:usque/services/engine_client.dart';
import 'package:usque/services/platform_shell_bridge.dart';
import 'package:usque/state/app_controller.dart';
import 'package:usque/state/network_quality_controller.dart';
import 'package:usque/widgets/controller_selector.dart';
import 'package:usque/widgets/listenable_selector.dart';

import 'app_test.dart' show EventEngineClient;
import 'quality_test_support.dart';

class _DesktopEngine extends EventEngineClient {
  int qualityReads = 0;
  int diagnosticReads = 0;
  int timelineReads = 0;
  int snapshotReads = 0;
  Completer<NetworkQualitySnapshot?>? pendingQuality;
  Completer<EngineSnapshot>? pendingSnapshot;

  @override
  Future<EngineCapabilities?> getCapabilities() async =>
      const EngineCapabilities(
        networkSettingsApplication: true,
        networkQuality: true,
      );

  @override
  Future<EngineSnapshot> snapshot() {
    snapshotReads++;
    return pendingSnapshot?.future ?? Future.value(current);
  }

  @override
  Future<NetworkQualitySnapshot?> getNetworkQuality() {
    qualityReads++;
    return pendingQuality?.future ?? Future.value(current.networkQuality);
  }

  @override
  Future<DiagnosticSession?> getDiagnostics() async {
    diagnosticReads++;
    return null;
  }

  @override
  Future<ConnectionTimeline> getConnectionTimeline() async {
    timelineReads++;
    return const ConnectionTimeline();
  }
}

const _channel = MethodChannel('io.github.georgexie2333.usque/engine');

Future<void> _nativeVisibility(WidgetTester tester, bool visible) {
  final delivered = Completer<void>();
  tester.binding.channelBuffers.push(
    _channel.name,
    const StandardMethodCodec().encodeMethodCall(
      MethodCall('windowVisibilityChanged', visible),
    ),
    (_) => delivered.complete(),
  );
  return delivered.future;
}

void main() {
  setUp(() {
    SharedPreferences.setMockInitialValues({
      'onboarding_complete': true,
      'update_checks_enabled': false,
    });
  });

  testWidgets('hidden desktop retains control events and user operations', (
    tester,
  ) async {
    var now = DateTime.utc(2026, 10, 28);
    final engine = _DesktopEngine();
    final quality = NetworkQualityController(engine, now: () => now);
    final app = AppController(engine, qualityController: quality);
    await app.initialize();
    engine.current = EngineSnapshot(
      phase: ConnectionPhase.connected,
      networkQuality: qualityFixture(now),
    );
    engine.emitSnapshot(engine.current);
    await tester.pump();
    app.diagnostics.beginTimelineUpdates();
    app.setUiVisible(false);
    final history = quality.history.length;
    var qualityNotifications = 0;
    quality.addListener(() => qualityNotifications++);
    final reads = (
      engine.qualityReads,
      engine.diagnosticReads,
      engine.timelineReads,
    );
    now = now.add(const Duration(minutes: 10));
    engine.emitNetworkQuality(qualityFixture(now, rtt: 300));
    engine.emitSnapshot(
      EngineSnapshot(
        phase: ConnectionPhase.reconnecting,
        networkQuality: qualityFixture(now, rtt: 300),
      ),
    );
    engine.eventControllers.single.add(
      const EngineSnapshotEvent(
        capabilities: EngineCapabilities(
          networkQuality: true,
          networkSettingsApplication: true,
          quicMigration: true,
        ),
        networkSettings: NetworkSettingsState(
          sourceEpoch: 'hidden-engine',
          sequence: 10,
        ),
        diagnosticsChanged: true,
      ),
    );
    await tester.pump(const Duration(minutes: 10));
    expect(engine.eventControllers.single.hasListener, isTrue);
    expect(app.snapshot.phase, ConnectionPhase.reconnecting);
    expect(app.engineCapabilities!.quicMigration, isTrue);
    expect(app.networkSettings.state!.sourceEpoch, 'hidden-engine');
    expect(quality.history.length, history);
    expect(qualityNotifications, 0);
    expect((
      engine.qualityReads,
      engine.diagnosticReads,
      engine.timelineReads,
    ), reads);
    await app.disconnectForExit();
    expect(engine.calls, contains('disconnect'));
    expect(app.snapshot.phase, ConnectionPhase.disconnected);
    app.dispose();
    await tester.pump();
  });

  testWidgets('desktop resume rejects pending replies and reads fresh once', (
    tester,
  ) async {
    var now = DateTime.utc(2026, 10, 28);
    final engine = _DesktopEngine();
    final quality = NetworkQualityController(engine, now: () => now);
    final app = AppController(engine, qualityController: quality);
    await app.initialize();
    engine.current = EngineSnapshot(
      phase: ConnectionPhase.connected,
      networkQuality: qualityFixture(now),
    );
    engine.emitSnapshot(engine.current);
    await tester.pump();
    final oldQuality = Completer<NetworkQualitySnapshot?>();
    final oldSnapshot = Completer<EngineSnapshot>();
    engine.pendingQuality = oldQuality;
    engine.pendingSnapshot = oldSnapshot;
    final qualityRead = quality.refresh();
    final snapshotRead = app.refreshSnapshot(silent: true);
    app.setUiVisible(false);
    app.setUiVisible(true);
    now = now.add(const Duration(seconds: 1));
    engine.current = EngineSnapshot(
      phase: ConnectionPhase.connected,
      transport: 'HTTP/2',
      networkQuality: qualityFixture(now, rtt: 22),
    );
    engine.pendingQuality = null;
    engine.pendingSnapshot = null;
    final qualityReads = engine.qualityReads;
    final snapshotReads = engine.snapshotReads;
    oldQuality.complete(qualityFixture(now, rtt: 999));
    oldSnapshot.complete(
      const EngineSnapshot(
        phase: ConnectionPhase.connected,
        transport: 'stale',
      ),
    );
    await Future.wait([qualityRead, snapshotRead]);
    await tester.pump();
    expect(app.snapshot.transport, 'HTTP/2');
    expect(quality.latest!.metrics.latestRttMilliseconds, 22);
    expect(engine.qualityReads, qualityReads + 1);
    expect(engine.snapshotReads, snapshotReads + 1);
    expect(engine.eventControllers, hasLength(1));
    app.dispose();
    await tester.pump();
  });

  testWidgets(
    'hidden desktop retains status fallback and reconnect ownership',
    (tester) async {
      final engine = _DesktopEngine();
      final app = AppController(engine);
      await app.initialize();
      engine.emitSnapshot(
        const EngineSnapshot(phase: ConnectionPhase.connected),
      );
      await tester.pump();
      app.setUiVisible(false);
      engine.eventControllers.single.addError(StateError('event stream lost'));
      await tester.pump();
      final reads = engine.snapshotReads;
      engine.current = const EngineSnapshot(phase: ConnectionPhase.error);
      await tester.pump(const Duration(seconds: 1));
      await tester.pump();
      expect(app.snapshotStreamDegraded, isTrue);
      expect(engine.snapshotReads, greaterThan(reads));
      expect(app.snapshot.phase, ConnectionPhase.error);
      expect(engine.eventControllers, hasLength(2));
      app.dispose();
      await tester.pump();
    },
  );

  testWidgets(
    'presentation selectors stay quiet hidden and catch up on restore',
    (tester) async {
      final app = AppController(_DesktopEngine());
      var controllerBuilds = 0;
      var mergedBuilds = 0;
      await tester.pumpWidget(
        Directionality(
          textDirection: TextDirection.ltr,
          child: Column(
            children: [
              ControllerSelector(
                controller: app,
                selector: (value) => value.section,
                builder: (_, value) {
                  controllerBuilds++;
                  return Text(value.name);
                },
              ),
              ListenableSelector(
                listenables: [app, app.quality],
                selector: () => app.section,
                builder: (_, value) {
                  mergedBuilds++;
                  return Text(value.name);
                },
              ),
            ],
          ),
        ),
      );
      app.setUiVisible(false);
      app.selectSection(AppSection.profiles);
      await tester.pump();
      expect((controllerBuilds, mergedBuilds), (1, 1));
      app.setUiVisible(true);
      await tester.pump();
      expect((controllerBuilds, mergedBuilds), (2, 2));
      expect(find.text('profiles'), findsNWidgets(2));
      await tester.pumpWidget(const SizedBox());
      app.dispose();
      await tester.pump();
    },
  );

  testWidgets(
    'background startup keeps controls live and tickers quiet until native show',
    (tester) async {
      final messenger = tester.binding.defaultBinaryMessenger;
      messenger.setMockMethodCallHandler(_channel, (call) async {
        if (call.method == 'getWindowVisibility') return false;
        return null;
      });
      addTearDown(() => messenger.setMockMethodCallHandler(_channel, null));
      final engine = _DesktopEngine();
      bool? tickersEnabled;
      await tester.pumpWidget(
        UsqueBootstrap(
          engine: engine,
          builder: (_, _) => Builder(
            builder: (context) {
              tickersEnabled = TickerMode.valuesOf(context).enabled;
              return const SizedBox();
            },
          ),
        ),
      );
      await tester.pumpAndSettle();
      expect(tickersEnabled, isFalse);
      expect(engine.eventControllers.single.hasListener, isTrue);
      expect(engine.snapshotReads, 1);
      expect(engine.diagnosticReads, 0);
      expect(engine.qualityReads, 0);
      await _nativeVisibility(tester, true);
      await tester.pumpAndSettle();
      expect(tickersEnabled, isTrue);
      expect(engine.snapshotReads, 2);
      expect(engine.diagnosticReads, 1);
      expect(engine.qualityReads, 1);
      await _nativeVisibility(tester, false);
      await tester.pump();
      expect(tickersEnabled, isFalse);
      expect(engine.eventControllers, hasLength(1));
      await tester.pumpWidget(const SizedBox());
      await tester.pump();
    },
    variant: TargetPlatformVariant.only(TargetPlatform.windows),
  );

  testWidgets(
    'an older native host without visibility support remains usable',
    (tester) async {
      final messenger = tester.binding.defaultBinaryMessenger;
      messenger.setMockMethodCallHandler(_channel, (call) async {
        throw MissingPluginException();
      });
      addTearDown(() => messenger.setMockMethodCallHandler(_channel, null));
      final app = AppController(_DesktopEngine())..setUiVisible(false);
      final bridge = PlatformShellBridge(app);
      await tester.pump();
      expect(app.uiVisible, isTrue);
      bridge.dispose();
      app.dispose();
      await tester.pump();
    },
    variant: TargetPlatformVariant.only(TargetPlatform.windows),
  );

  testWidgets(
    'late initial visibility query cannot overwrite a native hide event',
    (tester) async {
      final messenger = tester.binding.defaultBinaryMessenger;
      final pending = Completer<bool>();
      messenger.setMockMethodCallHandler(_channel, (call) async {
        if (call.method == 'getWindowVisibility') return pending.future;
        return null;
      });
      addTearDown(() => messenger.setMockMethodCallHandler(_channel, null));
      final app = AppController(_DesktopEngine());
      final bridge = PlatformShellBridge(app);
      await _nativeVisibility(tester, false);
      pending.complete(true);
      await tester.pump();
      expect(app.uiVisible, isFalse);
      bridge.dispose();
      app.dispose();
      await tester.pump();
    },
    variant: TargetPlatformVariant.only(TargetPlatform.windows),
  );
}
