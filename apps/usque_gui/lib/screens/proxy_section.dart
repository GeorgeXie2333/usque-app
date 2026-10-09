import 'dart:async';

import 'package:flutter/material.dart';

import '../models/app_models.dart';
import '../state/app_controller.dart';
import '../widgets/controller_selector.dart';
import '../widgets/section_navigator.dart';
import 'chain_proxy_screen.dart';
import 'proxy_screen.dart';

/// The Proxy section and its chain-proxy subpage.
class ProxySection extends StatefulWidget {
  const ProxySection({
    required this.controller,
    required this.active,
    required this.onSubpageChanged,
    required this.navigatorKey,
    super.key,
  });

  final AppController controller;
  final bool active;
  final ValueChanged<bool> onSubpageChanged;
  final GlobalKey<SectionNavigatorState> navigatorKey;

  @override
  State<ProxySection> createState() => ProxySectionState();
}

class ProxySectionState extends State<ProxySection> {
  late final _active = ValueNotifier(widget.active);
  MaterialPageRoute<void>? _chainRoute;

  @override
  void didUpdateWidget(covariant ProxySection oldWidget) {
    super.didUpdateWidget(oldWidget);
    _active.value = widget.active;
  }

  @override
  void dispose() {
    _active.dispose();
    super.dispose();
  }

  Future<void> openChainProxy() async {
    final section = widget.navigatorKey.currentState;
    final navigator = section?.navigator;
    if (_chainRoute != null || navigator == null || section!.closing) return;
    final route = MaterialPageRoute<void>(
      settings: const RouteSettings(name: '/proxy/chain-proxy'),
      builder: (context) => ValueListenableBuilder<bool>(
        valueListenable: _active,
        builder: (context, active, _) =>
            ChainProxyScreen(controller: widget.controller, active: active),
      ),
    );
    _chainRoute = route;
    try {
      await navigator.push(route);
      await route.completed;
    } finally {
      if (_chainRoute == route) _chainRoute = null;
    }
  }

  @override
  Widget build(BuildContext context) => SectionNavigator(
    key: widget.navigatorKey,
    active: widget.active,
    rootName: '/proxy',
    onSubpageChanged: widget.onSubpageChanged,
    builder: (context) => ControllerSelector<UsqueProfile>(
      controller: widget.controller,
      active: (controller) => controller.section == AppSection.proxy,
      selector: (controller) => controller.activeProfile,
      builder: (context, _) => ProxyScreen(
        controller: widget.controller,
        onOpenChainProxy: () => unawaited(openChainProxy()),
      ),
    ),
  );
}
