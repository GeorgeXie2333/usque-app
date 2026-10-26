import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:usque/core/usque_theme.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/models/diagnostics_models.dart';
import 'package:usque/screens/about_screen.dart';
import 'package:usque/screens/diagnostics_screen.dart';
import 'package:usque/screens/network_quality_screen.dart';
import 'package:usque/state/app_controller.dart';
import 'package:usque/widgets/clear_data_panel.dart';

import 'diagnostics_controller_test.dart' show DiagnosticsEngineStub;
import 'quality_test_support.dart';

Widget _host(Widget home) => MaterialApp(
  theme: UsqueTheme.light(),
  builder: (context, child) => MediaQuery(
    data: MediaQuery.of(context).copyWith(disableAnimations: true),
    child: child!,
  ),
  home: Scaffold(body: home),
);

DiagnosticSession _completed(List<DiagnosticFinding> findings) =>
    DiagnosticSession(
      sessionId: 'ux-session',
      state: DiagnosticSessionState.completed,
      startedAt: DateTime.utc(2026, 9, 2, 12),
      completedAt: DateTime.utc(2026, 9, 2, 12, 0, 12),
      mode: DiagnosticMode.standard,
      progressPercent: 100,
      findings: findings,
      summary: DiagnosticSummary(
        passed: findings
            .where((finding) => finding.status == DiagnosticCheckStatus.passed)
            .length,
        failed: findings
            .where((finding) => finding.status == DiagnosticCheckStatus.failed)
            .length,
      ),
    );

Future<AppController> _diagnostics(
  WidgetTester tester,
  DiagnosticSession session,
) async {
  await tester.binding.setSurfaceSize(const Size(390, 1400));
  addTearDown(() => tester.binding.setSurfaceSize(null));
  final app = AppController(DiagnosticsEngineStub()..recovered = session)
    ..localePreference = LocalePreference.english;
  addTearDown(app.dispose);
  await tester.pumpWidget(_host(DiagnosticsScreen(controller: app)));
  await tester.pumpAndSettle();
  return app;
}

const _passed = [
  DiagnosticFinding(
    checkId: 'engine.control_channel',
    category: DiagnosticCategory.localComponent,
    status: DiagnosticCheckStatus.passed,
  ),
  DiagnosticFinding(
    checkId: 'engine.configuration',
    category: DiagnosticCategory.localComponent,
    status: DiagnosticCheckStatus.passed,
  ),
  DiagnosticFinding(
    checkId: 'physical.network_present',
    category: DiagnosticCategory.physicalNetwork,
    status: DiagnosticCheckStatus.passed,
  ),
  DiagnosticFinding(
    checkId: 'physical.dns_available',
    category: DiagnosticCategory.physicalNetwork,
    status: DiagnosticCheckStatus.passed,
  ),
];

void main() {
  testWidgets('a completed run concludes and leads to its problem', (
    tester,
  ) async {
    final app = await _diagnostics(
      tester,
      _completed(const [
        ..._passed,
        DiagnosticFinding(
          checkId: 'transport.h3_connect',
          category: DiagnosticCategory.transport,
          status: DiagnosticCheckStatus.failed,
          failure: TransportFailureInfo(
            code: 'H3_UDP_UNREACHABLE',
            stage: 'quic_handshake',
            remediationKey: 'try_http2',
          ),
        ),
      ]),
    );
    final s = app.strings;
    expect(
      find.text(s.get('diag_conclusion_issues').replaceAll('{count}', '1')),
      findsOneWidget,
    );
    // A finished run no longer says it is waiting for a check.
    expect(find.text(s.get('diag_waiting_check')), findsNothing);
    // Counts that did not occur are left out.
    expect(
      find.text(s.get('diag_summary_warnings').replaceAll('{count}', '0')),
      findsNothing,
    );
    expect(
      find.text(s.get('diag_summary_failed').replaceAll('{count}', '1')),
      findsOneWidget,
    );
    // The problem's advice leads; the failed check opens with its details
    // and reads its failure, not an internal code.
    expect(find.text(s.get('diag_fix_try_http2')), findsWidgets);
    expect(find.text('H3_UDP_UNREACHABLE'), findsOneWidget);

    final scrollable = find.byType(Scrollable).first;
    final row = find.byKey(
      const ValueKey('diag-conclusion-transport.h3_connect'),
    );
    final offset = tester.state<ScrollableState>(scrollable).position.pixels;
    await tester.tap(row);
    await tester.pumpAndSettle();
    expect(
      tester.state<ScrollableState>(scrollable).position.pixels,
      greaterThan(offset),
    );
    expect(tester.takeException(), isNull);
  });

  testWidgets('a clean run says so and folds passing categories', (
    tester,
  ) async {
    final app = await _diagnostics(tester, _completed(_passed));
    final s = app.strings;
    expect(find.text(s.get('diag_conclusion_clear')), findsOneWidget);
    // Passing categories fold to one line each, with their counts.
    expect(
      find.text(s.get('diag_summary_passed').replaceAll('{count}', '2')),
      findsNWidgets(2),
    );
    expect(find.text(s.get('diag_status_passed')), findsNothing);
    expect(tester.takeException(), isNull);
  });

  testWidgets('clearing all data lives in About, not Diagnostics', (
    tester,
  ) async {
    final app = await _diagnostics(tester, _completed(_passed));
    expect(find.byType(ClearDataPanel), findsNothing);
    await tester.pumpWidget(_host(AboutScreen(controller: app)));
    await tester.pumpAndSettle();
    expect(find.byType(ClearDataPanel), findsOneWidget);
  });

  for (final (state, emphasized) in [
    ('h3', false),
    ('degraded', true),
    ('disconnected', false),
  ]) {
    testWidgets('Doctor is the primary action only when needed: $state', (
      tester,
    ) async {
      await tester.binding.setSurfaceSize(const Size(1280, 2400));
      addTearDown(() => tester.binding.setSurfaceSize(null));
      final app = qualityApp(QualityEngineStub(), state: state);
      addTearDown(app.dispose);
      await tester.pumpWidget(_host(NetworkQualityScreen(controller: app)));
      await tester.pumpAndSettle();
      final doctor = find.byKey(const ValueKey('network-doctor-standard'));
      expect(tester.widget(doctor), isA<ButtonStyleButton>());
      expect(tester.widget(doctor) is FilledButton, emphasized, reason: state);
      // A degraded phase is paired with the way to find its cause.
      expect(
        find.byKey(const ValueKey('network-quality-attention-diagnose')),
        emphasized ? findsOneWidget : findsNothing,
      );
      await tester.pumpWidget(const SizedBox.shrink());
    });
  }

  testWidgets('the attention hint opens Diagnostics with a Standard run', (
    tester,
  ) async {
    await tester.binding.setSurfaceSize(const Size(1280, 2400));
    addTearDown(() => tester.binding.setSurfaceSize(null));
    final engine = QualityEngineStub();
    final app = qualityApp(engine, state: 'degraded');
    addTearDown(app.dispose);
    await tester.pumpWidget(_host(NetworkQualityScreen(controller: app)));
    await tester.pumpAndSettle();
    final hint = find.byKey(
      const ValueKey('network-quality-attention-diagnose'),
    );
    await tester.ensureVisible(hint);
    await tester.tap(hint);
    await tester.pumpAndSettle();
    expect(find.byType(DiagnosticsScreen), findsOneWidget);
    expect(engine.modes, [DiagnosticMode.standard]);
    await tester.pumpWidget(const SizedBox.shrink());
  });
}
