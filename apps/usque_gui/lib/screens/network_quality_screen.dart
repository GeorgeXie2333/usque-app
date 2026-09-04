import 'dart:async';

import 'package:flutter/material.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../core/app_strings.dart';
import '../core/usque_theme.dart';
import '../models/app_models.dart';
import '../models/diagnostics_models.dart';
import '../models/network_quality_models.dart';
import '../state/app_controller.dart';
import '../widgets/common.dart';
import '../widgets/sparkline.dart';
import 'diagnostics_screen.dart';

class NetworkQualityScreen extends StatelessWidget {
  const NetworkQualityScreen({required this.controller, super.key});
  final AppController controller;

  @override
  Widget build(BuildContext context) {
    return ListenableBuilder(
      listenable: Listenable.merge(<Listenable>[
        controller,
        controller.quality,
        controller.diagnostics,
      ]),
      builder: (context, _) => _build(context),
    );
  }

  Widget _build(BuildContext context) {
    final s = controller.strings;
    final state = controller.quality;
    final snapshot = state.latest;
    final metrics = snapshot?.metrics ?? const NetworkConnectionMetrics();
    final transport = controller.snapshot.transport?.toLowerCase();
    final h2 = const <String>{'h2', 'http2', 'http/2'}.contains(transport);
    final h3 = const <String>{'h3', 'http3', 'http/3'}.contains(transport);
    final tokens = UsqueTokens.of(context);
    final connected = controller.snapshot.isConnected;
    final supported = controller.engineCapabilities?.networkQuality ?? false;
    final doctorBusy =
        controller.diagnostics.isActive ||
        controller.diagnostics.state == DiagnosticsControllerState.starting ||
        controller.diagnostics.state == DiagnosticsControllerState.cancelling;
    final level = !connected
        ? 'disconnected'
        : state.stale
        ? 'limited'
        : switch (snapshot?.level) {
            NetworkQualityLevel.good => 'good',
            NetworkQualityLevel.fair => 'fair',
            NetworkQualityLevel.poor => 'poor',
            _ => 'limited',
          };
    String ms(int? value, MetricAvailability availability) =>
        availableMetric(value, availability) == null
        ? s.get('nq_unavailable')
        : '$value ms';
    String exactBytes(int? value) =>
        value == null ? s.get('nq_unavailable') : '$value B';
    final rtt = state.trace((point) => point.rttMilliseconds);
    final down = state.trace((point) => point.downloadBytesPerSecond);
    final up = state.trace((point) => point.uploadBytesPerSecond);
    final loss = state.trace((point) => point.lossBasisPoints);
    final lossValue = h2
        ? null
        : availableMetric(
            metrics.intervalLossBasisPoints,
            metrics.intervalLossAvailability,
          );
    final pmtu = snapshot?.pmtu ?? const PmtuQualityInfo();
    final migration = snapshot?.migration ?? const MigrationQualityInfo();
    final dns = snapshot?.directDns ?? const DirectDnsQualityInfo();
    final migrationKnown = connected && h3 && snapshot != null;
    final dnsKnown =
        connected &&
        (dns.mode == DirectDnsMode.doh || dns.mode == DirectDnsMode.dot);
    String count(int value, bool known) =>
        known ? '$value' : s.get('nq_unavailable');
    final queues = snapshot?.queues ?? const <NetworkQueueQuality>[];
    final mainQueues = queues
        .where(
          (queue) =>
              queue.kind != NetworkQueueKind.h3DatagramSend &&
              queue.kind != NetworkQueueKind.h3WireSend,
        )
        .toList(growable: false);
    final lowQueues = queues
        .where(
          (queue) =>
              queue.kind == NetworkQueueKind.h3DatagramSend ||
              queue.kind == NetworkQueueKind.h3WireSend,
        )
        .toList(growable: false);

    return FocusTraversalGroup(
      policy: ReadingOrderTraversalPolicy(),
      child: SubPage(
        title: s.get('network_quality'),
        backLabel: s.get('back'),
        subtitle: s.get('nq_subtitle'),
        actions: <Widget>[
          FilledButton.icon(
            key: const ValueKey<String>('network-doctor-standard'),
            onPressed: doctorBusy
                ? null
                : () {
                    unawaited(
                      controller.diagnostics.start(DiagnosticMode.standard),
                    );
                    unawaited(
                      Navigator.of(context).push<void>(
                        MaterialPageRoute<void>(
                          builder: (_) =>
                              DiagnosticsScreen(controller: controller),
                        ),
                      ),
                    );
                  },
            icon: const Icon(LucideIcons.stethoscope),
            label: Text(s.get('nq_doctor')),
          ),
        ],
        child: PanelStack(
          children: <Widget>[
            if (!supported)
              WarningBanner(
                title: s.get('nq_unsupported'),
                message: s.get('nq_capability_missing'),
              ),
            Panel(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: <Widget>[
                  Wrap(
                    spacing: 10,
                    runSpacing: 10,
                    crossAxisAlignment: WrapCrossAlignment.center,
                    children: <Widget>[
                      StatusPill(
                        label: s.get('nq_$level'),
                        tone: switch (level) {
                          'good' => StatusTone.success,
                          'fair' => StatusTone.warning,
                          'poor' => StatusTone.danger,
                          _ => StatusTone.neutral,
                        },
                        icon: level == 'good'
                            ? LucideIcons.circleCheck
                            : LucideIcons.activity,
                      ),
                      Text(
                        s.get(
                          controller.snapshot.isTransitional
                              ? 'nq_connecting'
                              : connected
                              ? 'nq_connected'
                              : 'nq_disconnected',
                        ),
                      ),
                      if (connected)
                        Text(
                          h2
                              ? 'HTTP/2'
                              : h3
                              ? 'HTTP/3'
                              : s.get('nq_unavailable'),
                        ),
                      if (connected)
                        Text(switch (controller.snapshot.addressFamily
                            ?.toLowerCase()) {
                          'ipv4' => 'IPv4',
                          'ipv6' => 'IPv6',
                          _ => s.get('nq_unavailable'),
                        }),
                      if (connected)
                        Text(
                          s.get(state.stale ? 'nq_stale' : 'nq_live'),
                          style: TextStyle(
                            color: state.stale
                                ? tokens.caution
                                : tokens.success,
                          ),
                        ),
                    ],
                  ),
                  const SizedBox(height: 12),
                  Text(
                    !connected
                        ? s.get('nq_empty')
                        : state.stale
                        ? s.get('nq_stale_help')
                        : '${s.get('nq_updated')}: ${state.sampleAge?.inSeconds ?? 0} ${s.get('nq_seconds')}',
                  ),
                  const SizedBox(height: 8),
                  Text(
                    s.get('nq_local_only'),
                    style: Theme.of(context).textTheme.bodySmall,
                  ),
                ],
              ),
            ),
            Wrap(
              spacing: 12,
              runSpacing: 8,
              crossAxisAlignment: WrapCrossAlignment.center,
              children: <Widget>[
                Semantics(
                  header: true,
                  child: Text(
                    s.get('nq_trends'),
                    style: Theme.of(context).textTheme.titleMedium,
                  ),
                ),
                TextButton.icon(
                  onPressed: state.togglePaused,
                  icon: Icon(
                    state.paused ? LucideIcons.play : LucideIcons.pause,
                  ),
                  label: Text(s.get(state.paused ? 'nq_resume' : 'nq_pause')),
                ),
                if (state.paused) Text(s.get('nq_paused')),
                Text(
                  s.get('nq_gaps'),
                  style: Theme.of(context).textTheme.bodySmall,
                ),
              ],
            ),
            _QualityGrid(
              children: <Widget>[
                _QualityPanel(
                  title: s.get('nq_rtt'),
                  icon: LucideIcons.timer,
                  children: <Widget>[
                    _Readouts(
                      values: <(String, String)>[
                        (
                          s.get('nq_latest'),
                          ms(
                            metrics.latestRttMilliseconds,
                            metrics.latestRttAvailability,
                          ),
                        ),
                        (
                          s.get('nq_smoothed'),
                          ms(
                            metrics.smoothedRttMilliseconds,
                            metrics.smoothedRttAvailability,
                          ),
                        ),
                        (
                          s.get('nq_minimum'),
                          ms(
                            metrics.minimumRttMilliseconds,
                            metrics.minimumRttAvailability,
                          ),
                        ),
                      ],
                    ),
                    _Trace(
                      samples: rtt,
                      color: tokens.inbound,
                      label: s.get('nq_rtt'),
                      format: (value) => '$value ms',
                      strings: s,
                    ),
                    Text(
                      s.get(h2 ? 'nq_h2_ping' : 'nq_h3_rtt'),
                      style: Theme.of(context).textTheme.bodySmall,
                    ),
                  ],
                ),
                _QualityPanel(
                  title: s.get('nq_throughput'),
                  icon: LucideIcons.gauge,
                  children: <Widget>[
                    _Readouts(
                      values: <(String, String)>[
                        (
                          '${s.get('nq_download')} · ${s.get('nq_one_second')}',
                          qualityByteRate(
                            state.rateAverage(download: true, seconds: 1),
                          ),
                        ),
                        (
                          '${s.get('nq_upload')} · ${s.get('nq_one_second')}',
                          qualityByteRate(
                            state.rateAverage(download: false, seconds: 1),
                          ),
                        ),
                        (
                          '${s.get('nq_download')} · ${s.get('nq_five_seconds')}',
                          qualityByteRate(
                            state.rateAverage(download: true, seconds: 5),
                          ),
                        ),
                        (
                          '${s.get('nq_upload')} · ${s.get('nq_five_seconds')}',
                          qualityByteRate(
                            state.rateAverage(download: false, seconds: 5),
                          ),
                        ),
                      ],
                    ),
                    _Trace(
                      samples: down,
                      color: tokens.inbound,
                      label: s.get('nq_download'),
                      format: qualityByteRate,
                      strings: s,
                    ),
                    _Trace(
                      samples: up,
                      color: tokens.outbound,
                      label: s.get('nq_upload'),
                      format: qualityByteRate,
                      strings: s,
                    ),
                  ],
                ),
                _QualityPanel(
                  title: s.get('nq_loss'),
                  icon: LucideIcons.chartNoAxesCombined,
                  children: <Widget>[
                    _Readouts(
                      values: <(String, String)>[
                        (
                          s.get('nq_loss'),
                          lossValue == null
                              ? s.get('nq_unavailable')
                              : '${(lossValue / 100).toStringAsFixed(2)}%',
                        ),
                      ],
                    ),
                    if (!h2)
                      _Trace(
                        samples: loss,
                        color: tokens.caution,
                        label: s.get('nq_loss'),
                        format: (value) =>
                            '${(value / 100).toStringAsFixed(2)}%',
                        strings: s,
                      ),
                    Text(
                      s.get(h2 ? 'nq_loss_h2' : 'nq_loss_interval'),
                      style: Theme.of(context).textTheme.bodySmall,
                    ),
                  ],
                ),
                _QualityPanel(
                  title: s.get(h2 ? 'nq_h2_window' : 'nq_congestion'),
                  icon: LucideIcons.waves,
                  children: <Widget>[
                    _Readouts(
                      values: h2
                          ? <(String, String)>[
                              (
                                s.get('nq_stream_window'),
                                metrics.h2StreamReceiveWindowBytes > 0
                                    ? qualityBytes(
                                        metrics.h2StreamReceiveWindowBytes,
                                      )
                                    : s.get('nq_unavailable'),
                              ),
                              (
                                s.get('nq_connection_window'),
                                metrics.h2ConnectionReceiveWindowBytes > 0
                                    ? qualityBytes(
                                        metrics.h2ConnectionReceiveWindowBytes,
                                      )
                                    : s.get('nq_unavailable'),
                              ),
                              (
                                s.get('nq_stalls'),
                                count(
                                  metrics.h2FlowControlStallCount,
                                  metrics.h2StreamReceiveWindowBytes > 0,
                                ),
                              ),
                            ]
                          : <(String, String)>[
                              (
                                s.get('nq_cwnd'),
                                qualityBytes(
                                  availableMetric(
                                    metrics.congestionWindowBytes,
                                    metrics.congestionWindowAvailability,
                                  ),
                                ),
                              ),
                              (
                                s.get('nq_in_flight'),
                                qualityBytes(
                                  availableMetric(
                                    metrics.bytesInFlight,
                                    metrics.bytesInFlightAvailability,
                                  ),
                                ),
                              ),
                              (
                                s.get('nq_send_rate'),
                                metrics.sendRateAvailability ==
                                            MetricAvailability.available &&
                                        metrics.sendRateBitsPerSecond != null
                                    ? qualityByteRate(
                                        metrics.sendRateBitsPerSecond! ~/ 8,
                                      )
                                    : s.get('nq_unavailable'),
                              ),
                            ],
                    ),
                  ],
                ),
              ],
            ),
            _QualityGrid(
              children: <Widget>[
                _QualityPanel(
                  title: s.get('nq_pmtu'),
                  icon: LucideIcons.scanLine,
                  children: <Widget>[
                    Text(_phase(s, h2 ? 'unsupported' : pmtu.phaseCode)),
                    _Readouts(
                      values: <(String, String)>[
                        (
                          s.get('nq_outer_pmtu'),
                          h2
                              ? s.get('nq_unsupported')
                              : exactBytes(
                                  availableMetric(
                                    pmtu.outerPmtuBytes,
                                    pmtu.availability,
                                  ),
                                ),
                        ),
                        (
                          s.get('nq_inner_payload'),
                          h2
                              ? s.get('nq_unsupported')
                              : exactBytes(
                                  availableMetric(
                                    pmtu.effectiveConnectIpPayloadBytes,
                                    pmtu.effectivePayloadAvailability,
                                  ),
                                ),
                        ),
                      ],
                    ),
                    Text(
                      s.get('nq_pmtu_help'),
                      style: Theme.of(context).textTheme.bodySmall,
                    ),
                  ],
                ),
                _QualityPanel(
                  title: s.get('nq_migration'),
                  icon: LucideIcons.route,
                  children: <Widget>[
                    Text(_phase(s, h2 ? 'unsupported' : migration.phaseCode)),
                    _Readouts(
                      values: <(String, String)>[
                        (
                          s.get('nq_attempts'),
                          count(migration.attemptCount, migrationKnown),
                        ),
                        (
                          s.get('nq_successes'),
                          count(migration.successCount, migrationKnown),
                        ),
                        (
                          s.get('nq_failures'),
                          count(migration.failureCount, migrationKnown),
                        ),
                        (
                          s.get('nq_last_duration'),
                          !migrationKnown ||
                                  migration.lastDurationMilliseconds == null
                              ? s.get('nq_not_ready')
                              : '${migration.lastDurationMilliseconds} ms',
                        ),
                      ],
                    ),
                    if (migration.lastReasonCode.isNotEmpty)
                      Text(_reason(s, migration.lastReasonCode)),
                    Text(
                      s.get('nq_migration_help'),
                      style: Theme.of(context).textTheme.bodySmall,
                    ),
                  ],
                ),
                _QualityPanel(
                  title: s.get('nq_direct_dns'),
                  icon: LucideIcons.shieldCheck,
                  children: <Widget>[
                    Text(
                      s.get(switch (dns.mode) {
                        DirectDnsMode.physicalSystem => 'nq_system_dns',
                        DirectDnsMode.doh => 'nq_doh',
                        DirectDnsMode.dot => 'nq_dot',
                        _ => 'nq_unavailable',
                      }),
                    ),
                    Text(
                      s.get(switch (dns.phaseCode) {
                        'ready' || 'system' => 'nq_ready',
                        'degraded' => 'nq_degraded',
                        'connecting' => 'nq_connecting',
                        _ => 'nq_not_ready',
                      }),
                    ),
                    _Readouts(
                      values: <(String, String)>[
                        (
                          s.get('nq_last_rtt'),
                          !dnsKnown || dns.lastRttMilliseconds == null
                              ? s.get('nq_not_ready')
                              : '${dns.lastRttMilliseconds} ms',
                        ),
                        (
                          s.get('nq_successes'),
                          count(dns.successCount, dnsKnown),
                        ),
                        (
                          s.get('nq_failures'),
                          count(dns.failureCount, dnsKnown),
                        ),
                        (
                          s.get('nq_timeouts'),
                          count(dns.timeoutCount, dnsKnown),
                        ),
                      ],
                    ),
                    Text(
                      s.get('nq_dns_redacted'),
                      style: Theme.of(context).textTheme.bodySmall,
                    ),
                  ],
                ),
              ],
            ),
            _QualityPanel(
              title: s.get('nq_queues'),
              icon: LucideIcons.layers3,
              children: <Widget>[
                if (queues.isEmpty) Text(s.get('nq_queue_empty')),
                for (final queue in mainQueues)
                  _QueueReadout(queue: queue, strings: s),
                if (lowQueues.isNotEmpty)
                  ExpansionTile(
                    // Keep expansion state separate from PageFrame's scroll offset.
                    key: const PageStorageKey<String>(
                      'network-quality-low-level-queues',
                    ),
                    title: Text(s.get('nq_queue_details')),
                    children: <Widget>[
                      for (final queue in lowQueues)
                        Padding(
                          padding: const EdgeInsets.only(bottom: 16),
                          child: _QueueReadout(queue: queue, strings: s),
                        ),
                    ],
                  ),
              ],
            ),
            Text(s.get('nq_doctor_help')),
            Text(
              s.get('nq_doctor_evidence'),
              style: Theme.of(context).textTheme.bodySmall,
            ),
          ],
        ),
      ),
    );
  }
}

