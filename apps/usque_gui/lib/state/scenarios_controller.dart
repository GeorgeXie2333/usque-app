import 'dart:async';
import 'dart:math';

import 'package:flutter/foundation.dart';

import '../models/scenarios.dart';
import '../services/engine_client.dart';

/// Scene operations are owned by the Engine. A missing reply never causes an
/// activation to be replayed, and template saves never apply network settings.
class ScenariosController extends ChangeNotifier {
  ScenariosController(this._command);
  final Future<Map<String, Object?>> Function(Map<String, Object?>) _command;
  ScenarioCatalog? catalog;
  bool supported = false, busy = false;
  bool _activationUnconfirmed = false;
  int? _templateUnconfirmedSequence;
  bool get unconfirmed =>
      _activationUnconfirmed || _templateUnconfirmedSequence != null;
  bool queryUnconfirmed = false;
  String? errorCode;
  String? _queryErrorCode;
  String? pendingOperationId;
  bool _disposed = false, _visible = false;
  int _generation = 0;
  int _requestSequence = 0, _acceptedSequence = 0;
  int _pollAttempts = 0;
  Timer? _refreshTimer;
  Future<void>? _refresh;
  Future<void> Function()? afterActivation;
  String? _confirmedActivationId;
  Future<void>? _activationReadback;
  int _activationSequence = 0;
  String? _settledError;
  bool _fullListRequired = false;

  void setSupported(bool value) {
    if (supported == value) return;
    supported = value;
    if (!_disposed) notifyListeners();
    if (value && _visible) unawaited(refresh());
  }

  void setVisible(bool value) {
    if (_visible != value) _pollAttempts = 0;
    _visible = value;
    if (value && supported) unawaited(refresh());
    if (!value) _refreshTimer?.cancel();
  }

  void invalidate() {
    if (!_visible || !supported || _disposed) return;
    _refreshTimer?.cancel();
    _refreshTimer = Timer(
      const Duration(milliseconds: 150),
      () => unawaited(refresh()),
    );
  }

  void _accept(Map<String, Object?> result, int sequence) {
    if (_disposed) return;
    if (result['error'] != null) {
      final error = result['error'];
      final code = error is String
          ? error
          : error is Map
          ? error['code']
          : null;
      throw EngineException(
        code is String && code.startsWith('SCENARIO_')
            ? code
            : 'SCENARIO_OPERATION_FAILED',
        'Scene operation failed.',
      );
    }
    // An older command can acknowledge itself without replacing a newer view.
    final pending = pendingOperationId;
    if (pending != null && result['persisted_operation_id'] == pending) {
      _confirmedActivationId = pending;
      _activationUnconfirmed = false;
    } else if (pending != null &&
        _activationUnconfirmed &&
        sequence > _activationSequence &&
        result.containsKey('unresolved_operation_id') &&
        result.containsKey('last_operation_id') &&
        result['unresolved_operation_id'] != pending) {
      // Without a receipt, a later catalogue settles the outcome once the
      // owner reports no unresolved commit: the durable record either holds
      // this operation or it never committed. Both still require a readback
      // before connecting; the activation itself is never replayed.
      _confirmedActivationId = pending;
      _activationUnconfirmed = false;
      _settledError = result['last_operation_id'] == pending
          ? 'SCENARIO_COMMIT_UNCONFIRMED'
          : 'SCENARIO_NOT_APPLIED';
    }
    final current = catalog;
    final unchanged =
        result['scenarios_unchanged'] == true &&
        current != null &&
        current.revision != null &&
        result['catalogue_revision'] == current.revision;
    if (result['scenarios_unchanged'] == true && !unchanged) {
      _fullListRequired = true;
    }
    if (sequence >= _acceptedSequence &&
        (result.containsKey('scenarios') || unchanged)) {
      catalog = ScenarioCatalog.fromMap(
        result,
        unchanged: unchanged ? current.scenarios : null,
      );
      _acceptedSequence = sequence;
      // A fresh authoritative catalogue reconciles template uncertainty. Older
      // queries issued before the failed mutation cannot establish its outcome.
      if (_templateUnconfirmedSequence != null &&
          sequence > _templateUnconfirmedSequence!) {
        _templateUnconfirmedSequence = null;
        errorCode = null;
      }
    }
    notifyListeners();
  }

  Future<void> refresh() {
    if (_disposed || !supported) return Future.value();
    return _refresh ??= _refreshOnce().whenComplete(() => _refresh = null);
  }

