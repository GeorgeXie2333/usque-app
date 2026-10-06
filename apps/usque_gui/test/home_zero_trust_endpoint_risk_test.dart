import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:usque/core/app_strings.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/models/network_settings.dart';
import 'package:usque/screens/home_screen.dart';
import 'package:usque/state/app_controller.dart';
import 'package:usque/widgets/common.dart';

import 'endpoint_selection_test.dart' show screen;
import 'ui_workflow_test.dart' show workflowHost;
import 'zero_trust_endpoint_test.dart' show ZtEndpointEngine, ztEngine, ztId;

final risk = find.byKey(const ValueKey('home-zero-trust-endpoint-risk'));

Future<AppController> home(
  WidgetTester tester,
  ZtEndpointEngine engine, {
  LocalePreference locale = LocalePreference.english,
  Size size = const Size(1280, 900),
  double scale = 1,
}) async {
  final app = await screen(
    tester,
    engine,
    locale: locale,
    size: size,
    scale: scale,
  );
  await tester.pumpWidget(
    workflowHost(
      app,
      dark: locale == LocalePreference.simplifiedChinese,
      scale: scale,
      home: HomeScreen(controller: app),
    ),
  );
  await tester.pumpAndSettle();
  return app;
}

void observe(
  AppController app, {
  required int sequence,
  UsqueProfile? applied,
}) {
  app.networkSettings.accept(
    NetworkSettingsState(
      sourceEpoch: 'home-risk-test',
      sequence: sequence,
      appliedProfile: applied,
    ),
  );
}

