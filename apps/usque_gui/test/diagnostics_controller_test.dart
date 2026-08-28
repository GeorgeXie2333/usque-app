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
  DiagnosticSession? recovered;
  ConnectionTimeline timeline = const ConnectionTimeline();
  int startCalls = 0;
  int cancelCalls = 0;
  int restoreCalls = 0;
  int exportCalls = 0;
  Completer<DiagnosticSession>? pendingStart;

  @override
  bool get supportsSnapshotEvents => false;

  @override
  Stream<EngineSnapshotEvent> get snapshotEvents =>
      const Stream<EngineSnapshotEvent>.empty();

  @override
  Future<DiagnosticSession?> getDiagnostics() async {
    restoreCalls += 1;
    return recovered;
  }

  @override
  Future<ConnectionTimeline> getConnectionTimeline() async => timeline;

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
    await controller.start(DiagnosticMode.deep);
    expect(engine.startCalls, 1);

    engine.pendingStart!.complete(runningSession());
    await first;
    expect(controller.state, DiagnosticsControllerState.running);
    controller.dispose();
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
      expect(find.text('H3 handshake timeout'), findsOneWidget);
      expect(tester.takeException(), isNull);
      app.dispose();
    },
  );
}
