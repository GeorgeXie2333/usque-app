import 'dart:async';
import 'dart:math';

import 'package:flutter/foundation.dart';

import '../models/app_models.dart';
import '../models/endpoint_subscriptions.dart';
import '../services/engine_client.dart';

/// Coordinates endpoint subscription CRUD, activation, and live MASQUE
/// probing without storing endpoint inventories, URLs, or notes in logs.
class EndpointSubscriptionsController extends ChangeNotifier {
  EndpointSubscriptionsController(this._command);

  final Future<Map<String, Object?>> Function(Map<String, Object?>) _command;

  bool supported = false;
  bool domainEndpointsSupported = false;
  bool probeSupported = false;
  bool connectedTunProbeSupported = false;

  EndpointSubscriptionCatalog? catalog;
  bool loading = false;
  bool busy = false;
  String? errorCode;
  String? lastProbeErrorCode;
  EndpointProbeProgress? activeProbeProgress;

  final Map<String, Map<String, EndpointProbeResult>> _probeResults = {};
  final Set<String> _localRefreshingIds = {};
  Future<void> _serial = Future<void>.value();
  Future<void>? _refreshWork;
  bool _disposed = false;
  int _generation = 0;

  Future<void> Function()? onMutationApplied;
  Future<void> Function()? onCatalogChanged;

  Map<String, EndpointProbeResult> probeResultsFor(String subscriptionId) =>
      Map<String, EndpointProbeResult>.unmodifiable(
        _probeResults[subscriptionId] ?? const <String, EndpointProbeResult>{},
      );

  bool isRefreshing(String subscriptionId) =>
      _localRefreshingIds.contains(subscriptionId) ||
      (catalog?.refreshingSubscriptionIds.contains(subscriptionId) ?? false);

  /// Probes that reached a terminal state, were cancelled or were superseded.
  /// A catalogue snapshot taken before that point, or a late event, must not
  /// make them look running again.
  final Set<String> _retiredProbeIds = <String>{};

  void _retireProbe(String probeId) {
    _retiredProbeIds.add(probeId);
    while (_retiredProbeIds.length > 32) {
      _retiredProbeIds.remove(_retiredProbeIds.first);
    }
  }

  String? get _catalogProbeId {
    final id = catalog?.activeProbeId;
    return id == null || _retiredProbeIds.contains(id) ? null : id;
  }

  String? get runningProbeId =>
      activeProbeProgress?.state == EndpointProbeState.running
      ? activeProbeProgress!.probeId
      : _catalogProbeId;

  String? get runningProbeSubscriptionId =>
      activeProbeProgress?.state == EndpointProbeState.running
      ? activeProbeProgress!.subscriptionId
      : _catalogProbeId == null
      ? null
      : catalog?.activeProbeSubscriptionId;

  bool get probeRunning => runningProbeId != null;

  void setCapabilities(EngineCapabilities? capabilities) {
    final nextSupported = capabilities?.endpointSubscriptions ?? false;
    final nextDomain =
        capabilities?.endpointSubscriptionDomainEndpoints ?? false;
    final nextProbe = capabilities?.endpointSubscriptionProbe ?? false;
    final nextTunProbe =
        capabilities?.endpointSubscriptionConnectedTunProbe ?? false;
    final changed =
        supported != nextSupported ||
        domainEndpointsSupported != nextDomain ||
        probeSupported != nextProbe ||
        connectedTunProbeSupported != nextTunProbe;
    final newlySupported = !supported && nextSupported;
    supported = nextSupported;
    domainEndpointsSupported = nextDomain;
    probeSupported = nextProbe;
    connectedTunProbeSupported = nextTunProbe;
    if (!supported) {
      _probeResults.clear();
      activeProbeProgress = null;
      lastProbeErrorCode = null;
    }
    if (changed && !_disposed) notifyListeners();
    if (newlySupported) unawaited(refresh(force: true));
  }

  Future<T> _enqueue<T>(Future<T> Function() action) {
    final completer = Completer<T>();
    _serial = _serial.then((_) async {
      try {
        completer.complete(await action());
      } on Object catch (error, stack) {
        completer.completeError(error, stack);
      }
    });
    return completer.future;
  }

