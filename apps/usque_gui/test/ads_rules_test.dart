import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:shared_preferences/shared_preferences.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/screens/geo_direct_settings_screen.dart';
import 'package:usque/services/control_codec.dart';
import 'package:usque/services/engine_client.dart';
import 'package:usque/state/app_controller.dart';
import 'package:usque/widgets/save_changes_bar.dart';

import 'audit_forms_test.dart' show FormEngine;
import 'ui_workflow_test.dart' show workflowHost;

class AdsEngine extends FormEngine implements AdsRulesClient {
  int downloads = 0;
  bool fail = false;
  bool brokenCache = false;
  Completer<GeoRulesList>? pendingList;
  Completer<void>? pendingDownload;

  @override
  Future<GeoRulesList> listGeoRules() async {
    if (brokenCache) {
      throw const EngineException('GEO_RULES_FAILED', 'invalid geosite');
    }
    final pending = pendingList;
    pendingList = null;
    return pending == null ? storedGeoRules : pending.future;
  }

  @override
  Future<EngineCapabilities?> getCapabilities() async =>
      const EngineCapabilities(
        tlsFingerprints: TlsFingerprint.values,
        networkSettingsApplication: true,
        automaticEndpoints: true,
        routingRules: true,
        liveAdsRules: true,
      );

  @override
  Future<List<GeoRulesUpdateResult>> updateAdsRules() async {
    downloads++;
    await pendingDownload?.future;
    if (!fail) {
      brokenCache = false;
      storedGeoRules = GeoRulesList(
        hasAds: true,
        hasGlobalGeosite: true,
        adsRevision: 'revision-$downloads',
        globalGeositeUpdatedUnixMilliseconds: DateTime(
          2026,
          10,
          12,
        ).millisecondsSinceEpoch,
      );
    }
    return [
      GeoRulesUpdateResult(
        countryCode: '',
        artifactKind: 'geosite',
        artifactScope: 'global',
        status: fail
            ? GeoRulesUpdateStatus.failed
            : GeoRulesUpdateStatus.updated,
      ),
    ];
  }
}

