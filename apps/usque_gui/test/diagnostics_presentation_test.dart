import 'dart:typed_data';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:usque/core/app_strings.dart';
import 'package:usque/core/diagnostics_strings.dart';
import 'package:usque/core/l10n/catalogs.dart';
import 'package:usque/core/l10n/diagnostics.dart';
import 'package:usque/core/usque_theme.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/models/diagnostics_models.dart';
import 'package:usque/services/control_codec.dart';
import 'package:usque/widgets/connection_timeline.dart';
import 'package:usque/widgets/diagnostic_finding_card.dart';

Uint8List _timelineFrame(Uint8List timeline) {
  final body =
      (ControlPayloadWriter()
            ..string(1, 'test')
            ..message(20, timeline))
          .takeBytes();
  final prefix = ByteData(4)..setUint32(0, body.length);
  return Uint8List.fromList([...prefix.buffer.asUint8List(), ...body]);
}

Widget _host(Widget child) => MaterialApp(
  theme: UsqueTheme.light(),
  home: Scaffold(body: SingleChildScrollView(child: child)),
);

void main() {
  final strings = AppStrings(LocalePreference.english);

  test(
    'new timeline events have distinct, translated labels in every catalog',
    () {
      expect(kDiagnosticsCatalogs.keys.toSet(), kCatalogs.keys.toSet());
      for (final id in kCatalogs.keys) {
        final localized = AppStrings(
          LocalePreference.system,
          systemLocale: Locale(
            id.split('_').first,
            id.contains('_') ? id.split('_').last : null,
          ),
        );
        final labels = <String>{};
        for (final type in <ConnectionTimelineEventType>[
          ConnectionTimelineEventType.migrationStarted,
          ConnectionTimelineEventType.migrationPathValidated,
          ConnectionTimelineEventType.migrationPromoted,
          ConnectionTimelineEventType.migrationFailed,
          ConnectionTimelineEventType.pmtuChanged,
          ConnectionTimelineEventType.pmtuRevalidationStarted,
          ConnectionTimelineEventType.pmtuRevalidationFailed,
          ConnectionTimelineEventType.directDnsDegraded,
          ConnectionTimelineEventType.directDnsRecovered,
          ConnectionTimelineEventType.unknown,
        ]) {
          final label = connectionEventLabel(localized, type);
          expect(label, isNot(startsWith('diag_')), reason: '$id.${type.name}');
          expect(labels.add(label), isTrue, reason: '$id.${type.name}');
          expect(
            label,
            isNot(
              connectionEventLabel(
                localized,
                ConnectionTimelineEventType.failed,
              ),
            ),
          );
        }
      }
      expect(AppStrings.debugCatalogsAreComplete, isTrue);
      expect(AppStrings.debugPlaceholdersArePreserved, isTrue);
      expect(AppStrings.debugUntranslatedFeatureKeys(), isEmpty);
    },
  );

  test('missing JSON counters stay unknown while observed zero stays zero', () {
    final missing = connectionTimelineFromMap({
      'metrics': {'reconnect_count': 0},
    }).metrics;
    expect(missing.reconnectCount, 0);
    expect(missing.fallbackCount, isNull);
    expect(missing.networkChangeCount, isNull);
    expect(missing.sendQueueHighWatermark, isNull);
    expect(missing.sendQueueDropCount, isNull);
    final absent = debugDecodeConnectionTimelineFrame(
      _timelineFrame(Uint8List(0)),
      'test',
    )!;
    expect(absent.metrics.reconnectCount, isNull);
    // Legacy protobuf scalar counters keep their established zero semantics
    // when the sender supplies the metrics message.
    final metrics = (ControlPayloadWriter()..message(2, Uint8List(0)))
        .takeBytes();
    final native = debugDecodeConnectionTimelineFrame(
      _timelineFrame(metrics),
      'test',
    )!;
    expect(native.metrics.reconnectCount, 0);
  });

  testWidgets(
    'timeline shows source omissions, view omissions and unknown metrics',
    (tester) async {
      final events = List.generate(
        102,
        (index) => ConnectionTimelineEvent(
          sequence: index + 1,
          eventType: ConnectionTimelineEventType.unknown,
          elapsedMilliseconds: index,
        ),
      );
      await tester.pumpWidget(
        _host(
          ConnectionTimelineView(
            timeline: ConnectionTimeline(events: events, droppedEventCount: 7),
            strings: strings,
          ),
        ),
      );
      expect(find.text('Events omitted by source: 7'), findsOneWidget);
      expect(
        find.text('Earlier events hidden in this view: 2'),
        findsOneWidget,
      );
      expect(find.text('Reconnects · Unknown'), findsOneWidget);
      expect(
        find.text('${strings.get('diag_metric_fallbacks')} · Unknown'),
        findsOneWidget,
      );
      expect(find.text('Connection failed'), findsNothing);
      expect(tester.takeException(), isNull);
    },
  );

  testWidgets(
    'an empty retained timeline still reports omitted source events',
    (tester) async {
      await tester.pumpWidget(
        _host(
          ConnectionTimelineView(
            timeline: const ConnectionTimeline(droppedEventCount: 3),
            strings: strings,
          ),
        ),
      );
      expect(find.text('Events omitted by source: 3'), findsOneWidget);
      expect(find.text(strings.get('diag_timeline_empty')), findsOneWidget);
    },
  );

  testWidgets(
    'quality warning displays actionable remediation without a failure',
    (tester) async {
      await tester.pumpWidget(
        _host(
          DiagnosticFindingCard(
            finding: const DiagnosticFinding(
              checkId: 'quality.rtt',
              category: DiagnosticCategory.transport,
              status: DiagnosticCheckStatus.warning,
              summaryKey: 'nq_finding_rtt_high',
              remediationKey: 'nq_network',
            ),
            strings: strings,
          ),
        ),
      );
      expect(find.text(strings.get('nq_finding_rtt_high')), findsOneWidget);
      expect(
        find.text(diagnosticRemediation(strings, 'nq_network')),
        findsOneWidget,
      );
    },
  );

  testWidgets('finding remediation overrides generic failure remediation', (
    tester,
  ) async {
    await tester.pumpWidget(
      _host(
        DiagnosticFindingCard(
          finding: const DiagnosticFinding(
            checkId: 'protection.route_ownership',
            category: DiagnosticCategory.protection,
            status: DiagnosticCheckStatus.warning,
            remediationKey: 'inspect_platform_state',
            failure: TransportFailureInfo(
              code: 'PLATFORM_RECOVERY_PENDING',
              stage: 'platform_recovery',
              remediationKey: 'retry',
            ),
          ),
          strings: strings,
        ),
      ),
    );
    expect(
      find.text(diagnosticRemediation(strings, 'inspect_platform_state')),
      findsOneWidget,
    );
    expect(find.text(diagnosticRemediation(strings, 'retry')), findsNothing);
  });
}
