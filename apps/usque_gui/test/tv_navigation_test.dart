import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/screens/advanced_settings_screen.dart';
import 'package:usque/screens/scenario_editor_screen.dart';
import 'package:usque/widgets/tv_input_scope.dart';
import 'package:usque/widgets/usque_dialog.dart';

import 'scenarios_test.dart' show SceneEngine;
import 'section_navigation_test.dart' show openSettingsRow;
import 'ui_workflow_test.dart'
    show WorkflowEngine, fieldWithLabel, pumpWorkflow, workflowHost;

void main() {
  testWidgets(
    'TV Advanced preserves edits before the nested route discard prompt',
    (tester) async {
      final app = await pumpWorkflow(
        tester,
        WorkflowEngine(),
        section: AppSection.settings,
      );
      final input = TvInputController(isTv: true);
      addTearDown(input.dispose);
      await tester.pumpWidget(
        TvInputHost(controller: input, child: workflowHost(app)),
      );
      await tester.pumpAndSettle();
      await openSettingsRow(tester, app, 'advanced');
      final field = fieldWithLabel('SNI');
      await tester.ensureVisible(field);
      tester.widget<TextField>(field).focusNode!.requestFocus();
      await tester.pumpAndSettle();
      expect(tester.testTextInput.isVisible, isFalse);
      await tester.sendKeyEvent(LogicalKeyboardKey.select);
      await tester.pumpAndSettle();
      expect(tester.testTextInput.isVisible, isTrue);
      tester.testTextInput.enterText('tv.example');
      await tester.pumpAndSettle();
      await tester.binding.handlePopRoute();
      await tester.pumpAndSettle();
      expect(find.byType(AdvancedSettingsScreen), findsOneWidget);
      expect(find.byType(UsqueDialog), findsNothing);
      expect(tester.widget<TextField>(field).controller!.text, 'tv.example');
      expect(tester.widget<TextField>(field).focusNode!.hasFocus, isTrue);
      await tester.binding.handlePopRoute();
      await tester.pumpAndSettle();
      expect(
        find.text(app.strings.get('discard_changes_title')),
        findsOneWidget,
      );
      await tester.tap(find.text(app.strings.get('keep_editing')));
      await tester.pumpAndSettle();
      expect(tester.testTextInput.isVisible, isFalse);
      await tester.pumpWidget(const SizedBox.shrink());
    },
    variant: TargetPlatformVariant.only(TargetPlatform.android),
  );

  testWidgets(
    'TV scene and nested account dialog retain independent drafts',
    (tester) async {
      final engine = SceneEngine();
      final app = await pumpWorkflow(
        tester,
        engine,
        section: AppSection.profiles,
      );
      app.engineCapabilities = await engine.getCapabilities();
      await app.scenarios.refresh();
      final input = TvInputController(isTv: true);
      addTearDown(input.dispose);
      await tester.pumpWidget(
        TvInputHost(controller: input, child: workflowHost(app)),
      );
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(const ValueKey('scenario-create')));
      await tester.pumpAndSettle();
      final scene = find.byKey(const ValueKey('scenario-name'));
      await tester.tap(scene);
      await tester.pumpAndSettle();
      tester.testTextInput.enterText('TV scene draft');
      await tester.pumpAndSettle();
      await tester.binding.handlePopRoute();
      await tester.pumpAndSettle();
      expect(find.byType(ScenarioEditorScreen), findsOneWidget);
      expect(find.byType(UsqueDialog), findsNothing);
      await tester.tap(
        find.byKey(const ValueKey('scenario-account-management')),
      );
      await tester.pumpAndSettle();
      final add = find.text(app.strings.get('new_profile'));
      await tester.ensureVisible(add);
      await tester.tap(add);
      await tester.pumpAndSettle();
      final name = fieldWithLabel(app.strings.get('profile_name'));
      expect(tester.testTextInput.isVisible, isFalse);
      await tester.tap(name);
      await tester.pumpAndSettle();
      tester.testTextInput.enterText('TV account draft');
      await tester.pumpAndSettle();
      await tester.binding.handlePopRoute();
      await tester.pumpAndSettle();
      expect(find.byType(UsqueDialog), findsOneWidget);
      expect(
        tester.widget<TextField>(name).controller!.text,
        'TV account draft',
      );
      await tester.binding.handlePopRoute();
      await tester.pumpAndSettle();
      expect(find.byType(UsqueDialog), findsNothing);
      await tester.binding.handlePopRoute();
      await tester.pumpAndSettle();
      expect(
        tester.widget<TextFormField>(scene).controller!.text,
        'TV scene draft',
      );
      await tester.pumpWidget(const SizedBox.shrink());
    },
    variant: TargetPlatformVariant.only(TargetPlatform.android),
  );
}
