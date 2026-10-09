import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../core/usque_theme.dart';
import '../models/app_models.dart';
import '../models/network_settings.dart';
import '../state/app_controller.dart';
import '../widgets/common.dart';
import '../widgets/external_link.dart';
import '../widgets/usque_dialog.dart';
import 'advanced_settings_screen.dart';
import 'diagnostics_screen.dart';
import 'geo_direct_settings_screen.dart';
import 'network_quality_screen.dart';
import 'per_app_proxy_screen.dart';

class SettingsScreen extends StatelessWidget {
  const SettingsScreen({required this.controller, super.key});

  final AppController controller;

  @override
  Widget build(BuildContext context) {
    final strings = controller.strings;
    final bool android = defaultTargetPlatform == TargetPlatform.android;
    final bool windows = defaultTargetPlatform == TargetPlatform.windows;
    return PageFrame(
      title: strings.get('settings'),
      contentWidth: 880,
      subtitle: strings.get('settings_subtitle'),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: <Widget>[
          BannerSlot(
            child: controller.lastError == null
                ? null
                : WarningBanner(
                    title: strings.get('error_generic'),
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
          BannerSlot(child: _networkSettingsProblem(controller)),
          RowTileTheme(
            child: PanelStack(
              spacing: 32,
              children: <Widget>[
                _SettingsGroup(
                  title: strings.get('connection_protection_group'),
                  children: [
                    ContentSection(
                      padding: _tilePadding,
                      child: SwitchListTile(
                        contentPadding: EdgeInsets.zero,
                        secondary: const Icon(LucideIcons.zap),
                        title: Text(strings.get('auto_connect')),
                        value: controller.activeProfile.autoConnect,
                        onChanged: (value) => controller.updateNetwork(
                          controller.activeProfile.copyWith(autoConnect: value),
                          changedFields: const ['auto_connect'],
                        ),
                      ),
                    ),
                    LinkRow(
                      key: const ValueKey('settings-kill-switch-row'),
                      icon: LucideIcons.shieldCheck,
                      title: strings.get('kill_switch'),
                      subtitle: strings.get('kill_switch_help'),
                      value: strings.get(
                        _killSwitchSettingKey(controller.activeProfile),
                      ),
                      valueKey: const ValueKey('settings-kill-switch-value'),
                      onTap: () => _open(
                        context,
                        AdvancedSettingsScreen(
                          controller: controller,
                          revealKillSwitch: true,
                        ),
                      ),
                    ),
                    if (android)
                      LinkRow(
                        icon: LucideIcons.shield,
                        title: strings.get('always_on_vpn'),
                        subtitle: strings.get('always_on_vpn_help'),
                        onTap: controller.openAlwaysOnVpnSettings,
                      ),
                    LinkRow(
                      icon: LucideIcons.slidersHorizontal,
                      title: strings.get('advanced'),
                      subtitle: strings.get('advanced_subtitle'),
                      onTap: () => _open(
                        context,
                        AdvancedSettingsScreen(controller: controller),
                      ),
                    ),
                  ],
                ),
                _SettingsGroup(
                  title: strings.get('proxy_routing_group'),
                  children: [
                    LinkRow(
                      icon: LucideIcons.route,
                      title: strings.get('geo_direct'),
                      subtitle: _bypassSummary(controller),
                      onTap: () => _open(
                        context,
                        GeoDirectSettingsScreen(controller: controller),
                      ),
                    ),
                    if (android)
                      LinkRow(
                        icon: LucideIcons.layers3,
                        title: strings.get('per_app_proxy'),
                        subtitle: controller.perAppProxy.enabled
                            ? strings
                                  .get('per_app_proxy_on')
                                  .replaceAll(
                                    '{count}',
                                    '${controller.perAppProxy.packageNames.length}',
                                  )
                            : strings.get('per_app_proxy_off'),
                        onTap: () => _open(
                          context,
                          PerAppProxyScreen(controller: controller),
                        ),
                      ),
                  ],
                ),
                _SettingsGroup(
                  title: strings.get('tools_group'),
                  children: [
                    if (controller.engineCapabilities?.networkQuality ?? false)
                      LinkRow(
                        icon: LucideIcons.gauge,
                        title: strings.get('network_quality'),
                        subtitle: strings.get('nq_subtitle'),
                        onTap: () => _open(
                          context,
                          NetworkQualityScreen(controller: controller),
                        ),
                      ),
                    LinkRow(
                      icon: LucideIcons.activity,
                      title: strings.get('diagnostics'),
                      subtitle: strings.get('diagnostics_subtitle'),
                      onTap: () => _open(
                        context,
                        DiagnosticsScreen(controller: controller),
                      ),
                    ),
                  ],
                ),
                _SettingsGroup(
                  title: strings.get('application_group'),
                  children: [
                    ContentSection(
                      padding: _rowPadding,
                      child: _SettingRow(
                        icon: LucideIcons.sunMoon,
                        title: strings.get('theme'),
                        control: _Picker<ThemePreference>(
                          semanticLabel: strings.get('theme'),
                          value: controller.themePreference,
                          values: ThemePreference.values,
                          onChanged: controller.setTheme,
                          labelOf: (value) => strings.get(switch (value) {
                            ThemePreference.system => 'theme_system',
                            ThemePreference.light => 'theme_light',
                            ThemePreference.dark => 'theme_dark',
                          }),
                        ),
                      ),
                    ),
                    ContentSection(
                      padding: _rowPadding,
                      child: _SettingRow(
                        icon: LucideIcons.languages,
                        title: strings.get('language'),
                        control: _Picker<LocalePreference>(
                          semanticLabel: strings.get('language'),
                          value: controller.localePreference,
                          values: LocalePreference.pickerOrder,
                          onChanged: controller.setLocale,
                          labelOf: (value) =>
                              strings.get(value.languageLabelKey),
                        ),
                      ),
                    ),
                    ContentSection(
                      padding: _tilePadding,
                      child: SwitchListTile(
                        contentPadding: EdgeInsets.zero,
                        secondary: const Icon(LucideIcons.power),
                        title: Text(strings.get('start_on_boot')),
                        subtitle: android
                            ? Text(strings.get('start_on_boot_android'))
                            : null,
                        value: controller.startOnBoot,
                        onChanged: controller.setStartOnBoot,
                      ),
                    ),
                    if (windows)
                      ContentSection(
                        padding: _tilePadding,
                        child: SwitchListTile(
                          contentPadding: EdgeInsets.zero,
                          secondary: const Icon(LucideIcons.panelTopClose),
                          title: Text(strings.get('close_to_tray')),
                          value: controller.closeToTray,
                          onChanged: controller.setCloseToTray,
                        ),
                      ),
                    if (android)
                      LinkRow(
                        icon: LucideIcons.panelTop,
                        title: strings.get('add_quick_settings_tile'),
                        subtitle: strings.get('add_quick_settings_tile_help'),
                        onTap: controller.requestAddQuickSettingsTile,
                      ),
                    ContentSection(
                      padding: _tilePadding.copyWith(bottom: 12),
                      children: <Widget>[
                        SwitchListTile(
                          contentPadding: EdgeInsets.zero,
                          secondary: const Icon(LucideIcons.bell),
                          title: Text(strings.get('check_updates')),
                          subtitle: Text(
                            strings.get('update_startup_description'),
                          ),
                          value: controller.updateChecksEnabled,
                          onChanged: controller.setUpdateChecks,
                        ),
                        const SizedBox(height: 6),
                        _UpdateActions(controller: controller),
                      ],
                    ),
                  ],
                ),
              ],
            ),
          ),
        ],
      ),
    );
  }
}

void _open(BuildContext context, Widget page) =>
    Navigator.of(context).push(MaterialPageRoute<void>(builder: (_) => page));

String _bypassSummary(AppController controller) {
  final profile = controller.activeProfile;
  return controller.strings
      .get('bypass_summary')
      .replaceAll('{countries}', '${profile.geoDirectCountries.length}')
      .replaceAll(
        '{targets}',
        '${profile.routing.rules.length + profile.bypassCidrs.length + profile.bypassDomains.length}',
      );
}

/// Settings has no apply bar of its own, so only a network-settings problem
/// that still needs attention is surfaced here. Success results stay with the
/// page that made the change.
Widget? _networkSettingsProblem(AppController controller) {
  final settings = controller.networkSettings;
  final status = settings.state?.status;
  final failed =
      settings.saveError != null || status == NetworkSettingsApplyStatus.failed;
  final message = controller.networkSettingsMessage;
  if (message == null ||
      !(failed ||
          settings.unconfirmed ||
          status == NetworkSettingsApplyStatus.unknown)) {
    return null;
  }
  return Column(
    crossAxisAlignment: CrossAxisAlignment.stretch,
    children: <Widget>[
      Semantics(
        liveRegion: true,
        child: WarningBanner(title: null, message: message, danger: failed),
      ),
      if (controller.networkSettingsCanReconnect) ...<Widget>[
        const SizedBox(height: 8),
        Align(
          alignment: AlignmentDirectional.centerEnd,
          child: OutlinedButton(
            onPressed: controller.retry,
            child: Text(controller.strings.get('settings_reconnect')),
          ),
        ),
      ],
    ],
  );
}

/// Settings rows are denser than general content. Sections holding a single
/// Material list tile rely on the tile's own height for the touch target.
const EdgeInsets _rowPadding = EdgeInsets.symmetric(
  horizontal: 8,
  vertical: 12,
);
const EdgeInsets _tilePadding = EdgeInsets.symmetric(
  horizontal: 8,
  vertical: 4,
);

class _SettingsGroup extends StatelessWidget {
  const _SettingsGroup({required this.title, required this.children});
  final String title;
  final List<Widget> children;
  @override
  Widget build(BuildContext context) => Column(
    crossAxisAlignment: CrossAxisAlignment.stretch,
    children: [
      // A quiet label, so groups never compete with the page title.
      Padding(
        padding: const EdgeInsetsDirectional.only(start: 8),
        child: Semantics(
          header: true,
          child: Text(
            title,
            style: Theme.of(context).textTheme.titleSmall?.copyWith(
              color: Theme.of(context).colorScheme.onSurfaceVariant,
            ),
          ),
        ),
      ),
      const SizedBox(height: 4),
      ContentList(children: children),
    ],
  );
}

class _UpdateActions extends StatelessWidget {
  const _UpdateActions({required this.controller});

