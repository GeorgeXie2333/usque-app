import 'package:flutter/material.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';
import 'package:url_launcher/url_launcher.dart';

import '../core/connection_presentation.dart';
import '../core/usque_theme.dart';
import '../state/app_controller.dart';
import '../widgets/common.dart';
import '../widgets/usque_dialog.dart';

class DiagnosticsScreen extends StatelessWidget {
  const DiagnosticsScreen({required this.controller, super.key});

  final AppController controller;

  @override
  Widget build(BuildContext context) {
    final strings = controller.strings;
    final ConnectionPresentation presentation = ConnectionPresentation.of(
      controller.snapshot.phase,
    );
    return PageFrame(
      title: strings.get('diagnostics_title'),
      subtitle: strings.get('diagnostics_subtitle'),
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
          PanelStack(
            children: <Widget>[
              SectionPanel(
                icon: LucideIcons.activity,
                title: strings.get('engine_status'),
                trailing: StatusPill(
                  label: strings.get(presentation.labelKey),
                  tone: presentation.tone,
                  icon: controller.snapshot.isConnected
                      ? LucideIcons.circleCheck
                      : LucideIcons.circle,
                ),
                gap: 20,
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
                  const SizedBox(height: 12),
                  ReadoutRow(
                    icon: LucideIcons.scale,
                    label: strings.get('license'),
                    value: const MonoValue(value: 'MIT'),
                  ),
                ],
              ),
              SectionPanel(
                icon: LucideIcons.logs,
                title: strings.get('logs'),
                subtitle: strings.get('export_help'),
                children: <Widget>[
                  Align(
                    alignment: AlignmentDirectional.centerEnd,
                    child: FilledButton.tonalIcon(
                      onPressed: controller.busy
                          ? null
                          : () => _confirmAndExport(context),
                      icon: const Icon(LucideIcons.fileArchive),
                      label: Text(strings.get('export_diagnostics')),
                    ),
                  ),
                ],
              ),
              SectionPanel(
                icon: LucideIcons.info,
                title: 'Usque',
                subtitle: strings.get('unofficial'),
                children: <Widget>[
                  Align(
                    alignment: AlignmentDirectional.centerEnd,
                    child: OutlinedButton.icon(
                      onPressed: () => launchUrl(
                        Uri.parse('https://github.com/GeorgeXie2333/usque-app'),
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
    );
  }

  Future<void> _confirmAndExport(BuildContext context) async {
    final strings = controller.strings;
    final confirmed = await showDialog<bool>(
      context: context,
      builder: (context) => UsqueDialog(
        icon: LucideIcons.fileArchive,
        title: strings.get('export_diagnostics'),
        width: 420,
        content: Text(strings.get('export_help')),
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
    if (confirmed == true) {
      await controller.exportDiagnostics();
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
      await controller.clearAllData();
    }
  }
}

/// Destructive work lives in its own plate, ruled in the danger colour so it
/// never reads as one more setting.
class _DangerPanel extends StatelessWidget {
  const _DangerPanel({required this.controller, required this.onClear});

  final AppController controller;
  final VoidCallback onClear;

  @override
  Widget build(BuildContext context) {
    final strings = controller.strings;
    final Color danger = UsqueTokens.of(context).danger;
    return Panel(
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: <Widget>[
          Row(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: <Widget>[
              Container(
                width: 34,
                height: 34,
                alignment: Alignment.center,
                decoration: BoxDecoration(
                  color: danger.withValues(alpha: UsqueTokens.of(context).tint),
                  borderRadius: BorderRadius.circular(UsqueRadii.chip),
                ),
                child: Icon(LucideIcons.trash2, size: 17, color: danger),
              ),
              const SizedBox(width: 12),
              Expanded(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: <Widget>[
                    Padding(
                      padding: const EdgeInsets.only(top: 2),
                      child: Text(
                        strings.get('clear_all_data'),
                        style: Theme.of(
                          context,
                        ).textTheme.titleMedium?.copyWith(color: danger),
                      ),
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
          const SizedBox(height: 18),
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
