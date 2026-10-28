import 'package:flutter/material.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../../core/usque_motion.dart';
import '../../core/usque_theme.dart';
import '../../models/app_models.dart';
import '../../models/network_quality_models.dart';
import '../../state/app_controller.dart';
import '../common.dart';
import '../controller_selector.dart';
import '../traffic_chart.dart';

/// Both home layouts describe the same observed history, not just its capacity.
String homeTrafficNoteKey(
  AppController controller, {
  required bool hasSamples,
}) {
  final quality = controller.quality;
  return !controller.snapshot.isConnected
      ? 'home_traffic_idle'
      : !quality.enabled
      ? 'home_traffic_unavailable'
      : quality.paused
      ? 'nq_paused'
      // A known outage or an expired reading is not an empty start-up window.
      : !hasSamples &&
            (quality.streamUnavailable ||
                (quality.latest != null && quality.stale))
      ? 'home_traffic_stale'
      : !hasSamples
      ? 'home_traffic_waiting'
      : quality.stale
      ? 'home_traffic_stale'
      : 'home_traffic_window';
}

/// Live rates over one shared chart.
///
/// Uses the existing timestamped read model; this view neither samples on
/// rebuild nor starts a timer, probe, persistence operation or upload. The
/// chart keeps its footprint in every state, so connecting never moves the
/// page.
class HomeTrafficCard extends StatelessWidget {
  const HomeTrafficCard({
    required this.controller,
    this.compact = false,
    this.chartHeight = 96,
    super.key,
  });

  final AppController controller;

  /// Phone layout: smaller type and its own value keys.
  final bool compact;

  /// Minimum chart height; on desktop the card can stretch it further.
  final double chartHeight;

  @override
  Widget build(BuildContext context) => ControllerSelector<bool>(
    controller: controller,
    observeWhenHidden: true,
    selector: (app) => app.uiVisible && app.section == AppSection.home,
    builder: (context, active) => ListenableBuilder(
      listenable: Listenable.merge(
        active ? [controller, controller.quality] : [],
      ),
      builder: (context, _) => _build(context),
    ),
  );

