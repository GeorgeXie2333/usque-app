import 'package:flutter/material.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../core/app_strings.dart';
import '../core/chain_strings.dart';
import '../core/usque_theme.dart';
import '../models/app_models.dart';
import 'country_flag.dart';
import 'save_changes_bar.dart';
import 'usque_dialog.dart';

class VpnGateNodeIdentity extends StatelessWidget {
  const VpnGateNodeIdentity({required this.server, super.key});
  final VpnGateServer server;

  @override
  Widget build(BuildContext context) => Row(
    children: [
      CountryFlag(countryCode: server.countryCode),
      const SizedBox(width: 8),
      Expanded(
        child: Text(
          '${server.countryCode ?? '—'} · ${server.ip}',
          style: Theme.of(context).textTheme.bodyMedium?.copyWith(
            fontFeatures: UsqueTheme.tabularFigures,
          ),
        ),
      ),
    ],
  );
}

class VpnGateSelectionBar extends StatelessWidget {
  const VpnGateSelectionBar({
    required this.strings,
    required this.draft,
    required this.savedEnabled,
    required this.dirty,
    required this.connected,
    required this.saving,
    required this.preparing,
    required this.onApply,
    required this.onCancel,
    this.server,
    this.saveError,
    this.nodeError,
    this.statusLabel,
    super.key,
  });
  final AppStrings strings;
  final VpnGateSettings draft;
  final bool savedEnabled, dirty, connected, saving, preparing;
  final VpnGateServer? server;
  final String? saveError, nodeError;
  final String? statusLabel;
  final VoidCallback? onApply;
  final VoidCallback onCancel;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final disabling = dirty && savedEnabled && !draft.enabled;
    final error = nodeError != null
        ? strings.get('gate_prepare_error')
        : saveError == null
        ? null
        : strings.get(saveError!);
    return SaveChangesBar(
      key: const ValueKey('vpn-gate-selection-bar'),
      saveButtonKey: const ValueKey('vpn-gate-apply'),
      strings: strings,
      contentWidth: 880,
      matchPageGutter: true,
      dirty: dirty,
      saving: saving && !preparing,
      onSave: onApply,
      error: error,
      validationError: dirty && draft.enabled && !draft.hasSelection
          ? strings.get('gate_not_selected')
          : null,
      statusLabel: error == null && !preparing ? statusLabel : null,
      saveLabel: connected && dirty
          ? strings.chain('apply_reconnect')
          : strings.get('save_changes'),
      summary: !dirty
          ? null
          : disabling
          ? Text(
              strings.chain('pending_disable'),
              style: theme.textTheme.labelLarge,
            )
          : draft.enabled && draft.hasSelection
          ? Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              mainAxisSize: MainAxisSize.min,
              children: [
                Text(
                  '${strings.chain('draft')}: VPN Gate',
                  style: theme.textTheme.labelLarge,
                ),
                const SizedBox(height: 2),
                if (server != null && server!.matches(draft))
                  VpnGateNodeIdentity(server: server!)
                else
                  Text(draft.serverId, style: UsqueTheme.mono(context)),
              ],
            )
          : null,
      activity: preparing || nodeError != null
          ? Column(
              mainAxisSize: MainAxisSize.min,
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                if (preparing) ...[
                  const LinearProgressIndicator(minHeight: 2),
                  Row(
                    children: [
                      Expanded(child: Text(strings.get('gate_preparing'))),
                      TextButton.icon(
                        key: const ValueKey('vpn-gate-cancel-node'),
                        onPressed: onCancel,
                        icon: const Icon(LucideIcons.x, size: 18),
                        label: Text(strings.get('cancel')),
                      ),
                    ],
                  ),
                ],
                if (nodeError != null)
                  Align(
                    alignment: AlignmentDirectional.centerStart,
                    child: TextButton.icon(
                      onPressed: () =>
                          showVpnGateError(context, strings, nodeError!),
                      icon: const Icon(LucideIcons.info, size: 18),
                      label: Text(strings.get('gate_error_details')),
                    ),
                  ),
              ],
            )
          : null,
    );
  }
}

void showVpnGateError(BuildContext context, AppStrings strings, String error) {
  showDialog<void>(
    context: context,
    builder: (context) => UsqueDialog(
      title: strings.get('gate_error_details'),
      icon: LucideIcons.info,
      content: SingleChildScrollView(child: SelectableText(error)),
      actions: [
        TextButton(
          onPressed: () => Navigator.of(context).pop(),
          child: Text(strings.get('close')),
        ),
      ],
    ),
  );
}
