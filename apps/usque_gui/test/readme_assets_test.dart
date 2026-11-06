/// Renders the README banners, social preview, screenshots, gallery and the
/// connect animation frames from fixture data with the bundled fonts.
///
/// Skipped unless `USQUE_README_ASSETS` names the repository root. Run it with
/// `tool/dev.sh readme-assets`, which also assembles the animation. Every
/// address is a documentation address and every engine call is local.
@Tags(['readme-assets'])
library;

import 'dart:ffi' show Abi;
import 'dart:io';
import 'dart:math' as math;
import 'dart:ui' as ui;

import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter/rendering.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:shared_preferences/shared_preferences.dart';
import 'package:usque/core/country_flags.dart';
import 'package:usque/core/usque_theme.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/screens/chain_proxy_screen.dart';
import 'package:usque/screens/geo_direct_settings_screen.dart';
import 'package:usque/screens/network_quality_screen.dart';
import 'package:usque/screens/shell_screen.dart';
import 'package:usque/services/engine_client.dart';
import 'package:usque/state/app_controller.dart';
import 'package:usque/state/network_quality_controller.dart';
import 'package:usque/state/window_frame.dart';
import 'package:usque/widgets/usque_logo.dart';
import 'package:usque/widgets/window_titlebar.dart';

import 'quality_test_support.dart' show qualityFixture;
import 'scenarios_test.dart' show SceneEngine, richSceneEngine;
import 'support/linux_golden_support.dart';
import 'ui_workflow_test.dart' show workflowHost;

final String? _root = Platform.environment['USQUE_README_ASSETS'];
final String? _frames = Platform.environment['USQUE_README_FRAMES'];

const _capabilities = EngineCapabilities(
  vpn: true,
  systemProxy: true,
  vpnGateTcp: true,
  chainProfileImport: true,
  chainOpenvpnUdp: true,
  chainWireguard: true,
  chainWarpWireguard: true,
  chainProtonVpn: true,
  chainHttpProxy: true,
  chainSocks5Proxy: true,
  routingRules: true,
  liveAdsRules: true,
  automaticEndpoints: true,
  networkSettingsApplication: true,
  networkQuality: true,
  connectionScenarios: true,
  endpointPresets: true,
);

const _office = ChainProfileSummary(
  id: 'office-wireguard',
  revision: 'fixture-revision',
  editRevision: 'fixture-edit-revision',
  name: 'Office WireGuard',
  protocol: 'wireguard',
  host: 'vpn.example',
  port: 51820,
  addresses: ['10.8.0.2/32'],
  allowedIps: ['0.0.0.0/0', '::/0'],
  dns: ['10.8.0.1'],
  mtu: 1280,
);

/// Scene, chain and settings fixtures with no native, file or network access.
class _ReadmeEngine extends SceneEngine implements ChainProfileClient {
  @override
  Future<EngineCapabilities?> getCapabilities() async => _capabilities;

  @override
  Future<ChainProfileResult> chainProfile(Map<String, Object?> request) async {
    if (request['action'] != 'list') {
      throw StateError('README fixtures never change chain profiles.');
    }
    return const ChainProfileResult(
      profiles: [
        _office,
        ChainProfileSummary(
          id: 'home-wireguard',
          revision: 'fixture-revision-home',
          editRevision: 'fixture-edit-revision-home',
          name: 'Home router',
          protocol: 'wireguard',
          host: 'home.example',
          port: 51820,
          addresses: ['10.9.0.2/32'],
          allowedIps: ['0.0.0.0/0', '::/0'],
          dns: ['10.9.0.1'],
          mtu: 1280,
        ),
      ],
    );
  }

  @override
  Future<List<ChainConfigurationFile>> pickChainConfigurations() =>
      throw StateError('README fixtures never open a file picker.');
}

enum _Screen { home, scenes, chain, routing, quality }

/// A smooth, deterministic traffic trace in bytes per second.
int _download(int second) =>
    ((430 +
                230 * math.sin(second / 6.5) +
                90 * math.sin(second / 2.1 + 1.3) +
                40 * math.sin(second * 1.7)) *
            1024)
        .round();

int _upload(int second) =>
    ((96 +
                40 * math.sin(second / 5.2 + 2) +
                16 * math.sin(second * 1.3 + 0.4)) *
            1024)
        .round();

