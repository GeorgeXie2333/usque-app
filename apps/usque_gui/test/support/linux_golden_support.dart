import 'dart:io';

import 'package:flutter/services.dart';
import 'package:usque/core/usque_theme.dart';

const _fallbackFonts = <String, Map<String, String>>{
  'Noto Sans CJK SC': {
    'NotoSansCJKsc-Regular.subset.otf':
        'f2b9c78cd1566f7334921aca5bf07e1c5c84b1774544ce281f5f14f8d99857c9',
    'NotoSansCJKsc-Bold.subset.otf':
        'e3d1607ee156f090d9ea826a7788aca1b04ee3f333bb6e6491d3e60390a0291a',
  },
  'Noto Naskh Arabic': {
    'NotoNaskhArabic-Regular.ttf':
        '6f0a92031367b2f5a2078fe9d24f3433122b61a0bad57c423aad8f3c39aa2e6e',
    'NotoNaskhArabic-Bold.ttf':
        '04017b0ccdd1156dcde3bc386734c1b03165261762f16a025c8afc9088ae76e9',
  },
};

Future<ByteData> _fixtureData(String name, String sha256) async {
  final file = File('test/fonts/$name');
  final result = await Process.run('/usr/bin/sha256sum', ['--', file.path]);
  if (result.exitCode != 0 ||
      !(result.stdout as String).startsWith('$sha256  ')) {
    throw StateError(
      'Linux golden font hash mismatch: ${file.path}. '
      'Restore the reviewed fixture; see test/fonts/README.md.',
    );
  }
  return ByteData.sublistView(await file.readAsBytes());
}

/// Loads deterministic, real-family fonts for the Linux golden test isolate.
///
/// These fixtures never enter production assets. Missing or changed fixtures
/// fail the suite instead of silently selecting a host-installed fallback.
Future<void> loadLinuxGoldenFonts() async {
  if (!Platform.isLinux) {
    throw UnsupportedError('Linux golden fonts require a Linux test host.');
  }
  // FlutterTest substitutes Square glyphs for a few CJK characters when an
  // unavailable family is requested. An empty face models that family's real
  // absence on this Linux fixture and lets the actual Noto fallback win.
  // No Noto glyphs are registered under Windows/macOS or other absent names.
  final missingFamily = await _fixtureData(
    'MissingFamily.ttf',
    'dc5c8578f5f46765ab2764558d910c48e3b36c0841db30e58b5e89bd33890ee9',
  );
  for (final family in {
    ...UsqueFonts.fallback,
    ...UsqueFonts.monoFallback,
  }.difference(_fallbackFonts.keys.toSet())) {
    await (FontLoader(family)..addFont(Future.value(missingFamily))).load();
  }
  await (FontLoader(
    'MaterialIcons',
  )..addFont(rootBundle.load('fonts/MaterialIcons-Regular.otf'))).load();
  await (FontLoader('packages/lucide_icons_flutter/Lucide')..addFont(
        rootBundle.load('packages/lucide_icons_flutter/assets/lucide.ttf'),
      ))
      .load();
  for (final family in <String, List<String>>{
    'SpaceGrotesk': ['Medium', 'SemiBold', 'Bold'],
    'Manrope': ['Regular', 'Medium', 'SemiBold', 'Bold'],
    'IBMPlexMono': ['Regular', 'Medium'],
  }.entries) {
    final loader = FontLoader(family.key);
    for (final weight in family.value) {
      loader.addFont(rootBundle.load('assets/fonts/${family.key}-$weight.ttf'));
    }
    await loader.load();
  }
  for (final family in _fallbackFonts.entries) {
    final loader = FontLoader(family.key);
    for (final fixture in family.value.entries) {
      loader.addFont(_fixtureData(fixture.key, fixture.value));
    }
    await loader.load();
  }
}
