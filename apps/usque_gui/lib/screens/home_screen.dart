import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../core/app_strings.dart';
import '../core/chain_home_status.dart';
import '../core/chain_scope_presentation.dart';
import '../core/connection_presentation.dart';
import '../core/usque_motion.dart';
import '../core/usque_theme.dart';
import '../models/app_models.dart';
import '../models/network_quality_models.dart';
import '../state/app_controller.dart';
import '../widgets/common.dart';
import '../widgets/connection_ring.dart';
import '../widgets/controller_selector.dart';
import '../widgets/country_flag.dart';
import '../widgets/home_desktop_controls.dart';
import '../widgets/live_duration.dart';
import '../widgets/mobile_home_panels.dart';
import '../widgets/profile_identity_dialog.dart';
import '../widgets/sparkline.dart';
import '../widgets/usque_logo.dart';
import 'chain_proxy_screen.dart';

/// The instrument panel: one connection control, one status readout, and the
/// live numbers that prove the tunnel is doing something.
///
/// Each block subscribes to its own slice of the controller, so a traffic
/// sample arriving every second repaints two counters instead of the page.
class HomeScreen extends StatelessWidget {
  const HomeScreen({
    required this.controller,
    this.onOpenChainProxy,
    super.key,
  });

  final AppController controller;
  final VoidCallback? onOpenChainProxy;

  @override
  Widget build(BuildContext context) {
    final AppStrings strings = controller.strings;
    final viewport = MediaQuery.sizeOf(context);
    final bool compact =
        viewport.width < 760 ||
        defaultTargetPlatform == TargetPlatform.android &&
            viewport.shortestSide < 600;
    void openChainProxy() => Navigator.of(context).push<void>(
      MaterialPageRoute(
        builder: (_) => ChainProxyScreen(controller: controller),
      ),
    );
    final notice = _ZeroTrustEndpointRiskNotice(controller: controller);
    if (compact) {
      return PageFrame(
        title: strings.get('home'),
        titleWidget: const _NarrowBrandHeader(),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: <Widget>[
            notice,
            if (defaultTargetPlatform == TargetPlatform.android)
              _VpnGateReadout(
                controller: controller,
                strings: strings,
                onOpen: onOpenChainProxy ?? openChainProxy,
              ),
            PanelStack(
              spacing: 24 + mobileHomeExpansion(context) * 8,
              children: [
                _ConnectionHero(
                  controller: controller,
                  strings: strings,
                  compact: true,
                ),
                MobileTrafficPanel(controller: controller),
                MobileConnectionOverview(
                  controller: controller,
                  details: _HomeDetails(
                    controller: controller,
                    strings: strings,
                    compact: true,
                  ),
                ),
              ],
            ),
          ],
        ),
      );
    }
    // Traffic is last so its charts take up the height left in the window and
    // the page ends at the bottom margin rather than above an empty band.
    return PageFrame(
      title: strings.get('home'),
      showHeading: defaultTargetPlatform != TargetPlatform.windows,
      fillViewport: true,
      child: FillColumn(
        children: <Widget>[
          notice,
          _ErrorSlot(controller: controller, strings: strings),
          _DesktopHomeConnection(
            controller: controller,
            strings: strings,
            onOpenChainProxy: onOpenChainProxy ?? openChainProxy,
          ),
          const SizedBox(height: 32),
          Divider(height: 1, color: UsqueTokens.of(context).hairline),
          _DesktopLocalProxies(controller: controller),
          Divider(height: 1, color: UsqueTokens.of(context).hairline),
          const SizedBox(height: 20),
          _TrafficGrid(controller: controller, strings: strings),
        ],
      ),
    );
  }
}

class _ZeroTrustEndpointRiskNotice extends StatelessWidget {
  const _ZeroTrustEndpointRiskNotice({required this.controller});

  final AppController controller;

  @override
  Widget build(BuildContext context) => ControllerSelector<bool>(
    controller: controller,
    active: (app) => app.section == AppSection.home,
    selector: (app) => app.hasCustomZeroTrustEndpointRisk,
    builder: (context, show) => show
        ? Padding(
            padding: const EdgeInsets.only(bottom: 24),
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
          )
        : const SizedBox.shrink(),
  );
}

