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
  static const Duration _timelineRefreshInterval = Duration(seconds: 2);
  // Android's bridge waits up to 750 ms before returning its platform fallback.
  // Reserve delivery time around that budget while keeping caller waits bounded.
  static const Duration _timelineReadTimeout = Duration(seconds: 2);

  final EngineClient _engine;
  Timer? _activeRefreshTimer;
  Timer? _restoreConflictTimer;
  Timer? _timelineRefreshTimer;
  int _timelineObservers = 0;
  int _sessionVersion = 0;
  Future<void>? _restoreInFlight;
  Future<ConnectionTimeline>? _timelineInFlight;
  int _operationGeneration = 0;
  bool _cancelRequestedDuringStart = false;
  bool _startRequestInFlight = false;
  bool _disposed = false;
  bool _observationVisible = true;
  int _observationGeneration = 0;
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

  /// Visibility controls readback only; an explicit probe or export keeps ownership.
  void setObservationVisible(bool visible) {
    if (_disposed || visible == _observationVisible) return;
    _observationVisible = visible;
    _observationGeneration++;
    if (!visible) {
      _stopActiveRefresh();
      _stopTimelineRefresh();
      _restoreConflictTimer?.cancel();
      _restoreConflictTimer = null;
    } else {
      _startActiveRefresh();
      _startTimelineRefresh();
      unawaited(restore(silent: true, refreshTimeline: _timelineObservers > 0));
    }
  }

  Future<void> restore({bool silent = false, bool refreshTimeline = true}) {
    if (_disposed || !_observationVisible) return Future<void>.value();
    final generation = _dataGeneration;
    final operation = _operationGeneration;
    final observation = _observationGeneration;
    Future<void> withTimeline(Future<void> restored) => !refreshTimeline
        ? restored
        : restored.then((_) async {
            if (!_disposed &&
                !_resetting &&
                _observationVisible &&
                observation == _observationGeneration &&
                generation == _dataGeneration &&
                operation == _operationGeneration) {
              await loadTimeline(silent: true);
            }
          });
    final current = _restoreInFlight;
    if (current != null) {
      return withTimeline(current);
    }
    late final Future<void> restore;
    restore = _restore(silent: silent).whenComplete(() {
      if (identical(_restoreInFlight, restore)) {
        _restoreInFlight = null;
        if (!_disposed &&
            !_resetting &&
            _observationVisible &&
            observation != _observationGeneration) {
          unawaited(
            this.restore(silent: true, refreshTimeline: _timelineObservers > 0),
          );
        }
      }
    });
    _restoreInFlight = restore;
    return withTimeline(restore);
  }

  Future<void> _restore({required bool silent}) async {
    final generation = _dataGeneration;
    final operation = _operationGeneration;
    final version = _sessionVersion;
    final observation = _observationGeneration;
    if (_resetting || _startRequestInFlight || !_observationVisible) return;
    try {
      final recovered = await _engine.getDiagnostics();
      if (_disposed ||
          _resetting ||
          !_observationVisible ||
          observation != _observationGeneration ||
          generation != _dataGeneration ||
          operation != _operationGeneration ||
          _startRequestInFlight) {
        return;
      }
      if (recovered == null) {
        if (!_startRequestInFlight && version == _sessionVersion) {
          session = null;
          state = DiagnosticsControllerState.idle;
          _stopActiveRefresh();
        }
      } else if (version == _sessionVersion ||
          session?.sessionId == recovered.sessionId) {
        _applySession(recovered);
      } else {
        // The intervening event and reply may belong to either ordering of two
        // sessions. Keep the displayed event and read fresh authoritative state
        // once this request settles, at the normal recovery cadence.
        _scheduleConflictRestore();
      }
    } on EngineException catch (error) {
      if (!silent &&
          !_disposed &&
          _observationVisible &&
          observation == _observationGeneration &&
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
      final cached = _startEvents[reply.sessionId];
      final started =
          cached != null &&
              (_olderSession(reply, cached) || !_olderSession(cached, reply))
          ? cached
          : reply;
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
    if (_disposed ||
        _resetting ||
        !_observationVisible ||
        !event.diagnosticsChanged) {
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
        final previous = _startEvents[next.sessionId];
        if (previous == null || !_olderSession(next, previous)) {
          _startEvents[next.sessionId] = next;
        }
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
    unawaited(restore(silent: true, refreshTimeline: false));
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
    final observation = _observationGeneration;
    if (_disposed || _resetting || !_observationVisible) return;
    if (timelineLoading || _timelineInFlight != null) {
      return;
    }
    timelineLoading = true;
    if (!silent) {
      notifyListeners();
    }
    try {
      final request = _engine.getConnectionTimeline();
      _timelineInFlight = request;
      void release() {
        if (identical(_timelineInFlight, request)) {
          _timelineInFlight = null;
          if (!_disposed &&
              !_resetting &&
              _observationVisible &&
              observation != _observationGeneration &&
              _timelineObservers > 0) {
            unawaited(loadTimeline(silent: true));
          }
        }
      }

      // A timed-out read may still be unwinding in the bridge. Keep ownership
      // until its actual completion so subsequent ticks cannot overlap it.
      unawaited(
        request.then<void>(
          (_) => release(),
          onError: (Object error, StackTrace stack) => release(),
        ),
      );
      final next = await request.timeout(_timelineReadTimeout);
      if (!_disposed &&
          _observationVisible &&
          observation == _observationGeneration &&
          generation == _dataGeneration) {
        timeline = next;
      }
    } on Object catch (error) {
      if (!silent &&
          !_disposed &&
          _observationVisible &&
          observation == _observationGeneration &&
          generation == _dataGeneration) {
        lastError = userFacingError(resolveStrings(), error);
      }
    } finally {
      if (!_disposed && generation == _dataGeneration) {
        timelineLoading = false;
        if (_observationVisible) notifyListeners();
        if (_observationVisible &&
            observation != _observationGeneration &&
            _timelineInFlight == null &&
            _timelineObservers > 0) {
          unawaited(loadTimeline(silent: true));
        }
      }
    }
  }

  /// Observe existing runtime evidence while this page is visible. This never
  /// starts diagnostic probes and is independent of session recovery polling.
  void beginTimelineUpdates() {
    if (_disposed) return;
    _timelineObservers++;
    _startTimelineRefresh();
  }

  void endTimelineUpdates() {
    if (_timelineObservers > 0) _timelineObservers--;
    if (_timelineObservers == 0) _stopTimelineRefresh();
  }

  void _startTimelineRefresh() {
    if (_disposed ||
        _resetting ||
        !_observationVisible ||
        _timelineObservers == 0 ||
        _timelineRefreshTimer != null) {
      return;
    }
    _timelineRefreshTimer = Timer.periodic(
      _timelineRefreshInterval,
      (_) => unawaited(loadTimeline(silent: true)),
    );
  }

  void _stopTimelineRefresh() {
    _timelineRefreshTimer?.cancel();
    _timelineRefreshTimer = null;
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
    } on Object catch (error) {
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
    _restoreConflictTimer?.cancel();
    _restoreConflictTimer = null;
    _stopTimelineRefresh();
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
    _startTimelineRefresh();
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
    _startTimelineRefresh();
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
    final current = session;
    if (current != null && _olderSession(next, current)) return;
    _sessionVersion++;
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
    if (_disposed ||
        _resetting ||
        !_observationVisible ||
        _activeRefreshTimer != null ||
        !isActive) {
      return;
    }
    _activeRefreshTimer = Timer.periodic(
      _activeRefreshInterval,
      (_) => unawaited(restore(silent: true, refreshTimeline: false)),
    );
  }

  void _stopActiveRefresh() {
    _activeRefreshTimer?.cancel();
    _activeRefreshTimer = null;
  }

  void _scheduleConflictRestore() {
    if (!_observationVisible || _restoreConflictTimer != null) return;
    final generation = _dataGeneration;
    final operation = _operationGeneration;
    _restoreConflictTimer = Timer(_activeRefreshInterval, () {
      _restoreConflictTimer = null;
      if (!_disposed &&
          !_resetting &&
          _observationVisible &&
          generation == _dataGeneration &&
          operation == _operationGeneration) {
        unawaited(restore(silent: true, refreshTimeline: false));
      }
    });
  }

  @override
  void dispose() {
    _disposed = true;
    _operationGeneration += 1;
    _stopActiveRefresh();
    _restoreConflictTimer?.cancel();
    _stopTimelineRefresh();
    super.dispose();
  }
}

bool _olderSession(DiagnosticSession next, DiagnosticSession previous) {
  if (next.sessionId != previous.sessionId) return false;
  if (!previous.isActive && next.isActive) return true;
  final previousRevision = previous.revision;
  return previousRevision != null &&
      (next.revision == null || next.revision! < previousRevision);
}
