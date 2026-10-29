import 'dart:async';
import 'dart:io';
import 'dart:ui' as ui;

import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter/rendering.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:shared_preferences/shared_preferences.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/screens/shell_screen.dart';
import 'package:usque/services/engine_client.dart';
import 'package:usque/services/platform_shell_bridge.dart';
import 'package:usque/state/app_controller.dart';
import 'package:usque/state/window_frame.dart';
import 'package:usque/widgets/local_proxy_outputs.dart';
import 'package:usque/widgets/window_titlebar.dart';
import 'package:usque/widgets/zero_trust_enrollment_editor.dart';

import 'ui_workflow_test.dart' show WorkflowEngine, workflowHost;

// Platform widget simulations only: there are no engine sockets, native
// windows, browser sessions, permission prompts or system-network mutations.
class _DesktopQaEngine extends WorkflowEngine {
  _DesktopQaEngine({this.vpn = false, this.systemProxy = true});

  final bool vpn;
  final bool systemProxy;
  int browserBegins = 0;
  Object? callbackFailure;
  Object? startupFailure;
  Object? closePreferenceFailure;
  final startupChanges = <bool>[];
  final closePreferenceChanges = <bool>[];

  @override
  Future<EngineCapabilities?> getCapabilities() async => EngineCapabilities(
    vpn: vpn,
    systemProxy: systemProxy,
    networkSettingsApplication: true,
    automaticEndpoints: true,
    tlsFingerprints: TlsFingerprint.values,
  );

  @override
  Future<String?> beginZeroTrustLogin(String teamName) async {
    browserBegins++;
    return 'https://$teamName.cloudflareaccess.com/warp';
  }

  @override
  Future<String?> consumeZeroTrustCallback() async {
    if (callbackFailure case final failure?) {
      callbackFailure = null;
      throw failure;
    }
    return super.consumeZeroTrustCallback();
  }

  @override
  Future<void> setStartOnBoot(bool enabled) async {
    startupChanges.add(enabled);
    if (startupFailure case final failure?) throw failure;
  }

  @override
  Future<void> setCloseToTray(bool enabled) async {
    closePreferenceChanges.add(enabled);
    if (closePreferenceFailure case final failure?) throw failure;
  }
}

class _CapabilityQaEngine extends _DesktopQaEngine {
  final _events = StreamController<EngineSnapshotEvent>();

  @override
  bool get supportsSnapshotEvents => true;

  @override
  Stream<EngineSnapshotEvent> get snapshotEvents => _events.stream;

  void emitCapabilities(bool vpn) => _events.add(
    EngineSnapshotEvent(
      capabilities: EngineCapabilities(
        vpn: vpn,
        systemProxy: true,
        networkSettingsApplication: true,
        automaticEndpoints: true,
        tlsFingerprints: TlsFingerprint.values,
      ),
    ),
  );

  @override
  void dispose() {
    unawaited(_events.close());
    super.dispose();
  }
}

Future<void> _loadFonts() async {
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
}

Future<AppController> _controller(
  WidgetTester tester,
  _DesktopQaEngine engine,
  Size size, {
  bool outputsEnabled = false,
}) async {
  tester.view.devicePixelRatio = 1;
  tester.view.physicalSize = size;
  addTearDown(tester.view.resetDevicePixelRatio);
  addTearDown(tester.view.resetPhysicalSize);
  SharedPreferences.setMockInitialValues({
    'update_checks_enabled': false,
    'onboarding_complete': true,
  });
  final profile = UsqueProfile.defaultProfile();
  engine
    ..legacyProfilesImported = true
    ..storedProfiles = [
      profile.copyWith(
        frontends: profile.frontends.copyWith(tunnel: outputsEnabled),
        proxy: profile.proxy.copyWith(systemProxy: outputsEnabled),
      ),
    ];
  final app = AppController(engine);
  addTearDown(app.dispose);
  await app.initialize();
  app.localePreference = LocalePreference.english;
  return app;
}

