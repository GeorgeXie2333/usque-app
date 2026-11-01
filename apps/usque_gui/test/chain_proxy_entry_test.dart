import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:shared_preferences/shared_preferences.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/screens/proxy_screen.dart';
import 'package:usque/state/app_controller.dart';
import 'package:usque/widgets/chain_proxy_entry.dart';

import 'chain_proxy_test.dart' show ChainEngine, imported;
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
    ..engineCapabilities = const EngineCapabilities(
      tlsFingerprints: TlsFingerprint.values,
      vpnGateTcp: true,
      automaticEndpoints: true,
    );
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

Widget _entry(AppController app, {VoidCallback? onOpen}) => workflowHost(
  app,
  home: Scaffold(
    body: ChainProxyEntry(controller: app, onOpen: onOpen ?? () {}),
  ),
);

void main() {
  final entry = find.byKey(const ValueKey('proxy-chain-proxy-entry'));
  final section = find.byKey(const ValueKey('proxy-chain-proxy-section'));
  final selection = find.byKey(const ValueKey('proxy-chain-selection'));

  String selectionText(WidgetTester tester) => tester
      .widget<Text>(find.descendant(of: selection, matching: find.byType(Text)))
      .textSpan!
      .toPlainText();

  testWidgets(
    'live Gate updates preserve listener drafts and use chain wording',
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
      expect(
        find.descendant(
          of: section,
          matching: find.text('Enabled · not connected'),
        ),
        findsOneWidget,
      );
      expect(selectionText(tester), contains(server.ip));
      expect(selectionText(tester), startsWith('VPN Gate · '));
      final port = find.byWidgetPredicate(
        (widget) =>
            widget is TextFormField && widget.controller?.text == '1080',
      );
      expect(port, findsOneWidget);
      await tester.enterText(port, '9091');
      for (final (phase, stage, label) in [
        (ConnectionPhase.preparing, 'connected', 'Connected'),
        (ConnectionPhase.connected, 'connected', 'Connected'),
        (ConnectionPhase.error, 'error', 'Connection failed'),
        (ConnectionPhase.disconnecting, 'error', 'Disconnecting'),
      ]) {
        engine.current = EngineSnapshot(
          phase: phase,
          vpnGate: VpnGateStatus(stage: stage, server: current),
        );
        await app.refreshSnapshot();
        await tester.pumpAndSettle();
        expect(
          find.descendant(of: section, matching: find.text(label)),
          findsOneWidget,
        );
        expect(selectionText(tester), contains(current.ip));
        expect(selectionText(tester), isNot(contains(server.ip)));
        // The path is the page's own subject; only Home names WARP → exit.
        expect(find.textContaining('WARP →'), findsNothing);
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
      await tester.pumpWidget(_entry(app));
      await tester.pumpAndSettle();
      expect(find.text('Connected'), findsOneWidget);
      engine.current = const EngineSnapshot();
      await app.refreshSnapshot();
      await tester.pumpAndSettle();
      expect(selectionText(tester), contains(server.ip));
      expect(find.text('Enabled · not connected'), findsOneWidget);
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
    await tester.pumpWidget(_entry(app));
    await tester.pumpAndSettle();
    expect(selectionText(tester), contains(server.id));
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
    expect(selectionText(tester), isNot(contains(server.ip)));
    expect(selectionText(tester), contains(current.id));
    second.complete(const VpnGateDirectory(savedServer: current));
    await tester.pumpAndSettle();
    expect(selectionText(tester), contains(current.ip));
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
    expect(selectionText(tester), contains(server.id));
    expect(selectionText(tester), isNot(contains(current.ip)));
    expect(find.text('Enabled · not connected'), findsOneWidget);
    expect(tester.takeException(), isNull);
    expect(engine.reads, 3);
  });

  testWidgets('saved reads wait until the Proxy section is visible', (
    tester,
  ) async {
    final engine = _EntryEngine();
    final app = _controllerFor(engine)..section = AppSection.home;
    addTearDown(app.dispose);
    await tester.pumpWidget(_entry(app));
    await tester.pumpAndSettle();
    expect(engine.reads, 0);
    app.selectSection(AppSection.proxy);
    await tester.pumpAndSettle();
    expect(engine.reads, 1);
    expect(selectionText(tester), contains(server.ip));
  });

  testWidgets('saved configurations show their name from the local library', (
    tester,
  ) async {
    SharedPreferences.setMockInitialValues({});
    final engine = ChainEngine()..library = [imported];
    engine.legacyProfilesImported = true;
    engine.storedProfiles[0] = engine.storedProfiles[0].copyWith(
      chainExit: ChainExitSettings(
        enabled: true,
        source: imported.source,
        profileId: imported.id,
        revision: imported.revision,
      ),
    );
    final app = AppController(engine);
    await app.initialize();
    app
      ..localePreference = LocalePreference.english
      ..section = AppSection.proxy;
    addTearDown(app.dispose);
    await tester.pumpWidget(_entry(app));
    await tester.pumpAndSettle();
    expect(selectionText(tester), 'WireGuard · Office tunnel');
    expect(find.text('Enabled · not connected'), findsOneWidget);
    expect(find.text('Choose an exit reached through WARP.'), findsNothing);
    expect(engine.actions.where((action) => action == 'list'), hasLength(1));
  });

  testWidgets(
    'disabled chain remains reachable without directory reads or writes',
    (tester) async {
      final engine = _EntryEngine();
      final app = _controllerFor(engine, selected: false);
      addTearDown(app.dispose);
      var opened = 0;
      await tester.pumpWidget(_entry(app, onOpen: () => opened++));
      await tester.pumpAndSettle();
      expect(find.text('Not enabled'), findsOneWidget);
      expect(find.text('Choose an exit reached through WARP.'), findsOneWidget);
      expect(selection, findsNothing);
      await tester.tap(entry);
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
                child: ChainProxyEntry(controller: app, onOpen: () => opened++),
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
        Focus.of(
          tester.element(
            find.descendant(of: entry, matching: find.byType(Text)).first,
          ),
        ).requestFocus();
        await tester.pump();
        await tester.sendKeyEvent(LogicalKeyboardKey.select);
        await tester.pumpAndSettle();
        expect(tester.takeException(), isNull);
        expect(tester.getSize(section).width, lessThanOrEqualTo(size.width));
      }
      expect(opened, 4);
    },
  );
}
