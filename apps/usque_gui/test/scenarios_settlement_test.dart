import 'dart:async';

import 'package:flutter_test/flutter_test.dart';
import 'package:usque/models/scenarios.dart';
import 'package:usque/services/engine_client.dart';
import 'package:usque/state/scenarios_controller.dart';

/// An owner catalogue as returned after a lost activation reply.
Map<String, Object?> _catalog({
  String? last,
  String? unresolved,
  String? receipt,
  String revision = 'catalogue-1',
  bool unchanged = false,
}) => {
  if (!unchanged)
    'scenarios': [
      {
        'id': 'scene-a',
        'name': 'Daily',
        'revision': 'revision-a',
        'bound_account_id': null,
        'settings': <String, Object?>{},
      },
    ],
  if (unchanged) 'scenarios_unchanged': true,
  'catalogue_revision': revision,
  'state': {'apply_status': 'deferred'},
  'last_operation_id': last,
  'unresolved_operation_id': unresolved,
  'persisted_operation_id': ?receipt,
};

final _preview = ScenarioPreview.fromMap({
  'preview': {
    'preview_token': 't',
    'scenario_id': 'scene-a',
    'scenario_revision': 'revision-a',
  },
});

void main() {
  test('a lost activation the owner never committed releases the barrier '
      'without replay', () async {
    var activations = 0, readbacks = 0;
    final controller = ScenariosController((request) async {
      if (request['action'] == 'activate') {
        activations++;
        throw TimeoutException('Request lost before commit');
      }
      return _catalog(last: 'older-operation');
    })..setSupported(true);
    addTearDown(controller.dispose);
    controller.afterActivation = () async => readbacks++;
    expect(await controller.activate(_preview), false);
    expect(controller.unconfirmed, true);
    expect(await controller.synchronizeActivation(), true);
    expect(controller.pendingOperationId, isNull);
    expect(controller.unconfirmed, false);
    expect(controller.errorCode, 'SCENARIO_NOT_APPLIED');
    expect(readbacks, 1);
    expect(activations, 1);
  });

  test(
    'a durable record without a receipt is read back before connecting',
    () async {
      String? operation;
      var readbacks = 0;
      final controller = ScenariosController((request) async {
        if (request['action'] == 'activate') {
          operation = request['operation_id'] as String;
          throw TimeoutException('Committed, reply lost');
        }
        return _catalog(last: operation);
      })..setSupported(true);
      addTearDown(controller.dispose);
      controller.afterActivation = () async => readbacks++;
      expect(await controller.activate(_preview), false);
      expect(await controller.synchronizeActivation(), true);
      expect(readbacks, 1);
      expect(controller.errorCode, 'SCENARIO_COMMIT_UNCONFIRMED');
    },
  );

  test('the barrier holds while the owner cannot settle the commit', () async {
    String? operation;
    final controller = ScenariosController((request) async {
      if (request['action'] == 'activate') {
        operation = request['operation_id'] as String;
        throw TimeoutException('Commit uncertain');
      }
      return _catalog(last: operation, unresolved: operation);
    })..setSupported(true);
    addTearDown(controller.dispose);
    expect(await controller.activate(_preview), false);
    expect(await controller.synchronizeActivation(), false);
    expect(controller.pendingOperationId, operation);
    expect(controller.unconfirmed, true);
  });

  test(
    'an older owner without settlement data keeps the conservative barrier',
    () async {
      final controller = ScenariosController((request) async {
        if (request['action'] == 'activate') {
          throw TimeoutException('Unknown commit');
        }
        return {..._catalog(last: 'older-operation')}
          ..remove('unresolved_operation_id');
      })..setSupported(true);
      addTearDown(controller.dispose);
      expect(await controller.activate(_preview), false);
      expect(await controller.synchronizeActivation(), false);
    },
  );

  for (final code in [
    'SCENARIO_RUNTIME_UNCONFIRMED',
    'SCENARIO_STORAGE_UNAVAILABLE',
    'SCENARIO_SAVE_FAILED',
  ]) {
    test('$code is a pre-commit rejection, not an unknown result', () async {
      final controller = ScenariosController((request) async {
        if (request['action'] == 'activate') {
          throw EngineException(code, 'Rejected before commit');
        }
        return _catalog();
      })..setSupported(true);
      addTearDown(controller.dispose);
      expect(await controller.activate(_preview), false);
      expect(controller.unconfirmed, false);
      expect(controller.pendingOperationId, isNull);
      expect(await controller.synchronizeActivation(), true);
    });
  }

  test(
    'status polls reuse unchanged templates by catalogue revision',
    () async {
      final requests = <Map<String, Object?>>[];
      var unchanged = false;
      final controller = ScenariosController((request) async {
        requests.add(Map.of(request));
        final value = _catalog(unchanged: unchanged);
        unchanged = true;
        return value;
      })..setSupported(true);
      addTearDown(controller.dispose);
      controller.setVisible(true);
      await controller.refresh();
      expect(requests.single.containsKey('known_revision'), false);
      await controller.refresh();
      expect(requests.last['known_revision'], 'catalogue-1');
      expect(controller.catalog!.scenarios.single.id, 'scene-a');
      expect(controller.catalog!.revision, 'catalogue-1');
    },
  );

  test('a mismatched unchanged reply forces a full catalogue read', () async {
    final requests = <Map<String, Object?>>[];
    var call = 0;
    final controller = ScenariosController((request) async {
      requests.add(Map.of(request));
      return switch (call++) {
        0 => _catalog(),
        1 => _catalog(unchanged: true, revision: 'catalogue-2'),
        _ => _catalog(revision: 'catalogue-2'),
      };
    })..setSupported(true);
    addTearDown(controller.dispose);
    await controller.refresh();
    await controller.refresh();
    expect(controller.catalog!.revision, 'catalogue-1');
    await controller.refresh();
    expect(requests.last.containsKey('known_revision'), false);
    expect(controller.catalog!.revision, 'catalogue-2');
  });
}
