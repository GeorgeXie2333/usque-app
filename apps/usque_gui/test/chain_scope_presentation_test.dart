import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:usque/core/app_strings.dart';
import 'package:usque/core/chain_scope_presentation.dart';
import 'package:usque/core/chain_strings.dart';
import 'package:usque/core/l10n/chain.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/models/network_settings.dart';
import 'package:usque/screens/home_screen.dart';
import 'package:usque/services/engine_client.dart'
    show MethodChannelEngineClient;
import 'package:usque/state/app_controller.dart';
import 'package:usque/widgets/chain_current_connection.dart';
import 'package:usque/widgets/home_desktop_controls.dart';

import 'app_test.dart' show FakeEngineClient;
import 'ui_workflow_test.dart' show workflowHost;

ChainProfileSummary _summary(ChainSource source) => ChainProfileSummary(
  id: 'scope-exit',
  revision: 'r1',
  editRevision: 'e1',
  name: 'Current exit',
  protocol: source == ChainSource.socks5Proxy ? 'socks5' : 'http',
  source: source,
  host: 'proxy.example',
  port: 1080,
);

UsqueProfile _profile({
  bool tunnel = true,
  ChainSource source = ChainSource.socks5Proxy,
}) => UsqueProfile.defaultProfile().copyWith(
  chainExit: ChainExitSettings(enabled: true, source: source),
  frontends: FrontendSettings(tunnel: tunnel, socks5: true, http: true),
);

