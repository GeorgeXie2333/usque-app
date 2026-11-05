import 'package:flutter/material.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../core/chain_strings.dart';
import '../state/app_controller.dart';
import 'common.dart';

/// The saved account choice, or unknown while an unconfirmed read may have
/// replaced it.
String profilesSelectedAccountLabel(AppController app) {
  final strings = app.strings, state = app.scenarios.catalog;
  final accountId = state?.currentAccountId ?? app.activeProfileId;
  if (app.scenarios.queryUnconfirmed && accountId != app.activeProfileId) {
    return strings.get('scenario_unknown');
  }
  return app.profiles.where((a) => a.id == accountId).firstOrNull?.name ??
      strings.get('not_available');
}

/// The confirmed running account and scene, or null without a session. A
/// combination the Engine has not confirmed reads as unknown.
String? profilesRunningLabel(AppController app) {
  final strings = app.strings, state = app.scenarios.catalog;
  if (state?.sessionId == null && !app.snapshot.isConnected) return null;
  if (!app.scenarios.runtimeConfirmed || state?.appliedSceneConfirmed != true) {
    return strings.get('scenario_unknown');
  }
  final account = app.profiles
      .where((a) => a.id == state!.appliedAccountId)
      .firstOrNull;
  final scene =
      state!.scenarios
          .where((scene) => scene.id == state.appliedScenarioId)
          .firstOrNull
          ?.name ??
      strings.get('scenario_custom');
  return '${account?.name ?? strings.get('not_available')} · $scene';
}

/// The saved account beside the confirmed running combination on Accounts,
/// which can differ until a switch is applied.
class ProfilesSummary extends StatelessWidget {
  const ProfilesSummary({required this.controller, super.key});

  final AppController controller;

  @override
  Widget build(BuildContext context) {
    final app = controller;
    final strings = app.strings;
    final theme = Theme.of(context);
    final running = profilesRunningLabel(app);
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
          LucideIcons.user,
          strings.get('scenario_current_account'),
          profilesSelectedAccountLabel(app),
          key: const ValueKey('profiles-selected-account'),
        ),
        if (running != null)
          item(
            LucideIcons.activity,
            strings.chain('current'),
            running,
            key: const ValueKey('profiles-running'),
          ),
      ],
    );
    // Refresh stays beside the facts at every width; it is small, and an
    // uncertain switch is resolved by refreshing before anything else.
    return Row(
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
