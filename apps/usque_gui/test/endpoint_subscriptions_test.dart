import 'dart:async';
import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:shared_preferences/shared_preferences.dart';
import 'package:usque/dev/preview_engine.dart';
import 'package:usque/dev/preview_update_downloader.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/models/endpoint_subscriptions.dart';
import 'package:usque/models/scenarios.dart';
import 'package:usque/screens/advanced_settings_screen.dart';
import 'package:usque/screens/profiles_screen.dart' show AccountsScreen;
import 'package:usque/screens/scenario_editor_screen.dart';
import 'package:usque/screens/shell_screen.dart';
import 'package:usque/services/control_codec.dart';
import 'package:usque/services/desktop_engine_client.dart';
import 'package:usque/services/engine_client.dart';
import 'package:usque/state/app_controller.dart';
import 'package:usque/state/endpoint_subscriptions_controller.dart';
import 'package:usque/widgets/endpoint_fields.dart';
import 'package:usque/widgets/zero_trust_endpoint_warning.dart';

import 'endpoint_selection_test.dart'
    show apply, choose, decodeProfile, response;
import 'scenarios_test.dart' show SceneEngine;
import 'ui_workflow_test.dart' show workflowHost;

const _codec = ControlCodec();

class SubscriptionTestEngine extends SceneEngine
    implements EndpointSubscriptionClient {
  SubscriptionTestEngine() {
    storedIdentityStatuses = {
      UsqueProfile.defaultProfileId: const ProfileIdentityStatus(
        state: ProfileIdentityState.ready,
        provider: IdentityProvider.consumer,
        licenseState: LicenseState.free,
      ),
    };
  }

  final _eventController = StreamController<EngineSnapshotEvent>.broadcast();
  bool endpointSubscriptions = true;
  bool domainEndpoints = true;
  bool probeSupported = true;
  bool connectedTunProbe = true;
  bool networkMutationChanged = false;
  int nextRev = 1;
  String? activeProbeId;
  String? activeProbeSubId;
  final Map<String, String?> activeSubByTier = {
    'free': null,
    'plus': null,
    'zero_trust': null,
  };
  final List<Map<String, Object?>> subscriptions = [
    <String, Object?>{
      'id': 'sub-free-a',
      'revision': 'rev-1',
      'name': 'Free List A',
      'url': 'https://example.com/free-a.txt',
      'account_tier': 'free',
      'auto_refresh': 'daily',
      'skipped_line_count': 1,
      'selected_endpoint_id': 'ep-1',
      'endpoints': <Map<String, Object?>>[
        <String, Object?>{
          'id': 'ep-1',
          'host': <String, Object?>{'kind': 'ipv4', 'value': '162.159.193.10'},
          'port': 2408,
          'note': 'Alpha IPv4',
          'pinned': true,
        },
        <String, Object?>{
          'id': 'ep-2',
          'host': <String, Object?>{'kind': 'ipv4', 'value': '162.159.193.11'},
          'port': 2408,
          'note': 'Beta IPv4',
          'pinned': false,
        },
        <String, Object?>{
          'id': 'ep-dom',
          'host': <String, Object?>{
            'kind': 'domain',
            'value': 'edge.example.com',
          },
          'port': 443,
          'note': 'Edge Domain',
          'pinned': false,
        },
      ],
    },
    <String, Object?>{
      'id': 'sub-free-b',
      'revision': 'rev-1',
      'name': 'Free List B',
      'url': 'https://example.com/free-b.txt',
      'account_tier': 'free',
      'auto_refresh': 'off',
      'skipped_line_count': 0,
      'selected_endpoint_id': 'ep-b1',
      'endpoints': <Map<String, Object?>>[
        <String, Object?>{
          'id': 'ep-b1',
          'host': <String, Object?>{'kind': 'ipv4', 'value': '162.159.195.10'},
          'port': 2408,
          'note': 'Gamma IPv4',
          'pinned': false,
        },
      ],
    },
    <String, Object?>{
      'id': 'sub-zt',
      'revision': 'rev-1',
      'name': 'Zero Trust List',
      'url': 'https://example.com/zt.txt',
      'account_tier': 'zero_trust',
      'auto_refresh': 'off',
      'skipped_line_count': 0,
      'selected_endpoint_id': 'ep-zt-1',
      'endpoints': <Map<String, Object?>>[
        <String, Object?>{
          'id': 'ep-zt-1',
          'host': <String, Object?>{'kind': 'ipv4', 'value': '162.159.197.10'},
          'port': 443,
          'note': 'ZT Primary',
          'pinned': false,
        },
        <String, Object?>{
          'id': 'ep-zt-2',
          'host': <String, Object?>{'kind': 'ipv4', 'value': '162.159.197.11'},
          'port': 443,
          'note': 'ZT Secondary',
          'pinned': false,
        },
      ],
    },
  ];
  final List<Map<String, Object?>> subCalls = [];

  @override
  bool get supportsSnapshotEvents => true;

  @override
  Stream<EngineSnapshotEvent> get snapshotEvents => _eventController.stream;

  void emitEvent(EngineSnapshotEvent event) {
    if (!_eventController.isClosed) _eventController.add(event);
  }

  Future<void> closeEvents() => _eventController.close();

  String? _tierFor(String accountId) {
    final status = storedIdentityStatuses[accountId];
    if (status == null) return null;
    if (status.provider == IdentityProvider.zeroTrust) return 'zero_trust';
    if (status.provider == IdentityProvider.consumer) {
      return switch (status.licenseState) {
        LicenseState.free => 'free',
        LicenseState.warpPlus => 'plus',
        _ => null,
      };
    }
    return null;
  }

  Map<String, Object?> catalogPayload({String? applyStatus}) {
    final tier = _tierFor(storedActiveProfileId);
    final activeSubId = tier == null ? null : activeSubByTier[tier];
    final activeSub = subscriptions
        .where((s) => s['id'] == activeSubId)
        .firstOrNull;
    return <String, Object?>{
      'subscriptions': [
        for (final sub in subscriptions)
          <String, Object?>{
            ...sub,
            'endpoints': [
              for (final ep in (sub['endpoints'] as List))
                Map<String, Object?>.from(ep as Map),
            ],
          },
      ],
      'active_account_id': storedActiveProfileId,
      'active_account_tier': tier,
      'active_subscription_id': activeSubId,
      'active_endpoint_id': activeSub?['selected_endpoint_id'],
      'refreshing_subscription_ids': const <String>[],
      if (activeProbeId != null && activeProbeSubId != null)
        'active_probe': <String, Object?>{
          'probe_id': activeProbeId,
          'subscription_id': activeProbeSubId,
        },
      'apply_status': ?applyStatus,
    };
  }

  @override
  Future<EngineCapabilities?> getCapabilities() async => EngineCapabilities(
    tlsFingerprints: TlsFingerprint.values,
    networkSettingsApplication: true,
    automaticEndpoints: true,
    endpointPresets: true,
    zeroTrustEndpointEditing: true,
    connectionScenarios: true,
    endpointSubscriptions: endpointSubscriptions,
    endpointSubscriptionDomainEndpoints: domainEndpoints,
    endpointSubscriptionProbe: probeSupported,
    endpointSubscriptionConnectedTunProbe: connectedTunProbe,
  );

  @override
  Future<NetworkSettingsState> saveNetworkSettings(
    String operationId,
    String accountId,
    UsqueProfile values,
    List<String> changedFields,
  ) async {
    // Mirror EndpointSettings::validate on the complete wire profile.
    if (values.endpointSubscriptionId != null &&
        (values.endpointSelection != EndpointSelection.custom ||
            values.endpointPresetRegion != null)) {
      throw const EngineException(
        'ENDPOINT_SUBSCRIPTION_REQUIRES_CUSTOM',
        'A subscription binding requires Custom selection.',
      );
    }
    final tier = _tierFor(accountId);
    if (tier != null) {
      activeSubByTier[tier] = values.endpointSubscriptionId;
    }
    return super.saveNetworkSettings(
      operationId,
      accountId,
      values,
      changedFields,
    );
  }

  @override
  Future<Map<String, Object?>> endpointSubscriptionCommand(
    Map<String, Object?> request,
  ) async {
    subCalls.add(Map<String, Object?>.from(request));
    final action = request['action'] as String? ?? 'list';
    switch (action) {
      case 'list':
        return catalogPayload();
      case 'select_endpoint':
        final subId = request['subscription_id'] as String;
        final epId = request['endpoint_id'] as String;
        final tier = _tierFor(storedActiveProfileId);
        if (tier == null) {
          throw const EngineException(
            'SUBSCRIPTION_UNAVAILABLE',
            'Unverified account.',
          );
        }
        final idx = subscriptions.indexWhere((s) => s['id'] == subId);
        if (idx < 0) {
          throw const EngineException('SUBSCRIPTION_NOT_FOUND', 'Not found.');
        }
        final sub = subscriptions[idx];
        if (sub['account_tier'] != tier) {
          throw const EngineException(
            'SUBSCRIPTION_TIER_MISMATCH',
            'Tier mismatch.',
          );
        }
        if (tier == 'zero_trust' &&
            activeSubByTier[tier] != subId &&
            request['zero_trust_risk_acknowledged'] != true) {
          throw const EngineException(
            'ZERO_TRUST_WARNING_REQUIRED',
            'Confirm ZT risk first.',
          );
        }
        subscriptions[idx] = <String, Object?>{
          ...sub,
          'selected_endpoint_id': epId,
          'retained_missing_endpoint': null,
        };
        activeSubByTier[tier] = subId;
        final current = storedProfiles.firstWhere(
          (p) => p.id == storedActiveProfileId,
        );
        await upsertProfile(
          current.copyWith(
            endpointSelection: EndpointSelection.custom,
            clearEndpointPresetRegion: true,
            endpointSubscriptionId: subId,
          ),
        );
        return catalogPayload(applyStatus: 'applied');
      case 'deactivate':
        final tier = _tierFor(storedActiveProfileId);
        if (tier != null) activeSubByTier[tier] = null;
        final current = storedProfiles.firstWhere(
          (p) => p.id == storedActiveProfileId,
        );
        await upsertProfile(
          current.copyWith(clearEndpointSubscriptionId: true),
        );
        return catalogPayload(applyStatus: 'applied');
      case 'set_pinned':
        final subId = request['subscription_id'] as String;
        final epId = request['endpoint_id'] as String;
        final pinned = request['pinned'] == true;
        final idx = subscriptions.indexWhere((s) => s['id'] == subId);
        final eps = [
          for (final ep in (subscriptions[idx]['endpoints'] as List))
            if ((ep as Map)['id'] == epId)
              <String, Object?>{
                ...Map<String, Object?>.from(ep),
                'pinned': pinned,
              }
            else
              Map<String, Object?>.from(ep),
        ];
        subscriptions[idx] = <String, Object?>{
          ...subscriptions[idx],
          'revision': 'rev-${++nextRev}',
          'endpoints': eps,
        };
        return catalogPayload();
      case 'start_probe':
        activeProbeId = request['probe_id'] as String? ?? 'probe-1';
        activeProbeSubId = request['subscription_id'] as String?;
        return <String, Object?>{
          ...catalogPayload(),
          'probe_id': activeProbeId,
        };
      case 'cancel_probe':
        activeProbeId = null;
        activeProbeSubId = null;
        return catalogPayload();
      default:
        return catalogPayload();
    }
  }
}

