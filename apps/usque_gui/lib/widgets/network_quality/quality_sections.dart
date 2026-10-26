import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../../core/quality_presentation.dart';
import '../../core/usque_theme.dart';
import '../../models/app_models.dart';
import '../../models/network_quality_models.dart';
import '../../state/app_controller.dart';
import '../common.dart';
import '../context_help_button.dart';
import '../listenable_selector.dart';
import 'quality_layout.dart';

/// Connection facts every quality section reads. They change only when the
/// session or locale does, never with an ordinary sample.
typedef QualityLink = ({
  String catalog,
  bool connected,
  bool transitional,
  bool h2,
  bool h3,
  bool l4,
  String? family,
});

QualityLink qualityLinkOf(AppController app) {
  final transport = app.snapshot.transport?.toLowerCase();
  return (
    catalog: app.strings.catalogId,
    connected: app.snapshot.isConnected,
    transitional: app.snapshot.isTransitional,
    h2: const <String>{'h2', 'http2', 'http/2'}.contains(transport),
    h3: const <String>{'h3', 'http3', 'http/3'}.contains(transport),
    l4: app.snapshot.dataPlane == DataPlaneMode.l4Proxy,
    family: app.snapshot.addressFamily,
  );
}

/// Follows the connection and its quality samples; rebuilds only when the
/// selected value changes, so slow-moving sections skip ordinary samples.
class _QualitySelector<T> extends StatelessWidget {
  const _QualitySelector({
    required this.controller,
    required this.selector,
    required this.builder,
    this.equals,
  });
  final AppController controller;
  final T Function(AppController app) selector;
  final Widget Function(BuildContext context, T value) builder;
  final bool Function(T previous, T next)? equals;

  @override
  Widget build(BuildContext context) => ListenableSelector<T>(
    listenables: <Listenable>[controller, controller.quality],
    selector: () => selector(controller),
    equals: equals,
    builder: builder,
  );
}

int? _loss(QualityLink link, NetworkConnectionMetrics metrics) => link.h2
    ? null
    : availableMetric(
        metrics.intervalLossBasisPoints,
        metrics.intervalLossAvailability,
      );

String _count(AppController app, int value, bool known) =>
    known ? '$value' : app.strings.get('nq_unavailable');

/// Verdict, connection facts and sample freshness.
class QualityOverview extends StatelessWidget {
  const QualityOverview({required this.controller, super.key});
  final AppController controller;

  @override
  Widget build(BuildContext context) => _QualitySelector(
    controller: controller,
    selector: (app) => (
      link: qualityLinkOf(app),
      stale: app.quality.stale,
      level: app.quality.latest?.level,
      sampled: app.quality.latest != null,
      metrics: app.quality.latest?.metrics ?? const NetworkConnectionMetrics(),
      age: app.quality.sampleAge?.inSeconds ?? 0,
    ),
    builder: (context, view) {
      final s = controller.strings;
      final tokens = UsqueTokens.of(context);
      final link = view.link;
      final level = QualityLevelPresentation.of(
        connected: link.connected,
        stale: view.stale,
        level: view.level,
      );
      final loss = _loss(link, view.metrics);
      return ContentSection(
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: <Widget>[
            QualityVerdict(
              label: s.get(level.labelKey),
              tone: level.tone,
              icon: level.icon,
              // The two readings most users act on; everything else is in
              // the sections below.
              summary: link.connected && !view.stale && view.sampled
                  ? [
                      '${s.get('nq_rtt')} ${formatRoundTrip(s, view.metrics.latestRttMilliseconds, view.metrics.latestRttAvailability)}',
                      if (loss != null)
                        '${s.get('nq_loss')} ${formatLossBasisPoints(loss)}',
                    ].join(' · ')
                  : null,
            ),
            const SizedBox(height: 14),
            Wrap(
              spacing: 10,
              runSpacing: 10,
              crossAxisAlignment: WrapCrossAlignment.center,
              children: <Widget>[
                Text(
                  s.get(
                    link.transitional
                        ? 'nq_connecting'
                        : link.connected
                        ? 'nq_connected'
                        : 'nq_disconnected',
                  ),
                ),
                if (link.connected)
                  Text(
                    link.l4
                        ? 'L4 / HTTP/3'
                        : link.h2
                        ? 'HTTP/2'
                        : link.h3
                        ? 'HTTP/3'
                        : s.get('nq_unavailable'),
                  ),
                if (link.connected)
                  Text(switch (link.family?.toLowerCase()) {
                    'ipv4' => 'IPv4',
                    'ipv6' => 'IPv6',
                    _ => s.get('nq_unavailable'),
                  }),
                if (link.connected)
                  Text(
                    s.get(view.stale ? 'nq_stale' : 'nq_live'),
                    style: TextStyle(
                      color: view.stale ? tokens.caution : tokens.success,
                    ),
                  ),
              ],
            ),
            const SizedBox(height: 12),
            Text(
              !link.connected
                  ? s.get('nq_empty')
                  : view.stale
                  ? s.get('nq_stale_help')
                  : '${s.get('nq_updated')}: ${s.get('nq_seconds').replaceAll('{count}', '${view.age}')}',
            ),
            const SizedBox(height: 8),
            Text(
              s.get('nq_local_only'),
              style: Theme.of(context).textTheme.bodySmall,
            ),
          ],
        ),
      );
    },
  );
}

