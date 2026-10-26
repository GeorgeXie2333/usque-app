import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:usque/core/usque_theme.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/models/diagnostics_models.dart';
import 'package:usque/screens/diagnostics_screen.dart';
import 'package:usque/screens/settings_screen.dart';
import 'package:usque/screens/shell_screen.dart';
import 'package:usque/state/app_controller.dart';

import 'quality_test_support.dart';

void main() {
  Widget shell(AppController app) => MaterialApp(
    theme: UsqueTheme.light(),
    builder: (context, child) => MediaQuery(
      data: MediaQuery.of(context).copyWith(disableAnimations: true),
      child: child!,
    ),
    home: ShellScreen(controller: app),
  );

  for (final (name, size) in [
    ('desktop', const Size(1280, 900)),
    ('phone', const Size(390, 844)),
  ]) {
    testWidgets(
      'failed connection opens Diagnostics in Settings with a Standard run: $name',
      (tester) async {
        await tester.binding.setSurfaceSize(size);
        addTearDown(() => tester.binding.setSurfaceSize(null));
        final engine = QualityEngineStub();
        final app = qualityApp(engine, state: 'disconnected');
        addTearDown(app.dispose);
        app.profileIdentityStates = {
          app.activeProfileId: ProfileIdentityState.ready,
        };
        app.snapshot = const EngineSnapshot(phase: ConnectionPhase.error);
        app.lastError = 'The connection is unavailable.';
        await tester.pumpWidget(shell(app));
        await tester.pumpAndSettle();

        final action = find.byKey(const ValueKey('home-error-diagnostics'));
        expect(action, findsOneWidget);
        expect(
          find.descendant(
            of: action,
            matching: find.text(app.strings.get('diag_start')),
          ),
          findsOneWidget,
        );
        await tester.ensureVisible(action);
        await tester.pumpAndSettle();
        await tester.tap(action);
        await tester.pumpAndSettle();

        expect(app.section, AppSection.settings);
        expect(find.byType(DiagnosticsScreen), findsOneWidget);
        expect(engine.modes, [DiagnosticMode.standard]);

        // Back returns to the Settings list rather than leaving the section.
        await tester.tap(
          find.widgetWithText(TextButton, app.strings.get('back')),
        );
        await tester.pumpAndSettle();
        expect(find.byType(DiagnosticsScreen), findsNothing);
        expect(find.byType(SettingsScreen), findsOneWidget);
        expect(app.section, AppSection.settings);
        expect(tester.takeException(), isNull);
        await tester.pumpWidget(const SizedBox.shrink());
      },
    );
  }

  testWidgets('errors from other operations get no Diagnostics action', (
    tester,
  ) async {
    await tester.binding.setSurfaceSize(const Size(1280, 900));
    addTearDown(() => tester.binding.setSurfaceSize(null));
    final engine = QualityEngineStub();
    final app = qualityApp(engine, state: 'disconnected');
    addTearDown(app.dispose);
    app.lastError = 'Saved accounts were reset.';
    await tester.pumpWidget(shell(app));
    await tester.pumpAndSettle();
    expect(find.text('Saved accounts were reset.'), findsOneWidget);
    expect(find.byKey(const ValueKey('home-error-diagnostics')), findsNothing);
    expect(engine.modes, isEmpty);
    await tester.pumpWidget(const SizedBox.shrink());
  });
}
