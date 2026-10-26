import 'dart:ffi' show Abi;
import 'dart:io';
import 'dart:typed_data';

import 'package:flutter/material.dart';
import 'package:flutter_localizations/flutter_localizations.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:shared_preferences/shared_preferences.dart';
import 'package:usque/core/chain_strings.dart';
import 'package:usque/core/country_flags.dart';
import 'package:usque/core/proton_strings.dart';
import 'package:usque/core/user_facing_errors.dart';
import 'package:usque/core/usque_theme.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/models/diagnostics_models.dart';
import 'package:usque/screens/chain_proxy_screen.dart';
import 'package:usque/screens/diagnostics_screen.dart';
import 'package:usque/screens/geo_direct_settings_screen.dart';
import 'package:usque/screens/network_quality_screen.dart';
import 'package:usque/screens/onboarding_screen.dart';
import 'package:usque/screens/shell_screen.dart';
import 'package:usque/services/engine_client.dart';
import 'package:usque/state/app_controller.dart';
import 'package:usque/state/network_quality_controller.dart';
import 'package:usque/widgets/chain_source_picker.dart';
import 'package:usque/widgets/linux_service_section.dart';
import 'package:usque/widgets/proton_vpn_panel.dart';
import 'package:usque/widgets/usque_logo.dart';
import 'package:usque/widgets/window_titlebar.dart';

import 'app_test.dart' show FakeEngineClient;
import 'quality_test_support.dart' show qualityFixture;
import 'support/linux_golden_support.dart';

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
);

// These fixtures exercise the Linux widgets without reaching a session bus,
// keyring, service, socket, browser, filesystem picker, or network backend.
class _LinuxGoldenEngine extends FakeEngineClient
    implements LinuxServiceClient, ChainProfileClient, ProtonVpnClient {
  LinuxServiceState service = const LinuxServiceState(
    installed: true,
    compatible: true,
    canRepair: true,
  );
  bool cancelMaintenance = false;
  int maintenanceCalls = 0;
  Map<Object?, Object?> protonJob = const {};
  List<Map<Object?, Object?>> protonProfiles = const [];
  bool hasProtonProfile = false;
  DiagnosticSession? diagnosticSession;
  ConnectionTimeline timeline = const ConnectionTimeline();

  @override
  Future<DiagnosticSession?> getDiagnostics() async => diagnosticSession;

  @override
  Future<ConnectionTimeline> getConnectionTimeline() async => timeline;

  @override
  Future<DiagnosticSession> startDiagnostics(DiagnosticMode mode) =>
      throw StateError('Screenshot fixtures never run diagnostics.');

  @override
  Future<ProtonVpnResult> protonVpn(
    Map<String, Object?> command, {
    Uint8List? sensitiveInput,
  }) async {
    if (command['action'] == 'servers') {
      return ProtonVpnResult(
        metadata: {
          'server_offset': 0,
          'server_total': 12,
          'server_revision': 'fixture-revision',
          'servers': [
            {
              'id': 'logical-jp',
              'name': 'JP-FREE#1',
              'country': 'JP',
              'city': 'Tokyo',
              'load': 42,
            },
            {
              'id': 'logical-nl',
              'name': 'NL-FREE#2',
              'country': 'NL',
              'city': 'Amsterdam',
              'load': 28,
            },
            for (var index = 3; index <= 12; index++)
              {
                'id': 'logical-nl-$index',
                'name': 'NL-FREE#$index',
                'country': 'NL',
                'city': 'Amsterdam',
                'load': 28,
              },
          ],
        },
      );
    }
    if (command['action'] != 'get' && command['action'] != 'cancel') {
      throw StateError(
        'Screenshot fixtures never authenticate or alter Proton sessions.',
      );
    }
    return ProtonVpnResult(
      metadata: {'job': protonJob, 'profiles': protonProfiles},
    );
  }

  @override
  Future<EngineCapabilities?> getCapabilities() async => _capabilities;

  @override
  Future<LinuxServiceState> getLinuxServiceState() async => service;

  @override
  Future<void> maintainLinuxService({required bool install}) async {
    maintenanceCalls++;
    if (cancelMaintenance) {
      throw const EngineException(
        'LINUX_SERVICE_MAINTENANCE_FAILED',
        'authorization cancelled',
      );
    }
    throw StateError('Unexpected service mutation in a screenshot fixture.');
  }

  @override
  Future<ChainProfileResult> chainProfile(Map<String, Object?> request) async {
    if (request['action'] != 'list') {
      throw StateError('Unexpected chain mutation in a screenshot fixture.');
    }
    if (hasProtonProfile) {
      return const ChainProfileResult(
        profiles: [
          ChainProfileSummary(
            id: 'fixture-proton',
            revision: 'fixture-proton-revision',
            editRevision: 'fixture-proton-edit-revision',
            name: 'Proton Free',
            source: ChainSource.protonVpn,
            protocol: 'wireguard',
            host: '192.0.2.2',
            port: 51820,
          ),
        ],
      );
    }
    return const ChainProfileResult(
      profiles: [
        ChainProfileSummary(
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
        ),
      ],
    );
  }

  @override
  Future<List<ChainConfigurationFile>> pickChainConfigurations() =>
      throw StateError('Screenshot fixtures never open a native file picker.');
}