EngineSnapshot _connected(
  DateTime now, {
  required int second,
  required int downloaded,
  required int uploaded,
  required DateTime since,
  ChainExitStatus chainExit = const ChainExitStatus(),
}) => EngineSnapshot(
  phase: ConnectionPhase.connected,
  transport: 'HTTP/3',
  addressFamily: 'IPv4',
  killSwitchState: 'active',
  connectedAt: since,
  adsRuleRevision: 'fixture-ads',
  chainExit: chainExit,
  downloadBytesPerSecond: _download(second),
  uploadBytesPerSecond: _upload(second),
  downloadedBytes: downloaded,
  uploadedBytes: uploaded,
  networkQuality: qualityFixture(
    now,
    rtt: (38 + 9 * math.sin(second / 4.3) + 4 * math.sin(second * 1.1)).round(),
  ),
  frontends: const [
    FrontendRuntimeStatus(
      kind: FrontendKind.tunnel,
      phase: FrontendPhase.active,
    ),
    FrontendRuntimeStatus(
      kind: FrontendKind.socks5,
      phase: FrontendPhase.active,
    ),
    FrontendRuntimeStatus(kind: FrontendKind.http, phase: FrontendPhase.active),
  ],
  exit: const ExitInfo(
    country: 'Singapore',
    countryCode: 'SG',
    ipv4: '198.51.100.10',
    ipv6: '2001:db8::10',
  ),
);

/// Builds a ready controller whose scenes, accounts and settings come from
/// [engine]. [screen] selects the shared-network fixture.
Future<AppController> _app(
  _ReadmeEngine engine,
  DateTime Function() clock,
  _Screen screen,
) async {
  final app = AppController(
    engine,
    qualityController: NetworkQualityController(
      engine,
      now: clock,
      autoTick: false,
    ),
    adsClock: clock,
  );
  app
    ..localePreference = LocalePreference.english
    ..initialized = true
    ..onboardingTermsAccepted = true
    ..onboardingStep = 3
    ..section = screen == _Screen.scenes
        ? AppSection.profiles
        : AppSection.home;
  switch (screen) {
    case _Screen.chain:
      app.sharedNetwork = app.sharedNetwork.copyWith(
        chainExit: const ChainExitSettings(
          enabled: true,
          source: ChainSource.wireguardCustom,
          profileId: 'office-wireguard',
        ),
      );
    case _Screen.routing:
      app.sharedNetwork = app.sharedNetwork.copyWith(
        routing: RoutingSettings(
          adsEnabled: true,
          rules: [
            RoutingRule.create(
              'example.com',
              RoutingAction.direct,
              id: 'fixture-direct',
            ),
            RoutingRule.create(
              'tracker.example.net',
              RoutingAction.reject,
              id: 'fixture-reject',
            ),
            RoutingRule.create(
              'intranet.example',
              RoutingAction.direct,
              id: 'fixture-intranet',
            ),
            RoutingRule.create(
              '192.0.2.0/24',
              RoutingAction.proxy,
              id: 'fixture-proxy',
            ),
          ],
        ),
      );
    default:
      break;
  }
  final active = app.sharedNetwork.copyWith(
    id: app.activeProfileId,
    name: app.activeProfile.name,
  );
  engine
    ..storedProfiles = [
      active,
      active.copyWith(id: 'account-b', name: 'Work account'),
    ]
    ..storedActiveProfileId = app.activeProfileId
    ..settingsState = NetworkSettingsState(
      sourceEpoch: engine.settingsEpoch,
      sequence: 1,
      sessionId: 'session-a',
      appliedProfile: active,
    );
  app
    ..profiles = engine.storedProfiles
    ..profileIdentityStates = {
      for (final profile in engine.storedProfiles)
        profile.id: ProfileIdentityState.ready,
    }
    ..engineCapabilities = _capabilities;
  await app.scenarios.refresh();
  return app;
}

/// Copies the scene library of [richSceneEngine] without its modified state.
_ReadmeEngine _engine() {
  final rich = richSceneEngine();
  return _ReadmeEngine()
    ..selected = 'scene-a'
    ..entries = rich.entries
    ..extraState = {
      'state': {'modified': false, 'apply_status': 'not_required'},
      'session_id': 'session-a',
      'observation_session_id': 'session-a',
      'applied_account_id': UsqueProfile.defaultProfile().id,
      'applied_scenario_id': 'scene-a',
      'applied_scene_confirmed': true,
    };
}

