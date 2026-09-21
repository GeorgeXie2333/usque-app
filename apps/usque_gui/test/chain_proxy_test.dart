import 'dart:convert';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:shared_preferences/shared_preferences.dart';
import 'package:usque/core/chain_strings.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/screens/chain_proxy_screen.dart';
import 'package:usque/services/control_codec.dart';
import 'package:usque/services/engine_client.dart';
import 'package:usque/state/app_controller.dart';
import 'package:usque/widgets/chain_source_icon.dart';
import 'ui_workflow_test.dart' show workflowHost, fieldWithLabel;
import 'vpngate_test.dart' show GateEngine;

const imported = ChainProfileSummary(
  id: 'imported',
  revision: 'r1',
  editRevision: 'e1',
  name: 'Office tunnel',
  protocol: 'wireguard',
  host: 'vpn.example',
  port: 51820,
  addresses: ['10.8.0.2/32'],
  allowedIps: ['10.8.0.0/24'],
  dns: ['10.8.0.1'],
  mtu: 1280,
);

class ChainEngine extends GateEngine implements ChainProfileClient {
  List<ChainProfileSummary> library = [];
  final actions = <String>[];
  String? picked;
  @override
  Future<EngineCapabilities?> getCapabilities() async =>
      const EngineCapabilities(
        vpnGateTcp: true,
        vpnGatePoolFavorites: true,
        networkSettingsApplication: true,
        chainProfileImport: true,
        chainOpenvpnUdp: true,
        chainWireguard: true,
      );
  @override
  Future<String?> pickChainConfiguration() async => picked;
  @override
  Future<ChainProfileResult> chainProfile(Map<String, Object?> request) async {
    final action = request['action'] as String;
    actions.add(action);
    if (action == 'import') library = [imported];
    return ChainProfileResult(
      profiles: library,
      preview: action == 'preview' || action == 'import' ? imported : null,
    );
  }
}