  void _acceptCatalog(Map<String, Object?> reply, {int? expectedGen}) {
    if (_disposed || (expectedGen != null && expectedGen != _generation)) {
      return;
    }
    if (!reply.containsKey('revision') && !reply.containsKey('subscriptions')) {
      return;
    }
    final prevAccountId = catalog?.activeAccountId;
    final next = EndpointSubscriptionCatalog.fromMap(reply, previous: catalog);
    if (prevAccountId != null &&
        next.activeAccountId != null &&
        prevAccountId != next.activeAccountId) {
      _probeResults.clear();
      activeProbeProgress = null;
      lastProbeErrorCode = null;
    }
    catalog = next;
    errorCode = null;
  }

  Future<void> refresh({bool force = false}) {
    if (_disposed || !supported) return Future<void>.value();
    if (!force && _refreshWork != null) return _refreshWork!;
    final gen = _generation;
    late final Future<void> work;
    work =
        _enqueue(() async {
          if (_disposed || !supported || gen != _generation) return;
          if (catalog == null) {
            loading = true;
            _notify();
          }
          try {
            final reply = await _command(<String, Object?>{
              'action': 'list',
              if (!force && catalog != null && catalog!.revision.isNotEmpty)
                'known_revision': catalog!.revision,
            });
            _acceptCatalog(reply, expectedGen: gen);
          } on Object catch (error) {
            if (gen == _generation) {
              errorCode = _errorCode(error);
            }
          } finally {
            loading = false;
            _notify();
          }
        }).whenComplete(() {
          if (identical(_refreshWork, work)) _refreshWork = null;
        });
    _refreshWork = work;
    return work;
  }

  Future<({EndpointSubscription? subscription, String? errorCode})>
  addSubscription({
    required String name,
    required String url,
    required EndpointSubscriptionTier accountTier,
    SubscriptionRefreshInterval autoRefresh = SubscriptionRefreshInterval.off,
  }) {
    if (_disposed || !supported) {
      return Future.value((
        subscription: null,
        errorCode: 'SUBSCRIPTION_UNAVAILABLE',
      ));
    }
    final gen = _generation;
    return _enqueue(() async {
      busy = true;
      errorCode = null;
      _notify();
      try {
        final reply = await _command(<String, Object?>{
          'action': 'add',
          'name': name.trim(),
          'url': url.trim(),
          'account_tier': accountTier.wireName,
          'auto_refresh': autoRefresh.wireName,
        });
        _acceptCatalog(reply, expectedGen: gen);
        final subMap = reply['subscription'];
        final added = subMap is Map
            ? EndpointSubscription.fromMap(subMap)
            : null;
        if (onCatalogChanged != null && gen == _generation && !_disposed) {
          await onCatalogChanged!();
        }
        return (subscription: added, errorCode: null);
      } on Object catch (error) {
        final code = _errorCode(error);
        if (gen == _generation) errorCode = code;
        return (subscription: null, errorCode: code);
      } finally {
        busy = false;
        _notify();
      }
    });
  }

  Future<({bool ok, String? errorCode})> updateSubscription({
    required String id,
    required String name,
    required String url,
    required EndpointSubscriptionTier accountTier,
    required SubscriptionRefreshInterval autoRefresh,
    required String expectedRevision,
  }) {
    if (_disposed || !supported) {
      return Future.value((ok: false, errorCode: 'SUBSCRIPTION_UNAVAILABLE'));
    }
    final gen = _generation;
    return _enqueue(() async {
      busy = true;
      errorCode = null;
      _notify();
      try {
        final reply = await _command(<String, Object?>{
          'action': 'update',
          'id': id,
          'name': name.trim(),
          'url': url.trim(),
          'account_tier': accountTier.wireName,
          'auto_refresh': autoRefresh.wireName,
          'expected_revision': expectedRevision,
        });
        _acceptCatalog(reply, expectedGen: gen);
        if (onCatalogChanged != null && gen == _generation && !_disposed) {
          await onCatalogChanged!();
        }
        if (onMutationApplied != null && gen == _generation && !_disposed) {
          await onMutationApplied!();
        }
        return (ok: true, errorCode: null);
      } on Object catch (error) {
        final code = _errorCode(error);
        if (gen == _generation) errorCode = code;
        return (ok: false, errorCode: code);
      } finally {
        busy = false;
        _notify();
      }
    });
  }

