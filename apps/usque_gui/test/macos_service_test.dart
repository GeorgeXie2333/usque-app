import 'dart:ffi' show Abi;
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/services/engine_client.dart';
import 'package:usque/state/app_controller.dart';
import 'package:usque/widgets/local_proxy_outputs.dart';
import 'package:usque/widgets/macos_service_section.dart';

import 'app_test.dart' show FakeEngineClient;
import 'ui_workflow_test.dart' show workflowHost;

class ServiceEngine extends FakeEngineClient implements MacosServiceClient {
  MacosServiceState state = const MacosServiceState(canMaintain: true);
  final actions = <MacosServiceAction>[];
  bool cancelFirst = false;
  @override
  Future<MacosServiceState> getMacosServiceState() async => state;
  @override
  Future<void> maintainMacosService(MacosServiceAction action) async {
    actions.add(action);
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
      expect(find.text('Service is not installed.'), findsOneWidget);
      expect(
        find.text('The operation did not complete. You can retry.'),
        findsOneWidget,
      );
      expect(tester.widget<OutlinedButton>(install).onPressed, isNotNull);
      await tester.tap(install);
      await tester.pumpAndSettle();
      expect(engine.actions, [
        MacosServiceAction.install,
        MacosServiceAction.install,
      ]);
      expect(find.text('Service is installed.'), findsOneWidget);
      expect(
        find.text('System VPN awaits platform acceptance in this build.'),
        findsOneWidget,
      );
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
      for (final key in ['macos-service-install', 'macos-service-recover']) {
        expect(
          tester.widget<OutlinedButton>(find.byKey(ValueKey(key))).onPressed,
          isNull,
        );
      }
      expect(
        tester
            .widget<TextButton>(
              find.byKey(const ValueKey('macos-service-uninstall')),
            )
            .onPressed,
        isNull,
      );
      expect(engine.actions, isEmpty);
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
  }.entries) {
    testWidgets(
      'macOS exact golden ${fixture.key}',
      (tester) async {
        tester.view.devicePixelRatio = 1;
        tester.view.physicalSize = const Size(900, 720);
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
        final engine = ServiceEngine()..state = fixture.value;
        final app = AppController(engine)
          ..localePreference = LocalePreference.english
          ..engineCapabilities = EngineCapabilities(
            vpn: fixture.value.tunAvailable,
            systemProxy: true,
          );
        addTearDown(app.dispose);
        await tester.pumpWidget(
          workflowHost(
            app,
            home: RepaintBoundary(
              key: const ValueKey('mac-capture'),
              child: Scaffold(
                body: Padding(
                  padding: const EdgeInsets.all(24),
                  child: Column(
                    children: [
                      MacosServiceSection(controller: app),
                      const SizedBox(height: 24),
                      LocalProxyOutputs(controller: app, enabled: true),
                    ],
                  ),
                ),
              ),
            ),
          ),
        );
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
