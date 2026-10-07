import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:shared_preferences/shared_preferences.dart';
import 'package:usque/core/app_strings.dart';
import 'package:usque/core/usque_theme.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/models/network_settings.dart';
import 'package:usque/screens/advanced_settings_screen.dart';
import 'package:usque/services/control_codec.dart';
import 'package:usque/state/app_controller.dart';
import 'package:usque/widgets/save_changes_bar.dart';
import 'package:usque/widgets/warp_dns_editor.dart';

import 'audit_forms_test.dart' show FormEngine, decode;
import 'congestion_control_test.dart' show response;
import 'ui_workflow_test.dart' show fieldWithLabel, workflowHost;

const custom = WarpDnsSettings(
  mode: WarpDnsMode.doh,
  serverName: 'dns.example',
  dohPath: '/custom-dns',
  port: 8443,
  bootstrapIps: ['192.0.2.1', '2001:db8::1'],
);

// Keep unedited DNS addresses, matching the core's field-scoped merge.
class ScopedDnsEngine extends FormEngine {
  @override
  Future<NetworkSettingsState> saveNetworkSettings(
    String operationId,
    String accountId,
    UsqueProfile values,
    List<String> changedFields,
  ) {
    final prior = storedProfiles.firstWhere(
      (profile) => profile.id == accountId,
    );
    return super.saveNetworkSettings(
      operationId,
      accountId,
      changedFields.contains('dns_servers')
          ? values
          : values.copyWith(dnsIpv4: prior.dnsIpv4, dnsIpv6: prior.dnsIpv6),
      changedFields,
    );
  }
}

Future<AppController> advancedHost(
  WidgetTester tester,
  FormEngine engine,
) async {
  await tester.binding.setSurfaceSize(const Size(1000, 1200));
  addTearDown(() => tester.binding.setSurfaceSize(null));
  final app = AppController(engine)
    ..localePreference = LocalePreference.english;
  addTearDown(app.dispose);
  app.engineCapabilities = const EngineCapabilities(
    automaticEndpoints: true,
    networkSettingsApplication: true,
    encryptedWarpDns: true,
  );
  await tester.pumpWidget(
    workflowHost(app, home: AdvancedSettingsScreen(controller: app)),
  );
  await tester.pumpAndSettle();
  return app;
}

Future<void> fillEncryptedDraft(WidgetTester tester, AppController app) async {
  await selectMode(tester, app.strings.get('nq_doh'));
  await tester.enterText(
    fieldWithLabel(app.strings.get('nq_dns_server')),
    'draft.example',
  );
  await tester.enterText(
    fieldWithLabel(app.strings.get('nq_dns_path')),
    '/draft-query',
  );
  await tester.enterText(warpFieldWithLabel(app.strings.get('port')), '8443');
  await tester.enterText(
    fieldWithLabel(app.strings.get('nq_dns_bootstrap')),
    '192.0.2.1\n2001:db8::1',
  );
}

Future<void> applyAdvanced(WidgetTester tester) async {
  tester.widget<SaveChangesBar>(find.byType(SaveChangesBar)).onSave!();
  await tester.pumpAndSettle();
}

Future<void> resetAdvanced(
  WidgetTester tester,
  AppController app, {
  required bool confirm,
}) async {
  tester
      .state<ScrollableState>(find.byType(Scrollable).first)
      .position
      .jumpTo(0);
  await tester.pumpAndSettle();
  await tester.tap(find.text(app.strings.get('reset_defaults')));
  await tester.pumpAndSettle();
  await tester.tap(
    find.descendant(
      of: find.byType(AlertDialog),
      matching: find.text(app.strings.get(confirm ? 'reset' : 'cancel')),
    ),
  );
  await tester.pumpAndSettle();
}

String fieldText(WidgetTester tester, String label) =>
    tester.widget<TextField>(fieldWithLabel(label)).controller!.text;

Finder warpFieldWithLabel(String label) => find.descendant(
  of: find.byType(WarpDnsEditor),
  matching: find.byWidgetPredicate(
    (widget) => widget is TextField && widget.decoration?.labelText == label,
  ),
);

