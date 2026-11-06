import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/models/scenarios.dart';
import 'package:usque/screens/chain_proxy_screen.dart';
import 'package:usque/screens/profiles_screen.dart';
import 'package:usque/services/engine_client.dart';
import 'package:usque/state/app_controller.dart';
import 'package:usque/state/scenario_gate_leases.dart';
import 'package:usque/widgets/direct_dns_editor.dart';
import 'package:usque/widgets/routing_rules_editor.dart';
import 'package:usque/widgets/warp_dns_editor.dart';

import 'scenario_endpoints_test.dart' show enter, tapText;
import 'scenarios_test.dart' show SceneEngine;
import 'ui_workflow_test.dart' show pumpWorkflow;
import 'vpngate_test.dart' show server;

const _proxy = ChainProfileSummary(
  id: 'proxy-a',
  revision: 'r1',
  editRevision: 'e1',
  name: 'Scene proxy',
  source: ChainSource.httpProxy,
  protocol: 'http_connect',
  host: 'proxy.example',
  port: 8080,
);

class _ManagementEngine extends SceneEngine
    implements ChainProfileClient, VpnGateClient {
  int activeWrites = 0, networkWrites = 0;
  final releases = <String>[];
  final nodeRequests = <VpnGateNodeRequest>[];
  VpnGateNodeProgress nodeProgress = const VpnGateNodeProgress();
  bool failOwnerRead = false;
  bool restartBeforeSceneSave = false;

  @override
  Future<NetworkSettingsState> getNetworkSettingsState() => failOwnerRead
      ? Future.error(
          const EngineException('ENGINE_UNAVAILABLE', 'Owner unavailable'),
        )
      : super.getNetworkSettingsState();

  @override
  Future<Map<String, Object?>> scenarioCommand(Map<String, Object?> request) {
    if (request['action'] == 'save' && restartBeforeSceneSave) {
      restartBeforeSceneSave = false;
      settingsEpoch = 'restarted-before-commit';
      sceneCalls.add(Map.of(request));
      return Future.error(
        const EngineException(
          'SCENARIO_DEPENDENCY_UNAVAILABLE',
          'Prepared object was swept',
        ),
      );
    }
    return super.scenarioCommand(request);
  }

  @override
  Future<VpnGateDirectory> listVpnGate({
    String? countryCode,
    bool unknownCountry = false,
    int offset = 0,
    int limit = 50,
    bool favoritesOnly = false,
    bool statusOnly = false,
  }) async => VpnGateDirectory(
    servers: [server],
    savedServer: server,
    total: 1,
    fetchedAt: DateTime.now(),
    refreshStage: 'complete',
    nodeProgress: nodeProgress,
  );
  @override
  Future<void> refreshVpnGate({bool cancel = false}) async {}
  @override
  Future<void> vpnGateNode(VpnGateNodeRequest request) async {
    nodeRequests.add(request);
    if (request.expectedSourceEpoch.isNotEmpty &&
        request.expectedSourceEpoch != settingsEpoch) {
      throw const EngineException('VPN_GATE_OWNER_STALE', 'Stale owner');
    }
    if (request.action == 'release') releases.add(request.serverId);
    if (request.action == 'prepare') {
      nodeProgress = VpnGateNodeProgress(
        operationId: request.operationId,
        serverId: request.serverId,
        configSha256: request.configSha256,
        stage: 'complete',
      );
    }
  }

  @override
  Future<void> setActiveProfile(String id) async {
    activeWrites++;
    await super.setActiveProfile(id);
  }

  @override
  Future<NetworkSettingsState> saveNetworkSettings(
    String operation,
    String account,
    UsqueProfile profile,
    List<String> fields,
  ) {
    networkWrites++;
    return super.saveNetworkSettings(operation, account, profile, fields);
  }

  @override
  Future<ChainProfileResult> chainProfile(Map<String, Object?> request) async =>
      const ChainProfileResult(profiles: [_proxy]);
  @override
  Future<List<ChainConfigurationFile>> pickChainConfigurations() async => [];
  @override
  Future<EngineCapabilities?> getCapabilities() async =>
      const EngineCapabilities(
        connectionScenarios: true,
        networkSettingsApplication: true,
        automaticEndpoints: true,
        endpointPresets: true,
        zeroTrustEndpointEditing: true,
        chainProfileImport: true,
        chainHttpProxy: true,
        chainWireguard: true,
        chainOpenvpnUdp: true,
        vpnGateTcp: true,
        vpnGatePoolFavorites: true,
        vpnGateEpochRequests: true,
        tlsFingerprints: TlsFingerprint.values,
      );
}

