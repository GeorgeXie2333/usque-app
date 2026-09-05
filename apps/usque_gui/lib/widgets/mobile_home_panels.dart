import 'package:flutter/material.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../core/usque_theme.dart';
import '../models/app_models.dart';
import '../screens/diagnostics_screen.dart';
import '../screens/network_quality_screen.dart';
import '../state/app_controller.dart';
import 'common.dart';
import 'controller_selector.dart';
import 'live_duration.dart';
import 'sparkline.dart';

/// Share a small, bounded amount of breathing room across all three cards on
/// tall phones. Large text and short viewports keep the compact spacing.
double mobileHomeExpansion(BuildContext context) {
  final media = MediaQuery.of(context);
  if (media.textScaler.scale(14) > 21) return 0;
  final usableHeight = media.size.height - media.viewPadding.vertical;
  return ((usableHeight - 812) / 120).clamp(0, 1).toDouble();
}

/// Uses the existing timestamped read model; this view neither samples on
/// rebuild nor starts a timer, probe, persistence operation or upload.
class MobileTrafficPanel extends StatelessWidget {
  const MobileTrafficPanel({required this.controller, super.key});
  final AppController controller;

  @override
  Widget build(BuildContext context) => ControllerSelector<bool>(
    controller: controller,
    selector: (app) => app.section == AppSection.home,
    builder: (context, active) => ListenableBuilder(
      listenable: Listenable.merge(
        active ? [controller, controller.quality] : [],
      ),
      builder: (context, _) => _buildPanel(context),
    ),
  );

  Widget _buildPanel(BuildContext context) {
    final strings = controller.strings;
    final theme = Theme.of(context);
    final tokens = UsqueTokens.of(context);
    final quality = controller.quality;
    final snapshot = controller.snapshot;
    final connected = snapshot.isConnected;
    final down = quality.trace((point) => point.downloadBytesPerSecond);
    final up = quality.trace((point) => point.uploadBytesPerSecond);
    final hasSamples =
        connected &&
        (down.any((value) => value != null) ||
            up.any((value) => value != null));
    final note = strings.get(
      !connected
          ? 'home_traffic_idle'
          : !quality.enabled
          ? 'home_traffic_unavailable'
          : quality.paused
          ? 'nq_paused'
          : !hasSamples
          ? 'home_traffic_waiting'
          : quality.stale
          ? 'home_traffic_stale'
          : 'home_traffic_window',
    );
    return Panel(
      padding: const EdgeInsets.all(16),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Row(
            children: [
              Semantics(
                header: true,
                child: Text(
                  strings.get('home_traffic'),
                  style: theme.textTheme.titleMedium,
                ),
              ),
              const SizedBox(width: 12),
              Expanded(
                child: Text(
                  note,
                  textAlign: TextAlign.end,
                  style: theme.textTheme.bodySmall?.copyWith(
                    color: theme.colorScheme.onSurfaceVariant,
                  ),
                ),
              ),
            ],
          ),
          const SizedBox(height: 10),
          LayoutBuilder(
            builder: (context, constraints) {
              Widget metric(bool download) {
                final samples = connected
                    ? (download ? down : up)
                    : const <int?>[];
                final label = strings.get(download ? 'download' : 'upload');
                final rate = connected
                    ? (download
                          ? snapshot.downloadBytesPerSecond
                          : snapshot.uploadBytesPerSecond)
                    : 0;
                final color = download ? tokens.inbound : tokens.outbound;
                final present = samples.whereType<int>().toList(
                  growable: false,
                );
                final range = present.isEmpty
                    ? null
                    : present.reduce((a, b) => a < b ? a : b);
                final peak = present.isEmpty
                    ? null
                    : present.reduce((a, b) => a > b ? a : b);
                final summary = present.isEmpty
                    ? '$label · $note'
                    : '$label · ${strings.get('home_traffic_window')} · ${present.length}/60 · ${strings.get('nq_range')}: ${formatRate(range!)}–${formatRate(peak!)}';
                return Column(
                  crossAxisAlignment: CrossAxisAlignment.stretch,
                  children: [
                    Row(
                      children: [
                        Icon(
                          download
                              ? LucideIcons.arrowDown
                              : LucideIcons.arrowUp,
                          size: 16,
                          color: color,
                        ),
                        const SizedBox(width: 6),
                        Expanded(
                          child: Text(label, style: theme.textTheme.bodyMedium),
                        ),
                      ],
                    ),
                    const SizedBox(height: 6),
                    Text(
                      formatRate(rate),
                      style: UsqueTheme.mono(
                        context,
                        size: 18,
                        weight: FontWeight.w500,
                      ),
                    ),
                    const SizedBox(height: 8),
                    Sparkline(
                      key: ValueKey(
                        download ? 'home-download-trace' : 'home-upload-trace',
                      ),
                      samples: samples,
                      height: 24 + mobileHomeExpansion(context) * 12,
                      color: color,
                      semanticLabel: summary,
                    ),
                  ],
                );
              }

              if (constraints.maxWidth < 260 ||
                  MediaQuery.textScalerOf(context).scale(14) > 21) {
                return Column(
                  children: [
                    metric(true),
                    const SizedBox(height: 16),
                    metric(false),
                  ],
                );
              }
              return Row(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Expanded(child: metric(true)),
                  const SizedBox(width: 24),
                  Expanded(child: metric(false)),
                ],
              );
            },
          ),
        ],
      ),
    );
  }
}

