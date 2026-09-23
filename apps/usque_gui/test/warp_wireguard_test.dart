import 'dart:io';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:usque/core/l10n/chain.dart';
import 'package:usque/core/warp_strings.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/services/control_codec.dart';
import 'package:usque/services/engine_client.dart';
import 'package:usque/widgets/chain_source_picker.dart';
import 'package:usque/widgets/warp_wireguard_panel.dart';
import 'chain_proxy_test.dart' show ChainEngine, hostChain, chooseSource;
import 'ui_workflow_test.dart' show fieldWithLabel, workflowHost;

const warpProfile = ChainProfileSummary(
  id: 'warp',
  revision: 'r1',
  editRevision: 'r1',
  name: 'WARP test',
  protocol: 'wireguard',
  source: ChainSource.warpWireguard,
  host: '162.159.192.1',
  port: 2408,
);

class WarpEngine extends ChainEngine implements WarpWireguardClient {
  final commands = <Map<String, Object?>>[];
  @override
  Future<Map<Object?, Object?>> warpWireguard(
    Map<String, Object?> request,
  ) async {
    commands.add(request);
    if (request['action'] == 'generate') {
      library = [warpProfile];
      return {
        'job': {
          'id': 'job',
          'kind': 'generate',
          'state': 'completed',
          'mode': 'quick',
          'profile_id': 'warp',
          'countries': <String>[],
          'created_at': '2026-09-23T00:00:00Z',
        },
        'history': <Object?>[],
        'results': <Object?>[],
      };
    }
    return {
      'job': {
        'id': 'job',
        'kind': 'start',
        'state': 'completed',
        'mode': 'quick',
        'countries': ['US'],
        'created_at': '2026-09-23T00:00:00Z',
        'completed': 1,
        'total': 1,
        'working': 1,
      },
      'history': <Object?>[],
      'results': [
        {
          'index': 0,
          'endpoint': {'host': '188.114.98.1', 'port': 500},
          'checked_at': '2026-09-23T00:00:00Z',
          'ipv4': {
            'country': 'US',
            'exit_ip': '104.28.1.1',
            'colo': 'FRA',
            'response_ms': 42,
          },
        },
      ],
    };
  }
}

class WarpFailureEngine extends WarpEngine {
  WarpFailureEngine(this.failure, {this.platform = false});
  final String failure;
  final bool platform;
  @override
  Future<Map<Object?, Object?>> warpWireguard(
    Map<String, Object?> request,
  ) async {
    if (platform) throw EngineException(failure, 'WARP request failed');
    return {
      'job': {
        'id': 'job',
        'kind': 'generate',
        'state': 'failed',
        'failure': failure,
      },
    };
  }
}

