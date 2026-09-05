import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/screens/advanced_settings_screen.dart';
import 'package:usque/state/app_controller.dart';
import 'package:usque/state/network_quality_controller.dart';

import 'quality_test_support.dart' show qualityFixture;
import 'ui_workflow_test.dart' show WorkflowEngine, workflowHost;

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  setUpAll(() async {
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
    }
  });

  testWidgets(
    'workflow remains usable with real fonts, large text and landscape',
    (tester) async {
      tester.view.devicePixelRatio = 1;
      addTearDown(tester.view.resetDevicePixelRatio);
      addTearDown(tester.view.resetPhysicalSize);
      try {
        for (final size in const [
          Size(375, 812),
          Size(812, 375),
          Size(800, 1100),
          Size(1280, 720),
        ]) {
          tester.view.physicalSize = size;
          debugDefaultTargetPlatformOverride = size.width < 1000
              ? TargetPlatform.android
              : TargetPlatform.windows;
          for (final locale in [
            LocalePreference.english,
            LocalePreference.simplifiedChinese,
          ]) {
            for (final section in [
              AppSection.home,
              AppSection.proxy,
              AppSection.settings,
            ]) {
              final app = AppController(WorkflowEngine())
                ..section = section
                ..localePreference = locale
                ..engineCapabilities = const EngineCapabilities(
                  networkQuality: true,
                );
              try {
                await tester.pumpWidget(
                  workflowHost(
                    app,
                    dark: locale == LocalePreference.simplifiedChinese,
                    scale: 2,
                  ),
                );
                await tester.pumpAndSettle();
                expect(
                  tester.takeException(),
                  isNull,
                  reason: '$size $locale $section',
                );
                if (section == AppSection.proxy) {
                  final apply = find.widgetWithText(
                    FilledButton,
                    app.strings.get('save_changes'),
                  );
                  final rect = tester.getRect(apply);
                  expect(rect.height, greaterThanOrEqualTo(48));
                  expect(rect.bottom, lessThanOrEqualTo(size.height));
                }
                await tester.pumpWidget(const SizedBox.shrink());
              } finally {
                app.dispose();
              }
            }
            final app = AppController(WorkflowEngine())
              ..localePreference = locale;
            try {
              await tester.pumpWidget(
                workflowHost(
                  app,
                  scale: 2,
                  dark: locale == LocalePreference.simplifiedChinese,
                  home: AdvancedSettingsScreen(controller: app),
                ),
              );
              await tester.pumpAndSettle();
              expect(
                tester.takeException(),
                isNull,
                reason: 'advanced $size $locale',
              );
              final apply = find.widgetWithText(
                FilledButton,
                app.strings.get('save_changes'),
              );
              expect(
                tester.getRect(apply).bottom,
                lessThanOrEqualTo(size.height),
              );
              await tester.pumpWidget(const SizedBox.shrink());
            } finally {
              app.dispose();
            }
          }
        }
      } finally {
        debugDefaultTargetPlatformOverride = null;
      }
    },
    tags: 'golden',
  );

  for (final scene
      in <
        ({
          String name,
          Size size,
          AppSection section,
          bool dark,
          bool zh,
          bool connected,
        })
      >[
        (
          name: 'home_phone_connected',
          size: const Size(375, 812),
          section: AppSection.home,
          dark: false,
          zh: true,
          connected: true,
        ),
        (
          name: 'home_phone_idle',
          size: const Size(375, 812),
          section: AppSection.home,
          dark: true,
          zh: false,
          connected: false,
        ),
        (
          name: 'home_phone_error',
          size: const Size(375, 812),
          section: AppSection.home,
          dark: true,
          zh: true,
          connected: false,
        ),
        (
          name: 'home_phone_connected_tall',
          size: const Size(430, 932),
          section: AppSection.home,
          dark: true,
          zh: false,
          connected: true,
        ),
        (
          name: 'home_phone_details_expanded',
          size: const Size(424, 924),
          section: AppSection.home,
          dark: false,
          zh: true,
          connected: true,
        ),
        (
          name: 'home_phone_details_expanded_dark',
          size: const Size(375, 812),
          section: AppSection.home,
          dark: true,
          zh: false,
          connected: true,
        ),
        (
          name: 'home_desktop_connected',
          size: const Size(1280, 900),
          section: AppSection.home,
          dark: true,
          zh: false,
          connected: true,
        ),
        (
          name: 'proxy_phone_draft',
          size: const Size(375, 812),
          section: AppSection.proxy,
          dark: false,
          zh: true,
          connected: false,
        ),
        (
          name: 'settings_desktop_groups',
          size: const Size(1280, 1000),
          section: AppSection.settings,
          dark: false,
          zh: true,
          connected: false,
        ),
      ]) {
    testWidgets('workflow golden ${scene.name}', (tester) async {
      tester.view.devicePixelRatio = 1;
      tester.view.physicalSize = scene.size;
      addTearDown(tester.view.resetDevicePixelRatio);
      addTearDown(tester.view.resetPhysicalSize);
      debugDefaultTargetPlatformOverride = scene.size.width < 760
          ? TargetPlatform.android
          : TargetPlatform.windows;
      final engine = WorkflowEngine();
      var now = DateTime.utc(2026, 9, 2, 12);
      final app =
          AppController(
              engine,
              qualityController: NetworkQualityController(
                engine,
                now: () => now,
                autoTick: false,
              ),
            )
            ..section = scene.section
            ..localePreference = scene.zh
                ? LocalePreference.simplifiedChinese
                : LocalePreference.english
            ..engineCapabilities = const EngineCapabilities(
              networkQuality: true,
            );
      app.profileIdentityStates = {
        app.activeProfileId: ProfileIdentityState.ready,
      };
      if (scene.connected) {
        app.snapshot = const EngineSnapshot(
          phase: ConnectionPhase.connected,
          transport: 'HTTP/3',
          addressFamily: 'IPv4',
          killSwitchState: 'active',
          downloadBytesPerSecond: 262144,
          uploadBytesPerSecond: 32768,
          frontends: [
            FrontendRuntimeStatus(
              kind: FrontendKind.tunnel,
              phase: FrontendPhase.active,
            ),
            FrontendRuntimeStatus(
              kind: FrontendKind.socks5,
              phase: FrontendPhase.active,
            ),
            FrontendRuntimeStatus(
              kind: FrontendKind.http,
              phase: FrontendPhase.active,
            ),
          ],
          exit: ExitInfo(
            country: 'Singapore',
            ipv4: '198.51.100.10',
            ipv6: '2001:db8::10',
          ),
        );
      }
      if (scene.name == 'home_phone_error') {
        app.snapshot = const EngineSnapshot(
          phase: ConnectionPhase.error,
          errorCode: 'TEST_CONNECTION_FAILED',
        );
        app.lastError = '暂时无法连接，请检查网络后重试。';
      }
      if (scene.name.startsWith('home_phone_connected')) {
        var downloaded = 0;
        var uploaded = 0;
        // Synthetic engine samples, not a UI-generated curve. The rendered
        // trace is derived from these timestamped cumulative counters.
        for (var second = 0; second < 60; second++) {
          now = DateTime.utc(2026, 9, 2, 12).add(Duration(seconds: second));
          final down = (128 + second * 17 % 240) * 1024;
          final up = (20 + second * 7 % 48) * 1024;
          downloaded += down;
          uploaded += up;
          app.snapshot = EngineSnapshot(
            phase: ConnectionPhase.connected,
            transport: 'HTTP/3',
            addressFamily: 'IPv4',
            killSwitchState: 'active',
            downloadBytesPerSecond: down,
            uploadBytesPerSecond: up,
            downloadedBytes: downloaded,
            uploadedBytes: uploaded,
            networkQuality: qualityFixture(now),
            frontends: const [
              FrontendRuntimeStatus(
                kind: FrontendKind.tunnel,
                phase: FrontendPhase.active,
              ),
              FrontendRuntimeStatus(
                kind: FrontendKind.socks5,
                phase: FrontendPhase.active,
              ),
              FrontendRuntimeStatus(
                kind: FrontendKind.http,
                phase: FrontendPhase.active,
              ),
            ],
            exit: const ExitInfo(
              country: 'Singapore',
              ipv4: '198.51.100.10',
              ipv6: '2001:db8::10',
            ),
          );
        }
      }
      try {
        final boundary = GlobalKey();
        await tester.pumpWidget(
          RepaintBoundary(
            key: boundary,
            child: workflowHost(app, dark: scene.dark),
          ),
        );
        await tester.runAsync(
          () => precacheImage(
            const AssetImage('assets/branding/usque-ui-icon.png'),
            tester.element(find.byType(MaterialApp)),
          ),
        );
        await tester.pumpAndSettle();
        if (scene.name.startsWith('home_phone_details_expanded')) {
          final details = find.text(app.strings.get('connection_details'));
          await tester.ensureVisible(details);
          await tester.pumpAndSettle();
          await tester.tap(details);
          await tester.pumpAndSettle();
          await Scrollable.ensureVisible(tester.element(details));
          await tester.pumpAndSettle();
          expect(find.widgetWithText(SelectableText, 'HTTP/3'), findsOneWidget);
          expect(find.byType(ErrorWidget), findsNothing);
        }
        if (scene.section == AppSection.proxy) {
          final port = find
              .byWidgetPredicate(
                (widget) =>
                    widget is TextField && widget.decoration?.labelText == '端口',
              )
              .first;
          await tester.enterText(port, '9090');
          FocusManager.instance.primaryFocus?.unfocus();
          await tester.pumpAndSettle();
        }
        expect(tester.takeException(), isNull);
        await expectLater(
          find.byKey(boundary),
          matchesGoldenFile('goldens/${scene.name}.png'),
        );
        await tester.pumpWidget(const SizedBox.shrink());
      } finally {
        app.dispose();
        debugDefaultTargetPlatformOverride = null;
      }
    }, tags: 'golden');
  }
}