enum _Scene {
  homeIdle,
  homeConnected,
  homeDetails,
  homeError,
  homeLargeText,
  serviceInstall,
  serviceRepair,
  serviceMismatch,
  serviceCancelled,
  trayUnavailable,
  onboardingPermissions,
  onboardingKeyringCancelled,
  onboardingZeroTrustRtl,
  routing,
  chain,
  protonVerification,
  protonNodes,
  protonProfile,
  profiles,
  qualityGood,
  qualityDegraded,
  qualityDegradedPhone,
  qualityDisconnected,
  diagnosticsIdle,
  diagnosticsRunning,
  diagnosticsCompleted,
  diagnosticsCompletedPhone,
}

const _qualityScenes = {
  _Scene.qualityGood,
  _Scene.qualityDegraded,
  _Scene.qualityDegradedPhone,
  _Scene.qualityDisconnected,
};

const _diagnosticsScenes = {
  _Scene.diagnosticsIdle,
  _Scene.diagnosticsRunning,
  _Scene.diagnosticsCompleted,
  _Scene.diagnosticsCompletedPhone,
};

final _diagnosticsStarted = DateTime.utc(2026, 9, 2, 11, 58);

DiagnosticSession _runningDiagnostics() => DiagnosticSession(
  sessionId: 'fixture-running',
  state: DiagnosticSessionState.running,
  startedAt: _diagnosticsStarted,
  mode: DiagnosticMode.standard,
  currentCheck: 'transport.h3_connect',
  progressPercent: 45,
  summary: const DiagnosticSummary(passed: 4),
  findings: const [
    DiagnosticFinding(
      checkId: 'engine.control_channel',
      category: DiagnosticCategory.localComponent,
      status: DiagnosticCheckStatus.passed,
    ),
    DiagnosticFinding(
      checkId: 'engine.configuration',
      category: DiagnosticCategory.localComponent,
      status: DiagnosticCheckStatus.passed,
    ),
    DiagnosticFinding(
      checkId: 'physical.network_present',
      category: DiagnosticCategory.physicalNetwork,
      status: DiagnosticCheckStatus.passed,
    ),
    DiagnosticFinding(
      checkId: 'physical.dns_available',
      category: DiagnosticCategory.physicalNetwork,
      status: DiagnosticCheckStatus.passed,
    ),
    DiagnosticFinding(
      checkId: 'transport.h3_connect',
      category: DiagnosticCategory.transport,
      status: DiagnosticCheckStatus.running,
    ),
  ],
);

