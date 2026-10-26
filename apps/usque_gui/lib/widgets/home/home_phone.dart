import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../../core/app_strings.dart';
import '../../core/chain_home_status.dart';
import '../../core/chain_scope_presentation.dart';
import '../../core/usque_theme.dart';
import '../../models/app_models.dart';
import '../../state/app_controller.dart';
import '../chain_exit_summary.dart';
import '../chain_source_icon.dart';
import '../common.dart';
import '../controller_selector.dart';
import '../usque_logo.dart';
import 'home_details.dart';

/// Phone page heading: the brand and the active account, which opens
/// Accounts.
class HomePhoneHeader extends StatelessWidget {
  const HomePhoneHeader({required this.controller, super.key});

  final AppController controller;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return Row(
      children: [
        Semantics(
          label: 'Usque',
          header: true,
          excludeSemantics: true,
          child: Row(
            mainAxisSize: MainAxisSize.min,
            children: [
              const UsqueLogo(size: 40),
              const SizedBox(width: UsqueSpace.s),
              Text('Usque', style: theme.textTheme.titleLarge),
            ],
          ),
        ),
        const SizedBox(width: UsqueSpace.m),
        Expanded(
          child: Align(
            alignment: AlignmentDirectional.centerEnd,
            child: ControllerSelector<(String, String)>(
              controller: controller,
              active: (app) => app.section == AppSection.home,
              selector: (app) =>
                  (app.activeProfile.name, app.strings.catalogId),
              builder: (context, view) =>
                  _AccountChip(controller: controller, name: view.$1),
            ),
          ),
        ),
      ],
    );
  }
}

class _AccountChip extends StatelessWidget {
  const _AccountChip({required this.controller, required this.name});

  final AppController controller;
  final String name;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final label = '${controller.strings.get('active_profile')}: $name';
    return Tooltip(
      message: label,
      child: Semantics(
        button: true,
        label: label,
        excludeSemantics: true,
        child: Material(
          key: const ValueKey('home-account-chip'),
          color: theme.colorScheme.surfaceContainerHigh,
          shape: const StadiumBorder(),
          clipBehavior: Clip.antiAlias,
          child: InkWell(
            onTap: () => controller.selectSection(AppSection.profiles),
            child: ConstrainedBox(
              constraints: const BoxConstraints(minHeight: 40),
              child: Padding(
                padding: const EdgeInsetsDirectional.fromSTEB(10, 6, 14, 6),
                child: Row(
                  mainAxisSize: MainAxisSize.min,
                  children: [
                    Icon(
                      LucideIcons.circleUserRound,
                      size: 18,
                      color: theme.colorScheme.onSurfaceVariant,
                    ),
                    const SizedBox(width: UsqueSpace.xs - 2),
                    Flexible(
                      child: Text(
                        name,
                        maxLines: 1,
                        overflow: TextOverflow.ellipsis,
                        style: theme.textTheme.labelLarge?.copyWith(
                          fontWeight: FontWeight.w600,
                        ),
                      ),
                    ),
                  ],
                ),
              ),
            ),
          ),
        ),
      ),
    );
  }
}

/// Phone summary of the chain above the connection control. The ring and
/// headline below already carry the overall status, so this names the path,
/// the chosen exit and the WARP hop only.
class HomeChainSummary extends StatelessWidget {
  const HomeChainSummary({
    required this.controller,
    required this.strings,
    required this.onOpen,
    super.key,
  });
  final AppController controller;
  final AppStrings strings;
  final VoidCallback onOpen;

