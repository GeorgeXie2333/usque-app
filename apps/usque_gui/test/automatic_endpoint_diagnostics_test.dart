import 'dart:typed_data';

import 'package:flutter_test/flutter_test.dart';
import 'package:usque/core/app_strings.dart';
import 'package:usque/core/diagnostics_strings.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/models/diagnostics_models.dart';
import 'package:usque/services/control_codec.dart';

void main() {
  test(
    'appended timeline fields preserve zero round, causes and legacy defaults',
    () {
      final failure =
          (ControlPayloadWriter()
                ..string(1, 'ENDPOINT_PIN_RETRY_FAILED')
                ..string(2, 'socket_connect')
                ..string(10, 'H2_TCP_CONNECT_FAILED')
                ..string(11, 'socket_connect'))
              .takeBytes();
      final event =
          (ControlPayloadWriter()
                ..unsigned(1, 7)
                ..unsigned(4, 37)
                ..message(9, failure)
                ..unsigned(11, 3))
              .takeBytes();
      // The request writer omits scalar zero; encode optional-field presence explicitly.
      final timeline =
          (ControlPayloadWriter()
                ..message(1, Uint8List.fromList([...event, 0x60, 0])))
              .takeBytes();
      final body =
          (ControlPayloadWriter()
                ..string(1, 'test')
                ..message(20, timeline))
              .takeBytes();
      final prefix = ByteData(4)..setUint32(0, body.length);
      final decoded = debugDecodeConnectionTimelineFrame(
        Uint8List.fromList([...prefix.buffer.asUint8List(), ...body]),
        'test',
      )!.events.single;
      expect(decoded.eventType, ConnectionTimelineEventType.pinRetryFailed);
      expect(decoded.candidateId, 3);
      expect(decoded.enrollmentRound, 0);
      expect(decoded.failure!.causeCode, 'H2_TCP_CONNECT_FAILED');
      expect(decoded.failure!.causeStage, 'socket_connect');
      expect(
        TransportFailureInfo.fromMap({
          'code': 'ENDPOINT_PIN_MISMATCH',
        }).causeCode,
        isNull,
      );
      final legacy = connectionTimelineFromMap({
        'events': [
          {
            'sequence': 1,
            'event_type': 'candidate_rejected',
            'candidate_id': -1,
            'enrollment_round': 99,
          },
        ],
      }).events.single;
      expect(legacy.candidateId, isNull);
      expect(legacy.enrollmentRound, isNull);
    },
  );

  test('new failure and event labels exist in all supported languages', () {
    for (final locale in LocalePreference.values) {
      final strings = AppStrings(locale);
      for (final code in [
        'ENDPOINT_PIN_REFRESH_FAILED',
        'ENDPOINT_PIN_RETRY_FAILED',
      ]) {
        expect(
          diagnosticFailureTitle(strings, code),
          isNot(strings.get('operation_failed')),
        );
      }
      for (final event in [
        ConnectionTimelineEventType.candidateRejected,
        ConnectionTimelineEventType.pinRefreshStarted,
        ConnectionTimelineEventType.pinRefreshSucceeded,
        ConnectionTimelineEventType.pinRefreshFailed,
        ConnectionTimelineEventType.pinRetryStarted,
        ConnectionTimelineEventType.pinRetryFailed,
      ]) {
        expect(
          connectionEventLabel(strings, event),
          isNot(startsWith('diag_event_')),
        );
      }
      expect(
        diagnosticFailureTitle(strings, 'FUTURE_UNRECOGNIZED_ERROR'),
        strings.get('operation_failed'),
      );
    }
  });
}
