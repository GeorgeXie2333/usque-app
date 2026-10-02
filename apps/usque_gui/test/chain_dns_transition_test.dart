import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:shared_preferences/shared_preferences.dart';
import 'package:usque/core/l10n/l4.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/screens/advanced_settings_screen.dart';
import 'package:usque/screens/chain_proxy_screen.dart';
import 'package:usque/screens/proxy_screen.dart';
import 'package:usque/services/engine_client.dart';
import 'package:usque/state/app_controller.dart';
import 'package:usque/state/network_settings_controller.dart';
import 'package:usque/widgets/home_desktop_controls.dart';
import 'package:usque/widgets/save_changes_bar.dart';

import 'chain_proxy_test.dart' show ChainEngine, chooseSource;
import 'ui_workflow_test.dart' show workflowHost;
import 'vpngate_test.dart' show server;

ChainExitSettings _selection(ChainSource source, {bool enabled = true}) =>
    ChainExitSettings(
      enabled: enabled,
      source: source,
      profileId: source == ChainSource.vpnGate
          ? null
          : '${source.wire}-profile',
      revision: source == ChainSource.vpnGate ? null : 'revision-1',
    );

UsqueProfile _initial({DataPlaneMode mode = DataPlaneMode.connectIp}) =>
    UsqueProfile.defaultProfile().copyWith(
      dataPlane: mode,
      chainExit: _selection(ChainSource.httpProxy),
      dnsIpv4: '9.9.9.9',
      dnsIpv6: '2620:fe::fe',
      proxy: const ProxySettings(
        dnsMode: ProxyDnsMode.edgeResolved,
        dnsIpv4: '8.8.8.8',
        dnsIpv6: '2001:4860:4860::8888',
      ),
    );

class _TransitionEngine extends ChainEngine {
  final edits = <(UsqueProfile, List<String>)>[];
  Completer<void>? firstSaveBarrier;
  bool rejectSave = false;

  @override
  Future<EngineCapabilities?> getCapabilities() async =>
      const EngineCapabilities(
        automaticEndpoints: true,
        networkSettingsApplication: true,
        l4Tcp: true,
        l4TunTcp: true,
        l4DnsConversion: true,
        vpnGateTcp: true,
        vpnGatePoolFavorites: true,
        chainProfileImport: true,
        chainWireguard: true,
        chainHttpProxy: true,
        chainSocks5Proxy: true,
      );

  @override
  Future<NetworkSettingsState> saveNetworkSettings(
    String operationId,
    String accountId,
    UsqueProfile values,
    List<String> changedFields,
  ) async {
    edits.add((values, List.of(changedFields)));
    final barrier = firstSaveBarrier;
    firstSaveBarrier = null;
    await barrier?.future;
    // Model the native validators so a widget test cannot pass by accepting
    // the invalid complete profile that the real wire decoder would reject.
    final invalidDns =
        values.proxy.dnsMode == ProxyDnsMode.edgeResolved &&
        values.dataPlane != DataPlaneMode.l4Proxy &&
        !(values.chainExit?.enabled == true && values.chainSource.isProxy);
    if (rejectSave || invalidDns) {
      throw const EngineException('CONFIGURATION_INVALID', 'Invalid settings');
    }
    return super.saveNetworkSettings(
      operationId,
      accountId,
      values,
      changedFields,
    );
  }
}

NetworkSettingsController _controller(
  _TransitionEngine engine,
  UsqueProfile profile,
) {
  engine.storedProfiles = [profile];
  final controller = NetworkSettingsController(engine)
    ..supported = true
    ..accept(
      NetworkSettingsState(
        sourceEpoch: engine.settingsEpoch,
        sequence: 0,
        storedProfile: profile,
        sharedNetwork: profile,
      ),
    );
  addTearDown(controller.dispose);
  return controller;
}