void main() {
  setUp(
    () => SharedPreferences.setMockInitialValues({
      'onboarding_complete': true,
      'update_checks_enabled': false,
    }),
  );

  test('live Ads capability requires an explicit native advertisement', () {
    expect(EngineCapabilities.fromMap({}).liveAdsRules, isFalse);
    expect(
      EngineCapabilities.fromMap({'live_ads_rules': true}).liveAdsRules,
      isTrue,
    );
    final payload = ControlPayloadWriter()..boolean(47, true);
    final frame = const ControlCodec().frame(
      (ControlPayloadWriter()
            ..string(1, 'ads')
            ..message(15, payload.takeBytes()))
          .takeBytes(),
    );
    expect(debugDecodeCapabilitiesFrame(frame, 'ads')?.liveAdsRules, isTrue);
  });

  test(
    'apply prepares missing rules once and leaves the connection alone',
    () async {
      final engine = AdsEngine()..pendingDownload = Completer<void>();
      final app = AppController(engine);
      await app.initialize();
      final saved = app.saveNetwork(
        app.activeProfile.copyWith(
          routing: const RoutingSettings(adsEnabled: true),
        ),
        changedFields: ['routing'],
      );
      await Future<void>.delayed(Duration.zero);
      expect(engine.downloads, 1);
      expect(engine.saves, 0);
      expect(app.adsRulesBusy, isTrue);
      final joined = app.updateAdsRules(manual: true);
      engine.pendingDownload!.complete();
      expect(await saved, isTrue);
      expect(await joined, isTrue);
      await app.maintainAdsRules();
      expect(engine.downloads, 1);
      expect(app.geoRules.hasAds, isTrue);
      expect(app.activeProfile.routing.adsEnabled, isTrue);
      expect(
        engine.calls.where(
          (call) =>
              call == 'retry' || call == 'connect' || call == 'disconnect',
        ),
        isEmpty,
      );
      expect(engine.calls, isNot(contains('updateAllGeoRules')));
    },
  );

  test(
    'weekly checks and failed retries are throttled across restarts',
    () async {
      var now = DateTime(2026, 10, 12);
      final engine = AdsEngine();
      final app = AppController(engine, adsClock: () => now);
      await app.initialize();
      await app.saveNetwork(
        app.activeProfile.copyWith(
          routing: const RoutingSettings(adsEnabled: true),
        ),
        changedFields: ['routing'],
      );
      await app.maintainAdsRules();
      expect(engine.downloads, 1);
      now = now.add(const Duration(days: 6));
      await app.maintainAdsRules();
      expect(engine.downloads, 1);
      now = now.add(const Duration(days: 1));
      engine.fail = true;
      await app.maintainAdsRules();
      expect(engine.downloads, 2);
      expect(app.geoRules.adsRevision, 'revision-1');
      expect(app.adsRulesFailed, isTrue);
      app.dispose();

      final restarted = AppController(engine, adsClock: () => now);
      addTearDown(restarted.dispose);
      await restarted.initialize();
      await restarted.maintainAdsRules();
      expect(engine.downloads, 2);
      now = now.add(const Duration(hours: 1));
      engine.fail = false;
      await restarted.maintainAdsRules();
      expect(engine.downloads, 3);
      expect(restarted.geoRules.adsRevision, 'revision-3');
      await restarted.saveNetwork(
        restarted.activeProfile.copyWith(routing: const RoutingSettings()),
        changedFields: ['routing'],
      );
      now = now.add(const Duration(days: 8));
      await restarted.maintainAdsRules();
      expect(engine.downloads, 3);
    },
  );

  test(
    'first failure saves preference, exposes retry, and does not claim rules ready',
    () async {
      final engine = AdsEngine()..fail = true;
      final app = AppController(engine);
      await app.initialize();
      expect(
        await app.saveNetwork(
          app.activeProfile.copyWith(
            routing: const RoutingSettings(adsEnabled: true),
          ),
          changedFields: ['routing'],
        ),
        isTrue,
      );
      await app.maintainAdsRules();
      expect(engine.downloads, 1);
      expect(app.geoRules.hasAds, isFalse);
      expect(app.adsRulesFailed, isTrue);
      engine.fail = false;
      expect(await app.updateAdsRules(manual: true), isTrue);
      expect(app.adsRulesFailed, isFalse);
    },
  );

  test(
    'interrupted retry metadata cannot delay missing rules for a week',
    () async {
      final now = DateTime(2026, 10, 12);
      SharedPreferences.setMockInitialValues({
        'onboarding_complete': true,
        'update_checks_enabled': false,
        'ads_check_failed': true,
        'ads_next_check': now
            .add(const Duration(days: 6))
            .millisecondsSinceEpoch,
      });
      final engine = AdsEngine()..legacyProfilesImported = true;
      engine.storedProfiles = [
        UsqueProfile.defaultProfile().copyWith(
          routing: const RoutingSettings(adsEnabled: true),
        ),
      ];
      final app = AppController(engine, adsClock: () => now);
      addTearDown(app.dispose);
      await app.initialize();
      await app.maintainAdsRules();
      expect(engine.downloads, 1);
      expect(app.geoRules.hasAds, isTrue);
    },
  );

  for (final request in ['manual', 'prepare', 'automatic']) {
    test('a corrupt catalog allows verified repair during $request', () async {
      final engine = AdsEngine();
      final app = AppController(engine);
      addTearDown(app.dispose);
      await app.initialize();
      if (request == 'automatic') {
        await app.saveNetwork(
          app.activeProfile.copyWith(
            routing: const RoutingSettings(adsEnabled: true),
          ),
          changedFields: ['routing'],
        );
        await app.maintainAdsRules();
      }
      final before = engine.downloads;
      engine.brokenCache = true;
      if (request == 'automatic') {
        await app.maintainAdsRules();
      } else {
        expect(
          await app.updateAdsRules(
            manual: request == 'manual',
            prepare: request == 'prepare',
          ),
          isTrue,
        );
      }
      expect(engine.downloads, before + 1);
      expect(engine.brokenCache, isFalse);
      expect(app.geoRules.adsRevision, 'revision-${before + 1}');
      expect(app.adsRulesFailed, isFalse);
    });
  }

  test(
    'failed corrupt-cache repair remains failed and throttles retries',
    () async {
      final engine = AdsEngine();
      final app = AppController(engine);
      addTearDown(app.dispose);
      await app.initialize();
      engine.brokenCache = true;
      engine.fail = true;
      await app.saveNetwork(
        app.activeProfile.copyWith(
          routing: const RoutingSettings(adsEnabled: true),
        ),
        changedFields: ['routing'],
      );
      await app.maintainAdsRules();
      expect(engine.downloads, 1);
      expect(app.geoRules.hasAds, isFalse);
      expect(app.adsRulesFailed, isTrue);
      engine.fail = false;
      expect(await app.updateAdsRules(manual: true), isTrue);
      expect(engine.downloads, 2);
    },
  );

  test(
    'manual requests behind a throttled check share one forced download',
    () async {
      final now = DateTime(2026, 10, 12);
      final engine = AdsEngine();
      final app = AppController(engine, adsClock: () => now);
      addTearDown(app.dispose);
      await app.initialize();
      await app.saveNetwork(
        app.activeProfile.copyWith(
          routing: const RoutingSettings(adsEnabled: true),
        ),
        changedFields: ['routing'],
      );
      await app.maintainAdsRules();
      expect(engine.downloads, 1);
      final held = Completer<GeoRulesList>();
      engine.pendingList = held;
      final automatic = app.maintainAdsRules();
      await Future<void>.delayed(Duration.zero);
      expect(app.adsRulesBusy, isFalse);
      final manual = app.updateAdsRules(manual: true);
      final secondManual = app.updateAdsRules(manual: true);
      held.complete(engine.storedGeoRules);
      await automatic;
      expect(await manual, isTrue);
      expect(await secondManual, isTrue);
      expect(engine.downloads, 2);
      expect(app.geoRules.adsRevision, 'revision-2');
    },
  );

  for (final chinese in [false, true]) {
    testWidgets('Ads status and details work at large text, Chinese=$chinese', (
      tester,
    ) async {
      tester.view.physicalSize = const Size(720, 1200);
      tester.view.devicePixelRatio = 1;
      addTearDown(tester.view.resetPhysicalSize);
      addTearDown(tester.view.resetDevicePixelRatio);
      final engine = AdsEngine()..fail = true;
      final app = AppController(engine);
      await app.initialize();
      app.localePreference = chinese
          ? LocalePreference.simplifiedChinese
          : LocalePreference.english;
      await tester.pumpWidget(
        workflowHost(
          app,
          dark: chinese,
          scale: 2,
          home: GeoDirectSettingsScreen(controller: app),
        ),
      );
      await tester.pumpAndSettle();
      tester
          .widget<SwitchListTile>(find.byKey(const ValueKey('routing-ads')))
          .onChanged!(true);
      await tester.pumpAndSettle();
      expect(find.text(app.strings.get('ads_apply_on')), findsOneWidget);
      expect(find.byKey(const ValueKey('ads-retry')), findsNothing);
      tester.widget<SaveChangesBar>(find.byType(SaveChangesBar)).onSave!();
      await tester.pumpAndSettle();
      expect(find.text(app.strings.get('ads_failed_missing')), findsOneWidget);
      expect(find.text(app.strings.get('ads_active')), findsNothing);
      engine.fail = false;
      final retry = find.byKey(const ValueKey('ads-retry'));
      await tester.ensureVisible(retry);
      await tester.tap(retry);
      await tester.pumpAndSettle();
      expect(find.text(app.strings.get('ads_ready')), findsOneWidget);
      expect(find.byKey(const ValueKey('ads-update')), findsNothing);
      final details = find.text(app.strings.get('ads_details'));
      await Scrollable.ensureVisible(tester.element(details), alignment: 0.5);
      await tester.pumpAndSettle();
      await tester.tap(details);
      await tester.pumpAndSettle();
      expect(find.byKey(const ValueKey('ads-update')), findsOneWidget);
      expect(find.text(app.strings.get('ads_source')), findsOneWidget);
      app.snapshot = const EngineSnapshot(
        phase: ConnectionPhase.connected,
        adsRuleRevision: 'revision-2',
      );
      await app.refreshGeoRules();
      await tester.pumpAndSettle();
      expect(find.text(app.strings.get('ads_active')), findsOneWidget);
      expect(find.text(app.strings.get('routing_pending')), findsNothing);
      app.adsRulesFailed = true;
      engine.storedGeoRules = const GeoRulesList();
      await app.refreshGeoRules();
      await tester.pumpAndSettle();
      expect(find.text(app.strings.get('ads_failed_old')), findsOneWidget);
      expect(find.text(app.strings.get('ads_failed_missing')), findsNothing);
      expect(tester.takeException(), isNull);
      await tester.pumpWidget(const SizedBox.shrink());
      app.dispose();
    });
  }
}