String _phase(AppStrings strings, String value) {
  const allowed = <String>{
    'idle',
    'preparing_socket',
    'probing',
    'validated',
    'promoting',
    'stable',
    'aborted',
    'revalidating',
    'degraded',
    'unsupported',
  };
  return strings.get('nq_phase_${allowed.contains(value) ? value : 'unknown'}');
}

String _reason(AppStrings strings, String value) {
  const allowed = <String>{
    'family_unavailable',
    'socket_protect_failed',
    'generation_changed_during_setup',
    'peer_cid_unavailable',
    'local_cid_unavailable',
    'path_probe_rejected',
    'path_validation_timeout',
    'superseded',
    'promotion_failed',
    'connection_closed',
    'unsupported',
  };
  return strings.get(
    'nq_reason_${allowed.contains(value) ? value : 'unknown'}',
  );
}

class _QualityGrid extends StatelessWidget {
  const _QualityGrid({required this.children});
  final List<Widget> children;
  @override
  Widget build(BuildContext context) => LayoutBuilder(
    builder: (context, constraints) {
      final scale = MediaQuery.textScalerOf(context).scale(16) / 16;
      final columns = constraints.maxWidth >= 700 * scale ? 2 : 1;
      final width = (constraints.maxWidth - (columns - 1) * 16) / columns;
      return Wrap(
        spacing: 16,
        runSpacing: 16,
        children: <Widget>[
          for (final child in children) SizedBox(width: width, child: child),
        ],
      );
    },
  );
}

class _QualityPanel extends StatelessWidget {
  const _QualityPanel({
    required this.title,
    required this.icon,
    required this.children,
  });
  final String title;
  final IconData icon;
  final List<Widget> children;
  @override
  Widget build(BuildContext context) => Panel(
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

class _Readouts extends StatelessWidget {
  const _Readouts({required this.values});
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

class _Trace extends StatelessWidget {
  const _Trace({
    required this.samples,
    required this.color,
    required this.label,
    required this.strings,
    required this.format,
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

class _QueueReadout extends StatelessWidget {
  const _QueueReadout({required this.queue, required this.strings});
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
