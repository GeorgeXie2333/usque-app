import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:usque/core/app_strings.dart';
import 'package:usque/core/usque_theme.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/screens/vpn_gate_screen.dart';
import 'package:usque/widgets/vpn_gate_server_row.dart';

import 'ui_workflow_test.dart' show workflowHost;
import 'vpngate_test.dart' show GateEngine, host;

final observationNow = DateTime(2026, 9, 13, 22, 22);

VpnGateServer observationServer({
  String id = 'observed',
  bool present = false,
  bool inPool = true,
  bool expired = false,
  String tcpStatus = 'reachable',
  VpnGateFavoriteMetadata? favorite,
}) => VpnGateServer(
  id: id,
  ip: '74.194.252.187',
  hostname: 'vpn171113221',
  configSha256: 'observed-configuration',
  countryCode: 'US',
  score: 3760628,
  pingMs: 212,
  speedBps: 80200000,
  pool: VpnGatePoolMetadata(
    firstSeenAt: observationNow.subtract(const Duration(days: 2)),
    lastSeenAt: observationNow.subtract(const Duration(minutes: 22)),
    checkedAt: observationNow.subtract(
      expired ? const Duration(hours: 13) : const Duration(minutes: 12),
    ),
    tcpStatus: tcpStatus,
    present: present,
    inPool: inPool,
  ),
  favorite: favorite,
);

