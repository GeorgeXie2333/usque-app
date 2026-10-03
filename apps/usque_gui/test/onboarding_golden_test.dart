import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_localizations/flutter_localizations.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:shared_preferences/shared_preferences.dart';
import 'package:usque/core/usque_theme.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/models/onboarding_models.dart';
import 'package:usque/screens/onboarding_screen.dart';
import 'package:usque/state/app_controller.dart';

import 'app_test.dart' show FakeEngineClient;

class _DeniedPermissionsEngine extends FakeEngineClient {
  @override
  Future<OnboardingPermissionState> getOnboardingPermissions() async =>
      const OnboardingPermissionState(
        vpnGranted: false,
        notification: OnboardingNotificationPermission.notGranted,
      );
}

Widget _host(AppController app, {required bool dark, double scale = 1}) {
  final locale = switch (app.localePreference) {
    LocalePreference.simplifiedChinese => const Locale('zh', 'CN'),
    LocalePreference.persian => const Locale('fa'),
    _ => const Locale('en'),
  };
  return MaterialApp(
    debugShowCheckedModeBanner: false,
    theme: dark ? UsqueTheme.dark() : UsqueTheme.light(),
    locale: locale,
    supportedLocales: const [Locale('en'), Locale('zh', 'CN'), Locale('fa')],
    localizationsDelegates: const [
      GlobalMaterialLocalizations.delegate,
      GlobalWidgetsLocalizations.delegate,
      GlobalCupertinoLocalizations.delegate,
    ],
    builder: (context, child) => MediaQuery(
      data: MediaQuery.of(
        context,
      ).copyWith(textScaler: TextScaler.linear(scale), disableAnimations: true),
      child: child!,
    ),
    home: OnboardingScreen(controller: app),
  );
}

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  setUp(() => SharedPreferences.setMockInitialValues(<String, Object>{}));
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
      final arabicFallback = FontLoader('Segoe UI');
      for (final filename in ['segoeui.ttf', 'segoeuisb.ttf', 'segoeuib.ttf']) {
        final file = File('C:/Windows/Fonts/$filename');
        if (filename == 'segoeui.ttf' || file.existsSync()) {
          arabicFallback.addFont(
            SynchronousFuture(ByteData.sublistView(file.readAsBytesSync())),
          );
        }
      }
      await arabicFallback.load();
    }
  });

  for (final fixture in [
    (
      name: 'phone_zh_light',
      size: const Size(375, 812),
      dark: false,
      scale: 1.0,
      locale: LocalePreference.simplifiedChinese,
      platform: TargetPlatform.android,
    ),
    (
      name: 'desktop_en_dark',
      size: const Size(1280, 900),
      dark: true,
      scale: 1.0,
      locale: LocalePreference.english,
      platform: TargetPlatform.windows,
    ),
  ]) {
    for (final scene in [
      'intro',
      'permissions',
      'terms',
      'register',
      'license',
      'zero_trust',
      'zero_trust_invalid',
      'zero_trust_received',
    ]) {
      _scene(
        scene,
        fixture.name,
        fixture.size,
        fixture.dark,
        fixture.scale,
        fixture.locale,
        fixture.platform,
      );
    }
  }
  for (final scene in ['terms', 'zero_trust']) {
    _scene(
      scene,
      'landscape_en_large',
      const Size(900, 375),
      false,
      2,
      LocalePreference.english,
      TargetPlatform.android,
    );
    _scene(
      scene,
      'rtl_fa_dark',
      const Size(520, 900),
      true,
      1.5,
      LocalePreference.persian,
      TargetPlatform.android,
    );
  }
  for (final scene in ['ready', 'interrupted', 'unavailable', 'reconciling']) {
    _scene(
      scene,
      'phone_zh_light',
      const Size(375, 812),
      false,
      1,
      LocalePreference.simplifiedChinese,
      TargetPlatform.android,
    );
  }
  _scene(
    'permissions_denied',
    'phone_zh_light',
    const Size(375, 812),
    false,
    1,
    LocalePreference.simplifiedChinese,
    TargetPlatform.android,
  );
}

