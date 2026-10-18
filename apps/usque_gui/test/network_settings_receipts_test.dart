import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:shared_preferences/shared_preferences.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/services/control_codec.dart';
import 'package:usque/services/engine_client.dart';
import 'package:usque/state/app_controller.dart';
import 'package:usque/state/network_settings_controller.dart';

import 'network_settings_test.dart' show SettingsEngine;
import 'ui_workflow_test.dart' show workflowHost;

/// Persist the real edit, then lose its reply and initial readback.
class _ReceiptEngine extends SettingsEngine {
  final receipts = <String>[];
  bool loseNextReply = true;

  @override
  Future<NetworkSettingsState> saveNetworkSettings(
    String operationId,
    String accountId,
    UsqueProfile values,
    List<String> fields,
  ) async {
    final saved = await super.saveNetworkSettings(
      operationId,
      accountId,
      values,
      fields,
    );
    receipts.add(operationId);
    settingsState = NetworkSettingsState(
      sourceEpoch: saved.sourceEpoch,
      sequence: saved.sequence,
      operationId: operationId,
      persisted: saved.persisted,
      storedProfile: saved.storedProfile,
      sharedNetwork: saved.sharedNetwork,
      status: saved.status,
      deferredFields: saved.deferredFields,
      persistedOperationIds: List.of(receipts),
    );
    if (loseNextReply) {
      loseNextReply = false;
      throw const EngineException('ENGINE_REQUEST_TIMEOUT', 'lost save reply');
    }
    return settingsState!;
  }
}

const _offline = EngineException('ENGINE_IPC_UNAVAILABLE', 'unavailable');
const _endpointFields = ['endpoint.selection', 'endpoint.ipv4'];

