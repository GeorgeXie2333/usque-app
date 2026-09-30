import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:shared_preferences/shared_preferences.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/screens/geo_direct_settings_screen.dart';
import 'package:usque/services/engine_client.dart';
import 'package:usque/widgets/save_changes_bar.dart';
import 'audit_forms_test.dart' show FormEngine, appFor;
import 'ui_workflow_test.dart' show workflowHost;

class RejectDomainEngine extends FormEngine {
  @override
  Future<NetworkSettingsState> saveNetworkSettings(
    String operationId,
    String accountId,
    UsqueProfile values,
    List<String> changedFields,
  ) async {
    throw const EngineException(
      'CONFIGURATION_INVALID',
      'invalid bypass domain at entry 1',
    );
  }
}

void main() {
  setUp(
    () => SharedPreferences.setMockInitialValues({
      'onboarding_complete': true,
      'update_checks_enabled': false,
    }),
  );

  testWidgets(
    'custom targets save without GEO and retain drafts on invalid input',
    (tester) async {
      final engine = FormEngine()
        ..storedProfiles = [
          UsqueProfile.defaultProfile().copyWith(bypassCidrs: ['192.0.2.0/24']),
        ];
      final app = appFor(engine);
      await app.initialize();
      await tester.pumpAndSettle();
      app.engineCapabilities = const EngineCapabilities(
        automaticEndpoints: true,
        networkSettingsApplication: true,
        customBypass: true,
      );
      app.sharedNetwork = app.sharedNetwork.copyWith(
        bypassCidrs: ['192.0.2.0/24'],
      );
      expect(app.activeProfile.bypassCidrs, ['192.0.2.0/24']);
      await tester.pumpWidget(
        workflowHost(app, home: GeoDirectSettingsScreen(controller: app)),
      );
      await tester.pumpAndSettle();
      final field = find.byKey(const ValueKey('bypass-targets'));
      expect(tester.widget<TextField>(field).controller!.text, '192.0.2.0/24');
      await tester.enterText(
        field,
        '192.0.2.1\n2001:db8::1\nExample.com\n*.bad.com',
      );
      tester.widget<SaveChangesBar>(find.byType(SaveChangesBar)).onSave!();
      await tester.pump();
      expect(engine.saves, 0);
      expect(find.textContaining('Line 4:'), findsOneWidget);
      expect(
        tester.widget<TextField>(field).controller!.text,
        contains('*.bad.com'),
      );
      await tester.enterText(field, '192.0.2.1\n2001:db8::1\nExample.com');
      tester.widget<SaveChangesBar>(find.byType(SaveChangesBar)).onSave!();
      await tester.pumpAndSettle();
      expect(engine.savedValues!.bypassCidrs, [
        '192.0.2.1/32',
        '2001:db8::1/128',
      ]);
      expect(engine.savedValues!.bypassDomains, ['example.com']);
      expect(engine.savedValues!.geoDirectCountries, isEmpty);
      expect(
        engine.savedFields,
        containsAll(['split_exclusions', 'bypass_domains']),
      );
      expect(tester.takeException(), isNull);
    },
  );

  testWidgets('old engines show existing targets read only', (tester) async {
    final engine = FormEngine()
      ..storedProfiles = [
        UsqueProfile.defaultProfile().copyWith(bypassDomains: ['example.com']),
      ];
    final app = appFor(engine);
    await app.initialize();
    await tester.pumpAndSettle();
    app.sharedNetwork = app.sharedNetwork.copyWith(
      bypassDomains: ['example.com'],
    );
    await tester.pumpWidget(
      workflowHost(app, home: GeoDirectSettingsScreen(controller: app)),
    );
    await tester.pumpAndSettle();
    final field = tester.widget<TextField>(
      find.byKey(const ValueKey('bypass-targets')),
    );
    expect(field.readOnly, isTrue);
    expect(field.controller!.text, 'example.com');
    expect(find.text(app.strings.get('bypass_unsupported')), findsOneWidget);
    expect(engine.saves, 0);
  });
  testWidgets(
    'backend domain rejection maps to the input line and retains draft',
    (tester) async {
      final engine = RejectDomainEngine();
      final app = appFor(engine);
      await app.initialize();
      await tester.pumpAndSettle();
      app.engineCapabilities = const EngineCapabilities(
        automaticEndpoints: true,
        networkSettingsApplication: true,
        customBypass: true,
      );
      await tester.pumpWidget(
        workflowHost(app, home: GeoDirectSettingsScreen(controller: app)),
      );
      await tester.pumpAndSettle();
      final field = find.byKey(const ValueKey('bypass-targets'));
      await tester.enterText(field, '192.0.2.1\n\nexample.com');
      tester.widget<SaveChangesBar>(find.byType(SaveChangesBar)).onSave!();
      await tester.pumpAndSettle();
      expect(find.textContaining('Line 3:'), findsOneWidget);
      expect(
        tester.widget<TextField>(field).controller!.text,
        '192.0.2.1\n\nexample.com',
      );
      expect(app.activeProfile.bypassDomains, isEmpty);
      expect(
        tester.widget<SaveChangesBar>(find.byType(SaveChangesBar)).dirty,
        isTrue,
      );
    },
  );
}
