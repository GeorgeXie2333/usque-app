import 'dart:async';

import 'package:flutter_test/flutter_test.dart';
import 'package:shared_preferences/shared_preferences.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/models/scenarios.dart';
import 'package:usque/services/engine_client.dart';
import 'package:usque/state/app_controller.dart';
import 'package:usque/state/scenarios_controller.dart';

import 'scenarios_test.dart' show SceneEngine;

class LostActivationEngine extends SceneEngine {
  String? operation;
  int activations = 0;
  Completer<void>? catalogGate;
  final catalogReadStarted = Completer<void>();
  @override
  Future<ProfileCatalog> importLegacyProfiles(
    List<UsqueProfile> profiles,
    String activeProfileId,
  ) async {
    final result = await super.importLegacyProfiles(profiles, activeProfileId);
    if (catalogGate != null) {
      if (!catalogReadStarted.isCompleted) catalogReadStarted.complete();
      await catalogGate!.future;
    }
    return result;
  }

  @override
  Future<EngineCapabilities?> getCapabilities() async =>
      const EngineCapabilities(
        connectionScenarios: true,
        networkSettingsApplication: true,
        automaticEndpoints: true,
        vpn: true,
        tlsFingerprints: TlsFingerprint.values,
      );
  @override
  Future<Map<String, Object?>> scenarioCommand(
    Map<String, Object?> request,
  ) async {
    if (request['action'] == 'activate') {
      activations++;
      storedActiveProfileId = 'account-b';
      operation = request['operation_id'] as String;
      throw TimeoutException('Committed, reply lost');
    }
    return {
      ...await super.scenarioCommand(request),
      if (operation != null) 'persisted_operation_id': operation,
    };
  }
}

Map<String, Object?> catalog({
  String account = 'account-a',
  List<Map<String, Object?>> scenes = const [],
  String? operation,
}) => {
  'scenarios': scenes,
  'current_account_id': account,
  'state': {'apply_status': 'deferred'},
  'persisted_operation_id': ?operation,
};

