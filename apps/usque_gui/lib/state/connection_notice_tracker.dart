import 'dart:async';

import '../models/app_models.dart';

enum ConnectionNoticeKind { interrupted, failed, restored }

class ConnectionNotice {
  const ConnectionNotice(this.kind, {this.killSwitchBlocking = false});

  final ConnectionNoticeKind kind;

  /// The tunnel is down while the Kill Switch keeps blocking traffic.
  final bool killSwitchBlocking;
}

/// Turns connection phase changes into desktop notifications.
///
/// Only changes the user did not ask for are reported: a session that drops
/// and stays down past [interruptionDelay], a connection that ends in an
/// error, and recovery after an interruption that was reported. A short
/// reconnect, a user disconnect, and the first snapshot after start-up stay
/// silent.
class ConnectionNoticeTracker {
  ConnectionNoticeTracker({
    required this.onNotice,
    this.interruptionDelay = const Duration(seconds: 5),
  });

  final void Function(ConnectionNotice notice) onNotice;
  final Duration interruptionDelay;

  ConnectionPhase? _phase;
  bool _killSwitchBlocking = false;
  bool _interruptionReported = false;
  Timer? _pendingInterruption;

  /// [killSwitchArmed] is the saved preference for a tunnel session that
  /// blocks traffic while it is down.
  void update(EngineSnapshot snapshot, {required bool killSwitchArmed}) {
    final previous = _phase;
    final phase = snapshot.phase;
    _killSwitchBlocking =
        killSwitchArmed && snapshot.killSwitchState == 'active';
    if (phase == previous) return;
    _phase = phase;
    if (previous == null) return;

    switch (phase) {
      case ConnectionPhase.reconnecting:
        if (_established(previous)) {
          _pendingInterruption?.cancel();
          _pendingInterruption = Timer(interruptionDelay, () {
            _pendingInterruption = null;
            if (_phase != ConnectionPhase.reconnecting) return;
            _interruptionReported = true;
            onNotice(
              ConnectionNotice(
                ConnectionNoticeKind.interrupted,
                killSwitchBlocking: _killSwitchBlocking,
              ),
            );
          });
        }
      case ConnectionPhase.error:
        _cancelPending();
        _interruptionReported = false;
        onNotice(
          ConnectionNotice(
            ConnectionNoticeKind.failed,
            killSwitchBlocking: _killSwitchBlocking,
          ),
        );
      case ConnectionPhase.connected || ConnectionPhase.degraded:
        _cancelPending();
        if (_interruptionReported) {
          _interruptionReported = false;
          onNotice(const ConnectionNotice(ConnectionNoticeKind.restored));
        }
      case ConnectionPhase.disconnecting || ConnectionPhase.disconnected:
        _cancelPending();
        _interruptionReported = false;
      case ConnectionPhase.preparing ||
          ConnectionPhase.connectingH3 ||
          ConnectionPhase.connectingH2:
        break;
    }
  }

  void dispose() => _cancelPending();

  void _cancelPending() {
    _pendingInterruption?.cancel();
    _pendingInterruption = null;
  }

  static bool _established(ConnectionPhase phase) =>
      phase == ConnectionPhase.connected || phase == ConnectionPhase.degraded;
}