  Future<void> _reconcileActivation(int generation) {
    if (_confirmedActivationId == null) return Future.value();
    if (_activationReadback != null) return _activationReadback!;
    late final Future<void> work;
    work =
        Future<void>.sync(() async {
          while (!_disposed && generation == _generation) {
            final operation = _confirmedActivationId;
            if (operation == null) return;
            await afterActivation?.call();
            if (_disposed || generation != _generation) return;
            // An activation can supersede an asynchronous recovery readback.
            // Its callers must join a read of the new selection too.
            if (_confirmedActivationId != operation) continue;
            pendingOperationId = null;
            _confirmedActivationId = null;
            _activationUnconfirmed = false;
            errorCode = _settledError;
            _settledError = null;
          }
        }).whenComplete(() {
          if (identical(_activationReadback, work)) _activationReadback = null;
        });
    _activationReadback = work;
    queryUnconfirmed = true;
    notifyListeners();
    return work;
  }

  /// A new connection must not capture the old account during receipt recovery.
  /// Disconnect bypasses this barrier. An unresolved receipt never authorizes
  /// replay or a connection using an unconfirmed account selection.
  Future<bool> synchronizeActivation() async {
    if (_disposed) return false;
    if (pendingOperationId == null && _activationReadback == null) return true;
    final generation = _generation;
    await refresh();
    return !_disposed &&
        generation == _generation &&
        pendingOperationId == null &&
        _confirmedActivationId == null &&
        _activationReadback == null;
  }

  Future<void> _refreshOnce() async {
    final generation = _generation;
    final sequence = ++_requestSequence;
    try {
      final known = _fullListRequired ? null : catalog?.revision;
      _fullListRequired = false;
      final result = await _command({
        'action': 'list',
        'known_revision': ?known,
      });
      if (_disposed || generation != _generation) return;
      _accept(result, sequence);
      await _reconcileActivation(generation);
      if (_disposed || generation != _generation) return;
      if (sequence < _acceptedSequence) return;
      queryUnconfirmed = false;
      if (!unconfirmed && errorCode == _queryErrorCode) errorCode = null;
      _queryErrorCode = null;
    } on Object catch (error) {
      if (_disposed || generation != _generation) return;
      if (sequence < _acceptedSequence) return;
      queryUnconfirmed = true;
      _queryErrorCode = error is EngineException
          ? error.code
          : 'SCENARIO_QUERY_FAILED';
      errorCode = _queryErrorCode;
    }
    if (!_disposed) notifyListeners();
    if (_visible &&
        catalog?.applyStatus == 'applying' &&
        _pollAttempts++ < 30) {
      _refreshTimer?.cancel();
      _refreshTimer = Timer(
        const Duration(seconds: 1),
        () => unawaited(refresh()),
      );
    }
  }

  Future<Map<String, Object?>?> _run(
    Map<String, Object?> request, {
    bool activation = false,
  }) async {
    if (_disposed || busy || !supported) return null;
    final generation = _generation;
    final sequence = ++_requestSequence;
    final previousUnknown = _activationUnconfirmed;
    final previousOperation = pendingOperationId;
    final previousConfirmation = _confirmedActivationId;
    final mutation = !const {
      'list',
      'preview',
      'preview_restore',
    }.contains(request['action']);
    busy = true;
    errorCode = null;
    _queryErrorCode = null;
    if (activation) {
      pendingOperationId = request['operation_id'] as String;
      _confirmedActivationId = null;
      _settledError = null;
      _activationSequence = sequence;
      _pollAttempts = 0;
    }
    notifyListeners();
    try {
      final result = await _command(request);
      if (_disposed || generation != _generation) return null;
      _accept(result, sequence);
      if (mutation && result['persisted'] != true) {
        if (activation) {
          _activationUnconfirmed = true;
        } else {
          _templateUnconfirmedSequence = sequence;
        }
        return null;
      }
      if (activation) {
        // This request's direct durable reply is also a receipt. Recovered
        // receipts use the same readback, including retries after read failure.
        _confirmedActivationId = pendingOperationId;
        _activationUnconfirmed = false;
        try {
          await _reconcileActivation(generation);
          if (_disposed || generation != _generation) return null;
          queryUnconfirmed = false;
        } on Object {
          if (_disposed || generation != _generation) return null;
          queryUnconfirmed = true;
          errorCode = 'SCENARIO_QUERY_FAILED';
        }
      }
      return result;
    } on Object catch (error) {
      if (!_disposed && generation == _generation) {
        errorCode = error is EngineException
            ? error.code
            : 'SCENARIO_OPERATION_FAILED';
        final rejected =
            error is EngineException && _knownRejections.contains(error.code);
        if (mutation && !rejected) {
          if (activation) {
            _activationUnconfirmed = true;
          } else {
            _templateUnconfirmedSequence = sequence;
          }
        }
        if (activation && rejected) {
          pendingOperationId = previousOperation;
          _confirmedActivationId = previousConfirmation;
          _activationUnconfirmed = previousUnknown;
        }
      }
      return null;
    } finally {
      if (!_disposed && generation == _generation) {
        busy = false;
        notifyListeners();
        if (activation) unawaited(refresh());
      }
    }
  }

