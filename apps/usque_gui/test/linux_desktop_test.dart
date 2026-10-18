import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:shared_preferences/shared_preferences.dart';
import 'package:usque/core/l10n/linux_desktop.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/services/control_codec.dart';
import 'package:usque/services/desktop_engine_client.dart';
import 'package:usque/services/desktop_engine_transport.dart';
import 'package:usque/services/engine_client.dart';
import 'package:usque/services/platform_shell_bridge.dart';
import 'package:usque/services/update_downloader.dart';
import 'package:usque/state/app_controller.dart';
import 'package:usque/widgets/linux_service_section.dart';
import 'package:usque/widgets/local_proxy_outputs.dart';

import 'app_test.dart' show FakeEngineClient, RecordingUpdateDownloader;
import 'ui_workflow_test.dart' show WorkflowEngine, pumpWorkflow, workflowHost;

class _ServiceEngine extends FakeEngineClient implements LinuxServiceClient {
  LinuxServiceState state = const LinuxServiceState(canInstall: true);
  int attempts = 0;
  bool? lastInstall;

  @override
  Future<LinuxServiceState> getLinuxServiceState() async => state;

  @override
  Future<void> maintainLinuxService({required bool install}) async {
    lastInstall = install;
    attempts++;
    if (attempts == 1) {
      throw const EngineException(
        'LINUX_SERVICE_MAINTENANCE_FAILED',
        'cancelled',
      );
    }
    state = const LinuxServiceState(
      installed: true,
      compatible: true,
      canRepair: true,
    );
  }
}

