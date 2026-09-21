import 'dart:async';

import 'package:flutter/foundation.dart';

import '../core/app_strings.dart';
import '../core/user_facing_errors.dart';
import '../models/app_models.dart';
import '../models/diagnostics_models.dart';
import '../services/engine_client.dart';

class DiagnosticsController extends ChangeNotifier {
  DiagnosticsController(this._engine);

  AppStrings Function() resolveStrings = () =>
      AppStrings(LocalePreference.system);

  static const Duration _activeRefreshInterval = Duration(milliseconds: 750);

  final EngineClient _engine;
  Timer? _activeRefreshTimer;
  Future<void>? _restoreInFlight;
  int _operationGeneration = 0;
  bool _cancelRequestedDuringStart = false;
  bool _startRequestInFlight = false;
  bool _disposed = false;
  bool _resetting = false;
  bool _acceptUnownedEvents = true;
  int _dataGeneration = 0;
  Object? _startToken;
  final Map<String, DiagnosticSession> _startEvents = {};
  DiagnosticMode? _requestedMode;

  DiagnosticsControllerState state = DiagnosticsControllerState.idle;
  DiagnosticSession? session;
  ConnectionTimeline timeline = const ConnectionTimeline();
  String? lastError;
  String? lastExportPath;
  bool exporting = false;
  bool timelineLoading = false;
  bool eventStreamDegraded = false;

  bool get isActive => session?.isActive ?? false;
  DiagnosticMode? get requestedMode => _requestedMode;

  Future<void> restore({bool silent = false}) {
    final current = _restoreInFlight;
    if (current != null) {
      return current;
    }
    late final Future<void> restore;
    restore = _restore(silent: silent).whenComplete(() {
      if (identical(_restoreInFlight, restore)) {
        _restoreInFlight = null;
      }
    });
    _restoreInFlight = restore;
    return restore;
  }

  Future<void> _restore({required bool silent}) async {
    final generation = _dataGeneration;
    final operation = _operationGeneration;
    if (_resetting || _startRequestInFlight) return;
    try {
      final recovered = await _engine.getDiagnostics();
      if (_disposed ||
          _resetting ||
          generation != _dataGeneration ||
          operation != _operationGeneration ||
          _startRequestInFlight) {
        return;
      }
      if (recovered == null) {
        if (!_startRequestInFlight) {
          session = null;
          state = DiagnosticsControllerState.idle;
          _stopActiveRefresh();
        }
      } else {
        _applySession(recovered);
      }
      await loadTimeline(silent: true);
    } on EngineException catch (error) {
      if (!silent &&
          !_disposed &&
          generation == _dataGeneration &&
          operation == _operationGeneration &&
          !_startRequestInFlight) {
        lastError = userFacingError(resolveStrings(), error);
        state = DiagnosticsControllerState.failed;
        notifyListeners();
      }
    }
  }

  Future<void> start(DiagnosticMode mode) async {
    if (_resetting ||
        _startRequestInFlight ||
        state == DiagnosticsControllerState.starting ||
        state == DiagnosticsControllerState.cancelling ||
        isActive) {
      return;
    }
    final generation = ++_operationGeneration;
    _restoreInFlight = null;
    _stopActiveRefresh();
    session = null;
    _startEvents.clear();
    final startToken = Object();
    _startToken = startToken;
    _startRequestInFlight = true;
    _cancelRequestedDuringStart = false;
    _requestedMode = mode;
    state = DiagnosticsControllerState.starting;
    lastError = null;
    lastExportPath = null;
    notifyListeners();
    try {
      final reply = await _engine.startDiagnostics(mode);
      if (_disposed || generation != _operationGeneration) {
        return;
      }
      final started = _startEvents[reply.sessionId] ?? reply;
      if (_cancelRequestedDuringStart && started.isActive) {
        _cancelRequestedDuringStart = false;
        _requestedMode = null;
        session = started;
        state = DiagnosticsControllerState.cancelling;
        notifyListeners();
        await _cancelStartedSession(started, generation);
        return;
      }
      _cancelRequestedDuringStart = false;
      _applySession(started);
      unawaited(loadTimeline(silent: true));
    } on EngineException catch (error) {
      if (_disposed || generation != _operationGeneration) {
        return;
      }
      _requestedMode = null;
      lastError = userFacingError(resolveStrings(), error);
      state = DiagnosticsControllerState.failed;
      notifyListeners();
    } finally {
      if (identical(_startToken, startToken)) {
        _startRequestInFlight = false;
        _startToken = null;
        _startEvents.clear();
      }
    }
  }

  Future<void> cancel() async {
    if (_resetting) return;
    if (_startRequestInFlight) {
      _cancelRequestedDuringStart = true;
      state = DiagnosticsControllerState.cancelling;
      lastError = null;
      notifyListeners();
      return;
    }
    if (state == DiagnosticsControllerState.cancelling) return;
    final current = session;
    if (current == null || !current.isActive) {
      return;
    }
    final generation = ++_operationGeneration;
    _restoreInFlight = null;
    state = DiagnosticsControllerState.cancelling;
    lastError = null;
    notifyListeners();
    await _cancelStartedSession(current, generation);
  }

