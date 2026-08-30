import 'package:flutter/material.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';
import 'package:url_launcher/url_launcher.dart';

import '../core/connection_presentation.dart';
import '../core/diagnostics_strings.dart';
import '../core/usque_theme.dart';
import '../models/diagnostics_models.dart';
import '../state/app_controller.dart';
import '../widgets/common.dart';
import '../widgets/connection_timeline.dart';
import '../widgets/diagnostic_check_tile.dart';
import '../widgets/usque_dialog.dart';

class DiagnosticsScreen extends StatefulWidget {
  const DiagnosticsScreen({required this.controller, super.key});

  final AppController controller;

  @override
  State<DiagnosticsScreen> createState() => _DiagnosticsScreenState();
}

class _DiagnosticsScreenState extends State<DiagnosticsScreen> {
  DiagnosticMode _selectedMode = DiagnosticMode.standard;

  AppController get controller => widget.controller;

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (mounted) {
        controller.diagnostics.restore(silent: true);
      }
    });
  }

  @override
  Widget build(BuildContext context) {
    return ListenableBuilder(
      listenable: Listenable.merge(<Listenable>[
        controller,
        controller.diagnostics,
      ]),
      builder: (context, _) => _buildPage(context),
    );
  }

  Widget _buildPage(BuildContext context) {
    final strings = controller.strings;
    final diagnostics = controller.diagnostics;
    final session = diagnostics.session;
    final selectedMode = session?.isActive == true
        ? session!.mode
        : diagnostics.requestedMode ?? _selectedMode;
    final presentation = ConnectionPresentation.of(controller.snapshot.phase);

    return FocusTraversalGroup(
      policy: OrderedTraversalPolicy(),
      child: SubPage(
        title: strings.get('diagnostics_title'),
        subtitle: strings.get('diagnostics_page_subtitle'),
        backLabel: strings.get('back'),
        actions: <Widget>[
          OutlinedButton.icon(
            onPressed: diagnostics.timelineLoading
                ? null
                : diagnostics.loadTimeline,
            icon: diagnostics.timelineLoading
                ? const SizedBox.square(
                    dimension: 17,
                    child: CircularProgressIndicator(strokeWidth: 2),
                  )
                : const Icon(LucideIcons.refreshCw),
            label: Text(strings.get('diag_refresh_timeline')),
          ),
        ],
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: <Widget>[
            BannerSlot(
              child: controller.lastError == null
                  ? null
                  : WarningBanner(
                      title: strings.get('error'),
                      message: controller.lastError!,
                      danger: true,
                      onDismiss: controller.clearError,
                    ),
            ),
            BannerSlot(
              child: diagnostics.lastError == null
                  ? null
                  : WarningBanner(
                      title: strings.get('diag_operation_failed'),
                      message: diagnostics.lastError!,
                      danger: true,
                      onDismiss: diagnostics.clearError,
                    ),
            ),
            BannerSlot(
              child: controller.lastNotice == null
                  ? null
                  : WarningBanner(
                      title: strings.get('notice'),
                      message: controller.lastNotice!,
                      onDismiss: controller.clearNotice,
                    ),
            ),
            BannerSlot(
              child:
                  controller.snapshotStreamDegraded &&
                      (controller.snapshot.isConnected ||
                          controller.snapshot.isTransitional)
                  ? WarningBanner(
                      title: strings.get('status_stream_degraded'),
                      message: strings.get('status_stream_degraded_body'),
                    )
                  : null,
            ),
            BannerSlot(
              child: diagnostics.eventStreamDegraded && diagnostics.isActive
                  ? WarningBanner(
                      title: strings.get('diag_event_stream_degraded'),
                      message: strings.get('diag_event_stream_degraded_body'),
                    )
                  : null,
            ),
            PanelStack(
              children: <Widget>[
                _DiagnosticControlPanel(
                  controller: controller,
                  selectedMode: selectedMode,
                  onModeChanged: (mode) => setState(() => _selectedMode = mode),
                  presentation: presentation,
                ),
                if (session != null)
                  _SessionProgressPanel(
                    session: session,
                    controller: controller,
                  ),
                LayoutBuilder(
                  builder: (context, constraints) {
                    final results = _ChecksPanel(
                      controller: controller,
                      session: session,
                    );
                    final timeline = _TimelinePanel(controller: controller);
                    if (constraints.maxWidth < 840) {
                      return PanelStack(children: <Widget>[results, timeline]);
                    }
                    return Row(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: <Widget>[
                        Expanded(flex: 6, child: results),
                        const SizedBox(width: 16),
                        Expanded(flex: 5, child: timeline),
                      ],
                    );
                  },
                ),
                _ExportPanel(
                  controller: controller,
                  onExport: () => _confirmAndExport(context),
                ),
                SectionPanel(
                  icon: LucideIcons.info,
                  title: 'Usque',
                  subtitle: strings.get('unofficial'),
                  trailing: StatusPill(
                    label: strings.get(presentation.labelKey),
                    tone: presentation.tone,
                    icon: controller.snapshot.isConnected
                        ? LucideIcons.circleCheck
                        : LucideIcons.circle,
                  ),
                  children: <Widget>[
                    ReadoutRow(
                      icon: LucideIcons.tag,
                      label: strings.get('version'),
                      value: MonoValue(value: strings.get('app_version')),
                    ),
                    const SizedBox(height: 12),
                    const ReadoutRow(
                      icon: LucideIcons.monitor,
                      label: 'IPC API',
                      value: MonoValue(value: 'usque.v1'),
                    ),
                    const SizedBox(height: 16),
                    Align(
                      alignment: AlignmentDirectional.centerEnd,
                      child: OutlinedButton.icon(
                        onPressed: () => launchUrl(
                          Uri.parse(
                            'https://github.com/GeorgeXie2333/usque-app',
                          ),
                          mode: LaunchMode.externalApplication,
                        ),
                        icon: const Icon(LucideIcons.code2),
                        label: Text(strings.get('source_code')),
                      ),
                    ),
                  ],
                ),
                _DangerPanel(
                  controller: controller,
                  onClear: () => _confirmClearAllData(context),
                ),
              ],
            ),
          ],
        ),
      ),
    );
  }

  Future<void> _confirmAndExport(BuildContext context) async {
    final strings = controller.strings;
    final confirmed = await showDialog<bool>(
      context: context,
      builder: (context) => UsqueDialog(
        icon: LucideIcons.fileArchive,
        title: strings.get('export_diagnostics'),
        width: 500,
        content: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          mainAxisSize: MainAxisSize.min,
          children: <Widget>[
            Text(strings.get('diag_export_included')),
            const SizedBox(height: 8),
            _PrivacyRow(
              icon: LucideIcons.circleCheck,
              text: strings.get('diag_export_included_body'),
            ),
            const SizedBox(height: 6),
            Text(strings.get('diag_export_excluded')),
            const SizedBox(height: 8),
            _PrivacyRow(
              icon: LucideIcons.shieldCheck,
              text: strings.get('diag_export_excluded_body'),
            ),
            const SizedBox(height: 12),
            Text(
              strings.get('diag_export_local_only'),
              style: Theme.of(context).textTheme.bodySmall,
            ),
          ],
        ),
        actions: <Widget>[
          TextButton(
            onPressed: () => Navigator.of(context).pop(false),
            child: Text(strings.get('cancel')),
          ),
          FilledButton.icon(
            onPressed: () => Navigator.of(context).pop(true),
            icon: const Icon(LucideIcons.save),
            label: Text(strings.get('save')),
          ),
        ],
      ),
    );
    if (confirmed != true) return;
    final destination = await controller.diagnostics.export();
    if (context.mounted && destination != null) {
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(
          content: Text('${strings.get('diagnostics_saved')} $destination'),
        ),
      );
    }
  }

  Future<void> _confirmClearAllData(BuildContext context) async {
    final strings = controller.strings;
    final confirmed = await showDialog<bool>(
      context: context,
      builder: (context) => UsqueDialog(
        icon: LucideIcons.triangleAlert,
        title: strings.get('clear_all_data'),
        subtitle: strings.get('clear_all_data_help'),
        danger: true,
        width: 420,
        content: Text(strings.get('clear_all_data_confirm')),
        actions: <Widget>[
          TextButton(
            onPressed: () => Navigator.of(context).pop(false),
            child: Text(strings.get('cancel')),
          ),
          FilledButton.icon(
            style: FilledButton.styleFrom(
              backgroundColor: UsqueTokens.of(context).danger,
              foregroundColor: Theme.of(context).colorScheme.onError,
            ),
            onPressed: () => Navigator.of(context).pop(true),
            icon: const Icon(LucideIcons.trash2),
            label: Text(strings.get('clear_all_data')),
          ),
        ],
      ),
    );
    if (confirmed == true) {
      final cleared = await controller.clearAllData();
      if (cleared && !controller.onboardingComplete && context.mounted) {
        Navigator.of(context).popUntil((route) => route.isFirst);
      }
    }
  }
}