  final AppController controller;

  @override
  Widget build(BuildContext context) {
    final strings = controller.strings;
    final UpdateCheckResult? update = controller.updateResult;
    final UpdatePackage? package = update?.package;
    final bool offerRelease =
        update != null &&
        update.available &&
        (update.releaseUrl?.isNotEmpty ?? false);
    final bool canDownload =
        update?.available == true &&
        package != null &&
        (controller.updatePhase == UpdateOperationPhase.available ||
            controller.updatePhase == UpdateOperationPhase.failed);
    final bool canInstall =
        controller.updatePhase == UpdateOperationPhase.ready &&
        controller.downloadedUpdatePath != null;
    final String? statusKey = switch (controller.updatePhase) {
      UpdateOperationPhase.checking => 'update_checking',
      UpdateOperationPhase.downloading => 'update_downloading',
      UpdateOperationPhase.verifying => 'update_verifying',
      UpdateOperationPhase.ready => 'update_ready',
      UpdateOperationPhase.installing => 'update_installing',
      _ => null,
    };
    final bool showProgress =
        controller.updatePhase == UpdateOperationPhase.downloading ||
        controller.updatePhase == UpdateOperationPhase.verifying;
    final Widget actions = Wrap(
      spacing: 8,
      runSpacing: 8,
      alignment: WrapAlignment.end,
      children: <Widget>[
        OutlinedButton.icon(
          onPressed: controller.busy || controller.updateOperationActive
              ? null
              : controller.checkForUpdates,
          icon: const Icon(LucideIcons.refreshCw),
          label: Text(strings.get('check_now')),
        ),
        if (canDownload)
          FilledButton.icon(
            onPressed: controller.downloadUpdate,
            icon: const Icon(LucideIcons.download),
            label: Text(
              controller.updatePhase == UpdateOperationPhase.failed
                  ? strings.get('retry')
                  : strings.get('download'),
            ),
          ),
        if (controller.updatePhase == UpdateOperationPhase.downloading)
          OutlinedButton.icon(
            onPressed: controller.cancelUpdateDownload,
            icon: const Icon(LucideIcons.x),
            label: Text(strings.get('cancel')),
          ),
        if (canInstall)
          FilledButton.icon(
            onPressed: () => _confirmInstall(context),
            icon: const Icon(LucideIcons.rotateCw),
            label: Text(
              package?.platform == 'android'
                  ? strings.get('update_install_android')
                  : strings.get('update_restart_install'),
            ),
          ),
        if (offerRelease)
          FilledButton.tonalIcon(
            onPressed: () =>
                openExternalLink(context, strings, update.releaseUrl!),
            icon: const Icon(LucideIcons.externalLink),
            label: Text(strings.get('open_release')),
          ),
      ],
    );
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: <Widget>[
        if (update?.available == true) ...<Widget>[
          Row(
            children: <Widget>[
              Icon(
                LucideIcons.packageCheck,
                size: 20,
                color: Theme.of(context).colorScheme.primary,
              ),
              const SizedBox(width: 12),
              Expanded(
                child: Text(
                  <String>[
                    'v${(update?.version ?? '').replaceFirst(RegExp(r'^v'), '')}',
                    if (package != null) package.variant,
                    if (package != null) _formatUpdateBytes(package.size),
                  ].join('  •  '),
                  style: Theme.of(context).textTheme.titleSmall,
                ),
              ),
            ],
          ),
          if (package == null) ...<Widget>[
            const SizedBox(height: 10),
            WarningBanner(
              title: strings.get('updates'),
              message: strings.get('update_package_unavailable'),
            ),
          ],
          const SizedBox(height: 12),
        ],
        if (statusKey != null) ...<Widget>[
          Text(
            strings.get(statusKey),
            style: Theme.of(context).textTheme.bodySmall?.copyWith(
              color: Theme.of(context).colorScheme.onSurfaceVariant,
            ),
          ),
          const SizedBox(height: 8),
        ],
        if (showProgress) ...<Widget>[
          Semantics(
            label: strings.get(statusKey ?? 'update_downloading'),
            value: controller.updateProgress == null
                ? null
                : '${(controller.updateProgress! * 100).round()}%',
            child: LinearProgressIndicator(
              value: controller.updatePhase == UpdateOperationPhase.downloading
                  ? controller.updateProgress
                  : null,
            ),
          ),
          if (controller.updatePhase ==
              UpdateOperationPhase.downloading) ...<Widget>[
            const SizedBox(height: 6),
            Text(
              '${_formatUpdateBytes(controller.updateDownloadedBytes)} / '
              '${_formatUpdateBytes(controller.updateTotalBytes)}',
              textAlign: TextAlign.end,
              style: Theme.of(context).textTheme.labelSmall?.copyWith(
                color: Theme.of(context).colorScheme.onSurfaceVariant,
              ),
            ),
          ],
          const SizedBox(height: 12),
        ],
        if (controller.updateError case final message?) ...<Widget>[
          WarningBanner(
            title: strings.get('error_generic'),
            message: message,
            danger: true,
          ),
          const SizedBox(height: 12),
        ],
        LayoutBuilder(
          builder: (context, constraints) {
            // The installed version sits in the switch's text column, so the
            // update actions always read against what is running now.
            final version = Padding(
              padding: const EdgeInsetsDirectional.only(start: 32),
              child: Text(
                strings.get('app_version'),
                key: const ValueKey('settings-app-version'),
                style: Theme.of(context).textTheme.bodyMedium?.copyWith(
                  color: Theme.of(context).colorScheme.onSurfaceVariant,
                  fontFeatures: UsqueTheme.tabularFigures,
                ),
              ),
            );
            if (constraints.maxWidth < 280 ||
                MediaQuery.textScalerOf(context).scale(14) > 21) {
              return Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: <Widget>[version, const SizedBox(height: 8), actions],
              );
            }
            return Row(
              children: <Widget>[
                version,
                const SizedBox(width: 16),
                Expanded(child: actions),
              ],
            );
          },
        ),
      ],
    );
  }

  Future<void> _confirmInstall(BuildContext context) async {
    final strings = controller.strings;
    final package = controller.updateResult?.package;
    final confirmed = await showDialog<bool>(
      context: context,
      builder: (context) => UsqueDialog(
        icon: LucideIcons.refreshCw,
        title: strings.get('update_confirm_title'),
        width: 430,
        content: Text(strings.get('update_confirm_body')),
        actions: <Widget>[
          TextButton(
            onPressed: () => Navigator.pop(context, false),
            child: Text(strings.get('cancel')),
          ),
          FilledButton(
            onPressed: () => Navigator.pop(context, true),
            child: Text(
              package?.platform == 'android'
                  ? strings.get('update_install_android')
                  : strings.get('update_restart_install'),
            ),
          ),
        ],
      ),
    );
    if (confirmed ?? false) await controller.installDownloadedUpdate();
  }
}

