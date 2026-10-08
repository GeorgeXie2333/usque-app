import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:shared_preferences/shared_preferences.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/screens/proxy_screen.dart';
import 'package:usque/services/engine_client.dart';
import 'package:usque/state/app_controller.dart';

import 'audit_forms_test.dart' show FormEngine, appFor, apply;
import 'ui_workflow_test.dart' show workflowHost;

Finder get port => find
    .byWidgetPredicate(
      (widget) => widget is TextField && widget.decoration?.labelText == 'Port',
    )
    .first;

void confirmSettings(AppController app, FormEngine engine) {
  final profile = app.activeProfile;
  engine.storedProfiles = [profile];
  engine.settingsState = NetworkSettingsState(
    sourceEpoch: engine.settingsEpoch,
    sequence: ++engine.settingsSequence,
    storedProfile: profile,
    sharedNetwork: profile,
  );
  app.engineCapabilities = const EngineCapabilities(
    automaticEndpoints: true,
    networkSettingsApplication: true,
    l4Tcp: true,
    l4TunTcp: true,
    l4DnsConversion: true,
  );
  app.networkSettings
    ..supported = true
    ..accept(engine.settingsState!);
}

void expectNoDnsControls() {
  for (final key in [
    'proxy-dns-mode',
    'proxy-dns-ipv4',
    'proxy-dns-ipv6',
    'proxy-dns-override',
    'proxy-dns-restore',
  ]) {
    expect(find.byKey(ValueKey(key)), findsNothing);
  }
  expect(find.byType(DropdownButtonFormField<ProxyDnsMode>), findsNothing);
}

void main() {
  setUp(
    () => SharedPreferences.setMockInitialValues({
      'update_checks_enabled': false,
    }),
  );

  for (final mode in ProxyDnsMode.values) {
    testWidgets('proxy hides DNS controls and preserves saved $mode', (
      tester,
    ) async {
      final engine = FormEngine();
      final app = appFor(engine);
      app.sharedNetwork = app.sharedNetwork.copyWith(
        dataPlane: DataPlaneMode.l4Proxy,
        dnsIpv4: '8.8.8.8',
        proxy: ProxySettings(
          dnsMode: mode,
          dnsIpv4: '9.9.9.9',
          dnsIpv6: '2620:fe::fe',
          authUsername: 'saved-user',
        ),
      );
      confirmSettings(app, engine);
      await tester.pumpWidget(
        workflowHost(app, home: ProxyScreen(controller: app)),
      );
      await tester.pumpAndSettle();
      expectNoDnsControls();
      expect(engine.saves, 0);
      expect(
        find.text(app.strings.get('dns_leak_warning')),
        mode == ProxyDnsMode.localConfigured || mode == ProxyDnsMode.system
            ? findsOneWidget
            : findsNothing,
      );
      await tester.enterText(port, '9090');
      await tester.pump();
      apply(tester);
      await tester.pumpAndSettle();
      expect(engine.savedFields, ['proxy.socks5_listeners']);
      expect(app.activeProfile.proxy.dnsMode, mode);
      expect(app.activeProfile.proxy.dnsIpv4, '9.9.9.9');
      expect(app.activeProfile.proxy.dnsIpv6, '2620:fe::fe');
      expect(app.activeProfile.proxy.authUsername, 'saved-user');
      expect(app.activeProfile.proxy.socksPort, 9090);
      expect(app.activeProfile.dnsIpv4, '8.8.8.8');
      expectNoDnsControls();
      expect(tester.takeException(), isNull);
    });
  }

  for (final confirmedMode in [ProxyDnsMode.remote, ProxyDnsMode.system]) {
    testWidgets(
      'listener draft preserves externally confirmed $confirmedMode',
      (tester) async {
        final engine = FormEngine();
        final app = appFor(engine);
        app.sharedNetwork = app.sharedNetwork.copyWith(
          dataPlane: DataPlaneMode.l4Proxy,
          proxy: ProxySettings(
            dnsMode: confirmedMode == ProxyDnsMode.remote
                ? ProxyDnsMode.edgeResolved
                : ProxyDnsMode.remote,
          ),
        );
        confirmSettings(app, engine);
        Widget page() => workflowHost(app, home: ProxyScreen(controller: app));
        await tester.pumpWidget(page());
        await tester.pumpAndSettle();
        await tester.enterText(port, '9090');
        app.sharedNetwork = app.sharedNetwork.copyWith(
          dataPlane: DataPlaneMode.connectIp,
          proxy: app.sharedNetwork.proxy.copyWith(
            dnsMode: confirmedMode,
            dnsIpv4: '9.9.9.9',
          ),
        );
        confirmSettings(app, engine);
        await tester.pumpWidget(page());
        await tester.pumpAndSettle();
        expectNoDnsControls();
        expect(find.text(app.strings.get('l4_edge_requires_l4')), findsNothing);
        apply(tester);
        await tester.pumpAndSettle();
        expect(engine.savedFields, ['proxy.socks5_listeners']);
        expect(engine.savedValues?.proxy.dnsMode, confirmedMode);
        expect(app.activeProfile.proxy.dnsMode, confirmedMode);
        expect(app.activeProfile.proxy.dnsIpv4, '9.9.9.9');
        expect(app.activeProfile.proxy.socksPort, 9090);
      },
    );
  }
}
