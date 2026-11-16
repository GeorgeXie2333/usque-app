import 'dart:async';
import 'dart:ffi' show Abi;
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:shared_preferences/shared_preferences.dart';
import 'package:usque/core/usque_theme.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/screens/onboarding_screen.dart';
import 'package:usque/services/desktop_engine_transport.dart';
import 'package:usque/services/engine_client.dart';
import 'package:usque/state/app_controller.dart';
import 'package:usque/widgets/local_proxy_outputs.dart';
import 'package:usque/widgets/macos_service_section.dart';
import 'package:usque/widgets/usque_logo.dart';

import 'app_test.dart' show FakeEngineClient;
import 'ui_workflow_test.dart' show workflowHost;

class ServiceEngine extends FakeEngineClient implements MacosServiceClient {
  MacosServiceState state = const MacosServiceState(canMaintain: true);
  final actions = <MacosServiceAction>[];
  bool cancelFirst = false;
  bool cleanupFails = false;
  bool relaunchFails = false;
  Completer<void>? authorization;
  Completer<void>? statusGate;
  int statusReads = 0;

  @override
  Future<void> relaunchMacosAppAfterCleanup() async {
    calls.add('relaunch');
    if (relaunchFails) throw StateError('simulated relaunch failure');
  }

  @override
  Future<EngineSnapshot> disconnect() async {
    final result = await super.disconnect();
    if (cleanupFails) {
      throw const EngineException('DISCONNECT_CLEANUP_FAILED', 'fixture');
    }
    return result;
  }

  @override
  Future<MacosServiceState> getMacosServiceState() async {
    statusReads++;
    await statusGate?.future;
    return state;
  }