// Problems sit in later categories, so ordering by category alone would
// put them below the passing checks.
DiagnosticSession _completedDiagnostics() => DiagnosticSession(
  sessionId: 'fixture-completed',
  state: DiagnosticSessionState.completed,
  startedAt: _diagnosticsStarted,
  completedAt: _diagnosticsStarted.add(const Duration(seconds: 12)),
  mode: DiagnosticMode.standard,
  progressPercent: 100,
  summary: const DiagnosticSummary(passed: 5, warnings: 1, failed: 1),
  findings: const [
    DiagnosticFinding(
      checkId: 'engine.control_channel',
      category: DiagnosticCategory.localComponent,
      status: DiagnosticCheckStatus.passed,
    ),
    DiagnosticFinding(
      checkId: 'engine.configuration',
      category: DiagnosticCategory.localComponent,
      status: DiagnosticCheckStatus.passed,
    ),
    DiagnosticFinding(
      checkId: 'physical.network_present',
      category: DiagnosticCategory.physicalNetwork,
      status: DiagnosticCheckStatus.passed,
    ),
    DiagnosticFinding(
      checkId: 'physical.dns_available',
      category: DiagnosticCategory.physicalNetwork,
      status: DiagnosticCheckStatus.passed,
    ),
    DiagnosticFinding(
      checkId: 'transport.h3_connect',
      category: DiagnosticCategory.transport,
      status: DiagnosticCheckStatus.failed,
      severity: DiagnosticSeverity.error,
      remediationKey: 'try_http2',
      failure: TransportFailureInfo(
        code: 'H3_UDP_UNREACHABLE',
        stage: 'quic_handshake',
        transport: 'h3',
        addressFamily: 'ipv4',
        retryable: true,
        fallbackAllowed: true,
        remediationKey: 'try_http2',
      ),
    ),
    DiagnosticFinding(
      checkId: 'transport.h2_connect',
      category: DiagnosticCategory.transport,
      status: DiagnosticCheckStatus.passed,
    ),
    DiagnosticFinding(
      checkId: 'quality.packet_loss',
      category: DiagnosticCategory.tunnel,
      status: DiagnosticCheckStatus.warning,
      severity: DiagnosticSeverity.warning,
      remediationKey: 'nq_network',
    ),
  ],
);

const _diagnosticsTimeline = ConnectionTimeline(
  events: [
    ConnectionTimelineEvent(
      sequence: 1,
      eventType: ConnectionTimelineEventType.attemptStarted,
      elapsedMilliseconds: 0,
      transport: 'h3',
      addressFamily: 'ipv4',
    ),
    ConnectionTimelineEvent(
      sequence: 2,
      eventType: ConnectionTimelineEventType.fallbackStarted,
      elapsedMilliseconds: 3100,
      transport: 'h2',
    ),
    ConnectionTimelineEvent(
      sequence: 3,
      eventType: ConnectionTimelineEventType.tunnelReady,
      elapsedMilliseconds: 4200,
      transport: 'h2',
      addressFamily: 'ipv4',
    ),
  ],
);

typedef _Fixture = ({
  _Scene scene,
  String name,
  Size size,
  LocalePreference locale,
  bool dark,
  double scale,
});

