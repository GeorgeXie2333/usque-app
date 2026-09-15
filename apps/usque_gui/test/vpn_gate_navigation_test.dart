import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:shared_preferences/shared_preferences.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/screens/shell_screen.dart';
import 'package:usque/screens/vpn_gate_screen.dart';
import 'package:usque/state/app_controller.dart';

import 'ui_workflow_test.dart' show workflowHost;
import 'vpngate_test.dart' show GateEngine, server;

Future<AppController> openGate(
  WidgetTester tester,
  GateEngine engine, {
  Size size = const Size(1220, 1000),
}) async {
  SharedPreferences.setMockInitialValues({});
  tester.view.devicePixelRatio = 1;
  tester.view.physicalSize = size;
  addTearDown(tester.view.resetDevicePixelRatio);
  addTearDown(tester.view.resetPhysicalSize);
  final app = AppController(engine);
  await app.initialize();
  addTearDown(app.dispose);
  app.selectSection(AppSection.proxy);
  await tester.pumpWidget(
    workflowHost(app, home: ShellScreen(controller: app)),
  );
  await tester.pumpAndSettle();
  await tester.tap(find.text('VPN Gate'));
  await tester.pumpAndSettle();
  return app;
}

Finder railItem(AppController app, String key) => find.descendant(
  of: find.byType(NavigationRail),
  matching: find.text(app.strings.get(key)),
);

