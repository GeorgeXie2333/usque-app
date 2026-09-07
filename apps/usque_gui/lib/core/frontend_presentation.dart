import 'package:flutter/material.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../models/app_models.dart';
import '../widgets/common.dart';

/// Configuration intent is not proof of a running listener. Both pages use
/// the same conservative mapping of connection and per-output runtime state.
class FrontendPresentation {
  const FrontendPresentation(this.labelKey, this.tone, this.icon);
  final String labelKey;
  final StatusTone tone;
  final IconData icon;

  static FrontendPresentation of({
    required bool configured,
    required ConnectionPhase connection,
    FrontendPhase? runtime,
  }) {
    const disabled = FrontendPresentation(
      'output_disabled',
      StatusTone.neutral,
      LucideIcons.circleMinus,
    );
    const waiting = FrontendPresentation(
      'output_waiting',
      StatusTone.neutral,
      LucideIcons.pause,
    );
    const stopping = FrontendPresentation(
      'output_stopping',
      StatusTone.neutral,
      LucideIcons.loader,
    );
    if (connection == ConnectionPhase.disconnected) {
      return configured ? waiting : disabled;
    }
    if (connection == ConnectionPhase.disconnecting) {
      return configured || runtime == FrontendPhase.active
          ? stopping
          : disabled;
    }
    if (connection == ConnectionPhase.error) {
      return configured
          ? const FrontendPresentation(
              'output_error',
              StatusTone.danger,
              LucideIcons.circleX,
            )
          : disabled;
    }
    if (!configured) {
      return runtime == FrontendPhase.active ? stopping : disabled;
    }
    if (connection == ConnectionPhase.reconnecting) {
      return const FrontendPresentation(
        'output_reconnecting',
        StatusTone.warning,
        LucideIcons.refreshCw,
      );
    }
    if (connection != ConnectionPhase.connected &&
        connection != ConnectionPhase.degraded) {
      return const FrontendPresentation(
        'output_starting',
        StatusTone.neutral,
        LucideIcons.loader,
      );
    }
    return switch (runtime) {
      FrontendPhase.active => const FrontendPresentation(
        'output_running',
        StatusTone.success,
        LucideIcons.circleCheck,
      ),
      FrontendPhase.degraded => const FrontendPresentation(
        'output_degraded',
        StatusTone.warning,
        LucideIcons.triangleAlert,
      ),
      FrontendPhase.error => const FrontendPresentation(
        'output_error',
        StatusTone.danger,
        LucideIcons.circleX,
      ),
      FrontendPhase.reconnecting => const FrontendPresentation(
        'output_reconnecting',
        StatusTone.warning,
        LucideIcons.refreshCw,
      ),
      FrontendPhase.preparing => const FrontendPresentation(
        'output_starting',
        StatusTone.neutral,
        LucideIcons.loader,
      ),
      FrontendPhase.disabled => waiting,
      null => const FrontendPresentation(
        'output_unknown',
        StatusTone.neutral,
        LucideIcons.circleHelp,
      ),
    };
  }
}
