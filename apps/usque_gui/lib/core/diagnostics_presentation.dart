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

/// Failures and warnings are what the user acts on.
bool diagnosticNeedsAttention(DiagnosticFinding finding) =>
    finding.status == DiagnosticCheckStatus.failed ||
    finding.status == DiagnosticCheckStatus.warning;

int _statusRank(DiagnosticCheckStatus status) => switch (status) {
  DiagnosticCheckStatus.failed => 0,
  DiagnosticCheckStatus.warning => 1,
  DiagnosticCheckStatus.cancelled => 2,
  DiagnosticCheckStatus.running => 3,
  DiagnosticCheckStatus.pending => 4,
  DiagnosticCheckStatus.passed => 5,
  DiagnosticCheckStatus.skipped => 6,
};

/// Most severe first; equal statuses keep the engine's check order.
List<DiagnosticFinding> _bySeverity(Iterable<DiagnosticFinding> findings) {
  final indexed = findings.indexed.toList()
    ..sort((a, b) {
      final rank = _statusRank(a.$2.status) - _statusRank(b.$2.status);
      return rank != 0 ? rank : a.$1 - b.$1;
    });
  return [for (final (_, finding) in indexed) finding];
}

/// Findings that need attention, most severe first.
List<DiagnosticFinding> diagnosticProblems(List<DiagnosticFinding> findings) =>
    _bySeverity(findings.where(diagnosticNeedsAttention));

/// The finding's own advice, falling back to its failure's advice.
String diagnosticRemediationKey(DiagnosticFinding finding) =>
    finding.remediationKey.isNotEmpty && finding.remediationKey != 'none'
    ? finding.remediationKey
    : finding.failure?.remediationKey ?? finding.remediationKey;

typedef DiagnosticFindingGroup = ({
  DiagnosticCategory category,
  List<DiagnosticFinding> findings,

  /// Every check in the group passed or was skipped.
  bool settled,
});

/// Category groups ordered by their most severe check, so problems lead the
/// results; categories with equal severity keep their usual order.
List<DiagnosticFindingGroup> groupDiagnosticFindings(
  List<DiagnosticFinding> findings,
) {
  final groups = <DiagnosticFindingGroup>[
    for (final category in DiagnosticCategory.values)
      if (findings.where((finding) => finding.category == category).toList()
          case final members when members.isNotEmpty)
        (
          category: category,
          findings: _bySeverity(members),
          settled: members.every(
            (finding) =>
                finding.status == DiagnosticCheckStatus.passed ||
                finding.status == DiagnosticCheckStatus.skipped,
          ),
        ),
  ];
  final indexed = groups.indexed.toList()
    ..sort((a, b) {
      final rank =
          _statusRank(a.$2.findings.first.status) -
          _statusRank(b.$2.findings.first.status);
      return rank != 0 ? rank : a.$1 - b.$1;
    });
  return [for (final (_, group) in indexed) group];
}
