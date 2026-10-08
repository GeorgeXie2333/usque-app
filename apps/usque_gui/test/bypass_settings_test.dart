import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:shared_preferences/shared_preferences.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/screens/geo_direct_settings_screen.dart';
import 'package:usque/services/engine_client.dart';
import 'package:usque/widgets/save_changes_bar.dart';
import 'audit_forms_test.dart' show FormEngine, appFor;
import 'ui_workflow_test.dart' show workflowHost;

class RejectRuleEngine extends FormEngine {
  @override
  Future<NetworkSettingsState> saveNetworkSettings(
    String operationId,
    String accountId,
    UsqueProfile values,
    List<String> changedFields,
  ) async {
    throw EngineException(
      'CONFIGURATION_INVALID',
      'ROUTING_RULE_INVALID:${values.routing.rules.last.id}',
    );
  }
}

class ConflictRuleEngine extends FormEngine {
  ConflictRuleEngine({this.timeoutOnRetry = false});
  final bool timeoutOnRetry;
  int attempts = 0;
  @override
  Future<NetworkSettingsState> saveNetworkSettings(
    String operationId,
    String accountId,
    UsqueProfile values,
    List<String> changedFields,
  ) async {
    if (attempts++ > 0 && timeoutOnRetry) {
      throw const EngineException('NETWORK_SETTINGS_UNCONFIRMED', 'unknown');
    }
    throw EngineException(
      'ROUTING_RULE_CONFLICT',
      'ROUTING_RULE_CONFLICT:${values.routing.rules[0].id}:${values.routing.rules[1].id}',
    );
  }
}

Future<void> pasteRules(WidgetTester tester, String text) async {
  await tester.tap(find.byKey(const ValueKey('routing-batch')));
  await tester.pumpAndSettle();
  await tester.enterText(
    find.byKey(const ValueKey('routing-target-input')),
    text,
  );
  await tester.tap(find.widgetWithText(FilledButton, 'Save'));
  await tester.pumpAndSettle();
}

