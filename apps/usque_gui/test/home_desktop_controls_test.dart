import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:shared_preferences/shared_preferences.dart';
import 'package:usque/core/usque_theme.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/services/engine_client.dart';
import 'package:usque/state/app_controller.dart';
import 'package:usque/widgets/common.dart';
import 'package:usque/widgets/home_desktop_controls.dart';

import 'app_test.dart' show FakeEngineClient;

class _ControlsEngine extends FakeEngineClient {
  final edits = <(UsqueProfile, List<String>)>[];
  final operationIds = <String>[];
  Completer<void>? saveBarrier;
  Object? saveFailure;
  Object? queryFailure;
  NetworkSettingsApplyStatus applyStatus = NetworkSettingsApplyStatus.deferred;
  String? applyError;
  bool chainAvailable = true;

  @override
  Future<EngineCapabilities?> getCapabilities() async => EngineCapabilities(
    networkSettingsApplication: true,
    automaticEndpoints: true,
    vpnGateTcp: chainAvailable,
    chainProfileImport: chainAvailable,
    chainWireguard: chainAvailable,
    chainWarpWireguard: chainAvailable,
    chainHttpProxy: chainAvailable,
    chainSocks5Proxy: chainAvailable,
  );

  @override
  Future<NetworkSettingsState> saveNetworkSettings(
    String operationId,
    String accountId,
    UsqueProfile values,
    List<String> changedFields,
  ) async {
    edits.add((values, List.of(changedFields)));
    operationIds.add(operationId);
    await saveBarrier?.future;
    if (saveFailure case final failure?) throw failure;
    final result = await super.saveNetworkSettings(
      operationId,
      accountId,
      values,
      changedFields,
    );
    return settingsState = NetworkSettingsState(
      sourceEpoch: result.sourceEpoch,
      sequence: result.sequence,
      operationId: operationId,
      storedProfile: result.storedProfile,
      sharedNetwork: result.sharedNetwork,
      persisted: true,
      status: applyStatus,
      errorCode: applyError,
      deferredFields: applyStatus == NetworkSettingsApplyStatus.deferred
          ? changedFields
          : const [],
    );
  }

  @override
  Future<NetworkSettingsState> getNetworkSettingsState() async {
    if (queryFailure case final failure?) throw failure;
    return super.getNetworkSettingsState();
  }
}

Future<AppController> _app(
  WidgetTester tester,
  _ControlsEngine engine, {
  UsqueProfile? profile,
  VoidCallback? onOpen,
  double width = 320,
  double scale = 1,
  LocalePreference locale = LocalePreference.english,
  Brightness brightness = Brightness.light,
}) async {
  SharedPreferences.setMockInitialValues({'update_checks_enabled': false});
  engine.legacyProfilesImported = true;
  engine.storedProfiles = [profile ?? UsqueProfile.defaultProfile()];
  final app = AppController(engine);
  await app.initialize();
  app.localePreference = locale;
  addTearDown(app.dispose);
  await tester.pumpWidget(
    MaterialApp(
      theme: brightness == Brightness.light
          ? UsqueTheme.light()
          : UsqueTheme.dark(),
      home: MediaQuery(
        data: MediaQueryData(textScaler: TextScaler.linear(scale)),
        child: Scaffold(
          body: SingleChildScrollView(
            child: Align(
              alignment: Alignment.topLeft,
              child: SizedBox(
                width: width,
                child: HomeDesktopControls(
                  controller: app,
                  onOpenChainProxy: onOpen ?? () {},
                ),
              ),
            ),
          ),
        ),
      ),
    ),
  );
  await tester.pumpAndSettle();
  return app;
}

Finder _toggle(String kind) => find.byKey(ValueKey('home-$kind-switch'));

