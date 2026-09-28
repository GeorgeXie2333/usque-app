import 'dart:async';
import 'dart:io';
import 'dart:typed_data';

import 'package:flutter_test/flutter_test.dart';
import 'package:shared_preferences/shared_preferences.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/services/control_codec.dart';
import 'package:usque/services/desktop_engine_client.dart';
import 'package:usque/services/desktop_engine_transport.dart';
import 'package:usque/services/engine_client.dart';
import 'package:usque/state/app_controller.dart';

import 'app_test.dart' show FakeEngineClient;

class DelayedBootstrap extends FakeEngineClient {
  final capabilitiesRequested = Completer<void>();
  final capabilitiesReply = Completer<EngineCapabilities?>();
  final statusReply = Completer<EngineSnapshot>();
  @override
  Future<EngineCapabilities?> getCapabilities() {
    if (!capabilitiesRequested.isCompleted) capabilitiesRequested.complete();
    return capabilitiesReply.future;
  }

  @override
  Future<EngineSnapshot> snapshot() => statusReply.future;
}

class FirstCatalogFailure extends FakeEngineClient {
  int loads = 0;
  @override
  Future<ProfileCatalog> importLegacyProfiles(
    List<UsqueProfile> profiles,
    String activeId,
  ) {
    if (++loads == 1) {
      throw const EngineException('ENGINE_UNAVAILABLE', 'Starting');
    }
    return super.importLegacyProfiles(profiles, activeId);
  }
}

class HeldIdentityMutation extends FakeEngineClient {
  final entered = Completer<void>();
  final release = Completer<void>();
  Future<void> hold() async {
    entered.complete();
    await release.future;
  }

  @override
  Future<void> updateLicenseKey(String id, String license) => hold();
  @override
  Future<void> unbindLicenseKey(String id) => hold();
  @override
  Future<void> provisionIdentity(
    UsqueProfile profile, {
    required IdentityProvisioningMethod method,
    String? licenseKey,
    String? teamName,
    String? callbackUri,
  }) => hold();
}

class LiveTransport extends DesktopEngineTransport {
  LiveTransport(Future<Uint8List> Function(Uint8List) exchange)
    : super.forTest(exchange: exchange, requestIdFactory: () => 'r');
  @override
  bool get hasLiveProcess => true;
}