const _fixtures = <_Fixture>[
  (
    scene: _Scene.homeIdle,
    name: 'home_idle_en_light',
    size: Size(1280, 900),
    locale: LocalePreference.english,
    dark: false,
    scale: 1,
  ),
  (
    scene: _Scene.homeConnected,
    name: 'home_connected_en_dark',
    size: Size(1280, 900),
    locale: LocalePreference.english,
    dark: true,
    scale: 1,
  ),
  (
    scene: _Scene.homeDetails,
    name: 'home_details_zh_dark',
    size: Size(1440, 1000),
    locale: LocalePreference.simplifiedChinese,
    dark: true,
    scale: 1,
  ),
  (
    scene: _Scene.homeError,
    name: 'home_error_en_light',
    size: Size(1280, 900),
    locale: LocalePreference.english,
    dark: false,
    scale: 1,
  ),
  (
    scene: _Scene.homeLargeText,
    name: 'home_text_scale_200_en_light',
    size: Size(1600, 1200),
    locale: LocalePreference.english,
    dark: false,
    scale: 2,
  ),
  (
    scene: _Scene.serviceInstall,
    name: 'settings_service_install_en_light',
    size: Size(1280, 1000),
    locale: LocalePreference.english,
    dark: false,
    scale: 1,
  ),
  (
    scene: _Scene.serviceRepair,
    name: 'settings_service_repair_zh_dark',
    size: Size(1280, 1000),
    locale: LocalePreference.simplifiedChinese,
    dark: true,
    scale: 1,
  ),
  (
    scene: _Scene.serviceMismatch,
    name: 'settings_service_mismatch_en_light',
    size: Size(1280, 1000),
    locale: LocalePreference.english,
    dark: false,
    scale: 1,
  ),
  (
    scene: _Scene.serviceCancelled,
    name: 'settings_service_cancelled_en_dark',
    size: Size(1280, 1000),
    locale: LocalePreference.english,
    dark: true,
    scale: 1,
  ),
  (
    scene: _Scene.trayUnavailable,
    name: 'settings_tray_unavailable_en_light',
    size: Size(1280, 1000),
    locale: LocalePreference.english,
    dark: false,
    scale: 1,
  ),
  (
    scene: _Scene.onboardingPermissions,
    name: 'onboarding_permissions_en_light',
    size: Size(1280, 900),
    locale: LocalePreference.english,
    dark: false,
    scale: 1,
  ),
  (
    scene: _Scene.onboardingKeyringCancelled,
    name: 'onboarding_keyring_cancelled_zh_dark',
    size: Size(1280, 900),
    locale: LocalePreference.simplifiedChinese,
    dark: true,
    scale: 1,
  ),
  (
    scene: _Scene.onboardingZeroTrustRtl,
    name: 'onboarding_zero_trust_fa_rtl_dark',
    size: Size(1280, 1000),
    locale: LocalePreference.persian,
    dark: true,
    scale: 1,
  ),
  (
    scene: _Scene.routing,
    name: 'routing_rules_zh_dark',
    size: Size(1280, 1000),
    locale: LocalePreference.simplifiedChinese,
    dark: true,
    scale: 1,
  ),
  (
    scene: _Scene.chain,
    name: 'chain_wireguard_en_light',
    size: Size(1280, 1000),
    locale: LocalePreference.english,
    dark: false,
    scale: 1,
  ),
  (
    scene: _Scene.protonVerification,
    name: 'chain_proton_verification_zh_phone',
    size: Size(390, 1000),
    locale: LocalePreference.simplifiedChinese,
    dark: false,
    scale: 1,
  ),
  (
    scene: _Scene.protonNodes,
    name: 'chain_proton_nodes_en_dark',
    size: Size(1280, 1100),
    locale: LocalePreference.english,
    dark: true,
    scale: 1,
  ),
  (
    scene: _Scene.protonProfile,
    name: 'chain_proton_profile_text_scale_200_en_light',
    size: Size(1600, 1400),
    locale: LocalePreference.english,
    dark: false,
    scale: 2,
  ),
  (
    scene: _Scene.profiles,
    name: 'profiles_zh_light',
    size: Size(1280, 900),
    locale: LocalePreference.simplifiedChinese,
    dark: false,
    scale: 1,
  ),
  (
    scene: _Scene.qualityGood,
    name: 'network_quality_good_en_light',
    size: Size(1280, 1500),
    locale: LocalePreference.english,
    dark: false,
    scale: 1,
  ),
  (
    scene: _Scene.qualityDegraded,
    name: 'network_quality_degraded_zh_dark',
    size: Size(1280, 2400),
    locale: LocalePreference.simplifiedChinese,
    dark: true,
    scale: 1,
  ),
  (
    scene: _Scene.qualityDegradedPhone,
    name: 'network_quality_degraded_en_phone',
    size: Size(390, 3000),
    locale: LocalePreference.english,
    dark: false,
    scale: 1,
  ),
  (
    scene: _Scene.qualityDisconnected,
    name: 'network_quality_disconnected_en_dark',
    size: Size(1280, 1000),
    locale: LocalePreference.english,
    dark: true,
    scale: 1,
  ),
  (
    scene: _Scene.diagnosticsIdle,
    name: 'diagnostics_idle_en_light',
    size: Size(1280, 1100),
    locale: LocalePreference.english,
    dark: false,
    scale: 1,
  ),
  (
    scene: _Scene.diagnosticsRunning,
    name: 'diagnostics_running_zh_dark',
    size: Size(1280, 1300),
    locale: LocalePreference.simplifiedChinese,
    dark: true,
    scale: 1,
  ),
  (
    scene: _Scene.diagnosticsCompleted,
    name: 'diagnostics_completed_en_light',
    size: Size(1280, 1700),
    locale: LocalePreference.english,
    dark: false,
    scale: 1,
  ),
  (
    scene: _Scene.diagnosticsCompletedPhone,
    name: 'diagnostics_completed_zh_phone',
    size: Size(390, 2600),
    locale: LocalePreference.simplifiedChinese,
    dark: false,
    scale: 1,
  ),
];

