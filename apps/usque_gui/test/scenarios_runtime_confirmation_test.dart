import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/models/network_settings.dart';
import 'package:usque/screens/profiles_hub_screen.dart';
import 'package:usque/state/scenarios_controller.dart';

import 'scenarios_test.dart' show SceneEngine, fixture;
import 'ui_workflow_test.dart' show pumpWorkflow;

Map<String, Object?> confirmedCatalog({
  String epoch = 'engine-a',
  String? session = 'session-a',
  String? account,
}) => {
  ...fixture(
    account: account ?? UsqueProfile.defaultProfileId,
    selected: 'scene-a',
    applied: true,
    status: 'applied',
  ),
  'source_epoch': epoch,
  'session_id': session,
  'observation_session_id': session,
  'applied_account_id': account ?? UsqueProfile.defaultProfileId,
  'applied_scenario_id': 'scene-a',
  'applied_scene_confirmed': true,
};

Map<String, Object?> disconnectedCatalog({
  String epoch = 'engine-a',
  String? observationSession = 'session-b',
  bool modified = false,
  String status = 'deferred',
}) => {
  ...fixture(selected: 'scene-a', modified: modified, status: status),
  'source_epoch': epoch,
  'session_id': null,
  'observation_session_id': observationSession,
  'applied_account_id': null,
  'applied_scenario_id': null,
  'applied_scene_confirmed': false,
};

NetworkSettingsState runtime({
  String epoch = 'engine-a',
  String? session = 'session-a',
  UsqueProfile? profile,
}) => NetworkSettingsState(
  sourceEpoch: epoch,
  sequence: 1,
  sessionId: session,
  storedProfile: profile ?? UsqueProfile.defaultProfile(),
  appliedProfile: profile ?? UsqueProfile.defaultProfile(),
  status: NetworkSettingsApplyStatus.applied,
  persisted: true,
);

class _QueryFailureEngine extends SceneEngine {
  bool failQuery = false;
  @override
  Future<Map<String, Object?>> scenarioCommand(Map<String, Object?> request) {
    if (failQuery && request['action'] == 'list') {
      throw TimeoutException('Scene query unavailable');
    }
    return super.scenarioCommand(request);
  }
}

