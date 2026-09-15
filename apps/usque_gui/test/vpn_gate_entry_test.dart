import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/screens/proxy_screen.dart';
import 'package:usque/state/app_controller.dart';
import 'package:usque/widgets/vpn_gate_entry.dart';

import 'ui_workflow_test.dart' show workflowHost;
import 'vpn_gate_summary_test.dart' show current;
import 'vpngate_test.dart' show GateEngine, server;

class _EntryEngine extends GateEngine {
  int reads = 0;
  bool failRead = false;

  @override
  Future<VpnGateDirectory> listVpnGate({
    String? countryCode,
    bool unknownCountry = false,
    int offset = 0,
    int limit = 50,
    bool favoritesOnly = false,
    bool statusOnly = false,
  }) {
    reads++;
    expect(limit, 1);
    expect(statusOnly, isFalse);
    if (failRead) return Future.error(StateError('Catalogue unavailable'));
    return pending?.future ??
        Future.value(const VpnGateDirectory(savedServer: server));
  }
}

AppController _controllerFor(_EntryEngine engine, {bool selected = true}) {
  final app = AppController(engine)
    ..section = AppSection.proxy
    ..localePreference = LocalePreference.english
    ..engineCapabilities = const EngineCapabilities(vpnGateTcp: true);
  if (selected) {
    app.sharedNetwork = app.sharedNetwork.copyWith(
      vpnGate: VpnGateSettings(
        enabled: true,
        serverId: server.id,
        configSha256: server.configSha256,
      ),
    );
  }
  return app;
}