void main() {
  testWidgets(
    'TUN saves only its field and waits for confirmation',
    (tester) async {
      final engine = _ControlsEngine();
      final original = UsqueProfile.defaultProfile().copyWith(
        mtu: 1400,
        endpointPort: 8443,
        proxy: const ProxySettings(httpPort: 9090, systemProxy: true),
      );
      final app = await _app(tester, engine, profile: original);
      expect(tester.getSize(find.byType(ContentList)).height, 330);
      engine.saveBarrier = Completer();
      await tester.tap(_toggle('tun'));
      await tester.pump();
      expect(engine.edits, hasLength(1));
      expect(engine.edits.single.$2, ['frontends.tunnel']);
      expect(engine.edits.single.$1.frontends.tunnel, isFalse);
      expect(engine.edits.single.$1.mtu, 1400);
      expect(engine.edits.single.$1.endpointPort, 8443);
      expect(engine.edits.single.$1.proxy.httpPort, 9090);
      expect(engine.edits.single.$1.proxy.systemProxy, isTrue);
      expect(tester.widget<Switch>(_toggle('tun')).value, isTrue);
      expect(tester.widget<Switch>(_toggle('tun')).onChanged, isNull);
      expect(tester.widget<Switch>(_toggle('system-proxy')).onChanged, isNull);
      engine.saveBarrier!.complete();
      await tester.pumpAndSettle();
      expect(app.activeProfile.frontends.tunnel, isFalse);
      expect(tester.widget<Switch>(_toggle('tun')).value, isFalse);
      expect(engine.calls, isNot(contains('connect')));
      expect(engine.calls, isNot(contains('retry')));
      expect(engine.calls, isNot(contains('disconnect')));
    },
    variant: TargetPlatformVariant.only(TargetPlatform.windows),
  );

  testWidgets(
    'system proxy preserves listeners and requires HTTP',
    (tester) async {
      final engine = _ControlsEngine();
      final profile = UsqueProfile.defaultProfile().copyWith(
        proxy: const ProxySettings(httpPort: 9090, socksPort: 1180),
      );
      final app = await _app(tester, engine, profile: profile);
      await tester.tap(_toggle('system-proxy'));
      await tester.pumpAndSettle();
      expect(engine.edits.single.$2, ['proxy.system_proxy']);
      expect(app.activeProfile.proxy.systemProxy, isTrue);
      expect(app.activeProfile.proxy.httpPort, 9090);
      expect(app.activeProfile.proxy.socksPort, 1180);
      await tester.pumpWidget(const SizedBox());

      final blockedEngine = _ControlsEngine();
      final blocked = await _app(
        tester,
        blockedEngine,
        profile: profile.copyWith(
          frontends: profile.frontends.copyWith(http: false),
        ),
      );
      expect(tester.widget<Switch>(_toggle('system-proxy')).onChanged, isNull);
      expect(
        find.byTooltip(blocked.strings.get('home_system_proxy_requires_http')),
        findsOneWidget,
      );
      expect(blockedEngine.edits, isEmpty);
      expect(find.text(blocked.strings.get('home_options')), findsNWidgets(2));
      for (final text
          in find.text(blocked.strings.get('home_options')).evaluate()) {
        expect(text.findAncestorWidgetOfExactType<ActionRow>(), isNull);
        expect(text.findAncestorWidgetOfExactType<TextButton>(), isNull);
      }
    },
    variant: TargetPlatformVariant.only(TargetPlatform.windows),
  );

  for (final source in ChainSource.values.where(
    (s) => s != ChainSource.vpnGate,
  )) {
    testWidgets(
      '${source.name} shortcut retains selection and endpoint',
      (tester) async {
        final engine = _ControlsEngine();
        final selection = ChainExitSettings(
          source: source,
          profileId: 'saved-profile',
          revision: 'revision-1',
          endpointOverride: const ChainEndpoint('192.0.2.10', 443),
        );
        final gate = const VpnGateSettings(
          serverId: 'saved-gate',
          configSha256: 'saved-config',
        );
        final app = await _app(
          tester,
          engine,
          profile: UsqueProfile.defaultProfile().copyWith(
            chainExit: selection,
            vpnGate: gate,
          ),
        );
        await tester.tap(_toggle('chain-proxy'));
        await tester.pumpAndSettle();
        expect(engine.edits.single.$2, ['chain_exit']);
        expect(app.activeProfile.chainExit, selection.copyWith(enabled: true));
        expect(app.activeProfile.vpnGate, gate);
        await tester.tap(_toggle('chain-proxy'));
        await tester.pumpAndSettle();
        expect(app.activeProfile.chainExit, selection);
        expect(engine.edits.last.$2, ['chain_exit']);
        expect(engine.calls, isNot(contains('connect')));
      },
      variant: TargetPlatformVariant.only(TargetPlatform.windows),
    );
  }

  for (final legacy in [false, true]) {
    testWidgets(
      'VPN Gate shortcut retains selection (legacy: $legacy)',
      (tester) async {
        final engine = _ControlsEngine();
        const selection = VpnGateSettings(
          serverId: 'saved-server',
          configSha256: 'saved-sha256',
        );
        final app = await _app(
          tester,
          engine,
          profile: UsqueProfile.defaultProfile().copyWith(
            vpnGate: selection,
            chainExit: legacy
                ? null
                : const ChainExitSettings(source: ChainSource.vpnGate),
          ),
        );
        await tester.tap(_toggle('chain-proxy'));
        await tester.pumpAndSettle();
        expect(engine.edits.single.$2, ['vpn_gate', if (!legacy) 'chain_exit']);
        expect(app.activeProfile.vpnGate, selection.copyWith(enabled: true));
        expect(app.activeProfile.chainEnabled, isTrue);
        expect(
          app.activeProfile.chainExit?.source,
          legacy ? null : ChainSource.vpnGate,
        );
        await tester.tap(_toggle('chain-proxy'));
        await tester.pumpAndSettle();
        expect(app.activeProfile.vpnGate, selection);
        expect(app.activeProfile.chainEnabled, isFalse);
      },
      variant: TargetPlatformVariant.only(TargetPlatform.windows),
    );
  }

  testWidgets(
    'missing or unsupported chain opens settings without saving',
    (tester) async {
      for (final source in ChainSource.values) {
        var opened = 0;
        final engine = _ControlsEngine();
        final app = await _app(
          tester,
          engine,
          onOpen: () => opened++,
          profile: UsqueProfile.defaultProfile().copyWith(
            chainExit: ChainExitSettings(source: source),
          ),
        );
        await tester.tap(_toggle('chain-proxy'));
        await tester.pumpAndSettle();
        expect(opened, 1);
        expect(engine.edits, isEmpty);
        expect(app.activeProfile.chainEnabled, isFalse);
        await tester.pumpWidget(const SizedBox());
      }
      var opened = 0;
      final engine = _ControlsEngine()..chainAvailable = false;
      await _app(
        tester,
        engine,
        onOpen: () => opened++,
        profile: UsqueProfile.defaultProfile().copyWith(
          chainExit: const ChainExitSettings(
            source: ChainSource.httpProxy,
            profileId: 'saved-profile',
            revision: 'revision-1',
          ),
        ),
      );
      await tester.tap(_toggle('chain-proxy'));
      await tester.pumpAndSettle();
      expect(opened, 1);
      expect(engine.edits, isEmpty);
    },
    variant: TargetPlatformVariant.only(TargetPlatform.windows),
  );

  testWidgets(
    'save rejection retains switch; application failure is visible',
    (tester) async {
      final engine = _ControlsEngine();
      final app = await _app(tester, engine);
      engine.saveFailure = const EngineException(
        'NETWORK_SETTINGS_SAVE_FAILED',
        'failed',
      );
      await tester.tap(_toggle('tun'));
      await tester.pumpAndSettle();
      expect(app.activeProfile.frontends.tunnel, isTrue);
      expect(tester.widget<Switch>(_toggle('tun')).value, isTrue);
      expect(
        find.text(app.strings.get('settings_save_failed')),
        findsOneWidget,
      );
      engine.saveFailure = null;
      engine.applyStatus = NetworkSettingsApplyStatus.failed;
      await tester.tap(_toggle('tun'));
      await tester.pumpAndSettle();
      expect(app.activeProfile.frontends.tunnel, isFalse);
      expect(find.text(app.strings.get('settings_failed')), findsOneWidget);
      expect(
        find.byKey(const ValueKey('home-controls-reconnect')),
        findsOneWidget,
      );
      expect(find.text(app.strings.get('settings_applied')), findsNothing);
    },
    variant: TargetPlatformVariant.only(TargetPlatform.windows),
  );

  for (final phase in [
    ConnectionPhase.connected,
    ConnectionPhase.degraded,
    ConnectionPhase.disconnected,
  ]) {
    for (final withError in [true, false]) {
      testWidgets(
        'deferred save feedback ${phase.name} (with error: $withError)',
        (tester) async {
          final engine = _ControlsEngine()
            ..current = EngineSnapshot(phase: phase)
            ..applyError = withError
                ? 'NETWORK_SETTINGS_SYSTEM_PROXY_BUSY'
                : null;
          final app = await _app(tester, engine);
          await tester.tap(_toggle('system-proxy'));
          await tester.pumpAndSettle();
          expect(app.activeProfile.proxy.systemProxy, isTrue);
          expect(engine.edits.single.$2, ['proxy.system_proxy']);
          expect(
            find.text(app.strings.get('settings_deferred')),
            withError && phase != ConnectionPhase.disconnected
                ? findsOneWidget
                : findsNothing,
          );
          expect(
            find.byKey(const ValueKey('home-controls-reconnect')),
            findsNothing,
          );
          expect(engine.calls, isNot(contains('retry')));
          expect(engine.calls, isNot(contains('connect')));
          await tester.pumpWidget(const SizedBox());
        },
        variant: TargetPlatformVariant.only(TargetPlatform.windows),
      );
    }
  }

  testWidgets(
    'successful external settings save clears the home rejection message',
    (tester) async {
      final engine = _ControlsEngine();
      final app = await _app(tester, engine);
      engine.saveFailure = const EngineException(
        'NETWORK_SETTINGS_SAVE_FAILED',
        'failed',
      );
      await tester.tap(_toggle('tun'));
      await tester.pumpAndSettle();
      expect(
        find.text(app.strings.get('settings_save_failed')),
        findsOneWidget,
      );
      engine.saveFailure = null;
      expect(
        await app.saveNetwork(
          app.activeProfile.copyWith(mtu: 1400),
          changedFields: const ['mtu'],
        ),
        isTrue,
      );
      await tester.pumpAndSettle();
      expect(find.byKey(const ValueKey('home-controls-error')), findsNothing);
      expect(app.activeProfile.frontends.tunnel, isTrue);
      expect(engine.edits, hasLength(2));
    },
    variant: TargetPlatformVariant.only(TargetPlatform.windows),
  );

  testWidgets(
    'confirmed external save clears a local capability-check fallback',
    (tester) async {
      final engine = _ControlsEngine();
      final app = await _app(tester, engine);
      expect(
        await app.saveNetwork(
          app.activeProfile.copyWith(mtu: 1300),
          changedFields: const ['mtu'],
        ),
        isTrue,
      );
      await tester.pumpAndSettle();
      final previousSettings = app.networkSettings.state!;
      app.engineCapabilities = const EngineCapabilities(
        networkSettingsApplication: true,
      );
      await tester.tap(_toggle('tun'));
      await tester.pumpAndSettle();
      expect(
        find.text(app.strings.get('endpoint_unsupported')),
        findsOneWidget,
      );
      expect(engine.edits, hasLength(1));
      app.networkSettings.accept(
        NetworkSettingsState(
          sourceEpoch: previousSettings.sourceEpoch,
          sequence: ++engine.settingsSequence,
          operationId: previousSettings.operationId,
          sharedNetwork: app.activeProfile,
          persisted: true,
          status: NetworkSettingsApplyStatus.applied,
        ),
      );
      await tester.pumpAndSettle();
      expect(
        find.text(app.strings.get('endpoint_unsupported')),
        findsOneWidget,
      );
      app.engineCapabilities = await engine.getCapabilities();
      expect(
        await app.saveNetwork(
          app.activeProfile.copyWith(mtu: 1400),
          changedFields: const ['mtu'],
        ),
        isTrue,
      );
      await tester.pumpAndSettle();
      expect(find.byKey(const ValueKey('home-controls-error')), findsNothing);
      expect(app.activeProfile.frontends.tunnel, isTrue);
      expect(engine.edits, hasLength(2));
    },
    variant: TargetPlatformVariant.only(TargetPlatform.windows),
  );

  testWidgets(
    'ambiguous save disables shortcuts and refresh never replays it',
    (tester) async {
      final engine = _ControlsEngine();
      final app = await _app(tester, engine);
      engine.saveFailure = const EngineException(
        'ENGINE_REQUEST_TIMEOUT',
        'timeout',
      );
      engine.queryFailure = const EngineException(
        'ENGINE_IPC_UNAVAILABLE',
        'offline',
      );
      await tester.tap(_toggle('tun'));
      await tester.pumpAndSettle();
      expect(find.text(app.strings.get('settings_unknown')), findsOneWidget);
      expect(tester.widget<Switch>(_toggle('tun')).onChanged, isNull);
      expect(tester.widget<Switch>(_toggle('system-proxy')).onChanged, isNull);
      expect(engine.edits, hasLength(1));
      engine.queryFailure = null;
      engine.settingsState = NetworkSettingsState(
        sourceEpoch: engine.settingsEpoch,
        sequence: ++engine.settingsSequence,
        operationId: engine.operationIds.single,
        persisted: true,
        sharedNetwork: app.activeProfile.copyWith(
          frontends: app.activeProfile.frontends.copyWith(tunnel: false),
        ),
        status: NetworkSettingsApplyStatus.deferred,
      );
      await tester.tap(find.byKey(const ValueKey('home-controls-refresh')));
      await tester.pumpAndSettle();
      expect(engine.edits, hasLength(1));
      expect(app.activeProfile.frontends.tunnel, isFalse);
      expect(tester.widget<Switch>(_toggle('tun')).onChanged, isNotNull);
      expect(find.byKey(const ValueKey('home-controls-error')), findsNothing);
    },
    variant: TargetPlatformVariant.only(TargetPlatform.windows),
  );

  testWidgets(
    'busy, transition, recovery and applying states disable shortcuts',
    (tester) async {
      final engine = _ControlsEngine();
      engine.saveBarrier = Completer();
      final app = await _app(tester, engine);
      for (final snapshot in [
        const EngineSnapshot(phase: ConnectionPhase.preparing),
        const EngineSnapshot(phase: ConnectionPhase.disconnecting),
        const EngineSnapshot(
          phase: ConnectionPhase.error,
          errorCode: 'WINDOWS_RECOVERY_BLOCKED',
        ),
      ]) {
        engine.current = snapshot;
        await app.refreshSnapshot();
        await tester.pumpAndSettle();
        for (final kind in ['tun', 'system-proxy', 'chain-proxy']) {
          expect(tester.widget<Switch>(_toggle(kind)).onChanged, isNull);
        }
      }
      engine.current = const EngineSnapshot();
      await app.refreshSnapshot();
      app.busy = true;
      app.networkSettings.accept(
        const NetworkSettingsState(sourceEpoch: 'busy', sequence: 1),
      );
      await tester.pumpAndSettle();
      expect(tester.widget<Switch>(_toggle('tun')).onChanged, isNull);
      app.busy = false;
      app.networkSettings.accept(
        const NetworkSettingsState(
          sourceEpoch: 'busy',
          sequence: 2,
          status: NetworkSettingsApplyStatus.applying,
        ),
      );
      await tester.pumpAndSettle();
      expect(tester.widget<Switch>(_toggle('tun')).onChanged, isNull);
      app.networkSettings.supported = false;
      app.networkSettings.accept(
        const NetworkSettingsState(sourceEpoch: 'busy', sequence: 3),
      );
      await tester.pumpAndSettle();
      for (final kind in ['tun', 'system-proxy', 'chain-proxy']) {
        expect(tester.widget<Switch>(_toggle(kind)).onChanged, isNull);
      }
      expect(engine.edits, isEmpty);
    },
    variant: TargetPlatformVariant.only(TargetPlatform.windows),
  );

  testWidgets(
    'open layout, 200 percent text and keyboard work in both themes',
    (tester) async {
      for (final locale in [
        LocalePreference.english,
        LocalePreference.simplifiedChinese,
      ]) {
        for (final brightness in Brightness.values) {
          var opened = 0;
          final engine = _ControlsEngine();
          await _app(
            tester,
            engine,
            onOpen: () => opened++,
            width: 240,
            scale: 2,
            locale: locale,
            brightness: brightness,
          );
          expect(find.byType(Card), findsNothing);
          expect(find.byType(Panel), findsNothing);
          expect(tester.takeException(), isNull);
          await tester.sendKeyEvent(LogicalKeyboardKey.tab);
          await tester.sendKeyEvent(LogicalKeyboardKey.space);
          await tester.pumpAndSettle();
          expect(engine.edits.single.$2, ['frontends.tunnel']);
          await tester.ensureVisible(
            find.byKey(const ValueKey('home-chain-proxy-heading')),
          );
          await tester.tap(
            find.byKey(const ValueKey('home-chain-proxy-heading')),
          );
          await tester.pumpAndSettle();
          expect(opened, 1);
          await tester.ensureVisible(
            find.byKey(const ValueKey('home-chain-proxy-settings')),
          );
          await tester.tap(
            find.byKey(const ValueKey('home-chain-proxy-settings')),
          );
          await tester.pumpAndSettle();
          expect(opened, 2);
          expect(tester.takeException(), isNull);
          await tester.pumpWidget(const SizedBox());
        }
      }
    },
    variant: TargetPlatformVariant.only(TargetPlatform.windows),
  );
}
