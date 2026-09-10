import 'dart:typed_data';
import 'package:flutter_test/flutter_test.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/services/control_codec.dart';
import 'package:usque/services/engine_client.dart';
import 'congestion_control_test.dart' show codec, response;

void main() {
  EngineSnapshot decode(Uint8List l4) => codec
      .decodeResponse(
        response(11, (ControlPayloadWriter()..message(20, l4)).takeBytes()),
        'cc',
      )
      .snapshot!;
  test(
    'H3 receive wire field 10 preserves policy, actual size and unknowns',
    () {
      NetworkQualitySnapshot quality(Uint8List value) =>
          codec.decodeResponse(response(21, value), 'cc').networkQuality!;
      expect(quality(Uint8List(0)).udpSocketReceive, isNull);
      expect(NetworkQualitySnapshot.fromMap({}).udpSocketReceive, isNull);
      final observation =
          (ControlPayloadWriter()
                ..unsigned(14, 2097152)
                ..string(2, 'already_sufficient')
                ..string(10, 'portable'))
              .takeBytes();
      final bytes =
          (ControlPayloadWriter()..message(
                10,
                (ControlPayloadWriter()
                      ..unsigned(1, 4194304)
                      ..message(3, observation))
                    .takeBytes(),
              ))
              .takeBytes();
      final value = quality(bytes).udpSocketReceive!;
      expect(value.receiveBufferBytes, 4194304);
      expect(value.sendBufferBytes, isNull);
      expect(value.observation!.counters['buffer_target_bytes'], 2097152);
      expect(value.observation!.counters['requested_buffer_bytes'], isNull);
      expect(value.observation!.counters['socket_drops_reported'], isNull);
      expect(
        value.observation!.states['buffer_request_status'],
        'already_sufficient',
      );
      final mapped = NetworkQualitySnapshot.fromMap({
        'udp_socket_receive': {
          'receive_buffer_bytes': 4194304,
          'send_buffer_bytes': null,
          'private_key': 'private-value',
          'observation': {
            'buffer_target_bytes': 2097152,
            'buffer_request_status': 'already_sufficient',
            'receive_backend': 'portable',
            // Native JSON includes scalar zeros that protobuf omits on wire.
            'overflow_reports': 0,
            'ancillary_errors': 0,
            'recv_syscalls': 0,
            'received_datagrams': 0,
            'empty_recv_syscalls': 0,
            'history_dropped': 0,
          },
        },
      }).udpSocketReceive;
      expect(value, mapped);
      expect(value.hashCode, mapped.hashCode);
      final malformed = UdpSocketReceiveSnapshot.from({
        'receive_buffer_bytes': -1,
        'send_buffer_bytes': 'private-value',
        'observation': {'buffer_request_status': 'private-value'},
      })!;
      expect(malformed.receiveBufferBytes, isNull);
      expect(malformed.sendBufferBytes, isNull);
      expect(malformed.observation!.states, isEmpty);
    },
  );

  test('performance is additive and absent observations stay unknown', () {
    expect(decode(Uint8List(0)).l4!.performance, isNull);
    final empty = decode(
      (ControlPayloadWriter()..message(23, Uint8List(0))).takeBytes(),
    ).l4!.performance!;
    expect(empty.counters['h3_read_bytes'], 0);
    expect(empty.counters['udp_receive_buffer_bytes'], isNull);
    expect(empty.counters['tun_write_calls'], isNull);
    expect(empty.commandWait, isNull);
    expect(L4Snapshot.fromMap({}).performance, isNull);
  });
  test(
    'field 23 round trips counters, optional metadata and bounded histogram',
    () {
      final wait = ControlPayloadWriter()
        ..unsigned(1, 1)
        ..unsigned(2, 8)
        ..unsigned(3, 8);
      wait.message(
        4,
        Uint8List.fromList(List<int>.generate(32, (i) => i == 4 ? 1 : 0)),
      );
      final perf =
          (ControlPayloadWriter()
                ..unsigned(2, 32768)
                ..unsigned(12, 32768)
                ..unsigned(25, 2097152)
                ..string(27, 'getsockopt_raw')
                ..unsigned(28, 1280)
                ..unsigned(39, 3)
                ..string(29, 'applied_profile')
                ..message(33, wait.takeBytes()))
              .takeBytes();
      final value = decode(
        (ControlPayloadWriter()..message(23, perf)).takeBytes(),
      ).l4!.performance!;
      expect(value.counters['h3_read_bytes'], 32768);
      expect(value.counters['actor_no_progress_wakeups'], 3);
      expect(value.counters['tcp_accepted_bytes'], 32768);
      expect(value.counters['udp_receive_buffer_bytes'], 2097152);
      expect(value.udpBufferSource, 'getsockopt_raw');
      expect(value.commandWait!.hasSamples, isTrue);
      expect(value.commandWait!.buckets[4], 1);
      final malformed =
          (ControlPayloadWriter()..message(
                33,
                (ControlPayloadWriter()..message(4, Uint8List(33))).takeBytes(),
              ))
              .takeBytes();
      expect(
        () => decode(
          (ControlPayloadWriter()..message(23, malformed)).takeBytes(),
        ),
        throwsA(isA<EngineException>()),
      );
    },
  );
  test('native map drops unknown and malformed data with stable equality', () {
    final map = <Object?, Object?>{
      'h3_read_bytes': 32768,
      'udp_receive_buffer_bytes': null,
      'tun_write_calls': -1,
      'target': 'private',
      'udp_buffer_source': 'private',
      'command_wait': {
        'samples': 0,
        'sum_us': 0,
        'max_us': 0,
        'buckets': List<int>.filled(32, 0),
      },
    };
    final value = L4PerformanceSnapshot.fromMap(map);
    expect(value, L4PerformanceSnapshot.fromMap(map));
    expect(value.hashCode, L4PerformanceSnapshot.fromMap(map).hashCode);
    expect(value.counters.keys, ['h3_read_bytes']);
    expect(value.udpBufferSource, isNull);
    expect(value.commandWait!.hasSamples, isFalse);
    expect(() => value.counters['h3_read_bytes'] = 0, throwsUnsupportedError);
    expect(L4Snapshot(performance: value), isNot(const L4Snapshot()));
  });
  test(
    'receive observations retain optional loss, source status and bounded phase history',
    () {
      final interval =
          (ControlPayloadWriter()
                ..unsigned(1, 1000)
                ..unsigned(2, 1000)
                ..unsigned(4, 4000)
                ..unsigned(5, 4000)
                ..unsigned(6, 60)
                ..unsigned(7, 4))
              .takeBytes();
      final rx =
          (ControlPayloadWriter()
                ..unsigned(1, 2097152)
                ..string(2, 'accepted')
                ..string(3, 'enabled')
                ..string(10, 'recvmmsg')
                ..string(11, 'sendmmsg')
                ..message(12, interval))
              .takeBytes();
      L4ReceiveSnapshot receive(Uint8List value) => decode(
        (ControlPayloadWriter()..message(
              23,
              (ControlPayloadWriter()..message(40, value)).takeBytes(),
            ))
            .takeBytes(),
      ).l4!.performance!.receive!;
      final value = receive(rx);
      expect(value.counters['requested_buffer_bytes'], 2097152);
      expect(value.counters['socket_drops_reported'], isNull);
      expect(value.states['receive_backend'], 'recvmmsg');
      expect(value.history.single.counters['h3_read_bytes'], 4000);
      expect(value.history.single.counters['socket_drops'], isNull);
      expect(value, receive(rx));
      final excessive = ControlPayloadWriter();
      for (var i = 0; i < 121; i++) {
        excessive.message(12, interval);
      }
      expect(
        () => receive(excessive.takeBytes()),
        throwsA(isA<EngineException>()),
      );
      expect(
        L4ReceiveSnapshot.from({
          'history': List.filled(121, <Object?, Object?>{}),
        }),
        isNull,
      );
    },
  );
}
