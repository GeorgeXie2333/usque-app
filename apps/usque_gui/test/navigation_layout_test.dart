import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter/rendering.dart';
import 'package:flutter/services.dart';
import 'package:flutter_localizations/flutter_localizations.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:usque/core/app_strings.dart';
import 'package:usque/core/usque_theme.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/screens/shell_screen.dart';
import 'package:usque/services/engine_client.dart';
import 'package:usque/state/app_controller.dart';

Future<ByteData> _fontData(String path) => SynchronousFuture<ByteData>(
  ByteData.sublistView(File(path).readAsBytesSync()),
);

String _fontConfigMatch(String pattern) {
  final result = Process.runSync('fc-match', <String>[
    '--format=%{file}',
    pattern,
  ]);
  if (result.exitCode != 0) {
    throw StateError('fc-match failed for $pattern: ${result.stderr}');
  }
  final path = (result.stdout as String).trim();
  if (path.isEmpty || !File(path).existsSync()) {
    throw StateError('fc-match returned no readable font for $pattern');
  }
  return path;
}

Future<void> _loadPlatformFallbackFonts() async {
  final fonts = switch (Platform.operatingSystem) {
    'windows' => <(String, String)>[
      ('Microsoft YaHei UI', r'C:\Windows\Fonts\msyh.ttc'),
      ('Microsoft JhengHei UI', r'C:\Windows\Fonts\msjh.ttc'),
      ('Yu Gothic UI', r'C:\Windows\Fonts\YuGothR.ttc'),
      ('Malgun Gothic', r'C:\Windows\Fonts\malgun.ttf'),
      ('Tahoma', r'C:\Windows\Fonts\tahoma.ttf'),
      ('Leelawadee UI', r'C:\Windows\Fonts\LeelawUI.ttf'),
    ],
    'linux' => <(String, String)>[
      ('Noto Sans CJK SC', _fontConfigMatch('Noto Sans CJK SC:lang=zh-cn')),
      ('Noto Sans CJK TC', _fontConfigMatch('Noto Sans CJK TC:lang=zh-tw')),
      ('Noto Sans JP', _fontConfigMatch('Noto Sans JP:lang=ja')),
      ('Noto Sans KR', _fontConfigMatch('Noto Sans KR:lang=ko')),
      ('Noto Naskh Arabic', _fontConfigMatch('Noto Naskh Arabic:lang=ar')),
      ('Noto Sans Thai', _fontConfigMatch('Noto Sans Thai:lang=th')),
    ],
    'macos' => <(String, String)>[
      ('PingFang SC', '/System/Library/Fonts/PingFang.ttc'),
      ('PingFang TC', '/System/Library/Fonts/PingFang.ttc'),
      ('PingFang HK', '/System/Library/Fonts/PingFang.ttc'),
      ('Hiragino Sans', '/System/Library/Fonts/ヒラギノ角ゴシック W3.ttc'),
      ('Apple SD Gothic Neo', '/System/Library/Fonts/AppleSDGothicNeo.ttc'),
      ('Geeza Pro', '/System/Library/Fonts/Supplemental/Geeza Pro.ttc'),
      ('Thonburi', '/System/Library/Fonts/Thonburi.ttc'),
    ],
    final platform => throw UnsupportedError(
      'Navigation fallback-font layout is not configured for $platform',
    ),
  };

  for (final (family, path) in fonts) {
    if (!File(path).existsSync()) {
      throw StateError('Required $family fallback font is missing: $path');
    }
    await (FontLoader(family)..addFont(_fontData(path))).load();
  }
}

