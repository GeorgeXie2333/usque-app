import 'package:flutter/material.dart';
import 'package:flutter_svg/flutter_svg.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../core/app_strings.dart';
import '../core/usque_theme.dart';
import '../models/app_models.dart';
import '../state/app_controller.dart';
import '../widgets/common.dart';
import '../widgets/profile_identity_dialog.dart';

class HomeScreen extends StatelessWidget {
  const HomeScreen({required this.controller, super.key});

  final AppController controller;

  @override
  Widget build(BuildContext context) {
    final strings = controller.strings;
    final snapshot = controller.snapshot;
    return PageFrame(
      title: strings.get('home'),
      subtitle: strings.get('tagline'),
      header: MediaQuery.sizeOf(context).width < 760
          ? const _NarrowBrandHeader()
          : null,
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
          LayoutBuilder(
            builder: (context, constraints) {
              final sideBySide = constraints.maxWidth >= 820;
              final hero = _ConnectionHero(
                controller: controller,
                strings: strings,
              );
              final details = _ConnectionDetails(
                snapshot: snapshot,
                mode: controller.activeProfile.mode,
                strings: strings,
              );
              if (sideBySide) {
                return Row(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: <Widget>[
                    Expanded(flex: 5, child: hero),
                    const SizedBox(width: 16),
                    Expanded(flex: 4, child: details),
                  ],
                );
              }
              return Column(
                children: <Widget>[hero, const SizedBox(height: 16), details],
              );
            },
          ),
          const SizedBox(height: 16),
          _TrafficGrid(snapshot: snapshot, strings: strings),
          const SizedBox(height: 16),
          _ExitPanel(snapshot: snapshot, strings: strings),
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

class _ConnectionHero extends StatelessWidget {
  const _ConnectionHero({required this.controller, required this.strings});

  final AppController controller;
  final AppStrings strings;

  @override
  Widget build(BuildContext context) {
    final snapshot = controller.snapshot;
    final status = _phaseLabel(strings, snapshot.phase);
    final connected = snapshot.isConnected;
    final accent = connected
        ? UsqueColors.success
        : snapshot.phase == ConnectionPhase.error
        ? Theme.of(context).colorScheme.error
        : Theme.of(context).colorScheme.secondary;
    return Semantics(
      container: true,
      label: status,
      child: Card(
        clipBehavior: Clip.antiAlias,
        child: DecoratedBox(
          decoration: BoxDecoration(
            gradient: LinearGradient(
              begin: Alignment.topLeft,
              end: Alignment.bottomRight,
              colors: <Color>[
                accent.withValues(alpha: 0.14),
                Theme.of(context).colorScheme.surface,
              ],
            ),
          ),
          child: Padding(
            padding: const EdgeInsets.all(28),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: <Widget>[
                Row(
                  children: <Widget>[
                    Expanded(
                      child: Text(
                        strings.get('active_profile'),
                        style: Theme.of(context).textTheme.labelLarge?.copyWith(
                          color: Theme.of(context).colorScheme.onSurfaceVariant,
                        ),
                      ),
                    ),
                    StatusPill(
                      label: status,
                      tone: connected
                          ? snapshot.phase == ConnectionPhase.degraded
                                ? StatusTone.warning
                                : StatusTone.success
                          : snapshot.phase == ConnectionPhase.error
                          ? StatusTone.danger
                          : StatusTone.neutral,
                      icon: connected
                          ? LucideIcons.shieldCheck
                          : snapshot.phase == ConnectionPhase.error
                          ? LucideIcons.circleX
                          : LucideIcons.shield,
                    ),
                  ],
                ),
                const SizedBox(height: 18),
                Text(
                  controller.activeProfile.name,
                  style: Theme.of(context).textTheme.displaySmall,
                ),
                const SizedBox(height: 8),
                Text(
                  _modeLabel(strings, controller.activeProfile.mode),
                  style: Theme.of(context).textTheme.bodyLarge?.copyWith(
                    color: Theme.of(context).colorScheme.onSurfaceVariant,
                  ),
                ),
                const SizedBox(height: 30),
                SizedBox(
                  width: double.infinity,
                  child: FilledButton.icon(
                    onPressed: controller.busy
                        ? null
                        : () => _connectOrRepairIdentity(context),
                    style: FilledButton.styleFrom(
                      minimumSize: const Size.fromHeight(58),
                      backgroundColor: connected
                          ? Theme.of(
                              context,
                            ).colorScheme.surfaceContainerHighest
                          : null,
                      foregroundColor: connected
                          ? Theme.of(context).colorScheme.onSurface
                          : null,
                    ),
                    icon: controller.busy
                        ? const SizedBox(
                            width: 20,
                            height: 20,
                            child: CircularProgressIndicator(
                              strokeWidth: 2.4,
                              color: Colors.white,
                            ),
                          )
                        : Icon(
                            connected
                                ? LucideIcons.powerOff
                                : LucideIcons.power,
                          ),
                    label: Text(
                      strings.get(
                        connected
                            ? 'disconnect'
                            : snapshot.isTransitional
                            ? 'connecting'
                            : 'connect',
                      ),
                    ),
                  ),
                ),
                if (controller.activeProfile.mode == OperatingMode.vpn &&
                    connected) ...<Widget>[
                  const SizedBox(height: 12),
                  SizedBox(
                    width: double.infinity,
                    child: OutlinedButton.icon(
                      onPressed:
                          controller.busy ||
                              snapshot.platformLockdown ||
                              snapshot.phase ==
                                  ConnectionPhase.captivePortalPaused
                          ? null
                          : () => _confirmCaptivePortalPause(context),
                      icon: const Icon(LucideIcons.wifiOff),
                      label: Text(strings.get('captive_portal_access')),
                    ),
                  ),
                  if (snapshot.platformLockdown) ...<Widget>[
                    const SizedBox(height: 8),
                    Text(
                      strings.get('lockdown_pause_blocked'),
                      style: Theme.of(context).textTheme.bodySmall?.copyWith(
                        color: Theme.of(context).colorScheme.error,
                      ),
                    ),
                  ],
                  if (snapshot.phase ==
                      ConnectionPhase.captivePortalPaused) ...<Widget>[
                    const SizedBox(height: 8),
                    Text(
                      '${strings.get('pause_remaining')}: '
                      '${formatDuration(Duration(seconds: snapshot.captivePauseRemainingSeconds))}',
                      style: Theme.of(context).textTheme.bodySmall,
                    ),
                  ],
                ],
              ],
            ),
          ),
        ),
      ),
    );
  }

  Future<void> _confirmCaptivePortalPause(BuildContext context) async {
    final confirmed = await showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        icon: const Icon(LucideIcons.wifiOff),
        title: Text(strings.get('captive_portal_access')),
        content: Text(strings.get('captive_pause_help')),
        actions: <Widget>[
          TextButton(
            onPressed: () => Navigator.of(context).pop(false),
            child: Text(strings.get('cancel')),
          ),
          FilledButton(
            onPressed: () => Navigator.of(context).pop(true),
            child: Text(strings.get('pause_10_minutes')),
          ),
        ],
      ),
    );
    if (confirmed == true) {
      await controller.pauseCaptivePortal();
    }
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

class _ConnectionDetails extends StatelessWidget {
  const _ConnectionDetails({
    required this.snapshot,
    required this.mode,
    required this.strings,
  });

  final EngineSnapshot snapshot;
  final OperatingMode mode;
  final AppStrings strings;

  @override
  Widget build(BuildContext context) {
    final duration = snapshot.connectedAt == null
        ? null
        : DateTime.now().difference(snapshot.connectedAt!);
    return Panel(
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: <Widget>[
          SectionTitle(
            icon: LucideIcons.activity,
            title: strings.get('engine_status'),
          ),
          const SizedBox(height: 22),
          _DetailRow(
            icon: LucideIcons.cable,
            label: strings.get('protocol'),
            value: snapshot.transport ?? '—',
          ),
          const Divider(height: 28),
          _DetailRow(
            icon: LucideIcons.network,
            label: strings.get('address_family'),
            value: snapshot.addressFamily ?? '—',
          ),
          const Divider(height: 28),
          _DetailRow(
            icon: LucideIcons.clock3,
            label: strings.get('duration'),
            value: duration == null ? '—' : formatDuration(duration),
          ),
          const Divider(height: 28),
          _DetailRow(
            icon: mode == OperatingMode.vpn
                ? LucideIcons.shieldCheck
                : LucideIcons.shieldOff,
            label: strings.get('kill_switch'),
            value: mode == OperatingMode.vpn
                ? strings.get(snapshot.isConnected ? 'on' : 'ready')
                : strings.get('not_used_proxy'),
          ),
        ],
      ),
    );
  }
}

class _DetailRow extends StatelessWidget {
  const _DetailRow({
    required this.icon,
    required this.label,
    required this.value,
  });

