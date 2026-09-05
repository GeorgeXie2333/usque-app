import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_localizations/flutter_localizations.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:usque/core/frontend_presentation.dart';
import 'package:usque/core/usque_theme.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/screens/advanced_settings_screen.dart';
import 'package:usque/screens/network_quality_screen.dart';
import 'package:usque/screens/shell_screen.dart';
import 'package:usque/state/app_controller.dart';
import 'package:usque/widgets/common.dart';

import 'app_test.dart' show FakeEngineClient;

class WorkflowEngine extends FakeEngineClient {
  int writes = 0;
  Completer<void>? pendingWrite;

  @override
  Future<void> upsertProfile(UsqueProfile profile) async {
    writes++;
    await pendingWrite?.future;
    await super.upsertProfile(profile);
  }
}

Widget workflowHost(
  AppController app, {
  bool dark = false,
  double scale = 1,
  bool reducedMotion = true,
  Widget? home,
}) {
  final locale = app.localePreference == LocalePreference.simplifiedChinese
      ? const Locale('zh', 'CN')
      : const Locale('en');
  return MaterialApp(
    debugShowCheckedModeBanner: false,
    theme: dark ? UsqueTheme.dark() : UsqueTheme.light(),
    locale: locale,
    supportedLocales: const [Locale('en'), Locale('zh', 'CN')],
    localizationsDelegates: const [
      GlobalMaterialLocalizations.delegate,
      GlobalWidgetsLocalizations.delegate,
      GlobalCupertinoLocalizations.delegate,
    ],
    builder: (context, child) => MediaQuery(
      data: MediaQuery.of(context).copyWith(
        textScaler: TextScaler.linear(scale),
        disableAnimations: reducedMotion,
      ),
      child: child!,
    ),
    home: home ?? ShellScreen(controller: app),
  );
}

Future<AppController> pumpWorkflow(
  WidgetTester tester,
  WorkflowEngine engine, {
  AppSection section = AppSection.proxy,
  Size size = const Size(1280, 900),
}) async {
  tester.view.devicePixelRatio = 1;
  tester.view.physicalSize = size;
  addTearDown(tester.view.resetDevicePixelRatio);
  addTearDown(tester.view.resetPhysicalSize);
  final app = AppController(engine)
    ..localePreference = LocalePreference.english
    ..section = section;
  addTearDown(app.dispose);
  await tester.pumpWidget(workflowHost(app));
  await tester.pumpAndSettle();
  return app;
}

Finder fieldWithLabel(String label) => find
    .byWidgetPredicate(
      (widget) => widget is TextField && widget.decoration?.labelText == label,
    )
    .first;

