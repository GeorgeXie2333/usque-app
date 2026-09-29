import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter/semantics.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:usque/core/usque_theme.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/models/diagnostics_models.dart';
import 'package:usque/screens/diagnostics_screen.dart';
import 'package:usque/services/engine_client.dart';
import 'package:usque/state/app_controller.dart';
import 'package:usque/state/diagnostics_controller.dart';

class DiagnosticsEngineStub implements EngineClient {
  @override
  Future<NetworkQualitySnapshot?> getNetworkQuality() async => null;

  @override
  Future<EngineCapabilities?> getCapabilities() async => null;
  DiagnosticSession? recovered;
  ConnectionTimeline timeline = const ConnectionTimeline();
  int startCalls = 0;
  int cancelCalls = 0;
  int restoreCalls = 0;
  int exportCalls = 0;
  int timelineCalls = 0;
  Completer<DiagnosticSession>? pendingStart;
  Completer<DiagnosticSession?>? pendingRestore;
  Completer<ConnectionTimeline>? pendingTimeline;
  String? cancelledId;

  @override
  bool get supportsSnapshotEvents => false;

  @override
  Stream<EngineSnapshotEvent> get snapshotEvents =>
      const Stream<EngineSnapshotEvent>.empty();

  @override
  Future<DiagnosticSession?> getDiagnostics() async {
    restoreCalls += 1;
    return pendingRestore?.future ?? recovered;
  }

  @override
  Future<ConnectionTimeline> getConnectionTimeline() async {
    timelineCalls += 1;
    return pendingTimeline?.future ?? timeline;
  }

  @override
  Future<DiagnosticSession> startDiagnostics(DiagnosticMode mode) {
    startCalls += 1;
    final pending = pendingStart;
    if (pending != null) return pending.future;
    final session = runningSession(mode: mode);
    recovered = session;
    return Future<DiagnosticSession>.value(session);
  }

  @override
  Future<DiagnosticSession> cancelDiagnostics(String sessionId) async {
    cancelCalls += 1;
    cancelledId = sessionId;
    final cancelled = DiagnosticSession(
      sessionId: sessionId,
      state: DiagnosticSessionState.cancelled,
      startedAt: DateTime.fromMillisecondsSinceEpoch(1, isUtc: true),
      completedAt: DateTime.fromMillisecondsSinceEpoch(2, isUtc: true),
      mode: recovered?.mode ?? DiagnosticMode.standard,
      progressPercent: 100,
    );
    recovered = cancelled;
    return cancelled;
  }

  @override
  Future<String?> exportDiagnostics({String? diagnosticSessionId}) async {
    exportCalls += 1;
    return 'test-diagnostics.zip';
  }

  @override
  void dispose() {}

  @override
  dynamic noSuchMethod(Invocation invocation) => super.noSuchMethod(invocation);
}

DiagnosticSession runningSession({
  DiagnosticMode mode = DiagnosticMode.standard,
  String id = 'session-one',
}) {
  return DiagnosticSession(
    sessionId: id,
    state: DiagnosticSessionState.running,
    startedAt: DateTime.fromMillisecondsSinceEpoch(1, isUtc: true),
    mode: mode,
    currentCheck: 'engine.control_channel',
    progressPercent: 20,
    findings: const <DiagnosticFinding>[
      DiagnosticFinding(
        checkId: 'engine.control_channel',
        category: DiagnosticCategory.localComponent,
        status: DiagnosticCheckStatus.running,
      ),
    ],
  );
}

