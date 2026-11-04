import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:shared_preferences/shared_preferences.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/models/scenarios.dart' show ConnectionScenario;
import 'package:usque/services/engine_client.dart';
import 'package:usque/state/app_controller.dart';
import 'package:usque/widgets/usque_dialog.dart';

import 'android_passive_observation_test.dart' show storedCatalog;
import 'scenarios_recovery_test.dart' show LostActivationEngine;
import 'scenarios_test.dart' show SceneEngine;
import 'ui_workflow_test.dart' show pumpWorkflow;

class OwnerSceneEngine extends SceneEngine implements ControlOperationClient {
  final owners = <String>{};
  var begins = 0, ends = 0;
  bool failBegin = false;

  @override
  Future<String> beginControlOperation() async {
    if (failBegin) throw TimeoutException('Local channel unavailable');
    final token = 'owner-${++begins}';
    owners.add(token);
    return token;
  }

  @override
  Future<void> endControlOperation(String token) async {
    ends++;
    owners.remove(token);
  }

  @override
  Future<Map<String, Object?>> scenarioCommand(Map<String, Object?> request) {
    if (request['action'] != 'list') expect(owners, isNotEmpty);
    return super.scenarioCommand(request);
  }
}

class OwnerRecoveryEngine extends LostActivationEngine
    implements ControlOperationClient {
  final owners = <String>{};
  var begins = 0, ends = 0;
  var serviceStarts = 0, passiveLists = 0, liveLists = 0;
  bool serviceAlive = false;
  Completer<void>? beginGate;
  final beginEntered = Completer<void>();

  @override
  Future<String> beginControlOperation() async {
    if (!beginEntered.isCompleted) beginEntered.complete();
    await beginGate?.future;
    if (!serviceAlive) {
      serviceAlive = true;
      settingsEpoch = 'control-service-${++serviceStarts}';
      settingsState = null;
      settingsSequence = 0;
    }
    final token = 'owner-${++begins}';
    owners.add(token);
    return token;
  }

  @override
  Future<void> endControlOperation(String token) async {
    ends++;
    owners.remove(token);
    if (owners.isEmpty && !current.isConnected && !current.isTransitional) {
      serviceAlive = false;
    }
  }

  @override
  Future<NetworkSettingsState> getNetworkSettingsState() async => serviceAlive
      ? super.getNetworkSettingsState()
      : NetworkSettingsState.fromMap({
          'runtime_observed': false,
          'stored_profile': storedProfiles
              .firstWhere((profile) => profile.id == storedActiveProfileId)
              .toMap(),
        });

  @override
  Future<Map<String, Object?>> scenarioCommand(
    Map<String, Object?> request,
  ) async {
    if (request['action'] == 'list') {
      if (!serviceAlive) {
        passiveLists++;
        return {
          ...storedCatalog(),
          'scenarios': entries,
          'current_account_id': storedActiveProfileId,
        };
      }
      liveLists++;
    }
    if (request['action'] == 'activate') {
      expect(owners, isNotEmpty);
      try {
        return await super.scenarioCommand(request);
      } finally {
        // The durable activation survived, but the service died before its
        // reply. Passive reads cannot create its replacement or certify it.
        serviceAlive = false;
      }
    }
    return super.scenarioCommand(request);
  }

  @override
  Future<EngineSnapshot> connect(UsqueProfile profile) {
    expect(owners, isNotEmpty);
    expect(serviceAlive, true);
    return super.connect(profile);
  }

  @override
  Future<EngineSnapshot> retry() {
    expect(owners, isNotEmpty);
    expect(serviceAlive, true);
    calls.add('retry');
    return connect(
      storedProfiles.firstWhere(
        (profile) => profile.id == storedActiveProfileId,
      ),
    );
  }
}

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();

  testWidgets('scene owner failure stays visible and permits a later retry', (
    tester,
  ) async {
    final engine = OwnerSceneEngine()..failBegin = true;
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
    expect(tester.takeException(), isNull);
    expect(app.scenarios.errorCode, 'SCENARIO_QUERY_FAILED');
    expect(app.scenarios.unconfirmed, false);
    expect(app.scenarios.pendingOperationId, isNull);
    expect(engine.owners, isEmpty);
    expect(
      engine.sceneCalls.where((entry) => entry['action'] != 'list'),
      isEmpty,
    );
    expect(find.text(app.strings.get('scenario_save_failed')), findsOneWidget);
    engine.failBegin = false;
    await tester.tap(find.byKey(const ValueKey('scenario-use-scene-a')));
    await tester.pumpAndSettle();
    expect(find.byKey(const ValueKey('scenario-confirm-use')), findsOneWidget);
    expect(engine.owners, hasLength(1));
    await tester.tap(
      find.descendant(
        of: find.byType(UsqueDialog),
        matching: find.text(app.strings.get('cancel')),
      ),
    );
    await tester.pumpAndSettle();
    expect(engine.owners, isEmpty);
  });

  for (final confirm in [false, true]) {
    testWidgets(
      'scene preview owner spans dialog until ${confirm ? 'activation' : 'cancel'}',
      (tester) async {
        final engine = OwnerSceneEngine();
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
          find.byKey(const ValueKey('scenario-confirm-use')),
          findsOneWidget,
        );
        expect(engine.owners, hasLength(1));
        expect(engine.begins, 1);
        expect(engine.ends, 0);
        await tester.pump(const Duration(seconds: 2));
        expect(engine.owners, hasLength(1));
        expect(engine.ends, 0);
        expect(
          engine.sceneCalls.where((entry) => entry['action'] == 'activate'),
          isEmpty,
        );
        if (confirm) {
          await tester.tap(find.byKey(const ValueKey('scenario-confirm-use')));
        } else {
          await tester.tap(
            find.descendant(
              of: find.byType(UsqueDialog),
              matching: find.text(app.strings.get('cancel')),
            ),
          );
        }
        await tester.pumpAndSettle();
        expect(engine.owners, isEmpty);
        expect(engine.begins, 1);
        expect(engine.ends, 1);
        expect(
          engine.sceneCalls.where((entry) => entry['action'] == 'activate'),
          hasLength(confirm ? 1 : 0),
        );
        expect(engine.selected, confirm ? 'scene-a' : isNull);
      },
    );
  }

  for (final retry in [false, true]) {
    test(
      'explicit ${retry ? 'retry' : 'connect'} obtains an owner to settle a lost activation',
      () async {
        SharedPreferences.setMockInitialValues({});
        final engine = OwnerRecoveryEngine()..legacyProfilesImported = true;
        engine.storedProfiles = [
          engine.storedProfiles.single,
          engine.storedProfiles.single.copyWith(id: 'account-b', name: 'Bound'),
        ];
        final app = AppController(engine);
        addTearDown(app.dispose);
        await app.initialize();
        final scene = ConnectionScenario.fromMap(engine.entries.single);
        expect(
          await app.withControlOperation(() async {
            final preview = await app.scenarios.preview(scene);
            expect(preview, isNotNull);
            return app.scenarios.activate(preview!);
          }),
          false,
        );
        final operation = app.scenarios.pendingOperationId;
        expect(operation, isNotNull);
        expect(operation, engine.operation);
        expect(engine.owners, isEmpty);
        expect(engine.serviceAlive, false);
        final begins = engine.begins;
        // Join the automatic read started by activation's finally block before
        // issuing a distinct passive read against the stopped service.
        await app.scenarios.refresh();
        final passiveLists = engine.passiveLists;
        await app.scenarios.refresh();
        expect(engine.passiveLists, greaterThan(passiveLists));
        expect(engine.begins, begins);
        expect(engine.serviceAlive, false);
        expect(app.scenarios.pendingOperationId, operation);
        expect(app.scenarios.runtimeConfirmed, false);
        final liveLists = engine.liveLists;
        if (retry) {
          await app.retry();
          expect(engine.calls, contains('retry'));
        } else {
          await app.connectOrDisconnect();
        }
        expect(engine.lastConnectedProfile!.id, 'account-b');
        expect(engine.liveLists, greaterThan(liveLists));
        expect(engine.begins, begins + 1);
        expect(app.scenarios.pendingOperationId, isNull);
        expect(engine.activations, 1);
        expect(engine.owners, isEmpty);
        expect(engine.begins, engine.ends);
      },
    );
  }

  test(
    'disconnect while acquiring an owner cannot revive the connection',
    () async {
      SharedPreferences.setMockInitialValues({});
      final engine = OwnerRecoveryEngine()..legacyProfilesImported = true;
      final app = AppController(engine);
      addTearDown(app.dispose);
      await app.initialize();
      final gate = engine.beginGate = Completer<void>();
      addTearDown(() {
        if (!gate.isCompleted) gate.complete();
      });
      final connecting = app.connectOrDisconnect();
      await engine.beginEntered.future;
      expect(app.snapshot.phase, ConnectionPhase.preparing);
      await app.connectOrDisconnect();
      gate.complete();
      await connecting;
      expect(engine.lastConnectedProfile, isNull);
      expect(engine.owners, isEmpty);
      expect(engine.begins, engine.ends);
      expect(engine.serviceAlive, false);
      expect(app.snapshot.phase, ConnectionPhase.disconnected);
    },
  );
}
