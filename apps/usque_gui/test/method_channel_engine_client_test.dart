import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/services/engine_client.dart';

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();

  test('reconfigureActiveProfile does not follow up with connect', () async {
    const channel = MethodChannel('io.github.georgexie2333.usque/engine');
    final calls = <String>[];
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(channel, (MethodCall call) async {
          calls.add(call.method);
          return null;
        });
    addTearDown(() {
      TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
          .setMockMethodCallHandler(channel, null);
    });

    final client = MethodChannelEngineClient();
    await client.reconfigureActiveProfile(UsqueProfile.defaultProfile());
    expect(calls, <String>['reconfigureActiveProfile']);
  });
}
