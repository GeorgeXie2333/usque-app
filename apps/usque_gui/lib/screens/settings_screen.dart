import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';
import 'package:url_launcher/url_launcher.dart';

import '../core/usque_theme.dart';
import '../models/app_models.dart';
import '../state/app_controller.dart';
import '../widgets/common.dart';
import 'advanced_settings_screen.dart';
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
                      values: LocalePreference.values,
                      onChanged: controller.setLocale,
                      labelOf: (value) => strings.get(switch (value) {
                        LocalePreference.system => 'language_system',
                        LocalePreference.english => 'language_en',
                        LocalePreference.simplifiedChinese => 'language_zh',
                      }),
                    ),
                  ),
                ],
              ),
              _NetworkOutputsPanel(controller: controller),
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
                      trailing: const Icon(LucideIcons.chevronRight, size: 18),
                      onTap: controller.requestAddQuickSettingsTile,
                    ),
                    ListTile(
                      contentPadding: EdgeInsets.zero,
                      leading: const Icon(LucideIcons.shield),
                      title: Text(strings.get('always_on_vpn')),
                      subtitle: Text(strings.get('always_on_vpn_help')),
                      trailing: const Icon(LucideIcons.chevronRight, size: 18),
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
                      trailing: const Icon(LucideIcons.chevronRight, size: 18),
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
                    value: controller.updateChecksEnabled,
                    onChanged: controller.setUpdateChecks,
                  ),
                  const SizedBox(height: 6),
                  _UpdateActions(controller: controller),
                ],
              ),
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
    final bool offerRelease =
        update != null &&
        update.available &&
        (update.releaseUrl?.isNotEmpty ?? false);
    return Wrap(
      spacing: 8,
      runSpacing: 8,
      alignment: WrapAlignment.end,
      children: <Widget>[
        OutlinedButton.icon(
          onPressed: controller.busy ? null : controller.checkForUpdates,
          icon: const Icon(LucideIcons.refreshCw),
          label: Text(strings.get('check_now')),
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
    );
  }
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
          title: Text(strings.get('tunnel_output')),
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
          LucideIcons.chevronRight,
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
    return Row(
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
        const SizedBox(width: 12),
        control,
      ],
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
    return DecoratedBox(
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
                    child: Text(labelOf(item)),
                  ),
                )
                .toList(growable: false),
          ),
        ),
      ),
    );
  }
}
