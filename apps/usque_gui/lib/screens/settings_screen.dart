import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';
import 'package:url_launcher/url_launcher.dart';

import '../core/usque_theme.dart';
import '../models/app_models.dart';
import '../state/app_controller.dart';
import '../widgets/common.dart';
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
      subtitle: strings.get('settings_subtitle'),
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
          PanelStack(
            children: <Widget>[
              SectionPanel(
                icon: LucideIcons.paintbrush,
                title: strings.get('appearance'),
                gap: 20,
                children: <Widget>[
                  _SettingRow(
                    icon: LucideIcons.sunMoon,
                    title: strings.get('theme'),
                    control: _Picker<ThemePreference>(
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
                  const _RowDivider(),
                  _SettingRow(
                    icon: LucideIcons.languages,
                    title: strings.get('language'),
                    control: _Picker<LocalePreference>(
                      value: controller.localePreference,
                      values: LocalePreference.pickerOrder,
                      onChanged: controller.setLocale,
                      labelOf: (value) => strings.get(value.languageLabelKey),
                    ),
                  ),
                ],
              ),
              _NetworkOutputsPanel(controller: controller),
              _GeoDirectCard(controller: controller),
              SectionPanel(
                icon: LucideIcons.monitorCog,
                title: strings.get('system_integration'),
                gap: 10,
                children: <Widget>[
                  SwitchListTile(
                    contentPadding: EdgeInsets.zero,
                    secondary: const Icon(LucideIcons.power),
                    title: Text(strings.get('start_on_boot')),
                    subtitle: android
                        ? Text(strings.get('start_on_boot_android'))
                        : null,
                    value: controller.startOnBoot,
                    onChanged: controller.setStartOnBoot,
                  ),
                  if (windows) ...<Widget>[
                    SwitchListTile(
                      contentPadding: EdgeInsets.zero,
                      secondary: const Icon(LucideIcons.panelTopClose),
                      title: Text(strings.get('close_to_tray')),
                      value: controller.closeToTray,
                      onChanged: controller.setCloseToTray,
                    ),
                    SwitchListTile(
                      contentPadding: EdgeInsets.zero,
                      secondary: const Icon(LucideIcons.link),
                      title: Text(
                        strings.get('zero_trust_protocol_association'),
                      ),
                      subtitle: Text(
                        strings.get('zero_trust_protocol_association_help'),
                      ),
                      value: controller.warpProtocolAssociation,
                      onChanged: controller.setWarpProtocolAssociation,
                    ),
                  ],
                  if (android) ...<Widget>[
                    ListTile(
                      contentPadding: EdgeInsets.zero,
                      leading: const Icon(LucideIcons.panelTop),
                      title: Text(strings.get('add_quick_settings_tile')),
                      subtitle: Text(
                        strings.get('add_quick_settings_tile_help'),
                      ),
                      trailing: const Icon(
                        LucideIcons.chevronRightDir,
                        size: 18,
                      ),
                      onTap: controller.requestAddQuickSettingsTile,
                    ),
                    ListTile(
                      contentPadding: EdgeInsets.zero,
                      leading: const Icon(LucideIcons.shield),
                      title: Text(strings.get('always_on_vpn')),
                      subtitle: Text(strings.get('always_on_vpn_help')),
                      trailing: const Icon(
                        LucideIcons.chevronRightDir,
                        size: 18,
                      ),
                      onTap: controller.openAlwaysOnVpnSettings,
                    ),
                    ListTile(
                      contentPadding: EdgeInsets.zero,
                      leading: const Icon(LucideIcons.layers3),
                      title: Text(strings.get('per_app_proxy')),
                      subtitle: Text(
                        controller.perAppProxy.enabled
                            ? strings
                                  .get('per_app_proxy_on')
                                  .replaceAll(
                                    '{count}',
                                    '${controller.perAppProxy.packageNames.length}',
                                  )
                            : strings.get('per_app_proxy_off'),
                      ),
                      trailing: const Icon(
                        LucideIcons.chevronRightDir,
                        size: 18,
                      ),
                      onTap: () => Navigator.of(context).push(
                        MaterialPageRoute<void>(
                          builder: (_) =>
                              PerAppProxyScreen(controller: controller),
                        ),
                      ),
                    ),
                  ],
                ],
              ),
              SectionPanel(
                icon: LucideIcons.refreshCw,
                title: strings.get('updates'),
                gap: 10,
                children: <Widget>[
                  SwitchListTile(
                    contentPadding: EdgeInsets.zero,
                    secondary: const Icon(LucideIcons.bell),
                    title: Text(strings.get('check_updates')),
                    subtitle: Text(strings.get('update_startup_description')),
                    value: controller.updateChecksEnabled,
                    onChanged: controller.setUpdateChecks,
                  ),
                  const SizedBox(height: 6),
                  _UpdateActions(controller: controller),
                ],
              ),
              if (controller.engineCapabilities?.networkQuality ?? false)
                _NetworkQualityCard(controller: controller),
              _DiagnosticsCard(controller: controller),
              _AdvancedCard(controller: controller),
            ],
          ),
        ],
      ),
    );
  }
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
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: <Widget>[
        if (update?.available == true) ...<Widget>[
          Row(
            children: <Widget>[
              Icon(
                LucideIcons.packageCheck,
                size: 18,
                color: Theme.of(context).colorScheme.primary,
              ),
              const SizedBox(width: 10),
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
            title: strings.get('error'),
            message: message,
            danger: true,
          ),
          const SizedBox(height: 12),
        ],
        Wrap(
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
                onPressed: () => launchUrl(
                  Uri.parse(update.releaseUrl!),
                  mode: LaunchMode.externalApplication,
                ),
                icon: const Icon(LucideIcons.externalLink),
                label: Text(strings.get('open_release')),
              ),
          ],
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

class _NetworkOutputsPanel extends StatelessWidget {
  const _NetworkOutputsPanel({required this.controller});

  final AppController controller;

  @override
  Widget build(BuildContext context) {
    final strings = controller.strings;
    final profile = controller.activeProfile;
    final frontends = profile.frontends;
    final bool windows = defaultTargetPlatform == TargetPlatform.windows;
    return SectionPanel(
      icon: LucideIcons.share2,
      title: strings.get('outputs'),
      gap: 10,
      children: <Widget>[
        SwitchListTile(
          contentPadding: EdgeInsets.zero,
          secondary: const Icon(LucideIcons.shield),
          title: Text(strings.tunnelOutputLabel(defaultTargetPlatform)),
          value: frontends.tunnel,
          onChanged: (value) => controller.updateNetwork(
            profile.copyWith(frontends: frontends.copyWith(tunnel: value)),
          ),
        ),
        SwitchListTile(
          contentPadding: EdgeInsets.zero,
          secondary: const Icon(LucideIcons.network),
          title: const Text('SOCKS5'),
          value: frontends.socks5,
          onChanged: (value) => controller.updateNetwork(
            profile.copyWith(frontends: frontends.copyWith(socks5: value)),
          ),
        ),
        SwitchListTile(
          contentPadding: EdgeInsets.zero,
          secondary: const Icon(LucideIcons.globe2),
          title: const Text('HTTP'),
          value: frontends.http,
          onChanged: (value) => controller.updateNetwork(
            profile.copyWith(frontends: frontends.copyWith(http: value)),
          ),
        ),
        if (windows)
          SwitchListTile(
            contentPadding: EdgeInsets.zero,
            secondary: const Icon(LucideIcons.link),
            title: Text(strings.get('system_proxy')),
            value: profile.proxy.systemProxy,
            onChanged: frontends.http
                ? (value) => controller.updateNetwork(
                    profile.copyWith(
                      proxy: profile.proxy.copyWith(systemProxy: value),
                    ),
                  )
                : null,
          ),
        SwitchListTile(
          contentPadding: EdgeInsets.zero,
          secondary: const Icon(LucideIcons.zap),
          title: Text(strings.get('auto_connect')),
          value: profile.autoConnect,
          onChanged: (value) =>
              controller.updateNetwork(profile.copyWith(autoConnect: value)),
        ),
        if (!frontends.any) ...<Widget>[
          const SizedBox(height: 8),
          WarningBanner(
            title: strings.get('channel_only'),
            message: strings.get('channel_only_warning'),
          ),
        ],
      ],
    );
  }
}

class _GeoDirectCard extends StatelessWidget {
  const _GeoDirectCard({required this.controller});

  final AppController controller;

  @override
  Widget build(BuildContext context) {
    final enabled = controller.activeProfile.geoDirectCountries;
    final preview = enabled.take(4).join(' · ');
    final remaining = enabled.length - 4;
    return Panel(
      onTap: () => Navigator.of(context).push(
        MaterialPageRoute<void>(
          builder: (_) => GeoDirectSettingsScreen(controller: controller),
        ),
      ),
      child: SectionTitle(
        icon: LucideIcons.route,
        title: controller.strings.get('geo_direct'),
        subtitle: enabled.isEmpty
            ? null
            : remaining > 0
            ? '$preview · +$remaining'
            : preview,
        trailing: Semantics(
          label: '${controller.strings.get('geo_direct')}: ${enabled.length}',
          child: Row(
            mainAxisSize: MainAxisSize.min,
            children: <Widget>[
              Text(
                '${enabled.length}',
                style: Theme.of(context).textTheme.labelLarge?.copyWith(
                  color: Theme.of(context).colorScheme.onSurfaceVariant,
                ),
              ),
              const SizedBox(width: 10),
              Icon(
                LucideIcons.chevronRightDir,
                size: 20,
                color: Theme.of(context).colorScheme.onSurfaceVariant,
              ),
            ],
          ),
        ),
      ),
    );
  }
}

class _NetworkQualityCard extends StatelessWidget {
  const _NetworkQualityCard({required this.controller});

  final AppController controller;

  @override
  Widget build(BuildContext context) {
    final strings = controller.strings;
    return Panel(
      onTap: () => Navigator.of(context).push(
        MaterialPageRoute<void>(
          builder: (_) => NetworkQualityScreen(controller: controller),
        ),
      ),
      child: SectionTitle(
        icon: LucideIcons.gauge,
        title: strings.get('network_quality'),
        subtitle: strings.get('nq_subtitle'),
        trailing: Icon(
          LucideIcons.chevronRightDir,
          size: 20,
          color: Theme.of(context).colorScheme.onSurfaceVariant,
        ),
      ),
    );
  }
}

class _DiagnosticsCard extends StatelessWidget {
  const _DiagnosticsCard({required this.controller});

  final AppController controller;

  @override
  Widget build(BuildContext context) {
    final strings = controller.strings;
    return Panel(
      onTap: () => Navigator.of(context).push(
        MaterialPageRoute<void>(
          builder: (_) => DiagnosticsScreen(controller: controller),
        ),
      ),
      child: SectionTitle(
        icon: LucideIcons.activity,
        title: strings.get('diagnostics'),
        subtitle: strings.get('diagnostics_subtitle'),
        trailing: Icon(
          LucideIcons.chevronRightDir,
          size: 20,
          color: Theme.of(context).colorScheme.onSurfaceVariant,
        ),
      ),
    );
  }
}

/// The one door out of Settings, so the whole plate is the target.
class _AdvancedCard extends StatelessWidget {
  const _AdvancedCard({required this.controller});

  final AppController controller;

  @override
  Widget build(BuildContext context) {
    final strings = controller.strings;
    return Panel(
      onTap: () => Navigator.of(context).push(
        MaterialPageRoute<void>(
          builder: (_) => AdvancedSettingsScreen(controller: controller),
        ),
      ),
      child: SectionTitle(
        icon: LucideIcons.slidersHorizontal,
        title: strings.get('advanced'),
        subtitle: strings.get('advanced_subtitle'),
        trailing: Icon(
          LucideIcons.chevronRightDir,
          size: 20,
          color: Theme.of(context).colorScheme.onSurfaceVariant,
        ),
      ),
    );
  }
}

class _RowDivider extends StatelessWidget {
  const _RowDivider();

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 14),
      child: Divider(height: 1, color: UsqueTokens.of(context).hairline),
    );
  }
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
    Widget label() => Row(
      children: <Widget>[
        SizedBox(
          width: 22,
          child: Icon(
            icon,
            size: 18,
            color: Theme.of(context).colorScheme.onSurfaceVariant,
          ),
        ),
        const SizedBox(width: 11),
        Expanded(child: Text(title)),
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
    required this.value,
    required this.values,
    required this.labelOf,
    required this.onChanged,
  });

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
    );
  }
}