Future<Uint8List> _capture(WidgetTester tester, GlobalKey key) async {
  final bytes = await tester.runAsync(() async {
    final boundary =
        key.currentContext!.findRenderObject()! as RenderRepaintBoundary;
    final image = await boundary.toImage(
      pixelRatio: tester.view.devicePixelRatio,
    );
    try {
      final data = await image.toByteData(format: ui.ImageByteFormat.png);
      return data!.buffer.asUint8List();
    } finally {
      image.dispose();
    }
  });
  return bytes!;
}

void _write(String relative, Uint8List bytes) {
  File('$_root/$relative')
    ..parent.createSync(recursive: true)
    ..writeAsBytesSync(bytes);
}

void _view(WidgetTester tester, Size logical, double ratio) {
  tester.view.devicePixelRatio = ratio;
  tester.view.physicalSize = logical * ratio;
  addTearDown(tester.view.resetDevicePixelRatio);
  addTearDown(tester.view.resetPhysicalSize);
}

Future<void> _precache(
  WidgetTester tester,
  bool dark, {
  Iterable<ImageProvider> extra = const [],
}) async {
  await tester.runAsync(() async {
    final context = tester.element(find.byType(MaterialApp).first);
    await Future.wait([
      precacheImage(
        AssetImage(
          UsqueLogo.assetFor(dark ? Brightness.dark : Brightness.light),
        ),
        context,
      ),
      precacheImage(const AssetImage('assets/flags/w80/sg.png'), context),
      for (final provider in extra) precacheImage(provider, context),
    ]);
  });
}

/// Renders one app screen and returns its PNG bytes.
Future<Uint8List> _renderScreen(
  WidgetTester tester, {
  required _Screen screen,
  required bool dark,
  required Size size,
  required double ratio,
  required TargetPlatform platform,
  bool windowFrame = false,
}) async {
  _view(tester, size, ratio);
  debugDefaultTargetPlatformOverride = platform;
  if (windowFrame) {
    WindowFrame.instance.debugEnable();
  }
  final base = DateTime.utc(2026, 9, 2, 12);
  var now = base;
  final engine = _engine();
  final app = await _app(engine, () => now, screen);
  try {
    // Every screen shows a running session. The duration readout formats
    // against the wall clock, so the start time is relative to it.
    final since = DateTime.now().toUtc().subtract(
      const Duration(minutes: 23, seconds: 41),
    );
    var downloaded = 0;
    var uploaded = 0;
    for (var second = 0; second < 60; second++) {
      now = base.add(Duration(seconds: second));
      downloaded += _download(second);
      uploaded += _upload(second);
      app.snapshot = _connected(
        now,
        second: second,
        downloaded: downloaded,
        uploaded: uploaded,
        since: since,
        chainExit: screen == _Screen.chain
            ? const ChainExitStatus(
                stage: 'connected',
                tcpConnectVerified: true,
                currentProfile: _office,
                activeEndpoint: '203.0.113.5:51820',
              )
            : const ChainExitStatus(),
      );
    }
    final home = switch (screen) {
      _Screen.chain => ChainProxyScreen(
        controller: app,
        now: () => DateTime.utc(2026, 9, 2, 12, 1),
      ),
      _Screen.routing => GeoDirectSettingsScreen(controller: app),
      _Screen.quality => NetworkQualityScreen(controller: app),
      _ => ShellScreen(controller: app),
    };
    final boundary = GlobalKey();
    await tester.pumpWidget(
      RepaintBoundary(
        key: boundary,
        child: workflowHost(
          app,
          dark: dark,
          home: windowFrame
              ? WindowFrameScaffold(
                  strings: app.strings,
                  phase: app.snapshot.phase,
                  child: home,
                )
              : home,
        ),
      ),
    );
    await _precache(
      tester,
      dark,
      extra: [
        if (screen == _Screen.routing)
          for (final code in kCountryFlagCodes)
            AssetImage('assets/flags/w80/$code.png'),
      ],
    );
    await tester.pumpAndSettle();
    if (windowFrame) {
      expect(find.byType(WindowTitleBar), findsOneWidget);
    }
    expect(tester.takeException(), isNull);
    expect(find.byType(ErrorWidget), findsNothing);
    final bytes = await _capture(tester, boundary);
    await tester.pumpWidget(const SizedBox.shrink());
    app.diagnostics.setObservationVisible(false);
    return bytes;
  } finally {
    app.dispose();
    debugDefaultTargetPlatformOverride = null;
    if (windowFrame) WindowFrame.instance.debugReset();
  }
}

