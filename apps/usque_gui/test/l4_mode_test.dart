import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:usque/core/l10n/l4.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/screens/advanced_settings_screen.dart';
import 'package:usque/screens/proxy_screen.dart';
import 'package:usque/services/control_codec.dart';
import 'package:usque/state/app_controller.dart';

import 'app_test.dart' show FakeEngineClient;
import 'congestion_control_test.dart' show codec, decodeProfile, response;
import 'ui_workflow_test.dart' show workflowHost;

void main() {
  testWidgets('edge-resolved DNS does not claim local DNS leakage', (
    tester,
  ) async {
    final controller = AppController(FakeEngineClient());
    controller.sharedNetwork = controller.sharedNetwork.copyWith(
      dataPlane: DataPlaneMode.l4Proxy,
      proxy: controller.sharedNetwork.proxy.copyWith(
        dnsMode: ProxyDnsMode.edgeResolved,
      ),
    );
    addTearDown(controller.dispose);
    await tester.pumpWidget(
      workflowHost(controller, home: ProxyScreen(controller: controller)),
    );
    await tester.pumpAndSettle();
    expect(find.text(controller.strings.get('dns_leak_warning')), findsNothing);
    expect(tester.takeException(), isNull);
  });

  test('L4 is an appended dimension and legacy preferences survive', () {
    final legacy = UsqueProfile.defaultProfile().copyWith(
      transport: TransportPolicy.http2,
      sni: 'legacy.example.com',
    );
    final l4 = legacy.copyWith(dataPlane: DataPlaneMode.l4Proxy);
    expect(
      decodeProfile(codec.encodeProfile(l4)).dataPlane,
      DataPlaneMode.l4Proxy,
    );
    expect(UsqueProfile.fromMap(l4.toMap()).dataPlane, DataPlaneMode.l4Proxy);
    expect(l4.transport, TransportPolicy.http2);
    expect(l4.sni, legacy.sni);
    expect(l4.resetAdvancedDefaults().dataPlane, DataPlaneMode.connectIp);
    expect(l4.resetAdvancedDefaults().transport, TransportPolicy.automatic);
    expect(
      UsqueProfile.fromMap(legacy.toMap()..remove('data_plane')).dataPlane,
      DataPlaneMode.connectIp,
    );
    expect(
      () => UsqueProfile.fromMap({...legacy.toMap(), 'data_plane': 'auto'}),
      throwsFormatException,
    );
    expect(kL4En.keys.toSet(), kL4ZhCn.keys.toSet());
  });

  test('old capabilities disable L4 and unknown status is never success', () {
    expect(const EngineCapabilities().l4Available, isFalse);
    expect(EngineCapabilities.fromMap({'l4_tcp': true}).l4Available, isFalse);
    final payload =
        (ControlPayloadWriter()
              ..boolean(26, true)
              ..boolean(27, true)
              ..boolean(28, true))
            .takeBytes();
    expect(
      codec
          .decodeResponse(response(15, payload), 'cc')
          .capabilities!
          .l4Available,
      isTrue,
    );
    expect(
      EngineSnapshot.fromMap({
        'phase': 'connected',
        'data_plane': 'future',
      }).dataPlane,
      isNull,
    );
    expect(
      EngineSnapshot.fromMap({
        'phase': 'connected',
        'data_plane': 'l4_proxy',
      }).l4,
      isNull,
    );
    final live = EngineSnapshot.fromMap({
      'phase': 'connected',
      'data_plane': 'l4_proxy',
      'l4': {'connect_verified': false},
    });
    expect(live.l4!.connectVerified, isFalse);
    expect(live, isNot(const EngineSnapshot(phase: ConnectionPhase.connected)));
  });

  for (final supported in [false, true]) {
    testWidgets(
      'unified selector, preserved SNI and 200% accessibility support=$supported',
      (tester) async {
        tester.view.physicalSize = const Size(420, 900);
        tester.view.devicePixelRatio = 1;
        addTearDown(tester.view.resetPhysicalSize);
        addTearDown(tester.view.resetDevicePixelRatio);
        final controller = AppController(FakeEngineClient())
          ..localePreference = LocalePreference.simplifiedChinese
          ..engineCapabilities = EngineCapabilities(
            l4Tcp: supported,
            l4TunTcp: supported,
            l4DnsConversion: supported,
          );
        controller.sharedNetwork = controller.sharedNetwork.copyWith(
          sni: 'legacy.example.com',
          transport: TransportPolicy.http2,
        );
        addTearDown(controller.dispose);
        await tester.pumpWidget(
          workflowHost(
            controller,
            scale: 2,
            dark: true,
            home: AdvancedSettingsScreen(controller: controller),
          ),
        );
        await tester.pumpAndSettle();
        final selector = tester.widget<SegmentedButton<String>>(
          find.byType(SegmentedButton<String>),
        );
        expect(selector.segments.last.value, 'l4');
        expect(selector.segments.last.enabled, supported);
        if (supported) {
          selector.onSelectionChanged!({'l4'});
          await tester.pumpAndSettle();
          expect(
            find.widgetWithText(
              TextFormField,
              'consumer-masque-proxy.cloudflareclient.com',
            ),
            findsOneWidget,
          );
          expect(controller.sharedNetwork.sni, 'legacy.example.com');
          tester
              .widget<SegmentedButton<String>>(
                find.byType(SegmentedButton<String>),
              )
              .onSelectionChanged!({'http2'});
          await tester.pumpAndSettle();
          expect(
            find.widgetWithText(TextFormField, 'legacy.example.com'),
            findsOneWidget,
          );
        }
        expect(tester.takeException(), isNull);
      },
    );
  }
}