Uint8List response() => const ControlCodec().frame(
  (ControlPayloadWriter()
        ..string(1, 'r')
        ..message(11, (ControlPayloadWriter()..enumeration(1, 1)).takeBytes()))
      .takeBytes(),
);

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  setUp(
    () => SharedPreferences.setMockInitialValues({
      'onboarding_complete': true,
      'update_checks_enabled': false,
    }),
  );

  test('startup waits for both capabilities and the initial status', () async {
    for (final connected in [false, true]) {
      final engine = DelayedBootstrap()
        ..legacyProfilesImported = true
        ..storedProfiles = [
          UsqueProfile.defaultProfile().copyWith(
            autoConnect: true,
            dataPlane: DataPlaneMode.l4Proxy,
          ),
        ];
      final app = AppController(engine);
      addTearDown(app.dispose);
      final initialized = app.initialize();
      await engine.capabilitiesRequested.future;
      expect(engine.calls, isNot(contains('connect')));
      engine.capabilitiesReply.complete(
        const EngineCapabilities(
          l4Tcp: true,
          l4TunTcp: true,
          l4DnsConversion: true,
        ),
      );
      await Future<void>.delayed(Duration.zero);
      expect(engine.calls, isNot(contains('connect')));
      engine.statusReply.complete(
        EngineSnapshot(
          phase: connected
              ? ConnectionPhase.connected
              : ConnectionPhase.disconnected,
        ),
      );
      await initialized;
      expect(
        engine.calls.where((c) => c == 'connect').length,
        connected ? 0 : 1,
      );
    }
  });

  test(
    'initial catalogue failure is recovered before using a placeholder',
    () async {
      final engine = FirstCatalogFailure()..legacyProfilesImported = true;
      final profile = UsqueProfile.defaultProfile().copyWith(
        id: 'saved-account',
        name: 'Saved',
      );
      engine.storedProfiles = [profile];
      engine.storedActiveProfileId = profile.id;
      final app = AppController(engine);
      addTearDown(app.dispose);
      await app.initialize();
      expect(engine.loads, 2);
      expect(app.activeProfile.id, profile.id);
      expect(app.lastError, isNull);
    },
  );

  test(
    'identity completion cannot reconnect after cancellation or account change',
    () async {
      for (final action in ['license', 'unbind', 'provision', 'switch']) {
        final engine = HeldIdentityMutation()..legacyProfilesImported = true;
        final a = UsqueProfile.defaultProfile();
        engine.storedProfiles = [a, a.copyWith(id: 'b', name: 'B')];
        final app = AppController(engine);
        addTearDown(app.dispose);
        await app.initialize();
        app.snapshot = engine.current = const EngineSnapshot(
          phase: ConnectionPhase.connected,
        );
        final operation = switch (action) {
          'unbind' => app.unbindLicenseKey(a.id),
          'provision' => app.provisionProfileIdentity(
            a,
            method: IdentityProvisioningMethod.register,
          ),
          _ => app.updateLicenseKey(a.id, 'test-license'),
        };
        await engine.entered.future;
        if (action == 'switch') {
          app.setActiveProfile('b');
          await app.flushProfileWrites();
        } else {
          app.cancelIdentityFlow(a.id);
        }
        engine.release.complete();
        await operation;
        expect(engine.calls, isNot(contains('connect')), reason: action);
      }
    },
  );

  test(
    'an ambiguous mutation is not replayed while the process lives',
    () async {
      var count = 0;
      final client = DesktopEngineClient.forTest(
        transport: LiveTransport((_) async {
          count++;
          throw const SocketException('reply lost after commit');
        }),
      );
      addTearDown(client.dispose);
      await expectLater(
        client.upsertProfile(UsqueProfile.defaultProfile()),
        throwsA(isA<EngineException>()),
      );
      expect(count, 1);
    },
  );

  test('safe status reads may reconnect after a broken transport', () async {
    var count = 0;
    final client = DesktopEngineClient.forTest(
      transport: LiveTransport((_) async {
        if (++count == 1) throw const SocketException('transient read failure');
        return response();
      }),
    );
    addTearDown(client.dispose);
    expect((await client.snapshot()).phase, ConnectionPhase.disconnected);
    expect(count, 2);
  });

  test(
    'disconnect invalidates connects and retries waiting in the queue',
    () async {
      for (final retry in [false, true]) {
        final held = Completer<void>();
        final entered = Completer<void>();
        var calls = 0;
        final client = DesktopEngineClient.forTest(
          transport: LiveTransport((_) async {
            if (++calls == 1) {
              entered.complete();
              await held.future;
            }
            return response();
          }),
        );
        addTearDown(client.dispose);
        final reading = client.snapshot();
        await entered.future;
        final connecting = retry
            ? client.retry()
            : client.connect(UsqueProfile.defaultProfile());
        final cancelled = expectLater(
          connecting,
          throwsA(
            isA<EngineException>().having(
              (e) => e.code,
              'code',
              'ENGINE_REQUEST_CANCELLED',
            ),
          ),
        );
        await client.disconnect();
        held.complete();
        await reading;
        await cancelled;
        expect(calls, 2);
      }
    },
  );

  test(
    'disconnect during startup prevents a delayed connect from being sent',
    () async {
      final started = Completer<void>();
      var exchanges = 0;
      final client = DesktopEngineClient.forTest(
        transport: DesktopEngineTransport.forTest(
          ensureStarted: () => started.future,
          requestIdFactory: () => 'r',
          exchange: (_) async {
            exchanges++;
            return response();
          },
        ),
      );
      addTearDown(client.dispose);
      final connect = client.connect(UsqueProfile.defaultProfile());
      final cancelled = expectLater(connect, throwsA(isA<EngineException>()));
      await Future<void>.delayed(Duration.zero);
      final disconnect = client.disconnect();
      started.complete();
      await disconnect;
      await cancelled;
      expect(exchanges, 1);
    },
  );
}
