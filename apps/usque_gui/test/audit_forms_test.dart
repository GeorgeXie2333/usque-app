import 'dart:async';
import 'dart:typed_data';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:shared_preferences/shared_preferences.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/screens/advanced_settings_screen.dart';
import 'package:usque/screens/geo_direct_settings_screen.dart';
import 'package:usque/screens/per_app_proxy_screen.dart';
import 'package:usque/screens/profiles_screen.dart';
import 'package:usque/screens/proxy_screen.dart';
import 'package:usque/services/control_codec.dart';
import 'package:usque/services/engine_client.dart';
import 'package:usque/state/app_controller.dart';
import 'package:usque/widgets/save_changes_bar.dart';
import 'app_test.dart' show FakeEngineClient;
import 'ui_workflow_test.dart' show workflowHost;

class FormEngine extends FakeEngineClient {
  Completer<void>? pendingSave;
  bool failPerApp = false;
  int saves = 0, perAppSaves = 0, licenseSaves = 0;
  UsqueProfile? savedValues;
  List<String>? savedFields;
  @override
  Future<NetworkSettingsState> saveNetworkSettings(
    String operationId,
    String accountId,
    UsqueProfile values,
    List<String> changedFields,
  ) async {
    saves++;
    savedValues = values;
    savedFields = changedFields;
    await pendingSave?.future;
    return super.saveNetworkSettings(
      operationId,
      accountId,
      values,
      changedFields,
    );
  }

  @override
  Future<PerAppProxySettings> setPerAppProxy(PerAppProxySettings value) async {
    perAppSaves++;
    await pendingSave?.future;
    if (failPerApp) {
      throw const EngineException('CONFIGURATION_INVALID', 'private details');
    }
    return super.setPerAppProxy(value);
  }

  @override
  Future<void> updateLicenseKey(String profileId, String licenseKey) async {
    licenseSaves++;
    await pendingSave?.future;
  }
}

UsqueProfile decode(Uint8List profile) => debugDecodeProfileCatalogFrame(
  const ControlCodec().frame(
    (ControlPayloadWriter()
          ..string(1, 'r')
          ..message(
            12,
            (ControlPayloadWriter()
                  ..message(1, profile)
                  ..string(2, UsqueProfile.defaultProfileId))
                .takeBytes(),
          ))
        .takeBytes(),
  ),
  'r',
).profiles.single;
const listeners = ['127.0.0.1:1080', '127.0.0.2:1081', '[::1]:1082'];
AppController appFor(FormEngine engine) {
  final app = AppController(engine)
    ..localePreference = LocalePreference.english;
  addTearDown(app.dispose);
  return app;
}

void apply(WidgetTester tester) =>
    tester.widget<SaveChangesBar>(find.byType(SaveChangesBar)).onSave!();

