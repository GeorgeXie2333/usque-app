import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/models/network_settings.dart';
import 'package:usque/screens/advanced_settings_screen.dart';
import 'package:usque/services/control_codec.dart';
import 'package:usque/state/app_controller.dart';
import 'package:usque/widgets/save_changes_bar.dart';

import 'app_test.dart' show FakeEngineClient;
import 'congestion_control_test.dart' show decodeProfile, response;
import 'ui_workflow_test.dart' show workflowHost;

class QuicEngine extends FakeEngineClient {
  bool supported = true;
  List<String>? fields;

  @override
  Future<EngineCapabilities?> getCapabilities() async => EngineCapabilities(
    automaticEndpoints: true,
    networkSettingsApplication: true,
    applicationQuicBlocking: supported,
  );

  @override
  Future<NetworkSettingsState> saveNetworkSettings(
    String operationId,
    String accountId,
    UsqueProfile values,
    List<String> changedFields,
  ) async {
    fields = changedFields;
    await super.saveNetworkSettings(
      operationId,
      accountId,
      values,
      changedFields,
    );
    return settingsState = NetworkSettingsState(
      sourceEpoch: 'test-engine',
      sequence: settingsSequence,
      operationId: operationId,
      sessionId: 'retained-session',
      storedProfile: values,
      appliedProfile: values,
      persisted: true,
      status: NetworkSettingsApplyStatus.applied,
    );
  }
}