/// Soft brand shapes behind the banner and social preview.
class _Backdrop extends CustomPainter {
  const _Backdrop({required this.dark, required this.circles});

  final bool dark;
  final List<(Offset, double, double)> circles;

  @override
  void paint(Canvas canvas, Size size) {
    for (final (center, radius, strength) in circles) {
      canvas.drawCircle(
        center,
        radius,
        Paint()
          ..color = dark
              ? UsqueColors.ember.withValues(alpha: 0.11 * strength)
              : Color.lerp(Colors.white, const Color(0xFFFFE3CC), strength)!,
      );
    }
  }

  @override
  bool shouldRepaint(covariant _Backdrop oldDelegate) =>
      oldDelegate.dark != dark || oldDelegate.circles != circles;
}

class _Chip extends StatelessWidget {
  const _Chip(this.label, {required this.fontSize});

  final String label;
  final double fontSize;

  @override
  Widget build(BuildContext context) {
    final tokens = UsqueTokens.of(context);
    return Container(
      padding: EdgeInsets.symmetric(
        horizontal: fontSize * 0.8,
        vertical: fontSize * 0.36,
      ),
      decoration: BoxDecoration(
        color: tokens.brandSoft,
        borderRadius: BorderRadius.circular(UsqueRadii.pill),
        border: Border.all(color: tokens.brand.withValues(alpha: 0.28)),
      ),
      child: Text(
        label,
        style: TextStyle(
          fontFamily: UsqueFonts.body,
          fontFamilyFallback: UsqueFonts.fallback,
          fontWeight: FontWeight.w700,
          fontSize: fontSize,
          height: 1.2,
          color: tokens.brand,
        ),
      ),
    );
  }
}

/// README banner: brand mark, wordmark and language-neutral output names.
class _Banner extends StatelessWidget {
  const _Banner({required this.dark});

  final bool dark;

  @override
  Widget build(BuildContext context) {
    final tokens = UsqueTokens.of(context);
    final muted = Theme.of(context).colorScheme.onSurfaceVariant;
    return ColoredBox(
      color: dark ? const Color(0xFF0D1117) : Colors.white,
      child: CustomPaint(
        painter: _Backdrop(
          dark: dark,
          circles: const [
            (Offset(150, 40), 250, 0.55),
            (Offset(770, 250), 115, 0.35),
          ],
        ),
        child: Center(
          child: Row(
            mainAxisSize: MainAxisSize.min,
            children: [
              // 128 logical pixels at 2x is the bundled logo's native size.
              const UsqueLogo(size: 128),
              const SizedBox(width: 48),
              Column(
                mainAxisSize: MainAxisSize.min,
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text(
                    'Usque',
                    style: TextStyle(
                      fontFamily: UsqueFonts.display,
                      fontWeight: FontWeight.w700,
                      fontSize: 84,
                      height: 1,
                      letterSpacing: -1.5,
                      color: tokens.brand,
                    ),
                  ),
                  const SizedBox(height: 20),
                  const Row(
                    children: [
                      _Chip('VPN', fontSize: 15),
                      SizedBox(width: 10),
                      _Chip('SOCKS5', fontSize: 15),
                      SizedBox(width: 10),
                      _Chip('HTTP', fontSize: 15),
                    ],
                  ),
                  const SizedBox(height: 16),
                  Text(
                    'Windows · Linux · Android',
                    style: TextStyle(
                      fontFamily: UsqueFonts.body,
                      fontWeight: FontWeight.w600,
                      fontSize: 17,
                      letterSpacing: 0.2,
                      color: muted,
                    ),
                  ),
                ],
              ),
            ],
          ),
        ),
      ),
    );
  }
}

/// Social preview: what the project is, beside the desktop and phone views.
class _SocialPreview extends StatelessWidget {
  const _SocialPreview({required this.desktop, required this.phone});

  final ImageProvider desktop;
  final ImageProvider phone;

