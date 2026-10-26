import 'package:flutter/material.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../../core/usque_theme.dart';
import '../../state/app_controller.dart';
import '../common.dart';
import '../listenable_selector.dart';
import '../usque_dialog.dart';

/// States what the bundle contains and excludes before writing it, then
/// reports where it was saved. Nothing is uploaded.
Future<void> exportDiagnosticsWithConsent(
  BuildContext context,
  AppController controller,
) async {
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

class DiagnosticExportPanel extends StatelessWidget {
  const DiagnosticExportPanel({required this.controller, super.key});
  final AppController controller;

  @override
  Widget build(BuildContext context) => ListenableSelector(
    listenables: <Listenable>[controller, controller.diagnostics],
    selector: () => (
      catalog: controller.strings.catalogId,
      path: controller.diagnostics.lastExportPath,
      exporting: controller.diagnostics.exporting,
    ),
    builder: (context, view) {
      final strings = controller.strings;
      return ContentSection(
        icon: LucideIcons.logs,
        title: strings.get('logs'),
        subtitle: strings.get('diag_logs_subtitle'),
        children: <Widget>[
          if (view.path case final path?) ...<Widget>[
            SelectableText(
              path,
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
              onPressed: view.exporting
                  ? null
                  : () => exportDiagnosticsWithConsent(context, controller),
              icon: view.exporting
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
    },
  );
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
