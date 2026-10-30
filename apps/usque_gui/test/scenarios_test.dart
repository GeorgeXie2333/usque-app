import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';
import 'package:shared_preferences/shared_preferences.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/models/scenarios.dart';
import 'package:usque/screens/profiles_hub_screen.dart';
import 'package:usque/screens/scenario_editor_screen.dart';
import 'package:usque/services/engine_client.dart';
import 'package:usque/state/app_controller.dart';
import 'package:usque/state/scenarios_controller.dart';
import 'package:usque/widgets/common.dart';
import 'package:usque/widgets/usque_dialog.dart';

import 'ui_workflow_test.dart' show WorkflowEngine, pumpWorkflow;

Map<String, Object?> fixture({
  String account = 'account-a',
  String? selected,
  bool modified = false,
  bool applied = false,
  String status = 'not_required',
  String? operation,
  bool receipt = false,
  List<Map<String, Object?>>? scenes,
}) => {
  'endpoint_editing': true,
  'scenarios':
      scenes ??
      [
        {
          'id': 'scene-a',
          'name': 'Daily',
          'revision': 'revision-a',
          'bound_account_id': null,
          'settings': ScenarioSettings.fromProfile(
            UsqueProfile.defaultProfile(),
          ).toMap(),
        },
      ],
  'current_settings': ScenarioSettings.fromProfile(
    UsqueProfile.defaultProfile(),
  ).toMap(),
  'current_account_id': account,
  'selection': selected == null
      ? null
      : {
          'scenario_id': selected,
          'scenario_revision': 'revision-a',
          'bound_account_id': null,
          'settings': ScenarioSettings.fromProfile(
            UsqueProfile.defaultProfile(),
          ).toMap(),
        },
  'state': {
    'modified': modified,
    'template_updated': false,
    'bound_account_missing': false,
    'applied_matches': applied,
    'apply_status': status,
  },
  'can_restore': false,
  'source_epoch': 'epoch-a',
  'session_id': null,
  'last_operation_id': ?operation,
  if (receipt) 'persisted_operation_id': operation,
};

class SceneEngine extends WorkflowEngine {
  String? launchTarget;
  @override
  Future<String?> consumeLaunchTarget() async => launchTarget;
  Map<String, Object?> extraState = {};
  List<String> actualPreviewFields = [];
  List<String> previewFields = ['frontends.http'];
  Map<String, Object?>? settingsOverride;
  Map<String, Object?> previewExtra = {};
  final sceneCalls = <Map<String, Object?>>[];
  List<Map<String, Object?>> entries = [
    {
      'id': 'scene-a',
      'name': 'Daily',
      'revision': 'revision-a',
      'bound_account_id': null,
      'settings': ScenarioSettings.fromProfile(
        UsqueProfile.defaultProfile(),
      ).toMap(),
    },
  ];
  String? selected;
  @override
  Future<EngineCapabilities?> getCapabilities() async =>
      const EngineCapabilities(
        networkSettingsApplication: true,
        connectionScenarios: true,
        automaticEndpoints: true,
        endpointPresets: true,
        zeroTrustEndpointEditing: true,
        tlsFingerprints: TlsFingerprint.values,
      );
  @override
  Future<Map<String, Object?>> scenarioCommand(
    Map<String, Object?> command,
  ) async {
    sceneCalls.add(Map.of(command));
    switch (command['action']) {
      case 'save':
        final id = command['id'] as String? ?? 'scene-new';
        entries.removeWhere((e) => e['id'] == id);
        entries.add({
          'id': id,
          'name': command['name'],
          'revision': 'revision-next',
          'bound_account_id': command['bound_account_id'],
          'settings': command['settings'],
        });
      case 'delete':
        entries.removeWhere((e) => e['id'] == command['id']);
      case 'activate':
        selected = command['id'] as String;
    }
    return {
      ...fixture(
        account: storedActiveProfileId,
        scenes: entries,
        selected: selected,
      ),
      if (settingsOverride != null) 'current_settings': settingsOverride,
      ...extraState,
      if (command['action'] == 'save')
        'saved_scenario_id': command['id'] as String? ?? 'scene-new',
      if (!const {
        'list',
        'preview',
        'preview_restore',
      }.contains(command['action']))
        'persisted': true,
      if (command['action'] == 'preview')
        'preview': {
          'scenario_id': command['id'],
          'scenario_revision': command['expected_revision'],
          'account_id': storedActiveProfileId,
          'previous_account_id': storedActiveProfileId,
          'account_changed': false,
          'changed_fields': previewFields,
          'before_settings':
              settingsOverride ??
              ScenarioSettings.fromProfile(
                UsqueProfile.defaultProfile(),
              ).toMap(),
          'target_settings': entries
              .where((entry) => entry['id'] == command['id'])
              .firstOrNull?['settings'],
          'actual_changed_fields': actualPreviewFields,
          'preview_token': 'token-a',
          'restore': false,
          'apply_kind': 'select',
          'needs_reconnect': false,
          ...previewExtra,
        },
    };
  }
}

