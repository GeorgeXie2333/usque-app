import 'dart:async';

import 'package:flutter_test/flutter_test.dart';
import 'package:shared_preferences/shared_preferences.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/models/scenarios.dart';
import 'package:usque/services/engine_client.dart';
import 'package:usque/state/app_controller.dart';
import 'package:usque/state/network_settings_controller.dart';
import 'package:usque/state/scenario_gate_leases.dart';
import 'package:usque/state/scenarios_controller.dart';

import 'network_settings_test.dart' show SettingsEngine;
import 'scenario_gate_leases_test.dart' show LeaseEngine, host;
import 'scenarios_test.dart' show fixture;
import 'vpngate_test.dart' show server;

NetworkSettingsState storedSettings({int mtu = 1400}) =>
    NetworkSettingsState.fromMap({
      'runtime_observed': false,
      'stored_profile': UsqueProfile.defaultProfile()
          .copyWith(mtu: mtu)
          .toMap(),
    });

Map<String, Object?> storedCatalog() {
  final live = fixture();
  return {
    'runtime_observed': false,
    'scenarios': live['scenarios'],
    'current_settings': live['current_settings'],
    'current_account_id': live['current_account_id'],
    'state': {'apply_status': 'unknown'},
  };
}

class OwnerSettingsEngine extends SettingsEngine
    implements ControlOperationClient {
  final owners = <String>{};
  var begins = 0, ends = 0;
  bool failEnd = false;

  @override
  Future<String> beginControlOperation() async {
    final token = 'owner-${++begins}';
    owners.add(token);
    return token;
  }

  @override
  Future<void> endControlOperation(String token) async {
    ends++;
    if (failEnd) throw TimeoutException('Local channel unavailable');
    owners.remove(token);
  }

  @override
  Future<NetworkSettingsState> getNetworkSettingsState() async =>
      owners.isEmpty ? storedSettings() : super.getNetworkSettingsState();
}

class OwnerLeaseEngine extends LeaseEngine implements ControlOperationClient {
  final owners = <String>{};
  var begins = 0, ends = 0;
  bool failEnd = false;

  @override
  Future<String> beginControlOperation() async {
    final token = 'owner-${++begins}';
    owners.add(token);
    return token;
  }

  @override
  Future<void> endControlOperation(String token) async {
    ends++;
    if (failEnd) throw TimeoutException('Local channel unavailable');
    owners.remove(token);
  }

  @override
  Future<NetworkSettingsState> getNetworkSettingsState() async =>
      owners.isEmpty ? storedSettings() : super.getNetworkSettingsState();
}

class DelayedOwnerLeaseEngine extends OwnerLeaseEngine {
  Completer<NetworkSettingsState>? pendingPassive;

