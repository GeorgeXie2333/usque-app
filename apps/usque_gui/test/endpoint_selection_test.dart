import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:shared_preferences/shared_preferences.dart';
import 'package:usque/core/app_strings.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/screens/advanced_settings_screen.dart';
import 'package:usque/services/control_codec.dart';
import 'package:usque/services/engine_client.dart';
import 'package:usque/state/app_controller.dart';

import 'app_test.dart' show FakeEngineClient;
import 'ui_workflow_test.dart' show workflowHost;

class EndpointEngine extends FakeEngineClient {
  bool automaticSupported = true;
  UsqueProfile? submitted;
  List<String> fields = [];

  @override
  Future<EngineCapabilities?> getCapabilities() async => EngineCapabilities(
    networkSettingsApplication: true,
    automaticEndpoints: automaticSupported,
  );

  @override
  Future<NetworkSettingsState> saveNetworkSettings(
    String operationId,
    String accountId,
    UsqueProfile values,
    List<String> changedFields,
  ) {
    submitted = values;
    fields = changedFields;
    return super.saveNetworkSettings(
      operationId,
      accountId,
      values,
      changedFields,
    );
  }
}

class DelayedEndpointEngine extends EndpointEngine {
  final requested = Completer<void>();
  final reply = Completer<EngineCapabilities?>();

  @override
  Future<EngineCapabilities?> getCapabilities() {
    if (!requested.isCompleted) requested.complete();
    return reply.future;
  }
}

const codec = ControlCodec();

Uint8List response(int field, Uint8List payload) => codec.frame(
  (ControlPayloadWriter()
        ..string(1, 'endpoint')
        ..message(field, payload))
      .takeBytes(),
);

UsqueProfile decodeProfile(Uint8List bytes) => codec
    .requireProfileCatalog(
      codec.decodeResponse(
        response(
          12,
          (ControlPayloadWriter()
                ..message(1, bytes)
                ..string(2, UsqueProfile.defaultProfileId))
              .takeBytes(),
        ),
        'endpoint',
      ),
    )
    .profiles
    .single;

Future<AppController> screen(
  WidgetTester tester,
  EndpointEngine engine, {
  LocalePreference locale = LocalePreference.english,
  double scale = 1,
  Size size = const Size(1280, 1000),
}) async {
  tester.view.devicePixelRatio = 1;
  tester.view.physicalSize = size;
  addTearDown(tester.view.resetDevicePixelRatio);
  addTearDown(tester.view.resetPhysicalSize);
  SharedPreferences.setMockInitialValues({'onboarding_complete': true});
  engine.legacyProfilesImported = true;
  final app = AppController(engine);
  await app.initialize();
  app.localePreference = locale;
  addTearDown(app.dispose);
  await tester.pumpWidget(
    workflowHost(
      app,
      dark: locale == LocalePreference.simplifiedChinese,
      scale: scale,
      home: AdvancedSettingsScreen(controller: app),
    ),
  );
  await tester.pumpAndSettle();
  return app;
}

Future<void> choose(WidgetTester tester, String text) async {
  final finder = find.text(text);
  await tester.ensureVisible(finder);
  await tester.pumpAndSettle();
  await tester.tap(finder);
  await tester.pumpAndSettle();
}

Future<void> apply(WidgetTester tester, AppController app) async {
  await tester.pumpAndSettle();
  await tester.tap(
    find.widgetWithText(FilledButton, app.strings.get('save_changes')),
  );
  await tester.pumpAndSettle();
}

