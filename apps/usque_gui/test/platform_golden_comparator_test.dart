import 'dart:ffi';
import 'dart:typed_data';

import 'package:flutter_test/flutter_test.dart';

import 'support/platform_golden_comparator.dart';

class _RecordingComparator extends GoldenFileComparator {
  final compared = <Uri>[];
  final updated = <Uri>[];
  bool matches = true;

  @override
  Future<bool> compare(Uint8List imageBytes, Uri golden) async {
    compared.add(golden);
    return matches;
  }

  @override
  Future<void> update(Uri golden, Uint8List imageBytes) async {
    updated.add(golden);
  }
}

void main() {
  final bytes = Uint8List(0);
  final windowsGolden = Uri.parse('goldens/home_windows_default_idle.png');
  final linuxGolden = Uri.parse('goldens/linux/home_idle.png');

  for (final host in [
    (os: 'linux', abi: Abi.linuxX64, own: linuxGolden, other: windowsGolden),
    (
      os: 'windows',
      abi: Abi.windowsX64,
      own: windowsGolden,
      other: linuxGolden,
    ),
  ]) {
    test('${host.os} compares and updates only its own baselines', () async {
      final delegate = _RecordingComparator();
      final comparator = PlatformGoldenComparator(
        delegate,
        operatingSystem: host.os,
        abi: host.abi,
      );
      expect(await comparator.compare(bytes, host.own), isTrue);
      await comparator.update(host.own, bytes);
      expect(delegate.compared, [host.own]);
      expect(delegate.updated, [host.own]);

      await expectLater(
        comparator.compare(bytes, host.other),
        throwsStateError,
      );
      await expectLater(comparator.update(host.other, bytes), throwsStateError);
      expect(delegate.compared, [host.own]);
      expect(delegate.updated, [host.own]);
    });
  }

  test('a pixel mismatch is never accepted by the host guard', () async {
    final delegate = _RecordingComparator()..matches = false;
    final comparator = PlatformGoldenComparator(
      delegate,
      operatingSystem: 'linux',
      abi: Abi.linuxX64,
    );
    expect(await comparator.compare(bytes, linuxGolden), isFalse);
    expect(delegate.updated, isEmpty);
    expect(
      comparator.getTestUri(linuxGolden, 2),
      Uri.parse('goldens/linux/home_idle.2.png'),
    );
  });

  test(
    'unsupported hosts and architectures cannot read or write goldens',
    () async {
      for (final host in [
        (os: 'linux', abi: Abi.linuxArm64),
        (os: 'windows', abi: Abi.windowsArm64),
        (os: 'macos', abi: Abi.macosX64),
      ]) {
        final delegate = _RecordingComparator();
        final comparator = PlatformGoldenComparator(
          delegate,
          operatingSystem: host.os,
          abi: host.abi,
        );
        for (final golden in [linuxGolden, windowsGolden]) {
          await expectLater(
            comparator.compare(bytes, golden),
            throwsStateError,
          );
          await expectLater(comparator.update(golden, bytes), throwsStateError);
        }
        expect(delegate.compared, isEmpty);
        expect(delegate.updated, isEmpty);
      }
    },
  );

  test('golden paths cannot escape their baseline directory', () async {
    final delegate = _RecordingComparator();
    final comparator = PlatformGoldenComparator(
      delegate,
      operatingSystem: 'linux',
      abi: Abi.linuxX64,
    );
    for (final path in [
      '../goldens/linux/home.png',
      'goldens/linux/../../../home.png',
      '/goldens/linux/home.png',
      'file:///goldens/linux/home.png',
      '//server/goldens/linux/home.png',
      'goldens/linux/home.png?other=goldens/windows.png',
      'goldens/linux/home.png#fragment',
      r'goldens/linux/..\home.png',
      'goldens/linux/home.txt',
    ]) {
      final golden = Uri.parse(path);
      await expectLater(comparator.compare(bytes, golden), throwsStateError);
      await expectLater(comparator.update(golden, bytes), throwsStateError);
    }
    expect(delegate.compared, isEmpty);
    expect(delegate.updated, isEmpty);
  });
}
