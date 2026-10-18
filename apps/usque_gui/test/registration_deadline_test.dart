import 'dart:async';

import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/services/control_codec.dart';
import 'package:usque/services/desktop_engine_client.dart';
import 'package:usque/services/desktop_engine_transport.dart';
import 'package:usque/services/engine_client.dart';

void main() {
  for (final linux in [false, true]) {
    for (final (name, field, networkDeadline) in [
      ('provision identity', 23, 240),
      ('create account', 26, 180),
      ('replace license', 29, 240),
      ('unbind license', 30, 240),
      ('initialize identity', 49, 180),
    ]) {
      final deadline = networkDeadline + (linux ? 360 : 0);
      for (final completes in [true, false]) {
        testWidgets(
          '${linux ? 'Linux' : 'Windows'} $name ${completes ? 'accepts slow fallback completion' : 'times out without replaying registration'}',
          (tester) async {
            final response = Completer<Uint8List>();
            var exchanges = 0;
            final client = DesktopEngineClient.forTest(
              requestTimeout: linux
                  ? linuxRequestTimeoutForPayload
                  : requestTimeoutForPayload,
              transport: DesktopEngineTransport.forTest(
                requestIdFactory: () => 'registration',
                exchange: (_) {
                  exchanges++;
                  return response.future;
                },
              ),
            );
            addTearDown(client.dispose);
            var succeeded = false;
            Object? failure;
            final operation = _startRegistration(client, field).then<void>(
              (_) => succeeded = true,
              onError: (Object error) => failure = error,
            );
            await tester.pump();
            // Four HTTP steps can each spend 20 seconds at both hosts. Identity
            // replacement can also wait for retired-credential cleanup.
            await tester.pump(Duration(seconds: deadline - 10));
            expect(failure, isNull);
            expect(succeeded, isFalse);
            expect(exchanges, 1);
            if (!completes) {
              await tester.pump(const Duration(seconds: 10));
              await operation;
              expect(
                failure,
                isA<EngineException>().having(
                  (error) => error.code,
                  'code',
                  'ENGINE_REQUEST_TIMEOUT',
                ),
              );
            }
            response.complete(_registrationResponse(field));
            await tester.pump();
            await operation;
            expect(succeeded, completes);
            if (completes) expect(failure, isNull);
            expect(exchanges, 1);
          },
        );
      }
    }
  }

  for (final completes in [true, false]) {
    testWidgets(
      'Android initialization ${completes ? 'accepts slow fallback completion' : 'times out without replaying registration'}',
      (tester) async {
        const channel = MethodChannel('io.github.georgexie2333.usque/engine');
        final messenger =
            TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger;
        final response = Completer<Map<Object?, Object?>>();
        var calls = 0;
        messenger.setMockMethodCallHandler(channel, (call) {
          expect(call.method, 'initializeIdentity');
          calls++;
          return response.future;
        });
        addTearDown(() => messenger.setMockMethodCallHandler(channel, null));
        var succeeded = false;
        Object? failure;
        final operation = MethodChannelEngineClient()
            .initializeIdentity(
              UsqueProfile.defaultProfile(),
              operationId: 'operation',
              method: IdentityProvisioningMethod.registerWithLicense,
              licenseKey: 'test-license',
            )
            .then<void>(
              (_) => succeeded = true,
              onError: (Object error) => failure = error,
            );
        await tester.pump();
        await tester.pump(const Duration(seconds: 170));
        expect(failure, isNull);
        expect(succeeded, isFalse);
        expect(calls, 1);
        if (!completes) {
          await tester.pump(const Duration(seconds: 10));
          await operation;
          expect(failure, isA<TimeoutException>());
        }
        response.complete({
          'operation_id': 'operation',
          'profile_id': UsqueProfile.defaultProfileId,
          'phase': 'completed',
        });
        await tester.pump();
        await operation;
        expect(succeeded, completes);
        if (completes) expect(failure, isNull);
        expect(calls, 1);
      },
    );
  }
}

Future<void> _startRegistration(DesktopEngineClient client, int field) {
  final profile = UsqueProfile.defaultProfile();
  return switch (field) {
    23 => client.provisionIdentity(
      profile,
      method: IdentityProvisioningMethod.registerWithLicense,
      licenseKey: 'test-license',
    ),
    26 =>
      client
          .createProfileWithIdentity(
            profile,
            method: IdentityProvisioningMethod.registerWithLicense,
            licenseKey: 'test-license',
          )
          .then<void>((_) {}),
    29 => client.updateLicenseKey(profile.id, 'test-license'),
    30 => client.unbindLicenseKey(profile.id),
    49 =>
      client
          .initializeIdentity(
            profile,
            operationId: 'operation',
            method: IdentityProvisioningMethod.registerWithLicense,
            licenseKey: 'test-license',
          )
          .then<void>((_) {}),
    _ => throw ArgumentError.value(field),
  };
}

Uint8List _registrationResponse(int field) {
  const codec = ControlCodec();
  final profile = UsqueProfile.defaultProfile();
  final response = ControlPayloadWriter()..string(1, 'registration');
  if (field == 26) {
    response.message(
      12,
      (ControlPayloadWriter()
            ..message(1, codec.encodeProfile(profile))
            ..string(2, profile.id))
          .takeBytes(),
    );
  } else if (field == 49) {
    response.message(
      26,
      (ControlPayloadWriter()
            ..string(1, 'operation')
            ..string(2, profile.id)
            ..enumeration(3, 3))
          .takeBytes(),
    );
  } else {
    response.message(10, Uint8List(0));
  }
  return codec.frame(response.takeBytes());
}
