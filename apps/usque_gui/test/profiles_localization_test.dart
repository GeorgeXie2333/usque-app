import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:usque/core/app_strings.dart';
import 'package:usque/core/chain_strings.dart';
import 'package:usque/core/l10n/catalogs.dart';
import 'package:usque/core/l10n/scenarios.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/models/network_settings.dart';
import 'package:usque/screens/profiles_hub_screen.dart';
import 'package:usque/widgets/save_changes_bar.dart';
import 'package:usque/widgets/usque_dialog.dart';

import 'scenarios_test.dart' show SceneEngine;
import 'ui_workflow_test.dart' show pumpWorkflow, workflowHost;

class _SavingSceneEngine extends SceneEngine {
  final saveGate = Completer<void>();

  @override
  Future<Map<String, Object?>> scenarioCommand(
    Map<String, Object?> command,
  ) async {
    if (command['action'] == 'save') await saveGate.future;
    return super.scenarioCommand(command);
  }
}

void main() {
  test('Profiles and Scenes remain distinct and base catalogs agree', () {
    for (final id in kCatalogs.keys) {
      final scene = kScenarioCatalogs[id]!;
      expect(scene['profiles'], isNot(scene['scenario_scenes']), reason: id);
      for (final key in ['profiles', 'nav_profiles', 'profiles_subtitle']) {
        expect(kCatalogs[id]![key], scene[key], reason: '$id.$key');
      }
    }
  });

  test('default account localization preserves stored and custom names', () {
    final account = UsqueProfile.defaultProfile();
    final before = account.toMap();
    final chinese = AppStrings(LocalePreference.simplifiedChinese);
    final japanese = AppStrings(LocalePreference.japanese);
    expect(chinese.accountName(account), '默认账号');
    expect(japanese.accountName(account), '既定のアカウント');
    expect(account.toMap(), before);
    expect(
      chinese.accountName(account.copyWith(name: 'My account')),
      'My account',
    );
    expect(chinese.accountName(account.copyWith(id: 'custom')), 'Default');
  });

  test(
    'scene exit previews localize source descriptions and preserve names',
    () {
      final strings = AppStrings(LocalePreference.simplifiedChinese);
      final label = scenarioValueLabel(
        strings,
        {
          'chain_exit': {
            'enabled': true,
            'source': 'warp_wireguard',
            'profile_id': 'exit-a',
          },
        },
        'chain_exit',
        exitNames: {'exit-a': 'My exit'},
      );
      expect(
        label,
        '${strings.chainSourceLabel(ChainSource.warpWireguard)} · My exit',
      );
      expect(label, isNot(contains('via')));
      expect(strings.chainSourceLabel(ChainSource.openvpnCustom), 'OpenVPN');
    },
  );

  for (final locale in [
    LocalePreference.spanish,
    LocalePreference.portuguese,
    LocalePreference.polish,
  ]) {
    testWidgets('account rows use account word forms in ${locale.name}', (
      tester,
    ) async {
      final base = UsqueProfile.defaultProfile();
      final work = base.copyWith(id: 'work', name: 'Work');
      final engine = SceneEngine()
        ..storedProfiles = [base, work]
        ..extraState = {
          'session_id': 'session-a',
          'observation_session_id': 'session-a',
          'applied_account_id': 'work',
        };
      engine.settingsState = NetworkSettingsState(
        sourceEpoch: engine.settingsEpoch,
        sequence: 1,
        sessionId: 'session-a',
        appliedProfile: work,
      );
      final app = await pumpWorkflow(
        tester,
        engine,
        section: AppSection.profiles,
      );
      app
        ..localePreference = locale
        ..profiles = engine.storedProfiles;
      app.engineCapabilities = await engine.getCapabilities();
      await app.networkSettings.refresh();
      await app.scenarios.refresh();
      app.snapshot = const EngineSnapshot(phase: ConnectionPhase.connected);
      await tester.pumpWidget(workflowHost(app));
      app.openAccounts();
      await tester.pumpAndSettle();
      expect(find.text(app.strings.get('account_selected')), findsOneWidget);
      expect(find.text(app.strings.get('scenario_selected')), findsNothing);
      expect(
        find.descendant(
          of: find.byKey(const ValueKey('profile-connected-work')),
          matching: find.text(app.strings.get('account_connected')),
        ),
        findsOneWidget,
      );
    });
  }

  for (final locale in [
    LocalePreference.english,
    LocalePreference.simplifiedChinese,
  ]) {
    testWidgets(
      'scene save and leave copy describe the draft in ${locale.name}',
      (tester) async {
        final engine = _SavingSceneEngine();
        final app = await pumpWorkflow(
          tester,
          engine,
          section: AppSection.profiles,
        );
        app.localePreference = locale;
        app.engineCapabilities = await engine.getCapabilities();
        await app.scenarios.refresh();
        await tester.pumpWidget(workflowHost(app));
        await tester.pumpAndSettle();
        await tester.tap(find.byKey(const ValueKey('scenario-create')));
        await tester.pumpAndSettle();
        await tester.enterText(
          find.byKey(const ValueKey('scenario-name')),
          'Draft',
        );
        await tester.binding.handlePopRoute();
        await tester.pumpAndSettle();
        expect(
          find.descendant(
            of: find.byType(UsqueDialog),
            matching: find.text(app.strings.get('scenario_discard_title')),
          ),
          findsOneWidget,
        );
        expect(
          find.text(app.strings.get('scenario_discard_body')),
          findsOneWidget,
        );
        await tester.tap(find.text(app.strings.get('keep_editing')));
        await tester.pumpAndSettle();
        final save = find.widgetWithText(
          FilledButton,
          app.strings.get('scenario_save'),
        );
        await tester.ensureVisible(save);
        await tester.tap(save);
        await tester.pump();
        expect(
          find.descendant(
            of: find.byType(SaveChangesBar),
            matching: find.text(app.strings.get('scenario_saving')),
          ),
          findsNWidgets(2),
        );
        expect(find.text(app.strings.get('saving_changes')), findsNothing);
        engine.saveGate.complete();
        await tester.pumpAndSettle();
        expect(
          engine.sceneCalls.where((call) => call['action'] == 'save'),
          hasLength(1),
        );
        expect(
          engine.sceneCalls.where((call) => call['action'] == 'activate'),
          isEmpty,
        );
        expect(engine.writes, 0);
      },
    );
  }
}
