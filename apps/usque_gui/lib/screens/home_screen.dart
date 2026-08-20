import 'package:flutter/material.dart';
import 'package:flutter_svg/flutter_svg.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../core/app_strings.dart';
import '../core/usque_motion.dart';
import '../core/usque_theme.dart';
import '../models/app_models.dart';
import '../state/app_controller.dart';
import '../widgets/common.dart';
import '../widgets/connection_ring.dart';
import '../widgets/controller_selector.dart';
import '../widgets/profile_identity_dialog.dart';
import '../widgets/sparkline.dart';

/// The instrument panel: one connection control, one status readout, and the
/// live numbers that prove the tunnel is doing something.
///
/// Each block subscribes to its own slice of the controller, so a traffic
/// sample arriving every second repaints two counters instead of the page.
class HomeScreen extends StatelessWidget {
  const HomeScreen({required this.controller, super.key});

  final AppController controller;

  /// Below this the hero and the readout stack instead of sitting side by side.
  static const double _splitWidth = 820;

  @override
  Widget build(BuildContext context) {
    final AppStrings strings = controller.strings;
    return PageFrame(
      title: strings.get('home'),
      header: MediaQuery.sizeOf(context).width < 760
          ? const _NarrowBrandHeader()
          : null,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: <Widget>[
          _ErrorSlot(controller: controller, strings: strings),
          LayoutBuilder(
            builder: (context, constraints) {
              final Widget hero = _ConnectionHero(
                controller: controller,
                strings: strings,
              );
              final Widget readout = _EngineReadout(
                controller: controller,
                strings: strings,
              );
              if (constraints.maxWidth >= _splitWidth) {
                return Row(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: <Widget>[
                    Expanded(flex: 5, child: hero),
                    const SizedBox(width: 16),
                    Expanded(flex: 4, child: readout),
                  ],
                );
              }
              return Column(
                children: <Widget>[hero, const SizedBox(height: 16), readout],
              );
            },
          ),
          const SizedBox(height: 16),
          _TrafficGrid(controller: controller, strings: strings),
          const SizedBox(height: 16),
          _ExitPanel(controller: controller, strings: strings),
        ],
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
          Image.asset(
            'assets/branding/usque-ui-icon.png',
            width: 40,
            height: 40,
            filterQuality: FilterQuality.medium,
          ),
          const SizedBox(width: 12),
          Text('Usque', style: Theme.of(context).textTheme.titleLarge),
        ],
      ),
    );
  }
}

class _ErrorSlot extends StatelessWidget {
  const _ErrorSlot({required this.controller, required this.strings});

  final AppController controller;
  final AppStrings strings;