void main() {
  testWidgets(
    'phone keeps protection and both rates above the fold and opens quality',
    (tester) async {
      final app = await pumpWorkflow(
        tester,
        WorkflowEngine(),
        section: AppSection.home,
        size: const Size(375, 812),
      );
      app.engineCapabilities = const EngineCapabilities(networkQuality: true);
      await tester.pumpWidget(workflowHost(app));
      await tester.pumpAndSettle();
      final navTop = tester.getTopLeft(find.byType(NavigationBar)).dy;
      for (final label in ['Kill Switch', 'Download', 'Upload']) {
        expect(tester.getBottomLeft(find.text(label)).dy, lessThan(navTop));
      }
      expect(find.text('Protocol'), findsNothing);
      final quality = find.byKey(const ValueKey('home-network-quality'));
      expect(quality.hitTestable(), findsOneWidget);
      await tester.tap(quality);
      await tester.pumpAndSettle();
      expect(find.byType(NetworkQualityScreen), findsOneWidget);
      await tester.binding.handlePopRoute();
      await tester.pumpAndSettle();
      expect(find.byType(NetworkQualityScreen), findsNothing);
      await tester.ensureVisible(find.text('Connection details'));
      await tester.pumpAndSettle();
      await tester.tap(find.text('Connection details'));
      await tester.pumpAndSettle();
      expect(find.text('Protocol'), findsOneWidget);
      expect(find.text('SOCKS5 · Enabled · not running'), findsOneWidget);
      expect(tester.takeException(), isNull);
    },
  );

  testWidgets(
    'advanced defaults stay in the draft until applied and successful save clears guard',
    (tester) async {
      final engine = WorkflowEngine();
      final app = await pumpWorkflow(
        tester,
        engine,
        section: AppSection.settings,
      );
      app.sharedNetwork = app.activeProfile.copyWith(sni: 'custom.example');
      await tester.pumpWidget(
        workflowHost(app, home: AdvancedSettingsScreen(controller: app)),
      );
      await tester.pumpAndSettle();
      await tester.tap(find.text(app.strings.get('reset_defaults')));
      await tester.pumpAndSettle();
      await tester.tap(find.text(app.strings.get('reset')));
      await tester.pumpAndSettle();
      expect(engine.writes, 0);
      expect(app.activeProfile.sni, 'custom.example');
      expect(find.text('Unapplied changes'), findsOneWidget);
      await tester.tap(find.widgetWithText(FilledButton, 'Apply changes'));
      await tester.pumpAndSettle();
      expect(engine.writes, 1);
      expect(app.activeProfile.sni, UsqueProfile.defaultProfile().sni);
      expect(find.text('Unapplied changes'), findsNothing);
      final guard = tester.widget<PopScope<Object?>>(
        find.byType(PopScope<Object?>),
      );
      expect(guard.canPop, isTrue);
      expect(tester.takeException(), isNull);
    },
  );

  testWidgets(
    'proxy draft survives section changes without applying or changing runtime',
    (tester) async {
      final engine = WorkflowEngine();
      final app = await pumpWorkflow(tester, engine);
      await tester.enterText(fieldWithLabel('Port'), '9090');
      await tester.pumpAndSettle();
      app.selectSection(AppSection.home);
      await tester.pumpAndSettle();
      app.selectSection(AppSection.proxy);
      await tester.pumpAndSettle();
      expect(
        tester.widget<TextField>(fieldWithLabel('Port')).controller!.text,
        '9090',
      );
      expect(engine.writes, 0);
      expect(app.snapshot.phase, ConnectionPhase.disconnected);
      expect(find.text('Unapplied changes'), findsOneWidget);
    },
  );

  test('runtime badges do not equate enabled with running', () {
    expect(
      FrontendPresentation.of(
        configured: true,
        connection: ConnectionPhase.disconnected,
        runtime: FrontendPhase.active,
      ).labelKey,
      'output_waiting',
    );
    expect(
      FrontendPresentation.of(
        configured: true,
        connection: ConnectionPhase.connected,
      ).labelKey,
      'output_unknown',
    );
    for (final entry in {
      FrontendPhase.active: 'output_running',
      FrontendPhase.degraded: 'output_degraded',
      FrontendPhase.error: 'output_error',
      FrontendPhase.disabled: 'output_waiting',
    }.entries) {
      expect(
        FrontendPresentation.of(
          configured: true,
          connection: ConnectionPhase.connected,
          runtime: entry.key,
        ).labelKey,
        entry.value,
      );
    }
  });

  testWidgets(
    'advanced back keeps or discards a draft and persistent apply is reachable',
    (tester) async {
      final engine = WorkflowEngine();
      final app = await pumpWorkflow(
        tester,
        engine,
        section: AppSection.settings,
        size: const Size(375, 812),
      );
      final advanced = find.widgetWithText(Panel, app.strings.get('advanced'));
      await tester.ensureVisible(advanced);
      await tester.pumpAndSettle();
      await tester.tap(advanced);
      await tester.pumpAndSettle();
      await tester.enterText(fieldWithLabel('SNI'), 'review.example');
      await tester.pumpAndSettle();
      expect(find.text('Unapplied changes'), findsOneWidget);
      expect(
        find.widgetWithText(FilledButton, 'Apply changes').hitTestable(),
        findsOneWidget,
      );
      await tester.binding.handlePopRoute();
      await tester.pumpAndSettle();
      expect(find.text('Discard unapplied changes?'), findsOneWidget);
      await tester.tap(find.text('Keep editing'));
      await tester.pumpAndSettle();
      expect(
        tester.widget<TextField>(fieldWithLabel('SNI')).controller!.text,
        'review.example',
      );
      expect(engine.writes, 0);
      await tester.binding.handlePopRoute();
      await tester.pumpAndSettle();
      await tester.tap(find.text('Discard changes'));
      await tester.pumpAndSettle();
      expect(find.byType(AdvancedSettingsScreen), findsNothing);
      expect(engine.writes, 0);
    },
  );

  testWidgets('proxy keeps edits local, validates and applies once', (
    tester,
  ) async {
    final engine = WorkflowEngine();
    final app = await pumpWorkflow(tester, engine);
    final port = fieldWithLabel('Port');
    for (final value in ['7', '70', '700', '70000']) {
      await tester.enterText(port, value);
      await tester.pump();
    }
    expect(engine.writes, 0);
    expect(app.activeProfile.proxy.socksPort, 1080);
    final apply = find.widgetWithText(FilledButton, 'Apply changes');
    await tester.tap(apply);
    await tester.pumpAndSettle();
    expect(find.text(app.strings.get('invalid_port')), findsOneWidget);
    expect(engine.writes, 0);
    expect(tester.widget<TextField>(port).focusNode!.hasFocus, isTrue);
    await tester.enterText(port, '9090');
    await tester.pumpAndSettle();
    await tester.tap(apply);
    await tester.pumpAndSettle();
    expect(engine.writes, 1);
    expect(app.activeProfile.proxy.socksPort, 9090);
    expect(find.text('Changes applied'), findsOneWidget);
    expect(tester.takeException(), isNull);
  });

  testWidgets('failed proxy apply restores saved text and shows local error', (
    tester,
  ) async {
    final engine = WorkflowEngine()..failProfileUpsert = true;
    final app = await pumpWorkflow(tester, engine);
    await tester.enterText(fieldWithLabel('Port'), '9090');
    await tester.pumpAndSettle();
    await tester.tap(find.widgetWithText(FilledButton, 'Apply changes'));
    await tester.pumpAndSettle();
    expect(app.activeProfile.proxy.socksPort, 1080);
    expect(
      tester.widget<TextField>(fieldWithLabel('Port')).controller!.text,
      '1080',
    );
    expect(find.text(app.strings.get('changes_failed')), findsOneWidget);
    expect(tester.takeException(), isNull);
  });

  testWidgets('pending apply blocks duplicate submissions and edits', (
    tester,
  ) async {
    final engine = WorkflowEngine()..pendingWrite = Completer<void>();
    await pumpWorkflow(tester, engine);
    await tester.enterText(fieldWithLabel('Port'), '9090');
    await tester.pumpAndSettle();
    await tester.tap(find.widgetWithText(FilledButton, 'Apply changes'));
    await tester.pump();
    expect(engine.writes, 1);
    expect(tester.widget<TextField>(fieldWithLabel('Port')).enabled, isFalse);
    final button = tester.widget<FilledButton>(
      find.widgetWithText(FilledButton, 'Applying changes…'),
    );
    expect(button.onPressed, isNull);
    engine.pendingWrite!.complete();
    await tester.pumpAndSettle();
    expect(engine.writes, 1);
  });

  testWidgets(
    'credential completion is visible beside its action and clears password',
    (tester) async {
      final app = await pumpWorkflow(tester, WorkflowEngine());
      final username = find.byKey(const ValueKey('proxy-auth-username'));
      final password = find.byKey(const ValueKey('proxy-auth-password'));
      await tester.ensureVisible(username);
      await tester.pumpAndSettle();
      await tester.enterText(username, 'review-user');
      await tester.enterText(password, 'synthetic-test-password');
      final apply = find.byKey(const ValueKey('proxy-auth-apply'));
      await tester.ensureVisible(apply);
      await tester.pumpAndSettle();
      await tester.tap(apply);
      await tester.pumpAndSettle();
      expect(find.text(app.strings.get('proxy_auth_saved')), findsOneWidget);
      expect(tester.widget<TextField>(password).controller!.text, isEmpty);
      expect(tester.takeException(), isNull);
    },
  );
}
