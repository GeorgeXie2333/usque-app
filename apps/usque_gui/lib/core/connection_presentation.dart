import 'package:flutter/material.dart';

import '../models/app_models.dart';
import '../widgets/common.dart';
import 'usque_theme.dart';

/// How the connection bezel behaves for a given engine phase.
enum RingMode {
  /// Nothing is running: an unlit bezel.
  idle,

  /// Work in progress: a lit arc travels around the bezel.
  scan,

  /// The tunnel carries traffic: the bezel closes once and then holds.
  steady,

  /// The tunnel is broken: a closed ring with a gap, held still.
  fault,
}

/// Everything the UI needs to draw a [ConnectionPhase].
///
/// Ring, title-bar lamp, home copy, and the diagnostics pill all read this
/// so a new phase cannot be painted four different ways.
@immutable
class ConnectionPresentation {
  const ConnectionPresentation({
    required this.mode,
    required this.tone,
    required this.labelKey,
    required this.actionKey,
    required this.recoverable,
  });

  final RingMode mode;
  final StatusTone tone;

  /// Catalog key for the phase name.
  final String labelKey;

  /// Catalog key for the power-button label.
  final String actionKey;

  /// True when a Retry control is worth showing.
  final bool recoverable;

  bool get engaged => mode == RingMode.steady;

  bool get scanning => mode == RingMode.scan;

  /// Colour of the ring accent and the caption lamp.
  Color indicatorColor(UsqueTokens tokens, ColorScheme scheme) {
    if (mode == RingMode.idle) {
      return tokens.hairlineStrong;
    }
    return switch (tone) {
      StatusTone.success => tokens.success,
      StatusTone.warning => tokens.caution,
      StatusTone.danger => tokens.danger,
      StatusTone.brand => tokens.brand,
      StatusTone.neutral => scheme.onSurfaceVariant,
    };
  }

  static ConnectionPresentation of(ConnectionPhase phase) {
    return switch (phase) {
      ConnectionPhase.disconnected => const ConnectionPresentation(
        mode: RingMode.idle,
        tone: StatusTone.neutral,
        labelKey: 'disconnected',
        actionKey: 'connect',
        recoverable: false,
      ),
      ConnectionPhase.preparing => const ConnectionPresentation(
        mode: RingMode.scan,
        tone: StatusTone.brand,
        labelKey: 'preparing',
        actionKey: 'connecting',
        recoverable: false,
      ),
      ConnectionPhase.connectingH3 ||
      ConnectionPhase.connectingH2 => const ConnectionPresentation(
        mode: RingMode.scan,
        tone: StatusTone.brand,
        labelKey: 'connecting',
        actionKey: 'connecting',
        recoverable: false,
      ),
      ConnectionPhase.connected => const ConnectionPresentation(
        mode: RingMode.steady,
        tone: StatusTone.success,
        labelKey: 'connected',
        actionKey: 'disconnect',
        recoverable: false,
      ),
      ConnectionPhase.degraded => const ConnectionPresentation(
        mode: RingMode.steady,
        tone: StatusTone.warning,
        labelKey: 'degraded',
        actionKey: 'disconnect',
        recoverable: true,
      ),
      ConnectionPhase.reconnecting => const ConnectionPresentation(
        mode: RingMode.scan,
        tone: StatusTone.warning,
        labelKey: 'reconnecting',
        actionKey: 'connecting',
        recoverable: false,
      ),
      ConnectionPhase.disconnecting => const ConnectionPresentation(
        mode: RingMode.scan,
        tone: StatusTone.neutral,
        labelKey: 'disconnecting',
        actionKey: 'connecting',
        recoverable: false,
      ),
      ConnectionPhase.error => const ConnectionPresentation(
        mode: RingMode.fault,
        tone: StatusTone.danger,
        labelKey: 'error',
        actionKey: 'connect',
        recoverable: true,
      ),
    };
  }
}
