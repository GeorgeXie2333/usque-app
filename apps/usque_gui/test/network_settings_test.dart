import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:shared_preferences/shared_preferences.dart';
import 'package:usque/core/app_strings.dart';
import 'package:usque/core/usque_theme.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/services/control_codec.dart';
import 'package:usque/services/engine_client.dart';
import 'package:usque/state/app_controller.dart';
import 'package:usque/state/network_settings_controller.dart';
import 'package:usque/widgets/save_changes_bar.dart';

import 'app_test.dart' show FakeEngineClient;

class SettingsEngine extends FakeEngineClient {
  final submitted = <String>[];
  Completer<NetworkSettingsState>? response;
  Completer<NetworkSettingsState>? queryResponse;
  Object? saveFailure;
  Object? queryFailure;
  @override
  Future<NetworkSettingsState> saveNetworkSettings(
    String operationId,
    String accountId,
    UsqueProfile values,
    List<String> fields,
  ) async {
    submitted.add(operationId);
    if (saveFailure != null) throw saveFailure!;
    return response?.future ??
        super.saveNetworkSettings(operationId, accountId, values, fields);
  }

  @override
  Future<NetworkSettingsState> getNetworkSettingsState() async {
    if (queryFailure != null) throw queryFailure!;
    return queryResponse?.future ?? super.getNetworkSettingsState();
  }
}

