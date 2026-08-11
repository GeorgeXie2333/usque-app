import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';
import 'package:url_launcher/url_launcher.dart';

import '../models/app_models.dart';
import '../state/app_controller.dart';
import '../widgets/common.dart';
import 'advanced_settings_screen.dart';

class SettingsScreen extends StatelessWidget {
  const SettingsScreen({required this.controller, super.key});

  final AppController controller;

  @override
  Widget build(BuildContext context) {
    final strings = controller.strings;
    return PageFrame(
      title: strings.get('settings'),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: <Widget>[
          if (controller.lastError != null) ...<Widget>[
            WarningBanner(
              title: strings.get('error'),
              message: controller.lastError!,
              danger: true,
              onDismiss: controller.clearError,
            ),
            const SizedBox(height: 16),
          ],
          if (controller.lastNotice != null) ...<Widget>[
            WarningBanner(
              title: strings.get('notice'),
              message: controller.lastNotice!,
              onDismiss: controller.clearNotice,
            ),
            const SizedBox(height: 16),
          ],
          Panel(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: <Widget>[
                SectionTitle(
                  icon: LucideIcons.paintbrush,
                  title: strings.get('appearance'),
                ),
                const SizedBox(height: 22),
                _SettingRow(
                  icon: LucideIcons.sunMoon,
                  title: strings.get('theme'),
                  control: DropdownButton<ThemePreference>(
                    value: controller.themePreference,
                    underline: const SizedBox.shrink(),
                    borderRadius: BorderRadius.circular(14),
                    onChanged: (value) {
                      if (value != null) {
                        controller.setTheme(value);
                      }
                    },
                    items: ThemePreference.values
                        .map(
                          (value) => DropdownMenuItem<ThemePreference>(
                            value: value,
                            child: Text(
                              strings.get(switch (value) {
                                ThemePreference.system => 'theme_system',
                                ThemePreference.light => 'theme_light',
                                ThemePreference.dark => 'theme_dark',
                              }),
                            ),
                          ),
                        )
                        .toList(growable: false),
                  ),
                ),
                const Divider(height: 28),
                _SettingRow(
                  icon: LucideIcons.languages,
                  title: strings.get('language'),
                  control: DropdownButton<LocalePreference>(
                    value: controller.localePreference,
                    underline: const SizedBox.shrink(),
                    borderRadius: BorderRadius.circular(14),
                    onChanged: (value) {
                      if (value != null) {
                        controller.setLocale(value);
                      }
                    },
                    items: LocalePreference.values
                        .map(
                          (value) => DropdownMenuItem<LocalePreference>(
                            value: value,
                            child: Text(
                              strings.get(switch (value) {
                                LocalePreference.system => 'language_system',
                                LocalePreference.english => 'language_en',
                                LocalePreference.simplifiedChinese =>
                                  'language_zh',
                              }),
                            ),
                          ),
                        )
                        .toList(growable: false),
                  ),
                ),
              ],
            ),
          ),
          if (defaultTargetPlatform != TargetPlatform.android) ...<Widget>[
            const SizedBox(height: 16),
            Panel(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: <Widget>[
                  SectionTitle(
                    icon: LucideIcons.monitorCog,
                    title: strings.get('system_integration'),
                  ),
                  const SizedBox(height: 12),
                  SwitchListTile(
                    contentPadding: EdgeInsets.zero,
                    secondary: const Icon(LucideIcons.power),
                    title: Text(strings.get('start_on_boot')),
                    value: controller.startOnBoot,
                    onChanged: controller.setStartOnBoot,
                  ),
                  if (defaultTargetPlatform == TargetPlatform.windows)
                    SwitchListTile(
                      contentPadding: EdgeInsets.zero,
                      secondary: const Icon(LucideIcons.panelTopClose),
                      title: Text(strings.get('close_to_tray')),
                      value: controller.closeToTray,
                      onChanged: controller.setCloseToTray,
                    ),
                ],
              ),
            ),
          ],
          const SizedBox(height: 16),
          Panel(
            child: Column(
              children: <Widget>[
                SectionTitle(
                  icon: LucideIcons.refreshCw,
                  title: strings.get('updates'),
                ),
                const SizedBox(height: 12),
                SwitchListTile(
                  contentPadding: EdgeInsets.zero,
                  secondary: const Icon(LucideIcons.bell),
                  title: Text(strings.get('check_updates')),
                  value: controller.updateChecksEnabled,
                  onChanged: controller.setUpdateChecks,
                ),
                Align(
                  alignment: Alignment.centerRight,
                  child: OutlinedButton.icon(
                    onPressed: controller.busy
                        ? null
                        : controller.checkForUpdates,
                    icon: const Icon(LucideIcons.refreshCw),
                    label: Text(strings.get('check_now')),
                  ),
                ),
                if (controller.updateResult case final update?
                    when update.available &&
                        update.releaseUrl != null &&
                        update.releaseUrl!.isNotEmpty) ...<Widget>[
                  const SizedBox(height: 12),
                  Align(
                    alignment: Alignment.centerRight,
                    child: FilledButton.tonalIcon(
                      onPressed: () => launchUrl(
                        Uri.parse(update.releaseUrl!),
                        mode: LaunchMode.externalApplication,
                      ),
                      icon: const Icon(LucideIcons.externalLink),
                      label: Text(strings.get('open_release')),
                    ),
                  ),
                ],
              ],
            ),
          ),
          Panel(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: <Widget>[
                SectionTitle(
                  icon: LucideIcons.slidersHorizontal,
                  title: strings.get('advanced'),
                ),
                const SizedBox(height: 12),
                Align(
                  alignment: Alignment.centerRight,
                  child: FilledButton.tonalIcon(
                    onPressed: () => Navigator.of(context).push(
                      MaterialPageRoute<void>(
                        builder: (_) =>
                            AdvancedSettingsScreen(controller: controller),
                      ),
                    ),
                    icon: const Icon(LucideIcons.chevronRight),
                    label: Text(strings.get('open_advanced')),
                  ),
                ),
              ],
            ),
          ),
        ],
      ),
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
        Icon(
          icon,
          size: 20,
          color: Theme.of(context).colorScheme.onSurfaceVariant,
        ),
        const SizedBox(width: 13),
        Expanded(child: Text(title)),
        const SizedBox(width: 12),
        control,
      ],
    );
  }
}
