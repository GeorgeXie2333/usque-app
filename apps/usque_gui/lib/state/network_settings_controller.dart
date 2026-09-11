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
  final Map<String, _SaveAttempt> _saveAttempts = {};
  int _observationRevision = 0;
  bool supported = false;
  bool _disposed = false;

  Future<void> get flushed => _tail;
  bool get unconfirmed =>
      _queryUnconfirmed ||
      _saveAttempts.values.any((attempt) => attempt.unconfirmed);

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
    if (confirmed != null && confirmed.persisted == true) {
      final attempt = _saveAttempts.remove(confirmed.operationId);
      // The in-flight save keeps this record even after a newer snapshot
      // replaces the acknowledgement and before its own reply arrives.
      if (attempt != null) attempt.acknowledged = true;
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

  Future<bool> save(UsqueProfile values, List<String> fields) =>
      enqueue(() async {
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
        final attempt = _SaveAttempt();
        _saveAttempts[operationId] = attempt;
        saveError = null;
        _notify();
        try {
          final result = await _engine.saveNetworkSettings(
            operationId,
            values.id,
            values,
            fields,
          );
          accept(result);
          attempt.unconfirmed = !attempt.acknowledged;
          return attempt.acknowledged;
        } on Object catch (error) {
          if (attempt.acknowledged) return true;
          final definitive =
              error is EngineException &&
              !error.code.startsWith('ENGINE_') &&
              error.code != 'NETWORK_SETTINGS_UNCONFIRMED';
          if (definitive) {
            _saveAttempts.remove(operationId);
            saveError = error.code;
          } else {
            attempt.unconfirmed = true;
            // A mutation is never replayed after an ambiguous reply.
            try {
              accept(await _engine.getNetworkSettingsState());
            } on Object {
              /* Retain unknown state and the page's draft. */
            }
          }
          return attempt.acknowledged;
        } finally {
          _notify();
        }
      });

  void _notify() {
    if (!_disposed) notifyListeners();
  }

  @override
  void dispose() {
    _disposed = true;
    super.dispose();
  }
}

class _SaveAttempt {
  bool acknowledged = false;
  bool unconfirmed = false;
}

String _operationId() {
  final random = Random.secure();
  final bytes = List.generate(16, (_) => random.nextInt(256));
  bytes[6] = (bytes[6] & 15) | 64;
  bytes[8] = (bytes[8] & 63) | 128;
  final hex = bytes.map((b) => b.toRadixString(16).padLeft(2, '0')).join();
  return '${hex.substring(0, 8)}-${hex.substring(8, 12)}-${hex.substring(12, 16)}-${hex.substring(16, 20)}-${hex.substring(20)}';
}