typedef _OverviewView = ({
  ConnectionPhase phase,
  bool connected,
  String? transport,
  String? family,
  DateTime? since,
  ExitInfo exit,
  FrontendSettings outputs,
  bool systemProxy,
  bool quality,
});

class MobileConnectionOverview extends StatelessWidget {
  const MobileConnectionOverview({
    required this.controller,
    required this.details,
    super.key,
  });
  final AppController controller;
  final Widget details;

  @override
  Widget build(BuildContext context) => ControllerSelector<_OverviewView>(
    controller: controller,
    active: (app) => app.section == AppSection.home,
    selector: (app) => (
      phase: app.snapshot.phase,
      connected: app.snapshot.isConnected,
      transport: app.snapshot.transport,
      family: app.snapshot.addressFamily,
      since: app.snapshot.connectedAt,
      exit: app.snapshot.exit,
      outputs: app.activeProfile.frontends,
      systemProxy: app.activeProfile.proxy.systemProxy,
      quality: app.engineCapabilities?.networkQuality ?? false,
    ),
    builder: (context, view) => _buildPanel(context, view),
  );

  Widget _buildPanel(BuildContext context, _OverviewView view) {
    final strings = controller.strings;
    final theme = Theme.of(context);
    final location = [
      view.exit.country,
      view.exit.city,
    ].whereType<String>().where((value) => value.trim().isNotEmpty).firstOrNull;
    final protocol = [
      view.transport,
      view.family,
    ].whereType<String>().where((value) => value.isNotEmpty).join(' · ');
    return Panel(
      padding: const EdgeInsets.fromLTRB(16, 16, 16, 4),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Semantics(
            header: true,
            child: Text(
              strings.get('home_overview'),
              style: theme.textTheme.titleMedium,
            ),
          ),
          const SizedBox(height: 12),
          // A minimum (not fixed) height steadies normal state transitions while
          // allowing long locales and enlarged text to grow and scroll.
          ConstrainedBox(
            constraints: BoxConstraints(
              minHeight: 58 + mobileHomeExpansion(context) * 20,
            ),
            child: view.connected
                ? Column(
                    crossAxisAlignment: CrossAxisAlignment.stretch,
                    children: [
                      Row(
                        crossAxisAlignment: CrossAxisAlignment.start,
                        children: [
                          Expanded(
                            child: _OverviewFact(
                              label: strings.get('home_exit_region'),
                              child: Tooltip(
                                message:
                                    location ?? strings.get('not_available'),
                                child: Text(
                                  location ?? strings.get('not_available'),
                                  maxLines: 2,
                                  overflow: TextOverflow.ellipsis,
                                  style: theme.textTheme.titleSmall,
                                ),
                              ),
                            ),
                          ),
                          const SizedBox(width: 16),
                          Expanded(
                            child: _OverviewFact(
                              label: strings.get('duration'),
                              trailing: true,
                              child: LiveDuration(since: view.since),
                            ),
                          ),
                        ],
                      ),
                      const SizedBox(height: 8),
                      Text(
                        '${strings.get('protocol')} · ${protocol.isEmpty ? strings.get('not_available') : protocol}',
                        style: theme.textTheme.bodySmall?.copyWith(
                          color: theme.colorScheme.onSurfaceVariant,
                        ),
                      ),
                    ],
                  )
                : Column(
                    crossAxisAlignment: CrossAxisAlignment.stretch,
                    children: [
                      Text(
                        strings.get(
                          view.phase == ConnectionPhase.error
                              ? 'home_outputs_retry'
                              : 'home_outputs_next',
                        ),
                        style: theme.textTheme.bodySmall?.copyWith(
                          color: theme.colorScheme.onSurfaceVariant,
                        ),
                      ),
                      const SizedBox(height: 8),
                      if (!view.outputs.any && !view.systemProxy)
                        Text(strings.get('channel_only_warning'))
                      else
                        Wrap(
                          spacing: 6,
                          runSpacing: 6,
                          children: [
                            if (view.outputs.tunnel)
                              _output(
                                strings.tunnelOutputLabel(theme.platform),
                              ),
                            if (view.outputs.socks5) _output('SOCKS5'),
                            if (view.outputs.http) _output('HTTP'),
                            if (view.systemProxy)
                              _output(strings.get('system_proxy')),
                          ],
                        ),
                    ],
                  ),
          ),
          const SizedBox(height: 12),
          LayoutBuilder(
            builder: (context, constraints) {
              final style = OutlinedButton.styleFrom(
                minimumSize: const Size(0, 48),
                padding: const EdgeInsets.symmetric(
                  horizontal: 10,
                  vertical: 10,
                ),
                foregroundColor: theme.colorScheme.onSurface,
              );
              final quality = OutlinedButton.icon(
                key: const ValueKey('home-network-quality'),
                style: style,
                onPressed: () => Navigator.of(context).push(
                  MaterialPageRoute<void>(
                    builder: (_) =>
                        NetworkQualityScreen(controller: controller),
                  ),
                ),
                icon: const Icon(LucideIcons.gauge, size: 16),
                label: Text(strings.get('network_quality')),
              );
              final diagnostics = OutlinedButton.icon(
                key: const ValueKey('home-diagnostics'),
                style: style,
                onPressed: () => Navigator.of(context).push(
                  MaterialPageRoute<void>(
                    builder: (_) => DiagnosticsScreen(controller: controller),
                  ),
                ),
                icon: const Icon(LucideIcons.activity, size: 16),
                label: Text(strings.get('diagnostics')),
              );
              if (!view.quality) return diagnostics;
              if (constraints.maxWidth < 280 ||
                  MediaQuery.textScalerOf(context).scale(14) > 21) {
                return Column(
                  crossAxisAlignment: CrossAxisAlignment.stretch,
                  children: [quality, const SizedBox(height: 8), diagnostics],
                );
              }
              return Row(
                children: [
                  Expanded(child: quality),
                  const SizedBox(width: 8),
                  Expanded(child: diagnostics),
                ],
              );
            },
          ),
          const SizedBox(height: 4),
          details,
        ],
      ),
    );
  }

  Widget _output(String label) =>
      StatusPill(label: label, tone: StatusTone.neutral, showIndicator: false);
}

class _OverviewFact extends StatelessWidget {
  const _OverviewFact({
    required this.label,
    required this.child,
    this.trailing = false,
  });
  final String label;
  final Widget child;
  final bool trailing;
  @override
  Widget build(BuildContext context) => Column(
    crossAxisAlignment: trailing
        ? CrossAxisAlignment.end
        : CrossAxisAlignment.start,
    children: [
      Text(
        label,
        style: Theme.of(context).textTheme.bodySmall?.copyWith(
          color: Theme.of(context).colorScheme.onSurfaceVariant,
        ),
      ),
      const SizedBox(height: 4),
      child,
    ],
  );
}