void main() {
  testWidgets(
    'four-item navigation labels fit every locale on phone and rail',
    (tester) async {
      // This file has its own test isolate, so registered fonts cannot alter
      // typography in app_test.dart or other widget-test files.
      await (FontLoader('Manrope')
            ..addFont(rootBundle.load('assets/fonts/Manrope-Regular.ttf'))
            ..addFont(rootBundle.load('assets/fonts/Manrope-Medium.ttf'))
            ..addFont(rootBundle.load('assets/fonts/Manrope-SemiBold.ttf'))
            ..addFont(rootBundle.load('assets/fonts/Manrope-Bold.ttf')))
          .load();
      await _loadPlatformFallbackFonts();
      tester.view.devicePixelRatio = 1;
      const safeInsets = FakeViewPadding(left: 12, right: 16, bottom: 24);
      tester.view.viewPadding = safeInsets;
      tester.view.padding = safeInsets;
      addTearDown(tester.view.resetDevicePixelRatio);
      addTearDown(tester.view.resetPhysicalSize);
      addTearDown(tester.view.resetPadding);
      addTearDown(tester.view.resetViewPadding);
      addTearDown(tester.platformDispatcher.clearTextScaleFactorTestValue);
      final preferences = LocalePreference.values
          .where((preference) => preference != LocalePreference.system)
          .toList(growable: false);
      final controller = AppController(MethodChannelEngineClient());
      addTearDown(controller.dispose);

      Future<void> verifyMatrix(Size size, double textScaleFactor) async {
        tester.view.physicalSize = size;
        tester.platformDispatcher.textScaleFactorTestValue = textScaleFactor;
        for (final preference in preferences) {
          controller.localePreference = preference;
          controller.section = AppSection.home;
          final strings = AppStrings(preference);
          final locale = Locale(strings.languageCode);
          final labels = <String>[
            strings.get('nav_home'),
            strings.get('nav_profiles'),
            strings.get('nav_proxy'),
            strings.get('nav_settings'),
          ];
          await tester.pumpWidget(
            MaterialApp(
              key: ValueKey<String>(
                'nav-${size.width}-$textScaleFactor-${preference.name}',
              ),
              theme: UsqueTheme.light(),
              locale: locale,
              supportedLocales: <Locale>[locale],
              localizationsDelegates: const <LocalizationsDelegate<dynamic>>[
                GlobalMaterialLocalizations.delegate,
                GlobalWidgetsLocalizations.delegate,
                GlobalCupertinoLocalizations.delegate,
              ],
              home: ShellScreen(controller: controller),
            ),
          );
          await tester.pump();
          await tester.pump(const Duration(milliseconds: 300));

          expect(
            tester.takeException(),
            isNull,
            reason: '${preference.name} at $size × $textScaleFactor',
          );
          final barFinder = find.byType(NavigationBar);
          final bar = tester.widget<NavigationBar>(barFinder);
          expect(bar.destinations, hasLength(4));
          final context = tester.element(barFinder);
          expect(
            bar.labelBehavior ??
                Theme.of(context).navigationBarTheme.labelBehavior,
            NavigationDestinationLabelBehavior.alwaysShow,
          );
          final barRect = tester.getRect(barFinder);
          expect(barRect.left, closeTo(safeInsets.left, 0.01));
          expect(barRect.right, closeTo(size.width - safeInsets.right, 0.01));
          expect(
            barRect.bottom,
            closeTo(size.height - safeInsets.bottom, 0.01),
          );
          expect(barRect.height, closeTo(66, 0.01));
          final cellWidth = tester.getSize(barFinder).width / 4;

          for (final label in labels) {
            final labelFinder = find.descendant(
              of: barFinder,
              matching: find.text(label),
            );
            expect(labelFinder, findsOneWidget);
            final paragraph = tester.renderObject<RenderParagraph>(labelFinder);
            final boxes = paragraph.getBoxesForSelection(
              TextSelection(baseOffset: 0, extentOffset: label.length),
            );
            final lineTops = boxes.map((box) => box.top.round()).toSet();
            final left = boxes
                .map((box) => box.left)
                .reduce((a, b) => a < b ? a : b);
            final right = boxes
                .map((box) => box.right)
                .reduce((a, b) => a > b ? a : b);

            expect(
              paragraph.didExceedMaxLines,
              isFalse,
              reason:
                  '${preference.name} "$label" was truncated at '
                  '$textScaleFactor×',
            );
            expect(
              lineTops,
              hasLength(1),
              reason:
                  '${preference.name} "$label" wrapped at $textScaleFactor×',
            );
            expect(
              right - left,
              lessThanOrEqualTo(cellWidth),
              reason:
                  '${preference.name} "$label" exceeds the '
                  '${cellWidth.toStringAsFixed(1)}dp navigation cell',
            );
          }
          if (preference == LocalePreference.arabic ||
              preference == LocalePreference.persian) {
            expect(Directionality.of(context), TextDirection.rtl);
          }
        }
      }

      Future<void> verifyCompactRail(Size size, double textScaleFactor) async {
        tester.view.physicalSize = size;
        tester.platformDispatcher.textScaleFactorTestValue = textScaleFactor;
        for (final preference in preferences) {
          controller.localePreference = preference;
          controller.section = AppSection.home;
          final strings = AppStrings(preference);
          final locale = Locale(strings.languageCode);
          final labels = <String>[
            strings.get('nav_home'),
            strings.get('nav_profiles'),
            strings.get('nav_proxy'),
            strings.get('nav_settings'),
          ];
          await tester.pumpWidget(
            MaterialApp(
              key: ValueKey<String>('rail-${preference.name}'),
              theme: UsqueTheme.light(),
              locale: locale,
              supportedLocales: <Locale>[locale],
              localizationsDelegates: const <LocalizationsDelegate<dynamic>>[
                GlobalMaterialLocalizations.delegate,
                GlobalWidgetsLocalizations.delegate,
                GlobalCupertinoLocalizations.delegate,
              ],
              home: ShellScreen(controller: controller),
            ),
          );
          await tester.pump();
          await tester.pump(const Duration(milliseconds: 300));

          expect(
            tester.takeException(),
            isNull,
            reason: '${preference.name} compact rail at $textScaleFactor×',
          );
          final railFinder = find.byType(NavigationRail);
          final rail = tester.widget<NavigationRail>(railFinder);
          expect(rail.extended, isFalse);
          expect(rail.labelType, NavigationRailLabelType.all);
          expect(rail.destinations, hasLength(4));
          final availableWidth = tester.getSize(railFinder).width - 16;

          for (final label in labels) {
            final labelFinder = find.descendant(
              of: railFinder,
              matching: find.text(label),
            );
            expect(labelFinder, findsOneWidget);
            final paragraph = tester.renderObject<RenderParagraph>(labelFinder);
            final boxes = paragraph.getBoxesForSelection(
              TextSelection(baseOffset: 0, extentOffset: label.length),
            );
            final lineTops = boxes.map((box) => box.top.round()).toSet();
            final left = boxes
                .map((box) => box.left)
                .reduce((a, b) => a < b ? a : b);
            final right = boxes
                .map((box) => box.right)
                .reduce((a, b) => a > b ? a : b);

            expect(
              paragraph.didExceedMaxLines,
              isFalse,
              reason: '${preference.name} rail "$label" was truncated',
            );
            expect(
              lineTops,
              hasLength(1),
              reason: '${preference.name} rail "$label" wrapped',
            );
            expect(
              right - left,
              lessThanOrEqualTo(availableWidth + 0.01),
              reason: '${preference.name} rail "$label" exceeds the rail',
            );
          }
        }
      }

      await verifyMatrix(const Size(320, 800), 1);
      await verifyMatrix(const Size(360, 800), 1.3);
      await verifyCompactRail(const Size(900, 800), 1.3);
      await tester.pumpWidget(const SizedBox.shrink());
      await tester.pump();
    },
  );
}