void main() {
  test(
    'disconnect cancels a manual connection waiting for save acknowledgement',
    () async {
      SharedPreferences.setMockInitialValues({});
      final engine = SettingsEngine();
      final app = AppController(engine);
      await app.initialize();
      addTearDown(app.dispose);
      engine.response = Completer();
      final saving = app.saveNetwork(app.activeProfile.copyWith(mtu: 1400));
      await Future<void>.delayed(Duration.zero);
      final connecting = app.connectOrDisconnect();
      await Future<void>.delayed(Duration.zero);
      await app.connectOrDisconnect();
      engine.response!.complete(
        NetworkSettingsState(
          sourceEpoch: 'one',
          sequence: 1,
          operationId: engine.submitted.single,
          persisted: true,
          storedProfile: app.activeProfile.copyWith(mtu: 1400),
          status: NetworkSettingsApplyStatus.deferred,
        ),
      );
      expect(await saving, isTrue);
      await connecting;
      expect(engine.calls, isNot(contains('connect')));
      expect(app.snapshot.phase, ConnectionPhase.disconnected);
    },
  );

  testWidgets(
    'failed application remains readable and reconnect is keyboard reachable at 200 percent',
    (tester) async {
      for (final locale in [
        LocalePreference.english,
        LocalePreference.simplifiedChinese,
      ]) {
        for (final brightness in [Brightness.light, Brightness.dark]) {
          var reconnected = 0;
          final strings = AppStrings(locale);
          await tester.pumpWidget(
            MaterialApp(
              theme: brightness == Brightness.light
                  ? UsqueTheme.light()
                  : UsqueTheme.dark(),
              home: MediaQuery(
                data: const MediaQueryData(textScaler: TextScaler.linear(2)),
                child: Scaffold(
                  body: Center(
                    child: SizedBox(
                      width: 360,
                      child: SaveChangesBar(
                        strings: strings,
                        dirty: false,
                        saving: false,
                        statusLabel: strings.get('settings_failed'),
                        onSave: null,
                        onReconnect: () => reconnected++,
                      ),
                    ),
                  ),
                ),
              ),
            ),
          );
          await tester.pumpAndSettle();
          expect(find.text(strings.get('settings_failed')), findsOneWidget);
          await tester.sendKeyEvent(LogicalKeyboardKey.tab);
          await tester.sendKeyEvent(LogicalKeyboardKey.enter);
          await tester.pumpAndSettle();
          expect(reconnected, 1);
          expect(tester.takeException(), isNull);
          await tester.pumpWidget(const SizedBox());
        }
      }
    },
  );
  test(
    'settings result wording uses English and Simplified Chinese catalogs',
    () {
      expect(
        AppStrings(LocalePreference.simplifiedChinese).get('settings_deferred'),
        '已保存，下次手动连接生效',
      );
      expect(
        AppStrings(LocalePreference.english).get('settings_failed'),
        'Saved, application failed',
      );
    },
  );
  test(
    'saved state does not change before acknowledgement, queue ignores apply lifetime',
    () async {
      final engine = SettingsEngine()..response = Completer();
      final controller = NetworkSettingsController(engine)..supported = true;
      addTearDown(controller.dispose);
      final profile = UsqueProfile.defaultProfile().copyWith(mtu: 1400);
      final save = controller.save(profile, ['mtu']);
      await Future<void>.delayed(Duration.zero);
      expect(controller.state, isNull);
      final op = engine.submitted.single;
      engine.response!.complete(
        NetworkSettingsState(
          sourceEpoch: 'one',
          sequence: 1,
          operationId: op,
          storedProfile: profile,
          persisted: true,
          status: NetworkSettingsApplyStatus.applying,
        ),
      );
      expect(await save, isTrue);
      await controller.flushed;
      expect(controller.state!.status, NetworkSettingsApplyStatus.applying);
      expect(controller.state!.storedProfile!.mtu, 1400);
    },
  );

  test(
    'late and duplicate responses cannot regress a confirmed save or revive an epoch',
    () {
      final controller = NetworkSettingsController(SettingsEngine());
      addTearDown(controller.dispose);
      final profile = UsqueProfile.defaultProfile();
      controller.accept(
        NetworkSettingsState(
          sourceEpoch: 'one',
          sequence: 2,
          storedProfile: profile.copyWith(mtu: 1400),
        ),
      );
      controller.accept(
        NetworkSettingsState(
          sourceEpoch: 'one',
          sequence: 1,
          storedProfile: profile,
        ),
      );
      expect(controller.state!.storedProfile!.mtu, 1400);
      controller.accept(
        const NetworkSettingsState(sourceEpoch: 'two', sequence: 0),
      );
      controller.accept(
        const NetworkSettingsState(sourceEpoch: 'one', sequence: 99),
      );
      expect(controller.state!.sourceEpoch, 'two');
    },
  );

  test('ambiguous save is queried once and never replayed', () async {
    final engine = SettingsEngine()
      ..saveFailure = const EngineException('ENGINE_REQUEST_TIMEOUT', 'timeout')
      ..queryFailure = const EngineException(
        'ENGINE_IPC_UNAVAILABLE',
        'offline',
      );
    final controller = NetworkSettingsController(engine)..supported = true;
    addTearDown(controller.dispose);
    expect(
      await controller.save(UsqueProfile.defaultProfile(), ['mtu']),
      isFalse,
    );
    expect(engine.submitted, hasLength(1));
    expect(controller.unconfirmed, isTrue);
    expect(controller.state, isNull);
  });

  test('persistence success is separate from application failure', () async {
    final engine = SettingsEngine()..response = Completer();
    final controller = NetworkSettingsController(engine)..supported = true;
    addTearDown(controller.dispose);
    final save = controller.save(UsqueProfile.defaultProfile(), ['mtu']);
    await Future<void>.delayed(Duration.zero);
    engine.response!.complete(
      NetworkSettingsState(
        sourceEpoch: 'one',
        sequence: 1,
        operationId: engine.submitted.single,
        persisted: true,
        status: NetworkSettingsApplyStatus.failed,
        errorCode: 'NETWORK_SETTINGS_APPLY_FAILED',
      ),
    );
    expect(await save, isTrue);
    expect(controller.state!.status, NetworkSettingsApplyStatus.failed);
    expect(controller.saveError, isNull);
  });

  for (final recovery in ['query', 'event']) {
    test(
      'save status recovers after a failed query through $recovery',
      () async {
        SharedPreferences.setMockInitialValues({});
        final engine = SettingsEngine()
          ..settingsState = const NetworkSettingsState(
            sourceEpoch: 'one',
            sequence: 1,
            operationId: 'confirmed',
            persisted: true,
            status: NetworkSettingsApplyStatus.applied,
          );
        final app = AppController(engine);
        await app.initialize();
        addTearDown(app.dispose);
        await Future<void>.delayed(Duration.zero);
        await app.networkSettings.refresh();
        engine.queryFailure = const EngineException(
          'ENGINE_IPC_UNAVAILABLE',
          'offline',
        );
        await app.networkSettings.refresh();
        expect(app.networkSettingsMessage, app.strings.get('settings_unknown'));

        engine.queryFailure = null;
        if (recovery == 'query') {
          // Desktop queries need not advance the state sequence.
          await app.networkSettings.refresh();
        } else {
          app.networkSettings.accept(
            const NetworkSettingsState(
              sourceEpoch: 'one',
              sequence: 2,
              operationId: 'confirmed',
              persisted: true,
              status: NetworkSettingsApplyStatus.applied,
            ),
          );
        }
        expect(app.networkSettings.unconfirmed, isFalse);
        expect(app.networkSettingsMessage, app.strings.get('settings_applied'));
        expect(engine.submitted, isEmpty);
      },
    );
  }

  test(
    'stale replies cannot clear a failed query or replace confirmed state',
    () async {
      final engine = SettingsEngine()
        ..queryFailure = const EngineException(
          'ENGINE_IPC_UNAVAILABLE',
          'offline',
        );
      final controller = NetworkSettingsController(engine)..supported = true;
      addTearDown(controller.dispose);
      controller.accept(
        const NetworkSettingsState(sourceEpoch: 'old', sequence: 9),
      );
      const current = NetworkSettingsState(
        sourceEpoch: 'current',
        sequence: 2,
        operationId: 'saved',
        persisted: true,
      );
      controller.accept(current);
      await controller.refresh();
      controller.accept(
        const NetworkSettingsState(sourceEpoch: 'old', sequence: 99),
      );
      controller.accept(
        const NetworkSettingsState(sourceEpoch: 'current', sequence: 1),
      );
      expect(controller.unconfirmed, isTrue);
      expect(controller.state, same(current));
      controller.accept(
        const NetworkSettingsState(sourceEpoch: 'current', sequence: 2),
      );
      expect(controller.unconfirmed, isFalse);
      expect(controller.state, same(current));
    },
  );

  test(
    'late query failure cannot invalidate a newer authoritative event',
    () async {
      final engine = SettingsEngine()..queryResponse = Completer();
      final controller = NetworkSettingsController(engine)..supported = true;
      addTearDown(controller.dispose);
      final refreshing = controller.refresh();
      controller.accept(
        const NetworkSettingsState(
          sourceEpoch: 'one',
          sequence: 2,
          operationId: 'saved',
          persisted: true,
        ),
      );
      engine.queryResponse!.completeError(
        const EngineException('ENGINE_IPC_UNAVAILABLE', 'offline'),
      );
      await refreshing;
      expect(controller.unconfirmed, isFalse);
      expect(controller.state!.sequence, 2);
    },
  );

  test(
    'an ambiguous save clears only after its own durable confirmation',
    () async {
      final engine = SettingsEngine()
        ..saveFailure = const EngineException(
          'ENGINE_REQUEST_TIMEOUT',
          'timeout',
        )
        ..queryFailure = const EngineException(
          'ENGINE_IPC_UNAVAILABLE',
          'offline',
        );
      final controller = NetworkSettingsController(engine)..supported = true;
      addTearDown(controller.dispose);
      expect(
        await controller.save(UsqueProfile.defaultProfile(), ['mtu']),
        isFalse,
      );
      final operation = engine.submitted.single;
      engine.queryFailure = null;
      engine.settingsState = const NetworkSettingsState(
        sourceEpoch: 'one',
        sequence: 1,
        operationId: 'another-save',
        persisted: true,
      );
      await controller.refresh();
      expect(controller.unconfirmed, isTrue);
      controller.accept(
        NetworkSettingsState(
          sourceEpoch: 'one',
          sequence: 2,
          operationId: operation,
        ),
      );
      expect(controller.unconfirmed, isTrue);
      controller.accept(
        NetworkSettingsState(
          sourceEpoch: 'one',
          sequence: 3,
          operationId: operation,
          persisted: true,
          status: NetworkSettingsApplyStatus.failed,
        ),
      );
      expect(controller.unconfirmed, isFalse);
      expect(controller.state!.status, NetworkSettingsApplyStatus.failed);
      expect(engine.submitted, hasLength(1));
    },
  );

  test(
    'an event confirming a save survives a later response timeout',
    () async {
      final engine = SettingsEngine()
        ..response = Completer()
        ..queryFailure = const EngineException(
          'ENGINE_IPC_UNAVAILABLE',
          'offline',
        );
      final controller = NetworkSettingsController(engine)..supported = true;
      addTearDown(controller.dispose);
      final saving = controller.save(UsqueProfile.defaultProfile(), ['mtu']);
      await Future<void>.delayed(Duration.zero);
      controller.accept(
        NetworkSettingsState(
          sourceEpoch: 'one',
          sequence: 1,
          operationId: engine.submitted.single,
          persisted: true,
        ),
      );
      engine.response!.completeError(
        const EngineException('ENGINE_REQUEST_TIMEOUT', 'timeout'),
      );
      expect(await saving, isTrue);
      expect(controller.unconfirmed, isFalse);
      expect(engine.submitted, hasLength(1));
    },
  );

  test('new protocol uses appended response, event and capability fields', () {
    const codec = ControlCodec();
    final state = ControlPayloadWriter()
      ..string(1, 'one')
      ..unsigned(2, 9)
      ..string(3, 'op')
      ..enumeration(7, 4)
      ..string(8, 'congestion_control')
      ..boolean(10, true);
    final bytes = state.takeBytes();
    final response = codec.frame(
      (ControlPayloadWriter()
            ..string(1, 'request')
            ..message(22, bytes))
          .takeBytes(),
    );
    final decoded = codec.decodeResponse(response, 'request').networkSettings!;
    expect(decoded.persisted, isTrue);
    expect(decoded.status, NetworkSettingsApplyStatus.deferred);
    expect(decoded.deferredFields, ['congestion_control']);
    final event = codec.frame(
      (ControlPayloadWriter()
            ..unsigned(1, 99)
            ..message(24, bytes))
          .takeBytes(),
    );
    expect(codec.decodeEvent(event).networkSettings!.sequence, 9);
    final caps = codec.frame(
      (ControlPayloadWriter()
            ..string(1, 'request')
            ..message(
              15,
              (ControlPayloadWriter()..boolean(25, true)).takeBytes(),
            ))
          .takeBytes(),
    );
    expect(
      codec
          .decodeResponse(caps, 'request')
          .capabilities!
          .networkSettingsApplication,
      isTrue,
    );
    final malformed = codec.frame(
      (ControlPayloadWriter()
            ..string(1, 'request')
            ..message(22, Uint8List.fromList([0x0a, 0xff])))
          .takeBytes(),
    );
    expect(
      () => codec.decodeResponse(malformed, 'request'),
      throwsA(isA<EngineException>()),
    );
  });
}
