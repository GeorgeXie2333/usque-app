import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/models/network_settings.dart';
import 'package:usque/screens/advanced_settings_screen.dart';
import 'package:usque/screens/proxy_screen.dart';
import 'package:usque/screens/settings_screen.dart';
import 'package:usque/state/app_controller.dart';
import 'package:usque/widgets/common.dart';
import 'package:usque/widgets/local_proxy_outputs.dart';

import 'ui_workflow_test.dart'
    show WorkflowEngine, fieldWithLabel, pumpWorkflow, workflowHost;

void main() {
  testWidgets('Settings groups tools apart and leaves outputs to Proxy', (
    tester,
  ) async {
    final app = await pumpWorkflow(
      tester,
      WorkflowEngine(),
      section: AppSection.settings,
    );
    final settings = find.byType(SettingsScreen);
    double top(Finder finder) => tester.getTopLeft(finder).dy;
    Finder heading(String key) => find.descendant(
      of: settings,
      matching: find.text(app.strings.get(key)),
    );
    final connection = top(heading('connection_protection_group'));
    final routing = top(heading('proxy_routing_group'));
    final tools = top(heading('tools_group'));
    final application = top(heading('application_group'));
    expect(connection, lessThan(routing));
    expect(routing, lessThan(tools));
    expect(tools, lessThan(application));
    final diagnostics = top(heading('diagnostics'));
    expect(diagnostics, greaterThan(tools));
    expect(diagnostics, lessThan(application));
    expect(
      find.descendant(
        of: settings,
        matching: find.widgetWithText(
          SwitchListTile,
          app.strings.tunnelOutputLabel(defaultTargetPlatform),
        ),
      ),
      findsNothing,
    );
    expect(
      find.descendant(
        of: settings,
        matching: find.text('Local proxy settings'),
      ),
      findsNothing,
    );
    final autoConnect = find.widgetWithText(
      SwitchListTile,
      app.strings.get('auto_connect'),
    );
    expect(autoConnect, findsOneWidget);
    expect(top(autoConnect), lessThan(routing));
  });

  testWidgets('Settings Kill Switch row reports the configured state', (
    tester,
  ) async {
    final app = await pumpWorkflow(
      tester,
      WorkflowEngine(),
      section: AppSection.settings,
    );
    String value() => tester
        .widget<Text>(find.byKey(const ValueKey('settings-kill-switch-value')))
        .data!;
    Future<void> show(UsqueProfile profile) async {
      app.sharedNetwork = profile;
      await tester.pumpWidget(
        workflowHost(app, home: SettingsScreen(controller: app)),
      );
      await tester.pumpAndSettle();
    }

    final base = app.sharedNetwork.copyWith(
      frontends: const FrontendSettings(tunnel: true, socks5: true, http: true),
    );
    await show(base.copyWith(killSwitch: true));
    expect(value(), app.strings.get('on'));
    await show(base.copyWith(killSwitch: false));
    expect(value(), app.strings.get('off'));
    await show(
      base.copyWith(
        killSwitch: true,
        frontends: const FrontendSettings(
          tunnel: false,
          socks5: true,
          http: true,
        ),
      ),
    );
    expect(value(), app.strings.get('not_used_proxy'));
  });

  testWidgets('Kill Switch row reveals the draft switch without applying', (
    tester,
  ) async {
    final engine = WorkflowEngine();
    final app = await pumpWorkflow(
      tester,
      engine,
      section: AppSection.settings,
      size: const Size(375, 812),
    );
    final row = find.byKey(const ValueKey('settings-kill-switch-row'));
    await tester.ensureVisible(row);
    await tester.pumpAndSettle();
    await tester.tap(row);
    await tester.pumpAndSettle();
    final advanced = find.byType(AdvancedSettingsScreen);
    expect(advanced, findsOneWidget);
    expect(
      find
          .descendant(
            of: advanced,
            matching: find.widgetWithText(
              SwitchListTile,
              app.strings.get('kill_switch'),
            ),
          )
          .hitTestable(),
      findsOneWidget,
    );
    expect(engine.writes, 0);
  });

  testWidgets('Proxy outputs lead with the tunnel switch', (tester) async {
    final engine = WorkflowEngine();
    final app = await pumpWorkflow(tester, engine);
    expect(find.byType(ProxyScreen), findsOneWidget);
    final outputs = find.byType(LocalProxyOutputs);
    Finder output(String label) => find.descendant(
      of: outputs,
      matching: find.widgetWithText(SwitchListTile, label),
    );
    final tunnel = output(app.strings.tunnelOutputLabel(defaultTargetPlatform));
    expect(tunnel, findsOneWidget);
    expect(
      tester.getTopLeft(tunnel).dy,
      lessThan(tester.getTopLeft(output('SOCKS5')).dy),
    );
    expect(app.activeProfile.frontends.tunnel, isTrue);
    await tester.ensureVisible(tunnel);
    await tester.pumpAndSettle();
    await tester.tap(tunnel);
    await tester.pumpAndSettle();
    expect(engine.writes, 1);
    expect(app.activeProfile.frontends.tunnel, isFalse);
  });

  testWidgets('phone Settings rows keep their chevron beside the text', (
    tester,
  ) async {
    final app = await pumpWorkflow(
      tester,
      WorkflowEngine(),
      section: AppSection.settings,
      size: const Size(375, 812),
    );
    for (final key in ['kill_switch', 'geo_direct']) {
      final row = find.widgetWithText(LinkRow, app.strings.get(key));
      await tester.ensureVisible(row);
      await tester.pumpAndSettle();
      final rowRect = tester.getRect(row);
      final title = tester.getRect(
        find.descendant(of: row, matching: find.text(app.strings.get(key))),
      );
      final chevron = tester.getRect(
        find.descendant(
          of: row,
          matching: find.byIcon(LucideIcons.chevronRightDir),
        ),
      );
      expect(chevron.right, closeTo(rowRect.right - 8, 1), reason: key);
      expect(chevron.left, greaterThan(title.right), reason: key);
      // Vertically centred on the row, not pushed onto a line of its own.
      expect(chevron.center.dy, closeTo(rowRect.center.dy, 1), reason: key);
    }
    final value = find.byKey(const ValueKey('settings-kill-switch-value'));
    expect(value, findsOneWidget);
    expect(
      tester.getTopLeft(value).dx,
      tester
          .getTopLeft(
            find.descendant(
              of: find.byKey(const ValueKey('settings-kill-switch-row')),
              matching: find.text(app.strings.get('kill_switch')),
            ),
          )
          .dx,
    );
  });

  testWidgets('Application rows are flat and show the installed version', (
    tester,
  ) async {
    final app = await pumpWorkflow(
      tester,
      WorkflowEngine(),
      section: AppSection.settings,
    );
    final settings = find.byType(SettingsScreen);
    for (final key in ['appearance', 'system_integration', 'updates']) {
      expect(
        find.descendant(
          of: settings,
          matching: find.text(app.strings.get(key)),
        ),
        findsNothing,
        reason: key,
      );
    }
    final version = find.byKey(const ValueKey('settings-app-version'));
    await tester.ensureVisible(version);
    await tester.pumpAndSettle();
    expect(tester.widget<Text>(version).data, app.strings.get('app_version'));
    final checkNow = find.text(app.strings.get('check_now'));
    expect(
      tester.getCenter(version).dy,
      closeTo(tester.getCenter(checkNow).dy, 2),
    );
    // Theme and language labels share the switch rows' text column.
    expect(
      tester.getTopLeft(find.text(app.strings.get('theme'))).dx,
      tester.getTopLeft(find.text(app.strings.get('check_updates'))).dx,
    );
  });

  testWidgets('Settings reports only network-settings problems', (
    tester,
  ) async {
    final app = await pumpWorkflow(
      tester,
      WorkflowEngine(),
      section: AppSection.settings,
    );
    Future<void> show(NetworkSettingsApplyStatus status, int sequence) async {
      app.networkSettings.accept(
        NetworkSettingsState(
          sourceEpoch: 'settings-status',
          sequence: sequence,
          operationId: 'operation-$sequence',
          status: status,
          persisted: true,
        ),
      );
      await tester.pumpWidget(
        workflowHost(app, home: SettingsScreen(controller: app)),
      );
      await tester.pumpAndSettle();
    }

    final settings = find.byType(SettingsScreen);
    Finder text(String key) => find.descendant(
      of: settings,
      matching: find.text(app.strings.get(key)),
    );
    await show(NetworkSettingsApplyStatus.applied, 1);
    expect(app.networkSettingsMessage, app.strings.get('settings_applied'));
    expect(text('settings_applied'), findsNothing);
    expect(find.byType(WarningBanner), findsNothing);

    await show(NetworkSettingsApplyStatus.failed, 2);
    expect(text('settings_failed'), findsOneWidget);
    final banner = find.ancestor(
      of: text('settings_failed'),
      matching: find.byType(WarningBanner),
    );
    expect(tester.widget<WarningBanner>(banner).danger, isTrue);
    expect(
      find.widgetWithText(
        OutlinedButton,
        app.strings.get('settings_reconnect'),
      ),
      findsOneWidget,
    );
    // Shown above the groups, not inside the auto-connect row.
    expect(
      tester.getBottomLeft(banner).dy,
      lessThan(tester.getTopLeft(text('connection_protection_group')).dy),
    );
  });

  testWidgets('Proxy apply bar appears with edits and lines up with the form', (
    tester,
  ) async {
    final app = await pumpWorkflow(tester, WorkflowEngine());
    final bar = find.byKey(const ValueKey('proxy-save-bar'));
    expect(app.networkSettingsMessage, isNull);
    expect(bar, findsNothing);
    final port = fieldWithLabel(app.strings.get('port'));
    await tester.enterText(port, '9090');
    await tester.pumpAndSettle();
    expect(bar, findsOneWidget);
    final apply = find.widgetWithText(
      FilledButton,
      app.strings.get('save_changes'),
    );
    // The bar uses the form's 880 px column and page gutter.
    expect(tester.getTopRight(apply).dx, tester.getTopRight(port).dx);
    await tester.enterText(port, '1080');
    await tester.pumpAndSettle();
    expect(bar, findsNothing);
  });

  testWidgets('phone keeps the last listener field visible as the bar opens', (
    tester,
  ) async {
    final app = await pumpWorkflow(
      tester,
      WorkflowEngine(),
      size: const Size(375, 812),
    );
    final httpPort = find
        .byWidgetPredicate(
          (widget) =>
              widget is TextField &&
              widget.decoration?.labelText == app.strings.get('port'),
        )
        .last;
    await tester.ensureVisible(httpPort);
    await tester.pumpAndSettle();
    await tester.tap(httpPort);
    await tester.pumpAndSettle();
    await tester.enterText(httpPort, '8081');
    await tester.pumpAndSettle();
    expect(find.byKey(const ValueKey('proxy-save-bar')), findsOneWidget);
    expect(httpPort.hitTestable(), findsOneWidget);
  });

  group('Advanced network settings', () {
    Future<AppController> pumpAdvanced(
      WidgetTester tester, {
      Size size = const Size(1280, 900),
    }) async {
      final app = await pumpWorkflow(
        tester,
        WorkflowEngine(),
        section: AppSection.settings,
        size: size,
      );
      app.engineCapabilities = const EngineCapabilities(
        automaticEndpoints: true,
        h3CongestionControlAlgorithms: CongestionControlAlgorithm.values,
      );
      await tester.pumpWidget(
        workflowHost(app, home: AdvancedSettingsScreen(controller: app)),
      );
      await tester.pumpAndSettle();
      return app;
    }

    Finder heading(String title) => find.widgetWithText(ContentHeading, title);

    testWidgets('sections run from protection to transport', (tester) async {
      final app = await pumpAdvanced(tester);
      final order = <Finder>[
        heading(app.strings.get('routing_protection')),
        find.byType(WarningBanner),
        heading(app.strings.get('warp_dns_type')),
        heading(app.strings.get('nq_direct_dns')),
        heading(app.strings.get('endpoint_section')),
        heading(app.strings.get('transport')),
      ];
      final tops = [for (final finder in order) tester.getTopLeft(finder).dy];
      for (var index = 1; index < tops.length; index++) {
        expect(tops[index], greaterThan(tops[index - 1]), reason: '$index');
      }
      // The banner no longer repeats the page title.
      expect(tester.widget<WarningBanner>(order[1]).title, isNull);
      expect(find.text(app.strings.get('advanced_subtitle')), findsNothing);
      expect(
        find.text(app.strings.get('shared_network_scope')),
        findsOneWidget,
      );
      // Endpoint IP version and MTU live with the endpoint and transport.
      expect(
        tester.getTopLeft(find.text(app.strings.get('ip_policy'))).dy,
        greaterThan(tops[4]),
      );
      expect(
        tester.getTopLeft(find.text(app.strings.get('mtu'))).dy,
        greaterThan(tops[5]),
      );
    });

    testWidgets('congestion control and MTU share one row and height', (
      tester,
    ) async {
      final app = await pumpAdvanced(tester);
      final cc = find.byKey(const ValueKey('congestion-control'));
      final mtu = find.byWidgetPredicate(
        (widget) =>
            widget is TextField &&
            widget.decoration?.labelText == app.strings.get('mtu'),
      );
      await tester.ensureVisible(mtu);
      await tester.pumpAndSettle();
      final ccRect = tester.getRect(cc);
      final mtuRect = tester.getRect(mtu);
      expect(ccRect.top, mtuRect.top);
      expect(ccRect.height, closeTo(mtuRect.height, 0.5));
      expect(mtuRect.left, greaterThan(ccRect.right));
      // The apply bar lines up with the 880 px form column.
      final apply = find.widgetWithText(
        FilledButton,
        app.strings.get('save_changes'),
      );
      expect(tester.getTopRight(apply).dx, closeTo(mtuRect.right, 0.5));
    });

    testWidgets('phone keeps endpoint choices side by side', (tester) async {
      await pumpAdvanced(tester, size: const Size(375, 812));
      final selector = find.byType(SegmentedButton<EndpointSelection>);
      expect(
        tester.widget<SegmentedButton<EndpointSelection>>(selector).direction,
        Axis.horizontal,
      );
    });

    testWidgets('validation focuses the first invalid field on the page', (
      tester,
    ) async {
      final app = await pumpAdvanced(tester);
      Finder field(String key) => find.byWidgetPredicate(
        (widget) =>
            widget is TextField &&
            widget.decoration?.labelText == app.strings.get(key),
      );
      await tester.enterText(field('port'), '70000');
      await tester.enterText(field('dns_ipv4'), 'not-an-address');
      await tester.pumpAndSettle();
      await tester.tap(
        find.widgetWithText(FilledButton, app.strings.get('save_changes')),
      );
      await tester.pumpAndSettle();
      // WARP DNS now precedes the endpoint, so its error is focused first.
      expect(
        tester.widget<TextField>(field('dns_ipv4')).focusNode!.hasFocus,
        isTrue,
      );
    });
  });
}
