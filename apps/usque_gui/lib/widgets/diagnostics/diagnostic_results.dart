import 'package:flutter/material.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../../core/app_strings.dart';
import '../../core/diagnostics_presentation.dart';
import '../../core/diagnostics_strings.dart';
import '../../core/usque_motion.dart';
import '../../core/usque_theme.dart';
import '../../models/diagnostics_models.dart';
import '../../state/app_controller.dart';
import '../common.dart';
import '../connection_timeline.dart';
import '../diagnostic_check_tile.dart';
import '../listenable_selector.dart';

/// Follows the locale and the diagnostics controller only: connection
/// samples arriving every second never repaint the results.
Widget _diagnosticsSelector<T>({
  required AppController controller,
  required T Function() selector,
  required Widget Function(BuildContext context, T value) builder,
}) => ListenableSelector<T>(
  listenables: <Listenable>[controller, controller.diagnostics],
  selector: selector,
  builder: builder,
);

/// Locates result rows, so the run's conclusion can bring a problem into
/// view.
class DiagnosticRowKeys {
  final _keys = <String, GlobalKey>{};

  GlobalKey of(String checkId) =>
      _keys.putIfAbsent(checkId, () => GlobalKey(debugLabel: checkId));

  void reveal(BuildContext context, String checkId) {
    final row = _keys[checkId]?.currentContext;
    if (row == null) return;
    Scrollable.ensureVisible(
      row,
      alignment: 0.1,
      duration: UsqueMotion.of(context, UsqueMotion.base),
      curve: UsqueMotion.standard,
    );
  }
}

/// Progress of the current run, then its conclusion once it completes.
class DiagnosticSessionPanel extends StatelessWidget {
  const DiagnosticSessionPanel({
    required this.controller,
    required this.rows,
    super.key,
  });
  final AppController controller;
  final DiagnosticRowKeys rows;

  @override
  Widget build(BuildContext context) => _diagnosticsSelector(
    controller: controller,
    selector: () => (
      catalog: controller.strings.catalogId,
      session: controller.diagnostics.session,
    ),
    builder: (context, view) {
      final session = view.session;
      if (session == null) return const SizedBox.shrink();
      final strings = controller.strings;
      final summary = session.summary;
      final activeChecks = session.runningCheckIds;
      // Zero counts say nothing; only the outcomes that occurred are listed.
      final counts = <(int, String, StatusTone, IconData)>[
        (
          summary.failed,
          'diag_summary_failed',
          StatusTone.danger,
          LucideIcons.circleX,
        ),
        (
          summary.warnings,
          'diag_summary_warnings',
          StatusTone.warning,
          LucideIcons.triangleAlert,
        ),
        (
          summary.passed,
          'diag_summary_passed',
          StatusTone.success,
          LucideIcons.circleCheck,
        ),
        (
          summary.skipped,
          'diag_summary_skipped',
          StatusTone.neutral,
          LucideIcons.circle,
        ),
      ].where((count) => count.$1 > 0);
      return ContentSection(
        icon: LucideIcons.radio,
        title: strings.get('diag_session'),
        trailing: InlineStatus(
          label: diagnosticSessionStateLabel(strings, session.state),
          tone: diagnosticSessionTone(session.state),
          icon: session.isActive
              ? LucideIcons.loaderCircle
              : LucideIcons.circleCheck,
        ),
        children: <Widget>[
          Semantics(
            label: strings
                .get('diag_progress_semantics')
                .replaceAll('{current}', '${session.progressPercent}'),
            value: '${session.progressPercent}%',
            child: LinearProgressIndicator(
              value: session.progressPercent / 100,
            ),
          ),
          const SizedBox(height: 10),
          if (session.isActive)
            Row(
              children: <Widget>[
                Expanded(
                  child: Text(
                    activeChecks.isNotEmpty
                        ? activeChecks
                              .map(
                                (check) => diagnosticCheckLabel(strings, check),
                              )
                              .join(' · ')
                        : session.currentCheck == null
                        ? strings.get('diag_waiting_check')
                        : diagnosticCheckLabel(strings, session.currentCheck!),
                    overflow: TextOverflow.ellipsis,
                    maxLines: 2,
                  ),
                ),
                const SizedBox(width: 12),
                MonoValue(value: '${session.progressPercent}%'),
              ],
            ),
          if (counts.isNotEmpty) ...<Widget>[
            const SizedBox(height: 14),
            Wrap(
              spacing: 8,
              runSpacing: 8,
              children: <Widget>[
                for (final (count, key, tone, icon) in counts)
                  InlineStatus(
                    label: strings.get(key).replaceAll('{count}', '$count'),
                    tone: tone,
                    icon: icon,
                  ),
              ],
            ),
          ],
          if (session.state == DiagnosticSessionState.completed) ...<Widget>[
            const SizedBox(height: 20),
            _Conclusion(
              strings: strings,
              findings: session.findings,
              onReveal: (checkId) => rows.reveal(context, checkId),
            ),
          ],
        ],
      );
    },
  );
}

