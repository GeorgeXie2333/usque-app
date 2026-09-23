import 'dart:typed_data';

import 'package:flutter_test/flutter_test.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/models/diagnostics_models.dart';
import 'package:usque/services/control_codec.dart';
import 'package:usque/services/engine_client.dart';

Uint8List response(int field, Uint8List payload) {
  final body =
      (ControlPayloadWriter()
            ..string(1, 'test')
            ..message(field, payload))
          .takeBytes();
  final prefix = ByteData(4)..setUint32(0, body.length);
  return Uint8List.fromList([...prefix.buffer.asUint8List(), ...body]);
}

void main() {
  test('absent performance stays unknown; JSON counters have fixed bounds', () {
    expect(NetworkQualitySnapshot.fromMap({}).transportPerformance, isNull);
    final performance = TransportPerformanceSnapshot.from({
      'h2': {'data_frames': 0, 'assembly_copy_bytes': 123, 'secret': 'private'},
      'h2_batch_sizes': List.filled(100, 7),
    })!;
    expect(performance.h2!.values['data_frames'], 0);
    expect(performance.h2!.values['assembly_copy_bytes'], 123);
    expect(performance.h2!.values.containsKey('secret'), isFalse);
    expect(performance.h3, isNull);
    expect(performance.h2BatchSizes, hasLength(7));
  });

  test(
    'append-only wire performance and queue waits survive unknown fields',
    () {
      final wait =
          (ControlPayloadWriter()
                ..unsigned(1, 3)
                ..unsigned(7, 2500)
                ..unsigned(9, 1)
                ..unsigned(99, 42))
              .takeBytes();
      final queue =
          (ControlPayloadWriter()
                ..unsigned(1, 3)
                ..message(17, wait))
              .takeBytes();
      final h3 =
          (ControlPayloadWriter()
                ..unsigned(1, 2)
                ..unsigned(2, 128)
                ..unsigned(4, 5))
              .takeBytes();
      final performance =
          (ControlPayloadWriter()
                ..message(2, h3)
                ..unsigned(3, 17)
                ..unsigned(6, 2))
              .takeBytes();
      final quality =
          (ControlPayloadWriter()
                ..message(5, queue)
                ..message(11, performance)
                ..unsigned(99, 9))
              .takeBytes();
      final decoded = debugDecodeNetworkQualityFrame(
        response(21, quality),
        'test',
      )!;
      expect(decoded.queues.single.backpressure!.values['waits'], 3);
      expect(decoded.queues.single.backpressure!.values['total_us'], 2500);
      expect(decoded.queues.single.backpressure!.buckets, [1]);
      expect(decoded.transportPerformance!.h2, isNull);
      expect(
        decoded.transportPerformance!.h3!.values['encode_pool_exhausted'],
        5,
      );
      expect(decoded.transportPerformance!.incomingCopyBytes, 17);
      final oversized = ControlPayloadWriter();
      for (var i = 0; i < 33; i++) {
        oversized.unsigned(9, 1);
      }
      final badQueue =
          (ControlPayloadWriter()..message(17, oversized.takeBytes()))
              .takeBytes();
      final bad = (ControlPayloadWriter()..message(5, badQueue)).takeBytes();
      expect(
        () => debugDecodeNetworkQualityFrame(response(21, bad), 'test'),
        throwsA(isA<EngineException>()),
      );
    },
  );

  test('backpressure event carries queue and duration without failure', () {
    for (final duration in [0, 4]) {
      final event =
          (ControlPayloadWriter()
                ..unsigned(1, 1)
                ..unsigned(4, 31)
                ..unsigned(8, duration)
                ..unsigned(10, 3))
              .takeBytes();
      final timeline = (ControlPayloadWriter()..message(1, event)).takeBytes();
      final decoded = debugDecodeConnectionTimelineFrame(
        response(20, timeline),
        'test',
      )!;
      expect(
        decoded.events.single.eventType,
        ConnectionTimelineEventType.queueBackpressured,
      );
      expect(decoded.events.single.queueKind, 'transport_outgoing');
      expect(decoded.events.single.durationMilliseconds, duration);
      expect(decoded.events.single.failure, isNull);
    }
  });
}