class _DesktopHomeConnection extends StatelessWidget {
  const _DesktopHomeConnection({
    required this.controller,
    required this.strings,
    required this.onOpenChainProxy,
  });

  final AppController controller;
  final AppStrings strings;
  final VoidCallback onOpenChainProxy;

  @override
  Widget build(BuildContext context) => LayoutBuilder(
    builder: (context, constraints) {
      final hero = _ConnectionHero(controller: controller, strings: strings);
      final controls = HomeDesktopControls(
        controller: controller,
        onOpenChainProxy: onOpenChainProxy,
      );
      if (constraints.maxWidth >= 860 &&
          MediaQuery.textScalerOf(context).scale(14) <= 21) {
        final controlWidth = constraints.maxWidth >= 940 ? 320.0 : 280.0;
        return Row(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Expanded(child: hero),
            const SizedBox(width: 32),
            SizedBox(width: controlWidth, child: controls),
          ],
        );
      }
      return PanelStack(spacing: 32, children: [hero, controls]);
    },
  );
}

typedef _LocalProxiesView = ({
  bool http,
  bool socks,
  String httpAddress,
  String socksAddress,
});

class _DesktopLocalProxies extends StatelessWidget {
  const _DesktopLocalProxies({required this.controller});
  final AppController controller;

  @override
  Widget build(BuildContext context) => ControllerSelector<_LocalProxiesView>(
    controller: controller,
    active: (app) => app.section == AppSection.home,
    selector: (app) {
      final profile = app.activeProfile;
      return (
        http: profile.frontends.http,
        socks: profile.frontends.socks5,
        httpAddress: profile.proxy.httpListeners.join(', '),
        socksAddress: profile.proxy.socksListeners.join(', '),
      );
    },
    builder: (context, view) {
      final strings = controller.strings;
      Widget output(IconData icon, String name, bool enabled, String address) =>
          Row(
            mainAxisSize: MainAxisSize.min,
            children: [
              Icon(
                icon,
                size: 16,
                color: Theme.of(context).colorScheme.onSurfaceVariant,
              ),
              const SizedBox(width: 8),
              Text(name, style: Theme.of(context).textTheme.bodySmall),
              const SizedBox(width: 8),
              Flexible(
                child: enabled
                    ? _ListenerAddresses(addresses: address)
                    : const MonoValue(value: '—'),
              ),
            ],
          );
      final values = Wrap(
        spacing: 24,
        runSpacing: 12,
        crossAxisAlignment: WrapCrossAlignment.center,
        children: [
          Text(
            strings.get('home_local_proxies'),
            style: Theme.of(context).textTheme.titleSmall,
          ),
          output(LucideIcons.globe, 'HTTP', view.http, view.httpAddress),
          output(LucideIcons.network, 'SOCKS5', view.socks, view.socksAddress),
        ],
      );
      final manage = TextButton.icon(
        key: const ValueKey('home-manage-proxies'),
        onPressed: () => controller.selectSection(AppSection.proxy),
        icon: const Icon(LucideIcons.chevronRight, size: 16),
        iconAlignment: IconAlignment.end,
        label: Text(strings.get('home_manage_proxies')),
      );
      return ContentSection(
        key: const ValueKey('home-local-proxies'),
        padding: const EdgeInsets.symmetric(vertical: 12),
        child: LayoutBuilder(
          builder: (context, constraints) =>
              constraints.maxWidth >= 760 &&
                  MediaQuery.textScalerOf(context).scale(14) <= 21
              ? Row(
                  children: [
                    Expanded(child: values),
                    const SizedBox(width: 16),
                    manage,
                  ],
                )
              : Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [values, const SizedBox(height: 8), manage],
                ),
        ),
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

class _NarrowBrandHeader extends StatelessWidget {
  const _NarrowBrandHeader();

  @override
  Widget build(BuildContext context) {
    return Semantics(
      label: 'Usque',
      header: true,
      child: Row(
        children: <Widget>[
          const UsqueLogo(size: 40),
          const SizedBox(width: 12),
          Text('Usque', style: Theme.of(context).textTheme.titleLarge),
        ],
      ),
    );
  }
}

class _VpnGateReadout extends StatelessWidget {
  const _VpnGateReadout({
    required this.controller,
    required this.strings,
    required this.onOpen,
  });
  final AppController controller;
  final AppStrings strings;
  final VoidCallback onOpen;

  @override
  Widget build(BuildContext context) =>
      ControllerSelector<
        ({
          bool enabled,
          ConnectionPhase phase,
          VpnGateStatus status,
          ChainExitStatus chain,
          ChainSource source,
          ChainScopePresentation scope,
        })
      >(
        controller: controller,
        active: (app) => app.section == AppSection.home,
        selector: (app) => (
          enabled: app.activeProfile.chainEnabled,
          phase: app.snapshot.phase,
          status: app.snapshot.vpnGate,
          chain: app.snapshot.chainExit,
          source: app.activeProfile.chainSource,
          scope: ChainScopePresentation.of(
            app.snapshot,
            app.networkSettings.state?.appliedProfile,
            isAndroid: defaultTargetPlatform == TargetPlatform.android,
          ),
        ),
        builder: (context, view) {
          final homeStatus = ChainHomeStatus.of(
            phase: view.phase,
            chainEnabled: view.enabled,
            chainStage: view.chain.stage,
            gateStage: view.status.stage,
            hasCurrentProfile: view.chain.currentProfile != null,
            hasGateServer: view.status.server != null,
          );
          if (!homeStatus.showChainRow && view.scope.isEmpty) {
            return const SizedBox.shrink();
          }
          final status = view.status;
          final server = status.server;
          final warpKey = ChainHomeStatus.warpLabelKey(
            phase: view.phase,
            chainStage: view.chain.stage,
            gateStage: status.stage,
            reportedStage: view.chain.warpStage ?? status.warpStage,
          );
          final phaseLabel =
              !homeStatus.showChainRow && view.phase == ConnectionPhase.error
              ? strings.get('error')
              : homeStatus.label(strings);
          return Padding(
            padding: const EdgeInsets.only(bottom: 24),
            child: Semantics(
              container: true,
              liveRegion: true,
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  Align(
                    alignment: AlignmentDirectional.centerStart,
                    child: TextButton(
                      key: const ValueKey('home-vpn-gate-settings'),
                      style: TextButton.styleFrom(
                        foregroundColor: Theme.of(
                          context,
                        ).colorScheme.onSurface,
                        padding: const EdgeInsets.symmetric(vertical: 8),
                        alignment: AlignmentDirectional.centerStart,
                      ),
                      onPressed: onOpen,
                      child: Row(
                        mainAxisSize: MainAxisSize.min,
                        children: [
                          Flexible(
                            child: Column(
                              mainAxisSize: MainAxisSize.min,
                              crossAxisAlignment: CrossAxisAlignment.start,
                              children: [
                                Text(
                                  'WARP → ${view.chain.currentProfile?.name ?? view.scope.source?.label ?? view.source.label}',
                                  style: Theme.of(
                                    context,
                                  ).textTheme.titleMedium,
                                ),
                                const SizedBox(height: 4),
                                Text(
                                  phaseLabel,
                                  style: Theme.of(context).textTheme.bodySmall
                                      ?.copyWith(
                                        color: Theme.of(
                                          context,
                                        ).colorScheme.onSurfaceVariant,
                                      ),
                                ),
                              ],
                            ),
                          ),
                          const SizedBox(width: 12),
                          const Icon(LucideIcons.chevronRight, size: 18),
                        ],
                      ),
                    ),
                  ),
                  const SizedBox(height: 8),
                  Text(
                    'WARP: ${warpKey == null ? '—' : strings.get(warpKey)}',
                    key: const ValueKey('home-chain-warp-status'),
                  ),
                  if (view.chain.currentProfile case final current?)
                    Text(current.source.label),
                  if (server != null && view.chain.currentProfile == null)
                    Row(
                      children: [
                        CountryFlag(countryCode: server.countryCode),
                        const SizedBox(width: 8),
                        Expanded(
                          child: Text(
                            '${strings.get(status.connected ? 'gate_current' : 'gate_draft')}: ${server.countryCode ?? '—'} · ${server.ip}',
                            style: const TextStyle(
                              fontFeatures: UsqueTheme.tabularFigures,
                            ),
                          ),
                        ),
                      ],
                    ),
                ],
              ),
            ),
          );
        },
      );
}

class _ErrorSlot extends StatelessWidget {
  const _ErrorSlot({required this.controller, required this.strings});