String warpFieldText(WidgetTester tester, String label) =>
    tester.widget<TextField>(warpFieldWithLabel(label)).controller!.text;

Future<void> selectMode(WidgetTester tester, String label) async {
  final selector = find.byType(DropdownButtonFormField<WarpDnsMode>);
  await tester.ensureVisible(selector);
  await tester.pumpAndSettle();
  await tester.tap(selector);
  await tester.pumpAndSettle();
  await tester.tap(find.text(label).last);
  await tester.pumpAndSettle();
}

void main() {
  setUp(
    () => SharedPreferences.setMockInitialValues({
      'update_checks_enabled': false,
    }),
  );

  test(
    'WARP DNS maps, protobuf 24, reset and mask preserve ordinary addresses',
    () {
      final plain = UsqueProfile.defaultProfile();
      final encrypted = plain.copyWith(warpDns: custom);
      expect(UsqueProfile.fromMap(encrypted.toMap()).warpDns, custom);
      expect(
        decode(const ControlCodec().encodeProfile(encrypted)).warpDns,
        custom,
      );
      expect(networkSettingsChangedFields(plain, encrypted), ['warp_dns']);
      expect(
        encrypted.resetAdvancedDefaults().warpDns,
        const WarpDnsSettings(),
      );
      expect(encrypted.dnsIpv4, plain.dnsIpv4);
      expect(encrypted.dnsIpv6, plain.dnsIpv6);
      expect(
        UsqueProfile.fromMap(plain.toMap()..remove('warp_dns')).warpDns,
        const WarpDnsSettings(),
      );
      expect(
        decode(const ControlCodec().encodeProfile(plain)).warpDns,
        const WarpDnsSettings(),
      );
      expect(
        WarpDnsSettings.fromMap({'mode': 'future'}).mode,
        WarpDnsMode.unknown,
      );
    },
  );

  test(
    'encrypted WARP capability 44 and Android map fail closed by default',
    () {
      const codec = ControlCodec();
      final wire = (ControlPayloadWriter()..boolean(44, true)).takeBytes();
      expect(wire, [0xe0, 2, 1]);
      final capabilities = codec
          .decodeResponse(response(15, wire), 'cc')
          .capabilities!;
      expect(capabilities.encryptedWarpDns, isTrue);
      expect(
        EngineCapabilities.fromMap({'encrypted_warp_dns': true}),
        capabilities,
      );
      expect(const EngineCapabilities().encryptedWarpDns, isFalse);
      expect(EngineCapabilities.fromMap({}).encryptedWarpDns, isFalse);
      expect(capabilities, isNot(const EngineCapabilities()));
    },
  );

  for (final locale in [
    LocalePreference.english,
    LocalePreference.simplifiedChinese,
  ]) {
    for (final dark in [false, true]) {
      testWidgets(
        'WARP DNS fits narrow 200 percent ${locale.name} dark=$dark',
        (tester) async {
          await tester.binding.setSurfaceSize(const Size(375, 900));
          addTearDown(() => tester.binding.setSurfaceSize(null));
          final strings = AppStrings(locale);
          await tester.pumpWidget(
            MaterialApp(
              theme: dark ? UsqueTheme.dark() : UsqueTheme.light(),
              home: Builder(
                builder: (context) => MediaQuery(
                  data: MediaQuery.of(
                    context,
                  ).copyWith(textScaler: TextScaler.linear(2)),
                  child: Scaffold(
                    body: SingleChildScrollView(
                      child: Form(
                        child: WarpDnsEditor(
                          value: custom,
                          enabled: true,
                          strings: strings,
                          onChanged: (_) {},
                        ),
                      ),
                    ),
                  ),
                ),
              ),
            ),
          );
          await tester.pumpAndSettle();
          expect(tester.takeException(), isNull);
          expect(find.text(strings.get('nq_dns_scope')), findsNothing);
          expect(find.text(strings.get('nq_dns_no_fallback')), findsNothing);
          await tester.drag(
            find.byType(SingleChildScrollView),
            const Offset(0, -2000),
          );
          await tester.pumpAndSettle();
          expect(tester.takeException(), isNull);
        },
      );
    }
  }

  testWidgets(
    'picker retains encrypted drafts and validates only active fields',
    (tester) async {
      final form = GlobalKey<FormState>();
      final editor = GlobalKey<WarpDnsEditorState>();
      var value = const WarpDnsSettings();
      await tester.pumpWidget(
        MaterialApp(
          home: Scaffold(
            body: SingleChildScrollView(
              child: Form(
                key: form,
                child: WarpDnsEditor(
                  key: editor,
                  value: value,
                  enabled: true,
                  strings: AppStrings(LocalePreference.english),
                  onChanged: (next) => value = next,
                ),
              ),
            ),
          ),
        ),
      );
      expect(find.byType(TextFormField), findsNothing);
      await selectMode(
        tester,
        AppStrings(LocalePreference.english).get('nq_doh'),
      );
      expect(value.port, 443);
      expect(value.dohPath, '/dns-query');
      expect(form.currentState!.validate(), isFalse);
      editor.currentState!.focusFirstError();
      await tester.pump();
      expect(
        tester
            .widget<TextField>(find.byType(TextField).first)
            .focusNode!
            .hasFocus,
        isTrue,
      );
      await tester.enterText(find.byType(TextFormField).at(0), 'dns.example');
      await tester.enterText(find.byType(TextFormField).at(1), '/saved-path');
      await tester.enterText(find.byType(TextFormField).at(2), '8443');
      expect(form.currentState!.validate(), isTrue);
      expect(value.bootstrapIps, isEmpty);
      expect(find.text('Optional'), findsOneWidget);
      await tester.enterText(find.byType(TextFormField).at(3), 'invalid-ip');
      expect(form.currentState!.validate(), isFalse);
      await tester.enterText(find.byType(TextFormField).at(3), '192.0.2.1');
      expect(form.currentState!.validate(), isTrue);
      await selectMode(
        tester,
        AppStrings(LocalePreference.english).get('nq_dot'),
      );
      expect(value.port, 853);
      expect(value.dohPath, isEmpty);
      await tester.enterText(find.byType(TextFormField).at(2), '');
      expect(form.currentState!.validate(), isTrue);
      expect(value.bootstrapIps, isEmpty);
      await tester.enterText(find.byType(TextFormField).at(1), '8853');
      await selectMode(tester, 'Plain DNS');
      expect(value, const WarpDnsSettings());
      expect(form.currentState!.validate(), isTrue);
      await selectMode(
        tester,
        AppStrings(LocalePreference.english).get('nq_doh'),
      );
      expect(value.serverName, 'dns.example');
      expect(value.dohPath, '/saved-path');
      expect(value.port, 8443);
      await selectMode(
        tester,
        AppStrings(LocalePreference.english).get('nq_dot'),
      );
      expect(value.port, 8853);
      // The same Flutter focus traversal is used by keyboard and TV D-pad.
      await tester.sendKeyEvent(LogicalKeyboardKey.tab);
      await tester.pump();
      expect(tester.takeException(), isNull);
    },
  );

  testWidgets(
    'unsupported engine preserves encrypted values and disables encrypted choices',
    (tester) async {
      var value = custom;
      await tester.pumpWidget(
        MaterialApp(
          home: Scaffold(
            body: SingleChildScrollView(
              child: Form(
                child: WarpDnsEditor(
                  value: custom,
                  enabled: true,
                  encryptedAvailable: false,
                  strings: AppStrings(LocalePreference.english),
                  onChanged: (next) => value = next,
                ),
              ),
            ),
          ),
        ),
      );
      final selector = tester.widget<DropdownButton<WarpDnsMode>>(
        find.byType(DropdownButton<WarpDnsMode>),
      );
      expect(
        selector.items!
            .where((item) => item.value != WarpDnsMode.plain)
            .every((item) => !item.enabled),
        isTrue,
      );
      expect(
        tester
            .widgetList<TextField>(find.byType(TextField))
            .every((field) => field.readOnly),
        isTrue,
      );
      expect(value, custom);
      await selectMode(tester, 'Plain DNS');
      expect(value, const WarpDnsSettings());
    },
  );

  testWidgets('plain DNS drafts persist when applied after an encrypted save', (
    tester,
  ) async {
    final engine = ScopedDnsEngine();
    final app = await advancedHost(tester, engine);
    final originalV4 = app.activeProfile.dnsIpv4;
    final originalV6 = app.activeProfile.dnsIpv6;
    const draftV4 = '9.9.9.9';
    const draftV6 = '2620:fe::fe';
    await tester.ensureVisible(fieldWithLabel('DNS IPv4'));
    await tester.enterText(fieldWithLabel('DNS IPv4'), draftV4);
    await tester.enterText(fieldWithLabel('DNS IPv6'), draftV6);
    await fillEncryptedDraft(tester, app);
    await applyAdvanced(tester);
    expect(app.activeProfile.dnsIpv4, originalV4);
    expect(app.activeProfile.dnsIpv6, originalV6);
    expect(engine.savedFields, ['warp_dns']);
    expect(
      tester.widget<SaveChangesBar>(find.byType(SaveChangesBar)).dirty,
      isFalse,
    );
    await selectMode(tester, app.strings.get('warp_dns_plain'));
    expect(fieldText(tester, 'DNS IPv4'), draftV4);
    expect(fieldText(tester, 'DNS IPv6'), draftV6);
    expect(
      tester.widget<SaveChangesBar>(find.byType(SaveChangesBar)).dirty,
      isTrue,
    );
    await applyAdvanced(tester);
    expect(engine.savedFields, unorderedEquals(['dns_servers', 'warp_dns']));
    expect(app.activeProfile.warpDns, const WarpDnsSettings());
    expect(app.activeProfile.dnsIpv4, draftV4);
    expect(app.activeProfile.dnsIpv6, draftV6);
    expect(
      tester.widget<SaveChangesBar>(find.byType(SaveChangesBar)).dirty,
      isFalse,
    );
    expect(tester.takeException(), isNull);
  });

  testWidgets('encrypted WARP DNS saves without server IP addresses', (
    tester,
  ) async {
    final engine = ScopedDnsEngine();
    final app = await advancedHost(tester, engine);
    await fillEncryptedDraft(tester, app);
    await tester.enterText(
      fieldWithLabel(app.strings.get('nq_dns_bootstrap')),
      '',
    );
    await applyAdvanced(tester);
    expect(engine.savedFields, ['warp_dns']);
    expect(app.activeProfile.warpDns.mode, WarpDnsMode.doh);
    expect(app.activeProfile.warpDns.bootstrapIps, isEmpty);
    expect(tester.takeException(), isNull);
  });

  testWidgets(
    'confirmed Reset from plain clears hidden encrypted drafts and custom ports',
    (tester) async {
      final engine = FormEngine();
      final app = await advancedHost(tester, engine);
      await fillEncryptedDraft(tester, app);
      await selectMode(tester, app.strings.get('nq_dot'));
      await tester.enterText(
        warpFieldWithLabel(app.strings.get('port')),
        '8853',
      );
      await selectMode(tester, app.strings.get('warp_dns_plain'));
      await resetAdvanced(tester, app, confirm: true);
      expect(engine.saves, 0);
      expect(app.activeProfile.warpDns, const WarpDnsSettings());
      for (final mode in [WarpDnsMode.doh, WarpDnsMode.dot]) {
        await selectMode(
          tester,
          app.strings.get(mode == WarpDnsMode.doh ? 'nq_doh' : 'nq_dot'),
        );
        expect(fieldText(tester, app.strings.get('nq_dns_server')), isEmpty);
        expect(fieldText(tester, app.strings.get('nq_dns_bootstrap')), isEmpty);
        expect(
          warpFieldText(tester, app.strings.get('port')),
          mode == WarpDnsMode.doh ? '443' : '853',
        );
        if (mode == WarpDnsMode.doh) {
          expect(
            fieldText(tester, app.strings.get('nq_dns_path')),
            '/dns-query',
          );
        } else {
          expect(
            find.byWidgetPredicate(
              (widget) =>
                  widget is TextField &&
                  widget.decoration?.labelText ==
                      app.strings.get('nq_dns_path'),
            ),
            findsNothing,
          );
        }
      }
      expect(engine.saves, 0);
      expect(tester.takeException(), isNull);
    },
  );

  testWidgets(
    'cancelled Reset from plain preserves hidden encrypted drafts and ports',
    (tester) async {
      final engine = FormEngine();
      final app = await advancedHost(tester, engine);
      await fillEncryptedDraft(tester, app);
      await selectMode(tester, app.strings.get('nq_dot'));
      await tester.enterText(
        warpFieldWithLabel(app.strings.get('port')),
        '8853',
      );
      await selectMode(tester, app.strings.get('warp_dns_plain'));
      await resetAdvanced(tester, app, confirm: false);
      await selectMode(tester, app.strings.get('nq_doh'));
      expect(
        fieldText(tester, app.strings.get('nq_dns_server')),
        'draft.example',
      );
      expect(fieldText(tester, app.strings.get('nq_dns_path')), '/draft-query');
      expect(warpFieldText(tester, app.strings.get('port')), '8443');
      expect(
        fieldText(tester, app.strings.get('nq_dns_bootstrap')),
        '192.0.2.1\n2001:db8::1',
      );
      await selectMode(tester, app.strings.get('nq_dot'));
      expect(warpFieldText(tester, app.strings.get('port')), '8853');
      expect(
        fieldText(tester, app.strings.get('nq_dns_server')),
        'draft.example',
      );
      expect(engine.saves, 0);
      expect(app.activeProfile.warpDns, const WarpDnsSettings());
      expect(tester.takeException(), isNull);
    },
  );

  testWidgets(
    'Advanced saves only active DNS fields and retains drafts through failed saves',
    (tester) async {
      await tester.binding.setSurfaceSize(const Size(1000, 1200));
      addTearDown(() => tester.binding.setSurfaceSize(null));
      final engine = ScopedDnsEngine();
      final app = AppController(engine)
        ..localePreference = LocalePreference.english;
      addTearDown(app.dispose);
      app.engineCapabilities = const EngineCapabilities(
        automaticEndpoints: true,
        networkSettingsApplication: true,
        encryptedWarpDns: true,
      );
      await tester.pumpWidget(
        workflowHost(app, home: AdvancedSettingsScreen(controller: app)),
      );
      await tester.pumpAndSettle();
      final originalDns = app.activeProfile.dnsIpv4;
      await tester.ensureVisible(fieldWithLabel('DNS IPv4'));
      await tester.enterText(
        fieldWithLabel('DNS IPv4'),
        'invalid-hidden-draft',
      );
      await selectMode(
        tester,
        AppStrings(LocalePreference.english).get('nq_doh'),
      );
      expect(
        find.byWidgetPredicate(
          (widget) =>
              widget is TextField && widget.decoration?.labelText == 'DNS IPv4',
        ),
        findsNothing,
      );
      await tester.enterText(
        fieldWithLabel(app.strings.get('nq_dns_server')),
        'dns.example',
      );
      await tester.enterText(
        fieldWithLabel(app.strings.get('nq_dns_bootstrap')),
        '192.0.2.1',
      );
      final apply = tester
          .widget<SaveChangesBar>(find.byType(SaveChangesBar))
          .onSave!;
      apply();
      await tester.pumpAndSettle();
      expect(engine.savedValues!.warpDns.mode, WarpDnsMode.doh);
      expect(engine.savedValues!.dnsIpv4, originalDns);
      expect(engine.savedFields, ['warp_dns']);
      engine.failProfileUpsert = true;
      await tester.enterText(
        fieldWithLabel(app.strings.get('nq_dns_server')),
        'changed.example',
      );
      tester.widget<SaveChangesBar>(find.byType(SaveChangesBar)).onSave!();
      await tester.pumpAndSettle();
      expect(app.activeProfile.warpDns.serverName, 'dns.example');
      expect(
        tester
            .widget<TextField>(fieldWithLabel(app.strings.get('nq_dns_server')))
            .controller!
            .text,
        'changed.example',
      );
      await selectMode(tester, 'Plain DNS');
      expect(
        tester.widget<TextField>(fieldWithLabel('DNS IPv4')).controller!.text,
        'invalid-hidden-draft',
      );
    },
  );
}
