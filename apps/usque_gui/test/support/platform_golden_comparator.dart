import 'dart:ffi';
import 'dart:typed_data';

import 'package:flutter_test/flutter_test.dart';

/// Keeps each host's exact-pixel baselines in its own directory.
///
/// The delegate remains Flutter's LocalFileComparator, including its normal
/// failure images. Neither comparisons nor updates may cross host boundaries.
class PlatformGoldenComparator extends GoldenFileComparator {
  PlatformGoldenComparator(
    this.delegate, {
    required this.operatingSystem,
    required this.abi,
  });

  final GoldenFileComparator delegate;
  final String operatingSystem;
  final Abi abi;

  void _checkHost(Uri golden) {
    if (golden.hasScheme ||
        golden.hasAuthority ||
        golden.hasQuery ||
        golden.hasFragment ||
        !RegExp(
          r'^goldens/(?:(?:linux|macos/arm64|macos/x86_64)/)?[A-Za-z0-9_.-]+\.png$',
        ).hasMatch(golden.path)) {
      throw StateError('Unsupported golden path: $golden');
    }
    final linux = golden.path.startsWith('goldens/linux/');
    final macArm = golden.path.startsWith('goldens/macos/arm64/');
    final macIntel = golden.path.startsWith('goldens/macos/x86_64/');
    final supported = macArm
        ? operatingSystem == 'macos' && abi == Abi.macosArm64
        : macIntel
        ? operatingSystem == 'macos' && abi == Abi.macosX64
        : linux
        ? operatingSystem == 'linux' && abi == Abi.linuxX64
        : operatingSystem == 'windows' && abi == Abi.windowsX64;
    if (!supported) {
      throw StateError(
        '$golden requires ${macArm
            ? 'macOS arm64'
            : macIntel
            ? 'macOS x86_64'
            : linux
            ? 'Linux amd64'
            : 'Windows x64'} with '
        'the pinned Flutter SDK. This host is $operatingSystem ($abi). '
        'On Linux amd64, select --tags linux-golden; ordinary widget tests '
        'use --exclude-tags golden. Baselines were not changed.',
      );
    }
  }

  @override
  Future<bool> compare(Uint8List imageBytes, Uri golden) async {
    _checkHost(golden);
    return delegate.compare(imageBytes, golden);
  }

  @override
  Future<void> update(Uri golden, Uint8List imageBytes) async {
    _checkHost(golden);
    await delegate.update(golden, imageBytes);
  }

  @override
  Uri getTestUri(Uri key, int? version) => delegate.getTestUri(key, version);
}