void main() {
  testWidgets(
    'saved custom ZT endpoints have a non-dismissible, persistent Home warning',
    (tester) async {
      final app = await home(tester, ztEngine(custom: true));
      expect(risk, findsOneWidget);
      final banner = tester.widget<WarningBanner>(risk);
      expect(banner.danger, isTrue);
      expect(banner.onDismiss, isNull);
      expect(
        banner.message,
        app.strings.get('zero_trust_endpoint_home_risk_body'),
      );
      expect(tester.getTopLeft(risk).dy, lessThan(120));
      await tester.pumpWidget(const SizedBox.shrink());
      await tester.pumpWidget(
        workflowHost(app, home: HomeScreen(controller: app)),
      );
      await tester.pumpAndSettle();
      expect(risk, findsOneWidget);
    },
  );

  testWidgets(
    'saving and restoring addresses updates Home without recreating it',
    (tester) async {
      final app = await home(tester, ztEngine());
      expect(risk, findsNothing);
      expect(
        await app.saveNetwork(
          app.activeProfile.copyWith(endpointIpv4: '192.0.2.45'),
          changedFields: ['endpoint.ipv4'],
        ),
        isTrue,
      );
      await tester.pumpAndSettle();
      expect(risk, findsOneWidget);
      expect(
        await app.saveNetwork(
          app.activeProfile.copyWith(endpointIpv4: '162.159.197.2'),
          changedFields: ['endpoint.ipv4'],
        ),
        isTrue,
      );
      await tester.pumpAndSettle();
      expect(risk, findsNothing);
    },
  );

  testWidgets(
    'a still-applied custom session keeps the warning after a saved reset or account switch',
    (tester) async {
      final app = await home(tester, ztEngine());
      final registered = app.activeProfile;
      final custom = registered.copyWith(endpointIpv6: '2001:db8::45');
      app.snapshot = const EngineSnapshot(phase: ConnectionPhase.connected);
      observe(app, sequence: 1, applied: custom);
      await tester.pumpAndSettle();
      expect(risk, findsOneWidget);
      app.setActiveProfile(UsqueProfile.defaultProfileId);
      await tester.pumpAndSettle();
      expect(risk, findsOneWidget);
      app.snapshot = const EngineSnapshot(phase: ConnectionPhase.disconnected);
      observe(app, sequence: 2, applied: custom);
      await tester.pumpAndSettle();
      expect(risk, findsNothing);
      app.setActiveProfile(ztId);
      app.snapshot = const EngineSnapshot(phase: ConnectionPhase.connected);
      observe(app, sequence: 3, applied: custom);
      await tester.pumpAndSettle();
      expect(risk, findsOneWidget);
      observe(app, sequence: 4, applied: registered);
      await tester.pumpAndSettle();
      expect(risk, findsNothing);
    },
  );

  for (final phase in [
    ConnectionPhase.preparing,
    ConnectionPhase.reconnecting,
    ConnectionPhase.disconnecting,
  ]) {
    testWidgets('retains the risk while a custom session is ${phase.name}', (
      tester,
    ) async {
      final app = await home(tester, ztEngine());
      app.snapshot = EngineSnapshot(phase: phase);
      observe(
        app,
        sequence: 1,
        applied: app.activeProfile.copyWith(endpointIpv4: '192.0.2.45'),
      );
      await tester.pumpAndSettle();
      expect(risk, findsOneWidget);
    });
  }

  testWidgets('ordinary WARP addresses do not trigger the ZT warning', (
    tester,
  ) async {
    final engine = ztEngine(custom: true)
      ..storedActiveProfileId = UsqueProfile.defaultProfileId;
    engine.storedProfiles = [
      engine.storedProfiles.first.copyWith(
        endpointIpv4: '192.0.2.45',
        endpointIpv6: '2001:db8::45',
      ),
      engine.storedProfiles.last,
    ];
    await home(tester, engine);
    expect(risk, findsNothing);
  });

  testWidgets('registered addresses and port/SNI edits are not custom ZT IPs', (
    tester,
  ) async {
    final engine = ztEngine();
    engine.storedProfiles = [
      for (final profile in engine.storedProfiles)
        profile.copyWith(
          endpointPort: 8443,
          sni: 'example.com',
          endpointIpv6: profile.id == ztId
              ? '2606:4700:0102:0000:0000:0000:0000:0002'
              : profile.endpointIpv6,
        ),
    ];
    await home(tester, engine);
    expect(risk, findsNothing);
  });

  testWidgets(
    'unavailable registration metadata does not guess that an address is custom',
    (tester) async {
      await home(tester, ztEngine(custom: true, registered: false));
      expect(risk, findsNothing);
    },
  );

  for (final locale in [
    LocalePreference.english,
    LocalePreference.simplifiedChinese,
    LocalePreference.persian,
  ]) {
    testWidgets(
      'warning fits 200 percent text and semantics in ${locale.name}',
      (tester) async {
        final app = await home(
          tester,
          ztEngine(custom: true),
          locale: locale,
          size: const Size(360, 800),
          scale: 2,
        );
        final semantics = tester.ensureSemantics();
        try {
          // Home's existing host uses English widgets for Persian; preserve the
          // actual RTL text direction explicitly for this fixture.
          await tester.pumpWidget(
            workflowHost(
              app,
              scale: 2,
              dark: locale != LocalePreference.english,
              home: Directionality(
                textDirection: locale == LocalePreference.persian
                    ? TextDirection.rtl
                    : TextDirection.ltr,
                child: HomeScreen(controller: app),
              ),
            ),
          );
          await tester.pumpAndSettle();
          expect(risk, findsOneWidget);
          expect(
            tester.getSemantics(risk).getSemanticsData().label,
            contains(app.strings.get('zero_trust_endpoint_home_risk_title')),
          );
          expect(tester.takeException(), isNull);
        } finally {
          semantics.dispose();
        }
      },
    );
  }

  test('all catalogs translate the persistent warning', () {
    expect(AppStrings.debugCatalogsAreComplete, isTrue);
    expect(
      AppStrings.debugUntranslatedKeys([
        'zero_trust_endpoint_home_risk_title',
        'zero_trust_endpoint_home_risk_body',
      ]),
      isEmpty,
    );
  });
}
