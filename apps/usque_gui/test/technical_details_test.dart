import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:usque/core/app_strings.dart';
import 'package:usque/core/diagnostics_strings.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/models/diagnostics_models.dart';
import 'package:usque/screens/diagnostics_screen.dart';
import 'package:usque/screens/network_quality_screen.dart';
import 'package:usque/widgets/diagnostic_finding_card.dart';

import 'quality_test_support.dart';
import 'ui_workflow_test.dart' show workflowHost;

void main() {
  test('skip reasons are readable in every supported language', () {
    for (final locale in LocalePreference.values) {
      if (locale == LocalePreference.system) continue;
      final strings = AppStrings(locale);
      for (final (reason, key) in [
        ('no_active_tunnel', 'diag_skip_disconnected'),
        ('not_configured', 'diag_skip_disabled'),
        ('platform_capability_unavailable', 'diag_skip_unsupported'),
        ('no_application_traffic', 'diag_skip_traffic'),
        ('run_deep_diagnostics', 'diag_skip_deep'),
        ('future_internal_reason', 'diag_finding_skipped'),
      ]) {
        final result = diagnosticSkipReason(
          strings,
          DiagnosticFinding(
            checkId: 'tunnel.dns',
            category: DiagnosticCategory.tunnel,
            status: DiagnosticCheckStatus.skipped,
            dependencyReason: reason,
          ),
        );
        expect(result, strings.get(key));
        expect(result, isNot(key));
        expect(result, isNot(contains(reason)));
        if (locale != LocalePreference.english) {
          expect(result, isNot(AppStrings(LocalePreference.english).get(key)));
        }
      }
    }
  });

  for (final locale in [
    LocalePreference.english,
    LocalePreference.simplifiedChinese,
    LocalePreference.arabic,
  ]) {
    testWidgets('diagnostic evidence is opt-in and selectable: $locale', (
      tester,
    ) async {
      await tester.binding.setSurfaceSize(const Size(375, 900));
      addTearDown(() => tester.binding.setSurfaceSize(null));
      final app = qualityApp(QualityEngineStub(), locale: locale);
      addTearDown(app.dispose);
      await tester.pumpWidget(
        workflowHost(
          app,
          scale: 2,
          dark: locale != LocalePreference.english,
          home: Scaffold(
            body: Directionality(
              textDirection: locale == LocalePreference.arabic
                  ? TextDirection.rtl
                  : TextDirection.ltr,
              child: SingleChildScrollView(
                child: DiagnosticFindingCard(
                  strings: app.strings,
                  finding: const DiagnosticFinding(
                    checkId: 'tunnel.dns',
                    category: DiagnosticCategory.tunnel,
                    status: DiagnosticCheckStatus.skipped,
                    dependencyReason: 'no_active_tunnel',
                    sanitizedEvidence: ['probe_ms=42'],
                  ),
                ),
              ),
            ),
          ),
        ),
      );
      await tester.pumpAndSettle();
      expect(
        find.text(app.strings.get('diag_skip_disconnected')),
        findsOneWidget,
      );
      expect(find.text('no_active_tunnel'), findsNothing);
      expect(find.text('probe_ms=42'), findsNothing);
      await tester.sendKeyEvent(LogicalKeyboardKey.tab);
      await tester.sendKeyEvent(LogicalKeyboardKey.enter);
      await tester.pumpAndSettle();
      expect(
        find.widgetWithText(SelectableText, 'probe_ms=42'),
        findsOneWidget,
      );
      await tester.sendKeyEvent(LogicalKeyboardKey.enter);
      await tester.pumpAndSettle();
      expect(find.text('probe_ms=42'), findsNothing);
      expect(tester.takeException(), isNull);
    });
  }

  testWidgets('L4 details and metric help leave live status visible', (
    tester,
  ) async {
    await tester.binding.setSurfaceSize(const Size(1280, 2000));
    addTearDown(() => tester.binding.setSurfaceSize(null));
    final engine = QualityEngineStub();
    final app = qualityApp(engine);
    addTearDown(app.dispose);
    app.snapshot = EngineSnapshot(
      phase: ConnectionPhase.connected,
      transport: 'HTTP/3',
      dataPlane: DataPlaneMode.l4Proxy,
      networkQuality: qualityFixture(DateTime.utc(2026, 9, 2)),
      l4: const L4Snapshot(sessions: 1, connectVerified: true, bufferBytes: 42),
    );
    await tester.pumpWidget(
      workflowHost(app, home: NetworkQualityScreen(controller: app)),
    );
    await tester.pumpAndSettle();
    expect(find.text(app.strings.get('l4_verified')), findsOneWidget);
    expect(find.text(app.strings.get('l4_na')), findsNothing);
    final budget = '${app.strings.get('l4_buffers')}: 42';
    expect(find.text(budget), findsNothing);
    await tester.tap(find.text(app.strings.get('technical_details')).first);
    await tester.pumpAndSettle();
    expect(find.text(budget), findsOneWidget);
    expect(find.text(app.strings.get('l4_na')), findsOneWidget);
    await tester.tap(find.text(app.strings.get('technical_details')).first);
    await tester.pumpAndSettle();
    expect(find.text(budget), findsNothing);
    expect(find.text(app.strings.get('nq_pmtu_help')), findsNothing);
    final help = find.byTooltip(app.strings.get('nq_pmtu'));
    await tester.ensureVisible(help);
    await tester.tap(help);
    await tester.pumpAndSettle();
    expect(find.text(app.strings.get('nq_pmtu_help')), findsOneWidget);
    await tester.tap(find.text(app.strings.get('close')));
    await tester.pumpAndSettle();
    expect(find.text(app.strings.get('nq_pmtu_help')), findsNothing);
    expect(app.snapshot.isConnected, isTrue);
    expect(engine.modes, isEmpty);
    expect(tester.takeException(), isNull);
  });

  testWidgets(
    'app API information is hidden until expanded and can be selected',
    (tester) async {
      await tester.binding.setSurfaceSize(const Size(1280, 1500));
      addTearDown(() => tester.binding.setSurfaceSize(null));
      final app = qualityApp(QualityEngineStub());
      addTearDown(app.dispose);
      await tester.pumpWidget(
        workflowHost(app, home: DiagnosticsScreen(controller: app)),
      );
      await tester.pumpAndSettle();
      expect(find.text('IPC API: usque.v1'), findsNothing);
      final details = find.text(app.strings.get('technical_details'));
      await tester.ensureVisible(details);
      await tester.tap(details);
      await tester.pumpAndSettle();
      expect(
        find.widgetWithText(SelectableText, 'IPC API: usque.v1'),
        findsOneWidget,
      );
      await tester.tap(details);
      await tester.pumpAndSettle();
      expect(find.text('IPC API: usque.v1'), findsNothing);
      expect(tester.takeException(), isNull);
    },
  );
}