  @override
  Widget build(BuildContext context) {
    final tokens = UsqueTokens.of(context);
    final scheme = Theme.of(context).colorScheme;
    TextStyle body(double size, FontWeight weight, Color color) => TextStyle(
      fontFamily: UsqueFonts.body,
      fontWeight: weight,
      fontSize: size,
      height: 1.3,
      color: color,
    );
    Widget framed(ImageProvider image, double width, double radius) =>
        DecoratedBox(
          decoration: BoxDecoration(
            borderRadius: BorderRadius.circular(radius),
            boxShadow: const [
              BoxShadow(
                color: Color(0x33000000),
                blurRadius: 36,
                offset: Offset(0, 16),
              ),
            ],
          ),
          child: ClipRRect(
            borderRadius: BorderRadius.circular(radius),
            child: Image(
              image: image,
              width: width,
              filterQuality: FilterQuality.high,
            ),
          ),
        );
    return ColoredBox(
      color: tokens.canvas,
      child: CustomPaint(
        painter: const _Backdrop(
          dark: false,
          circles: [(Offset(140, 40), 300, 0.6), (Offset(1180, 640), 260, 0.5)],
        ),
        child: Stack(
          children: [
            Positioned(
              left: 72,
              top: 0,
              bottom: 0,
              width: 560,
              child: Column(
                mainAxisAlignment: MainAxisAlignment.center,
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Row(
                    children: [
                      const UsqueLogo(size: 96),
                      const SizedBox(width: 24),
                      Text(
                        'Usque',
                        style: TextStyle(
                          fontFamily: UsqueFonts.display,
                          fontWeight: FontWeight.w700,
                          fontSize: 78,
                          height: 1,
                          letterSpacing: -1.5,
                          color: tokens.brand,
                        ),
                      ),
                    ],
                  ),
                  const SizedBox(height: 30),
                  Text(
                    'Unofficial client compatible with '
                    'Cloudflare® WARP® services',
                    style: body(32, FontWeight.w700, scheme.onSurface),
                  ),
                  const SizedBox(height: 22),
                  const Wrap(
                    spacing: 10,
                    runSpacing: 10,
                    children: [
                      _Chip('System VPN', fontSize: 17),
                      _Chip('SOCKS5', fontSize: 17),
                      _Chip('HTTP proxy', fontSize: 17),
                      _Chip('Chain proxy', fontSize: 17),
                    ],
                  ),
                  const SizedBox(height: 20),
                  Text(
                    'Windows · Linux · Android / Android TV',
                    style: body(21, FontWeight.w600, scheme.onSurfaceVariant),
                  ),
                  const SizedBox(height: 36),
                  Text(
                    'Independent project. Not affiliated with Cloudflare, Inc.',
                    style: body(15, FontWeight.w500, scheme.onSurfaceVariant),
                  ),
                ],
              ),
            ),
            Positioned(left: 676, top: 92, child: framed(desktop, 540, 14)),
            Positioned(left: 1016, top: 168, child: framed(phone, 208, 26)),
          ],
        ),
      ),
    );
  }
}

Future<Uint8List> _renderArt(
  WidgetTester tester, {
  required Size size,
  required double ratio,
  required bool dark,
  required Widget child,
  Iterable<ImageProvider> images = const [],
}) async {
  _view(tester, size, ratio);
  final boundary = GlobalKey();
  await tester.pumpWidget(
    RepaintBoundary(
      key: boundary,
      child: MaterialApp(
        debugShowCheckedModeBanner: false,
        theme: dark ? UsqueTheme.dark() : UsqueTheme.light(),
        home: Material(type: MaterialType.transparency, child: child),
      ),
    ),
  );
  await _precache(tester, dark, extra: images);
  await tester.pumpAndSettle();
  expect(tester.takeException(), isNull);
  final bytes = await _capture(tester, boundary);
  await tester.pumpWidget(const SizedBox.shrink());
  return bytes;
}