  Widget _build(BuildContext context) {
    final strings = controller.strings;
    final theme = Theme.of(context);
    final tokens = UsqueTokens.of(context);
    final snapshot = controller.snapshot;
    final quality = controller.quality;
    final connected = snapshot.isConnected;
    final down = connected
        ? quality.traceFor(NetworkQualityTrace.download)
        : const <int?>[];
    final up = connected
        ? quality.traceFor(NetworkQualityTrace.upload)
        : const <int?>[];
    final hasSamples =
        down.any((value) => value != null) || up.any((value) => value != null);
    final note = strings.get(
      homeTrafficNoteKey(controller, hasSamples: hasSamples),
    );
    final present = [...down, ...up].whereType<int>().toList(growable: false);
    final summary = present.isEmpty
        ? '${strings.get('home_traffic')} · $note'
        : '${strings.get('home_traffic')} · $note · '
              '${strings.get('nq_range')}: '
              '${formatRate(present.reduce((a, b) => a < b ? a : b))}–'
              '${formatRate(present.reduce((a, b) => a > b ? a : b))}';
    final keyPrefix = compact ? 'home' : 'home-desktop';

    Widget rate(bool download) => _RateReadout(
      key: ValueKey('$keyPrefix-${download ? 'download' : 'upload'}-readout'),
      valueKey: ValueKey('$keyPrefix-${download ? 'download' : 'upload'}-rate'),
      icon: download ? LucideIcons.arrowDown : LucideIcons.arrowUp,
      label: strings.get(download ? 'download' : 'upload'),
      bytesPerSecond: !connected
          ? null
          : download
          ? snapshot.downloadBytesPerSecond
          : snapshot.uploadBytesPerSecond,
      color: download ? tokens.inbound : tokens.outbound,
      size: compact ? 20 : 22,
    );

    final muted = theme.colorScheme.onSurfaceVariant;
    final caption = Text(
      note,
      key: const ValueKey('home-traffic-note'),
      textAlign: hasSamples ? TextAlign.start : TextAlign.center,
      style: theme.textTheme.bodySmall?.copyWith(color: muted),
    );
    final chart = TrafficChart(
      key: const ValueKey('home-traffic-chart'),
      download: down,
      upload: up,
      downloadColor: tokens.inbound,
      uploadColor: tokens.outbound,
      height: chartHeight,
      semanticLabel: summary,
    );
    // Without a reading the note explains the empty chart from inside it.
    final plot = hasSamples
        ? chart
        : Stack(
            fit: StackFit.passthrough,
            children: [
              chart,
              Positioned.fill(
                child: Center(
                  child: Padding(
                    padding: const EdgeInsets.symmetric(
                      horizontal: UsqueSpace.m,
                    ),
                    child: Row(
                      mainAxisSize: MainAxisSize.min,
                      children: [
                        ExcludeSemantics(
                          child: Icon(
                            LucideIcons.activity,
                            size: 16,
                            color: muted,
                          ),
                        ),
                        const SizedBox(width: UsqueSpace.xs),
                        Flexible(child: caption),
                      ],
                    ),
                  ),
                ),
              ),
            ],
          );
    final title = Semantics(
      header: true,
      child: Text(
        strings.get('home_traffic'),
        style: theme.textTheme.titleMedium,
      ),
    );
    final rates = Wrap(
      spacing: UsqueSpace.xl,
      runSpacing: UsqueSpace.s,
      children: [rate(true), rate(false)],
    );
    final largeText = MediaQuery.textScalerOf(context).scale(14) > 21;
    final Widget header;
    if (compact || largeText) {
      header = Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        mainAxisSize: MainAxisSize.min,
        children: [
          if (hasSamples && !largeText)
            Row(
              crossAxisAlignment: CrossAxisAlignment.baseline,
              textBaseline: TextBaseline.alphabetic,
              children: [
                title,
                const SizedBox(width: UsqueSpace.s),
                Expanded(
                  child: Align(
                    alignment: AlignmentDirectional.centerEnd,
                    child: caption,
                  ),
                ),
              ],
            )
          else ...[
            title,
            if (hasSamples) ...[
              const SizedBox(height: UsqueSpace.xxs),
              caption,
            ],
          ],
          const SizedBox(height: UsqueSpace.s),
          rates,
        ],
      );
    } else {
      header = Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              mainAxisSize: MainAxisSize.min,
              children: [
                title,
                if (hasSamples) ...[
                  const SizedBox(height: UsqueSpace.xxs),
                  caption,
                ],
              ],
            ),
          ),
          const SizedBox(width: UsqueSpace.l),
          rates,
        ],
      );
    }
    return Panel(
      key: const ValueKey('home-traffic'),
      padding: const EdgeInsets.all(UsqueSpace.m + 4),
      // The chart is last: a stretched card hands it every spare pixel.
      child: FillColumn(
        children: [
          header,
          const SizedBox(height: UsqueSpace.s),
          plot,
        ],
      ),
    );
  }
}

/// A direction label over its rate. Rates ease between samples instead of
/// jumping, and a missing reading is a dash rather than zero.
class _RateReadout extends StatelessWidget {
  const _RateReadout({
    required this.valueKey,
    required this.icon,
    required this.label,
    required this.bytesPerSecond,
    required this.color,
    required this.size,
    super.key,
  });

  final Key valueKey;
  final IconData icon;
  final String label;
  final int? bytesPerSecond;
  final Color color;
  final double size;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final muted = theme.colorScheme.onSurfaceVariant;
    final rate = bytesPerSecond;
    final style = UsqueTheme.readout(
      context,
      size: size,
      color: rate == null ? muted : null,
    );
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      mainAxisSize: MainAxisSize.min,
      children: [
        Row(
          mainAxisSize: MainAxisSize.min,
          children: [
            ExcludeSemantics(child: Icon(icon, size: 14, color: color)),
            const SizedBox(width: 6),
            Text(
              label,
              style: theme.textTheme.bodySmall?.copyWith(color: muted),
            ),
          ],
        ),
        const SizedBox(height: 2),
        if (rate == null)
          Text('—', key: valueKey, style: style)
        else
          TweenAnimationBuilder<double>(
            tween: Tween<double>(end: rate.toDouble()),
            duration: UsqueMotion.of(context, UsqueMotion.gentle),
            curve: UsqueMotion.standard,
            builder: (context, value, _) =>
                Text(formatRate(value.round()), key: valueKey, style: style),
          ),
      ],
    );
  }
}