  Future<({bool ok, String? errorCode})> deleteSubscription({
    required String id,
    required String expectedRevision,
  }) {
    if (_disposed || !supported) {
      return Future.value((ok: false, errorCode: 'SUBSCRIPTION_UNAVAILABLE'));
    }
    final gen = _generation;
    return _enqueue(() async {
      busy = true;
      errorCode = null;
      _notify();
      try {
        final reply = await _command(<String, Object?>{
          'action': 'delete',
          'id': id,
          'expected_revision': expectedRevision,
        });
        _probeResults.remove(id);
        if (activeProbeProgress?.subscriptionId == id) {
          activeProbeProgress = null;
        }
        _acceptCatalog(reply, expectedGen: gen);
        if (onMutationApplied != null && gen == _generation && !_disposed) {
          await onMutationApplied!();
        }
        if (onCatalogChanged != null && gen == _generation && !_disposed) {
          await onCatalogChanged!();
        }
        return (ok: true, errorCode: null);
      } on Object catch (error) {
        final code = _errorCode(error);
        if (gen == _generation) errorCode = code;
        return (ok: false, errorCode: code);
      } finally {
        busy = false;
        _notify();
      }
    });
  }

  Future<({bool ok, String? errorCode})> refreshSubscription(String id) async {
    if (_disposed || !supported) {
      return (ok: false, errorCode: 'SUBSCRIPTION_UNAVAILABLE');
    }
    final gen = _generation;
    _localRefreshingIds.add(id);
    errorCode = null;
    _notify();
    try {
      final reply = await _command(<String, Object?>{
        'action': 'refresh',
        'id': id,
      });
      _acceptCatalog(reply, expectedGen: gen);
      return (ok: true, errorCode: null);
    } on Object catch (error) {
      final code = _errorCode(error);
      if (gen == _generation) errorCode = code;
      return (ok: false, errorCode: code);
    } finally {
      _localRefreshingIds.remove(id);
      _notify();
    }
  }

  Future<void> cancelRefresh(String id) async {
    if (_disposed || !supported) return;
    final gen = _generation;
    try {
      final reply = await _command(<String, Object?>{
        'action': 'cancel_refresh',
        'id': id,
      });
      // A reply from before Clear All Data must not restore that catalogue.
      _acceptCatalog(reply, expectedGen: gen);
    } on Object {
      // Cancellation is best-effort.
    } finally {
      _localRefreshingIds.remove(id);
      _notify();
    }
  }

  Future<({bool applied, String? applyStatus, String? errorCode})>
  selectEndpoint({
    required String subscriptionId,
    required String endpointId,
    bool zeroTrustRiskAcknowledged = false,
  }) {
    if (_disposed || !supported) {
      return Future.value((
        applied: false,
        applyStatus: null,
        errorCode: 'SUBSCRIPTION_UNAVAILABLE',
      ));
    }
    final gen = _generation;
    return _enqueue(() async {
      busy = true;
      errorCode = null;
      _notify();
      try {
        final reply = await _command(<String, Object?>{
          'action': 'select_endpoint',
          'subscription_id': subscriptionId,
          'endpoint_id': endpointId,
          'operation_id': _newUuidV4(),
          'zero_trust_risk_acknowledged': zeroTrustRiskAcknowledged,
        });
        _acceptCatalog(reply, expectedGen: gen);
        if (onMutationApplied != null && gen == _generation && !_disposed) {
          await onMutationApplied!();
        }
        return (
          applied: true,
          applyStatus: reply['apply_status'] as String?,
          errorCode: null,
        );
      } on Object catch (error) {
        final code = _errorCode(error);
        if (gen == _generation) errorCode = code;
        return (applied: false, applyStatus: null, errorCode: code);
      } finally {
        busy = false;
        _notify();
      }
    });
  }

