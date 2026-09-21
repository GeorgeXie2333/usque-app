import 'dart:async';
import 'dart:typed_data';

import 'package:flutter_test/flutter_test.dart';
import 'package:shared_preferences/shared_preferences.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/models/network_settings.dart';
import 'package:usque/services/control_codec.dart';
import 'package:usque/services/engine_client.dart' show EngineException;
import 'package:usque/state/app_controller.dart';

import 'app_test.dart' show FakeEngineClient;

class FailingAccountSelection extends FakeEngineClient {
  @override
  Future<void> setActiveProfile(String profileId) async {
    throw const EngineException('PROFILE_SAVE_FAILED', 'Selection failed');
  }
}

class HeldAccountSave extends FakeEngineClient {
  final entered = Completer<void>();
  final release = Completer<void>();

  @override
  Future<NetworkSettingsState> saveNetworkSettings(
    String operationId,
    String accountId,
    UsqueProfile values,
    List<String> fields,
  ) async {
    final result = await super.saveNetworkSettings(
      operationId,
      accountId,
      values,
      fields,
    );
    entered.complete();
    await release.future;
    return result;
  }
}

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  setUp(
    () => SharedPreferences.setMockInitialValues({
      'update_checks_enabled': false,
    }),
  );

  test('failed earlier mutation preserves the later rename intent', () async {
    final a = UsqueProfile.defaultProfile();
    final engine = FailingAccountSelection()
      ..legacyProfilesImported = true
      ..storedProfiles = [a, a.copyWith(id: 'b', name: 'B')];
    final app = AppController(engine);
    addTearDown(app.dispose);
    await app.initialize();
    await Future<void>.delayed(Duration.zero);
    app.setActiveProfile('b');
    app.renameProfile(a.id, 'New name');
    await app.flushProfileWrites();
    expect(engine.storedProfiles.first.name, 'New name');
    expect(app.profiles.first.name, 'New name');
    expect(app.activeProfileId, a.id);
    expect(app.lastError, isNotNull);
  });

  test('disabled Kill Switch survives a proto3 omitted bool', () {
    const codec = ControlCodec();
    final profile = UsqueProfile.defaultProfile().copyWith(killSwitch: false);
    final catalog = ControlPayloadWriter()
      ..message(1, codec.encodeProfile(profile))
      ..string(2, profile.id);
    final response = ControlPayloadWriter()
      ..string(1, 'audit')
      ..message(12, catalog.takeBytes());
    expect(
      debugDecodeProfileCatalogFrame(
        codec.frame(response.takeBytes()),
        'audit',
      ).profiles.single.killSwitch,
      false,
    );
  });

  test('Rust omitted-false fixture decodes without product defaults', () {
    const codec = ControlCodec();
    final payload = ControlPayloadWriter()
      ..string(1, 'audit')
      ..message(
        12,
        (ControlPayloadWriter()
              ..message(1, Uint8List.fromList([10, 1, 112, 18, 1, 88]))
              ..string(2, 'p'))
            .takeBytes(),
      );
    expect(
      debugDecodeProfileCatalogFrame(
        codec.frame(payload.takeBytes()),
        'audit',
      ).profiles.single.killSwitch,
      false,
    );
  });

  test(
    'second-account credentials and rename preserve confirmed network',
    () async {
      final a = UsqueProfile.defaultProfile();
      final b = a.copyWith(id: 'b', name: 'B');
      final engine = FakeEngineClient()
        ..legacyProfilesImported = true
        ..storedProfiles = [a, b]
        ..storedActiveProfileId = b.id;
      final app = AppController(engine);
      addTearDown(app.dispose);
      await app.initialize();
      await Future<void>.delayed(Duration.zero);
      expect(
        await app.saveNetwork(app.activeProfile.copyWith(allowLan: false)),
        true,
      );
      expect(
        await app.updateProxyAuth(username: 'audit', password: 'test-only'),
        true,
      );
      expect(app.sharedNetwork.allowLan, false);
      app.renameProfile(b.id, 'Renamed');
      await app.flushProfileWrites();
      expect(engine.storedProfiles.first.allowLan, false);
      expect(engine.storedProfiles.last.name, 'Renamed');
    },
  );

  test('delete during save retains global acknowledgement', () async {
    final a = UsqueProfile.defaultProfile();
    final engine = HeldAccountSave()
      ..legacyProfilesImported = true
      ..storedProfiles = [a, a.copyWith(id: 'b', name: 'B')];
    final app = AppController(engine);
    addTearDown(app.dispose);
    await app.initialize();
    await Future<void>.delayed(Duration.zero);
    final saving = app.saveNetwork(app.activeProfile.copyWith(mtu: 1400));
    await engine.entered.future;
    app.deleteProfile(a.id);
    engine.release.complete();
    expect(await saving, true);
    await app.flushProfileWrites();
    expect(app.activeProfile.mtu, 1400);
    expect(engine.storedProfiles.single.mtu, 1400);
  });

  test(
    'queued rename then delete does not restore a phantom account',
    () async {
      final a = UsqueProfile.defaultProfile();
      final engine = FakeEngineClient()
        ..legacyProfilesImported = true
        ..storedProfiles = [a, a.copyWith(id: 'b', name: 'B')];
      final app = AppController(engine);
      addTearDown(app.dispose);
      await app.initialize();
      await Future<void>.delayed(Duration.zero);
      app.renameProfile(a.id, 'Renamed');
      app.deleteProfile(a.id);
      await app.flushProfileWrites();
      expect(app.profiles.map((p) => p.id), ['b']);
      expect(engine.storedProfiles.map((p) => p.id), ['b']);
    },
  );
}
