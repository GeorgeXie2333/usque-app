import 'dart:typed_data';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:usque/core/app_strings.dart';
import 'package:usque/core/usque_theme.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/models/diagnostics_models.dart';
import 'package:usque/services/control_codec.dart';
import 'package:usque/widgets/diagnostic_finding_card.dart';

const _identity = '550e8400-e29b-41d4-a716-446655440000';

Uint8List _sessionFrame(Uint8List session) {
  final body =
      (ControlPayloadWriter()
            ..string(1, 'test')
            ..message(19, session))
          .takeBytes();
  final prefix = ByteData(4)..setUint32(0, body.length);
  return Uint8List.fromList([...prefix.buffer.asUint8List(), ...body]);
}

DiagnosticSession _decode(Uint8List payload) => const ControlCodec()
    .decodeResponse(_sessionFrame(payload), 'test')
    .diagnosticSession!;

Uint8List _session(Uint8List finding, {bool metadata = false}) {
  final writer = ControlPayloadWriter()
    ..string(1, 'session-one')
    ..unsigned(2, 2)
    ..unsigned(5, 1)
    ..message(8, finding);
  if (metadata) {
    writer
      ..string(10, 'quality.rtt')
      ..string(10, 'quality.packet_loss')
      ..string(10, 'private.example')
      ..unsigned(11, 7);
  }
  return writer.takeBytes();
}

