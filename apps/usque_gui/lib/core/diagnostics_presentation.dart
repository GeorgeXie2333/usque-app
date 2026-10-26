import 'package:flutter/material.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../models/diagnostics_models.dart';
import '../widgets/common.dart';
import 'usque_theme.dart';

/// Tone of a diagnostic run's overall state.
StatusTone diagnosticSessionTone(DiagnosticSessionState state) =>
    switch (state) {
      DiagnosticSessionState.pending ||
      DiagnosticSessionState.running => StatusTone.brand,
      DiagnosticSessionState.cancelling ||
      DiagnosticSessionState.cancelled => StatusTone.warning,
      DiagnosticSessionState.completed => StatusTone.success,
      DiagnosticSessionState.failed => StatusTone.danger,
    };

IconData diagnosticCategoryIcon(DiagnosticCategory category) =>
    switch (category) {
      DiagnosticCategory.localComponent => LucideIcons.cpu,
      DiagnosticCategory.physicalNetwork => LucideIcons.wifi,
      DiagnosticCategory.transport => LucideIcons.route,
      DiagnosticCategory.tunnel => LucideIcons.network,
      DiagnosticCategory.protection => LucideIcons.shieldCheck,
      DiagnosticCategory.recovery => LucideIcons.rotateCcw,
    };

/// One check's status icon, shared by the result row and its detail card.
IconData diagnosticCheckStatusIcon(DiagnosticCheckStatus status) =>
    switch (status) {
      DiagnosticCheckStatus.passed => LucideIcons.circleCheck,
      DiagnosticCheckStatus.warning => LucideIcons.triangleAlert,
      DiagnosticCheckStatus.failed => LucideIcons.circleX,
      DiagnosticCheckStatus.cancelled => LucideIcons.circleX,
      DiagnosticCheckStatus.pending ||
      DiagnosticCheckStatus.skipped => LucideIcons.circle,
      DiagnosticCheckStatus.running => LucideIcons.refreshCw,
    };

Color diagnosticCheckStatusColor(
  UsqueTokens tokens,
  DiagnosticCheckStatus status,
) => switch (status) {
  DiagnosticCheckStatus.passed => tokens.success,
  DiagnosticCheckStatus.warning => tokens.caution,
  DiagnosticCheckStatus.failed => tokens.danger,
  DiagnosticCheckStatus.running => tokens.brand,
  DiagnosticCheckStatus.pending ||
  DiagnosticCheckStatus.skipped ||
  DiagnosticCheckStatus.cancelled => tokens.hairlineStrong,
};
