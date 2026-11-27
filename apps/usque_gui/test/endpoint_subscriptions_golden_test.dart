import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:shared_preferences/shared_preferences.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/screens/shell_screen.dart';
import 'package:usque/state/app_controller.dart';

import 'endpoint_subscriptions_test.dart';
import 'support/linux_golden_support.dart';
import 'ui_workflow_test.dart' show workflowHost;

SubscriptionTestEngine _goldenEngine({required bool zeroTrustAccount}) {
  final engine = SubscriptionTestEngine();
  final updatedUnixMs = DateTime.now()
      .subtract(const Duration(minutes: 15, seconds: 10))
      .millisecondsSinceEpoch;
  engine.subscriptions
    ..clear()
    ..addAll([
      <String, Object?>{
        'id': 'sub-free',
        'revision': 'rev-3',
        'name': 'Community Anycast',
        'url': 'https://masque.pages.dev/endpoint/3rd/warp-ipv4.txt',
        'account_tier': 'free',
        'auto_refresh': 'daily',
        'last_refreshed_at_unix_ms': updatedUnixMs,
        'skipped_line_count': 2,
        'selected_endpoint_id': 'ep-retained',
        'retained_missing_endpoint': <String, Object?>{
          'id': 'ep-retained',
          'host': <String, Object?>{'kind': 'ipv4', 'value': '162.159.199.9'},
          'port': 2408,
          'note': 'Legacy Active Edge',
          'pinned': false,
        },
        'endpoints': <Map<String, Object?>>[
          <String, Object?>{
            'id': 'ep-tokyo',
            'host': <String, Object?>{
              'kind': 'ipv4',
              'value': '162.159.193.10',
            },
            'port': 2408,
            'note': 'Tokyo Edge 01',
            'pinned': true,
          },
          <String, Object?>{
            'id': 'ep-osaka',
            'host': <String, Object?>{
              'kind': 'ipv6',
              'value': '2606:4700:103::10',
            },
            'port': 2408,
            'note': 'Osaka Dual-Stack',
            'pinned': false,
          },
          <String, Object?>{
            'id': 'ep-dom',
            'host': <String, Object?>{
              'kind': 'domain',
              'value': 'edge.example.com',
            },
            'port': 443,
            'note': 'Singapore Domain',
            'pinned': false,
          },
        ],
      },
      <String, Object?>{
        'id': 'sub-plus',
        'revision': 'rev-1',
        'name': 'WARP+ Reserve',
        'url': 'https://example.com/plus.txt',
        'account_tier': 'plus',
        'auto_refresh': 'weekly',
        'skipped_line_count': 0,
        'selected_endpoint_id': 'ep-p1',
        'endpoints': <Map<String, Object?>>[
          <String, Object?>{
            'id': 'ep-p1',
            'host': <String, Object?>{
              'kind': 'ipv4',
              'value': '162.159.195.21',
            },
            'port': 2408,
            'note': 'Frankfurt Plus',
            'pinned': true,
          },
        ],
      },
      <String, Object?>{
        'id': 'sub-zt',
        'revision': 'rev-2',
        'name': 'Enterprise ZT Edges',
        'url': 'https://masque.pages.dev/endpoint/3rd/zt-ipv4.txt',
        'account_tier': 'zero_trust',
        'auto_refresh': 'off',
        'skipped_line_count': 0,
        'selected_endpoint_id': 'ep-zt1',
        'endpoints': <Map<String, Object?>>[
          <String, Object?>{
            'id': 'ep-zt1',
            'host': <String, Object?>{
              'kind': 'ipv4',
              'value': '162.159.197.42',
            },
            'port': 443,
            'note': 'Corp ZT Tokyo',
            'pinned': false,
          },
        ],
      },
    ]);
  engine.activeSubByTier['free'] = 'sub-free';
  engine.storedProfiles = [
    UsqueProfile.defaultProfile().copyWith(
      id: 'golden-acc',
      name: zeroTrustAccount ? 'Enterprise Org' : 'Personal Free',
      endpointSelection: EndpointSelection.custom,
      endpointSubscriptionId: zeroTrustAccount ? null : 'sub-free',
    ),
  ];
  engine.storedActiveProfileId = 'golden-acc';
  engine.storedIdentityStatuses = {
    'golden-acc': ProfileIdentityStatus(
      state: ProfileIdentityState.ready,
      provider: zeroTrustAccount
          ? IdentityProvider.zeroTrust
          : IdentityProvider.consumer,
      licenseState: zeroTrustAccount
          ? LicenseState.notApplicable
          : LicenseState.free,
    ),
  };
  return engine;
}

void main() {
  setUpAll(loadLinuxGoldenFonts);

  setUp(() {
    SharedPreferences.setMockInitialValues(<String, Object>{
      'onboarding_complete': true,
    });
  });

  final fixtures = [
    (
      name: 'profiles_subscriptions_desktop_en',
      size: const Size(1280, 900),
      locale: LocalePreference.english,
      dark: false,
      ztAccount: false,
    ),
    (
      name: 'profiles_subscriptions_phone_en_dark',
      size: const Size(390, 844),
      locale: LocalePreference.english,
      dark: true,
      ztAccount: false,
    ),
    (
      name: 'profiles_subscriptions_rtl_fa',
      size: const Size(960, 820),
      locale: LocalePreference.persian,
      dark: false,
      ztAccount: true,
    ),
  ];

  for (final fixture in fixtures) {
    testWidgets(
      fixture.name,
      (tester) async {
        final engine = _goldenEngine(zeroTrustAccount: fixture.ztAccount)
          ..legacyProfilesImported = true;
        addTearDown(engine.closeEvents);
        tester.view.devicePixelRatio = 1;
        tester.view.physicalSize = fixture.size;
        addTearDown(tester.view.resetDevicePixelRatio);
        addTearDown(tester.view.resetPhysicalSize);

        final app = AppController(engine);
        await app.initialize();
        app.localePreference = fixture.locale;
        app.selectSection(AppSection.profiles);
        app.setProfilesTab(ProfilesTab.subscriptions);
        addTearDown(app.dispose);

        await tester.pumpWidget(
          workflowHost(
            app,
            dark: fixture.dark,
            home: ShellScreen(controller: app),
          ),
        );
        await tester.pumpAndSettle();
        if (fixture.ztAccount) {
          await tester.tap(
            find.byKey(const ValueKey('subscription-chip-sub-free')),
          );
          await tester.pumpAndSettle();
        }

        // Populate deterministic RTT/status readouts in memory for desktop & phone views.
        app.endpointSubscriptions.acceptProbeProgress(<String, Object?>{
          'probe_id': 'golden-probe',
          'subscription_id': 'sub-free',
          'state': 'completed',
          'total': 3,
          'completed': 3,
          'results': <Map<String, Object?>>[
            <String, Object?>{
              'endpoint_id': 'ep-tokyo',
              'status': 'reachable',
              'protocol': 'http3',
              'rtt_ms': 42,
            },
            <String, Object?>{
              'endpoint_id': 'ep-osaka',
              'status': 'reachable',
              'protocol': 'http2',
              'rtt_ms': 185,
            },
            <String, Object?>{'endpoint_id': 'ep-dom', 'status': 'timed_out'},
          ],
        });
        await tester.pumpAndSettle();

        await expectLater(
          find.byType(ShellScreen),
          matchesGoldenFile('goldens/linux/${fixture.name}.png'),
        );
      },
      tags: const ['golden', 'linux-golden'],
      variant: const TargetPlatformVariant({TargetPlatform.linux}),
    );
  }
}
