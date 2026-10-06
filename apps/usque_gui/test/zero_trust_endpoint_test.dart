import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:usque/core/app_strings.dart';
import 'package:usque/core/l10n/catalogs.dart';
import 'package:usque/core/usque_theme.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/models/network_settings.dart';
import 'package:usque/screens/advanced_settings_screen.dart';
import 'package:usque/services/control_codec.dart';
import 'package:usque/widgets/zero_trust_endpoint_warning.dart';

import 'endpoint_selection_test.dart'
    show EndpointEngine, screen, apply, choose;
import 'ui_workflow_test.dart' show workflowHost;

const ztId = 'aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee';

class ZtEndpointEngine extends EndpointEngine {
  bool supported = true;

  @override
  Future<EngineCapabilities?> getCapabilities() async => EngineCapabilities(
    networkSettingsApplication: true,
    automaticEndpoints: true,
    zeroTrustEndpointEditing: supported,
  );

  @override
  Future<NetworkSettingsState> saveNetworkSettings(
    String operationId,
    String accountId,
    UsqueProfile values,
    List<String> changedFields,
  ) async {
    final sharedSelection = storedProfiles.first.endpointSelection;
    final reply = await super.saveNetworkSettings(
      operationId,
      accountId,
      values,
      changedFields,
    );
    // Simulate the native account-scoped field mask, rather than a full-profile
    // write (the shared fake deliberately preserves registered addresses).
    storedProfiles = [
      for (final profile in storedProfiles)
        if (profile.id == accountId)
          profile.copyWith(
            endpointSelection: EndpointSelection.custom,
            endpointIpv4: changedFields.contains('endpoint.ipv4')
                ? values.endpointIpv4
                : profile.endpointIpv4,
            endpointIpv6: changedFields.contains('endpoint.ipv6')
                ? values.endpointIpv6
                : profile.endpointIpv6,
          )
        else
          profile.copyWith(endpointSelection: sharedSelection),
    ];
    return settingsState = NetworkSettingsState(
      sourceEpoch: reply.sourceEpoch,
      sequence: reply.sequence,
      operationId: operationId,
      storedProfile: storedProfiles.firstWhere((p) => p.id == accountId),
      sharedNetwork: reply.sharedNetwork?.copyWith(
        endpointSelection: sharedSelection,
      ),
      persisted: true,
      status: NetworkSettingsApplyStatus.deferred,
      deferredFields: changedFields,
    );
  }
}

ZtEndpointEngine ztEngine({bool custom = false, bool registered = true}) =>
    ZtEndpointEngine()
      ..storedProfiles = [
        UsqueProfile.defaultProfile(),
        UsqueProfile.defaultProfile().copyWith(
          id: ztId,
          name: 'Work',
          endpointSelection: EndpointSelection.custom,
          endpointIpv4: custom ? '192.0.2.45' : '162.159.197.2',
          endpointIpv6: custom ? '2001:db8::45' : '2606:4700:102::2',
        ),
      ]
      ..storedActiveProfileId = ztId
      ..storedIdentityStatuses = {
        ztId: ProfileIdentityStatus(
          state: ProfileIdentityState.ready,
          provider: IdentityProvider.zeroTrust,
          organization: 'example',
          registeredEndpointIpv4: registered ? '162.159.197.2' : '',
          registeredEndpointIpv6: registered ? '2606:4700:102::2' : '',
        ),
      };

Finder field(String family) =>
    find.widgetWithText(TextFormField, 'Endpoint $family');

Future<void> openWarning(WidgetTester tester) async {
  final edit = find.byKey(const ValueKey('zt-endpoint-edit'));
  await tester.ensureVisible(edit);
  await tester.tap(edit);
  await tester.pumpAndSettle();
}

Future<void> acceptWarning(WidgetTester tester) async {
  final ack = find.byKey(const ValueKey('zt-endpoint-risk-ack'));
  await tester.ensureVisible(ack);
  await tester.tap(ack);
  await tester.pumpAndSettle();
  final button = find.byKey(const ValueKey('zt-endpoint-risk-continue'));
  await tester.ensureVisible(button);
  await tester.tap(button);
  await tester.pumpAndSettle();
}

