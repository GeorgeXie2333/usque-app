import 'package:flutter/material.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../core/chain_strings.dart';
import '../state/app_controller.dart';
import 'common.dart';

/// The same saved and observed combination on Profiles and its account page.
class ProfilesSummary extends StatelessWidget {
  const ProfilesSummary({required this.controller, this.onRestore, super.key});

  final AppController controller;
  final VoidCallback? onRestore;

  /// The saved choices beside the confirmed running combination, which can
  /// differ until a switch is applied.
  @override
  Widget build(BuildContext context) {
    final app = controller;
    final strings = app.strings, state = app.scenarios.catalog;
    final theme = Theme.of(context);
    final accountId = state?.currentAccountId ?? app.activeProfileId;
    final account = app.profiles.where((a) => a.id == accountId).firstOrNull;
    final accountUnconfirmed =
        app.scenarios.queryUnconfirmed && accountId != app.activeProfileId;
    final runtimeConfirmed =
        app.scenarios.runtimeConfirmed && state?.appliedSceneConfirmed == true;
    final appliedAccount = runtimeConfirmed
        ? app.profiles.where((a) => a.id == state?.appliedAccountId).firstOrNull
        : null;
    final appliedScene = runtimeConfirmed
        ? state?.scenarios
                  .where((scene) => scene.id == state.appliedScenarioId)
                  .firstOrNull
                  ?.name ??
              strings.get('scenario_custom')
        : strings.get('scenario_unknown');
    Widget item(IconData icon, String label, String value, {Key? key}) =>
        Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          mainAxisSize: MainAxisSize.min,
          children: [
            Row(
              mainAxisSize: MainAxisSize.min,
              children: [
                Icon(icon, size: 16, color: theme.colorScheme.onSurfaceVariant),
                const SizedBox(width: 6),
                Flexible(
                  child: Text(
                    label,
                    style: theme.textTheme.bodySmall?.copyWith(
                      color: theme.colorScheme.onSurfaceVariant,
                    ),
                  ),
                ),
              ],
            ),
            const SizedBox(height: 4),
            Text(value, key: key, style: theme.textTheme.titleSmall),
          ],
        );
    final facts = Wrap(
      spacing: 40,
      runSpacing: 12,
      children: [
        item(
          LucideIcons.layers3,
          strings.get('scenario_current'),
          state?.selected?.name ?? strings.get('scenario_custom'),
        ),
        item(
          LucideIcons.user,
          strings.get('scenario_current_account'),
          accountUnconfirmed
              ? strings.get('scenario_unknown')
              : account?.name ?? strings.get('not_available'),
          key: const ValueKey('profiles-selected-account'),
        ),
        if (state?.sessionId != null || app.snapshot.isConnected)
          item(
            LucideIcons.activity,
            strings.chain('current'),
            runtimeConfirmed
                ? '${appliedAccount?.name ?? strings.get('not_available')} · $appliedScene'
                : strings.get('scenario_unknown'),
            key: const ValueKey('profiles-running'),
          ),
      ],
    );
    // Refresh stays beside the facts at every width; it is small, and an
    // uncertain switch is resolved by refreshing before anything else.
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Row(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Expanded(child: facts),
            const SizedBox(width: 16),
            IconButton(
              tooltip: strings.get('settings_refresh'),
              onPressed: app.scenarios.busy ? null : app.scenarios.refresh,
              icon: const Icon(LucideIcons.refreshCw),
            ),
          ],
        ),
        if (state?.canRestore == true && onRestore != null) ...[
          const SizedBox(height: 8),
          Align(
            alignment: AlignmentDirectional.centerStart,
            child: TextButton.icon(
              onPressed: app.scenarios.busy ? null : onRestore,
              icon: const Icon(LucideIcons.history, size: 18),
              label: Text(strings.get('scenario_restore')),
            ),
          ),
        ],
      ],
    );
  }
}

/// Engine and switch problems, above the content they concern.
Widget? profilesOperationBanner(AppController app) {
  final strings = app.strings, scenes = app.scenarios, state = scenes.catalog;
  final failed = scenes.runtimeConfirmed && state?.applyStatus == 'failed';
  final banners = [
    if (app.lastError != null)
      WarningBanner(
        title: strings.get('error_generic'),
        message: app.lastError!,
        danger: true,
        onDismiss: app.clearError,
      ),
    if (app.lastNotice != null)
      WarningBanner(
        title: strings.get('notice'),
        message: app.lastNotice!,
        onDismiss: app.clearNotice,
      ),
    if (state?.currentSettingsError != null)
      WarningBanner(
        title: null,
        message: strings.get(scenarioErrorKey(state!.currentSettingsError)),
      ),
    if (scenes.errorCode != null || scenes.unconfirmed || failed)
      WarningBanner(
        title: null,
        danger: failed,
        message: strings.get(
          scenes.unconfirmed
              ? 'scenario_unknown'
              : failed
              ? 'scenario_failed'
              : scenarioErrorKey(scenes.errorCode),
        ),
      ),
  ];
  if (banners.isEmpty) return null;
  return Column(
    crossAxisAlignment: CrossAxisAlignment.stretch,
    children: [
      for (var i = 0; i < banners.length; i++) ...[
        if (i > 0) const SizedBox(height: 8),
        banners[i],
      ],
    ],
  );
}

String scenarioErrorKey(String? code) => switch (code) {
  'SCENARIO_ACCOUNT_UNAVAILABLE' ||
  'SCENARIO_DEPENDENCY_UNAVAILABLE' => 'scenario_reference_missing',
  'SCENARIO_VPN_PERMISSION_REQUIRED' => 'scenario_vpn_permission',
  'SCENARIO_RULES_UNAVAILABLE' => 'scenario_rules_missing',
  'SCENARIO_UNCONFIRMED' ||
  'SCENARIO_COMMIT_UNCONFIRMED' ||
  'SCENARIO_RESULT_UNKNOWN' => 'scenario_unknown',
  'SCENARIO_PLATFORM_UNSUPPORTED' => 'scenario_platform_unsupported',
  'SCENARIO_NOT_APPLIED' => 'scenario_not_applied',
  'SCENARIO_INVALID_SETTINGS' => 'scenario_settings_invalid',
  'SCENARIO_PROTECTION_UNAVAILABLE' => 'protection_handoff_unavailable',
  'SCENARIO_LIMIT' => 'scenario_limit',
  'SCENARIOS_UNSUPPORTED' => 'scenario_unsupported',
  _ => 'scenario_save_failed',
};
