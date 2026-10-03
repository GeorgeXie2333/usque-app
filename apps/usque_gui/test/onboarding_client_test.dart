import 'dart:async';
import 'dart:io';

import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/services/control_codec.dart';
import 'package:usque/services/desktop_engine_client.dart';
import 'package:usque/services/desktop_engine_transport.dart';
import 'package:usque/services/engine_client.dart';

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  test(
    'initial identity appended wire fields preserve terms and license boundaries',
    () async {
      const codec = ControlCodec();
      final profile = UsqueProfile.defaultProfile();
      final state = ControlPayloadWriter()
        ..string(1, 'operation')
        ..string(2, profile.id)
        ..enumeration(3, 2);
      final response = codec.frame(
        (ControlPayloadWriter()
              ..string(1, 'r1')
              ..message(26, state.takeBytes()))
            .takeBytes(),
      );
      final frames = <Uint8List>[];
      final client = DesktopEngineClient.forTest(
        transport: DesktopEngineTransport.forTest(
          exchange: (frame) async {
            frames.add(Uint8List.fromList(frame));
            return response;
          },
          requestIdFactory: () => 'r1',
        ),
      );
      addTearDown(client.dispose);
      final result = await client.initializeIdentity(
        profile,
        operationId: 'operation',
        method: IdentityProvisioningMethod.registerWithLicense,
        licenseKey: 'test-license',
      );
      expect(result.phase, InitialIdentityPhase.pending);
      final provisioning = ControlPayloadWriter()
        ..enumeration(1, 3)
        ..boolean(3, true)
        ..string(4, Platform.localeName)
        ..string(6, 'test-license');
      final payload = ControlPayloadWriter()
        ..string(1, 'operation')
        ..string(2, profile.id)
        ..message(3, provisioning.takeBytes());
      expect(
        frames.single,
        codec.buildRequestFrame(
          requestId: 'r1',
          payloadField: 49,
          payload: payload.takeBytes(),
        ),
      );
      expect(requestTimeoutForPayload(49), const Duration(seconds: 90));
      expect(requestTimeoutForPayload(50), const Duration(seconds: 5));
    },
  );

  test('initial request timeout never replays registration', () async {
    var exchanges = 0;
    final client = DesktopEngineClient.forTest(
      transport: DesktopEngineTransport.forTest(
        exchange: (_) {
          exchanges++;
          return Completer<Uint8List>().future;
        },
        requestIdFactory: () => 'r1',
      ),
      requestTimeout: (_) => const Duration(milliseconds: 20),
    );
    addTearDown(client.dispose);
    await expectLater(
      client.initializeIdentity(
        UsqueProfile.defaultProfile(),
        operationId: 'operation',
        method: IdentityProvisioningMethod.register,
      ),
      throwsA(
        isA<EngineException>().having(
          (error) => error.code,
          'code',
          'ENGINE_REQUEST_TIMEOUT',
        ),
      ),
    );
    expect(exchanges, 1);
  });

  test(
    'old engine empty response cannot fall back to generic provisioning',
    () async {
      const codec = ControlCodec();
      var exchanges = 0;
      final client = DesktopEngineClient.forTest(
        transport: DesktopEngineTransport.forTest(
          exchange: (_) async {
            exchanges++;
            return codec.frame(
              (ControlPayloadWriter()..string(1, 'r1')).takeBytes(),
            );
          },
          requestIdFactory: () => 'r1',
        ),
      );
      addTearDown(client.dispose);
      await expectLater(
        client.getInitialIdentityState(UsqueProfile.defaultProfileId),
        throwsA(
          isA<EngineException>().having(
            (error) => error.code,
            'code',
            'INITIAL_IDENTITY_UNSUPPORTED',
          ),
        ),
      );
      expect(exchanges, 1);
    },
  );

  test(
    'Android permission adapter never issues a connection command',
    () async {
      const channel = MethodChannel('io.github.georgexie2333.usque/engine');
      final calls = <MethodCall>[];
      final messenger =
          TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger;
      messenger.setMockMethodCallHandler(channel, (call) async {
        calls.add(call);
        return {'vpnGranted': true, 'notification': 'notGranted'};
      });
      addTearDown(() => messenger.setMockMethodCallHandler(channel, null));
      final client = MethodChannelEngineClient();
      expect((await client.getOnboardingPermissions()).vpnGranted, isTrue);
      expect(
        (await client.prepareOnboardingPermissions()).notification,
        OnboardingNotificationPermission.notGranted,
      );
      expect(calls.map((call) => call.method), [
        'getOnboardingPermissions',
        'prepareOnboardingPermissions',
      ]);
    },
  );
}
