import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/widgets/connection_ring.dart';
import 'package:usque/widgets/usque_dialog.dart';

import 'ui_workflow_test.dart';

void main() {
  for (final platform in [TargetPlatform.windows, TargetPlatform.android]) {
    for (final locale in [
      LocalePreference.english,
      LocalePreference.simplifiedChinese,
    ]) {
      testWidgets(
        'error has one retry action using engine retry: ${platform.name} ${locale.name}',
        (tester) async {
          final engine = WorkflowEngine();
          final app = await pumpWorkflow(
            tester,
            engine,
            section: AppSection.home,
            size: platform == TargetPlatform.android
                ? const Size(375, 812)
                : const Size(1280, 900),
          );
          app.localePreference = locale;
          app.profileIdentityStates = {
            app.activeProfileId: ProfileIdentityState.ready,
          };
          app.snapshot = const EngineSnapshot(phase: ConnectionPhase.error);
          await tester.pumpWidget(workflowHost(app));
          await tester.pumpAndSettle();
          expect(find.text(app.strings.get('retry')), findsOneWidget);
          expect(
            find.widgetWithText(OutlinedButton, app.strings.get('retry')),
            findsNothing,
          );
          final ring = find.byType(ConnectionRing);
          expect(
            tester.widget<ConnectionRing>(ring).actionLabel,
            app.strings.get('retry'),
          );
          await tester.tap(
            find.descendant(of: ring, matching: find.byType(InkWell)),
          );
          await tester.pumpAndSettle();
          expect(engine.calls.where((call) => call == 'retry'), hasLength(1));
          expect(app.snapshot.phase, ConnectionPhase.connected);
          await app.connectOrDisconnect();
          await tester.pumpAndSettle();
          await tester.pumpWidget(const SizedBox.shrink());
        },
        variant: TargetPlatformVariant.only(platform),
      );
    }
  }

  testWidgets('error without identity opens setup instead of engine retry', (
    tester,
  ) async {
    final engine = WorkflowEngine();
    final app = await pumpWorkflow(tester, engine, section: AppSection.home);
    app.snapshot = const EngineSnapshot(phase: ConnectionPhase.error);
    await tester.pumpWidget(workflowHost(app));
    await tester.pumpAndSettle();
    final ring = find.byType(ConnectionRing);
    expect(
      tester.widget<ConnectionRing>(ring).actionLabel,
      app.strings.get('configure_identity'),
    );
    expect(find.text(app.strings.get('retry')), findsNothing);
    await tester.tap(find.descendant(of: ring, matching: find.byType(InkWell)));
    await tester.pumpAndSettle();
    expect(find.byType(UsqueDialog), findsOneWidget);
    expect(engine.calls, isNot(contains('retry')));
    expect(engine.calls, isNot(contains('connect')));
  });

  testWidgets('degraded connection retains separate disconnect and retry', (
    tester,
  ) async {
    final app = await pumpWorkflow(
      tester,
      WorkflowEngine(),
      section: AppSection.home,
    );
    app.snapshot = const EngineSnapshot(phase: ConnectionPhase.degraded);
    await tester.pumpWidget(workflowHost(app));
    await tester.pumpAndSettle();
    expect(
      tester.widget<ConnectionRing>(find.byType(ConnectionRing)).actionLabel,
      app.strings.get('disconnect'),
    );
    expect(
      find.widgetWithText(OutlinedButton, app.strings.get('retry')),
      findsOneWidget,
    );
  });
}