String _formatUpdateBytes(int bytes) {
  if (bytes < 1024) return '$bytes B';
  if (bytes < 1024 * 1024) return '${(bytes / 1024).toStringAsFixed(1)} KiB';
  return '${(bytes / (1024 * 1024)).toStringAsFixed(1)} MiB';
}

/// Catalog key for the configured Kill Switch preference. Live enforcement
/// state belongs to Home; Settings reports only what will be applied.
String _killSwitchSettingKey(UsqueProfile profile) {
  if (!profile.frontends.tunnel) return 'not_used_proxy';
  return profile.killSwitch ? 'on' : 'off';
}

class _SettingRow extends StatelessWidget {
  const _SettingRow({
    required this.icon,
    required this.title,
    required this.control,
  });

  final IconData icon;
  final String title;
  final Widget control;

  @override
  Widget build(BuildContext context) {
    final textScale = MediaQuery.textScalerOf(context).scale(14) / 14;
    // The same icon size, gap and title style as the neighbouring list rows.
    Widget label() => Row(
      children: <Widget>[
        Icon(
          icon,
          size: 20,
          color: Theme.of(context).colorScheme.onSurfaceVariant,
        ),
        const SizedBox(width: 12),
        Expanded(
          child: Text(title, style: Theme.of(context).textTheme.titleMedium),
        ),
      ],
    );
    return LayoutBuilder(
      builder: (context, constraints) {
        if (constraints.maxWidth < 520 || textScale > 1.3) {
          return Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: <Widget>[label(), const SizedBox(height: 10), control],
          );
        }
        return Row(
          children: <Widget>[
            Expanded(child: label()),
            const SizedBox(width: 12),
            SizedBox(
              width: constraints.maxWidth < 680
                  ? constraints.maxWidth * 0.48
                  : 320,
              child: control,
            ),
          ],
        );
      },
    );
  }
}

