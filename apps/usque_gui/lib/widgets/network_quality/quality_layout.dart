import 'package:flutter/material.dart';

import '../../core/app_strings.dart';
import '../../core/quality_presentation.dart';
import '../../core/usque_theme.dart';
import '../../models/app_models.dart';
import '../../models/network_quality_models.dart';
import '../common.dart';
import '../sparkline.dart';

/// Metric sections separated by hairlines, without card surfaces.
class QualitySectionList extends StatelessWidget {
  const QualitySectionList({required this.children, super.key});
  final List<Widget> children;
  @override
  Widget build(BuildContext context) => ContentList(
    children: [
      for (final child in children)
        Padding(
          padding: const EdgeInsets.symmetric(vertical: 24),
          child: child,
        ),
    ],
  );
}

class QualitySection extends StatelessWidget {
  const QualitySection({
    required this.title,
    required this.icon,
    required this.children,
    this.help,
    super.key,
  });
  final String title;
  final IconData icon;
  final List<Widget> children;
  final Widget? help;
  @override
  Widget build(BuildContext context) => ContentSection(
    child: Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: <Widget>[
        Row(
          children: <Widget>[
            Icon(icon, size: 20),
            const SizedBox(width: 10),
            Expanded(
              child: Semantics(
                header: true,
                child: Text(
                  title,
                  style: Theme.of(context).textTheme.titleMedium,
                ),
              ),
            ),
            ?help,
          ],
        ),
        const SizedBox(height: 18),
        for (var index = 0; index < children.length; index++) ...<Widget>[
          if (index > 0) const SizedBox(height: 14),
          children[index],
        ],
      ],
    ),
  );
}

/// Labelled readings in two columns, or one when narrow or enlarged.
class QualityReadouts extends StatelessWidget {
  const QualityReadouts({required this.values, super.key});
  final List<(String, String)> values;
  @override
  Widget build(BuildContext context) => LayoutBuilder(
    builder: (context, constraints) {
      final scale = MediaQuery.textScalerOf(context).scale(16) / 16;
      final width = constraints.maxWidth >= 330 * scale
          ? (constraints.maxWidth - 16) / 2
          : constraints.maxWidth;
      return Wrap(
        spacing: 16,
        runSpacing: 16,
        children: <Widget>[
          for (final (label, value) in values)
            SizedBox(
              width: width,
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: <Widget>[
                  Text(label, style: Theme.of(context).textTheme.bodySmall),
                  const SizedBox(height: 4),
                  Text(
                    value,
                    style: Theme.of(context).textTheme.titleLarge?.copyWith(
                      fontFamily: UsqueFonts.display,
                      fontFeatures: const <FontFeature>[
                        FontFeature.tabularFigures(),
                      ],
                    ),
                  ),
                ],
              ),
            ),
        ],
      );
    },
  );
}

/// A 60-second sparkline with its sample count and range.
class QualityTrace extends StatelessWidget {
  const QualityTrace({
    required this.samples,
    required this.color,
    required this.label,
    required this.strings,
    required this.format,
    super.key,
  });
  final List<int?> samples;
  final Color color;
  final String label;
  final AppStrings strings;
  final String Function(int) format;
  @override
  Widget build(BuildContext context) {
    final known = samples.whereType<int>().toList()..sort();
    final range = known.isEmpty
        ? ''
        : ' · ${strings.get('nq_range')}: ${format(known.first)}–${format(known.last)}';
    final summary =
        '$label · ${known.length}/60 ${strings.get('nq_samples')}$range';
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: <Widget>[
        Sparkline(
          samples: samples,
          color: color,
          height: 54,
          semanticLabel: '$summary. ${strings.get('nq_gaps')}',
        ),
        const SizedBox(height: 4),
        Text(summary, style: Theme.of(context).textTheme.bodySmall),
      ],
    );
  }
}

class QueueReadout extends StatelessWidget {
  const QueueReadout({required this.queue, required this.strings, super.key});
  final NetworkQueueQuality queue;
  final AppStrings strings;
  @override
  Widget build(BuildContext context) {
    final pressure = queuePressure(queue);
    final tokens = UsqueTokens.of(context);
    final color = queue.dropItems > 0 || (pressure ?? 0) >= .8
        ? tokens.danger
        : (pressure ?? 0) >= .5
        ? tokens.caution
        : tokens.inbound;
    final label = strings.get(
      queue.kind == NetworkQueueKind.unknown
          ? 'nq_unknown_queue'
          : 'nq_${queue.kind.name}',
    );
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: <Widget>[
        Text(label, style: Theme.of(context).textTheme.titleSmall),
        const SizedBox(height: 8),
        if (pressure != null)
          Semantics(
            label: label,
            value: '${(pressure * 100).round()}%',
            child: LinearProgressIndicator(value: pressure, color: color),
          ),
        const SizedBox(height: 8),
        Text(
          pressure == null
              ? strings.get('nq_unavailable')
              : '${strings.get('nq_current_capacity')}: ${queue.currentItems}/${queue.capacityItems} · ${strings.get('nq_high_water')}: ${queue.highWaterItems}',
        ),
        if (queue.availability == MetricAvailability.available) ...<Widget>[
          Text(
            '${strings.get('nq_bytes')}: ${qualityBytes(queue.currentBytes)}/${qualityBytes(queue.capacityBytes)} · ${strings.get('nq_high_water')}: ${qualityBytes(queue.highWaterBytes)}',
          ),
          Text(
            '${strings.get('nq_drops')}: ${queue.dropItems} · ${strings.get('nq_oldest')}: ${queue.oldestAgeMilliseconds == null ? strings.get('nq_not_ready') : '${queue.oldestAgeMilliseconds} ms'}',
          ),
        ],
      ],
    );
  }
}

/// The overall level in display size, with the latest round trip and loss,
/// so the answer to "is my connection OK?" is the first thing on the page.
class QualityVerdict extends StatelessWidget {
  const QualityVerdict({
    required this.label,
    required this.tone,
    required this.icon,
    this.summary,
    super.key,
  });
  final String label;
  final StatusTone tone;
  final IconData icon;
  final String? summary;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return Semantics(
      container: true,
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: <Widget>[
          Padding(
            padding: const EdgeInsets.only(top: 2),
            child: ExcludeSemantics(
              child: Icon(
                icon,
                size: 28,
                color: statusToneColor(context, tone),
              ),
            ),
          ),
          const SizedBox(width: 12),
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: <Widget>[
                Text(label, style: theme.textTheme.headlineSmall),
                if (summary case final summary?) ...<Widget>[
                  const SizedBox(height: 4),
                  Text(
                    summary,
                    style: theme.textTheme.bodyMedium?.copyWith(
                      color: theme.colorScheme.onSurfaceVariant,
                    ),
                  ),
                ],
              ],
            ),
          ),
        ],
      ),
    );
  }
}

InlineStatus qualityPhaseStatus(AppStrings strings, String phase) =>
    InlineStatus(
      label: qualityPhaseLabel(strings, phase),
      tone: qualityPhaseTone(phase),
    );
