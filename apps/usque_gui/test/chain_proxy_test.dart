import 'dart:convert';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';
import 'package:shared_preferences/shared_preferences.dart';
import 'package:usque/core/app_strings.dart';
import 'package:usque/core/chain_strings.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/screens/chain_proxy_screen.dart';
import 'package:usque/services/control_codec.dart';
import 'package:usque/services/engine_client.dart';
import 'package:usque/state/app_controller.dart';
import 'package:usque/widgets/chain_proxy_entry.dart';
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
  final names = <String>[];
  String? picked;
  EngineException? pickerError;
  bool multiEndpoint = true;
  ChainProfileSummary previewProfile = imported;
  @override
  Future<EngineCapabilities?> getCapabilities() async => EngineCapabilities(
    vpnGateTcp: true,
    vpnGatePoolFavorites: true,
    networkSettingsApplication: true,
    chainProfileImport: true,
    chainOpenvpnUdp: true,
    chainWireguard: true,
    chainOpenvpnMultiEndpoint: multiEndpoint,
  );
  @override
  Future<String?> pickChainConfiguration() async {
    if (pickerError case final error?) {
      throw error;
    }
    return picked;
  }

  @override
  Future<ChainProfileResult> chainProfile(Map<String, Object?> request) async {
    final action = request['action'] as String;
    actions.add(action);
    if (action == 'preview' || action == 'import') {
      // The engine validates the name even when only checking.
      final name = (request['name'] as String? ?? '').trim();
      names.add(name);
      if (name.isEmpty || name.length > 64) {
        return ChainProfileResult(
          profiles: library,
          error: const {'reason': 'invalid_name', 'field': 'name'},
        );
      }
    }
    if (action == 'import') {
      library = [previewProfile.copyWith(name: names.last)];
    }
    return ChainProfileResult(
      profiles: library,
      preview: action == 'preview' || action == 'import'
          ? previewProfile
          : null,
    );
  }
}

extension on ChainProfileSummary {
  ChainProfileSummary copyWith({String? name}) => ChainProfileSummary(
    id: id,
    revision: revision,
    editRevision: editRevision,
    name: name ?? this.name,
    protocol: protocol,
    host: host,
    port: port,
    addresses: addresses,
    allowedIps: allowedIps,
    dns: dns,
    mtu: mtu,
    candidates: candidates,
  );
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
  testWidgets('disabled chain entry shows status without a default protocol', (
    tester,
  ) async {
    final app = await hostChain(tester, ChainEngine());
    await tester.pumpWidget(
      workflowHost(
        app,
        home: Scaffold(
          body: ChainProxyEntry(controller: app, onOpen: () {}),
        ),
      ),
    );
    await tester.pumpAndSettle();
    expect(find.text('Not enabled'), findsOneWidget);
    expect(find.text('WireGuard'), findsNothing);
    expect(find.text('OpenVPN'), findsNothing);
  });
  testWidgets(
    'live and disconnecting sessions override a saved disabled selection',
    (tester) async {
      final app = await hostChain(tester, ChainEngine());
      for (final (phase, stage, expected) in [
        (ConnectionPhase.connected, 'connected', 'Connected'),
        (ConnectionPhase.disconnecting, 'connected', 'Disconnecting'),
        (ConnectionPhase.reconnecting, 'negotiating', 'Connecting'),
      ]) {
        app.snapshot = EngineSnapshot(
          phase: phase,
          chainExit: ChainExitStatus(stage: stage, currentProfile: imported),
        );
        await tester.pumpWidget(
          workflowHost(
            app,
            home: Scaffold(
              body: ChainProxyEntry(controller: app, onOpen: () {}),
            ),
          ),
        );
        await tester.pumpAndSettle();
        expect(find.text('Not enabled'), findsNothing);
        expect(find.textContaining(expected), findsOneWidget);
        expect(find.text('WireGuard'), findsOneWidget);
      }
      app.snapshot = const EngineSnapshot();
      app.localePreference = LocalePreference.simplifiedChinese;
      await tester.pumpWidget(
        workflowHost(
          app,
          home: Scaffold(
            body: ChainProxyEntry(controller: app, onOpen: () {}),
          ),
        ),
      );
      await tester.pumpAndSettle();
      expect(find.text('未启用'), findsOneWidget);
    },
  );

