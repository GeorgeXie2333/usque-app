import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:shared_preferences/shared_preferences.dart';
import 'package:usque/core/app_strings.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/screens/home_screen.dart';
import 'package:usque/screens/vpn_gate_screen.dart';
import 'package:usque/services/control_codec.dart';
import 'package:usque/services/engine_client.dart';
import 'package:usque/state/app_controller.dart';
import 'package:usque/widgets/country_flag.dart';

import 'app_test.dart' show FakeEngineClient;
import 'ui_workflow_test.dart' show workflowHost;

const server = VpnGateServer(
  id: 'v1:node',
  ip: '203.0.113.7',
  hostname: 'volunteer.example',
  configSha256: 'config-one',
  countryCode: 'JP',
  countryName: 'Japan',
  score: 1200,
  pingMs: 25,
  speedBps: 25000000,
);

class GateEngine extends FakeEngineClient implements VpnGateClient {
  @override
  Future<EngineCapabilities?> getCapabilities() async =>
      const EngineCapabilities(
        vpnGateTcp: true,
        vpnGatePoolFavorites: true,
        networkSettingsApplication: true,
      );
  final favorites = <String, VpnGateServer>{};
  VpnGateNodeProgress nodeProgress = const VpnGateNodeProgress();
  bool holdPreparation = false;
  final nodeRequests = <VpnGateNodeRequest>[];
  @override
  Future<void> vpnGateNode(VpnGateNodeRequest request) async {
    nodeRequests.add(request);
    if (request.action == 'release') return;
    if (request.action == 'remove_favorite') {
      favorites.remove(request.serverId);
      return;
    }
    nodeProgress = VpnGateNodeProgress(
      operationId: request.operationId,
      serverId: request.serverId,
      configSha256: request.configSha256,
      stage: request.action == 'cancel'
          ? 'cancelled'
          : holdPreparation
          ? 'preparing'
          : 'complete',
    );
    if ((request.action == 'favorite' || request.action == 'update_favorite') &&
        !holdPreparation) {
      favorites[request.serverId] = VpnGateServer(
        id: server.id,
        ip: server.ip,
        hostname: server.hostname,
        configSha256: request.configSha256,
        countryCode: server.countryCode,
        favorite: VpnGateFavoriteMetadata(
          configSha256: request.configSha256,
          savedAt: DateTime(2026),
        ),
      );
    }
  }

  int refreshes = 0, cancellations = 0, saves = 0;
  String? country;
  List<String>? fields;
  Completer<VpnGateDirectory>? pending;
  bool failSave = false;
  DateTime? fetchedAt;
  List<VpnGateServer> nodes = const [server];

  @override
  Future<VpnGateDirectory> listVpnGate({
    String? countryCode,
    bool unknownCountry = false,
    int offset = 0,
    int limit = 50,
    bool favoritesOnly = false,
    bool statusOnly = false,
  }) async {
    country = countryCode;
    return pending?.future ??
        VpnGateDirectory(
          servers: favoritesOnly
              ? favorites.values.toList()
              : nodes.map((node) {
                  final favorite = favorites[node.id]?.favorite;
                  if (favorite == null) return node;
                  return VpnGateServer(
                    id: node.id,
                    ip: node.ip,
                    hostname: node.hostname,
                    configSha256: node.configSha256,
                    countryCode: node.countryCode,
                    countryName: node.countryName,
                    score: node.score,
                    pingMs: node.pingMs,
                    speedBps: node.speedBps,
                    favorite: favorite,
                  );
                }).toList(),
          favoriteCount: favorites.length,
          nodeProgress: nodeProgress,
          countries: const [
            VpnGateCountry(code: 'JP', name: 'Japan', count: 1),
          ],
          total: favoritesOnly ? favorites.length : nodes.length,
          fetchedAt: fetchedAt ?? DateTime.now(),
          refreshStage: 'complete',
          cached: true,
          savedServer: server,
        );
  }

  @override
  Future<void> refreshVpnGate({bool cancel = false}) async {
    if (cancel) {
      cancellations++;
    } else {
      refreshes++;
    }
  }