void main() {
  testWidgets('a retired passive scene read schedules a fresh visible read', (
    tester,
  ) async {
    final firstReply = Completer<Map<String, Object?>>();
    final saved = {...fixture(), 'runtime_observed': false};
    var reads = 0;
    final controller =
        ScenariosController((_) {
            reads++;
            return reads == 1 ? firstReply.future : Future.value(saved);
          })
          ..setSupported(true)
          ..setVisible(true);
    addTearDown(controller.dispose);
    final firstRead = controller.refresh();
    controller.observeRuntime(
      const NetworkSettingsState(
        sourceEpoch: '',
        sequence: 0,
        runtimeObserved: false,
        status: NetworkSettingsApplyStatus.unknown,
      ),
    );
    // The ordinary invalidation fires before the pending scene reply arrives.
    controller.invalidate();
    await tester.pump(const Duration(milliseconds: 150));
    expect(reads, 1);
    firstReply.complete(saved);
    await firstRead;
    await tester.pump(const Duration(milliseconds: 150));
    expect(reads, 2);
    expect(controller.catalog?.scenarios.single.name, 'Daily');
    expect(controller.runtimeConfirmed, false);
    expect(controller.queryUnconfirmed, false);
  });

  testWidgets(
    'confirmed Android disconnect keeps saved scene status without claiming a running session',
    (tester) async {
      final engine = _QueryFailureEngine()
        ..selected = 'scene-a'
        ..extraState = confirmedCatalog();
      final app = await pumpWorkflow(
        tester,
        engine,
        section: AppSection.profiles,
      );
      app.engineCapabilities = await engine.getCapabilities();
      await app.networkSettings.refresh();
      app.networkSettings.accept(runtime());
      app.snapshot = const EngineSnapshot(phase: ConnectionPhase.connected);
      await app.scenarios.refresh();
      await tester.pumpAndSettle();
      expect(scenarioStateKey(app), 'scenario_applied');

      app.snapshot = const EngineSnapshot(phase: ConnectionPhase.disconnected);
      app.networkSettings.accept(
        const NetworkSettingsState(
          sourceEpoch: 'engine-a',
          sequence: 2,
          sessionId: 'session-b',
          status: NetworkSettingsApplyStatus.deferred,
        ),
      );
      expect(app.scenarios.runtimeConfirmed, false);
      engine.extraState = disconnectedCatalog();
      await app.scenarios.refresh();
      await tester.pumpAndSettle();
      expect(app.scenarios.runtimeConfirmed, true);
      expect(scenarioStateKey(app), 'scenario_pending');
      expect(app.scenarios.catalog!.selected!.name, 'Daily');
      expect(find.byKey(const ValueKey('profiles-running')), findsNothing);

      engine.extraState = disconnectedCatalog(modified: true);
      await app.scenarios.refresh();
      await tester.pumpAndSettle();
      expect(scenarioStateKey(app), 'scenario_modified');
      engine.failQuery = true;
      await app.scenarios.refresh();
      expect(scenarioStateKey(app), 'scenario_unknown');
      engine.failQuery = false;
      await app.scenarios.refresh();
      expect(scenarioStateKey(app), 'scenario_modified');
    },
  );

  test(
    'disconnected read cannot reconfirm an earlier Android generation or epoch',
    () async {
      var response = disconnectedCatalog(observationSession: 'session-a');
      final controller = ScenariosController((_) async => response)
        ..setSupported(true);
      addTearDown(controller.dispose);
      controller.observeRuntime(
        const NetworkSettingsState(
          sourceEpoch: 'engine-a',
          sequence: 2,
          sessionId: 'session-b',
          status: NetworkSettingsApplyStatus.deferred,
        ),
      );
      await controller.refresh();
      expect(controller.runtimeConfirmed, false);
      response = disconnectedCatalog(epoch: 'retired-engine');
      await controller.refresh();
      expect(controller.runtimeConfirmed, false);
      response = disconnectedCatalog();
      await controller.refresh();
      expect(controller.runtimeConfirmed, true);
      // A newer connecting observation remains distinct even without a profile.
      controller.observeRuntime(
        const NetworkSettingsState(
          sourceEpoch: 'engine-a',
          sequence: 3,
          sessionId: 'session-c',
          status: NetworkSettingsApplyStatus.applying,
        ),
      );
      await controller.refresh();
      expect(controller.runtimeConfirmed, false);
    },
  );

  test(
    'explicit null observation identity does not fall back to a public session',
    () async {
      var response = <String, Object?>{
        ...disconnectedCatalog(observationSession: null),
        'session_id': 'session-c',
      };
      final controller = ScenariosController((_) async => response)
        ..setSupported(true);
      addTearDown(controller.dispose);
      controller.observeRuntime(
        const NetworkSettingsState(
          sourceEpoch: 'engine-a',
          sequence: 3,
          sessionId: 'session-c',
          status: NetworkSettingsApplyStatus.unknown,
        ),
      );
      await controller.refresh();
      expect(controller.runtimeConfirmed, false);
      // Missing authority is an invalid response, not a public-session fallback.
      response.remove('observation_session_id');
      await controller.refresh();
      expect(controller.queryUnconfirmed, true);
      expect(controller.runtimeConfirmed, false);
      response = {...response, 'observation_session_id': 'session-b'};
      await controller.refresh();
      expect(controller.runtimeConfirmed, false);
    },
  );

  test(
    'late disconnected catalogue cannot confirm a newer Android observation',
    () async {
      final reply = Completer<Map<String, Object?>>();
      final controller = ScenariosController((_) => reply.future)
        ..setSupported(true);
      addTearDown(controller.dispose);
      controller.observeRuntime(
        const NetworkSettingsState(
          sourceEpoch: 'engine-a',
          sequence: 2,
          sessionId: 'session-b',
          status: NetworkSettingsApplyStatus.deferred,
        ),
      );
      final read = controller.refresh();
      controller.observeRuntime(
        const NetworkSettingsState(
          sourceEpoch: 'engine-a',
          sequence: 3,
          sessionId: 'session-c',
          status: NetworkSettingsApplyStatus.deferred,
        ),
      );
      reply.complete(disconnectedCatalog());
      await read;
      expect(controller.queryUnconfirmed, false);
      expect(controller.catalog!.sessionId, isNull);
      expect(controller.runtimeConfirmed, false);
    },
  );

  testWidgets(
    'matching Android observation never changes unknown or failed into success',
    (tester) async {
      final engine = SceneEngine()
        ..selected = 'scene-a'
        ..extraState = disconnectedCatalog(status: 'unknown');
      final app = await pumpWorkflow(
        tester,
        engine,
        section: AppSection.profiles,
      );
      app.engineCapabilities = await engine.getCapabilities();
      await app.networkSettings.refresh();
      app.networkSettings.accept(
        const NetworkSettingsState(
          sourceEpoch: 'engine-a',
          sequence: 2,
          sessionId: 'session-b',
          status: NetworkSettingsApplyStatus.unknown,
        ),
      );
      await app.scenarios.refresh();
      await tester.pumpAndSettle();
      expect(app.scenarios.runtimeConfirmed, true);
      expect(scenarioStateKey(app), 'scenario_unknown');
      engine.extraState = disconnectedCatalog(status: 'failed');
      await app.scenarios.refresh();
      await tester.pumpAndSettle();
      expect(scenarioStateKey(app), 'scenario_failed');
      expect(find.byKey(const ValueKey('profiles-running')), findsNothing);
    },
  );

  testWidgets(
    'failed scene query hides cached applied status and current connection but keeps the saved scene',
    (tester) async {
      final engine = _QueryFailureEngine()
        ..selected = 'scene-a'
        ..extraState = confirmedCatalog();
      final app = await pumpWorkflow(
        tester,
        engine,
        section: AppSection.profiles,
      );
      app.engineCapabilities = await engine.getCapabilities();
      await app.networkSettings.refresh();
      app.networkSettings.accept(runtime());
      app.snapshot = const EngineSnapshot(phase: ConnectionPhase.connected);
      await app.scenarios.refresh();
      await tester.pumpAndSettle();
      expect(scenarioStateKey(app), 'scenario_applied');

      engine.failQuery = true;
      await app.scenarios.refresh();
      await tester.pumpAndSettle();
      expect(scenarioStateKey(app), 'scenario_unknown');
      expect(
        tester
            .widget<Text>(find.byKey(const ValueKey('profiles-running')))
            .data,
        app.strings.get('scenario_unknown'),
      );
      expect(app.scenarios.catalog!.selected!.name, 'Daily');

      engine.failQuery = false;
      await app.scenarios.refresh();
      await tester.pumpAndSettle();
      expect(scenarioStateKey(app), 'scenario_applied');
      expect(
        tester
            .widget<Text>(find.byKey(const ValueKey('profiles-running')))
            .data,
        contains('Daily'),
      );
    },
  );

  testWidgets(
    'new native account and session retire the previous running summary immediately',
    (tester) async {
      final a = UsqueProfile.defaultProfile().copyWith(name: 'Account A');
      final b = a.copyWith(id: 'account-b', name: 'Account B');
      final engine = _QueryFailureEngine()
        ..storedProfiles = [a, b]
        ..selected = 'scene-a'
        ..extraState = confirmedCatalog(account: a.id);
      final app = await pumpWorkflow(
        tester,
        engine,
        section: AppSection.profiles,
      );
      app.engineCapabilities = await engine.getCapabilities();
      await app.networkSettings.refresh();
      app.profiles = [a, b];
      app.networkSettings.accept(runtime(profile: a));
      app.snapshot = EngineSnapshot(
        phase: ConnectionPhase.connected,
        connectedAt: DateTime.utc(2026, 10, 30),
      );
      await app.scenarios.refresh();
      await tester.pumpAndSettle();
      expect(
        tester
            .widget<Text>(find.byKey(const ValueKey('profiles-running')))
            .data,
        'Account A · Daily',
      );
      String? selectedAccount() => tester
          .widget<Text>(find.byKey(const ValueKey('profiles-selected-account')))
          .data;
      expect(selectedAccount(), 'Account A');

      engine.failQuery = true;
      app.setActiveProfile(b.id);
      await app.flushProfileWrites();
      app.networkSettings.accept(
        runtime(epoch: 'engine-b', session: 'session-b', profile: b),
      );
      app.snapshot = EngineSnapshot(
        phase: ConnectionPhase.connected,
        connectedAt: DateTime.utc(2026, 10, 31),
        reconnectCount: 1,
      );
      await tester.pump();
      expect(app.scenarios.runtimeConfirmed, false);
      expect(
        tester
            .widget<Text>(find.byKey(const ValueKey('profiles-running')))
            .data,
        app.strings.get('scenario_unknown'),
      );
      await app.scenarios.refresh();
      await tester.pumpAndSettle();
      expect(scenarioStateKey(app), 'scenario_unknown');
      expect(app.scenarios.catalog!.selectedId, 'scene-a');
      expect(selectedAccount(), app.strings.get('scenario_unknown'));

      engine.failQuery = false;
      engine.extraState = confirmedCatalog(
        epoch: 'engine-b',
        session: 'session-b',
        account: b.id,
      );
      await app.scenarios.refresh();
      await tester.pumpAndSettle();
      expect(scenarioStateKey(app), 'scenario_applied');
      expect(selectedAccount(), 'Account B');
      expect(
        tester
            .widget<Text>(find.byKey(const ValueKey('profiles-running')))
            .data,
        'Account B · Daily',
      );
    },
  );

  for (final change in ['epoch', 'session', 'account']) {
    test('a delayed catalogue cannot reconfirm the retired $change', () async {
      final gate = Completer<Map<String, Object?>>();
      var reply = confirmedCatalog();
      var delayed = false;
      final controller = ScenariosController(
        (_) => delayed ? gate.future : Future.value(reply),
      )..setSupported(true);
      addTearDown(controller.dispose);
      controller.observeRuntime(runtime());
      await controller.refresh();
      expect(controller.runtimeConfirmed, true);
      delayed = true;
      final oldRead = controller.refresh();
      final epoch = change == 'epoch' ? 'engine-b' : 'engine-a';
      final session = change == 'session' ? 'session-b' : 'session-a';
      final account = change == 'account'
          ? 'account-b'
          : UsqueProfile.defaultProfileId;
      controller.observeRuntime(
        runtime(
          epoch: epoch,
          session: session,
          profile: UsqueProfile.defaultProfile().copyWith(id: account),
        ),
      );
      expect(controller.runtimeConfirmed, false);
      gate.complete(reply);
      await oldRead;
      expect(controller.runtimeConfirmed, false);
      expect(controller.catalog!.selectedId, 'scene-a');
      delayed = false;
      reply = confirmedCatalog(
        epoch: epoch,
        session: session,
        account: account,
      );
      await controller.refresh();
      expect(controller.runtimeConfirmed, true);
    });
  }

  test(
    'late observation matching a fresh native catalogue retains confirmation',
    () async {
      final controller = ScenariosController(
        (_) async => confirmedCatalog(epoch: 'engine-b', session: 'session-b'),
      )..setSupported(true);
      addTearDown(controller.dispose);
      controller.observeRuntime(runtime());
      await controller.refresh();
      expect(controller.runtimeConfirmed, false);
      controller.observeRuntime(
        runtime(epoch: 'engine-b', session: 'session-b'),
      );
      expect(controller.runtimeConfirmed, true);
    },
  );

  test(
    'a response from a retired engine cannot regain authority on a later request',
    () async {
      var reply = confirmedCatalog();
      final controller = ScenariosController((_) async => reply)
        ..setSupported(true);
      addTearDown(controller.dispose);
      controller.observeRuntime(runtime());
      await controller.refresh();
      controller.observeRuntime(runtime(epoch: 'engine-b'));
      await controller.refresh();
      expect(controller.runtimeConfirmed, false);
      reply = confirmedCatalog(epoch: 'engine-b');
      await controller.refresh();
      expect(controller.runtimeConfirmed, true);
    },
  );

  test(
    'a later query for the old session stays unconfirmed in the same epoch',
    () async {
      var reply = confirmedCatalog();
      final controller = ScenariosController((_) async => reply)
        ..setSupported(true);
      addTearDown(controller.dispose);
      controller.observeRuntime(runtime());
      await controller.refresh();
      controller.observeRuntime(runtime(session: 'session-b'));
      await controller.refresh();
      expect(controller.runtimeConfirmed, false);
      expect(controller.catalog!.selectedId, 'scene-a');
      reply = confirmedCatalog(session: 'session-b');
      await controller.refresh();
      expect(controller.runtimeConfirmed, true);
    },
  );

  test(
    'clearing the applied profile invalidates a cached successful connection',
    () async {
      var reply = confirmedCatalog();
      final controller = ScenariosController((_) async => reply)
        ..setSupported(true);
      addTearDown(controller.dispose);
      controller.observeRuntime(runtime());
      await controller.refresh();
      expect(controller.runtimeConfirmed, true);
      controller.observeRuntime(
        const NetworkSettingsState(
          sourceEpoch: 'engine-a',
          sequence: 2,
          sessionId: 'session-a',
          status: NetworkSettingsApplyStatus.unknown,
        ),
      );
      expect(controller.runtimeConfirmed, false);
      await controller.refresh();
      expect(controller.runtimeConfirmed, false);
      reply = {
        ...confirmedCatalog(),
        'applied_account_id': null,
        'applied_scenario_id': null,
        'applied_scene_confirmed': false,
        'state': {'applied_matches': null, 'apply_status': 'unknown'},
      };
      await controller.refresh();
      expect(controller.runtimeConfirmed, true);
      expect(controller.catalog!.appliedSceneConfirmed, false);
      expect(controller.catalog!.appliedMatches, isNull);
    },
  );

  testWidgets('fresh native unknown state never renders as Applied', (
    tester,
  ) async {
    final engine = SceneEngine()
      ..selected = 'scene-a'
      ..extraState = {
        ...confirmedCatalog(),
        'state': {'applied_matches': true, 'apply_status': 'unknown'},
      };
    final app = await pumpWorkflow(
      tester,
      engine,
      section: AppSection.profiles,
    );
    app.engineCapabilities = await engine.getCapabilities();
    await app.networkSettings.refresh();
    app.networkSettings.accept(runtime());
    await app.scenarios.refresh();
    await tester.pumpAndSettle();
    expect(scenarioStateKey(app), 'scenario_unknown');
    await tester.pump(const Duration(milliseconds: 200));
    await tester.pumpAndSettle();
  });

  test(
    'catalogue without runtime identity is rejected and preserves confirmed selection',
    () async {
      var response = fixture(selected: 'scene-a');
      final controller = ScenariosController((_) async => response)
        ..setSupported(true);
      addTearDown(controller.dispose);
      await controller.refresh();
      expect(controller.runtimeConfirmed, true);
      for (final field in ['source_epoch', 'observation_session_id']) {
        response = fixture()..remove(field);
        await controller.refresh();
        expect(controller.queryUnconfirmed, true);
        expect(controller.runtimeConfirmed, false);
        expect(controller.catalog!.selectedId, 'scene-a');
      }
      response = fixture(selected: 'scene-a');
      await controller.refresh();
      expect(controller.runtimeConfirmed, true);
    },
  );
}