  final IconData icon;
  final String label;
  final String value;

  @override
  Widget build(BuildContext context) {
    return Row(
      children: <Widget>[
        Icon(
          icon,
          size: 19,
          color: Theme.of(context).colorScheme.onSurfaceVariant,
        ),
        const SizedBox(width: 12),
        Expanded(child: Text(label)),
        const SizedBox(width: 12),
        Text(value, style: Theme.of(context).textTheme.titleSmall),
      ],
    );
  }
}

class _TrafficGrid extends StatelessWidget {
  const _TrafficGrid({required this.snapshot, required this.strings});

  final EngineSnapshot snapshot;
  final AppStrings strings;

  @override
  Widget build(BuildContext context) {
    return LayoutBuilder(
      builder: (context, constraints) {
        final cards = <Widget>[
          _MetricCard(
            icon: LucideIcons.arrowDown,
            label: strings.get('download'),
            value: formatRate(snapshot.downloadBytesPerSecond),
            color: const Color(0xFF176B87),
          ),
          _MetricCard(
            icon: LucideIcons.arrowUp,
            label: strings.get('upload'),
            value: formatRate(snapshot.uploadBytesPerSecond),
            color: UsqueColors.deepOrange,
          ),
        ];
        if (constraints.maxWidth >= 560) {
          return Row(
            children: <Widget>[
              Expanded(child: cards[0]),
              const SizedBox(width: 16),
              Expanded(child: cards[1]),
            ],
          );
        }
        return Column(
          children: <Widget>[cards[0], const SizedBox(height: 16), cards[1]],
        );
      },
    );
  }
}

class _MetricCard extends StatelessWidget {
  const _MetricCard({
    required this.icon,
    required this.label,
    required this.value,
    required this.color,
  });