  @override
  Future<NetworkSettingsState> saveNetworkSettings(
    String operationId,
    String accountId,
    UsqueProfile values,
    List<String> changedFields,
  ) async {
    saves++;
    fields = changedFields;
    if (failSave) {
      throw const EngineException(
        'VPN_GATE_SELECTION_STALE',
        'Select the server again.',
      );
    }
    return super.saveNetworkSettings(
      operationId,
      accountId,
      values,
      changedFields,
    );
  }
}

Future<AppController> host(
  WidgetTester tester,
  GateEngine engine, {
  bool dark = false,
  double scale = 1,
  LocalePreference locale = LocalePreference.english,
  Size size = const Size(980, 1000),
}) async {
  SharedPreferences.setMockInitialValues({});
  tester.view.devicePixelRatio = 1;
  tester.view.physicalSize = size;
  addTearDown(tester.view.resetDevicePixelRatio);
  addTearDown(tester.view.resetPhysicalSize);
  final app = AppController(engine);
  await app.initialize();
  app.localePreference = locale;
  addTearDown(app.dispose);
  await tester.pumpWidget(
    workflowHost(
      app,
      dark: dark,
      scale: scale,
      home: VpnGateScreen(controller: app),
    ),
  );
  await tester.pumpAndSettle();
  return app;
}

