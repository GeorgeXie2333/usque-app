import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:shared_preferences/shared_preferences.dart';
import 'package:usque/core/chain_strings.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/screens/chain_proxy_screen.dart';
import 'package:usque/state/app_controller.dart';
import 'package:usque/state/scenario_gate_leases.dart';
import 'package:usque/widgets/chain_current_connection.dart';
import 'package:usque/widgets/proton_vpn_panel.dart';
import 'package:usque/widgets/save_changes_bar.dart';
import 'package:usque/widgets/vpn_gate_server_row.dart';

import 'chain_proxy_test.dart' show ChainEngine, imported, chooseSource;
import 'proton_vpn_test.dart' show ProtonEngine;
import 'ui_workflow_test.dart' show workflowHost;
import 'vpngate_test.dart' show server;

const _proxy = ChainProfileSummary(
  id: 'scene-proxy',
  revision: 'r1',
  editRevision: 'e1',
  name: 'Scene proxy',
  protocol: 'http_connect',
  source: ChainSource.httpProxy,
  host: 'proxy.example',
  port: 8080,
);

Future<AppController> _host(
  WidgetTester tester,
  ChainEngine engine, {
  required UsqueProfile Function(UsqueProfile) draft,
  required ValueChanged<UsqueProfile> selected,
  VpnGateSettings? preparedGateDraft,
  ValueChanged<VpnGateSettings?>? onDraftGateLease,
}) async {
  SharedPreferences.setMockInitialValues({});
  tester.view.devicePixelRatio = 1;
  tester.view.physicalSize = const Size(980, 1100);
  addTearDown(tester.view.resetDevicePixelRatio);
  addTearDown(tester.view.resetPhysicalSize);
  engine.legacyProfilesImported = true;
  final app = AppController(engine);
  await app.initialize();
  app.localePreference = LocalePreference.english;
  addTearDown(app.dispose);
  await tester.pumpWidget(
    workflowHost(
      app,
      home: Builder(
        builder: (context) => Scaffold(
          body: TextButton(
            onPressed: () => Navigator.of(context).push<void>(
              MaterialPageRoute(
                builder: (context) => ChainProxyScreen(
                  controller: app,
                  draftProfile: draft(app.activeProfile),
                  preparedGateDraft: preparedGateDraft == null
                      ? null
                      : ScenarioGateLease(
                          preparedGateDraft,
                          engine.settingsEpoch,
                        ),
                  onDraftGateLease: (lease) =>
                      onDraftGateLease?.call(lease?.selection),
                  onDraftSelected: (profile) {
                    selected(profile);
                    Navigator.of(context).pop();
                  },
                ),
              ),
            ),
            child: const Text('Edit scene chain'),
          ),
        ),
      ),
    ),
  );
  await tester.tap(find.text('Edit scene chain'));
  await tester.pumpAndSettle();
  return app;
}

Future<void> _accept(WidgetTester tester) async {
  final button = find.descendant(
    of: find.byType(SaveChangesBar),
    matching: find.byType(FilledButton),
  );
  await tester.ensureVisible(button);
  await tester.tap(button);
  await tester.pumpAndSettle();
}

void _unchanged(ChainEngine engine, AppController app, UsqueProfile before) {
  expect(engine.saves, 0);
  expect(engine.calls, isNot(contains('connect')));
  expect(engine.calls, isNot(contains('disconnect')));
  expect(app.activeProfile.toMap(), before.toMap());
}

