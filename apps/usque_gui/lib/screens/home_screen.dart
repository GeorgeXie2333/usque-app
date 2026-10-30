import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../core/app_strings.dart';
import '../core/usque_theme.dart';
import '../models/app_models.dart';
import '../state/app_controller.dart';
import '../widgets/common.dart';
import '../widgets/controller_selector.dart';
import '../widgets/home/home_details.dart';
import '../widgets/home/home_hero.dart';
import '../widgets/home/home_local_proxies.dart';
import '../widgets/home/home_phone.dart';
import '../widgets/home/home_traffic.dart';
import '../widgets/home_desktop_controls.dart';
import '../widgets/setup_handoff.dart';
import 'chain_proxy_screen.dart';
import 'diagnostics_screen.dart';

export '../widgets/home/home_details.dart' show killSwitchStatusKey;

/// The instrument panel: one connection control, one status readout, and the
/// live numbers that prove the tunnel is doing something.
///
/// Each block subscribes to its own slice of the controller, so a traffic
/// sample arriving every second repaints the traffic card instead of the
/// page. Cards group the session, the outputs and the traffic; everything
/// else stays on the canvas.
class HomeScreen extends StatelessWidget {
  const HomeScreen({
    required this.controller,
    this.onOpenChainProxy,
    this.onOpenDiagnostics,
    super.key,
  });

  final AppController controller;
  final VoidCallback? onOpenChainProxy;

  /// Opens Diagnostics with a Standard run; the shell keeps it in Settings.
  final VoidCallback? onOpenDiagnostics;

  @override
  Widget build(BuildContext context) {
    final AppStrings strings = controller.strings;
    final viewport = MediaQuery.sizeOf(context);
    // Windows always keeps the desktop sections; a narrow window stacks them
    // instead of dropping the output switches and local proxies.
    final bool compact =
        defaultTargetPlatform != TargetPlatform.windows &&
        defaultTargetPlatform != TargetPlatform.linux &&
        (viewport.width < 760 ||
            defaultTargetPlatform == TargetPlatform.android &&
                viewport.shortestSide < 600);
    void openChainProxy() => Navigator.of(context).push<void>(
      MaterialPageRoute(
        builder: (_) => ChainProxyScreen(controller: controller),
      ),
    );
    final diagnose =
        onOpenDiagnostics ?? () => openDiagnostics(context, controller);
    final notice = _ZeroTrustEndpointRiskNotice(controller: controller);
    final errorSlot = _ErrorSlot(
      controller: controller,
      strings: strings,
      onOpenDiagnostics: diagnose,
    );
    if (compact) {
      final expansion = _phoneExpansion(context);
      return PageFrame(
        title: strings.get('home'),
        titleWidget: HomePhoneHeader(controller: controller),
        child: SetupEntranceTransition(
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: <Widget>[
              notice,
              HomeChainSummary(
                controller: controller,
                strings: strings,
                onOpen: onOpenChainProxy ?? openChainProxy,
              ),
              HomeConnectionHero(
                controller: controller,
                strings: strings,
                compact: true,
                banner: errorSlot,
              ),
              SizedBox(height: UsqueSpace.xs + expansion * UsqueSpace.m),
              HomeTrafficCard(
                controller: controller,
                compact: true,
                chartHeight: 64 + expansion * 24,
              ),
              const SizedBox(height: UsqueSpace.m),
              HomeStatusCard(controller: controller, strings: strings),
            ],
          ),
        ),
      );
    }
    return PageFrame(
      title: strings.get('home'),
      showHeading: defaultTargetPlatform != TargetPlatform.windows,
      fillViewport: true,
      child: SetupEntranceTransition(
        child: _DesktopHome(
          controller: controller,
          strings: strings,
          header: [notice, errorSlot],
          onOpenChainProxy: onOpenChainProxy ?? openChainProxy,
        ),
      ),
    );
  }
}

/// Shares a small, bounded amount of breathing room across the phone
/// sections on tall screens. Large text and short viewports keep the compact
/// spacing.
double _phoneExpansion(BuildContext context) {
  final media = MediaQuery.of(context);
  if (media.textScaler.scale(14) > 21) return 0;
  final usableHeight = media.size.height - media.viewPadding.vertical;
  return ((usableHeight - 812) / 120).clamp(0, 1).toDouble();
}

/// Desktop Home: the session and output cards side by side at one height,
/// with the traffic card taking the window height left below them.
///
/// Narrow windows and text above 150% stack the three cards.
class _DesktopHome extends StatelessWidget {
  const _DesktopHome({
    required this.controller,
    required this.strings,
    required this.header,
    required this.onOpenChainProxy,
  });

  final AppController controller;
  final AppStrings strings;
  final List<Widget> header;
  final VoidCallback onOpenChainProxy;

  /// Content width from which the output card sits beside the session card.
  static const double _sideBySide = 860;
  static const double _cardGap = UsqueSpace.m;
  static const double _heroPadding = UsqueSpace.l;

  /// Narrowest session readout beside the ring.
  static const double _readoutMin = 220;

  /// The traffic chart grows into spare window height up to this card height.
  static const double _maxTrafficExtent = 420;