  final AppController controller;
  final AppStrings strings;

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
              ),
      ),
    );
  }
}

typedef _HeroView = ({
  ConnectionPhase phase,
  bool busy,
  String? errorCode,
  String profileName,
  bool identityReady,
  bool chainEnabled,
  String chainStage,
  String gateStage,
  bool chainProfile,
  bool gateServer,
});

_HeroView _heroView(AppController controller) => (
  phase: controller.snapshot.phase,
  busy: controller.busy,
  errorCode: controller.snapshot.errorCode,
  profileName: controller.activeProfile.name,
  identityReady:
      controller.identityState(controller.activeProfileId) ==
      ProfileIdentityState.ready,
  chainEnabled: controller.activeProfile.chainEnabled,
  chainStage: controller.snapshot.chainExit.stage,
  gateStage: controller.snapshot.vpnGate.stage,
  chainProfile: controller.snapshot.chainExit.currentProfile != null,
  gateServer: controller.snapshot.vpnGate.server != null,
);

class _ConnectionHero extends StatelessWidget {
  const _ConnectionHero({
    required this.controller,
    required this.strings,
    this.compact = false,
  });

  final bool compact;

  final AppController controller;
  final AppStrings strings;

  @override
  Widget build(BuildContext context) {
    return ControllerSelector<_HeroView>(
      controller: controller,
      active: (controller) => controller.section == AppSection.home,
      selector: _heroView,
      builder: (context, view) => _buildHero(context, view),
    );
  }