/// Enum picker drawn as a bordered control rather than Material's underlined
/// dropdown, so it matches the text fields elsewhere in the app.
class _Picker<T> extends StatelessWidget {
  const _Picker({
    required this.semanticLabel,
    required this.value,
    required this.values,
    required this.labelOf,
    required this.onChanged,
  });

  final String semanticLabel;
  final T value;
  final List<T> values;
  final String Function(T value) labelOf;
  final ValueChanged<T> onChanged;

  @override
  Widget build(BuildContext context) {
    final ThemeData theme = Theme.of(context);
    final bool touchPlatform = theme.platform == TargetPlatform.android;
    return ConstrainedBox(
      constraints: BoxConstraints(minHeight: touchPlatform ? 48 : 0),
      child: DecoratedBox(
        decoration: BoxDecoration(
          color: theme.colorScheme.surfaceContainerLow,
          borderRadius: BorderRadius.circular(UsqueRadii.control),
          border: Border.all(color: UsqueTokens.of(context).hairline),
        ),
        child: Padding(
          padding: const EdgeInsets.symmetric(horizontal: 12),
          child: DropdownButtonHideUnderline(
            child: Semantics(
              label: semanticLabel,
              child: DropdownButton<T>(
                value: value,
                isExpanded: true,
                isDense: true,
                borderRadius: BorderRadius.circular(UsqueRadii.control),
                icon: const Padding(
                  padding: EdgeInsetsDirectional.only(start: 6),
                  child: Icon(LucideIcons.chevronDown, size: 16),
                ),
                style: theme.textTheme.bodyMedium?.copyWith(
                  color: theme.colorScheme.onSurface,
                ),
                padding: const EdgeInsets.symmetric(vertical: 11),
                onChanged: (next) {
                  if (next != null) {
                    onChanged(next);
                  }
                },
                items: values
                    .map(
                      (item) => DropdownMenuItem<T>(
                        value: item,
                        child: Text(
                          labelOf(item),
                          maxLines: 1,
                          overflow: TextOverflow.ellipsis,
                        ),
                      ),
                    )
                    .toList(growable: false),
              ),
            ),
          ),
        ),
      ),
    );
  }
}
