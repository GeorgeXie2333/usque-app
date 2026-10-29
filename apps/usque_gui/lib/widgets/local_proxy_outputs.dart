import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../state/app_controller.dart';
import 'common.dart';

/// Immediate device-wide outputs: the tunnel and, on desktop, the system proxy.
/// Each local listener's switch sits in its own section on the Proxy page, so
/// no output is listed twice; listener addresses remain a separate draft until
/// Apply changes is pressed.
class LocalProxyOutputs extends StatelessWidget {
  const LocalProxyOutputs({
    required this.controller,
    required this.enabled,
    super.key,
  });

  final AppController controller;
  final bool enabled;

  @override
  Widget build(BuildContext context) {
    final strings = controller.strings;
    final profile = controller.activeProfile;
    final frontends = profile.frontends;
    final systemProxy = profile.proxy.systemProxy;
    return ContentSection(
      icon: LucideIcons.share2,
      title: strings.get('proxy_device_outputs'),
      subtitle: strings.get('proxy_switches_hint'),
      gap: 10,
      child: RowTileTheme(
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            SwitchListTile(
              contentPadding: EdgeInsets.zero,
              secondary: const Icon(LucideIcons.ethernetPort),
              title: Text(strings.tunnelOutputLabel(defaultTargetPlatform)),
              subtitle: Text(
                strings.get(
                  controller.tunnelOutputSupported
                      ? 'home_tun_hint'
                      : defaultTargetPlatform == TargetPlatform.macOS
                      ? 'macos_output_unavailable'
                      : 'linux_output_unavailable',
                ),
              ),
              value: frontends.tunnel,
              onChanged:
                  !enabled ||
                      !(controller.tunnelOutputSupported || frontends.tunnel)
                  ? null
                  : (value) => controller.updateNetwork(
                      profile.copyWith(
                        frontends: frontends.copyWith(tunnel: value),
                      ),
                      changedFields: const ['frontends.tunnel'],
                    ),
            ),
            if (defaultTargetPlatform == TargetPlatform.windows ||
                defaultTargetPlatform == TargetPlatform.linux ||
                defaultTargetPlatform == TargetPlatform.macOS)
              SwitchListTile(
                contentPadding: EdgeInsets.zero,
                secondary: const Icon(LucideIcons.monitorCog),
                title: Text(strings.get('home_system_proxy')),
                // Same rule and wording as Home: system proxy needs the HTTP
                // listener, but one that is already on can always be turned off.
                subtitle: Text(
                  strings.get(
                    !controller.systemProxyOutputSupported
                        ? 'linux_output_unavailable'
                        : !frontends.http && !systemProxy
                        ? 'home_system_proxy_requires_http'
                        : 'home_system_proxy_hint',
                  ),
                ),
                value: systemProxy,
                onChanged:
                    !enabled ||
                        !(controller.systemProxyOutputSupported ||
                            systemProxy) ||
                        !(frontends.http || systemProxy)
                    ? null
                    : (value) => controller.updateNetwork(
                        profile.copyWith(
                          proxy: profile.proxy.copyWith(systemProxy: value),
                        ),
                        changedFields: const ['proxy.system_proxy'],
                      ),
              ),
            if (!frontends.any)
              WarningBanner(
                title: strings.get('channel_only'),
                message: strings.get('channel_only_warning'),
              ),
          ],
        ),
      ),
    );
  }
}