Map<String, Object?> scene(
  String id,
  String name,
  String binding,
  ScenarioSettings settings,
) => {
  'id': id,
  'name': name,
  'revision': 'revision-$id',
  'bound_account_id': binding,
  'settings': settings.toMap(),
};

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();

  for (final cancel in [false, true]) {
    test(
      cancel
          ? 'disconnect cancels a connection waiting for scene readback'
          : 'connection waits for the recovered account readback',
      () async {
        SharedPreferences.setMockInitialValues({});
        final engine = LostActivationEngine()..legacyProfilesImported = true;
        engine.storedProfiles = [
          engine.storedProfiles.single,
          engine.storedProfiles.single.copyWith(id: 'account-b', name: 'Bound'),
        ];
        final app = AppController(engine);
        addTearDown(app.dispose);
        await app.initialize();
        await app.scenarios.refresh();
        final preview = await app.scenarios.preview(
          app.scenarios.catalog!.scenarios.single,
        );
        final gate = engine.catalogGate = Completer<void>();
        addTearDown(() {
          if (!gate.isCompleted) gate.complete();
        });
        expect(await app.scenarios.activate(preview!), false);
        await engine.catalogReadStarted.future;
        final connecting = app.connectOrDisconnect();
        await Future<void>.delayed(Duration.zero);
        expect(engine.lastConnectedProfile, isNull);
        expect(app.scenarios.queryUnconfirmed, true);
        if (cancel) {
          await app.connectOrDisconnect();
          expect(engine.calls, contains('disconnect'));
        }
        gate.complete();
        await connecting;
        expect(engine.lastConnectedProfile?.id, cancel ? null : 'account-b');
        expect(engine.activations, 1);
      },
    );
  }

  test(
    'unresolved activation cannot pass the connection barrier or replay',
    () async {
      var activations = 0;
      final controller = ScenariosController((request) async {
        if (request['action'] == 'activate') {
          activations++;
          throw TimeoutException('Unknown commit');
        }
        return catalog();
      })..setSupported(true);
      addTearDown(controller.dispose);
      expect(
        await controller.activate(
          ScenarioPreview.fromMap({
            'preview': {'preview_token': 't'},
          }),
        ),
        false,
      );
      expect(await controller.synchronizeActivation(), false);
      expect(controller.pendingOperationId, isNotNull);
      expect(controller.unconfirmed, true);
      expect(activations, 1);
    },
  );

  test(
    'a newer activation receives its own readback after a pending recovery',
    () async {
      String? operation;
      var activations = 0, readbacks = 0;
      final started = Completer<void>(), release = Completer<void>();
      final controller = ScenariosController((request) async {
        if (request['action'] == 'activate') {
          operation = request['operation_id'] as String;
          if (++activations == 1) throw TimeoutException('First reply lost');
        }
        return {...catalog(operation: operation), 'persisted': true};
      })..setSupported(true);
      addTearDown(controller.dispose);
      addTearDown(() {
        if (!release.isCompleted) release.complete();
      });
      controller.afterActivation = () async {
        if (++readbacks == 1) {
          started.complete();
          await release.future;
        }
      };
      final preview = ScenarioPreview.fromMap({
        'preview': {'preview_token': 't'},
      });
      expect(await controller.activate(preview), false);
      await started.future;
      final second = controller.activate(preview);
      await Future<void>.delayed(Duration.zero);
      release.complete();
      expect(await second, true);
      expect(readbacks, 2);
      expect(controller.pendingOperationId, isNull);
      expect(activations, 2);
    },
  );

  test(
    'recovered bound account is the account used by the next connection',
    () async {
      SharedPreferences.setMockInitialValues({});
      final engine = LostActivationEngine();
      engine.legacyProfilesImported = true;
      engine.storedProfiles = [
        engine.storedProfiles.single,
        engine.storedProfiles.single.copyWith(
          id: 'account-b',
          name: 'Bound account',
        ),
      ];
      final app = AppController(engine);
      addTearDown(app.dispose);
      await app.initialize();
      await app.scenarios.refresh();
      final preview = await app.scenarios.preview(
        app.scenarios.catalog!.scenarios.single,
      );
      expect(await app.scenarios.activate(preview!), false);
      await app.scenarios.refresh();
      expect(app.activeProfileId, 'account-b');
      await app.connectOrDisconnect();
      expect(engine.lastConnectedProfile?.id, 'account-b');
      expect(engine.activations, 1);
    },
  );

  test(
    'readback failure is retried without replaying the activation',
    () async {
      var reads = 0, activations = 0;
      String? operation;
      var allowReadback = false;
      final controller = ScenariosController((request) async {
        if (request['action'] == 'activate') {
          activations++;
          operation = request['operation_id'] as String;
        }
        return {...catalog(operation: operation), 'persisted': true};
      })..setSupported(true);
      addTearDown(controller.dispose);
      controller.afterActivation = () async {
        reads++;
        if (!allowReadback) throw TimeoutException('Readback unavailable');
      };
      final preview = ScenarioPreview.fromMap({
        'preview': {'preview_token': 't'},
      });
      expect(await controller.activate(preview), true);
      await controller.refresh();
      expect(controller.queryUnconfirmed, true);
      expect(controller.pendingOperationId, operation);
      allowReadback = true;
      await controller.refresh();
      expect(controller.queryUnconfirmed, false);
      expect(controller.pendingOperationId, isNull);
      final confirmedReads = reads;
      await controller.refresh();
      expect(reads, confirmedReads);
      expect(activations, 1);
    },
  );

  test(
    'a query issued before a failed template mutation cannot reconcile it',
    () async {
      final old = Completer<Map<String, Object?>>();
      var queries = 0;
      final controller = ScenariosController((request) async {
        if (request['action'] == 'list') {
          return ++queries == 1 ? old.future : catalog();
        }
        throw TimeoutException('Delete reply lost');
      })..setSupported(true);
      addTearDown(controller.dispose);
      final query = controller.refresh();
      final settings = ScenarioSettings.fromProfile(
        UsqueProfile.defaultProfile(),
      );
      final entry = ConnectionScenario.fromMap(
        scene('scene-a', 'A', 'account-a', settings),
      );
      expect(await controller.delete(entry), false);
      old.complete(catalog());
      await query;
      expect(controller.unconfirmed, true);
      await controller.refresh();
      expect(controller.unconfirmed, false);
    },
  );

  test(
    'recovered activation receipt refreshes app account/settings views',
    () async {
      var afterActivationCalls = 0;
      String? committedOperation;
      final controller = ScenariosController((request) async {
        if (request['action'] == 'activate') {
          // Native switches account and commits, but its direct reply is lost.
          committedOperation = request['operation_id'] as String;
          throw const EngineException('SCENARIO_RESULT_UNKNOWN', 'Lost reply');
        }
        return catalog(
          account: committedOperation == null ? 'account-a' : 'account-b',
          operation: committedOperation,
        );
      })..setSupported(true);
      addTearDown(controller.dispose);
      controller.afterActivation = () async {
        afterActivationCalls++;
      };
      await controller.refresh();
      final preview = ScenarioPreview.fromMap({
        ...catalog(),
        'preview': {
          'preview_token': 'preview-token',
          'scenario_id': 'bound-scene',
          'scenario_revision': 'revision-bound-scene',
          'account_id': 'account-b',
          'previous_account_id': 'account-a',
          'account_changed': true,
        },
      });

      expect(await controller.activate(preview), false);
      await controller.refresh();
      expect(controller.catalog!.currentAccountId, 'account-b');
      expect(controller.pendingOperationId, isNull);
      expect(controller.unconfirmed, false);
      expect(
        afterActivationCalls,
        1,
        reason:
            'A recovered durable activation must run the same app-account '
            'readback callback as an immediate successful reply.',
      );
    },
  );

  test(
    'creation retry returns newly saved B instead of earlier unseen A',
    () async {
      final settings = ScenarioSettings.fromProfile(
        UsqueProfile.defaultProfile(),
      );
      final persistedScenes = <Map<String, Object?>>[];
      var saves = 0;
      final controller = ScenariosController((request) async {
        if (request['action'] == 'save') {
          saves++;
          persistedScenes.add(
            scene(
              saves == 1 ? 'scene-a' : 'scene-b',
              request['name'] as String,
              request['bound_account_id'] as String,
              ScenarioSettings(scenarioMap(request['settings'])),
            ),
          );
          if (saves == 1) {
            // A exists natively, but the editor has received no saved record.
            throw TimeoutException('First creation committed; reply lost');
          }
          return {
            ...catalog(scenes: persistedScenes),
            'persisted': true,
            'saved_scenario_id': persistedScenes.last['id'],
          };
        }
        return catalog(scenes: persistedScenes);
      })..setSupported(true);
      addTearDown(controller.dispose);
      await controller.refresh();
      expect(
        await controller.save(
          name: 'Original draft',
          boundAccountId: 'account-a',
          settings: settings,
        ),
        isNull,
      );
      expect(controller.catalog!.scenarios, isEmpty);

      // User changes the still-unsaved draft, then explicitly retries Save.
      final saved = await controller.save(
        name: 'Revised draft',
        boundAccountId: 'account-b',
        settings: settings,
      );
      expect(controller.catalog!.scenarios.map((s) => s.id), [
        'scene-a',
        'scene-b',
      ]);
      expect(saved, isNotNull);
      expect(
        saved!.id,
        'scene-b',
        reason:
            'The response contains both native creations; the editor must '
            'receive its newly committed Revised draft/account-b, not the older '
            'Original draft/account-a.',
      );
      expect(saved.name, 'Revised draft');
      expect(saved.boundAccountId, 'account-b');
    },
  );
  test(
    'later acknowledged activation clears prior template-save uncertainty',
    () async {
      final settings = ScenarioSettings.fromProfile(
        UsqueProfile.defaultProfile(),
      );
      String? operation;
      final controller = ScenariosController((request) async {
        if (request['action'] == 'save') {
          throw TimeoutException('Template save reply unavailable');
        }
        if (request['action'] == 'activate') {
          operation = request['operation_id'] as String;
          return {...catalog(operation: operation), 'persisted': true};
        }
        return catalog(operation: operation);
      })..setSupported(true);
      addTearDown(controller.dispose);
      await controller.refresh();
      expect(
        await controller.save(
          name: 'Uncertain template',
          boundAccountId: null,
          settings: settings,
        ),
        isNull,
      );
      expect(controller.unconfirmed, true);
      await controller.refresh();

      final preview = ScenarioPreview.fromMap({
        ...catalog(),
        'preview': {
          'preview_token': 'fresh-token',
          'scenario_id': 'existing-scene',
          'scenario_revision': 'existing-revision',
        },
      });
      expect(await controller.activate(preview), true);
      await controller.refresh();
      expect(controller.pendingOperationId, isNull);
      expect(
        controller.unconfirmed,
        false,
        reason:
            'A confirmed later activation and fresh catalog must not leave '
            'the selected scene permanently labeled Result not confirmed '
            'because of an unrelated earlier template-save failure.',
      );
    },
  );
}