  @override
  Widget build(BuildContext context) =>
      ControllerSelector<
        ({
          bool show,
          ConnectionPhase phase,
          String chainStage,
          String gateStage,
          String? reportedWarpStage,
        })
      >(
        controller: controller,
        active: (app) => app.section == AppSection.home,
        selector: (app) {
          final snapshot = app.snapshot;
          final homeStatus = ChainHomeStatus.of(
            phase: snapshot.phase,
            chainEnabled: app.activeProfile.chainEnabled,
            chainStage: snapshot.chainExit.stage,
            gateStage: snapshot.vpnGate.stage,
            hasCurrentProfile: snapshot.chainExit.currentProfile != null,
            hasGateServer: snapshot.vpnGate.server != null,
          );
          final scope = ChainScopePresentation.of(
            snapshot,
            app.networkSettings.state?.appliedProfile,
            isAndroid: defaultTargetPlatform == TargetPlatform.android,
          );
          return (
            show: homeStatus.showChainRow || !scope.isEmpty,
            phase: snapshot.phase,
            chainStage: snapshot.chainExit.stage,
            gateStage: snapshot.vpnGate.stage,
            reportedWarpStage:
                snapshot.chainExit.warpStage ?? snapshot.vpnGate.warpStage,
          );
        },
        builder: (context, view) {
          if (!view.show) return const SizedBox.shrink();
          final theme = Theme.of(context);
          final muted = theme.colorScheme.onSurfaceVariant;
          final warpKey = ChainHomeStatus.warpLabelKey(
            phase: view.phase,
            chainStage: view.chainStage,
            gateStage: view.gateStage,
            reportedStage: view.reportedWarpStage,
          );
          return ChainExitSummaryBuilder(
            controller: controller,
            section: AppSection.home,
            builder: (context, chain) => Padding(
              padding: const EdgeInsets.only(bottom: UsqueSpace.l),
              child: Semantics(
                container: true,
                liveRegion: true,
                child: Panel(
                  key: const ValueKey('home-vpn-gate-settings'),
                  onTap: onOpen,
                  padding: const EdgeInsetsDirectional.fromSTEB(16, 14, 12, 14),
                  child: Row(
                    children: [
                      ChainSourceIcon(source: chain.source, size: 20),
                      const SizedBox(width: UsqueSpace.s),
                      Expanded(
                        child: Column(
                          crossAxisAlignment: CrossAxisAlignment.start,
                          children: [
                            Text(
                              'WARP → ${chain.source.label}',
                              style: theme.textTheme.titleSmall,
                            ),
                            if (chain.selection != null) ...[
                              const SizedBox(height: 2),
                              ChainSelectionText(
                                chain,
                                key: const ValueKey('home-chain-selection'),
                                showSource: false,
                                style: theme.textTheme.bodyMedium,
                              ),
                            ],
                            const SizedBox(height: 2),
                            Text(
                              'WARP: ${warpKey == null ? '—' : strings.get(warpKey)}',
                              key: const ValueKey('home-chain-warp-status'),
                              style: theme.textTheme.bodySmall?.copyWith(
                                color: muted,
                              ),
                            ),
                          ],
                        ),
                      ),
                      const SizedBox(width: UsqueSpace.xs),
                      Icon(LucideIcons.chevronRightDir, size: 18, color: muted),
                    ],
                  ),
                ),
              ),
            ),
          );
        },
      );
}

typedef _StatusView = ({
  String killKey,
  bool alwaysOn,
  bool lockdown,
  bool connected,
  String? transport,
  String? family,
  FrontendSettings outputs,
  bool systemProxy,
});

/// Protection, the session protocol or the outputs a connection will use,
/// and the folded connection details, in one card under the traffic.
class HomeStatusCard extends StatelessWidget {
  const HomeStatusCard({
    required this.controller,
    required this.strings,
    super.key,
  });

  final AppController controller;
  final AppStrings strings;

  @override
  Widget build(BuildContext context) => ControllerSelector<_StatusView>(
    controller: controller,
    active: (app) => app.section == AppSection.home,
    selector: (app) => (
      killKey: killSwitchStatusKey(
        profile: app.activeProfile,
        snapshot: app.snapshot,
      ),
      alwaysOn: app.snapshot.alwaysOn,
      lockdown: app.snapshot.platformLockdown,
      connected: app.snapshot.isConnected,
      transport: app.snapshot.transport,
      family: app.snapshot.addressFamily,
      outputs: app.activeProfile.frontends,
      systemProxy: app.activeProfile.proxy.systemProxy,
    ),
    builder: _build,
  );