  testWidgets(
    'picker read errors remain visible after an earlier saved settings message',
    (tester) async {
      final engine = ChainEngine()
        ..pickerError = const EngineException(
          'CHAIN_FILE_READ_FAILED',
          'Read failed',
        );
      final app = await hostChain(tester, engine);
      await app.saveNetwork(
        app.activeProfile.copyWith(mtu: 1400),
        changedFields: ['mtu'],
      );
      await tester.pumpAndSettle();
      await tester.tap(find.text('Import file'));
      await tester.pumpAndSettle();
      expect(
        find.text('The configuration file could not be read.'),
        findsOneWidget,
      );
      expect(engine.actions.where((action) => action == 'import'), isEmpty);
    },
  );

  test('multi-endpoint capability is appended and absent on old replies', () {
    const codec = ControlCodec();
    for (final enabled in [false, true]) {
      final capabilities = ControlPayloadWriter();
      if (enabled) capabilities.boolean(37, true);
      final response = ControlPayloadWriter()
        ..string(1, 'cap')
        ..message(15, capabilities.takeBytes());
      expect(
        debugDecodeCapabilitiesFrame(
          codec.frame(response.takeBytes()),
          'cap',
        )!.chainOpenvpnMultiEndpoint,
        enabled,
      );
    }
  });

  test('candidate status decoding keeps the saved endpoint independent', () {
    final summary = <String, Object?>{
      'id': 'id',
      'revision': 'rev',
      'edit_revision': 'edit',
      'name': 'Multi',
      'protocol': 'openvpn_udp',
      'endpoint': {'host': 'first.example', 'port': 1194},
      'candidates': [
        {
          'endpoint': {'host': 'first.example', 'port': 1194},
          'ipv6': null,
        },
        {
          'endpoint': {'host': '2001:db8::1', 'port': 80},
          'ipv6': true,
        },
      ],
      'remote_random': true,
    };
    final status = ChainExitStatus.fromMap({
      'stage': 'negotiating',
      'current_profile': summary,
      'attempting_endpoint': {'host': '2001:db8::1', 'port': 80},
      'attempt_count': 2,
      'candidate_count': 2,
      'active_endpoint': '[2001:db8::1]:80',
      'attempt_failures': ['transport'],
    });
    expect(status.currentProfile!.host, 'first.example');
    expect(status.currentProfile!.candidates.last.ipv6, isTrue);
    expect(status.attemptingEndpoint!.label, '[2001:db8::1]:80');
    expect(status.attemptCount, 2);
    expect(
      status,
      isNot(
        ChainExitStatus(
          stage: 'negotiating',
          currentProfile: status.currentProfile,
        ),
      ),
    );
  });