Widget _host(AppController app, _Fixture fixture) {
  final locale = switch (fixture.locale) {
    LocalePreference.simplifiedChinese => const Locale('zh', 'CN'),
    LocalePreference.persian => const Locale('fa'),
    _ => const Locale('en'),
  };
  final home = switch (fixture.scene) {
    _Scene.onboardingPermissions ||
    _Scene.onboardingKeyringCancelled ||
    _Scene.onboardingZeroTrustRtl => OnboardingScreen(controller: app),
    _Scene.routing => GeoDirectSettingsScreen(controller: app),
    _ when _qualityScenes.contains(fixture.scene) => NetworkQualityScreen(
      controller: app,
    ),
    _ when _diagnosticsScenes.contains(fixture.scene) => DiagnosticsScreen(
      controller: app,
    ),
    _Scene.chain ||
    _Scene.protonVerification ||
    _Scene.protonNodes ||
    _Scene.protonProfile => ChainProxyScreen(
      controller: app,
      now: () => DateTime.utc(2026, 9, 2, 12, 1),
    ),
    _ => ShellScreen(controller: app),
  };
  return MaterialApp(
    debugShowCheckedModeBanner: false,
    theme: fixture.dark ? UsqueTheme.dark() : UsqueTheme.light(),
    locale: locale,
    supportedLocales: const [Locale('en'), Locale('zh', 'CN'), Locale('fa')],
    localizationsDelegates: const [
      GlobalMaterialLocalizations.delegate,
      GlobalWidgetsLocalizations.delegate,
      GlobalCupertinoLocalizations.delegate,
    ],
    builder: (context, child) => MediaQuery(
      data: MediaQuery.of(context).copyWith(
        textScaler: TextScaler.linear(fixture.scale),
        disableAnimations: true,
      ),
      child: child!,
    ),
    home: home,
  );
}

void _configure(
  AppController app,
  _LinuxGoldenEngine engine,
  _Fixture fixture,
) {
  app
    ..localePreference = fixture.locale
    ..initialized = true
    ..onboardingTermsAccepted = true
    ..onboardingStep = 3;
  app.profileIdentityStates = {
    for (final profile in app.profiles) profile.id: ProfileIdentityState.ready,
  };
  switch (fixture.scene) {
    case _Scene.serviceInstall:
    case _Scene.serviceRepair:
    case _Scene.serviceMismatch:
    case _Scene.serviceCancelled:
    case _Scene.trayUnavailable:
      app.section = AppSection.settings;
    case _Scene.profiles:
      app.section = AppSection.profiles;
      final active = app.activeProfile;
      app.profiles = [
        active,
        active.copyWith(id: 'work', name: '工作账号'),
        active.copyWith(id: 'travel', name: '旅行账号'),
      ];
      app.profileIdentityStates = {
        for (final profile in app.profiles)
          profile.id: ProfileIdentityState.ready,
      };
    default:
      break;
  }
  switch (fixture.scene) {
    case _Scene.serviceInstall:
    case _Scene.serviceCancelled:
    case _Scene.onboardingPermissions:
      engine.service = const LinuxServiceState(canInstall: true);
      engine.cancelMaintenance = fixture.scene == _Scene.serviceCancelled;
      if (fixture.scene == _Scene.onboardingPermissions) {
        app.onboardingStep = 1;
      }
    case _Scene.serviceMismatch:
      engine.service = const LinuxServiceState(installed: true);
    case _Scene.trayUnavailable:
      app.noteTrayAvailability(false);
    case _Scene.onboardingKeyringCancelled:
      app.lastError = userFacingError(
        app.strings,
        const EngineException('SECURE_STORAGE_FAILED', 'unlock cancelled'),
      );
    case _Scene.homeError:
      app.snapshot = const EngineSnapshot(
        phase: ConnectionPhase.error,
        errorCode: 'AGENT_UNREACHABLE',
      );
      app.lastError = userFacingFailure(app.strings, code: 'AGENT_UNREACHABLE');
    case _Scene.routing:
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
              'ads.example.com',
              RoutingAction.reject,
              id: 'fixture-reject',
            ),
            RoutingRule.create(
              '192.0.2.7',
              RoutingAction.proxy,
              id: 'fixture-proxy',
            ),
          ],
        ),
      );
    case _Scene.diagnosticsRunning:
      engine.diagnosticSession = _runningDiagnostics();
      engine.timeline = _diagnosticsTimeline;
    case _Scene.diagnosticsCompleted:
    case _Scene.diagnosticsCompletedPhone:
      engine.diagnosticSession = _completedDiagnostics();
      engine.timeline = _diagnosticsTimeline;
    case _Scene.chain:
      app.sharedNetwork = app.sharedNetwork.copyWith(
        chainExit: const ChainExitSettings(
          enabled: true,
          source: ChainSource.wireguardCustom,
          profileId: 'office-wireguard',
        ),
      );
    case _Scene.protonVerification:
    case _Scene.protonNodes:
    case _Scene.protonProfile:
      app.sharedNetwork = app.sharedNetwork.copyWith(
        chainExit: const ChainExitSettings(source: ChainSource.protonVpn),
      );
      if (fixture.scene == _Scene.protonVerification) {
        engine.protonJob = const {
          'id': 'fixture-proton',
          'state': 'waiting_verification',
          'phase': 'credentialless',
          'challenge_id': 'fixture-challenge',
        };
      } else {
        engine.hasProtonProfile = true;
        engine.protonProfiles = [
          {
            'id': 'fixture-proton',
            'name': 'Proton Free',
            'node_name': 'JP-FREE#1',
            'server_id': 'logical-jp',
            'server_count': fixture.scene == _Scene.protonNodes ? 12 : 119,
            'needs_login': false,
          },
        ];
      }
    default:
      break;
  }
  // Enabling the capability starts an asynchronous authoritative readback.
  // Seed that fake readback before it can replace the controller's draft.
  engine.storedProfiles = [
    for (final profile in app.profiles)
      app.sharedNetwork.copyWith(id: profile.id, name: profile.name),
  ];
  engine.storedActiveProfileId = app.activeProfileId;
  app.engineCapabilities = _capabilities;
}

