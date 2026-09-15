import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:usque/models/app_models.dart';

import 'vpngate_test.dart' show GateEngine, host;

void main() {
  for (final platform in [TargetPlatform.windows, TargetPlatform.android]) {
    testWidgets(
      'directory failures after scrolling preserve page and text state: $platform',
      (tester) async {
        final phone = platform == TargetPlatform.android;
        final engine = GateEngine()
          ..nodes = List.generate(
            12,
            (i) => VpnGateServer(
              id: 'node-$i',
              ip: '203.0.113.${i + 1}',
              hostname: 'node-$i.example',
              configSha256: 'config-$i',
              countryCode: 'JP',
            ),
          );
        final app = await host(
          tester,
          engine,
          dark: phone,
          scale: phone ? 2 : 1,
          locale: phone
              ? LocalePreference.simplifiedChinese
              : LocalePreference.english,
          size: phone ? const Size(390, 844) : const Size(1280, 800),
        );
        // Start a refresh, then scroll while its response is pending.
        await tester.tap(find.text(app.strings.get('gate_refresh')));
        await tester.pump();
        final page = tester.state<ScrollableState>(
          find
              .descendant(
                of: find.byKey(const PageStorageKey<String>('VPN Gate')),
                matching: find.byType(Scrollable),
              )
              .first,
        );
        page.position.jumpTo(page.position.maxScrollExtent);
        await tester.pump();
        final offset = page.position.pixels;
        expect(offset, greaterThan(0));
        final storage = PageStorage.of(page.context);
        expect(storage.readState(page.context), offset);

        const failure = 'primary_raw: https://example.invalid: timeout';
        engine.failures = const [failure, 'primary_cdn: request failed'];
        await tester.pump(const Duration(seconds: 2));
        await tester.pumpAndSettle();
        expect(tester.takeException(), isNull);
        expect(find.byType(ErrorWidget), findsNothing);
        expect(page.position.pixels, offset);

        final details = find.widgetWithText(
          ExpansionTile,
          app.strings.get('gate_fetch_error'),
        );
        await tester.ensureVisible(details);
        await tester.pumpAndSettle();
        expect(tester.getSize(details).height, lessThan(160));
        final beforeExpansion = page.position.pixels;
        await tester.tap(details);
        await tester.pumpAndSettle();
        expect(tester.takeException(), isNull);
        expect(find.byType(ErrorWidget), findsNothing);
        expect(find.text(failure), findsOneWidget);
        expect(storage.readState(page.context), beforeExpansion);

        // Text selection can scroll its internal viewport. It must not save
        // over either the page offset or the enclosing tile's expansion flag.
        for (final text in [
          find.widgetWithText(
            SelectableText,
            '${app.strings.get('gate_source')}: —',
          ),
          find.widgetWithText(SelectableText, failure),
        ]) {
          final textScroll = tester.state<ScrollableState>(
            find.descendant(of: text, matching: find.byType(Scrollable)),
          );
          textScroll.position.jumpTo(1);
          await tester.pumpAndSettle();
          expect(storage.readState(page.context), beforeExpansion);
        }

        await tester.ensureVisible(details);
        await tester.pumpAndSettle();
        await tester.tap(details);
        await tester.pumpAndSettle();
        await tester.tap(details);
        await tester.pumpAndSettle();
        expect(find.text(failure), findsOneWidget);
        expect(tester.takeException(), isNull);
        expect(find.byType(ErrorWidget), findsNothing);
        await tester.pumpWidget(const SizedBox());
      },
      variant: TargetPlatformVariant({platform}),
    );
  }
}