  Widget _buildHero(BuildContext context, _HeroView view) {
    final theme = Theme.of(context);
    final connection = ConnectionPresentation.of(view.phase);
    final chainStatus = ChainHomeStatus.of(
      phase: view.phase,
      chainEnabled: view.chainEnabled,
      chainStage: view.chainStage,
      gateStage: view.gateStage,
      hasCurrentProfile: view.chainProfile,
      hasGateServer: view.gateServer,
    );
    final status = chainStatus.drivesHome
        ? chainStatus.label(strings)
        : strings.get(connection.labelKey);
    final error = view.phase == ConnectionPhase.error;
    final recoveryBlocked =
        error && view.errorCode == 'WINDOWS_RECOVERY_BLOCKED';
    final primaryRetry = error && !recoveryBlocked && view.identityReady;
    final action = strings.get(
      primaryRetry
          ? 'retry'
          : error && !recoveryBlocked && !view.identityReady
          ? 'configure_identity'
          : connection.actionKey,
    );
    final canAct =
        (!view.busy ||
            view.phase == ConnectionPhase.preparing ||
            view.phase == ConnectionPhase.connectingH3 ||
            view.phase == ConnectionPhase.connectingH2 ||
            view.phase == ConnectionPhase.reconnecting) &&
        view.phase != ConnectionPhase.disconnecting &&
        !recoveryBlocked;
    Widget ring(double size) => ConnectionRing(
      phase: view.phase,
      presentation: chainStatus.drivesHome
          ? chainStatus.presentation(connection)
          : null,
      busy: view.busy,
      actionLabel: action,
      semanticLabel: '${strings.get('connection_status')}: $status',
      size: size,
      compactControl: compact,
      onPressed: !canAct
          ? null
          : primaryRetry
          ? controller.retry
          : () => _connectOrRepairIdentity(context),
    );
    Widget statusText() => Semantics(
      liveRegion: true,
      child: FadeThroughSwitcher(
        alignment: compact
            ? Alignment.center
            : AlignmentDirectional.centerStart,
        child: Text(
          status,
          key: ValueKey(
            chainStatus.drivesHome ? chainStatus.labelKey : view.phase,
          ),
          textAlign: compact ? TextAlign.center : TextAlign.start,
          style: compact
              ? theme.textTheme.headlineSmall
              : theme.textTheme.headlineMedium,
        ),
      ),
    );
    Widget recovery() => Wrap(
      alignment: compact ? WrapAlignment.start : WrapAlignment.center,
      spacing: 8,
      runSpacing: 8,
      children: [
        if (!error)
          OutlinedButton.icon(
            onPressed: view.busy ? null : controller.retry,
            icon: const Icon(LucideIcons.refreshCw),
            label: Text(strings.get('retry')),
          ),
      ],
    );
    if (compact) {
      final expansion = mobileHomeExpansion(context);
      final ringSize = 148 + expansion * 32;
      return ContentSection(
        key: const ValueKey('mobile-connection-section'),
        padding: EdgeInsets.symmetric(
          horizontal: 0,
          vertical: 12 + expansion * 12,
        ),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Row(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Expanded(
                  child: Text(
                    strings.get('active_profile'),
                    style: theme.textTheme.bodySmall?.copyWith(
                      color: theme.colorScheme.onSurfaceVariant,
                    ),
                  ),
                ),
                const SizedBox(width: 12),
                Expanded(
                  flex: 2,
                  child: Tooltip(
                    message: view.profileName,
                    child: Text(
                      view.profileName,
                      maxLines: 2,
                      overflow: TextOverflow.ellipsis,
                      textAlign: TextAlign.end,
                      style: theme.textTheme.titleMedium,
                    ),
                  ),
                ),
              ],
            ),
            const SizedBox(height: 8),
            Center(child: ring(ringSize)),
            const SizedBox(height: 6),
            statusText(),
            const SizedBox(height: 10),
            _ErrorSlot(controller: controller, strings: strings),
            if (connection.recoverable && !error) ...[
              Center(
                child: OutlinedButton.icon(
                  onPressed: view.busy ? null : controller.retry,
                  icon: const Icon(LucideIcons.refreshCw, size: 18),
                  label: Text(strings.get('retry')),
                ),
              ),
              const SizedBox(height: 10),
            ],
            Divider(height: 1, color: UsqueTokens.of(context).hairline),
            const SizedBox(height: 8),
            _ProtectionSummary(controller: controller),
          ],
        ),
      );
    }
    Widget overview() => Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Text(
          strings.get('active_profile'),
          style: theme.textTheme.bodySmall?.copyWith(
            color: theme.colorScheme.onSurfaceVariant,
          ),
        ),
        const SizedBox(height: 5),
        Tooltip(
          message: view.profileName,
          child: Text(
            view.profileName,
            maxLines: 2,
            overflow: TextOverflow.ellipsis,
            style: theme.textTheme.titleMedium,
          ),
        ),
        const SizedBox(height: 14),
        statusText(),
        const SizedBox(height: 20),
        _DesktopSessionReadout(controller: controller),
        if (connection.recoverable && !error) ...[
          const SizedBox(height: 12),
          recovery(),
        ],
        _HomeDetails(controller: controller, strings: strings),
      ],
    );
    return ContentSection(
      key: const ValueKey('home-desktop-connection'),
      child: LayoutBuilder(
        builder: (context, constraints) {
          final split =
              constraints.maxWidth >= 560 &&
              MediaQuery.textScalerOf(context).scale(14) <= 21;
          if (split) {
            final size = (constraints.maxWidth - 220 - 24).clamp(224.0, 330.0);
            return Row(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                SizedBox(width: size, child: ring(size)),
                const SizedBox(width: 24),
                Expanded(child: overview()),
              ],
            );
          }
          return PanelStack(
            spacing: 24,
            children: [
              Center(
                child: ring(constraints.maxWidth.clamp(180, 330).toDouble()),
              ),
              overview(),
            ],
          );
        },
      ),
    );
  }

  Future<void> _connectOrRepairIdentity(BuildContext context) async {
    if (controller.snapshot.isConnected) {
      await controller.connectOrDisconnect();
      return;
    }
    final profile = controller.activeProfile;
    if (controller.identityState(profile.id) != ProfileIdentityState.ready) {
      final repaired = await showProfileIdentityDialog(
        context,
        controller: controller,
        profile: profile,
      );
      if (!repaired || !context.mounted) return;
    }
    await controller.connectOrDisconnect();
  }
}

