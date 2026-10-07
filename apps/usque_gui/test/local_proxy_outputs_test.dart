import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/screens/proxy_screen.dart';
import 'package:usque/screens/settings_screen.dart';

import 'ui_workflow_test.dart';

void main() {
  for (final platform in [TargetPlatform.windows, TargetPlatform.android]) {
    testWidgets(
      'proxy entry groups switches and preserves unapplied listener edits: ${platform.name}',
      (tester) async {
        final engine = WorkflowEngine();
        final app = await pumpWorkflow(
          tester,
          engine,
          section: AppSection.settings,
        );
        final settings = find.byType(SettingsScreen);
        for (final output in [
          'SOCKS5',
          app.strings.tunnelOutputLabel(platform),
        ]) {
          expect(
            find.descendant(
              of: settings,
              matching: find.widgetWithText(SwitchListTile, output),
            ),
            findsNothing,
          );
        }
        app.selectSection(AppSection.proxy);
        await tester.pumpAndSettle();
        expect(find.byType(ProxyScreen), findsOneWidget);
        expect(
          find.text(app.strings.get('proxy_switches_hint')),
          findsOneWidget,
        );
        final systemProxy = find.widgetWithText(
          SwitchListTile,
          app.strings.get('system_proxy'),
        );
        expect(
          systemProxy,
          platform == TargetPlatform.windows ? findsOneWidget : findsNothing,
        );
        // Each output keeps the icon Home uses for the same concept.
        Finder rowIcon(Finder row, IconData icon) =>
            find.descendant(of: row, matching: find.byIcon(icon));
        expect(
          rowIcon(
            find.widgetWithText(
              SwitchListTile,
              app.strings.tunnelOutputLabel(platform),
            ),
            LucideIcons.ethernetPort,
          ),
          findsOneWidget,
        );
        expect(
          rowIcon(
            find.widgetWithText(SwitchListTile, 'HTTP'),
            LucideIcons.globe,
          ),
          findsOneWidget,
        );
        if (platform == TargetPlatform.windows) {
          expect(rowIcon(systemProxy, LucideIcons.monitorCog), findsOneWidget);
        }
        final port = fieldWithLabel(app.strings.get('port'));
        await tester.ensureVisible(port);
        await tester.enterText(port, '9090');
        await tester.pumpAndSettle();
        expect(engine.writes, 0);
        final http = find.widgetWithText(SwitchListTile, 'HTTP');
        await tester.ensureVisible(http);
        await tester.pumpAndSettle();
        await tester.tap(http);
        await tester.pumpAndSettle();
        expect(engine.writes, 1);
        expect(app.activeProfile.frontends.http, isFalse);
        expect(app.activeProfile.proxy.socksPort, 1080);
        expect(tester.widget<TextField>(port).controller!.text, '9090');
        if (platform == TargetPlatform.windows) {
          expect(tester.widget<SwitchListTile>(systemProxy).onChanged, isNull);
        }
        await tester.tap(
          find.widgetWithText(FilledButton, app.strings.get('save_changes')),
        );
        await tester.pumpAndSettle();
        expect(app.activeProfile.proxy.socksPort, 9090);
        expect(app.activeProfile.frontends.http, isFalse);
      },
      variant: TargetPlatformVariant.only(platform),
    );
  }
}
