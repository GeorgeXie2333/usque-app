import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:shared_preferences/shared_preferences.dart';
import 'package:usque/app.dart';
import 'package:usque/core/app_strings.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/screens/shell_screen.dart';

import 'app_test.dart' show FakeEngineClient;

void main() {
  test('catalogs and framework controls share the resolved locale', () {
    const cases = <(Locale, Locale)>[
      (
        Locale.fromSubtags(languageCode: 'zh', scriptCode: 'Hant'),
        Locale('zh', 'TW'),
      ),
      (
        Locale.fromSubtags(
          languageCode: 'zh',
          scriptCode: 'Hant',
          countryCode: 'HK',
        ),
        Locale('zh', 'HK'),
      ),
      (Locale('zh', 'MO'), Locale('zh', 'HK')),
      (Locale('zh', 'TW'), Locale('zh', 'TW')),
      (Locale('zh', 'CN'), Locale('zh', 'CN')),
      (Locale('pt', 'PT'), Locale('pt', 'BR')),
      (Locale('de', 'CH'), Locale('de')),
      (Locale('xx'), Locale('en')),
    ];
    for (final (systemLocale, expected) in cases) {
      final strings = AppStrings(
        LocalePreference.system,
        systemLocale: systemLocale,
      );
      expect(strings.locale, expected, reason: '$systemLocale');
      expect(
        AppStrings.resolveCatalogId(LocalePreference.system, strings.locale),
        strings.catalogId,
      );
    }
    for (final preference in LocalePreference.values) {
      final strings = AppStrings(preference, systemLocale: const Locale('en'));
      expect(
        AppStrings.resolveCatalogId(LocalePreference.system, strings.locale),
        strings.catalogId,
        reason: preference.name,
      );
    }
  });

  const cases = <String, (LocalePreference, Locale, Locale)>{
    'system Hant': (
      LocalePreference.system,
      Locale.fromSubtags(languageCode: 'zh', scriptCode: 'Hant'),
      Locale('zh', 'TW'),
    ),
    'system Macau': (
      LocalePreference.system,
      Locale('zh', 'MO'),
      Locale('zh', 'HK'),
    ),
    'explicit Hong Kong': (
      LocalePreference.traditionalChineseHongKong,
      Locale('en'),
      Locale('zh', 'HK'),
    ),
    'explicit Taiwan': (
      LocalePreference.traditionalChineseTaiwan,
      Locale('en'),
      Locale('zh', 'TW'),
    ),
  };
  for (final entry in cases.entries) {
    testWidgets(
      '${entry.key} keeps framework controls in Traditional Chinese',
      (tester) async {
        final (preference, systemLocale, expected) = entry.value;
        tester.platformDispatcher.localeTestValue = systemLocale;
        tester.platformDispatcher.localesTestValue = [systemLocale];
        addTearDown(tester.platformDispatcher.clearLocaleTestValue);
        addTearDown(tester.platformDispatcher.clearLocalesTestValue);
        const channel = MethodChannel('io.github.georgexie2333.usque/engine');
        final messenger = tester.binding.defaultBinaryMessenger;
        final trayStates = <Map<String, Object?>>[];
        messenger.setMockMethodCallHandler(channel, (call) async {
          if (call.method == 'getWindowVisibility') return true;
          if (call.method == 'updateTrayState') {
            trayStates.add(
              Map<String, Object?>.from(
                call.arguments as Map<Object?, Object?>,
              ),
            );
          }
          return null;
        });
        addTearDown(() => messenger.setMockMethodCallHandler(channel, null));
        SharedPreferences.setMockInitialValues({
          'onboarding_complete': true,
          'update_checks_enabled': false,
          'locale': preference.name,
        });
        // Headless widgets and mocked native calls only: never launch a desktop
        // app, permission prompt, browser, Engine or Network Extension.
        await tester.pumpWidget(UsqueBootstrap(engine: FakeEngineClient()));
        await tester.pumpAndSettle();
        final context = tester.element(find.byType(Scaffold).first);
        expect(Localizations.localeOf(context), expected);
        expect(MaterialLocalizations.of(context).copyButtonLabel, '複製');
        final strings = AppStrings(preference, systemLocale: systemLocale);
        expect(strings.locale, expected);
        final controller = tester
            .widget<ShellScreen>(find.byType(ShellScreen))
            .controller;
        expect(controller.strings.catalogId, strings.catalogId);
        expect(trayStates.last['catalog_id'], strings.catalogId);
        if (preference == LocalePreference.system) {
          // A hidden window still publishes the new native/CEF language. OS
          // events never replace the saved "follow system" preference.
          controller.setUiVisible(false);
          for (final next in [
            const Locale('en'),
            const Locale('zh', 'HK'),
            const Locale('zh', 'TW'),
          ]) {
            tester.platformDispatcher.localeTestValue = next;
            tester.platformDispatcher.localesTestValue = [next];
            await tester.pumpAndSettle();
            expect(controller.strings.locale, next);
            expect(trayStates.last['catalog_id'], controller.strings.catalogId);
            expect(Localizations.localeOf(context), next);
          }
          final preferences = await SharedPreferences.getInstance();
          expect(preferences.getString('locale'), LocalePreference.system.name);
        } else {
          tester.platformDispatcher.localeTestValue = const Locale('zh', 'CN');
          tester.platformDispatcher.localesTestValue = [
            const Locale('zh', 'CN'),
          ];
          await tester.pumpAndSettle();
          expect(controller.strings.catalogId, strings.catalogId);
          expect(trayStates.last['catalog_id'], strings.catalogId);
          expect(Localizations.localeOf(context), expected);
          final next = preference == LocalePreference.traditionalChineseHongKong
              ? LocalePreference.traditionalChineseTaiwan
              : LocalePreference.traditionalChineseHongKong;
          await controller.setLocale(next);
          await tester.pumpAndSettle();
          expect(trayStates.last['catalog_id'], AppStrings(next).catalogId);
          expect(Localizations.localeOf(context), AppStrings(next).locale);
        }
        expect(tester.takeException(), isNull);
        await tester.pumpWidget(const SizedBox.shrink());
        await tester.pumpAndSettle();
      },
      variant: TargetPlatformVariant.only(TargetPlatform.macOS),
    );
  }
}