class _DiagnosticControlPanel extends StatelessWidget {
  const _DiagnosticControlPanel({
    required this.controller,
    required this.selectedMode,
    required this.onModeChanged,
    required this.presentation,
  });

  final AppController controller;
  final DiagnosticMode selectedMode;
  final ValueChanged<DiagnosticMode> onModeChanged;
  final ConnectionPresentation presentation;

  @override
  Widget build(BuildContext context) {
    final diagnostics = controller.diagnostics;
    final strings = controller.strings;
    final busy =
        diagnostics.isActive ||
        diagnostics.state == DiagnosticsControllerState.starting ||
        diagnostics.state == DiagnosticsControllerState.cancelling;
    final canRequestCancel =
        diagnostics.isActive ||
        diagnostics.state == DiagnosticsControllerState.starting;
    return SectionPanel(
      icon: LucideIcons.stethoscope,
      title: strings.get('diag_run_title'),
      subtitle: strings.get('diag_run_subtitle'),
      trailing: StatusPill(
        label: strings.get(presentation.labelKey),
        tone: presentation.tone,
        icon: controller.snapshot.isConnected
            ? LucideIcons.circleCheck
            : LucideIcons.circle,
      ),
      children: <Widget>[
        SegmentedButton<DiagnosticMode>(
          segments: <ButtonSegment<DiagnosticMode>>[
            ButtonSegment<DiagnosticMode>(
              value: DiagnosticMode.standard,
              icon: const Icon(LucideIcons.gauge),
              label: Text(strings.get('diag_mode_standard')),
            ),
            ButtonSegment<DiagnosticMode>(
              value: DiagnosticMode.deep,
              icon: const Icon(LucideIcons.microscope),
              label: Text(strings.get('diag_mode_deep')),
            ),
          ],
          selected: <DiagnosticMode>{selectedMode},
          onSelectionChanged: busy
              ? null
              : (values) => onModeChanged(values.first),
          showSelectedIcon: false,
        ),
        if (selectedMode == DiagnosticMode.deep) ...<Widget>[
          const SizedBox(height: 12),
          WarningBanner(
            title: strings.get('diag_deep_title'),
            message: strings.get(
              controller.snapshot.isConnected
                  ? 'diag_deep_connected'
                  : 'diag_deep_disconnected',
            ),
          ),
        ],
        const SizedBox(height: 16),
        Row(
          children: <Widget>[
            Expanded(
              child: FilledButton.icon(
                onPressed: busy ? null : () => diagnostics.start(selectedMode),
                icon: diagnostics.state == DiagnosticsControllerState.starting
                    ? const SizedBox.square(
                        dimension: 17,
                        child: CircularProgressIndicator(strokeWidth: 2),
                      )
                    : const Icon(LucideIcons.play),
                label: Text(strings.get('diag_start')),
              ),
            ),
            if (canRequestCancel ||
                diagnostics.state ==
                    DiagnosticsControllerState.cancelling) ...<Widget>[
              const SizedBox(width: 10),
              OutlinedButton.icon(
                onPressed:
                    diagnostics.state == DiagnosticsControllerState.cancelling
                    ? null
                    : diagnostics.cancel,
                icon: const Icon(LucideIcons.square),
                label: Text(strings.get('cancel')),
              ),
            ],
          ],
        ),
      ],
    );
  }
}