  @override
  Widget build(BuildContext context) => LayoutBuilder(
    builder: (context, constraints) {
      final width = constraints.maxWidth;
      final largeText = MediaQuery.textScalerOf(context).scale(14) > 21;
      final details = HomeDetails(controller: controller, strings: strings);
      Widget outputs({required bool fill}) => Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        mainAxisSize: fill ? MainAxisSize.max : MainAxisSize.min,
        children: [
          HomeDesktopControls(
            controller: controller,
            onOpenChainProxy: onOpenChainProxy,
          ),
          if (fill) const Spacer() else const SizedBox(height: UsqueSpace.xxs),
          Divider(height: 1, color: UsqueTokens.of(context).hairline),
          const SizedBox(height: UsqueSpace.s),
          HomeLocalProxies(controller: controller),
        ],
      );
      const outputsPadding = EdgeInsets.fromLTRB(20, 6, 20, 16);
      const heroPadding = EdgeInsets.fromLTRB(
        _heroPadding,
        _heroPadding,
        _heroPadding,
        UsqueSpace.xs,
      );
      final traffic = HomeTrafficCard(controller: controller);

      if (width >= _sideBySide && !largeText) {
        final outputsWidth = width >= 960 ? 340.0 : 312.0;
        final heroInner = width - _cardGap - outputsWidth - 2 * _heroPadding;
        final ring = (heroInner - _readoutMin - UsqueSpace.xl).clamp(
          200.0,
          264.0,
        );
        return FillColumn(
          maxLastExtent: _maxTrafficExtent,
          children: [
            ...header,
            IntrinsicHeight(
              child: Row(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  Expanded(
                    child: Panel(
                      padding: heroPadding,
                      child: HomeConnectionHero(
                        controller: controller,
                        strings: strings,
                        ringSize: ring,
                        fill: true,
                        footer: details,
                      ),
                    ),
                  ),
                  const SizedBox(width: _cardGap),
                  SizedBox(
                    width: outputsWidth,
                    child: Panel(
                      padding: outputsPadding,
                      child: outputs(fill: true),
                    ),
                  ),
                ],
              ),
            ),
            const SizedBox(height: _cardGap),
            traffic,
          ],
        );
      }

      // Stacked: the ring stays beside its readouts while the card is wide
      // enough, and moves above them otherwise.
      final heroInner = width - 2 * _heroPadding;
      final ring = heroInner >= 520 && !largeText
          ? (heroInner - _readoutMin - UsqueSpace.xl).clamp(200.0, 264.0)
          : null;
      return FillColumn(
        maxLastExtent: _maxTrafficExtent,
        children: [
          ...header,
          Panel(
            padding: heroPadding,
            child: HomeConnectionHero(
              controller: controller,
              strings: strings,
              ringSize: ring,
              footer: details,
            ),
          ),
          const SizedBox(height: _cardGap),
          Panel(padding: outputsPadding, child: outputs(fill: false)),
          const SizedBox(height: _cardGap),
          traffic,
        ],
      );
    },
  );
}

class _ZeroTrustEndpointRiskNotice extends StatelessWidget {
  const _ZeroTrustEndpointRiskNotice({required this.controller});

  final AppController controller;

  @override
  Widget build(BuildContext context) => ControllerSelector<(bool, bool)>(
    controller: controller,
    active: (app) => app.section == AppSection.home,
    selector: (app) => (
      app.hasCustomZeroTrustEndpointRisk,
      app.snapshot.isConnected &&
          app.snapshot.endpointSource == 'manual_ipv4_fallback',
    ),
    builder: (context, state) => Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        if (state.$2)
          Padding(
            padding: const EdgeInsets.only(bottom: UsqueSpace.m),
            child: HintText(
              controller.strings.get('endpoint_preset_fallback_active'),
              key: const ValueKey('home-endpoint-preset-fallback'),
            ),
          ),
        if (state.$1)
          Padding(
            padding: const EdgeInsets.only(bottom: UsqueSpace.m),
            child: WarningBanner(
              key: const ValueKey('home-zero-trust-endpoint-risk'),
              danger: true,
              title: controller.strings.get(
                'zero_trust_endpoint_home_risk_title',
              ),
              message: controller.strings.get(
                'zero_trust_endpoint_home_risk_body',
              ),
            ),
          ),
      ],
    ),
  );
}

class _ErrorSlot extends StatelessWidget {
  const _ErrorSlot({
    required this.controller,
    required this.strings,
    this.onOpenDiagnostics,
  });

  final AppController controller;
  final AppStrings strings;
  final VoidCallback? onOpenDiagnostics;

  @override
  Widget build(BuildContext context) {
    return ControllerSelector<({String? error, bool failed})>(
      controller: controller,
      active: (controller) => controller.section == AppSection.home,
      selector: (controller) => (
        error: controller.lastError,
        failed: controller.snapshot.phase == ConnectionPhase.error,
      ),
      builder: (context, view) => BannerSlot(
        child: view.error == null
            ? null
            : WarningBanner(
                // The connection heading already names a failed connection.
                // Other operations share this slot and get a neutral title.
                title: view.failed ? null : strings.get('error_generic'),
                message: view.error!,
                danger: true,
                onDismiss: controller.clearError,
                // A failed connection gets a next step beyond Retry; other
                // operations sharing this slot report their own outcome.
                action: view.failed && onOpenDiagnostics != null
                    ? TextButton.icon(
                        key: const ValueKey('home-error-diagnostics'),
                        onPressed: onOpenDiagnostics,
                        icon: const Icon(LucideIcons.stethoscope),
                        label: Text(strings.get('diag_start')),
                      )
                    : null,
              ),
      ),
    );
  }
}
