import 'dart:async';
import 'dart:convert';

import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';
import 'package:shared_preferences/shared_preferences.dart';
import 'package:usque/app.dart';
import 'package:usque/core/app_strings.dart';
import 'package:usque/core/usque_theme.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/screens/advanced_settings_screen.dart';
import 'package:usque/screens/home_screen.dart';
import 'package:usque/screens/per_app_proxy_screen.dart';
import 'package:usque/screens/profiles_screen.dart';
import 'package:usque/screens/proxy_screen.dart';
import 'package:usque/screens/settings_screen.dart';
import 'package:usque/services/desktop_engine_client.dart';
import 'package:usque/services/engine_client.dart';
import 'package:usque/state/app_controller.dart';
import 'package:usque/widgets/common.dart';
import 'package:usque/widgets/connection_ring.dart';
import 'package:usque/widgets/controller_selector.dart';
import 'package:usque/widgets/profile_identity_dialog.dart';

class FakeEngineClient implements EngineClient {
  @override
  bool get supportsSnapshotEvents => false;

  @override
  Stream<EngineSnapshot> get snapshotEvents =>
      const Stream<EngineSnapshot>.empty();

  bool provisioned = false;
  IdentityProvisioningMethod? lastProvisioningMethod;
  String? lastZeroTrustTeam;
  String? lastZeroTrustCallback;
  bool failProfileIdentityCreation = false;
  final List<String> calls = <String>[];
  UsqueProfile? lastConnectedProfile;
  EngineSnapshot current = const EngineSnapshot();
  List<UsqueProfile> storedProfiles = <UsqueProfile>[
    UsqueProfile.defaultProfile(),
  ];
  String storedActiveProfileId = UsqueProfile.defaultProfileId;
  bool legacyProfilesImported = false;
  Map<String, ProfileIdentityStatus> storedIdentityStatuses =
      <String, ProfileIdentityStatus>{};

  @override
  Future<ProfileCatalog> importLegacyProfiles(
    List<UsqueProfile> profiles,
    String activeProfileId,
  ) async {
    if (!legacyProfilesImported) {
      if (profiles.isNotEmpty) {
        storedProfiles = List<UsqueProfile>.from(profiles);
        storedActiveProfileId = activeProfileId;
      }
      legacyProfilesImported = true;
    }
    return ProfileCatalog(
      profiles: List<UsqueProfile>.unmodifiable(storedProfiles),
      activeProfileId: storedActiveProfileId,
      identityStates: <String, ProfileIdentityState>{
        for (final profile in storedProfiles)
          profile.id: ProfileIdentityState.ready,
      },
      identityStatuses: storedIdentityStatuses,
    );
  }

  bool _preservesEndpoint(String id) =>
      storedIdentityStatuses[id]?.provider == IdentityProvider.zeroTrust;

  UsqueProfile _hydrate(UsqueProfile account, UsqueProfile network) {
    final keepEndpoint = _preservesEndpoint(account.id);
    return network.copyWith(
      id: account.id,
      name: account.name,
      endpointIpv4: keepEndpoint ? account.endpointIpv4 : network.endpointIpv4,
      endpointIpv6: keepEndpoint ? account.endpointIpv6 : network.endpointIpv6,
      endpointPort: keepEndpoint ? account.endpointPort : network.endpointPort,
      sni: keepEndpoint ? account.sni : network.sni,
    );
  }

  UsqueProfile _currentNetwork(UsqueProfile fallback) {
    if (storedProfiles.isEmpty) {
      return fallback;
    }
    final source = storedProfiles.firstWhere(
      (stored) => !_preservesEndpoint(stored.id),
      orElse: () => storedProfiles.first,
    );
    if (_preservesEndpoint(source.id)) {
      return source.copyWith(
        endpointIpv4: UsqueProfile.defaultEndpointIpv4,
        endpointIpv6: UsqueProfile.defaultEndpointIpv6,
        endpointPort: UsqueProfile.defaultEndpointPort,
        sni: UsqueProfile.defaultSni,
      );
    }
    return source;
  }

  @override
  Future<void> upsertProfile(UsqueProfile profile) async {
    final index = storedProfiles.indexWhere(
      (stored) => stored.id == profile.id,
    );
    if (index < 0) {
      storedProfiles = <UsqueProfile>[
        ...storedProfiles,
        _hydrate(profile, _currentNetwork(profile)),
      ];
      return;
    }
    final current = _currentNetwork(profile);
    final network = _preservesEndpoint(profile.id)
        ? profile.copyWith(
            endpointIpv4: current.endpointIpv4,
            endpointIpv6: current.endpointIpv6,
            endpointPort: current.endpointPort,
            sni: current.sni,
          )
        : profile;
    storedProfiles = storedProfiles
        .map((stored) {
          final account = stored.id == profile.id
              ? stored.copyWith(name: profile.name)
              : stored;
          return _hydrate(account, network);
        })
        .toList(growable: false);
  }

  @override
  Future<void> deleteProfile(String profileId) async {
    storedProfiles = storedProfiles
        .where((profile) => profile.id != profileId)
        .toList(growable: false);
    if (storedActiveProfileId == profileId) {
      storedActiveProfileId = storedProfiles.first.id;
    }
  }

  @override
  Future<void> setActiveProfile(String profileId) async {
    storedActiveProfileId = profileId;
  }

  @override
  Future<void> provisionIdentity(
    UsqueProfile profile, {
    required IdentityProvisioningMethod method,
    String? licenseKey,
    String? teamName,
    String? callbackUri,
  }) async {
    calls.add('provision');
    if (failProfileIdentityCreation) {
      throw const EngineException(
        'REGISTRATION_FAILED',
        'Registration failed.',
      );
    }
    provisioned = true;
    lastProvisioningMethod = method;
    lastZeroTrustTeam = teamName;
    lastZeroTrustCallback = callbackUri;
    storedIdentityStatuses = <String, ProfileIdentityStatus>{
      ...storedIdentityStatuses,
      profile.id: switch (method) {
        IdentityProvisioningMethod.zeroTrust => ProfileIdentityStatus(
          state: ProfileIdentityState.ready,
          licenseState: LicenseState.notApplicable,
          accountType: 'Zero Trust',
          provider: IdentityProvider.zeroTrust,
          organization: teamName ?? '',
        ),
        IdentityProvisioningMethod.registerWithLicense =>
          const ProfileIdentityStatus(
            state: ProfileIdentityState.ready,
            licenseState: LicenseState.warpPlus,
            accountType: 'WARP+',
          ),
        _ => const ProfileIdentityStatus(
          state: ProfileIdentityState.ready,
          licenseState: LicenseState.free,
          accountType: 'Free',
        ),
      },
    };
  }

  @override
  Future<ProfileCatalog> createProfileWithIdentity(
    UsqueProfile profile, {
    required IdentityProvisioningMethod method,
    String? licenseKey,
    String? teamName,
    String? callbackUri,
  }) async {
    if (failProfileIdentityCreation) {
      throw const EngineException(
        'REGISTRATION_FAILED',
        'Registration failed.',
      );
    }
    if (method == IdentityProvisioningMethod.registerWithLicense &&
        (licenseKey == null || licenseKey.isEmpty)) {
      throw const EngineException(
        'INVALID_LICENSE_KEY',
        'A WARP License Key is required.',
      );
    }
    provisioned = true;
    lastProvisioningMethod = method;
    lastZeroTrustTeam = teamName;
    lastZeroTrustCallback = callbackUri;
    storedProfiles = <UsqueProfile>[...storedProfiles, profile];
    return ProfileCatalog(
      profiles: List<UsqueProfile>.unmodifiable(storedProfiles),
      activeProfileId: storedActiveProfileId,
      identityStates: <String, ProfileIdentityState>{
        for (final stored in storedProfiles)
          stored.id: ProfileIdentityState.ready,
      },
      identityStatuses: storedIdentityStatuses,
    );
  }

  @override
  Future<void> reconfigureActiveProfile(UsqueProfile profile) =>
      upsertProfile(profile);

  @override
  Future<void> copyLicenseKey(String profileId) async {}

  @override
  Future<void> updateLicenseKey(String profileId, String licenseKey) async {}

  String? lastProxyAuthUsername;
  String? lastProxyAuthPassword;

  @override
  Future<void> updateProxyAuth(
    String profileId, {
    required String username,
    required String password,
    bool confirmed = true,
  }) async {
    calls.add('updateProxyAuth');
    if (!confirmed) {
      throw const EngineException(
        'CONFIRMATION_REQUIRED',
        'Saving listener credentials requires confirmation.',
      );
    }
    if (username.isNotEmpty && password.isEmpty) {
      throw const EngineException(
        'CONFIGURATION_INVALID',
        'proxy username requires a password',
      );
    }
    lastProxyAuthUsername = username;
    lastProxyAuthPassword = password;
    storedProfiles = storedProfiles
        .map(
          (profile) => profile.copyWith(
            proxy: profile.proxy.copyWith(authUsername: username),
          ),
        )
        .toList(growable: false);
  }

  @override
  Future<void> unbindLicenseKey(String profileId) async {}