void main() {
  test('new and reset profiles use Automatic; legacy JSON keeps Custom', () {
    final profile = UsqueProfile.defaultProfile();
    expect(profile.endpointSelection, EndpointSelection.automatic);
    for (final selection in EndpointSelection.values) {
      final selected = profile.copyWith(endpointSelection: selection);
      expect(
        UsqueProfile.fromMap(selected.toMap()).endpointSelection,
        selection,
      );
      expect(
        decodeProfile(codec.encodeProfile(selected)).endpointSelection,
        selection,
      );
      expect(
        selected.resetAdvancedDefaults().endpointSelection,
        EndpointSelection.automatic,
      );
    }
    final legacy = profile.toMap()..remove('endpoint_selection');
    expect(
      UsqueProfile.fromMap(legacy).endpointSelection,
      EndpointSelection.custom,
    );
    for (final invalid in ['Auto', 'unknown', 0, null]) {
      expect(
        () => UsqueProfile.fromMap({...legacy, 'endpoint_selection': invalid}),
        throwsFormatException,
      );
    }
    expect(
      networkSettingsChangedFields(
        profile,
        profile.copyWith(endpointSelection: EndpointSelection.custom),
      ),
      ['endpoint.selection'],
    );
  });

  test('absent and unspecified endpoint wire modes keep Custom', () {
    for (final value in [null, 0, 1, 2, 3]) {
      final endpoint = ControlPayloadWriter()
        ..string(1, UsqueProfile.defaultEndpointIpv4)
        ..string(2, UsqueProfile.defaultEndpointIpv6)
        ..unsigned(3, 443)
        ..string(4, UsqueProfile.defaultSni);
      if (value != null) endpoint.enumeration(5, value);
      final bytes =
          (ControlPayloadWriter()
                ..string(1, UsqueProfile.defaultProfileId)
                ..string(2, 'Legacy')
                ..message(5, endpoint.takeBytes()))
              .takeBytes();
      if (value == 3) {
        expect(() => decodeProfile(bytes), throwsA(isA<EngineException>()));
      } else {
        expect(
          decodeProfile(bytes).endpointSelection,
          value == 1 ? EndpointSelection.automatic : EndpointSelection.custom,
        );
      }
    }
  });

  test('capability 42 is append-only and absent engines lack Automatic', () {
    expect(
      EngineCapabilities.fromMap({
        'automatic_endpoints': true,
      }).automaticEndpoints,
      isTrue,
    );
    expect(EngineCapabilities.fromMap({}).automaticEndpoints, isFalse);
    expect(
      codec
          .decodeResponse(response(15, Uint8List(0)), 'endpoint')
          .capabilities!
          .automaticEndpoints,
      isFalse,
    );
    final payload = (ControlPayloadWriter()..boolean(42, true)).takeBytes();
    expect(payload, [0xd0, 0x02, 0x01]);
    expect(
      codec
          .decodeResponse(response(15, payload), 'endpoint')
          .capabilities!
          .automaticEndpoints,
      isTrue,
    );
    expect(
      const EngineCapabilities(automaticEndpoints: true),
      isNot(const EngineCapabilities()),
    );
  });

  testWidgets('Automatic saves confirmed addresses and retains custom drafts', (
    tester,
  ) async {
    final engine = EndpointEngine()
      ..storedProfiles = [
        UsqueProfile.defaultProfile().copyWith(
          endpointSelection: EndpointSelection.custom,
        ),
      ];
    final app = await screen(tester, engine);
    await tester.enterText(
      find.widgetWithText(TextFormField, 'Endpoint IPv4'),
      'invalid',
    );
    await choose(tester, 'Automatic');
    expect(find.widgetWithText(TextFormField, 'Endpoint IPv4'), findsNothing);
    await tester.enterText(find.widgetWithText(TextFormField, 'Port'), '8443');
    await tester.enterText(
      find.widgetWithText(TextFormField, 'SNI'),
      'custom.example',
    );
    await apply(tester, app);
    expect(engine.submitted!.endpointSelection, EndpointSelection.automatic);
    expect(engine.submitted!.endpointIpv4, UsqueProfile.defaultEndpointIpv4);
    expect(
      engine.fields,
      containsAll(['endpoint.selection', 'endpoint.port', 'endpoint.sni']),
    );
    expect(engine.fields, isNot(contains('endpoint.ipv4')));
    expect(engine.fields, isNot(contains('endpoint.ipv6')));
    await choose(tester, 'Custom');
    expect(
      tester
          .widget<TextField>(
            find.descendant(
              of: find.widgetWithText(TextFormField, 'Endpoint IPv4'),
              matching: find.byType(TextField),
            ),
          )
          .controller!
          .text,
      'invalid',
    );
    await apply(tester, app);
    expect(find.text(app.strings.get('invalid_address')), findsOneWidget);
    await tester.enterText(
      find.widgetWithText(TextFormField, 'Endpoint IPv4'),
      '192.0.2.45',
    );
    await apply(tester, app);
    expect(engine.submitted!.endpointSelection, EndpointSelection.custom);
    expect(engine.submitted!.endpointIpv4, '192.0.2.45');
  });

  testWidgets('valid custom drafts remain pending after an Automatic save', (
    tester,
  ) async {
    final engine = EndpointEngine()
      ..storedProfiles = [
        UsqueProfile.defaultProfile().copyWith(
          endpointSelection: EndpointSelection.custom,
        ),
      ];
    final app = await screen(tester, engine);
    await tester.enterText(
      find.widgetWithText(TextFormField, 'Endpoint IPv4'),
      '192.0.2.45',
    );
    await choose(tester, 'Automatic');
    await apply(tester, app);
    expect(engine.fields, ['endpoint.selection']);
    expect(app.activeProfile.endpointIpv4, UsqueProfile.defaultEndpointIpv4);
    expect(
      tester.widget<PopScope<Object?>>(find.byType(PopScope<Object?>)).canPop,
      isTrue,
    );
    await choose(tester, 'Custom');
    await apply(tester, app);
    expect(engine.fields, containsAll(['endpoint.selection', 'endpoint.ipv4']));
    expect(app.activeProfile.endpointIpv4, '192.0.2.45');
  });

  testWidgets('failed save retains the endpoint mode draft', (tester) async {
    final engine = EndpointEngine()..failProfileUpsert = true;
    final app = await screen(tester, engine);
    await choose(tester, 'Custom');
    await apply(tester, app);
    expect(app.activeProfile.endpointSelection, EndpointSelection.automatic);
    expect(
      tester
          .widget<SegmentedButton<EndpointSelection>>(
            find.byKey(const ValueKey('endpoint-selection')),
          )
          .selected,
      {EndpointSelection.custom},
    );
    expect(
      tester.widget<PopScope<Object?>>(find.byType(PopScope<Object?>)).canPop,
      isFalse,
    );
  });

  testWidgets('reset stages Automatic until Apply', (tester) async {
    final engine = EndpointEngine()
      ..storedProfiles = [
        UsqueProfile.defaultProfile().copyWith(
          endpointSelection: EndpointSelection.custom,
        ),
      ];
    final app = await screen(tester, engine);
    await choose(tester, app.strings.get('reset_defaults'));
    await choose(tester, app.strings.get('reset'));
    expect(app.activeProfile.endpointSelection, EndpointSelection.custom);
    expect(
      tester
          .widget<SegmentedButton<EndpointSelection>>(
            find.byKey(const ValueKey('endpoint-selection')),
          )
          .selected,
      {EndpointSelection.automatic},
    );
    await apply(tester, app);
    expect(app.activeProfile.endpointSelection, EndpointSelection.automatic);
  });

  testWidgets('old engines keep Custom available and reject Automatic saves', (
    tester,
  ) async {
    final engine = EndpointEngine()..automaticSupported = false;
    final app = await screen(tester, engine);
    final control = tester.widget<SegmentedButton<EndpointSelection>>(
      find.byKey(const ValueKey('endpoint-selection')),
    );
    expect(control.segments.first.enabled, isFalse);
    expect(find.text(app.strings.get('endpoint_unsupported')), findsOneWidget);
    expect(
      await app.saveNetwork(app.activeProfile.copyWith(mtu: 1400)),
      isFalse,
    );
    expect(engine.submitted, isNull);
    await choose(tester, 'Custom');
    await apply(tester, app);
    expect(engine.submitted!.endpointSelection, EndpointSelection.custom);
  });

  testWidgets(
    'late endpoint capabilities refresh controls without losing drafts',
    (tester) async {
      tester.view.devicePixelRatio = 1;
      tester.view.physicalSize = const Size(1280, 1000);
      addTearDown(tester.view.resetDevicePixelRatio);
      addTearDown(tester.view.resetPhysicalSize);
      SharedPreferences.setMockInitialValues({
        'onboarding_complete': true,
        'update_checks_enabled': false,
      });
      final engine = DelayedEndpointEngine()
        ..legacyProfilesImported = true
        ..storedProfiles = [
          UsqueProfile.defaultProfile().copyWith(
            endpointSelection: EndpointSelection.custom,
          ),
        ];
      final app = AppController(engine);
      addTearDown(app.dispose);
      final initializing = app.initialize();
      await tester.pump();
      expect(engine.requested.isCompleted, isTrue);
      expect(app.initialized, isTrue);
      app.localePreference = LocalePreference.english;
      await tester.pumpWidget(
        workflowHost(app, home: AdvancedSettingsScreen(controller: app)),
      );
      await tester.pumpAndSettle();
      final selector = find.byKey(const ValueKey('endpoint-selection'));
      expect(
        tester
            .widget<SegmentedButton<EndpointSelection>>(selector)
            .segments
            .first
            .enabled,
        isFalse,
      );
      await tester.enterText(
        find.widgetWithText(TextFormField, 'Endpoint IPv4'),
        '192.0.2.45',
      );
      engine.reply.complete(
        const EngineCapabilities(
          networkSettingsApplication: true,
          automaticEndpoints: true,
        ),
      );
      await tester.pump();
      await initializing;
      await tester.pumpAndSettle();
      expect(
        tester
            .widget<SegmentedButton<EndpointSelection>>(selector)
            .segments
            .first
            .enabled,
        isTrue,
      );
      expect(find.text(app.strings.get('endpoint_unsupported')), findsNothing);
      await apply(tester, app);
      expect(engine.submitted!.endpointIpv4, '192.0.2.45');
      expect(engine.submitted!.endpointSelection, EndpointSelection.custom);
    },
  );

  testWidgets('Zero Trust hides selection and preserves the consumer mode', (
    tester,
  ) async {
    final consumer = UsqueProfile.defaultProfile().copyWith(
      endpointSelection: EndpointSelection.custom,
    );
    final managed = consumer.copyWith(
      id: 'aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee',
      name: 'Work',
      endpointIpv4: '162.159.197.2',
      endpointIpv6: '2606:4700:102::2',
    );
    final engine = EndpointEngine()
      ..storedProfiles = [consumer, managed]
      ..storedActiveProfileId = managed.id
      ..storedIdentityStatuses = {
        managed.id: const ProfileIdentityStatus(
          state: ProfileIdentityState.ready,
          provider: IdentityProvider.zeroTrust,
          licenseState: LicenseState.notApplicable,
        ),
      };
    final app = await screen(tester, engine);
    expect(find.byKey(const ValueKey('endpoint-selection')), findsNothing);
    await tester.enterText(find.widgetWithText(TextFormField, 'Port'), '8443');
    await apply(tester, app);
    expect(engine.submitted, isNotNull);
    expect(engine.fields, ['endpoint.port']);
    expect(app.sharedNetwork.endpointSelection, EndpointSelection.custom);
    expect(app.activeProfile.endpointIpv4, managed.endpointIpv4);
    expect(app.activeProfile.endpointPort, 8443);
  });

  for (final locale in [
    LocalePreference.english,
    LocalePreference.simplifiedChinese,
  ]) {
    testWidgets(
      'endpoint choice fits 200 percent and supports keyboard in ${locale.name}',
      (tester) async {
        final app = await screen(
          tester,
          EndpointEngine(),
          locale: locale,
          scale: 2,
          size: const Size(360, 800),
        );
        final selector = find.byKey(const ValueKey('endpoint-selection'));
        await tester.scrollUntilVisible(
          selector,
          200,
          scrollable: find.byType(Scrollable).first,
        );
        await tester.pumpAndSettle();
        final button = find.widgetWithText(
          TextButton,
          app.strings.get('endpoint_custom'),
        );
        Focus.of(
          tester.element(find.text(app.strings.get('endpoint_custom'))),
        ).requestFocus();
        await tester.pump();
        await tester.sendKeyEvent(LogicalKeyboardKey.enter);
        await tester.pumpAndSettle();
        expect(
          tester.widget<SegmentedButton<EndpointSelection>>(selector).selected,
          {EndpointSelection.custom},
        );
        expect(tester.getSize(button).height, greaterThanOrEqualTo(48));
        expect(tester.takeException(), isNull);
      },
    );
  }

  test('all catalogs translate endpoint controls and help', () {
    expect(AppStrings.debugCatalogsAreComplete, isTrue);
    expect(
      AppStrings.debugUntranslatedKeys([
        'endpoint_selection',
        'endpoint_automatic',
        'endpoint_custom',
        'endpoint_automatic_help',
        'endpoint_custom_help',
        'endpoint_unsupported',
      ]),
      isEmpty,
    );
  });
}
