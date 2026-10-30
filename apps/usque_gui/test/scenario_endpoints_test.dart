import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/models/scenarios.dart';
import 'package:usque/screens/profiles_hub_screen.dart';
import 'package:usque/screens/scenario_editor_screen.dart';
import 'package:usque/state/app_controller.dart';
import 'package:usque/widgets/scenario_endpoint_editor.dart';
import 'package:usque/widgets/zero_trust_endpoint_warning.dart';

import 'scenarios_test.dart' show SceneEngine;
import 'ui_workflow_test.dart' show pumpWorkflow, workflowHost;

Future<AppController> editor(
  WidgetTester tester,
  SceneEngine engine, {
  LocalePreference locale = LocalePreference.english,
  double scale = 1,
  Size size = const Size(1280, 900),
}) async {
  final app = await pumpWorkflow(
    tester,
    engine,
    section: AppSection.profiles,
    size: size,
  );
  app.localePreference = locale;
  app.profileIdentityStatuses = engine.storedIdentityStatuses;
  app.engineCapabilities = await engine.getCapabilities();
  await app.scenarios.refresh();
  await tester.pumpWidget(
    workflowHost(
      app,
      scale: scale,
      home: ScenarioEditorScreen(
        controller: app,
        scene: app.scenarios.catalog!.scenarios.single,
      ),
    ),
  );
  await tester.pumpAndSettle();
  return app;
}

Future<void> tapText(WidgetTester tester, String text) async {
  final finder = find.text(text).last;
  await tester.ensureVisible(finder);
  await tester.pumpAndSettle();
  await tester.tap(finder);
  await tester.pumpAndSettle();
}

Future<void> enter(WidgetTester tester, String key, String value) async {
  final finder = find.byKey(ValueKey(key));
  await tester.ensureVisible(finder);
  await tester.enterText(finder, value);
  await tester.pumpAndSettle();
}

