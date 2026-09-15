import 'dart:convert';
import 'dart:io';
import 'dart:ui' as ui;

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';
import 'package:usque/core/country_flags.dart';
import 'package:usque/core/iso_countries.dart';
import 'package:usque/core/usque_theme.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/screens/home_screen.dart';
import 'package:usque/widgets/country_flag.dart';

import 'ui_workflow_test.dart' show WorkflowEngine, pumpWorkflow, workflowHost;
import 'vpngate_test.dart' show server;

class _MissingFlags extends CachingAssetBundle {
  @override
  Future<ByteData> load(String key) => key.endsWith('.png')
      ? Future.error(FlutterError('Missing test flag'))
      : rootBundle.load(key);
}

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();

  test(
    'the complete indexed bundle decodes at its recorded source size',
    () async {
      final manifest =
          jsonDecode(File('assets/flags/manifest.json').readAsStringSync())
              as Map<String, dynamic>;
      final flags = manifest['flags'] as Map<String, dynamic>;
      expect(flags.length, manifest['count']);
      expect(flags.keys.toSet(), kCountryFlagCodes);
      expect(
        kCountryFlagCodes,
        containsAll(kIsoCountries.map((country) => country.code.toLowerCase())),
      );
      final assets = await AssetManifest.loadFromAssetBundle(rootBundle);
      expect(
        assets
            .listAssets()
            .where((path) => path.startsWith('assets/flags/w80/'))
            .toSet(),
        flags.keys.map((code) => 'assets/flags/w80/$code.png').toSet(),
      );
      for (final entry in flags.entries) {
        final record = entry.value as Map<String, dynamic>;
        final bytes = await rootBundle.load(
          'assets/flags/w80/${record['file']}',
        );
        final codec = await ui.instantiateImageCodec(
          bytes.buffer.asUint8List(),
        );
        final frame = await codec.getNextFrame();
        expect(frame.image.width, 80, reason: entry.key);
        expect(frame.image.width, record['width'], reason: entry.key);
        expect(frame.image.height, record['height'], reason: entry.key);
        frame.image.dispose();
        codec.dispose();
      }
    },
  );

  testWidgets(
    'flags normalize codes, preserve proportions and use one text label',
    (tester) async {
      await tester.pumpWidget(
        MaterialApp(
          theme: UsqueTheme.light(),
          home: const Scaffold(
            body: Row(
              children: [
                CountryFlag(countryCode: ' jP '),
                Text('Japan'),
              ],
            ),
          ),
        ),
      );
      await tester.pumpAndSettle();
      final flag = find.byType(CountryFlag);
      expect(tester.getSize(flag), const Size(24, 18));
      final image = tester.widget<Image>(find.byType(Image));
      expect((image.image as AssetImage).assetName, 'assets/flags/w80/jp.png');
      expect(image.fit, BoxFit.contain);
      expect(find.bySemanticsLabel('Japan'), findsOneWidget);
      expect(
        find.descendant(of: flag, matching: find.byType(Focus)),
        findsNothing,
      );
    },
  );

  testWidgets(
    'directional layout and failed or unknown assets retain their space',
    (tester) async {
      for (final code in <String?>['ALL', null, '../jp', 'ZZ', 'JP']) {
        await tester.pumpWidget(
          MaterialApp(
            theme: UsqueTheme.dark(),
            home: MediaQuery(
              data: const MediaQueryData(
                navigationMode: NavigationMode.directional,
              ),
              child: DefaultAssetBundle(
                bundle: _MissingFlags(),
                child: Scaffold(
                  body: Center(
                    child: CountryFlag(countryCode: code, enabled: false),
                  ),
                ),
              ),
            ),
          ),
        );
        await tester.pumpAndSettle();
        expect(tester.getSize(find.byType(CountryFlag)), const Size(32, 24));
        expect(find.byIcon(LucideIcons.globe), findsOneWidget);
        expect(tester.takeException(), isNull);
      }
    },
  );

  testWidgets(
    'Home keeps measured exit flags separate from Gate node flags and legacy SVG',
    (tester) async {
      final app = await pumpWorkflow(
        tester,
        WorkflowEngine(),
        section: AppSection.home,
      );
      for (final gate in [false, true]) {
        for (final measured in [true, false]) {
          app.snapshot = EngineSnapshot(
            phase: ConnectionPhase.connected,
            vpnGate: gate
                ? const VpnGateStatus(stage: 'connected', server: server)
                : const VpnGateStatus(),
            exit: ExitInfo(
              country: measured ? 'Singapore' : null,
              countryCode: measured ? 'SG' : null,
              ipv4: '203.0.113.8',
              flagSvg: '<svg>legacy image must be ignored</svg>',
            ),
          );
          await tester.pumpWidget(
            workflowHost(app, home: HomeScreen(controller: app)),
          );
          await tester.pumpAndSettle();
          final codes = tester
              .widgetList<CountryFlag>(find.byType(CountryFlag))
              .map((flag) => flag.countryCode)
              .toList();
          expect(codes, contains(measured ? 'SG' : null));
          expect(codes.where((code) => code == 'JP').length, gate ? 1 : 0);
          expect(tester.takeException(), isNull);
        }
      }
    },
  );
}