void main() {
  final entry = find.byKey(const ValueKey('proxy-vpn-gate-entry'));

  testWidgets(
    'live Gate updates preserve listener drafts and platform readiness',
    (tester) async {
      tester.view.devicePixelRatio = 1;
      tester.view.physicalSize = const Size(1220, 1000);
      addTearDown(tester.view.resetDevicePixelRatio);
      addTearDown(tester.view.resetPhysicalSize);
      final engine = _EntryEngine();
      final app = _controllerFor(engine);
      addTearDown(app.dispose);
      await tester.pumpWidget(
        workflowHost(app, home: ProxyScreen(controller: app)),
      );
      await tester.pumpAndSettle();
      expect(find.text('Enabled · not connected'), findsOneWidget);
      expect(find.text('Saved server'), findsOneWidget);
      expect(find.textContaining(server.ip), findsOneWidget);
      final port = find.byWidgetPredicate(
        (widget) =>
            widget is TextFormField && widget.controller?.text == '1080',
      );
      expect(port, findsOneWidget);
      await tester.enterText(port, '9091');
      for (final (phase, stage, label) in [
        (ConnectionPhase.preparing, 'connected', 'Applying network settings'),
        (ConnectionPhase.connected, 'connected', 'Connected'),
        (ConnectionPhase.error, 'error', 'Connection failed'),
        (
          ConnectionPhase.disconnecting,
          'error',
          app.strings.get('disconnecting'),
        ),
      ]) {
        engine.current = EngineSnapshot(
          phase: phase,
          vpnGate: VpnGateStatus(stage: stage, server: current),
        );
        await app.refreshSnapshot();
        await tester.pumpAndSettle();
        expect(
          find.descendant(of: entry, matching: find.text(label)),
          findsOneWidget,
        );
        expect(find.textContaining(current.ip), findsOneWidget);
        expect(find.textContaining(server.ip), findsNothing);
        expect(
          find.text('WARP → VPN Gate'),
          phase == ConnectionPhase.connected ? findsOneWidget : findsNothing,
        );
        expect(find.text('9091'), findsOneWidget);
      }
      expect(engine.reads, 1);
      expect(engine.refreshes, 0);
      expect(engine.nodeRequests, isEmpty);
      expect(engine.saves, 0);
    },
  );

  testWidgets(
    'disconnect retains matching node details without a catalogue read',
    (tester) async {
      final engine = _EntryEngine();
      final app = _controllerFor(engine)
        ..snapshot = const EngineSnapshot(
          phase: ConnectionPhase.connected,
          vpnGate: VpnGateStatus(stage: 'connected', server: server),
        );
      addTearDown(app.dispose);
      await tester.pumpWidget(
        workflowHost(
          app,
          home: Scaffold(
            body: VpnGateEntry(controller: app, onOpen: () {}),
          ),
        ),
      );
      await tester.pumpAndSettle();
      expect(find.text('Connected server'), findsOneWidget);
      engine.current = const EngineSnapshot();
      await app.refreshSnapshot();
      await tester.pumpAndSettle();
      expect(find.text('Saved server'), findsOneWidget);
      expect(find.textContaining(server.ip), findsOneWidget);
      expect(find.text('Enabled · not connected'), findsOneWidget);
      expect(find.text('WARP → VPN Gate'), findsNothing);
      expect(engine.reads, 0);
    },
  );

  testWidgets('saved node reads ignore old selections and catalogue failures', (
    tester,
  ) async {
    final engine = _EntryEngine();
    final first = Completer<VpnGateDirectory>();
    engine.pending = first;
    final app = _controllerFor(engine);
    addTearDown(app.dispose);
    await tester.pumpWidget(
      workflowHost(
        app,
        home: Scaffold(
          body: VpnGateEntry(controller: app, onOpen: () {}),
        ),
      ),
    );
    await tester.pumpAndSettle();
    expect(find.text(server.id), findsOneWidget);
    final second = Completer<VpnGateDirectory>();
    engine.pending = second;
    app.sharedNetwork = app.sharedNetwork.copyWith(
      vpnGate: VpnGateSettings(
        enabled: true,
        serverId: current.id,
        configSha256: current.configSha256,
      ),
    );
    app.selectSection(AppSection.proxy);
    await tester.pump();
    first.complete(const VpnGateDirectory(savedServer: server));
    await tester.pumpAndSettle();
    expect(find.textContaining(server.ip), findsNothing);
    expect(find.text(current.id), findsOneWidget);
    second.complete(const VpnGateDirectory(savedServer: current));
    await tester.pumpAndSettle();
    expect(find.textContaining(current.ip), findsOneWidget);
    expect(find.text('Saved server'), findsOneWidget);
    engine.failRead = true;
    app.sharedNetwork = app.sharedNetwork.copyWith(
      vpnGate: VpnGateSettings(
        enabled: true,
        serverId: server.id,
        configSha256: 'new-config',
      ),
    );
    app.selectSection(AppSection.proxy);
    await tester.pumpAndSettle();
    expect(find.text(server.id), findsOneWidget);
    expect(find.textContaining(current.ip), findsNothing);
    expect(find.text('Enabled · not connected'), findsOneWidget);
    expect(tester.takeException(), isNull);
    expect(engine.reads, 3);
  });

  testWidgets(
    'disabled Gate remains reachable without directory reads or writes',
    (tester) async {
      final engine = _EntryEngine();
      final app = _controllerFor(engine, selected: false);
      addTearDown(app.dispose);
      var opened = 0;
      await tester.pumpWidget(
        workflowHost(
          app,
          home: Scaffold(
            body: VpnGateEntry(controller: app, onOpen: () => opened++),
          ),
        ),
      );
      await tester.pumpAndSettle();
      expect(find.text('VPN Gate is off'), findsOneWidget);
      await tester.tap(find.text('Manage servers'));
      expect(opened, 1);
      expect(engine.reads, 0);
      expect(engine.saves, 0);
      expect(engine.refreshes, 0);
    },
  );

  testWidgets(
    'entry supports narrow layouts, large text and keyboard activation',
    (tester) async {
      tester.view.devicePixelRatio = 1;
      addTearDown(tester.view.resetDevicePixelRatio);
      addTearDown(tester.view.resetPhysicalSize);
      final app = _controllerFor(_EntryEngine());
      addTearDown(app.dispose);
      app.snapshot = const EngineSnapshot(
        phase: ConnectionPhase.connected,
        vpnGate: VpnGateStatus(stage: 'connected', server: server),
      );
      var opened = 0;
      for (final (size, scale, locale, dark) in [
        (const Size(320, 844), 1.0, LocalePreference.english, false),
        (const Size(375, 844), 2.0, LocalePreference.simplifiedChinese, true),
        (const Size(844, 390), 2.0, LocalePreference.english, true),
        (const Size(1220, 900), 1.0, LocalePreference.simplifiedChinese, false),
      ]) {
        tester.view.physicalSize = size;
        app.localePreference = locale;
        await tester.pumpWidget(
          workflowHost(
            app,
            dark: dark,
            scale: scale,
            home: Scaffold(
              body: SingleChildScrollView(
                child: VpnGateEntry(controller: app, onOpen: () => opened++),
              ),
            ),
          ),
        );
        await tester.pumpAndSettle();
        final buttonSemantics = tester.ensureSemantics();
        await tester.pump();
        expect(
          tester
              .getSemantics(entry)
              .getSemanticsData()
              .flagsCollection
              .isButton,
          isTrue,
        );
        buttonSemantics.dispose();
        Focus.of(tester.element(find.text('VPN Gate'))).requestFocus();
        await tester.pump();
        await tester.sendKeyEvent(LogicalKeyboardKey.select);
        await tester.pumpAndSettle();
        expect(tester.takeException(), isNull);
        expect(tester.getSize(entry).width, lessThanOrEqualTo(size.width));
      }
      expect(opened, 4);
    },
  );
}