/// A running diagnostic keeps an indeterminate spinner, which never settles;
/// a fixed pump in fake time renders the same frame on every run.
Future<void> _settle(WidgetTester tester, _Fixture fixture) async {
  if (fixture.scene == _Scene.diagnosticsRunning) {
    await tester.pump(const Duration(milliseconds: 600));
    await tester.pump(const Duration(milliseconds: 600));
    return;
  }
  await tester.pumpAndSettle();
}

void _expectInViewport(WidgetTester tester, Finder finder, Size size) {
  expect(finder, findsOneWidget);
  final bounds = tester.getRect(finder);
  expect(bounds.left, greaterThanOrEqualTo(0));
  expect(bounds.top, greaterThanOrEqualTo(0));
  expect(bounds.right, lessThanOrEqualTo(size.width));
  expect(bounds.bottom, lessThanOrEqualTo(size.height));
}

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  final supportedHost = Platform.isLinux && Abi.current() == Abi.linuxX64;
  group(
    'Linux desktop exact goldens',
    () {
      setUpAll(loadLinuxGoldenFonts);
      setUp(() {
        SharedPreferences.setMockInitialValues(<String, Object>{});
      });

      for (final fixture in _fixtures) {
        testWidgets(
          fixture.name,
          (tester) async {
            tester.view.devicePixelRatio = 1;
            tester.view.physicalSize = fixture.size;
            addTearDown(tester.view.resetDevicePixelRatio);
            addTearDown(tester.view.resetPhysicalSize);
            final engine = _LinuxGoldenEngine();
            var now = DateTime.utc(2026, 9, 2, 12);
            final app = AppController(
              engine,
              qualityController: NetworkQualityController(
                engine,
                now: () => now,
                autoTick: false,
              ),
              adsClock: () => now,
            );
            addTearDown(app.dispose);
            _configure(app, engine, fixture);
            final connected =
                fixture.scene == _Scene.homeConnected ||
                fixture.scene == _Scene.homeDetails;
            final qualityState = switch (fixture.scene) {
              _Scene.qualityGood => 'h3',
              _Scene.qualityDegraded ||
              _Scene.qualityDegradedPhone => 'degraded',
              _ => null,
            };
            if (qualityState != null) {
              for (var second = 0; second < 60; second++) {
                now = DateTime.utc(
                  2026,
                  9,
                  2,
                  12,
                ).add(Duration(seconds: second));
                app.snapshot = EngineSnapshot(
                  phase: ConnectionPhase.connected,
                  transport: 'HTTP/3',
                  addressFamily: 'IPv4',
                  downloadedBytes: second * 750000,
                  uploadedBytes: second * 210000,
                  networkQuality: qualityFixture(
                    now,
                    state: qualityState,
                    rtt: 32 + (second % 11) * 3,
                  ),
                );
              }
            }
            if (connected) {
              var downloaded = 0;
              var uploaded = 0;
              for (var second = 0; second < 60; second++) {
                now = DateTime.utc(
                  2026,
                  9,
                  2,
                  12,
                ).add(Duration(seconds: second));
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
            final boundary = GlobalKey();
            await tester.pumpWidget(
              RepaintBoundary(key: boundary, child: _host(app, fixture)),
            );
            await tester.runAsync(() async {
              final context = tester.element(find.byType(MaterialApp));
              await precacheImage(
                AssetImage(
                  UsqueLogo.assetFor(
                    fixture.dark ? Brightness.dark : Brightness.light,
                  ),
                ),
                context,
              );
              if (connected) {
                await precacheImage(
                  const AssetImage('assets/flags/w80/sg.png'),
                  context,
                );
              }
              if (fixture.scene == _Scene.protonNodes) {
                await Future.wait([
                  for (final code in ['jp', 'nl'])
                    precacheImage(
                      AssetImage('assets/flags/w80/$code.png'),
                      context,
                    ),
                ]);
              }
              if (fixture.scene == _Scene.routing) {
                await Future.wait([
                  for (final code in kCountryFlagCodes)
                    precacheImage(
                      AssetImage('assets/flags/w80/$code.png'),
                      context,
                    ),
                ]);
              }
            });
            await _settle(tester, fixture);
            expect(engine.maintenanceCalls, 0);
            expect(find.byType(WindowTitleBar), findsNothing);
            switch (fixture.scene) {
              case _Scene.homeIdle:
              case _Scene.homeConnected:
              case _Scene.homeDetails:
              case _Scene.homeError:
              case _Scene.homeLargeText:
                for (final key in [
                  'home-tun-switch',
                  'home-system-proxy-switch',
                  'home-chain-proxy-switch',
                  'home-chain-proxy-settings',
                  'home-local-proxies',
                ]) {
                  expect(find.byKey(ValueKey(key)), findsOneWidget);
                }
              default:
                break;
            }
            switch (fixture.scene) {
              case _Scene.homeDetails:
                final details = find.text(
                  app.strings.get('connection_details'),
                );
                await tester.ensureVisible(details);
                await tester.tap(details);
                await tester.pumpAndSettle();
                await Scrollable.ensureVisible(tester.element(details));
                expect(
                  find.widgetWithText(SelectableText, '198.51.100.10'),
                  findsOneWidget,
                );
                expect(
                  find.widgetWithText(SelectableText, '2001:db8::10'),
                  findsOneWidget,
                );
              case _Scene.serviceInstall:
              case _Scene.onboardingPermissions:
                expect(find.byType(LinuxServiceSection), findsOneWidget);
                expect(
                  find.text(app.strings.get('linux_service_install')),
                  findsOneWidget,
                );
              case _Scene.serviceRepair:
                expect(
                  find.text(app.strings.get('linux_service_repair')),
                  findsOneWidget,
                );
              case _Scene.serviceMismatch:
                expect(
                  find.text(app.strings.get('linux_service_reinstall')),
                  findsOneWidget,
                );
                expect(
                  find.byKey(const ValueKey('linux-service-maintain')),
                  findsNothing,
                );
              case _Scene.serviceCancelled:
                final action = find.byKey(
                  const ValueKey('linux-service-maintain'),
                );
                await tester.tap(action);
                await tester.pumpAndSettle();
                expect(engine.maintenanceCalls, 1);
                expect(
                  tester.widget<OutlinedButton>(action).onPressed,
                  isNotNull,
                );
              case _Scene.trayUnavailable:
                final closeToTray = find.widgetWithText(
                  SwitchListTile,
                  app.strings.get('close_to_tray'),
                );
                await Scrollable.ensureVisible(
                  tester.element(closeToTray),
                  alignment: 0.5,
                );
                expect(
                  tester.widget<SwitchListTile>(closeToTray).value,
                  isFalse,
                );
                expect(
                  tester.widget<SwitchListTile>(closeToTray).onChanged,
                  isNull,
                );
              case _Scene.onboardingKeyringCancelled:
                expect(
                  find.text(app.strings.get('linux_keyring_unavailable')),
                  findsOneWidget,
                );
                expect(
                  tester
                      .widget<FilledButton>(
                        find.widgetWithText(
                          FilledButton,
                          app.strings.get('finish_setup'),
                        ),
                      )
                      .onPressed,
                  isNotNull,
                );
              case _Scene.routing:
                expect(
                  tester
                      .widget<SwitchListTile>(
                        find.byKey(const ValueKey('routing-ads')),
                      )
                      .value,
                  isTrue,
                );
                for (final target in [
                  'example.com',
                  'ads.example.com',
                  '192.0.2.7/32',
                ]) {
                  _expectInViewport(tester, find.text(target), fixture.size);
                }
              case _Scene.chain:
                expect(
                  tester
                      .widget<ChainSourcePicker>(find.byType(ChainSourcePicker))
                      .source,
                  ChainSource.wireguardCustom,
                );
                expect(
                  tester
                      .widget<SwitchListTile>(
                        find.byKey(const ValueKey('chain-proxy-toggle')),
                      )
                      .value,
                  isTrue,
                );
                expect(find.text('Office WireGuard'), findsWidgets);
                _expectInViewport(
                  tester,
                  find.byKey(const ValueKey('chain-profile-office-wireguard')),
                  fixture.size,
                );
              case _Scene.protonVerification:
              case _Scene.protonNodes:
              case _Scene.protonProfile:
                expect(find.byType(ProtonVpnPanel), findsOneWidget);
                expect(
                  find.text(app.strings.chain('import_file')),
                  findsNothing,
                );
                if (fixture.scene == _Scene.protonNodes) {
                  final change = find.byKey(
                    const ValueKey('proton-account-server-fixture-proton'),
                  );
                  await tester.ensureVisible(change);
                  await tester.tap(change);
                  await tester.pumpAndSettle();
                }
                final target = switch (fixture.scene) {
                  _Scene.protonVerification => find.text(
                    app.strings.proton('verify'),
                  ),
                  _Scene.protonNodes => find.text('NL-FREE#2'),
                  _ => find.byKey(
                    const ValueKey('proton-account-menu-fixture-proton'),
                  ),
                };
                if (fixture.scene != _Scene.protonNodes) {
                  await tester.ensureVisible(target);
                  await tester.pumpAndSettle();
                }
                _expectInViewport(tester, target, fixture.size);
              case _Scene.diagnosticsCompleted:
              case _Scene.diagnosticsCompletedPhone:
                // The failed check's explanation is part of the reviewed
                // layout, so open it before capturing.
                final failed = find.byKey(
                  const PageStorageKey<String>(
                    'diagnostic-transport.h3_connect',
                  ),
                );
                await tester.ensureVisible(failed);
                await tester.tap(failed);
                await tester.pumpAndSettle();
              case _Scene.onboardingZeroTrustRtl:
                final method = find.text(app.strings.get('zero_trust_title'));
                await tester.ensureVisible(method);
                await tester.tap(method);
                await tester.pumpAndSettle();
                expect(
                  Directionality.of(
                    tester.element(find.byType(OnboardingScreen)),
                  ),
                  TextDirection.rtl,
                );
              default:
                break;
            }
            FocusManager.instance.primaryFocus?.unfocus();
            await _settle(tester, fixture);
            if ({
              _Scene.serviceInstall,
              _Scene.serviceRepair,
              _Scene.serviceMismatch,
              _Scene.serviceCancelled,
              _Scene.onboardingPermissions,
            }.contains(fixture.scene)) {
              _expectInViewport(
                tester,
                find.byType(LinuxServiceSection),
                fixture.size,
              );
            }
            if (fixture.scene == _Scene.trayUnavailable) {
              _expectInViewport(
                tester,
                find.widgetWithText(
                  SwitchListTile,
                  app.strings.get('close_to_tray'),
                ),
                fixture.size,
              );
            }
            if (fixture.scene == _Scene.onboardingKeyringCancelled) {
              _expectInViewport(
                tester,
                find.text(app.strings.get('linux_keyring_unavailable')),
                fixture.size,
              );
            }
            expect(tester.takeException(), isNull);
            expect(find.byType(ErrorWidget), findsNothing);
            await expectLater(
              find.byKey(boundary),
              matchesGoldenFile('goldens/linux/${fixture.name}.png'),
            );
            await tester.pumpWidget(const SizedBox.shrink());
            // An active session polls the engine until it is no longer
            // observed; stop it before the test checks for pending timers.
            app.diagnostics.setObservationVisible(false);
          },
          variant: const TargetPlatformVariant({TargetPlatform.linux}),
          tags: const ['golden', 'linux-golden'],
        );
      }
    },
    skip: supportedHost ? false : 'Linux amd64 rasterization baseline only.',
  );
}