  @override
  Future<NetworkSettingsState> getNetworkSettingsState() {
    final pending = pendingPassive;
    if (pending != null) {
      pendingPassive = null;
      return pending.future;
    }
    return super.getNetworkSettingsState();
  }
}

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();

  test(
    'draft cleanup preserves its result when owner release is delayed',
    () async {
      SharedPreferences.setMockInitialValues({});
      final engine = OwnerLeaseEngine()..legacyProfilesImported = true;
      final app = AppController(engine);
      await app.initialize();
      final leases = ScenarioGateLeases.of(app);
      final selection = const VpnGateSettings(
        enabled: true,
      ).copyWith(server: server);
      final lease = await leases.ensure(
        selection,
        isCurrent: () => true,
        prepare: (_) async => true,
      );
      expect(lease, isNotNull);
      engine.failEnd = true;
      await leases.release(lease);
      expect(engine.owners, isNotEmpty);
      final releases = engine.nodeRequests
          .where((call) => call.action == 'release')
          .length;
      engine.failEnd = false;
      app.dispose();
      await Future<void>.delayed(Duration.zero);
      expect(engine.owners, isEmpty);
      expect(
        engine.nodeRequests.where((call) => call.action == 'release'),
        hasLength(releases),
      );
    },
  );

  test('explicit draft waits past an older passive settings query', () async {
    final engine = DelayedOwnerLeaseEngine();
    final (app, leases) = await host(engine);
    final oldRead = engine.pendingPassive = Completer<NetworkSettingsState>();
    final passive = app.networkSettings.refresh();
    final selection = const VpnGateSettings(
      enabled: true,
    ).copyWith(server: server);
    var prepared = false;
    final preparing = leases.ensure(
      selection,
      isCurrent: () => true,
      prepare: (_) async {
        expect(engine.owners, isNotEmpty);
        return prepared = true;
      },
    );
    await Future<void>.delayed(Duration.zero);
    expect(engine.owners, isNotEmpty);
    oldRead.complete(storedSettings());
    await passive;
    final lease = await preparing;
    expect(prepared, true);
    expect(lease, isNotNull);
    expect(engine.owners, hasLength(1));
    await leases.release(lease);
    expect(engine.owners, isEmpty);
    expect(engine.begins, engine.ends);
  });

  test('stored settings have no runtime authority even with extra claims', () {
    final value = NetworkSettingsState.fromMap({
      'runtime_observed': false,
      'source_epoch': 'untrusted-owner',
      'sequence': 90,
      'session_id': 'old-session',
      'apply_status': 'applied',
      'operation_id': 'unconfirmed-write',
      'persisted': true,
      'persisted_operation_ids': ['unconfirmed-write'],
      'applied_profile': UsqueProfile.defaultProfile().toMap(),
      'stored_profile': UsqueProfile.defaultProfile()
          .copyWith(mtu: 1400)
          .toMap(),
    });
    expect(value.runtimeObserved, false);
    expect(value.sourceEpoch, isEmpty);
    expect(value.appliedProfile, isNull);
    expect(value.sessionId, isNull);
    expect(value.operationId, isNull);
    expect(value.persisted, isNull);
    expect(value.persistedOperationIds, isEmpty);
    expect(value.status, NetworkSettingsApplyStatus.unknown);
    expect(value.storedProfile!.mtu, 1400);
    expect(() => NetworkSettingsState.fromMap({}), throwsFormatException);
  });

  test('stored reads do not retire the last real epoch or reset ordering', () {
    final controller = NetworkSettingsController(SettingsEngine());
    addTearDown(controller.dispose);
    controller.accept(
      const NetworkSettingsState(sourceEpoch: 'a', sequence: 5),
    );
    controller.accept(storedSettings());
    controller.accept(
      const NetworkSettingsState(sourceEpoch: 'a', sequence: 4),
    );
    expect(controller.state!.runtimeObserved, false);
    controller.accept(
      const NetworkSettingsState(sourceEpoch: 'a', sequence: 6),
    );
    expect(controller.state!.sourceEpoch, 'a');
    expect(controller.state!.runtimeObserved, true);
    controller.accept(
      const NetworkSettingsState(sourceEpoch: 'b', sequence: 1),
    );
    controller.accept(storedSettings());
    controller.accept(
      const NetworkSettingsState(sourceEpoch: 'a', sequence: 7),
    );
    expect(controller.state!.runtimeObserved, false);
    controller.accept(
      const NetworkSettingsState(sourceEpoch: 'b', sequence: 1),
    );
    expect(controller.state!.sourceEpoch, 'b');
    expect(controller.state!.runtimeObserved, true);
  });

  test(
    'a delayed stored read cannot replace a newer live observation',
    () async {
      final reply = Completer<NetworkSettingsState>();
      final engine = SettingsEngine()..queryResponse = reply;
      final controller = NetworkSettingsController(engine)..supported = true;
      addTearDown(controller.dispose);
      final reading = controller.refresh();
      controller.accept(
        const NetworkSettingsState(sourceEpoch: 'live', sequence: 1),
      );
      reply.complete(storedSettings());
      await reading;
      expect(controller.state!.runtimeObserved, true);
      expect(controller.state!.sourceEpoch, 'live');
    },
  );

  test('stored configuration cannot acknowledge an uncertain save', () async {
    final engine = SettingsEngine()
      ..settingsState = storedSettings()
      ..saveFailure = TimeoutException('Lost save reply');
    final controller = NetworkSettingsController(engine)..supported = true;
    addTearDown(controller.dispose);
    expect(
      await controller.save(UsqueProfile.defaultProfile().copyWith(mtu: 1400), [
        'mtu',
      ]),
      false,
    );
    expect(controller.unconfirmed, true);
    await controller.refresh();
    expect(controller.unconfirmed, true);
    expect(engine.submitted, hasLength(1));
    controller.accept(
      NetworkSettingsState(
        sourceEpoch: 'live',
        sequence: 1,
        persistedOperationIds: [engine.submitted.single],
      ),
    );
    expect(controller.unconfirmed, false);
    expect(engine.submitted, hasLength(1));
  });

  test(
    'stored fields remain editable and an explicit save can succeed',
    () async {
      final engine = SettingsEngine();
      final controller = NetworkSettingsController(engine)..supported = true;
      addTearDown(controller.dispose);
      controller.accept(storedSettings());
      expect(controller.unconfirmed, false);
      expect(
        await controller.save(
          UsqueProfile.defaultProfile().copyWith(mtu: 1450),
          ['mtu'],
        ),
        true,
      );
      expect(controller.state!.runtimeObserved, true);
      expect(engine.submitted, hasLength(1));
    },
  );

  test('only explicit confirmation obtains a live control owner', () async {
    final engine = OwnerSettingsEngine();
    final controller = NetworkSettingsController(engine)..supported = true;
    addTearDown(controller.dispose);
    await controller.refresh();
    expect(engine.begins, 0);
    expect(controller.state!.runtimeObserved, false);
    await controller.refresh(requireRuntime: true);
    expect(engine.begins, 1);
    expect(engine.ends, 1);
    expect(engine.owners, isEmpty);
    expect(controller.state!.runtimeObserved, true);
  });

  test(
    'manual confirmation retains a failed owner release until disposal',
    () async {
      final engine = OwnerSettingsEngine()..failEnd = true;
      final controller = NetworkSettingsController(engine)..supported = true;
      await controller.refresh(requireRuntime: true);
      expect(controller.state!.runtimeObserved, true);
      expect(engine.owners, hasLength(1));
      engine.failEnd = false;
      controller.dispose();
      await Future<void>.delayed(Duration.zero);
      expect(engine.owners, isEmpty);
      expect(engine.ends, 2);
    },
  );

  test(
    'stored scenes cannot certify runtime or settle an uncertain save',
    () async {
      var response = storedCatalog();
      final controller = ScenariosController((request) async {
        if (request['action'] == 'save') {
          throw TimeoutException('Lost template reply');
        }
        return response;
      })..setSupported(true);
      addTearDown(controller.dispose);
      await controller.refresh();
      expect(controller.catalog!.scenarios, hasLength(1));
      expect(controller.runtimeConfirmed, false);
      expect(
        await controller.save(
          name: 'Changed',
          boundAccountId: null,
          settings: ScenarioSettings.fromProfile(UsqueProfile.defaultProfile()),
        ),
        isNull,
      );
      expect(controller.unconfirmed, true);
      await controller.refresh();
      expect(controller.unconfirmed, true);
      response = fixture();
      await controller.refresh();
      expect(controller.unconfirmed, false);
      expect(controller.runtimeConfirmed, true);
    },
  );

  test('stored scene model ignores runtime and rollback claims', () {
    final catalog = ScenarioCatalog.fromMap({
      ...fixture(
        applied: true,
        status: 'applied',
        operation: 'old',
        receipt: true,
      ),
      'runtime_observed': false,
      'can_restore': true,
      'applied_scene_confirmed': true,
      'applied_account_id': 'old-account',
    });
    expect(catalog.runtimeObserved, false);
    expect(catalog.sourceEpoch, isEmpty);
    expect(catalog.observationSessionId, isNull);
    expect(catalog.appliedAccountId, isNull);
    expect(catalog.appliedSceneConfirmed, false);
    expect(catalog.canRestore, false);
    expect(catalog.lastOperationId, isNull);
    expect(catalog.applyStatus, 'unknown');
  });

  test(
    'a draft holds its control owner through preparation and release',
    () async {
      final engine = OwnerLeaseEngine();
      final (app, leases) = await host(engine);
      final selection = const VpnGateSettings(
        enabled: true,
      ).copyWith(server: server);
      expect(app.networkSettings.state!.runtimeObserved, false);
      final lease = await leases.ensure(
        selection,
        isCurrent: () => true,
        prepare: (epoch) async {
          expect(engine.owners, hasLength(1));
          expect(epoch, engine.settingsEpoch);
          return true;
        },
      );
      expect(lease, isNotNull);
      expect(engine.owners, hasLength(1));
      expect(
        await leases.ensure(selection, existing: lease, isCurrent: () => true),
        same(lease),
      );
      expect(engine.owners, hasLength(1));
      await leases.release(lease);
      await leases.release(lease);
      expect(engine.owners, isEmpty);
      expect(engine.ends, engine.begins);
    },
  );

  test('hidden and failed preparations leave no control owner', () async {
    final engine = OwnerLeaseEngine();
    final (app, leases) = await host(engine);
    final selection = const VpnGateSettings(
      enabled: true,
    ).copyWith(server: server);
    app.setUiVisible(false);
    expect(await leases.ensure(selection, isCurrent: () => true), isNull);
    expect(engine.begins, 0);
    app.setUiVisible(true);
    expect(
      await leases.ensure(
        selection,
        isCurrent: () => true,
        prepare: (_) async => false,
      ),
      isNull,
    );
    expect(engine.owners, isEmpty);
    expect(engine.begins, engine.ends);
  });

  test(
    'an unacknowledged owner release remains retryable at disposal',
    () async {
      final engine = OwnerSettingsEngine()..failEnd = true;
      final app = AppController(engine);
      final release = (await app.acquireControlOperation())!;
      await expectLater(release(), throwsA(isA<TimeoutException>()));
      expect(engine.owners, hasLength(1));
      engine.failEnd = false;
      app.dispose();
      await Future<void>.delayed(Duration.zero);
      expect(engine.owners, isEmpty);
      expect(engine.ends, 2);
      await release();
      expect(engine.ends, 2);
    },
  );
}
