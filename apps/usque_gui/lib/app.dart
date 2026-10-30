import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter_localizations/flutter_localizations.dart';

import 'core/app_strings.dart';
import 'core/usque_theme.dart';
import 'models/app_models.dart';
import 'screens/onboarding_screen.dart';
import 'screens/shell_screen.dart';
import 'services/engine_client.dart';
import 'services/engine_client_factory.dart';
import 'services/gui_startup_observation.dart';
import 'services/platform_shell_bridge.dart';
import 'services/update_downloader.dart';
import 'state/app_controller.dart';
import 'state/window_frame.dart';
import 'widgets/controller_selector.dart';
import 'widgets/window_titlebar.dart';

typedef _BootstrapView = ({
  bool initialized,
  bool onboardingComplete,
  ThemePreference theme,
  LocalePreference locale,
  String catalogId,
  bool onboardingBusy,
  String? onboardingError,
  String? startupError,
  bool uiVisible,
});

class UsqueBootstrap extends StatefulWidget {
  const UsqueBootstrap({
    super.key,
    this.engine,
    this.updateDownloader,
    this.builder,
  });

  final EngineClient? engine;
  final UpdateDownloader? updateDownloader;
  final TransitionBuilder? builder;

  @override
  State<UsqueBootstrap> createState() => _UsqueBootstrapState();
}

class _UsqueBootstrapState extends State<UsqueBootstrap>
    with WidgetsBindingObserver {
  late final AppController controller;
  late final PlatformShellBridge shellBridge;

  @override
  void initState() {
    super.initState();
    controller = AppController(
      widget.engine ?? createDefaultEngineClient(),
      updateDownloader: widget.updateDownloader,
    );
    controller.noteSystemLocale(
      WidgetsBinding.instance.platformDispatcher.locale,
    );
    if (defaultTargetPlatform == TargetPlatform.windows ||
        defaultTargetPlatform == TargetPlatform.macOS) {
      // The native visibility query also covers an initially hidden --background
      // launch, before Flutter has received any window lifecycle transition.
      controller.setUiVisible(false);
    }
    WidgetsBinding.instance.addObserver(this);
    final lifecycle = WidgetsBinding.instance.lifecycleState;
    if (lifecycle != null) didChangeAppLifecycleState(lifecycle);
    shellBridge = PlatformShellBridge(controller);
    if (defaultTargetPlatform == TargetPlatform.linux ||
        defaultTargetPlatform == TargetPlatform.macOS) {
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (!mounted) return;
        GuiStartupObservation.instance.mark(2);
        controller.initialize();
      });
    } else {
      controller.initialize();
    }
  }

  @override
  void didChangeLocales(List<Locale>? locales) {
    controller.noteSystemLocale(
      locales?.firstOrNull ?? WidgetsBinding.instance.platformDispatcher.locale,
    );
  }

  @override
  void didChangeAppLifecycleState(AppLifecycleState state) {
    // Desktop tray sessions keep observing while their window is hidden.
    if (defaultTargetPlatform != TargetPlatform.android) return;
    switch (state) {
      case AppLifecycleState.hidden:
      case AppLifecycleState.paused:
      case AppLifecycleState.detached:
        controller.setObservationVisible(false);
      case AppLifecycleState.resumed:
        controller.setObservationVisible(true);
      case AppLifecycleState.inactive:
        // Focus loss (including permission dialogs) can leave the UI visible.
        break;
    }
  }

  @override
  void dispose() {
    WidgetsBinding.instance.removeObserver(this);
    shellBridge.dispose();
    controller.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return ControllerSelector<_BootstrapView>(
      controller: controller,
      observeWhenHidden: true,
      selector: (controller) => (
        initialized: controller.initialized,
        startupError: controller.startupError,
        onboardingComplete: controller.onboardingComplete,
        theme: controller.themePreference,
        locale: controller.localePreference,
        catalogId: controller.strings.catalogId,
        onboardingBusy: controller.onboardingComplete ? false : controller.busy,
        onboardingError: controller.onboardingComplete
            ? null
            : controller.lastError,
        uiVisible: controller.uiVisible,
      ),
      builder: (context, view) {
        final themeMode = switch (view.theme) {
          ThemePreference.system => ThemeMode.system,
          ThemePreference.light => ThemeMode.light,
          ThemePreference.dark => ThemeMode.dark,
        };
        final locale = view.locale == LocalePreference.system
            ? null
            : AppStrings(view.locale).locale;
        return MaterialApp(
          title: 'Usque',
          debugShowCheckedModeBanner: false,
          theme: UsqueTheme.light(),
          darkTheme: UsqueTheme.dark(),
          themeMode: themeMode,
          locale: locale,
          localeListResolutionCallback: (locales, _) => AppStrings(
            view.locale,
            systemLocale: locales?.firstOrNull,
          ).locale,
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
          builder: (context, child) {
            final content = WindowFrame.instance.enabled
                ? _WindowChrome(
                    controller: controller,
                    child: child ?? const SizedBox.shrink(),
                  )
                : child ?? const SizedBox.shrink();
            return TickerMode(
              enabled: view.uiVisible,
              child: widget.builder?.call(context, content) ?? content,
            );
          },
          home: !view.initialized
              ? _LoadingScreen(controller: controller, error: view.startupError)
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
        strings: controller.strings,
        phase: view.phase,
        child: child,
      ),
    );
  }
}

class _LoadingScreen extends StatelessWidget {
  const _LoadingScreen({required this.controller, this.error});
  final AppController controller;
  final String? error;

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      body: Center(
        child: error != null
            ? Padding(
                padding: const EdgeInsets.all(24),
                child: Column(
                  mainAxisSize: MainAxisSize.min,
                  children: [
                    Text(error!, textAlign: TextAlign.center),
                    const SizedBox(height: 16),
                    FilledButton(
                      onPressed: controller.initialize,
                      child: Text(controller.strings.get('retry')),
                    ),
                  ],
                ),
              )
            : Semantics(
                label: controller.strings.get('app_loading'),
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
