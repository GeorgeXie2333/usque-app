import 'package:flutter/material.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../../core/usque_theme.dart';
import '../../models/app_models.dart';
import '../../state/app_controller.dart';
import '../controller_selector.dart';

typedef _LocalProxiesView = ({
  String catalogId,
  bool http,
  bool socks,
  String httpAddress,
  String socksAddress,
});

/// HTTP and SOCKS5 listener addresses with a shortcut to the Proxy page.
class HomeLocalProxies extends StatelessWidget {
  const HomeLocalProxies({required this.controller, super.key});
  final AppController controller;

  @override
  Widget build(BuildContext context) => ControllerSelector<_LocalProxiesView>(
    controller: controller,
    active: (app) => app.section == AppSection.home,
    selector: (app) {
      final profile = app.activeProfile;
      return (
        catalogId: app.strings.catalogId,
        http: profile.frontends.http,
        socks: profile.frontends.socks5,
        httpAddress: profile.proxy.httpListeners.join(', '),
        socksAddress: profile.proxy.socksListeners.join(', '),
      );
    },
    builder: (context, view) {
      final strings = controller.strings;
      final theme = Theme.of(context);
      final muted = theme.colorScheme.onSurfaceVariant;
      // A Wrap moves the address under its name when large text leaves no
      // room beside it.
      Widget output(String name, bool enabled, String address) => Padding(
        padding: const EdgeInsets.only(top: UsqueSpace.xxs + 2),
        child: Wrap(
          spacing: UsqueSpace.xs,
          runSpacing: UsqueSpace.xxs,
          crossAxisAlignment: WrapCrossAlignment.center,
          children: [
            ConstrainedBox(
              constraints: const BoxConstraints(minWidth: 56),
              child: Text(
                name,
                style: theme.textTheme.labelMedium?.copyWith(
                  color: muted,
                  fontWeight: FontWeight.w600,
                ),
              ),
            ),
            if (enabled)
              _ListenerAddresses(addresses: address)
            else
              Text('—', style: UsqueTheme.address(context, color: muted)),
          ],
        ),
      );
      return Column(
        key: const ValueKey('home-local-proxies'),
        crossAxisAlignment: CrossAxisAlignment.stretch,
        mainAxisSize: MainAxisSize.min,
        children: [
          Row(
            children: [
              Expanded(
                child: Semantics(
                  header: true,
                  child: Text(
                    strings.get('home_local_proxies'),
                    style: theme.textTheme.titleSmall,
                  ),
                ),
              ),
              TextButton.icon(
                key: const ValueKey('home-manage-proxies'),
                onPressed: () => controller.selectSection(AppSection.proxy),
                style: TextButton.styleFrom(
                  padding: const EdgeInsetsDirectional.only(start: 8, end: 4),
                  visualDensity: VisualDensity.compact,
                ),
                icon: const Icon(LucideIcons.chevronRightDir, size: 16),
                iconAlignment: IconAlignment.end,
                label: Text(strings.get('home_manage_proxies')),
              ),
            ],
          ),
          output('HTTP', view.http, view.httpAddress),
          output('SOCKS5', view.socks, view.socksAddress),
        ],
      );
    },
  );
}

/// Selectable listener list; IPv6 listeners are secondary to the IPv4 one.
class _ListenerAddresses extends StatefulWidget {
  const _ListenerAddresses({required this.addresses});

  /// Comma-separated listeners, kept as one string so selector equality holds.
  final String addresses;

  @override
  State<_ListenerAddresses> createState() => _ListenerAddressesState();
}

class _ListenerAddressesState extends State<_ListenerAddresses> {
  // See MonoValue: SelectableText must not inherit the page's storage slot.
  final _textStorage = PageStorageBucket();

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    final parts = widget.addresses.split(', ');
    final spans = <InlineSpan>[];
    for (var i = 0; i < parts.length; i++) {
      if (i > 0) {
        spans.add(
          TextSpan(
            text: ', ',
            style: TextStyle(color: scheme.onSurfaceVariant),
          ),
        );
      }
      spans.add(
        TextSpan(
          text: parts[i],
          style: parts[i].startsWith('[')
              // One weight per line; colour alone ranks the IPv6 listener.
              ? TextStyle(color: scheme.onSurfaceVariant)
              : null,
        ),
      );
    }
    return PageStorage(
      bucket: _textStorage,
      child: SelectableText.rich(
        TextSpan(children: spans),
        style: UsqueTheme.address(context, weight: FontWeight.w500),
      ),
    );
  }
}
