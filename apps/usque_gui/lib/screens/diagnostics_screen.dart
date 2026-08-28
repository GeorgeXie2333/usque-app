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
      listenable: controller.diagnostics,
      builder: (context, _) => _buildPage(context),
    );
  }

  Widget _buildPage(BuildContext context) {
    final strings = controller.strings;
    final diagnostics = controller.diagnostics;
    final session = diagnostics.session;
    final presentation = ConnectionPresentation.of(controller.snapshot.phase);
    final zh = strings.languageCode == 'zh';

    return FocusTraversalGroup(
      policy: OrderedTraversalPolicy(),
      child: PageFrame(
        title: strings.get('diagnostics_title'),
        subtitle: zh
            ? '逐层检查连接、平台保护和恢复状态；结果只保存在本机。'
            : 'Inspect connection, platform protection, and recovery state. Results remain local.',
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
            label: Text(zh ? '刷新时间线' : 'Refresh timeline'),
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
                      title: zh ? '诊断操作失败' : 'Diagnostics operation failed',
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
                      title: zh
                          ? '诊断事件流已中断'
                          : 'Diagnostics event stream interrupted',
                      message: zh
                          ? '正在通过有界轮询恢复会话状态；诊断不会重复启动。'
                          : 'Session state is being recovered with bounded polling; the run will not restart.',
                    )
                  : null,
            ),
            PanelStack(
              children: <Widget>[
                _DiagnosticControlPanel(
                  controller: controller,
                  selectedMode: _selectedMode,
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
    final zh = strings.languageCode == 'zh';
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
            Text(zh ? '将包含：' : 'Included:'),
            const SizedBox(height: 8),
            _PrivacyRow(
              icon: LucideIcons.circleCheck,
              text: zh
                  ? '错误码、阶段、相对耗时、计数器和布尔状态'
                  : 'Error codes, stages, relative timing, counters, and boolean state',
            ),
            const SizedBox(height: 6),
            Text(zh ? '不会包含：' : 'Excluded:'),
            const SizedBox(height: 8),
            _PrivacyRow(
              icon: LucideIcons.shieldCheck,
              text: zh
                  ? '密钥、Token、Profile 名称、完整地址、SSID、应用列表和用户路径'
                  : 'Keys, tokens, profile names, full addresses, SSIDs, app lists, and user paths',
            ),
            const SizedBox(height: 12),
            Text(
              zh
                  ? '压缩包只写入你选择的位置，不会自动上传。'
                  : 'The archive is written only to the location you choose and is never uploaded automatically.',
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
    if (confirmed == true) await controller.clearAllData();
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
    final zh = strings.languageCode == 'zh';
    final busy =
        diagnostics.isActive ||
        diagnostics.state == DiagnosticsControllerState.starting ||
        diagnostics.state == DiagnosticsControllerState.cancelling;
    final canRequestCancel =
        diagnostics.isActive ||
        diagnostics.state == DiagnosticsControllerState.starting;
    return SectionPanel(
      icon: LucideIcons.stethoscope,
      title: zh ? '运行网络诊断' : 'Run network diagnostics',
      subtitle: zh
          ? '标准模式仅做只读和被动检查。'
          : 'Standard mode uses read-only and passive checks.',
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
              label: Text(zh ? '标准' : 'Standard'),
            ),
            ButtonSegment<DiagnosticMode>(
              value: DiagnosticMode.deep,
              icon: const Icon(LucideIcons.microscope),
              label: Text(zh ? '深度' : 'Deep'),
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
            title: zh ? '深度诊断说明' : 'About deep diagnostics',
            message: controller.snapshot.isConnected
                ? (zh
                      ? '当前已有连接：不会创建第二条 MASQUE 数据通道，主动传输检查将明确标记为跳过或警告。'
                      : 'A tunnel is active: no second MASQUE data path will be opened; active transport checks will be marked skipped or warning.')
                : (zh
                      ? '未连接时可运行受超时与取消保护的主动检查；完成后会比较平台状态。'
                      : 'When disconnected, active checks are bounded by timeout and cancellation, followed by a platform-state comparison.'),
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
                label: Text(zh ? '开始诊断' : 'Start diagnostics'),
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
                label: Text(zh ? '取消' : 'Cancel'),
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
    final zh = strings.languageCode == 'zh';
    final summary = session.summary;
    return SectionPanel(
      icon: LucideIcons.radio,
      title: zh ? '诊断会话' : 'Diagnostic session',
      trailing: StatusPill(
        label: _sessionStateLabel(zh, session.state),
        tone: _sessionTone(session.state),
        icon: session.isActive
            ? LucideIcons.loaderCircle
            : LucideIcons.circleCheck,
      ),
      children: <Widget>[
        Semantics(
          label: zh
              ? '诊断进度 ${session.progressPercent}%'
              : 'Diagnostic progress ${session.progressPercent}%',
          value: '${session.progressPercent}%',
          child: LinearProgressIndicator(value: session.progressPercent / 100),
        ),
        const SizedBox(height: 10),
        Row(
          children: <Widget>[
            Expanded(
              child: Text(
                session.currentCheck == null
                    ? (zh ? '等待检查状态…' : 'Waiting for check state…')
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
              label: '${zh ? '通过' : 'Passed'} ${summary.passed}',
              tone: StatusTone.success,
              icon: LucideIcons.circleCheck,
            ),
            StatusPill(
              label: '${zh ? '警告' : 'Warnings'} ${summary.warnings}',
              tone: StatusTone.warning,
              icon: LucideIcons.triangleAlert,
            ),
            StatusPill(
              label: '${zh ? '失败' : 'Failed'} ${summary.failed}',
              tone: StatusTone.danger,
              icon: LucideIcons.circleX,
            ),
            StatusPill(
              label: '${zh ? '跳过' : 'Skipped'} ${summary.skipped}',
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
    final zh = strings.languageCode == 'zh';
    final findings = session?.findings ?? const <DiagnosticFinding>[];
    if (findings.isEmpty) {
      return SectionPanel(
        icon: LucideIcons.listChecks,
        title: zh ? '检查结果' : 'Check results',
        children: <Widget>[
          Text(
            zh
                ? '启动诊断后，检查会按依赖关系和层级显示在这里。'
                : 'Start a diagnostic run to see dependency-aware checks grouped by layer.',
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
      title: strings.languageCode == 'zh' ? '连接时间线' : 'Connection timeline',
      subtitle: strings.languageCode == 'zh'
          ? '仅记录有界状态事件，不记录逐包内容或原始地址。'
          : 'Bounded state events only; packet contents and raw addresses are never recorded.',
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
    final zh = strings.languageCode == 'zh';
    return SectionPanel(
      icon: LucideIcons.logs,
      title: strings.get('logs'),
      subtitle: zh
          ? '导出会话、时间线、平台健康摘要和脱敏日志。'
          : 'Export the session, timeline, platform-health summary, and sanitized logs.',
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

String _sessionStateLabel(bool zh, DiagnosticSessionState state) {
  return switch (state) {
    DiagnosticSessionState.pending => zh ? '准备中' : 'Pending',
    DiagnosticSessionState.running => zh ? '运行中' : 'Running',
    DiagnosticSessionState.cancelling => zh ? '取消中' : 'Cancelling',
    DiagnosticSessionState.completed => zh ? '已完成' : 'Completed',
    DiagnosticSessionState.failed => zh ? '失败' : 'Failed',
    DiagnosticSessionState.cancelled => zh ? '已取消' : 'Cancelled',
  };
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