  Widget _build(BuildContext context, _StatusView view) {
    final theme = Theme.of(context);
    final (killIcon, killTone) = killSwitchPresentation(view.killKey);
    final protocol = [
      view.transport,
      view.family,
    ].whereType<String>().where((value) => value.isNotEmpty).join(' · ');
    Widget value(String text, {Color? color}) => Text(
      text,
      textAlign: TextAlign.end,
      style: theme.textTheme.titleSmall?.copyWith(color: color),
    );
    final rows = <Widget>[
      ReadoutRow(
        key: const ValueKey('home-kill-switch'),
        label: strings.get('home_kill_switch'),
        leading: Icon(
          killIcon,
          size: 18,
          color: statusToneColor(context, killTone),
        ),
        value: value(
          strings.get(view.killKey),
          color: killTone == StatusTone.neutral
              ? null
              : statusToneColor(context, killTone),
        ),
      ),
      if (view.alwaysOn)
        ReadoutRow(
          label: strings.get('always_on'),
          icon: LucideIcons.shield,
          value: value(strings.get('on')),
        ),
      if (view.lockdown)
        ReadoutRow(
          label: strings.get('lockdown'),
          icon: LucideIcons.shieldBan,
          value: value(strings.get('on')),
        ),
      if (view.connected)
        ReadoutRow(
          key: const ValueKey('home-protocol'),
          label: strings.get('protocol'),
          icon: LucideIcons.activity,
          value: Text(
            protocol.isEmpty ? strings.get('not_available') : protocol,
            textAlign: TextAlign.end,
            style: UsqueTheme.readout(context, size: 14),
          ),
        )
      else
        // Idle and failed sessions both list what the next connection
        // will enable.
        Column(
          key: const ValueKey('home-next-outputs'),
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text(
              strings.get('home_outputs_retry'),
              style: theme.textTheme.bodyMedium?.copyWith(
                color: theme.colorScheme.onSurfaceVariant,
              ),
            ),
            const SizedBox(height: UsqueSpace.xs),
            if (!view.outputs.any && !view.systemProxy)
              Text(strings.get('channel_only_warning'))
            else
              Wrap(
                spacing: UsqueSpace.xs,
                runSpacing: UsqueSpace.xs,
                children: [
                  if (view.outputs.tunnel)
                    _OutputTag(strings.tunnelOutputLabel(theme.platform)),
                  if (view.outputs.socks5) const _OutputTag('SOCKS5'),
                  if (view.outputs.http) const _OutputTag('HTTP'),
                  if (view.systemProxy)
                    _OutputTag(strings.get('home_system_proxy')),
                ],
              ),
          ],
        ),
    ];
    return Panel(
      key: const ValueKey('home-status'),
      padding: const EdgeInsetsDirectional.fromSTEB(20, 18, 20, 4),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          for (var i = 0; i < rows.length; i++) ...[
            if (i > 0) const SizedBox(height: UsqueSpace.m),
            rows[i],
          ],
          const SizedBox(height: UsqueSpace.s),
          Divider(height: 1, color: UsqueTokens.of(context).hairline),
          HomeDetails(controller: controller, strings: strings),
        ],
      ),
    );
  }
}

/// A configured output named in a quiet tag.
class _OutputTag extends StatelessWidget {
  const _OutputTag(this.label);

  final String label;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return DecoratedBox(
      decoration: BoxDecoration(
        color: theme.colorScheme.surfaceContainerHigh,
        borderRadius: BorderRadius.circular(UsqueRadii.chip - 2),
      ),
      child: Padding(
        padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 4),
        child: Text(
          label,
          style: theme.textTheme.labelMedium?.copyWith(
            color: theme.colorScheme.onSurface,
            fontWeight: FontWeight.w600,
          ),
        ),
      ),
    );
  }
}
