import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';
import 'package:usque/core/connection_presentation.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/screens/diagnostics_screen.dart';
import 'package:usque/state/app_controller.dart';
import 'package:usque/widgets/common.dart';

import 'ui_workflow_test.dart';

void main() {
  testWidgets('bypass row leaves its counts to the summary', (tester) async {
    final app = await pumpWorkflow(
      tester,
      WorkflowEngine(),
      section: AppSection.settings,
    );
    final row = find.ancestor(
      of: find.text(app.strings.get('geo_direct')),
      matching: find.byType(ActionRow),
    );
    expect(row, findsOneWidget);
    // The summary counts countries and custom targets; a lone trailing
    // country count would contradict it when only custom targets exist.
    expect(find.descendant(of: row, matching: find.text('0')), findsNothing);
  });

  testWidgets('diagnostics shows the connection state once', (tester) async {
    tester.view.devicePixelRatio = 1;
    tester.view.physicalSize = const Size(1280, 900);
    addTearDown(tester.view.resetDevicePixelRatio);
    addTearDown(tester.view.resetPhysicalSize);
    final app = AppController(WorkflowEngine())
      ..localePreference = LocalePreference.english;
    addTearDown(app.dispose);
    await tester.pumpWidget(
      workflowHost(app, home: DiagnosticsScreen(controller: app)),
    );
    await tester.pumpAndSettle();
    String label(ConnectionPhase phase) =>
        app.strings.get(ConnectionPresentation.of(phase).labelKey);
    expect(find.text(label(ConnectionPhase.disconnected)), findsOneWidget);

    app.snapshot = const EngineSnapshot(phase: ConnectionPhase.error);
    app.selectSection(AppSection.settings);
    await tester.pumpAndSettle();
    final status = find.ancestor(
      of: find.text(label(ConnectionPhase.error)),
      matching: find.byType(InlineStatus),
    );
    expect(status, findsOneWidget);
    // Failure keeps a distinct shape, not only a danger colour.
    expect(
      find.descendant(of: status, matching: find.byIcon(LucideIcons.circleX)),
      findsOneWidget,
    );
  });
}
