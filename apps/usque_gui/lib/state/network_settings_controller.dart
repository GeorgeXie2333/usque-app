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
  // Only an ordinal is retained; backend target text never reaches diagnostics.
  int? invalidBypassDomainEntry;
  List<String> routingErrorIds = [];
  String? routingErrorKey;
  bool _queryUnconfirmed = false;
  final Map<String, _SaveAttempt> _saveAttempts = {};
  int _observationRevision = 0;
  bool _supported = false;
  bool _observationVisible = true;
  Future<void>? _refreshing;
  Timer? _confirmationRetry;
  int _confirmationRetries = 0;
  static const _confirmationRetryDelays = [
    Duration(seconds: 1),
    Duration(seconds: 2),
    Duration(seconds: 4),
  ];
  bool _disposed = false;
  bool _resetting = false;
  int _dataGeneration = 0;

  Future<void> get flushed => _tail;
  bool get unconfirmed =>
      _queryUnconfirmed ||
      _saveAttempts.values.any((attempt) => attempt.unconfirmed);
  bool get needsConfirmation =>
      unconfirmed ||
      state?.operationId != null &&
          state?.status == NetworkSettingsApplyStatus.unknown;
  bool get canRefresh => supported && needsConfirmation;
  bool get refreshing => _refreshing != null;
  bool get confirmationRetriesExhausted =>
      needsConfirmation &&
      _confirmationRetries >= _confirmationRetryDelays.length &&
      _confirmationRetry == null &&
      !refreshing;
  bool get supported => _supported;
  set supported(bool value) {
    _supported = value;
    if (!value) _cancelConfirmationRetry();
  }

  void setObservationVisible(bool visible) {
    if (_disposed || visible == _observationVisible) return;
    _observationVisible = visible;
    if (!visible) {
      _cancelConfirmationRetry();
    } else if (needsConfirmation) {
      unawaited(refresh());
    }
  }

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
    if (_resetting || _disposed) return;
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
    final wasUnsupported =
        supported && saveError == 'NETWORK_SETTINGS_UNSUPPORTED';
    state = incoming;
    if (wasUnsupported) saveError = null;
    _observationRevision++;
    _queryUnconfirmed = false;
    _confirmPendingSave();
    if (changed || wasUnconfirmed != unconfirmed || wasUnsupported) _notify();
  }

  void _confirmPendingSave() {
    final confirmed = state;
    if (confirmed == null) return;
    final operations = [
      ...confirmed.persistedOperationIds,
      if (confirmed.persisted == true && confirmed.operationId != null)
        confirmed.operationId!,
    ];
    for (final operation in operations) {
      final attempt = _saveAttempts.remove(operation);
      // The in-flight save keeps this record even after a newer snapshot
      // replaces the acknowledgement and before its own reply arrives.
      if (attempt != null) attempt.acknowledged = true;
    }
  }

  Future<void> refresh() {
    if (_disposed || !supported || _resetting || !_observationVisible) {
      return Future<void>.value();
    }
    final pending = _refreshing;
    if (pending != null) return pending;
    _cancelConfirmationRetry();
    _confirmationRetries = 0;
    return _startRefresh();
  }

  Future<void> _startRefresh() {
    late final Future<void> work;
    work = _refreshOnce().whenComplete(() {
      if (identical(_refreshing, work)) {
        _refreshing = null;
        _notify();
      }
    });
    _refreshing = work;
    _notify();
    return work;
  }

  Future<void> _refreshOnce() async {
    final generation = _dataGeneration;
    final revision = _observationRevision;
    try {
      final incoming = await _engine.getNetworkSettingsState();
      if (generation == _dataGeneration) accept(incoming);
    } on Object catch (error) {
      // An older query failure must not invalidate a newer authoritative reply.
      if (generation == _dataGeneration && revision == _observationRevision) {
        if (error is EngineException &&
            error.code == 'NETWORK_SETTINGS_UNSUPPORTED') {
          supported = false;
          saveError = error.code;
          _queryUnconfirmed = false;
        } else {
          _queryUnconfirmed = true;
        }
        _notify();
      }
    }
  }

  Future<bool> save(UsqueProfile values, List<String> fields) {
    final generation = _dataGeneration;
    final requestedFields = List<String>.of(fields);
    return enqueue(() async {
      if (_resetting || generation != _dataGeneration) return false;
      if (!supported) {
        try {
          supported =
              (await _engine.getCapabilities())?.networkSettingsApplication ??
              false;
        } on Object {
          supported = false;
        }
      }
      if (_resetting || generation != _dataGeneration) return false;
      if (!supported) {
        saveError = 'NETWORK_SETTINGS_UNSUPPORTED';
        _notify();
        return false;
      }
      final operationId = _operationId();
      final attempt = _SaveAttempt();
      _saveAttempts[operationId] = attempt;
      saveError = null;
      invalidBypassDomainEntry = null;
      routingErrorIds = [];
      routingErrorKey = null;
      _notify();
      try {
        // Earlier queued saves may have changed the DNS mode or exit. Rebase
        // these dependencies immediately before sending the field-scoped edit.
        final prepared = _prepareExitDns(
          values,
          requestedFields,
          state?.sharedNetwork ?? state?.storedProfile,
        );
        final result = await _engine.saveNetworkSettings(
          operationId,
          values.id,
          prepared.values,
          prepared.fields,
        );
        if (_resetting || generation != _dataGeneration) return false;
        // A reply to this request can durably confirm its own save even when
        // newer events have superseded the snapshot it carries. Never roll
        // the current configuration or engine epoch back to that snapshot.
        if (result.operationId == operationId && result.persisted == true) {
          attempt.acknowledged = true;
          _saveAttempts.remove(operationId);
        }
        accept(result);
        attempt.unconfirmed = !attempt.acknowledged;
        return attempt.acknowledged;
      } on Object catch (error) {
        if (_resetting || generation != _dataGeneration) return false;
        if (attempt.acknowledged) return true;
        final definitive =
            error is EngineException &&
            !error.code.startsWith('ENGINE_') &&
            error.code != 'NETWORK_SETTINGS_UNCONFIRMED';
        if (definitive) {
          _saveAttempts.remove(operationId);
          saveError = error.code;
          routingErrorIds = [];
          routingErrorKey = null;
          final routingError = RegExp(
            r'ROUTING_RULE_(CONFLICT|INVALID):([a-fA-F0-9:-]+)',
          ).firstMatch(error.message);
          if (routingError != null) {
            routingErrorIds = routingError.group(2)!.split(':');
            routingErrorKey = routingError.group(1) == 'CONFLICT'
                ? 'routing_conflict'
                : 'invalid_dns_name';
          }
          final invalidDomain = RegExp(
            r'invalid bypass domain at entry (\d+)',
          ).firstMatch(error.message);
          invalidBypassDomainEntry = invalidDomain == null
              ? null
              : int.tryParse(invalidDomain.group(1)!);
        } else {
          attempt.unconfirmed = true;
          // A mutation is never replayed after an ambiguous reply.
          await refresh();
        }
        return attempt.acknowledged;
      } finally {
        _notify();
      }
    });
  }

  void suspendForReset() {
    _dataGeneration++;
    _resetting = true;
    _observationRevision++;
    _cancelConfirmationRetry();
    _confirmationRetries = 0;
    _refreshing = null;
    _saveAttempts.clear();
    _queryUnconfirmed = true;
  }

  void resumeAfterReset() {
    _resetting = false;
  }

  void reset() {
    final epoch = state?.sourceEpoch;
    if (epoch != null) _retiredEpochs.add(epoch);
    _dataGeneration++;
    _cancelConfirmationRetry();
    _confirmationRetries = 0;
    _refreshing = null;
    state = null;
    saveError = null;
    invalidBypassDomainEntry = null;
    routingErrorIds = [];
    routingErrorKey = null;
    _saveAttempts.clear();
    _queryUnconfirmed = false;
    _resetting = false;
    _observationRevision++;
    _notify();
  }

  void _cancelConfirmationRetry() {
    _confirmationRetry?.cancel();
    _confirmationRetry = null;
  }

  void _syncConfirmationRetry() {
    if (!needsConfirmation) {
      _cancelConfirmationRetry();
      _confirmationRetries = 0;
      return;
    }
    if (_disposed ||
        _resetting ||
        !supported ||
        !_observationVisible ||
        refreshing ||
        _confirmationRetry != null ||
        _confirmationRetries >= _confirmationRetryDelays.length) {
      return;
    }
    _confirmationRetry = Timer(
      _confirmationRetryDelays[_confirmationRetries],
      () {
        _confirmationRetry = null;
        _confirmationRetries++;
        unawaited(_startRefresh());
      },
    );
  }

  void _notify() {
    if (_disposed) return;
    _syncConfirmationRetry();
    notifyListeners();
  }

  @override
  void dispose() {
    _disposed = true;
    _cancelConfirmationRetry();
    super.dispose();
  }
}

