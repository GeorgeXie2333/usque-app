import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';
import 'package:usque/core/app_strings.dart';
import 'package:usque/core/usque_theme.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/models/network_settings.dart';
import 'package:usque/screens/advanced_settings_screen.dart';
import 'package:usque/screens/diagnostics_screen.dart';
import 'package:usque/screens/home_screen.dart';
import 'package:usque/screens/onboarding_screen.dart';
import 'package:usque/screens/shell_screen.dart';
import 'package:usque/screens/vpn_gate_screen.dart';
import 'package:usque/state/app_controller.dart';
import 'package:usque/state/network_quality_controller.dart';
import 'package:usque/state/window_frame.dart';
import 'package:usque/widgets/chain_proxy_entry.dart';
import 'package:usque/widgets/common.dart';
import 'package:usque/widgets/connection_ring.dart';
import 'package:usque/widgets/country_flag.dart';
import 'package:usque/widgets/usque_dialog.dart';
import 'package:usque/widgets/usque_logo.dart';
import 'package:usque/widgets/vpn_gate_entry.dart';
import 'package:usque/widgets/vpn_gate_server_row.dart';
import 'package:usque/widgets/warp_dns_editor.dart';
import 'package:usque/widgets/window_titlebar.dart';
import 'package:usque/widgets/zero_trust_endpoint_warning.dart';