void main() {
  setUp(
    () => SharedPreferences.setMockInitialValues({
      'onboarding_complete': true,
      'update_checks_enabled': false,
    }),
  );

  testWidgets(
    'custom rules save without GEO and invalid bulk drafts remain editable',
    (tester) async {
      final engine = FormEngine();
      final app = appFor(engine);
      await app.initialize();
      await tester.pumpAndSettle();
      app.engineCapabilities = const EngineCapabilities(
        automaticEndpoints: true,
        networkSettingsApplication: true,
        routingRules: true,
      );
      await tester.pumpWidget(
        workflowHost(app, home: GeoDirectSettingsScreen(controller: app)),
      );
      await tester.pumpAndSettle();
      await pasteRules(
        tester,
        '192.0.2.1\n2001:db8::1\nExample.com\n*.bad.com',
      );
      expect(engine.saves, 0);
      expect(find.textContaining('Line 4:'), findsOneWidget);
      expect(find.byType(AlertDialog), findsOneWidget);
      await tester.enterText(
        find.byKey(const ValueKey('routing-target-input')),
        '192.0.2.1\n2001:db8::1\nExample.com',
      );
      await tester.tap(find.widgetWithText(FilledButton, 'Save'));
      await tester.pumpAndSettle();
      tester.widget<SaveChangesBar>(find.byType(SaveChangesBar)).onSave!();
      await tester.pumpAndSettle();
      expect(engine.savedValues!.routing.rules.map((rule) => rule.target), [
        '192.0.2.1/32',
        '2001:db8::1/128',
        'example.com',
      ]);
      expect(
        engine.savedValues!.routing.rules.every(
          (rule) => rule.action == RoutingAction.reject,
        ),
        isTrue,
      );
      expect(engine.savedValues!.geoDirectCountries, isEmpty);
      expect(engine.savedFields, ['routing']);
      expect(tester.takeException(), isNull);
    },
  );

  testWidgets('old engines show existing targets read only', (tester) async {
    final app = appFor(FormEngine());
    await app.initialize();
    await tester.pumpAndSettle();
    app.sharedNetwork = app.sharedNetwork.copyWith(
      bypassDomains: ['example.com'],
    );
    await tester.pumpWidget(
      workflowHost(app, home: GeoDirectSettingsScreen(controller: app)),
    );
    await tester.pumpAndSettle();
    expect(find.text('1. example.com'), findsOneWidget);
    expect(
      tester
          .widget<FilledButton>(find.byKey(const ValueKey('routing-add')))
          .onPressed,
      isNull,
    );
    expect(
      tester
          .widget<SwitchListTile>(find.byKey(const ValueKey('routing-ads')))
          .onChanged,
      isNull,
    );
    expect(find.text(app.strings.get('bypass_unsupported')), findsOneWidget);
  });

  testWidgets(
    'authoritative IDNA conflict is visible and retains both draft rows',
    (tester) async {
      final app = appFor(ConflictRuleEngine());
      await app.initialize();
      await tester.pumpAndSettle();
      app.engineCapabilities = const EngineCapabilities(
        automaticEndpoints: true,
        networkSettingsApplication: true,
        routingRules: true,
      );
      app.sharedNetwork = app.sharedNetwork.copyWith(
        routing: RoutingSettings(
          rules: [RoutingRule.create('bücher.de', RoutingAction.direct)],
        ),
      );
      await tester.pumpWidget(
        workflowHost(app, home: GeoDirectSettingsScreen(controller: app)),
      );
      await tester.pumpAndSettle();
      await pasteRules(tester, 'xn--bcher-kva.de');
      tester.widget<SaveChangesBar>(find.byType(SaveChangesBar)).onSave!();
      await tester.pumpAndSettle();
      expect(app.networkSettings.routingErrorIds, hasLength(2));
      expect(app.networkSettings.unconfirmed, isFalse);
      expect(
        find.text('${app.strings.get('routing_conflict')} (1, 2)'),
        findsOneWidget,
      );
      expect(find.text('1. bücher.de'), findsOneWidget);
      expect(find.text('2. xn--bcher-kva.de'), findsOneWidget);
      expect(
        tester.widget<SaveChangesBar>(find.byType(SaveChangesBar)).dirty,
        isTrue,
      );
    },
  );

  testWidgets(
    'backend rule rejection identifies the row and preserves the draft',
    (tester) async {
      final app = appFor(RejectRuleEngine());
      await app.initialize();
      await tester.pumpAndSettle();
      app.engineCapabilities = const EngineCapabilities(
        automaticEndpoints: true,
        networkSettingsApplication: true,
        routingRules: true,
      );
      await tester.pumpWidget(
        workflowHost(app, home: GeoDirectSettingsScreen(controller: app)),
      );
      await tester.pumpAndSettle();
      await pasteRules(tester, '192.0.2.1\nexample.com');
      tester.widget<SaveChangesBar>(find.byType(SaveChangesBar)).onSave!();
      await tester.pumpAndSettle();
      expect(app.networkSettings.routingErrorIds, hasLength(1));
      expect(find.text('2. example.com'), findsOneWidget);
      expect(app.activeProfile.routing.rules, isEmpty);
      expect(
        tester.widget<SaveChangesBar>(find.byType(SaveChangesBar)).dirty,
        isTrue,
      );
    },
  );

  testWidgets(
    'a corrected rule followed by timeout keeps the result uncertain',
    (tester) async {
      final app = appFor(ConflictRuleEngine(timeoutOnRetry: true));
      await app.initialize();
      await tester.pumpAndSettle();
      app.engineCapabilities = const EngineCapabilities(
        automaticEndpoints: true,
        networkSettingsApplication: true,
        routingRules: true,
      );
      app.sharedNetwork = app.sharedNetwork.copyWith(
        routing: RoutingSettings(
          rules: [RoutingRule.create('bücher.de', RoutingAction.direct)],
        ),
      );
      await tester.pumpWidget(
        workflowHost(app, home: GeoDirectSettingsScreen(controller: app)),
      );
      await tester.pumpAndSettle();
      await pasteRules(tester, 'xn--bcher-kva.de');
      tester.widget<SaveChangesBar>(find.byType(SaveChangesBar)).onSave!();
      await tester.pumpAndSettle();
      expect(app.networkSettings.routingErrorIds, hasLength(2));
      await tester.tap(find.text('2. xn--bcher-kva.de'));
      await tester.pumpAndSettle();
      await tester.enterText(
        find.byKey(const ValueKey('routing-target-input')),
        'allowed.test',
      );
      await tester.tap(find.widgetWithText(FilledButton, 'Save'));
      await tester.pumpAndSettle();
      tester.widget<SaveChangesBar>(find.byType(SaveChangesBar)).onSave!();
      await tester.pumpAndSettle();
      expect(app.networkSettings.unconfirmed, isTrue);
      expect(app.networkSettings.routingErrorIds, isEmpty);
      expect(app.networkSettings.routingErrorKey, isNull);
      final bar = tester.widget<SaveChangesBar>(find.byType(SaveChangesBar));
      expect(bar.statusLabel, app.networkSettingsMessage);
      expect(find.text(bar.statusLabel!), findsOneWidget);
      expect(
        find.textContaining(app.strings.get('routing_conflict')),
        findsNothing,
      );
      expect(find.text('2. allowed.test'), findsOneWidget);
    },
  );

  testWidgets(
    'conflicts prevent applying while more specific exceptions are allowed',
    (tester) async {
      final app = appFor(FormEngine());
      await app.initialize();
      await tester.pumpAndSettle();
      app.engineCapabilities = const EngineCapabilities(
        automaticEndpoints: true,
        networkSettingsApplication: true,
        routingRules: true,
      );
      app.sharedNetwork = app.sharedNetwork.copyWith(
        routing: RoutingSettings(
          rules: [RoutingRule.create('example.com', RoutingAction.direct)],
        ),
      );
      await tester.pumpWidget(
        workflowHost(app, home: GeoDirectSettingsScreen(controller: app)),
      );
      await tester.pumpAndSettle();
      await pasteRules(tester, 'ads.example.com');
      expect(
        tester.widget<SaveChangesBar>(find.byType(SaveChangesBar)).onSave,
        isNotNull,
      );
      expect(
        find.textContaining(app.strings.get('routing_overlap')),
        findsOneWidget,
      );
      await pasteRules(tester, 'Example.COM.');
      expect(
        tester.widget<SaveChangesBar>(find.byType(SaveChangesBar)).onSave,
        isNull,
      );
      expect(
        find.textContaining(app.strings.get('routing_conflict')),
        findsOneWidget,
      );
      expect(find.text('3. example.com'), findsOneWidget);
      expect(tester.takeException(), isNull);
    },
  );

  testWidgets(
    'Ads without downloaded data remains configurable at large text',
    (tester) async {
      final app = appFor(FormEngine());
      await app.initialize();
      await tester.pumpAndSettle();
      app.engineCapabilities = const EngineCapabilities(
        automaticEndpoints: true,
        networkSettingsApplication: true,
        routingRules: true,
      );
      await tester.pumpWidget(
        workflowHost(
          app,
          scale: 2,
          home: GeoDirectSettingsScreen(controller: app),
        ),
      );
      await tester.pumpAndSettle();
      final ads = find.byKey(const ValueKey('routing-ads'));
      await tester.ensureVisible(ads);
      tester.widget<SwitchListTile>(ads).onChanged!(true);
      await tester.pumpAndSettle();
      tester.widget<SaveChangesBar>(find.byType(SaveChangesBar)).onSave!();
      await tester.pumpAndSettle();
      expect(app.activeProfile.routing.adsEnabled, isTrue);
      expect(tester.takeException(), isNull);
    },
  );

  for (final platform in [TargetPlatform.windows, TargetPlatform.android]) {
    testWidgets(
      'routing editor supports keyboard and D-pad actions on $platform',
      (tester) async {
        final engine = FormEngine();
        final app = appFor(engine);
        await app.initialize();
        await tester.pumpAndSettle();
        app.engineCapabilities = const EngineCapabilities(
          automaticEndpoints: true,
          networkSettingsApplication: true,
          routingRules: true,
        );
        await tester.pumpWidget(
          workflowHost(app, home: GeoDirectSettingsScreen(controller: app)),
        );
        await tester.pumpAndSettle();
        final add = find.byKey(const ValueKey('routing-add'));
        await tester.ensureVisible(add);
        Focus.of(
          tester.element(find.descendant(of: add, matching: find.byType(Text))),
        ).requestFocus();
        await tester.pump();
        final activate = platform == TargetPlatform.android
            ? LogicalKeyboardKey.select
            : LogicalKeyboardKey.enter;
        await tester.sendKeyEvent(activate);
        await tester.pumpAndSettle();
        expect(find.byType(AlertDialog), findsOneWidget);

        final selector = find.byType(DropdownButtonFormField<RoutingAction>);
        tester
            .widgetList<Focus>(
              find.descendant(of: selector, matching: find.byType(Focus)),
            )
            .map((widget) => widget.focusNode)
            .whereType<FocusNode>()
            .first
            .requestFocus();
        await tester.pump();
        await tester.sendKeyEvent(activate);
        await tester.pumpAndSettle();
        await tester.sendKeyEvent(LogicalKeyboardKey.arrowDown);
        await tester.sendKeyEvent(activate);
        await tester.pumpAndSettle();
        await tester.enterText(
          find.byKey(const ValueKey('routing-target-input')),
          'allowed.example',
        );
        final save = find.widgetWithText(FilledButton, 'Save');
        Focus.of(
          tester.element(
            find.descendant(of: save, matching: find.text('Save')),
          ),
        ).requestFocus();
        await tester.pump();
        await tester.sendKeyEvent(activate);
        await tester.pumpAndSettle();
        expect(find.byType(AlertDialog), findsNothing);
        tester.widget<SaveChangesBar>(find.byType(SaveChangesBar)).onSave!();
        await tester.pumpAndSettle();
        expect(
          engine.savedValues!.routing.rules.single.action,
          RoutingAction.proxy,
        );
        expect(tester.takeException(), isNull);
      },
      variant: TargetPlatformVariant.only(platform),
    );
  }
}