/// Rebase the exit/DNS dependencies of every field-scoped edit before encoding.
/// Only exit/mode edits repair an inherited, now unsupported DNS choice.
({UsqueProfile values, List<String> fields}) _prepareExitDns(
  UsqueProfile values,
  List<String> fields,
  UsqueProfile? confirmed,
) {
  final changesExit = fields.any(
    (field) => const ['chain_exit', 'vpn_gate', 'data_plane'].contains(field),
  );
  final changesDns = fields.contains('proxy.dns_mode');
  final current = confirmed ?? values;
  final legacyGateEdit =
      fields.contains('vpn_gate') &&
      !fields.contains('chain_exit') &&
      values.chainExit == null;
  if (legacyGateEdit &&
      current.chainSource != ChainSource.vpnGate &&
      current.chainExit?.profileId != null) {
    // A legacy Gate edit must not erase a retained custom selection. Rust
    // rejects this combination, including when the old chain is disabled.
    return (values: values, fields: fields);
  }
  final chain = legacyGateEdit
      ? null
      : fields.contains('chain_exit')
      ? values.chainExit
      : current.chainExit;
  var target = values.copyWith(
    dataPlane: fields.contains('data_plane')
        ? values.dataPlane
        : current.dataPlane,
    vpnGate: fields.contains('vpn_gate') ? values.vpnGate : current.vpnGate,
    chainExit: chain,
    clearChainExit: chain == null,
    proxy: values.proxy.copyWith(
      dnsMode: changesDns ? values.proxy.dnsMode : current.proxy.dnsMode,
    ),
  );
  if (!changesExit ||
      changesDns ||
      target.proxy.dnsMode != ProxyDnsMode.edgeResolved ||
      target.dataPlane == DataPlaneMode.l4Proxy ||
      target.chainExit?.enabled == true && target.chainSource.isProxy) {
    return (values: target, fields: fields);
  }
  target = target.copyWith(
    proxy: target.proxy.copyWith(dnsMode: ProxyDnsMode.remote),
  );
  return (values: target, fields: [...fields, 'proxy.dns_mode']);
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