/// Desktop and phone charts share timestamped observations, including zeros
/// and gaps. Widget rebuilds and unchanged values never alter the history.
class _TrafficGrid extends StatelessWidget {
  const _TrafficGrid({required this.controller, required this.strings});
  final AppController controller;
  final AppStrings strings;

  @override
  Widget build(BuildContext context) => ControllerSelector<bool>(
    controller: controller,
    selector: (app) => app.section == AppSection.home,
    builder: (context, active) => ListenableBuilder(
      listenable: Listenable.merge(
        active ? [controller, controller.quality] : [],
      ),
      builder: (context, _) {
        final tokens = UsqueTokens.of(context);
        final snapshot = controller.snapshot;
        final quality = controller.quality;
        final down = snapshot.isConnected
            ? quality.traceFor(NetworkQualityTrace.download)
            : const <int?>[];
        final up = snapshot.isConnected
            ? quality.traceFor(NetworkQualityTrace.upload)
            : const <int?>[];
        final note = strings.get(
          homeTrafficNoteKey(
            controller,
            hasSamples:
                down.any((value) => value != null) ||
                up.any((value) => value != null),
          ),
        );
        Widget readout(bool download) => _TrafficRate(
          direction: download ? 'download' : 'upload',
          icon: download ? LucideIcons.arrowDown : LucideIcons.arrowUp,
          label: strings.get(download ? 'download' : 'upload'),
          bytesPerSecond: !snapshot.isConnected
              ? null
              : download
              ? snapshot.downloadBytesPerSecond
              : snapshot.uploadBytesPerSecond,
          color: download ? tokens.inbound : tokens.outbound,
        );
        Widget trace(bool download) {
          final direction = download ? 'download' : 'upload';
          return Sparkline(
            key: ValueKey('home-desktop-$direction-trace'),
            samples: download ? down : up,
            color: download ? tokens.inbound : tokens.outbound,
            height: _minTraceHeight,
            semanticLabel: '${strings.get(direction)} · $note',
          );
        }

        final theme = Theme.of(context);
        final heading = ContentHeading(
          title: strings.get('home_traffic'),
          trailing: Text(
            note,
            style: theme.textTheme.bodySmall?.copyWith(
              color: theme.colorScheme.onSurfaceVariant,
            ),
          ),
        );
        return LayoutBuilder(
          builder: (context, constraints) {
            if (constraints.maxWidth < 560 ||
                MediaQuery.textScalerOf(context).scale(14) > 21) {
              return Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                mainAxisSize: MainAxisSize.min,
                children: [
                  heading,
                  const SizedBox(height: 16),
                  readout(true),
                  const SizedBox(height: 14),
                  trace(true),
                  const SizedBox(height: 24),
                  readout(false),
                  const SizedBox(height: 14),
                  trace(false),
                ],
              );
            }
            // The fixed trace height is a minimum here: the chart row
            // stretches to whatever height the page offers below it.
            return FillColumn(
              maxLastExtent: _maxTraceHeight,
              children: [
                heading,
                const SizedBox(height: 16),
                Row(
                  children: [
                    Expanded(child: readout(true)),
                    const SizedBox(width: _traceGap),
                    Expanded(child: readout(false)),
                  ],
                ),
                const SizedBox(height: 14),
                SizedBox(
                  height: _minTraceHeight,
                  child: Row(
                    crossAxisAlignment: CrossAxisAlignment.stretch,
                    children: [
                      Expanded(child: trace(true)),
                      const SizedBox(width: _traceGap),
                      Expanded(child: trace(false)),
                    ],
                  ),
                ),
              ],
            );
          },
        );
      },
    ),
  );

  static const double _minTraceHeight = 96;
  static const double _maxTraceHeight = 240;
  static const double _traceGap = 32;
}

