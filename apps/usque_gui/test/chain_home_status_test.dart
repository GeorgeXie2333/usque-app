import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:usque/core/app_strings.dart';
import 'package:usque/core/chain_home_status.dart';
import 'package:usque/core/chain_strings.dart';
import 'package:usque/core/connection_presentation.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/screens/home_screen.dart';
import 'package:usque/state/app_controller.dart';
import 'package:usque/widgets/common.dart';
import 'package:usque/widgets/connection_ring.dart';

import 'app_test.dart' show FakeEngineClient;
import 'ui_workflow_test.dart' show workflowHost;

void main() {
  const vpnGate = ChainExitSettings(enabled: true, source: ChainSource.vpnGate);
  const custom = ChainExitSettings(
    enabled: true,
    source: ChainSource.wireguardCustom,
  );
  final strings = AppStrings(LocalePreference.english);

  ChainHomeStatus status({
    required ConnectionPhase phase,
    String chainStage = 'disabled',
    String gateStage = 'disabled',
    bool enabled = true,
    bool profile = false,
    bool server = false,
  }) => ChainHomeStatus.of(
    phase: phase,
    chainEnabled: enabled,
    chainStage: chainStage,
    gateStage: gateStage,
    hasCurrentProfile: profile,
    hasGateServer: server,
  );

  test('home chain status uses one sentence for every exit', () {
    final preparing = status(
      phase: ConnectionPhase.preparing,
      gateStage: 'connecting_server',
    );
    final connecting = status(
      phase: ConnectionPhase.connectingH3,
      gateStage: 'connecting_warp',
    );
    final negotiating = status(
      phase: ConnectionPhase.connectingH3,
      chainStage: 'negotiating',
      gateStage: 'disabled',
    );
    expect(preparing.labelKey, 'connecting');
    expect(preparing.chainCatalog, isTrue);
    expect(connecting.labelKey, preparing.labelKey);
    expect(negotiating.labelKey, preparing.labelKey);
    expect(preparing.mode, RingMode.scan);
    expect(preparing.label(strings), strings.chain('connecting'));
    expect(preparing.label(strings), isNot(strings.get('preparing')));
    expect(preparing.label(strings), isNot('Connecting to VPN Gate'));

    final applying = status(
      phase: ConnectionPhase.connected,
      gateStage: 'configuring_network',
    );
    final reapplying = status(
      phase: ConnectionPhase.reconnecting,
      gateStage: 'configuring_network',
    );
    expect(applying.labelKey, 'connecting');
    expect(reapplying.labelKey, applying.labelKey);
    expect(applying.mode, RingMode.scan);
    expect(applying.drivesHome, isTrue);

    final idle = status(phase: ConnectionPhase.disconnected);
    expect(idle.labelKey, 'enabled_idle');
    expect(idle.chainCatalog, isTrue);
    expect(idle.drivesHome, isFalse);
    expect(idle.showChainRow, isTrue);
    expect(idle.mode, RingMode.idle);
    expect(idle.label(strings), 'Enabled · not connected');

    final restored = status(
      phase: ConnectionPhase.connected,
      chainStage: 'connected',
      gateStage: 'disabled',
    );
    expect(restored.labelKey, 'connected');
    expect(restored.mode, RingMode.steady);
    expect(restored.tone, StatusTone.success);

    final limited = status(
      phase: ConnectionPhase.degraded,
      gateStage: 'connected',
    );
    expect(limited.labelKey, 'degraded');
    expect(limited.mode, RingMode.steady);

    final missing = status(phase: ConnectionPhase.connected);
    expect(missing.drivesHome, isFalse);
    expect(missing.showChainRow, isFalse);

    final off = status(phase: ConnectionPhase.disconnected, enabled: false);
    expect(off.showChainRow, isFalse);
    expect(off.drivesHome, isFalse);
  });

  for (final source in [ChainSource.httpProxy, ChainSource.socks5Proxy]) {
    for (final locale in [
      LocalePreference.english,
      LocalePreference.simplifiedChinese,
    ]) {
      testWidgets(
        'mobile $source/$locale keeps WARP status stable across sparse snapshots',
        (tester) async {
          tester.view.devicePixelRatio = 1;
          tester.view.physicalSize = const Size(390, 1000);
          addTearDown(tester.view.resetDevicePixelRatio);
          addTearDown(tester.view.resetPhysicalSize);

          EngineSnapshot sample({
            String? warp,
            String? legacyWarp,
            String phase = 'connected',
            String stage = 'connected',
          }) => EngineSnapshot.fromMap({
            'phase': phase,
            'kill_switch_state': 'active',
            'chain_exit': {
              'stage': stage,
              'warp_stage': ?warp,
              'current_profile': {
                'id': 'exit',
                'revision': 'r1',
                'name': 'NY ${source.label}',
                'source': source.wire,
                'protocol': source == ChainSource.httpProxy ? 'http' : 'socks5',
                'endpoint': {'host': 'proxy.example', 'port': 1080},
              },
            },
            'vpn_gate': {'stage': 'disabled', 'warp_stage': ?legacyWarp},
            'frontends': [
              {'kind': 'tunnel', 'phase': 'active'},
            ],
          });

          final app = AppController(FakeEngineClient())
            ..localePreference = locale
            ..sharedNetwork = UsqueProfile.defaultProfile().copyWith(
              chainExit: ChainExitSettings(enabled: true, source: source),
            )
            ..snapshot = sample(legacyWarp: 'connected');
          addTearDown(app.dispose);
          await tester.pumpWidget(
            workflowHost(app, home: HomeScreen(controller: app)),
          );
          await tester.pumpAndSettle();
          final connected = 'WARP: ${app.strings.get('connected')}';
          expect(find.text(connected), findsOneWidget);
          final ring = tester.getRect(find.byType(ConnectionRing));
          for (final next in [
            sample(),
            sample(warp: 'connected'),
            sample(legacyWarp: 'connected'),
            sample(),
          ]) {
            app.snapshot = next;
            app.selectSection(AppSection.home);
            await tester.pumpAndSettle();
            expect(find.text(connected), findsOneWidget);
            expect(tester.getRect(find.byType(ConnectionRing)), ring);
          }
          expect(find.byKey(const ValueKey('home-chain-scope')), findsNothing);

          for (final (snapshot, label) in [
            (
              sample(phase: 'preparing', stage: 'connecting_server'),
              app.strings.get('connected'),
            ),
            (
              sample(
                phase: 'reconnecting',
                stage: 'reconnecting',
                warp: 'reconnecting',
                legacyWarp: 'connected',
              ),
              app.strings.get('reconnecting'),
            ),
            (
              sample(
                phase: 'error',
                stage: 'error',
                warp: 'disconnected',
                legacyWarp: 'connected',
              ),
              app.strings.get('disconnected'),
            ),
            (sample(phase: 'error', stage: 'error'), '—'),
            (
              sample(phase: 'disconnected', legacyWarp: 'connected'),
              app.strings.get('disconnected'),
            ),
          ]) {
            app.snapshot = snapshot;
            app.selectSection(AppSection.home);
            await tester.pumpAndSettle();
            expect(find.text('WARP: $label'), findsOneWidget);
            expect(tester.takeException(), isNull);
          }
          await tester.pumpWidget(const SizedBox());
        },
        variant: TargetPlatformVariant.only(TargetPlatform.android),
      );
    }
  }

  testWidgets('Home keeps the mobile chain block and omits it on desktop', (
    tester,
  ) async {
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetDevicePixelRatio);
    addTearDown(tester.view.resetPhysicalSize);

    Future<void> expectStatus(
      Size size,
      EngineSnapshot snapshot,
      ChainExitSettings chain,
      String label, {
      int? copies,
      List<String> present = const [],
      List<String> absent = const [],
      bool row = true,
    }) async {
      tester.view.physicalSize = size;
      final mobile = defaultTargetPlatform == TargetPlatform.android;
      final app = AppController(FakeEngineClient())
        ..localePreference = LocalePreference.english
        ..sharedNetwork = UsqueProfile.defaultProfile().copyWith(
          chainExit: chain,
        )
        ..snapshot = snapshot;
      addTearDown(app.dispose);
      await tester.pumpWidget(
        workflowHost(app, home: HomeScreen(controller: app)),
      );
      await tester.pumpAndSettle();
      expect(
        find.text(label),
        findsNWidgets(copies ?? (mobile ? 2 : 1)),
        reason: '$size $label',
      );
      for (final other in present) {
        expect(find.text(other), findsOneWidget, reason: '$size $other');
      }
      for (final other in absent) {
        expect(find.text(other), findsNothing, reason: '$size $other');
      }
      expect(
        find.byKey(const ValueKey('home-vpn-gate-settings')),
        row && mobile ? findsOneWidget : findsNothing,
        reason: '$size top block',
      );
      expect(
        find.textContaining('WARP →'),
        row && mobile ? findsOneWidget : findsNothing,
      );
      await tester.pumpWidget(const SizedBox());
    }

    const sizes = [
      (Size(390, 844), TargetPlatform.android),
      (Size(1220, 1000), TargetPlatform.windows),
      (Size(640, 900), TargetPlatform.windows),
    ];
    try {
      for (final (size, platform) in sizes) {
        debugDefaultTargetPlatformOverride = platform;
        final mobile = platform == TargetPlatform.android;
        await expectStatus(
          size,
          const EngineSnapshot(
            phase: ConnectionPhase.preparing,
            vpnGate: VpnGateStatus(stage: 'connecting_server'),
          ),
          vpnGate,
          strings.chain('connecting'),
          absent: [strings.get('preparing'), 'Connecting to VPN Gate'],
        );
        await expectStatus(
          size,
          const EngineSnapshot(
            phase: ConnectionPhase.connectingH3,
            vpnGate: VpnGateStatus(stage: 'connecting_warp'),
          ),
          vpnGate,
          strings.chain('connecting'),
          absent: [strings.get('preparing'), 'Connecting to WARP'],
        );
        await expectStatus(
          size,
          const EngineSnapshot(
            phase: ConnectionPhase.connected,
            vpnGate: VpnGateStatus(stage: 'configuring_network'),
          ),
          vpnGate,
          strings.chain('connecting'),
          absent: [strings.get('connected'), 'Applying network settings'],
        );
        await expectStatus(
          size,
          const EngineSnapshot(
            phase: ConnectionPhase.reconnecting,
            chainExit: ChainExitStatus(stage: 'negotiating'),
          ),
          custom,
          strings.chain('connecting'),
          absent: [strings.get('reconnecting'), 'Connecting to VPN Gate'],
        );
        await expectStatus(
          size,
          const EngineSnapshot(),
          vpnGate,
          strings.get('disconnected'),
          copies: 1,
          present: mobile ? [strings.chain('enabled_idle')] : const [],
          absent: !mobile ? [strings.chain('enabled_idle')] : const [],
        );
        await expectStatus(
          size,
          const EngineSnapshot(
            phase: ConnectionPhase.connected,
            chainExit: ChainExitStatus(stage: 'connected'),
            vpnGate: VpnGateStatus(stage: 'disabled'),
          ),
          custom,
          strings.get('connected'),
        );
        await expectStatus(
          size,
          const EngineSnapshot(phase: ConnectionPhase.connected),
          vpnGate,
          strings.get('connected'),
          copies: 1,
          row: false,
        );
        await expectStatus(
          size,
          const EngineSnapshot(),
          const ChainExitSettings(),
          strings.get('disconnected'),
          copies: 1,
          row: false,
        );
      }
    } finally {
      debugDefaultTargetPlatformOverride = null;
    }
  });
}