/// The trends heading with its pause control.
class QualityTrendsHeading extends StatelessWidget {
  const QualityTrendsHeading({required this.controller, super.key});
  final AppController controller;

  @override
  Widget build(BuildContext context) => _QualitySelector(
    controller: controller,
    selector: (app) =>
        (catalog: app.strings.catalogId, paused: app.quality.paused),
    builder: (context, view) {
      final s = controller.strings;
      return Wrap(
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
            onPressed: controller.quality.togglePaused,
            icon: Icon(view.paused ? LucideIcons.play : LucideIcons.pause),
            label: Text(s.get(view.paused ? 'nq_resume' : 'nq_pause')),
          ),
          if (view.paused) Text(s.get('nq_paused')),
          ContextHelpButton(
            title: s.get('nq_trends'),
            message: s.get('nq_gaps'),
            strings: s,
          ),
        ],
      );
    },
  );
}

/// Round trip, throughput and loss: the readings that move every second.
class QualityTrends extends StatelessWidget {
  const QualityTrends({required this.controller, super.key});
  final AppController controller;

  @override
  Widget build(BuildContext context) => _QualitySelector(
    controller: controller,
    selector: (app) {
      final quality = app.quality;
      return (
        link: qualityLinkOf(app),
        metrics: quality.latest?.metrics ?? const NetworkConnectionMetrics(),
        rtt: quality.traceFor(NetworkQualityTrace.rtt),
        down: quality.traceFor(NetworkQualityTrace.download),
        up: quality.traceFor(NetworkQualityTrace.upload),
        loss: quality.traceFor(NetworkQualityTrace.loss),
        rates: (
          down1: quality.rateAverage(download: true, seconds: 1),
          up1: quality.rateAverage(download: false, seconds: 1),
          down5: quality.rateAverage(download: true, seconds: 5),
          up5: quality.rateAverage(download: false, seconds: 5),
        ),
      );
    },
    equals: (a, b) =>
        a.link == b.link &&
        a.metrics == b.metrics &&
        a.rates == b.rates &&
        listEquals(a.rtt, b.rtt) &&
        listEquals(a.down, b.down) &&
        listEquals(a.up, b.up) &&
        listEquals(a.loss, b.loss),
    builder: (context, view) {
      final s = controller.strings;
      final tokens = UsqueTokens.of(context);
      final metrics = view.metrics;
      final h2 = view.link.h2;
      final loss = _loss(view.link, metrics);
      return QualitySectionList(
        children: <Widget>[
          QualitySection(
            title: s.get('nq_rtt'),
            icon: LucideIcons.timer,
            children: <Widget>[
              QualityReadouts(
                values: <(String, String)>[
                  (
                    s.get('nq_latest'),
                    formatRoundTrip(
                      s,
                      metrics.latestRttMilliseconds,
                      metrics.latestRttAvailability,
                    ),
                  ),
                  (
                    s.get('nq_smoothed'),
                    formatRoundTrip(
                      s,
                      metrics.smoothedRttMilliseconds,
                      metrics.smoothedRttAvailability,
                    ),
                  ),
                  (
                    s.get('nq_minimum'),
                    formatRoundTrip(
                      s,
                      metrics.minimumRttMilliseconds,
                      metrics.minimumRttAvailability,
                    ),
                  ),
                ],
              ),
              QualityTrace(
                samples: view.rtt,
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
          QualitySection(
            title: s.get('nq_throughput'),
            icon: LucideIcons.gauge,
            children: <Widget>[
              QualityReadouts(
                values: <(String, String)>[
                  (
                    '${s.get('nq_download')} · ${s.get('nq_one_second')}',
                    qualityByteRate(view.rates.down1),
                  ),
                  (
                    '${s.get('nq_upload')} · ${s.get('nq_one_second')}',
                    qualityByteRate(view.rates.up1),
                  ),
                  (
                    '${s.get('nq_download')} · ${s.get('nq_five_seconds')}',
                    qualityByteRate(view.rates.down5),
                  ),
                  (
                    '${s.get('nq_upload')} · ${s.get('nq_five_seconds')}',
                    qualityByteRate(view.rates.up5),
                  ),
                ],
              ),
              QualityTrace(
                samples: view.down,
                color: tokens.inbound,
                label: s.get('nq_download'),
                format: qualityByteRate,
                strings: s,
              ),
              QualityTrace(
                samples: view.up,
                color: tokens.outbound,
                label: s.get('nq_upload'),
                format: qualityByteRate,
                strings: s,
              ),
            ],
          ),
          QualitySection(
            title: s.get('nq_loss'),
            icon: LucideIcons.chartNoAxesCombined,
            children: <Widget>[
              QualityReadouts(
                values: <(String, String)>[
                  (
                    s.get('nq_loss'),
                    loss == null
                        ? s.get('nq_unavailable')
                        : formatLossBasisPoints(loss),
                  ),
                ],
              ),
              if (!h2)
                QualityTrace(
                  samples: view.loss,
                  color: tokens.caution,
                  label: s.get('nq_loss'),
                  format: formatLossBasisPoints,
                  strings: s,
                ),
              Text(
                s.get(h2 ? 'nq_loss_h2' : 'nq_loss_interval'),
                style: Theme.of(context).textTheme.bodySmall,
              ),
            ],
          ),
        ],
      );
    },
  );
}

String _pmtuPhase(QualityLink link, NetworkQualitySnapshot? snapshot) => link.h2
    ? 'unsupported'
    : (snapshot?.pmtu ?? const PmtuQualityInfo()).phaseCode;

String _migrationPhase(QualityLink link, NetworkQualitySnapshot? snapshot) =>
    link.h2
    ? 'unsupported'
    : (snapshot?.migration ?? const MigrationQualityInfo()).phaseCode;

/// Transport internals for troubleshooting, folded below the everyday
/// readings. Opens on first view when one of them needs attention.
class QualityAdvancedMetrics extends StatelessWidget {
  const QualityAdvancedMetrics({
    required this.controller,
    required this.onDiagnose,
    super.key,
  });
  final AppController controller;

  /// Opens Diagnostics, offered beside the count of degraded phases.
  final VoidCallback onDiagnose;

  @override
  Widget build(BuildContext context) => _QualitySelector(
    controller: controller,
    selector: (app) {
      final link = qualityLinkOf(app);
      final latest = app.quality.latest;
      return (
        catalog: link.catalog,
        attention: qualityAttentionCount(
          connected: link.connected,
          pmtuPhase: _pmtuPhase(link, latest),
          migrationPhase: _migrationPhase(link, latest),
          directDnsPhase:
              (latest?.directDns ?? const DirectDnsQualityInfo()).phaseCode,
        ),
      );
    },
    builder: (context, view) {
      final s = controller.strings;
      return ExpansionTile(
        key: const PageStorageKey<String>('network-quality-advanced-metrics'),
        initiallyExpanded: view.attention > 0,
        tilePadding: EdgeInsets.zero,
        childrenPadding: EdgeInsets.zero,
        expandedCrossAxisAlignment: CrossAxisAlignment.stretch,
        title: Text(
          s.get('nq_advanced_metrics'),
          style: Theme.of(context).textTheme.titleMedium,
        ),
        subtitle: view.attention == 0
            ? null
            : Align(
                alignment: AlignmentDirectional.centerStart,
                child: InlineStatus(
                  label: s
                      .get('nq_attention_count')
                      .replaceAll('{count}', '${view.attention}'),
                  tone: StatusTone.warning,
                ),
              ),
        children: <Widget>[
          // Readings say what degraded; Diagnostics can say why.
          if (view.attention > 0)
            Padding(
              padding: const EdgeInsets.only(top: 12),
              child: Wrap(
                spacing: 8,
                runSpacing: 4,
                crossAxisAlignment: WrapCrossAlignment.center,
                children: <Widget>[
                  Text(s.get('nq_attention_hint')),
                  TextButton.icon(
                    key: const ValueKey('network-quality-attention-diagnose'),
                    onPressed: onDiagnose,
                    icon: const Icon(LucideIcons.stethoscope),
                    label: Text(s.get('diag_start')),
                  ),
                ],
              ),
            ),
          QualitySectionList(
            children: <Widget>[
              _Congestion(controller: controller),
              _PathMtu(controller: controller),
              _Migration(controller: controller),
              _DirectDns(controller: controller),
            ],
          ),
          _Queues(controller: controller),
        ],
      );
    },
  );
}

class _Congestion extends StatelessWidget {
  const _Congestion({required this.controller});
  final AppController controller;

  @override
  Widget build(BuildContext context) => _QualitySelector(
    controller: controller,
    selector: (app) => (
      link: qualityLinkOf(app),
      metrics: app.quality.latest?.metrics ?? const NetworkConnectionMetrics(),
    ),
    builder: (context, view) {
      final s = controller.strings;
      final metrics = view.metrics;
      final h2 = view.link.h2;
      return QualitySection(
        title: s.get(h2 ? 'nq_h2_window' : 'nq_congestion'),
        icon: LucideIcons.waves,
        children: <Widget>[
          QualityReadouts(
            values: h2
                ? <(String, String)>[
                    (
                      s.get('nq_stream_window'),
                      metrics.h2StreamReceiveWindowBytes > 0
                          ? qualityBytes(metrics.h2StreamReceiveWindowBytes)
                          : s.get('nq_unavailable'),
                    ),
                    (
                      s.get('nq_connection_window'),
                      metrics.h2ConnectionReceiveWindowBytes > 0
                          ? qualityBytes(metrics.h2ConnectionReceiveWindowBytes)
                          : s.get('nq_unavailable'),
                    ),
                    (
                      s.get('nq_stalls'),
                      _count(
                        controller,
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
                          ? qualityByteRate(metrics.sendRateBitsPerSecond! ~/ 8)
                          : s.get('nq_unavailable'),
                    ),
                  ],
          ),
        ],
      );
    },
  );
}

class _PathMtu extends StatelessWidget {
  const _PathMtu({required this.controller});
  final AppController controller;

  @override
  Widget build(BuildContext context) => _QualitySelector(
    controller: controller,
    selector: (app) => (
      link: qualityLinkOf(app),
      pmtu: app.quality.latest?.pmtu ?? const PmtuQualityInfo(),
      phase: _pmtuPhase(qualityLinkOf(app), app.quality.latest),
    ),
    builder: (context, view) {
      final s = controller.strings;
      final pmtu = view.pmtu;
      String exactBytes(int? value) =>
          value == null ? s.get('nq_unavailable') : '$value B';
      return QualitySection(
        title: s.get('nq_pmtu'),
        icon: LucideIcons.scanLine,
        help: ContextHelpButton(
          title: s.get('nq_pmtu'),
          message: s.get('nq_pmtu_help'),
          strings: s,
        ),
        children: <Widget>[
          qualityPhaseStatus(s, view.phase),
          QualityReadouts(
            values: <(String, String)>[
              (
                s.get('nq_outer_pmtu'),
                view.link.h2
                    ? s.get('nq_unsupported')
                    : exactBytes(
                        availableMetric(pmtu.outerPmtuBytes, pmtu.availability),
                      ),
              ),
              (
                s.get('nq_inner_payload'),
                view.link.l4
                    ? s.get('l4_not_applicable')
                    : view.link.h2
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
        ],
      );
    },
  );
}

class _Migration extends StatelessWidget {
  const _Migration({required this.controller});
  final AppController controller;

  @override
  Widget build(BuildContext context) => _QualitySelector(
    controller: controller,
    selector: (app) {
      final link = qualityLinkOf(app);
      final latest = app.quality.latest;
      return (
        link: link,
        migration: latest?.migration ?? const MigrationQualityInfo(),
        phase: _migrationPhase(link, latest),
        known: link.connected && link.h3 && latest != null,
      );
    },
    builder: (context, view) {
      final s = controller.strings;
      final migration = view.migration;
      final known = view.known;
      return QualitySection(
        title: s.get('nq_migration'),
        icon: LucideIcons.route,
        help: ContextHelpButton(
          title: s.get('nq_migration'),
          message: s.get('nq_migration_help'),
          strings: s,
        ),
        children: <Widget>[
          qualityPhaseStatus(s, view.phase),
          QualityReadouts(
            values: <(String, String)>[
              (
                s.get('nq_attempts'),
                _count(controller, migration.attemptCount, known),
              ),
              (
                s.get('nq_successes'),
                _count(controller, migration.successCount, known),
              ),
              (
                s.get('nq_failures'),
                _count(controller, migration.failureCount, known),
              ),
              (
                s.get('nq_last_duration'),
                !known || migration.lastDurationMilliseconds == null
                    ? s.get('nq_not_ready')
                    : '${migration.lastDurationMilliseconds} ms',
              ),
            ],
          ),
          if (migration.lastReasonCode.isNotEmpty)
            Text(qualityMigrationReasonLabel(s, migration.lastReasonCode)),
        ],
      );
    },
  );
}

class _DirectDns extends StatelessWidget {
  const _DirectDns({required this.controller});
  final AppController controller;

  @override
  Widget build(BuildContext context) => _QualitySelector(
    controller: controller,
    selector: (app) => (
      link: qualityLinkOf(app),
      dns: app.quality.latest?.directDns ?? const DirectDnsQualityInfo(),
    ),
    builder: (context, view) {
      final s = controller.strings;
      final dns = view.dns;
      final known =
          view.link.connected &&
          (dns.mode == DirectDnsMode.doh || dns.mode == DirectDnsMode.dot);
      final phase = directDnsPhase(dns.phaseCode);
      return QualitySection(
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
          InlineStatus(label: s.get(phase.labelKey), tone: phase.tone),
          QualityReadouts(
            values: <(String, String)>[
              (
                s.get('nq_last_rtt'),
                !known || dns.lastRttMilliseconds == null
                    ? s.get('nq_not_ready')
                    : '${dns.lastRttMilliseconds} ms',
              ),
              (
                s.get('nq_successes'),
                _count(controller, dns.successCount, known),
              ),
              (
                s.get('nq_failures'),
                _count(controller, dns.failureCount, known),
              ),
              (
                s.get('nq_timeouts'),
                _count(controller, dns.timeoutCount, known),
              ),
            ],
          ),
        ],
      );
    },
  );
}

bool _lowLevelQueue(NetworkQueueQuality queue) =>
    queue.kind == NetworkQueueKind.h3DatagramSend ||
    queue.kind == NetworkQueueKind.h3WireSend;

class _Queues extends StatelessWidget {
  const _Queues({required this.controller});
  final AppController controller;

  @override
  Widget build(BuildContext context) => _QualitySelector(
    controller: controller,
    selector: (app) {
      final l4 = qualityLinkOf(app).l4;
      return (
        catalog: app.strings.catalogId,
        // L4 carries no H3 datagrams, so that queue would only read empty.
        queues: (app.quality.latest?.queues ?? const <NetworkQueueQuality>[])
            .where(
              (queue) => !l4 || queue.kind != NetworkQueueKind.h3DatagramSend,
            )
            .toList(growable: false),
      );
    },
    equals: (a, b) => a.catalog == b.catalog && listEquals(a.queues, b.queues),
    builder: (context, view) {
      final s = controller.strings;
      final main = view.queues.where((queue) => !_lowLevelQueue(queue));
      final low = view.queues.where(_lowLevelQueue).toList(growable: false);
      return QualitySection(
        title: s.get('nq_queues'),
        icon: LucideIcons.layers3,
        children: <Widget>[
          if (view.queues.isEmpty) Text(s.get('nq_queue_empty')),
          for (final queue in main) QueueReadout(queue: queue, strings: s),
          if (low.isNotEmpty)
            ExpansionTile(
              // Keep expansion state separate from PageFrame's scroll offset.
              key: const PageStorageKey<String>(
                'network-quality-low-level-queues',
              ),
              title: Text(s.get('nq_queue_details')),
              children: <Widget>[
                for (final queue in low)
                  Padding(
                    padding: const EdgeInsets.only(bottom: 16),
                    child: QueueReadout(queue: queue, strings: s),
                  ),
              ],
            ),
        ],
      );
    },
  );
}

/// L4 mode status and its transport counters.
class QualityL4Panel extends StatelessWidget {
  const QualityL4Panel({required this.controller, super.key});
  final AppController controller;

  @override
  Widget build(BuildContext context) => ListenableSelector(
    listenables: <Listenable>[controller],
    selector: () =>
        (catalog: controller.strings.catalogId, stream: controller.snapshot.l4),
    builder: (context, view) {
      final s = controller.strings;
      final stream = view.stream;
      return ContentSection(
        title: s.get('l4_mode'),
        children: <Widget>[
          Text(
            s.get(
              stream == null
                  ? 'l4_status_unknown'
                  : stream.sessions == 0
                  ? 'l4_quic_not_ready'
                  : stream.connectVerified
                  ? 'l4_verified'
                  : 'l4_unverified',
            ),
          ),
          ExpansionTile(
            key: const PageStorageKey<String>('l4-technical-details'),
            title: Text(s.get('technical_details')),
            expandedCrossAxisAlignment: CrossAxisAlignment.stretch,
            childrenPadding: const EdgeInsets.all(16),
            children: <Widget>[
              Text(s.get('l4_na')),
              if (stream != null) ...<Widget>[
                const SizedBox(height: 16),
                // Slash-separated values keep each counter family together.
                QualityReadouts(
                  values: <(String, String)>[
                    (
                      s.get('l4_sessions'),
                      '${stream.sessions} / ${stream.drainingSessions}',
                    ),
                    (
                      s.get('l4_flows'),
                      '${stream.activeFlows} / ${stream.pendingFlows}',
                    ),
                    (
                      s.get('l4_connect'),
                      '${stream.connectSuccesses} / ${stream.connectFailures} / ${stream.connectTimeouts}',
                    ),
                    (s.get('l4_buffers'), '${stream.bufferBytes}'),
                    (
                      s.get('l4_backpressure'),
                      '${stream.sendBackpressure} / ${stream.receiveBackpressure}',
                    ),
                    (
                      s.get('l4_tun_flows'),
                      '${stream.tunFlows} / ${stream.halfOpenFlows}',
                    ),
                    (s.get('l4_udp'), '${stream.udpRejected}'),
                    (
                      s.get('l4_unsupported_packets'),
                      '${stream.unsupportedPackets}',
                    ),
                    (
                      s.get('l4_budget_rejections'),
                      '${stream.budgetRejections}',
                    ),
                    (
                      s.get('l4_dns'),
                      '${stream.dnsSuccesses} / ${stream.dnsFailures} / ${stream.dnsTimeouts}',
                    ),
                    (
                      s.get('l4_migration'),
                      '${stream.migrationPreservedFlows} / ${stream.reconnectTerminatedFlows}',
                    ),
                  ],
                ),
              ],
            ],
          ),
        ],
      );
    },
  );
}