void main() {
  test(
    'Linux keyring waits preserve Windows budgets and bound native operations',
    () {
      expect(requestTimeoutForPayload(28), const Duration(seconds: 5));
      expect(linuxRequestTimeoutForPayload(28), const Duration(seconds: 365));
      expect(linuxRequestTimeoutForPayload(47), const Duration(seconds: 365));
      expect(linuxRequestTimeoutForPayload(12), const Duration(seconds: 1075));
      expect(requestTimeoutForPayload(12), const Duration(seconds: 715));
      expect(linuxRequestTimeoutForPayload(27), const Duration(seconds: 1775));
      expect(linuxRequestTimeoutForPayload(24), const Duration(seconds: 10));
      expect(linuxRequestTimeoutForPayload(10), const Duration(seconds: 5));
    },
  );
  TestWidgetsFlutterBinding.ensureInitialized();
  for (final format in ['AppImage', 'deb', 'rpm']) {
    test(
      'Linux $format install cancellation keeps a retryable download',
      () async {
        SharedPreferences.setMockInitialValues({
          'update_checks_enabled': false,
        });
        final engine = FakeEngineClient()
          ..installUpdateError = const EngineException(
            'UPDATE_INSTALL_LAUNCH_FAILED',
            'authorization cancelled',
          );
        final downloader = RecordingUpdateDownloader(engine);
        final controller = AppController(engine, updateDownloader: downloader);
        addTearDown(controller.dispose);
        await controller.initialize();
        final name = 'usque-v0.3.3-linux-amd64.$format';
        final path = 'test-update-cache/$name';
        controller.updateResult = UpdateCheckResult(
          available: true,
          version: 'v0.3.3',
          package: UpdatePackage(
            name: name,
            downloadUrl: 'https://example.invalid/$name',
            size: 1024,
            sha256: 'a5' * 32,
            platform: 'linux',
            variant: 'amd64',
            installFormat: format,
          ),
        );
        controller.downloadedUpdatePath = path;
        controller.updateDownloadedBytes = 1024;
        controller.updatePhase = UpdateOperationPhase.ready;

        await controller.installDownloadedUpdate();
        expect(downloader.discardedPath, isNull);
        expect(controller.downloadedUpdatePath, path);
        expect(controller.updateDownloadedBytes, 1024);
        expect(controller.updatePhase, UpdateOperationPhase.ready);
        expect(controller.updateError, isNotNull);

        engine.installUpdateError = null;
        await controller.installDownloadedUpdate();
        expect(controller.updatePhase, UpdateOperationPhase.installing);
        expect(controller.updateError, isNull);
        expect(
          engine.calls.where((call) => call == 'installUpdatePackage').length,
          2,
        );
      },
    );
  }
  test(
    'Linux sensitive clipboard uses explicit native ownership and wipes the IPC frame',
    () async {
      if (!Platform.isLinux) return;
      const channel = MethodChannel('io.github.georgexie2333.usque/engine');
      final messenger =
          TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger;
      final secret =
          (ControlPayloadWriter()
                ..bytes(1, Uint8List.fromList('test-license'.codeUnits))
                ..unsigned(2, 30))
              .takeBytes();
      final response = const ControlCodec().frame(
        (ControlPayloadWriter()
              ..string(1, 'copy')
              ..message(27, secret))
            .takeBytes(),
      );
      String? copied;
      messenger.setMockMethodCallHandler(channel, (call) async {
        expect(call.method, 'copySensitiveText');
        final args = call.arguments as Map<Object?, Object?>;
        copied = String.fromCharCodes(args['value'] as Uint8List);
        expect(args['clear_after_seconds'], 30);
        return null;
      });
      addTearDown(() => messenger.setMockMethodCallHandler(channel, null));
      final engine = DesktopEngineClient.forTest(
        transport: DesktopEngineTransport.forTest(
          exchange: (_) async => response,
          requestIdFactory: () => 'copy',
        ),
      );
      addTearDown(engine.dispose);
      await engine.copyLicenseKey('profile');
      expect(copied, 'test-license');
      expect(response.every((byte) => byte == 0), isTrue);
    },
  );

  test('installed Engine receives session paths without loader injection', () {
    final environment = linuxInstalledEngineEnvironment({
      'HOME': '/home/user',
      'XDG_CONFIG_HOME': '/custom/config',
      'XDG_DATA_HOME': '/custom/data',
      'XDG_CACHE_HOME': '/custom/cache',
      'XDG_STATE_HOME': '/custom/state',
      'XDG_RUNTIME_DIR': '/run/user/1000',
      'DBUS_SESSION_BUS_ADDRESS': 'unix:path=/run/user/1000/bus',
      'LD_PRELOAD': '/tmp/injected.so',
      'LD_LIBRARY_PATH': '/tmp/appimage/lib',
      'GIO_EXTRA_MODULES': '/tmp/plugins',
      'APPDIR': '/tmp/appimage-mount',
      'APPIMAGE': '/home/user/Usque.AppImage',
      'PATH': '/tmp/bin',
    });
    expect(environment['HOME'], '/home/user');
    expect(environment['XDG_CONFIG_HOME'], '/custom/config');
    expect(environment['XDG_DATA_HOME'], '/custom/data');
    expect(environment['XDG_CACHE_HOME'], '/custom/cache');
    expect(environment['XDG_STATE_HOME'], '/custom/state');
    expect(environment['XDG_RUNTIME_DIR'], '/run/user/1000');
    expect(environment['DBUS_SESSION_BUS_ADDRESS'], isNotNull);
    expect(environment['PATH'], '/usr/sbin:/usr/bin:/sbin:/bin');
    expect(environment.containsKey('LD_PRELOAD'), isFalse);
    expect(environment.containsKey('LD_LIBRARY_PATH'), isFalse);
    expect(environment.containsKey('GIO_EXTRA_MODULES'), isFalse);
    expect(environment.containsKey('APPDIR'), isFalse);
    expect(environment['APPIMAGE'], '/home/user/Usque.AppImage');
    expect(
      linuxInstalledEngineEnvironment({
        'APPIMAGE': 'relative.AppImage',
      }).containsKey('APPIMAGE'),
      isFalse,
    );
  });

  test('Linux XDG paths reject unsafe runtime and ignore relative config', () {
    expect(
      linuxConfigPath({'XDG_CONFIG_HOME': '/config', 'HOME': '/home/user'}),
      '/config/usque/config.json',
    );
    expect(
      linuxConfigPath({'XDG_CONFIG_HOME': '../config', 'HOME': '/home/user'}),
      '/home/user/.config/usque/config.json',
    );
    for (final environment in <Map<String, String>>[
      {},
      {'XDG_RUNTIME_DIR': 'tmp'},
    ]) {
      expect(
        () => linuxSocketPath(environment),
        throwsA(isA<EngineException>()),
      );
    }
    expect(
      linuxSocketPath({'XDG_RUNTIME_DIR': '/run/user/1000'}),
      '/run/user/1000/usque/engine-v1.sock',
    );
    expect(
      () => linuxSocketPath({'XDG_RUNTIME_DIR': '/${'x' * 100}'}),
      throwsA(isA<EngineException>()),
    );
  });

  test('all 21 Linux catalogs contain complete localized service copy', () {
    expect(kLinuxDesktopCatalogs.length, 21);
    final keys = kLinuxDesktopCatalogs['en']!.keys.toSet();
    for (final catalog in kLinuxDesktopCatalogs.values) {
      expect(catalog.keys.toSet(), keys);
      expect(catalog.values.every((value) => value.trim().isNotEmpty), isTrue);
    }
  });

  test('Linux output controls follow real capability updates', () {
    debugDefaultTargetPlatformOverride = TargetPlatform.linux;
    addTearDown(() => debugDefaultTargetPlatformOverride = null);
    final controller = AppController(FakeEngineClient());
    addTearDown(controller.dispose);
    controller.engineCapabilities = const EngineCapabilities();
    expect(controller.tunnelOutputSupported, isFalse);
    expect(controller.systemProxyOutputSupported, isFalse);
    controller.engineCapabilities = const EngineCapabilities(
      vpn: true,
      systemProxy: true,
    );
    expect(controller.tunnelOutputSupported, isTrue);
    expect(controller.systemProxyOutputSupported, isTrue);
    controller.noteTrayAvailability(false);
    expect(controller.trayAvailable, isFalse);
    expect(controller.closeToTray, isFalse);
  });

  for (final section in [AppSection.home, AppSection.proxy]) {
    testWidgets(
      'unavailable Linux outputs can be disabled from ${section.name}',
      (tester) async {
        final engine = WorkflowEngine();
        final controller = await pumpWorkflow(tester, engine, section: section);
        controller.networkSettings.supported = true;
        controller
          ..initialized = true
          ..engineCapabilities = await engine.getCapabilities()
          ..sharedNetwork = controller.activeProfile.copyWith(
            proxy: controller.activeProfile.proxy.copyWith(systemProxy: true),
          );
        await tester.pumpWidget(workflowHost(controller));
        await tester.pumpAndSettle();

        Finder output(String key, String label) => section == AppSection.home
            ? find.byKey(ValueKey(key))
            : find.descendant(
                of: find.byType(LocalProxyOutputs),
                matching: find.widgetWithText(SwitchListTile, label),
              );
        bool enabled(Finder finder) => section == AppSection.home
            ? tester.widget<Switch>(finder).onChanged != null
            : tester.widget<SwitchListTile>(finder).onChanged != null;
        final tunnel = output(
          'home-tun-switch',
          controller.strings.tunnelOutputLabel(TargetPlatform.linux),
        );
        final proxy = output(
          'home-system-proxy-switch',
          controller.strings.get('home_system_proxy'),
        );
        expect(controller.activeProfile.frontends.tunnel, isTrue);
        expect(enabled(tunnel), isTrue);
        await tester.ensureVisible(tunnel);
        await tester.tap(tunnel);
        await tester.pumpAndSettle();
        expect(controller.activeProfile.frontends.tunnel, isFalse);
        expect(enabled(tunnel), isFalse);

        expect(enabled(proxy), isTrue);
        await tester.ensureVisible(proxy);
        await tester.tap(proxy);
        await tester.pumpAndSettle();
        expect(controller.activeProfile.proxy.systemProxy, isFalse);
        expect(enabled(proxy), isFalse);
        expect(controller.activeProfile.frontends.http, isTrue);
        expect(controller.activeProfile.frontends.socks5, isTrue);
        expect(engine.writes, 2);
        expect(tester.takeException(), isNull);
      },
      variant: TargetPlatformVariant.only(TargetPlatform.linux),
    );
  }

  test(
    'unavailable Linux tray outputs can only be disabled while unlocked',
    () async {
      debugDefaultTargetPlatformOverride = TargetPlatform.linux;
      addTearDown(() => debugDefaultTargetPlatformOverride = null);
      final engine = WorkflowEngine();
      final controller = AppController(engine);
      addTearDown(controller.dispose);
      controller.networkSettings.supported = true;
      controller
        ..initialized = true
        ..engineCapabilities = await engine.getCapabilities()
        ..sharedNetwork = controller.activeProfile.copyWith(
          proxy: controller.activeProfile.proxy.copyWith(systemProxy: true),
        )
        ..snapshot = const EngineSnapshot(phase: ConnectionPhase.preparing);
      expect(await toggleTunnelShortcut(controller), isFalse);
      expect(await toggleSystemProxyShortcut(controller), isFalse);
      expect(engine.writes, 0);
      controller.snapshot = const EngineSnapshot(
        phase: ConnectionPhase.disconnected,
      );
      expect(await toggleTunnelShortcut(controller), isTrue);
      expect(await toggleSystemProxyShortcut(controller), isTrue);
      expect(controller.activeProfile.frontends.tunnel, isFalse);
      expect(controller.activeProfile.proxy.systemProxy, isFalse);
      expect(await toggleTunnelShortcut(controller), isFalse);
      expect(await toggleSystemProxyShortcut(controller), isFalse);
      expect(engine.writes, 2);
    },
  );

  testWidgets(
    'service installation is explicit and cancellation remains retryable',
    (tester) async {
      final engine = _ServiceEngine();
      final controller = AppController(engine);
      addTearDown(controller.dispose);
      await tester.pumpWidget(
        MaterialApp(
          home: Scaffold(body: LinuxServiceSection(controller: controller)),
        ),
      );
      await tester.pumpAndSettle();
      expect(engine.attempts, 0);
      final action = find.byKey(const ValueKey('linux-service-maintain'));
      await tester.tap(action);
      await tester.pumpAndSettle();
      expect(engine.attempts, 1);
      expect(tester.widget<OutlinedButton>(action).onPressed, isNotNull);
      await tester.tap(action);
      await tester.pumpAndSettle();
      expect(engine.attempts, 2);
      expect(
        find.text(controller.strings.get('linux_service_repair')),
        findsOneWidget,
      );
    },
  );

  testWidgets(
    'AppImage version mismatch offers explicit matching service install',
    (tester) async {
      final engine = _ServiceEngine()
        ..attempts = 1
        ..state = const LinuxServiceState(
          installed: true,
          canInstall: true,
          canRepair: true,
        );
      final controller = AppController(engine);
      addTearDown(controller.dispose);
      await tester.pumpWidget(
        MaterialApp(
          home: Scaffold(body: LinuxServiceSection(controller: controller)),
        ),
      );
      await tester.pumpAndSettle();
      expect(
        find.text(controller.strings.get('linux_service_install')),
        findsOneWidget,
      );
      await tester.tap(find.byKey(const ValueKey('linux-service-maintain')));
      await tester.pumpAndSettle();
      expect(engine.lastInstall, isTrue);
    },
  );

  test(
    'Linux update formats require exact extension and signed metadata',
    () async {
      final root = await Directory.systemTemp.createTemp('usque-linux-update-');
      addTearDown(() => root.delete(recursive: true));
      final downloader = UpdateDownloader(FakeEngineClient());
      for (final format in ['AppImage', 'deb', 'rpm']) {
        final package = UpdatePackage(
          name: 'usque-v0.3.2-linux-amd64.$format',
          downloadUrl:
              'https://github.com/GeorgeXie2333/usque-app/releases/download/v0.3.2/usque-v0.3.2-linux-amd64.$format',
          size: 10,
          sha256: 'a' * 64,
          platform: 'linux',
          variant: 'amd64',
          installFormat: format,
          // No signature: reject before any network access or file operation.
        );
        await expectLater(
          downloader.download(
            package,
            onProgress: (_, _) {},
            cancellation: UpdateDownloadCancellation(),
          ),
          throwsA(isA<UpdateDownloadException>()),
        );
      }
    },
  );
}