Future<AppController> hostChain(
  WidgetTester tester,
  ChainEngine engine, {
  bool l4 = false,
  double width = 980,
}) async {
  SharedPreferences.setMockInitialValues({});
  tester.view.devicePixelRatio = 1;
  tester.view.physicalSize = Size(width, 1100);
  addTearDown(tester.view.resetDevicePixelRatio);
  addTearDown(tester.view.resetPhysicalSize);
  engine.legacyProfilesImported = true;
  engine.storedProfiles[0] = engine.storedProfiles[0].copyWith(
    chainExit: const ChainExitSettings(source: ChainSource.wireguardCustom),
    dataPlane: l4 ? DataPlaneMode.l4Proxy : DataPlaneMode.connectIp,
  );
  final app = AppController(engine);
  await app.initialize();
  app.localePreference = LocalePreference.english;
  addTearDown(app.dispose);
  await tester.pumpWidget(
    workflowHost(app, home: ChainProxyScreen(controller: app)),
  );
  await tester.pumpAndSettle();
  return app;
}

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
    for (final family in {
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
  });

  testWidgets('VPN Gate draft identifies the currently connected custom exit', (
    tester,
  ) async {
    final engine = ChainEngine()..library = [imported];
    final app = await hostChain(tester, engine);
    app.snapshot = const EngineSnapshot(
      phase: ConnectionPhase.connected,
      chainExit: ChainExitStatus(stage: 'connected', currentProfile: imported),
    );
    await tester.tap(find.byType(DropdownButtonFormField<ChainSource>));
    await tester.pumpAndSettle();
    await tester.tap(find.text('VPN Gate').last);
    await tester.pumpAndSettle();
    expect(find.text('WireGuard (Custom) · Office tunnel'), findsOneWidget);
    expect(find.textContaining('0.0.0.0'), findsNothing);
    expect(engine.saves, 0);
  });

  test('modern chain status is independent of legacy Gate field order', () {
    const codec = ControlCodec();
    final metadata = {
      'stage': 'connected',
      'current_profile': {
        'id': 'p',
        'revision': 'r',
        'edit_revision': 'e',
        'name': 'Office',
        'protocol': 'wireguard',
        'endpoint': {'host': 'vpn.example', 'port': 51820},
      },
    };
    for (final modernFirst in [true, false]) {
      final snapshot = ControlPayloadWriter();
      final modern = (ControlPayloadWriter()..string(1, jsonEncode(metadata)))
          .takeBytes();
      final legacy = (ControlPayloadWriter()..string(1, 'disabled'))
          .takeBytes();
      if (modernFirst) {
        snapshot
          ..message(22, modern)
          ..message(21, legacy);
      } else {
        snapshot
          ..message(21, legacy)
          ..message(22, modern);
      }
      final response =
          (ControlPayloadWriter()
                ..string(1, 'r')
                ..message(11, snapshot.takeBytes()))
              .takeBytes();
      final state = codec.decodeResponse(codec.frame(response), 'r').snapshot!;
      expect(state.chainExit.currentProfile!.name, 'Office');
      expect(state.vpnGate.connected, isTrue);
    }
  });

  testWidgets('custom configuration page golden on phone and desktop', (
    tester,
  ) async {
    final engine = ChainEngine()..library = [imported];
    final app = await hostChain(tester, engine);
    for (final width in [390.0, 1100.0]) {
      tester.view.physicalSize = Size(width, 1100);
      final boundary = GlobalKey();
      await tester.pumpWidget(
        RepaintBoundary(
          key: boundary,
          child: workflowHost(
            app,
            dark: width > 500,
            home: ChainProxyScreen(controller: app),
          ),
        ),
      );
      await tester.pumpAndSettle();
      expect(tester.takeException(), isNull);
      await expectLater(
        find.byKey(boundary),
        matchesGoldenFile(
          'goldens/chain_custom_${width > 500 ? 'desktop' : 'phone'}.png',
        ),
      );
    }
  }, tags: 'golden');
  test(
    'source names and order are locale independent and wire fields append',
    () {
      expect(ChainSource.values.map((s) => s.label), [
        'OpenVPN (Custom)',
        'WireGuard (Custom)',
        'VPN Gate',
      ]);
      const codec = ControlCodec();
      final payload = (ControlPayloadWriter()..string(1, 'list')).takeBytes();
      final request = codec.buildRequestFrame(
        requestId: '',
        payloadField: 47,
        payload: payload,
      );
      expect(request.sublist(4), [0xfa, 0x02, 6, 10, 4, 108, 105, 115, 116]);
      final body = ControlPayloadWriter()
        ..string(1, 'r')
        ..message(
          24,
          (ControlPayloadWriter()..string(
                1,
                jsonEncode({'profiles': <Object?>[], 'error': null}),
              ))
              .takeBytes(),
        );
      expect(
        codec
            .decodeResponse(codec.frame(body.takeBytes()), 'r')
            .chainProfiles!
            .profiles,
        isEmpty,
      );
      final capabilities = ControlPayloadWriter()
        ..boolean(34, true)
        ..boolean(35, true)
        ..boolean(36, true);
      expect(
        capabilities.takeBytes(),
        Uint8List.fromList([0x90, 2, 1, 0x98, 2, 1, 0xa0, 2, 1]),
      );
    },
  );

  testWidgets(
    'text preview and save leave the selection and connection unchanged',
    (tester) async {
      final engine = ChainEngine();
      final app = await hostChain(tester, engine);
      final before = app.activeProfile.chainExit;
      await tester.tap(find.text('Paste configuration'));
      await tester.pumpAndSettle();
      await tester.enterText(
        fieldWithLabel('Configuration text'),
        '[Interface]\nPrivateKey = fixture',
      );
      await tester.tap(find.text('Check configuration'));
      await tester.pumpAndSettle();
      expect(find.textContaining('AllowedIPs'), findsOneWidget);
      expect(engine.actions, contains('preview'));
      expect(engine.saves, 0);
      await tester.tap(find.text('Save configuration'));
      await tester.pumpAndSettle();
      expect(find.text('Office tunnel'), findsOneWidget);
      expect(app.activeProfile.chainExit, before);
      expect(engine.saves, 0);
      expect(app.snapshot.isConnected, isFalse);
    },
  );

  testWidgets(
    'L4 draft requires explicit CONNECT-IP apply and survives a cancelled source switch',
    (tester) async {
      final engine = ChainEngine()..library = [imported];
      final app = await hostChain(tester, engine, l4: true);
      await tester.tap(find.byKey(const ValueKey('chain-proxy-toggle')));
      await tester.pumpAndSettle();
      await tester.tap(find.text('Office tunnel'));
      await tester.pumpAndSettle();
      expect(engine.saves, 0);
      expect(app.activeProfile.dataPlane, DataPlaneMode.l4Proxy);
      await tester.tap(find.byType(DropdownButtonFormField<ChainSource>));
      await tester.pumpAndSettle();
      await tester.tap(find.text('OpenVPN (Custom)').last);
      await tester.pumpAndSettle();
      expect(find.text(app.strings.get('discard_changes_title')), findsWidgets);
      await tester.tap(find.text(app.strings.get('keep_editing')));
      await tester.pumpAndSettle();
      final apply = find.text('Switch to CONNECT-IP and apply');
      await Scrollable.ensureVisible(tester.element(apply), alignment: 0.5);
      await tester.pumpAndSettle();
      await tester.tap(apply);
      await tester.pumpAndSettle();
      expect(engine.saves, 1);
      expect(app.activeProfile.dataPlane, DataPlaneMode.connectIp);
      expect(app.activeProfile.chainExit!.profileId, imported.id);
      expect(engine.fields, ['chain_exit', 'vpn_gate', 'data_plane']);
    },
  );

  testWidgets('cancelled file picker does not create a profile or draft', (
    tester,
  ) async {
    final engine = ChainEngine();
    final app = await hostChain(tester, engine);
    final before = app.activeProfile.chainExit;
    await tester.tap(find.text('Import file'));
    await tester.pumpAndSettle();
    expect(engine.actions.where((a) => a != 'list'), isEmpty);
    expect(app.activeProfile.chainExit, before);
    expect(engine.saves, 0);
  });

  testWidgets(
    'custom page supports phone large text and TV focus in both locales',
    (tester) async {
      final engine = ChainEngine()..library = [imported];
      final app = await hostChain(tester, engine);
      for (final (size, scale, locale) in [
        (const Size(375, 1000), 2.0, LocalePreference.english),
        (const Size(1920, 1080), 2.0, LocalePreference.simplifiedChinese),
      ]) {
        tester.view.physicalSize = size;
        app.localePreference = locale;
        await tester.pumpWidget(
          workflowHost(
            app,
            dark: true,
            scale: scale,
            home: ChainProxyScreen(controller: app),
          ),
        );
        await tester.pumpAndSettle();
        expect(tester.takeException(), isNull);
        Focus.of(
          tester.element(find.text(app.strings.chain('paste'))),
        ).requestFocus();
        await tester.pump();
        await tester.sendKeyEvent(LogicalKeyboardKey.select);
        await tester.pumpAndSettle();
        expect(find.text(app.strings.chain('configuration')), findsOneWidget);
        expect(tester.takeException(), isNull);
        await tester.tap(find.text(app.strings.chain('cancel')));
        await tester.pumpAndSettle();
      }
    },
  );

  testWidgets(
    'SVGs render at 18 20 24 32 with selected disabled and focus colors',
    (tester) async {
      tester.view.devicePixelRatio = 1;
      tester.view.physicalSize = const Size(800, 420);
      addTearDown(tester.view.resetDevicePixelRatio);
      addTearDown(tester.view.resetPhysicalSize);
      final app = AppController(ChainEngine());
      addTearDown(app.dispose);
      for (final dark in [false, true]) {
        final boundary = GlobalKey();
        await tester.pumpWidget(
          workflowHost(
            app,
            dark: dark,
            home: RepaintBoundary(
              key: boundary,
              child: Scaffold(
                body: Builder(
                  builder: (context) => Padding(
                    padding: const EdgeInsets.all(24),
                    child: Column(
                      children: [
                        for (final source in ChainSource.values)
                          Padding(
                            padding: const EdgeInsets.all(12),
                            child: Row(
                              children: [
                                SizedBox(width: 190, child: Text(source.label)),
                                for (final size in [
                                  18.0,
                                  20.0,
                                  24.0,
                                  32.0,
                                ]) ...[
                                  ChainSourceIcon(source: source, size: size),
                                  const SizedBox(width: 16),
                                ],
                                ChainSourceIcon(
                                  source: source,
                                  color: Theme.of(context).colorScheme.primary,
                                ),
                                const SizedBox(width: 16),
                                ChainSourceIcon(
                                  source: source,
                                  color: Theme.of(context).disabledColor,
                                ),
                                const SizedBox(width: 16),
                                OutlinedButton(
                                  onPressed: () {},
                                  autofocus:
                                      source == ChainSource.openvpnCustom,
                                  child: ChainSourceIcon(source: source),
                                ),
                              ],
                            ),
                          ),
                      ],
                    ),
                  ),
                ),
              ),
            ),
          ),
        );
        await tester.pumpAndSettle();
        expect(tester.takeException(), isNull);
        await expectLater(
          find.byKey(boundary),
          matchesGoldenFile(
            'goldens/chain_icons_${dark ? 'dark' : 'light'}.png',
          ),
        );
      }
    },
    tags: 'golden',
  );
}