void main() {
  testWidgets(
    'ZT warning is fullscreen, requires acknowledgement and cancels safely',
    (tester) async {
      final engine = ztEngine();
      final app = await screen(tester, engine);
      await tester.enterText(
        find.widgetWithText(TextFormField, 'Port'),
        '8443',
      );
      expect(
        tester
            .widget<TextField>(
              find.descendant(
                of: field('IPv4'),
                matching: find.byType(TextField),
              ),
            )
            .readOnly,
        isTrue,
      );
      final editAgain = tester
          .widget<OutlinedButton>(
            find.byKey(const ValueKey('zt-endpoint-edit')),
          )
          .onPressed!;
      await openWarning(tester);
      editAgain();
      expect(find.byType(ZeroTrustEndpointWarning), findsOneWidget);
      final dialog = tester.widget<Dialog>(find.byType(Dialog));
      expect(dialog.backgroundColor, UsqueColors.danger);
      for (final key in [
        'zero_trust_endpoint_risk_title',
        'zero_trust_endpoint_risk_body',
        'zero_trust_endpoint_risk_ack',
      ]) {
        final label = find.text(app.strings.get(key));
        final text = tester.widget<Text>(label);
        expect(
          text.style?.color ??
              DefaultTextStyle.of(tester.element(label)).style.color,
          Colors.white,
        );
      }
      expect(
        1.05 / (UsqueColors.danger.computeLuminance() + 0.05),
        greaterThanOrEqualTo(4.5),
      );
      expect(tester.getSize(find.byType(Dialog)), const Size(1280, 1000));
      final proceed = tester.widget<FilledButton>(
        find.byKey(const ValueKey('zt-endpoint-risk-continue')),
      );
      expect(proceed.onPressed, isNull);
      expect(
        Focus.of(tester.element(find.text(app.strings.get('cancel')))).hasFocus,
        isTrue,
      );
      await tester.sendKeyEvent(LogicalKeyboardKey.escape);
      await tester.pumpAndSettle();
      expect(find.byType(ZeroTrustEndpointWarning), findsNothing);
      expect(
        tester
            .widget<TextField>(
              find.descendant(
                of: field('IPv4'),
                matching: find.byType(TextField),
              ),
            )
            .readOnly,
        isTrue,
      );
      expect(find.text('8443'), findsOneWidget);
      await openWarning(tester);
      await tester.binding.handlePopRoute();
      await tester.pumpAndSettle();
      expect(
        tester
            .widget<TextField>(
              find.descendant(
                of: field('IPv4'),
                matching: find.byType(TextField),
              ),
            )
            .readOnly,
        isTrue,
      );
      await openWarning(tester);
      await acceptWarning(tester);
      expect(
        tester
            .widget<TextField>(
              find.descendant(
                of: field('IPv4'),
                matching: find.byType(TextField),
              ),
            )
            .readOnly,
        isFalse,
      );
      await tester.enterText(field('IPv4'), '192.0.2.45');
      await tester.enterText(field('IPv6'), '2001:db8::45');
      await apply(tester, app);
      expect(engine.fields.toSet(), {
        'endpoint.ipv4',
        'endpoint.ipv6',
        'endpoint.port',
      });
      expect(app.activeProfile.endpointIpv4, '192.0.2.45');
      expect(app.sharedNetwork.endpointIpv4, UsqueProfile.defaultEndpointIpv4);
      expect(app.sharedNetwork.endpointSelection, EndpointSelection.automatic);
      await tester.tap(field('IPv6'));
      await tester.pumpAndSettle();
      expect(find.byType(ZeroTrustEndpointWarning), findsNothing);
    },
  );

  testWidgets(
    'failure retains draft and a new page requires confirmation again',
    (tester) async {
      final engine = ztEngine();
      final app = await screen(tester, engine);
      await openWarning(tester);
      await acceptWarning(tester);
      await tester.enterText(field('IPv4'), '192.0.2.46');
      engine.failProfileUpsert = true;
      await apply(tester, app);
      expect(find.text('192.0.2.46'), findsOneWidget);
      expect(app.activeProfile.endpointIpv4, '162.159.197.2');
      engine.failProfileUpsert = false;
      await apply(tester, app);
      expect(app.activeProfile.endpointIpv4, '192.0.2.46');
      await tester.pumpWidget(const SizedBox.shrink());
      await tester.pumpWidget(
        workflowHost(app, home: AdvancedSettingsScreen(controller: app)),
      );
      await tester.pumpAndSettle();
      expect(
        tester
            .widget<TextField>(
              find.descendant(
                of: field('IPv4'),
                matching: find.byType(TextField),
              ),
            )
            .readOnly,
        isTrue,
      );
    },
  );

  testWidgets(
    'account changes invalidate a late or previously accepted confirmation',
    (tester) async {
      final app = await screen(tester, ztEngine());
      await openWarning(tester);
      app.setActiveProfile(UsqueProfile.defaultProfileId);
      app.setActiveProfile(ztId);
      await tester.pumpAndSettle();
      await acceptWarning(tester);
      expect(
        tester
            .widget<TextField>(
              find.descendant(
                of: field('IPv4'),
                matching: find.byType(TextField),
              ),
            )
            .readOnly,
        isTrue,
      );
      await openWarning(tester);
      await acceptWarning(tester);
      app.setActiveProfile(UsqueProfile.defaultProfileId);
      app.setActiveProfile(ztId);
      await tester.pumpAndSettle();
      expect(
        tester
            .widget<TextField>(
              find.descendant(
                of: field('IPv4'),
                matching: find.byType(TextField),
              ),
            )
            .readOnly,
        isTrue,
      );
    },
  );

  testWidgets(
    'reset stages registered addresses without risky-edit confirmation',
    (tester) async {
      final engine = ztEngine(custom: true);
      final app = await screen(tester, engine);
      await choose(tester, app.strings.get('reset_defaults'));
      await tester.tap(
        find.widgetWithText(FilledButton, app.strings.get('reset')),
      );
      await tester.pumpAndSettle();
      expect(find.byType(ZeroTrustEndpointWarning), findsNothing);
      expect(find.text('162.159.197.2'), findsOneWidget);
      expect(app.activeProfile.endpointIpv4, '192.0.2.45');
      await apply(tester, app);
      expect(engine.fields, containsAll(['endpoint.ipv4', 'endpoint.ipv6']));
      expect(app.activeProfile.endpointIpv4, '162.159.197.2');
      expect(app.sharedNetwork.endpointSelection, EndpointSelection.automatic);
    },
  );

  for (final missingRegistration in [false, true]) {
    testWidgets(
      'unavailable ${missingRegistration ? 'registration' : 'capability'} keeps ZT locked',
      (tester) async {
        final engine = ztEngine(custom: true, registered: !missingRegistration)
          ..supported = missingRegistration;
        final app = await screen(tester, engine);
        expect(find.byKey(const ValueKey('zt-endpoint-edit')), findsNothing);
        expect(
          tester
              .widget<TextField>(
                find.descendant(
                  of: field('IPv4'),
                  matching: find.byType(TextField),
                ),
              )
              .readOnly,
          isTrue,
        );
        await tester.enterText(
          find.widgetWithText(TextFormField, 'Port'),
          '8443',
        );
        await apply(tester, app);
        expect(engine.fields, ['endpoint.port']);
        expect(app.activeProfile.endpointIpv4, '192.0.2.45');
      },
    );
  }

  test(
    'capability and registered-address metadata decode with legacy defaults',
    () {
      const codec = ControlCodec();
      Uint8List response(int field, Uint8List payload) => codec.frame(
        (ControlPayloadWriter()
              ..string(1, 'zt')
              ..message(field, payload))
            .takeBytes(),
      );
      final capabilities = codec
          .decodeResponse(
            response(
              15,
              (ControlPayloadWriter()..boolean(45, true)).takeBytes(),
            ),
            'zt',
          )
          .capabilities!;
      expect(capabilities.zeroTrustEndpointEditing, isTrue);
      expect(EngineCapabilities.fromMap({}).zeroTrustEndpointEditing, isFalse);
      expect(
        EngineCapabilities.fromMap({
          'zero_trust_endpoint_editing': 'true',
        }).zeroTrustEndpointEditing,
        isFalse,
      );
      final profile = UsqueProfile.defaultProfile();
      for (final includeAddresses in [false, true]) {
        final status = ControlPayloadWriter()
          ..string(1, profile.id)
          ..enumeration(2, 1)
          ..enumeration(6, 2);
        if (includeAddresses) {
          status
            ..string(8, '162.159.197.2')
            ..string(9, '2606:4700:102::2');
        }
        final catalog = codec.requireProfileCatalog(
          codec.decodeResponse(
            response(
              12,
              (ControlPayloadWriter()
                    ..message(1, codec.encodeProfile(profile))
                    ..string(2, profile.id)
                    ..message(3, status.takeBytes()))
                  .takeBytes(),
            ),
            'zt',
          ),
        );
        expect(
          catalog.identityStatuses[profile.id]!.registeredEndpointIpv4,
          includeAddresses ? '162.159.197.2' : '',
        );
      }
    },
  );

  for (final locale in [
    LocalePreference.english,
    LocalePreference.simplifiedChinese,
    LocalePreference.persian,
  ]) {
    for (final landscape in [false, true]) {
      testWidgets(
        'warning scales and supports keyboard/D-pad in ${locale.name} landscape=$landscape',
        (tester) async {
          tester.view.devicePixelRatio = 1;
          tester.view.physicalSize = landscape
              ? const Size(800, 360)
              : const Size(360, 800);
          addTearDown(tester.view.resetDevicePixelRatio);
          addTearDown(tester.view.resetPhysicalSize);
          if (landscape) {
            debugDefaultTargetPlatformOverride = TargetPlatform.android;
          }
          final strings = AppStrings(locale);
          final semantics = tester.ensureSemantics();
          try {
            await tester.pumpWidget(
              MaterialApp(
                theme: landscape ? UsqueTheme.dark() : UsqueTheme.light(),
                builder: (context, child) => MediaQuery(
                  data: MediaQuery.of(context).copyWith(
                    textScaler: const TextScaler.linear(2),
                    disableAnimations: true,
                  ),
                  child: Directionality(
                    textDirection: locale == LocalePreference.persian
                        ? TextDirection.rtl
                        : TextDirection.ltr,
                    child: child!,
                  ),
                ),
                home: ZeroTrustEndpointWarning(strings: strings),
              ),
            );
            await tester.pumpAndSettle();
            final ack = find.byKey(const ValueKey('zt-endpoint-risk-ack'));
            await tester.ensureVisible(ack);
            tester.widget<CheckboxListTile>(ack).focusNode!.requestFocus();
            await tester.pump();
            await tester.sendKeyEvent(
              landscape ? LogicalKeyboardKey.select : LogicalKeyboardKey.space,
            );
            await tester.pump();
            expect(tester.widget<CheckboxListTile>(ack).value, isTrue);
            expect(
              tester.getSemantics(ack).getSemanticsData().label,
              contains(strings.get('zero_trust_endpoint_risk_ack')),
            );
            final button = find.byKey(
              const ValueKey('zt-endpoint-risk-continue'),
            );
            await tester.ensureVisible(button);
            expect(tester.getSize(button).height, greaterThanOrEqualTo(48));
            expect(tester.takeException(), isNull);
          } finally {
            semantics.dispose();
            debugDefaultTargetPlatformOverride = null;
          }
        },
      );
    }
  }

  test(
    'every catalog translates the risk acknowledgement and reauthentication notice',
    () {
      final keys = kEnCatalog.keys.where(
        (key) =>
            key.startsWith('zero_trust_endpoint_') ||
            key == 'zero_trust_reauth_endpoints',
      );
      expect(AppStrings.debugCatalogsAreComplete, isTrue);
      expect(AppStrings.debugUntranslatedKeys(keys), isEmpty);
    },
  );
}