  @override
  Widget build(BuildContext context) {
    return ControllerSelector<String?>(
      controller: controller,
      active: (controller) => controller.section == AppSection.home,
      selector: (controller) => controller.lastError,
      builder: (context, error) => BannerSlot(
        child: error == null
            ? null
            : WarningBanner(
                title: strings.get('error'),
                message: error,
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
  String profileName,
  FrontendSettings frontends,
  bool systemProxy,
  _OutputPhases runtime,
});

/// Runtime phase of each output, flattened so the hero compares by value and
/// ignores the new list object that arrives with every traffic sample.
typedef _OutputPhases = ({
  FrontendPhase? tunnel,
  FrontendPhase? socks5,
  FrontendPhase? http,
  FrontendPhase? systemProxy,
});

_OutputPhases _outputPhases(EngineSnapshot snapshot) {
  FrontendPhase? phaseOf(FrontendKind kind) {
    for (final status in snapshot.frontends) {
      if (status.kind == kind) {
        return status.phase;
      }
    }
    return null;
  }

  return (
    tunnel: phaseOf(FrontendKind.tunnel),
    socks5: phaseOf(FrontendKind.socks5),
    http: phaseOf(FrontendKind.http),
    systemProxy: phaseOf(FrontendKind.systemProxy),
  );
}

class _ConnectionHero extends StatelessWidget {
  const _ConnectionHero({required this.controller, required this.strings});

  final AppController controller;
  final AppStrings strings;

  @override
  Widget build(BuildContext context) {
    return ControllerSelector<_HeroView>(
      controller: controller,
      active: (controller) => controller.section == AppSection.home,
      selector: (controller) => (
        phase: controller.snapshot.phase,
        busy: controller.busy,
        profileName: controller.activeProfile.name,
        frontends: controller.activeProfile.frontends,
        systemProxy: controller.activeProfile.proxy.systemProxy,
        runtime: _outputPhases(controller.snapshot),
      ),
      builder: (context, view) => _buildHero(context, view),
    );
  }

  Widget _buildHero(BuildContext context, _HeroView view) {
    final ThemeData theme = Theme.of(context);
    final bool connected =
        view.phase == ConnectionPhase.connected ||
        view.phase == ConnectionPhase.degraded;
    final bool transitional = switch (view.phase) {
      ConnectionPhase.preparing ||
      ConnectionPhase.connectingH3 ||
      ConnectionPhase.connectingH2 ||
      ConnectionPhase.reconnecting ||
      ConnectionPhase.disconnecting => true,
      _ => false,
    };
    final String status = phaseLabel(strings, view.phase);
    final String action = strings.get(
      connected
          ? 'disconnect'
          : transitional
          ? 'connecting'
          : 'connect',
    );
    final bool recoverable =
        view.phase == ConnectionPhase.error ||
        view.phase == ConnectionPhase.degraded;

    return Panel(
      padding: const EdgeInsets.fromLTRB(24, 22, 24, 24),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: <Widget>[
          Text(
            strings.get('active_profile').toUpperCase(),
            style: theme.textTheme.labelSmall?.copyWith(
              color: theme.colorScheme.onSurfaceVariant,
              letterSpacing: 1.1,
            ),
          ),
          const SizedBox(height: 5),
          Text(
            view.profileName,
            maxLines: 1,
            overflow: TextOverflow.ellipsis,
            style: theme.textTheme.titleLarge,
          ),
          const SizedBox(height: 22),
          Center(
            child: LayoutBuilder(
              builder: (context, constraints) => ConnectionRing(
                phase: view.phase,
                busy: view.busy,
                actionLabel: action,
                semanticLabel: '${strings.get('connection_status')}: $status',
                size: constraints.maxWidth.clamp(180, 244).toDouble(),
                onPressed: view.busy
                    ? null
                    : () => _connectOrRepairIdentity(context),
              ),
            ),
          ),
          const SizedBox(height: 20),
          Center(
            child: FadeThroughSwitcher(
              child: Text(
                status,
                key: ValueKey<ConnectionPhase>(view.phase),
                textAlign: TextAlign.center,
                style: theme.textTheme.headlineSmall,
              ),
            ),
          ),
          if (recoverable) ...<Widget>[
            const SizedBox(height: 16),
            OutlinedButton.icon(
              onPressed: view.busy ? null : controller.retry,
              icon: const Icon(LucideIcons.refreshCw),
              label: Text(strings.get('retry')),
            ),
          ],
          const SizedBox(height: 22),
          Divider(height: 1, color: UsqueTokens.of(context).hairline),
          const SizedBox(height: 18),
          Text(
            strings.get('outputs').toUpperCase(),
            style: theme.textTheme.labelSmall?.copyWith(
              color: theme.colorScheme.onSurfaceVariant,
              letterSpacing: 1.1,
            ),
          ),
          const SizedBox(height: 12),
          _FrontendChips(view: view, strings: strings),
        ],
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

class _FrontendChips extends StatelessWidget {
  const _FrontendChips({required this.view, required this.strings});

  final _HeroView view;
  final AppStrings strings;

  @override
  Widget build(BuildContext context) {
    final configured = <FrontendKind, FrontendPhase?>{
      if (view.frontends.tunnel) FrontendKind.tunnel: view.runtime.tunnel,
      if (view.frontends.socks5) FrontendKind.socks5: view.runtime.socks5,
      if (view.frontends.http) FrontendKind.http: view.runtime.http,
      if (view.systemProxy) FrontendKind.systemProxy: view.runtime.systemProxy,
    };
    final enabled = configured.entries.toList(growable: false);
    if (enabled.isEmpty) {
      return Text(
        strings.get('channel_only_warning'),
        style: Theme.of(context).textTheme.bodyMedium?.copyWith(
          color: Theme.of(context).colorScheme.onSurfaceVariant,
        ),
      );
    }
    return Wrap(
      spacing: 8,
      runSpacing: 8,
      children: enabled
          .map((entry) {
            final FrontendPhase? phase = entry.value;
            final bool active = phase == FrontendPhase.active;
            final bool degraded =
                phase == FrontendPhase.degraded ||
                phase == FrontendPhase.reconnecting ||
                phase == FrontendPhase.error;
            return StatusPill(
              label: switch (entry.key) {
                FrontendKind.tunnel => strings.get('tunnel_output'),
                FrontendKind.socks5 => 'SOCKS5',
                FrontendKind.http => 'HTTP',
                FrontendKind.systemProxy => strings.get('system_proxy'),
              },
              tone: degraded
                  ? StatusTone.warning
                  : active
                  ? StatusTone.success
                  : StatusTone.neutral,
              dim: !active && !degraded,
            );
          })
          .toList(growable: false),
    );
  }
}

typedef _ReadoutView = ({
  String? transport,
  String? addressFamily,
  DateTime? connectedAt,
  bool alwaysOn,
  bool platformLockdown,
  String killSwitchLabel,
});

class _EngineReadout extends StatelessWidget {
  const _EngineReadout({required this.controller, required this.strings});

  final AppController controller;
  final AppStrings strings;

  @override
  Widget build(BuildContext context) {
    return ControllerSelector<_ReadoutView>(
      controller: controller,
      active: (controller) => controller.section == AppSection.home,
      selector: (controller) {
        final EngineSnapshot snapshot = controller.snapshot;
        return (
          transport: snapshot.transport,
          addressFamily: snapshot.addressFamily,
          connectedAt: snapshot.connectedAt,
          alwaysOn: snapshot.alwaysOn,
          platformLockdown: snapshot.platformLockdown,
          killSwitchLabel: strings.get(
            killSwitchStatusKey(
              profile: controller.activeProfile,
              snapshot: snapshot,
            ),
          ),
        );
      },
      builder: (context, view) => _buildReadout(context, view),
    );
  }

  Widget _buildReadout(BuildContext context, _ReadoutView view) {
    final Color hairline = UsqueTokens.of(context).hairline;
    final Duration? uptime = view.connectedAt == null
        ? null
        : DateTime.now().difference(view.connectedAt!);
    final Widget divider = Padding(
      padding: const EdgeInsets.symmetric(vertical: 14),
      child: Divider(height: 1, color: hairline),
    );

    return Panel(
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: <Widget>[
          SectionTitle(
            icon: LucideIcons.activity,
            title: strings.get('engine_status'),
          ),
          const SizedBox(height: 20),
          ReadoutRow(
            icon: LucideIcons.cable,
            label: strings.get('protocol'),
            value: view.transport == null
                ? const EmptyValue(label: '—')
                : MonoValue(value: view.transport!),
          ),
          divider,
          ReadoutRow(
            icon: LucideIcons.network,
            label: strings.get('address_family'),
            value: view.addressFamily == null
                ? const EmptyValue(label: '—')
                : MonoValue(value: view.addressFamily!),
          ),
          divider,
          ReadoutRow(
            icon: LucideIcons.clock3,
            label: strings.get('duration'),
            value: uptime == null
                ? const EmptyValue(label: '—')
                : MonoValue(value: formatDuration(uptime)),
          ),
          divider,
          ReadoutRow.text(
            context,
            icon: LucideIcons.shieldCheck,
            label: strings.get('kill_switch'),
            value: view.killSwitchLabel,
          ),
          if (view.alwaysOn) ...<Widget>[
            divider,
            ReadoutRow.text(
              context,
              icon: LucideIcons.shield,
              label: strings.get('always_on'),
              value: strings.get('on'),
            ),
          ],
          if (view.platformLockdown) ...<Widget>[
            divider,
            ReadoutRow.text(
              context,
              icon: LucideIcons.shieldBan,
              label: strings.get('lockdown'),
              value: strings.get('on'),
            ),
          ],
        ],
      ),
    );
  }
}

typedef _TrafficView = ({int down, int up, bool live});

class _TrafficGrid extends StatelessWidget {
  const _TrafficGrid({required this.controller, required this.strings});

  final AppController controller;
  final AppStrings strings;

  @override
  Widget build(BuildContext context) {
    return ControllerSelector<_TrafficView>(
      controller: controller,
      active: (controller) => controller.section == AppSection.home,
      selector: (controller) => (
        down: controller.snapshot.downloadBytesPerSecond,
        up: controller.snapshot.uploadBytesPerSecond,
        live: controller.snapshot.isConnected,
      ),
      builder: (context, view) {
        final UsqueTokens tokens = UsqueTokens.of(context);
        final Widget download = _MetricCard(
          icon: LucideIcons.arrowDown,
          label: strings.get('download'),
          bytesPerSecond: view.down,
          color: tokens.inbound,
          live: view.live,
        );
        final Widget upload = _MetricCard(
          icon: LucideIcons.arrowUp,
          label: strings.get('upload'),
          bytesPerSecond: view.up,
          color: tokens.outbound,
          live: view.live,
        );
        return LayoutBuilder(
          builder: (context, constraints) {
            if (constraints.maxWidth >= 560) {
              return Row(
                children: <Widget>[
                  Expanded(child: download),
                  const SizedBox(width: 16),
                  Expanded(child: upload),
                ],
              );
            }
            return Column(
              children: <Widget>[download, const SizedBox(height: 16), upload],
            );
          },
        );
      },
    );
  }
}

/// One traffic direction: a badge, the current rate, and the recent shape of
/// that rate.
class _MetricCard extends StatefulWidget {
  const _MetricCard({
    required this.icon,
    required this.label,
    required this.bytesPerSecond,
    required this.color,
    required this.live,
  });

  final IconData icon;
  final String label;
  final int bytesPerSecond;
  final Color color;

  /// History only accumulates while the tunnel is up; a disconnect clears it.
  final bool live;

  @override
  State<_MetricCard> createState() => _MetricCardState();
}

class _MetricCardState extends State<_MetricCard> {
  static const int _window = 48;

  final List<int> _history = <int>[];

  @override
  void initState() {
    super.initState();
    _record();
  }

  @override
  void didUpdateWidget(covariant _MetricCard oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (!widget.live) {
      _history.clear();
      return;
    }
    if (oldWidget.bytesPerSecond != widget.bytesPerSecond || !oldWidget.live) {
      _record();
    }
  }

  void _record() {
    if (!widget.live) {
      return;
    }
    _history.add(widget.bytesPerSecond);
    if (_history.length > _window) {
      _history.removeRange(0, _history.length - _window);
    }
  }

  @override
  Widget build(BuildContext context) {
    final ThemeData theme = Theme.of(context);
    return Panel(
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: <Widget>[
          Row(
            children: <Widget>[
              Container(
                width: 34,
                height: 34,
                alignment: Alignment.center,
                decoration: BoxDecoration(
                  color: widget.color.withValues(
                    alpha: UsqueTokens.of(context).tint,
                  ),
                  borderRadius: BorderRadius.circular(UsqueRadii.chip),
                ),
                child: Icon(widget.icon, size: 17, color: widget.color),
              ),
              const SizedBox(width: 12),
              Expanded(
                child: Text(
                  widget.label,
                  style: theme.textTheme.bodyMedium?.copyWith(
                    color: theme.colorScheme.onSurfaceVariant,
                  ),
                ),
              ),
              const SizedBox(width: 10),
              Text(
                formatRate(widget.bytesPerSecond),
                style: UsqueTheme.mono(
                  context,
                  size: theme.textTheme.titleMedium?.fontSize,
                  weight: FontWeight.w500,
                ),
              ),
            ],
          ),
          const SizedBox(height: 14),
          Sparkline(samples: List<int>.of(_history), color: widget.color),
        ],
      ),
    );
  }
}

class _ExitPanel extends StatelessWidget {
  const _ExitPanel({required this.controller, required this.strings});

  final AppController controller;
  final AppStrings strings;

  @override
  Widget build(BuildContext context) {
    return ControllerSelector<({ExitInfo exit, bool connected})>(
      controller: controller,
      active: (controller) => controller.section == AppSection.home,
      selector: (controller) => (
        exit: controller.snapshot.exit,
        connected: controller.snapshot.isConnected,
      ),
      builder: (context, view) =>
          _buildExit(context, view.exit, view.connected),
    );
  }

  Widget _buildExit(BuildContext context, ExitInfo exit, bool connected) {
    final Color hairline = UsqueTokens.of(context).hairline;
    final String missing = strings.get('not_available');
    final Widget divider = Padding(
      padding: const EdgeInsets.symmetric(vertical: 14),
      child: Divider(height: 1, color: hairline),
    );

    Widget flag;
    if (exit.flagSvg case final svg? when svg.isNotEmpty) {
      flag = ClipRRect(
        borderRadius: BorderRadius.circular(3),
        child: SvgPicture.string(svg, width: 22, height: 16, fit: BoxFit.cover),
      );
    } else {
      flag = Icon(
        LucideIcons.mapPin,
        size: 17,
        color: Theme.of(context).colorScheme.onSurfaceVariant,
      );
    }

    return Panel(
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: <Widget>[
          SectionTitle(
            icon: LucideIcons.globe2,
            title: strings.get('location'),
            subtitle: connected ? 'ip.sb' : missing,
          ),
          const SizedBox(height: 20),
          ReadoutRow(
            leading: flag,
            label: strings.get('location'),
            value: exit.hasLocation
                ? Text(
                    exit.location,
                    textAlign: TextAlign.end,
                    style: Theme.of(context).textTheme.titleSmall,
                  )
                : EmptyValue(label: missing),
          ),
          divider,
          ReadoutRow(
            icon: LucideIcons.network,
            label: strings.get('ipv4'),
            value: exit.ipv4 == null
                ? EmptyValue(label: missing)
                : MonoValue(value: exit.ipv4!),
          ),
          divider,
          ReadoutRow(
            icon: LucideIcons.network,
            label: strings.get('ipv6'),
            value: exit.ipv6 == null
                ? EmptyValue(label: missing)
                : MonoValue(value: exit.ipv6!),
          ),
        ],
      ),
    );
  }
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

String phaseLabel(AppStrings strings, ConnectionPhase phase) {
  return strings.get(switch (phase) {
    ConnectionPhase.disconnected => 'disconnected',
    ConnectionPhase.preparing => 'preparing',
    ConnectionPhase.connectingH3 ||
    ConnectionPhase.connectingH2 => 'connecting',
    ConnectionPhase.connected => 'connected',
    ConnectionPhase.degraded => 'degraded',
    ConnectionPhase.reconnecting => 'reconnecting',
    ConnectionPhase.disconnecting => 'disconnecting',
    ConnectionPhase.error => 'error',
  });
}