void main() {
  final invalid = UsqueProfile.defaultProfile().copyWith(
    endpointSelection: EndpointSelection.custom,
    endpointIpv4: '192.0.2.1',
  );
  final corrected = invalid.copyWith(
    endpointIpv4: UsqueProfile.defaultEndpointIpv4,
  );

  for (final platform in [TargetPlatform.windows, TargetPlatform.android]) {
    for (final locale in [
      LocalePreference.english,
      LocalePreference.simplifiedChinese,
    ]) {
      testWidgets(
        'corrected endpoint recovers an earlier save receipt without restart: '
        '${platform.name} ${locale.name}',
        (tester) async {
          tester.view.devicePixelRatio = 1;
          tester.view.physicalSize = platform == TargetPlatform.android
              ? const Size(375, 812)
              : const Size(1280, 900);
          addTearDown(tester.view.resetDevicePixelRatio);
          addTearDown(tester.view.resetPhysicalSize);
          SharedPreferences.setMockInitialValues({
            'onboarding_complete': true,
            'update_checks_enabled': false,
          });
          final engine = _ReceiptEngine();
          final app = AppController(engine);
          addTearDown(app.dispose);
          await app.initialize();
          app.localePreference = locale;
          engine.queryFailure = _offline;
          expect(
            await app.saveNetwork(invalid, changedFields: _endpointFields),
            isFalse,
          );
          expect(engine.storedProfiles.single.endpointIpv4, '192.0.2.1');
          expect(app.networkSettings.unconfirmed, isTrue);
          engine.connectError = const EngineException(
            'H2_TCP_CONNECT_FAILED',
            'unreachable custom endpoint',
          );
          await app.connectOrDisconnect();
          expect(app.snapshot.phase, ConnectionPhase.error);

          engine
            ..connectError = null
            ..queryFailure = null;
          expect(
            await app.saveNetwork(corrected, changedFields: _endpointFields),
            isTrue,
          );
          await app.retry();
          engine.settingsState = NetworkSettingsState(
            sourceEpoch: engine.settingsEpoch,
            sequence: ++engine.settingsSequence,
            operationId: engine.submitted.last,
            persisted: true,
            sharedNetwork: corrected,
            storedProfile: corrected,
            appliedProfile: corrected,
            status: NetworkSettingsApplyStatus.applied,
            persistedOperationIds: List.of(engine.receipts),
          );
          await app.networkSettings.refresh();
          expect(app.snapshot.isConnected, isTrue);
          expect(app.activeProfile.endpointIpv4, corrected.endpointIpv4);
          expect(app.lastError, isNull);
          expect(app.networkSettings.unconfirmed, isFalse);
          expect(app.networkShortcutsLocked, isFalse);
          expect(engine.submitted, hasLength(2));

          await tester.pumpWidget(workflowHost(app));
          await tester.pumpAndSettle();
          expect(find.text(app.strings.get('settings_unknown')), findsNothing);
          expect(
            find.text(app.strings.get('settings_confirmation_unavailable')),
            findsNothing,
          );
          app.setObservationVisible(false);
          await tester.pumpWidget(const SizedBox.shrink());
        },
        variant: TargetPlatformVariant.only(platform),
      );
    }
  }

  testWidgets(
    'readback of receipts confirms a lost reply without another save',
    (tester) async {
      final engine = _ReceiptEngine()..queryFailure = _offline;
      final controller = NetworkSettingsController(engine)..supported = true;
      addTearDown(controller.dispose);
      expect(await controller.save(invalid, _endpointFields), isFalse);
      expect(controller.unconfirmed, isTrue);
      engine.queryFailure = null;
      await controller.refresh();
      expect(controller.unconfirmed, isFalse);
      expect(engine.submitted, hasLength(1));
    },
  );

  testWidgets('receipts confirm only named saves and retain runtime failure', (
    tester,
  ) async {
    final engine = SettingsEngine()
      ..saveFailure = const EngineException('ENGINE_REQUEST_TIMEOUT', 'lost')
      ..queryFailure = _offline;
    final controller = NetworkSettingsController(engine)..supported = true;
    addTearDown(controller.dispose);
    expect(await controller.save(invalid, _endpointFields), isFalse);
    expect(await controller.save(corrected, _endpointFields), isFalse);
    controller.accept(
      NetworkSettingsState(
        sourceEpoch: 'engine',
        sequence: 1,
        operationId: engine.submitted.last,
        persisted: null,
        status: NetworkSettingsApplyStatus.unknown,
        persistedOperationIds: [engine.submitted.first],
      ),
    );
    expect(controller.unconfirmed, isTrue);
    controller.accept(
      NetworkSettingsState(
        sourceEpoch: 'engine',
        sequence: 2,
        operationId: engine.submitted.last,
        persisted: true,
        status: NetworkSettingsApplyStatus.failed,
        errorCode: 'NETWORK_SETTINGS_APPLY_FAILED',
        persistedOperationIds: [engine.submitted.last],
      ),
    );
    expect(controller.unconfirmed, isFalse);
    expect(controller.state!.status, NetworkSettingsApplyStatus.failed);
    expect(controller.state!.errorCode, 'NETWORK_SETTINGS_APPLY_FAILED');
  });

  testWidgets('stale sequences and retired epochs cannot supply receipts', (
    tester,
  ) async {
    final engine = SettingsEngine()
      ..saveFailure = const EngineException('ENGINE_REQUEST_TIMEOUT', 'lost')
      ..queryFailure = _offline;
    final controller = NetworkSettingsController(engine)..supported = true;
    addTearDown(controller.dispose);
    expect(await controller.save(invalid, _endpointFields), isFalse);
    final operation = engine.submitted.single;
    controller.accept(
      const NetworkSettingsState(sourceEpoch: 'old', sequence: 1),
    );
    controller.accept(
      const NetworkSettingsState(sourceEpoch: 'new', sequence: 2),
    );
    for (final epoch in ['old', 'new']) {
      controller.accept(
        NetworkSettingsState(
          sourceEpoch: epoch,
          sequence: 1,
          persistedOperationIds: [operation],
        ),
      );
      expect(controller.unconfirmed, isTrue);
    }
    controller.accept(
      NetworkSettingsState(
        sourceEpoch: 'new',
        sequence: 3,
        persistedOperationIds: [operation],
      ),
    );
    expect(controller.unconfirmed, isFalse);
  });

  test(
    'receipt lists are immutable, bounded and optional for older engines',
    () {
      final source = <String>['saved'];
      final state = NetworkSettingsState.fromMap({
        'source_epoch': 'engine',
        'sequence': 1,
        'persisted_operation_ids': source,
      });
      source.clear();
      expect(state.persistedOperationIds, ['saved']);
      expect(() => state.persistedOperationIds.clear(), throwsUnsupportedError);
      expect(
        NetworkSettingsState.fromMap({
          'source_epoch': 'engine',
          'sequence': 1,
        }).persistedOperationIds,
        isEmpty,
      );
      for (final invalid in [
        'saved',
        [1],
        [''],
        ['x' * 37],
        List.filled(NetworkSettingsState.maxPersistedOperations + 1, 'saved'),
      ]) {
        expect(
          () => NetworkSettingsState.fromMap({
            'source_epoch': 'engine',
            'sequence': 1,
            'persisted_operation_ids': invalid,
          }),
          throwsFormatException,
        );
      }
    },
  );

  test('desktop replies and events decode bounded persistence receipts', () {
    const codec = ControlCodec();
    for (final count in [0, NetworkSettingsState.maxPersistedOperations, 65]) {
      final state = ControlPayloadWriter()
        ..string(1, 'engine')
        ..unsigned(2, 1);
      for (var index = 0; index < count; index++) {
        state.string(12, 'receipt-$index');
      }
      final payload = state.takeBytes();
      final reply = codec.frame(
        (ControlPayloadWriter()
              ..string(1, 'request')
              ..message(22, payload))
            .takeBytes(),
      );
      final event = codec.frame(
        (ControlPayloadWriter()
              ..unsigned(1, 1)
              ..message(24, payload))
            .takeBytes(),
      );
      if (count > NetworkSettingsState.maxPersistedOperations) {
        expect(
          () => codec.decodeResponse(reply, 'request'),
          throwsA(isA<EngineException>()),
        );
        expect(() => codec.decodeEvent(event), throwsA(isA<EngineException>()));
      } else {
        expect(
          codec
              .decodeResponse(reply, 'request')
              .networkSettings!
              .persistedOperationIds,
          hasLength(count),
        );
        expect(
          codec.decodeEvent(event).networkSettings!.persistedOperationIds,
          hasLength(count),
        );
      }
    }
  });

  test(
    'disconnected settings accept the complete deferred field catalogue',
    () {
      const codec = ControlCodec();
      for (final count in [32, 34, 64, 65]) {
        final state = ControlPayloadWriter()
          ..string(1, 'engine')
          ..unsigned(2, 1)
          ..enumeration(7, 4);
        for (var index = 0; index < count; index++) {
          state.string(8, 'pending-$index');
        }
        final reply = codec.frame(
          (ControlPayloadWriter()
                ..string(1, 'request')
                ..message(22, state.takeBytes()))
              .takeBytes(),
        );
        if (count > 64) {
          expect(
            () => codec.decodeResponse(reply, 'request'),
            throwsA(isA<EngineException>()),
          );
        } else {
          final decoded = codec
              .decodeResponse(reply, 'request')
              .networkSettings!;
          expect(decoded.status, NetworkSettingsApplyStatus.deferred);
          expect(decoded.deferredFields, hasLength(count));
        }
      }
    },
  );
}