class _SessionProgressPanel extends StatelessWidget {
  const _SessionProgressPanel({
    required this.session,
    required this.controller,
  });

  final DiagnosticSession session;
  final AppController controller;

  @override
  Widget build(BuildContext context) {
    final strings = controller.strings;
    final summary = session.summary;
    return SectionPanel(
      icon: LucideIcons.radio,
      title: strings.get('diag_session'),
      trailing: StatusPill(
        label: diagnosticSessionStateLabel(strings, session.state),
        tone: _sessionTone(session.state),
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
          child: LinearProgressIndicator(value: session.progressPercent / 100),
        ),
        const SizedBox(height: 10),
        Row(
          children: <Widget>[
            Expanded(
              child: Text(
                session.currentCheck == null
                    ? strings.get('diag_waiting_check')
                    : diagnosticCheckLabel(strings, session.currentCheck!),
                overflow: TextOverflow.ellipsis,
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
            StatusPill(
              label: strings
                  .get('diag_summary_passed')
                  .replaceAll('{count}', '${summary.passed}'),
              tone: StatusTone.success,
              icon: LucideIcons.circleCheck,
            ),
            StatusPill(
              label: strings
                  .get('diag_summary_warnings')
                  .replaceAll('{count}', '${summary.warnings}'),
              tone: StatusTone.warning,
              icon: LucideIcons.triangleAlert,
            ),
            StatusPill(
              label: strings
                  .get('diag_summary_failed')
                  .replaceAll('{count}', '${summary.failed}'),
              tone: StatusTone.danger,
              icon: LucideIcons.circleX,
            ),
            StatusPill(
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
  }
}

class _ChecksPanel extends StatelessWidget {
  const _ChecksPanel({required this.controller, required this.session});

  final AppController controller;
  final DiagnosticSession? session;

  @override
  Widget build(BuildContext context) {
    final strings = controller.strings;
    final findings = session?.findings ?? const <DiagnosticFinding>[];
    if (findings.isEmpty) {
      return SectionPanel(
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
        SectionPanel(
          icon: _categoryIcon(category),
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
  }
}

class _TimelinePanel extends StatelessWidget {
  const _TimelinePanel({required this.controller});

  final AppController controller;

  @override
  Widget build(BuildContext context) {
    final strings = controller.strings;
    return SectionPanel(
      icon: LucideIcons.gitCommitVertical,
      title: strings.get('diag_timeline'),
      subtitle: strings.get('diag_timeline_subtitle'),
      children: <Widget>[
        ConnectionTimelineView(
          timeline: controller.diagnostics.timeline,
          strings: strings,
        ),
      ],
    );
  }
}

class _ExportPanel extends StatelessWidget {
  const _ExportPanel({required this.controller, required this.onExport});

  final AppController controller;
  final VoidCallback onExport;

  @override
  Widget build(BuildContext context) {
    final strings = controller.strings;
    final diagnostics = controller.diagnostics;
    return SectionPanel(
      icon: LucideIcons.logs,
      title: strings.get('logs'),
      subtitle: strings.get('diag_logs_subtitle'),
      children: <Widget>[
        if (diagnostics.lastExportPath != null) ...<Widget>[
          SelectableText(
            diagnostics.lastExportPath!,
            style: Theme.of(context).textTheme.bodySmall?.copyWith(
              fontFamily: UsqueFonts.mono,
              fontFamilyFallback: UsqueFonts.monoFallback,
            ),
          ),
          const SizedBox(height: 12),
        ],
        Align(
          alignment: AlignmentDirectional.centerEnd,
          child: FilledButton.tonalIcon(
            onPressed: diagnostics.exporting ? null : onExport,
            icon: diagnostics.exporting
                ? const SizedBox.square(
                    dimension: 17,
                    child: CircularProgressIndicator(strokeWidth: 2),
                  )
                : const Icon(LucideIcons.fileArchive),
            label: Text(strings.get('export_diagnostics')),
          ),
        ),
      ],
    );
  }
}

class _PrivacyRow extends StatelessWidget {
  const _PrivacyRow({required this.icon, required this.text});

  final IconData icon;
  final String text;

  @override
  Widget build(BuildContext context) {
    return Row(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: <Widget>[
        Icon(icon, size: 18, color: UsqueTokens.of(context).success),
        const SizedBox(width: 10),
        Expanded(child: Text(text)),
      ],
    );
  }
}

class _DangerPanel extends StatelessWidget {
  const _DangerPanel({required this.controller, required this.onClear});

  final AppController controller;
  final VoidCallback onClear;

  @override
  Widget build(BuildContext context) {
    final strings = controller.strings;
    final danger = UsqueTokens.of(context).danger;
    return Panel(
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: <Widget>[
          Row(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: <Widget>[
              Icon(LucideIcons.trash2, size: 20, color: danger),
              const SizedBox(width: 12),
              Expanded(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: <Widget>[
                    Text(
                      strings.get('clear_all_data'),
                      style: Theme.of(
                        context,
                      ).textTheme.titleMedium?.copyWith(color: danger),
                    ),
                    const SizedBox(height: 4),
                    Text(
                      strings.get('clear_all_data_help'),
                      style: Theme.of(context).textTheme.bodySmall?.copyWith(
                        color: Theme.of(context).colorScheme.onSurfaceVariant,
                      ),
                    ),
                  ],
                ),
              ),
            ],
          ),
          const SizedBox(height: 16),
          Align(
            alignment: AlignmentDirectional.centerEnd,
            child: OutlinedButton.icon(
              style: OutlinedButton.styleFrom(
                foregroundColor: danger,
                side: BorderSide(color: danger.withValues(alpha: 0.45)),
              ),
              onPressed: controller.busy ? null : onClear,
              icon: const Icon(LucideIcons.trash2),
              label: Text(strings.get('clear_all_data')),
            ),
          ),
        ],
      ),
    );
  }
}

StatusTone _sessionTone(DiagnosticSessionState state) {
  return switch (state) {
    DiagnosticSessionState.pending ||
    DiagnosticSessionState.running => StatusTone.brand,
    DiagnosticSessionState.cancelling ||
    DiagnosticSessionState.cancelled => StatusTone.warning,
    DiagnosticSessionState.completed => StatusTone.success,
    DiagnosticSessionState.failed => StatusTone.danger,
  };
}

IconData _categoryIcon(DiagnosticCategory category) {
  return switch (category) {
    DiagnosticCategory.localComponent => LucideIcons.cpu,
    DiagnosticCategory.physicalNetwork => LucideIcons.wifi,
    DiagnosticCategory.transport => LucideIcons.route,
    DiagnosticCategory.tunnel => LucideIcons.network,
    DiagnosticCategory.protection => LucideIcons.shieldCheck,
    DiagnosticCategory.recovery => LucideIcons.rotateCcw,
  };
}