void main() {
  testWidgets('failed Gate shows WARP disconnected with the connection error', (
    tester,
  ) async {
    final app = await host(tester, GateEngine());
    app.snapshot = const EngineSnapshot(
      phase: ConnectionPhase.error,
      vpnGate: VpnGateStatus(
        stage: 'error',
        warpStage: 'disconnected',
        failure: 'authentication',
        server: server,
      ),
    );
    await tester.pumpWidget(
      workflowHost(app, home: HomeScreen(controller: app)),
    );
    await tester.pumpAndSettle();
    expect(
      find.text('WARP: ${app.strings.get('disconnected')}'),
      findsOneWidget,
    );
    expect(find.text('WARP: ${app.strings.get('connecting')}'), findsNothing);
  });

  testWidgets(
    'favorites can be added and removed while the master switch stays off',
    (tester) async {
      final engine = GateEngine();
      final app = await host(tester, engine);
      final star = find.byKey(const ValueKey('vpn-gate-favorite-v1:node'));
      await tester.ensureVisible(star);
      await tester.tap(star);
      await tester.pumpAndSettle();
      expect(engine.favorites.length, 1);
      expect(app.activeProfile.vpnGate, const VpnGateSettings());
      expect(engine.saves, 0);
      final tab = find.byKey(const ValueKey('vpn-gate-favorites'));
      await tester.ensureVisible(tab);
      await tester.tap(tab);
      await tester.pumpAndSettle();
      expect(
        tester
            .widget<ListTile>(
              find.byKey(const ValueKey('vpn-gate-node-v1:node')),
            )
            .onTap,
        isNull,
      );
      engine.nodes = [];
      await tester.tap(find.text('Refresh list'));
      await tester.pump(const Duration(seconds: 1));
      await tester.pumpAndSettle();
      expect(
        find.byKey(const ValueKey('vpn-gate-node-v1:node')),
        findsOneWidget,
      );
      await tester.ensureVisible(star);
      await tester.tap(star);
      await tester.pumpAndSettle();
      expect(engine.favorites, isEmpty);
      expect(engine.saves, 0);
      await tester.pumpWidget(const SizedBox());
    },
  );

  testWidgets('updating a favorite retains the exact pending configuration', (
    tester,
  ) async {
    final engine = GateEngine();
    engine.favorites[server.id] = VpnGateServer(
      id: server.id,
      ip: server.ip,
      hostname: server.hostname,
      countryCode: server.countryCode,
      configSha256: server.configSha256,
      favorite: VpnGateFavoriteMetadata(
        configSha256: server.configSha256,
        savedAt: DateTime(2026),
        latestConfigSha256: 'config-two',
      ),
    );
    final app = await host(tester, engine);
    await tester.tap(find.byKey(const ValueKey('vpn-gate-favorites')));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const ValueKey('vpn-gate-toggle')));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const ValueKey('vpn-gate-node-v1:node')));
    await tester.pumpAndSettle();
    final update = find.text('Update saved configuration');
    await tester.ensureVisible(update);
    await tester.tap(update);
    await tester.pumpAndSettle();
    expect(engine.nodeRequests.map((r) => r.action), [
      'prepare',
      'update_favorite',
    ]);
    expect(engine.favorites[server.id]!.configSha256, 'config-two');
    expect(engine.saves, 0);
    final apply = find.byKey(const ValueKey('vpn-gate-apply'));
    await tester.ensureVisible(apply);
    await tester.tap(apply);
    await tester.pumpAndSettle();
    expect(app.activeProfile.vpnGate.configSha256, 'config-one');
    await tester.pumpWidget(const SizedBox());
  });

  testWidgets(
    'disconnect during preparation prevents a late save or reconnect',
    (tester) async {
      final engine = GateEngine()..holdPreparation = true;
      final app = await host(tester, engine);
      await tester.tap(find.byKey(const ValueKey('vpn-gate-toggle')));
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(const ValueKey('vpn-gate-node-v1:node')));
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(const ValueKey('vpn-gate-apply')));
      await tester.pump(const Duration(milliseconds: 100));
      expect(engine.nodeRequests.single.action, 'prepare');
      expect(engine.saves, 0);
      await app.disconnectForExit();
      final request = engine.nodeRequests.first;
      engine.nodeProgress = VpnGateNodeProgress(
        operationId: request.operationId,
        stage: 'complete',
      );
      await tester.pump(const Duration(milliseconds: 500));
      await tester.pumpAndSettle();
      expect(engine.saves, 0);
      expect(engine.nodeRequests.last.action, 'cancel');
      await tester.pumpWidget(const SizedBox());
    },
  );

  test(
    'pool, favorites and task metadata survive protobuf decoding without config bodies',
    () {
      const codec = ControlCodec();
      final node = ControlPayloadWriter()
        ..string(1, 'id')
        ..string(10, 'current-hash')
        ..message(
          12,
          (ControlPayloadWriter()
                ..string(2, '2026-09-13T00:00:00.000Z')
                ..boolean(3, true)
                ..string(4, 'unknown')
                ..boolean(7, true))
              .takeBytes(),
        )
        ..message(
          13,
          (ControlPayloadWriter()
                ..string(1, 'saved-hash')
                ..unsigned(2, 42)
                ..string(3, 'current-hash'))
              .takeBytes(),
        );
      final directory = ControlPayloadWriter()
        ..message(1, node.takeBytes())
        ..unsigned(12, 1)
        ..string(13, '2026-09-13T00:00:00.000Z')
        ..message(
          14,
          (ControlPayloadWriter()
                ..string(1, 'operation')
                ..string(4, 'preparing'))
              .takeBytes(),
        );
      final frame = codec.frame(
        (ControlPayloadWriter()
              ..string(1, 'r')
              ..message(23, directory.takeBytes()))
            .takeBytes(),
      );
      final result = codec.decodeResponse(frame, 'r').vpnGateDirectory!;
      expect(result.favoriteCount, 1);
      expect(result.servers.single.favorite!.configSha256, 'saved-hash');
      expect(result.servers.single.pool!.tcpStatus, 'unknown');
      expect(result.nodeProgress.running, isTrue);
      expect(result.sourceFetchedAt, DateTime.utc(2026, 9, 13));
    },
  );
  test(
    'old profiles keep Gate disabled and new settings round-trip without configuration bytes',
    () {
      final original = UsqueProfile.defaultProfile();
      final enabled = original.copyWith(
        vpnGate: const VpnGateSettings(enabled: true).copyWith(server: server),
      );
      expect(UsqueProfile.fromMap(original.toMap()).vpnGate.enabled, isFalse);
      expect(UsqueProfile.fromMap(enabled.toMap()).vpnGate, enabled.vpnGate);
      expect(networkSettingsChangedFields(original, enabled), ['vpn_gate']);
      const codec = ControlCodec();
      final response = codec.frame(
        (ControlPayloadWriter()
              ..string(1, 'r')
              ..message(
                22,
                (ControlPayloadWriter()
                      ..string(1, 'epoch')
                      ..message(5, codec.encodeProfile(enabled)))
                    .takeBytes(),
              ))
            .takeBytes(),
      );
      // JSON carries only an immutable ID/hash reference, never an .ovpn body.
      expect(
        enabled.toMap().toString(),
        isNot(contains('openvpn_config_base64')),
      );
      expect(
        codec
            .decodeResponse(response, 'r')
            .networkSettings!
            .storedProfile!
            .vpnGate,
        enabled.vpnGate,
      );
    },
  );

  test(
    'directory protobuf preserves absent metrics and bounds repeated records',
    () {
      const codec = ControlCodec();
      final node =
          (ControlPayloadWriter()
                ..string(1, server.id)
                ..string(3, server.ip)
                ..string(10, server.configSha256))
              .takeBytes();
      Uint8List frame(int count) {
        final directory = ControlPayloadWriter()
          ..unsigned(3, count)
          ..boolean(9, true)
          ..message(11, node);
        for (var i = 0; i < count; i++) {
          directory.message(1, node);
        }
        return codec.frame(
          (ControlPayloadWriter()
                ..string(1, 'r')
                ..message(23, directory.takeBytes()))
              .takeBytes(),
        );
      }

      final list = codec.decodeResponse(frame(1), 'r').vpnGateDirectory!;
      expect(list.servers.single.pingMs, isNull);
      expect(list.savedServer!.configSha256, server.configSha256);
      expect(list.cached, isTrue);
      expect(
        () => codec.decodeResponse(frame(101), 'r'),
        throwsA(isA<EngineException>()),
      );
    },
  );

  test('Gate translations cover all catalogs and preserve placeholders', () {
    expect(AppStrings.debugCatalogsAreComplete, isTrue);
    expect(AppStrings.debugUntranslatedFeatureKeys(), isEmpty);
    expect(AppStrings.debugPlaceholdersArePreserved, isTrue);
  });

  testWidgets(
    'cache opens without refresh; choosing only changes draft; explicit save pins ID and hash',
    (tester) async {
      final engine = GateEngine();
      final app = await host(tester, engine);
      expect(engine.refreshes, 0);
      expect(find.text('Connected server'), findsOneWidget);
      await tester.tap(find.byKey(const ValueKey('vpn-gate-node-v1:node')));
      await tester.pumpAndSettle();
      expect(engine.saves, 0);
      expect(app.activeProfile.vpnGate.hasSelection, isFalse);
      await tester.tap(find.byKey(const ValueKey('vpn-gate-toggle')));
      await tester.pumpAndSettle();
      expect(
        tester
            .widget<FilledButton>(find.byKey(const ValueKey('vpn-gate-apply')))
            .onPressed,
        isNull,
      );
      await tester.tap(find.byKey(const ValueKey('vpn-gate-node-v1:node')));
      await tester.pumpAndSettle();
      expect(engine.saves, 0);
      await tester.tap(find.byKey(const ValueKey('vpn-gate-apply')));
      await tester.pumpAndSettle();
      expect(engine.saves, 1);
      expect(engine.fields, ['vpn_gate']);
      expect(
        app.activeProfile.vpnGate,
        const VpnGateSettings(enabled: true).copyWith(server: server),
      );
      await tester.pumpWidget(const SizedBox());
    },
  );

  testWidgets('failed save preserves draft; closing cancels an owned refresh', (
    tester,
  ) async {
    final engine = GateEngine()..failSave = true;
    final app = await host(tester, engine);
    await tester.tap(find.byKey(const ValueKey('vpn-gate-toggle')));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const ValueKey('vpn-gate-node-v1:node')));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const ValueKey('vpn-gate-apply')));
    await tester.pumpAndSettle();
    expect(app.activeProfile.vpnGate.hasSelection, isFalse);
    expect(
      tester
          .widget<FilledButton>(find.byKey(const ValueKey('vpn-gate-apply')))
          .onPressed,
      isNotNull,
    );
    await tester.tap(find.text('Refresh list'));
    await tester.pump(const Duration(milliseconds: 100));
    expect(engine.refreshes, 1);
    await tester.pumpWidget(const SizedBox());
    await tester.pump();
    expect(engine.cancellations, 1);
  });

  testWidgets(
    'current and draft flags survive a missing catalog node and a disabled list',
    (tester) async {
      final engine = GateEngine();
      final app = await host(tester, engine);
      app.snapshot = const EngineSnapshot(
        phase: ConnectionPhase.connected,
        vpnGate: VpnGateStatus(
          stage: 'connected',
          server: VpnGateServer(
            id: 'current',
            ip: '203.0.113.9',
            hostname: 'connected.example',
            configSha256: 'current-config',
            countryCode: 'KR',
          ),
        ),
      );
      await tester.pumpWidget(
        workflowHost(app, home: VpnGateScreen(controller: app)),
      );
      await tester.pumpAndSettle();
      final toggle = find.byKey(const ValueKey('vpn-gate-toggle'));
      final node = find.byKey(const ValueKey('vpn-gate-node-v1:node'));
      await tester.tap(toggle);
      await tester.pumpAndSettle();
      await tester.tap(node);
      await tester.pumpAndSettle();
      await tester.tap(toggle);
      await tester.pumpAndSettle();
      expect(tester.widget<ListTile>(node).onTap, isNull);
      expect(
        tester
            .widget<CountryFlag>(
              find.descendant(of: node, matching: find.byType(CountryFlag)),
            )
            .enabled,
        isFalse,
      );
      final connected = tester
          .widgetList<CountryFlag>(find.byType(CountryFlag))
          .singleWhere((flag) => flag.countryCode == 'KR');
      expect(connected.enabled, isTrue);
      engine.nodes = [];
      await tester.tap(find.text('Refresh list'));
      await tester.pump(const Duration(seconds: 2));
      await tester.pumpAndSettle();
      expect(node, findsNothing);
      expect(find.text('JP · ${server.ip}'), findsOneWidget);
      expect(find.text('KR · 203.0.113.9'), findsOneWidget);
      expect(engine.saves, 0);
      await tester.pumpWidget(const SizedBox());
    },
  );

  testWidgets(
    'a background directory response cannot erase a stale-selection save error',
    (tester) async {
      final engine = GateEngine()..failSave = true;
      final app = await host(tester, engine);
      await tester.tap(find.byKey(const ValueKey('vpn-gate-toggle')));
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(const ValueKey('vpn-gate-node-v1:node')));
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(const ValueKey('vpn-gate-apply')));
      await tester.pumpAndSettle();
      expect(engine.saves, 1);
      final message = app.strings.get('gate_select_again');
      expect(find.text(message), findsOneWidget);
      await tester.tap(find.text('Refresh list'));
      await tester.pump(const Duration(seconds: 2));
      await tester.pumpAndSettle();
      expect(find.text(message), findsOneWidget);
      await tester.pumpWidget(const SizedBox());
    },
  );

  testWidgets(
    'phone at 200 percent supports light, dark, Chinese and keyboard activation',
    (tester) async {
      for (final dark in [false, true]) {
        for (final locale in [
          LocalePreference.english,
          LocalePreference.simplifiedChinese,
        ]) {
          final engine = GateEngine();
          final app = await host(
            tester,
            engine,
            dark: dark,
            scale: 2,
            locale: locale,
            size: const Size(390, 844),
          );
          final toggle = find.byKey(const ValueKey('vpn-gate-toggle'));
          await tester.ensureVisible(toggle);
          await tester.pumpAndSettle();
          await tester.tap(toggle);
          await tester.pumpAndSettle();
          final node = find.byKey(const ValueKey('vpn-gate-node-v1:node'));
          await tester.ensureVisible(node);
          await tester.pumpAndSettle();
          await tester.tap(node);
          await tester.pumpAndSettle();
          final apply = find.byKey(const ValueKey('vpn-gate-apply'));
          await tester.ensureVisible(apply);
          await tester.pumpAndSettle();
          expect(app.activeProfile.vpnGate.hasSelection, isFalse);
          Focus.of(
            tester.element(find.text(app.strings.get('gate_save'))),
          ).requestFocus();
          await tester.pump();
          await tester.sendKeyEvent(LogicalKeyboardKey.enter);
          await tester.pumpAndSettle();
          expect(engine.saves, 1);
          expect(tester.takeException(), isNull);
          await tester.pumpWidget(const SizedBox());
        }
      }
    },
  );
}
