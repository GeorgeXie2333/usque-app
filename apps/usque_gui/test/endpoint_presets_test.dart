import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:usque/core/app_strings.dart';
import 'package:usque/core/l10n/endpoint_presets.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/models/network_settings.dart';
import 'package:usque/services/control_codec.dart';
import 'package:usque/widgets/zero_trust_endpoint_warning.dart';

import 'endpoint_selection_test.dart'
    show EndpointEngine, screen, choose, apply, decodeProfile, response;

class PresetEngine extends EndpointEngine {
  PresetEngine() {
    storedIdentityStatuses = {
      UsqueProfile.defaultProfileId: const ProfileIdentityStatus(
        state: ProfileIdentityState.ready,
        licenseState: LicenseState.free,
      ),
    };
  }

  bool presetsSupported = true;

  @override
  Future<EngineCapabilities?> getCapabilities() async => EngineCapabilities(
    tlsFingerprints: TlsFingerprint.values,
    networkSettingsApplication: true,
    automaticEndpoints: true,
    endpointPresets: presetsSupported,
    zeroTrustEndpointEditing: true,
  );
}

void main() {
  test('preset registry follows verified account tier for all nine hosts', () {
    const tiers = [
      (IdentityProvider.consumer, LicenseState.free, 'free'),
      (IdentityProvider.consumer, LicenseState.warpPlus, 'plus'),
      (IdentityProvider.zeroTrust, LicenseState.notApplicable, 'zt'),
    ];
    for (final region in EndpointPresetRegion.values) {
      for (final (provider, license, tier) in tiers) {
        expect(
          region.hostname(
            ProfileIdentityStatus(
              state: ProfileIdentityState.ready,
              provider: provider,
              licenseState: license,
            ),
          ),
          '${region.name}.v6.$tier.orangesky.dev',
        );
      }
      expect(
        region.hostname(
          const ProfileIdentityStatus(state: ProfileIdentityState.missing),
        ),
        isNull,
      );
      expect(
        region.hostname(
          const ProfileIdentityStatus(state: ProfileIdentityState.ready),
        ),
        isNull,
      );
    }
  });

  test(
    'preset JSON and appended protobuf round-trip and reset clears source',
    () {
      final profile = UsqueProfile.defaultProfile().copyWith(
        endpointSelection: EndpointSelection.custom,
        endpointPresetRegion: EndpointPresetRegion.de,
        ipPolicy: IpPolicy.ipv6Only,
      );
      expect(
        UsqueProfile.fromMap(profile.toMap()).endpointPresetRegion,
        EndpointPresetRegion.de,
      );
      expect(
        decodeProfile(
          const ControlCodec().encodeProfile(profile),
        ).endpointPresetRegion,
        EndpointPresetRegion.de,
      );
      expect(profile.resetAdvancedDefaults().endpointPresetRegion, isNull);
      expect(
        profile.copyWith(clearEndpointPresetRegion: true).endpointPresetRegion,
        isNull,
      );
      expect(
        networkSettingsChangedFields(
          profile,
          profile.copyWith(endpointPresetRegion: EndpointPresetRegion.fi),
        ),
        ['endpoint.preset_region'],
      );
      final legacy = profile.toMap()..remove('endpoint_preset_region');
      expect(UsqueProfile.fromMap(legacy).endpointPresetRegion, isNull);
      expect(
        () => UsqueProfile.fromMap({
          ...legacy,
          'endpoint_preset_region': 'unknown',
        }),
        throwsFormatException,
      );
    },
  );

  test(
    'old engines default capability false; append-only wire field enables it',
    () {
      expect(const EngineCapabilities().endpointPresets, isFalse);
      expect(
        EngineCapabilities.fromMap({'endpoint_presets': true}).endpointPresets,
        isTrue,
      );
      const codec = ControlCodec();
      final payload = (ControlPayloadWriter()..boolean(49, true)).takeBytes();
      expect(
        codec
            .decodeResponse(response(15, payload), 'endpoint')
            .capabilities!
            .endpointPresets,
        isTrue,
      );
      expect(
        EngineSnapshot.fromMap({
          'phase': 'connected',
          'endpoint_source': 'manual_ipv4_fallback',
        }).endpointSource,
        'manual_ipv4_fallback',
      );
    },
  );

  testWidgets(
    'preset policy cancellation is atomic; apply saves country and preserves manual drafts',
    (tester) async {
      final engine = PresetEngine();
      final app = await screen(tester, engine);
      await choose(tester, 'Custom');
      await tester.enterText(
        find.widgetWithText(TextFormField, 'Endpoint IPv6'),
        'invalid',
      );
      await choose(tester, app.strings.get('endpoint_presets'));
      expect(find.byType(RadioListTile<IpPolicy>), findsNWidgets(2));
      await choose(tester, app.strings.get('cancel'));
      expect(
        tester
            .widget<SegmentedButton<bool>>(
              find.byKey(const ValueKey('endpoint-custom-source')),
            )
            .selected,
        {false},
      );
      expect(engine.submitted, isNull);
      await choose(tester, app.strings.get('endpoint_presets'));
      await choose(tester, app.strings.get('continue'));
      expect(
        find.byKey(const ValueKey('endpoint-preset-region')),
        findsOneWidget,
      );
      expect(find.textContaining('orangesky.dev'), findsNothing);
      expect(find.textContaining('Free ·'), findsNothing);
      expect(find.text('Endpoint IPv6'), findsNothing);
      expect(app.activeProfile.endpointPresetRegion, isNull);
      await apply(tester, app);
      expect(engine.submitted!.endpointPresetRegion, EndpointPresetRegion.us);
      expect(engine.submitted!.ipPolicy, IpPolicy.ipv6Only);
      expect(engine.submitted!.endpointIpv6, UsqueProfile.defaultEndpointIpv6);
      expect(
        engine.fields,
        containsAll([
          'endpoint.selection',
          'endpoint.preset_region',
          'ip_policy',
        ]),
      );
      expect(engine.fields, isNot(contains('endpoint.ipv6')));
      await choose(tester, app.strings.get('endpoint_manual'));
      final text = tester.widget<TextField>(
        find.descendant(
          of: find.widgetWithText(TextFormField, 'Endpoint IPv6'),
          matching: find.byType(TextField),
        ),
      );
      expect(text.controller!.text, 'invalid');
    },
  );

  testWidgets(
    'compatible policy skips prompt, save failure keeps preset draft, Automatic clears it',
    (tester) async {
      final engine = PresetEngine()
        ..storedProfiles = [
          UsqueProfile.defaultProfile().copyWith(ipPolicy: IpPolicy.preferIpv6),
        ];
      final app = await screen(tester, engine);
      await choose(tester, 'Custom');
      await choose(tester, app.strings.get('endpoint_presets'));
      expect(find.byType(RadioListTile<IpPolicy>), findsNothing);
      engine.failProfileUpsert = true;
      await apply(tester, app);
      expect(app.activeProfile.endpointPresetRegion, isNull);
      expect(
        find.byKey(const ValueKey('endpoint-preset-region')),
        findsOneWidget,
      );
      engine.failProfileUpsert = false;
      await apply(tester, app);
      expect(app.activeProfile.endpointPresetRegion, EndpointPresetRegion.us);
      await choose(tester, 'Automatic');
      await apply(tester, app);
      expect(app.activeProfile.endpointPresetRegion, isNull);
      expect(engine.fields, contains('endpoint.preset_region'));
    },
  );

  testWidgets(
    'unverified accounts and older engines disable built-in selection',
    (tester) async {
      final engine = PresetEngine()..storedIdentityStatuses = {};
      final app = await screen(tester, engine);
      await choose(tester, 'Custom');
      expect(
        tester
            .widget<SegmentedButton<bool>>(
              find.byKey(const ValueKey('endpoint-custom-source')),
            )
            .segments
            .last
            .enabled,
        isFalse,
      );
      expect(
        find.text(app.strings.get('endpoint_preset_verify')),
        findsOneWidget,
      );
      engine.presetsSupported = false;
      expect(
        await app.saveNetwork(
          app.activeProfile.copyWith(
            endpointPresetRegion: EndpointPresetRegion.us,
          ),
        ),
        isFalse,
      );
      expect(engine.submitted, isNull);
    },
  );

  testWidgets('a saved preset never labels an unverified account as Free', (
    tester,
  ) async {
    final engine = PresetEngine()
      ..storedIdentityStatuses = {}
      ..storedProfiles = [
        UsqueProfile.defaultProfile().copyWith(
          endpointSelection: EndpointSelection.custom,
          endpointPresetRegion: EndpointPresetRegion.us,
          ipPolicy: IpPolicy.ipv6Only,
        ),
      ];
    final app = await screen(tester, engine);
    expect(
      find.text(app.strings.get('endpoint_preset_verify')),
      findsOneWidget,
    );
    expect(find.text('us.v6.free.orangesky.dev'), findsNothing);
    expect(find.textContaining('Free ·'), findsNothing);
  });

  testWidgets('Zero Trust uses presets without manual risk confirmation', (
    tester,
  ) async {
    final engine = PresetEngine();
    final profile = UsqueProfile.defaultProfile().copyWith(
      endpointSelection: EndpointSelection.custom,
      endpointIpv4: '192.0.2.45',
      endpointIpv6: '2001:db8::45',
    );
    engine.storedProfiles = [profile];
    engine.storedIdentityStatuses = {
      profile.id: const ProfileIdentityStatus(
        state: ProfileIdentityState.ready,
        provider: IdentityProvider.zeroTrust,
        licenseState: LicenseState.notApplicable,
        registeredEndpointIpv4: '162.159.197.2',
        registeredEndpointIpv6: '2606:4700:102::2',
      ),
    };
    final app = await screen(tester, engine);
    expect(app.hasCustomZeroTrustEndpointRisk, isTrue);
    await choose(tester, app.strings.get('endpoint_presets'));
    await choose(tester, app.strings.get('continue'));
    expect(find.byType(ZeroTrustEndpointWarning), findsNothing);
    expect(
      find.byKey(const ValueKey('endpoint-preset-region')),
      findsOneWidget,
    );
    expect(find.textContaining('orangesky.dev'), findsNothing);
    expect(find.textContaining('Zero Trust ·'), findsNothing);
    await apply(tester, app);
    expect(engine.fields, isNot(contains('endpoint.selection')));
    expect(app.hasCustomZeroTrustEndpointRisk, isFalse);
    app.networkSettings.accept(
      NetworkSettingsState(
        sourceEpoch: engine.settingsEpoch,
        sequence: ++engine.settingsSequence,
        appliedProfile: app.activeProfile,
      ),
    );
    app.snapshot = const EngineSnapshot(
      phase: ConnectionPhase.connected,
      endpointSource: 'manual_ipv4_fallback',
    );
    expect(app.hasCustomZeroTrustEndpointRisk, isTrue);
    app.snapshot = const EngineSnapshot(
      phase: ConnectionPhase.connected,
      endpointSource: 'preset_ipv6',
    );
    expect(app.hasCustomZeroTrustEndpointRisk, isFalse);
  });

  for (final locale in [
    LocalePreference.english,
    LocalePreference.simplifiedChinese,
  ]) {
    testWidgets(
      'presets fit 200 percent with keyboard and TV selection in ${locale.name}',
      (tester) async {
        debugDefaultTargetPlatformOverride = TargetPlatform.android;
        try {
          final app = await screen(
            tester,
            PresetEngine(),
            locale: locale,
            scale: 2,
            size: const Size(360, 800),
          );
          await tester.scrollUntilVisible(
            find.byKey(const ValueKey('endpoint-selection')),
            200,
            scrollable: find.byType(Scrollable).first,
          );
          await choose(tester, app.strings.get('endpoint_custom'));
          final label = find.text(app.strings.get('endpoint_presets'));
          await tester.ensureVisible(label);
          await tester.pumpAndSettle();
          Focus.of(tester.element(label)).requestFocus();
          await tester.pump();
          await tester.sendKeyEvent(LogicalKeyboardKey.enter);
          await tester.pumpAndSettle();
          expect(find.byType(RadioListTile<IpPolicy>), findsNWidgets(2));
          await tester.sendKeyEvent(LogicalKeyboardKey.arrowDown);
          await tester.sendKeyEvent(LogicalKeyboardKey.select);
          await choose(tester, app.strings.get('continue'));
          final region = find.byKey(const ValueKey('endpoint-preset-region'));
          await tester.ensureVisible(region);
          expect(tester.getSize(region).height, greaterThanOrEqualTo(48));
          expect(tester.takeException(), isNull);
        } finally {
          debugDefaultTargetPlatformOverride = null;
        }
      },
    );
  }

  test(
    'every preset control is available in all 21 catalogs with concise hints',
    () {
      expect(kEndpointPresetCatalogs.length, 21);
      expect(AppStrings.debugCatalogsAreComplete, isTrue);
      expect(AppStrings.debugUntranslatedFeatureKeys(), isEmpty);
      for (final catalog in kEndpointPresetCatalogs.values) {
        expect(catalog['endpoint_preset_prefer_hint']!.length, lessThan(100));
      }
    },
  );
}