class _TrafficRate extends StatelessWidget {
  const _TrafficRate({
    required this.direction,
    required this.icon,
    required this.label,
    required this.bytesPerSecond,
    required this.color,
  });
  final String direction;
  final IconData icon;
  final String label;
  final int? bytesPerSecond;
  final Color color;
  @override
  Widget build(BuildContext context) {
    final ThemeData theme = Theme.of(context);
    final rate = bytesPerSecond;
    return Row(
      children: <Widget>[
        Icon(icon, size: 20, color: color),
        const SizedBox(width: 12),
        Expanded(
          child: Text(
            label,
            style: theme.textTheme.bodyMedium?.copyWith(
              color: theme.colorScheme.onSurfaceVariant,
            ),
          ),
        ),
        const SizedBox(width: 10),
        Text(
          rate == null ? '—' : formatRate(rate),
          key: ValueKey('home-desktop-$direction-rate'),
          style: UsqueTheme.readout(
            context,
            size: 16,
            color: rate == null ? theme.colorScheme.onSurfaceVariant : null,
          ),
        ),
      ],
    );
  }
}

class _ProtectionSummary extends StatelessWidget {
  const _ProtectionSummary({required this.controller});
  final AppController controller;
  @override
  Widget build(BuildContext context) =>
      ControllerSelector<({String key, bool alwaysOn, bool lockdown})>(
        controller: controller,
        active: (app) => app.section == AppSection.home,
        selector: (app) => (
          key: killSwitchStatusKey(
            profile: app.activeProfile,
            snapshot: app.snapshot,
          ),
          alwaysOn: app.snapshot.alwaysOn,
          lockdown: app.snapshot.platformLockdown,
        ),
        builder: (context, view) {
          final strings = controller.strings;
          return Column(
            children: [
              ReadoutRow.text(
                context,
                icon: view.key == 'ks_active'
                    ? LucideIcons.shieldCheck
                    : view.key == 'ks_error'
                    ? LucideIcons.shieldAlert
                    : LucideIcons.shield,
                label: strings.get('home_kill_switch'),
                value: strings.get(view.key),
              ),
              if (view.alwaysOn) ...[
                const SizedBox(height: 12),
                ReadoutRow.text(
                  context,
                  icon: LucideIcons.shield,
                  label: strings.get('always_on'),
                  value: strings.get('on'),
                ),
              ],
              if (view.lockdown) ...[
                const SizedBox(height: 12),
                ReadoutRow.text(
                  context,
                  icon: LucideIcons.shieldBan,
                  label: strings.get('lockdown'),
                  value: strings.get('on'),
                ),
              ],
            ],
          );
        },
      );
}

