import 'dart:async';

import '../models/app_models.dart';
import 'app_controller.dart';

/// An unsaved configuration reference owned by one native process. A selection
/// alone does not prove that its downloaded object survived a process restart.
class ScenarioGateLease {
  const ScenarioGateLease(this.selection, this.sourceEpoch);
  final VpnGateSettings selection;
  final String sourceEpoch;

  bool matches(VpnGateSettings value) =>
      value.hasSelection &&
      selection.serverId == value.serverId &&
      selection.configSha256 == value.configSha256;
}

/// Serializes this GUI's scene draft cleanup and preparation. Native epoch
/// preconditions additionally prevent a delayed old-owner release from deleting
/// the same selected object after a new process has prepared it again.
class ScenarioGateLeases {
  ScenarioGateLeases._(this.app);
  static final _instances = Expando<ScenarioGateLeases>();
  static ScenarioGateLeases of(AppController app) =>
      _instances[app] ??= ScenarioGateLeases._(app);
  final AppController app;
  Future<void> _tail = Future.value();
  final _uncertainCleanup = <(String, String, String)>{};
  final _released = Expando<bool>();
  final _controlReleases = Expando<Future<void> Function()>();

  Future<T> _enqueue<T>(Future<T> Function() work) {
    final result = Completer<T>();
    _tail = _tail.then((_) async {
      try {
        result.complete(await work());
      } on Object catch (error, stack) {
        result.completeError(error, stack);
      }
    });
    return result.future;
  }

  Future<String?> _epoch() async {
    if (!app.uiVisible ||
        app.engineCapabilities?.vpnGateEpochRequests != true ||
        !app.networkSettings.observationEnabled) {
      return null;
    }
    // A passive startup read may predate our explicit control owner. Join it,
    // then obtain a live observation before certifying this resource lease.
    await app.networkSettings.refresh(requireRuntime: true);
    if (!app.uiVisible ||
        app.engineCapabilities?.vpnGateEpochRequests != true ||
        !app.networkSettings.observationEnabled ||
        app.networkSettings.unconfirmed) {
      return null;
    }
    final state = app.networkSettings.state;
    if (state?.runtimeObserved != true) return null;
    final epoch = state?.sourceEpoch;
    return epoch == null || epoch.isEmpty ? null : epoch;
  }

  /// Preparation callbacks must send [epoch] in the native request. The
  /// independent confirmation afterwards rejects restarts during that request.
  Future<ScenarioGateLease?> ensure(
    VpnGateSettings selection, {
    ScenarioGateLease? existing,
    required bool Function() isCurrent,
    Future<bool> Function(String epoch)? prepare,
  }) => _enqueue(() async {
    if (!selection.hasSelection ||
        !isCurrent() ||
        !app.uiVisible ||
        app.engineCapabilities?.vpnGateEpochRequests != true ||
        !app.networkSettings.observationEnabled) {
      return null;
    }
    Future<void> Function()? releaseControl;
    try {
      try {
        releaseControl = await app.acquireControlOperation();
      } on Object {
        return null;
      }
      final epoch = await _epoch();
      if (epoch == null || !isCurrent()) return null;
      _uncertainCleanup.removeWhere((key) => key.$1 != epoch);
      final key = (epoch, selection.serverId, selection.configSha256);
      // A timed-out same-owner release may still be running. Do not prepare the
      // same key until that owner is retired; epoch guards make its late work inert.
      if (_uncertainCleanup.contains(key)) return null;
      if (existing?.sourceEpoch == epoch &&
          existing!.matches(selection) &&
          _released[existing] != true) {
        if (_controlReleases[existing] == null && releaseControl != null) {
          _controlReleases[existing] = releaseControl;
          releaseControl = null;
        }
        return existing;
      }
      final prepared =
          await (prepare?.call(epoch) ?? _prepare(selection, epoch, isCurrent));
      final confirmed = await _epoch();
      if (!prepared || confirmed != epoch || !isCurrent()) {
        if (prepared && confirmed == epoch) {
          await _release(ScenarioGateLease(selection, epoch));
        }
        return null;
      }
      final lease = ScenarioGateLease(selection, epoch);
      _controlReleases[lease] = releaseControl;
      releaseControl = null;
      return lease;
    } finally {
      await app.releaseControlOperation(releaseControl);
    }
  });

  Future<bool> _prepare(
    VpnGateSettings selection,
    String epoch,
    bool Function() isCurrent,
  ) async {
    final operation = app.newVpnGateOperationId();
    var complete = false;
    try {
      await app.vpnGateNode(
        VpnGateNodeRequest(
          operationId: operation,
          action: 'prepare',
          serverId: selection.serverId,
          configSha256: selection.configSha256,
          expectedSourceEpoch: epoch,
        ),
      );
      for (var attempt = 0; attempt < 150 && isCurrent(); attempt++) {
        final state = (await app.listVpnGate(
          limit: 1,
          statusOnly: true,
        )).nodeProgress;
        if (state.operationId != operation ||
            state.serverId != selection.serverId ||
            state.configSha256 != selection.configSha256) {
          return false;
        }
        if (state.stage == 'complete') return complete = true;
        if (state.stage != 'preparing') return false;
        await Future<void>.delayed(const Duration(milliseconds: 400));
      }
      return false;
    } on Object {
      return false;
    } finally {
      if (!complete) {
        try {
          await app.vpnGateNode(
            VpnGateNodeRequest(
              operationId: operation,
              action: 'cancel',
              expectedSourceEpoch: epoch,
            ),
          );
        } on Object {
          // No replay; owner recovery sweeps any unclaimed temporary reference.
        }
      }
    }
  }

  Future<void> release(ScenarioGateLease? lease) {
    if (lease == null || _released[lease] == true) return Future.value();
    _released[lease] = true;
    return _enqueue(() => _release(lease));
  }

  Future<void> _release(ScenarioGateLease lease) async {
    final releaseControl = _controlReleases[lease];
    _controlReleases[lease] = null;
    try {
      final current = await _epoch();
      if (current == null || current != lease.sourceEpoch) return;
      try {
        await app.vpnGateNode(
          VpnGateNodeRequest(
            operationId: app.newVpnGateOperationId(),
            action: 'release',
            serverId: lease.selection.serverId,
            configSha256: lease.selection.configSha256,
            expectedSourceEpoch: lease.sourceEpoch,
          ),
        );
      } on Object {
        _uncertainCleanup.add((
          lease.sourceEpoch,
          lease.selection.serverId,
          lease.selection.configSha256,
        ));
      }
    } finally {
      await app.releaseControlOperation(releaseControl);
    }
  }
}
