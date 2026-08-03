import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:shared_preferences/shared_preferences.dart';
import 'package:usque/app.dart';
import 'package:usque/core/app_strings.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/services/engine_client.dart';
import 'package:usque/services/desktop_engine_client.dart';
import 'package:usque/state/app_controller.dart';
import 'package:usque/widgets/controller_selector.dart';

class FakeEngineClient implements EngineClient {
  @override
  bool get supportsSnapshotEvents => false;

  @override
  Stream<EngineSnapshot> get snapshotEvents =>
      const Stream<EngineSnapshot>.empty();

  bool provisioned = false;
  bool failProfileIdentityCreation = false;
  EngineSnapshot current = const EngineSnapshot();
  List<UsqueProfile> storedProfiles = <UsqueProfile>[
    UsqueProfile.defaultProfile(),
  ];
  String storedActiveProfileId = UsqueProfile.defaultProfileId;
  bool legacyProfilesImported = false;

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
    );
  }

  @override
  Future<void> upsertProfile(UsqueProfile profile) async {
    final index = storedProfiles.indexWhere(
      (stored) => stored.id == profile.id,
    );
    if (index < 0) {
      storedProfiles = <UsqueProfile>[...storedProfiles, profile];
    } else {
      storedProfiles = <UsqueProfile>[...storedProfiles]..[index] = profile;
    }
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
    String? warpSecret,
  }) async {
    if (failProfileIdentityCreation) {
      throw const EngineException(
        'REGISTRATION_FAILED',
        'Registration failed.',
      );
    }
    provisioned = true;
  }

  @override
  Future<ProfileCatalog> createProfileWithIdentity(
    UsqueProfile profile, {
    required IdentityProvisioningMethod method,
    String? warpSecret,
  }) async {
    if (failProfileIdentityCreation) {
      throw const EngineException(
        'REGISTRATION_FAILED',
        'Registration failed.',
      );
    }
    if (method == IdentityProvisioningMethod.importSecret &&
        (warpSecret == null || warpSecret.isEmpty)) {
      throw const EngineException(
        'INVALID_WARP_SECRET',
        'A WARP Secret is required.',
      );
    }
    provisioned = true;
    storedProfiles = <UsqueProfile>[...storedProfiles, profile];
    return ProfileCatalog(
      profiles: List<UsqueProfile>.unmodifiable(storedProfiles),
      activeProfileId: storedActiveProfileId,
      identityStates: <String, ProfileIdentityState>{
        for (final stored in storedProfiles)
          stored.id: ProfileIdentityState.ready,
      },
    );
  }

  @override
  Future<EngineSnapshot> connect(UsqueProfile profile) async {
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
    current = const EngineSnapshot();
    return current;
  }

  @override
  Future<EngineSnapshot> snapshot() async => current;

  @override
  Future<EngineSnapshot> pauseCaptivePortal({int seconds = 600}) async {
    current = EngineSnapshot(
      phase: ConnectionPhase.captivePortalPaused,
      captivePauseRemainingSeconds: seconds,
    );
    return current;
  }

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

  test('profile defaults match the product contract', () {
    final profile = UsqueProfile.defaultProfile();
    expect(profile.endpointIpv4, '162.159.198.2');
    expect(profile.endpointIpv6, '2606:4700:103::2');
    expect(profile.endpointPort, 443);
    expect(profile.sni, 'www.visa.cn');
    expect(profile.mtu, 1280);
    expect(profile.proxy.socksPort, 1080);
    expect(profile.proxy.httpPort, 8080);
    expect(profile.killSwitch, isTrue);
    expect(profile.proxy.exposesLan, isFalse);
    expect(profile.proxy.dnsMode, ProxyDnsMode.remote);
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
      proxy: ProxySettings(dnsMode: ProxyDnsMode.system, systemProxy: true),
    );

    final restored = UsqueProfile.fromMap(profile.toMap());
    expect(restored.toMap(), profile.toMap());
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
        mode: OperatingMode.httpProxy,
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
    second.dispose();
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

  test('leaving HTTP mode disables the Windows system proxy setting', () async {
    SharedPreferences.setMockInitialValues(<String, Object>{});
    final engine = FakeEngineClient();
    final controller = AppController(engine);
    await controller.initialize();

    controller.updateProfile(
      controller.activeProfile.copyWith(
        mode: OperatingMode.httpProxy,
        proxy: controller.activeProfile.proxy.copyWith(systemProxy: true),
      ),
    );
    await controller.flushProfileWrites();
    controller.updateProfile(
      controller.activeProfile.copyWith(mode: OperatingMode.socks5),
    );
    await controller.flushProfileWrites();

    expect(controller.activeProfile.mode, OperatingMode.socks5);
    expect(controller.activeProfile.proxy.systemProxy, isFalse);
    expect(engine.storedProfiles.single.proxy.systemProxy, isFalse);
    controller.dispose();
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
    expect(
      find.text(
        'Save endpoint and routing choices as profiles. '
        'Identity secrets stay in the system vault.',
      ),
      findsOneWidget,
    );
  });
}
