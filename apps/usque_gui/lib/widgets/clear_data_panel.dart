import 'package:flutter/material.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../core/usque_theme.dart';
import '../state/app_controller.dart';
import 'common.dart';
import 'controller_selector.dart';
import 'usque_dialog.dart';

/// The isolated danger area that deletes every account, setting, cache and
/// diagnostic record on this device after an explicit confirmation.
class ClearDataPanel extends StatelessWidget {
  const ClearDataPanel({required this.controller, super.key});
  final AppController controller;

  Future<void> _confirmAndClear(BuildContext context) async {
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
    if (confirmed != true) return;
    final cleared = await controller.clearAllData();
    // A reset returns to onboarding, so leave every subpage above it.
    if (cleared && !controller.onboardingComplete && context.mounted) {
      Navigator.of(context).popUntil((route) => route.isFirst);
    }
  }

  @override
  Widget build(BuildContext context) => ControllerSelector(
    controller: controller,
    selector: (app) => (catalog: app.strings.catalogId, busy: app.busy),
    builder: (context, view) {
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
                onPressed: view.busy ? null : () => _confirmAndClear(context),
                icon: const Icon(LucideIcons.trash2),
                label: Text(strings.get('clear_all_data')),
              ),
            ),
          ],
        ),
      );
    },
  );
}
