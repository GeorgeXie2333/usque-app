import 'dart:async';
import 'dart:typed_data';

import 'package:flutter_test/flutter_test.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/services/control_codec.dart';
import 'package:usque/services/desktop_engine_client.dart';
import 'package:usque/services/desktop_engine_transport.dart';
import 'package:usque/services/engine_client.dart';

/// Compact profile used only for fixed encode goldens (short wire fields).
const UsqueProfile _goldenProfile = UsqueProfile(
  id: 'p',
  name: 'X',
  endpointIpv4: 'a',
  endpointIpv6: 'b',
  endpointPort: 1,
  sni: 'c',
  mtu: 1280,
  dnsIpv4: 'd',
  dnsIpv6: 'e',
  killSwitch: true,
  allowLan: false,
  autoConnect: false,
  proxy: ProxySettings(
    socksIpv4: 's',
    socksIpv6: 't',
    socksPort: 1,
    httpIpv4: 'h',
    httpIpv6: 'i',
    httpPort: 1,
    dnsIpv4: 'j.j',
    dnsIpv6: 'k:k',
  ),
);

/// Fixed protobuf bytes for [_goldenProfile] encode (pins v1 field layout).
const List<int> _goldenProfileBytes = <int>[
  0x0a, 0x01, 0x70, // id "p"
  0x12, 0x01, 0x58, // name "X"
  0x18, 0x01, // mode VPN
  0x20, 0x01, // transport AUTO
  0x2a, 0x0b, // endpoint { (11 bytes)
  0x0a, 0x01, 0x61, //   ipv4 "a"
  0x12, 0x01, 0x62, //   ipv6 "b"
  0x18, 0x01, //   port 1
  0x22, 0x01, 0x63, //   sni "c"
  // }
  0x30, 0x01, // ip policy AUTO
  0x38, 0x80, 0x0a, // mtu 1280
  0x42, 0x01, 0x64, // dns "d"
  0x42, 0x01, 0x65, // dns "e"
  0x58, 0x01, // kill_switch true
  0x6a, 0x26, // proxy {
  0x0a, 0x03, 0x73, 0x3a, 0x31, //   socks "s:1"
  0x0a, 0x05, 0x5b, 0x74, 0x5d, 0x3a, 0x31, //   socks "[t]:1"
  0x12, 0x03, 0x68, 0x3a, 0x31, //   http "h:1"
  0x12, 0x05, 0x5b, 0x69, 0x5d, 0x3a, 0x31, //   http "[i]:1"
  0x20, 0x3c, //   udp idle 60
  0x28, 0x01, //   dns mode REMOTE
  0x32, 0x03, 0x6a, 0x2e, 0x6a, //   dns "j.j"
  0x32, 0x03, 0x6b, 0x3a, 0x6b, //   dns "k:k"
  // }
  0x70, 0x01, // dns mode TUNNEL
  0x7a, 0x06, // frontends {
  0x08, 0x01, //   tunnel true
  0x10, 0x01, //   socks5 true
  0x18, 0x01, //   http true
  // }
];

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

    test('minimal profile encode matches fixed golden bytes', () {
      expect(codec.encodeProfile(_goldenProfile), _goldenProfileBytes);
    });

    test('encoded profile decodes through a catalog response frame', () {
      final profileBytes = codec.encodeProfile(_goldenProfile);
      final catalogBody = ControlPayloadWriter()
        ..message(1, Uint8List.fromList(profileBytes))
        ..string(2, 'p');
      final responseBody = ControlPayloadWriter()
        ..string(1, 'r2')
        ..message(12, catalogBody.takeBytes());
      final framed = codec.frame(responseBody.takeBytes());
      final catalog = debugDecodeProfileCatalogFrame(framed, 'r2');
      expect(catalog.activeProfileId, 'p');
      expect(catalog.profiles, hasLength(1));
      expect(catalog.profiles.single.id, 'p');
      expect(catalog.profiles.single.name, 'X');
      expect(catalog.profiles.single.endpointIpv4, 'a');
      expect(catalog.profiles.single.endpointPort, 1);
      expect(catalog.profiles.single.sni, 'c');
      expect(catalog.profiles.single.mtu, 1280);
      expect(catalog.profiles.single.killSwitch, isTrue);
      expect(catalog.profiles.single.proxy.socksPort, 1);
      expect(catalog.profiles.single.proxy.httpPort, 1);
      expect(catalog.profiles.single.proxy.dnsIpv4, 'j.j');
      expect(catalog.profiles.single.proxy.dnsIpv6, 'k:k');
    });
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

    test(
      'dispose during ensureStarted closes waiters with ENGINE_CLOSED',
      () async {
        final entered = Completer<void>();
        final release = Completer<void>();
        final transport = DesktopEngineTransport.forTest(
          exchange: (_) async => _statusResponse('1'),
          ensureStarted: () async {
            entered.complete();
            await release.future;
          },
          requestIdFactory: () => '1',
        );
        final client = DesktopEngineClient.forTest(transport: transport);
        final pending = client.snapshot();
        await entered.future;
        client.dispose();
        release.complete();
        await expectLater(
          pending,
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
      final firstEntered = Completer<void>();
      final releaseFirst = Completer<void>();
      var requestIds = 0;
      final transport = DesktopEngineTransport.forTest(
        exchange: (Uint8List request) async {
          final id = ++requestIds;
          order.add('start-$id');
          if (id == 1) {
            firstEntered.complete();
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
      await firstEntered.future;
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
      'hanging exchange maps to ENGINE_REQUEST_TIMEOUT without retry',
      () async {
        var exchanges = 0;
        final transport = DesktopEngineTransport.forTest(
          exchange: (_) async {
            exchanges++;
            // Never complete — client timeout must abort without a second attempt.
            return Completer<Uint8List>().future;
          },
          requestIdFactory: () => 'r1',
        );
        final client = DesktopEngineClient.forTest(
          transport: transport,
          requestTimeout: (_) => const Duration(milliseconds: 30),
        );
        await expectLater(
          client.snapshot(),
          throwsA(
            isA<EngineException>().having(
              (e) => e.code,
              'code',
              'ENGINE_REQUEST_TIMEOUT',
            ),
          ),
        );
        expect(exchanges, 1);
        client.dispose();
      },
    );

    test('production timeout table matches the pre-split contract', () {
      expect(requestTimeoutForPayload(12), const Duration(seconds: 55));
      expect(requestTimeoutForPayload(23), const Duration(seconds: 60));
      expect(requestTimeoutForPayload(26), const Duration(seconds: 60));
      expect(requestTimeoutForPayload(20), const Duration(seconds: 20));
      expect(requestTimeoutForPayload(21), const Duration(seconds: 15));
      expect(requestTimeoutForPayload(22), const Duration(seconds: 30));
      expect(requestTimeoutForPayload(10), const Duration(seconds: 5));
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

    test('retry encodes control request payload field 14', () async {
      Uint8List? seen;
      final client = DesktopEngineClient.forTest(
        transport: DesktopEngineTransport.forTest(
          exchange: (Uint8List request) async {
            seen = request;
            return _statusResponse('1');
          },
          requestIdFactory: () => '1',
        ),
      );
      await client.retry();
      expect(seen, isNotNull);
      // ControlRequest.retry is protobuf field 14, wire type 2 (tag 0x72).
      expect(seen!.sublist(4), contains(0x72));
      expect(
        const ControlCodec().buildRequestFrame(
          requestId: '1',
          payloadField: 14,
          payload: Uint8List(0),
        ),
        seen,
      );
      client.dispose();
    });
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