typedef _DesktopSessionView = ({
  String catalogId,
  String? transport,
  String? family,
  DateTime? since,
  bool connected,
  String location,
  String? countryCode,
  String killKey,
  bool alwaysOn,
  bool lockdown,
});

class _DesktopSessionReadout extends StatelessWidget {
  const _DesktopSessionReadout({required this.controller});
  final AppController controller;

  @override
  Widget build(BuildContext context) => ControllerSelector<_DesktopSessionView>(
    controller: controller,
    active: (app) => app.section == AppSection.home,
    selector: (app) {
      final snapshot = app.snapshot;
      final strings = app.strings;
      return (
        catalogId: strings.catalogId,
        transport: snapshot.dataPlane == DataPlaneMode.l4Proxy
            ? 'L4 / H3'
            : snapshot.transport,
        family: snapshot.addressFamily,
        since: snapshot.connectedAt,
        connected: snapshot.isConnected,
        location: snapshot.exit.country?.trim().isNotEmpty == true
            ? snapshot.exit.country!.trim()
            : strings.get('not_available'),
        countryCode: snapshot.exit.countryCode,
        killKey: killSwitchStatusKey(
          profile: app.activeProfile,
          snapshot: snapshot,
        ),
        alwaysOn: snapshot.alwaysOn,
        lockdown: snapshot.platformLockdown,
      );
    },
    builder: (context, view) {
      final strings = controller.strings;
      final theme = Theme.of(context);
      final tokens = UsqueTokens.of(context);
      Widget metric(String label, Widget value) => Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text(
            label,
            style: theme.textTheme.bodySmall?.copyWith(
              color: theme.colorScheme.onSurfaceVariant,
            ),
          ),
          const SizedBox(height: 5),
          value,
        ],
      );
      return Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Wrap(
            spacing: 20,
            runSpacing: 12,
            children: [
              metric(
                strings.get('protocol'),
                view.transport == null
                    ? const EmptyValue(label: '—')
                    : Text(view.transport!, style: UsqueTheme.readout(context)),
              ),
              metric(
                strings.get('address_family'),
                view.family == null
                    ? const EmptyValue(label: '—')
                    : Text(view.family!, style: UsqueTheme.readout(context)),
              ),
              metric(strings.get('duration'), LiveDuration(since: view.since)),
            ],
          ),
          const SizedBox(height: 20),
          Wrap(
            crossAxisAlignment: WrapCrossAlignment.center,
            spacing: 8,
            runSpacing: 4,
            children: [
              Icon(
                view.killKey == 'ks_active'
                    ? LucideIcons.shieldCheck
                    : LucideIcons.shield,
                size: 16,
              ),
              Text(
                strings.get('kill_switch'),
                style: theme.textTheme.bodySmall,
              ),
              Text(
                strings.get(view.killKey),
                style: theme.textTheme.bodySmall?.copyWith(
                  color: view.killKey == 'ks_active'
                      ? tokens.success
                      : view.killKey == 'ks_error'
                      ? tokens.danger
                      : theme.colorScheme.onSurfaceVariant,
                ),
              ),
              if (view.alwaysOn)
                Text(
                  '${strings.get('always_on')} · ${strings.get('on')}',
                  style: theme.textTheme.bodySmall,
                ),
              if (view.lockdown)
                Text(
                  '${strings.get('lockdown')} · ${strings.get('on')}',
                  style: theme.textTheme.bodySmall,
                ),
            ],
          ),
          const SizedBox(height: 12),
          Row(
            key: const ValueKey('home-exit-location'),
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              const Icon(LucideIcons.mapPin, size: 16),
              const SizedBox(width: 8),
              Expanded(
                child: Row(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    if (view.connected) ...[
                      CountryFlag(countryCode: view.countryCode),
                      const SizedBox(width: 8),
                    ],
                    Expanded(
                      child: Text(
                        view.connected
                            ? view.location
                            : strings.get('location_disconnected'),
                        style: theme.textTheme.bodySmall?.copyWith(
                          color: theme.colorScheme.onSurfaceVariant,
                        ),
                      ),
                    ),
                  ],
                ),
              ),
            ],
          ),
        ],
      );
    },
  );
}

