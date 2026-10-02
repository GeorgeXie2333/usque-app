import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/screens/advanced_settings_screen.dart';
import 'package:usque/screens/diagnostics_screen.dart';
import 'package:usque/screens/settings_screen.dart';
import 'package:usque/state/app_controller.dart';
import 'package:usque/widgets/common.dart';

import 'ui_workflow_test.dart'
    show WorkflowEngine, fieldWithLabel, pumpWorkflow;

Finder railItem(AppController app, String key) => find.descendant(
  of: find.byType(NavigationRail),
  matching: find.text(app.strings.get(key)),
);

Future<void> openSettingsRow(
  WidgetTester tester,
  AppController app,
  String key,
) async {
  final row = find.widgetWithText(ActionRow, app.strings.get(key));
  await tester.ensureVisible(row);
  await tester.pumpAndSettle();
  await tester.tap(row);
  await tester.pumpAndSettle();
}

void main() {
  testWidgets('desktop Settings subpages keep the navigation rail', (
    tester,
  ) async {
    final app = await pumpWorkflow(
      tester,
      WorkflowEngine(),
      section: AppSection.settings,
    );
    await openSettingsRow(tester, app, 'advanced');
    expect(find.byType(AdvancedSettingsScreen), findsOneWidget);
    expect(find.byType(NavigationRail).hitTestable(), findsOneWidget);
    expect(railItem(app, 'nav_home').hitTestable(), findsOneWidget);
  });

  testWidgets(
    'phone Settings subpages hide the bar and system back returns first',
    (tester) async {
      final app = await pumpWorkflow(
        tester,
        WorkflowEngine(),
        section: AppSection.settings,
        size: const Size(375, 812),
      );
      await openSettingsRow(tester, app, 'diagnostics');
      expect(find.byType(DiagnosticsScreen), findsOneWidget);
      expect(find.byType(NavigationBar), findsNothing);
      await tester.binding.handlePopRoute();
      await tester.pumpAndSettle();
      expect(find.byType(DiagnosticsScreen), findsNothing);
      expect(find.byType(SettingsScreen), findsOneWidget);
      expect(find.byType(NavigationBar), findsOneWidget);
      expect(app.section, AppSection.settings);
    },
  );

  testWidgets(
    'rail departure from a Settings draft awaits the discard choice',
    (tester) async {
      final engine = WorkflowEngine();
      final app = await pumpWorkflow(
        tester,
        engine,
        section: AppSection.settings,
      );
      await openSettingsRow(tester, app, 'advanced');
      await tester.enterText(fieldWithLabel('SNI'), 'review.example');
      await tester.pumpAndSettle();
      await tester.tap(railItem(app, 'nav_home'));
      await tester.pumpAndSettle();
      expect(app.section, AppSection.settings);
      expect(
        find.text(app.strings.get('discard_changes_title')),
        findsOneWidget,
      );
      await tester.tap(find.text(app.strings.get('keep_editing')));
      await tester.pumpAndSettle();
      expect(app.section, AppSection.settings);
      expect(
        tester.widget<TextField>(fieldWithLabel('SNI')).controller!.text,
        'review.example',
      );
      await tester.tap(railItem(app, 'nav_home'));
      await tester.pumpAndSettle();
      await tester.tap(find.text(app.strings.get('discard_changes')));
      await tester.pumpAndSettle();
      expect(app.section, AppSection.home);
      expect(
        find.byType(AdvancedSettingsScreen, skipOffstage: false),
        findsNothing,
      );
      expect(engine.writes, 0);
    },
  );

  testWidgets('leaving Settings closes a clean subpage without asking', (
    tester,
  ) async {
    final app = await pumpWorkflow(
      tester,
      WorkflowEngine(),
      section: AppSection.settings,
    );
    await openSettingsRow(tester, app, 'diagnostics');
    await tester.tap(railItem(app, 'nav_home'));
    await tester.pumpAndSettle();
    expect(app.section, AppSection.home);
    expect(find.text(app.strings.get('discard_changes_title')), findsNothing);
    expect(find.byType(DiagnosticsScreen, skipOffstage: false), findsNothing);
    await tester.tap(railItem(app, 'nav_settings'));
    await tester.pumpAndSettle();
    expect(find.byType(SettingsScreen), findsOneWidget);
  });
}
