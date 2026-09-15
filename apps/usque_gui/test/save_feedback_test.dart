import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/screens/advanced_settings_screen.dart';
import 'package:usque/widgets/save_changes_bar.dart';

import 'ui_workflow_test.dart';

void main() {
  for (final advanced in [false, true]) {
    for (final locale in [
      LocalePreference.english,
      LocalePreference.simplifiedChinese,
    ]) {
      testWidgets(
        'validation supersedes previous save failure: advanced=$advanced ${locale.name}',
        (tester) async {
          final engine = WorkflowEngine()..failProfileUpsert = true;
          final app = await pumpWorkflow(tester, engine);
          app.localePreference = locale;
          await tester.pumpWidget(
            workflowHost(
              app,
              home: advanced ? AdvancedSettingsScreen(controller: app) : null,
            ),
          );
          await tester.pumpAndSettle();
          final port = fieldWithLabel(app.strings.get('port'));
          final apply = find.widgetWithText(
            FilledButton,
            app.strings.get('save_changes'),
          );
          final bar = find.byType(SaveChangesBar);
          Finder barText(String key) => find.descendant(
            of: bar,
            matching: find.text(app.strings.get(key)),
          );
          await tester.ensureVisible(port);
          await tester.enterText(port, '9090');
          await tester.pumpAndSettle();
          await tester.tap(apply);
          await tester.pumpAndSettle();
          expect(barText('settings_save_failed'), findsOneWidget);
          expect(engine.writes, 1);
          await tester.enterText(port, '70000');
          await tester.pumpAndSettle();
          await tester.tap(apply);
          await tester.pumpAndSettle();
          expect(barText('form_errors'), findsOneWidget);
          expect(barText('settings_save_failed'), findsNothing);
          expect(tester.widget<TextField>(port).focusNode!.hasFocus, isTrue);
          expect(engine.writes, 1);
          engine.failProfileUpsert = false;
          await tester.enterText(port, '9090');
          await tester.pumpAndSettle();
          expect(barText('form_errors'), findsNothing);
          await tester.tap(apply);
          await tester.pumpAndSettle();
          expect(engine.writes, 2);
          expect(barText('settings_save_failed'), findsNothing);
          expect(
            advanced
                ? app.activeProfile.endpointPort
                : app.activeProfile.proxy.socksPort,
            9090,
          );
        },
      );
    }
  }
}
