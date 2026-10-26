import 'package:flutter/material.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../../core/diagnostics_presentation.dart';
import '../../core/diagnostics_strings.dart';
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

/// Progress and counts of the current run.
class DiagnosticSessionPanel extends StatelessWidget {
  const DiagnosticSessionPanel({required this.controller, super.key});
  final AppController controller;

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
          const SizedBox(height: 14),
          Wrap(
            spacing: 8,
            runSpacing: 8,
            children: <Widget>[
              InlineStatus(
                label: strings
                    .get('diag_summary_passed')
                    .replaceAll('{count}', '${summary.passed}'),
                tone: StatusTone.success,
                icon: LucideIcons.circleCheck,
              ),
              InlineStatus(
                label: strings
                    .get('diag_summary_warnings')
                    .replaceAll('{count}', '${summary.warnings}'),
                tone: StatusTone.warning,
                icon: LucideIcons.triangleAlert,
              ),
              InlineStatus(
                label: strings
                    .get('diag_summary_failed')
                    .replaceAll('{count}', '${summary.failed}'),
                tone: StatusTone.danger,
                icon: LucideIcons.circleX,
              ),
              InlineStatus(
                label: strings
                    .get('diag_summary_skipped')
                    .replaceAll('{count}', '${summary.skipped}'),
                tone: StatusTone.neutral,
                icon: LucideIcons.circle,
              ),
            ],
          ),
        ],
      );
    },
  );
}

/// Check results grouped by category.
class DiagnosticResultsPanel extends StatelessWidget {
  const DiagnosticResultsPanel({required this.controller, super.key});
  final AppController controller;

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
      final groups = <Widget>[];
      for (final category in DiagnosticCategory.values) {
        final categoryFindings = findings
            .where((finding) => finding.category == category)
            .toList(growable: false);
        if (categoryFindings.isEmpty) continue;
        groups.add(
          ContentSection(
            icon: diagnosticCategoryIcon(category),
            title: diagnosticCategoryLabel(strings, category),
            gap: 8,
            children: categoryFindings
                .map(
                  (finding) =>
                      DiagnosticCheckTile(finding: finding, strings: strings),
                )
                .toList(growable: false),
          ),
        );
      }
      return PanelStack(spacing: 12, children: groups);
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
