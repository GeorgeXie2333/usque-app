import 'dart:typed_data';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/screens/advanced_settings_screen.dart';
import 'package:usque/services/control_codec.dart';
import 'package:usque/services/engine_client.dart';
import 'package:usque/state/app_controller.dart';

import 'app_test.dart' show FakeEngineClient;
import 'congestion_control_test.dart' show codec, decodeProfile, response;
import 'ui_workflow_test.dart' show workflowHost;

void main() {
  test('count persists through JSON, optional protobuf and reset', () {
    for (var count = 1; count <= 6; count++) {
      final profile = UsqueProfile.defaultProfile().copyWith(
        masqueConnections: count,
      );
      expect(UsqueProfile.fromMap(profile.toMap()).masqueConnections, count);
      expect(
        decodeProfile(codec.encodeProfile(profile)).masqueConnections,
        count,
      );
      expect(profile.resetAdvancedDefaults().masqueConnections, 1);
    }
    final legacy = UsqueProfile.defaultProfile().toMap()
      ..remove('masque_connections');
    expect(UsqueProfile.fromMap(legacy).masqueConnections, 1);
    expect(decodeProfile(Uint8List(0)).masqueConnections, 1);
    for (final value in [0, 7, -1, 1.5, '2', null]) {
      expect(
        () => UsqueProfile.fromMap({...legacy, 'masque_connections': value}),
        throwsFormatException,
      );
    }
    for (final value in [0, 7]) {
      expect(
        () => decodeProfile(
          (ControlPayloadWriter()..optionalUnsigned(27, value)).takeBytes(),
        ),
        throwsA(isA<EngineException>()),
      );
    }
  });

  test('capabilities 56 and 57 gate older engines', () {
    final caps = codec
        .decodeResponse(
          response(
            15,
            (ControlPayloadWriter()
                  ..unsigned(56, 6)
                  ..boolean(57, true))
                .takeBytes(),
          ),
          'cc',
        )
        .capabilities!;
    expect(caps.maxMasqueConnections, 6);
    expect(caps.l4MasqueConnections, isTrue);
    expect(
      codec
          .decodeResponse(response(15, Uint8List(0)), 'cc')
          .capabilities!
          .l4MasqueConnections,
      isFalse,
    );
    expect(
      codec
          .decodeResponse(response(15, Uint8List(0)), 'cc')
          .capabilities!
          .maxMasqueConnections,
      0,
    );
    expect(
      EngineCapabilities.fromMap({
        'max_masque_connections': 6,
        'l4_masque_connections': true,
      }).maxMasqueConnections,
      6,
    );
  });

  Finder field() => find.descendant(
    of: find.byKey(const ValueKey('masque-connections')),
    matching: find.byType(TextFormField),
  );

  Future<AppController> screen(
    WidgetTester tester, {
    bool supported = true,
    bool l4 = false,
    bool l4Supported = true,
    bool h2 = false,
    double scale = 1,
  }) async {
    tester.view.physicalSize = const Size(430, 1000);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);
    final app = AppController(FakeEngineClient())
      ..localePreference = LocalePreference.english
      ..engineCapabilities = EngineCapabilities(
        networkSettingsApplication: true,
        automaticEndpoints: true,
        l4Tcp: true,
        l4TunTcp: true,
        l4DnsConversion: true,
        tlsFingerprints: TlsFingerprint.values,
        maxMasqueConnections: supported ? 6 : 0,
        l4MasqueConnections: supported && l4Supported,
      );
    await app.networkSettings.refresh();
    app.sharedNetwork = app.sharedNetwork.copyWith(
      dataPlane: l4 ? DataPlaneMode.l4Proxy : DataPlaneMode.connectIp,
      transport: h2 ? TransportPolicy.http2 : TransportPolicy.http3,
    );
    addTearDown(app.dispose);
    await tester.pumpWidget(
      workflowHost(
        app,
        scale: scale,
        home: AdvancedSettingsScreen(controller: app),
      ),
    );
    await tester.pumpAndSettle();
    await tester.ensureVisible(field());
    await tester.pumpAndSettle();
    return app;
  }

  testWidgets('manual entry validates bounds and saves the exact field', (
    tester,
  ) async {
    final app = await screen(tester);
    expect(tester.widget<TextFormField>(field()).controller!.text, '1');
    for (final invalid in ['0', '7', '10', '']) {
      await tester.enterText(field(), invalid);
      await tester.pumpAndSettle();
      await tester.tap(find.widgetWithText(FilledButton, 'Apply changes'));
      await tester.pumpAndSettle();
      expect(find.text('1–6'), findsOneWidget);
      expect(app.sharedNetwork.masqueConnections, 1);
    }
    await tester.enterText(field(), '6');
    await tester.pumpAndSettle();
    await tester.tap(find.widgetWithText(FilledButton, 'Apply changes'));
    await tester.pumpAndSettle();
    expect(app.sharedNetwork.masqueConnections, 6);
    expect(app.networkSettings.state?.deferredFields, ['masque_connections']);
    expect(tester.takeException(), isNull);
  });

  testWidgets('H2 supports manual entry at 200 percent text', (tester) async {
    await screen(tester, h2: true, scale: 2);
    expect(tester.widget<TextFormField>(field()).enabled, isTrue);
    await tester.enterText(field(), '2');
    expect(tester.takeException(), isNull);
  });

  testWidgets('L4 saves the same count field', (tester) async {
    final app = await screen(tester, l4: true);
    expect(tester.widget<TextFormField>(field()).enabled, isTrue);
    await tester.enterText(field(), '4');
    await tester.pumpAndSettle();
    await tester.tap(find.widgetWithText(FilledButton, 'Apply changes'));
    await tester.pumpAndSettle();
    expect(app.sharedNetwork.masqueConnections, 4);
    expect(app.networkSettings.state?.deferredFields, ['masque_connections']);
  });

  testWidgets('older L4 and general engines keep the count read only', (
    tester,
  ) async {
    await screen(tester, l4: true, l4Supported: false);
    expect(tester.widget<TextFormField>(field()).enabled, isFalse);
    await tester.pumpWidget(const SizedBox());
    await screen(tester, supported: false);
    expect(tester.widget<TextFormField>(field()).enabled, isFalse);
    expect(
      find.text('Update Usque to configure multiple MASQUE connections.'),
      findsOneWidget,
    );
  });
}
