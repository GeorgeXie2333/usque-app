import 'dart:async';

import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:shared_preferences/shared_preferences.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/services/control_codec.dart';
import 'package:usque/services/desktop_engine_client.dart';
import 'package:usque/services/desktop_engine_transport.dart';
import 'package:usque/services/engine_client.dart';
import 'package:usque/state/app_controller.dart';
import 'package:usque/state/scenario_gate_leases.dart';

import 'chain_proxy_test.dart' show ChainEngine;
import 'vpngate_test.dart' show server;

class LeaseEngine extends ChainEngine {
  bool failQuery = false, failPrepare = false, failRelease = false;
  bool epochRequests = true;
  Completer<void>? holdPrepare;
  String? restartOnRelease;

  @override
  Future<EngineCapabilities?> getCapabilities() async => EngineCapabilities(
    networkSettingsApplication: true,
    vpnGateTcp: true,
    vpnGatePoolFavorites: true,
    vpnGateEpochRequests: epochRequests,
  );

  @override
  Future<NetworkSettingsState> getNetworkSettingsState() => failQuery
      ? Future.error(TimeoutException('Native owner not confirmed'))
      : super.getNetworkSettingsState();

  @override
  Future<void> vpnGateNode(VpnGateNodeRequest request) async {
    if (request.action == 'release' && restartOnRelease != null) {
      settingsEpoch = restartOnRelease!;
      restartOnRelease = null;
    }
    if (request.expectedSourceEpoch.isNotEmpty &&
        request.expectedSourceEpoch != settingsEpoch) {
      nodeRequests.add(request);
      throw const EngineException('VPN_GATE_OWNER_STALE', 'Stale owner');
    }
    if ((request.action == 'prepare' && failPrepare) ||
        (request.action == 'release' && failRelease)) {
      nodeRequests.add(request);
      throw TimeoutException('Native request not confirmed');
    }
    if (request.action == 'prepare') await holdPrepare?.future;
    await super.vpnGateNode(request);
  }
}