EngineSnapshot _snapshot({
  ConnectionPhase phase = ConnectionPhase.connected,
  FrontendPhase? tunnel = FrontendPhase.active,
  bool proxyActive = true,
  FrontendPhase proxyPhase = FrontendPhase.active,
  String? killSwitch,
  String? udp,
  bool lockdown = false,
  bool current = true,
  ChainSource source = ChainSource.socks5Proxy,
}) => EngineSnapshot(
  phase: phase,
  killSwitchState: killSwitch,
  platformLockdown: lockdown,
  chainExit: ChainExitStatus(
    stage: current ? 'connected' : 'disabled',
    currentProfile: current ? _summary(source) : null,
    proxyUdp: udp,
  ),
  frontends: [
    if (tunnel != null)
      FrontendRuntimeStatus(kind: FrontendKind.tunnel, phase: tunnel),
    if (proxyActive)
      FrontendRuntimeStatus(kind: FrontendKind.socks5, phase: proxyPhase),
  ],
);

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  final strings = AppStrings(LocalePreference.english);

  test('only HTTP/SOCKS sessions get scope hints, independently of UDP', () {
    for (final source in ChainSource.values) {
      for (final udp in [null, 'unknown', 'available', 'unavailable']) {
        final presentation = ChainScopePresentation.of(
          _snapshot(source: source, udp: udp),
          _profile(source: source),
          isAndroid: false,
        );
        expect(
          presentation.scopeKey,
          source.isProxy ? 'scope_bypass' : null,
          reason: '$source / $udp',
        );
      }
    }
  });

  test(
    'observed frontends take priority and absent observations stay unknown',
    () {
      final vpn = _profile();
      final proxy = _profile(tunnel: false);
      expect(
        ChainScopePresentation.of(
          _snapshot(),
          proxy,
          isAndroid: false,
        ).scopeKey,
        'scope_bypass',
      );
      expect(
        ChainScopePresentation.of(
          _snapshot(tunnel: FrontendPhase.disabled),
          vpn,
          isAndroid: false,
        ).scopeKey,
        'scope_proxy_only',
      );
      expect(
        ChainScopePresentation.of(
          _snapshot(tunnel: null),
          proxy,
          isAndroid: false,
        ).scopeKey,
        'scope_proxy_only',
      );
      expect(
        ChainScopePresentation.of(
          _snapshot(tunnel: null),
          null,
          isAndroid: false,
        ).isEmpty,
        isTrue,
      );
      expect(
        ChainScopePresentation.of(
          _snapshot(tunnel: FrontendPhase.error, proxyActive: false),
          vpn,
          isAndroid: false,
        ).isEmpty,
        isTrue,
      );
    },
  );

  test(
    'ordinary networking warning requires failed VPN and explicit inactive state',
    () {
      for (final state in [
        null,
        'inactive',
        'active',
        'error',
        'notApplicable',
      ]) {
        for (final phase in [
          ConnectionPhase.error,
          ConnectionPhase.connected,
        ]) {
          final presentation = ChainScopePresentation.of(
            _snapshot(
              phase: phase,
              killSwitch: state,
              current: false,
              tunnel: FrontendPhase.disabled,
            ),
            _profile(),
            isAndroid: false,
          );
          expect(
            presentation.scopeKey,
            phase == ConnectionPhase.error && state == 'inactive'
                ? 'scope_interrupted'
                : null,
          );
        }
      }
      for (final profile in [null, _profile(tunnel: false)]) {
        expect(
          ChainScopePresentation.of(
            _snapshot(
              phase: ConnectionPhase.error,
              tunnel: null,
              killSwitch: 'inactive',
            ),
            profile,
            isAndroid: false,
          ).scopeKey,
          profile == null ? null : 'scope_proxy_only',
        );
      }
    },
  );

  test('current source wins over the previously applied selection', () {
    expect(
      ChainScopePresentation.of(
        _snapshot(source: ChainSource.wireguardCustom),
        _profile(),
        isAndroid: false,
      ).isEmpty,
      isTrue,
    );
    for (final phase in [
      ConnectionPhase.disconnected,
      ConnectionPhase.disconnecting,
    ]) {
      expect(
        ChainScopePresentation.of(
          _snapshot(phase: phase),
          _profile(),
          isAndroid: true,
        ).isEmpty,
        isTrue,
      );
    }
  });

  test(
    'retired native VPN error metadata preserves scope without an applied profile',
    () {
      for (final source in [ChainSource.httpProxy, ChainSource.socks5Proxy]) {
        for (final state in [
          null,
          'inactive',
          'active',
          'error',
          'notApplicable',
        ]) {
          final presentation = ChainScopePresentation.of(
            _snapshot(
              phase: ConnectionPhase.error,
              source: source,
              tunnel: FrontendPhase.error,
              proxyActive: false,
              killSwitch: state,
            ),
            null,
            isAndroid: false,
          );
          expect(
            presentation.scopeKey,
            state == 'inactive' ? 'scope_interrupted' : null,
          );
        }
      }
      for (final tunnel in [null, FrontendPhase.disabled]) {
        expect(
          ChainScopePresentation.of(
            _snapshot(
              phase: ConnectionPhase.error,
              tunnel: tunnel,
              proxyActive: false,
              killSwitch: 'inactive',
            ),
            null,
            isAndroid: false,
          ).scopeKey,
          tunnel == FrontendPhase.disabled ? 'scope_proxy_only' : null,
        );
      }
    },
  );

  test(
    'failed proxy-only sessions retain their coverage limit without a VPN interruption claim',
    () {
      for (final source in [ChainSource.httpProxy, ChainSource.socks5Proxy]) {
        for (final isAndroid in [false, true]) {
          for (final explicitTunnelOff in [false, true]) {
            final scope = ChainScopePresentation.of(
              _snapshot(
                phase: ConnectionPhase.error,
                source: source,
                tunnel: explicitTunnelOff ? FrontendPhase.disabled : null,
                proxyPhase: FrontendPhase.error,
                killSwitch: 'notApplicable',
              ),
              explicitTunnelOff
                  ? null
                  : _profile(source: source, tunnel: false),
              isAndroid: isAndroid,
            );
            expect(scope.scopeKey, 'scope_proxy_only');
            expect(scope.androidSettings, isFalse);
            expect(
              scope.message(strings),
              isNot(contains(strings.chain('scope_interrupted'))),
            );
          }
          final fromAppliedAndFailedListener = ChainScopePresentation.of(
            _snapshot(
              phase: ConnectionPhase.error,
              current: false,
              tunnel: null,
              proxyPhase: FrontendPhase.error,
              killSwitch: 'notApplicable',
            ),
            _profile(source: source, tunnel: false),
            isAndroid: isAndroid,
          );
          expect(fromAppliedAndFailedListener.scopeKey, 'scope_proxy_only');
          for (final tunnel in [
            null,
            FrontendPhase.error,
            FrontendPhase.active,
          ]) {
            final unknownOrVpn = ChainScopePresentation.of(
              _snapshot(
                phase: ConnectionPhase.error,
                source: source,
                tunnel: tunnel,
                proxyPhase: FrontendPhase.error,
                killSwitch: 'notApplicable',
              ),
              null,
              isAndroid: isAndroid,
            );
            expect(unknownOrVpn.scopeKey, isNot('scope_proxy_only'));
          }
        }
      }
      expect(
        ChainScopePresentation.of(
          _snapshot(
            phase: ConnectionPhase.error,
            current: false,
            tunnel: null,
            proxyActive: false,
          ),
          _profile(tunnel: false),
          isAndroid: false,
        ).isEmpty,
        isTrue,
      );
      expect(
        ChainScopePresentation.of(
          _snapshot(
            phase: ConnectionPhase.error,
            tunnel: FrontendPhase.error,
            killSwitch: 'inactive',
          ),
          _profile(tunnel: false),
          isAndroid: false,
        ).scopeKey,
        'scope_interrupted',
      );
    },
  );

  test(
    'Android advice is conditional when Lockdown is false or unreported',
    () {
      final unknown = ChainScopePresentation.of(
        _snapshot(),
        _profile(),
        isAndroid: true,
      );
      expect(unknown.androidSettings, isTrue);
      expect(unknown.message(strings), contains('To keep blocking after'));
      expect(
        ChainScopePresentation.of(
          _snapshot(lockdown: true),
          _profile(),
          isAndroid: true,
        ).androidSettings,
        isFalse,
      );
      expect(
        ChainScopePresentation.of(
          _snapshot(tunnel: FrontendPhase.disabled),
          _profile(tunnel: false),
          isAndroid: true,
        ).androidSettings,
        isFalse,
      );
    },
  );

  test(
    'Android stopped-native method replies retain only explicit terminal VPN evidence',
    () async {
      const channel = MethodChannel('io.github.georgexie2333.usque/engine');
      final messenger =
          TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger;
      addTearDown(() => messenger.setMockMethodCallHandler(channel, null));
      for (final source in [
        ChainSource.httpProxy,
        ChainSource.socks5Proxy,
        ChainSource.wireguardCustom,
      ]) {
        for (final state in [
          null,
          'active',
          'inactive',
          'notApplicable',
          'notSupported',
          'error',
        ]) {
          for (final pending in [false, true]) {
            messenger.setMockMethodCallHandler(
              channel,
              (call) async => <Object?, Object?>{
                'phase': 'error',
                'vpn_gate': {
                  'stage': 'error',
                  'current_profile': {
                    'id': 'scope-exit',
                    'revision': 'r1',
                    'edit_revision': 'e1',
                    'name': 'Current exit',
                    'protocol': source == ChainSource.httpProxy
                        ? 'http_connect'
                        : 'socks5',
                    'source': source.wire,
                    'endpoint': {'host': 'proxy.example', 'port': 1080},
                  },
                },
                'active_frontends': <String>[],
                'active_listeners': <String>[],
                'platform_state_observed': true,
                'vpn_service_state': 'running',
                'vpn_process_state': 'reachable',
                'native_runtime_state': 'stopped',
                'tun_fd_valid': state == 'active',
                'tun_interface_present': state == 'active',
                'pending_cleanup': pending,
                'kill_switch_state': state,
              },
            );
            final snapshot = await MethodChannelEngineClient().snapshot();
            expect(snapshot.frontends, isEmpty);
            final scope = ChainScopePresentation.of(
              snapshot,
              null,
              isAndroid: true,
            );
            expect(
              scope.scopeKey,
              source.isProxy && state == 'inactive'
                  ? 'scope_interrupted'
                  : null,
            );
            expect(
              scope.androidSettings,
              source.isProxy && (state == 'active' || state == 'inactive'),
            );
            // The Android-specific enum contract must never infer desktop scope.
            expect(
              ChainScopePresentation.of(
                snapshot,
                null,
                isAndroid: false,
              ).isEmpty,
              isTrue,
            );
          }
        }
      }
      expect(
        ChainScopePresentation.of(
          _snapshot(
            phase: ConnectionPhase.error,
            current: false,
            tunnel: null,
            killSwitch: 'inactive',
          ),
          null,
          isAndroid: true,
        ).isEmpty,
        isTrue,
      );
    },
  );

  test(
    'all 21 catalogs contain limits and distinguish UDP acceptance from forwarding',
    () {
      expect(kChainCatalogs, hasLength(21));
      for (final entry in kChainCatalogs.entries) {
        for (final key in [
          'scope_proxy_only',
          'scope_bypass',
          'scope_interrupted',
          'scope_android_settings',
          'udp_available',
        ]) {
          expect(entry.value[key], isNotEmpty, reason: '${entry.key}: $key');
          expect(
            RegExp(r'\{[^}]+\}')
                .allMatches(entry.value[key]!)
                .map((match) => match.group(0))
                .toList(),
            RegExp(r'\{[^}]+\}')
                .allMatches(kChainEn[key]!)
                .map((match) => match.group(0))
                .toList(),
            reason: '${entry.key}: $key placeholders',
          );
        }
        expect(entry.value['udp_available'], isNot(entry.value['udp_unknown']));
      }
      expect(strings.chain('udp_available'), contains('not verified'));
      final chinese = AppStrings(LocalePreference.simplifiedChinese);
      expect(chinese.chain('scope_interrupted'), '连接已中断，设备可能恢复普通网络。');
      expect(chinese.chain('udp_available'), contains('尚未验证'));
    },
  );

  for (final surface in ['current', 'desktop', 'mobile']) {
    testWidgets(
      '$surface uses the same limits and refreshes when runtime scope changes',
      (tester) async {
        tester.view.devicePixelRatio = 1;
        tester.view.physicalSize = const Size(390, 1100);
        addTearDown(tester.view.resetDevicePixelRatio);
        addTearDown(tester.view.resetPhysicalSize);
        final applied = _profile(tunnel: false);
        // Saved settings intentionally disagree with the actual session. Seed
        // the fake as well so future capability refreshes cannot replace them.
        final saved = _profile(source: ChainSource.wireguardCustom);
        final engine = FakeEngineClient()
          ..storedProfiles = [saved]
          ..storedActiveProfileId = saved.id
          ..settingsState = NetworkSettingsState(
            sourceEpoch: 'scope',
            sequence: 1,
            sessionId: 'session',
            storedProfile: saved,
            sharedNetwork: saved,
            appliedProfile: applied,
          );
        final app = AppController(engine)
          ..initialized = true
          ..localePreference = LocalePreference.english
          ..sharedNetwork = saved
          ..snapshot = _snapshot(
            tunnel: FrontendPhase.disabled,
            udp: 'available',
          );
        app.networkSettings.accept(engine.settingsState!);
        addTearDown(app.dispose);
        Widget content() => switch (surface) {
          'current' => ChainCurrentConnection(
            controller: app,
            configuredEnabled: true,
          ),
          'desktop' => HomeDesktopControls(
            controller: app,
            onOpenChainProxy: () {},
          ),
          _ => HomeScreen(controller: app),
        };
        await tester.pumpWidget(
          workflowHost(
            app,
            scale: 2,
            home: surface == 'mobile'
                ? content()
                : Scaffold(body: SingleChildScrollView(child: content())),
          ),
        );
        await tester.pumpAndSettle();
        expect(find.text(strings.chain('scope_proxy_only')), findsOneWidget);
        if (surface == 'current') {
          expect(find.text(strings.chain('udp_available')), findsOneWidget);
        }
        expect(tester.takeException(), isNull);
        // No chain status field changes: the selector must observe the shared
        // scope presentation, including actual frontend state.
        app.snapshot = _snapshot(udp: 'available');
        app.networkSettings.accept(
          NetworkSettingsState(
            sourceEpoch: 'scope',
            sequence: 2,
            sessionId: 'session',
            storedProfile: saved,
            sharedNetwork: saved,
            appliedProfile: applied,
          ),
        );
        await tester.pumpAndSettle();
        final expected = ChainScopePresentation.of(
          app.snapshot,
          applied,
          isAndroid: surface == 'mobile',
        ).message(strings);
        expect(find.text(expected), findsOneWidget);
        expect(find.text(strings.chain('scope_proxy_only')), findsNothing);
        expect(tester.takeException(), isNull);

        // A terminal snapshot can omit chain metadata. The latest applied VPN
        // profile still supplies scope, but only explicit inactive permits this
        // warning. A stored WireGuard draft must not hide the SOCKS5 failure.
        app.snapshot = _snapshot(
          phase: ConnectionPhase.error,
          current: false,
          tunnel: FrontendPhase.disabled,
          killSwitch: 'inactive',
        );
        app.networkSettings.accept(
          NetworkSettingsState(
            sourceEpoch: 'scope',
            sequence: 3,
            sessionId: 'session',
            storedProfile: saved,
            sharedNetwork: saved,
            appliedProfile: _profile(),
          ),
        );
        await tester.pumpAndSettle();
        expect(
          find.textContaining(strings.chain('scope_interrupted')),
          findsOneWidget,
        );
        if (surface == 'current') {
          expect(find.text(strings.chain('proxy_ready')), findsNothing);
          expect(find.text(strings.chain('proxy_verified')), findsNothing);
          expect(find.text(strings.chain('udp_available')), findsNothing);
          expect(find.text(strings.chain('udp_unknown')), findsNothing);
        }
        expect(tester.takeException(), isNull);

        // Native desktop retirement clears applied_profile and retains the
        // failed tunnel frontend plus the actual HTTP/SOCKS source instead.
        app.snapshot = _snapshot(
          phase: ConnectionPhase.error,
          tunnel: FrontendPhase.error,
          proxyActive: false,
          killSwitch: 'inactive',
          udp: 'available',
        );
        app.networkSettings.accept(
          NetworkSettingsState(
            sourceEpoch: 'scope',
            sequence: 4,
            storedProfile: saved,
            sharedNetwork: saved,
          ),
        );
        await tester.pumpAndSettle();
        expect(
          find.textContaining(strings.chain('scope_interrupted')),
          findsOneWidget,
        );
        if (surface == 'current') {
          expect(find.text(strings.chain('proxy_ready')), findsNothing);
          expect(find.text(strings.chain('proxy_verified')), findsNothing);
          expect(find.text(strings.chain('udp_available')), findsNothing);
        }
        expect(tester.takeException(), isNull);

        app.snapshot = _snapshot(
          phase: ConnectionPhase.error,
          tunnel: FrontendPhase.disabled,
          proxyPhase: FrontendPhase.error,
          killSwitch: 'notApplicable',
        );
        app.networkSettings.accept(
          NetworkSettingsState(
            sourceEpoch: 'scope',
            sequence: 5,
            storedProfile: saved,
            sharedNetwork: saved,
            appliedProfile: _profile(tunnel: false),
          ),
        );
        await tester.pumpAndSettle();
        expect(find.text(strings.chain('scope_proxy_only')), findsOneWidget);
        expect(
          find.textContaining(strings.chain('scope_interrupted')),
          findsNothing,
        );
        expect(
          find.textContaining(strings.chain('scope_android_settings')),
          findsNothing,
        );
        expect(tester.takeException(), isNull);
        await tester.pumpWidget(const SizedBox());
      },
      variant: TargetPlatformVariant.only(
        surface == 'mobile' ? TargetPlatform.android : TargetPlatform.windows,
      ),
    );
  }
}
