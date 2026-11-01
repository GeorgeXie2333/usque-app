import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:shared_preferences/shared_preferences.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/screens/profiles_screen.dart';
import 'package:usque/screens/scenario_editor_screen.dart';
import 'package:usque/services/engine_client.dart';
import 'package:usque/state/app_controller.dart';
import 'package:usque/widgets/tv_input_scope.dart';
import 'package:usque/widgets/usque_dialog.dart';

import 'scenarios_test.dart' show SceneEngine;
import 'section_navigation_test.dart' show railItem;
import 'ui_workflow_test.dart'
    show WorkflowEngine, fieldWithLabel, pumpWorkflow, workflowHost;

final _windows = TargetPlatformVariant.only(TargetPlatform.windows);
final _manage = find.byKey(const ValueKey('profiles-manage-accounts'));

class _AccountsEngine extends SceneEngine {
  int accountWrites = 0;
  bool failSwitch = false;
  bool loseActivationReply = false;

  @override
  Future<void> setActiveProfile(String id) async {
    accountWrites++;
    if (failSwitch) {
      throw const EngineException('ENGINE_UNAVAILABLE', 'Unavailable');
    }
    await super.setActiveProfile(id);
  }

  @override
  Future<Map<String, Object?>> scenarioCommand(
    Map<String, Object?> request,
  ) async {
    if (request['action'] == 'activate' && loseActivationReply) {
      sceneCalls.add(Map.of(request));
      throw const EngineException('ENGINE_UNAVAILABLE', 'Reply lost');
    }
    return super.scenarioCommand(request);
  }
}

class _StartingEngine extends SceneEngine {
  final capabilities = Completer<EngineCapabilities?>();

  @override
  Future<EngineCapabilities?> getCapabilities() => capabilities.future;
}

Future<AppController> _profiles(
  WidgetTester tester,
  SceneEngine engine, {
  AppSection section = AppSection.profiles,
  Size size = const Size(1280, 900),
}) async {
  final app = await pumpWorkflow(tester, engine, section: section, size: size);
  app.profiles = engine.storedProfiles;
  app.engineCapabilities = await engine.getCapabilities();
  await app.networkSettings.refresh();
  await app.scenarios.refresh();
  await tester.pumpAndSettle();
  return app;
}

Future<void> _key(
  WidgetTester tester,
  LogicalKeyboardKey key, {
  LogicalKeyboardKey? modifier,
}) async {
  if (modifier != null) await tester.sendKeyDownEvent(modifier);
  await tester.sendKeyEvent(key);
  if (modifier != null) await tester.sendKeyUpEvent(modifier);
  await tester.pumpAndSettle();
}

