import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:usque/services/gui_startup_observation.dart';

Map<int, List<Object>> fields(Uint8List bytes) {
  var offset = 0;
  int varint() {
    var value = 0, shift = 0;
    while (true) {
      final byte = bytes[offset++];
      value |= (byte & 127) << shift;
      if (byte < 128) return value;
      shift += 7;
    }
  }

  final result = <int, List<Object>>{};
  while (offset < bytes.length) {
    final tag = varint();
    Object value;
    if ((tag & 7) == 0) {
      value = varint();
    } else {
      expect(tag & 7, 2);
      final length = varint();
      value = Uint8List.sublistView(bytes, offset, offset + length);
      offset += length;
    }
    result.putIfAbsent(tag >> 3, () => []).add(value);
  }
  return result;
}

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  const channel = MethodChannel('io.github.georgexie2333.usque/engine');
  final messenger =
      TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger;
  tearDown(() => messenger.setMockMethodCallHandler(channel, null));
  test(
    'zero timestamp remains present and absent samples remain unavailable',
    () async {
      messenger.setMockMethodCallHandler(
        channel,
        (_) async => {
          'max_gap_ms': -1,
          'private': '/private 192.0.2.53',
          'stages': [
            {'stage': 999, 'status': 2, 'elapsed_ms': 10},
          ],
        },
      );
      final observation = GuiStartupObservation(elapsedMilliseconds: () => 0);
      addTearDown(observation.dispose);
      expect(await observation.encode(), isNull);
      observation.start();
      observation.mark(2);
      final value = fields((await observation.encode())!);
      expect(value[3], isNull);
      expect(value[4], isNull);
      final stages = value[5]!.cast<Uint8List>().map(fields).toList();
      expect(stages.length, 6);
      final firstFrame = stages.singleWhere((s) => s[1]!.single == 2);
      expect(firstFrame[4], [0]);
      expect(firstFrame[3], [2]);
      final preferences = stages.singleWhere((s) => s[1]!.single == 3);
      expect(preferences[3], [1]);
      expect(preferences[4], isNull);
    },
  );
  test('sampling stops at sixty seconds and pending stages time out', () async {
    messenger.setMockMethodCallHandler(channel, (_) async => null);
    var elapsed = 0;
    final observation = GuiStartupObservation(
      elapsedMilliseconds: () => elapsed,
    );
    addTearDown(observation.dispose);
    observation.start();
    elapsed = 60001;
    await Future<void>.delayed(const Duration(milliseconds: 150));
    observation.mark(3);
    final value = fields((await observation.encode())!);
    expect(value[2], [60000]);
    expect(value[3], [60000]);
    expect(value[6], [1]);
    for (final stage in value[5]!.cast<Uint8List>().map(fields)) {
      expect(stage[3], [4]);
      expect(stage[4], [60000]);
    }
  });
}