  testWidgets(
    'old engines cannot preview-save or enable multiple endpoints but can disable them',
    (tester) async {
      const multi = ChainProfileSummary(
        id: 'multi',
        revision: 'r',
        editRevision: 'e',
        name: 'Multiple servers',
        protocol: 'openvpn_udp',
        host: 'first.example',
        port: 1194,
        candidates: [
          ChainEndpoint('first.example', 1194),
          ChainEndpoint('second.example', 1194),
        ],
      );
      final engine = ChainEngine()
        ..multiEndpoint = false
        ..previewProfile = multi
        ..library = [multi]
        ..picked = 'client';
      final app = await hostChain(tester, engine);
      await tester.tap(
        find.byKey(const ValueKey('chain-source-openvpn_custom')),
      );
      await tester.pumpAndSettle();
      await tester.tap(find.text('Import file'));
      await tester.pumpAndSettle();
      // A chosen file is checked as soon as the dialog opens.
      expect(engine.actions, contains('preview'));
      expect(
        find.text(
          'This configuration lists several servers. Update Usque to use it.',
        ),
        findsOneWidget,
      );
      expect(
        tester
            .widget<FilledButton>(
              find.widgetWithText(FilledButton, 'Save configuration'),
            )
            .onPressed,
        isNull,
      );
      expect(engine.actions.where((action) => action == 'import'), isEmpty);
      await tester.tap(find.text('Cancel'));
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(const ValueKey('chain-proxy-toggle')));
      await tester.pumpAndSettle();
      await tester.tap(find.text('Multiple servers'));
      await tester.pumpAndSettle();
      await tester.tap(find.text('Apply changes'));
      await tester.pumpAndSettle();
      expect(engine.saves, 0);
      expect(
        find.text(
          'This configuration lists several servers. Update Usque to use it.',
        ),
        findsOneWidget,
      );
      await tester.tap(find.byKey(const ValueKey('chain-proxy-toggle')));
      await tester.pumpAndSettle();
      await tester.tap(find.text('Apply changes'));
      await tester.pumpAndSettle();
      expect(engine.saves, 1);
      expect(app.activeProfile.chainExit!.enabled, isFalse);
    },
  );

  testWidgets(
    'action bar explains a blocked draft and protects a selected configuration',
    (tester) async {
      final engine = ChainEngine()..library = [imported];
      final app = await hostChain(tester, engine);
      expect(
        find.text('Enable chain proxy to select a configuration.'),
        findsOneWidget,
      );
      await tester.tap(find.byKey(const ValueKey('chain-proxy-toggle')));
      await tester.pumpAndSettle();
      expect(
        find.text('Select a saved configuration to apply.'),
        findsOneWidget,
      );
      expect(
        tester
            .widget<FilledButton>(
              find.widgetWithText(FilledButton, 'Apply changes'),
            )
            .onPressed,
        isNull,
      );
      await tester.tap(find.text('Office tunnel'));
      await tester.pumpAndSettle();
      expect(find.text('Pending selection: Office tunnel'), findsOneWidget);
      expect(find.textContaining('AllowedIPs'), findsOneWidget);
      expect(
        tester
            .widget<FilledButton>(
              find.widgetWithText(FilledButton, 'Apply changes'),
            )
            .onPressed,
        isNotNull,
      );
      await tester.tap(
        find.byKey(ValueKey('chain-profile-menu-${imported.id}')),
      );
      await tester.pumpAndSettle();
      await tester.tap(find.text('Delete'));
      await tester.pumpAndSettle();
      expect(
        find.text(
          'Choose another configuration or clear the saved selection before deleting this configuration.',
        ),
        findsOneWidget,
      );
      expect(engine.actions, isNot(contains('remove')));
      expect(engine.saves, 0);
      expect(app.activeProfile.chainExit!.enabled, isFalse);
    },
  );

  testWidgets('a WireGuard file pasted under OpenVPN is caught locally', (
    tester,
  ) async {
    final engine = ChainEngine();
    await hostChain(tester, engine);
    await tester.tap(find.byKey(const ValueKey('chain-source-openvpn_custom')));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Paste configuration'));
    await tester.pumpAndSettle();
    await tester.enterText(
      fieldWithLabel('Configuration text'),
      '[Interface]\nPrivateKey = fixture',
    );
    await tester.tap(find.text('Check configuration'));
    await tester.pumpAndSettle();
    expect(
      find.text(
        'This looks like a WireGuard configuration. Switch the exit source to WireGuard.',
      ),
      findsOneWidget,
    );
    expect(engine.actions, isNot(contains('preview')));
  });

  testWidgets('file import checks immediately and accepts a custom name', (
    tester,
  ) async {
    final engine = ChainEngine()
      ..picked =
          '[Interface]\nPrivateKey = fixture\n[Peer]\nEndpoint = vpn.example:51820\n';
    await hostChain(tester, engine);
    await tester.tap(find.text('Import file'));
    await tester.pumpAndSettle();
    // The check runs before the user has named anything, so it must not be
    // rejected for the empty name; the name is validated when saving.
    expect(engine.actions, contains('preview'));
    expect(find.byKey(const ValueKey('chain-import-error')), findsNothing);
    expect(find.text('Check configuration'), findsNothing);
    expect(
      find.text('Configuration loaded from file (4 lines).'),
      findsOneWidget,
    );
    final name = fieldWithLabel('Name');
    expect(tester.widget<TextField>(name).controller!.text, 'vpn.example');
    expect(find.textContaining('PrivateKey'), findsNothing);
    await tester.enterText(name, 'Office exit');
    await tester.tap(find.text('Save configuration'));
    await tester.pumpAndSettle();
    expect(engine.actions, contains('import'));
    expect(engine.names.last, 'Office exit');
    expect(find.byType(Dialog), findsNothing);
    expect(find.text('Office exit'), findsOneWidget);
  });

  testWidgets('an empty name is refused only when saving', (tester) async {
    final engine = ChainEngine()..picked = '[Interface]\nPrivateKey = fixture';
    await hostChain(tester, engine);
    await tester.tap(find.text('Import file'));
    await tester.pumpAndSettle();
    await tester.enterText(fieldWithLabel('Name'), '   ');
    await tester.tap(find.text('Save configuration'));
    await tester.pumpAndSettle();
    expect(engine.actions, isNot(contains('import')));
    expect(find.text('A required field is missing.'), findsOneWidget);
    expect(find.byType(Dialog), findsOneWidget);
  });

  testWidgets('connection failures and DNS gaps are explained inline', (
    tester,
  ) async {
    final engine = ChainEngine()..library = [imported];
    final app = await hostChain(tester, engine);
    engine.current = const EngineSnapshot(
      phase: ConnectionPhase.error,
      chainExit: ChainExitStatus(
        stage: 'error',
        currentProfile: imported,
        failure: 'transport',
        dnsUnavailable: true,
        attemptFailures: ['transport', 'address_changed'],
      ),
    );
    await app.refreshSnapshot();
    await tester.pumpAndSettle();
    expect(find.text('Reason: the connection dropped.'), findsOneWidget);
    expect(find.text('No DNS through this exit'), findsOneWidget);
    expect(
      find.text(
        'Failed attempts: the connection dropped, the server address changed',
      ),
      findsOneWidget,
    );
    expect(
      find.descendant(
        of: find.byKey(ValueKey('chain-profile-${imported.id}')),
        matching: find.text('Current connection'),
      ),
      findsOneWidget,
    );
  });

  test('error locations and localized chain copy resolve', () {
    final en = AppStrings(
      LocalePreference.english,
      systemLocale: const Locale('en'),
    );
    expect(
      en.chainError({
        'reason': 'unsupported_or_duplicate_field',
        'field': 'PostUp',
        'line': 7,
      }),
      'This field is unsupported or duplicated. (PostUp, line 7)',
    );
    expect(
      en.chainError({'reason': 'missing_field', 'field': '', 'line': 0}),
      'A required field is missing.',
    );
    for (final preference in [
      LocalePreference.traditionalChineseTaiwan,
      LocalePreference.traditionalChineseHongKong,
    ]) {
      final strings = AppStrings(preference, systemLocale: const Locale('en'));
      expect(strings.chain('title'), '鏈式代理');
      expect(
        strings.chainError({'reason': 'invalid_key', 'field': 'PublicKey'}),
        '金鑰必須是有效的 32 位元組 Base64 金鑰。（PublicKey）',
      );
    }
    expect(
      AppStrings(
        LocalePreference.japanese,
        systemLocale: const Locale('en'),
      ).chain('title'),
      'チェーンプロキシ',
    );
    expect(
      AppStrings(
        LocalePreference.german,
        systemLocale: const Locale('en'),
      ).chain('title'),
      'Kettenproxy',
    );
    final hongKong = AppStrings(
      LocalePreference.traditionalChineseHongKong,
      systemLocale: const Locale('en'),
    );
    final taiwan = AppStrings(
      LocalePreference.traditionalChineseTaiwan,
      systemLocale: const Locale('en'),
    );
    expect(hongKong.chain('address_family'), '地址族');
    expect(taiwan.chain('address_family'), '位址族');
    expect(hongKong.chain('username'), '用戶名稱');
    expect(taiwan.chain('username'), '使用者名稱');
    expect(hongKong.chain('addresses'), '隧道地址');
    expect(taiwan.chain('addresses'), '通道位址');
  });

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
    await tester.tap(find.byKey(const ValueKey('chain-source-vpn_gate')));
    await tester.pumpAndSettle();
    expect(find.text('WireGuard · Office tunnel'), findsOneWidget);
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
        'OpenVPN',
        'WireGuard',
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
      // The name defaulted to the endpoint host once the check passed.
      expect(engine.names.last, 'vpn.example');
      expect(find.text('vpn.example'), findsOneWidget);
      expect(app.activeProfile.chainExit, before);
      expect(engine.saves, 0);
      expect(app.snapshot.isConnected, isFalse);
    },
  );

  testWidgets(
    'L4 draft requires explicit CONNECT-IP apply and survives a source switch',
    (tester) async {
      final engine = ChainEngine()..library = [imported];
      final app = await hostChain(tester, engine, l4: true);
      await tester.tap(find.byKey(const ValueKey('chain-proxy-toggle')));
      await tester.pumpAndSettle();
      await tester.tap(find.text('Office tunnel'));
      await tester.pumpAndSettle();
      expect(engine.saves, 0);
      expect(app.activeProfile.dataPlane, DataPlaneMode.l4Proxy);
      expect(find.text('Requires CONNECT-IP'), findsOneWidget);
      // Switching sources is browsing: no discard prompt, and the page switch
      // travels with the user.
      await tester.tap(
        find.byKey(const ValueKey('chain-source-openvpn_custom')),
      );
      await tester.pumpAndSettle();
      expect(find.text(app.strings.get('discard_changes_title')), findsNothing);
      expect(
        tester
            .widget<ChoiceChip>(
              find.byKey(const ValueKey('chain-source-openvpn_custom')),
            )
            .selected,
        isTrue,
      );
      expect(
        tester
            .widget<SwitchListTile>(
              find.byKey(const ValueKey('chain-proxy-toggle')),
            )
            .value,
        isTrue,
      );
      // Merely looking at another source is not an edit.
      expect(
        tester
            .widget<FilledButton>(
              find.widgetWithText(FilledButton, 'Apply changes'),
            )
            .onPressed,
        isNull,
      );
      // Leaving the page still protects the selection made under WireGuard.
      await tester.binding.handlePopRoute();
      await tester.pumpAndSettle();
      expect(
        find.text(app.strings.get('discard_changes_title')),
        findsOneWidget,
      );
      await tester.tap(find.text(app.strings.get('keep_editing')));
      await tester.pumpAndSettle();
      await tester.tap(
        find.byKey(const ValueKey('chain-source-wireguard_custom')),
      );
      await tester.pumpAndSettle();
      expect(find.text(app.strings.get('discard_changes_title')), findsNothing);
      expect(find.text('Requires CONNECT-IP'), findsOneWidget);
      // The conflict and its resolution live in the action bar together.
      expect(
        find.text('This configuration requires CONNECT-IP.'),
        findsOneWidget,
      );
      final apply = find.text('Switch to CONNECT-IP and apply');
      expect(apply, findsOneWidget);
      await tester.tap(apply);
      await tester.pumpAndSettle();
      expect(engine.saves, 1);
      expect(app.activeProfile.dataPlane, DataPlaneMode.connectIp);
      expect(app.activeProfile.chainExit!.profileId, imported.id);
      expect(engine.fields, ['chain_exit', 'vpn_gate', 'data_plane']);
    },
  );

  testWidgets(
    'the page switch is shared with VPN Gate and switching back never prompts',
    (tester) async {
      final engine = ChainEngine()..library = [imported];
      final app = await hostChain(tester, engine);
      await tester.tap(find.byKey(const ValueKey('chain-proxy-toggle')));
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(const ValueKey('chain-source-vpn_gate')));
      await tester.pumpAndSettle();
      expect(find.text(app.strings.get('discard_changes_title')), findsNothing);
      final gateToggle = find.byKey(const ValueKey('vpn-gate-toggle'));
      expect(tester.widget<SwitchListTile>(gateToggle).value, isTrue);
      expect(
        tester.getTopLeft(gateToggle).dy,
        lessThan(
          tester
              .getTopLeft(find.byKey(const ValueKey('chain-source-vpn_gate')))
              .dy,
        ),
      );
      await tester.tap(gateToggle);
      await tester.pumpAndSettle();
      await tester.tap(
        find.byKey(const ValueKey('chain-source-wireguard_custom')),
      );
      await tester.pumpAndSettle();
      expect(find.text(app.strings.get('discard_changes_title')), findsNothing);
      expect(
        tester
            .widget<SwitchListTile>(
              find.byKey(const ValueKey('chain-proxy-toggle')),
            )
            .value,
        isFalse,
      );
      // Nothing is pending any more, so leaving asks nothing.
      await tester.binding.handlePopRoute();
      await tester.pumpAndSettle();
      expect(find.text(app.strings.get('discard_changes_title')), findsNothing);
      expect(engine.saves, 0);
    },
  );

  testWidgets(
    'source choices stack on phones, keep their icons clear and follow the switch',
    (tester) async {
      final app = await hostChain(tester, ChainEngine(), width: 390);
      final chips = [
        for (final source in ChainSource.values)
          find.byKey(ValueKey('chain-source-${source.wire}')),
      ];
      final lefts = chips.map((chip) => tester.getTopLeft(chip).dx).toSet();
      final tops = chips.map((chip) => tester.getTopLeft(chip).dy).toSet();
      expect(lefts, hasLength(1));
      expect(tops, hasLength(3));
      expect(
        tester.getTopLeft(find.byKey(const ValueKey('chain-proxy-toggle'))).dy,
        lessThan(tester.getTopLeft(chips.first).dy),
      );
      for (final chip in chips) {
        expect(tester.widget<ChoiceChip>(chip).showCheckmark, isFalse);
      }
      final selected = find.byKey(
        const ValueKey('chain-source-wireguard_custom'),
      );
      expect(
        find.descendant(of: selected, matching: find.byIcon(LucideIcons.check)),
        findsOneWidget,
      );
      expect(
        find.descendant(
          of: chips.first,
          matching: find.byIcon(LucideIcons.check),
        ),
        findsNothing,
      );
      tester.view.physicalSize = const Size(980, 1100);
      await tester.pumpWidget(
        workflowHost(app, home: ChainProxyScreen(controller: app)),
      );
      await tester.pumpAndSettle();
      expect(
        chips.map((chip) => tester.getTopLeft(chip).dy).toSet(),
        hasLength(1),
      );
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