void main() {
  testWidgets(
    'registration and Android identity failures remain distinct without exposing exception text',
    (tester) async {
      for (final (code, platform, expected) in [
        ('registration_create_http_403', false, 'registration_create_http_403'),
        ('registration_device_timeout', false, 'registration_device_timeout'),
        (
          'identity_required',
          true,
          'Select an account with a saved MASQUE identity first.',
        ),
        (
          'underlay_start_failed',
          false,
          'The temporary MASQUE connection could not be established. Check the outer connection settings.',
        ),
      ]) {
        await hostChain(tester, WarpFailureEngine(code, platform: platform));
        await chooseSource(tester, ChainSource.warpWireguard);
        expect(find.textContaining(expected), findsOneWidget);
        expect(tester.takeException(), isNull);
        await tester.pumpWidget(const SizedBox.shrink());
      }
      await hostChain(
        tester,
        WarpFailureEngine('Bearer private-token', platform: true),
      );
      await chooseSource(tester, ChainSource.warpWireguard);
      expect(find.textContaining('private-token'), findsNothing);
    },
  );
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
            Future.value(
              ByteData.sublistView(
                File(r'C:\Windows\Fonts\msyh.ttc').readAsBytesSync(),
              ),
            ),
          ))
          .load();
    }
  });
  testWidgets(
    'WARP discovery panel renders measured countries and endpoint drafts',
    (tester) async {
      final app = await hostChain(tester, WarpEngine(), width: 390);
      for (final dark in [false, true]) {
        app.localePreference = dark
            ? LocalePreference.simplifiedChinese
            : LocalePreference.english;
        final boundary = GlobalKey();
        await tester.pumpWidget(
          workflowHost(
            app,
            dark: dark,
            home: Scaffold(
              body: SingleChildScrollView(
                child: RepaintBoundary(
                  key: boundary,
                  child: Builder(
                    builder: (context) => Material(
                      color: Theme.of(context).scaffoldBackgroundColor,
                      child: Padding(
                        padding: const EdgeInsets.all(24),
                        child: WarpWireguardPanel(
                          controller: app,
                          endpoint: const ChainEndpoint('162.159.192.1', 2408),
                          overrideEndpoint: const ChainEndpoint(
                            '188.114.98.1',
                            500,
                          ),
                          onEndpoint: (_) {},
                          onValid: (_) {},
                          onGenerated: () {},
                        ),
                      ),
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
            'goldens/warp_discovery_${dark ? 'dark_zh' : 'light_en'}.png',
          ),
        );
      }
    },
    tags: 'golden',
  );

  test('every supported locale includes the full scanner vocabulary', () {
    expect(kWarpCatalogs.keys.toSet(), kChainCatalogs.keys.toSet());
    for (final values in kWarpCatalogs.values) {
      expect(values.length, kWarpCatalogs['en']!.length);
      expect(values.every((v) => v.trim().isNotEmpty), isTrue);
    }
  });
  test('new source and override survive JSON and appended control fields', () {
    expect(
      ChainSourcePicker.available(null, ChainSource.warpWireguard),
      isFalse,
    );
    expect(
      ChainSourcePicker.available(
        const EngineCapabilities(
          chainProfileImport: true,
          chainWireguard: true,
        ),
        ChainSource.warpWireguard,
      ),
      isFalse,
    );
    const settings = ChainExitSettings(
      enabled: true,
      source: ChainSource.warpWireguard,
      profileId: 'warp',
      revision: 'r1',
      endpointOverride: ChainEndpoint('2606:4700:d0::1', 500),
    );
    expect(ChainExitSettings.fromMap(settings.toMap()), settings);
    expect(ChainSource.values.map((s) => s.wire), [
      'openvpn_custom',
      'wireguard_custom',
      'warp_wireguard',
      'vpn_gate',
    ]);
    final body = ControlPayloadWriter()
      ..string(1, 'warp-cap')
      ..message(15, (ControlPayloadWriter()..boolean(38, true)).takeBytes());
    const codec = ControlCodec();
    expect(
      codec
          .decodeResponse(codec.frame(body.takeBytes()), 'warp-cap')
          .capabilities!
          .chainWarpWireguard,
      isTrue,
    );
    expect(warpProfile.source, ChainSource.warpWireguard);
  });
  testWidgets(
    'generating a profile neither selects it nor changes the connection',
    (tester) async {
      final engine = WarpEngine();
      final app = await hostChain(tester, engine);
      await chooseSource(tester, ChainSource.warpWireguard);
      final before = app.activeProfile.chainExit;
      final generate = find.widgetWithText(
        OutlinedButton,
        'Generate WARP configuration',
      );
      await tester.ensureVisible(generate);
      await tester.tap(generate);
      await tester.pumpAndSettle();
      expect(engine.library.single.source, ChainSource.warpWireguard);
      expect(app.activeProfile.chainExit, before);
      expect(engine.saves, 0);
      expect(engine.commands.any((r) => r['action'] == 'generate'), isTrue);
      await tester.pumpWidget(const SizedBox.shrink());
      expect(engine.commands.any((r) => r['action'] == 'cancel'), isFalse);
    },
  );
  testWidgets(
    'scanner supports 200 percent text and remote selection on phone and TV',
    (tester) async {
      final app = await hostChain(tester, WarpEngine());
      for (final (size, locale) in [
        (const Size(375, 1000), LocalePreference.english),
        (const Size(1920, 1080), LocalePreference.simplifiedChinese),
      ]) {
        tester.view.physicalSize = size;
        app.localePreference = locale;
        ChainEndpoint? selected;
        await tester.pumpWidget(
          workflowHost(
            app,
            dark: true,
            scale: 2,
            home: Scaffold(
              body: SingleChildScrollView(
                child: Padding(
                  padding: const EdgeInsets.all(16),
                  child: WarpWireguardPanel(
                    controller: app,
                    endpoint: const ChainEndpoint('162.159.192.1', 2408),
                    overrideEndpoint: null,
                    onEndpoint: (value) => selected = value,
                    onValid: (_) {},
                    onGenerated: () {},
                  ),
                ),
              ),
            ),
          ),
        );
        await tester.pumpAndSettle();
        final candidate = find.text('188.114.98.1:500');
        await tester.ensureVisible(candidate);
        Focus.of(tester.element(candidate)).requestFocus();
        await tester.pump();
        await tester.sendKeyEvent(LogicalKeyboardKey.select);
        await tester.pumpAndSettle();
        expect(selected, const ChainEndpoint('188.114.98.1', 500));
        expect(tester.takeException(), isNull);
      }
    },
  );
  testWidgets(
    'manual endpoint and scanned selection remain drafts and invalid ports block apply',
    (tester) async {
      final engine = WarpEngine()..library = [warpProfile];
      final app = await hostChain(tester, engine);
      await chooseSource(tester, ChainSource.warpWireguard);
      await tester.tap(find.byKey(const ValueKey('chain-proxy-toggle')));
      await tester.pumpAndSettle();
      final row = find.text('WARP test');
      await tester.scrollUntilVisible(
        row,
        300,
        scrollable: find.byType(Scrollable).first,
      );
      await tester.pumpAndSettle();
      await tester.tap(row);
      await tester.pumpAndSettle();
      final ip = fieldWithLabel('Endpoint IP');
      await tester.ensureVisible(ip);
      await tester.enterText(ip, '2606:4700:d0::99');
      final port = fieldWithLabel('Port');
      await tester.enterText(port, '65536');
      await tester.pumpAndSettle();
      expect(engine.saves, 0);
      expect(app.activeProfile.chainExit?.endpointOverride, isNull);
      expect(tester.widget<TextField>(port).decoration!.errorText, isNotNull);
      await tester.enterText(port, '4500');
      await tester.pumpAndSettle();
      expect(tester.widget<TextField>(port).decoration!.errorText, isNull);
      final panel = tester.widget<WarpWireguardPanel>(
        find.byType(WarpWireguardPanel),
      );
      expect(
        panel.overrideEndpoint,
        const ChainEndpoint('2606:4700:d0::99', 4500),
      );
      final candidate = find.text('188.114.98.1:500');
      await tester.ensureVisible(candidate);
      await tester.tap(candidate);
      await tester.pumpAndSettle();
      expect(
        tester
            .widget<WarpWireguardPanel>(find.byType(WarpWireguardPanel))
            .overrideEndpoint,
        const ChainEndpoint('188.114.98.1', 500),
      );
      expect(engine.saves, 0);
      expect(tester.takeException(), isNull);
    },
  );
}
