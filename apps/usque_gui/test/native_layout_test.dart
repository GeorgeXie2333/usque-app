import 'dart:ui' show SemanticsAction;

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:usque/core/usque_theme.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/screens/advanced_settings_screen.dart';
import 'package:usque/screens/diagnostics_screen.dart';
import 'package:usque/screens/geo_direct_settings_screen.dart';
import 'package:usque/screens/home_screen.dart';
import 'package:usque/screens/network_quality_screen.dart';
import 'package:usque/screens/onboarding_screen.dart';
import 'package:usque/screens/per_app_proxy_screen.dart';
import 'package:usque/state/app_controller.dart';
import 'package:usque/widgets/common.dart';

import 'ui_workflow_test.dart' show WorkflowEngine, workflowHost;

Widget _host(Widget child, {bool dark = false}) => MaterialApp(
  theme: dark ? UsqueTheme.dark() : UsqueTheme.light(),
  home: Scaffold(body: Center(child: child)),
);

void main() {
  testWidgets(
    'Windows home hides the content heading and retains navigation',
    (tester) async {
      tester.view.devicePixelRatio = 1;
      tester.view.physicalSize = const Size(1280, 900);
      addTearDown(tester.view.resetDevicePixelRatio);
      addTearDown(tester.view.resetPhysicalSize);
      final app = AppController(WorkflowEngine())
        ..localePreference = LocalePreference.simplifiedChinese;
      addTearDown(app.dispose);
      await tester.pumpWidget(workflowHost(app));
      await tester.pumpAndSettle();
      expect(
        find.descendant(of: find.byType(HomeScreen), matching: find.text('首页')),
        findsNothing,
      );
      expect(
        find.descendant(
          of: find.byType(NavigationRail),
          matching: find.text('首页'),
        ),
        findsOneWidget,
      );
      expect(tester.getTopLeft(find.byType(ContentSection).first).dy, 32);
      app.selectSection(AppSection.settings);
      await tester.pumpAndSettle();
      expect(
        find.descendant(of: find.byType(PageFrame), matching: find.text('设置')),
        findsOneWidget,
      );
      await tester.pumpWidget(const SizedBox.shrink());
    },
    variant: TargetPlatformVariant.only(TargetPlatform.windows),
  );

  testWidgets(
    'home tool buttons share the mobile outline style and open their pages',
    (tester) async {
      tester.view.devicePixelRatio = 1;
      addTearDown(tester.view.resetDevicePixelRatio);
      addTearDown(tester.view.resetPhysicalSize);
      for (final dark in [false, true]) {
        for (final size in [const Size(1280, 900), const Size(375, 812)]) {
          tester.view.physicalSize = size;
          final app = AppController(WorkflowEngine())
            ..localePreference = LocalePreference.english
            ..engineCapabilities = const EngineCapabilities(
              networkQuality: true,
            );
          try {
            await tester.pumpWidget(workflowHost(app, dark: dark));
            await tester.pumpAndSettle();
            for (final key in ['home-network-quality', 'home-diagnostics']) {
              final finder = find.byKey(ValueKey(key));
              expect(tester.widget(finder), isA<OutlinedButton>());
              final button = tester.widget<OutlinedButton>(finder);
              final states = <WidgetState>{};
              expect(
                button.style?.minimumSize?.resolve(states),
                const Size(0, 48),
              );
              expect(
                button.style?.padding?.resolve(states),
                const EdgeInsets.all(10),
              );
              expect(
                button.style?.foregroundColor?.resolve(states),
                Theme.of(tester.element(finder)).colorScheme.onSurface,
              );
              expect(tester.getSize(finder).height, greaterThanOrEqualTo(48));
              await tester.ensureVisible(finder);
              await tester.pumpAndSettle();
              await tester.tap(finder);
              await tester.pumpAndSettle();
              final page = key == 'home-diagnostics'
                  ? find.byType(DiagnosticsScreen)
                  : find.byType(NetworkQualityScreen);
              expect(page, findsOneWidget);
              Navigator.of(tester.element(page)).pop();
              await tester.pumpAndSettle();
            }
            expect(tester.takeException(), isNull);
            await tester.pumpWidget(const SizedBox.shrink());
          } finally {
            app.dispose();
          }
        }
      }
    },
    variant: TargetPlatformVariant.only(TargetPlatform.windows),
  );

  testWidgets(
    'action rows have stable focus, keyboard, D-pad and tap feedback',
    (tester) async {
      var taps = 0;
      await tester.pumpWidget(
        _host(
          ActionRow(onTap: () => taps++, child: const Text('Open settings')),
        ),
      );
      final row = find.byType(ActionRow);
      final before = tester.getRect(row);
      expect(before.height, greaterThanOrEqualTo(48));
      await tester.sendKeyEvent(LogicalKeyboardKey.tab);
      await tester.pumpAndSettle();
      final material = tester.widget<Material>(
        find.descendant(of: row, matching: find.byType(Material)).first,
      );
      final side = (material.shape! as RoundedRectangleBorder).side;
      expect(side.width, 2);
      expect(side.color.a, 1);
      expect(tester.getRect(row), before);
      for (final key in [
        LogicalKeyboardKey.enter,
        LogicalKeyboardKey.space,
        LogicalKeyboardKey.select,
        LogicalKeyboardKey.gameButtonA,
      ]) {
        await tester.sendKeyEvent(key);
        await tester.pump();
      }
      expect(taps, 4);
      await tester.tap(row);
      await tester.pump();
      expect(taps, 5);
      expect(
        find.descendant(of: row, matching: find.byType(InkWell)),
        findsOneWidget,
      );
      expect(
        find.semantics.byPredicate((node) {
          final data = node.getSemanticsData();
          return data.label.contains('Open settings') &&
              data.hasAction(SemanticsAction.tap);
        }, describeMatch: (_) => 'Open settings action'),
        findsOneWidget,
      );
    },
  );

  testWidgets('disabled action rows do not activate', (tester) async {
    await tester.pumpWidget(
      _host(const ActionRow(onTap: null, child: Text('Unavailable'))),
    );
    final ink = tester.widget<InkWell>(
      find.descendant(
        of: find.byType(ActionRow),
        matching: find.byType(InkWell),
      ),
    );
    expect(ink.onTap, isNull);
    final semantics = tester.widget<Semantics>(
      find
          .descendant(
            of: find.byType(ActionRow),
            matching: find.byType(Semantics),
          )
          .first,
    );
    expect(semantics.properties.enabled, isFalse);
  });

  testWidgets(
    'open sections and inline statuses never introduce a card or badge',
    (tester) async {
      for (final dark in [false, true]) {
        for (final tone in StatusTone.values) {
          await tester.pumpWidget(
            _host(
              ContentSection(
                title: 'Network',
                children: [InlineStatus(label: tone.name, tone: tone)],
              ),
              dark: dark,
            ),
          );
          expect(find.byType(Panel), findsNothing);
          expect(find.byType(StatusPill), findsNothing);
          expect(
            find.descendant(
              of: find.byType(ContentSection),
              matching: find.byType(DecoratedBox),
            ),
            findsNothing,
          );
          final text = tester.widget<Text>(find.text(tone.name));
          final context = tester.element(find.text(tone.name));
          final foreground = text.style!.color!.computeLuminance();
          final background = UsqueTokens.of(context).canvas.computeLuminance();
          final contrast = foreground > background
              ? (foreground + 0.05) / (background + 0.05)
              : (background + 0.05) / (foreground + 0.05);
          expect(contrast, greaterThanOrEqualTo(4.5));
        }
      }
    },
  );

  testWidgets('every primary page is card-free without losing its actions', (
    tester,
  ) async {
    for (final section in AppSection.values) {
      final app = AppController(WorkflowEngine())
        ..section = section
        ..localePreference = LocalePreference.english
        ..engineCapabilities = const EngineCapabilities(networkQuality: true);
      try {
        await tester.pumpWidget(workflowHost(app));
        await tester.pumpAndSettle();
        expect(find.byType(Panel), findsNothing, reason: section.name);
        expect(find.byType(StatusPill), findsNothing, reason: section.name);
        expect(tester.takeException(), isNull);
        await tester.pumpWidget(const SizedBox.shrink());
      } finally {
        app.dispose();
      }
    }
  });

  testWidgets('account rows never activate on a label tap', (tester) async {
    final app = AppController(WorkflowEngine())
      ..section = AppSection.profiles
      ..localePreference = LocalePreference.english;
    addTearDown(app.dispose);
    final active = app.activeProfile;
    app.profiles = [active, active.copyWith(id: 'work', name: 'Work')];
    app.profileIdentityStates = {
      for (final profile in app.profiles)
        profile.id: ProfileIdentityState.ready,
    };
    await tester.pumpWidget(workflowHost(app));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Work'));
    await tester.pump();
    expect(app.activeProfileId, active.id);
    final activate = find.widgetWithText(TextButton, 'Set active');
    await tester.ensureVisible(activate);
    await tester.tap(activate);
    await tester.pumpAndSettle();
    expect(app.activeProfileId, 'work');
    expect(find.byType(Panel), findsNothing);
    await tester.pumpWidget(const SizedBox.shrink());
  });

  testWidgets(
    'secondary pages remain usable at 200 percent in both themes and languages',
    (tester) async {
      tester.view.devicePixelRatio = 1;
      addTearDown(tester.view.resetDevicePixelRatio);
      addTearDown(tester.view.resetPhysicalSize);
      for (final size in [
        const Size(320, 568),
        const Size(812, 375),
        const Size(800, 1100),
        const Size(1280, 900),
      ]) {
        tester.view.physicalSize = size;
        for (final zh in [false, true]) {
          final app = AppController(WorkflowEngine())
            ..localePreference = zh
                ? LocalePreference.simplifiedChinese
                : LocalePreference.english
            ..engineCapabilities = const EngineCapabilities(
              networkQuality: true,
            );
          try {
            final pages = <Widget>[
              AdvancedSettingsScreen(controller: app),
              GeoDirectSettingsScreen(controller: app),
              PerAppProxyScreen(controller: app),
              NetworkQualityScreen(controller: app),
              DiagnosticsScreen(controller: app),
              OnboardingScreen(controller: app),
            ];
            for (final page in pages) {
              await tester.pumpWidget(
                workflowHost(app, scale: 2, dark: zh, home: page),
              );
              await tester.pumpAndSettle();
              expect(
                find.byType(ErrorWidget),
                findsNothing,
                reason: '$size $zh ${page.runtimeType}',
              );
              expect(
                tester.takeException(),
                isNull,
                reason: '$size $zh ${page.runtimeType}',
              );
              await tester.pumpWidget(const SizedBox.shrink());
            }
          } finally {
            app.dispose();
          }
        }
      }
    },
  );
}
