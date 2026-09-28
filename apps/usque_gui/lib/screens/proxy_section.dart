import 'dart:async';

import 'package:flutter/material.dart';

import '../models/app_models.dart';
import '../state/app_controller.dart';
import '../widgets/controller_selector.dart';
import '../widgets/unsaved_changes_guard.dart';
import 'chain_proxy_screen.dart';
import 'proxy_screen.dart';

/// Keeps proxy subroutes inside the shell, including across rail breakpoints.
class ProxySection extends StatefulWidget {
  const ProxySection({
    required this.controller,
    required this.active,
    required this.onSubpageChanged,
    super.key,
  });

  final AppController controller;
  final bool active;
  final ValueChanged<bool> onSubpageChanged;

  @override
  State<ProxySection> createState() => ProxySectionState();
}

class ProxySectionState extends State<ProxySection> {
  final _navigator = GlobalKey<NavigatorState>();
  final _leaveGuard = GlobalKey<UnsavedChangesGuardState>();
  late final _active = ValueNotifier(widget.active);
  MaterialPageRoute<void>? _gateRoute;
  bool _closing = false;

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

  Future<void> openVpnGate() async {
    if (_gateRoute != null || _closing) return;
    final route = MaterialPageRoute<void>(
      settings: const RouteSettings(name: '/proxy/chain-proxy'),
      builder: (context) => ValueListenableBuilder<bool>(
        valueListenable: _active,
        builder: (context, active, _) => ChainProxyScreen(
          controller: widget.controller,
          active: active,
          leaveGuardKey: _leaveGuard,
        ),
      ),
    );
    _gateRoute = route;
    unawaited(_navigator.currentState!.push(route));
    widget.onSubpageChanged(true);
    // Finish the exit animation while the section is still visible so its
    // TickerMode cannot delay disposal and the cancellation of owned tasks.
    await route.completed;
    if (!mounted || _gateRoute != route) return;
    _gateRoute = null;
    widget.onSubpageChanged(false);
  }

  Future<bool> closeSubpage() async {
    final route = _gateRoute;
    if (route == null) return true;
    if (_closing) return false;
    _closing = true;
    try {
      if (!route.isActive) {
        await route.completed;
        return mounted;
      }
      // A country dropdown is a local popup route; dismiss it before asking
      // about the page's draft, without bypassing the page's own leave guard.
      _navigator.currentState!.popUntil((candidate) => candidate == route);
      if (!await (_leaveGuard.currentState?.confirmLeave() ??
          Future.value(false))) {
        return false;
      }
      if (!mounted) return false;
      if (route.isCurrent) _navigator.currentState!.pop();
      await route.completed;
      return mounted;
    } finally {
      _closing = false;
    }
  }

  @override
  Widget build(BuildContext context) => NavigatorPopHandler<void>(
    enabled: widget.active,
    onPopWithResult: (_) {
      if (widget.active) unawaited(_navigator.currentState!.maybePop());
    },
    child: Navigator(
      key: _navigator,
      onGenerateRoute: (_) => MaterialPageRoute<void>(
        settings: const RouteSettings(name: '/proxy'),
        builder: (context) => ControllerSelector<UsqueProfile>(
          controller: widget.controller,
          active: (controller) => controller.section == AppSection.proxy,
          selector: (controller) => controller.activeProfile,
          builder: (context, _) => ProxyScreen(
            controller: widget.controller,
            onOpenVpnGate: () => unawaited(openVpnGate()),
          ),
        ),
      ),
    ),
  );
}