/// Frames of the phone Home: idle, connecting, then connected traffic with
/// one engine second per frame. The helper encodes them at 15 frames/s.
Future<void> _renderConnectFrames(WidgetTester tester, bool dark) async {
  const ratio = 1.3;
  _view(tester, const Size(430, 932), ratio);
  debugDefaultTargetPlatformOverride = TargetPlatform.android;
  final base = DateTime.utc(2026, 9, 2, 12);
  var now = base;
  final engine = _engine();
  final app = await _app(engine, () => now, _Screen.home);
  final directory = Directory('$_frames/connect-${dark ? 'dark' : 'light'}')
    ..createSync(recursive: true);
  var index = 0;
  const step = Duration(microseconds: 66667);
  final boundary = GlobalKey();
  // Assigning a snapshot does not notify; rebuilding the same tree each frame
  // re-reads it while element and animation state persist.
  Widget host() => RepaintBoundary(
    key: boundary,
    child: workflowHost(app, dark: dark, reducedMotion: false),
  );
  Future<void> frame() async {
    await tester.pumpWidget(host(), duration: step);
    final bytes = await _capture(tester, boundary);
    File(
      '${directory.path}/frame_${(index++).toString().padLeft(4, '0')}.png',
    ).writeAsBytesSync(bytes);
  }

  try {
    await tester.pumpWidget(host());
    await _precache(tester, dark);
    for (var i = 0; i < 9; i++) {
      await frame();
    }
    app.snapshot = const EngineSnapshot(phase: ConnectionPhase.connectingH3);
    for (var i = 0; i < 18; i++) {
      await frame();
    }
    var downloaded = 0;
    var uploaded = 0;
    for (var second = 0; second < 60; second++) {
      now = base.add(Duration(seconds: second));
      downloaded += _download(second);
      uploaded += _upload(second);
      app.snapshot = _connected(
        now,
        second: second,
        downloaded: downloaded,
        uploaded: uploaded,
        // One engine second per frame: the readout counts up from zero.
        since: DateTime.now().toUtc().subtract(Duration(seconds: second)),
      );
      await frame();
    }
    expect(tester.takeException(), isNull);
    await tester.pumpWidget(const SizedBox.shrink());
    app.diagnostics.setObservationVisible(false);
  } finally {
    app.dispose();
    debugDefaultTargetPlatformOverride = null;
  }
}

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  final skip = _root == null
      ? 'Set USQUE_README_ASSETS; use tool/dev.sh readme-assets.'
      : !(Platform.isLinux && Abi.current() == Abi.linuxX64)
      ? 'README assets use the Linux amd64 font fixtures.'
      : null;
  group('README assets', skip: skip, () {
    setUpAll(loadLinuxGoldenFonts);
    setUp(() => SharedPreferences.setMockInitialValues(<String, Object>{}));

    for (final dark in [false, true]) {
      final suffix = dark ? '-dark' : '';
      testWidgets('banner$suffix', (tester) async {
        _write(
          'assets/branding/usque-readme-banner$suffix.png',
          await _renderArt(
            tester,
            size: const Size(800, 250),
            ratio: 2,
            dark: dark,
            child: _Banner(dark: dark),
          ),
        );
      });

      testWidgets('desktop and phone Home$suffix', (tester) async {
        _write(
          'assets/screenshots/usque-windows-home$suffix.png',
          await _renderScreen(
            tester,
            screen: _Screen.home,
            dark: dark,
            size: const Size(1040, 760),
            ratio: 1.5,
            platform: TargetPlatform.windows,
            windowFrame: true,
          ),
        );
        _write(
          'assets/screenshots/usque-android-home$suffix.png',
          await _renderScreen(
            tester,
            screen: _Screen.home,
            dark: dark,
            size: const Size(430, 932),
            ratio: 3,
            platform: TargetPlatform.android,
          ),
        );
      });

      testWidgets('gallery$suffix', (tester) async {
        for (final (screen, name) in const [
          (_Screen.scenes, 'scenes'),
          (_Screen.chain, 'chain'),
          (_Screen.routing, 'routing'),
          (_Screen.quality, 'quality'),
        ]) {
          _write(
            'assets/screenshots/usque-gallery-$name$suffix.png',
            await _renderScreen(
              tester,
              screen: screen,
              dark: dark,
              size: const Size(390, 844),
              ratio: 1.5,
              platform: TargetPlatform.android,
            ),
          );
        }
      });

      testWidgets('connect animation frames$suffix', (tester) async {
        if (_frames == null) {
          markTestSkipped('USQUE_README_FRAMES is not set.');
          return;
        }
        await _renderConnectFrames(tester, dark);
      });
    }

    testWidgets('social preview', (tester) async {
      final desktop = await _renderScreen(
        tester,
        screen: _Screen.home,
        dark: false,
        size: const Size(1040, 760),
        ratio: 1,
        platform: TargetPlatform.windows,
        windowFrame: true,
      );
      final phone = await _renderScreen(
        tester,
        screen: _Screen.home,
        dark: true,
        size: const Size(430, 932),
        ratio: 1.5,
        platform: TargetPlatform.android,
      );
      final desktopImage = MemoryImage(desktop);
      final phoneImage = MemoryImage(phone);
      _write(
        'assets/branding/usque-social-preview.png',
        await _renderArt(
          tester,
          size: const Size(1280, 640),
          ratio: 1,
          dark: false,
          images: [desktopImage, phoneImage],
          child: _SocialPreview(desktop: desktopImage, phone: phoneImage),
        ),
      );
    });
  });
}