  static const _knownRejections = {
    'SCENARIO_INVALID_REQUEST',
    'SCENARIO_INVALID_SETTINGS',
    'SCENARIO_INVALID_NAME',
    'SCENARIO_LIMIT',
    'SCENARIO_NOT_FOUND',
    'SCENARIO_STALE_REVISION',
    'SCENARIO_ACCOUNT_UNAVAILABLE',
    'SCENARIO_PREVIEW_STALE',
    'SCENARIO_RESTORE_UNAVAILABLE',
    'SCENARIO_OPERATION_REUSED',
    'SCENARIO_BUSY',
    'SCENARIO_PLATFORM_UNSUPPORTED',
    'SCENARIO_VPN_PERMISSION_REQUIRED',
    'SCENARIO_DEPENDENCY_UNAVAILABLE',
    'SCENARIO_RULES_UNAVAILABLE',
    'SCENARIO_PROTECTION_UNAVAILABLE',
    'SCENARIOS_UNSUPPORTED',
    // Owner checks that fail before any storage change.
    'SCENARIO_RUNTIME_UNCONFIRMED',
    'SCENARIO_STORAGE_UNAVAILABLE',
    'SCENARIO_SAVE_FAILED',
    'SCENARIO_CANCELLED',
  };

  Future<ConnectionScenario?> save({
    ConnectionScenario? previous,
    required String name,
    required String? boundAccountId,
    required ScenarioSettings settings,
  }) async {
    final result = await _run({
      'action': 'save',
      if (previous != null) 'id': previous.id,
      if (previous != null) 'expected_revision': previous.revision,
      'name': name,
      'bound_account_id': boundAccountId,
      'settings': settings.toMap(),
    });
    if (result == null) return null;
    final savedCatalog = ScenarioCatalog.fromMap(result);
    final savedId = result['saved_scenario_id'];
    final saved = savedCatalog.scenarios
        .where(
          (s) => s.id == savedId && (previous == null || s.id == previous.id),
        )
        .firstOrNull;
    if (saved == null) {
      _templateUnconfirmedSequence = _requestSequence;
      errorCode = 'SCENARIO_RESULT_UNKNOWN';
      notifyListeners();
    }
    return saved;
  }

  Future<bool> delete(ConnectionScenario scene) async =>
      await _run({
        'action': 'delete',
        'id': scene.id,
        'expected_revision': scene.revision,
      }) !=
      null;
  Future<ScenarioPreview?> preview(ConnectionScenario scene) async {
    final result = await _run({
      'action': 'preview',
      'id': scene.id,
      'expected_revision': scene.revision,
    });
    if (result == null) return null;
    final value = ScenarioPreview.fromMap(result);
    return value.token.isEmpty ? null : value;
  }

  Future<ScenarioPreview?> previewRestore() async {
    final result = await _run({'action': 'preview_restore'});
    return result == null ? null : ScenarioPreview.fromMap(result);
  }

  Future<bool> activate(ScenarioPreview preview) async =>
      await _run({
        'action': preview.restore ? 'restore' : 'activate',
        if (!preview.restore) 'id': preview.scenarioId,
        if (!preview.restore) 'expected_revision': preview.scenarioRevision,
        'preview_token': preview.token,
        'operation_id': _operationId(),
      }, activation: true) !=
      null;

  void reset() {
    _generation++;
    _acceptedSequence = ++_requestSequence;
    _refreshTimer?.cancel();
    queryUnconfirmed = false;
    catalog = null;
    pendingOperationId = null;
    _confirmedActivationId = null;
    _activationReadback = null;
    _settledError = null;
    _fullListRequired = false;
    busy = false;
    _activationUnconfirmed = false;
    _templateUnconfirmedSequence = null;
    errorCode = null;
    notifyListeners();
  }

  @override
  void dispose() {
    _disposed = true;
    _generation++;
    _refreshTimer?.cancel();
    super.dispose();
  }
}

String _operationId() {
  final bytes = List.generate(16, (_) => Random.secure().nextInt(256));
  bytes[6] = (bytes[6] & 15) | 64;
  bytes[8] = (bytes[8] & 63) | 128;
  final hex = bytes.map((b) => b.toRadixString(16).padLeft(2, '0')).join();
  return '${hex.substring(0, 8)}-${hex.substring(8, 12)}-${hex.substring(12, 16)}-${hex.substring(16, 20)}-${hex.substring(20)}';
}