  final IconData icon;
  final String label;
  final String value;
  final Color color;

  @override
  Widget build(BuildContext context) {
    return Panel(
      child: Row(
        children: <Widget>[
          DecoratedBox(
            decoration: BoxDecoration(
              color: color.withValues(alpha: 0.12),
              borderRadius: BorderRadius.circular(15),
            ),
            child: Padding(
              padding: const EdgeInsets.all(13),
              child: Icon(icon, color: color),
            ),
          ),
          const SizedBox(width: 16),
          Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: <Widget>[
              Text(
                label,
                style: Theme.of(context).textTheme.bodyMedium?.copyWith(
                  color: Theme.of(context).colorScheme.onSurfaceVariant,
                ),
              ),
              const SizedBox(height: 3),
              Text(value, style: Theme.of(context).textTheme.titleLarge),
            ],
          ),
        ],
      ),
    );
  }
}

class _ExitPanel extends StatelessWidget {
  const _ExitPanel({required this.snapshot, required this.strings});

  final EngineSnapshot snapshot;
  final AppStrings strings;

  @override
  Widget build(BuildContext context) {
    final exit = snapshot.exit;
    Widget locationIcon;
    if (exit.flagSvg case final flag? when flag.isNotEmpty) {
      locationIcon = ClipRRect(
        borderRadius: BorderRadius.circular(3),
        child: SvgPicture.string(
          flag,
          width: 24,
          height: 18,
          fit: BoxFit.cover,
        ),
      );
    } else {
      locationIcon = const Icon(LucideIcons.mapPin, size: 20);
    }
    return Panel(
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: <Widget>[
          SectionTitle(
            icon: LucideIcons.globe2,
            title: strings.get('location'),
            subtitle: snapshot.isConnected
                ? 'ip.sb'
                : strings.get('not_available'),
          ),
          const SizedBox(height: 22),
          _ExitRow(
            icon: locationIcon,
            label: strings.get('location'),
            value: exit.hasLocation
                ? exit.location
                : strings.get('not_available'),
          ),
          const Divider(height: 28),
          _ExitRow(
            icon: const Icon(LucideIcons.network, size: 20),
            label: strings.get('ipv4'),
            value: exit.ipv4 ?? strings.get('not_available'),
            monospace: true,
          ),
          const Divider(height: 28),
          _ExitRow(
            icon: const Icon(LucideIcons.network, size: 20),
            label: strings.get('ipv6'),
            value: exit.ipv6 ?? strings.get('not_available'),
            monospace: true,
          ),
        ],
      ),
    );
  }
}

class _ExitRow extends StatelessWidget {
  const _ExitRow({
    required this.icon,
    required this.label,
    required this.value,
    this.monospace = false,
  });

  final Widget icon;
  final String label;
  final String value;
  final bool monospace;

  @override
  Widget build(BuildContext context) {
    return Row(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: <Widget>[
        SizedBox(width: 28, child: Center(child: icon)),
        const SizedBox(width: 10),
        SizedBox(width: 86, child: Text(label)),
        const SizedBox(width: 12),
        Expanded(
          child: SelectableText(
            value,
            style: monospace
                ? const TextStyle(fontFamily: 'monospace')
                : Theme.of(context).textTheme.titleSmall,
          ),
        ),
      ],
    );
  }
}

String _phaseLabel(AppStrings strings, ConnectionPhase phase) {
  return strings.get(switch (phase) {
    ConnectionPhase.disconnected => 'disconnected',
    ConnectionPhase.preparing => 'preparing',
    ConnectionPhase.connectingH3 ||
    ConnectionPhase.connectingH2 => 'connecting',
    ConnectionPhase.connected => 'connected',
    ConnectionPhase.degraded => 'degraded',
    ConnectionPhase.reconnecting => 'reconnecting',
    ConnectionPhase.disconnecting => 'disconnecting',
    ConnectionPhase.captivePortalPaused => 'captive_pause',
    ConnectionPhase.error => 'error',
  });
}

String _modeLabel(AppStrings strings, OperatingMode mode) {
  return strings.get(switch (mode) {
    OperatingMode.vpn => 'vpn_mode',
    OperatingMode.socks5 => 'socks_mode',
    OperatingMode.httpProxy => 'http_mode',
  });
}