  Future<({bool applied, String? applyStatus, String? errorCode})>
  deactivate() {
    if (_disposed || !supported) {
      return Future.value((
        applied: false,
        applyStatus: null,
        errorCode: 'SUBSCRIPTION_UNAVAILABLE',
      ));
    }
    final gen = _generation;
    return _enqueue(() async {
      busy = true;
      errorCode = null;
      _notify();
      try {
        final reply = await _command(<String, Object?>{
          'action': 'deactivate',
          'operation_id': _newUuidV4(),
        });
        _acceptCatalog(reply, expectedGen: gen);
        if (onMutationApplied != null && gen == _generation && !_disposed) {
          await onMutationApplied!();
        }
        return (
          applied: true,
          applyStatus: reply['apply_status'] as String?,
          errorCode: null,
        );
      } on Object catch (error) {
        final code = _errorCode(error);
        if (gen == _generation) errorCode = code;
        return (applied: false, applyStatus: null, errorCode: code);
      } finally {
        busy = false;
        _notify();
      }
    });
  }

  Future<({bool ok, String? errorCode})> setPinned({
    required String subscriptionId,
    required String endpointId,
    required bool pinned,
  }) {
    if (_disposed || !supported) {
      return Future.value((ok: false, errorCode: 'SUBSCRIPTION_UNAVAILABLE'));
    }
    final gen = _generation;
    return _enqueue(() async {
      try {
        final reply = await _command(<String, Object?>{
          'action': 'set_pinned',
          'subscription_id': subscriptionId,
          'endpoint_id': endpointId,
          'pinned': pinned,
        });
        _acceptCatalog(reply, expectedGen: gen);
        return (ok: true, errorCode: null);
      } on Object catch (error) {
        final code = _errorCode(error);
        if (gen == _generation) errorCode = code;
        return (ok: false, errorCode: code);
      } finally {
        _notify();
      }
    });
  }

  Future<({bool started, String? errorCode})> startProbe(
    String subscriptionId, {
    List<String>? endpointIds,
  }) async {
    if (_disposed || !supported || !probeSupported) {
      return (started: false, errorCode: 'SUBSCRIPTION_PROBE_UNAVAILABLE');
    }
    final gen = _generation;
    final probeId = _newUuidV4();
    // The Engine replaces a running probe; its late events are stale.
    if (runningProbeId case final previous?) _retireProbe(previous);
    lastProbeErrorCode = null;
    errorCode = null;
    final sub = catalog?.byId(subscriptionId);
    final total =
        endpointIds?.length ??
        ((sub?.endpoints.length ?? 0).clamp(0, 512).toInt());
    activeProbeProgress = EndpointProbeProgress(
      probeId: probeId,
      subscriptionId: subscriptionId,
      state: EndpointProbeState.running,
      completed: 0,
      total: total,
    );
    if (endpointIds == null) {
      _probeResults.remove(subscriptionId);
    } else {
      final bucket = _probeResults[subscriptionId];
      if (bucket != null) {
        for (final id in endpointIds) {
          bucket.remove(id);
        }
      }
    }
    _notify();
    try {
      final reply = await _command(<String, Object?>{
        'action': 'start_probe',
        'subscription_id': subscriptionId,
        'endpoint_ids': ?endpointIds,
        'probe_id': probeId,
      });
      _acceptCatalog(reply, expectedGen: gen);
      _notify();
      return (started: true, errorCode: null);
    } on Object catch (error) {
      final code = _errorCode(error);
      _retireProbe(probeId);
      if (gen == _generation) {
        if (activeProbeProgress?.probeId == probeId) {
          activeProbeProgress = null;
        }
        lastProbeErrorCode = code;
      }
      _notify();
      return (started: false, errorCode: code);
    }
  }