/// What the completed run found: either a clean result, or the count of
/// problems with the two most severe and their advice.
class _Conclusion extends StatelessWidget {
  const _Conclusion({
    required this.strings,
    required this.findings,
    required this.onReveal,
  });
  final AppStrings strings;
  final List<DiagnosticFinding> findings;
  final ValueChanged<String> onReveal;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final tokens = UsqueTokens.of(context);
    final problems = diagnosticProblems(findings);
    if (problems.isEmpty) {
      return InlineStatus(
        label: strings.get('diag_conclusion_clear'),
        tone: StatusTone.success,
      );
    }
    final failed = problems.first.status == DiagnosticCheckStatus.failed;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: <Widget>[
        Semantics(
          header: true,
          child: Text(
            strings
                .get('diag_conclusion_issues')
                .replaceAll('{count}', '${problems.length}'),
            style: theme.textTheme.titleMedium?.copyWith(
              color: failed ? tokens.danger : tokens.caution,
            ),
          ),
        ),
        const SizedBox(height: 4),
        for (final finding in problems.take(2))
          ActionRow(
            key: ValueKey('diag-conclusion-${finding.checkId}'),
            padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 10),
            onTap: () => onReveal(finding.checkId),
            child: Row(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: <Widget>[
                Icon(
                  diagnosticCheckStatusIcon(finding.status),
                  size: 19,
                  color: diagnosticCheckStatusColor(tokens, finding.status),
                ),
                const SizedBox(width: 12),
                Expanded(
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: <Widget>[
                      Text(
                        diagnosticCheckLabel(strings, finding.checkId),
                        style: theme.textTheme.titleSmall,
                      ),
                      const SizedBox(height: 2),
                      Text(
                        _conclusionDetail(strings, finding),
                        style: theme.textTheme.bodySmall?.copyWith(
                          color: theme.colorScheme.onSurfaceVariant,
                        ),
                      ),
                    ],
                  ),
                ),
                const SizedBox(width: 8),
                const Icon(LucideIcons.arrowDown, size: 17),
              ],
            ),
          ),
      ],
    );
  }
}

/// Advice when the check has some; otherwise what went wrong. A problem
/// never reads as "No action is required."
String _conclusionDetail(AppStrings strings, DiagnosticFinding finding) {
  final remediation = diagnosticRemediationKey(finding);
  if (remediation.isNotEmpty && remediation != 'none') {
    return diagnosticRemediation(strings, remediation);
  }
  final failure = finding.failure;
  return failure != null
      ? diagnosticFailureTitle(strings, failure.code)
      : diagnosticFindingSummary(strings, finding);
}

/// Check results, most severe category first. A category whose checks all
/// passed or were skipped folds to one line.
class DiagnosticResultsPanel extends StatelessWidget {
  const DiagnosticResultsPanel({
    required this.controller,
    required this.rows,
    super.key,
  });
  final AppController controller;
  final DiagnosticRowKeys rows;

  @override
  Widget build(BuildContext context) => _diagnosticsSelector(
    controller: controller,
    selector: () => (
      catalog: controller.strings.catalogId,
      findings:
          controller.diagnostics.session?.findings ??
          const <DiagnosticFinding>[],
    ),
    builder: (context, view) {
      final strings = controller.strings;
      final findings = view.findings;
      if (findings.isEmpty) {
        return ContentSection(
          icon: LucideIcons.listChecks,
          title: strings.get('diag_check_results'),
          children: <Widget>[
            Text(
              strings.get('diag_check_results_empty'),
              style: Theme.of(context).textTheme.bodyMedium?.copyWith(
                color: Theme.of(context).colorScheme.onSurfaceVariant,
              ),
            ),
          ],
        );
      }
      Widget row(DiagnosticFinding finding) => KeyedSubtree(
        key: rows.of(finding.checkId),
        child: DiagnosticCheckTile(finding: finding, strings: strings),
      );
      return PanelStack(
        spacing: 12,
        children: <Widget>[
          for (final group in groupDiagnosticFindings(findings))
            if (group.settled)
              ExpansionTile(
                key: PageStorageKey<String>(
                  'diagnostic-category-${group.category.name}',
                ),
                tilePadding: EdgeInsets.zero,
                childrenPadding: EdgeInsets.zero,
                leading: Icon(diagnosticCategoryIcon(group.category), size: 20),
                title: Text(
                  diagnosticCategoryLabel(strings, group.category),
                  style: Theme.of(context).textTheme.titleMedium,
                ),
                subtitle: Align(
                  alignment: AlignmentDirectional.centerStart,
                  child: InlineStatus(
                    label: strings
                        .get('diag_summary_passed')
                        .replaceAll(
                          '{count}',
                          '${group.findings.where((finding) => finding.status == DiagnosticCheckStatus.passed).length}',
                        ),
                    tone: StatusTone.success,
                  ),
                ),
                children: <Widget>[
                  for (final finding in group.findings) row(finding),
                ],
              )
            else
              ContentSection(
                icon: diagnosticCategoryIcon(group.category),
                title: diagnosticCategoryLabel(strings, group.category),
                gap: 8,
                children: <Widget>[
                  for (final finding in group.findings) row(finding),
                ],
              ),
        ],
      );
    },
  );
}

/// Recent connection transitions; owns its refresh (also F5 on desktop).
class DiagnosticTimelinePanel extends StatelessWidget {
  const DiagnosticTimelinePanel({required this.controller, super.key});
  final AppController controller;

  @override
  Widget build(BuildContext context) => _diagnosticsSelector(
    controller: controller,
    selector: () => (
      catalog: controller.strings.catalogId,
      timeline: controller.diagnostics.timeline,
      loading: controller.diagnostics.timelineLoading,
    ),
    builder: (context, view) {
      final strings = controller.strings;
      return ContentSection(
        icon: LucideIcons.gitCommitVertical,
        title: strings.get('diag_timeline'),
        subtitle: strings.get('diag_timeline_subtitle'),
        trailing: IconButton(
          key: const ValueKey('diag-refresh-timeline'),
          tooltip: strings.get('diag_refresh_timeline'),
          onPressed: view.loading ? null : controller.diagnostics.loadTimeline,
          icon: view.loading
              ? const SizedBox.square(
                  dimension: 17,
                  child: CircularProgressIndicator(strokeWidth: 2),
                )
              : const Icon(LucideIcons.refreshCw),
        ),
        children: <Widget>[
          ConnectionTimelineView(timeline: view.timeline, strings: strings),
        ],
      );
    },
  );
}