import 'quality_test_support.dart' show qualityFixture;
import 'ui_workflow_test.dart' show WorkflowEngine, workflowHost;
import 'vpn_gate_server_row_test.dart' show observationNow, observationServer;
import 'vpn_gate_summary_test.dart' show current;
import 'vpngate_test.dart' show GateEngine, server;
import 'zero_trust_endpoint_test.dart' show ztEngine;

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
    for (final family in <String, List<String>>{
      'SpaceGrotesk': ['Medium', 'SemiBold', 'Bold'],
      'Manrope': ['Regular', 'Medium', 'SemiBold', 'Bold'],
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
      await (FontLoader('Tahoma')..addFont(
            SynchronousFuture(
              ByteData.sublistView(
                File(r'C:\Windows\Fonts\tahoma.ttf').readAsBytesSync(),
              ),
            ),
          ))
          .load();
      await (FontLoader('Microsoft YaHei UI')..addFont(
            SynchronousFuture(
              ByteData.sublistView(
                File(r'C:\Windows\Fonts\msyh.ttc').readAsBytesSync(),
              ),
            ),
          ))
          .load();
    }
  });

  for (final fixture in [
    (
      name: 'zt_warning_desktop_dark',
      locale: LocalePreference.english,
      size: const Size(1280, 900),
      dark: true,
      rtl: false,
    ),
    (
      name: 'zt_warning_phone_light',
      locale: LocalePreference.simplifiedChinese,
      size: const Size(375, 812),
      dark: false,
      rtl: false,
    ),
    (
      name: 'zt_warning_persian_landscape',
      locale: LocalePreference.persian,
      size: const Size(812, 375),
      dark: true,
      rtl: true,
    ),
  ]) {
    testWidgets('ZT warning golden ${fixture.name}', (tester) async {
      tester.view.devicePixelRatio = 1;
      tester.view.physicalSize = fixture.size;
      addTearDown(tester.view.resetDevicePixelRatio);
      addTearDown(tester.view.resetPhysicalSize);
      final boundary = GlobalKey();
      await tester.pumpWidget(
        RepaintBoundary(
          key: boundary,
          child: MaterialApp(
            debugShowCheckedModeBanner: false,
            theme: fixture.dark ? UsqueTheme.dark() : UsqueTheme.light(),
            home: Directionality(
              textDirection: fixture.rtl
                  ? TextDirection.rtl
                  : TextDirection.ltr,
              child: ZeroTrustEndpointWarning(
                strings: AppStrings(fixture.locale),
              ),
            ),
          ),
        ),
      );
      await tester.pumpAndSettle();
      expect(tester.takeException(), isNull);
      await expectLater(
        find.byKey(boundary),
        matchesGoldenFile('goldens/${fixture.name}.png'),
      );
    }, tags: 'golden');
  }

  for (final fixture in [
    (
      name: 'zt_home_risk_desktop_dark',
      locale: LocalePreference.english,
      size: const Size(1280, 900),
      dark: true,
      rtl: false,
    ),
    (
      name: 'zt_home_risk_phone_light',
      locale: LocalePreference.simplifiedChinese,
      size: const Size(375, 812),
      dark: false,
      rtl: false,
    ),
    (
      name: 'zt_home_risk_persian_landscape',
      locale: LocalePreference.persian,
      size: const Size(812, 375),
      dark: true,
      rtl: true,
    ),
  ]) {
    testWidgets('ZT Home risk golden ${fixture.name}', (tester) async {
      tester.view.devicePixelRatio = 1;
      tester.view.physicalSize = fixture.size;
      addTearDown(tester.view.resetDevicePixelRatio);
      addTearDown(tester.view.resetPhysicalSize);
      final engine = ztEngine(custom: true);
      final app = AppController(engine)
        ..profiles = engine.storedProfiles
        ..activeProfileId = engine.storedActiveProfileId
        ..profileIdentityStatuses = engine.storedIdentityStatuses
        ..localePreference = fixture.locale;
      try {
        final boundary = GlobalKey();
        await tester.pumpWidget(
          RepaintBoundary(
            key: boundary,
            child: workflowHost(
              app,
              dark: fixture.dark,
              home: Directionality(
                textDirection: fixture.rtl
                    ? TextDirection.rtl
                    : TextDirection.ltr,
                child: HomeScreen(controller: app),
              ),
            ),
          ),
        );
        await tester.pumpAndSettle();
        expect(
          find.byKey(const ValueKey('home-zero-trust-endpoint-risk')),
          findsOneWidget,
        );
        expect(tester.takeException(), isNull);
        await expectLater(
          find.byKey(boundary),
          matchesGoldenFile('goldens/${fixture.name}.png'),
        );
        await tester.pumpWidget(const SizedBox.shrink());
      } finally {
        app.dispose();
      }
    }, tags: 'golden');
  }

  for (final doh in [true, false]) {
    testWidgets('compact WARP DNS ${doh ? 'doh_en_light' : 'dot_zh_dark'}', (
      tester,
    ) async {
      tester.view.devicePixelRatio = 1;
      tester.view.physicalSize = const Size(420, 650);
      addTearDown(tester.view.resetDevicePixelRatio);
      addTearDown(tester.view.resetPhysicalSize);
      final boundary = GlobalKey();
      final strings = AppStrings(
        doh ? LocalePreference.english : LocalePreference.simplifiedChinese,
      );
      await tester.pumpWidget(
        RepaintBoundary(
          key: boundary,
          child: MaterialApp(
            debugShowCheckedModeBanner: false,
            theme: doh ? UsqueTheme.light() : UsqueTheme.dark(),
            home: Scaffold(
              body: SingleChildScrollView(
                padding: const EdgeInsets.all(24),
                child: Form(
                  child: ContentSection(
                    icon: LucideIcons.network,
                    title: strings.get('ip_dns'),
                    children: [
                      WarpDnsEditor(
                        value: WarpDnsSettings(
                          mode: doh ? WarpDnsMode.doh : WarpDnsMode.dot,
                          serverName: 'dns.example.com',
                          dohPath: doh ? '/dns-query' : '',
                          port: doh ? 443 : 853,
                          bootstrapIps: const ['192.0.2.1', '2001:db8::1'],
                        ),
                        enabled: true,
                        strings: strings,
                        onChanged: (_) {},
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
      expect(find.text(strings.get('nq_dns_scope')), findsNothing);
      expect(find.text(strings.get('nq_dns_no_fallback')), findsNothing);
      await expectLater(
        find.byKey(boundary),
        matchesGoldenFile(
          'goldens/warp_dns_${doh ? 'doh_en_light' : 'dot_zh_dark'}.png',
        ),
      );
    }, tags: 'golden');
  }

  for (final custom in [false, true]) {
    testWidgets('endpoint selection ${custom ? 'custom_zh' : 'automatic_en'}', (
      tester,
    ) async {
      tester.view.devicePixelRatio = 1;
      tester.view.physicalSize = const Size(390, 1000);
      addTearDown(tester.view.resetDevicePixelRatio);
      addTearDown(tester.view.resetPhysicalSize);
      final app = AppController(WorkflowEngine())
        ..localePreference = custom
            ? LocalePreference.simplifiedChinese
            : LocalePreference.english
        ..engineCapabilities = const EngineCapabilities(
          automaticEndpoints: true,
          h3CongestionControlAlgorithms: CongestionControlAlgorithm.values,
        );
      app.sharedNetwork = app.sharedNetwork.copyWith(
        endpointSelection: custom
            ? EndpointSelection.custom
            : EndpointSelection.automatic,
      );
      addTearDown(app.dispose);
      final boundary = GlobalKey();
      await tester.pumpWidget(
        RepaintBoundary(
          key: boundary,
          child: workflowHost(
            app,
            dark: custom,
            home: AdvancedSettingsScreen(controller: app),
          ),
        ),
      );
      await tester.pumpAndSettle();
      final selector = find.byKey(const ValueKey('endpoint-selection'));
      await tester.scrollUntilVisible(
        selector,
        200,
        scrollable: find.byType(Scrollable).first,
      );
      await Scrollable.ensureVisible(tester.element(selector), alignment: 0.15);
      await tester.pumpAndSettle();
      expect(tester.takeException(), isNull);
      await expectLater(
        find.byKey(boundary),
        matchesGoldenFile(
          'goldens/endpoint_${custom ? 'custom_zh_dark' : 'automatic_en_light'}.png',
        ),
      );
    }, tags: 'golden');
  }

  testWidgets('QUIC traffic policy phone and TV layouts', (tester) async {
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetDevicePixelRatio);
    addTearDown(tester.view.resetPhysicalSize);
    for (final tv in [false, true]) {
      tester.view.physicalSize = tv
          ? const Size(1280, 1000)
          : const Size(390, 1000);
      final app = AppController(WorkflowEngine())
        ..engineCapabilities = const EngineCapabilities(
          automaticEndpoints: true,
          networkSettingsApplication: true,
          applicationQuicBlocking: true,
        )
        ..localePreference = tv
            ? LocalePreference.simplifiedChinese
            : LocalePreference.english;
      final boundary = GlobalKey();
      try {
        await tester.pumpWidget(
          RepaintBoundary(
            key: boundary,
            child: workflowHost(
              app,
              dark: tv,
              scale: tv ? 2 : 1,
              home: AdvancedSettingsScreen(controller: app),
            ),
          ),
        );
        await tester.pumpAndSettle();
        await tester.ensureVisible(
          find.byKey(const ValueKey('disable-quic-switch')),
        );
        await tester.pumpAndSettle();
        if (tv) {
          await tester.tap(find.byKey(const ValueKey('disable-quic-switch')));
          await tester.pumpAndSettle();
          expect(
            tester
                .widget<SwitchListTile>(
                  find.byKey(const ValueKey('disable-quic-switch')),
                )
                .value,
            isTrue,
          );
        }
        await tester.pumpAndSettle();
        expect(tester.takeException(), isNull);
        await expectLater(
          find.byKey(boundary),
          matchesGoldenFile(
            'goldens/quic_${tv ? 'tv_dark' : 'phone_light'}.png',
          ),
        );
        await tester.pumpWidget(const SizedBox.shrink());
      } finally {
        app.dispose();
      }
    }
  }, tags: 'golden');

  testWidgets('VPN Gate proxy entry on desktop and phone', (tester) async {
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetDevicePixelRatio);
    addTearDown(tester.view.resetPhysicalSize);
    for (final phone in [false, true]) {
      tester.view.physicalSize = Size(phone ? 375 : 880, 600);
      final app = AppController(GateEngine())
        ..section = AppSection.proxy
        ..localePreference = phone
            ? LocalePreference.simplifiedChinese
            : LocalePreference.english
        ..snapshot = const EngineSnapshot(
          phase: ConnectionPhase.connected,
          vpnGate: VpnGateStatus(stage: 'connected', server: server),
        );
      final boundary = GlobalKey();
      try {
        await tester.pumpWidget(
          workflowHost(
            app,
            dark: phone,
            home: Scaffold(
              body: Padding(
                padding: const EdgeInsets.all(16),
                child: RepaintBoundary(
                  key: boundary,
                  child: VpnGateEntry(controller: app, onOpen: () {}),
                ),
              ),
            ),
          ),
        );
        await tester.runAsync(
          () => precacheImage(
            const AssetImage('assets/flags/w80/jp.png'),
            tester.element(find.byType(VpnGateEntry)),
          ),
        );
        await tester.pumpAndSettle();
        expect(tester.takeException(), isNull);
        await expectLater(
          find.byKey(boundary),
          matchesGoldenFile(
            'goldens/proxy_gate_${phone ? 'phone_dark' : 'desktop_light'}.png',
          ),
        );
        await tester.pumpWidget(const SizedBox.shrink());
      } finally {
        app.dispose();
      }
    }
  }, tags: 'golden');

  testWidgets('VPN Gate selection layout on desktop and phone', (tester) async {
    addTearDown(tester.view.resetDevicePixelRatio);
    addTearDown(tester.view.resetPhysicalSize);
    tester.view.devicePixelRatio = 1;
    for (final phone in [false, true]) {
      tester.view.physicalSize = phone
          ? const Size(390, 844)
          : const Size(1080, 920);
      final engine = GateEngine()..fetchedAt = DateTime(2026, 9, 12, 8);
      final app = AppController(engine)
        ..engineCapabilities = const EngineCapabilities(
          automaticEndpoints: true,
          vpnGateTcp: true,
          vpnGatePoolFavorites: true,
        )
        ..localePreference = phone
            ? LocalePreference.simplifiedChinese
            : LocalePreference.english;
      final boundary = GlobalKey();
      try {
        await tester.pumpWidget(
          RepaintBoundary(
            key: boundary,
            child: workflowHost(
              app,
              dark: phone,
              home: VpnGateScreen(
                controller: app,
                now: () => DateTime(2020, 1, 3, 14),
              ),
            ),
          ),
        );
        await tester.pump(const Duration(seconds: 2));
        await tester.pumpAndSettle();
        await tester.tap(find.byKey(const ValueKey('vpn-gate-toggle')));
        await tester.pumpAndSettle();
        await tester.tap(find.byKey(const ValueKey('vpn-gate-node-v1:node')));
        await tester.pumpAndSettle();
        await tester.tap(find.byKey(const ValueKey('vpn-gate-toggle')));
        await tester.pumpAndSettle();
        expect(tester.takeException(), isNull);
        await tester.runAsync(
          () => precacheImage(
            const AssetImage('assets/flags/w80/jp.png'),
            tester.element(find.byType(VpnGateScreen)),
          ),
        );
        await tester.pumpAndSettle();
        await expectLater(
          find.byKey(boundary),
          matchesGoldenFile(
            'goldens/vpngate_${phone ? 'phone_dark' : 'desktop_light'}.png',
          ),
        );
        engine.favorites[server.id] = VpnGateServer(
          id: server.id,
          ip: server.ip,
          hostname: server.hostname,
          configSha256: 'saved-configuration',
          countryCode: 'JP',
          countryName: 'Japan',
          favorite: VpnGateFavoriteMetadata(
            configSha256: 'saved-configuration',
            savedAt: DateTime(2020),
            latestConfigSha256: 'new-configuration',
          ),
          pool: VpnGatePoolMetadata(
            firstSeenAt: DateTime.utc(2020),
            lastSeenAt: DateTime.utc(2020, 1, 2),
            checkedAt: DateTime.utc(2020, 1, 2),
            tcpStatus: 'reachable',
            inPool: false,
          ),
        );
        final favoritesTab = find.byKey(const ValueKey('vpn-gate-favorites'));
        await tester.ensureVisible(favoritesTab);
        await tester.tap(favoritesTab);
        await tester.pumpAndSettle();
        await tester.ensureVisible(
          find.byKey(const ValueKey('vpn-gate-update-v1:node')),
        );
        await tester.pumpAndSettle();
        await expectLater(
          find.byKey(boundary),
          matchesGoldenFile(
            'goldens/vpngate_favorites_${phone ? 'phone_dark' : 'desktop_light'}.png',
          ),
        );
        await tester.pumpWidget(const SizedBox.shrink());
      } finally {
        app.dispose();
      }
    }
  }, tags: 'golden');

  testWidgets('VPN Gate observation summaries and details', (tester) async {
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetDevicePixelRatio);
    addTearDown(tester.view.resetPhysicalSize);
    for (final variant in [
      'desktop_light',
      'desktop_disabled_dark',
      'phone_details_dark',
    ]) {
      final phone = variant.startsWith('phone');
      final enabled = variant == 'desktop_light';
      tester.view.physicalSize = phone
          ? const Size(390, 844)
          : const Size(980, 610);
      final boundary = GlobalKey();
      final strings = AppStrings(LocalePreference.simplifiedChinese);
      final nodes = [
        observationServer(),
        observationServer(id: 'stale', expired: true, tcpStatus: 'unreachable'),
        observationServer(id: 'removed', inPool: false, tcpStatus: 'unknown'),
      ];
      await tester.pumpWidget(
        RepaintBoundary(
          key: boundary,
          child: MaterialApp(
            debugShowCheckedModeBanner: false,
            theme: enabled ? UsqueTheme.light() : UsqueTheme.dark(),
            home: Scaffold(
              body: SingleChildScrollView(
                padding: const EdgeInsets.all(16),
                child: Column(
                  children: [
                    for (final server in phone ? nodes.take(1) : nodes)
                      VpnGateServerRow(
                        key: ValueKey(server.id),
                        server: server,
                        strings: strings,
                        now: observationNow,
                        selected: false,
                        onSelect: enabled ? () {} : null,
                        onFavorite: () {},
                      ),
                  ],
                ),
              ),
            ),
          ),
        ),
      );
      await tester.pumpAndSettle();
      if (phone) {
        await tester.tap(
          find.byKey(const ValueKey('vpn-gate-details-observed')),
        );
        await tester.pumpAndSettle();
      }
      await tester.runAsync(
        () => precacheImage(
          const AssetImage('assets/flags/w80/us.png'),
          tester.element(find.byType(VpnGateServerRow).first),
        ),
      );
      await tester.pumpAndSettle();
      expect(tester.takeException(), isNull);
      await expectLater(
        find.byKey(boundary),
        matchesGoldenFile('goldens/vpngate_observations_$variant.png'),
      );
      await tester.pumpWidget(const SizedBox());
    }
  }, tags: 'golden');

  testWidgets('Home desktop chain controls and mobile top block', (
    tester,
  ) async {
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetDevicePixelRatio);
    addTearDown(tester.view.resetPhysicalSize);
    for (final (phone, dark) in const [
      (false, false),
      (true, false),
      (true, true),
    ]) {
      tester.view.physicalSize = phone
          ? const Size(390, 844)
          : const Size(1220, 1000);
      debugDefaultTargetPlatformOverride = phone
          ? TargetPlatform.android
          : TargetPlatform.windows;
      final app = AppController(GateEngine())
        ..localePreference = LocalePreference.simplifiedChinese
        ..sharedNetwork = UsqueProfile.defaultProfile().copyWith(
          vpnGate: const VpnGateSettings(enabled: true),
        )
        ..snapshot = phone
            ? const EngineSnapshot(
                phase: ConnectionPhase.connected,
                transport: 'HTTP/3',
                addressFamily: 'IPv4',
                killSwitchState: 'active',
                frontends: [
                  FrontendRuntimeStatus(
                    kind: FrontendKind.tunnel,
                    phase: FrontendPhase.active,
                  ),
                  FrontendRuntimeStatus(
                    kind: FrontendKind.socks5,
                    phase: FrontendPhase.active,
                  ),
                  FrontendRuntimeStatus(
                    kind: FrontendKind.http,
                    phase: FrontendPhase.active,
                  ),
                ],
                vpnGate: VpnGateStatus(
                  stage: 'connected',
                  warpStage: 'connected',
                  server: server,
                ),
              )
            : const EngineSnapshot();
      final boundary = GlobalKey();
      try {
        await tester.pumpWidget(
          RepaintBoundary(
            key: boundary,
            child: workflowHost(
              app,
              dark: dark,
              home: ShellScreen(controller: app),
            ),
          ),
        );
        await tester.pumpAndSettle();
        await tester.runAsync(() async {
          final context = tester.element(find.byType(ShellScreen));
          await Future.wait([
            precacheImage(
              AssetImage(
                UsqueLogo.assetFor(dark ? Brightness.dark : Brightness.light),
              ),
              context,
            ),
            if (phone)
              precacheImage(
                const AssetImage('assets/flags/w80/jp.png'),
                context,
              ),
          ]);
        });
        await tester.pumpAndSettle();
        expect(
          find.byKey(const ValueKey('home-network-quality')),
          findsNothing,
        );
        expect(find.byKey(const ValueKey('home-diagnostics')), findsNothing);
        expect(
          find.byKey(const ValueKey('home-vpn-gate-settings')),
          phone ? findsOneWidget : findsNothing,
        );
        expect(
          find.textContaining('WARP →'),
          phone ? findsOneWidget : findsNothing,
        );
        if (!phone) {
          expect(
            find.byKey(const ValueKey('home-chain-proxy-settings')),
            findsOneWidget,
          );
          expect(
            find.byKey(const ValueKey('home-chain-proxy-switch')),
            findsOneWidget,
          );
        }
        expect(tester.takeException(), isNull);
        await expectLater(
          find.byKey(boundary),
          matchesGoldenFile(
            'goldens/home_vpngate_${phone ? 'phone' : 'desktop'}_${dark ? 'dark' : 'light'}.png',
          ),
        );
      } finally {
        await tester.pumpWidget(const SizedBox());
        app.dispose();
        debugDefaultTargetPlatformOverride = null;
      }
    }
  }, tags: 'golden');

  testWidgets('VPN Gate preserves the wide shell navigation', (tester) async {
    tester.view.devicePixelRatio = 1;
    tester.view.physicalSize = const Size(1220, 920);
    addTearDown(tester.view.resetDevicePixelRatio);
    addTearDown(tester.view.resetPhysicalSize);
    final engine = GateEngine()..fetchedAt = DateTime(2026, 9, 12, 8);
    final app = AppController(engine)
      ..engineCapabilities = const EngineCapabilities(
        automaticEndpoints: true,
        vpnGateTcp: true,
        vpnGatePoolFavorites: true,
      )
      ..localePreference = LocalePreference.simplifiedChinese;
    app.selectSection(AppSection.proxy);
    final boundary = GlobalKey();
    try {
      await tester.pumpWidget(
        RepaintBoundary(
          key: boundary,
          child: workflowHost(
            app,
            dark: true,
            home: ShellScreen(controller: app),
          ),
        ),
      );
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(const ValueKey('proxy-chain-proxy-entry')));
      await tester.pumpAndSettle();
      if (find.byType(VpnGateScreen).evaluate().isEmpty) {
        await tester.tap(find.byKey(const ValueKey('chain-source-vpn_gate')));
      }
      await tester.pumpAndSettle();
      await tester.runAsync(() async {
        final context = tester.element(find.byType(VpnGateScreen));
        await Future.wait([
          precacheImage(const AssetImage('assets/flags/w80/jp.png'), context),
          precacheImage(const AssetImage(UsqueLogo.darkAsset), context),
        ]);
      });
      await tester.pumpAndSettle();
      expect(tester.takeException(), isNull);
      await expectLater(
        find.byKey(boundary),
        matchesGoldenFile('goldens/vpngate_shell_desktop_dark.png'),
      );
      await tester.pumpWidget(const SizedBox.shrink());
    } finally {
      app.dispose();
    }
  }, tags: 'golden');

  testWidgets('VPN Gate live server and pending selection remain distinct', (
    tester,
  ) async {
    tester.view.devicePixelRatio = 1;
    tester.view.physicalSize = const Size(1220, 920);
    addTearDown(tester.view.resetDevicePixelRatio);
    addTearDown(tester.view.resetPhysicalSize);
    final app =
        AppController(GateEngine()..fetchedAt = DateTime(2026, 9, 12, 8))
          ..engineCapabilities = const EngineCapabilities(
            automaticEndpoints: true,
            vpnGateTcp: true,
            vpnGatePoolFavorites: true,
          )
          ..localePreference = LocalePreference.simplifiedChinese
          ..sharedNetwork = UsqueProfile.defaultProfile().copyWith(
            vpnGate: const VpnGateSettings(
              enabled: true,
            ).copyWith(server: current),
          )
          ..snapshot = const EngineSnapshot(
            phase: ConnectionPhase.connected,
            vpnGate: VpnGateStatus(
              stage: 'connected',
              warpStage: 'connected',
              server: current,
            ),
          );
    app.selectSection(AppSection.proxy);
    final boundary = GlobalKey();
    try {
      await tester.pumpWidget(
        RepaintBoundary(
          key: boundary,
          child: workflowHost(
            app,
            dark: true,
            home: ShellScreen(controller: app),
          ),
        ),
      );
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(const ValueKey('proxy-chain-proxy-entry')));
      await tester.pumpAndSettle();
      if (find.byType(VpnGateScreen).evaluate().isEmpty) {
        await tester.tap(find.byKey(const ValueKey('chain-source-vpn_gate')));
      }
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(ValueKey('vpn-gate-node-${server.id}')));
      await tester.pumpAndSettle();
      await tester.runAsync(() async {
        final context = tester.element(find.byType(VpnGateScreen));
        await Future.wait([
          precacheImage(const AssetImage('assets/flags/w80/jp.png'), context),
          precacheImage(const AssetImage('assets/flags/w80/kr.png'), context),
          precacheImage(const AssetImage(UsqueLogo.darkAsset), context),
        ]);
      });
      await tester.pumpAndSettle();
      expect(tester.takeException(), isNull);
      await expectLater(
        find.byKey(boundary),
        matchesGoldenFile('goldens/vpngate_live_draft_desktop_dark.png'),
      );
      await tester.pumpWidget(const SizedBox.shrink());
    } finally {
      app.dispose();
    }
  }, tags: 'golden');

  testWidgets(
    'country flags preserve unusual shapes in light and directional dark layouts',
    (tester) async {
      tester.view.devicePixelRatio = 1;
      tester.view.physicalSize = const Size(540, 430);
      addTearDown(tester.view.resetDevicePixelRatio);
      addTearDown(tester.view.resetPhysicalSize);
      const labels = <String?, String>{
        'JP': 'Japan',
        'CH': 'Switzerland',
        'NP': 'Nepal',
        'QA': 'Qatar',
        null: 'Unknown region',
      };
      for (final dark in [false, true]) {
        final app = AppController(WorkflowEngine());
        final boundary = GlobalKey();
        try {
          await tester.pumpWidget(
            RepaintBoundary(
              key: boundary,
              child: workflowHost(
                app,
                dark: dark,
                home: Builder(
                  builder: (context) => MediaQuery(
                    data: MediaQuery.of(context).copyWith(
                      navigationMode: dark
                          ? NavigationMode.directional
                          : NavigationMode.traditional,
                    ),
                    child: Scaffold(
                      body: Padding(
                        padding: const EdgeInsets.all(24),
                        child: Column(
                          crossAxisAlignment: CrossAxisAlignment.start,
                          children: [
                            Text(
                              'Country / region',
                              style: Theme.of(context).textTheme.titleLarge,
                            ),
                            const SizedBox(height: 20),
                            for (final entry in labels.entries)
                              Padding(
                                padding: const EdgeInsets.symmetric(
                                  vertical: 12,
                                ),
                                child: Row(
                                  children: [
                                    CountryFlag(countryCode: entry.key),
                                    const SizedBox(width: 12),
                                    Expanded(child: Text(entry.value)),
                                    CountryFlag(
                                      countryCode: entry.key,
                                      enabled: false,
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
            ),
          );
          await tester.runAsync(() async {
            final context = tester.element(find.byType(Scaffold));
            for (final code in labels.keys.whereType<String>()) {
              await precacheImage(
                AssetImage('assets/flags/w80/${code.toLowerCase()}.png'),
                context,
              );
            }
          });
          await tester.pumpAndSettle();
          expect(tester.takeException(), isNull);
          await expectLater(
            find.byKey(boundary),
            matchesGoldenFile(
              'goldens/country_flags_${dark ? 'directional_dark' : 'light'}.png',
            ),
          );
          await tester.pumpWidget(const SizedBox.shrink());
        } finally {
          app.dispose();
        }
      }
    },
    tags: 'golden',
  );

  testWidgets(
    'Windows startup size fits Home including the native caption',
    (tester) async {
      // Read the runner's actual defaults so a larger test-only viewport cannot
      // hide a regression in the shipped startup window.
      final geometry = File(
        'windows/runner/window_geometry.h',
      ).readAsStringSync();
      double dimension(String name) => double.parse(
        RegExp('$name = (\\d+);').firstMatch(geometry)!.group(1)!,
      );
      final size = Size(
        dimension('kDefaultWindowWidth'),
        dimension('kDefaultWindowHeight'),
      );
      addTearDown(tester.view.resetDevicePixelRatio);
      addTearDown(tester.view.resetPhysicalSize);
      addTearDown(WindowFrame.instance.debugReset);
      WindowFrame.instance.debugEnable();
      for (final dpi in [1.0, 1.25, 1.5, 2.0]) {
        tester.view.devicePixelRatio = dpi;
        tester.view.physicalSize = size * dpi;
        for (final connected in [false, true]) {
          for (final zh in [false, true]) {
            final app = AppController(WorkflowEngine())
              ..localePreference = zh
                  ? LocalePreference.simplifiedChinese
                  : LocalePreference.english
              ..engineCapabilities = const EngineCapabilities(
                automaticEndpoints: true,
                networkQuality: true,
              );
            if (connected) {
              app.snapshot = const EngineSnapshot(
                phase: ConnectionPhase.connected,
                transport: 'HTTP/3',
                addressFamily: 'IPv4',
                killSwitchState: 'active',
                frontends: [
                  FrontendRuntimeStatus(
                    kind: FrontendKind.tunnel,
                    phase: FrontendPhase.active,
                  ),
                  FrontendRuntimeStatus(
                    kind: FrontendKind.socks5,
                    phase: FrontendPhase.active,
                  ),
                  FrontendRuntimeStatus(
                    kind: FrontendKind.http,
                    phase: FrontendPhase.active,
                  ),
                ],
                exit: ExitInfo(
                  country: 'Singapore',
                  countryCode: 'SG',
                  ipv4: '198.51.100.10',
                  ipv6: '2001:db8:1234:5678:abcd:ef01:2345:6789',
                ),
              );
            }
            try {
              final boundary = GlobalKey();
              await tester.pumpWidget(
                RepaintBoundary(
                  key: boundary,
                  child: workflowHost(
                    app,
                    dark: !zh,
                    home: WindowFrameScaffold(
                      strings: app.strings,
                      phase: app.snapshot.phase,
                      child: ShellScreen(controller: app),
                    ),
                  ),
                ),
              );
              await tester.pumpAndSettle();
              final reason = '$size dpi=$dpi connected=$connected zh=$zh';
              expect(
                tester.getTopLeft(find.text(app.strings.get('duration'))).dy,
                tester.getTopLeft(find.text(app.strings.get('protocol'))).dy,
                reason: reason,
              );
              expect(tester.getSize(find.byType(WindowTitleBar)).height, 40);
              final scrollable = find
                  .descendant(
                    of: find.byType(PageFrame),
                    matching: find.byType(Scrollable),
                  )
                  .first;
              expect(
                tester
                    .state<ScrollableState>(scrollable)
                    .position
                    .maxScrollExtent,
                0,
                reason: reason,
              );
              for (final button in [
                find.byType(ConnectionRing),
                for (final key in [
                  'home-tun-switch',
                  'home-system-proxy-switch',
                  'home-chain-proxy-switch',
                  'home-chain-proxy-settings',
                ])
                  find.byKey(ValueKey(key)),
              ]) {
                expect(button.hitTestable(), findsOneWidget, reason: reason);
                expect(
                  tester.getRect(button).bottom,
                  lessThanOrEqualTo(size.height - 16),
                  reason: reason,
                );
              }
              for (final key in ['home-network-quality', 'home-diagnostics']) {
                expect(find.byKey(ValueKey(key)), findsNothing, reason: reason);
              }
              final localProxy = find.byKey(
                const ValueKey('home-local-proxies'),
              );
              expect(localProxy, findsOneWidget, reason: reason);
              expect(
                tester.getRect(localProxy).bottom,
                lessThanOrEqualTo(size.height - 16),
                reason: reason,
              );
              expect(
                find.text(app.activeProfile.proxy.socksListeners.join(', ')),
                findsOneWidget,
                reason: reason,
              );
              expect(
                find.text(app.activeProfile.proxy.httpListeners.join(', ')),
                findsOneWidget,
                reason: reason,
              );
              expect(
                find.byKey(const ValueKey('home-manage-proxies')).hitTestable(),
                findsOneWidget,
                reason: reason,
              );
              expect(tester.takeException(), isNull, reason: reason);
              if (dpi == 1.25 && !connected && zh) {
                await tester.runAsync(
                  () => precacheImage(
                    const AssetImage(UsqueLogo.lightAsset),
                    tester.element(find.byType(MaterialApp)),
                  ),
                );
                await tester.pumpAndSettle();
                await expectLater(
                  find.byKey(boundary),
                  matchesGoldenFile('goldens/home_windows_default_idle.png'),
                );
              }
              await tester.pumpWidget(const SizedBox.shrink());
            } finally {
              app.dispose();
            }
          }
        }
      }
    },
    variant: TargetPlatformVariant.only(TargetPlatform.windows),
    tags: 'golden',
  );

  for (final chinese in [false, true]) {
    testWidgets('congestion selector ${chinese ? 'Chinese TV' : 'phone'}', (
      tester,
    ) async {
      tester.view.devicePixelRatio = 1;
      tester.view.physicalSize = chinese
          ? const Size(1280, 900)
          : const Size(375, 812);
      addTearDown(tester.view.resetDevicePixelRatio);
      addTearDown(tester.view.resetPhysicalSize);
      final app = AppController(WorkflowEngine())
        ..localePreference = chinese
            ? LocalePreference.simplifiedChinese
            : LocalePreference.english
        ..engineCapabilities = const EngineCapabilities(
          automaticEndpoints: true,
          h3CongestionControlAlgorithms: CongestionControlAlgorithm.values,
        )
        ..snapshot = const EngineSnapshot(
          phase: ConnectionPhase.connected,
          transport: 'h3',
          sessionCongestionControl: CongestionControlAlgorithm.cubic,
        );
      addTearDown(app.dispose);
      app.sharedNetwork = app.sharedNetwork.copyWith(
        allowLan: false,
        congestionControl: CongestionControlAlgorithm.bbr3,
      );
      app.networkSettings.accept(
        NetworkSettingsState(
          sourceEpoch: 'golden-engine',
          sequence: 1,
          operationId: 'saved-settings',
          persisted: true,
          storedProfile: app.activeProfile,
          appliedProfile: app.activeProfile.copyWith(
            congestionControl: CongestionControlAlgorithm.cubic,
          ),
          status: NetworkSettingsApplyStatus.deferred,
          deferredFields: const ['congestion_control'],
        ),
      );
      final boundary = GlobalKey();
      await tester.pumpWidget(
        RepaintBoundary(
          key: boundary,
          child: workflowHost(
            app,
            dark: chinese,
            scale: chinese ? 2 : 1,
            home: AdvancedSettingsScreen(controller: app),
          ),
        ),
      );
      await tester.pumpAndSettle();
      await Scrollable.ensureVisible(
        tester.element(find.byKey(const ValueKey('congestion-control'))),
        alignment: 0.15,
      );
      await tester.pumpAndSettle();
      expect(tester.takeException(), isNull);
      await expectLater(
        find.byKey(boundary),
        matchesGoldenFile(
          'goldens/congestion_${chinese ? 'tv_dark' : 'phone_light'}.png',
        ),
      );
    }, tags: 'golden');
  }

  for (final chinese in [false, true]) {
    testWidgets('L4 transport hint ${chinese ? 'zh_dark' : 'en_light'}', (
      tester,
    ) async {
      tester.view.devicePixelRatio = 1;
      tester.view.physicalSize = const Size(375, 812);
      addTearDown(tester.view.resetDevicePixelRatio);
      addTearDown(tester.view.resetPhysicalSize);
      final app = AppController(WorkflowEngine())
        ..localePreference = chinese
            ? LocalePreference.simplifiedChinese
            : LocalePreference.english
        ..engineCapabilities = const EngineCapabilities(
          automaticEndpoints: true,
          l4Tcp: true,
          l4TunTcp: true,
          l4DnsConversion: true,
          h3CongestionControlAlgorithms: CongestionControlAlgorithm.values,
        );
      app.sharedNetwork = app.sharedNetwork.copyWith(
        dataPlane: DataPlaneMode.l4Proxy,
      );
      addTearDown(app.dispose);
      final boundary = GlobalKey();
      await tester.pumpWidget(
        RepaintBoundary(
          key: boundary,
          child: workflowHost(
            app,
            dark: chinese,
            scale: chinese ? 2 : 1,
            home: AdvancedSettingsScreen(controller: app),
          ),
        ),
      );
      await tester.pumpAndSettle();
      await tester.scrollUntilVisible(
        find.byType(SegmentedButton<String>),
        200,
        scrollable: find.byType(Scrollable).first,
      );
      await Scrollable.ensureVisible(
        tester.element(find.byType(SegmentedButton<String>)),
        alignment: 0.15,
      );
      await tester.pumpAndSettle();
      expect(find.text(app.strings.get('l4_explanation')), findsNothing);
      expect(find.byKey(const ValueKey('l4-transport-hint')), findsOneWidget);
      expect(tester.takeException(), isNull);
      await expectLater(
        find.byKey(boundary),
        matchesGoldenFile(
          'goldens/l4_hint_${chinese ? 'zh_dark' : 'en_light'}.png',
        ),
      );
    }, tags: 'golden');
  }

  testWidgets(
    'workflow remains usable with real fonts, large text and landscape',
    (tester) async {
      tester.view.devicePixelRatio = 1;
      addTearDown(tester.view.resetDevicePixelRatio);
      addTearDown(tester.view.resetPhysicalSize);
      try {
        for (final size in const [
          Size(375, 812),
          Size(812, 375),
          Size(800, 1100),
          Size(1280, 720),
        ]) {
          tester.view.physicalSize = size;
          debugDefaultTargetPlatformOverride = size.width < 1000
              ? TargetPlatform.android
              : TargetPlatform.windows;
          for (final locale in [
            LocalePreference.english,
            LocalePreference.simplifiedChinese,
          ]) {
            for (final section in [
              AppSection.home,
              AppSection.profiles,
              AppSection.proxy,
              AppSection.settings,
            ]) {
              final app = AppController(WorkflowEngine())
                ..section = section
                ..localePreference = locale
                ..engineCapabilities = const EngineCapabilities(
                  automaticEndpoints: true,
                  networkQuality: true,
                );
              try {
                await tester.pumpWidget(
                  workflowHost(
                    app,
                    dark: locale == LocalePreference.simplifiedChinese,
                    scale: 2,
                  ),
                );
                await tester.pumpAndSettle();
                expect(
                  tester.takeException(),
                  isNull,
                  reason: '$size $locale $section',
                );
                if (section == AppSection.proxy) {
                  final apply = find.widgetWithText(
                    FilledButton,
                    app.strings.get('save_changes'),
                  );
                  final rect = tester.getRect(apply);
                  expect(rect.height, greaterThanOrEqualTo(48));
                  expect(rect.bottom, lessThanOrEqualTo(size.height));
                }
                await tester.pumpWidget(const SizedBox.shrink());
              } finally {
                app.dispose();
              }
            }
            final app = AppController(WorkflowEngine())
              ..localePreference = locale;
            try {
              await tester.pumpWidget(
                workflowHost(
                  app,
                  scale: 2,
                  dark: locale == LocalePreference.simplifiedChinese,
                  home: AdvancedSettingsScreen(controller: app),
                ),
              );
              await tester.pumpAndSettle();
              expect(
                tester.takeException(),
                isNull,
                reason: 'advanced $size $locale',
              );
              final apply = find.widgetWithText(
                FilledButton,
                app.strings.get('save_changes'),
              );
              expect(
                tester.getRect(apply).bottom,
                lessThanOrEqualTo(size.height),
              );
              await tester.pumpWidget(const SizedBox.shrink());
            } finally {
              app.dispose();
            }
          }
        }
      } finally {
        debugDefaultTargetPlatformOverride = null;
      }
    },
    tags: 'golden',
  );

  for (final scene
      in <
        ({
          String name,
          Size size,
          AppSection section,
          bool dark,
          bool zh,
          bool connected,
        })
      >[
        (
          name: 'home_phone_connected',
          size: const Size(375, 812),
          section: AppSection.home,
          dark: false,
          zh: true,
          connected: true,
        ),
        (
          name: 'home_phone_idle',
          size: const Size(375, 812),
          section: AppSection.home,
          dark: true,
          zh: false,
          connected: false,
        ),
        (
          name: 'home_phone_error',
          size: const Size(375, 812),
          section: AppSection.home,
          dark: true,
          zh: true,
          connected: false,
        ),
        (
          name: 'home_phone_connected_tall',
          size: const Size(430, 932),
          section: AppSection.home,
          dark: true,
          zh: false,
          connected: true,
        ),
        (
          name: 'home_phone_details_expanded',
          size: const Size(424, 924),
          section: AppSection.home,
          dark: false,
          zh: true,
          connected: true,
        ),
        (
          name: 'home_phone_details_expanded_dark',
          size: const Size(375, 812),
          section: AppSection.home,
          dark: true,
          zh: false,
          connected: true,
        ),
        (
          name: 'home_desktop_connected',
          size: const Size(1280, 900),
          section: AppSection.home,
          dark: true,
          zh: false,
          connected: true,
        ),
        (
          name: 'home_desktop_details_expanded_light',
          size: const Size(1440, 900),
          section: AppSection.home,
          dark: false,
          zh: true,
          connected: true,
        ),
        (
          name: 'home_desktop_details_expanded_dark',
          size: const Size(1440, 900),
          section: AppSection.home,
          dark: true,
          zh: true,
          connected: true,
        ),
        (
          name: 'proxy_phone_draft',
          size: const Size(375, 812),
          section: AppSection.proxy,
          dark: false,
          zh: true,
          connected: false,
        ),
        (
          name: 'settings_desktop_groups',
          size: const Size(1280, 1000),
          section: AppSection.settings,
          dark: false,
          zh: true,
          connected: false,
        ),
        (
          name: 'home_desktop_light',
          size: const Size(1280, 900),
          section: AppSection.home,
          dark: false,
          zh: true,
          connected: true,
        ),
        (
          name: 'proxy_desktop_dark',
          size: const Size(1280, 900),
          section: AppSection.proxy,
          dark: true,
          zh: false,
          connected: false,
        ),
        (
          name: 'settings_phone_dark',
          size: const Size(375, 812),
          section: AppSection.settings,
          dark: true,
          zh: false,
          connected: false,
        ),
        (
          name: 'profiles_phone',
          size: const Size(375, 812),
          section: AppSection.profiles,
          dark: false,
          zh: true,
          connected: false,
        ),
        (
          name: 'profiles_desktop',
          size: const Size(1280, 900),
          section: AppSection.profiles,
          dark: true,
          zh: false,
          connected: false,
        ),
      ]) {
    testWidgets('workflow golden ${scene.name}', (tester) async {
      tester.view.devicePixelRatio = 1;
      tester.view.physicalSize = scene.size;
      addTearDown(tester.view.resetDevicePixelRatio);
      addTearDown(tester.view.resetPhysicalSize);
      debugDefaultTargetPlatformOverride = scene.size.width < 760
          ? TargetPlatform.android
          : TargetPlatform.windows;
      final engine = WorkflowEngine();
      var now = DateTime.utc(2026, 9, 2, 12);
      final app =
          AppController(
              engine,
              qualityController: NetworkQualityController(
                engine,
                now: () => now,
                autoTick: false,
              ),
            )
            ..section = scene.section
            ..localePreference = scene.zh
                ? LocalePreference.simplifiedChinese
                : LocalePreference.english
            ..engineCapabilities = const EngineCapabilities(
              automaticEndpoints: true,
              networkQuality: true,
            );
      if (scene.section == AppSection.profiles) {
        final active = app.activeProfile;
        app.profiles = [
          active,
          active.copyWith(id: 'work', name: scene.zh ? '工作账号' : 'Work'),
          active.copyWith(id: 'travel', name: scene.zh ? '旅行账号' : 'Travel'),
        ];
      }
      app.profileIdentityStates = {
        for (final profile in app.profiles)
          profile.id: ProfileIdentityState.ready,
      };
      if (scene.name.startsWith('home_desktop_details_expanded')) {
        app.sharedNetwork = app.sharedNetwork.copyWith(
          chainExit: const ChainExitSettings(
            enabled: true,
            source: ChainSource.socks5Proxy,
          ),
        );
      }
      if (scene.connected) {
        app.snapshot = const EngineSnapshot(
          phase: ConnectionPhase.connected,
          transport: 'HTTP/3',
          addressFamily: 'IPv4',
          killSwitchState: 'active',
          downloadBytesPerSecond: 262144,
          uploadBytesPerSecond: 32768,
          frontends: [
            FrontendRuntimeStatus(
              kind: FrontendKind.tunnel,
              phase: FrontendPhase.active,
            ),
            FrontendRuntimeStatus(
              kind: FrontendKind.socks5,
              phase: FrontendPhase.active,
            ),
            FrontendRuntimeStatus(
              kind: FrontendKind.http,
              phase: FrontendPhase.active,
            ),
          ],
          exit: ExitInfo(
            country: 'Singapore',
            countryCode: 'SG',
            ipv4: '198.51.100.10',
            ipv6: '2001:db8::10',
          ),
        );
      }
      if (scene.name == 'home_phone_error') {
        app.snapshot = const EngineSnapshot(
          phase: ConnectionPhase.error,
          errorCode: 'TEST_CONNECTION_FAILED',
        );
        app.lastError = '暂时无法连接，请检查网络后重试。';
      }
      if (scene.connected && !scene.name.contains('details_expanded')) {
        var downloaded = 0;
        var uploaded = 0;
        // Synthetic engine samples, not a UI-generated curve. The rendered
        // trace is derived from these timestamped cumulative counters.
        for (var second = 0; second < 60; second++) {
          now = DateTime.utc(2026, 9, 2, 12).add(Duration(seconds: second));
          final down = (128 + second * 17 % 240) * 1024;
          final up = (20 + second * 7 % 48) * 1024;
          downloaded += down;
          uploaded += up;
          app.snapshot = EngineSnapshot(
            phase: ConnectionPhase.connected,
            transport: 'HTTP/3',
            addressFamily: 'IPv4',
            killSwitchState: 'active',
            downloadBytesPerSecond: down,
            uploadBytesPerSecond: up,
            downloadedBytes: downloaded,
            uploadedBytes: uploaded,
            networkQuality: qualityFixture(now),
            frontends: const [
              FrontendRuntimeStatus(
                kind: FrontendKind.tunnel,
                phase: FrontendPhase.active,
              ),
              FrontendRuntimeStatus(
                kind: FrontendKind.socks5,
                phase: FrontendPhase.active,
              ),
              FrontendRuntimeStatus(
                kind: FrontendKind.http,
                phase: FrontendPhase.active,
              ),
            ],
            exit: const ExitInfo(
              country: 'Singapore',
              countryCode: 'SG',
              ipv4: '198.51.100.10',
              ipv6: '2001:db8::10',
            ),
          );
        }
      }
      try {
        final boundary = GlobalKey();
        await tester.pumpWidget(
          RepaintBoundary(
            key: boundary,
            child: workflowHost(app, dark: scene.dark),
          ),
        );
        await tester.runAsync(() async {
          final context = tester.element(find.byType(MaterialApp));
          await precacheImage(
            AssetImage(
              UsqueLogo.assetFor(
                scene.dark ? Brightness.dark : Brightness.light,
              ),
            ),
            context,
          );
          if (scene.connected) {
            await precacheImage(
              const AssetImage('assets/flags/w80/sg.png'),
              context,
            );
          }
        });
        await tester.pumpAndSettle();
        if (scene.section == AppSection.home) {
          expect(
            find.byKey(const ValueKey('home-network-quality')),
            findsNothing,
          );
          expect(find.byKey(const ValueKey('home-diagnostics')), findsNothing);
          if (scene.size.width >= 760) {
            for (final key in [
              'home-tun-switch',
              'home-system-proxy-switch',
              'home-chain-proxy-switch',
              'home-chain-proxy-settings',
              'home-local-proxies',
            ]) {
              expect(find.byKey(ValueKey(key)), findsOneWidget);
            }
          }
        }
        if (scene.name.contains('details_expanded')) {
          final details = find.text(app.strings.get('connection_details'));
          await tester.ensureVisible(details);
          await tester.pumpAndSettle();
          await tester.tap(details);
          await tester.pumpAndSettle();
          await Scrollable.ensureVisible(tester.element(details));
          await tester.pumpAndSettle();
          expect(
            find.widgetWithText(SelectableText, '198.51.100.10'),
            findsOneWidget,
          );
          expect(
            find.widgetWithText(SelectableText, '2001:db8::10'),
            findsOneWidget,
          );
          expect(
            find.byKey(const ValueKey('home-vpn-gate-settings')),
            findsNothing,
          );
          expect(find.textContaining('WARP →'), findsNothing);
          expect(find.byType(ErrorWidget), findsNothing);
        }
        if (scene.section == AppSection.proxy) {
          final port = find
              .byWidgetPredicate(
                (widget) =>
                    widget is TextField &&
                    widget.decoration?.labelText == app.strings.get('port'),
              )
              .first;
          await tester.enterText(port, '9090');
          FocusManager.instance.primaryFocus?.unfocus();
          await tester.pumpAndSettle();
          Scrollable.of(
            tester.element(find.byType(ChainProxyEntry)),
          ).position.jumpTo(0);
          await tester.pumpAndSettle();
        }
        expect(tester.takeException(), isNull);
        await expectLater(
          find.byKey(boundary),
          matchesGoldenFile('goldens/${scene.name}.png'),
        );
        await tester.pumpWidget(const SizedBox.shrink());
      } finally {
        app.dispose();
        debugDefaultTargetPlatformOverride = null;
      }
    }, tags: 'golden');
  }
  for (final phone in [true, false]) {
    for (final page in ['onboarding', 'diagnostics', 'dialog']) {
      final name = '${page}_${phone ? 'phone_light' : 'desktop_dark'}';
      testWidgets('native golden $name', (tester) async {
        tester.view.devicePixelRatio = 1;
        tester.view.physicalSize = phone
            ? const Size(375, 812)
            : const Size(1280, 900);
        addTearDown(tester.view.resetDevicePixelRatio);
        addTearDown(tester.view.resetPhysicalSize);
        debugDefaultTargetPlatformOverride = phone
            ? TargetPlatform.android
            : TargetPlatform.windows;
        final app = AppController(WorkflowEngine())
          ..localePreference = phone
              ? LocalePreference.simplifiedChinese
              : LocalePreference.english
          ..engineCapabilities = const EngineCapabilities(networkQuality: true);
        try {
          final boundary = GlobalKey();
          final child = switch (page) {
            'onboarding' => OnboardingScreen(controller: app),
            'diagnostics' => DiagnosticsScreen(controller: app),
            _ => Scaffold(
              body: UsqueDialog(
                icon: LucideIcons.pencil,
                title: app.strings.get('edit'),
                subtitle: app.activeProfile.name,
                content: DialogGroup(
                  child: TextFormField(
                    initialValue: 'Default',
                    decoration: InputDecoration(
                      labelText: app.strings.get('profiles'),
                    ),
                  ),
                ),
                actions: [
                  TextButton(
                    onPressed: () {},
                    child: Text(app.strings.get('cancel')),
                  ),
                  FilledButton(
                    onPressed: () {},
                    child: Text(app.strings.get('save_changes')),
                  ),
                ],
              ),
            ),
          };
          await tester.pumpWidget(
            RepaintBoundary(
              key: boundary,
              child: workflowHost(app, dark: !phone, home: child),
            ),
          );
          await tester.runAsync(
            () => precacheImage(
              AssetImage(
                UsqueLogo.assetFor(phone ? Brightness.light : Brightness.dark),
              ),
              tester.element(find.byType(MaterialApp)),
            ),
          );
          await tester.pumpAndSettle();
          expect(find.byType(StatusPill), findsNothing);
          expect(tester.takeException(), isNull);
          await expectLater(
            find.byKey(boundary),
            matchesGoldenFile('goldens/$name.png'),
          );
          await tester.pumpWidget(const SizedBox.shrink());
        } finally {
          app.dispose();
          debugDefaultTargetPlatformOverride = null;
        }
      }, tags: 'golden');
    }
  }
}
