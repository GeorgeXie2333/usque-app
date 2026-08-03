import 'dart:async';
import 'dart:typed_data';

import 'package:flutter_test/flutter_test.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/services/control_codec.dart';
import 'package:usque/services/desktop_engine_client.dart';
import 'package:usque/services/desktop_engine_transport.dart';
import 'package:usque/services/engine_client.dart';

void main() {
  group('ControlCodec protobuf golden bytes', () {
    const codec = ControlCodec();

    test('GetStatus request frame matches the fixed Rust v1 snapshot', () {
      expect(debugEncodeGetStatusFrame('r1'), <int>[
        0,
        0,
        0,
        6,
        0x0a,
        2,
        0x72,
        0x31,
        0x52,
        0,
      ]);
    });

    test('status response golden decodes disconnected phase', () {
      final snapshot = debugDecodeStatusFrame(
        Uint8List.fromList(<int>[
          0,
          0,
          0,
          8,
          0x0a,
          2,
          0x72,
          0x31,
          0x5a,
          2,
          0x08,
          1,
        ]),
        'r1',
      );
      expect(snapshot.phase, ConnectionPhase.disconnected);
    });

    test('buildRequestFrame is stable for empty GetStatus payload', () {
      final frame = codec.buildRequestFrame(
        requestId: 'r1',
        payloadField: 10,
        payload: Uint8List(0),
      );
      expect(frame, debugEncodeGetStatusFrame('r1'));
    });

    test(
      'encodeProfile then round-trips catalog identity through wire shape',
      () {
        final profile = UsqueProfile.defaultProfile();
        final encoded = codec.encodeProfile(profile);
        // Field 1 = id (length-delimited): tag 0x0a
        expect(encoded.first, 0x0a);
        expect(encoded, isNotEmpty);
        expect(encoded.length, lessThan(kMaximumFrameBytes));
      },
    );
  });

  group('ControlCodec truncated / oversized handling', () {
    const codec = ControlCodec();

    test('truncated response header throws ENGINE_IPC_TRUNCATED', () {
      expect(
        () => codec.decodeResponse(Uint8List.fromList(<int>[0, 0]), 'r1'),
        throwsA(
          isA<EngineException>().having(
            (e) => e.code,
            'code',
            'ENGINE_IPC_TRUNCATED',
          ),
        ),
      );
    });

    test('length mismatch throws ENGINE_IPC_INVALID_RESPONSE', () {
      // Claims 8 payload bytes but only provides 2 after the header.
      final frame = Uint8List.fromList(<int>[0, 0, 0, 8, 0x0a, 0]);
      expect(
        () => codec.decodeResponse(frame, 'r1'),
        throwsA(
          isA<EngineException>().having(
            (e) => e.code,
            'code',
            'ENGINE_IPC_INVALID_RESPONSE',
          ),
        ),
      );
    });

    test('oversized framed length throws ENGINE_IPC_INVALID_RESPONSE', () {
      final frame = Uint8List(8);
      ByteData.sublistView(frame).setUint32(0, kMaximumFrameBytes + 1);
      expect(
        () => codec.decodeResponse(frame, 'r1'),
        throwsA(
          isA<EngineException>().having(
            (e) => e.code,
            'code',
            'ENGINE_IPC_INVALID_RESPONSE',
          ),
        ),
      );
    });

    test('request frame larger than 4 MiB is rejected', () {
      final huge = Uint8List(kMaximumFrameBytes + 1);
      expect(
        () => codec.frame(huge),
        throwsA(
          isA<EngineException>().having(
            (e) => e.code,
            'code',
            'ENGINE_IPC_FRAME_TOO_LARGE',
          ),
        ),
      );
    });

    test('truncated event header throws ENGINE_EVENT_TRUNCATED', () {
      expect(
        () => codec.decodeEventSnapshot(Uint8List.fromList(<int>[0, 0, 0])),
        throwsA(
          isA<EngineException>().having(
            (e) => e.code,
            'code',
            'ENGINE_EVENT_TRUNCATED',
          ),
        ),
      );
    });

    test('readFrame rejects oversized declared length', () async {
      final header = Uint8List(4);
      ByteData.sublistView(header).setUint32(0, kMaximumFrameBytes + 1);
      expect(
        () => readFrame(Stream<List<int>>.value(header)),
        throwsA(
          isA<EngineException>().having(
            (e) => e.code,
            'code',
            'ENGINE_IPC_FRAME_TOO_LARGE',
          ),
        ),
      );
    });

    test('readFrame rejects truncated stream', () async {
      final header = Uint8List(4);
      ByteData.sublistView(header).setUint32(0, 10);
      expect(
        () => readFrame(Stream<List<int>>.value(header)),
        throwsA(
          isA<EngineException>().having(
            (e) => e.code,
            'code',
            'ENGINE_IPC_TRUNCATED',
          ),
        ),
      );
    });
  });

  group('DesktopEngineClient coordination', () {
    test(
      'start-once: concurrent ensureStarted runs the start path once',
      () async {
        var starts = 0;
        final entered = Completer<void>();
        final release = Completer<void>();
        final transport = DesktopEngineTransport.forTest(
          exchange: (_) async => _statusResponse('1'),
          ensureStarted: () async {
            starts++;
            if (!entered.isCompleted) {
              entered.complete();
            }
            await release.future;
          },
          requestIdFactory: () => '1',
        );

        final a = transport.ensureStarted();
        await entered.future;
        final b = transport.ensureStarted();
        final c = transport.ensureStarted();
        release.complete();
        await Future.wait(<Future<void>>[a, b, c]);
        expect(starts, 1);
        expect(transport.startCount, 1);

        // Subsequent ensureStarted is a no-op after a successful start.
        await transport.ensureStarted();
        expect(starts, 1);
        expect(transport.startCount, 1);
      },
    );

    test('client requests share a single transport start', () async {
      var starts = 0;
      var idSeq = 0;
      final client = DesktopEngineClient.forTest(
        transport: DesktopEngineTransport.forTest(
          exchange: (_) async => _statusResponse('$idSeq'),
          ensureStarted: () async {
            starts++;
          },
          requestIdFactory: () => '${++idSeq}',
        ),
      );
      await client.snapshot();
      await client.snapshot();
      expect(starts, 1);
      client.dispose();
    });

    test(
      'dispose marks transport closed and blocks subsequent requests',
      () async {
        final transport = DesktopEngineTransport.forTest(
          exchange: (_) async => _statusResponse('1'),
          requestIdFactory: () => '1',
        );
        final client = DesktopEngineClient.forTest(transport: transport);
        client.dispose();
        expect(transport.isDisposed, isTrue);
        await expectLater(
          client.snapshot(),
          throwsA(
            isA<EngineException>().having(
              (e) => e.code,
              'code',
              'ENGINE_CLOSED',
            ),
          ),
        );
      },
    );

    test('concurrent requests are serialized on the client queue', () async {
      final order = <String>[];
      final releaseFirst = Completer<void>();
      var requestIds = 0;
      final transport = DesktopEngineTransport.forTest(
        exchange: (Uint8List request) async {
          final id = ++requestIds;
          order.add('start-$id');
          if (id == 1) {
            await releaseFirst.future;
          }
          order.add('end-$id');
          return _statusResponse('$id');
        },
        requestIdFactory: () => '${requestIds + 1}',
      );
      final client = DesktopEngineClient.forTest(transport: transport);

      final first = client.snapshot();
      final second = client.snapshot();
      // Allow the first request to reach exchange before releasing.
      await Future<void>.delayed(Duration.zero);
      await Future<void>.delayed(Duration.zero);
      expect(order, <String>['start-1']);
      releaseFirst.complete();
      await Future.wait(<Future<EngineSnapshot>>[first, second]);
      expect(order, <String>['start-1', 'end-1', 'start-2', 'end-2']);
      client.dispose();
    });

    test(
      'structured engine errors map to EngineException without retry mask',
      () async {
        final transport = DesktopEngineTransport.forTest(
          exchange: (_) async => Uint8List.fromList(<int>[
            0,
            0,
            0,
            18,
            0x0a,
            2,
            0x72,
            0x31,
            0x12,
            12,
            0x0a,
            1,
            0x45,
            0x12,
            7,
            0x62,
            0x6c,
            0x6f,
            0x63,
            0x6b,
            0x65,
            0x64,
          ]),
          requestIdFactory: () => 'r1',
        );
        final client = DesktopEngineClient.forTest(transport: transport);
        await expectLater(
          client.snapshot(),
          throwsA(
            isA<EngineException>()
                .having((e) => e.code, 'code', 'E')
                .having((e) => e.message, 'message', 'blocked'),
          ),
        );
        client.dispose();
      },
    );

    test('transport exchange failures map to ENGINE_IPC_UNAVAILABLE', () async {
      final transport = DesktopEngineTransport.forTest(
        exchange: (_) async {
          throw StateError('pipe broken');
        },
        requestIdFactory: () => 'r1',
      );
      final client = DesktopEngineClient.forTest(transport: transport);
      await expectLater(
        client.snapshot(),
        throwsA(
          isA<EngineException>().having(
            (e) => e.code,
            'code',
            'ENGINE_IPC_UNAVAILABLE',
          ),
        ),
      );
      client.dispose();
    });

    test('request id mismatch is mapped by the codec', () async {
      final transport = DesktopEngineTransport.forTest(
        exchange: (_) async => _statusResponse('other'),
        requestIdFactory: () => 'expected',
      );
      final client = DesktopEngineClient.forTest(transport: transport);
      await expectLater(
        client.snapshot(),
        throwsA(
          isA<EngineException>().having(
            (e) => e.code,
            'code',
            'ENGINE_IPC_REQUEST_MISMATCH',
          ),
        ),
      );
      client.dispose();
    });

    test(
      'disconnect is not blocked by an in-flight serialized request',
      () async {
        final exchangeStarted = Completer<void>();
        final releaseSnapshot = Completer<void>();
        final ids = <String>[];
        var seq = 0;
        final transport = DesktopEngineTransport.forTest(
          exchange: (Uint8List request) async {
            final id = utf8RequestIdHint(request) ?? 'unknown';
            ids.add(id);
            if (id == 'snap') {
              if (!exchangeStarted.isCompleted) {
                exchangeStarted.complete();
              }
              await releaseSnapshot.future;
            }
            return _statusResponse(id);
          },
          requestIdFactory: () {
            seq++;
            return seq == 1 ? 'snap' : 'disc';
          },
        );
        final client = DesktopEngineClient.forTest(transport: transport);

        final snapFuture = client.snapshot();
        await exchangeStarted.future;
        final discFuture = client.disconnect();
        // Disconnect should reach exchange while snapshot is still held.
        await Future<void>.delayed(const Duration(milliseconds: 20));
        expect(ids, containsAll(<String>['snap', 'disc']));
        releaseSnapshot.complete();
        await Future.wait(<Future<EngineSnapshot>>[snapFuture, discFuture]);
        client.dispose();
      },
    );
  });
}

/// Minimal GetStatus-style success response for [requestId].
Uint8List _statusResponse(String requestId) {
  final idBytes = requestId.codeUnits;
  final payload = <int>[
    0x0a,
    idBytes.length,
    ...idBytes,
    0x5a,
    2,
    0x08,
    1, // snapshot phase = disconnected
  ];
  final frame = Uint8List(payload.length + 4);
  ByteData.sublistView(frame).setUint32(0, payload.length);
  frame.setRange(4, frame.length, payload);
  return frame;
}

String? utf8RequestIdHint(Uint8List frame) {
  // Framed envelope: skip 4-byte length, then field 1 string.
  if (frame.length < 6) {
    return null;
  }
  final payload = Uint8List.sublistView(frame, 4);
  if (payload.isEmpty || payload[0] != 0x0a) {
    return null;
  }
  final len = payload[1];
  if (payload.length < 2 + len) {
    return null;
  }
  return String.fromCharCodes(payload.sublist(2, 2 + len));
}