void main() {
  setUp(
    () => SharedPreferences.setMockInitialValues({
      'update_checks_enabled': false,
    }),
  );
  test(
    'complete listeners survive protobuf, Android maps and unrelated edits',
    () {
      final original = UsqueProfile.defaultProfile().copyWith(
        proxy: const ProxySettings(
          socksListeners: listeners,
          httpListeners: [],
          authUsername: 'user',
        ),
      );
      final decoded = decode(const ControlCodec().encodeProfile(original));
      expect(decoded.proxy.socksListeners, listeners);
      expect(decoded.proxy.httpListeners, isEmpty);
      final mapped = UsqueProfile.fromMap(decoded.toMap()).copyWith(
        name: 'Renamed',
        proxy: decoded.proxy.copyWith(dnsMode: ProxyDnsMode.system),
      );
      final encoded = decode(const ControlCodec().encodeProfile(mapped));
      expect(encoded.proxy.socksListeners, listeners);
      expect(encoded.proxy.httpListeners, isEmpty);
      expect(encoded.proxy.authUsername, 'user');
      final changed = original.copyWith(
        proxy: original.proxy.copyWith(
          socksListeners: [...listeners.take(2), '[::1]:1083'],
        ),
      );
      expect(networkSettingsChangedFields(original, changed), [
        'proxy.socks5_listeners',
      ]);
      expect(
        const ProxySettings(socksListeners: ['0.0.0.0:1080']).exposesLan,
        isTrue,
      );
    },
  );
  for (final failure in [false, true]) {
    testWidgets(
      'per-app save uses its own result and freezes its draft: failure=$failure',
      (tester) async {
        final engine = FormEngine()
          ..pendingSave = Completer<void>()
          ..failPerApp = failure;
        final app = appFor(engine);
        await tester.pumpWidget(
          workflowHost(
            app,
            home: Scaffold(
              body: Builder(
                builder: (context) => FilledButton(
                  onPressed: () => Navigator.of(context).push<void>(
                    MaterialPageRoute(
                      builder: (_) => PerAppProxyScreen(controller: app),
                    ),
                  ),
                  child: const Text('Open'),
                ),
              ),
            ),
          ),
        );
        await tester.tap(find.text('Open'));
        await tester.pumpAndSettle();
        app.lastError = 'Earlier unrelated error';
        final save = tester
            .widget<FilledButton>(find.widgetWithText(FilledButton, 'Save'))
            .onPressed!;
        save();
        save();
        await tester.pump();
        expect(engine.perAppSaves, 1);
        expect(
          tester
              .widget<CheckboxListTile>(find.byType(CheckboxListTile).first)
              .onChanged,
          isNull,
        );
        engine.pendingSave!.complete();
        await tester.pumpAndSettle();
        expect(
          find.byType(PerAppProxyScreen),
          failure ? findsOneWidget : findsNothing,
        );
        if (failure) {
          expect(find.text('private details'), findsNothing);
          expect(
            find.text(app.strings.get('diag_fix_review_configuration')),
            findsOneWidget,
          );
          expect(
            tester
                .widget<FilledButton>(find.widgetWithText(FilledButton, 'Save'))
                .onPressed,
            isNotNull,
          );
        }
        await tester.pumpWidget(const SizedBox());
      },
    );
  }
  testWidgets('Geo selection cannot change during save', (tester) async {
    final engine = FormEngine()..pendingSave = Completer<void>();
    final app = appFor(engine);
    app.sharedNetwork = app.sharedNetwork.copyWith(geoDirectCountries: ['US']);
    await tester.pumpWidget(
      workflowHost(app, home: GeoDirectSettingsScreen(controller: app)),
    );
    await tester.pumpAndSettle();
    await tester.enterText(find.byType(TextField), 'United States');
    await tester.pumpAndSettle();
    await tester.tap(find.text('Save'));
    await tester.pump();
    expect(tester.widget<Switch>(find.byType(Switch)).onChanged, isNull);
    expect(tester.widget<Switch>(find.byType(Switch)).value, isTrue);
    expect(engine.savedValues?.geoDirectCountries, ['US']);
    engine.pendingSave!.complete();
    await tester.pumpAndSettle();
    await tester.pumpWidget(const SizedBox());
  });
  testWidgets(
    'unsupported edge DNS retains the draft without assertion or save',
    (tester) async {
      final engine = FormEngine();
      final app = appFor(engine);
      app.sharedNetwork = app.sharedNetwork.copyWith(
        dataPlane: DataPlaneMode.l4Proxy,
      );
      Widget page() => workflowHost(app, home: ProxyScreen(controller: app));
      await tester.pumpWidget(page());
      final menu = find.byKey(const ValueKey('proxy-dns-mode'));
      tester.widget<DropdownButtonFormField<ProxyDnsMode>>(menu).onChanged!(
        ProxyDnsMode.edgeResolved,
      );
      final port = find
          .byWidgetPredicate(
            (w) => w is TextField && w.decoration?.labelText == 'Port',
          )
          .first;
      await tester.enterText(port, '9090');
      app.sharedNetwork = app.sharedNetwork.copyWith(
        dataPlane: DataPlaneMode.connectIp,
      );
      await tester.pumpWidget(page());
      expect(tester.takeException(), isNull);
      expect(tester.widget<TextField>(port).controller!.text, '9090');
      expect(find.text(app.strings.get('l4_edge_requires_l4')), findsOneWidget);
      apply(tester);
      await tester.pump();
      expect(engine.saves, 0);
      await tester.pumpWidget(const SizedBox());
    },
  );
  testWidgets('custom listeners preserve every port during DNS edits', (
    tester,
  ) async {
    final engine = FormEngine();
    final app = appFor(engine);
    app.sharedNetwork = app.sharedNetwork.copyWith(
      proxy: const ProxySettings(socksListeners: listeners),
    );
    await tester.pumpWidget(
      workflowHost(app, home: ProxyScreen(controller: app)),
    );
    await tester.pumpAndSettle();
    expect(
      tester
          .widget<TextFormField>(
            find.byKey(const ValueKey('socks-listener-addresses')),
          )
          .controller!
          .text,
      listeners.join('\n'),
    );
    tester
        .widget<DropdownButtonFormField<ProxyDnsMode>>(
          find.byKey(const ValueKey('proxy-dns-mode')),
        )
        .onChanged!(ProxyDnsMode.system);
    await tester.pump();
    apply(tester);
    await tester.pumpAndSettle();
    expect(engine.savedValues?.proxy.socksListeners, listeners);
    expect(engine.savedFields, ['proxy.dns_mode']);
    final editor = find.byKey(const ValueKey('socks-listener-addresses'));
    const changed = ['127.0.0.1:2080', '127.0.0.2:2081', '[::1]:2082'];
    await tester.enterText(editor, changed.join('\n'));
    await tester.pump();
    apply(tester);
    await tester.pumpAndSettle();
    expect(engine.savedValues?.proxy.socksListeners, changed);
    expect(engine.savedFields, ['proxy.socks5_listeners']);
    final saves = engine.saves;
    await tester.enterText(editor, '127.0.0.1:0');
    await tester.pump();
    apply(tester);
    await tester.pumpAndSettle();
    expect(engine.saves, saves);
    expect(find.text(app.strings.get('invalid_address')), findsOneWidget);
    await tester.pumpWidget(const SizedBox());
  });
  testWidgets(
    'reset applies listeners and DNS mode while retaining credentials',
    (tester) async {
      final engine = FormEngine();
      final app = appFor(engine);
      app.sharedNetwork = app.sharedNetwork.copyWith(
        dataPlane: DataPlaneMode.l4Proxy,
        dnsMode: DnsMode.system,
        proxy: const ProxySettings(
          socksPort: 9090,
          dnsMode: ProxyDnsMode.edgeResolved,
          systemProxy: true,
          authUsername: 'existing-user',
        ),
      );
      await tester.pumpWidget(
        workflowHost(app, home: AdvancedSettingsScreen(controller: app)),
      );
      await tester.pumpAndSettle();
      await tester.tap(find.text(app.strings.get('reset_defaults')));
      await tester.pumpAndSettle();
      await tester.tap(find.text(app.strings.get('reset')));
      await tester.pumpAndSettle();
      expect(engine.saves, 0);
      apply(tester);
      await tester.pumpAndSettle();
      expect(engine.saves, 1);
      expect(
        engine.savedValues?.proxy.socksListeners,
        const ProxySettings().socksListeners,
      );
      expect(engine.savedValues?.proxy.dnsMode, ProxyDnsMode.remote);
      expect(engine.savedValues?.dnsMode, DnsMode.tunnel);
      expect(engine.savedValues?.proxy.authUsername, 'existing-user');
      expect(
        engine.savedFields,
        containsAll([
          'proxy.socks5_listeners',
          'proxy.dns_mode',
          'proxy.system_proxy',
          'dns_mode',
        ]),
      );
      await tester.pumpWidget(const SizedBox());
    },
  );
  testWidgets('repeated license keyboard submission is single flight', (
    tester,
  ) async {
    final engine = FormEngine()..pendingSave = Completer<void>();
    final app = appFor(engine);
    await app.initialize();
    await tester.pumpWidget(
      workflowHost(app, home: ProfilesScreen(controller: app)),
    );
    await tester.pumpAndSettle();
    await tester.tap(find.byTooltip(app.strings.get('identity_and_license')));
    await tester.pumpAndSettle();
    final input = find.byType(TextField);
    await tester.enterText(input, 'example-license');
    final submit = tester.widget<TextField>(input).onSubmitted!;
    submit('example-license');
    submit('example-license');
    await tester.pump();
    expect(engine.licenseSaves, 1);
    expect(tester.widget<TextField>(input).enabled, isFalse);
    expect(tester.widget<TextField>(input).onSubmitted, isNull);
    engine.pendingSave!.complete();
    await tester.pumpAndSettle();
    await tester.pumpWidget(const SizedBox());
  });
}
