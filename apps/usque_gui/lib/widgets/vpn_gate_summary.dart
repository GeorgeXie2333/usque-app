import 'package:flutter/material.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../core/app_strings.dart';
import '../core/usque_theme.dart';
import '../core/vpn_gate_presentation.dart';
import '../models/app_models.dart';
import 'common.dart';
import 'country_flag.dart';
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
          style: Theme.of(
            context,
          ).textTheme.bodyMedium?.copyWith(fontFamily: UsqueFonts.mono),
        ),
      ),
    ],
  );
}

class VpnGateConnectionSummary extends StatelessWidget {
  const VpnGateConnectionSummary({
    required this.strings,
    required this.view,
    this.error,
    super.key,
  });
  final AppStrings strings;
  final VpnGatePresentation view;
  final String? error;

  @override
  Widget build(BuildContext context) => ContentSection(
    key: const ValueKey('vpn-gate-connection-status'),
    title: strings.get('connection_status'),
    gap: 12,
    child: PanelStack(
      spacing: 8,
      children: [
        Semantics(
          liveRegion: true,
          child: InlineStatus(
            label: strings.get(view.statusKey),
            tone: view.failed
                ? StatusTone.danger
                : view.connected
                ? StatusTone.success
                : view.busy
                ? StatusTone.brand
                : StatusTone.neutral,
          ),
        ),
        if (view.warpKey != null)
          Text(
            'WARP: ${strings.get(view.warpKey!)}',
            style: Theme.of(context).textTheme.bodyMedium?.copyWith(
              color: Theme.of(context).colorScheme.onSurfaceVariant,
            ),
          ),
        if (view.server case final server?) ...[
          Text(
            strings.get(view.serverLabelKey),
            style: Theme.of(context).textTheme.labelMedium,
          ),
          VpnGateNodeIdentity(server: server),
        ],
        if (view.failed && error != null)
          Align(
            alignment: AlignmentDirectional.centerStart,
            child: TextButton.icon(
              onPressed: () => _showError(context, strings, error!),
              icon: const Icon(LucideIcons.info, size: 18),
              label: Text(strings.get('gate_error_details')),
            ),
          ),
      ],
    ),
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
    required this.actionKey,
    required this.onApply,
    required this.onCancel,
    this.server,
    this.saveError,
    this.nodeError,
    super.key,
  });
  final AppStrings strings;
  final VpnGateSettings draft;
  final bool savedEnabled, dirty, connected, saving, preparing;
  final VpnGateServer? server;
  final String actionKey;
  final String? saveError, nodeError;
  final VoidCallback? onApply;
  final VoidCallback onCancel;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final disabling = dirty && savedEnabled && !draft.enabled;
    final label = disabling
        ? 'gate_pending_disable'
        : !draft.hasSelection
        ? 'gate_not_selected'
        : !dirty
        ? 'gate_saved'
        : connected
        ? 'gate_draft'
        : 'gate_pending_save';
    final summary = PanelStack(
      spacing: 6,
      children: [
        Text(strings.get(label), style: theme.textTheme.labelLarge),
        if (!disabling && draft.hasSelection)
          if (server != null && server!.matches(draft))
            VpnGateNodeIdentity(server: server!)
          else
            Text(draft.serverId, style: theme.textTheme.bodyMedium)
        else if (!disabling && !draft.enabled)
          Text(
            strings.get('gate_enable_to_choose'),
            style: theme.textTheme.bodyMedium?.copyWith(
              color: theme.colorScheme.onSurfaceVariant,
            ),
          ),
      ],
    );
    final apply = FilledButton.icon(
      key: const ValueKey('vpn-gate-apply'),
      onPressed: onApply,
      icon: saving && !preparing
          ? const SizedBox.square(
              dimension: 18,
              child: CircularProgressIndicator(strokeWidth: 2),
            )
          : const Icon(LucideIcons.check, size: 18),
      label: Text(
        strings.get(saving && !preparing ? 'saving_changes' : actionKey),
        textAlign: TextAlign.center,
      ),
    );
    return Material(
      key: const ValueKey('vpn-gate-selection-bar'),
      color: theme.colorScheme.surface,
      child: DecoratedBox(
        decoration: BoxDecoration(
          border: Border(
            top: BorderSide(color: UsqueTokens.of(context).hairline),
          ),
        ),
        child: SafeArea(
          top: false,
          child: ConstrainedBox(
            constraints: BoxConstraints(
              maxHeight: MediaQuery.sizeOf(context).height * .45,
            ),
            child: SingleChildScrollView(
              primary: false,
              child: Padding(
                padding: EdgeInsets.symmetric(
                  horizontal: MediaQuery.sizeOf(context).width < 600 ? 16 : 32,
                  vertical: 12,
                ),
                child: Center(
                  heightFactor: 1,
                  child: ConstrainedBox(
                    constraints: const BoxConstraints(maxWidth: 880),
                    child: LayoutBuilder(
                      builder: (context, constraints) {
                        final stacked =
                            constraints.maxWidth < 600 ||
                            MediaQuery.textScalerOf(context).scale(14) > 21;
                        return PanelStack(
                          spacing: 12,
                          children: [
                            if (preparing) ...[
                              const LinearProgressIndicator(minHeight: 2),
                              Row(
                                children: [
                                  Expanded(
                                    child: Text(strings.get('gate_preparing')),
                                  ),
                                  const SizedBox(width: 8),
                                  TextButton.icon(
                                    key: const ValueKey('vpn-gate-cancel-node'),
                                    onPressed: onCancel,
                                    icon: const Icon(LucideIcons.x, size: 18),
                                    label: Text(strings.get('cancel')),
                                  ),
                                ],
                              ),
                            ],
                            if (saveError != null || nodeError != null)
                              Row(
                                crossAxisAlignment: CrossAxisAlignment.start,
                                children: [
                                  Expanded(
                                    child: Text(
                                      strings.get(
                                        nodeError != null
                                            ? 'gate_prepare_error'
                                            : saveError!,
                                      ),
                                      style: theme.textTheme.bodyMedium
                                          ?.copyWith(
                                            color: theme.colorScheme.error,
                                          ),
                                    ),
                                  ),
                                  if (nodeError != null)
                                    IconButton(
                                      tooltip: strings.get(
                                        'gate_error_details',
                                      ),
                                      onPressed: () => _showError(
                                        context,
                                        strings,
                                        nodeError!,
                                      ),
                                      icon: const Icon(
                                        LucideIcons.info,
                                        size: 20,
                                      ),
                                    ),
                                ],
                              ),
                            if (stacked)
                              Column(
                                crossAxisAlignment: CrossAxisAlignment.stretch,
                                children: [
                                  summary,
                                  const SizedBox(height: 12),
                                  apply,
                                ],
                              )
                            else
                              Row(
                                children: [
                                  Expanded(child: summary),
                                  const SizedBox(width: 24),
                                  ConstrainedBox(
                                    constraints: BoxConstraints(
                                      maxWidth: constraints.maxWidth * .42,
                                    ),
                                    child: apply,
                                  ),
                                ],
                              ),
                          ],
                        );
                      },
                    ),
                  ),
                ),
              ),
            ),
          ),
        ),
      ),
    );
  }
}

void _showError(BuildContext context, AppStrings strings, String error) {
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