class DelayedSceneEngine extends SceneEngine {
  final previewGate = Completer<void>();
  @override
  Future<Map<String, Object?>> scenarioCommand(
    Map<String, Object?> command,
  ) async {
    final response = await super.scenarioCommand(command);
    if (command['action'] == 'preview') await previewGate.future;
    return response;
  }
}

class ProtonSceneEngine extends SceneEngine
    implements ChainProfileClient, ProtonVpnClient {
  int serverReads = 0;
  @override
  Future<ChainProfileResult> chainProfile(Map<String, Object?> request) async =>
      const ChainProfileResult(
        profiles: [
          ChainProfileSummary(
            id: 'proton-a',
            revision: 'proton-r1',
            editRevision: 'e1',
            name: 'Saved Proton',
            protocol: 'wireguard',
            host: 'test.invalid',
            port: 443,
            source: ChainSource.protonVpn,
          ),
        ],
      );
  @override
  Future<List<ChainConfigurationFile>> pickChainConfigurations() async => [];
  @override
  Future<ProtonVpnResult> protonVpn(
    Map<String, Object?> request, {
    Uint8List? sensitiveInput,
  }) async {
    expect(request['action'], 'servers');
    serverReads++;
    return ProtonVpnResult(
      metadata: {
        'servers': [
          <Object?, Object?>{'id': 'opaque-node-a', 'name': 'JP#12'},
          <Object?, Object?>{'id': 'opaque-node-b', 'name': 'US#34'},
        ],
        'server_total': 2,
        'server_offset': 0,
        'server_revision': 'directory-r1',
      },
    );
  }
}

Map<String, Object?> protonSceneSettings(String server) =>
    ScenarioSettings.fromProfile(
      UsqueProfile.defaultProfile().copyWith(
        chainExit: ChainExitSettings(
          enabled: true,
          source: ChainSource.protonVpn,
          profileId: 'proton-a',
          revision: 'proton-r1',
          protonServerId: server,
        ),
      ),
    ).toMap();