void main() {
  testWidgets(
    'Home Gate shortcut opens the shared subpage by pointer and D-pad',
    (tester) async {
      SharedPreferences.setMockInitialValues({});
      tester.view.devicePixelRatio = 1;
      addTearDown(tester.view.resetDevicePixelRatio);
      addTearDown(tester.view.resetPhysicalSize);
      for (final (width, keyboard, stage) in [
        (1220.0, false, 'disabled'),
        (390.0, false, 'connected'),
        (1220.0, true, 'error'),
      ]) {
        tester.view.physicalSize = Size(width, 1000);
        final engine = GateEngine()..legacyProfilesImported = true;
        engine.storedProfiles[0] = engine.storedProfiles[0].copyWith(
          vpnGate: const VpnGateSettings(enabled: true),
        );
        engine.current = EngineSnapshot(
          vpnGate: VpnGateStatus(
            stage: stage,
            server: stage == 'disabled' ? null : server,
          ),
        );
        final app = AppController(engine);
        await app.initialize();
        final snapshot = engine.current;
        final settings = engine.storedProfiles[0].vpnGate;
        try {
          await tester.pumpWidget(
            workflowHost(
              app,
              scale: keyboard ? 2 : 1,
              home: ShellScreen(controller: app),
            ),
          );
          await tester.pumpAndSettle();
          final entry = find.byKey(const ValueKey('home-vpn-gate-settings'));
          expect(entry, findsOneWidget);
          expect(
            find.textContaining('Proxied traffic is blocked until'),
            findsNothing,
          );
          if (keyboard) {
            Focus.of(
              tester.element(find.text('WARP → VPN Gate')),
            ).requestFocus();
            await tester.pump();
            await tester.sendKeyEvent(LogicalKeyboardKey.select);
          } else {
            await tester.tap(entry);
            // A second activation before the section paints cannot stack routes.
            await tester.tap(entry);
          }
          await tester.pumpAndSettle();
          expect(find.byType(VpnGateScreen), findsOneWidget);
          expect(app.section, AppSection.proxy);
          expect(app.activeProfile.vpnGate, settings);
          expect(app.snapshot, snapshot);
          expect(engine.saves, 0);
          if (width >= 760) {
            expect(
              tester
                  .widget<NavigationRail>(find.byType(NavigationRail))
                  .selectedIndex,
              2,
            );
            await tester.tap(find.text(app.strings.get('back')).hitTestable());
          } else {
            expect(find.byType(NavigationBar), findsNothing);
            await tester.binding.handlePopRoute();
          }
          await tester.pumpAndSettle();
          expect(find.byType(VpnGateScreen, skipOffstage: false), findsNothing);
          expect(app.section, AppSection.proxy);
          expect(tester.takeException(), isNull);
        } finally {
          await tester.pumpWidget(const SizedBox());
          app.dispose();
        }
      }
    },
  );

  testWidgets(
    'system back closes the country popup before leaving the subpage',
    (tester) async {
      await openGate(tester, GateEngine(), size: const Size(390, 1000));
      final country = find.byType(DropdownButtonFormField<String>);
      await tester.ensureVisible(country);
      await tester.tap(country);
      await tester.pumpAndSettle();
      await tester.binding.handlePopRoute();
      await tester.pumpAndSettle();
      expect(find.byType(VpnGateScreen), findsOneWidget);
      await tester.binding.handlePopRoute();
      await tester.pumpAndSettle();
      expect(find.byType(VpnGateScreen), findsNothing);
      expect(find.byType(NavigationBar), findsOneWidget);
    },
  );

  testWidgets(
    'wide Gate keeps the rail and its Proxy item returns from the bottom',
    (tester) async {
      final engine = GateEngine()
        ..nodes = List.generate(
          30,
          (i) => VpnGateServer(
            id: 'node-$i',
            ip: '203.0.113.${i + 1}',
            hostname: 'node-$i.example',
            configSha256: 'config-$i',
            countryCode: 'JP',
          ),
        );
      final app = await openGate(tester, engine);
      expect(
        tester
            .widget<NavigationRail>(find.byType(NavigationRail))
            .selectedIndex,
        2,
      );
      final scroll = tester.state<ScrollableState>(
        find
            .descendant(
              of: find.byType(VpnGateScreen),
              matching: find.byType(Scrollable),
            )
            .first,
      );
      scroll.position.jumpTo(scroll.position.maxScrollExtent);
      await tester.pumpAndSettle();
      expect(find.text(app.strings.get('back')).hitTestable(), findsNothing);
      await tester.tap(railItem(app, 'nav_proxy'));
      await tester.pumpAndSettle();
      expect(find.byType(VpnGateScreen), findsNothing);
      expect(app.section, AppSection.proxy);
      expect(find.text('VPN Gate').hitTestable(), findsOneWidget);
    },
  );

  testWidgets(
    'sidebar departure awaits the same discard decision as route back',
    (tester) async {
      final engine = GateEngine();
      final app = await openGate(tester, engine);
      await tester.tap(find.byKey(const ValueKey('vpn-gate-toggle')));
      await tester.pumpAndSettle();
      await tester.tap(railItem(app, 'nav_home'));
      await tester.pumpAndSettle();
      expect(app.section, AppSection.proxy);
      expect(
        find.text(app.strings.get('discard_changes_title')),
        findsOneWidget,
      );
      await tester.tap(find.text(app.strings.get('keep_editing')));
      await tester.pumpAndSettle();
      expect(find.byType(VpnGateScreen), findsOneWidget);
      await tester.tap(railItem(app, 'nav_home'));
      await tester.pumpAndSettle();
      await tester.tap(find.text(app.strings.get('discard_changes')));
      await tester.pumpAndSettle();
      expect(app.section, AppSection.home);
      expect(find.byType(VpnGateScreen, skipOffstage: false), findsNothing);
      expect(engine.saves, 0);
    },
  );

  testWidgets('rail keyboard navigation cannot discard a Gate draft', (
    tester,
  ) async {
    final app = await openGate(tester, GateEngine());
    await tester.tap(find.byKey(const ValueKey('vpn-gate-toggle')));
    await tester.pumpAndSettle();
    Focus.of(tester.element(railItem(app, 'nav_proxy'))).requestFocus();
    await tester.pump();
    await tester.sendKeyEvent(LogicalKeyboardKey.arrowDown);
    await tester.pumpAndSettle();
    expect(app.section, AppSection.proxy);
    await tester.tap(find.text(app.strings.get('keep_editing')));
    await tester.pumpAndSettle();
    expect(find.byType(VpnGateScreen), findsOneWidget);
  });

  testWidgets(
    'rail breakpoints keep the same Gate draft and compact system back',
    (tester) async {
      final app = await openGate(tester, GateEngine());
      await tester.tap(find.byKey(const ValueKey('vpn-gate-toggle')));
      await tester.pumpAndSettle();
      final node = find.byKey(ValueKey('vpn-gate-node-${server.id}'));
      await tester.ensureVisible(node);
      await tester.tap(node);
      await tester.pumpAndSettle();
      final state = tester.state(find.byType(VpnGateScreen));
      for (final width in [900.0, 700.0, 390.0, 1200.0, 390.0]) {
        tester.view.physicalSize = Size(width, 1000);
        await tester.pumpAndSettle();
        expect(tester.state(find.byType(VpnGateScreen)), same(state));
        expect(tester.widget<ListTile>(node).selected, isTrue);
        expect(
          find.byType(NavigationRail),
          width >= 760 ? findsOneWidget : findsNothing,
        );
        expect(find.byType(NavigationBar), findsNothing);
        expect(tester.takeException(), isNull);
      }
      await tester.binding.handlePopRoute();
      await tester.pumpAndSettle();
      expect(
        find.text(app.strings.get('discard_changes_title')),
        findsOneWidget,
      );
      await tester.tap(find.text(app.strings.get('discard_changes')));
      await tester.pumpAndSettle();
      expect(find.byType(VpnGateScreen), findsNothing);
      expect(find.byType(NavigationBar), findsOneWidget);
      expect(app.section, AppSection.proxy);
    },
  );

  testWidgets('leaving cancels an unfinished favorite without saving it', (
    tester,
  ) async {
    final engine = GateEngine()..holdPreparation = true;
    final app = await openGate(tester, engine);
    await tester.tap(find.byKey(ValueKey('vpn-gate-favorite-${server.id}')));
    await tester.pump(const Duration(milliseconds: 20));
    expect(
      engine.nodeRequests.any((request) => request.action == 'favorite'),
      isTrue,
    );
    await tester.tap(railItem(app, 'nav_settings'));
    await tester.pump(const Duration(seconds: 1));
    await tester.pumpAndSettle();
    expect(app.section, AppSection.settings);
    expect(find.byType(VpnGateScreen, skipOffstage: false), findsNothing);
    expect(
      engine.nodeRequests.any((request) => request.action == 'cancel'),
      isTrue,
    );
    expect(engine.favorites, isEmpty);
    expect(engine.saves, 0);
  });
}