void _expectDnsAddresses(UsqueProfile actual, UsqueProfile original) {
  expect(actual.dnsIpv4, original.dnsIpv4);
  expect(actual.dnsIpv6, original.dnsIpv6);
  expect(actual.proxy.dnsIpv4, original.proxy.dnsIpv4);
  expect(actual.proxy.dnsIpv6, original.proxy.dnsIpv6);
}

Future<AppController> _host(
  WidgetTester tester,
  _TransitionEngine engine,
  UsqueProfile profile,
  Widget Function(AppController) page, {
  LocalePreference locale = LocalePreference.english,
}) async {
  SharedPreferences.setMockInitialValues({'update_checks_enabled': false});
  tester.view.devicePixelRatio = 1;
  tester.view.physicalSize = const Size(980, 1100);
  addTearDown(tester.view.resetDevicePixelRatio);
  addTearDown(tester.view.resetPhysicalSize);
  engine.legacyProfilesImported = true;
  engine.storedProfiles = [profile];
  engine.library = const [
    ChainProfileSummary(
      id: 'http_proxy-profile',
      revision: 'revision-1',
      editRevision: 'revision-1',
      name: 'Proxy',
      source: ChainSource.httpProxy,
      protocol: 'http_connect',
      host: 'proxy.example',
      port: 8080,
    ),
  ];
  final app = AppController(engine);
  await app.initialize();
  app.localePreference = locale;
  addTearDown(app.dispose);
  await tester.pumpWidget(workflowHost(app, home: page(app)));
  await tester.pumpAndSettle();
  return app;
}

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();

  for (final mode in DataPlaneMode.values) {
    for (final targetSource in ChainSource.values) {
      for (final enabled in [false, true]) {
        test('DNS follows $mode / $targetSource / enabled=$enabled', () async {
          final initial = _initial(mode: mode);
          final engine = _TransitionEngine();
          final controller = _controller(engine, initial);
          final target = initial.copyWith(
            chainExit: _selection(targetSource, enabled: enabled),
          );
          final fields = ['chain_exit'];
          expect(await controller.save(target, fields), isTrue);
          final supported =
              mode == DataPlaneMode.l4Proxy || enabled && targetSource.isProxy;
          expect(
            engine.edits.single.$1.proxy.dnsMode,
            supported ? ProxyDnsMode.edgeResolved : ProxyDnsMode.remote,
          );
          expect(engine.edits.single.$2, [
            'chain_exit',
            if (!supported) 'proxy.dns_mode',
          ]);
          expect(fields, ['chain_exit']);
          _expectDnsAddresses(engine.edits.single.$1, initial);
        });
      }
    }
  }

  test('unrelated edits and explicit DNS choices are not normalized', () async {
    final initial = _initial();
    final engine = _TransitionEngine();
    final controller = _controller(engine, initial);
    expect(await controller.save(initial.copyWith(mtu: 1400), ['mtu']), isTrue);
    expect(engine.edits.single.$1.proxy.dnsMode, ProxyDnsMode.edgeResolved);
    expect(engine.edits.single.$2, ['mtu']);
    final disabled = initial.copyWith(
      chainExit: initial.chainExit!.copyWith(enabled: false),
    );
    expect(
      await controller.save(disabled, ['chain_exit', 'proxy.dns_mode']),
      isFalse,
    );
    expect(engine.edits.last.$1.proxy.dnsMode, ProxyDnsMode.edgeResolved);
    expect(controller.saveError, 'CONFIGURATION_INVALID');
    for (final dns in [ProxyDnsMode.remote, ProxyDnsMode.system]) {
      expect(
        await controller.save(
          disabled.copyWith(proxy: disabled.proxy.copyWith(dnsMode: dns)),
          ['chain_exit', 'proxy.dns_mode'],
        ),
        isTrue,
      );
      expect(engine.edits.last.$1.proxy.dnsMode, dns);
      expect(engine.edits.last.$2, ['chain_exit', 'proxy.dns_mode']);
    }
  });

  test('stale exit draft preserves the confirmed DNS mode', () async {
    final stale = _initial();
    final current = stale.copyWith(
      proxy: stale.proxy.copyWith(dnsMode: ProxyDnsMode.system),
    );
    final engine = _TransitionEngine();
    final controller = _controller(engine, current);
    expect(
      await controller.save(
        stale.copyWith(chainExit: stale.chainExit!.copyWith(enabled: false)),
        ['chain_exit'],
      ),
      isTrue,
    );
    expect(engine.edits.single.$1.proxy.dnsMode, ProxyDnsMode.system);
    expect(engine.edits.single.$2, ['chain_exit']);
  });

  test(
    'stale mode draft cannot revive a removed chain in wire values',
    () async {
      final stale = _initial(mode: DataPlaneMode.l4Proxy);
      final current = stale.copyWith(clearChainExit: true);
      final engine = _TransitionEngine();
      final controller = _controller(engine, current);
      expect(
        await controller.save(
          stale.copyWith(dataPlane: DataPlaneMode.connectIp),
          ['data_plane'],
        ),
        isTrue,
      );
      expect(engine.edits.single.$1.chainExit, isNull);
      expect(engine.edits.single.$1.proxy.dnsMode, ProxyDnsMode.remote);
      expect(engine.edits.single.$2, ['data_plane', 'proxy.dns_mode']);
    },
  );

  test(
    'unrelated stale draft rebases exit and DNS without expanding its mask',
    () async {
      final stale = _initial();
      final current = stale.copyWith(
        chainExit: stale.chainExit!.copyWith(enabled: false),
        proxy: stale.proxy.copyWith(dnsMode: ProxyDnsMode.remote),
      );
      final engine = _TransitionEngine();
      final controller = _controller(engine, current);
      expect(await controller.save(stale.copyWith(mtu: 1400), ['mtu']), isTrue);
      final sent = engine.edits.single;
      expect(sent.$2, ['mtu']);
      expect(sent.$1.mtu, 1400);
      expect(sent.$1.chainEnabled, isFalse);
      expect(sent.$1.proxy.dnsMode, ProxyDnsMode.remote);
      _expectDnsAddresses(sent.$1, current);
    },
  );

  test(
    'explicit DNS edits still rebase unedited mode and exit dependencies',
    () async {
      final stale = _initial();
      final current = stale.copyWith(
        dataPlane: DataPlaneMode.l4Proxy,
        clearChainExit: true,
        proxy: stale.proxy.copyWith(dnsMode: ProxyDnsMode.remote),
      );
      final engine = _TransitionEngine();
      final controller = _controller(engine, current);
      expect(await controller.save(stale, ['proxy.dns_mode']), isTrue);
      final sent = engine.edits.single;
      expect(sent.$2, ['proxy.dns_mode']);
      expect(sent.$1.chainExit, isNull);
      expect(sent.$1.dataPlane, DataPlaneMode.l4Proxy);
      expect(sent.$1.proxy.dnsMode, ProxyDnsMode.edgeResolved);
    },
  );

  for (final firstChangesMode in [false, true]) {
    test(
      'queued exit edit observes preceding save: $firstChangesMode',
      () async {
        final initial = _initial();
        final barrier = Completer<void>();
        final engine = _TransitionEngine()..firstSaveBarrier = barrier;
        final controller = _controller(engine, initial);
        final first = controller.save(
          firstChangesMode
              ? initial.copyWith(dataPlane: DataPlaneMode.l4Proxy)
              : initial.copyWith(
                  proxy: initial.proxy.copyWith(dnsMode: ProxyDnsMode.system),
                ),
          [firstChangesMode ? 'data_plane' : 'proxy.dns_mode'],
        );
        final second = controller.save(
          initial.copyWith(
            chainExit: initial.chainExit!.copyWith(enabled: false),
          ),
          ['chain_exit'],
        );
        await Future<void>.delayed(Duration.zero);
        expect(engine.edits, hasLength(1));
        barrier.complete();
        expect(await first, isTrue);
        expect(await second, isTrue);
        final sent = engine.edits.last;
        expect(sent.$2, ['chain_exit']);
        expect(
          sent.$1.proxy.dnsMode,
          firstChangesMode ? ProxyDnsMode.edgeResolved : ProxyDnsMode.system,
        );
        expect(
          sent.$1.dataPlane,
          firstChangesMode ? DataPlaneMode.l4Proxy : DataPlaneMode.connectIp,
        );
      },
    );
  }

  test(
    'failed normalized save leaves confirmed exit and DNS untouched',
    () async {
      final initial = _initial();
      final engine = _TransitionEngine()..rejectSave = true;
      final controller = _controller(engine, initial);
      expect(
        await controller.save(
          initial.copyWith(
            chainExit: initial.chainExit!.copyWith(enabled: false),
          ),
          ['chain_exit'],
        ),
        isFalse,
      );
      expect(engine.edits.single.$1.proxy.dnsMode, ProxyDnsMode.remote);
      expect(
        controller.state!.sharedNetwork!.proxy.dnsMode,
        ProxyDnsMode.edgeResolved,
      );
      expect(controller.state!.sharedNetwork!.chainEnabled, isTrue);
    },
  );

  for (final locale in [
    LocalePreference.english,
    LocalePreference.simplifiedChinese,
  ]) {
    for (final entry in ['chain', 'gate', 'home']) {
      testWidgets(
        '$entry applies inherited DNS without another prompt ($locale)',
        (tester) async {
          final initial = _initial();
          final engine = _TransitionEngine();
          final app = await _host(
            tester,
            engine,
            initial,
            (app) => entry == 'home'
                ? Scaffold(
                    body: HomeDesktopControls(
                      controller: app,
                      onOpenChainProxy: () {},
                    ),
                  )
                : ChainProxyScreen(controller: app),
            locale: locale,
          );
          if (entry == 'home') {
            await tester.tap(
              find.byKey(const ValueKey('home-chain-proxy-switch')),
            );
          } else {
            if (entry == 'gate') {
              await chooseSource(tester, ChainSource.vpnGate);
              final node = find.byKey(ValueKey('vpn-gate-node-${server.id}'));
              await tester.ensureVisible(node);
              await tester.tap(node);
            } else {
              await tester.tap(
                find.byKey(const ValueKey('chain-proxy-toggle')),
              );
            }
            await tester.pumpAndSettle();
            tester
                .widget<SaveChangesBar>(find.byType(SaveChangesBar))
                .onSave!();
          }
          await tester.pumpAndSettle();
          expect(find.byType(AlertDialog), findsNothing);
          expect(engine.edits, hasLength(1));
          expect(engine.edits.single.$2, [
            if (entry == 'gate') 'vpn_gate',
            'chain_exit',
            if (entry == 'chain') 'vpn_gate',
            'proxy.dns_mode',
          ]);
          expect(app.activeProfile.proxy.dnsMode, ProxyDnsMode.remote);
          expect(app.activeProfile.chainEnabled, entry == 'gate');
          _expectDnsAddresses(app.activeProfile, initial);
          expect(tester.takeException(), isNull);
        },
      );
    }
  }

  testWidgets(
    'leaving L4 uses the same DNS transition at the settings boundary',
    (tester) async {
      final initial = _initial(
        mode: DataPlaneMode.l4Proxy,
      ).copyWith(clearChainExit: true);
      final engine = _TransitionEngine();
      final app = await _host(
        tester,
        engine,
        initial,
        (app) => AdvancedSettingsScreen(controller: app),
      );
      tester
          .widget<SegmentedButton<String>>(
            find.byType(SegmentedButton<String>).first,
          )
          .onSelectionChanged!({'automatic'});
      await tester.pumpAndSettle();
      tester.widget<SaveChangesBar>(find.byType(SaveChangesBar)).onSave!();
      await tester.pumpAndSettle();
      expect(engine.edits, hasLength(1));
      expect(
        engine.edits.single.$2,
        containsAll(['data_plane', 'proxy.dns_mode']),
      );
      expect(app.activeProfile.proxy.dnsMode, ProxyDnsMode.remote);
      _expectDnsAddresses(app.activeProfile, initial);
      final mtu = find.byWidgetPredicate(
        (widget) =>
            widget is TextField &&
            widget.decoration?.labelText == app.strings.get('mtu'),
      );
      await tester.ensureVisible(mtu);
      await tester.enterText(mtu, '1400');
      await tester.pumpAndSettle();
      tester.widget<SaveChangesBar>(find.byType(SaveChangesBar)).onSave!();
      await tester.pumpAndSettle();
      expect(engine.edits, hasLength(2));
      expect(engine.edits.last.$2, ['mtu']);
      expect(engine.edits.last.$1.proxy.dnsMode, ProxyDnsMode.remote);
      expect(tester.takeException(), isNull);
    },
  );

  for (final proxyPage in [false, true]) {
    testWidgets(
      'unrelated form draft survives an external exit change: proxy=$proxyPage',
      (tester) async {
        final initial = _initial();
        final engine = _TransitionEngine();
        final app = await _host(
          tester,
          engine,
          initial,
          (app) => proxyPage
              ? Scaffold(body: ProxyScreen(controller: app))
              : AdvancedSettingsScreen(controller: app),
        );
        final label = app.strings.get(proxyPage ? 'port' : 'mtu');
        final input = find
            .byWidgetPredicate(
              (widget) =>
                  widget is TextField && widget.decoration?.labelText == label,
            )
            .first;
        await tester.ensureVisible(input);
        await tester.enterText(input, proxyPage ? '2080' : '1400');
        await tester.pumpAndSettle();
        expect(
          tester.widget<SaveChangesBar>(find.byType(SaveChangesBar)).dirty,
          isTrue,
        );
        expect(
          await app.saveNetwork(
            app.activeProfile.copyWith(
              chainExit: initial.chainExit!.copyWith(enabled: false),
              proxy: app.activeProfile.proxy.copyWith(
                dnsMode: ProxyDnsMode.remote,
              ),
            ),
            // Model a completed edit on another surface independently of the
            // automatic transition under test, so the stale form itself goes red.
            changedFields: ['chain_exit', 'proxy.dns_mode'],
          ),
          isTrue,
        );
        await tester.pumpAndSettle();
        expect(app.activeProfile.proxy.dnsMode, ProxyDnsMode.remote);
        expect(
          tester.widget<TextField>(input).controller!.text,
          proxyPage ? '2080' : '1400',
        );
        tester.widget<SaveChangesBar>(find.byType(SaveChangesBar)).onSave!();
        await tester.pumpAndSettle();
        expect(engine.edits, hasLength(2));
        expect(engine.edits.last.$2, [
          proxyPage ? 'proxy.socks5_listeners' : 'mtu',
        ]);
        expect(engine.edits.last.$1.proxy.dnsMode, ProxyDnsMode.remote);
        expect(app.activeProfile.chainEnabled, isFalse);
        expect(app.activeProfile.proxy.dnsMode, ProxyDnsMode.remote);
        expect(
          tester.widget<SaveChangesBar>(find.byType(SaveChangesBar)).dirty,
          isFalse,
        );
        _expectDnsAddresses(app.activeProfile, initial);
        expect(tester.takeException(), isNull);
      },
    );
  }

  test(
    'server-resolution copy does not name a specific provider or data plane',
    () {
      expect(kL4En['proxy_dns_edge_resolved'], 'Resolve at the proxy server');
      expect(kL4ZhCn['proxy_dns_edge_resolved'], '由代理服务器解析');
      for (final catalog in kL4Catalogs.values) {
        for (final key in ['proxy_dns_edge_resolved', 'l4_edge_requires_l4']) {
          expect(catalog[key], isNot(contains('Cloudflare')));
          expect(catalog[key], isNot(contains('L4')));
        }
      }
    },
  );
}