void main() {
  testWidgets(
    'update from current settings previews individual endpoint differences',
    (tester) async {
      final engine = SceneEngine()
        ..selected = 'scene-a'
        ..extraState = {
          'state': {'modified': true},
        }
        ..settingsOverride = ScenarioSettings.fromProfile(
          UsqueProfile.defaultProfile().copyWith(endpointPort: 8443),
        ).toMap();
      final app = await pumpWorkflow(
        tester,
        engine,
        section: AppSection.profiles,
      );
      app.engineCapabilities = await engine.getCapabilities();
      await app.scenarios.refresh();
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(const ValueKey('scenario-update-scene-a')));
      await tester.pumpAndSettle();
      expect(find.text(app.strings.get('port')), findsOneWidget);
      expect(find.text('443  →  8443'), findsOneWidget);
      await tapText(tester, app.strings.get('cancel'));
      expect(engine.sceneCalls.where((c) => c['action'] == 'save'), isEmpty);
    },
  );

  testWidgets(
    'scene endpoint drafts validate and save without changing current settings',
    (tester) async {
      final engine = SceneEngine();
      final app = await editor(tester, engine);
      final original = app.activeProfile.toMap();
      await tapText(tester, app.strings.get('endpoint_custom'));
      await enter(tester, 'scenario-endpoint_ipv4', 'invalid');
      await enter(tester, 'scenario-endpoint-port', '0');
      await enter(tester, 'scenario-endpoint-sni', 'https://invalid.test');
      await tapText(tester, app.strings.get('scenario_save'));
      expect(engine.sceneCalls.where((c) => c['action'] == 'save'), isEmpty);
      expect(find.text(app.strings.get('invalid_address')), findsOneWidget);
      await enter(tester, 'scenario-endpoint_ipv4', '192.0.2.10');
      await enter(tester, 'scenario-endpoint_ipv6', '2001:db8::10');
      await enter(tester, 'scenario-endpoint-port', '8443');
      await enter(tester, 'scenario-endpoint-sni', 'edge.example.com');
      await tapText(tester, app.strings.get('scenario_save'));
      final save = engine.sceneCalls.singleWhere((c) => c['action'] == 'save');
      expect((save['settings'] as Map)['endpoint'], {
        'selection': 'custom',
        'preset_region': null,
        'ipv4': '192.0.2.10',
        'ipv6': '2001:db8::10',
        'port': 8443,
        'sni': 'edge.example.com',
      });
      expect(app.activeProfile.toMap(), original);
      expect(engine.writes, 0);
      expect(
        engine.sceneCalls.where((c) => c['action'] == 'activate'),
        isEmpty,
      );
    },
  );

  testWidgets('automatic endpoint save retains the confirmed manual pair', (
    tester,
  ) async {
    final engine = SceneEngine();
    final app = await editor(tester, engine);
    await tapText(tester, app.strings.get('endpoint_custom'));
    await enter(tester, 'scenario-endpoint_ipv4', '192.0.2.20');
    await tapText(tester, app.strings.get('endpoint_automatic'));
    await enter(tester, 'scenario-endpoint-port', '8443');
    await tapText(tester, app.strings.get('scenario_save'));
    final value =
        (engine.sceneCalls.singleWhere((c) => c['action'] == 'save')['settings']
                as Map)['endpoint']
            as Map;
    expect(value['selection'], 'automatic');
    expect(value['ipv4'], UsqueProfile.defaultEndpointIpv4);
    expect(value['port'], 8443);
  });

  testWidgets(
    'preset cancellation retains manual draft and selection stages an IPv6 policy',
    (tester) async {
      final engine = SceneEngine();
      engine.storedIdentityStatuses = {
        UsqueProfile.defaultProfileId: const ProfileIdentityStatus(
          state: ProfileIdentityState.ready,
          licenseState: LicenseState.free,
        ),
      };
      final app = await editor(tester, engine);
      await tapText(tester, app.strings.get('endpoint_custom'));
      await enter(tester, 'scenario-endpoint_ipv6', 'invalid');
      await tapText(tester, app.strings.get('endpoint_presets'));
      await tapText(tester, app.strings.get('cancel'));
      expect(
        tester
            .widget<SegmentedButton<bool>>(
              find.byKey(const ValueKey('endpoint-custom-source')),
            )
            .selected,
        {false},
      );
      await tapText(tester, app.strings.get('endpoint_presets'));
      await tapText(tester, app.strings.get('continue'));
      await tapText(tester, app.strings.get('scenario_save'));
      final settings =
          engine.sceneCalls.singleWhere(
                (c) => c['action'] == 'save',
              )['settings']
              as Map;
      expect(settings['ip_policy'], 'ipv6_only');
      expect((settings['endpoint'] as Map)['preset_region'], 'us');
      expect(
        (settings['endpoint'] as Map)['ipv6'],
        UsqueProfile.defaultEndpointIpv6,
      );
      expect(engine.writes, 0);
    },
  );

  testWidgets(
    'legacy scene rename does not add endpoints and old engines hide editing',
    (tester) async {
      final engine = SceneEngine();
      (engine.entries.single['settings'] as Map).remove('endpoint');
      final app = await editor(tester, engine);
      await enter(tester, 'scenario-name', 'Renamed');
      await tapText(tester, app.strings.get('scenario_save'));
      expect(
        (engine.sceneCalls.singleWhere((c) => c['action'] == 'save')['settings']
                as Map)
            .containsKey('endpoint'),
        false,
      );
      engine.extraState = {'endpoint_editing': false};
      await app.scenarios.refresh();
      await tester.pumpWidget(
        workflowHost(
          app,
          home: ScenarioEditorScreen(
            key: const ValueKey('old-engine'),
            controller: app,
            scene: app.scenarios.catalog!.scenarios.single,
          ),
        ),
      );
      await tester.pumpAndSettle();
      expect(find.byType(ScenarioEndpointEditor), findsNothing);
    },
  );

  testWidgets(
    'Zero Trust scene manual fields require the existing risk acknowledgement',
    (tester) async {
      final engine = SceneEngine();
      engine.storedIdentityStatuses = {
        UsqueProfile.defaultProfileId: const ProfileIdentityStatus(
          state: ProfileIdentityState.ready,
          provider: IdentityProvider.zeroTrust,
          registeredEndpointIpv4: '162.159.197.2',
          registeredEndpointIpv6: '2606:4700:102::2',
        ),
      };
      final app = await editor(tester, engine);
      final field = find.byKey(const ValueKey('scenario-endpoint_ipv4'));
      expect(tester.widget<TextFormField>(field).enabled, true);
      final textField = find.descendant(
        of: field,
        matching: find.byType(TextField),
      );
      expect(tester.widget<TextField>(textField).readOnly, true);
      await tapText(tester, app.strings.get('zero_trust_endpoint_edit'));
      expect(find.byType(ZeroTrustEndpointWarning), findsOneWidget);
      await tester.tap(find.byKey(const ValueKey('zt-endpoint-risk-ack')));
      await tester.pumpAndSettle();
      await tapText(
        tester,
        app.strings.get('zero_trust_endpoint_risk_continue'),
      );
      expect(tester.widget<TextField>(textField).readOnly, false);
      expect(engine.writes, 0);
    },
  );

  testWidgets('using a manual ZT scene cannot bypass the risk confirmation', (
    tester,
  ) async {
    final engine = SceneEngine()
      ..previewExtra = {'zero_trust_endpoint_warning': true};
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
    expect(find.byType(ZeroTrustEndpointWarning), findsOneWidget);
    expect(find.byKey(const ValueKey('scenario-confirm-use')), findsNothing);
    await tapText(tester, app.strings.get('cancel'));
    expect(engine.sceneCalls.where((c) => c['action'] == 'activate'), isEmpty);
  });

  for (final locale in [
    LocalePreference.english,
    LocalePreference.simplifiedChinese,
    LocalePreference.persian,
  ]) {
    testWidgets(
      'scene endpoint fields fit 200 percent and keyboard focus in ${locale.name}',
      (tester) async {
        final engine = SceneEngine();
        final app = await editor(
          tester,
          engine,
          locale: locale,
          scale: 2,
          size: const Size(390, 844),
        );
        await tapText(tester, app.strings.get('endpoint_custom'));
        await enter(tester, 'scenario-endpoint-port', '8443');
        await tester.sendKeyEvent(LogicalKeyboardKey.tab);
        await tester.pumpAndSettle();
        expect(tester.takeException(), isNull);
        expect(engine.sceneCalls.where((c) => c['action'] == 'save'), isEmpty);
      },
    );
  }

  test(
    'endpoint differences have localized values and survive the profile projection',
    () {
      final profile = UsqueProfile.defaultProfile().copyWith(
        endpointSelection: EndpointSelection.custom,
        endpointPresetRegion: EndpointPresetRegion.fi,
        endpointPort: 8443,
      );
      final settings = ScenarioSettings.fromProfile(profile);
      final restored = settings.toProfile(UsqueProfile.defaultProfile());
      expect(restored.endpointPresetRegion, EndpointPresetRegion.fi);
      expect(restored.endpointPort, 8443);
      final engine = SceneEngine();
      final app = AppController(engine);
      addTearDown(app.dispose);
      expect(
        scenarioValueLabel(
          app.strings,
          settings.toMap(),
          'endpoint.preset_region',
        ),
        app.strings.get('endpoint_preset_fi'),
      );
      expect(
        scenarioValueLabel(app.strings, settings.toMap(), 'endpoint.port'),
        '8443',
      );
    },
  );
}
