import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:usque/core/app_strings.dart';
import 'package:usque/core/vpn_gate_presentation.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/widgets/vpn_gate_summary.dart';

import 'ui_workflow_test.dart' show workflowHost;
import 'vpngate_test.dart' show GateEngine, host, server;

const current = VpnGateServer(
  id: 'current',
  ip: '203.0.113.90',
  hostname: 'current.example',
  configSha256: 'current-hash',
  countryCode: 'KR',
);

void main() {
  test(
    'connection stages distinguish configured settings from platform readiness',
    () {
      for (final (snapshot, enabled, key) in [
        (const EngineSnapshot(), false, 'gate_disabled'),
        (const EngineSnapshot(), true, 'gate_no_connection'),
        (
          const EngineSnapshot(phase: ConnectionPhase.connected),
          false,
          'gate_warp_only',
        ),
        (
          const EngineSnapshot(
            phase: ConnectionPhase.preparing,
            vpnGate: VpnGateStatus(stage: 'connecting_warp'),
          ),
          true,
          'gate_connecting_warp',
        ),
        (
          const EngineSnapshot(
            phase: ConnectionPhase.preparing,
            vpnGate: VpnGateStatus(stage: 'connecting_server'),
          ),
          true,
          'gate_connecting_server',
        ),
        (
          const EngineSnapshot(
            phase: ConnectionPhase.preparing,
            vpnGate: VpnGateStatus(stage: 'negotiating'),
          ),
          true,
          'gate_negotiating',
        ),
        (
          const EngineSnapshot(
            phase: ConnectionPhase.preparing,
            vpnGate: VpnGateStatus(stage: 'connected'),
          ),
          true,
          'gate_configuring_network',
        ),
        (
          const EngineSnapshot(
            phase: ConnectionPhase.connected,
            vpnGate: VpnGateStatus(stage: 'connected', server: current),
          ),
          false,
          'connected',
        ),
        (
          const EngineSnapshot(
            phase: ConnectionPhase.error,
            vpnGate: VpnGateStatus(
              stage: 'error',
              warpStage: 'disconnected',
              server: current,
            ),
          ),
          true,
          'gate_failed',
        ),
        (
          const EngineSnapshot(
            phase: ConnectionPhase.disconnecting,
            vpnGate: VpnGateStatus(stage: 'error', server: current),
          ),
          true,
          'disconnecting',
        ),
      ]) {
        final view = VpnGatePresentation(snapshot, configuredEnabled: enabled);
        expect(view.statusKey, key);
        expect(
          view.serverLabelKey,
          view.connected ? 'gate_current' : 'gate_target',
        );
      }
      final stopped = VpnGatePresentation(
        const EngineSnapshot(
          vpnGate: VpnGateStatus(stage: 'connected', server: current),
        ),
        configuredEnabled: true,
      );
      expect(stopped.connected, isFalse);
      expect(stopped.server, isNull);
      final failedDuringCleanup = VpnGatePresentation(
        const EngineSnapshot(
          phase: ConnectionPhase.preparing,
          vpnGate: VpnGateStatus(stage: 'error', warpStage: 'disconnected'),
        ),
        configuredEnabled: true,
      );
      expect(failedDuringCleanup.statusKey, 'gate_failed');
      expect(failedDuringCleanup.warpKey, 'disconnected');
    },
  );

  testWidgets(
    'a changed or disabled draft never replaces the connected server',
    (tester) async {
      final engine = GateEngine();
      final app = await host(tester, engine);
      app.sharedNetwork = app.activeProfile.copyWith(
        vpnGate: const VpnGateSettings(enabled: true).copyWith(server: current),
      );
      app.snapshot = const EngineSnapshot(
        phase: ConnectionPhase.connected,
        vpnGate: VpnGateStatus(
          stage: 'connected',
          warpStage: 'connected',
          server: current,
        ),
      );
      app.selectSection(app.section);
      await tester.pumpAndSettle();
      final status = find.byType(VpnGateConnectionSummary);
      final bar = find.byType(VpnGateSelectionBar);
      expect(tester.widget<VpnGateSelectionBar>(bar).actionKey, 'gate_switch');
      await tester.tap(find.byKey(ValueKey('vpn-gate-node-${server.id}')));
      await tester.pumpAndSettle();
      expect(
        find.descendant(of: status, matching: find.text('KR · ${current.ip}')),
        findsOneWidget,
      );
      expect(
        find.descendant(of: bar, matching: find.text('JP · ${server.ip}')),
        findsOneWidget,
      );
      expect(
        find.descendant(
          of: bar,
          matching: find.text(app.strings.get('gate_draft')),
        ),
        findsOneWidget,
      );
      await tester.tap(find.byKey(const ValueKey('vpn-gate-toggle')));
      await tester.pumpAndSettle();
      expect(
        find.descendant(
          of: status,
          matching: find.text(app.strings.get('connected')),
        ),
        findsOneWidget,
      );
      expect(
        find.descendant(
          of: bar,
          matching: find.text(app.strings.get('gate_pending_disable')),
        ),
        findsOneWidget,
      );
      expect(engine.saves, 0);
    },
  );

  testWidgets('connection error details preserve the actual WARP state', (
    tester,
  ) async {
    final app = await host(tester, GateEngine());
    app.snapshot = const EngineSnapshot(
      phase: ConnectionPhase.error,
      warning: 'AUTHENTICATION_FAILED',
      vpnGate: VpnGateStatus(
        stage: 'error',
        warpStage: 'disconnected',
        server: current,
      ),
    );
    app.selectSection(app.section);
    await tester.pumpAndSettle();
    expect(
      find.text('WARP: ${app.strings.get('disconnected')}'),
      findsOneWidget,
    );
    expect(find.text(app.strings.get('gate_current')), findsNothing);
    await tester.tap(find.text(app.strings.get('gate_error_details')));
    await tester.pumpAndSettle();
    expect(find.text('AUTHENTICATION_FAILED'), findsOneWidget);
  });

  testWidgets(
    'footer keeps the selected version and only says saved after success',
    (tester) async {
      final app = await host(tester, GateEngine());
      final bar = find.byType(VpnGateSelectionBar);
      await tester.tap(find.byKey(const ValueKey('vpn-gate-toggle')));
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(ValueKey('vpn-gate-node-${server.id}')));
      await tester.pumpAndSettle();
      expect(
        find.descendant(
          of: bar,
          matching: find.text(app.strings.get('gate_pending_save')),
        ),
        findsOneWidget,
      );
      await tester.tap(find.byKey(const ValueKey('vpn-gate-apply')));
      await tester.pumpAndSettle();
      expect(
        find.descendant(
          of: bar,
          matching: find.text(app.strings.get('gate_saved')),
        ),
        findsOneWidget,
      );
      expect(
        tester.widget<VpnGateSelectionBar>(bar).draft.configSha256,
        server.configSha256,
      );
    },
  );

  testWidgets(
    'preparation stays cancellable in the footer after scrolling down',
    (tester) async {
      final engine = GateEngine()..holdPreparation = true;
      await host(tester, engine, size: const Size(780, 650));
      final star = find.byKey(ValueKey('vpn-gate-favorite-${server.id}'));
      await tester.ensureVisible(star);
      await tester.pumpAndSettle();
      expect(tester.widget<IconButton>(star).onPressed, isNotNull);
      await tester.tap(star);
      await tester.pump(const Duration(milliseconds: 100));
      final cancel = find.byKey(const ValueKey('vpn-gate-cancel-node'));
      expect(cancel.hitTestable(), findsOneWidget);
      await tester.tap(cancel);
      await tester.pumpAndSettle();
      expect(engine.nodeRequests.any((r) => r.action == 'cancel'), isTrue);
      expect(engine.favorites, isEmpty);
      expect(engine.saves, 0);
    },
  );

  testWidgets(
    'selection footer fits all languages at 200 percent in short and narrow views',
    (tester) async {
      final app = await host(tester, GateEngine());
      for (final locale in LocalePreference.values.where(
        (v) => v != LocalePreference.system,
      )) {
        app.localePreference = locale;
        for (final size in [const Size(390, 844), const Size(740, 360)]) {
          tester.view.physicalSize = size;
          await tester.pumpWidget(
            workflowHost(
              app,
              scale: 2,
              home: Directionality(
                textDirection:
                    [
                      LocalePreference.arabic,
                      LocalePreference.persian,
                    ].contains(locale)
                    ? TextDirection.rtl
                    : TextDirection.ltr,
                child: Scaffold(
                  body: const SizedBox.expand(),
                  bottomNavigationBar: VpnGateSelectionBar(
                    strings: AppStrings(locale),
                    draft: const VpnGateSettings(
                      enabled: true,
                    ).copyWith(server: server),
                    savedEnabled: false,
                    dirty: true,
                    connected: true,
                    saving: false,
                    preparing: false,
                    actionKey: 'gate_enable_reconnect',
                    onApply: () {},
                    onCancel: () {},
                    server: server,
                  ),
                ),
              ),
            ),
          );
          await tester.pumpAndSettle();
          expect(tester.takeException(), isNull, reason: '$locale / $size');
          final button = find.byKey(const ValueKey('vpn-gate-apply'));
          await tester.ensureVisible(button);
          await tester.pumpAndSettle();
          expect(
            button.hitTestable(),
            findsOneWidget,
            reason: '$locale / $size',
          );
        }
      }
    },
  );
}