void main() {
  test(
    'observation ages have bounded units and do not turn clock skew into freshness',
    () {
      for (final (locale, expected) in [
        (
          LocalePreference.english,
          [
            'Just now',
            '1 min ago',
            '59 min ago',
            '1 h ago',
            '23 h ago',
            '2 d ago',
          ],
        ),
        (
          LocalePreference.simplifiedChinese,
          ['刚刚', '1 分钟前', '59 分钟前', '1 小时前', '23 小时前', '2 天前'],
        ),
      ]) {
        final strings = AppStrings(locale);
        for (final (index, age) in const [
          Duration(seconds: 59),
          Duration(minutes: 1),
          Duration(minutes: 59, seconds: 59),
          Duration(hours: 1),
          Duration(hours: 23, minutes: 59),
          Duration(days: 2),
        ].indexed) {
          expect(
            vpnGateObservationAge(
              observationNow.subtract(age),
              observationNow,
              strings,
            ),
            expected[index],
          );
        }
        expect(vpnGateObservationAge(null, observationNow, strings), '—');
        expect(
          vpnGateObservationAge(
            observationNow.add(const Duration(minutes: 1)),
            observationNow,
            strings,
          ),
          '—',
        );
      }
    },
  );

  testWidgets(
    'details are independent of selection and expose full local observations',
    (tester) async {
      final engine = GateEngine()..nodes = [observationServer()];
      final app = await host(tester, engine);
      final details = find.byKey(const ValueKey('vpn-gate-details-observed'));
      final observations = find.byKey(
        const ValueKey('vpn-gate-observations-observed'),
      );
      expect(observations, findsNothing);
      await tester.ensureVisible(details);
      await tester.tap(details);
      await tester.pumpAndSettle();
      expect(observations, findsOneWidget);
      expect(find.text('Local time'), findsOneWidget);
      expect(
        find.text('First listed by VPN Gate: 2026-09-11 22:22:00'),
        findsOneWidget,
      );
      expect(app.activeProfile.vpnGate.hasSelection, isFalse);
      expect(engine.nodeRequests, isEmpty);
      expect(engine.saves, 0);
      // Collapse with the keyboard without selecting the disabled node.
      Focus.of(tester.element(find.text('Details'))).requestFocus();
      await tester.pump();
      await tester.sendKeyEvent(LogicalKeyboardKey.enter);
      await tester.pumpAndSettle();
      expect(observations, findsNothing);
      expect(
        tester
            .widget<ListTile>(
              find.byKey(const ValueKey('vpn-gate-node-observed')),
            )
            .onTap,
        isNull,
      );
      await tester.ensureVisible(find.byKey(const ValueKey('vpn-gate-toggle')));
      await tester.tap(find.byKey(const ValueKey('vpn-gate-toggle')));
      await tester.pumpAndSettle();
      await tester.ensureVisible(details);
      await tester.tap(details);
      await tester.pumpAndSettle();
      expect(
        tester
            .widget<ListTile>(
              find.byKey(const ValueKey('vpn-gate-node-observed')),
            )
            .selected,
        isFalse,
      );
      expect(engine.saves, 0);
      await tester.pumpWidget(const SizedBox());
    },
  );

  testWidgets(
    'visible observation ages advance without refreshing the directory',
    (tester) async {
      final engine = GateEngine()..nodes = [observationServer()];
      final app = await host(tester, engine);
      var now = observationNow;
      await tester.pumpWidget(
        workflowHost(
          app,
          home: VpnGateScreen(controller: app, now: () => now),
        ),
      );
      await tester.pumpAndSettle();
      expect(find.text('Last listed: 22 min ago'), findsOneWidget);
      now = now.add(const Duration(minutes: 1));
      await tester.pump(const Duration(minutes: 1));
      expect(find.text('Last listed: 23 min ago'), findsOneWidget);
      expect(engine.refreshes, 0);
      await tester.pumpWidget(const SizedBox());
    },
  );

  testWidgets(
    'localized summaries and expanded records fit small, wide and directional layouts',
    (tester) async {
      tester.view.devicePixelRatio = 1;
      tester.view.physicalSize = const Size(1000, 900);
      addTearDown(tester.view.resetDevicePixelRatio);
      addTearDown(tester.view.resetPhysicalSize);
      for (final locale in LocalePreference.values.where(
        (value) => value != LocalePreference.system,
      )) {
        for (final width in [343.0, 880.0]) {
          for (final scale in [1.0, 2.0]) {
            final strings = AppStrings(locale);
            final rtl = [
              LocalePreference.arabic,
              LocalePreference.persian,
            ].contains(locale);
            var selections = 0, favorites = 0, updates = 0;
            final node = observationServer(
              expired: true,
              favorite: VpnGateFavoriteMetadata(
                configSha256: 'observed-configuration',
                savedAt: observationNow,
                latestConfigSha256: 'new-configuration',
              ),
            );
            await tester.pumpWidget(
              MaterialApp(
                theme: scale == 1 ? UsqueTheme.light() : UsqueTheme.dark(),
                home: Scaffold(
                  body: MediaQuery(
                    data: MediaQueryData(
                      textScaler: TextScaler.linear(scale),
                      navigationMode: NavigationMode.directional,
                      disableAnimations: true,
                    ),
                    child: Directionality(
                      textDirection: rtl
                          ? TextDirection.rtl
                          : TextDirection.ltr,
                      child: SingleChildScrollView(
                        child: Align(
                          alignment: Alignment.topCenter,
                          child: SizedBox(
                            width: width,
                            child: VpnGateServerRow(
                              key: ValueKey((locale, width, scale)),
                              server: node,
                              strings: strings,
                              now: observationNow,
                              selected: false,
                              onSelect: scale == 1 ? () => selections++ : null,
                              onFavorite: () => favorites++,
                              onUpdateFavorite: () => updates++,
                            ),
                          ),
                        ),
                      ),
                    ),
                  ),
                ),
              ),
            );
            await tester.pumpAndSettle();
            expect(
              find.text(strings.get('gate_probe_expired')),
              findsOneWidget,
            );
            final details = find.byKey(
              const ValueKey('vpn-gate-details-observed'),
            );
            await tester.ensureVisible(details);
            await tester.pumpAndSettle();
            Focus.of(
              tester.element(find.text(strings.get('gate_node_details'))),
            ).requestFocus();
            await tester.pump();
            await tester.sendKeyEvent(LogicalKeyboardKey.select);
            await tester.pumpAndSettle();
            expect(
              find.byKey(const ValueKey('vpn-gate-observations-observed')),
              findsOneWidget,
            );
            final update = find.byKey(
              const ValueKey('vpn-gate-update-observed'),
            );
            await tester.ensureVisible(update);
            await tester.pumpAndSettle();
            await tester.tap(update);
            final favorite = find.byKey(
              const ValueKey('vpn-gate-favorite-observed'),
            );
            await tester.ensureVisible(favorite);
            await tester.pumpAndSettle();
            await tester.tap(favorite);
            expect(selections, 0);
            expect(favorites, 1);
            expect(updates, 1);
            expect(
              tester.takeException(),
              isNull,
              reason: '$locale / $width / $scale',
            );
          }
        }
      }
    },
  );
}
