import 'dart:async';

import 'package:flutter/foundation.dart';

import '../models/diagnostics_models.dart';
import '../services/engine_client.dart';

class DiagnosticsController extends ChangeNotifier {
  DiagnosticsController(this._engine);

  static const Duration _activeRefreshInterval = Duration(milliseconds: 750);

  final EngineClient _engine;
  Timer? _activeRefreshTimer;
  Future<void>? _restoreInFlight;
  int _operationGeneration = 0;
  bool _cancelRequestedDuringStart = false;
  bool _startRequestInFlight = false;
  bool _disposed = false;

  DiagnosticsControllerState state = DiagnosticsControllerState.idle;
  DiagnosticSession? session;
  ConnectionTimeline timeline = const ConnectionTimeline();
  String? lastError;
  String? lastExportPath;
  bool exporting = false;
  bool timelineLoading = false;
  bool eventStreamDegraded = false;

  bool get isActive => session?.isActive ?? false;

  Future<void> restore({bool silent = false}) {
    final current = _restoreInFlight;
    if (current != null) {
      return current;
    }
    final restore = _restore(silent: silent);
    _restoreInFlight = restore;
    return restore.whenComplete(() {
      if (identical(_restoreInFlight, restore)) {
        _restoreInFlight = null;
      }
    });
  }

  Future<void> _restore({required bool silent}) async {
    try {
      final recovered = await _engine.getDiagnostics();
      if (_disposed) {
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
      if (!silent && !_disposed) {
        lastError = '${error.code}: ${error.message}';
        state = DiagnosticsControllerState.failed;
        notifyListeners();
      }
    }
  }

  Future<void> start(DiagnosticMode mode) async {
    if (_startRequestInFlight ||
        state == DiagnosticsControllerState.starting ||
        state == DiagnosticsControllerState.cancelling ||
        isActive) {
      return;
    }
    final generation = ++_operationGeneration;
    _startRequestInFlight = true;
    _cancelRequestedDuringStart = false;
    state = DiagnosticsControllerState.starting;
    lastError = null;
    lastExportPath = null;
    notifyListeners();
    try {
      final started = await _engine.startDiagnostics(mode);
      if (_disposed || generation != _operationGeneration) {
        return;
      }
      if (_cancelRequestedDuringStart && started.isActive) {
        _cancelRequestedDuringStart = false;
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
      lastError = '${error.code}: ${error.message}';
      state = DiagnosticsControllerState.failed;
      notifyListeners();
    } finally {
      _startRequestInFlight = false;
    }
  }

  Future<void> cancel() async {
    if (state == DiagnosticsControllerState.starting && session == null) {
      _cancelRequestedDuringStart = true;
      state = DiagnosticsControllerState.cancelling;
      lastError = null;
      notifyListeners();
      return;
    }
    final current = session;
    if (current == null || !current.isActive) {
      return;
    }
    final generation = ++_operationGeneration;
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
      lastError = '${error.code}: ${error.message}';
      state = DiagnosticsControllerState.failed;
      _startActiveRefresh();
      notifyListeners();
    }
  }

  void handleEngineEvent(EngineSnapshotEvent event) {
    if (_disposed || !event.diagnosticsChanged) {
      return;
    }
    eventStreamDegraded = false;
    final next = event.diagnosticSession;
    if (next != null) {
      final currentId = session?.sessionId;
      if (currentId == null || currentId == next.sessionId) {
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
    if (timelineLoading) {
      return;
    }
    timelineLoading = true;
    if (!silent) {
      notifyListeners();
    }
    try {
      final next = await _engine.getConnectionTimeline();
      if (!_disposed) {
        timeline = next;
      }
    } on EngineException catch (error) {
      if (!silent && !_disposed) {
        lastError = '${error.code}: ${error.message}';
      }
    } finally {
      if (!_disposed) {
        timelineLoading = false;
        notifyListeners();
      }
    }
  }

  Future<String?> export() async {
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
      if (!_disposed && destination != null) {
        lastExportPath = destination;
      }
      return destination;
    } on EngineException catch (error) {
      if (!_disposed) {
        lastError = '${error.code}: ${error.message}';
      }
      return null;
    } finally {
      if (!_disposed) {
        exporting = false;
        notifyListeners();
      }
    }
  }

  void clearError() {
    if (lastError == null) {
      return;
    }
    lastError = null;
    notifyListeners();
  }

  void _applySession(DiagnosticSession next) {
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