Future<AppController> _openEditor(
  WidgetTester tester,
  _ManagementEngine engine,
) async {
  final app = await pumpWorkflow(tester, engine, section: AppSection.profiles);
  app.engineCapabilities = await engine.getCapabilities();
  await app.scenarios.refresh();
  await tester.pumpAndSettle();
  await tester.tap(find.byKey(const ValueKey('scenario-create')));
  await tester.pumpAndSettle();
  await enter(tester, 'scenario-name', 'Draft scene');
  return app;
}

Future<void> _returnGateDraft(
  WidgetTester tester,
  _ManagementEngine engine,
) async {
  await tester.tap(find.byKey(const ValueKey('scenario-chain-toggle')));
  await tester.pumpAndSettle();
  await tester.tap(find.byKey(const ValueKey('scenario-chain-settings')));
  await tester.pumpAndSettle();
  final child = tester.widget<ChainProxyScreen>(find.byType(ChainProxyScreen));
  const gate = VpnGateSettings(
    enabled: true,
    serverId: 'v1:node',
    configSha256: 'config-one',
  );
  child.onDraftGateLease!(ScenarioGateLease(gate, engine.settingsEpoch));
  child.onDraftSelected!(
    child.draftProfile!.copyWith(
      vpnGate: gate,
      chainExit: const ChainExitSettings(
        enabled: true,
        source: ChainSource.vpnGate,
      ),
    ),
  );
  await tester.pumpAndSettle();
}

