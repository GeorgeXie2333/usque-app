import 'package:flutter/material.dart';
import 'package:flutter_localizations/flutter_localizations.dart';

import 'core/app_strings.dart';
import 'core/usque_theme.dart';
import 'models/app_models.dart';
import 'screens/onboarding_screen.dart';
import 'screens/shell_screen.dart';
import 'services/engine_client.dart';
import 'services/engine_client_factory.dart';
import 'services/platform_shell_bridge.dart';
import 'state/app_controller.dart';
import 'state/window_frame.dart';
import 'widgets/controller_selector.dart';
import 'widgets/window_titlebar.dart';

typedef _BootstrapView = ({
  bool initialized,
  bool onboardingComplete,
  ThemePreference theme,
  LocalePreference locale,
  bool onboardingBusy,
  String? onboardingError,
});

class UsqueBootstrap extends StatefulWidget {
  const UsqueBootstrap({super.key, this.engine});

  final EngineClient? engine;

  @override
  State<UsqueBootstrap> createState() => _UsqueBootstrapState();
}

class _UsqueBootstrapState extends State<UsqueBootstrap> {
  late final AppController controller;
  late final PlatformShellBridge shellBridge;

  @override
  void initState() {
    super.initState();
    controller = AppController(widget.engine ?? createDefaultEngineClient());
    shellBridge = PlatformShellBridge(controller);
    controller.initialize();
  }

  @override
  void dispose() {
    shellBridge.dispose();
    controller.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return ControllerSelector<_BootstrapView>(
      controller: controller,
      selector: (controller) => (
        initialized: controller.initialized,
        onboardingComplete: controller.onboardingComplete,
        theme: controller.themePreference,
        locale: controller.localePreference,
        onboardingBusy: controller.onboardingComplete ? false : controller.busy,
        onboardingError: controller.onboardingComplete
            ? null
            : controller.lastError,
      ),
      builder: (context, view) {
        final themeMode = switch (view.theme) {
          ThemePreference.system => ThemeMode.system,
          ThemePreference.light => ThemeMode.light,
          ThemePreference.dark => ThemeMode.dark,
        };
        final locale = switch (view.locale) {
          LocalePreference.system => null,
          LocalePreference.english => const Locale('en'),
          LocalePreference.simplifiedChinese => const Locale('zh', 'CN'),
          LocalePreference.traditionalChineseHongKong => const Locale(
            'zh',
            'HK',
          ),
          LocalePreference.traditionalChineseTaiwan => const Locale('zh', 'TW'),
          LocalePreference.japanese => const Locale('ja'),
          LocalePreference.korean => const Locale('ko'),
          LocalePreference.spanish => const Locale('es'),
          LocalePreference.portuguese => const Locale('pt', 'BR'),
          LocalePreference.french => const Locale('fr'),
          LocalePreference.dutch => const Locale('nl'),
          LocalePreference.turkish => const Locale('tr'),
          LocalePreference.russian => const Locale('ru'),
          LocalePreference.persian => const Locale('fa'),
          LocalePreference.arabic => const Locale('ar'),
          LocalePreference.german => const Locale('de'),
          LocalePreference.indonesian => const Locale('id'),
          LocalePreference.italian => const Locale('it'),
          LocalePreference.polish => const Locale('pl'),
          LocalePreference.thai => const Locale('th'),
          LocalePreference.ukrainian => const Locale('uk'),
          LocalePreference.vietnamese => const Locale('vi'),
        };
        return MaterialApp(
          title: 'Usque',
          debugShowCheckedModeBanner: false,
          theme: UsqueTheme.light(),
          darkTheme: UsqueTheme.dark(),
          themeMode: themeMode,
          locale: locale,
          supportedLocales: const <Locale>[
            Locale('en'),
            Locale('zh', 'CN'),
            Locale('zh', 'HK'),
            Locale('zh', 'TW'),
            Locale('ja'),
            Locale('ko'),
            Locale('es'),
            Locale('pt', 'BR'),
            Locale('fr'),
            Locale('nl'),
            Locale('tr'),
            Locale('ru'),
            Locale('fa'),
            Locale('ar'),
            Locale('de'),
            Locale('id'),
            Locale('it'),
            Locale('pl'),
            Locale('th'),
            Locale('uk'),
            Locale('vi'),
          ],
          localizationsDelegates: const <LocalizationsDelegate<dynamic>>[
            GlobalMaterialLocalizations.delegate,
            GlobalWidgetsLocalizations.delegate,
            GlobalCupertinoLocalizations.delegate,
          ],
          builder: WindowFrame.instance.enabled
              ? (context, child) => _WindowChrome(
                  controller: controller,
                  child: child ?? const SizedBox.shrink(),
                )
              : null,
          home: !view.initialized
              ? const _LoadingScreen()
              : view.onboardingComplete
              ? ShellScreen(controller: controller)
              : OnboardingScreen(controller: controller),
        );
      },
    );
  }
}

typedef _ChromeView = ({ConnectionPhase phase, LocalePreference locale});

/// Wraps every route in the Flutter-drawn Windows caption.
class _WindowChrome extends StatelessWidget {
  const _WindowChrome({required this.controller, required this.child});

  final AppController controller;
  final Widget child;

  @override
  Widget build(BuildContext context) {
    return ControllerSelector<_ChromeView>(
      controller: controller,
      selector: (controller) => (
        phase: controller.snapshot.phase,
        locale: controller.localePreference,
      ),
      builder: (context, view) => WindowFrameScaffold(
        strings: AppStrings(view.locale),
        phase: view.phase,
        child: child,
      ),
    );
  }
}

class _LoadingScreen extends StatelessWidget {
  const _LoadingScreen();

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      body: Center(
        child: Semantics(
          label: 'Loading Usque',
          child: const SizedBox(
            width: 34,
            height: 34,
            child: CircularProgressIndicator(strokeWidth: 3),
          ),
        ),
      ),
    );
  }
}