void main() {
  test('legacy evidence and absent observation remain compatible', () {
    final legacy = DiagnosticFinding.fromMap({
      'check_id': 'quality.rtt',
      'sanitized_evidence': [
        'rtt_ms=0',
        'active_path',
        'private.example',
        'rtt_ms=-1',
      ],
    });
    expect(legacy.observation, isNull);
    expect(legacy.publicEvidence, ['rtt_ms=0', 'active_path']);
    expect(legacy.evidence, isEmpty);

    final finding =
        (ControlPayloadWriter()
              ..string(1, 'quality.rtt')
              ..unsigned(2, 3)
              ..unsigned(3, 3)
              ..string(8, 'rtt_ms=0'))
            .takeBytes();
    final decoded = _decode(_session(finding));
    expect(decoded.findings.single.observation, isNull);
    expect(decoded.findings.single.publicEvidence, ['rtt_ms=0']);
    expect(decoded.activeChecks, isEmpty);
    expect(decoded.revision, isNull);
  });

  test(
    'hostile optional metadata cannot become public evidence or identity',
    () {
      final finding = DiagnosticFinding.fromMap({
        'observation': {
          'source': 'private.example',
          'availability': 'made_up',
          'age_milliseconds': -1,
          'connection_instance_id': 'account-secret',
          'network_generation': 1.5,
        },
        'evidence': [
          {'key': 'rtt_ms', 'number': 0},
          {'key': 'fact', 'token': 'runtime_path'},
          {'key': 'rtt_ms', 'number': 3, 'token': 'runtime_path'},
          {'key': 'fact', 'token': 'secret-value'},
          {'key': 'private.example', 'number': 1},
          {'key': 'rtt_ms', 'number': -1},
          {'key': 'rtt_ms', 'number': 1.5},
        ],
      });
      expect(finding.publicEvidence, ['rtt_ms=0', 'runtime_path']);
      expect(finding.observation!.source, DiagnosticObservationSource.unknown);
      expect(
        finding.observation!.availability,
        DiagnosticObservationAvailability.unknown,
      );
      expect(finding.observation!.ageMilliseconds, isNull);
      expect(finding.observation!.connectionInstanceId, isNull);
      expect(finding.observation!.networkGeneration, isNull);
    },
  );

  test(
    'wire metadata retains observed zero, active checks and bounded evidence',
    () {
      final observationPayload =
          (ControlPayloadWriter()
                ..string(1, 'active_probe')
                ..string(2, 'observed')
                ..unsigned(3, 0)
                ..string(4, _identity)
                ..unsigned(5, 0)
                ..string(99, 'ignored'))
              .takeBytes();
      final observation = Uint8List.fromList([...observationPayload, 0x28, 0]);
      final numberPayload =
          (ControlPayloadWriter()
                ..string(1, 'rtt_ms')
                ..unsigned(2, 0))
              .takeBytes();
      // Optional uint64 zero must be encoded with presence. The request
      // writer intentionally omits ordinary default-valued scalars.
      final number = Uint8List.fromList([...numberPayload, 0x10, 0]);
      final hostile =
          (ControlPayloadWriter()
                ..string(1, 'fact')
                ..string(3, 'private.example'))
              .takeBytes();
      final finding = ControlPayloadWriter()
        ..string(1, 'quality.rtt')
        ..unsigned(2, 3)
        ..unsigned(3, 3)
        ..message(12, observation)
        ..message(13, hostile);
      for (var i = 0; i < 33; i++) {
        finding.message(13, number);
      }
      final decoded = _decode(_session(finding.takeBytes(), metadata: true));
      final result = decoded.findings.single;
      expect(
        result.observation!.source,
        DiagnosticObservationSource.activeProbe,
      );
      expect(
        result.observation!.availability,
        DiagnosticObservationAvailability.observed,
      );
      expect(result.observation!.ageMilliseconds, 0);
      expect(result.observation!.connectionInstanceId, _identity);
      expect(result.observation!.networkGeneration, 0);
      expect(result.evidence, hasLength(32));
      expect(result.publicEvidence.toSet(), {'rtt_ms=0'});
      expect(decoded.activeChecks, ['quality.rtt', 'quality.packet_loss']);
      expect(decoded.revision, 7);
    },
  );

  testWidgets('provenance is readable without exposing runtime identity', (
    tester,
  ) async {
    final strings = AppStrings(LocalePreference.english);
    await tester.pumpWidget(
      MaterialApp(
        theme: UsqueTheme.light(),
        home: Scaffold(
          body: DiagnosticFindingCard(
            finding: DiagnosticFinding(
              checkId: 'quality.rtt',
              category: DiagnosticCategory.transport,
              status: DiagnosticCheckStatus.passed,
              observation: DiagnosticObservation.fromMap({
                'source': 'runtime',
                'availability': 'inferred',
                'connection_instance_id': _identity,
                'network_generation': 23,
              }),
              evidence: const [DiagnosticEvidence(key: 'rtt_ms', number: 0)],
            ),
            strings: strings,
          ),
        ),
      ),
    );
    expect(find.text('Runtime state · Inferred'), findsOneWidget);
    expect(find.textContaining(_identity), findsNothing);
    expect(find.textContaining('23'), findsNothing);
    await tester.tap(find.text(strings.get('technical_details')));
    await tester.pumpAndSettle();
    expect(find.text('rtt_ms=0'), findsOneWidget);
    expect(tester.takeException(), isNull);
  });

  test(
    'full diagnostic snapshot event preserves revision without connection state',
    () {
      final session =
          (ControlPayloadWriter()
                ..string(1, 'session-one')
                ..unsigned(2, 2)
                ..unsigned(5, 1)
                ..string(10, 'quality.rtt')
                ..unsigned(11, 8))
              .takeBytes();
      final payload = (ControlPayloadWriter()..message(1, session)).takeBytes();
      final envelope = (ControlPayloadWriter()..message(25, payload))
          .takeBytes();
      final prefix = ByteData(4)..setUint32(0, envelope.length);
      final event = debugDecodeEventFrame(
        Uint8List.fromList([...prefix.buffer.asUint8List(), ...envelope]),
      );
      expect(event.diagnosticsChanged, isTrue);
      expect(event.diagnosticSession!.revision, 8);
      expect(event.diagnosticSession!.activeChecks, ['quality.rtt']);
      expect(event.snapshot, isNull);
    },
  );
}
