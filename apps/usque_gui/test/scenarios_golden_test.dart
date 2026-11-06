@Tags(['golden'])
library;

import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/widgets/home/home_phone.dart';

import 'scenarios_test.dart'
    show SceneEngine, confirmRichScenes, richSceneEngine, seedGoldenAccounts;
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
      accounts: false,
    ),
    (
      name: 'profiles_scenes_phone',
      size: const Size(390, 844),
      locale: LocalePreference.simplifiedChinese,
      scale: 1.0,
      editor: false,
      accounts: false,
    ),
    (
      name: 'profiles_scenes_rtl_large',
      size: const Size(1280, 1000),
      locale: LocalePreference.persian,
      scale: 2.0,
      editor: false,
      accounts: false,
    ),
    (
      name: 'profiles_accounts_desktop',
      size: const Size(1280, 900),
      locale: LocalePreference.english,
      scale: 1.0,
      editor: false,
      accounts: true,
    ),
    (
      name: 'profiles_accounts_phone_large',
      size: const Size(390, 844),
      locale: LocalePreference.simplifiedChinese,
      scale: 2.0,
      editor: false,
      accounts: true,
    ),
    (
      name: 'profiles_accounts_rtl_large',
      size: const Size(1280, 1000),
      locale: LocalePreference.persian,
      scale: 2.0,
      editor: false,
      accounts: true,
    ),
    (
      name: 'profiles_scene_editor_phone',
      size: const Size(390, 844),
      locale: LocalePreference.simplifiedChinese,
      scale: 1.0,
      editor: true,
      accounts: false,
    ),
    (
      name: 'profiles_scene_accounts_phone_large',
      size: const Size(390, 844),
      locale: LocalePreference.simplifiedChinese,
      scale: 2.0,
      editor: true,
      accounts: true,
    ),
    (
      name: 'profiles_scene_accounts_rtl_large',
      size: const Size(1280, 1000),
      locale: LocalePreference.persian,
      scale: 2.0,
      editor: true,
      accounts: true,
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
        if (scene.accounts) seedGoldenAccounts(engine, app);
        await app.scenarios.refresh();
        await tester.pumpWidget(workflowHost(app, scale: scene.scale));
        await tester.pumpAndSettle();
        if (scene.editor) {
          await tester.tap(find.byKey(const ValueKey('scenario-create')));
          await tester.pumpAndSettle();
        }
        if (scene.accounts) {
          final account = find.byKey(
            ValueKey(
              scene.editor
                  ? 'scenario-account-management'
                  : 'profiles-manage-accounts',
            ),
          );
          await tester.ensureVisible(account);
          await tester.tap(account);
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
  testWidgets(
    'scene golden profiles_scenes_modified_desktop',
    (tester) async {
      // A drifted selection with a running session and differing rows.
      final engine = richSceneEngine();
      final app = await pumpWorkflow(
        tester,
        engine,
        section: AppSection.profiles,
        size: const Size(1280, 900),
      );
      app.localePreference = LocalePreference.english;
      await confirmRichScenes(engine, app);
      await tester.pumpWidget(workflowHost(app));
      await tester.pumpAndSettle();
      expect(tester.takeException(), isNull);
      await expectLater(
        find.byType(MaterialApp),
        matchesGoldenFile('goldens/profiles_scenes_modified_desktop.png'),
      );
    },
    skip: !Platform.isWindows,
    variant: TargetPlatformVariant.only(TargetPlatform.windows),
  );
  for (final scale in [1.0, 2.0]) {
    testWidgets(
      'phone scene status card at text scale $scale',
      (tester) async {
        final engine = SceneEngine()..selected = 'scene-a';
        final app = await pumpWorkflow(
          tester,
          engine,
          section: AppSection.home,
          size: const Size(424, 924),
        );
        app.localePreference = LocalePreference.simplifiedChinese;
        app.engineCapabilities = await engine.getCapabilities();
        await app.scenarios.refresh();
        await tester.pumpWidget(
          workflowHost(
            app,
            scale: scale,
            home: Scaffold(
              body: SingleChildScrollView(
                padding: const EdgeInsets.all(16),
                child: RepaintBoundary(
                  key: const ValueKey('scene-status-golden'),
                  child: HomeStatusCard(controller: app, strings: app.strings),
                ),
              ),
            ),
          ),
        );
        await tester.pumpAndSettle();
        expect(tester.takeException(), isNull);
        await expectLater(
          find.byKey(const ValueKey('scene-status-golden')),
          matchesGoldenFile('goldens/home_scene_phone_${scale.toInt()}x.png'),
        );
      },
      skip: !Platform.isWindows,
      variant: TargetPlatformVariant.only(TargetPlatform.android),
    );
  }
}