void main() {
  const codec = ControlCodec();
  test(
    'QUIC JSON, protobuf 21, reset, and field mask preserve GEO settings',
    () {
      final previous = UsqueProfile.defaultProfile().copyWith(
        geoDirectCountries: ['JP', 'CN'],
      );
      final next = previous.copyWith(disableQuic: true);
      expect(UsqueProfile.fromMap(next.toMap()).disableQuic, isTrue);
      expect(decodeProfile(codec.encodeProfile(next)).disableQuic, isTrue);
      expect(
        codec.encodeProfile(next).sublist(codec.encodeProfile(next).length - 3),
        [0xa8, 1, 1],
      );
      expect(networkSettingsChangedFields(previous, next), ['disable_quic']);
      expect(next.resetAdvancedDefaults().disableQuic, isFalse);
      expect(next.geoDirectCountries, previous.geoDirectCountries);
      expect(
        UsqueProfile.fromMap(next.toMap()..remove('disable_quic')).disableQuic,
        isFalse,
      );
      expect(decodeProfile(codec.encodeProfile(previous)).disableQuic, isFalse);
      expect(
        () => UsqueProfile.fromMap(next.toMap()..['disable_quic'] = 'true'),
        throwsFormatException,
      );
    },
  );

  test('capability 31 and Android map require explicit support', () {
    final wire = (ControlPayloadWriter()..boolean(31, true)).takeBytes();
    expect(wire, [0xf8, 1, 1]);
    expect(
      codec
          .decodeResponse(response(15, wire), 'cc')
          .capabilities!
          .applicationQuicBlocking,
      isTrue,
    );
    expect(const EngineCapabilities().applicationQuicBlocking, isFalse);
    expect(
      EngineCapabilities.fromMap({
        'application_quic_blocking': true,
      }).applicationQuicBlocking,
      isTrue,
    );
    expect(EngineCapabilities.fromMap({}).applicationQuicBlocking, isFalse);
  });

  Future<AppController> screen(
    WidgetTester tester,
    QuicEngine engine, {
    bool chinese = false,
    double scale = 1,
    UsqueProfile? profile,
  }) async {
    tester.view.physicalSize = const Size(390, 1000);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);
    if (profile != null) {
      engine.storedProfiles = [profile];
      engine.storedActiveProfileId = profile.id;
      engine.settingsState = NetworkSettingsState(
        sourceEpoch: engine.settingsEpoch,
        sequence: engine.settingsSequence,
        storedProfile: profile,
        sharedNetwork: profile,
      );
    }
    final app = AppController(engine)
      ..localePreference = chinese
          ? LocalePreference.simplifiedChinese
          : LocalePreference.english
      ..snapshot = const EngineSnapshot(
        phase: ConnectionPhase.connected,
        transport: 'h3',
      );
    if (profile != null) app.sharedNetwork = profile;
    app.engineCapabilities = await engine.getCapabilities();
    addTearDown(app.dispose);
    await tester.pumpWidget(
      workflowHost(
        app,
        dark: chinese,
        scale: scale,
        home: AdvancedSettingsScreen(controller: app),
      ),
    );
    await tester.pumpAndSettle();
    await tester.ensureVisible(
      find.byKey(const ValueKey('disable-quic-switch')),
    );
    await tester.pumpAndSettle();
    return app;
  }

  for (final chinese in [false, true]) {
    testWidgets(
      'QUIC draft applies with keyboard and retained session, Chinese=$chinese',
      (tester) async {
        final engine = QuicEngine();
        final app = await screen(tester, engine, chinese: chinese, scale: 2);
        final toggle = find.byKey(const ValueKey('disable-quic-switch'));
        expect(tester.widget<SwitchListTile>(toggle).value, isFalse);
        // Use the native switch's focus and activation rather than a tiny target.
        tester.widget<SwitchListTile>(toggle).focusNode!.requestFocus();
        await tester.pump();
        await tester.sendKeyEvent(
          chinese ? LogicalKeyboardKey.enter : LogicalKeyboardKey.space,
        );
        await tester.pumpAndSettle();
        expect(tester.widget<SwitchListTile>(toggle).value, isTrue);
        expect(app.activeProfile.disableQuic, isFalse);
        expect(
          tester.widget<SaveChangesBar>(find.byType(SaveChangesBar)).dirty,
          isTrue,
        );
        await tester.tap(find.text(app.strings.get('save_changes')));
        await tester.pumpAndSettle();
        expect(engine.fields, ['disable_quic']);
        expect(app.activeProfile.disableQuic, isTrue);
        expect(app.networkSettings.state!.sessionId, 'retained-session');
        expect(
          tester.widget<SaveChangesBar>(find.byType(SaveChangesBar)).dirty,
          isFalse,
        );
        expect(tester.takeException(), isNull);
      },
    );
  }

  for (final source in [ChainSource.httpProxy, ChainSource.socks5Proxy]) {
    for (final manualPreference in [false, true]) {
      for (final chinese in [false, true]) {
        testWidgets(
          'proxy exit manages QUIC and restores $manualPreference: $source / $chinese',
          (tester) async {
            final engine = QuicEngine();
            final profile = UsqueProfile.defaultProfile().copyWith(
              disableQuic: manualPreference,
              chainExit: ChainExitSettings(
                enabled: true,
                source: source,
                profileId: 'saved-proxy',
                revision: 'revision-1',
              ),
            );
            final app = await screen(
              tester,
              engine,
              profile: profile,
              chinese: chinese,
              scale: 2,
            );
            final finder = find.byKey(const ValueKey('disable-quic-switch'));
            final toggle = tester.widget<SwitchListTile>(finder);
            expect(toggle.value, isTrue);
            expect(toggle.onChanged, isNull);
            expect(
              find.text(app.strings.get('disable_quic_managed')),
              findsOneWidget,
            );
            expect(app.activeProfile.disableQuic, manualPreference);
            expect(
              tester.widget<SaveChangesBar>(find.byType(SaveChangesBar)).dirty,
              isFalse,
            );
            // Read-only policy must not turn a keyboard activation into a
            // persisted manual preference or a hidden pending form edit.
            toggle.focusNode!.requestFocus();
            await tester.pump();
            await tester.sendKeyEvent(LogicalKeyboardKey.space);
            await tester.pumpAndSettle();
            expect(app.activeProfile.disableQuic, manualPreference);
            expect(engine.fields, isNull);

            // A confirmed edit from another surface must update this still-open
            // page even when EngineCapabilities itself has not changed.
            expect(
              await app.saveNetwork(
                app.activeProfile.copyWith(
                  chainExit: profile.chainExit!.copyWith(enabled: false),
                ),
                changedFields: ['chain_exit'],
              ),
              isTrue,
            );
            await tester.pumpAndSettle();
            final restored = tester.widget<SwitchListTile>(finder);
            expect(restored.value, manualPreference);
            expect(restored.onChanged, isNotNull);
            expect(
              find.text(app.strings.get('disable_quic_managed')),
              findsNothing,
            );
            expect(engine.fields, ['chain_exit']);
            expect(app.activeProfile.disableQuic, manualPreference);
            expect(
              tester.widget<SaveChangesBar>(find.byType(SaveChangesBar)).dirty,
              isFalse,
            );
            expect(tester.takeException(), isNull);
          },
        );
      }
    }
  }

  testWidgets('automatic QUIC does not overwrite an existing manual draft', (
    tester,
  ) async {
    final engine = QuicEngine();
    final app = await screen(tester, engine);
    final finder = find.byKey(const ValueKey('disable-quic-switch'));
    tester.widget<SwitchListTile>(finder).onChanged!(true);
    await tester.pumpAndSettle();
    expect(app.activeProfile.disableQuic, isFalse);
    const proxy = ChainExitSettings(
      enabled: true,
      source: ChainSource.socks5Proxy,
      profileId: 'saved-proxy',
      revision: 'revision-1',
    );
    expect(
      await app.saveNetwork(
        app.activeProfile.copyWith(chainExit: proxy),
        changedFields: ['chain_exit'],
      ),
      isTrue,
    );
    await tester.pumpAndSettle();
    expect(tester.widget<SwitchListTile>(finder).onChanged, isNull);
    expect(app.activeProfile.disableQuic, isFalse);
    expect(
      await app.saveNetwork(
        app.activeProfile.copyWith(chainExit: proxy.copyWith(enabled: false)),
        changedFields: ['chain_exit'],
      ),
      isTrue,
    );
    await tester.pumpAndSettle();
    expect(tester.widget<SwitchListTile>(finder).value, isTrue);
    expect(tester.widget<SwitchListTile>(finder).onChanged, isNotNull);
    expect(app.activeProfile.disableQuic, isFalse);
    expect(
      tester.widget<SaveChangesBar>(find.byType(SaveChangesBar)).dirty,
      isTrue,
    );
    expect(engine.fields, ['chain_exit']);
  });

  testWidgets('non-proxy chain keeps the manual QUIC control', (tester) async {
    final app = await screen(
      tester,
      QuicEngine(),
      profile: UsqueProfile.defaultProfile().copyWith(
        chainExit: const ChainExitSettings(
          enabled: true,
          source: ChainSource.openvpnCustom,
          profileId: 'saved-vpn',
          revision: 'revision-1',
        ),
      ),
    );
    final toggle = tester.widget<SwitchListTile>(
      find.byKey(const ValueKey('disable-quic-switch')),
    );
    expect(toggle.value, isFalse);
    expect(toggle.onChanged, isNotNull);
    expect(find.text(app.strings.get('disable_quic_managed')), findsNothing);
    expect(tester.takeException(), isNull);
  });

  testWidgets('unsupported engine disables the setting with an explanation', (
    tester,
  ) async {
    final app = await screen(tester, QuicEngine()..supported = false);
    final toggle = tester.widget<SwitchListTile>(
      find.byKey(const ValueKey('disable-quic-switch')),
    );
    expect(toggle.onChanged, isNull);
    expect(
      find.text(app.strings.get('disable_quic_unsupported')),
      findsOneWidget,
    );
  });

  testWidgets('failed save keeps the QUIC draft', (tester) async {
    final engine = QuicEngine()..failProfileUpsert = true;
    final app = await screen(tester, engine);
    final toggle = find.byKey(const ValueKey('disable-quic-switch'));
    await tester.tap(toggle);
    await tester.pumpAndSettle();
    await tester.tap(find.text(app.strings.get('save_changes')));
    await tester.pumpAndSettle();
    expect(tester.widget<SwitchListTile>(toggle).value, isTrue);
    expect(
      tester.widget<SaveChangesBar>(find.byType(SaveChangesBar)).dirty,
      isTrue,
    );
    expect(app.activeProfile.disableQuic, isFalse);
  });
}