void main() {
  testWidgets('Profiles has scenes at its root and accounts in a subpage', (
    tester,
  ) async {
    final app = await _profiles(tester, SceneEngine());
    expect(find.text('Daily'), findsOneWidget);
    expect(find.byType(AccountsScreen), findsNothing);
    expect(_manage, findsOneWidget);
    await tester.tap(_manage);
    await tester.pumpAndSettle();
    expect(find.byType(AccountsPage), findsOneWidget);
    expect(find.byType(AccountsScreen), findsOneWidget);
    expect(find.byType(NavigationRail).hitTestable(), findsOneWidget);
    await tester.tap(railItem(app, 'nav_profiles'));
    await tester.pumpAndSettle();
    expect(find.byType(AccountsPage), findsNothing);
    expect(find.text('Daily'), findsOneWidget);
    await tester.tap(railItem(app, 'nav_home'));
    await tester.pumpAndSettle();
    await tester.tap(railItem(app, 'nav_profiles'));
    await tester.pumpAndSettle();
    expect(find.text('Daily'), findsOneWidget);
    expect(find.byType(AccountsScreen), findsNothing);
  }, variant: _windows);

  testWidgets(
    'phone Home opens Accounts directly and Back returns to Profiles',
    (tester) async {
      final app = await _profiles(
        tester,
        SceneEngine(),
        section: AppSection.home,
        size: const Size(390, 844),
      );
      await tester.tap(find.byKey(const ValueKey('home-account-chip')));
      await tester.pumpAndSettle();
      expect(app.section, AppSection.profiles);
      expect(find.byType(AccountsPage), findsOneWidget);
      expect(find.byType(NavigationBar), findsNothing);
      await tester.binding.handlePopRoute();
      await tester.pumpAndSettle();
      expect(app.section, AppSection.profiles);
      expect(find.byType(AccountsPage), findsNothing);
      expect(find.byType(NavigationBar), findsOneWidget);
      expect(find.text('Daily'), findsOneWidget);
    },
    variant: TargetPlatformVariant.only(TargetPlatform.android),
  );

  testWidgets(
    'returning from Accounts retains the scene list scroll position',
    (tester) async {
      final engine = SceneEngine();
      final scene = engine.entries.single;
      engine.entries = [
        for (var i = 0; i < 20; i++)
          {...scene, 'id': 'scene-$i', 'name': 'Scene $i'},
      ];
      final app = await _profiles(tester, engine);
      final list = find.byKey(
        PageStorageKey<String>(app.strings.get('profiles')),
      );
      await tester.drag(list, const Offset(0, -600));
      await tester.pumpAndSettle();
      final scroll = tester.state<ScrollableState>(
        find.descendant(of: list, matching: find.byType(Scrollable)).first,
      );
      final before = scroll.position.pixels;
      expect(before, greaterThan(0));
      app.openAccounts();
      await tester.pumpAndSettle();
      await tester.binding.handlePopRoute();
      await tester.pumpAndSettle();
      expect(find.byType(AccountsPage), findsNothing);
      expect(scroll.position.pixels, before);
    },
  );

  testWidgets('account launch target is consumed after the first Shell mount', (
    tester,
  ) async {
    SharedPreferences.setMockInitialValues({
      'onboarding_complete': true,
      'update_checks_enabled': false,
    });
    final app = AppController(SceneEngine()..launchTarget = 'profiles');
    addTearDown(app.dispose);
    await app.initialize();
    app.localePreference = LocalePreference.english;
    await tester.pumpWidget(workflowHost(app));
    await tester.pumpAndSettle();
    expect(app.section, AppSection.profiles);
    expect(find.byType(AccountsPage), findsOneWidget);
    await tester.binding.handlePopRoute();
    await tester.pumpAndSettle();
    expect(find.text('Daily'), findsOneWidget);
    await tester.pumpWidget(const SizedBox.shrink());
    await tester.pumpWidget(workflowHost(app));
    await tester.pumpAndSettle();
    expect(find.byType(AccountsPage), findsNothing);
    expect(find.text('Daily'), findsOneWidget);
  });

  testWidgets(
    'account startup works when Shell mounts before capabilities arrive',
    (tester) async {
      SharedPreferences.setMockInitialValues({
        'onboarding_complete': true,
        'update_checks_enabled': false,
      });
      final engine = _StartingEngine()..launchTarget = 'profiles';
      final app = AppController(engine);
      addTearDown(app.dispose);
      final initialized = app.initialize();
      await tester.pump();
      expect(app.initialized, isTrue);
      expect(app.engineCapabilities, isNull);
      app.localePreference = LocalePreference.english;
      await tester.pumpWidget(workflowHost(app));
      await tester.pumpAndSettle();
      expect(find.byType(AccountsPage), findsOneWidget);
      engine.capabilities.complete(await SceneEngine().getCapabilities());
      await initialized;
      await tester.pumpAndSettle();
      expect(find.byType(AccountsPage, skipOffstage: false), findsOneWidget);
      await tester.binding.handlePopRoute();
      await tester.pumpAndSettle();
      expect(find.text('Daily'), findsOneWidget);
    },
  );

  testWidgets(
    'a hidden account request opens once when the UI becomes visible',
    (tester) async {
      final app = await _profiles(
        tester,
        SceneEngine(),
        section: AppSection.home,
      );
      app.setUiVisible(false);
      app.openAccounts();
      await tester.pumpAndSettle();
      expect(app.section, AppSection.home);
      expect(find.byType(AccountsPage, skipOffstage: false), findsNothing);
      app.setUiVisible(true);
      await tester.pumpAndSettle();
      expect(find.byType(AccountsPage), findsOneWidget);
      await tester.binding.handlePopRoute();
      await tester.pumpAndSettle();
      app.setUiVisible(false);
      app.setUiVisible(true);
      await tester.pumpAndSettle();
      expect(find.text('Daily'), findsOneWidget);
      expect(find.byType(AccountsPage), findsNothing);
    },
  );

  for (final target in [AppSection.profiles, AppSection.settings]) {
    testWidgets(
      'explicit $target navigation supersedes a queued account request',
      (tester) async {
        final app = await _profiles(
          tester,
          SceneEngine(),
          section: AppSection.home,
        );
        app.openAccounts();
        await _key(
          tester,
          target == AppSection.profiles
              ? LogicalKeyboardKey.digit2
              : LogicalKeyboardKey.digit4,
          modifier: LogicalKeyboardKey.controlLeft,
        );
        expect(app.section, target);
        expect(find.byType(AccountsPage, skipOffstage: false), findsNothing);
        await tester.pumpAndSettle();
        expect(app.section, target);
      },
      variant: _windows,
    );
  }

  testWidgets(
    'account shortcut keeps confirmed older engines at the account root',
    (tester) async {
      final engine = WorkflowEngine();
      final app = await pumpWorkflow(tester, engine, section: AppSection.home);
      app.engineCapabilities = await engine.getCapabilities();
      app.openAccounts();
      await tester.pumpAndSettle();
      expect(app.section, AppSection.profiles);
      expect(find.byType(AccountsPage), findsNothing);
      expect(find.byType(AccountsScreen), findsOneWidget);
      expect(find.text(app.strings.get('new_profile')), findsOneWidget);
    },
  );

  testWidgets(
    'repeated account requests create one page and do not reopen it',
    (tester) async {
      final app = await _profiles(
        tester,
        SceneEngine(),
        section: AppSection.home,
      );
      app.openAccounts();
      app.openAccounts();
      await tester.pumpAndSettle();
      final firstPage = tester.element(find.byType(AccountsPage));
      app.openAccounts();
      await tester.pumpAndSettle();
      expect(tester.element(find.byType(AccountsPage)), same(firstPage));
      expect(find.byType(AccountsPage, skipOffstage: false), findsOneWidget);
      await tester.binding.handlePopRoute();
      await tester.pumpAndSettle();
      expect(find.byType(AccountsPage, skipOffstage: false), findsNothing);
      expect(find.text('Daily'), findsOneWidget);
    },
  );

  for (final key in [
    LogicalKeyboardKey.escape,
    LogicalKeyboardKey.arrowLeft,
    LogicalKeyboardKey.digit2,
  ]) {
    testWidgets('Accounts returns to Profiles using ${key.keyLabel}', (
      tester,
    ) async {
      final app = await _profiles(tester, SceneEngine());
      await tester.tap(_manage);
      await tester.pumpAndSettle();
      await _key(
        tester,
        key,
        modifier: key == LogicalKeyboardKey.arrowLeft
            ? LogicalKeyboardKey.altLeft
            : key == LogicalKeyboardKey.digit2
            ? LogicalKeyboardKey.controlLeft
            : null,
      );
      expect(app.section, AppSection.profiles);
      expect(find.byType(AccountsPage), findsNothing);
      expect(find.text('Daily'), findsOneWidget);
    }, variant: _windows);
  }

  testWidgets(
    'account requests preserve a scene draft when leaving is cancelled',
    (tester) async {
      final engine = SceneEngine();
      final app = await _profiles(tester, engine);
      await tester.tap(find.byKey(const ValueKey('scenario-create')));
      await tester.pumpAndSettle();
      final name = find.byKey(const ValueKey('scenario-name'));
      await tester.enterText(name, 'Unfinished scene');
      await tester.pumpAndSettle();
      app.openAccounts();
      await tester.pumpAndSettle();
      expect(
        find.text(app.strings.get('discard_changes_title')),
        findsOneWidget,
      );
      app.openAccounts();
      app.openAccounts();
      await tester.pumpAndSettle();
      await tester.tap(find.text(app.strings.get('keep_editing')));
      await tester.pumpAndSettle();
      expect(find.byType(ScenarioEditorScreen), findsOneWidget);
      expect(find.byType(AccountsPage), findsNothing);
      expect(
        tester.widget<TextFormField>(name).controller!.text,
        'Unfinished scene',
      );
      app.openProfiles();
      await tester.pumpAndSettle();
      expect(
        find.text(app.strings.get('discard_changes_title')),
        findsOneWidget,
      );
      await tester.tap(find.text(app.strings.get('keep_editing')));
      await tester.pumpAndSettle();
      app.openAccounts();
      await tester.pumpAndSettle();
      await tester.tap(find.text(app.strings.get('discard_changes')));
      await tester.pumpAndSettle();
      expect(find.byType(AccountsPage), findsOneWidget);
      expect(
        find.byType(ScenarioEditorScreen, skipOffstage: false),
        findsNothing,
      );
      expect(engine.writes, 0);
      expect(engine.sceneCalls.where((r) => r['action'] == 'save'), isEmpty);
    },
  );

  testWidgets('account creation rename and deletion update the open subpage', (
    tester,
  ) async {
    final engine = _AccountsEngine();
    final app = await _profiles(tester, engine);
    await tester.tap(_manage);
    await tester.pumpAndSettle();
    await tester.tap(find.text(app.strings.get('new_profile')));
    await tester.pumpAndSettle();
    await tester.enterText(
      fieldWithLabel(app.strings.get('profile_name')),
      'Added here',
    );
    await tester.tap(find.text(app.strings.get('continue')));
    await tester.pumpAndSettle();
    await tester.tap(find.text(app.strings.get('create')));
    await tester.pumpAndSettle();
    final created = app.profiles.singleWhere((p) => p.name == 'Added here');
    expect(find.text('Added here'), findsOneWidget);
    expect(engine.accountWrites, 0);
    app.renameProfile(created.id, 'Renamed here');
    await app.flushProfileWrites();
    await tester.pumpAndSettle();
    expect(find.text('Added here'), findsNothing);
    expect(find.text('Renamed here'), findsOneWidget);
    await tester.tap(find.byKey(ValueKey('profile-delete-${created.id}')));
    await tester.pumpAndSettle();
    await tester.tap(
      find.widgetWithText(FilledButton, app.strings.get('delete')),
    );
    await tester.pumpAndSettle();
    expect(find.text('Renamed here'), findsNothing);
    expect(find.byType(AccountsPage), findsOneWidget);
    expect(app.activeProfileId, UsqueProfile.defaultProfileId);
    expect(engine.accountWrites, 0);
  });

  testWidgets(
    'hidden account subpage keeps its presentation quiet and catches up on restore',
    (tester) async {
      final first = UsqueProfile.defaultProfile().copyWith(
        name: 'Before hiding',
      );
      final engine = _AccountsEngine()
        ..storedProfiles = [first, first.copyWith(id: 'work', name: 'Work')];
      final app = await _profiles(tester, engine);
      await tester.tap(_manage);
      await tester.pumpAndSettle();
      final renderedList = tester.widget<AccountsScreen>(
        find.byType(AccountsScreen),
      );
      app.setUiVisible(false);
      await tester.pumpAndSettle();
      app.renameProfile(first.id, 'Updated while hidden');
      engine.failSwitch = true;
      app.setActiveProfile('work');
      await app.flushProfileWrites();
      await tester.pumpAndSettle();
      expect(app.profiles.first.name, 'Updated while hidden');
      expect(app.lastError, isNotNull);
      expect(find.text('Before hiding'), findsWidgets);
      expect(find.text('Updated while hidden'), findsNothing);
      expect(find.text(app.lastError!), findsNothing);
      expect(
        tester.widget<AccountsScreen>(find.byType(AccountsScreen)),
        same(renderedList),
      );
      app.setUiVisible(true);
      await tester.pumpAndSettle();
      expect(find.text('Before hiding'), findsNothing);
      expect(find.text('Updated while hidden'), findsWidgets);
      expect(find.text(app.lastError!), findsOneWidget);
      expect(find.byType(AccountsPage), findsOneWidget);
    },
    variant: _windows,
  );

  testWidgets(
    'account subpage confirms live switches and exposes native failure',
    (tester) async {
      final engine = _AccountsEngine();
      engine.storedProfiles.add(
        UsqueProfile.defaultProfile().copyWith(id: 'work', name: 'Work'),
      );
      final app = await _profiles(tester, engine);
      app.snapshot = const EngineSnapshot(phase: ConnectionPhase.connected);
      await tester.tap(_manage);
      await tester.pumpAndSettle();
      await tester.tap(find.text(app.strings.get('set_active')));
      await tester.pumpAndSettle();
      expect(engine.accountWrites, 0);
      await tester.tap(find.text(app.strings.get('cancel')));
      await tester.pumpAndSettle();
      expect(engine.accountWrites, 0);
      engine.failSwitch = true;
      await tester.tap(find.text(app.strings.get('set_active')));
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(const ValueKey('profile-switch-confirm')));
      await tester.pumpAndSettle();
      expect(engine.accountWrites, 1);
      expect(app.activeProfileId, UsqueProfile.defaultProfileId);
      expect(app.lastError, isNotNull);
      expect(find.text(app.lastError!), findsOneWidget);
      expect(find.byType(AccountsPage), findsOneWidget);
      engine.failSwitch = false;
      await tester.tap(find.text(app.strings.get('set_active')));
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(const ValueKey('profile-switch-confirm')));
      await tester.pumpAndSettle();
      expect(engine.accountWrites, 2);
      expect(app.activeProfileId, 'work');
    },
  );

  testWidgets(
    'account subpage retains unconfirmed scene feedback and does not replay',
    (tester) async {
      final engine = _AccountsEngine()..loseActivationReply = true;
      final app = await _profiles(tester, engine);
      final preview = await app.scenarios.preview(
        app.scenarios.catalog!.scenarios.single,
      );
      expect(preview, isNotNull);
      expect(await app.scenarios.activate(preview!), isFalse);
      await tester.pumpAndSettle();
      await tester.tap(_manage);
      await tester.pumpAndSettle();
      expect(find.byType(AccountsPage), findsOneWidget);
      expect(app.scenarios.unconfirmed, isTrue);
      expect(find.text(app.strings.get('scenario_unknown')), findsWidgets);
      expect(
        engine.sceneCalls.where((r) => r['action'] == 'activate'),
        hasLength(1),
      );
    },
  );

  testWidgets(
    'TV can focus open and leave account management with the remote',
    (tester) async {
      final app = await _profiles(tester, SceneEngine());
      final input = TvInputController(isTv: true);
      addTearDown(input.dispose);
      await tester.pumpWidget(
        TvInputHost(controller: input, child: workflowHost(app)),
      );
      await tester.pumpAndSettle();
      final link = find.text(app.strings.get('profiles_manage_accounts'));
      final focus = Focus.of(tester.element(link));
      focus.requestFocus();
      await tester.pumpAndSettle();
      expect(focus.hasFocus, isTrue);
      await _key(tester, LogicalKeyboardKey.select);
      expect(find.byType(AccountsPage), findsOneWidget);
      final add = find.text(app.strings.get('new_profile'));
      Focus.of(tester.element(add)).requestFocus();
      await tester.pumpAndSettle();
      await _key(tester, LogicalKeyboardKey.select);
      expect(find.byType(UsqueDialog), findsOneWidget);
      await tester.binding.handlePopRoute();
      await tester.pumpAndSettle();
      expect(find.byType(AccountsPage), findsOneWidget);
      await tester.binding.handlePopRoute();
      await tester.pumpAndSettle();
      expect(find.text('Daily'), findsOneWidget);
      expect(_manage, findsOneWidget);
      await tester.pumpWidget(const SizedBox.shrink());
    },
    variant: TargetPlatformVariant.only(TargetPlatform.android),
  );
}