  Future<void> cancelProbe() async {
    if (_disposed || !supported) return;
    final probeId = runningProbeId;
    if (probeId == null) return;
    final gen = _generation;
    // Even if the request fails, this probe is no longer shown as running.
    _retireProbe(probeId);
    try {
      final reply = await _command(<String, Object?>{
        'action': 'cancel_probe',
        'probe_id': probeId,
      });
      _acceptCatalog(reply, expectedGen: gen);
    } on Object {
      // Cancellation is best-effort.
    } finally {
      if (activeProbeProgress?.probeId == probeId) {
        activeProbeProgress = EndpointProbeProgress(
          probeId: probeId,
          subscriptionId: activeProbeProgress!.subscriptionId,
          state: EndpointProbeState.cancelled,
          completed: activeProbeProgress!.completed,
          total: activeProbeProgress!.total,
          results: activeProbeProgress!.results,
        );
      }
      _notify();
    }
  }

  void acceptProbeProgress(Map<String, Object?> raw) {
    if (_disposed || !supported) return;
    final EndpointProbeProgress progress;
    try {
      progress = EndpointProbeProgress.fromMap(raw);
    } on FormatException {
      return;
    }
    // A cancelled, failed-to-start or superseded probe cannot reappear.
    if (_retiredProbeIds.contains(progress.probeId)) return;
    final activeId = activeProbeProgress?.probeId ?? catalog?.activeProbeId;
    if (activeId != null && activeId != progress.probeId) {
      // Ignore stale progress from a superseded probe unless we have none.
      if (activeProbeProgress?.state == EndpointProbeState.running) return;
    }
    final bucket = _probeResults.putIfAbsent(
      progress.subscriptionId,
      () => <String, EndpointProbeResult>{},
    );
    for (final result in progress.results) {
      bucket[result.endpointId] = result;
    }
    if (progress.state == EndpointProbeState.running) {
      activeProbeProgress = progress;
      lastProbeErrorCode = null;
    } else {
      activeProbeProgress = progress;
      lastProbeErrorCode = progress.errorCode;
      // A terminal event may precede the start reply; keep it terminal.
      _retireProbe(progress.probeId);
      if (catalog case final current?
          when current.activeProbeId == progress.probeId) {
        catalog = EndpointSubscriptionCatalog(
          revision: current.revision,
          unchanged: current.unchanged,
          subscriptions: current.subscriptions,
          activeAccountId: current.activeAccountId,
          activeAccountTier: current.activeAccountTier,
          activeSubscriptionId: current.activeSubscriptionId,
          activeEndpointId: current.activeEndpointId,
          refreshingSubscriptionIds: current.refreshingSubscriptionIds,
        );
      }
    }
    _notify();
  }

  void invalidateProbeResults() {
    if (_disposed) return;
    if (_probeResults.isEmpty &&
        activeProbeProgress == null &&
        lastProbeErrorCode == null) {
      return;
    }
    _probeResults.clear();
    activeProbeProgress = null;
    lastProbeErrorCode = null;
    _notify();
  }

  void reset() {
    _generation++;
    _refreshWork = null;
    _localRefreshingIds.clear();
    _probeResults.clear();
    catalog = null;
    loading = false;
    busy = false;
    errorCode = null;
    lastProbeErrorCode = null;
    activeProbeProgress = null;
    _notify();
  }

  @override
  void dispose() {
    _disposed = true;
    super.dispose();
  }

  void _notify() {
    if (!_disposed) notifyListeners();
  }

  static String _errorCode(Object error) {
    if (error is EngineException) return error.code;
    return 'SUBSCRIPTION_STORAGE_UNAVAILABLE';
  }

  static String _newUuidV4() {
    final random = Random.secure();
    final bytes = List<int>.generate(16, (_) => random.nextInt(256));
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    String hex(int start, int end) => bytes
        .sublist(start, end)
        .map((byte) => byte.toRadixString(16).padLeft(2, '0'))
        .join();
    return '${hex(0, 4)}-${hex(4, 6)}-${hex(6, 8)}-${hex(8, 10)}-${hex(10, 16)}';
  }
}
