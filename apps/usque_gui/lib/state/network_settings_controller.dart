import 'dart:async';
import 'dart:math';

import 'package:flutter/foundation.dart';

import '../models/app_models.dart';
import '../services/engine_client.dart';

/// Owns the submission order and confirmed settings, never a page's draft.
class NetworkSettingsController extends ChangeNotifier {
  NetworkSettingsController(this._engine);
  final EngineClient _engine;
  Future<void> _tail = Future.value();
  final Set<String> _retiredEpochs = {};
  NetworkSettingsState? state;
  String? saveError;
  bool _queryUnconfirmed = false;
  String? _unconfirmedOperationId;
  int _observationRevision = 0;
  bool supported = false;
  bool _disposed = false;

  Future<void> get flushed => _tail;
  bool get unconfirmed => _queryUnconfirmed || _unconfirmedOperationId != null;

  Future<T> enqueue<T>(Future<T> Function() operation) {
    final result = Completer<T>();
    _tail = _tail.then((_) async {
      try {
        result.complete(await operation());
      } on Object catch (error, stack) {
        result.completeError(error, stack);
      }
    });
    return result.future;
  }

  void accept(NetworkSettingsState incoming) {
    if (_retiredEpochs.contains(incoming.sourceEpoch)) return;
    final previous = state;
    if (previous != null) {
      if (previous.sourceEpoch == incoming.sourceEpoch &&
          incoming.sequence < previous.sequence) {
        return;
      }
      // A successful query may return the unchanged snapshot. It confirms
      // communication without allowing duplicate contents to replace state.
      if (previous.sourceEpoch == incoming.sourceEpoch &&
          incoming.sequence == previous.sequence) {
        incoming = previous;
      }
      if (previous.sourceEpoch != incoming.sourceEpoch) {
        _retiredEpochs.add(previous.sourceEpoch);
      }
    }
    final changed = !identical(state, incoming);
    final wasUnconfirmed = unconfirmed;
    state = incoming;
    _observationRevision++;
    _queryUnconfirmed = false;
    _confirmPendingSave();
    if (changed || wasUnconfirmed != unconfirmed) _notify();
  }

  void _confirmPendingSave() {
    final confirmed = state;
    if (confirmed != null &&
        confirmed.operationId == _unconfirmedOperationId &&
        confirmed.persisted == true) {
      _unconfirmedOperationId = null;
    }
  }

  Future<void> refresh() async {
    if (!supported) return;
    final revision = _observationRevision;
    try {
      accept(await _engine.getNetworkSettingsState());
    } on Object {
      // An older query failure must not invalidate a newer authoritative reply.
      if (revision == _observationRevision) {
        _queryUnconfirmed = true;
        _notify();
      }
    }
  }

  Future<bool> save(UsqueProfile values, List<String> fields) => enqueue(
    () async {
      if (!supported) {
        try {
          supported =
              (await _engine.getCapabilities())?.networkSettingsApplication ??
              false;
        } on Object {
          supported = false;
        }
      }
      if (!supported) {
        saveError = 'NETWORK_SETTINGS_UNSUPPORTED';
        _notify();
        return false;
      }
      final operationId = _operationId();
      saveError = null;
      _queryUnconfirmed = false;
      _unconfirmedOperationId = null;
      _notify();
      try {
        final result = await _engine.saveNetworkSettings(
          operationId,
          values.id,
          values,
          fields,
        );
        accept(result);
        final saved =
            result.operationId == operationId && result.persisted == true;
        _unconfirmedOperationId = saved ? null : operationId;
        _confirmPendingSave();
        _notify();
        return saved;
      } on Object catch (error) {
        final definitive =
            error is EngineException &&
            !error.code.startsWith('ENGINE_') &&
            error.code != 'NETWORK_SETTINGS_UNCONFIRMED';
        if (definitive) {
          saveError = error.code;
        } else {
          _unconfirmedOperationId = operationId;
          _confirmPendingSave();
          if (_unconfirmedOperationId == null) {
            _notify();
            return true;
          }
          // A mutation is never replayed after an ambiguous reply.
          try {
            final result = await _engine.getNetworkSettingsState();
            accept(result);
            if (result.operationId == operationId && result.persisted == true) {
              _unconfirmedOperationId = null;
              _notify();
              return true;
            }
          } on Object {
            /* Retain unknown state and the page's draft. */
          }
        }
        _notify();
        return false;
      }
    },
  );

  void _notify() {
    if (!_disposed) notifyListeners();
  }

  @override
  void dispose() {
    _disposed = true;
    super.dispose();
  }
}

String _operationId() {
  final random = Random.secure();
  final bytes = List.generate(16, (_) => random.nextInt(256));
  bytes[6] = (bytes[6] & 15) | 64;
  bytes[8] = (bytes[8] & 63) | 128;
  final hex = bytes.map((b) => b.toRadixString(16).padLeft(2, '0')).join();
  return '${hex.substring(0, 8)}-${hex.substring(8, 12)}-${hex.substring(12, 16)}-${hex.substring(16, 20)}-${hex.substring(20)}';
}
