import 'package:flutter/material.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../state/app_controller.dart';
import 'common.dart';

/// Whether an unconfirmed read may have replaced the saved account choice.
bool profilesSelectionUncertain(AppController app) {
  final accountId =
      app.scenarios.catalog?.currentAccountId ?? app.activeProfileId;
  return app.scenarios.queryUnconfirmed && accountId != app.activeProfileId;
}

/// The saved account choice, or unknown while an unconfirmed read may have
/// replaced it.
String profilesSelectedAccountLabel(AppController app) {
  final strings = app.strings, state = app.scenarios.catalog;
  final accountId = state?.currentAccountId ?? app.activeProfileId;
  if (profilesSelectionUncertain(app)) {
    return strings.get('scenario_unknown');
  }
  return app.profiles
          .where((a) => a.id == accountId)
          .map(strings.accountName)
          .firstOrNull ??
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
  return '${account == null ? strings.get('not_available') : strings.accountName(account)} · $scene';
}

/// The account a live session is confirmed to use, or null. Without a
/// connection or a runtime-confirmed catalogue nothing is claimed.
String? profilesConnectedAccountId(AppController app) {
  if (!app.snapshot.isConnected || !app.scenarios.runtimeConfirmed) {
    return null;
  }
  return app.scenarios.catalog?.appliedAccountId;
}

/// Engine and switch problems, above the content they concern. Pages without
/// their own refresh control set [refreshable], so an uncertain or failed
/// scene result offers the refresh that resolves it.
Widget? profilesOperationBanner(AppController app, {bool refreshable = false}) {
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
        action: refreshable
            ? TextButton.icon(
                key: const ValueKey('profiles-banner-refresh'),
                onPressed: scenes.busy ? null : scenes.refresh,
                icon: const Icon(LucideIcons.refreshCw, size: 18),
                label: Text(strings.get('settings_refresh')),
              )
            : null,
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