class _HomeDetails extends StatelessWidget {
  const _HomeDetails({
    required this.controller,
    required this.strings,
    this.compact = false,
  });
  final AppController controller;
  final AppStrings strings;
  final bool compact;

  // Leading geometry matches the neighbouring rows: ReadoutRow on phones and
  // the Kill Switch/location rows on desktop.
  @override
  Widget build(BuildContext context) => Material(
    color: Colors.transparent,
    child: ListTileTheme.merge(
      minLeadingWidth: compact ? 22 : 16,
      horizontalTitleGap: compact ? 11 : 8,
      child: ExpansionTile(
        key: const PageStorageKey('home-connection-details'),
        leading: compact
            ? Icon(
                LucideIcons.slidersHorizontal,
                size: 17,
                color: Theme.of(context).colorScheme.onSurfaceVariant,
              )
            : const Icon(LucideIcons.slidersHorizontal, size: 16),
        title: Text(
          strings.get('connection_details'),
          style: Theme.of(context).textTheme.bodyMedium,
        ),
        dense: true,
        minTileHeight: 48,
        tilePadding: EdgeInsets.zero,
        shape: const Border(),
        collapsedShape: const Border(),
        children: [_ConnectionDetailsReadout(controller: controller)],
      ),
    ),
  );
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
        padding: const EdgeInsets.symmetric(vertical: 8),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Wrap(
              key: const ValueKey('home-exit-ip'),
              spacing: 8,
              runSpacing: 4,
              crossAxisAlignment: WrapCrossAlignment.center,
              children: [
                Text(
                  strings.get('home_exit_ip'),
                  style: theme.textTheme.bodyMedium,
                ),
                if (addresses.isEmpty)
                  const EmptyValue(label: '—')
                else
                  for (final value in addresses) MonoValue(value: value),
              ],
            ),
            if (interfaces.isNotEmpty) ...[
              const SizedBox(height: 16),
              Text(
                strings
                    .get('home_enabled_interfaces')
                    .replaceAll('{interfaces}', interfaces.join(separator)),
                key: const ValueKey('home-enabled-interfaces'),
                style: theme.textTheme.bodyMedium,
              ),
            ],
          ],
        ),
      );
    },
  );
}

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
    case 'inactive':
    case 'notApplicable':
    case 'not_applicable':
      return snapshot.isTransitional ? 'ks_engaging' : 'ks_inactive';
    default:
      return snapshot.isTransitional ? 'ks_engaging' : 'ks_inactive';
  }
}
