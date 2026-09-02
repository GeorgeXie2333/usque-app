import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:shared_preferences/shared_preferences.dart';
import 'package:usque/core/app_strings.dart';
import 'package:usque/core/usque_theme.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/state/app_controller.dart';
import 'package:usque/widgets/direct_dns_editor.dart';

import 'quality_test_support.dart';

void main() {
  for (final locale in <LocalePreference>[
    LocalePreference.english,
    LocalePreference.simplifiedChinese,
  ]) {
    testWidgets(
      'custom DNS fields fit a narrow 200 percent form ${locale.name}',
      (tester) async {
        await tester.binding.setSurfaceSize(const Size(375, 900));
        addTearDown(() => tester.binding.setSurfaceSize(null));
        const settings = DirectDnsSettings(
          mode: DirectDnsMode.doh,
          serverName: 'dns.example',
          dohPath: '/dns-query',
          port: 443,
          bootstrapIps: <String>['192.0.2.1'],
        );
        await tester.pumpWidget(
          MaterialApp(
            theme: UsqueTheme.dark(),
            home: Builder(
              builder: (context) => MediaQuery(
                data: MediaQuery.of(
                  context,
                ).copyWith(textScaler: TextScaler.linear(2)),
                child: Scaffold(
                  body: SingleChildScrollView(
                    child: Form(
                      child: DirectDnsEditor(
                        value: settings,
                        enabled: true,
                        strings: AppStrings(locale),
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
        await tester.drag(
          find.byType(SingleChildScrollView).first,
          const Offset(0, -2000),
        );
        await tester.pumpAndSettle();
        expect(tester.takeException(), isNull);
      },
    );
  }
  test(
    'custom resolver validation rejects ambiguous names, paths and bootstrap lists',
    () {
      for (final value in <String>[
        '',
        'https://dns.example',
        'dns.example:443',
        '*.example',
        'dns.example ',
        '127.0.0.1',
        '-dns.example',
        'dns..example',
      ]) {
        expect(validDirectDnsName(value), isFalse, reason: value);
      }
      expect(validDirectDnsName('dns.example'), isTrue);
      expect(validDirectDnsName('例子.测试'), isTrue);
      for (final value in <String>[
        '',
        'dns-query',
        '//dns-query',
        '/dns-query?q=1',
        '/dns-query#x',
        '/dns query',
      ]) {
        expect(validDirectDnsPath(value), isFalse, reason: value);
      }
      expect(validDirectDnsPath('/custom-dns'), isTrue);
      for (final value in <String>[
        '',
        'dns.example',
        '0.0.0.0',
        '224.0.0.1',
        '::',
        'ff02::1',
        'fe80::1',
        '1.1.1.1 1.1.1.1',
        '::1 0:0:0:0:0:0:0:1',
      ]) {
        expect(validDirectDnsBootstrap(value), isFalse, reason: value);
      }
      expect(validDirectDnsBootstrap('192.0.2.1\n2001:db8::1'), isTrue);
    },
  );

  testWidgets(
    'system hides custom fields; encrypted validation focuses the first error',
    (tester) async {
      final form = GlobalKey<FormState>();
      final editor = GlobalKey<DirectDnsEditorState>();
      final strings = AppStrings(LocalePreference.english);
      var value = const DirectDnsSettings();
      await tester.pumpWidget(
        MaterialApp(
          theme: UsqueTheme.light(),
          home: Scaffold(
            body: SingleChildScrollView(
              child: Form(
                key: form,
                child: DirectDnsEditor(
                  key: editor,
                  value: value,
                  enabled: true,
                  strings: strings,
                  onChanged: (next) => value = next,
                ),
              ),
            ),
          ),
        ),
      );
      expect(find.byType(TextFormField), findsNothing);
      await tester.tap(find.byType(DropdownButtonFormField<DirectDnsMode>));
      await tester.pumpAndSettle();
      await tester.tap(find.text(strings.get('nq_doh')).last);
      await tester.pumpAndSettle();
      expect(find.byType(TextFormField), findsNWidgets(4));
      expect(find.text(strings.get('nq_dns_no_fallback')), findsOneWidget);
      expect(form.currentState!.validate(), isFalse);
      editor.currentState!.focusFirstError();
      await tester.pump();
      final fields = tester
          .widgetList<TextField>(find.byType(TextField))
          .toList();
      expect(fields.first.focusNode!.hasFocus, isTrue);
      await tester.enterText(find.byType(TextFormField).at(0), 'dns.example');
      await tester.enterText(find.byType(TextFormField).at(3), '192.0.2.1');
      expect(form.currentState!.validate(), isTrue);
      expect(value.mode, DirectDnsMode.doh);
      expect(value.dohPath, '/dns-query');
      expect(value.port, 443);
      expect(value.bootstrapIps, <String>['192.0.2.1']);
    },
  );

  testWidgets(
    'old capability preserves saved encrypted settings without edits',
    (tester) async {
      const original = DirectDnsSettings(
        mode: DirectDnsMode.dot,
        serverName: 'dns.example',
        port: 853,
        bootstrapIps: <String>['192.0.2.1'],
      );
      var value = original;
      await tester.pumpWidget(
        MaterialApp(
          theme: UsqueTheme.dark(),
          home: Scaffold(
            body: SingleChildScrollView(
              child: Form(
                child: DirectDnsEditor(
                  value: value,
                  enabled: true,
                  encryptedAvailable: false,
                  strings: AppStrings(LocalePreference.simplifiedChinese),
                  onChanged: (next) => value = next,
                ),
              ),
            ),
          ),
        ),
      );
      expect(
        tester
            .widget<DropdownButtonFormField<DirectDnsMode>>(
              find.byType(DropdownButtonFormField<DirectDnsMode>),
            )
            .onChanged,
        isNotNull,
      );
      expect(
        tester
            .widgetList<TextField>(find.byType(TextField))
            .every((field) => field.readOnly),
        isTrue,
      );
      expect(value, original);
      await tester.tap(find.byType(DropdownButtonFormField<DirectDnsMode>));
      await tester.pumpAndSettle();
      await tester.tap(
        find
            .text(
              AppStrings(
                LocalePreference.simplifiedChinese,
              ).get('nq_system_dns'),
            )
            .last,
      );
      await tester.pumpAndSettle();
      expect(value, const DirectDnsSettings());
      expect(find.byType(TextFormField), findsNothing);
    },
  );

  test(
    'Engine validation remains authoritative and failed save rolls back without downgrade',
    () async {
      SharedPreferences.setMockInitialValues(<String, Object>{});
      final engine = QualityEngineStub();
      final app = AppController(engine);
      await app.initialize();
      const custom = DirectDnsSettings(
        mode: DirectDnsMode.dot,
        serverName: 'dns.example',
        port: 853,
        bootstrapIps: <String>['192.0.2.1'],
      );
      expect(
        await app.saveNetwork(app.activeProfile.copyWith(directDns: custom)),
        isTrue,
      );
      expect(app.activeProfile.directDns, custom);
      engine.failProfileUpsert = true;
      expect(
        await app.saveNetwork(
          app.activeProfile.copyWith(
            directDns: custom.copyWith(serverName: 'different.example'),
          ),
        ),
        isFalse,
      );
      expect(app.activeProfile.directDns, custom);
      expect(app.lastError, isNotNull);
      app.dispose();
    },
  );
}
