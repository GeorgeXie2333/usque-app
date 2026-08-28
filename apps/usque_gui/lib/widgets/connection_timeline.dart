import 'package:flutter/material.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../core/app_strings.dart';
import '../core/diagnostics_strings.dart';
import '../core/usque_theme.dart';
import '../models/diagnostics_models.dart';

class ConnectionTimelineView extends StatelessWidget {
  const ConnectionTimelineView({
    required this.timeline,
    required this.strings,
    super.key,
  });

  final ConnectionTimeline timeline;
  final AppStrings strings;

  @override
  Widget build(BuildContext context) {
    if (timeline.events.isEmpty) {
      return Padding(
        padding: const EdgeInsets.symmetric(vertical: 20),
        child: Row(
          children: <Widget>[
            Icon(
              LucideIcons.activity,
              color: Theme.of(context).colorScheme.onSurfaceVariant,
            ),
            const SizedBox(width: 12),
            Expanded(
              child: Text(
                strings.get('diag_timeline_empty'),
                style: Theme.of(context).textTheme.bodyMedium?.copyWith(
                  color: Theme.of(context).colorScheme.onSurfaceVariant,
                ),
              ),
            ),
          ],
        ),
      );
    }
    final omitted = (timeline.events.length - 100).clamp(
      0,
      timeline.events.length,
    );
    final events = omitted == 0
        ? timeline.events
        : timeline.events.sublist(omitted);
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: <Widget>[
        _MetricsStrip(metrics: timeline.metrics, strings: strings),
        if (omitted > 0) ...<Widget>[
          const SizedBox(height: 10),
          Text(
            strings.get('diag_timeline_truncated'),
            style: Theme.of(context).textTheme.bodySmall?.copyWith(
              color: Theme.of(context).colorScheme.onSurfaceVariant,
            ),
          ),
        ],
        const SizedBox(height: 16),
        Semantics(
          label: strings.get('diag_timeline'),
          child: ListView.builder(
            shrinkWrap: true,
            physics: const NeverScrollableScrollPhysics(),
            itemCount: events.length,
            itemBuilder: (context, index) => _TimelineRow(
              event: events[index],
              strings: strings,
              first: index == 0,
              last: index == events.length - 1,
            ),
          ),
        ),
      ],
    );
  }
}

class _MetricsStrip extends StatelessWidget {
  const _MetricsStrip({required this.metrics, required this.strings});

  final ConnectionMetrics metrics;
  final AppStrings strings;

  @override
  Widget build(BuildContext context) {
    final items = <({String label, String value})>[
      (
        label: strings.get('diag_metric_reconnects'),
        value: '${metrics.reconnectCount}',
      ),
      (
        label: strings.get('diag_metric_fallbacks'),
        value: '${metrics.fallbackCount}',
      ),
      (
        label: strings.get('diag_metric_network_changes'),
        value: '${metrics.networkChangeCount}',
      ),
      (
        label: strings.get('diag_metric_queue_high_water'),
        value: '${metrics.sendQueueHighWatermark}',
      ),
      (
        label: 'RTT',
        value: metrics.currentSmoothedRttMilliseconds == null
            ? strings.get('diag_unknown')
            : '${metrics.currentSmoothedRttMilliseconds} ms',
      ),
    ];
    return Wrap(
      spacing: 8,
      runSpacing: 8,
      children: items
          .map(
            (item) => Container(
              padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 7),
              decoration: BoxDecoration(
                border: Border.all(color: UsqueTokens.of(context).hairline),
                borderRadius: BorderRadius.circular(UsqueRadii.chip),
              ),
              child: Text(
                '${item.label} · ${item.value}',
                style: Theme.of(context).textTheme.labelSmall,
              ),
            ),
          )
          .toList(growable: false),
    );
  }
}

class _TimelineRow extends StatelessWidget {
  const _TimelineRow({
    required this.event,
    required this.strings,
    required this.first,
    required this.last,
  });

  final ConnectionTimelineEvent event;
  final AppStrings strings;
  final bool first;
  final bool last;

  @override
  Widget build(BuildContext context) {
    final tokens = UsqueTokens.of(context);
    final color = event.failure == null ? tokens.brand : tokens.caution;
    final label = connectionEventLabel(strings, event.eventType);
    return Semantics(
      label: '$label, ${_elapsed(event.elapsedMilliseconds)}',
      child: IntrinsicHeight(
        child: Row(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: <Widget>[
            SizedBox(
              width: 28,
              child: Column(
                children: <Widget>[
                  Expanded(
                    child: Container(
                      width: 2,
                      color: first ? Colors.transparent : tokens.hairlineStrong,
                    ),
                  ),
                  Container(
                    width: 12,
                    height: 12,
                    decoration: BoxDecoration(
                      color: color,
                      shape: BoxShape.circle,
                      border: Border.all(
                        color: Theme.of(context).colorScheme.surface,
                        width: 2,
                      ),
                    ),
                  ),
                  Expanded(
                    child: Container(
                      width: 2,
                      color: last ? Colors.transparent : tokens.hairlineStrong,
                    ),
                  ),
                ],
              ),
            ),
            const SizedBox(width: 10),
            Expanded(
              child: Padding(
                padding: const EdgeInsets.symmetric(vertical: 10),
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: <Widget>[
                    Row(
                      children: <Widget>[
                        Expanded(
                          child: Text(
                            label,
                            style: Theme.of(context).textTheme.bodyMedium,
                          ),
                        ),
                        const SizedBox(width: 8),
                        Text(
                          _elapsed(event.elapsedMilliseconds),
                          style: Theme.of(context).textTheme.labelSmall
                              ?.copyWith(
                                fontFamily: UsqueFonts.mono,
                                fontFamilyFallback: UsqueFonts.monoFallback,
                                color: Theme.of(
                                  context,
                                ).colorScheme.onSurfaceVariant,
                              ),
                        ),
                      ],
                    ),
                    if (event.failure != null) ...<Widget>[
                      const SizedBox(height: 3),
                      Text(
                        event.failure!.code,
                        style: Theme.of(context).textTheme.labelSmall?.copyWith(
                          color: color,
                          fontFamily: UsqueFonts.mono,
                          fontFamilyFallback: UsqueFonts.monoFallback,
                        ),
                      ),
                    ],
                  ],
                ),
              ),
            ),
          ],
        ),
      ),
    );
  }
}

String _elapsed(int milliseconds) {
  if (milliseconds < 1000) {
    return '+${milliseconds}ms';
  }
  return '+${(milliseconds / 1000).toStringAsFixed(1)}s';
}
