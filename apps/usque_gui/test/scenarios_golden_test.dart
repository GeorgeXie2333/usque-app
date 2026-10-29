@Tags(['golden'])
library;

import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:usque/models/app_models.dart';

import 'scenarios_test.dart' show SceneEngine;
import 'ui_workflow_test.dart' show pumpWorkflow, workflowHost;

void main() {
  setUpAll(() async {
    await (FontLoader(
      'MaterialIcons',
    )..addFont(rootBundle.load('fonts/MaterialIcons-Regular.otf'))).load();
    await (FontLoader('packages/lucide_icons_flutter/Lucide')..addFont(
          rootBundle.load('packages/lucide_icons_flutter/assets/lucide.ttf'),
        ))
        .load();
    for (final family in {
      'SpaceGrotesk': ['Medium', 'SemiBold', 'Bold'],
      'Manrope': ['Regular', 'Medium', 'SemiBold', 'Bold'],
      'IBMPlexMono': ['Regular', 'Medium'],
    }.entries) {
      final loader = FontLoader(family.key);
      for (final weight in family.value) {
        loader.addFont(
          rootBundle.load('assets/fonts/${family.key}-$weight.ttf'),
        );
      }
      await loader.load();
    }
    if (Platform.isWindows) {
      await (FontLoader('Microsoft YaHei UI')..addFont(
            SynchronousFuture(
              ByteData.sublistView(
                File(r'C:\Windows\Fonts\msyh.ttc').readAsBytesSync(),
              ),
            ),
          ))
          .load();
      await (FontLoader('Tahoma')..addFont(
            SynchronousFuture(
              ByteData.sublistView(
                File(r'C:\Windows\Fonts\tahoma.ttf').readAsBytesSync(),
              ),
            ),
          ))
          .load();
    }
  });
  for (final scene in [
    (
      name: 'profiles_scenes_desktop',
      size: const Size(1280, 900),
      locale: LocalePreference.english,
      scale: 1.0,
      editor: false,
    ),
    (
      name: 'profiles_scenes_phone',
      size: const Size(390, 844),
      locale: LocalePreference.simplifiedChinese,
      scale: 1.0,
      editor: false,
    ),
    (
      name: 'profiles_scenes_rtl_large',
      size: const Size(1280, 1000),
      locale: LocalePreference.persian,
      scale: 2.0,
      editor: false,
    ),
    (
      name: 'profiles_scene_editor_phone',
      size: const Size(390, 844),
      locale: LocalePreference.simplifiedChinese,
      scale: 1.0,
      editor: true,
    ),
  ]) {
    testWidgets(
      'scene golden ${scene.name}',
      (tester) async {
        final engine = SceneEngine()..selected = 'scene-a';
        final app = await pumpWorkflow(
          tester,
          engine,
          section: AppSection.profiles,
          size: scene.size,
        );
        app.localePreference = scene.locale;
        app.engineCapabilities = await engine.getCapabilities();
        await app.scenarios.refresh();
        await tester.pumpWidget(workflowHost(app, scale: scene.scale));
        await tester.pumpAndSettle();
        if (scene.editor) {
          await tester.tap(find.byKey(const ValueKey('scenario-create')));
          await tester.pumpAndSettle();
        }
        expect(tester.takeException(), isNull);
        await expectLater(
          find.byType(MaterialApp),
          matchesGoldenFile('goldens/${scene.name}.png'),
        );
      },
      skip: !Platform.isWindows,
      variant: TargetPlatformVariant({
        scene.size.width >= 760
            ? TargetPlatform.windows
            : TargetPlatform.android,
      }),
    );
  }
}