Future<AppController> _pumpSubApp(
  WidgetTester tester,
  SubscriptionTestEngine engine, {
  AppSection section = AppSection.profiles,
  ProfilesTab? tab,
  LocalePreference locale = LocalePreference.english,
  double scale = 1.0,
  Size size = const Size(1280, 900),
}) async {
  engine.legacyProfilesImported = true;
  tester.view.devicePixelRatio = 1;
  tester.view.physicalSize = size;
  addTearDown(tester.view.resetDevicePixelRatio);
  addTearDown(tester.view.resetPhysicalSize);
  final app = AppController(engine);
  await app.initialize();
  app.localePreference = locale;
  app.selectSection(section);
  if (tab != null) app.setProfilesTab(tab);
  addTearDown(app.dispose);
  await tester.pumpWidget(
    workflowHost(
      app,
      scale: scale,
      home: ShellScreen(controller: app),
    ),
  );
  await tester.pumpAndSettle();
  return app;
}

void main() {
  setUp(() {
    SharedPreferences.setMockInitialValues(<String, Object>{
      'onboarding_complete': true,
    });
  });

  test(
    'a terminal probe event before the start reply never leaves the probe running',
    () async {
      late EndpointSubscriptionsController controller;
      String? startedId;
      Map<String, Object?> catalog({String? probeId}) => <String, Object?>{
        'revision': 'r1',
        'subscriptions': const <Object>[],
        'refreshing_subscription_ids': const <String>[],
        if (probeId != null)
          'active_probe': <String, Object?>{
            'probe_id': probeId,
            'subscription_id': 'sub-a',
          },
      };
      controller = EndpointSubscriptionsController((request) async {
        switch (request['action']) {
          case 'start_probe':
            startedId = request['probe_id'] as String;
            // The fast probe finishes before its own RPC reply arrives.
            controller.acceptProbeProgress(<String, Object?>{
              'probe_id': startedId,
              'subscription_id': 'sub-a',
              'state': 'completed',
              'completed': 1,
              'total': 1,
            });
            return <String, Object?>{
              ...catalog(probeId: startedId),
              'probe_id': startedId,
            };
          case 'cancel_probe':
            throw const EngineException('SUBSCRIPTION_BUSY', 'busy');
          default:
            return catalog(probeId: startedId);
        }
      });
      addTearDown(controller.dispose);
      controller.setCapabilities(
        const EngineCapabilities(
          endpointSubscriptions: true,
          endpointSubscriptionProbe: true,
        ),
      );
      await controller.startProbe('sub-a');
      expect(controller.probeRunning, isFalse);

      // A late duplicate running event for the finished probe is stale.
      controller.acceptProbeProgress(<String, Object?>{
        'probe_id': startedId,
        'subscription_id': 'sub-a',
        'state': 'running',
        'completed': 0,
        'total': 1,
      });
      expect(controller.probeRunning, isFalse);
      expect(
        controller.activeProbeProgress?.state,
        EndpointProbeState.completed,
      );
    },
  );

  test(
    'UsqueProfile, ControlCodec, and RPC timeouts round-trip append-only subscription fields',
    () {
      const subId = '11111111-1111-4111-8111-111111111111';
      final profile = UsqueProfile.defaultProfile().copyWith(
        endpointSelection: EndpointSelection.custom,
        endpointSubscriptionId: subId,
      );
      expect(
        UsqueProfile.fromMap(profile.toMap()).endpointSubscriptionId,
        subId,
      );
      expect(
        decodeProfile(_codec.encodeProfile(profile)).endpointSubscriptionId,
        subId,
      );
      expect(profile.resetAdvancedDefaults().endpointSubscriptionId, isNull);
      expect(
        profile
            .copyWith(clearEndpointSubscriptionId: true)
            .endpointSubscriptionId,
        isNull,
      );
      expect(
        networkSettingsChangedFields(UsqueProfile.defaultProfile(), profile),
        contains('endpoint.subscription_id'),
      );

      // Capabilities fields 60..63 default false on older engines and parse when present.
      const legacyCaps = EngineCapabilities();
      expect(legacyCaps.endpointSubscriptions, isFalse);
      expect(legacyCaps.endpointSubscriptionDomainEndpoints, isFalse);
      expect(legacyCaps.endpointSubscriptionProbe, isFalse);
      expect(legacyCaps.endpointSubscriptionConnectedTunProbe, isFalse);

      final capsPayload =
          (ControlPayloadWriter()
                ..boolean(60, true)
                ..boolean(61, true)
                ..boolean(62, true)
                ..boolean(63, true))
              .takeBytes();
      final decodedCaps = _codec
          .decodeResponse(response(15, capsPayload), 'endpoint')
          .capabilities!;
      expect(decodedCaps.endpointSubscriptions, isTrue);
      expect(decodedCaps.endpointSubscriptionDomainEndpoints, isTrue);
      expect(decodedCaps.endpointSubscriptionProbe, isTrue);
      expect(decodedCaps.endpointSubscriptionConnectedTunProbe, isTrue);

      // ControlResponse tag 30 & EventEnvelope tag 26 JSON envelopes.
      final subRespPayload =
          (ControlPayloadWriter()
                ..string(1, jsonEncode({'active_subscription_id': subId})))
              .takeBytes();
      final decodedResp = _codec.decodeResponse(
        response(30, subRespPayload),
        'endpoint',
      );
      expect(
        decodedResp.endpointSubscription?['active_subscription_id'],
        subId,
      );

      final probeEventPayload =
          (ControlPayloadWriter()..string(
                1,
                jsonEncode({'probe_id': 'p1', 'completed': 1, 'total': 2}),
              ))
              .takeBytes();
      final eventFrame = _codec.frame(
        (ControlPayloadWriter()..message(26, probeEventPayload)).takeBytes(),
      );
      final decodedEvent = _codec.decodeEvent(Uint8List.fromList(eventFrame));
      expect(decodedEvent.endpointSubscriptionProbeProgress?['probe_id'], 'p1');
      expect(requestTimeoutForPayload(54), const Duration(seconds: 30));
      expect(
        linuxRequestTimeoutForPayload(54),
        const Duration(seconds: 30 + 360),
      );
    },
  );

  testWidgets(
    'Consumer subscription binding hydrates as Custom while shared storage keeps Automatic',
    (tester) async {
      // The Engine keeps the shared Automatic fallback and reports the active
      // Consumer binding only through the subscription catalogue.
      final engine = SubscriptionTestEngine()
        ..activeSubByTier['free'] = 'sub-free-a';
      addTearDown(engine.closeEvents);
      final app = await _pumpSubApp(tester, engine);
      await app.endpointSubscriptions.refresh(force: true);
      await tester.pumpAndSettle();

      expect(app.activeProfile.endpointSubscriptionId, 'sub-free-a');
      expect(app.activeProfile.endpointSelection, EndpointSelection.custom);
      expect(app.activeProfile.endpointPresetRegion, isNull);

      // An unrelated save carries the hydrated binding and must validate.
      final saved = await app.saveNetwork(
        app.activeProfile.copyWith(mtu: app.activeProfile.mtu - 20),
      );
      await tester.pumpAndSettle();
      expect(saved, isTrue);
      expect(app.lastError, isNull);

      engine.activeSubByTier['free'] = null;
      await app.endpointSubscriptions.refresh(force: true);
      await tester.pumpAndSettle();
      expect(app.activeProfile.endpointSubscriptionId, isNull);
    },
  );

  testWidgets(
    'the accounts shortcut leaves the subscription tab when scenes are unsupported',
    (tester) async {
      final engine = _NoScenesSubscriptionEngine();
      addTearDown(engine.closeEvents);
      final app = await _pumpSubApp(
        tester,
        engine,
        tab: ProfilesTab.subscriptions,
      );
      expect(app.profilesTab, ProfilesTab.subscriptions);
      app.openAccounts();
      await tester.pumpAndSettle();
      // Without scenes, accounts render in the first tab.
      expect(app.profilesTab, ProfilesTab.scenes);
      expect(find.byType(AccountsScreen), findsOneWidget);
    },
  );

  testWidgets(
    'Profiles switcher is hidden when unsupported and responsive across desktop, phone, 200% text, and RTL',
    (tester) async {
      final legacyEngine = SubscriptionTestEngine()
        ..endpointSubscriptions = false;
      addTearDown(legacyEngine.closeEvents);
      await _pumpSubApp(tester, legacyEngine, section: AppSection.profiles);
      expect(find.byKey(const ValueKey('profiles-tab-switcher')), findsNothing);

      for (final layout in [
        (
          size: const Size(1280, 900),
          scale: 1.0,
          locale: LocalePreference.english,
        ),
        (
          size: const Size(390, 844),
          scale: 2.0,
          locale: LocalePreference.simplifiedChinese,
        ),
        (
          size: const Size(390, 844),
          scale: 1.0,
          locale: LocalePreference.persian,
        ),
        (
          size: const Size(390, 844),
          scale: 2.0,
          locale: LocalePreference.arabic,
        ),
      ]) {
        final engine = SubscriptionTestEngine();
        addTearDown(engine.closeEvents);
        final app = await _pumpSubApp(
          tester,
          engine,
          section: AppSection.profiles,
          locale: layout.locale,
          scale: layout.scale,
          size: layout.size,
        );

        final switcher = find.byKey(const ValueKey('profiles-tab-switcher'));
        expect(switcher, findsOneWidget);
        await tester.ensureVisible(switcher);
        await tester.tap(find.text(app.strings.get('sub_tab_label')).last);
        await tester.pumpAndSettle();
        expect(app.profilesTab, ProfilesTab.subscriptions);
        expect(tester.takeException(), isNull);
      }
    },
  );

  testWidgets(
    'same-subscription endpoint switch skips scene drift prompt; cross-subscription and deactivate ask when scene unmodified',
    (tester) async {
      final engine = SubscriptionTestEngine()
        ..selected = 'scene-a'
        ..activeSubByTier['free'] = 'sub-free-a';
      engine.storedProfiles = [
        UsqueProfile.defaultProfile().copyWith(
          endpointSelection: EndpointSelection.custom,
          endpointSubscriptionId: 'sub-free-a',
        ),
      ];
      addTearDown(engine.closeEvents);
      final app = await _pumpSubApp(
        tester,
        engine,
        section: AppSection.profiles,
        tab: ProfilesTab.subscriptions,
      );

      // Switch within the active subscription: immediate, no drift dialog.
      await tester.tap(find.byKey(const ValueKey('sub-endpoint-ep-2')));
      await tester.pumpAndSettle();
      expect(find.text(app.strings.get('sub_scene_drift_title')), findsNothing);
      expect(
        engine.subCalls.any(
          (c) =>
              c['action'] == 'select_endpoint' &&
              c['subscription_id'] == 'sub-free-a' &&
              c['endpoint_id'] == 'ep-2',
        ),
        isTrue,
      );

      // Focus Free List B and switch across subscriptions: drift confirmation shown.
      await tester.tap(
        find.byKey(const ValueKey('subscription-chip-sub-free-b')),
      );
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(const ValueKey('sub-endpoint-ep-b1')));
      await tester.pumpAndSettle();
      expect(
        find.text(app.strings.get('sub_scene_drift_title')),
        findsOneWidget,
      );
      await tester.tap(find.text(app.strings.get('cancel')));
      await tester.pumpAndSettle();
      expect(app.activeProfile.endpointSubscriptionId, 'sub-free-a');

      // Confirm switching to Free List B.
      await tester.tap(find.byKey(const ValueKey('sub-endpoint-ep-b1')));
      await tester.pumpAndSettle();
      await tester.tap(find.text(app.strings.get('continue')));
      await tester.pumpAndSettle();
      expect(app.activeProfile.endpointSubscriptionId, 'sub-free-b');

      // Deactivate prompts when an unmodified scene was active and clears binding on confirm.
      await tester.ensureVisible(
        find.byKey(const ValueKey('subscription-deactivate')),
      );
      await tester.tap(find.byKey(const ValueKey('subscription-deactivate')));
      await tester.pumpAndSettle();
      expect(
        find.text(app.strings.get('sub_scene_drift_title')),
        findsOneWidget,
      );
      await tester.tap(find.text(app.strings.get('continue')));
      await tester.pumpAndSettle();
      expect(app.activeProfile.endpointSubscriptionId, isNull);
    },
  );

  testWidgets(
    'Zero Trust subscription requires fullscreen warning once, surfaces Home banner, and quick-switches within ZT subscription without repeat dialog',
    (tester) async {
      final engine = SubscriptionTestEngine()
        ..storedIdentityStatuses = {
          UsqueProfile.defaultProfileId: const ProfileIdentityStatus(
            state: ProfileIdentityState.ready,
            provider: IdentityProvider.zeroTrust,
            licenseState: LicenseState.notApplicable,
          ),
        };
      addTearDown(engine.closeEvents);
      final app = await _pumpSubApp(
        tester,
        engine,
        section: AppSection.profiles,
        tab: ProfilesTab.subscriptions,
      );

      await tester.tap(find.byKey(const ValueKey('sub-endpoint-ep-zt-1')));
      await tester.pumpAndSettle();
      expect(find.byType(ZeroTrustEndpointWarning), findsOneWidget);
      await tester.tap(find.byKey(const ValueKey('zt-endpoint-risk-ack')));
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(const ValueKey('zt-endpoint-risk-continue')));
      await tester.pumpAndSettle();

      expect(app.activeProfile.endpointSubscriptionId, 'sub-zt');
      expect(app.hasCustomZeroTrustEndpointRisk, isTrue);

      // Quick switch within already-active ZT subscription skips ZT modal.
      await tester.tap(find.byKey(const ValueKey('sub-endpoint-ep-zt-2')));
      await tester.pumpAndSettle();
      expect(find.byType(ZeroTrustEndpointWarning), findsNothing);
      expect(app.endpointSubscriptions.catalog?.activeEndpointId, 'ep-zt-2');

      // Home displays both the non-dismissible ZT risk banner and the quick-switch link.
      app.selectSection(AppSection.home);
      await tester.pumpAndSettle();
      expect(
        find.byKey(const ValueKey('home-zero-trust-endpoint-risk')),
        findsOneWidget,
      );
      expect(
        find.byKey(const ValueKey('home-endpoint-subscription')),
        findsOneWidget,
      );
    },
  );

  testWidgets(
    'strict tier matching, unverified Consumer fail-closed, and retained upstream-removed snapshot render properly',
    (tester) async {
      final engine = SubscriptionTestEngine()
        ..activeSubByTier['free'] = 'sub-free-a';
      engine.subscriptions[0] = <String, Object?>{
        ...engine.subscriptions[0],
        'selected_endpoint_id': 'ep-removed',
        'retained_missing_endpoint': <String, Object?>{
          'id': 'ep-removed',
          'host': <String, Object?>{'kind': 'ipv4', 'value': '162.159.199.9'},
          'port': 2408,
          'note': 'Removed Upstream Edge',
          'pinned': false,
        },
      };
      engine.storedProfiles = [
        UsqueProfile.defaultProfile().copyWith(
          id: 'free-id',
          name: 'Free',
          endpointSelection: EndpointSelection.custom,
          endpointSubscriptionId: 'sub-free-a',
        ),
        UsqueProfile.defaultProfile().copyWith(
          id: 'unverified-id',
          name: 'Unverified',
        ),
      ];
      engine.storedActiveProfileId = 'free-id';
      engine.storedIdentityStatuses = {
        'free-id': const ProfileIdentityStatus(
          state: ProfileIdentityState.ready,
          provider: IdentityProvider.consumer,
          licenseState: LicenseState.free,
        ),
        'unverified-id': const ProfileIdentityStatus(
          state: ProfileIdentityState.ready,
          provider: IdentityProvider.consumer,
          licenseState: LicenseState.unknown,
        ),
      };
      addTearDown(engine.closeEvents);
      final app = await _pumpSubApp(
        tester,
        engine,
        section: AppSection.profiles,
        tab: ProfilesTab.subscriptions,
      );

      // Retained upstream-removed row appears pinned at top.
      expect(
        find.byKey(const ValueKey('sub-retained-endpoint')),
        findsOneWidget,
      );
      expect(
        find.text(app.strings.get('sub_removed_upstream')),
        findsOneWidget,
      );

      // Focusing ZT subscription on Free account shows tier warning and locks activation/probing.
      await tester.tap(find.byKey(const ValueKey('subscription-chip-sub-zt')));
      await tester.pumpAndSettle();
      expect(
        find.byKey(const ValueKey('subscription-tier-warning')),
        findsOneWidget,
      );
      await tester.tap(find.byKey(const ValueKey('sub-endpoint-ep-zt-1')));
      await tester.pumpAndSettle();
      expect(
        engine.subCalls.where((c) => c['action'] == 'select_endpoint'),
        isEmpty,
      );

      // Switching to unverified Consumer shows verify warning fail-closed.
      app.setActiveProfile('unverified-id');
      await app.flushProfileWrites();
      await tester.pumpAndSettle();
      await tester.tap(
        find.byKey(const ValueKey('subscription-chip-sub-free-a')),
      );
      await tester.pumpAndSettle();
      expect(
        find.byKey(const ValueKey('subscription-verify-warning')),
        findsOneWidget,
      );
    },
  );

  testWidgets(
    'Home EndpointSubscriptionHomeLink is hidden when inactive and opens quick-switch dialog when active',
    (tester) async {
      final engine = SubscriptionTestEngine();
      addTearDown(engine.closeEvents);
      final app = await _pumpSubApp(tester, engine, section: AppSection.home);
      expect(
        find.byKey(const ValueKey('home-endpoint-subscription')),
        findsNothing,
      );

      final result = await app.endpointSubscriptions.selectEndpoint(
        subscriptionId: 'sub-free-a',
        endpointId: 'ep-1',
      );
      expect(result.errorCode, isNull);
      await app.flushProfileWrites();
      await tester.pumpAndSettle();

      final row = find.byKey(const ValueKey('home-endpoint-subscription'));
      expect(row, findsOneWidget);
      await tester.ensureVisible(row);
      await tester.tap(row);
      await tester.pumpAndSettle();

      expect(find.byKey(const ValueKey('home-sub-ep-ep-1')), findsOneWidget);
      expect(find.byKey(const ValueKey('home-sub-ep-ep-2')), findsOneWidget);
      await tester.tap(find.byKey(const ValueKey('home-sub-ep-ep-2')));
      await tester.pumpAndSettle();
      expect(app.endpointSubscriptions.catalog?.activeEndpointId, 'ep-2');

      // Re-open quick switch and jump to Profiles -> Subscriptions via Manage subscriptions.
      await tester.tap(
        find.byKey(const ValueKey('home-endpoint-subscription')),
      );
      await tester.pumpAndSettle();
      await tester.ensureVisible(find.byKey(const ValueKey('home-sub-manage')));
      await tester.tap(find.byKey(const ValueKey('home-sub-manage')));
      await tester.pumpAndSettle();
      expect(app.section, AppSection.profiles);
      expect(app.profilesTab, ProfilesTab.subscriptions);
    },
  );

  testWidgets(
    'streaming probe results update RTT/status, cancel properly, and clear on account switch',
    (tester) async {
      final engine = SubscriptionTestEngine()
        ..storedProfiles = [
          UsqueProfile.defaultProfile().copyWith(id: 'acc-1', name: 'One'),
          UsqueProfile.defaultProfile().copyWith(id: 'acc-2', name: 'Two'),
        ]
        ..storedActiveProfileId = 'acc-1'
        ..storedIdentityStatuses = {
          'acc-1': const ProfileIdentityStatus(
            state: ProfileIdentityState.ready,
            provider: IdentityProvider.consumer,
            licenseState: LicenseState.free,
          ),
          'acc-2': const ProfileIdentityStatus(
            state: ProfileIdentityState.ready,
            provider: IdentityProvider.consumer,
            licenseState: LicenseState.warpPlus,
          ),
        };
      addTearDown(engine.closeEvents);
      final app = await _pumpSubApp(
        tester,
        engine,
        section: AppSection.profiles,
        tab: ProfilesTab.subscriptions,
      );

      await tester.tap(find.byKey(const ValueKey('subscription-probe')));
      await tester.pump();
      await tester.pump(const Duration(milliseconds: 50));
      expect(app.endpointSubscriptions.probeRunning, isTrue);

      engine.emitEvent(
        EngineSnapshotEvent(
          endpointSubscriptionProbeProgress: <String, Object?>{
            'probe_id': engine.activeProbeId,
            'subscription_id': 'sub-free-a',
            'state': 'running',
            'total': 3,
            'completed': 2,
            'results': <Map<String, Object?>>[
              <String, Object?>{
                'endpoint_id': 'ep-1',
                'status': 'reachable',
                'protocol': 'http3',
                'rtt_ms': 42,
              },
              <String, Object?>{'endpoint_id': 'ep-2', 'status': 'timed_out'},
            ],
          },
        ),
      );
      await tester.pump();
      await tester.pump(const Duration(milliseconds: 50));
      expect(find.textContaining('42 ms · H3'), findsOneWidget);
      expect(find.text(app.strings.get('sub_probe_timed_out')), findsOneWidget);

      // Cancel active probe.
      await tester.tap(find.byKey(const ValueKey('subscription-probe')));
      await tester.pumpAndSettle();
      expect(engine.subCalls.any((c) => c['action'] == 'cancel_probe'), isTrue);

      // Account switch invalidates stale probe results.
      app.setActiveProfile('acc-2');
      await app.flushProfileWrites();
      await tester.pumpAndSettle();
      expect(app.endpointSubscriptions.probeResultsFor('sub-free-a'), isEmpty);
    },
  );

  testWidgets(
    'PreviewEngine subscription and probe lifecycle disposes cleanly without pending timers',
    (tester) async {
      final preview = PreviewEngine();
      final app = AppController(
        preview,
        updateDownloader: PreviewUpdateDownloader(preview),
      );
      await app.initialize();
      app.selectSection(AppSection.profiles);
      app.setProfilesTab(ProfilesTab.subscriptions);
      addTearDown(() {
        app.dispose();
        preview.dispose();
      });
      await tester.pumpWidget(workflowHost(app));
      await tester.pumpAndSettle();
      final subs = app.endpointSubscriptions;
      expect(subs.catalog?.subscriptions, isNotEmpty);

      final first = subs.catalog!.subscriptions.first;
      await subs.startProbe(first.id);
      await tester.pump(const Duration(milliseconds: 120));
      await preview.clearAllData(confirmed: true);
      await tester.pump(const Duration(seconds: 1));
    },
  );

  testWidgets(
    'Advanced Settings and Scenario Editor filter subscriptions by tier and surface deleted/mismatched notices in Scenes list',
    (tester) async {
      final engine = SubscriptionTestEngine();
      final missingSubProfile = UsqueProfile.defaultProfile().copyWith(
        endpointSelection: EndpointSelection.custom,
        endpointSubscriptionId: 'deleted-sub-id',
      );
      engine.entries = [
        <String, Object?>{
          'id': 'scene-missing-sub',
          'name': 'Missing Sub Scene',
          'revision': 'r1',
          'settings': ScenarioSettings.fromProfile(missingSubProfile).toMap(),
        },
        <String, Object?>{
          'id': 'scene-mismatch-sub',
          'name': 'ZT Sub on Free Scene',
          'revision': 'r2',
          'settings': ScenarioSettings.fromProfile(
            UsqueProfile.defaultProfile().copyWith(
              endpointSelection: EndpointSelection.custom,
              endpointSubscriptionId: 'sub-zt',
            ),
          ).toMap(),
        },
      ];
      addTearDown(engine.closeEvents);
      final app = await _pumpSubApp(
        tester,
        engine,
        section: AppSection.profiles,
      );

      // Scenes tab shows notices for missing and mismatched subscription bindings.
      expect(
        find.text(app.strings.get('scenario_reference_missing')),
        findsOneWidget,
      );
      expect(
        find.text(
          app.strings
              .get('sub_tier_mismatch')
              .replaceAll('{tier}', app.strings.get('sub_tier_zt')),
        ),
        findsOneWidget,
      );

      // AdvancedSettingsScreen shows compatible Free subscriptions under Custom -> Subscription.
      await tester.pumpWidget(
        workflowHost(app, home: AdvancedSettingsScreen(controller: app)),
      );
      await tester.pumpAndSettle();
      await choose(tester, 'Custom');
      final sourcePicker = tester.widget<SegmentedButton<EndpointCustomSource>>(
        find.byKey(const ValueKey('endpoint-custom-source')),
      );
      expect(
        sourcePicker.segments.map((s) => s.value),
        contains(EndpointCustomSource.subscription),
      );
      await choose(tester, app.strings.get('endpoint_subscription_mode'));
      await tester.pumpAndSettle();
      expect(find.text('Free List A'), findsOneWidget);
      expect(find.text('Zero Trust List'), findsNothing);
      await apply(tester, app);
      await app.flushProfileWrites();
      expect(engine.storedProfiles.single.endpointSubscriptionId, 'sub-free-a');

      // ScenarioEditorScreen inherits subscription source without triggering ZT error on Save.
      await tester.pumpWidget(
        workflowHost(
          app,
          home: ScenarioEditorScreen(
            controller: app,
            scene: ConnectionScenario.fromMap({
              'id': 'scene-new',
              'name': 'Free Sub Scene',
              'revision': 'r3',
              'settings': ScenarioSettings.fromProfile(
                UsqueProfile.defaultProfile().copyWith(
                  endpointSelection: EndpointSelection.custom,
                  endpointSubscriptionId: 'sub-free-b',
                ),
              ).toMap(),
            }),
          ),
        ),
      );
      await tester.pumpAndSettle();
      expect(find.text('Free List B'), findsOneWidget);
    },
  );
}

class _NoScenesSubscriptionEngine extends SubscriptionTestEngine {
  @override
  Future<EngineCapabilities?> getCapabilities() async => EngineCapabilities(
    tlsFingerprints: TlsFingerprint.values,
    networkSettingsApplication: true,
    automaticEndpoints: true,
    endpointPresets: true,
    zeroTrustEndpointEditing: true,
    connectionScenarios: false,
    endpointSubscriptions: true,
    endpointSubscriptionDomainEndpoints: true,
    endpointSubscriptionProbe: true,
    endpointSubscriptionConnectedTunProbe: true,
  );
}