Future<void> _saveSimulation(WidgetTester tester, String scene) async {
  final destination = Platform.environment['USQUE_MACOS_QA_ARTIFACTS'];
  if (destination == null) return;
  final boundary = tester.renderObject<RenderRepaintBoundary>(
    find.byKey(const ValueKey('desktop-qa-capture')),
  );
  await tester.runAsync(() async {
    final image = await boundary.toImage(pixelRatio: 1);
    try {
      final png = await image.toByteData(format: ui.ImageByteFormat.png);
      final output = Directory(destination)..createSync(recursive: true);
      await File('${output.path}/$scene.png').writeAsBytes(
        png!.buffer.asUint8List(png.offsetInBytes, png.lengthInBytes),
      );
      await File('${output.path}/RENDERING.txt').writeAsString(
        'Simulated Flutter platform widgets with a fake Engine.\n'
        'No app, browser, CEF, native window, permission or real network ran.\n'
        'Both client areas are 1040 x 760 logical pixels at 100% text.\n'
        'Windows includes its actual shared Flutter caption; macOS native\n'
        'title bar is outside the client and is not rendered by this test.\n'
        'English uses the repository font assets. This is not device QA.\n',
      );
    } finally {
      image.dispose();
    }
  });
}

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  setUpAll(_loadFonts);
  tearDown(WindowFrame.instance.debugReset);

  for (final size in [
    const Size(1040, 760),
    const Size(800, 600),
    const Size(640, 480),
  ]) {
    for (final scale in [1.0, 2.0]) {
      testWidgets(
        'macOS sections fit ${size.width}x${size.height} at ${scale * 100}% text',
        (tester) async {
          final app = await _controller(tester, _DesktopQaEngine(), size);
          await tester.pumpWidget(workflowHost(app, scale: scale));
          await tester.pumpAndSettle();
          for (final section in AppSection.values) {
            app.selectSection(section);
            await tester.pumpAndSettle();
            expect(tester.takeException(), isNull, reason: section.name);
            expect(find.byType(ShellScreen), findsOneWidget);
            expect(find.byType(WindowTitleBar), findsNothing);
            expect(
              find.byType(size.width >= 760 ? NavigationRail : NavigationBar),
              findsOneWidget,
              reason: '${section.name} navigation must remain reachable',
            );
          }
        },
        variant: TargetPlatformVariant.only(TargetPlatform.macOS),
      );
    }
  }

  testWidgets(
    'macOS unavailable VPN stays disabled while system proxy remains available',
    (tester) async {
      final engine = _DesktopQaEngine();
      final app = await _controller(tester, engine, const Size(1040, 760));
      app.section = AppSection.home;
      await tester.pumpWidget(workflowHost(app));
      await tester.pumpAndSettle();
      final tunnel = find.byKey(const ValueKey('home-tun-switch'));
      final proxy = find.byKey(const ValueKey('home-system-proxy-switch'));
      expect(tester.widget<Switch>(tunnel).value, isFalse);
      expect(tester.widget<Switch>(tunnel).onChanged, isNull);
      expect(tester.widget<Switch>(proxy).onChanged, isNotNull);
      await tester.ensureVisible(tunnel);
      await tester.tap(tunnel);
      await tester.pumpAndSettle();
      expect(engine.writes, 0);
      expect(engine.calls, isNot(contains('connect')));
      expect(tester.takeException(), isNull);
    },
    variant: TargetPlatformVariant.only(TargetPlatform.macOS),
  );

  testWidgets(
    'macOS Settings protection hint follows capability events without changing preferences',
    (tester) async {
      final engine = _CapabilityQaEngine();
      final app = await _controller(tester, engine, const Size(1040, 760));
      app.section = AppSection.settings;
      await tester.pumpWidget(workflowHost(app));
      await tester.pumpAndSettle();
      final preference = app.activeProfile.killSwitch;
      final row = find.byKey(const ValueKey('settings-kill-switch-row'));
      final unavailable = app.strings.get('macos_kill_switch_unavailable');
      final normal = app.strings.get('kill_switch_help');
      expect(unavailable, isNot('macos_kill_switch_unavailable'));
      expect(find.text(unavailable), findsOneWidget);
      expect(find.text(normal), findsNothing);
      engine.emitCapabilities(true);
      await tester.pumpAndSettle();
      expect(find.text(unavailable), findsNothing);
      expect(find.text(normal), findsOneWidget);
      engine.emitCapabilities(false);
      await tester.pumpAndSettle();
      expect(find.text(unavailable), findsOneWidget);
      expect(find.text(normal), findsNothing);
      expect(tester.widget<SwitchListTile>(row).value, preference);
      expect(app.activeProfile.killSwitch, preference);
      expect(engine.writes, 0);
      await tester.tap(row);
      await tester.pumpAndSettle();
      expect(app.activeProfile.killSwitch, !preference);
      expect(engine.writes, 1);
      expect(engine.calls, isNot(contains('connect')));
      expect(tester.takeException(), isNull);
    },
    variant: TargetPlatformVariant.only(TargetPlatform.macOS),
  );

  testWidgets(
    'macOS Settings reflects successful native preference edits, failures and tray changes',
    (tester) async {
      final engine = _DesktopQaEngine();
      final app = await _controller(tester, engine, const Size(1040, 760));
      app.section = AppSection.settings;
      await tester.pumpWidget(workflowHost(app));
      await tester.pumpAndSettle();
      final startup = find.widgetWithText(
        SwitchListTile,
        app.strings.get('start_on_boot'),
      );
      final close = find.widgetWithText(
        SwitchListTile,
        app.strings.get('close_to_tray'),
      );
      expect(tester.widget<SwitchListTile>(startup).value, isFalse);
      await tester.ensureVisible(startup);
      await tester.pumpAndSettle();
      await tester.tap(startup);
      await tester.pumpAndSettle();
      expect(engine.startupChanges, [true]);
      expect(tester.widget<SwitchListTile>(startup).value, isTrue);
      expect(tester.widget<SwitchListTile>(close).value, isTrue);
      await tester.ensureVisible(close);
      await tester.pumpAndSettle();
      await tester.tap(close);
      await tester.pumpAndSettle();
      expect(engine.closePreferenceChanges, [false]);
      expect(tester.widget<SwitchListTile>(close).value, isFalse);
      engine.startupFailure = const EngineException(
        'MACOS_LOGIN_ITEM_FAILED',
        'Synthetic login-item failure.',
      );
      await tester.ensureVisible(startup);
      await tester.pumpAndSettle();
      await tester.tap(startup);
      await tester.pumpAndSettle();
      expect(engine.startupChanges, [true, false]);
      expect(app.startOnBoot, isTrue);
      expect(tester.widget<SwitchListTile>(startup).value, isTrue);
      app.clearError();
      engine.closePreferenceFailure = const EngineException(
        'NATIVE_PREFERENCE_FAILED',
        'Synthetic close preference failure.',
      );
      await tester.ensureVisible(close);
      await tester.pumpAndSettle();
      await tester.tap(close);
      await tester.pumpAndSettle();
      expect(engine.closePreferenceChanges, [false, true]);
      expect(app.closeToTray, isFalse);
      expect(tester.widget<SwitchListTile>(close).value, isFalse);
      app.clearError();
      app.noteTrayAvailability(false);
      await tester.pumpAndSettle();
      expect(tester.widget<SwitchListTile>(close).onChanged, isNull);
      app.noteTrayAvailability(true);
      await tester.pumpAndSettle();
      expect(tester.widget<SwitchListTile>(close).onChanged, isNotNull);
      expect(engine.writes, 0);
      expect(tester.takeException(), isNull);
    },
    variant: TargetPlatformVariant.only(TargetPlatform.macOS),
  );

  for (final section in [AppSection.home, AppSection.proxy]) {
    testWidgets(
      'macOS ${section.name} can disable saved outputs after capabilities disappear',
      (tester) async {
        final engine = _DesktopQaEngine(systemProxy: false);
        final app = await _controller(
          tester,
          engine,
          const Size(1040, 760),
          outputsEnabled: true,
        );
        app.section = section;
        await tester.pumpWidget(workflowHost(app));
        await tester.pumpAndSettle();
        final home = section == AppSection.home;
        final tunnel = home
            ? find.byKey(const ValueKey('home-tun-switch'))
            : find.descendant(
                of: find.byType(LocalProxyOutputs),
                matching: find.widgetWithText(
                  SwitchListTile,
                  app.strings.tunnelOutputLabel(TargetPlatform.macOS),
                ),
              );
        final proxy = home
            ? find.byKey(const ValueKey('home-system-proxy-switch'))
            : find.descendant(
                of: find.byType(LocalProxyOutputs),
                matching: find.widgetWithText(
                  SwitchListTile,
                  app.strings.get('home_system_proxy'),
                ),
              );
        bool enabled(Finder finder) => home
            ? tester.widget<Switch>(finder).onChanged != null
            : tester.widget<SwitchListTile>(finder).onChanged != null;
        for (final toggle in [tunnel, proxy]) {
          expect(enabled(toggle), isTrue);
          await tester.ensureVisible(toggle);
          await tester.pumpAndSettle();
          await tester.tap(toggle);
          await tester.pumpAndSettle();
          expect(enabled(toggle), isFalse);
        }
        expect(app.activeProfile.frontends.tunnel, isFalse);
        expect(app.activeProfile.proxy.systemProxy, isFalse);
        expect(app.activeProfile.frontends.http, isTrue);
        expect(app.activeProfile.frontends.socks5, isTrue);
        expect(engine.writes, 2);
        expect(tester.takeException(), isNull);
      },
      variant: TargetPlatformVariant.only(TargetPlatform.macOS),
    );
  }

  testWidgets(
    'macOS tray publishes unavailable capabilities and shortcuts only disable saved outputs',
    (tester) async {
      const channel = MethodChannel('io.github.georgexie2333.usque/engine');
      final states = <Map<Object?, Object?>>[];
      final messenger = tester.binding.defaultBinaryMessenger;
      messenger.setMockMethodCallHandler(channel, (call) async {
        if (call.method == 'getWindowVisibility') return true;
        if (call.method == 'updateTrayState') {
          states.add(call.arguments as Map<Object?, Object?>);
        }
        return null;
      });
      addTearDown(() => messenger.setMockMethodCallHandler(channel, null));
      final engine = _DesktopQaEngine(systemProxy: false);
      final app = await _controller(
        tester,
        engine,
        const Size(1040, 760),
        outputsEnabled: true,
      );
      final bridge = PlatformShellBridge(app);
      addTearDown(bridge.dispose);
      await tester.pump();
      expect(states.last['tunnel_available'], isFalse);
      expect(states.last['system_proxy_available'], isFalse);
      expect(states.last['tunnel'], isTrue);
      expect(states.last['system_proxy'], isTrue);
      app.snapshot = const EngineSnapshot(phase: ConnectionPhase.preparing);
      expect(await toggleTunnelShortcut(app), isFalse);
      expect(await toggleSystemProxyShortcut(app), isFalse);
      app.snapshot = const EngineSnapshot();
      expect(await toggleTunnelShortcut(app), isTrue);
      expect(await toggleSystemProxyShortcut(app), isTrue);
      await tester.pump();
      expect(states.last['tunnel'], isFalse);
      expect(states.last['system_proxy'], isFalse);
      expect(await toggleTunnelShortcut(app), isFalse);
      expect(await toggleSystemProxyShortcut(app), isFalse);
      expect(engine.writes, 2);
    },
    variant: TargetPlatformVariant.only(TargetPlatform.macOS),
  );

  testWidgets(
    'macOS native login callback reports cancellation and permits a clean retry',
    (tester) async {
      final engine = _DesktopQaEngine();
      final app = await _controller(tester, engine, const Size(800, 600));
      final key = GlobalKey<ZeroTrustEnrollmentEditorState>();
      await tester.pumpWidget(
        workflowHost(
          app,
          home: Scaffold(
            body: SingleChildScrollView(
              child: ZeroTrustEnrollmentEditor(
                key: key,
                controller: app,
                enabled: true,
                initialTeam: 'example-team',
              ),
            ),
          ),
        ),
      );
      await tester.pumpAndSettle();
      final login = find.widgetWithText(
        FilledButton,
        app.strings.get('zero_trust_open_login'),
      );
      await tester.ensureVisible(login);
      await tester.tap(login);
      await tester.pumpAndSettle();
      expect(engine.browserBegins, 1);
      expect(engine.zeroTrustCancelCount, 0);
      engine.callbackFailure = const EngineException(
        'ZERO_TRUST_LOGIN_CANCELLED',
        'Synthetic cancelled authentication session.',
      );
      app.noteZeroTrustCallbackArrived();
      await tester.pumpAndSettle();
      expect(
        find.text(app.strings.get('identity_setup_failed')),
        findsOneWidget,
      );
      await tester.ensureVisible(login);
      await tester.tap(login);
      await tester.pumpAndSettle();
      expect(engine.browserBegins, 2);
      expect(find.text(app.strings.get('identity_setup_failed')), findsNothing);
      const callback =
          'com.cloudflare.warp://example-team.cloudflareaccess.com/auth?token=test-only';
      engine.pendingZeroTrustCallback = callback;
      app.noteZeroTrustCallbackArrived();
      await tester.pumpAndSettle();
      expect(key.currentState!.validateAndRead()!.callbackUri, callback);
      expect(engine.pendingZeroTrustCallback, isNull);
      await key.currentState!.clearSensitive();
      expect(key.currentState!.validateAndRead(), isNull);
      await tester.pumpAndSettle();
      expect(tester.takeException(), isNull);
      await tester.pumpWidget(const SizedBox());
      await tester.pumpAndSettle();
    },
    variant: TargetPlatformVariant.only(TargetPlatform.macOS),
  );

  for (final section in [AppSection.home, AppSection.settings]) {
    testWidgets(
      'simulated platform ${section.name} uses the default desktop client size',
      (tester) async {
        final windows = defaultTargetPlatform == TargetPlatform.windows;
        final app = await _controller(
          tester,
          _DesktopQaEngine(vpn: windows),
          const Size(1040, 760),
        );
        app.section = section;
        if (windows) WindowFrame.instance.debugEnable();
        await tester.pumpWidget(
          RepaintBoundary(
            key: const ValueKey('desktop-qa-capture'),
            child: workflowHost(
              app,
              home: WindowFrameScaffold(
                strings: app.strings,
                phase: app.snapshot.phase,
                child: ShellScreen(controller: app),
              ),
            ),
          ),
        );
        await tester.pumpAndSettle();
        expect(tester.takeException(), isNull);
        expect(
          find.byType(WindowTitleBar),
          windows ? findsOneWidget : findsNothing,
        );
        await _saveSimulation(
          tester,
          '${windows ? 'windows' : 'macos'}_${section.name}_client_1040x760',
        );
      },
      variant: const TargetPlatformVariant({
        TargetPlatform.windows,
        TargetPlatform.macOS,
      }),
    );
  }
}