void _scene(
  String scene,
  String fixture,
  Size size,
  bool dark,
  double scale,
  LocalePreference locale,
  TargetPlatform platform,
) {
  final name = 'onboarding_${scene}_$fixture';
  testWidgets('onboarding golden $name', (tester) async {
    Future<void> settle() async {
      if (scene == 'reconciling') {
        await tester.pump(const Duration(milliseconds: 100));
      } else {
        await tester.pumpAndSettle();
      }
    }

    tester.view.devicePixelRatio = 1;
    tester.view.physicalSize = size;
    addTearDown(tester.view.resetDevicePixelRatio);
    addTearDown(tester.view.resetPhysicalSize);
    debugDefaultTargetPlatformOverride = platform;
    final engine = scene == 'permissions_denied'
        ? _DeniedPermissionsEngine()
        : FakeEngineClient();
    final app = AppController(engine)
      ..localePreference = locale
      ..onboardingStep = switch (scene) {
        'intro' => 0,
        'permissions' || 'permissions_denied' => 1,
        'terms' => 2,
        _ => 3,
      }
      ..onboardingTermsAccepted = scene != 'terms';
    addTearDown(app.dispose);
    try {
      await app.refreshOnboardingPermissions();
      app.onboardingPhase = switch (scene) {
        'ready' => OnboardingPhase.ready,
        'interrupted' => OnboardingPhase.interrupted,
        'unavailable' => OnboardingPhase.unavailable,
        'reconciling' => OnboardingPhase.reconciling,
        _ => OnboardingPhase.idle,
      };
      final boundary = GlobalKey();
      await tester.pumpWidget(
        RepaintBoundary(
          key: boundary,
          child: _host(app, dark: dark, scale: scale),
        ),
      );
      await tester.runAsync(
        () => precacheImage(
          const AssetImage('assets/branding/usque-ui-icon.png'),
          tester.element(find.byType(MaterialApp)),
        ),
      );
      await settle();
      if (scene == 'license') {
        final method = find.text(app.strings.get('use_license_key'));
        await tester.ensureVisible(method);
        await tester.tap(method);
        await settle();
        await tester.enterText(find.byType(TextField), 'example-license');
      } else if (scene.startsWith('zero_trust')) {
        final method = find.text(app.strings.get('zero_trust_title'));
        await tester.ensureVisible(method);
        await tester.tap(method);
        await settle();
        if (scene != 'zero_trust') {
          final team = find.widgetWithText(
            TextField,
            app.strings.get('zero_trust_team'),
          );
          await tester.ensureVisible(team);
          await tester.enterText(team, 'example-team');
          if (scene == 'zero_trust_invalid') {
            final callback = find.widgetWithText(
              TextField,
              app.strings.get('zero_trust_callback'),
            );
            await tester.ensureVisible(callback);
            await tester.enterText(callback, 'https://invalid.example/auth');
          } else {
            engine.pendingZeroTrustCallback =
                'com.cloudflare.warp://example-team.cloudflareaccess.com/auth?token=golden';
            app.noteZeroTrustCallbackArrived();
          }
        }
      }
      FocusManager.instance.primaryFocus?.unfocus();
      await settle();
      final formScroll = find
          .descendant(
            of: find.byType(OnboardingScreen),
            matching: find.byType(SingleChildScrollView),
          )
          .last;
      final scrollable = find
          .descendant(of: formScroll, matching: find.byType(Scrollable))
          .first;
      if (scene == 'zero_trust_invalid' || scene == 'zero_trust_received') {
        final callback = find.widgetWithText(
          TextField,
          app.strings.get('zero_trust_callback'),
        );
        await Scrollable.ensureVisible(
          tester.element(callback),
          alignment: 0.15,
        );
      } else if (fixture == 'landscape_en_large' && scene == 'terms') {
        final policy = find.widgetWithText(
          TextButton,
          app.strings.get('onboarding_application_terms'),
        );
        await Scrollable.ensureVisible(tester.element(policy), alignment: 0.05);
        await settle();
        expect(policy.hitTestable(), findsOneWidget);
      } else if (fixture == 'landscape_en_large' && scene == 'zero_trust') {
        final team = find.widgetWithText(
          TextField,
          app.strings.get('zero_trust_team'),
        );
        await Scrollable.ensureVisible(tester.element(team), alignment: 0.1);
        await settle();
        expect(team.hitTestable(), findsOneWidget);
      } else {
        tester.state<ScrollableState>(scrollable).position.jumpTo(0);
      }
      await settle();
      expect(tester.takeException(), isNull);
      await expectLater(
        find.byKey(boundary),
        matchesGoldenFile('goldens/$name.png'),
      );
      await tester.pumpWidget(const SizedBox.shrink());
    } finally {
      debugDefaultTargetPlatformOverride = null;
    }
  }, tags: 'golden');
}