  Future<void> _cancelStartedSession(
    DiagnosticSession current,
    int generation,
  ) async {
    try {
      final cancelling = await _engine.cancelDiagnostics(current.sessionId);
      if (_disposed || generation != _operationGeneration) {
        return;
      }
      _applySession(cancelling);
      _startActiveRefresh();
    } on EngineException catch (error) {
      if (_disposed || generation != _operationGeneration) {
        return;
      }
      lastError = userFacingError(resolveStrings(), error);
      state = DiagnosticsControllerState.failed;
      _startActiveRefresh();
      notifyListeners();
    }
  }

  void handleEngineEvent(EngineSnapshotEvent event) {
    if (_disposed || _resetting || !event.diagnosticsChanged) {
      return;
    }
    eventStreamDegraded = false;
    final next = event.diagnosticSession;
    if (_startRequestInFlight) {
      // The start reply identifies the session owned by this request. Keep
      // early progress without adopting an older session or losing cancel.
      if (next != null) {
        if (_startEvents.length >= 8) {
          _startEvents.remove(_startEvents.keys.first);
        }
        _startEvents[next.sessionId] = next;
      }
      return;
    }
    if (next != null) {
      final currentId = session?.sessionId;
      if ((currentId == null && _acceptUnownedEvents) ||
          currentId == next.sessionId) {
        _applySession(next);
        if (!next.isActive) {
          unawaited(loadTimeline(silent: true));
        }
        return;
      }
    }
    unawaited(restore(silent: true));
  }

  void markEventStreamUnavailable() {
    if (_disposed) {
      return;
    }
    eventStreamDegraded = true;
    if (isActive) {
      _startActiveRefresh();
    }
    notifyListeners();
  }

  Future<void> loadTimeline({bool silent = false}) async {
    final generation = _dataGeneration;
    if (_resetting) return;
    if (timelineLoading) {
      return;
    }
    timelineLoading = true;
    if (!silent) {
      notifyListeners();
    }
    try {
      final next = await _engine.getConnectionTimeline();
      if (!_disposed && generation == _dataGeneration) {
        timeline = next;
      }
    } on EngineException catch (error) {
      if (!silent && !_disposed && generation == _dataGeneration) {
        lastError = userFacingError(resolveStrings(), error);
      }
    } finally {
      if (!_disposed && generation == _dataGeneration) {
        timelineLoading = false;
        notifyListeners();
      }
    }
  }

  Future<String?> export() async {
    final generation = _dataGeneration;
    if (_resetting) return null;
    if (exporting) {
      return null;
    }
    exporting = true;
    lastError = null;
    notifyListeners();
    try {
      final destination = await _engine.exportDiagnostics(
        diagnosticSessionId: session?.sessionId,
      );
      if (!_disposed && generation == _dataGeneration && destination != null) {
        lastExportPath = destination;
      }
      return destination;
    } on EngineException catch (error) {
      if (!_disposed && generation == _dataGeneration) {
        lastError = userFacingError(resolveStrings(), error);
      }
      return null;
    } finally {
      if (!_disposed && generation == _dataGeneration) {
        exporting = false;
        notifyListeners();
      }
    }
  }

  void suspendForReset() {
    _dataGeneration++;
    _operationGeneration++;
    _resetting = true;
    _stopActiveRefresh();
    _restoreInFlight = null;
  }

  void resumeAfterReset() {
    _resetting = false;
    _startRequestInFlight = false;
    _startToken = null;
    _startEvents.clear();
    _cancelRequestedDuringStart = false;
    _requestedMode = null;
    exporting = false;
    timelineLoading = false;
    _startActiveRefresh();
  }

  void reset() {
    if (_disposed) return;
    suspendForReset();
    _startRequestInFlight = false;
    _startToken = null;
    _startEvents.clear();
    _cancelRequestedDuringStart = false;
    _requestedMode = null;
    session = null;
    timeline = const ConnectionTimeline();
    state = DiagnosticsControllerState.idle;
    lastError = null;
    lastExportPath = null;
    exporting = false;
    timelineLoading = false;
    eventStreamDegraded = false;
    _acceptUnownedEvents = false;
    _resetting = false;
    notifyListeners();
  }

  void clearError() {
    if (lastError == null) {
      return;
    }
    lastError = null;
    notifyListeners();
  }

  void _applySession(DiagnosticSession next) {
    _requestedMode = null;
    session = next;
    state = switch (next.state) {
      DiagnosticSessionState.pending => DiagnosticsControllerState.starting,
      DiagnosticSessionState.running => DiagnosticsControllerState.running,
      DiagnosticSessionState.cancelling =>
        DiagnosticsControllerState.cancelling,
      DiagnosticSessionState.completed ||
      DiagnosticSessionState.cancelled => DiagnosticsControllerState.completed,
      DiagnosticSessionState.failed => DiagnosticsControllerState.failed,
    };
    if (next.isActive) {
      _startActiveRefresh();
    } else {
      _stopActiveRefresh();
    }
    notifyListeners();
  }

  void _startActiveRefresh() {
    if (_activeRefreshTimer != null || !isActive) {
      return;
    }
    _activeRefreshTimer = Timer.periodic(
      _activeRefreshInterval,
      (_) => unawaited(restore(silent: true)),
    );
  }

  void _stopActiveRefresh() {
    _activeRefreshTimer?.cancel();
    _activeRefreshTimer = null;
  }

  @override
  void dispose() {
    _disposed = true;
    _operationGeneration += 1;
    _stopActiveRefresh();
    super.dispose();
  }
}
