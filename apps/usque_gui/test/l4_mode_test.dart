import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:usque/core/app_strings.dart';
import 'package:usque/core/l10n/l4.dart';
import 'package:usque/core/l10n/network_settings.dart';
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
    for (final table in kL4Catalogs.values) {
      expect(table.keys.toSet(), kL4En.keys.toSet());
      final hint = table['l4_transport_hint']!;
      expect(hint.length, lessThan(table['l4_explanation']!.length));
      for (final term in ['TCP', 'TUN', 'DNS', 'Auto', 'L4']) {
        expect(hint, contains(term));
      }
    }
    expect(kL4Catalogs['ja']!['l4_mode'], isNot(kL4En['l4_mode']));
    for (final table in kNetworkSettingsCatalogs.values) {
      expect(table.keys.toSet(), kNetworkSettingsEn.keys.toSet());
    }
    expect(
      kNetworkSettingsCatalogs['ja']!['settings_reconnect'],
      isNot(kNetworkSettingsEn['settings_reconnect']),
    );
  });

  test('transport copy resolves in every locale without English fallback', () {
    final covered = <String>{};
    for (final preference in LocalePreference.values) {
      if (preference == LocalePreference.system) continue;
      final strings = AppStrings(preference);
      final catalog = kL4Catalogs[strings.catalogId]!;
      final parts = strings.catalogId.split('_');
      final system = AppStrings(
        LocalePreference.system,
        systemLocale: Locale(parts.first, parts.length > 1 ? parts.last : null),
      );
      for (final key in ['l4_mode', 'l4_transport_hint', 'l4_unsupported']) {
        expect(
          strings.get(key),
          catalog[key],
          reason: '${strings.catalogId}.$key',
        );
        expect(
          system.get(key),
          catalog[key],
          reason: 'system ${strings.catalogId}.$key',
        );
        if (strings.catalogId != 'en') {
          expect(
            strings.get(key),
            isNot(kL4En[key]),
            reason: '${strings.catalogId}.$key',
          );
        }
      }
      covered.add(strings.catalogId);
    }
    expect(covered, kL4Catalogs.keys.toSet());
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
        expect(
          selector.segments.last.tooltip,
          supported ? isNull : controller.strings.get('l4_unsupported'),
        );
        expect(find.byKey(const ValueKey('l4-transport-hint')), findsNothing);
        expect(
          find.text(controller.strings.get('l4_explanation')),
          findsNothing,
        );
        expect(
          find.text(controller.strings.get('l4_unsupported')),
          findsNothing,
        );
        if (supported) {
          selector.onSelectionChanged!({'l4'});
          await tester.pumpAndSettle();
          expect(
            find.text(controller.strings.get('l4_transport_hint')),
            findsOneWidget,
          );
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
          expect(find.byKey(const ValueKey('l4-transport-hint')), findsNothing);
          expect(
            find.widgetWithText(TextFormField, 'legacy.example.com'),
            findsOneWidget,
          );
        }
        expect(tester.takeException(), isNull);
      },
    );
  }

  for (final scenario in [
    (size: const Size(375, 812), dark: false, scale: 1.0, zh: false),
    (size: const Size(375, 812), dark: true, scale: 2.0, zh: true),
    (size: const Size(812, 375), dark: false, scale: 2.0, zh: false),
    (size: const Size(1280, 720), dark: true, scale: 2.0, zh: true),
  ]) {
    testWidgets(
      'L4 hint follows selected draft and wraps: $scenario',
      (tester) async {
        tester.view.physicalSize = scenario.size;
        tester.view.devicePixelRatio = 1;
        addTearDown(tester.view.resetPhysicalSize);
        addTearDown(tester.view.resetDevicePixelRatio);
        final controller = AppController(FakeEngineClient())
          ..localePreference = scenario.zh
              ? LocalePreference.simplifiedChinese
              : LocalePreference.english
          ..engineCapabilities = const EngineCapabilities(
            l4Tcp: true,
            l4TunTcp: true,
            l4DnsConversion: true,
          );
        addTearDown(controller.dispose);
        await tester.pumpWidget(
          workflowHost(
            controller,
            dark: scenario.dark,
            scale: scenario.scale,
            reducedMotion: scenario.scale > 1,
            home: AdvancedSettingsScreen(controller: controller),
          ),
        );
        await tester.pumpAndSettle();
        await tester.scrollUntilVisible(
          find.byType(SegmentedButton<String>),
          200,
          scrollable: find.byType(Scrollable).first,
        );
        await tester.pumpAndSettle();
        final hint = find.byKey(const ValueKey('l4-transport-hint'));
        expect(hint, findsNothing);
        const labels = {
          'automatic': 'automatic',
          'http3': 'http3',
          'http2': 'http2',
          'l4': 'l4_mode',
        };
        for (final mode in [
          'http3',
          'http2',
          'automatic',
          'l4',
          'automatic',
          'l4',
          'http2',
        ]) {
          final segment = find.descendant(
            of: find.byType(SegmentedButton<String>),
            matching: find.text(controller.strings.get(labels[mode]!)),
          );
          await tester.ensureVisible(segment);
          await tester.pumpAndSettle();
          await tester.tap(segment);
          await tester.pumpAndSettle();
          expect(
            tester
                .widget<SegmentedButton<String>>(
                  find.byType(SegmentedButton<String>),
                )
                .selected,
            {mode},
          );
          expect(
            find.text(controller.strings.get('l4_explanation')),
            findsNothing,
          );
          expect(hint, mode == 'l4' ? findsOneWidget : findsNothing);
          if (mode == 'l4') {
            await tester.ensureVisible(hint);
            await tester.pumpAndSettle();
            expect(
              tester.getSemantics(hint).getSemanticsData().label,
              controller.strings.get('l4_transport_hint'),
            );
            expect(
              tester.widget<Semantics>(hint).properties.liveRegion,
              isTrue,
            );
            final rect = tester.getRect(hint);
            expect(rect.left, greaterThanOrEqualTo(0));
            expect(rect.right, lessThanOrEqualTo(scenario.size.width));
          }
          // Selection is only a draft until Save; the helper never changes settings.
          expect(controller.sharedNetwork.dataPlane, DataPlaneMode.connectIp);
          expect(tester.takeException(), isNull);
        }
      },
      semanticsEnabled: true,
    );
  }

  testWidgets(
    'unsupported saved L4 retains its warning until another mode is selected',
    (tester) async {
      final controller = AppController(FakeEngineClient())
        ..engineCapabilities = const EngineCapabilities();
      controller.sharedNetwork = controller.sharedNetwork.copyWith(
        dataPlane: DataPlaneMode.l4Proxy,
      );
      addTearDown(controller.dispose);
      await tester.pumpWidget(
        workflowHost(
          controller,
          home: AdvancedSettingsScreen(controller: controller),
        ),
      );
      await tester.pumpAndSettle();
      expect(
        find.text(controller.strings.get('l4_transport_hint')),
        findsOneWidget,
      );
      expect(
        find.text(controller.strings.get('l4_unsupported')),
        findsOneWidget,
      );
      tester
          .widget<SegmentedButton<String>>(find.byType(SegmentedButton<String>))
          .onSelectionChanged!({'automatic'});
      await tester.pumpAndSettle();
      expect(find.byKey(const ValueKey('l4-transport-hint')), findsNothing);
      expect(find.text(controller.strings.get('l4_unsupported')), findsNothing);
    },
  );

  for (final platform in [TargetPlatform.windows, TargetPlatform.android]) {
    testWidgets(
      'L4 helper follows keyboard and D-pad selection on $platform',
      (tester) async {
        tester.view.physicalSize = const Size(420, 900);
        tester.view.devicePixelRatio = 1;
        addTearDown(tester.view.resetPhysicalSize);
        addTearDown(tester.view.resetDevicePixelRatio);
        final controller = AppController(FakeEngineClient())
          ..engineCapabilities = const EngineCapabilities(
            l4Tcp: true,
            l4TunTcp: true,
            l4DnsConversion: true,
          );
        addTearDown(controller.dispose);
        await tester.pumpWidget(
          workflowHost(
            controller,
            home: AdvancedSettingsScreen(controller: controller),
          ),
        );
        await tester.pumpAndSettle();
        final selector = find.byType(SegmentedButton<String>);
        final l4 = find.descendant(
          of: selector,
          matching: find.text(controller.strings.get('l4_mode')),
        );
        await tester.ensureVisible(l4);
        await tester.pumpAndSettle();
        Focus.of(tester.element(l4)).requestFocus();
        await tester.pump();
        await tester.sendKeyEvent(LogicalKeyboardKey.enter);
        await tester.pumpAndSettle();
        expect(tester.widget<SegmentedButton<String>>(selector).selected, {
          'l4',
        });
        expect(find.byKey(const ValueKey('l4-transport-hint')), findsOneWidget);
        await tester.sendKeyEvent(LogicalKeyboardKey.arrowUp);
        await tester.sendKeyEvent(LogicalKeyboardKey.enter);
        await tester.pumpAndSettle();
        expect(tester.widget<SegmentedButton<String>>(selector).selected, {
          'http2',
        });
        expect(find.byKey(const ValueKey('l4-transport-hint')), findsNothing);
        expect(tester.takeException(), isNull);
      },
      variant: TargetPlatformVariant.only(platform),
    );
  }
}
