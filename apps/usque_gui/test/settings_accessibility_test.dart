import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:usque/models/app_models.dart';

import 'ui_workflow_test.dart';

void main() {
  for (final platform in [TargetPlatform.windows, TargetPlatform.android]) {
    for (final locale in [
      LocalePreference.english,
      LocalePreference.simplifiedChinese,
    ]) {
      testWidgets(
        'pickers announce field and value and remain keyboard operable: ${platform.name} ${locale.name}',
        (tester) async {
          final handle = tester.ensureSemantics();
          try {
            final app = await pumpWorkflow(
              tester,
              WorkflowEngine(),
              section: AppSection.settings,
              size: platform == TargetPlatform.android
                  ? const Size(390, 844)
                  : const Size(1280, 900),
            );
            app.localePreference = locale;
            await tester.pumpWidget(workflowHost(app));
            await tester.pumpAndSettle();
            final theme = find.byWidgetPredicate(
              (widget) => widget is DropdownButton<ThemePreference>,
            );
            final language = find.byWidgetPredicate(
              (widget) => widget is DropdownButton<LocalePreference>,
            );
            await tester.ensureVisible(theme);
            await tester.pumpAndSettle();
            final themeData = tester.getSemantics(theme).getSemanticsData();
            expect(themeData.label, contains(app.strings.get('theme')));
            expect(themeData.label, contains(app.strings.get('theme_system')));
            await tester.ensureVisible(language);
            await tester.pumpAndSettle();
            final languageData = tester
                .getSemantics(language)
                .getSemanticsData();
            expect(languageData.label, contains(app.strings.get('language')));
            expect(
              languageData.label,
              contains(app.strings.get(locale.languageLabelKey)),
            );
            await tester.ensureVisible(theme);
            await tester.pumpAndSettle();
            await tester.tap(theme);
            await tester.pumpAndSettle();
            await tester.sendKeyEvent(LogicalKeyboardKey.escape);
            await tester.pumpAndSettle();
            await tester.sendKeyEvent(LogicalKeyboardKey.enter);
            await tester.pumpAndSettle();
            await tester.tap(find.text(app.strings.get('theme_dark')).last);
            await tester.pumpAndSettle();
            expect(app.themePreference, ThemePreference.dark);
            expect(tester.takeException(), isNull);
          } finally {
            handle.dispose();
          }
        },
        variant: TargetPlatformVariant.only(platform),
      );
    }
  }
}
