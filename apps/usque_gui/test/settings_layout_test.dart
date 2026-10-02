import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/screens/advanced_settings_screen.dart';
import 'package:usque/screens/proxy_screen.dart';
import 'package:usque/screens/settings_screen.dart';
import 'package:usque/widgets/local_proxy_outputs.dart';

import 'ui_workflow_test.dart' show WorkflowEngine, pumpWorkflow, workflowHost;

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
}
