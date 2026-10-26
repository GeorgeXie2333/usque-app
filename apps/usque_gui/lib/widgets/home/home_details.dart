import 'package:flutter/material.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../../core/app_strings.dart';
import '../../core/usque_theme.dart';
import '../../models/app_models.dart';
import '../../state/app_controller.dart';
import '../common.dart';
import '../controller_selector.dart';

/// Catalog key for the Home Kill Switch value. Driven by the profile flag
/// and live engine state, not "the tunnel frontend is enabled".
String killSwitchStatusKey({
  required UsqueProfile profile,
  required EngineSnapshot snapshot,
}) {
  if (!profile.frontends.tunnel) {
    return 'not_used_proxy';
  }
  if (!profile.killSwitch) {
    return 'off';
  }
  switch (snapshot.killSwitchState) {
    case 'active':
      return 'ks_active';
    case 'error':
      return 'ks_error';
    default:
      if (snapshot.isTransitional) return 'ks_engaging';
      // While idle the enabled switch is armed, not failing; "Inactive" is
      // kept for a session that is up or failed without protection.
      return snapshot.phase == ConnectionPhase.disconnected
          ? 'ks_standby'
          : 'ks_inactive';
  }
}

/// Shield glyph and tone for a [killSwitchStatusKey] value. The label always
/// carries the meaning; colour only supports it.
(IconData, StatusTone) killSwitchPresentation(String key) => switch (key) {
  'ks_active' => (LucideIcons.shieldCheck, StatusTone.success),
  'ks_error' => (LucideIcons.shieldAlert, StatusTone.danger),
  'ks_engaging' => (LucideIcons.shield, StatusTone.brand),
  _ => (LucideIcons.shield, StatusTone.neutral),
};

/// Exit addresses and configured interfaces, folded under one row.
class HomeDetails extends StatelessWidget {
  const HomeDetails({
    required this.controller,
    required this.strings,
    super.key,
  });
  final AppController controller;
  final AppStrings strings;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return Material(
      color: Colors.transparent,
      child: ListTileTheme.merge(
        minLeadingWidth: 20,
        horizontalTitleGap: UsqueSpace.s,
        child: ExpansionTile(
          key: const PageStorageKey('home-connection-details'),
          leading: Icon(
            LucideIcons.slidersHorizontal,
            size: 18,
            color: theme.colorScheme.onSurfaceVariant,
          ),
          title: Text(
            strings.get('connection_details'),
            style: theme.textTheme.bodyMedium?.copyWith(
              fontWeight: FontWeight.w600,
            ),
          ),
          iconColor: theme.colorScheme.onSurfaceVariant,
          collapsedIconColor: theme.colorScheme.onSurfaceVariant,
          dense: true,
          minTileHeight: 48,
          tilePadding: EdgeInsets.zero,
          childrenPadding: const EdgeInsetsDirectional.only(start: 32),
          shape: const Border(),
          collapsedShape: const Border(),
          children: [_ConnectionDetailsReadout(controller: controller)],
        ),
      ),
    );
  }
}

typedef _DetailsView = ({
  String? ipv4,
  String? ipv6,
  bool tunnel,
  bool systemProxy,
  bool http,
  bool socks5,
});

class _ConnectionDetailsReadout extends StatelessWidget {
  const _ConnectionDetailsReadout({required this.controller});

  final AppController controller;
  @override
  Widget build(BuildContext context) => ControllerSelector<_DetailsView>(
    controller: controller,
    active: (app) => app.section == AppSection.home,
    selector: (app) => (
      ipv4: app.snapshot.isConnected ? app.snapshot.exit.ipv4 : null,
      ipv6: app.snapshot.isConnected ? app.snapshot.exit.ipv6 : null,
      tunnel: app.activeProfile.frontends.tunnel,
      systemProxy: app.activeProfile.proxy.systemProxy,
      http: app.activeProfile.frontends.http,
      socks5: app.activeProfile.frontends.socks5,
    ),
    builder: (context, view) {
      final strings = controller.strings;
      final theme = Theme.of(context);
      final addresses = [view.ipv4, view.ipv6]
          .whereType<String>()
          .map((value) => value.trim())
          .where((value) => value.isNotEmpty)
          .toList();
      final interfaces = [
        if (view.tunnel) strings.tunnelOutputLabel(theme.platform),
        if (view.systemProxy) strings.get('home_system_proxy'),
        if (view.http) 'HTTP',
        if (view.socks5) 'SOCKS5',
      ];
      final separator = switch (strings.languageCode) {
        'zh' => '、',
        'ar' || 'fa' => '، ',
        _ => ', ',
      };
      return Padding(
        key: const ValueKey('home-connection-detail-values'),
        padding: const EdgeInsets.only(bottom: UsqueSpace.s),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Wrap(
              key: const ValueKey('home-exit-ip'),
              spacing: UsqueSpace.xs,
              runSpacing: UsqueSpace.xxs,
              crossAxisAlignment: WrapCrossAlignment.center,
              children: [
                Text(
                  strings.get('home_exit_ip'),
                  style: theme.textTheme.bodyMedium?.copyWith(
                    color: theme.colorScheme.onSurfaceVariant,
                  ),
                ),
                if (addresses.isEmpty)
                  const EmptyValue(label: '—')
                else
                  for (final value in addresses) MonoValue(value: value),
              ],
            ),
            if (interfaces.isNotEmpty) ...[
              const SizedBox(height: UsqueSpace.xs),
              Text(
                strings
                    .get('home_enabled_interfaces')
                    .replaceAll('{interfaces}', interfaces.join(separator)),
                key: const ValueKey('home-enabled-interfaces'),
                style: theme.textTheme.bodyMedium?.copyWith(
                  color: theme.colorScheme.onSurfaceVariant,
                ),
              ),
            ],
          ],
        ),
      );
    },
  );
}