void main() {
  for (final reopen in [false, true]) {
    testWidgets(
      'native restart re-prepares the scene Gate draft for ${reopen ? "same-node child selection" : "direct parent save"}',
      (tester) async {
        final engine = _ManagementEngine();
        final app = await _openEditor(tester, engine);
        await _returnGateDraft(tester, engine);
        engine.settingsEpoch = 'owner-b';
        await app.networkSettings.refresh();
        await tester.pumpAndSettle();
        if (reopen) {
          await tester.tap(
            find.byKey(const ValueKey('scenario-chain-settings')),
          );
          await tester.pumpAndSettle();
          await tapText(tester, app.strings.get('scenario_choose_exit'));
        }
        await tapText(tester, app.strings.get('scenario_save'));
        expect(
          engine.sceneCalls.where((r) => r['action'] == 'save'),
          hasLength(1),
        );
        final preparations = engine.nodeRequests
            .where((r) => r.action == 'prepare')
            .toList();
        expect(preparations, hasLength(1));
        expect(preparations.single.expectedSourceEpoch, 'owner-b');
        expect(
          engine.nodeRequests
              .where((r) => r.action == 'release')
              .map((r) => r.expectedSourceEpoch),
          ['owner-b'],
        );
        expect(engine.networkWrites, 0);
        expect(engine.activeWrites, 0);
      },
    );
  }

  testWidgets(
    'unconfirmed owner read keeps the scene draft and retries preparation before save',
    (tester) async {
      final engine = _ManagementEngine();
      final app = await _openEditor(tester, engine);
      await _returnGateDraft(tester, engine);
      engine.failOwnerRead = true;
      await tapText(tester, app.strings.get('scenario_save'));
      expect(engine.sceneCalls.where((r) => r['action'] == 'save'), isEmpty);
      expect(engine.nodeRequests, isEmpty);
      expect(
        tester
            .widget<TextFormField>(find.byKey(const ValueKey('scenario-name')))
            .controller!
            .text,
        'Draft scene',
      );
      engine.failOwnerRead = false;
      engine.settingsEpoch = 'owner-b';
      await tapText(tester, app.strings.get('scenario_save'));
      expect(
        engine.sceneCalls.where((r) => r['action'] == 'save'),
        hasLength(1),
      );
      expect(engine.nodeRequests.first.action, 'prepare');
      expect(engine.nodeRequests.first.expectedSourceEpoch, 'owner-b');
    },
  );

  testWidgets(
    'restart between preparation and scene commit preserves draft and does not replay save',
    (tester) async {
      final engine = _ManagementEngine();
      final app = await _openEditor(tester, engine);
      await _returnGateDraft(tester, engine);
      engine.restartBeforeSceneSave = true;
      await tapText(tester, app.strings.get('scenario_save'));
      expect(
        engine.sceneCalls.where((r) => r['action'] == 'save'),
        hasLength(1),
      );
      expect(engine.releases, isEmpty);
      expect(
        tester
            .widget<TextFormField>(find.byKey(const ValueKey('scenario-name')))
            .controller!
            .text,
        'Draft scene',
      );
      await tapText(tester, app.strings.get('scenario_save'));
      expect(
        engine.sceneCalls.where((r) => r['action'] == 'save'),
        hasLength(2),
      );
      expect(engine.nodeRequests.first.action, 'prepare');
      expect(
        engine.nodeRequests.first.expectedSourceEpoch,
        'restarted-before-commit',
      );
      expect(
        engine.nodeRequests
            .where((r) => r.action == 'release')
            .map((r) => r.expectedSourceEpoch),
        ['restarted-before-commit'],
      );
    },
  );
  test('scene projection leaves current device settings intact', () {
    final scene = ScenarioSettings.fromProfile(UsqueProfile.defaultProfile());
    expect(scene.toMap().keys.toSet(), ScenarioSettings.managedFields);
    final current = UsqueProfile.defaultProfile().copyWith(
      frontends: const FrontendSettings(
        tunnel: false,
        socks5: true,
        http: false,
      ),
      allowLan: false,
      geoDirectCountries: ['DE'],
      dnsIpv4: '9.9.9.9',
      dnsMode: DnsMode.localConfigured,
      directDns: const DirectDnsSettings(
        mode: DirectDnsMode.dot,
        serverName: 'dns.example',
        port: 853,
      ),
      warpDns: const WarpDnsSettings(
        mode: WarpDnsMode.dot,
        serverName: 'warp.example',
        port: 853,
      ),
      proxy: const ProxySettings(
        dnsMode: ProxyDnsMode.localConfigured,
        dnsIpv4: '149.112.112.112',
      ),
    );
    final target = scene.toProfile(current);
    expect(target.frontends, same(current.frontends));
    expect(target.allowLan, current.allowLan);
    expect(target.routing, current.routing);
    expect(target.geoDirectCountries, current.geoDirectCountries);
    expect(target.directDns, current.directDns);
    expect(target.warpDns, current.warpDns);
    expect(target.dnsMode, current.dnsMode);
    expect(target.dnsIpv4, current.dnsIpv4);
    expect(target.proxy, same(current.proxy));
  });

  testWidgets(
    'scene form omits outputs routing and DNS and binds accounts without switching',
    (tester) async {
      final engine = _ManagementEngine();
      final app = await _openEditor(tester, engine);
      final original = app.activeProfileId;
      final b = app.activeProfile.copyWith(
        id: 'account-b',
        name: 'Work account',
      );
      app.profiles = [app.activeProfile, b];
      engine.storedProfiles = app.profiles;
      expect(find.byType(WarpDnsEditor), findsNothing);
      expect(find.byType(DirectDnsEditor), findsNothing);
      expect(find.byType(RoutingRulesEditor), findsNothing);
      expect(find.text(app.strings.get('allow_lan')), findsNothing);
      expect(find.text(app.strings.get('routing_ads')), findsNothing);
      expect(find.text(app.strings.get('scenario_output')), findsNothing);
      await enter(tester, 'scenario-endpoint-port', '8443');
      await tester.ensureVisible(
        find.byKey(const ValueKey('scenario-account-management')),
      );
      await tester.tap(
        find.byKey(const ValueKey('scenario-account-management')),
      );
      await tester.pumpAndSettle();
      // The picker only picks: adding stays, management does not.
      expect(find.byType(ScenarioAccountPicker), findsOneWidget);
      expect(find.byType(AccountsScreen), findsNothing);
      expect(find.byType(AddAccountButton), findsOneWidget);
      expect(find.text(app.strings.get('set_active')), findsNothing);
      expect(find.byType(PopupMenuButton<String>), findsNothing);
      expect(find.text(app.strings.get('edit_profile')), findsNothing);
      expect(find.text(app.strings.get('delete')), findsNothing);
      await tester.tap(
        find.byKey(const ValueKey('scene-select-account-account-b')),
      );
      await tester.pumpAndSettle();
      expect(
        tester
            .widget<TextFormField>(find.byKey(const ValueKey('scenario-name')))
            .controller!
            .text,
        'Draft scene',
      );
      expect(app.activeProfileId, original);
      expect(
        tester
            .widget<TextFormField>(
              find.byKey(const ValueKey('scenario-endpoint-port')),
            )
            .controller!
            .text,
        '8443',
      );
      expect(engine.activeWrites, 0);
      await tapText(tester, app.strings.get('scenario_save'));
      final save = engine.sceneCalls.singleWhere((r) => r['action'] == 'save');
      expect(save['bound_account_id'], 'account-b');
      expect(
        (save['settings'] as Map).keys.toSet(),
        ScenarioSettings.managedFields,
      );
      expect(engine.networkWrites, 0);
    },
  );

  testWidgets(
    'nested account creation returns a bindable account and retains the scene draft',
    (tester) async {
      final engine = _ManagementEngine();
      final app = await _openEditor(tester, engine);
      final original = app.activeProfileId;
      await tester.tap(
        find.byKey(const ValueKey('scenario-account-management')),
      );
      await tester.pumpAndSettle();
      await tapText(tester, app.strings.get('new_profile'));
      final name = find.byWidgetPredicate(
        (w) =>
            w is TextField &&
            w.decoration?.labelText == app.strings.get('profile_name'),
      );
      await tester.enterText(name, 'New scene account');
      await tapText(tester, app.strings.get('continue'));
      await tapText(tester, app.strings.get('create'));
      final created = app.profiles.singleWhere(
        (p) => p.name == 'New scene account',
      );
      expect(app.activeProfileId, original);
      expect(engine.activeWrites, 0);
      await tester.tap(
        find.byKey(ValueKey('scene-select-account-${created.id}')),
      );
      await tester.pumpAndSettle();
      expect(
        tester
            .widget<TextFormField>(find.byKey(const ValueKey('scenario-name')))
            .controller!
            .text,
        'Draft scene',
      );
      await tapText(tester, app.strings.get('scenario_save'));
      expect(
        engine.sceneCalls.singleWhere(
          (r) => r['action'] == 'save',
        )['bound_account_id'],
        created.id,
      );
      expect(engine.networkWrites, 0);
    },
  );

  testWidgets(
    'chain toggle opens the full nested editor and returned selection only changes the scene',
    (tester) async {
      final engine = _ManagementEngine();
      final app = await _openEditor(tester, engine);
      final before = app.activeProfile.toMap();
      expect(
        find.byKey(const ValueKey('scenario-chain-settings')),
        findsNothing,
      );
      await tester.tap(find.byKey(const ValueKey('scenario-chain-toggle')));
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(const ValueKey('scenario-chain-settings')));
      await tester.pumpAndSettle();
      final page = tester.widget<ChainProxyScreen>(
        find.byType(ChainProxyScreen),
      );
      expect(page.draftProfile, isNotNull);
      expect(page.onDraftSelected, isNotNull);
      page.onDraftGateLease?.call(null);
      page.onDraftSelected!(
        page.draftProfile!.copyWith(
          chainExit: const ChainExitSettings(
            enabled: true,
            source: ChainSource.httpProxy,
            profileId: 'proxy-a',
            revision: 'r1',
          ),
        ),
      );
      await tester.pumpAndSettle();
      expect(find.byType(ChainProxyScreen), findsNothing);
      expect(app.activeProfile.toMap(), before);
      await tapText(tester, app.strings.get('scenario_save'));
      final saved =
          engine.sceneCalls.singleWhere(
                (r) => r['action'] == 'save',
              )['settings']
              as Map;
      expect((saved['chain_exit'] as Map)['profile_id'], 'proxy-a');
      expect(engine.networkWrites, 0);
      expect(engine.activeWrites, 0);
    },
  );

  testWidgets(
    'returning from account management keeps the parent draft and system back still guards it',
    (tester) async {
      final engine = _ManagementEngine();
      final app = await _openEditor(tester, engine);
      await tester.tap(
        find.byKey(const ValueKey('scenario-account-management')),
      );
      await tester.pumpAndSettle();
      await tapText(tester, app.strings.get('back'));
      await tester.pumpAndSettle();
      expect(
        tester
            .widget<TextFormField>(find.byKey(const ValueKey('scenario-name')))
            .controller!
            .text,
        'Draft scene',
      );
      await tester.binding.handlePopRoute();
      await tester.pumpAndSettle();
      expect(
        find.text(app.strings.get('scenario_discard_title')),
        findsOneWidget,
      );
      await tapText(tester, app.strings.get('keep_editing'));
      expect(engine.sceneCalls.where((r) => r['action'] == 'save'), isEmpty);
    },
  );

  testWidgets(
    'accepted Gate lease survives nested reopen and cancel then releases once after save',
    (tester) async {
      final engine = _ManagementEngine();
      final app = await _openEditor(tester, engine);
      await tester.tap(find.byKey(const ValueKey('scenario-chain-toggle')));
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(const ValueKey('scenario-chain-settings')));
      await tester.pumpAndSettle();
      final child = tester.widget<ChainProxyScreen>(
        find.byType(ChainProxyScreen),
      );
      const gate = VpnGateSettings(
        enabled: true,
        serverId: 'v1:node',
        configSha256: 'config-one',
      );
      child.onDraftGateLease!(ScenarioGateLease(gate, engine.settingsEpoch));
      child.onDraftSelected!(
        child.draftProfile!.copyWith(
          vpnGate: gate,
          chainExit: const ChainExitSettings(
            enabled: true,
            source: ChainSource.vpnGate,
          ),
        ),
      );
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(const ValueKey('scenario-chain-settings')));
      await tester.pumpAndSettle();
      expect(
        tester
            .widget<ChainProxyScreen>(find.byType(ChainProxyScreen))
            .preparedGateDraft
            ?.selection,
        gate,
      );
      await tapText(tester, app.strings.get('back'));
      await tester.pumpAndSettle();
      expect(engine.releases, isEmpty);
      await tapText(tester, app.strings.get('scenario_save'));
      expect(engine.releases, ['v1:node']);
      await tapText(tester, app.strings.get('back'));
      await tester.pumpAndSettle();
      expect(engine.releases, ['v1:node']);
      expect(engine.networkWrites, 0);
    },
  );
}