  @override
  Future<String?> exportWarpSecret(String profileId) async =>
      'test-warp-secret.json';

  @override
  Future<String?> consumeLaunchTarget() async => null;

  @override
  Future<String?> beginZeroTrustLogin(String teamName) async => null;

  @override
  Future<String?> consumeZeroTrustCallback() async => null;

  @override
  Future<void> cancelZeroTrustLogin() async {}

  @override
  Future<PlatformPreferences> platformPreferences() async =>
      const PlatformPreferences();

  @override
  Future<void> setStartOnBoot(bool enabled) async {}

  @override
  Future<void> setCloseToTray(bool enabled) async {}

  @override
  Future<void> setWarpProtocolAssociation(bool enabled) async {
    calls.add('setWarpProtocolAssociation');
  }

  @override
  Future<void> requestAddQuickSettingsTile() async {}

  PerAppProxySettings storedPerAppProxy = const PerAppProxySettings();
  List<InstalledAppInfo> installedApps = const <InstalledAppInfo>[
    InstalledAppInfo(
      packageName: 'com.example.browser',
      label: 'Browser',
      isSystem: false,
      hasInternet: true,
    ),
    InstalledAppInfo(
      packageName: 'com.example.mail',
      label: 'Mail',
      isSystem: false,
      hasInternet: true,
    ),
    InstalledAppInfo(
      packageName: 'com.android.settings',
      label: 'Settings',
      isSystem: true,
      hasInternet: true,
    ),
  ];

  @override
  Future<PerAppProxySettings> perAppProxy() async => storedPerAppProxy;

  @override
  Future<PerAppProxySettings> setPerAppProxy(
    PerAppProxySettings settings,
  ) async {
    calls.add('setPerAppProxy');
    final error = settings.validationError();
    if (error != null) {
      throw EngineException(error, 'Invalid per-app proxy settings.');
    }
    storedPerAppProxy = PerAppProxySettings(
      enabled: settings.enabled,
      packageNames: PerAppProxySettings.sanitizePackages(settings.packageNames),
    );
    return storedPerAppProxy;
  }

  @override
  Future<List<InstalledAppInfo>> listInstalledApps() async => installedApps;

  @override
  Future<Uint8List?> getAppIcon(String packageName) async => null;

  @override
  Future<void> openAlwaysOnVpnSettings() async {
    calls.add('openAlwaysOnVpnSettings');
  }

  @override
  Future<EngineSnapshot> connect(UsqueProfile profile) async {
    calls.add('connect');
    lastConnectedProfile = profile;
    current = EngineSnapshot(
      phase: ConnectionPhase.connected,
      transport: 'HTTP/3',
      addressFamily: 'IPv6',
      connectedAt: DateTime.now(),
    );
    return current;
  }

  @override
  Future<EngineSnapshot> disconnect() async {
    calls.add('disconnect');
    current = const EngineSnapshot();
    return current;
  }

  @override
  Future<EngineSnapshot> retry() async {
    calls.add('retry');
    return connect(lastConnectedProfile ?? storedProfiles.first);
  }

  @override
  Future<EngineSnapshot> snapshot() async => current;

  @override
  Future<String?> exportDiagnostics() async => 'test-diagnostics.zip';

  @override
  Future<UpdateCheckResult> checkForUpdates({bool manual = true}) async =>
      const UpdateCheckResult.current();

  @override
  Future<void> clearAllData({required bool confirmed}) async {
    if (!confirmed) {
      throw const EngineException(
        'CONFIRMATION_REQUIRED',
        'Confirmation is required.',
      );
    }
    current = const EngineSnapshot();
    storedProfiles = <UsqueProfile>[UsqueProfile.defaultProfile()];
    storedActiveProfileId = UsqueProfile.defaultProfileId;
    legacyProfilesImported = false;
    provisioned = false;
    storedPerAppProxy = const PerAppProxySettings();
  }

  @override
  void dispose() {}
}

class EventEngineClient extends FakeEngineClient {
  final List<StreamController<EngineSnapshot>> eventControllers =
      <StreamController<EngineSnapshot>>[];
  bool subscribedAfterProfileImport = false;

  @override
  bool get supportsSnapshotEvents => true;

  @override
  Stream<EngineSnapshot> get snapshotEvents {
    subscribedAfterProfileImport = legacyProfilesImported;
    final controller = StreamController<EngineSnapshot>.broadcast();
    eventControllers.add(controller);
    return controller.stream;
  }