Future<(AppController, ScenarioGateLeases)> host(LeaseEngine engine) async {
  SharedPreferences.setMockInitialValues({});
  engine.legacyProfilesImported = true;
  final app = AppController(engine);
  await app.initialize();
  await app.networkSettings.refresh();
  addTearDown(app.dispose);
  return (app, ScenarioGateLeases.of(app));
}

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  final selection = const VpnGateSettings(
    enabled: true,
  ).copyWith(server: server);

  test(
    'hidden or suspended observations cannot certify a cached lease',
    () async {
      final engine = LeaseEngine();
      final (app, leases) = await host(engine);
      final original = ScenarioGateLease(selection, engine.settingsEpoch);
      app.setUiVisible(false);
      expect(
        await leases.ensure(
          selection,
          existing: original,
          isCurrent: () => true,
        ),
        isNull,
      );
      app.setUiVisible(true);
      app.setObservationVisible(false);
      engine.settingsEpoch = 'owner-b';
      expect(
        await leases.ensure(
          selection,
          existing: original,
          isCurrent: () => true,
        ),
        isNull,
      );
      expect(engine.nodeRequests, isEmpty);
      app.setObservationVisible(true);
      expect(
        (await leases.ensure(
          selection,
          existing: original,
          isCurrent: () => true,
        ))?.sourceEpoch,
        'owner-b',
      );
    },
  );

  test('a released lease cannot be reused to skip preparation', () async {
    final engine = LeaseEngine();
    final (_, leases) = await host(engine);
    final original = ScenarioGateLease(selection, engine.settingsEpoch);
    await leases.release(original);
    final renewed = await leases.ensure(
      selection,
      existing: original,
      isCurrent: () => true,
    );
    expect(renewed, isNot(same(original)));
    expect(engine.nodeRequests.map((r) => r.action), ['release', 'prepare']);
    await leases.release(renewed);
    expect(engine.nodeRequests.map((r) => r.action), [
      'release',
      'prepare',
      'release',
    ]);
  });

  test(
    'borrowed selection is reused only within its confirmed owner epoch',
    () async {
      final engine = LeaseEngine();
      final (_, leases) = await host(engine);
      final original = ScenarioGateLease(selection, engine.settingsEpoch);
      expect(
        await leases.ensure(
          selection,
          existing: original,
          isCurrent: () => true,
        ),
        same(original),
      );
      expect(engine.nodeRequests, isEmpty);
      engine.settingsEpoch = 'owner-b';
      final renewed = await leases.ensure(
        selection,
        existing: original,
        isCurrent: () => true,
      );
      expect(renewed?.sourceEpoch, 'owner-b');
      expect(engine.nodeRequests.single.action, 'prepare');
      expect(engine.nodeRequests.single.expectedSourceEpoch, 'owner-b');
      await leases.release(original);
      expect(engine.nodeRequests.length, 1);
      await leases.release(renewed);
      await leases.release(renewed);
      expect(engine.nodeRequests.map((r) => r.action), ['prepare', 'release']);
      expect(engine.nodeRequests.last.expectedSourceEpoch, 'owner-b');
    },
  );

  test(
    'failed refresh and unsupported old owners never authorize a cached lease',
    () async {
      final engine = LeaseEngine();
      final (_, leases) = await host(engine);
      final original = ScenarioGateLease(selection, engine.settingsEpoch);
      engine.failQuery = true;
      expect(
        await leases.ensure(
          selection,
          existing: original,
          isCurrent: () => true,
        ),
        isNull,
      );
      expect(engine.nodeRequests, isEmpty);
      engine.failQuery = false;
      expect(
        await leases.ensure(
          selection,
          existing: original,
          isCurrent: () => true,
        ),
        same(original),
      );
      final old = LeaseEngine()..epochRequests = false;
      final (_, oldLeases) = await host(old);
      expect(await oldLeases.ensure(selection, isCurrent: () => true), isNull);
      expect(old.nodeRequests, isEmpty);
    },
  );

  test(
    'a restart during preparation cannot certify or release a new-owner reference',
    () async {
      final engine = LeaseEngine()..holdPrepare = Completer<void>();
      final (app, leases) = await host(engine);
      final preparing = leases.ensure(selection, isCurrent: () => true);
      await Future<void>.delayed(Duration.zero);
      engine.settingsEpoch = 'owner-b';
      await app.networkSettings.refresh();
      engine.holdPrepare!.complete();
      expect(await preparing, isNull);
      expect(engine.nodeRequests.where((r) => r.action == 'release'), isEmpty);
      engine.holdPrepare = null;
      final retried = await leases.ensure(selection, isCurrent: () => true);
      expect(retried?.sourceEpoch, 'owner-b');
      expect(engine.nodeRequests.last.expectedSourceEpoch, 'owner-b');
    },
  );

  test(
    'failed preparation stays retryable and never returns a lease',
    () async {
      final engine = LeaseEngine()..failPrepare = true;
      final (_, leases) = await host(engine);
      expect(await leases.ensure(selection, isCurrent: () => true), isNull);
      expect(engine.nodeRequests.map((r) => r.action), ['prepare', 'cancel']);
      engine.failPrepare = false;
      expect(await leases.ensure(selection, isCurrent: () => true), isNotNull);
    },
  );

  test(
    'native epoch precondition closes restart between cleanup check and dispatch',
    () async {
      final engine = LeaseEngine();
      final (_, leases) = await host(engine);
      final original = ScenarioGateLease(selection, engine.settingsEpoch);
      engine.restartOnRelease = 'owner-b';
      await leases.release(original);
      expect(
        engine.nodeRequests.single.expectedSourceEpoch,
        original.sourceEpoch,
      );
      final renewed = await leases.ensure(
        selection,
        existing: original,
        isCurrent: () => true,
      );
      expect(renewed?.sourceEpoch, 'owner-b');
      expect(engine.nodeRequests.last.action, 'prepare');
      expect(engine.nodeRequests.last.expectedSourceEpoch, 'owner-b');
    },
  );

  test(
    'uncertain same-owner cleanup cannot race re-preparation of the same key',
    () async {
      final engine = LeaseEngine()..failRelease = true;
      final (_, leases) = await host(engine);
      final original = ScenarioGateLease(selection, engine.settingsEpoch);
      await leases.release(original);
      expect(await leases.ensure(selection, isCurrent: () => true), isNull);
      expect(engine.nodeRequests.map((r) => r.action), ['release']);
      engine.settingsEpoch = 'owner-b';
      engine.failRelease = false;
      expect(
        (await leases.ensure(selection, isCurrent: () => true))?.sourceEpoch,
        'owner-b',
      );
      expect(engine.nodeRequests.map((r) => r.action), ['release', 'prepare']);
    },
  );

  test(
    'VPN Gate owner capability and epoch request append to desktop wire and Android JSON',
    () async {
      final frames = <Uint8List>[];
      final client = DesktopEngineClient.forTest(
        transport: DesktopEngineTransport.forTest(
          requestIdFactory: () => 'r',
          exchange: (frame) async {
            frames.add(frame);
            final response = ControlPayloadWriter()..string(1, 'r');
            if (frames.length == 1) {
              response.message(
                15,
                (ControlPayloadWriter()..boolean(55, true)).takeBytes(),
              );
            }
            return const ControlCodec().frame(response.takeBytes());
          },
        ),
      );
      addTearDown(client.dispose);
      expect((await client.getCapabilities())?.vpnGateEpochRequests, isTrue);
      const request = VpnGateNodeRequest(
        operationId: 'op',
        action: 'release',
        serverId: 'node',
        configSha256: 'hash',
        expectedSourceEpoch: 'epoch',
      );
      await client.vpnGateNode(request);
      expect(frames.last.sublist(frames.last.length - 7), [
        0x32,
        5,
        101,
        112,
        111,
        99,
        104,
      ]);
      expect(request.toMap()['expected_source_epoch'], 'epoch');
      expect(
        const VpnGateNodeRequest(operationId: 'op', action: 'release').toMap(),
        isNot(contains('expected_source_epoch')),
      );
      expect(const EngineCapabilities().vpnGateEpochRequests, isFalse);
      expect(
        EngineCapabilities.fromMap({
          'vpn_gate_epoch_requests': true,
        }).vpnGateEpochRequests,
        isTrue,
      );
    },
  );
}