void main() {
  testWidgets('leaving Profiles does not open a delayed preview over Home', (
    tester,
  ) async {
    final engine = DelayedSceneEngine();
    final app = await pumpWorkflow(
      tester,
      engine,
      section: AppSection.profiles,
    );
    app.engineCapabilities = await engine.getCapabilities();
    await app.scenarios.refresh();
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const ValueKey('scenario-use-scene-a')));
    await tester.pump();
    app.selectSection(AppSection.home);
    await tester.pumpAndSettle();
    engine.previewGate.complete();
    await tester.pumpAndSettle();
    expect(find.byKey(const ValueKey('scenario-confirm-use')), findsNothing);
    expect(app.section, AppSection.home);
    expect(engine.sceneCalls.where((r) => r['action'] == 'activate'), isEmpty);
  });

  Future<void> controlKey(WidgetTester tester, LogicalKeyboardKey key) async {
    await tester.sendKeyDownEvent(LogicalKeyboardKey.controlLeft);
    await tester.sendKeyEvent(key);
    await tester.sendKeyUpEvent(LogicalKeyboardKey.controlLeft);
    await tester.pumpAndSettle();
  }

  testWidgets(
    'Ctrl+2 defaults to scenes while home account and scene entries select their tabs',
    (tester) async {
      final engine = SceneEngine();
      final app = await pumpWorkflow(tester, engine, section: AppSection.home);
      app.engineCapabilities = await engine.getCapabilities();
      await app.scenarios.refresh();
      await tester.pumpAndSettle();
      await controlKey(tester, LogicalKeyboardKey.digit2);
      expect(app.section, AppSection.profiles);
      expect(app.profilesTab, ProfilesTab.scenes);
      expect(find.text('Daily'), findsOneWidget);
      await controlKey(tester, LogicalKeyboardKey.digit1);
      await tester.tap(
        find.descendant(
          of: find.byKey(const ValueKey('home-active-account')),
          matching: find.byType(Text),
        ),
      );
      await tester.pumpAndSettle();
      expect(app.profilesTab, ProfilesTab.accounts);
      expect(find.text(app.strings.get('new_profile')), findsOneWidget);
      await controlKey(tester, LogicalKeyboardKey.digit1);
      final sceneLink = find.byKey(const ValueKey('home-scene-link'));
      await tester.ensureVisible(sceneLink);
      await tester.tap(sceneLink);
      await tester.pumpAndSettle();
      expect(app.profilesTab, ProfilesTab.scenes);
      expect(find.text('Daily'), findsOneWidget);
    },
    variant: TargetPlatformVariant.only(TargetPlatform.windows),
  );

  test(
    'the existing profiles launch target explicitly opens Accounts',
    () async {
      SharedPreferences.setMockInitialValues({
        'onboarding_complete': true,
        'update_checks_enabled': false,
      });
      final app = AppController(SceneEngine()..launchTarget = 'profiles');
      addTearDown(app.dispose);
      await app.initialize();
      expect(app.section, AppSection.profiles);
      expect(app.profilesTab, ProfilesTab.accounts);
    },
  );

  testWidgets(
    'Ctrl+S saves the visible scene draft once without applying live settings',
    (tester) async {
      final engine = SceneEngine();
      final app = await pumpWorkflow(
        tester,
        engine,
        section: AppSection.profiles,
      );
      app.engineCapabilities = await engine.getCapabilities();
      await app.scenarios.refresh();
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(const ValueKey('scenario-create')));
      await tester.pumpAndSettle();
      await tester.enterText(
        find.byKey(const ValueKey('scenario-name')),
        'Work',
      );
      await tester.pumpAndSettle();
      await controlKey(tester, LogicalKeyboardKey.keyS);
      final saves = engine.sceneCalls.where(
        (request) => request['action'] == 'save',
      );
      expect(saves, hasLength(1));
      expect(saves.single['name'], 'Work');
      expect(engine.writes, 0);
      expect(
        engine.sceneCalls.where((request) => request['action'] == 'activate'),
        isEmpty,
      );
      await controlKey(tester, LogicalKeyboardKey.keyS);
      expect(saves, hasLength(1));
    },
    variant: TargetPlatformVariant.only(TargetPlatform.windows),
  );

  testWidgets(
    'desktop rail departure keeps a scene draft until the discard choice',
    (tester) async {
      final engine = SceneEngine();
      final app = await pumpWorkflow(
        tester,
        engine,
        section: AppSection.profiles,
      );
      app.engineCapabilities = await engine.getCapabilities();
      await app.scenarios.refresh();
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(const ValueKey('scenario-create')));
      await tester.pumpAndSettle();
      final name = find.byKey(const ValueKey('scenario-name'));
      await tester.enterText(name, 'Keep this draft');
      await tester.pumpAndSettle();
      final home = find.descendant(
        of: find.byType(NavigationRail),
        matching: find.text(app.strings.get('nav_home')),
      );
      await tester.tap(home);
      await tester.pumpAndSettle();
      expect(app.section, AppSection.profiles);
      expect(
        find.text(app.strings.get('discard_changes_title')),
        findsOneWidget,
      );
      await tester.tap(find.text(app.strings.get('keep_editing')));
      await tester.pumpAndSettle();
      expect(
        tester.widget<TextFormField>(name).controller!.text,
        'Keep this draft',
      );
      await tester.tap(home);
      await tester.pumpAndSettle();
      await tester.tap(find.text(app.strings.get('discard_changes')));
      await tester.pumpAndSettle();
      expect(app.section, AppSection.home);
      expect(
        find.byType(ScenarioEditorScreen, skipOffstage: false),
        findsNothing,
      );
      expect(engine.writes, 0);
      expect(
        engine.sceneCalls.where((request) => request['action'] == 'save'),
        isEmpty,
      );
    },
    variant: TargetPlatformVariant.only(TargetPlatform.windows),
  );

  testWidgets(
    'TV D-pad reaches scene use and confirms its preview before activation',
    (tester) async {
      final engine = SceneEngine();
      final app = await pumpWorkflow(
        tester,
        engine,
        section: AppSection.profiles,
        size: const Size(1280, 720),
      );
      app.engineCapabilities = await engine.getCapabilities();
      await app.scenarios.refresh();
      await tester.pumpAndSettle();
      final editLabel = find.descendant(
        of: find.bySubtype<TextButton>(),
        matching: find.text(app.strings.get('edit')),
      );
      Focus.of(tester.element(editLabel)).requestFocus();
      await tester.pump();
      await tester.sendKeyEvent(LogicalKeyboardKey.arrowRight);
      await tester.pumpAndSettle();
      final useLabel = find.descendant(
        of: find.byKey(const ValueKey('scenario-use-scene-a')),
        matching: find.byType(Text),
      );
      expect(Focus.of(tester.element(useLabel)).hasPrimaryFocus, true);
      await tester.sendKeyEvent(LogicalKeyboardKey.gameButtonA);
      await tester.pumpAndSettle();
      expect(
        find.byKey(const ValueKey('scenario-confirm-use')),
        findsOneWidget,
      );
      expect(
        engine.sceneCalls.where((request) => request['action'] == 'activate'),
        isEmpty,
      );
      final confirmLabel = find.descendant(
        of: find.byKey(const ValueKey('scenario-confirm-use')),
        matching: find.byType(Text),
      );
      Focus.of(tester.element(confirmLabel)).requestFocus();
      await tester.pump();
      await tester.sendKeyEvent(LogicalKeyboardKey.gameButtonA);
      await tester.pumpAndSettle();
      expect(
        engine.sceneCalls.where((request) => request['action'] == 'activate'),
        hasLength(1),
      );
      expect(tester.takeException(), isNull);
    },
    variant: TargetPlatformVariant.only(TargetPlatform.android),
  );

  test(
    'native account deletion refreshes the scene account after metadata readback',
    () async {
      final engine = SceneEngine();
      final original = engine.storedProfiles.single;
      engine.storedProfiles = [
        original,
        original.copyWith(id: 'account-b', name: 'Other account'),
      ];
      final app = AppController(engine)
        ..profiles = engine.storedProfiles
        ..section = AppSection.profiles;
      addTearDown(app.dispose);
      app.engineCapabilities = await engine.getCapabilities();
      await app.scenarios.refresh();
      expect(app.scenarios.catalog!.currentAccountId, original.id);
      final refreshed = Completer<void>();
      void observeAccount() {
        if (!refreshed.isCompleted &&
            app.scenarios.catalog?.currentAccountId == 'account-b') {
          refreshed.complete();
        }
      }

      app.scenarios.addListener(observeAccount);
      addTearDown(() => app.scenarios.removeListener(observeAccount));
      expect(app.deleteProfile(original.id), true);
      await app.flushProfileWrites();
      await refreshed.future.timeout(const Duration(seconds: 5));
      expect(app.scenarios.catalog!.currentAccountId, 'account-b');
    },
  );

  test(
    'Proton scene names resolve locally once per profile revision and never expose IDs',
    () async {
      final engine = ProtonSceneEngine();
      final app = AppController(engine)
        ..localePreference = LocalePreference.english;
      addTearDown(app.dispose);
      final before = protonSceneSettings('opaque-node-a'),
          after = protonSceneSettings('opaque-node-b');
      final first = await scenarioExitNames(app, settings: [before, after]);
      final again = await scenarioExitNames(app, settings: [before, after]);
      expect(engine.serverReads, 1);
      expect(first, again);
      expect(
        scenarioValueLabel(app.strings, before, 'chain_exit', exitNames: first),
        contains('JP#12'),
      );
      expect(
        scenarioValueLabel(app.strings, after, 'chain_exit', exitNames: first),
        contains('US#34'),
      );
      expect(
        scenarioValueLabel(app.strings, after, 'chain_exit'),
        isNot(contains('opaque-node-b')),
      );
    },
  );

  testWidgets(
    'unresolved Proton node changes remain explicit in scene preview',
    (tester) async {
      final engine = SceneEngine()
        ..settingsOverride = protonSceneSettings('opaque-node-a')
        ..previewFields = ['chain_exit'];
      engine.entries.single['settings'] = protonSceneSettings('opaque-node-b');
      final app = await pumpWorkflow(
        tester,
        engine,
        section: AppSection.profiles,
      );
      app.engineCapabilities = await engine.getCapabilities();
      await app.scenarios.refresh();
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(const ValueKey('scenario-use-scene-a')));
      await tester.pumpAndSettle();
      expect(find.text('Server · Modified'), findsOneWidget);
      expect(find.textContaining('opaque-node-'), findsNothing);
    },
  );

  testWidgets(
    'Profiles separates the selected target from the confirmed running scene',
    (tester) async {
      final engine = SceneEngine()..selected = 'scene-a';
      engine.extraState = {
        'session_id': 'session-a',
        'applied_account_id': 'account-b',
        'applied_scenario_id': 'scene-a',
        'applied_scene_confirmed': true,
      };
      final app = await pumpWorkflow(
        tester,
        engine,
        section: AppSection.profiles,
      );
      app.profiles = [
        app.activeProfile,
        app.activeProfile.copyWith(id: 'account-b', name: 'Work account'),
      ];
      app.engineCapabilities = await engine.getCapabilities();
      await app.scenarios.refresh();
      await tester.pumpAndSettle();
      String? running() => tester
          .widget<Text>(find.byKey(const ValueKey('profiles-running')))
          .data;
      expect(find.text('Current connection'), findsOneWidget);
      expect(running(), 'Work account · Daily');
      engine.extraState['applied_scenario_id'] = null;
      await app.scenarios.refresh();
      await tester.pumpAndSettle();
      expect(running(), 'Work account · Custom');
      engine.extraState['applied_scene_confirmed'] = false;
      await app.scenarios.refresh();
      await tester.pumpAndSettle();
      expect(running(), 'Work account · Result not confirmed');
    },
  );

  testWidgets(
    'scene preview includes inherited actual network changes with user labels',
    (tester) async {
      final engine = SceneEngine()
        ..actualPreviewFields = ['mtu', 'endpoint.ipv4'];
      final app = await pumpWorkflow(
        tester,
        engine,
        section: AppSection.profiles,
      );
      app.engineCapabilities = await engine.getCapabilities();
      await app.scenarios.refresh();
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(const ValueKey('scenario-use-scene-a')));
      await tester.pumpAndSettle();
      expect(find.text(app.strings.get('mtu')), findsOneWidget);
      expect(find.text(app.strings.get('endpoint_ipv4')), findsOneWidget);
      expect(find.text('endpoint.ipv4'), findsNothing);
      expect(
        engine.sceneCalls.where((request) => request['action'] == 'activate'),
        isEmpty,
      );
    },
  );
  testWidgets(
    'scene editor saves optional binding and preserves every untouched DNS address',
    (tester) async {
      final settings = ScenarioSettings.fromProfile(
        UsqueProfile.defaultProfile(),
      ).toMap();
      settings['dns_servers'] = <String>[
        '1.1.1.1',
        '9.9.9.9',
        '2606:4700:4700::1111',
      ];
      final engine = SceneEngine()..settingsOverride = settings;
      final app = await pumpWorkflow(
        tester,
        engine,
        section: AppSection.profiles,
      );
      app.profiles = [
        app.activeProfile,
        app.activeProfile.copyWith(id: 'account-b', name: 'Work account'),
      ];
      app.engineCapabilities = await engine.getCapabilities();
      await app.scenarios.refresh();
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(const ValueKey('scenario-create')));
      await tester.pumpAndSettle();
      await tester.enterText(
        find.byKey(const ValueKey('scenario-name')),
        'Work scene',
      );
      await tester.tap(find.byType(DropdownButtonFormField<String>).first);
      await tester.pumpAndSettle();
      await tester.tap(find.text('Work account').last);
      await tester.pumpAndSettle();
      await tester.tap(find.text(app.strings.get('scenario_save')).last);
      await tester.pumpAndSettle();
      final saved = engine.sceneCalls.singleWhere((r) => r['action'] == 'save');
      expect(saved['bound_account_id'], 'account-b');
      expect(
        (saved['settings'] as Map)['dns_servers'],
        settings['dns_servers'],
      );
      expect(
        engine.sceneCalls.where((r) => r['action'] == 'activate'),
        isEmpty,
      );
      expect(engine.writes, 0);
    },
  );

  test('late query cannot replace a newer saved catalogue', () async {
    final oldQuery = Completer<Map<String, Object?>>();
    final controller = ScenariosController((request) async {
      if (request['action'] == 'list') return oldQuery.future;
      return {
        ...fixture(
          scenes: [
            {
              'id': 'new',
              'name': 'New scene',
              'revision': 'r',
              'settings': ScenarioSettings.fromProfile(
                UsqueProfile.defaultProfile(),
              ).toMap(),
            },
          ],
        ),
        'persisted': true,
        'saved_scenario_id': 'new',
      };
    })..setSupported(true);
    addTearDown(controller.dispose);
    final query = controller.refresh();
    final saved = await controller.save(
      name: 'New scene',
      boundAccountId: null,
      settings: ScenarioSettings.fromProfile(UsqueProfile.defaultProfile()),
    );
    expect(saved?.id, 'new');
    oldQuery.complete(fixture());
    await query;
    expect(controller.catalog!.scenarios.single.id, 'new');
  });

  test(
    'a rejected stale preview permits a fresh preview without unknown state',
    () async {
      final controller = ScenariosController((request) async {
        if (request['action'] == 'activate') {
          throw const EngineException('SCENARIO_PREVIEW_STALE', 'Expired');
        }
        return {
          ...fixture(),
          if (request['action'] == 'preview')
            'preview': {
              'preview_token': 'fresh',
              'scenario_id': 'scene-a',
              'scenario_revision': 'revision-a',
            },
        };
      })..setSupported(true);
      addTearDown(controller.dispose);
      final old = ScenarioPreview.fromMap({
        ...fixture(),
        'preview': {
          'preview_token': 'old',
          'scenario_id': 'scene-a',
          'scenario_revision': 'revision-a',
        },
      });
      expect(await controller.activate(old), false);
      expect(controller.unconfirmed, false);
      expect(controller.pendingOperationId, isNull);
      await controller.refresh();
      expect(
        (await controller.preview(controller.catalog!.scenarios.single))?.token,
        'fresh',
      );
    },
  );

  test('save transport failure keeps an unconfirmed draft result', () async {
    final controller = ScenariosController(
      (_) async => throw TimeoutException('No reply'),
    )..setSupported(true);
    addTearDown(controller.dispose);
    expect(
      await controller.save(
        name: 'Work',
        boundAccountId: null,
        settings: ScenarioSettings.fromProfile(UsqueProfile.defaultProfile()),
      ),
      isNull,
    );
    expect(controller.unconfirmed, true);
  });

  test(
    'failed status refresh after durable activation does not lose its ACK',
    () async {
      final controller = ScenariosController(
        (request) async => {
          ...fixture(),
          if (request['action'] == 'activate') 'persisted': true,
        },
      )..setSupported(true);
      controller.afterActivation = () async =>
          throw const EngineException('STATUS_FAILED', 'No status');
      addTearDown(controller.dispose);
      final preview = ScenarioPreview.fromMap({
        ...fixture(),
        'preview': {
          'preview_token': 't',
          'scenario_id': 'scene-a',
          'scenario_revision': 'revision-a',
        },
      });
      expect(await controller.activate(preview), true);
      expect(controller.unconfirmed, false);
    },
  );

  test(
    'scene projection includes endpoints but omits secrets and device protection',
    () {
      final profile = UsqueProfile.defaultProfile().copyWith(
        killSwitch: false,
        endpointIpv4: '192.0.2.1',
        autoConnect: true,
        proxy: const ProxySettings(authUsername: 'private-user'),
      );
      final map = ScenarioSettings.fromProfile(profile).toMap();
      expect(
        map.keys,
        unorderedEquals([
          'endpoint',
          'frontends',
          'transport',
          'ip_policy',
          'allow_lan',
          'routing',
          'geo_direct_countries',
          'direct_dns',
          'warp_dns',
          'dns_mode',
          'dns_servers',
          'proxy_system_proxy',
          'proxy_dns_mode',
          'proxy_dns_servers',
          'vpn_gate',
          'chain_exit',
        ]),
      );
      expect(map['transport'], 'auto');
      expect((map['direct_dns'] as Map)['mode'], 'physical_system');
      expect(map.toString(), isNot(contains('private-user')));
      expect((map['endpoint'] as Map)['ipv4'], '192.0.2.1');
      final roundTrip = ScenarioSettings(map).toProfile(profile);
      expect(roundTrip.directDns.mode, DirectDnsMode.physicalSystem);
      expect(roundTrip.killSwitch, false);
      expect(roundTrip.proxy.authUsername, 'private-user');
    },
  );

  test(
    'template save sends no activation and does not count missing ACK as saved',
    () async {
      final actions = <String>[];
      var acknowledge = false;
      final c = ScenariosController((request) async {
        actions.add(request['action'] as String);
        return {
          ...fixture(
            scenes: [
              {
                'id': 'new',
                'name': 'New',
                'revision': 'r',
                'settings': ScenarioSettings.fromProfile(
                  UsqueProfile.defaultProfile(),
                ).toMap(),
              },
            ],
          ),
          if (acknowledge) 'persisted': true,
        };
      })..setSupported(true);
      addTearDown(c.dispose);
      final result = await c.save(
        name: 'New',
        boundAccountId: null,
        settings: ScenarioSettings.fromProfile(UsqueProfile.defaultProfile()),
      );
      expect(result, isNull);
      expect(c.unconfirmed, true);
      expect(actions, ['save']);
      acknowledge = true;
    },
  );

  test(
    'activation is a single command with binding resolved by native preview',
    () async {
      final calls = <Map<String, Object?>>[];
      final c = ScenariosController((request) async {
        calls.add(request);
        return {
          ...fixture(selected: 'scene-a'),
          if (request['action'] == 'activate') 'persisted': true,
        };
      })..setSupported(true);
      addTearDown(c.dispose);
      final p = ScenarioPreview.fromMap({
        ...fixture(),
        'preview': {
          'scenario_id': 'scene-a',
          'scenario_revision': 'revision-a',
          'preview_token': 'token-a',
          'account_id': 'account-b',
          'previous_account_id': 'account-a',
          'account_changed': true,
        },
      });
      expect(await c.activate(p), true);
      expect(calls.where((r) => r['action'] == 'activate'), hasLength(1));
      expect(
        calls.any(
          (r) =>
              r['action'] == 'set_active_profile' ||
              r['action'] == 'save_network_settings',
        ),
        false,
      );
      expect(calls.first['preview_token'], 'token-a');
      expect(calls.first['operation_id'], matches(RegExp(r'^[0-9a-f-]{36}$')));
    },
  );

  test(
    'lost activation reply is not replayed; matching durable receipt resolves uncertainty',
    () async {
      String? operation;
      var receipt = false, activations = 0;
      final c = ScenariosController((request) async {
        if (request['action'] == 'activate') {
          activations++;
          operation = request['operation_id'] as String;
          throw const EngineException('SCENARIO_RESULT_UNKNOWN', 'Unconfirmed');
        }
        return fixture(
          selected: 'scene-a',
          operation: operation,
          receipt: receipt,
          status: 'applying',
        );
      })..setSupported(true);
      addTearDown(c.dispose);
      final p = ScenarioPreview.fromMap({
        ...fixture(),
        'preview': {
          'scenario_id': 'scene-a',
          'scenario_revision': 'revision-a',
          'preview_token': 't',
        },
      });
      expect(await c.activate(p), false);
      await c.refresh();
      expect(c.unconfirmed, true);
      expect(activations, 1);
      receipt = true;
      await c.refresh();
      expect(c.unconfirmed, false);
      expect(activations, 1);
      expect(c.catalog!.appliedMatches, false);
    },
  );

  test('reset retires an in-flight mutation result', () async {
    final reply = Completer<Map<String, Object?>>();
    final c = ScenariosController((_) => reply.future)..setSupported(true);
    addTearDown(c.dispose);
    final pending = c.save(
      name: 'New',
      boundAccountId: null,
      settings: ScenarioSettings.fromProfile(UsqueProfile.defaultProfile()),
    );
    c.reset();
    reply.complete({...fixture(), 'persisted': true});
    expect(await pending, isNull);
    expect(c.catalog, isNull);
  });

  testWidgets(
    'Profiles tabs retain accounts and scene use previews one native activation',
    (tester) async {
      final engine = SceneEngine();
      final app = await pumpWorkflow(
        tester,
        engine,
        section: AppSection.profiles,
      );
      app.engineCapabilities = await engine.getCapabilities();
      await app.scenarios.refresh();
      await tester.pumpAndSettle();
      expect(find.text('Daily'), findsOneWidget);
      await tester.tap(find.byKey(const ValueKey('scenario-use-scene-a')));
      await tester.pumpAndSettle();
      expect(
        engine.sceneCalls.where((r) => r['action'] == 'activate'),
        isEmpty,
      );
      await tester.tap(find.byKey(const ValueKey('scenario-confirm-use')));
      await tester.pumpAndSettle();
      expect(
        engine.sceneCalls.where((r) => r['action'] == 'activate'),
        hasLength(1),
      );
      app.selectProfilesTab(ProfilesTab.accounts);
      await tester.pumpAndSettle();
      expect(find.text(app.strings.get('new_profile')), findsOneWidget);
    },
  );

  testWidgets(
    'creating and discarding a scene draft leaves live settings and templates unchanged',
    (tester) async {
      final engine = SceneEngine();
      final app = await pumpWorkflow(
        tester,
        engine,
        section: AppSection.profiles,
        size: const Size(390, 844),
      );
      app.engineCapabilities = await engine.getCapabilities();
      await app.scenarios.refresh();
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(const ValueKey('scenario-create')));
      await tester.pumpAndSettle();
      expect(find.byType(ScenarioEditorScreen), findsOneWidget);
      expect(find.byType(NavigationBar), findsNothing);
      await tester.enterText(
        find.byKey(const ValueKey('scenario-name')),
        'Work',
      );
      await tester.pumpAndSettle();
      await tester.binding.handlePopRoute();
      await tester.pumpAndSettle();
      await tester.tap(find.text(app.strings.get('discard_changes')));
      await tester.pumpAndSettle();
      expect(find.byType(ScenarioEditorScreen), findsNothing);
      expect(find.byType(NavigationBar), findsOneWidget);
      expect(engine.writes, 0);
      expect(engine.sceneCalls.where((r) => r['action'] == 'save'), isEmpty);
    },
  );

  testWidgets('preview warns when a scene ends Kill Switch protection', (
    tester,
  ) async {
    final engine = SceneEngine()
      ..previewExtra = {'kill_switch_output_removed': true};
    final app = await pumpWorkflow(
      tester,
      engine,
      section: AppSection.profiles,
    );
    app.engineCapabilities = await engine.getCapabilities();
    await app.scenarios.refresh();
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const ValueKey('scenario-use-scene-a')));
    await tester.pumpAndSettle();
    expect(
      find.byKey(const ValueKey('scenario-kill-switch-warning')),
      findsOneWidget,
    );
    expect(
      find.text(app.strings.get('scenario_kill_switch_output')),
      findsOneWidget,
    );
  });

  testWidgets('a scene rejected by current rules cannot be used', (
    tester,
  ) async {
    final engine = SceneEngine()
      ..extraState = {
        'invalid_scenario_ids': ['scene-a'],
      };
    final app = await pumpWorkflow(
      tester,
      engine,
      section: AppSection.profiles,
    );
    app.engineCapabilities = await engine.getCapabilities();
    await app.scenarios.refresh();
    await tester.pumpAndSettle();
    final use = tester.widget<TextButton>(
      find.byKey(const ValueKey('scenario-use-scene-a')),
    );
    expect(use.onPressed, isNull);
    expect(
      find.text(app.strings.get('scenario_settings_invalid')),
      findsOneWidget,
    );
    await tester.pump(const Duration(milliseconds: 200));
    await tester.pumpAndSettle();
  });

  testWidgets('a binding to a deleted account is repaired without editing', (
    tester,
  ) async {
    final engine = SceneEngine();
    engine.entries.single['bound_account_id'] = 'deleted-account';
    final app = await pumpWorkflow(
      tester,
      engine,
      section: AppSection.profiles,
    );
    app.engineCapabilities = await engine.getCapabilities();
    await app.scenarios.refresh();
    await tester.pumpAndSettle();
    final use = tester.widget<TextButton>(
      find.byKey(const ValueKey('scenario-use-scene-a')),
    );
    expect(use.onPressed, isNull);
    await tester.tap(find.byKey(const ValueKey('scenario-rebind-scene-a')));
    await tester.pumpAndSettle();
    final save = engine.sceneCalls.lastWhere(
      (call) => call['action'] == 'save',
    );
    expect(save['id'], 'scene-a');
    expect(save['bound_account_id'], isNull);
    expect(save['settings'], engine.entries.single['settings']);
    expect(find.byKey(const ValueKey('scenario-rebind-scene-a')), findsNothing);
  });

  testWidgets(
    'scene states carry their own tone and a modified selection offers its update',
    (tester) async {
      final engine = SceneEngine()..selected = 'scene-a';
      final app = await pumpWorkflow(
        tester,
        engine,
        section: AppSection.profiles,
      );
      app.engineCapabilities = await engine.getCapabilities();
      await app.scenarios.refresh();
      await tester.pumpAndSettle();
      InlineStatus status(String key) => tester.widget<InlineStatus>(
        find.ancestor(
          of: find.text(app.strings.get(key)),
          matching: find.byType(InlineStatus),
        ),
      );
      // Selection alone is neither a notice nor a success.
      expect(status('scenario_selected').tone, StatusTone.brand);
      expect(status('scenario_selected').icon, LucideIcons.circleDot);
      expect(
        find.byKey(const ValueKey('scenario-update-scene-a')),
        findsNothing,
      );

      engine.extraState = {
        'state': {'modified': true, 'apply_status': 'not_required'},
      };
      await app.scenarios.refresh();
      await tester.pumpAndSettle();
      expect(status('scenario_modified').tone, StatusTone.warning);
      expect(
        find.byKey(const ValueKey('scenario-update-scene-a')),
        findsOneWidget,
      );

      engine.extraState = {
        'state': {'modified': false, 'apply_status': 'failed'},
      };
      await app.scenarios.refresh();
      await tester.pumpAndSettle();
      expect(status('scenario_failed').tone, StatusTone.danger);
      expect(
        tester.widget<WarningBanner>(find.byType(WarningBanner)).danger,
        isTrue,
      );
    },
  );

  testWidgets(
    'the scene editor reports unsaved template edits and saves and uses from its bar',
    (tester) async {
      final engine = SceneEngine();
      final app = await pumpWorkflow(
        tester,
        engine,
        section: AppSection.profiles,
      );
      app.engineCapabilities = await engine.getCapabilities();
      await app.scenarios.refresh();
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(const ValueKey('scenario-create')));
      await tester.pumpAndSettle();
      // A template save never applies, so the bar does not say it would.
      expect(find.text(app.strings.get('scenario_unsaved')), findsOneWidget);
      expect(find.text(app.strings.get('unsaved_changes')), findsNothing);
      await tester.enterText(
        find.byKey(const ValueKey('scenario-name')),
        'Work',
      );
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(const ValueKey('scenario-save-use')));
      await tester.pumpAndSettle();
      expect(
        engine.sceneCalls.where((r) => r['action'] == 'save'),
        hasLength(1),
      );
      expect(
        find.byKey(const ValueKey('scenario-confirm-use')),
        findsOneWidget,
      );
      expect(
        engine.sceneCalls.where((r) => r['action'] == 'activate'),
        isEmpty,
      );
      expect(engine.writes, 0);
    },
  );

  testWidgets('scene preview states its effect before the field changes', (
    tester,
  ) async {
    final engine = SceneEngine()
      ..previewExtra = {'apply_kind': 'hot', 'needs_reconnect': true};
    final app = await pumpWorkflow(
      tester,
      engine,
      section: AppSection.profiles,
    );
    app.engineCapabilities = await engine.getCapabilities();
    await app.scenarios.refresh();
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const ValueKey('scenario-use-scene-a')));
    await tester.pumpAndSettle();
    Finder inDialog(String text) => find.descendant(
      of: find.byType(UsqueDialog),
      matching: find.text(text),
    );
    final note = inDialog(app.strings.get('scenario_will_reconnect'));
    final field = inDialog(scenarioFieldLabel(app.strings, 'frontends.http'));
    expect(note, findsOneWidget);
    expect(field, findsOneWidget);
    expect(tester.getTopLeft(note).dy, lessThan(tester.getTopLeft(field).dy));
  });

  testWidgets(
    'an Engine without scenes shows Accounts without a tab selector',
    (tester) async {
      final app = await pumpWorkflow(
        tester,
        WorkflowEngine(),
        section: AppSection.profiles,
      );
      await tester.pumpAndSettle();
      expect(find.byType(SegmentedButton<ProfilesTab>), findsNothing);
      expect(
        find.text(app.strings.get('scenario_unsupported')),
        findsOneWidget,
      );
      expect(find.text(app.strings.get('new_profile')), findsOneWidget);
    },
  );

  testWidgets('deleting a scene confirms with the destructive dialog style', (
    tester,
  ) async {
    final engine = SceneEngine();
    final app = await pumpWorkflow(
      tester,
      engine,
      section: AppSection.profiles,
    );
    app.engineCapabilities = await engine.getCapabilities();
    await app.scenarios.refresh();
    await tester.pumpAndSettle();
    await tester.tap(find.byType(PopupMenuButton<String>));
    await tester.pumpAndSettle();
    await tester.tap(find.text(app.strings.get('delete')).last);
    await tester.pumpAndSettle();
    expect(tester.widget<UsqueDialog>(find.byType(UsqueDialog)).danger, isTrue);
    expect(engine.sceneCalls.where((r) => r['action'] == 'delete'), isEmpty);
  });
}
