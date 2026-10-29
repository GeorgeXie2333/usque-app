import 'dart:async';
import 'dart:typed_data';

import 'package:flutter_test/flutter_test.dart';
import 'package:usque/services/control_codec.dart';
import 'package:usque/services/desktop_engine_transport.dart';
import 'package:usque/services/engine_client.dart';

Uint8List frame(List<int> body) => Uint8List(body.length + 4)
  ..buffer.asByteData().setUint32(0, body.length)
  ..setRange(4, body.length + 4, body);

void main() {
  test(
    'background event decoder handles fragmented and coalesced frames',
    () async {
      final first = frame([1, 2]), second = frame([3, 4, 5]);
      final wire = [...first, ...second];
      final result = await readEventFrames(
        Stream.fromIterable([
          wire.sublist(0, 1),
          wire.sublist(1, 5),
          wire.sublist(5),
        ]),
      ).toList();
      expect(result, [first, second]);
      expect(await readEventFrames(const Stream.empty()).toList(), isEmpty);
    },
  );

  test(
    'background decoder rejects invalid lengths before reading bodies',
    () async {
      for (final length in [0, kMaximumFrameBytes + 1]) {
        final header = Uint8List(4)..buffer.asByteData().setUint32(0, length);
        await expectLater(
          readEventFrames(Stream.value(header)).drain<void>(),
          throwsA(
            isA<EngineException>().having(
              (e) => e.code,
              'code',
              'ENGINE_EVENT_FRAME_INVALID',
            ),
          ),
        );
      }
    },
  );

  test(
    'EOF in a header or body is reported rather than silently dropped',
    () async {
      for (final partial in [
        [0, 0],
        [0, 0, 0, 3, 1],
      ]) {
        await expectLater(
          readEventFrames(Stream.value(partial)).drain<void>(),
          throwsA(
            isA<EngineException>().having(
              (e) => e.code,
              'code',
              'ENGINE_EVENT_TRUNCATED',
            ),
          ),
        );
      }
    },
  );

  test('cancelling an event subscription releases its input', () async {
    final cancelled = Completer<void>();
    final source = StreamController<List<int>>(onCancel: cancelled.complete);
    final result = readEventFrames(source.stream).first;
    source.add(frame([1]));
    expect(await result, frame([1]));
    await cancelled.future.timeout(const Duration(seconds: 2));
    await source.close();
  });
}