  @override
  Future<void> maintainMacosService(MacosServiceAction action) async {
    actions.add(action);
    calls.add('service:${action.name}');
    await authorization?.future;
    if (cancelFirst && actions.length == 1) {
      throw const EngineException(
        'MACOS_AGENT_MAINTENANCE_FAILED',
        'cancelled',
      );
    }
    state = const MacosServiceState(
      installed: true,
      compatible: true,
      canMaintain: true,
      version: '0.3.4',
    );
  }
}

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  testWidgets(
    'first visit to macOS permissions requests installation once and preserves a retry after reopening',
    (tester) async {
      SharedPreferences.setMockInitialValues({});
      final engine = ServiceEngine()..cancelFirst = true;
      final app = AppController(engine)
        ..localePreference = LocalePreference.english;
      addTearDown(app.dispose);
      await tester.pumpWidget(
        workflowHost(app, home: OnboardingScreen(controller: app)),
      );
      await tester.pumpAndSettle();
      expect(engine.actions, isEmpty);
      await tester.tap(
        find.widgetWithText(FilledButton, app.strings.get('continue')),
      );
      await tester.pumpAndSettle();
      expect(app.onboardingStep, 1);
      expect(engine.actions, [MacosServiceAction.install]);
      expect(
        find.text(app.strings.get('macos_service_failed')),
        findsOneWidget,
      );
      await tester.tap(
        find.widgetWithText(FilledButton, app.strings.get('continue')),
      );
      await tester.pumpAndSettle();
      expect(app.onboardingStep, 2);
      await tester.tap(
        find.widgetWithText(OutlinedButton, app.strings.get('back')),
      );
      await tester.pumpAndSettle();
      await tester.pumpWidget(const SizedBox.shrink());
      final reopened = AppController(engine)
        ..localePreference = LocalePreference.english
        ..onboardingStep = (await SharedPreferences.getInstance()).getInt(
          'onboarding_step',
        )!;
      addTearDown(reopened.dispose);
      await tester.pumpWidget(
        workflowHost(reopened, home: OnboardingScreen(controller: reopened)),
      );
      await tester.pumpAndSettle();
      expect(engine.actions, [MacosServiceAction.install]);
      await tester.tap(find.byKey(const ValueKey('macos-service-install')));
      await tester.pump();
      expect(engine.calls.where((call) => call == 'relaunch'), hasLength(1));
      await tester.pumpWidget(const SizedBox.shrink());
      final resumed = AppController(engine)
        ..onboardingStep = reopened.onboardingStep;
      addTearDown(resumed.dispose);
      await tester.pumpWidget(
        workflowHost(resumed, home: OnboardingScreen(controller: resumed)),
      );
      await tester.pumpAndSettle();
      expect(
        find.text(resumed.strings.get('macos_service_installed')),
        findsOneWidget,
      );
      expect(engine.actions, [
        MacosServiceAction.install,
        MacosServiceAction.install,
      ]);
    },
    variant: TargetPlatformVariant.only(TargetPlatform.macOS),
  );

  testWidgets(
    'restored permissions step probes an installed service without a new prompt',
    (tester) async {
      SharedPreferences.setMockInitialValues({});
      final engine = ServiceEngine()
        ..state = const MacosServiceState(
          installed: true,
          compatible: true,
          canMaintain: true,
        );
      final app = AppController(engine)..onboardingStep = 1;
      addTearDown(app.dispose);
      await tester.pumpWidget(
        workflowHost(app, home: OnboardingScreen(controller: app)),
      );
      await tester.pumpAndSettle();
      expect(engine.statusReads, 1);
      expect(engine.actions, isEmpty);
      expect(find.byKey(const ValueKey('macos-service-more')), findsNothing);
      expect(find.text(app.strings.get('permission_note')), findsNothing);
    },
    variant: TargetPlatformVariant.only(TargetPlatform.macOS),
  );

  testWidgets(
    'a late service probe does not prompt over the terms page or consume the first visit',
    (tester) async {
      SharedPreferences.setMockInitialValues({});
      final engine = ServiceEngine()
        ..statusGate = Completer<void>()
        ..cancelFirst = true;
      final app = AppController(engine)..onboardingStep = 1;
      addTearDown(app.dispose);
      await tester.pumpWidget(
        workflowHost(app, home: OnboardingScreen(controller: app)),
      );
      await tester.pump();
      expect(engine.statusReads, 1);
      await tester.tap(
        find.widgetWithText(FilledButton, app.strings.get('continue')),
      );
      engine.statusGate!.complete();
      await tester.pumpAndSettle();
      expect(app.onboardingStep, 2);
      expect(engine.actions, isEmpty);
      await tester.tap(
        find.widgetWithText(OutlinedButton, app.strings.get('back')),
      );
      await tester.pumpAndSettle();
      expect(engine.actions, [MacosServiceAction.install]);
    },
    variant: TargetPlatformVariant.only(TargetPlatform.macOS),
  );

  testWidgets(
    'macOS setup permission actions fit narrow translated and right-to-left layouts',
    (tester) async {
      SharedPreferences.setMockInitialValues({
        'onboarding_macos_service_prompted': true,
      });
      tester.view.devicePixelRatio = 1;
      tester.view.physicalSize = const Size(375, 780);
      addTearDown(tester.view.resetDevicePixelRatio);
      addTearDown(tester.view.resetPhysicalSize);
      for (final locale in LocalePreference.values.where(
        (value) => value != LocalePreference.system,
      )) {
        final app = AppController(ServiceEngine())
          ..onboardingStep = 1
          ..localePreference = locale;
        addTearDown(app.dispose);
        await tester.pumpWidget(
          workflowHost(
            app,
            scale: 1.3,
            home: OnboardingScreen(controller: app),
          ),
        );
        await tester.pumpAndSettle();
        expect(
          find.text(app.strings.get('macos_service_install')),
          findsOneWidget,
          reason: locale.name,
        );
        expect(tester.takeException(), isNull, reason: locale.name);
        await tester.pumpWidget(const SizedBox.shrink());
      }
    },
    variant: TargetPlatformVariant.only(TargetPlatform.macOS),
  );
  test(
    'native relaunch blocks a second opener and Engine restart until a failed handoff returns',
    () async {
      final transport = DesktopEngineTransport.forTest(
        exchange: (_) => throw UnimplementedError(),
      );
      addTearDown(transport.dispose);
      const channel = MethodChannel('io.github.georgexie2333.usque/engine');
      final messenger =
          TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger;
      final called = Completer<void>();
      final finish = Completer<void>();
      messenger.setMockMethodCallHandler(channel, (call) async {
        expect(call.method, 'relaunchMacosApp');
        expect(call.arguments, {'cleanup_complete': true});
        called.complete();
        await finish.future;
        throw PlatformException(code: 'MACOS_SERVICE_RELAUNCH_FAILED');
      });
      addTearDown(() => messenger.setMockMethodCallHandler(channel, null));
      final failed = expectLater(
        transport.relaunchMacosAppAfterCleanup(),
        throwsA(isA<PlatformException>()),
      );
      await called.future;
      await expectLater(
        transport.ensureStarted(),
        throwsA(isA<EngineException>()),
      );
      await expectLater(
        transport.relaunchMacosAppAfterCleanup(),
        throwsA(isA<EngineException>()),
      );
      expect(transport.startCount, 0);
      finish.complete();
      await failed;
      await transport.ensureStarted();
      expect(transport.startCount, 1);
    },
  );

  testWidgets(
    'leaving settings during authorization preserves a single install and relaunch',
    (tester) async {
      final engine = ServiceEngine()..authorization = Completer<void>();
      final app = AppController(engine)
        ..localePreference = LocalePreference.simplifiedChinese;
      addTearDown(app.dispose);
      await tester.pumpWidget(
        workflowHost(
          app,
          home: Scaffold(body: MacosServiceSection(controller: app)),
        ),
      );
      await tester.pumpAndSettle();
      await tester.tap(find.text('授权并安装'));
      await tester.pump();
      await expectLater(
        app.maintainMacosService(MacosServiceAction.install),
        throwsA(isA<EngineException>()),
      );
      expect(app.macosServiceBusy, isTrue);
      await tester.pumpWidget(const SizedBox.shrink());
      engine.authorization!.complete();
      await tester.pumpAndSettle();
      expect(engine.calls, ['service:install', 'disconnect', 'relaunch']);
      expect(app.macosServiceBusy, isFalse);
    },
    variant: TargetPlatformVariant.only(TargetPlatform.macOS),
  );
  testWidgets(
    'only successful first install confirms cleanup then relaunches once',
    (tester) async {
      final engine = ServiceEngine()..cancelFirst = true;
      final app = AppController(engine);
      addTearDown(app.dispose);
      await expectLater(
        app.maintainMacosService(MacosServiceAction.install),
        throwsA(isA<EngineException>()),
      );
      expect(engine.calls, ['service:install']);
      await app.maintainMacosService(MacosServiceAction.install);
      expect(engine.calls, [
        'service:install',
        'service:install',
        'disconnect',
        'relaunch',
      ]);
      for (final action in [
        MacosServiceAction.repair,
        MacosServiceAction.recover,
        MacosServiceAction.uninstall,
      ]) {
        await app.maintainMacosService(action);
      }
      expect(engine.calls.where((c) => c == 'relaunch'), hasLength(1));
    },
    variant: TargetPlatformVariant.only(TargetPlatform.macOS),
  );

  testWidgets(
    'unconfirmed cleanup keeps the installed service and skips relaunch',
    (tester) async {
      final engine = ServiceEngine()..cleanupFails = true;
      final app = AppController(engine);
      addTearDown(app.dispose);
      await expectLater(
        app.maintainMacosService(MacosServiceAction.install),
        throwsA(isA<EngineException>()),
      );
      expect(engine.state.installed, isTrue);
      expect(engine.calls, contains('disconnect'));
      expect(engine.calls, isNot(contains('relaunch')));
    },
    variant: TargetPlatformVariant.only(TargetPlatform.macOS),
  );

  testWidgets(
    'refresh after external recovery clears stale errors only with clean proof',
    (tester) async {
      final engine = ServiceEngine()
        ..state = const MacosServiceState(
          installed: true,
          compatible: true,
          canMaintain: true,
          recoveryPending: true,
        )
        ..cancelFirst = true;
      final app = AppController(engine)
        ..localePreference = LocalePreference.english;
      addTearDown(app.dispose);
      final oldError = app.strings.get('macos_agent_quit_failed');
      app.lastError = oldError;
      await tester.pumpWidget(
        workflowHost(
          app,
          home: Scaffold(body: MacosServiceSection(controller: app)),
        ),
      );
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(const ValueKey('macos-service-install')));
      await tester.pumpAndSettle();
      final oldAction = find.text(
        'The operation did not complete. You can retry.',
      );
      expect(oldAction, findsOneWidget);
      final ready = <Object?, Object?>{
        'installed': true,
        'compatible': true,
        'canMaintain': true,
        'tunAvailable': true,
      };
      engine.state = MacosServiceState.fromMap(ready);
      await tester.tap(
        find.byTooltip(app.strings.get('macos_service_refresh')),
      );
      await tester.pumpAndSettle();
      expect(app.lastError, oldError);
      expect(oldAction, findsNothing);
      engine.state = MacosServiceState.fromMap({
        ...ready,
        'cleanupConfirmed': true,
      });
      await tester.tap(
        find.byTooltip(app.strings.get('macos_service_refresh')),
      );
      await tester.pumpAndSettle();
      expect(app.lastError, isNull);
      app.lastError = 'unrelated error';
      await tester.tap(
        find.byTooltip(app.strings.get('macos_service_refresh')),
      );
      await tester.pumpAndSettle();
      expect(app.lastError, 'unrelated error');
      expect(engine.actions, [MacosServiceAction.repair]);
    },
    variant: TargetPlatformVariant.only(TargetPlatform.macOS),
  );
  testWidgets(
    'cancelled authorization stays retryable and does not claim installation',
    (tester) async {
      final engine = ServiceEngine()..cancelFirst = true;
      final app = AppController(engine)
        ..localePreference = LocalePreference.english;
      addTearDown(app.dispose);
      await tester.pumpWidget(
        workflowHost(
          app,
          home: Scaffold(body: MacosServiceSection(controller: app)),
        ),
      );
      await tester.pumpAndSettle();
      expect(engine.actions, isEmpty);
      final install = find.byKey(const ValueKey('macos-service-install'));
      await tester.tap(install);
      await tester.pumpAndSettle();
      expect(
        find.text(app.strings.get('macos_service_missing')),
        findsOneWidget,
      );
      expect(
        find.text('The operation did not complete. You can retry.'),
        findsOneWidget,
      );
      expect(tester.widget<FilledButton>(install).onPressed, isNotNull);
      await tester.tap(install);
      await tester.pump();
      expect(engine.actions, [
        MacosServiceAction.install,
        MacosServiceAction.install,
      ]);
      expect(engine.calls.last, 'relaunch');
      expect(engine.statusReads, 2); // Initial load and the cancelled attempt.
      expect(tester.widget<FilledButton>(install).onPressed, isNull);
    },
    variant: TargetPlatformVariant.only(TargetPlatform.macOS),
  );

  testWidgets(
    'active connection disables service mutations',
    (tester) async {
      final engine = ServiceEngine()
        ..state = const MacosServiceState(
          installed: true,
          compatible: true,
          canMaintain: true,
          recoveryPending: true,
          protectionRetained: true,
        );
      final app = AppController(engine)
        ..snapshot = const EngineSnapshot(phase: ConnectionPhase.connected);
      addTearDown(app.dispose);
      await tester.pumpWidget(
        workflowHost(
          app,
          home: Scaffold(body: MacosServiceSection(controller: app)),
        ),
      );
      await tester.pumpAndSettle();
      expect(
        tester
            .widget<OutlinedButton>(
              find.byKey(const ValueKey('macos-service-install')),
            )
            .onPressed,
        isNull,
      );
      expect(
        tester
            .widget<FilledButton>(
              find.byKey(const ValueKey('macos-service-recover')),
            )
            .onPressed,
        isNull,
      );
      expect(
        tester
            .widget<PopupMenuButton<MacosServiceAction>>(
              find.byKey(const ValueKey('macos-service-more')),
            )
            .enabled,
        isFalse,
      );
      expect(engine.actions, isEmpty);
    },
    variant: TargetPlatformVariant.only(TargetPlatform.macOS),
  );

  testWidgets(
    'TUN test package shows the actual preflight result before production acceptance',
    (tester) async {
      final engine = ServiceEngine()
        ..state = MacosServiceState.fromMap({
          'installed': true,
          'compatible': true,
          'testTunBuild': true,
        });
      final app = AppController(engine)
        ..localePreference = LocalePreference.english;
      addTearDown(app.dispose);
      await tester.pumpWidget(
        workflowHost(
          app,
          home: Scaffold(body: MacosServiceSection(controller: app)),
        ),
      );
      await tester.pumpAndSettle();
      expect(
        find.text(app.strings.get('macos_service_preflight')),
        findsOneWidget,
      );
      expect(find.text(app.strings.get('macos_service_gated')), findsNothing);
    },
    variant: TargetPlatformVariant.only(TargetPlatform.macOS),
  );

  testWidgets(
    'install shows restart in advance and separates a relaunch failure from authorization',
    (tester) async {
      final engine = ServiceEngine()..relaunchFails = true;
      final app = AppController(engine)
        ..localePreference = LocalePreference.english;
      addTearDown(app.dispose);
      await tester.pumpWidget(
        workflowHost(
          app,
          home: Scaffold(body: MacosServiceSection(controller: app)),
        ),
      );
      await tester.pumpAndSettle();
      expect(
        find.text(app.strings.get('macos_service_install_hint')),
        findsOneWidget,
      );
      expect(find.byKey(const ValueKey('macos-service-more')), findsNothing);
      await tester.tap(find.byKey(const ValueKey('macos-service-install')));
      await tester.pumpAndSettle();
      expect(
        find.text(app.strings.get('macos_service_installed')),
        findsOneWidget,
      );
      expect(
        find.text(app.strings.get('macos_service_relaunch_failed')),
        findsOneWidget,
      );
      expect(find.text(app.strings.get('macos_service_failed')), findsNothing);
      expect(engine.actions, [MacosServiceAction.install]);
    },
    variant: TargetPlatformVariant.only(TargetPlatform.macOS),
  );

  testWidgets(
    'healthy service offers secondary actions in its menu and fits a narrow window',
    (tester) async {
      tester.view.devicePixelRatio = 1;
      tester.view.physicalSize = const Size(320, 720);
      addTearDown(tester.view.resetPhysicalSize);
      addTearDown(tester.view.resetDevicePixelRatio);
      final engine = ServiceEngine()
        ..state = const MacosServiceState(
          installed: true,
          compatible: true,
          canMaintain: true,
          tunAvailable: true,
          version: '0.3.4',
        );
      final app = AppController(engine)
        ..localePreference = LocalePreference.english;
      addTearDown(app.dispose);
      await tester.pumpWidget(
        workflowHost(
          app,
          home: Scaffold(
            body: MediaQuery(
              data: const MediaQueryData(textScaler: TextScaler.linear(1.6)),
              child: SingleChildScrollView(
                child: MacosServiceSection(controller: app),
              ),
            ),
          ),
        ),
      );
      await tester.pumpAndSettle();
      expect(find.text('Version 0.3.4'), findsOneWidget);
      expect(find.text(app.strings.get('macos_service_restore')), findsNothing);
      final more = find.byKey(const ValueKey('macos-service-more'));
      await tester.tap(more);
      await tester.pumpAndSettle();
      expect(
        find.text(app.strings.get('macos_service_uninstall')),
        findsOneWidget,
      );
      await tester.tap(find.text(app.strings.get('macos_service_restore')));
      await tester.pumpAndSettle();
      expect(engine.actions, [MacosServiceAction.recover]);
      expect(engine.calls, isNot(contains('relaunch')));
      expect(tester.takeException(), isNull);
    },
    variant: TargetPlatformVariant.only(TargetPlatform.macOS),
  );

  for (final fixture in <String, MacosServiceState>{
    'service_missing': const MacosServiceState(canMaintain: true),
    'service_gated': const MacosServiceState(
      installed: true,
      compatible: true,
      canMaintain: true,
      version: '0.3.4',
    ),
    'service_incompatible': const MacosServiceState(
      installed: true,
      canMaintain: true,
      recoveryPending: true,
      version: '0.3.3',
    ),
    'protection_retained': const MacosServiceState(
      installed: true,
      compatible: true,
      canMaintain: true,
      protectionRetained: true,
      recoveryPending: true,
      version: '0.3.4',
    ),
    'tun_ready': const MacosServiceState(
      installed: true,
      compatible: true,
      canMaintain: true,
      preflightPassed: true,
      releaseApproved: true,
      tunAvailable: true,
      version: '0.3.4',
    ),
    'service_missing_zh': const MacosServiceState(canMaintain: true),
    'onboarding_permissions_zh': const MacosServiceState(canMaintain: true),
  }.entries) {
    testWidgets(
      'macOS exact golden ${fixture.key}',
      (tester) async {
        final onboarding = fixture.key == 'onboarding_permissions_zh';
        final chinese = fixture.key.endsWith('_zh');
        tester.view.devicePixelRatio = 1;
        tester.view.physicalSize = chinese && !onboarding
            ? const Size(640, 210)
            : const Size(900, 720);
        addTearDown(tester.view.resetDevicePixelRatio);
        addTearDown(tester.view.resetPhysicalSize);
        await (FontLoader(
          'MaterialIcons',
        )..addFont(rootBundle.load('fonts/MaterialIcons-Regular.otf'))).load();
        await (FontLoader('packages/lucide_icons_flutter/Lucide')..addFont(
              rootBundle.load(
                'packages/lucide_icons_flutter/assets/lucide.ttf',
              ),
            ))
            .load();
        for (final family in <String, List<String>>{
          'Manrope': ['Regular', 'Medium', 'SemiBold', 'Bold'],
          'SpaceGrotesk': ['Medium', 'SemiBold', 'Bold'],
        }.entries) {
          final loader = FontLoader(family.key);
          for (final weight in family.value) {
            loader.addFont(
              rootBundle.load('assets/fonts/${family.key}-$weight.ttf'),
            );
          }
          await loader.load();
        }
        if (chinese) {
          // Use the repository's Chinese test face so the golden doesn't
          // depend on which PingFang collection a macOS runner has installed.
          final emptyFace = ByteData.sublistView(
            File('test/fonts/MissingFamily.ttf').readAsBytesSync(),
          );
          for (final family in UsqueFonts.fallback.takeWhile(
            (name) => name != 'Noto Sans CJK SC',
          )) {
            await (FontLoader(family)..addFont(Future.value(emptyFace))).load();
          }
          final loader = FontLoader('Noto Sans CJK SC');
          for (final weight in ['Regular', 'Bold']) {
            final face = File('test/fonts/NotoSansCJKsc-$weight.subset.otf');
            loader.addFont(
              Future.value(ByteData.sublistView(face.readAsBytesSync())),
            );
          }
          await loader.load();
        }
        final engine = ServiceEngine()..state = fixture.value;
        final app = AppController(engine)
          ..localePreference = chinese
              ? LocalePreference.simplifiedChinese
              : LocalePreference.english
          ..engineCapabilities = EngineCapabilities(
            vpn: fixture.value.tunAvailable,
            systemProxy: true,
          );
        if (onboarding) {
          SharedPreferences.setMockInitialValues({});
          app.onboardingStep = 1;
          engine.cancelFirst = true;
        }
        addTearDown(app.dispose);
        await tester.pumpWidget(
          workflowHost(
            app,
            home: RepaintBoundary(
              key: const ValueKey('mac-capture'),
              child: onboarding
                  ? OnboardingScreen(controller: app)
                  : Scaffold(
                      body: Padding(
                        padding: const EdgeInsets.all(24),
                        child: Column(
                          children: [
                            MacosServiceSection(controller: app),
                            if (!chinese) ...[
                              const SizedBox(height: 24),
                              LocalProxyOutputs(controller: app, enabled: true),
                            ],
                          ],
                        ),
                      ),
                    ),
            ),
          ),
        );
        if (onboarding) {
          await tester.runAsync(
            () => precacheImage(
              const AssetImage(UsqueLogo.lightAsset),
              tester.element(find.byType(MaterialApp)),
            ),
          );
        }
        await tester.pumpAndSettle();
        expect(tester.takeException(), isNull);
        final arch = Abi.current() == Abi.macosArm64 ? 'arm64' : 'x86_64';
        await expectLater(
          find.byKey(const ValueKey('mac-capture')),
          matchesGoldenFile('goldens/macos/$arch/${fixture.key}.png'),
        );
      },
      skip: !Platform.isMacOS,
      variant: TargetPlatformVariant.only(TargetPlatform.macOS),
      tags: ['golden', 'macos-golden'],
    );
  }
}