  @override
  void dispose() {
    for (final controller in eventControllers) {
      if (!controller.isClosed) {
        unawaited(controller.close());
      }
    }
  }
}

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();

  test('Windows profile defaults match the product contract', () {
    debugDefaultTargetPlatformOverride = TargetPlatform.windows;
    try {
      final profile = UsqueProfile.defaultProfile();
      expect(profile.endpointIpv4, '162.159.198.2');
      expect(profile.endpointIpv6, '2606:4700:103::2');
      expect(profile.endpointPort, 443);
      expect(profile.sni, 'speed.cloudflare.com');
      expect(profile.mtu, 1280);
      expect(profile.proxy.socksPort, 1080);
      expect(profile.proxy.httpPort, 8080);
      expect(profile.killSwitch, isTrue);
      expect(profile.proxy.exposesLan, isFalse);
      expect(profile.proxy.dnsMode, ProxyDnsMode.remote);
      expect(profile.proxy.dnsIpv4, '1.1.1.1');
      expect(profile.proxy.dnsIpv6, '2606:4700:4700::1111');
      expect(profile.frontends.tunnel, isTrue);
      expect(profile.frontends.socks5, isTrue);
      expect(profile.frontends.http, isTrue);
      expect(profile.proxy.systemProxy, isFalse);
    } finally {
      debugDefaultTargetPlatformOverride = null;
    }
  });

  test('Android profile output defaults remain unchanged', () {
    debugDefaultTargetPlatformOverride = TargetPlatform.android;
    try {
      final profile = UsqueProfile.defaultProfile();
      expect(profile.frontends.tunnel, isTrue);
      expect(profile.frontends.socks5, isTrue);
      expect(profile.frontends.http, isTrue);
      expect(profile.proxy.systemProxy, isFalse);
    } finally {
      debugDefaultTargetPlatformOverride = null;
    }
  });

  test('Android snapshots preserve structured errors and compare by value', () {
    final first = EngineSnapshot.fromMap(<Object?, Object?>{
      'phase': 'error',
      'warning': '127.0.0.1:1080 is already in use',
      'error_code': 'PROXY_LISTEN_FAILED',
      'active_listeners': <String>[],
    });
    final second = EngineSnapshot.fromMap(<Object?, Object?>{
      'phase': 'error',
      'warning': '127.0.0.1:1080 is already in use',
      'error_code': 'PROXY_LISTEN_FAILED',
      'active_listeners': <String>[],
    });

    expect(first, second);
    expect(first.hashCode, second.hashCode);
    expect(first.errorCode, 'PROXY_LISTEN_FAILED');
  });

  testWidgets('controller selectors ignore unrelated engine statistics', (
    tester,
  ) async {
    SharedPreferences.setMockInitialValues(<String, Object>{});
    final engine = EventEngineClient();
    final controller = AppController(engine);
    await controller.initialize();
    var builds = 0;

    await tester.pumpWidget(
      MaterialApp(
        theme: UsqueTheme.light(),
        home: ControllerSelector<ThemePreference>(
          controller: controller,
          selector: (controller) => controller.themePreference,
          builder: (context, value) {
            builds += 1;
            return Text(value.name);
          },
        ),
      ),
    );
    expect(builds, 1);

    engine.eventControllers.last.add(
      const EngineSnapshot(
        phase: ConnectionPhase.connected,
        downloadBytesPerSecond: 42,
      ),
    );
    await tester.pump();
    expect(builds, 1);

    await controller.setTheme(ThemePreference.dark);
    await tester.pump();
    expect(builds, 2);
    controller.dispose();
  });

  testWidgets('structured Android errors surface once with their error code', (
    tester,
  ) async {
    SharedPreferences.setMockInitialValues(<String, Object>{});
    final engine = EventEngineClient();
    final controller = AppController(engine);
    await controller.initialize();
    var notifications = 0;
    controller.addListener(() => notifications += 1);
    const failure = EngineSnapshot(
      phase: ConnectionPhase.error,
      warning: '127.0.0.1:1080 is already in use',
      errorCode: 'PROXY_LISTEN_FAILED',
    );

    engine.eventControllers.last.add(failure);
    await tester.pump();
    expect(
      controller.lastError,
      'PROXY_LISTEN_FAILED: 127.0.0.1:1080 is already in use',
    );
    final notificationsAfterFirstError = notifications;

    engine.eventControllers.last.add(failure);
    await tester.pump();
    expect(notifications, notificationsAfterFirstError);
    controller.dispose();
  });

  testWidgets(
    'status stream failures use polling and recover without a connection error',
    (tester) async {
      SharedPreferences.setMockInitialValues(<String, Object>{});
      final engine = EventEngineClient();
      final controller = AppController(engine);
      await controller.initialize();

      expect(engine.subscribedAfterProfileImport, isTrue);
      expect(engine.eventControllers, hasLength(1));
      expect(controller.lastError, isNull);

      engine.current = const EngineSnapshot(
        phase: ConnectionPhase.connected,
        transport: 'HTTP/3',
        addressFamily: 'IPv4',
      );
      engine.eventControllers.single.addError(
        PlatformException(
          code: 'ENGINE_EVENT_UNAVAILABLE',
          message: 'test stream failure',
        ),
      );
      await tester.pump();

      expect(controller.snapshotStreamDegraded, isTrue);
      expect(controller.lastError, isNull);

      await tester.pump(const Duration(seconds: 1));
      await tester.pump();
      expect(controller.snapshot.phase, ConnectionPhase.connected);
      expect(engine.eventControllers.length, greaterThanOrEqualTo(2));

      engine.eventControllers.last.add(
        const EngineSnapshot(
          phase: ConnectionPhase.connected,
          transport: 'HTTP/2',
          addressFamily: 'IPv4',
        ),
      );
      await tester.pump();

      expect(controller.snapshotStreamDegraded, isFalse);
      expect(controller.snapshot.transport, 'HTTP/2');
      expect(controller.lastError, isNull);
      controller.dispose();
      await tester.pump();
    },
  );

  test('English and Simplified Chinese catalogs contain identical keys', () {
    expect(AppStrings.debugCatalogsAreComplete, isTrue);
  });

  test(
    'desktop protobuf framing stays compatible with the Rust v1 snapshot',
    () {
      expect(debugEncodeGetStatusFrame('r1'), <int>[
        0,
        0,
        0,
        6,
        0x0a,
        2,
        0x72,
        0x31,
        0x52,
        0,
      ]);
      final snapshot = debugDecodeStatusFrame(
        Uint8List.fromList(<int>[
          0,
          0,
          0,
          8,
          0x0a,
          2,
          0x72,
          0x31,
          0x5a,
          2,
          0x08,
          1,
        ]),
        'r1',
      );
      expect(snapshot.phase, ConnectionPhase.disconnected);
    },
  );

  test('desktop event bridge filters metadata and decodes state frames', () {
    expect(
      debugDecodeEventSnapshot(
        Uint8List.fromList(<int>[0, 0, 0, 4, 0x08, 1, 0x72, 0]),
      ),
      isNull,
    );

    final snapshot = debugDecodeEventSnapshot(
      Uint8List.fromList(<int>[
        0,
        0,
        0,
        18,
        0x08,
        7,
        0x52,
        14,
        0x0a,
        12,
        0x08,
        5,
        0x12,
        2,
        0x68,
        0x33,
        0x1a,
        4,
        0x69,
        0x70,
        0x76,
        0x36,
      ]),
    );

    expect(snapshot, isNotNull);
    expect(snapshot!.phase, ConnectionPhase.connected);
    expect(snapshot.transport, 'h3');
    expect(snapshot.addressFamily, 'ipv6');
  });

  test('desktop protobuf bridge preserves structured engine errors', () {
    final frame = Uint8List.fromList(<int>[
      0,
      0,
      0,
      18,
      0x0a,
      2,
      0x72,
      0x31,
      0x12,
      12,
      0x0a,
      1,
      0x45,
      0x12,
      7,
      0x62,
      0x6c,
      0x6f,
      0x63,
      0x6b,
      0x65,
      0x64,
    ]);

    expect(
      () => debugDecodeStatusFrame(frame, 'r1'),
      throwsA(
        isA<EngineException>()
            .having((error) => error.code, 'code', 'E')
            .having((error) => error.message, 'message', 'blocked'),
      ),
    );
  });

  test('desktop protobuf bridge decodes the authoritative profile catalog', () {
    final catalog = debugDecodeProfileCatalogFrame(
      Uint8List.fromList(<int>[
        0,
        0,
        0,
        17,
        0x0a,
        2,
        0x72,
        0x32,
        0x62,
        11,
        0x0a,
        6,
        0x0a,
        1,
        0x70,
        0x12,
        1,
        0x58,
        0x12,
        1,
        0x70,
      ]),
      'r2',
    );

    expect(catalog.activeProfileId, 'p');
    expect(catalog.profiles, hasLength(1));
    expect(catalog.profiles.single.name, 'X');
    expect(catalog.profiles.single.killSwitch, isTrue);
  });

  test('non-loopback proxy address is treated as LAN exposure', () {
    const settings = ProxySettings(socksIpv4: '0.0.0.0');
    expect(settings.exposesLan, isTrue);
  });

  testWidgets(
    'custom proxy DNS reveals Cloudflare address fields and saves valid edits',
    (tester) async {
      SharedPreferences.setMockInitialValues(<String, Object>{});
      final engine = FakeEngineClient();
      final controller = AppController(engine);
      await controller.initialize();
      controller.updateProfile(
        controller.activeProfile.copyWith(
          dnsIpv4: '8.8.8.8',
          dnsIpv6: '2001:4860:4860::8888',
        ),
      );
      await controller.flushProfileWrites();
      await controller.setLocale(LocalePreference.simplifiedChinese);
      addTearDown(controller.dispose);
      addTearDown(() => tester.view.resetPhysicalSize());
      addTearDown(() => tester.view.resetDevicePixelRatio());
      tester.view.devicePixelRatio = 1;
      tester.view.physicalSize = const Size(1200, 900);

      await tester.pumpWidget(
        MaterialApp(
          theme: UsqueTheme.light(),
          home: ListenableBuilder(
            listenable: controller,
            builder: (context, _) => ProxyScreen(controller: controller),
          ),
        ),
      );
      await tester.pumpAndSettle();

      expect(
        find.byKey(const ValueKey<String>('proxy-dns-ipv4')),
        findsNothing,
      );
      expect(
        find.byKey(const ValueKey<String>('proxy-dns-ipv6')),
        findsNothing,
      );

      await tester.tap(find.byKey(const ValueKey<String>('proxy-dns-mode')));
      await tester.pumpAndSettle();
      await tester.tap(find.text('自定义 DNS 服务器').last);
      await tester.pumpAndSettle();

      final ipv4 = find.byKey(const ValueKey<String>('proxy-dns-ipv4'));
      final ipv6 = find.byKey(const ValueKey<String>('proxy-dns-ipv6'));
      expect(ipv4, findsOneWidget);
      expect(ipv6, findsOneWidget);
      expect(tester.widget<TextField>(ipv4).controller?.text, '1.1.1.1');
      expect(
        tester.widget<TextField>(ipv6).controller?.text,
        '2606:4700:4700::1111',
      );

      await tester.enterText(ipv4, '9.9.9.9');
      await tester.pump();
      await controller.flushProfileWrites();

      expect(
        controller.activeProfile.proxy.dnsMode,
        ProxyDnsMode.localConfigured,
      );
      expect(controller.activeProfile.proxy.dnsIpv4, '9.9.9.9');
      expect(controller.activeProfile.dnsIpv4, '8.8.8.8');
      expect(engine.storedProfiles.single.proxy.dnsIpv4, '9.9.9.9');
    },
  );

  test('proxy profile JSON stores username and never password bytes', () {
    final profile = UsqueProfile.defaultProfile().copyWith(
      proxy: const ProxySettings(authUsername: 'lan-user'),
    );
    final encoded = jsonEncode(profile.toMap());
    expect(profile.toMap()['proxy'], containsPair('auth_username', 'lan-user'));
    expect(encoded.contains('lan-user'), isTrue);
    expect(encoded.toLowerCase().contains('password'), isFalse);
  });

  test('profile model survives its versioned map representation', () {
    const profile = UsqueProfile(
      id: '4cf46553-86ea-4bf7-a283-dc26fa58ed79',
      name: 'Hotel Wi-Fi',
      mode: OperatingMode.socks5,
      transport: TransportPolicy.http2,
      ipPolicy: IpPolicy.preferIpv6,
      endpointIpv4: '192.0.2.1',
      endpointIpv6: '2001:db8::1',
      endpointPort: 8443,
      sni: 'example.com',
      mtu: 1400,
      dnsIpv4: '9.9.9.9',
      dnsIpv6: '2620:fe::fe',
      dnsMode: DnsMode.localConfigured,
      killSwitch: false,
      allowLan: true,
      autoConnect: true,
      bypassCidrs: <String>['192.168.0.0/16'],
      proxy: ProxySettings(
        dnsMode: ProxyDnsMode.system,
        dnsIpv4: '149.112.112.112',
        dnsIpv6: '2620:fe::9',
        systemProxy: true,
      ),
    );

    final restored = UsqueProfile.fromMap(profile.toMap());
    expect(restored.toMap(), profile.toMap());
  });

  test('proxy settings persist username and never serialize a password', () {
    const settings = ProxySettings(authUsername: 'lan-user');
    final map = settings.toMap();
    expect(map['auth_username'], 'lan-user');
    expect(map.keys, isNot(contains('password')));
    expect(map.keys, isNot(contains('auth_password')));
    expect(ProxySettings.fromMap(map).authUsername, 'lan-user');
    expect(settings.hasAuth, isTrue);
  });

  test('non-secret profiles persist across controller restarts', () async {
    SharedPreferences.setMockInitialValues(<String, Object>{});
    final engine = FakeEngineClient();
    final first = AppController(engine);
    await first.initialize();
    first.addProfile('Persistent');
    final persistent = first.profiles.last;
    first.updateProfile(
      persistent.copyWith(
        frontends: const FrontendSettings(
          tunnel: false,
          socks5: false,
          http: true,
        ),
        proxy: persistent.proxy.copyWith(dnsMode: ProxyDnsMode.system),
      ),
    );
    first.setActiveProfile(persistent.id);
    await first.flushProfileWrites();
    first.dispose();

    final second = AppController(engine);
    await second.initialize();
    expect(second.profiles, hasLength(2));
    expect(second.activeProfileId, persistent.id);
    expect(second.activeProfile.mode, OperatingMode.httpProxy);
    expect(second.activeProfile.proxy.dnsMode, ProxyDnsMode.system);
    expect(second.profiles.first.proxy.dnsMode, ProxyDnsMode.system);
    second.dispose();
  });

  test('network settings stay shared when switching accounts', () async {
    SharedPreferences.setMockInitialValues(<String, Object>{});
    final engine = FakeEngineClient();
    final controller = AppController(engine);
    await controller.initialize();
    controller.addProfile('Work');
    final work = controller.profiles.last;
    controller.updateProfile(
      controller.activeProfile.copyWith(
        mtu: 1400,
        autoConnect: true,
        frontends: controller.activeProfile.frontends.copyWith(tunnel: false),
      ),
    );
    controller.setActiveProfile(work.id);
    expect(controller.activeProfile.mtu, 1400);
    expect(controller.activeProfile.autoConnect, isTrue);
    expect(controller.activeProfile.frontends.tunnel, isFalse);
    expect(controller.activeProfile.mtu, 1400);
    controller.dispose();
  });

  test(
    'profile creation is committed only after identity provisioning',
    () async {
      SharedPreferences.setMockInitialValues(<String, Object>{});
      final engine = FakeEngineClient();
      final controller = AppController(engine);
      await controller.initialize();

      engine.failProfileIdentityCreation = true;
      expect(
        await controller.createProfileWithIdentity(
          'Rejected',
          method: IdentityProvisioningMethod.register,
        ),
        isFalse,
      );
      expect(controller.profiles, hasLength(1));
      expect(engine.storedProfiles, hasLength(1));

      engine.failProfileIdentityCreation = false;
      expect(
        await controller.createProfileWithIdentity(
          'Ready',
          method: IdentityProvisioningMethod.register,
        ),
        isTrue,
      );
      expect(controller.profiles, hasLength(2));
      expect(
        controller.identityState(controller.profiles.last.id),
        ProfileIdentityState.ready,
      );
      controller.dispose();
    },
  );

  test('disabling HTTP output disables the Windows system proxy', () async {
    SharedPreferences.setMockInitialValues(<String, Object>{});
    final engine = FakeEngineClient();
    final controller = AppController(engine);
    await controller.initialize();

    controller.updateProfile(
      controller.activeProfile.copyWith(
        frontends: controller.activeProfile.frontends.copyWith(http: true),
        proxy: controller.activeProfile.proxy.copyWith(systemProxy: true),
      ),
    );
    await controller.flushProfileWrites();
    controller.updateProfile(
      controller.activeProfile.copyWith(
        frontends: controller.activeProfile.frontends.copyWith(http: false),
      ),
    );
    await controller.flushProfileWrites();

    expect(controller.activeProfile.frontends.http, isFalse);
    expect(controller.activeProfile.proxy.systemProxy, isFalse);
    expect(engine.storedProfiles.single.proxy.systemProxy, isFalse);
    controller.dispose();
  });

  testWidgets('new profile accepts a manual Zero Trust callback and clears it', (
    tester,
  ) async {
    tester.view.devicePixelRatio = 1;
    tester.view.physicalSize = const Size(1280, 900);
    addTearDown(tester.view.resetDevicePixelRatio);
    addTearDown(tester.view.resetPhysicalSize);
    SharedPreferences.setMockInitialValues(<String, Object>{
      'onboarding_complete': true,
    });
    final engine = FakeEngineClient();

    await tester.pumpWidget(UsqueBootstrap(engine: engine));
    await tester.pumpAndSettle();
    await tester.tap(
      find.descendant(
        of: find.byType(NavigationRail),
        matching: find.text('Profiles'),
      ),
    );
    await tester.pumpAndSettle();
    await tester.tap(find.text('New profile'));
    await tester.pumpAndSettle();
    await tester.enterText(find.byType(TextField), 'Organization');
    await tester.tap(find.text('Continue'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Cloudflare Zero Trust'));
    await tester.pumpAndSettle();

    final callback =
        'com.cloudflare.warp://example-team.cloudflareaccess.com/auth?token=test-assertion';
    expect(find.byType(TextField), findsNWidgets(2));
    await tester.enterText(find.byType(TextField).at(0), 'Example-Team');
    await tester.enterText(find.byType(TextField).at(1), callback);
    expect(find.text('Organization callback received securely.'), findsNothing);
    await tester.tap(find.text('Create'));
    await tester.pumpAndSettle();

    expect(engine.lastProvisioningMethod, IdentityProvisioningMethod.zeroTrust);
    expect(engine.lastZeroTrustTeam, 'example-team');
    expect(engine.lastZeroTrustCallback, callback);
    expect(find.text('Complete callback URL'), findsNothing);
    expect(tester.takeException(), isNull);
  });

  testWidgets('invalid Zero Trust callback does not start registration', (
    tester,
  ) async {
    tester.view.devicePixelRatio = 1;
    tester.view.physicalSize = const Size(1280, 900);
    addTearDown(tester.view.resetDevicePixelRatio);
    addTearDown(tester.view.resetPhysicalSize);
    SharedPreferences.setMockInitialValues(<String, Object>{
      'onboarding_complete': true,
    });
    final engine = FakeEngineClient();

    await tester.pumpWidget(UsqueBootstrap(engine: engine));
    await tester.pumpAndSettle();
    await tester.tap(
      find.descendant(
        of: find.byType(NavigationRail),
        matching: find.text('Profiles'),
      ),
    );
    await tester.pumpAndSettle();
    await tester.tap(find.text('New profile'));
    await tester.pumpAndSettle();
    await tester.enterText(find.byType(TextField), 'Organization');
    await tester.tap(find.text('Continue'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Cloudflare Zero Trust'));
    await tester.pumpAndSettle();
    await tester.enterText(find.byType(TextField).at(0), 'example-team');
    await tester.enterText(
      find.byType(TextField).at(1),
      'https://example-team.cloudflareaccess.com/auth?token=x',
    );
    await tester.tap(find.text('Create'));
    await tester.pumpAndSettle();

    expect(engine.lastProvisioningMethod, isNull);
    expect(engine.lastZeroTrustCallback, isNull);
    expect(
      find.text(
        'Use a com.cloudflare.warp Access callback for this organization.',
      ),
      findsOneWidget,
    );
    expect(find.text('Complete callback URL'), findsOneWidget);
  });

  test('connected Zero Trust repair disconnects and reconnects', () async {
    SharedPreferences.setMockInitialValues(<String, Object>{
      'onboarding_complete': true,
    });
    final engine = FakeEngineClient();
    final controller = AppController(engine);
    await controller.initialize();
    controller.snapshot = const EngineSnapshot(
      phase: ConnectionPhase.connected,
    );
    engine.calls.clear();

    final success = await controller.provisionProfileIdentity(
      controller.activeProfile,
      method: IdentityProvisioningMethod.zeroTrust,
      teamName: 'example-team',
      callbackUri:
          'com.cloudflare.warp://example-team.cloudflareaccess.com/auth?token=test',
    );

    expect(success, isTrue);
    expect(engine.calls, <String>['disconnect', 'provision', 'connect']);
    expect(engine.lastConnectedProfile?.id, controller.activeProfile.id);
    controller.dispose();
  });

  testWidgets('Zero Trust registered endpoint fields are read only', (
    tester,
  ) async {
    SharedPreferences.setMockInitialValues(<String, Object>{
      'onboarding_complete': true,
    });
    final engine = FakeEngineClient();
    engine.storedIdentityStatuses = <String, ProfileIdentityStatus>{
      UsqueProfile.defaultProfileId: const ProfileIdentityStatus(
        state: ProfileIdentityState.ready,
        licenseState: LicenseState.notApplicable,
        accountType: 'Zero Trust',
        provider: IdentityProvider.zeroTrust,
        organization: 'example-team',
      ),
    };
    final controller = AppController(engine);
    await controller.initialize();
    addTearDown(controller.dispose);

    await tester.pumpWidget(
      MaterialApp(
        theme: UsqueTheme.light(),
        home: AdvancedSettingsScreen(controller: controller),
      ),
    );
    await tester.pumpAndSettle();

    expect(
      find.text(
        'This endpoint is managed by the Zero Trust device registration and cannot be edited here.',
      ),
      findsOneWidget,
    );
    final endpointFields = tester
        .widgetList<TextField>(find.byType(TextField))
        .take(4);
    expect(endpointFields.every((field) => field.readOnly), isTrue);
  });

  testWidgets('Zero Trust identity choice remains readable on a narrow phone', (
    tester,
  ) async {
    tester.view.devicePixelRatio = 1;
    tester.view.physicalSize = const Size(360, 800);
    addTearDown(tester.view.resetDevicePixelRatio);
    addTearDown(tester.view.resetPhysicalSize);
    SharedPreferences.setMockInitialValues(<String, Object>{
      'onboarding_complete': true,
    });
    final controller = AppController(FakeEngineClient());
    await controller.initialize();
    addTearDown(controller.dispose);

    await tester.pumpWidget(
      MaterialApp(
        theme: UsqueTheme.light(),
        home: Scaffold(
          body: Builder(
            builder: (context) => TextButton(
              onPressed: () =>
                  showProfileIdentityDialog(context, controller: controller),
              child: const Text('Open'),
            ),
          ),
        ),
      ),
    );
    await tester.tap(find.text('Open'));
    await tester.pumpAndSettle();
    await tester.enterText(find.byType(TextField), 'Organization');
    await tester.tap(find.text('Continue'));
    await tester.pumpAndSettle();

    final title = find.text('Cloudflare Zero Trust');
    expect(title, findsOneWidget);
    expect(tester.getSize(title).width, greaterThan(100));
    expect(tester.getSize(title).height, lessThan(72));
    expect(find.text('Experimental'), findsOneWidget);
    expect(tester.takeException(), isNull);
  });

  test('corrupt profile data is backed up and reset safely', () async {
    SharedPreferences.setMockInitialValues(<String, Object>{
      'profiles_v1': '{"schema_version":1,"profiles":"broken"}',
    });
    final controller = AppController(FakeEngineClient());
    await controller.initialize();
    await controller.flushProfileWrites();
    final preferences = await SharedPreferences.getInstance();

    expect(controller.profiles, hasLength(1));
    expect(controller.activeProfileId, UsqueProfile.defaultProfileId);
    expect(controller.lastError, isNotNull);
    expect(preferences.getString('profiles_v1_corrupt_backup'), isNotNull);
    controller.dispose();
  });

  test('clear all data resets profiles, preferences, and onboarding', () async {
    SharedPreferences.setMockInitialValues(<String, Object>{
      'onboarding_complete': true,
      'theme': 'dark',
      'locale': 'simplifiedChinese',
    });
    final engine = FakeEngineClient()..provisioned = true;
    final controller = AppController(engine);
    await controller.initialize();
    controller.addProfile('Temporary');
    await controller.flushProfileWrites();

    await controller.clearAllData();

    final preferences = await SharedPreferences.getInstance();
    expect(controller.onboardingComplete, isFalse);
    expect(controller.profiles, hasLength(1));
    expect(controller.activeProfileId, UsqueProfile.defaultProfileId);
    expect(controller.themePreference, ThemePreference.system);
    expect(controller.localePreference, LocalePreference.system);
    expect(engine.provisioned, isFalse);
    expect(preferences.getKeys(), isEmpty);
    controller.dispose();
  });

  testWidgets('onboarding provisions an identity before opening the shell', (
    tester,
  ) async {
    SharedPreferences.setMockInitialValues(<String, Object>{});
    final engine = FakeEngineClient();
    await tester.pumpWidget(UsqueBootstrap(engine: engine));
    await tester.pumpAndSettle();

    expect(find.text('Welcome to Usque'), findsOneWidget);

    await tester.tap(find.text('Continue'));
    await tester.pumpAndSettle();
    expect(find.text('System permissions'), findsWidgets);

    await tester.tap(find.text('Continue'));
    await tester.pumpAndSettle();
    expect(find.text('Cloudflare terms'), findsOneWidget);

    await tester.tap(find.byType(CheckboxListTile));
    await tester.pump();
    await tester.tap(find.text('Continue'));
    await tester.pumpAndSettle();
    expect(find.text('Set up Consumer WARP'), findsOneWidget);

    await tester.tap(find.text('Finish setup'));
    await tester.pumpAndSettle();

    expect(engine.provisioned, isTrue);
    expect(find.text('Home'), findsWidgets);
    expect(find.text('Default'), findsOneWidget);
  });

  testWidgets('connect button reflects a real engine snapshot', (tester) async {
    SharedPreferences.setMockInitialValues(<String, Object>{
      'onboarding_complete': true,
    });
    final engine = FakeEngineClient();
    await tester.pumpWidget(UsqueBootstrap(engine: engine));
    await tester.pumpAndSettle();

    await tester.tap(find.text('Connect'));
    await tester.pumpAndSettle();

    expect(find.text('Connected'), findsOneWidget);
    expect(find.text('HTTP/3'), findsOneWidget);
    expect(find.text('IPv6'), findsWidgets);
    expect(find.text('Disconnect'), findsOneWidget);
  });

  testWidgets('home renders without layout errors in a wide desktop window', (
    tester,
  ) async {
    tester.view.devicePixelRatio = 1;
    tester.view.physicalSize = const Size(1600, 900);
    addTearDown(tester.view.resetDevicePixelRatio);
    addTearDown(tester.view.resetPhysicalSize);
    SharedPreferences.setMockInitialValues(<String, Object>{
      'onboarding_complete': true,
    });

    await tester.pumpWidget(UsqueBootstrap(engine: FakeEngineClient()));
    await tester.pumpAndSettle();

    expect(tester.takeException(), isNull);
    expect(find.text('Home'), findsWidgets);
    expect(find.text('Engine status'), findsOneWidget);
    expect(find.text('Connect'), findsOneWidget);
  });

  testWidgets('narrow home uses safe areas and the compact Usque brand', (
    tester,
  ) async {
    tester.view.devicePixelRatio = 1;
    tester.view.physicalSize = const Size(430, 900);
    addTearDown(tester.view.resetDevicePixelRatio);
    addTearDown(tester.view.resetPhysicalSize);
    SharedPreferences.setMockInitialValues(<String, Object>{
      'onboarding_complete': true,
    });

    await tester.pumpWidget(UsqueBootstrap(engine: FakeEngineClient()));
    await tester.pumpAndSettle();

    expect(find.byType(SafeArea), findsAtLeastNWidgets(2));
    expect(find.text('Usque'), findsOneWidget);
    expect(
      find.byWidgetPredicate(
        (widget) =>
            widget is Image &&
            widget.image is AssetImage &&
            (widget.image as AssetImage).assetName ==
                'assets/branding/usque-ui-icon.png' &&
            widget.width == 40,
      ),
      findsOneWidget,
    );
    expect(tester.takeException(), isNull);
  });

  testWidgets(
    'profile dialogs survive repeated exit paths while status events arrive',
    (tester) async {
      tester.view.devicePixelRatio = 1;
      tester.view.physicalSize = const Size(1280, 800);
      addTearDown(tester.view.resetDevicePixelRatio);
      addTearDown(tester.view.resetPhysicalSize);
      SharedPreferences.setMockInitialValues(<String, Object>{
        'onboarding_complete': true,
      });
      final engine = EventEngineClient();

      await tester.pumpWidget(UsqueBootstrap(engine: engine));
      await tester.pumpAndSettle();
      await tester.tap(
        find.descendant(
          of: find.byType(NavigationRail),
          matching: find.text('Profiles'),
        ),
      );
      await tester.pumpAndSettle();

      Future<void> injectStatus(int index) async {
        engine.eventControllers.last.add(
          EngineSnapshot(
            phase: ConnectionPhase.connected,
            downloadBytesPerSecond: index + 1,
          ),
        );
        await tester.pump();
        expect(tester.takeException(), isNull);
      }

      for (var index = 0; index < 50; index += 1) {
        await tester.tap(find.text('New profile'));
        await tester.pumpAndSettle();
        await injectStatus(index);
        await tester.tap(find.text('Cancel'));
        await tester.pumpAndSettle();
      }

      for (var index = 0; index < 50; index += 1) {
        await tester.tap(find.text('New profile'));
        await tester.pumpAndSettle();
        await tester.enterText(find.byType(TextField), 'Created $index');
        await tester.tap(find.text('Continue'));
        await tester.pumpAndSettle();
        await injectStatus(index + 50);
        await tester.tap(find.text('Create'));
        await tester.pumpAndSettle();
      }

      for (var index = 0; index < 50; index += 1) {
        await tester.tap(find.byTooltip('Edit').first);
        await tester.pumpAndSettle();
        await tester.enterText(find.byType(TextField), 'Edited $index');
        await injectStatus(index + 100);
        await tester.tap(find.text('Save'));
        await tester.pumpAndSettle();
      }

      for (var index = 0; index < 50; index += 1) {
        await tester.tap(find.byTooltip('Edit').first);
        await tester.pumpAndSettle();
        await tester.enterText(find.byType(TextField), 'Enter $index');
        await injectStatus(index + 150);
        await tester.testTextInput.receiveAction(TextInputAction.done);
        await tester.pumpAndSettle();
      }

      expect(tester.takeException(), isNull);
    },
  );

  testWidgets(
    'Simplified Chinese dark theme renders at 200 percent on a TV viewport',
    (tester) async {
      tester.view.devicePixelRatio = 1;
      tester.view.physicalSize = const Size(1280, 720);
      tester.platformDispatcher.textScaleFactorTestValue = 2;
      addTearDown(tester.view.resetDevicePixelRatio);
      addTearDown(tester.view.resetPhysicalSize);
      addTearDown(tester.platformDispatcher.clearTextScaleFactorTestValue);
      SharedPreferences.setMockInitialValues(<String, Object>{
        'onboarding_complete': true,
        'theme': 'dark',
        'locale': 'simplifiedChinese',
      });

      await tester.pumpWidget(UsqueBootstrap(engine: FakeEngineClient()));
      await tester.pumpAndSettle();

      expect(tester.takeException(), isNull);
      expect(find.byType(NavigationRail), findsOneWidget);
      expect(find.text('首页'), findsWidgets);
      final context = tester.element(find.byType(Scaffold).first);
      expect(Theme.of(context).brightness, Brightness.dark);

      await tester.tap(find.text('配置').first);
      await tester.pumpAndSettle();
      expect(tester.takeException(), isNull);
      expect(find.text('新建配置'), findsOneWidget);
    },
  );

  testWidgets('Android TV D-pad moves through navigation rail destinations', (
    tester,
  ) async {
    tester.view.devicePixelRatio = 1;
    tester.view.physicalSize = const Size(1280, 720);
    addTearDown(tester.view.resetDevicePixelRatio);
    addTearDown(tester.view.resetPhysicalSize);
    SharedPreferences.setMockInitialValues(<String, Object>{
      'onboarding_complete': true,
    });

    await tester.pumpWidget(UsqueBootstrap(engine: FakeEngineClient()));
    await tester.pumpAndSettle();

    final homeLabel = find.descendant(
      of: find.byType(NavigationRail),
      matching: find.text('Home'),
    );
    expect(homeLabel, findsOneWidget);
    Focus.of(tester.element(homeLabel)).requestFocus();
    await tester.pump();
    await tester.sendKeyDownEvent(LogicalKeyboardKey.arrowDown);
    await tester.sendKeyUpEvent(LogicalKeyboardKey.arrowDown);
    await tester.pumpAndSettle();

    expect(tester.takeException(), isNull);
    expect(find.widgetWithText(FilledButton, 'New profile'), findsOneWidget);
  });

  testWidgets(
    'extended rail aligns the brand with destinations and docks theme at the end',
    (tester) async {
      tester.view.devicePixelRatio = 1;
      tester.view.physicalSize = const Size(1280, 900);
      addTearDown(tester.view.resetDevicePixelRatio);
      addTearDown(tester.view.resetPhysicalSize);
      SharedPreferences.setMockInitialValues(<String, Object>{
        'onboarding_complete': true,
      });

      await tester.pumpWidget(UsqueBootstrap(engine: FakeEngineClient()));
      await tester.pumpAndSettle();

      final Finder rail = find.byType(NavigationRail);
      final Finder brand = find.descendant(
        of: rail,
        matching: find.text('Usque'),
      );
      final Finder brandIcon = find.byWidgetPredicate(
        (widget) =>
            widget is Image &&
            widget.image is AssetImage &&
            (widget.image as AssetImage).assetName ==
                'assets/branding/usque-ui-icon.png' &&
            widget.width == 30,
      );
      final Finder homeIcon = find.descendant(
        of: rail,
        matching: find.byIcon(LucideIcons.house),
      );
      final Finder themeButton = find.descendant(
        of: rail,
        matching: find.byTooltip('Theme · System'),
      );
      expect(brand, findsOneWidget);
      expect(brandIcon, findsOneWidget);
      expect(themeButton, findsOneWidget);
      expect(
        tester.getCenter(brandIcon).dx,
        closeTo(tester.getCenter(homeIcon).dx, 1),
      );
      expect(
        tester.getCenter(themeButton).dx,
        greaterThan(tester.getRect(rail).center.dx),
      );
      expect(
        tester.getCenter(themeButton).dx,
        greaterThan(tester.getCenter(brand).dx),
      );
      expect(
        (tester.getCenter(themeButton).dy - tester.getCenter(brand).dy).abs(),
        lessThan(12),
      );

      await tester.tap(themeButton);
      await tester.pumpAndSettle();
      expect(
        find.descendant(of: rail, matching: find.byTooltip('Theme · Light')),
        findsOneWidget,
      );
    },
  );

  testWidgets('compact rail hides the brand and centres the theme cycle', (
    tester,
  ) async {
    tester.view.devicePixelRatio = 1;
    tester.view.physicalSize = const Size(900, 800);
    addTearDown(tester.view.resetDevicePixelRatio);
    addTearDown(tester.view.resetPhysicalSize);
    SharedPreferences.setMockInitialValues(<String, Object>{
      'onboarding_complete': true,
    });

    await tester.pumpWidget(UsqueBootstrap(engine: FakeEngineClient()));
    await tester.pumpAndSettle();

    final Finder rail = find.byType(NavigationRail);
    final Finder brandIcon = find.byWidgetPredicate(
      (widget) =>
          widget is Image &&
          widget.image is AssetImage &&
          (widget.image as AssetImage).assetName ==
              'assets/branding/usque-ui-icon.png' &&
          widget.width == 30,
    );
    final Finder themeButton = find.descendant(
      of: rail,
      matching: find.byTooltip('Theme · System'),
    );
    expect(
      find.descendant(of: rail, matching: find.text('Usque')),
      findsNothing,
    );
    expect(brandIcon, findsNothing);
    expect(themeButton, findsOneWidget);
    expect(
      tester.getCenter(themeButton).dx,
      closeTo(tester.getRect(rail).center.dx, 2),
    );
    expect(tester.takeException(), isNull);
  });

  testWidgets('Android settings exposes boot, tile, and Always-on controls', (
    tester,
  ) async {
    debugDefaultTargetPlatformOverride = TargetPlatform.android;
    try {
      tester.view.devicePixelRatio = 1;
      tester.view.physicalSize = const Size(430, 900);
      addTearDown(tester.view.resetDevicePixelRatio);
      addTearDown(tester.view.resetPhysicalSize);
      SharedPreferences.setMockInitialValues(<String, Object>{
        'onboarding_complete': true,
      });

      await tester.pumpWidget(UsqueBootstrap(engine: FakeEngineClient()));
      await tester.pumpAndSettle();
      await tester.tap(find.text('Settings').last);
      await tester.pumpAndSettle();

      expect(find.text('System integration'), findsOneWidget);
      expect(find.text('Start Usque when you sign in'), findsOneWidget);
      expect(find.text('Add Quick Settings Tile'), findsOneWidget);
      expect(find.text('Open Always-on VPN settings'), findsOneWidget);
      expect(find.text('Per-app proxy'), findsOneWidget);
      expect(find.text('All apps use the VPN'), findsOneWidget);
      expect(find.text('Updates'), findsOneWidget);
    } finally {
      debugDefaultTargetPlatformOverride = null;
    }
  });

  testWidgets('Windows settings hide the per-app proxy picker', (tester) async {
    debugDefaultTargetPlatformOverride = TargetPlatform.windows;
    try {
      SharedPreferences.setMockInitialValues(<String, Object>{
        'onboarding_complete': true,
      });
      await tester.pumpWidget(UsqueBootstrap(engine: FakeEngineClient()));
      await tester.pumpAndSettle();
      await tester.tap(find.text('Settings').last);
      await tester.pumpAndSettle();
      expect(find.byType(SettingsScreen), findsOneWidget);
      expect(find.text('Per-app proxy'), findsNothing);
    } finally {
      debugDefaultTargetPlatformOverride = null;
    }
  });

  testWidgets('Per-app picker can select visible apps and save', (
    tester,
  ) async {
    debugDefaultTargetPlatformOverride = TargetPlatform.android;
    try {
      SharedPreferences.setMockInitialValues(<String, Object>{
        'onboarding_complete': true,
      });
      final engine = FakeEngineClient();
      final controller = AppController(engine);
      await controller.initialize();
      addTearDown(controller.dispose);

      await tester.pumpWidget(
        MaterialApp(
          theme: UsqueTheme.light(),
          home: PerAppProxyScreen(controller: controller),
        ),
      );
      await tester.pumpAndSettle();

      expect(find.text('Browser'), findsOneWidget);
      expect(find.text('Mail'), findsOneWidget);
      expect(find.text('Settings'), findsNothing);
      expect(find.text('io.github.georgexie2333.usque'), findsNothing);

      await tester.tap(find.text('Proxy only selected apps'));
      await tester.pump();
      await tester.tap(find.text('Select visible'));
      await tester.pump();
      await tester.tap(find.text('Save'));
      await tester.pumpAndSettle();

      expect(engine.calls, contains('setPerAppProxy'));
      expect(engine.storedPerAppProxy.enabled, isTrue);
      expect(engine.storedPerAppProxy.packageNames, <String>[
        'com.example.browser',
        'com.example.mail',
      ]);
    } finally {
      debugDefaultTargetPlatformOverride = null;
    }
  });

  testWidgets('Per-app picker list is D-pad traversable', (tester) async {
    debugDefaultTargetPlatformOverride = TargetPlatform.android;
    try {
      SharedPreferences.setMockInitialValues(<String, Object>{
        'onboarding_complete': true,
      });
      final controller = AppController(FakeEngineClient());
      await controller.initialize();
      addTearDown(controller.dispose);

      await tester.pumpWidget(
        MaterialApp(
          theme: UsqueTheme.light(),
          home: PerAppProxyScreen(controller: controller),
        ),
      );
      await tester.pumpAndSettle();

      final browser = find.text('Browser');
      expect(browser, findsOneWidget);
      Focus.of(tester.element(browser)).requestFocus();
      await tester.pump();
      await tester.sendKeyDownEvent(LogicalKeyboardKey.arrowDown);
      await tester.sendKeyUpEvent(LogicalKeyboardKey.arrowDown);
      await tester.pump();
      expect(tester.takeException(), isNull);
      expect(find.text('Mail'), findsOneWidget);
    } finally {
      debugDefaultTargetPlatformOverride = null;
    }
  });

  test('per-app settings stay off UsqueProfile maps', () {
    final profile = UsqueProfile.defaultProfile();
    expect(profile.toMap().containsKey('per_app_proxy'), isFalse);
    expect(
      PerAppProxySettings(
        enabled: true,
        packageNames: const <String>['io.github.georgexie2333.usque'],
      ).validationError(selfPackage: 'io.github.georgexie2333.usque'),
      'ANDROID_PER_APP_EMPTY',
    );
  });

  testWidgets('Home does not show a per-app status row', (tester) async {
    SharedPreferences.setMockInitialValues(<String, Object>{
      'onboarding_complete': true,
    });
    final engine = FakeEngineClient()
      ..storedPerAppProxy = const PerAppProxySettings(
        enabled: true,
        packageNames: <String>['com.example.browser'],
      );
    await tester.pumpWidget(UsqueBootstrap(engine: engine));
    await tester.pumpAndSettle();
    expect(find.byType(HomeScreen), findsOneWidget);
    expect(find.textContaining('Per-app proxy'), findsNothing);
    expect(find.textContaining('分应用代理'), findsNothing);
  });

  test('kill switch status key follows profile flag and live state', () {
    final tunnelOn = UsqueProfile.defaultProfile();
    final tunnelOff = tunnelOn.copyWith(
      frontends: tunnelOn.frontends.copyWith(tunnel: false),
    );
    final ksOff = tunnelOn.copyWith(killSwitch: false);

    expect(
      killSwitchStatusKey(
        profile: tunnelOff,
        snapshot: const EngineSnapshot(phase: ConnectionPhase.connected),
      ),
      'not_used_proxy',
    );
    expect(
      killSwitchStatusKey(
        profile: ksOff,
        snapshot: const EngineSnapshot(
          phase: ConnectionPhase.connected,
          killSwitchState: 'active',
        ),
      ),
      'off',
    );
    expect(
      killSwitchStatusKey(
        profile: tunnelOn,
        snapshot: const EngineSnapshot(
          phase: ConnectionPhase.connected,
          killSwitchState: 'active',
        ),
      ),
      'ks_active',
    );
    expect(
      killSwitchStatusKey(
        profile: tunnelOn,
        snapshot: const EngineSnapshot(
          phase: ConnectionPhase.disconnected,
          killSwitchState: 'inactive',
        ),
      ),
      'ks_inactive',
    );
    expect(
      killSwitchStatusKey(
        profile: tunnelOn,
        snapshot: const EngineSnapshot(
          phase: ConnectionPhase.error,
          killSwitchState: 'error',
        ),
      ),
      'ks_error',
    );
    expect(
      killSwitchStatusKey(
        profile: tunnelOn,
        snapshot: const EngineSnapshot(phase: ConnectionPhase.connectingH3),
      ),
      'ks_engaging',
    );
  });

  testWidgets('home shows Off, Active, or proxy-not-used for kill switch', (
    tester,
  ) async {
    SharedPreferences.setMockInitialValues(<String, Object>{
      'onboarding_complete': true,
    });
    final engine = FakeEngineClient();
    final controller = AppController(engine);
    await controller.initialize();
    addTearDown(controller.dispose);

    Future<void> pumpHome() async {
      await tester.pumpWidget(
        MaterialApp(
          theme: UsqueTheme.light(),
          home: HomeScreen(controller: controller),
        ),
      );
      await tester.pump();
    }

    controller.updateProfile(
      controller.activeProfile.copyWith(killSwitch: false),
    );
    await pumpHome();
    expect(find.text('Off'), findsOneWidget);

    controller.updateProfile(
      controller.activeProfile.copyWith(killSwitch: true),
    );
    controller.snapshot = const EngineSnapshot(
      phase: ConnectionPhase.connected,
      killSwitchState: 'active',
    );
    await pumpHome();
    expect(find.text('Active'), findsOneWidget);

    controller.updateProfile(
      controller.activeProfile.copyWith(
        frontends: controller.activeProfile.frontends.copyWith(tunnel: false),
      ),
    );
    await pumpHome();
    expect(find.text('Not used in proxy mode'), findsOneWidget);
  });

  testWidgets(
    'home location sits under engine status and waits until connected',
    (tester) async {
      SharedPreferences.setMockInitialValues(<String, Object>{
        'onboarding_complete': true,
      });
      final engine = FakeEngineClient();
      final controller = AppController(engine);
      await controller.initialize();
      await controller.setLocale(LocalePreference.english);
      addTearDown(controller.dispose);

      await tester.binding.setSurfaceSize(const Size(1200, 900));
      addTearDown(() => tester.binding.setSurfaceSize(null));

      Future<void> pumpHome() async {
        await tester.pumpWidget(
          MaterialApp(
            theme: UsqueTheme.light(),
            home: HomeScreen(controller: controller),
          ),
        );
        await tester.pump();
      }

      await pumpHome();
      expect(find.text('Waiting to connect'), findsOneWidget);
      expect(find.text('Not currently connected'), findsNothing);
      expect(find.byIcon(LucideIcons.mapPinOff), findsOneWidget);
      expect(find.text('IPv4'), findsNothing);
      expect(find.text('IPv6'), findsNothing);
      expect(find.text('Not available'), findsNothing);

      final Offset engineOrigin = tester.getTopLeft(find.text('Engine status'));
      final Offset locationOrigin = tester.getTopLeft(find.text('Location'));
      final Offset downloadOrigin = tester.getTopLeft(find.text('Download'));
      expect(locationOrigin.dx, closeTo(engineOrigin.dx, 1));
      expect(locationOrigin.dy, greaterThan(engineOrigin.dy));
      expect(downloadOrigin.dy, greaterThan(locationOrigin.dy));

      final Rect heroRect = tester.getRect(
        find.ancestor(
          of: find.byType(ConnectionRing),
          matching: find.byType(Panel),
        ),
      );
      final Rect locationRect = tester.getRect(
        find.ancestor(of: find.text('Location'), matching: find.byType(Panel)),
      );
      expect(locationRect.bottom, closeTo(heroRect.bottom, 2));

      controller.snapshot = const EngineSnapshot(
        phase: ConnectionPhase.connected,
        exit: ExitInfo(
          city: 'Singapore',
          country: 'Singapore',
          ipv4: '1.2.3.4',
          ipv6: '2001:db8::1',
        ),
      );
      await pumpHome();
      await tester.pump(const Duration(milliseconds: 350));
      expect(find.text('Waiting to connect'), findsNothing);
      expect(find.byIcon(LucideIcons.mapPinOff), findsNothing);
      expect(find.text('Singapore, Singapore'), findsOneWidget);
      expect(find.text('1.2.3.4'), findsOneWidget);
      expect(find.text('2001:db8::1'), findsOneWidget);
    },
  );

  testWidgets('error and degraded home show Retry and invoke the engine', (
    tester,
  ) async {
    SharedPreferences.setMockInitialValues(<String, Object>{
      'onboarding_complete': true,
    });
    final engine = EventEngineClient();
    engine.current = const EngineSnapshot(phase: ConnectionPhase.error);
    final controller = AppController(engine);
    await controller.initialize();
    addTearDown(controller.dispose);
    controller.snapshot = const EngineSnapshot(phase: ConnectionPhase.error);

    await tester.pumpWidget(
      MaterialApp(
        theme: UsqueTheme.light(),
        home: HomeScreen(controller: controller),
      ),
    );
    await tester.pump();
    expect(find.text('Retry'), findsOneWidget);
    await tester.tap(find.text('Retry'));
    await tester.pump();
    expect(engine.calls, contains('retry'));
  });

  test(
    'initialize with auto_connect connects a ready disconnected profile once',
    () async {
      SharedPreferences.setMockInitialValues(<String, Object>{
        'onboarding_complete': true,
      });
      final engine = FakeEngineClient();
      engine.legacyProfilesImported = true;
      engine.storedProfiles = <UsqueProfile>[
        UsqueProfile.defaultProfile().copyWith(autoConnect: true),
      ];
      final controller = AppController(engine);
      await controller.initialize();
      expect(engine.calls.where((call) => call == 'connect'), hasLength(1));

      engine.calls.clear();
      await controller.connectOrDisconnect();
      expect(engine.calls, contains('disconnect'));

      engine.calls.clear();
      await controller.initialize();
      expect(engine.calls, isNot(contains('connect')));
      controller.dispose();
    },
  );

  testWidgets('Settings network outputs edit the active profile immediately', (
    tester,
  ) async {
    debugDefaultTargetPlatformOverride = TargetPlatform.windows;
    try {
      tester.view.devicePixelRatio = 1;
      tester.view.physicalSize = const Size(1280, 900);
      addTearDown(tester.view.resetDevicePixelRatio);
      addTearDown(tester.view.resetPhysicalSize);
      SharedPreferences.setMockInitialValues(<String, Object>{
        'onboarding_complete': true,
      });

      await tester.pumpWidget(UsqueBootstrap(engine: FakeEngineClient()));
      await tester.pumpAndSettle();
      await tester.tap(
        find.descendant(
          of: find.byType(NavigationRail),
          matching: find.text('Settings'),
        ),
      );
      await tester.pumpAndSettle();

      Future<void> toggle(String title) async {
        final tile = find.widgetWithText(SwitchListTile, title);
        await tester.ensureVisible(tile);
        await tester.tap(tile);
        await tester.pumpAndSettle();
      }

      SettingsScreen settings() =>
          tester.widget<SettingsScreen>(find.byType(SettingsScreen));

      expect(find.text('Network outputs'), findsOneWidget);
      expect(settings().controller.activeProfile.frontends.tunnel, isTrue);
      expect(settings().controller.activeProfile.frontends.socks5, isTrue);
      expect(settings().controller.activeProfile.frontends.http, isTrue);
      expect(settings().controller.activeProfile.proxy.systemProxy, isFalse);
      expect(settings().controller.activeProfile.autoConnect, isFalse);

      await toggle('VPN / virtual adapter');
      expect(settings().controller.activeProfile.frontends.tunnel, isFalse);

      await toggle('SOCKS5');
      expect(settings().controller.activeProfile.frontends.socks5, isFalse);

      await toggle('Connect the current account automatically on start');
      expect(settings().controller.activeProfile.autoConnect, isTrue);

      await toggle('Configure system proxy');
      expect(settings().controller.activeProfile.proxy.systemProxy, isTrue);

      await toggle('HTTP');
      expect(settings().controller.activeProfile.frontends.http, isFalse);
      expect(settings().controller.activeProfile.proxy.systemProxy, isFalse);
      expect(settings().controller.activeProfile.frontends.any, isFalse);
      expect(find.text('No network output is enabled.'), findsOneWidget);
    } finally {
      debugDefaultTargetPlatformOverride = null;
    }
  });

  testWidgets('Android settings hide the system proxy switch', (tester) async {
    debugDefaultTargetPlatformOverride = TargetPlatform.android;
    try {
      tester.view.devicePixelRatio = 1;
      tester.view.physicalSize = const Size(1280, 900);
      addTearDown(tester.view.resetDevicePixelRatio);
      addTearDown(tester.view.resetPhysicalSize);
      SharedPreferences.setMockInitialValues(<String, Object>{
        'onboarding_complete': true,
      });

      await tester.pumpWidget(UsqueBootstrap(engine: FakeEngineClient()));
      await tester.pumpAndSettle();
      await tester.tap(
        find.descendant(
          of: find.byType(NavigationRail),
          matching: find.text('Settings'),
        ),
      );
      await tester.pumpAndSettle();

      expect(find.text('Network outputs'), findsOneWidget);
      expect(find.text('VPN / virtual adapter'), findsOneWidget);
      expect(find.text('Configure system proxy'), findsNothing);
    } finally {
      debugDefaultTargetPlatformOverride = null;
    }
  });

  testWidgets('profile cards show account identity instead of output tags', (
    tester,
  ) async {
    tester.view.devicePixelRatio = 1;
    tester.view.physicalSize = const Size(1280, 900);
    addTearDown(tester.view.resetDevicePixelRatio);
    addTearDown(tester.view.resetPhysicalSize);
    SharedPreferences.setMockInitialValues(<String, Object>{
      'onboarding_complete': true,
    });

    await tester.pumpWidget(UsqueBootstrap(engine: FakeEngineClient()));
    await tester.pumpAndSettle();
    await tester.tap(
      find.descendant(
        of: find.byType(NavigationRail),
        matching: find.text('Profiles'),
      ),
    );
    await tester.pumpAndSettle();

    final profiles = find.byType(ProfilesScreen);
    expect(
      find.descendant(of: profiles, matching: find.text('WARP Free')),
      findsOneWidget,
    );
    expect(
      find.descendant(of: profiles, matching: find.text('SOCKS5')),
      findsNothing,
    );
    expect(
      find.descendant(of: profiles, matching: find.text('HTTP')),
      findsNothing,
    );
    expect(
      find.descendant(of: profiles, matching: find.text('Identity ready')),
      findsNothing,
    );
    expect(
      find.descendant(of: profiles, matching: find.text('Kill Switch')),
      findsNothing,
    );
    expect(
      find.descendant(of: profiles, matching: find.text('162.159.198.2:443')),
      findsNothing,
    );

    await tester.tap(find.byTooltip('Edit').first);
    await tester.pumpAndSettle();
    expect(find.text('Edit profile'), findsOneWidget);
    expect(find.text('Profile name'), findsOneWidget);
    expect(find.widgetWithText(SwitchListTile, 'SOCKS5'), findsNothing);
    expect(
      find.widgetWithText(SwitchListTile, 'VPN / virtual adapter'),
      findsNothing,
    );
    expect(
      find.widgetWithText(SwitchListTile, 'Connect this Profile automatically'),
      findsNothing,
    );
    await tester.enterText(find.byType(TextField), 'Personal');
    await tester.tap(find.text('Save'));
    await tester.pumpAndSettle();
    expect(find.text('Personal'), findsOneWidget);
  });

  testWidgets('profile cards show WARP+ and Zero Trust identity tags', (
    tester,
  ) async {
    tester.view.devicePixelRatio = 1;
    tester.view.physicalSize = const Size(1280, 900);
    addTearDown(tester.view.resetDevicePixelRatio);
    addTearDown(tester.view.resetPhysicalSize);
    SharedPreferences.setMockInitialValues(<String, Object>{
      'onboarding_complete': true,
    });
    final plus = UsqueProfile.defaultProfile();
    final zeroTrust = plus.copyWith(
      id: 'aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee',
      name: 'Work',
    );
    final engine = FakeEngineClient()
      ..legacyProfilesImported = true
      ..storedProfiles = <UsqueProfile>[plus, zeroTrust]
      ..storedActiveProfileId = plus.id
      ..storedIdentityStatuses = <String, ProfileIdentityStatus>{
        plus.id: const ProfileIdentityStatus(
          state: ProfileIdentityState.ready,
          licenseState: LicenseState.warpPlus,
          accountType: 'WARP+',
        ),
        zeroTrust.id: const ProfileIdentityStatus(
          state: ProfileIdentityState.ready,
          licenseState: LicenseState.notApplicable,
          accountType: 'Zero Trust',
          provider: IdentityProvider.zeroTrust,
          organization: 'example-team',
        ),
      };

    await tester.pumpWidget(UsqueBootstrap(engine: engine));
    await tester.pumpAndSettle();
    await tester.tap(
      find.descendant(
        of: find.byType(NavigationRail),
        matching: find.text('Profiles'),
      ),
    );
    await tester.pumpAndSettle();

    expect(find.text('WARP+'), findsOneWidget);
    expect(find.text('Zero Trust'), findsOneWidget);
    expect(find.text('WARP Free'), findsNothing);
    expect(find.text('example-team · Experimental'), findsNothing);
  });

  testWidgets('profile cards show WARP Free from license state', (
    tester,
  ) async {
    tester.view.devicePixelRatio = 1;
    tester.view.physicalSize = const Size(1280, 900);
    addTearDown(tester.view.resetDevicePixelRatio);
    addTearDown(tester.view.resetPhysicalSize);
    SharedPreferences.setMockInitialValues(<String, Object>{
      'onboarding_complete': true,
    });
    final engine = FakeEngineClient()
      ..legacyProfilesImported = true
      ..storedIdentityStatuses = <String, ProfileIdentityStatus>{
        UsqueProfile.defaultProfileId: const ProfileIdentityStatus(
          state: ProfileIdentityState.ready,
          licenseState: LicenseState.free,
          accountType: 'Free',
        ),
      };

    await tester.pumpWidget(UsqueBootstrap(engine: engine));
    await tester.pumpAndSettle();
    await tester.tap(
      find.descendant(
        of: find.byType(NavigationRail),
        matching: find.text('Profiles'),
      ),
    );
    await tester.pumpAndSettle();

    expect(find.text('WARP Free'), findsOneWidget);
    expect(find.text('WARP+'), findsNothing);
  });
}