void main() {
  testWidgets('timeline refresh is independent of active session polling', (
    tester,
  ) async {
    final engine = DiagnosticsEngineStub()..recovered = runningSession();
    final controller = DiagnosticsController(engine);
    await controller.restore();
    controller.beginTimelineUpdates();
    final initialTimelineReads = engine.timelineCalls;
    final initialSessionReads = engine.restoreCalls;
    await tester.pump(const Duration(milliseconds: 800));
    await tester.pump();
    expect(engine.restoreCalls, greaterThan(initialSessionReads));
    expect(engine.timelineCalls, initialTimelineReads);
    await tester.pump(const Duration(milliseconds: 1200));
    await tester.pump();
    expect(engine.timelineCalls, initialTimelineReads + 1);
    expect(engine.startCalls, 0);
    controller.endTimelineUpdates();
    final stoppedReads = engine.timelineCalls;
    await tester.pump(const Duration(seconds: 2));
    expect(engine.timelineCalls, stoppedReads);
    controller.dispose();
  });

  testWidgets('timeline updates without a diagnostic session or new probes', (
    tester,
  ) async {
    final engine = DiagnosticsEngineStub();
    final controller = DiagnosticsController(engine);
    addTearDown(controller.dispose);
    await controller.restore();
    controller.beginTimelineUpdates();
    final initialSessionReads = engine.restoreCalls;
    await tester.pump(const Duration(seconds: 2));
    await tester.pump();
    expect(engine.timelineCalls, 2);
    expect(engine.restoreCalls, initialSessionReads);
    expect(engine.startCalls, 0);
    controller.endTimelineUpdates();
  });

  testWidgets(
    'timed-out timeline read retains single flight until bridge completion',
    (tester) async {
      final engine = DiagnosticsEngineStub()
        ..pendingTimeline = Completer<ConnectionTimeline>();
      final controller = DiagnosticsController(engine);
      addTearDown(controller.dispose);
      final read = controller.loadTimeline(silent: true);
      await tester.pump(const Duration(milliseconds: 751));
      await read;
      expect(controller.timelineLoading, isFalse);
      await controller.loadTimeline(silent: true);
      expect(engine.timelineCalls, 1);
      engine.pendingTimeline!.complete(
        const ConnectionTimeline(droppedEventCount: 9),
      );
      await tester.pump();
      expect(controller.timeline.droppedEventCount, 0);
      engine.pendingTimeline = null;
      await controller.loadTimeline();
      expect(engine.timelineCalls, 2);
    },
  );

  testWidgets('session revisions prevent late poll or event regression', (
    tester,
  ) async {
    final engine = DiagnosticsEngineStub()..recovered = runningSession();
    final controller = DiagnosticsController(engine);
    addTearDown(controller.dispose);
    await controller.restore();
    engine.pendingRestore = Completer<DiagnosticSession?>();
    final poll = controller.restore(refreshTimeline: false);
    final terminal = DiagnosticSession(
      sessionId: 'session-one',
      state: DiagnosticSessionState.completed,
      startedAt: DateTime.fromMillisecondsSinceEpoch(1),
      mode: DiagnosticMode.standard,
      revision: 5,
    );
    controller.handleEngineEvent(
      EngineSnapshotEvent(
        diagnosticsChanged: true,
        diagnosticSession: terminal,
      ),
    );
    engine.pendingRestore!.complete(runningSession());
    await poll;
    controller.handleEngineEvent(
      EngineSnapshotEvent(
        diagnosticsChanged: true,
        diagnosticSession: runningSession(),
      ),
    );
    expect(controller.session, same(terminal));
    expect(controller.state, DiagnosticsControllerState.completed);
  });

  testWidgets(
    'reset discards a late timeline response and stops old session polling',
    (tester) async {
      final engine = DiagnosticsEngineStub()
        ..pendingTimeline = Completer<ConnectionTimeline>();
      final controller = DiagnosticsController(engine);
      addTearDown(controller.dispose);
      final read = controller.loadTimeline();
      controller.reset();
      engine.pendingTimeline!.complete(
        const ConnectionTimeline(droppedEventCount: 4),
      );
      await read;
      expect(controller.timeline.droppedEventCount, 0);
      expect(controller.timelineLoading, isFalse);
      expect(controller.session, isNull);
    },
  );

  testWidgets('older full snapshot cannot regress parallel active checks', (
    tester,
  ) async {
    final engine = DiagnosticsEngineStub();
    final controller = DiagnosticsController(engine);
    final newest = DiagnosticSession(
      sessionId: 'session-one',
      state: DiagnosticSessionState.running,
      startedAt: DateTime.fromMillisecondsSinceEpoch(1),
      mode: DiagnosticMode.standard,
      revision: 7,
      activeChecks: const ['quality.rtt', 'quality.packet_loss'],
    );
    controller.handleEngineEvent(
      EngineSnapshotEvent(diagnosticsChanged: true, diagnosticSession: newest),
    );
    final old = DiagnosticSession(
      sessionId: 'session-one',
      state: DiagnosticSessionState.running,
      startedAt: newest.startedAt,
      mode: DiagnosticMode.standard,
      revision: 6,
      activeChecks: const ['quality.rtt'],
    );
    controller.handleEngineEvent(
      EngineSnapshotEvent(diagnosticsChanged: true, diagnosticSession: old),
    );
    expect(controller.session, same(newest));
    expect(controller.session!.runningCheckIds, hasLength(2));
    controller.dispose();
    await tester.pump();
  });

  testWidgets(
    'second pending start cancels its own session despite old events',
    (tester) async {
      final old = DiagnosticSession(
        sessionId: 'old',
        state: DiagnosticSessionState.completed,
        startedAt: DateTime.fromMillisecondsSinceEpoch(1),
        mode: DiagnosticMode.standard,
        progressPercent: 100,
      );
      final engine = DiagnosticsEngineStub()..recovered = old;
      final controller = DiagnosticsController(engine);
      addTearDown(controller.dispose);
      await controller.restore();
      engine.pendingStart = Completer<DiagnosticSession>();
      final start = controller.start(DiagnosticMode.deep);
      controller.handleEngineEvent(
        EngineSnapshotEvent(diagnosticsChanged: true, diagnosticSession: old),
      );
      final next = runningSession(id: 'new', mode: DiagnosticMode.deep);
      controller.handleEngineEvent(
        EngineSnapshotEvent(diagnosticsChanged: true, diagnosticSession: next),
      );
      await controller.cancel();
      expect(controller.state, DiagnosticsControllerState.cancelling);
      engine.pendingStart!.complete(next);
      await start;
      expect(engine.cancelCalls, 1);
      expect(engine.cancelledId, 'new');
      expect(controller.session?.state, DiagnosticSessionState.cancelled);
    },
  );

  testWidgets('restore begun before start cannot overwrite the new session', (
    tester,
  ) async {
    final engine = DiagnosticsEngineStub()
      ..pendingRestore = Completer<DiagnosticSession?>();
    final controller = DiagnosticsController(engine);
    addTearDown(controller.dispose);
    final restore = controller.restore();
    await controller.start(DiagnosticMode.deep);
    engine.pendingRestore!.complete(null);
    await restore;
    expect(controller.isActive, isTrue);
    expect(controller.session?.mode, DiagnosticMode.deep);
    engine.pendingRestore = null;
    final reads = engine.restoreCalls;
    await tester.pump(const Duration(milliseconds: 800));
    expect(engine.restoreCalls, greaterThan(reads));
    await controller.cancel();
  });

  testWidgets('restore recovers an active diagnostic session', (tester) async {
    final engine = DiagnosticsEngineStub()..recovered = runningSession();
    final controller = DiagnosticsController(engine);

    await controller.restore();

    expect(controller.state, DiagnosticsControllerState.running);
    expect(controller.session?.sessionId, 'session-one');
    expect(controller.timeline, same(engine.timeline));
    controller.dispose();
  });

  testWidgets('repeated start while pending creates only one session', (
    tester,
  ) async {
    final engine = DiagnosticsEngineStub()
      ..pendingStart = Completer<DiagnosticSession>();
    final controller = DiagnosticsController(engine);

    final first = controller.start(DiagnosticMode.standard);
    expect(controller.requestedMode, DiagnosticMode.standard);
    await controller.start(DiagnosticMode.deep);
    expect(engine.startCalls, 1);

    engine.pendingStart!.complete(runningSession());
    await first;
    expect(controller.state, DiagnosticsControllerState.running);
    expect(controller.requestedMode, isNull);
    controller.dispose();
  });

  testWidgets('active deep session keeps Deep selected after reopening', (
    tester,
  ) async {
    final engine = DiagnosticsEngineStub()
      ..recovered = runningSession(mode: DiagnosticMode.deep);
    final app = AppController(engine);

    await tester.pumpWidget(
      MaterialApp(
        theme: UsqueTheme.light(),
        home: DiagnosticsScreen(controller: app),
      ),
    );
    await tester.pump();
    await tester.pump();

    final modePicker = tester.widget<SegmentedButton<DiagnosticMode>>(
      find.byType(SegmentedButton<DiagnosticMode>),
    );
    expect(modePicker.selected, <DiagnosticMode>{DiagnosticMode.deep});
    expect(modePicker.onSelectionChanged, isNull);
    app.dispose();
  });

  testWidgets(
    'empty recovery during a pending start cannot open a second session',
    (tester) async {
      final engine = DiagnosticsEngineStub()
        ..pendingStart = Completer<DiagnosticSession>();
      final controller = DiagnosticsController(engine);

      final first = controller.start(DiagnosticMode.standard);
      controller.handleEngineEvent(
        const EngineSnapshotEvent(diagnosticsChanged: true),
      );
      await tester.pump();
      await controller.start(DiagnosticMode.deep);

      expect(engine.startCalls, 1);
      expect(controller.state, DiagnosticsControllerState.starting);
      engine.pendingStart!.complete(runningSession());
      await first;
      controller.dispose();
    },
  );

  testWidgets(
    'cancel requested while start is pending cancels the created session',
    (tester) async {
      final engine = DiagnosticsEngineStub()
        ..pendingStart = Completer<DiagnosticSession>();
      final controller = DiagnosticsController(engine);

      final start = controller.start(DiagnosticMode.standard);
      await controller.cancel();
      await controller.start(DiagnosticMode.deep);
      expect(controller.state, DiagnosticsControllerState.cancelling);

      final running = runningSession();
      engine.recovered = running;
      engine.pendingStart!.complete(running);
      await start;

      expect(engine.startCalls, 1);
      expect(engine.cancelCalls, 1);
      expect(controller.state, DiagnosticsControllerState.completed);
      expect(controller.session?.state, DiagnosticSessionState.cancelled);
      controller.dispose();
    },
  );

  testWidgets(
    'cancel reaches a terminal state and does not remain cancelling',
    (tester) async {
      final engine = DiagnosticsEngineStub()..recovered = runningSession();
      final controller = DiagnosticsController(engine);
      await controller.restore();

      await controller.cancel();

      expect(controller.state, DiagnosticsControllerState.completed);
      expect(controller.session?.state, DiagnosticSessionState.cancelled);
      controller.dispose();
    },
  );

  testWidgets('lost diagnostic event recovers from GetDiagnostics', (
    tester,
  ) async {
    final engine = DiagnosticsEngineStub()..recovered = runningSession();
    final controller = DiagnosticsController(engine);

    controller.handleEngineEvent(
      const EngineSnapshotEvent(diagnosticsChanged: true),
    );
    await tester.pump();

    expect(engine.restoreCalls, greaterThanOrEqualTo(1));
    expect(controller.session?.sessionId, 'session-one');
    controller.dispose();
  });

  testWidgets('export has independent state and retains the safe destination', (
    tester,
  ) async {
    final engine = DiagnosticsEngineStub()..recovered = runningSession();
    final controller = DiagnosticsController(engine);
    await controller.restore();

    final destination = await controller.export();

    expect(destination, 'test-diagnostics.zip');
    expect(controller.exporting, isFalse);
    expect(controller.lastExportPath, destination);
    expect(engine.exportCalls, 1);
    controller.dispose();
  });

  testWidgets('diagnostics layout supports narrow Chinese and long failures', (
    tester,
  ) async {
    await tester.binding.setSurfaceSize(const Size(375, 812));
    addTearDown(() => tester.binding.setSurfaceSize(null));
    final engine = DiagnosticsEngineStub()
      ..recovered = DiagnosticSession(
        sessionId: 'layout-session',
        state: DiagnosticSessionState.completed,
        startedAt: DateTime.fromMillisecondsSinceEpoch(1, isUtc: true),
        completedAt: DateTime.fromMillisecondsSinceEpoch(2, isUtc: true),
        mode: DiagnosticMode.deep,
        progressPercent: 100,
        findings: const <DiagnosticFinding>[
          DiagnosticFinding(
            checkId: 'transport.h3_connect',
            category: DiagnosticCategory.transport,
            status: DiagnosticCheckStatus.failed,
            failure: TransportFailureInfo(
              code: 'H3_HANDSHAKE_TIMEOUT',
              stage: 'quic_handshake',
              retryable: true,
              fallbackAllowed: true,
              remediationKey: 'try_http2',
            ),
          ),
        ],
        summary: const DiagnosticSummary(failed: 1),
      );
    final app = AppController(engine)
      ..localePreference = LocalePreference.simplifiedChinese;

    await tester.pumpWidget(
      MaterialApp(
        theme: UsqueTheme.light(),
        home: DiagnosticsScreen(controller: app),
      ),
    );
    await tester.pumpAndSettle();

    expect(find.text('开始诊断'), findsOneWidget);
    expect(find.text('HTTP/3 连接'), findsOneWidget);
    expect(tester.takeException(), isNull);
    app.dispose();
  });

  testWidgets('diagnostics layout supports a wide dark desktop viewport', (
    tester,
  ) async {
    await tester.binding.setSurfaceSize(const Size(1280, 800));
    addTearDown(() => tester.binding.setSurfaceSize(null));
    final engine = DiagnosticsEngineStub();
    final app = AppController(engine)
      ..localePreference = LocalePreference.english;

    await tester.pumpWidget(
      MaterialApp(
        theme: UsqueTheme.dark(),
        home: DiagnosticsScreen(controller: app),
      ),
    );
    await tester.pumpAndSettle();

    expect(find.text('Run network diagnostics'), findsOneWidget);
    expect(find.text('Connection timeline'), findsOneWidget);
    expect(tester.takeException(), isNull);
    app.dispose();
  });

  testWidgets(
    'TV viewport exposes each check as an expandable semantic button',
    (tester) async {
      await tester.binding.setSurfaceSize(const Size(1920, 1080));
      addTearDown(() => tester.binding.setSurfaceSize(null));
      final engine = DiagnosticsEngineStub()
        ..recovered = DiagnosticSession(
          sessionId: 'tv-session',
          state: DiagnosticSessionState.completed,
          startedAt: DateTime.fromMillisecondsSinceEpoch(1, isUtc: true),
          completedAt: DateTime.fromMillisecondsSinceEpoch(2, isUtc: true),
          mode: DiagnosticMode.standard,
          progressPercent: 100,
          findings: const <DiagnosticFinding>[
            DiagnosticFinding(
              checkId: 'transport.h3_connect',
              category: DiagnosticCategory.transport,
              status: DiagnosticCheckStatus.failed,
              failure: TransportFailureInfo(
                code: 'H3_HANDSHAKE_TIMEOUT',
                stage: 'quic_handshake',
              ),
            ),
          ],
          summary: const DiagnosticSummary(failed: 1),
        );
      final app = AppController(engine)
        ..localePreference = LocalePreference.english;

      await tester.pumpWidget(
        MaterialApp(
          theme: UsqueTheme.light(),
          home: DiagnosticsScreen(controller: app),
        ),
      );
      await tester.pumpAndSettle();

      final check = find.semantics.byPredicate(
        (node) =>
            node.label.contains('HTTP/3 connection') &&
            node.getSemanticsData().hasAction(SemanticsAction.tap),
        describeMatch: (_) => 'expandable HTTP/3 diagnostic check',
      );
      expect(check, findsOneWidget);
      tester.semantics.tap(check);
      await tester.pumpAndSettle();
      expect(find.text('HTTP/3 handshake timeout'), findsOneWidget);
      expect(tester.takeException(), isNull);
      app.dispose();
    },
  );
}
