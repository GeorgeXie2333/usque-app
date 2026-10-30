import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/screens/about_screen.dart';
import 'package:usque/screens/profiles_screen.dart';
import 'package:usque/screens/proxy_screen.dart';
import 'package:usque/state/app_controller.dart';

import 'ui_workflow_test.dart' show WorkflowEngine, pumpWorkflow, workflowHost;

final _work = UsqueProfile.defaultProfile().copyWith(id: 'work', name: 'Work');

Future<AppController> _accounts(
  WidgetTester tester, {
  bool second = true,
  ConnectionPhase phase = ConnectionPhase.disconnected,
}) async {
  final app = await pumpWorkflow(
    tester,
    WorkflowEngine(),
    section: AppSection.profiles,
  );
  app
    ..profiles = [UsqueProfile.defaultProfile(), if (second) _work]
    ..snapshot = EngineSnapshot(phase: phase);
  await tester.pumpWidget(
    workflowHost(
      app,
      home: Scaffold(body: ProfilesScreen(controller: app)),
    ),
  );
  await tester.pumpAndSettle();
  return app;
}

void main() {
  testWidgets('switching the account of a live session asks first', (
    tester,
  ) async {
    final app = await _accounts(tester, phase: ConnectionPhase.connected);
    final body = app.strings.get('switch_account_reconnect_body');
    await tester.tap(find.text('Set active'));
    await tester.pumpAndSettle();
    expect(find.text(body), findsOneWidget);
    await tester.tap(find.text('Cancel'));
    await tester.pumpAndSettle();
    expect(app.activeProfileId, UsqueProfile.defaultProfileId);

    await tester.tap(find.text('Set active'));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const ValueKey('profile-switch-confirm')));
    await tester.pumpAndSettle();
    expect(app.activeProfileId, 'work');
  });

  testWidgets('a disconnected or failed session switches directly', (
    tester,
  ) async {
    for (final phase in [ConnectionPhase.disconnected, ConnectionPhase.error]) {
      final app = await _accounts(tester, phase: phase);
      await tester.tap(find.text('Set active'));
      await tester.pumpAndSettle();
      expect(
        find.text(app.strings.get('switch_account_reconnect_body')),
        findsNothing,
      );
      expect(app.activeProfileId, 'work', reason: '$phase');
    }
  });

  testWidgets('the only account cannot be deleted and actions name renames', (
    tester,
  ) async {
    final app = await _accounts(tester, second: false);
    final delete = find.byKey(
      ValueKey('profile-delete-${UsqueProfile.defaultProfileId}'),
    );
    expect(tester.widget<IconButton>(delete).onPressed, isNull);
    expect(find.byTooltip(app.strings.get('profile_required')), findsOneWidget);
    expect(find.byTooltip('Rename account'), findsOneWidget);
    expect(find.byTooltip('Edit'), findsNothing);
    // Touch screens show no tooltips, so the actions carry visible labels.
    expect(
      find.descendant(
        of: find.byTooltip('Rename account'),
        matching: find.text('Rename'),
      ),
      findsOneWidget,
    );
  });

  testWidgets('Accounts, Proxy and Settings share one heading column', (
    tester,
  ) async {
    final app = await pumpWorkflow(
      tester,
      WorkflowEngine(),
      section: AppSection.profiles,
    );
    double headingLeft(String title) => tester
        .getTopLeft(
          find.descendant(
            of: find.byKey(PageStorageKey<String>(title)),
            matching: find.text(title),
          ),
        )
        .dx;
    final lefts = <double>[];
    for (final (section, title) in [
      (AppSection.profiles, 'Profiles'),
      (AppSection.proxy, 'Proxy'),
      (AppSection.settings, 'Settings'),
    ]) {
      app.selectSection(section);
      await tester.pumpAndSettle();
      lefts.add(headingLeft(title));
    }
    expect(lefts.toSet(), hasLength(1), reason: '$lefts');
  });

  testWidgets('Settings opens About with legal documents and licenses', (
    tester,
  ) async {
    final app = await pumpWorkflow(
      tester,
      WorkflowEngine(),
      section: AppSection.settings,
    );
    final about = find.byKey(const ValueKey('settings-about'));
    await tester.ensureVisible(about);
    await tester.pumpAndSettle();
    await tester.tap(about);
    await tester.pumpAndSettle();
    expect(find.byType(AboutScreen), findsOneWidget);
    expect(find.text('About Usque'), findsWidgets);
    expect(find.text(app.strings.get('app_version')), findsOneWidget);
    expect(find.text(app.strings.get('unofficial')), findsOneWidget);
    expect(find.text(app.strings.get('trademark_attribution')), findsOneWidget);
    for (final key in [
      'about-onboarding_application_terms',
      'about-onboarding_personal_privacy',
      'about-onboarding_zero_trust_privacy',
      'about-source-code',
      'about-licenses',
    ]) {
      expect(find.byKey(ValueKey(key)), findsOneWidget, reason: key);
    }
    await tester.tap(find.byKey(const ValueKey('about-licenses')));
    await tester.pumpAndSettle();
    expect(find.byType(LicensePage), findsOneWidget);
  });

  testWidgets('a disabled listener keeps its capabilities and status', (
    tester,
  ) async {
    final app = await pumpWorkflow(tester, WorkflowEngine());
    app.sharedNetwork = app.sharedNetwork.copyWith(
      frontends: app.activeProfile.frontends.copyWith(socks5: false),
    );
    await tester.pumpWidget(
      workflowHost(
        app,
        home: Scaffold(body: ProxyScreen(controller: app)),
      ),
    );
    await tester.pumpAndSettle();
    expect(find.text(app.strings.get('socks_capabilities')), findsOneWidget);
    expect(find.text(app.strings.get('output_disabled')), findsOneWidget);
    expect(find.textContaining('Settings'), findsNothing);
  });
}
