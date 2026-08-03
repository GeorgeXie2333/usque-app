import 'package:flutter/material.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';
import 'package:url_launcher/url_launcher.dart';

import '../models/app_models.dart';
import '../state/app_controller.dart';
import '../widgets/common.dart';

class DiagnosticsScreen extends StatelessWidget {
  const DiagnosticsScreen({required this.controller, super.key});

  final AppController controller;

  @override
  Widget build(BuildContext context) {
    final strings = controller.strings;
    return PageFrame(
      title: strings.get('diagnostics_title'),
      subtitle: strings.get('diagnostics_subtitle'),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: <Widget>[
          if (controller.lastError != null) ...<Widget>[
            WarningBanner(
              title: strings.get('error'),
              message: controller.lastError!,
              danger: true,
              onDismiss: controller.clearError,
            ),
            const SizedBox(height: 16),
          ],
          if (controller.lastNotice != null) ...<Widget>[
            WarningBanner(
              title: strings.get('notice'),
              message: controller.lastNotice!,
              onDismiss: controller.clearNotice,
            ),
            const SizedBox(height: 16),
          ],
          if (controller.snapshotStreamDegraded) ...<Widget>[
            WarningBanner(
              title: strings.get('status_stream_degraded'),
              message: strings.get('status_stream_degraded_body'),
            ),
            const SizedBox(height: 16),
          ],
          Panel(
            child: Column(
              children: <Widget>[
                SectionTitle(
                  icon: LucideIcons.activity,
                  title: strings.get('engine_status'),
                  trailing: StatusPill(
                    label: strings.get(
                      _connectionPhaseKey(controller.snapshot.phase),
                    ),
                    tone: controller.snapshot.isConnected
                        ? StatusTone.success
                        : StatusTone.neutral,
                    icon: controller.snapshot.isConnected
                        ? LucideIcons.circleCheck
                        : LucideIcons.circle,
                  ),
                ),
                const Divider(height: 30),
                _InfoRow(
                  icon: LucideIcons.tag,
                  title: strings.get('version'),
                  value: strings.get('app_version'),
                ),
                const Divider(height: 30),
                _InfoRow(
                  icon: LucideIcons.monitor,
                  title: 'IPC API',
                  value: 'usque.v1',
                ),
              ],
            ),
          ),
          const SizedBox(height: 16),
          Panel(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: <Widget>[
                SectionTitle(
                  icon: LucideIcons.trash2,
                  title: strings.get('clear_all_data'),
                  subtitle: strings.get('clear_all_data_help'),
                ),
                const SizedBox(height: 18),
                Align(
                  alignment: Alignment.centerLeft,
                  child: FilledButton.icon(
                    style: FilledButton.styleFrom(
                      backgroundColor: Theme.of(context).colorScheme.error,
                      foregroundColor: Theme.of(context).colorScheme.onError,
                    ),
                    onPressed: controller.busy
                        ? null
                        : () => _confirmClearAllData(context),
                    icon: const Icon(LucideIcons.trash2),
                    label: Text(strings.get('clear_all_data')),
                  ),
                ),
              ],
            ),
          ),
          const SizedBox(height: 16),
          Panel(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: <Widget>[
                SectionTitle(
                  icon: LucideIcons.logs,
                  title: strings.get('logs'),
                  subtitle: strings.get('logs_help'),
                ),
                const SizedBox(height: 18),
                Align(
                  alignment: Alignment.centerLeft,
                  child: FilledButton.tonalIcon(
                    onPressed: controller.busy
                        ? null
                        : () => _confirmAndExport(context),
                    icon: const Icon(LucideIcons.fileArchive),
                    label: Text(strings.get('export_diagnostics')),
                  ),
                ),
                const SizedBox(height: 12),
                Text(
                  strings.get('export_help'),
                  style: Theme.of(context).textTheme.bodySmall?.copyWith(
                    color: Theme.of(context).colorScheme.onSurfaceVariant,
                  ),
                ),
              ],
            ),
          ),
          const SizedBox(height: 16),
          Panel(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: <Widget>[
                SectionTitle(
                  icon: LucideIcons.info,
                  title: 'Usque',
                  subtitle: strings.get('unofficial'),
                ),
                const SizedBox(height: 20),
                _InfoRow(
                  icon: LucideIcons.shieldCheck,
                  title: strings.get('privacy'),
                  value: strings.get('privacy_body'),
                ),
                const Divider(height: 30),
                _InfoRow(
                  icon: LucideIcons.scale,
                  title: strings.get('license'),
                  value: 'MIT',
                ),
                const SizedBox(height: 22),
                Align(
                  alignment: Alignment.centerLeft,
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
          ),
        ],
      ),
    );
  }

  Future<void> _confirmAndExport(BuildContext context) async {
    final strings = controller.strings;
    final confirmed = await showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        icon: const Icon(LucideIcons.shieldCheck),
        title: Text(strings.get('export_diagnostics')),
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
      builder: (context) => AlertDialog(
        icon: Icon(
          LucideIcons.triangleAlert,
          color: Theme.of(context).colorScheme.error,
        ),
        title: Text(strings.get('clear_all_data')),
        content: Text(strings.get('clear_all_data_confirm')),
        actions: <Widget>[
          TextButton(
            onPressed: () => Navigator.of(context).pop(false),
            child: Text(strings.get('cancel')),
          ),
          FilledButton.icon(
            style: FilledButton.styleFrom(
              backgroundColor: Theme.of(context).colorScheme.error,
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

String _connectionPhaseKey(ConnectionPhase phase) {
  return switch (phase) {
    ConnectionPhase.disconnected => 'disconnected',
    ConnectionPhase.preparing => 'preparing',
    ConnectionPhase.connectingH3 ||
    ConnectionPhase.connectingH2 => 'connecting',
    ConnectionPhase.connected => 'connected',
    ConnectionPhase.degraded => 'degraded',
    ConnectionPhase.reconnecting => 'reconnecting',
    ConnectionPhase.disconnecting => 'disconnecting',
    ConnectionPhase.captivePortalPaused => 'captive_pause',
    ConnectionPhase.error => 'error',
  };
}

class _InfoRow extends StatelessWidget {
  const _InfoRow({
    required this.icon,
    required this.title,
    required this.value,
  });

  final IconData icon;
  final String title;
  final String value;

  @override
  Widget build(BuildContext context) {
    return Row(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: <Widget>[
        Icon(
          icon,
          size: 20,
          color: Theme.of(context).colorScheme.onSurfaceVariant,
        ),
        const SizedBox(width: 13),
        SizedBox(width: 120, child: Text(title)),
        const SizedBox(width: 12),
        Expanded(
          child: SelectableText(
            value,
            style: Theme.of(
              context,
            ).textTheme.bodyMedium?.copyWith(fontWeight: FontWeight.w600),
          ),
        ),
      ],
    );
  }
}