void main() {
  testWidgets('scene can create and select a proxy without applying settings', (
    tester,
  ) async {
    final engine = ChainEngine()..previewProfile = _proxy;
    UsqueProfile? result;
    final app = await _host(
      tester,
      engine,
      draft: (profile) => profile.copyWith(
        chainExit: const ChainExitSettings(source: ChainSource.httpProxy),
      ),
      selected: (profile) => result = profile,
    );
    final before = app.activeProfile;
    expect(find.byType(ChainCurrentConnection), findsNothing);
    await tester.tap(find.text(app.strings.chain('add_proxy')));
    await tester.pumpAndSettle();
    await tester.enterText(
      find.byKey(const ValueKey('chain-proxy-host')),
      'proxy.example',
    );
    await tester.enterText(
      find.byKey(const ValueKey('chain-proxy-name')),
      'Scene proxy',
    );
    await tester.tap(find.byKey(const ValueKey('chain-proxy-save-select')));
    await tester.pumpAndSettle();
    expect(engine.actions, contains('import'));
    expect(engine.library.single.id, _proxy.id);
    expect(result, isNull);
    _unchanged(engine, app, before);

    // Browsing another source preserves the newly created local selection.
    await chooseSource(tester, ChainSource.wireguardCustom);
    await chooseSource(tester, ChainSource.httpProxy);
    expect(
      tester
          .widget<RadioGroup<String>>(find.byType(RadioGroup<String>))
          .groupValue,
      _proxy.id,
    );
    await _accept(tester);
    expect(result!.chainExit!.profileId, _proxy.id);
    expect(result!.chainExit!.enabled, isTrue);
    expect(result!.dataPlane, before.dataPlane);
    expect(find.byType(ChainProxyScreen), findsNothing);
    expect(find.text(app.strings.get('scenario_discard_title')), findsNothing);
    _unchanged(engine, app, before);
  });

  testWidgets('scene chain can return an unchanged valid selection', (
    tester,
  ) async {
    final engine = ChainEngine()..library = [_proxy];
    final leases = <VpnGateSettings?>[];
    UsqueProfile? result;
    final app = await _host(
      tester,
      engine,
      onDraftGateLease: leases.add,
      draft: (profile) => profile.copyWith(
        chainExit: const ChainExitSettings(
          source: ChainSource.httpProxy,
          enabled: true,
          profileId: 'scene-proxy',
          revision: 'r1',
        ),
        vpnGate: const VpnGateSettings(
          serverId: 'dormant',
          configSha256: 'old',
        ),
      ),
      selected: (profile) => result = profile,
    );
    final before = app.activeProfile;
    await _accept(tester);
    expect(result!.chainExit!.profileId, _proxy.id);
    expect(result!.vpnGate.hasSelection, isTrue);
    expect(leases, [null]);
    _unchanged(engine, app, before);
  });

  testWidgets('discarding a scene chain selection keeps the running settings', (
    tester,
  ) async {
    final engine = ChainEngine()..library = [_proxy];
    UsqueProfile? result;
    final app = await _host(
      tester,
      engine,
      draft: (profile) => profile.copyWith(
        chainExit: const ChainExitSettings(source: ChainSource.httpProxy),
      ),
      selected: (profile) => result = profile,
    );
    final before = app.activeProfile;
    await tester.tap(find.text(_proxy.name));
    await tester.pumpAndSettle();
    await tester.binding.handlePopRoute();
    await tester.pumpAndSettle();
    expect(
      find.text(app.strings.get('scenario_discard_title')),
      findsOneWidget,
    );
    await tester.tap(find.text(app.strings.get('discard_changes')));
    await tester.pumpAndSettle();
    expect(find.byType(ChainProxyScreen), findsNothing);
    expect(result, isNull);
    expect(engine.library, [_proxy]);
    _unchanged(engine, app, before);
  });

  testWidgets('scene UDP exit cannot change global L4 mode', (tester) async {
    final engine = ChainEngine()..library = [imported];
    UsqueProfile? result;
    final app = await _host(
      tester,
      engine,
      draft: (profile) => profile.copyWith(
        dataPlane: DataPlaneMode.l4Proxy,
        chainExit: const ChainExitSettings(source: ChainSource.wireguardCustom),
      ),
      selected: (profile) => result = profile,
    );
    final before = app.activeProfile;
    await tester.tap(find.text(imported.name));
    await tester.pumpAndSettle();
    final bar = tester.widget<SaveChangesBar>(find.byType(SaveChangesBar));
    expect(bar.onSave, isNull);
    expect(bar.validationError, app.strings.chain('l4'));
    expect(find.text(app.strings.chain('switch_mode')), findsNothing);
    expect(result, isNull);
    _unchanged(engine, app, before);
  });

  testWidgets('scene VPN Gate prepares and transfers its draft reference', (
    tester,
  ) async {
    final engine = ChainEngine();
    final leases = <VpnGateSettings?>[];
    UsqueProfile? result;
    final app = await _host(
      tester,
      engine,
      onDraftGateLease: leases.add,
      draft: (profile) => profile.copyWith(
        chainExit: const ChainExitSettings(source: ChainSource.vpnGate),
      ),
      selected: (profile) => result = profile,
    );
    final before = app.activeProfile;
    final row = tester.widget<VpnGateServerRow>(
      find.byType(VpnGateServerRow).first,
    );
    row.onSelect!();
    await tester.pumpAndSettle();
    await _accept(tester);
    expect(result!.chainSource, ChainSource.vpnGate);
    expect(result!.vpnGate.serverId, server.id);
    expect(result!.vpnGate.configSha256, server.configSha256);
    expect(leases.single!.serverId, server.id);
    expect(leases.single!.configSha256, server.configSha256);
    expect(engine.nodeRequests.map((request) => request.action), ['prepare']);
    expect(find.byType(ChainProxyScreen), findsNothing);
    _unchanged(engine, app, before);
  });

  testWidgets('scene Proton removal cannot clear the globally saved exit', (
    tester,
  ) async {
    final engine = ProtonEngine();
    engine.storedProfiles[0] = engine.storedProfiles[0].copyWith(
      chainExit: const ChainExitSettings(
        source: ChainSource.protonVpn,
        profileId: 'proton-profile',
        revision: 'r1',
      ),
    );
    final app = await _host(
      tester,
      engine,
      draft: (profile) => profile.copyWith(
        chainExit: const ChainExitSettings(source: ChainSource.protonVpn),
      ),
      selected: (_) => fail('Removal cannot accept a scene draft.'),
    );
    final before = app.activeProfile;
    final panel = tester.widget<ProtonVpnPanel>(find.byType(ProtonVpnPanel));
    expect(await panel.beforeRemove!(['proton-profile']), isFalse);
    await tester.pumpAndSettle();
    _unchanged(engine, app, before);
    expect(
      engine.requests.where((request) => request['action'] == 'logout'),
      isEmpty,
    );
  });

  testWidgets('reopened scene borrows its prepared Gate selection', (
    tester,
  ) async {
    final engine = ChainEngine();
    final leases = <VpnGateSettings?>[];
    final selection = VpnGateSettings(enabled: true).copyWith(server: server);
    engine.favorites[server.id] = VpnGateServer(
      id: server.id,
      ip: server.ip,
      hostname: server.hostname,
      configSha256: server.configSha256,
      favorite: VpnGateFavoriteMetadata(
        configSha256: server.configSha256,
        savedAt: DateTime(2026),
      ),
    );
    UsqueProfile? result;
    final app = await _host(
      tester,
      engine,
      preparedGateDraft: selection,
      onDraftGateLease: leases.add,
      draft: (profile) => profile.copyWith(
        chainExit: const ChainExitSettings(
          source: ChainSource.vpnGate,
          enabled: true,
        ),
        vpnGate: selection,
      ),
      selected: (profile) => result = profile,
    );
    final before = app.activeProfile;
    // A library edit must not prepare a duplicate reference owned by the
    // subpage, whose disposal would release the parent's unsaved selection.
    tester
        .widget<VpnGateServerRow>(find.byType(VpnGateServerRow).first)
        .onFavorite!();
    await tester.pumpAndSettle();
    await _accept(tester);
    expect(result!.vpnGate, selection);
    expect(leases, [selection]);
    expect(engine.nodeRequests.map((request) => request.action), [
      'remove_favorite',
    ]);
    _unchanged(engine, app, before);
  });

  testWidgets(
    'custom choice retains the parent lease for its dormant Gate settings',
    (tester) async {
      final engine = ChainEngine()..library = [_proxy];
      final selection = const VpnGateSettings(
        enabled: true,
      ).copyWith(server: server);
      final leases = <VpnGateSettings?>[];
      UsqueProfile? result;
      final app = await _host(
        tester,
        engine,
        preparedGateDraft: selection,
        onDraftGateLease: leases.add,
        draft: (profile) => profile.copyWith(
          chainExit: const ChainExitSettings(
            source: ChainSource.vpnGate,
            enabled: true,
          ),
          vpnGate: selection,
        ),
        selected: (profile) => result = profile,
      );
      final before = app.activeProfile;
      await chooseSource(tester, ChainSource.httpProxy);
      await tester.tap(find.text(_proxy.name));
      await tester.pumpAndSettle();
      await _accept(tester);
      expect(result!.chainSource, ChainSource.httpProxy);
      expect(result!.vpnGate.enabled, isFalse);
      expect(result!.vpnGate.serverId, selection.serverId);
      expect(leases, [selection]);
      expect(engine.nodeRequests, isEmpty);
      _unchanged(engine, app, before);
    },
  );

  for (final chosen in [ChainSource.vpnGate, ChainSource.httpProxy, null]) {
    testWidgets(
      'prepared Gate draft survives source switches before accepting $chosen',
      (tester) async {
        final engine = ChainEngine()..library = [_proxy];
        final selection = const VpnGateSettings(
          enabled: true,
        ).copyWith(server: server);
        engine.favorites[server.id] = VpnGateServer(
          id: server.id,
          ip: server.ip,
          hostname: server.hostname,
          configSha256: server.configSha256,
          favorite: VpnGateFavoriteMetadata(
            configSha256: server.configSha256,
            savedAt: DateTime(2026),
          ),
        );
        final leases = <VpnGateSettings?>[];
        UsqueProfile? result;
        final app = await _host(
          tester,
          engine,
          onDraftGateLease: leases.add,
          draft: (profile) => profile.copyWith(
            chainExit: const ChainExitSettings(
              source: ChainSource.vpnGate,
              enabled: true,
            ),
            vpnGate: selection,
          ),
          selected: (profile) => result = profile,
        );
        final before = app.activeProfile;
        tester
            .widget<VpnGateServerRow>(find.byType(VpnGateServerRow).first)
            .onFavorite!();
        await tester.pumpAndSettle();
        expect(engine.nodeRequests.map((request) => request.action), [
          'prepare',
          'remove_favorite',
        ]);
        await chooseSource(tester, ChainSource.httpProxy);
        expect(
          engine.nodeRequests.where((request) => request.action == 'release'),
          isEmpty,
        );
        if (chosen == null) {
          await tester.binding.handlePopRoute();
          await tester.pumpAndSettle();
          expect(find.byType(ChainProxyScreen), findsNothing);
          expect(result, isNull);
          expect(leases, isEmpty);
          expect(engine.nodeRequests.map((request) => request.action), [
            'prepare',
            'remove_favorite',
            'release',
          ]);
          _unchanged(engine, app, before);
          return;
        }
        if (chosen == ChainSource.vpnGate) {
          await chooseSource(tester, ChainSource.vpnGate);
        } else {
          await tester.tap(find.text(_proxy.name));
          await tester.pumpAndSettle();
        }
        await _accept(tester);
        expect(result!.chainSource, chosen);
        expect(leases.single!.serverId, selection.serverId);
        expect(leases.single!.configSha256, selection.configSha256);
        expect(engine.nodeRequests.map((request) => request.action), [
          'prepare',
          'remove_favorite',
        ]);
        _unchanged(engine, app, before);
      },
    );
  }
}
