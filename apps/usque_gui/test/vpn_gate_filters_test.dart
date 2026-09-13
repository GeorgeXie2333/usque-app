import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:usque/core/app_strings.dart';
import 'package:usque/core/usque_theme.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/widgets/vpn_gate_filters.dart';

import 'vpn_gate_navigation_test.dart' show openGate;
import 'vpngate_test.dart' show GateEngine, server;

void main() {
  testWidgets(
    'filters use available content width and keep their labels apart',
    (tester) async {
      tester.view.devicePixelRatio = 1;
      tester.view.physicalSize = const Size(1220, 920);
      addTearDown(tester.view.resetDevicePixelRatio);
      addTearDown(tester.view.resetPhysicalSize);
      for (final locale in LocalePreference.values.where(
        (value) => value != LocalePreference.system,
      )) {
        for (final width in [880.0, 580.0, 358.0]) {
          for (final scale in [1.0, 2.0]) {
            final strings = AppStrings(locale);
            final rtl = [
              LocalePreference.arabic,
              LocalePreference.persian,
            ].contains(locale);
            await tester.pumpWidget(
              MaterialApp(
                theme: scale == 1 ? UsqueTheme.light() : UsqueTheme.dark(),
                home: Scaffold(
                  body: MediaQuery(
                    data: MediaQueryData(
                      size: const Size(1220, 920),
                      textScaler: TextScaler.linear(scale),
                    ),
                    child: Directionality(
                      textDirection: rtl
                          ? TextDirection.rtl
                          : TextDirection.ltr,
                      child: Align(
                        alignment: Alignment.topLeft,
                        child: SizedBox(
                          width: width,
                          child: VpnGateFilters(
                            strings: strings,
                            favoritesOnly: false,
                            favoriteCount: 1200,
                            country: 'ALL',
                            countries: const [],
                            onScopeChanged: (_) {},
                            onCountryChanged: (_) {},
                          ),
                        ),
                      ),
                    ),
                  ),
                ),
              ),
            );
            await tester.pumpAndSettle();
            final scopes = tester.getRect(
              find.byKey(const ValueKey('vpn-gate-scopes')),
            );
            final country = tester.getRect(
              find.byKey(const ValueKey('vpn-gate-country-field')),
            );
            expect(
              scopes.overlaps(country),
              isFalse,
              reason: '$locale / $width / $scale',
            );
            if (width < 640 || scale == 2) {
              expect(country.top - scopes.bottom, greaterThanOrEqualTo(16));
            } else {
              expect(
                rtl ? scopes.left - country.right : country.left - scopes.right,
                greaterThanOrEqualTo(24),
              );
            }
            expect(
              tester.takeException(),
              isNull,
              reason: '$locale / $width / $scale',
            );
            expect(find.text(strings.get('gate_pool')), findsOneWidget);
          }
        }
      }
    },
  );

  testWidgets(
    'Android wide filters and favorites remain usable while selection is disabled',
    (tester) async {
      debugDefaultTargetPlatformOverride = TargetPlatform.android;
      try {
        final engine = GateEngine();
        final app = await openGate(tester, engine);
        expect(find.text('All servers'), findsOneWidget);
        final country = find.byType(DropdownButtonFormField<String>);
        await tester.ensureVisible(country);
        await tester.pumpAndSettle();
        await tester.tap(country);
        await tester.pumpAndSettle();
        await tester.tap(find.text('Japan (1)').last);
        await tester.pumpAndSettle();
        expect(engine.country, 'JP');
        final node = find.byKey(ValueKey('vpn-gate-node-${server.id}'));
        expect(tester.widget<ListTile>(node).onTap, isNull);
        final favorite = find.byKey(ValueKey('vpn-gate-favorite-${server.id}'));
        await tester.ensureVisible(favorite);
        await tester.pumpAndSettle();
        await tester.tap(favorite);
        await tester.pumpAndSettle();
        final scope = find.byKey(const ValueKey('vpn-gate-favorites'));
        await tester.ensureVisible(scope);
        await tester.pumpAndSettle();
        await tester.tap(scope);
        await tester.pumpAndSettle();
        expect(engine.country, isNull);
        expect(engine.favorites.containsKey(server.id), isTrue);
        expect(app.activeProfile.vpnGate.hasSelection, isFalse);
      } finally {
        debugDefaultTargetPlatformOverride = null;
      }
    },
  );
}
