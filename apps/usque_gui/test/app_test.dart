import 'dart:async';
import 'dart:convert';

import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_localizations/flutter_localizations.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';
import 'package:shared_preferences/shared_preferences.dart';
import 'package:usque/app.dart';
import 'package:usque/core/app_strings.dart';
import 'package:usque/core/diagnostics_strings.dart';
import 'package:usque/core/l10n/catalogs.dart';
import 'package:usque/core/usque_theme.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/models/diagnostics_models.dart';
import 'package:usque/screens/advanced_settings_screen.dart';
import 'package:usque/screens/diagnostics_screen.dart';
import 'package:usque/screens/geo_direct_settings_screen.dart';
import 'package:usque/screens/home_screen.dart';
import 'package:usque/screens/onboarding_screen.dart';
import 'package:usque/screens/per_app_proxy_screen.dart';
import 'package:usque/screens/profiles_screen.dart';
import 'package:usque/screens/proxy_screen.dart';
import 'package:usque/screens/settings_screen.dart';
import 'package:usque/screens/shell_screen.dart';
import 'package:usque/services/desktop_engine_client.dart';
import 'package:usque/services/engine_client.dart';
import 'package:usque/services/update_downloader.dart';
import 'package:usque/state/app_controller.dart';
import 'package:usque/widgets/common.dart';
import 'package:usque/widgets/connection_ring.dart';
import 'package:usque/widgets/controller_selector.dart';
import 'package:usque/widgets/profile_identity_dialog.dart';

class FakeEngineClient implements EngineClient {
  @override
  bool get supportsSnapshotEvents => false;

  @override
  Stream<EngineSnapshotEvent> get snapshotEvents =>
      const Stream<EngineSnapshotEvent>.empty();

  bool provisioned = false;
  IdentityProvisioningMethod? lastProvisioningMethod;
  String? lastZeroTrustTeam;
  String? lastZeroTrustCallback;
  bool failProfileIdentityCreation = false;
  bool failProfileUpsert = false;
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
  GeoRulesList storedGeoRules = const GeoRulesList();
  List<GeoRulesUpdateResult> geoDownloadResults =
      const <GeoRulesUpdateResult>[];
  List<GeoRulesUpdateResult> geoUpdateResults = const <GeoRulesUpdateResult>[];
  UpdateCheckResult updateCheckResult = const UpdateCheckResult.current();
  Object? updateCheckError;
  Object? installUpdateError;
  Completer<UpdateCheckResult>? pendingUpdateCheck;
  final List<bool> updateCheckManualValues = <bool>[];

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

  bool _preservesEndpointIps(String id) =>
      storedIdentityStatuses[id]?.provider == IdentityProvider.zeroTrust;

  UsqueProfile _hydrate(UsqueProfile account, UsqueProfile network) {
    final keepEndpointIps = _preservesEndpointIps(account.id);
    return network.copyWith(
      id: account.id,
      name: account.name,
      endpointIpv4: keepEndpointIps
          ? account.endpointIpv4
          : network.endpointIpv4,
      endpointIpv6: keepEndpointIps
          ? account.endpointIpv6
          : network.endpointIpv6,
    );
  }

  UsqueProfile _currentNetwork(UsqueProfile fallback) {
    if (storedProfiles.isEmpty) {
      return fallback;
    }
    final source = storedProfiles.firstWhere(
      (stored) => !_preservesEndpointIps(stored.id),
      orElse: () => storedProfiles.first,
    );
    if (_preservesEndpointIps(source.id)) {
      return source.copyWith(
        endpointIpv4: UsqueProfile.defaultEndpointIpv4,
        endpointIpv6: UsqueProfile.defaultEndpointIpv6,
      );
    }
    return source;
  }

  @override
  Future<void> upsertProfile(UsqueProfile profile) async {
    if (failProfileUpsert) {
      throw const EngineException(
        'PROFILE_SAVE_FAILED',
        'Profile save failed.',
      );
    }
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
    final network = _preservesEndpointIps(profile.id)
        ? profile.copyWith(
            endpointIpv4: current.endpointIpv4,
            endpointIpv6: current.endpointIpv6,
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
    final storedProfile = method == IdentityProvisioningMethod.zeroTrust
        ? profile.copyWith(
            endpointIpv4: '162.159.197.2',
            endpointIpv6: '2606:4700:102::2',
          )
        : profile;
    storedProfiles = <UsqueProfile>[...storedProfiles, storedProfile];
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

  Object? connectError;
  Object? retryError;
  Object? clearAllDataError;
  Completer<EngineSnapshot>? pendingConnect;

  @override
  Future<EngineSnapshot> connect(UsqueProfile profile) async {
    calls.add('connect');
    if (connectError != null) {
      throw connectError!;
    }
    final pending = pendingConnect;
    if (pending != null) return pending.future;
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
    if (retryError != null) {
      throw retryError!;
    }
    return connect(lastConnectedProfile ?? storedProfiles.first);
  }

  @override
  Future<EngineSnapshot> snapshot() async => current;

  @override
  Future<DiagnosticSession> startDiagnostics(DiagnosticMode mode) async {
    return DiagnosticSession(
      sessionId: 'test-diagnostic-session',
      state: DiagnosticSessionState.completed,
      startedAt: DateTime.fromMillisecondsSinceEpoch(1, isUtc: true),
      completedAt: DateTime.fromMillisecondsSinceEpoch(2, isUtc: true),
      mode: mode,
      progressPercent: 100,
    );
  }

  @override
  Future<DiagnosticSession> cancelDiagnostics(String sessionId) async {
    return DiagnosticSession(
      sessionId: sessionId,
      state: DiagnosticSessionState.cancelled,
      startedAt: DateTime.fromMillisecondsSinceEpoch(1, isUtc: true),
      completedAt: DateTime.fromMillisecondsSinceEpoch(2, isUtc: true),
      mode: DiagnosticMode.standard,
      progressPercent: 100,
    );
  }

  @override
  Future<DiagnosticSession?> getDiagnostics() async => null;

  @override
  Future<ConnectionTimeline> getConnectionTimeline() async =>
      const ConnectionTimeline();

  @override
  Future<String?> exportDiagnostics({String? diagnosticSessionId}) async =>
      'test-diagnostics.zip';

  @override
  Future<UpdateCheckResult> checkForUpdates({bool manual = true}) async {
    updateCheckManualValues.add(manual);
    final pending = pendingUpdateCheck;
    if (pending != null) return pending.future;
    if (updateCheckError case final error?) throw error;
    return updateCheckResult;
  }

  @override
  Future<String> getUpdateCacheDirectory() async => 'test-update-cache';

  @override
  Future<void> verifyUpdatePackage({
    required String path,
    required String version,
    required UpdatePackage package,
  }) async {
    calls.add('verifyUpdatePackage');
  }

  @override
  Future<void> installUpdatePackage({
    required String path,
    required String version,
    required UpdatePackage package,
  }) async {
    calls.add('installUpdatePackage');
    if (installUpdateError case final error?) throw error;
  }

  @override
  Future<GeoRulesList> listGeoRules() async => storedGeoRules;

  @override
  Future<List<GeoRulesUpdateResult>> downloadGeoRules(
    String countryCode,
  ) async => geoDownloadResults;

  @override
  Future<List<GeoRulesUpdateResult>> updateAllGeoRules() async {
    calls.add('updateAllGeoRules');
    return geoUpdateResults;
  }

  @override
  Future<void> clearAllData({required bool confirmed}) async {
    if (!confirmed) {
      throw const EngineException(
        'CONFIRMATION_REQUIRED',
        'Confirmation is required.',
      );
    }
    if (clearAllDataError != null) {
      throw clearAllDataError!;
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

class RecordingUpdateDownloader extends UpdateDownloader {
  RecordingUpdateDownloader(super.engine);

  String? discardedPath;
  Object? discardError;

  @override
  Future<void> discard(String? path) async {
    discardedPath = path;
    if (discardError != null) {
      throw discardError!;
    }
  }
}

class ConcurrentGeoEngineClient extends FakeEngineClient {
  final connectStarted = Completer<void>();
  final connectResult = Completer<EngineSnapshot>();
  bool downloadedWhileConnecting = false;

  @override
  Future<EngineSnapshot> connect(UsqueProfile profile) {
    calls.add('connect');
    connectStarted.complete();
    return connectResult.future;
  }

  @override
  Future<List<GeoRulesUpdateResult>> downloadGeoRules(
    String countryCode,
  ) async {
    downloadedWhileConnecting = !connectResult.isCompleted;
    return const <GeoRulesUpdateResult>[];
  }
}

class EventEngineClient extends FakeEngineClient {
  final List<StreamController<EngineSnapshotEvent>> eventControllers =
      <StreamController<EngineSnapshotEvent>>[];
  bool subscribedAfterProfileImport = false;
  Completer<void>? delayCancel;

  @override
  bool get supportsSnapshotEvents => true;

  @override
  Stream<EngineSnapshotEvent> get snapshotEvents {
    subscribedAfterProfileImport = legacyProfilesImported;
    final controller = StreamController<EngineSnapshotEvent>(
      onCancel: () async {
        final hold = delayCancel;
        if (hold != null && !hold.isCompleted) {
          await hold.future;
        }
      },
    );
    eventControllers.add(controller);
    return controller.stream;
  }

  void emitSnapshot(EngineSnapshot snapshot) {
    eventControllers.last.add(EngineSnapshotEvent(snapshot: snapshot));
  }

  void emitHeartbeat() {
    eventControllers.last.add(const EngineSnapshotEvent());
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
  test(
    'cold process startup checks exactly once after initialization',
    () async {
      SharedPreferences.setMockInitialValues(<String, Object>{});
      final engine = FakeEngineClient();
      final controller = AppController(engine);

      await controller.initialize();
      await Future<void>.delayed(Duration.zero);
      await controller.initialize();
      await Future<void>.delayed(Duration.zero);

      expect(engine.updateCheckManualValues, <bool>[false]);
      controller.dispose();
    },
  );

  test('startup update check begins while auto-connect is pending', () async {
    SharedPreferences.setMockInitialValues(<String, Object>{
      'onboarding_complete': true,
    });
    final engine = FakeEngineClient()
      ..legacyProfilesImported = true
      ..storedProfiles = <UsqueProfile>[
        UsqueProfile.defaultProfile().copyWith(autoConnect: true),
      ]
      ..pendingConnect = Completer<EngineSnapshot>();
    final controller = AppController(engine);

    final initialization = controller.initialize();
    await Future<void>.delayed(Duration.zero);

    expect(engine.calls, contains('connect'));
    expect(engine.updateCheckManualValues, <bool>[false]);
    engine.pendingConnect!.complete(
      EngineSnapshot(
        phase: ConnectionPhase.connected,
        connectedAt: DateTime.now(),
      ),
    );
    await initialization;
    controller.dispose();
  });

  test('disabled startup checks still allow every manual live check', () async {
    SharedPreferences.setMockInitialValues(<String, Object>{
      'update_checks_enabled': false,
    });
    final engine = FakeEngineClient();
    final controller = AppController(engine);

    await controller.initialize();
    await Future<void>.delayed(Duration.zero);
    expect(engine.updateCheckManualValues, isEmpty);

    await controller.checkForUpdates();
    await controller.checkForUpdates();
    expect(engine.updateCheckManualValues, <bool>[true, true]);
    controller.dispose();
  });

  test(
    'automatic update failure stays silent and manual failure is visible',
    () async {
      SharedPreferences.setMockInitialValues(<String, Object>{});
      final engine = FakeEngineClient()
        ..updateCheckError = const EngineException(
          'UPDATE_CHECK_FAILED',
          'release endpoint unavailable',
        );
      final controller = AppController(engine);

      await controller.initialize();
      await Future<void>.delayed(Duration.zero);
      expect(controller.lastError, isNull);
      expect(controller.updatePhase, UpdateOperationPhase.idle);

      await controller.checkForUpdates();
      expect(controller.lastError, 'release endpoint unavailable');
      controller.dispose();
    },
  );

  test('concurrent manual update checks share the in-flight guard', () async {
    SharedPreferences.setMockInitialValues(<String, Object>{
      'update_checks_enabled': false,
    });
    final engine = FakeEngineClient()
      ..pendingUpdateCheck = Completer<UpdateCheckResult>();
    final controller = AppController(engine);
    await controller.initialize();

    final first = controller.checkForUpdates();
    await Future<void>.delayed(Duration.zero);
    final second = controller.checkForUpdates();
    expect(engine.updateCheckManualValues, <bool>[true]);
    engine.pendingUpdateCheck!.complete(const UpdateCheckResult.current());
    await Future.wait(<Future<void>>[first, second]);
    expect(engine.updateCheckManualValues, <bool>[true]);
    controller.dispose();
  });

  test('Android install handoff failure discards the terminal APK', () async {
    SharedPreferences.setMockInitialValues(<String, Object>{
      'update_checks_enabled': false,
    });
    final engine = FakeEngineClient()
      ..installUpdateError = const EngineException(
        'UPDATE_INSTALL_PERMISSION_DENIED',
        'permission denied',
      );
    final downloader = RecordingUpdateDownloader(engine);
    final controller = AppController(engine, updateDownloader: downloader);
    await controller.initialize();
    const path = 'test-update-cache/usque-v0.2.3-android-arm64-v8a.apk';
    controller.updateResult = const UpdateCheckResult(
      available: true,
      version: 'v0.2.3',
      package: UpdatePackage(
        name: 'usque-v0.2.3-android-arm64-v8a.apk',
        downloadUrl:
            'https://github.com/GeorgeXie2333/usque-app/releases/download/v0.2.3/usque-v0.2.3-android-arm64-v8a.apk',
        size: 1024,
        sha256:
            'a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5',
        platform: 'android',
        variant: 'arm64-v8a',
      ),
    );
    controller.downloadedUpdatePath = path;
    controller.updatePhase = UpdateOperationPhase.ready;

    await controller.installDownloadedUpdate();

    expect(downloader.discardedPath, path);
    expect(controller.downloadedUpdatePath, isNull);
    expect(controller.updatePhase, UpdateOperationPhase.available);
    expect(controller.updateError, 'permission denied');
    controller.dispose();
  });

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

  test(
    'geo updates report successful artifacts and partial failures',
    () async {
      final engine = FakeEngineClient()
        ..geoDownloadResults = const <GeoRulesUpdateResult>[
          GeoRulesUpdateResult(
            countryCode: 'CN',
            artifactKind: 'geoip',
            status: GeoRulesUpdateStatus.updated,
          ),
          GeoRulesUpdateResult(
            countryCode: 'CN',
            artifactKind: 'geosite',
            status: GeoRulesUpdateStatus.failed,
            reason: 'checksum mismatch',
          ),
        ];
      final controller = AppController(engine);

      await controller.downloadGeoRules('CN');

      expect(controller.lastNotice, contains('1 updated'));
      expect(controller.lastError, contains('CN geosite: checksum mismatch'));
      expect(controller.geoProgress, isNull);
      controller.dispose();
    },
  );

  test('geo rules can download while a connection is still starting', () async {
    SharedPreferences.setMockInitialValues(<String, Object>{});
    final engine = ConcurrentGeoEngineClient();
    final controller = AppController(engine);
    await controller.initialize();

    final connecting = controller.connectOrDisconnect();
    await engine.connectStarted.future;
    expect(controller.busy, isTrue);

    await controller.downloadGeoRules('CN');
    expect(engine.downloadedWhileConnecting, isTrue);
    expect(controller.busy, isTrue);

    engine.connectResult.complete(
      const EngineSnapshot(phase: ConnectionPhase.connected),
    );
    await connecting;
    expect(controller.busy, isFalse);
    controller.dispose();
  });

  test('advanced defaults preserve countries managed on their own page', () {
    final reset = UsqueProfile.defaultProfile()
        .copyWith(geoDirectCountries: const <String>['CN', 'US'])
        .resetAdvancedDefaults();

    expect(reset.geoDirectCountries, const <String>['CN', 'US']);
    expect(
      AppStrings(LocalePreference.english).get('reset_defaults_body'),
      isNot(contains('direct countries')),
    );
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

    engine.emitSnapshot(
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

    engine.emitSnapshot(failure);
    await tester.pump();
    expect(
      controller.lastError,
      'PROXY_LISTEN_FAILED: 127.0.0.1:1080 is already in use',
    );
    final notificationsAfterFirstError = notifications;

    engine.emitSnapshot(failure);
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

      const live = EngineSnapshot(
        phase: ConnectionPhase.connected,
        transport: 'HTTP/3',
        addressFamily: 'IPv4',
      );
      engine.emitSnapshot(live);
      await tester.pump();
      expect(controller.snapshotStreamDegraded, isFalse);

      engine.current = const EngineSnapshot(
        phase: ConnectionPhase.connected,
        transport: 'HTTP/2',
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
      expect(controller.snapshot.transport, 'HTTP/3');

      await tester.pump(const Duration(seconds: 1));
      await tester.pump();
      expect(controller.snapshot.phase, ConnectionPhase.connected);
      expect(controller.snapshot.transport, 'HTTP/2');
      expect(engine.eventControllers, hasLength(2));

      engine.emitSnapshot(
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

  testWidgets('cold start disconnected without events is not degraded', (
    tester,
  ) async {
    SharedPreferences.setMockInitialValues(<String, Object>{});
    final engine = EventEngineClient();
    final controller = AppController(engine);
    await controller.initialize();

    expect(controller.snapshotStreamDegraded, isFalse);
    expect(controller.snapshot.phase, ConnectionPhase.disconnected);

    await tester.pumpWidget(
      MaterialApp(
        theme: UsqueTheme.light(),
        home: DiagnosticsScreen(controller: controller),
      ),
    );
    expect(find.text('Live status updates are degraded'), findsNothing);
    controller.dispose();
  });

  testWidgets('diagnostics hides the degraded banner while disconnected', (
    tester,
  ) async {
    SharedPreferences.setMockInitialValues(<String, Object>{});
    final engine = EventEngineClient();
    final controller = AppController(engine);
    await controller.initialize();

    engine.emitSnapshot(const EngineSnapshot(phase: ConnectionPhase.connected));
    await tester.pump();
    engine.eventControllers.single.addError(
      PlatformException(
        code: 'ENGINE_EVENT_UNAVAILABLE',
        message: 'test stream failure',
      ),
    );
    await tester.pump();
    expect(controller.snapshotStreamDegraded, isTrue);
    expect(controller.snapshot.isConnected, isTrue);

    engine.current = const EngineSnapshot();
    await tester.pump(const Duration(seconds: 1));
    await tester.pump();
    expect(controller.snapshot.phase, ConnectionPhase.disconnected);
    expect(controller.snapshotStreamDegraded, isTrue);

    await tester.pumpWidget(
      MaterialApp(
        theme: UsqueTheme.light(),
        home: DiagnosticsScreen(controller: controller),
      ),
    );
    expect(find.text('Live status updates are degraded'), findsNothing);
    controller.dispose();
  });

  testWidgets('error before any live event is not degraded', (tester) async {
    SharedPreferences.setMockInitialValues(<String, Object>{});
    final engine = EventEngineClient();
    final controller = AppController(engine);
    await controller.initialize();
    expect(engine.eventControllers, hasLength(1));

    engine.eventControllers.single.addError(
      PlatformException(
        code: 'ENGINE_EVENT_UNAVAILABLE',
        message: 'test stream failure',
      ),
    );
    await tester.pump();

    expect(controller.snapshotStreamDegraded, isFalse);
    expect(controller.lastError, isNull);

    await tester.pump(const Duration(seconds: 1));
    await tester.pump();
    expect(engine.eventControllers.length, greaterThanOrEqualTo(2));
    controller.dispose();
    await tester.pump();
  });

  testWidgets('heartbeat recovers degraded stream without resetting snapshot', (
    tester,
  ) async {
    SharedPreferences.setMockInitialValues(<String, Object>{});
    final engine = EventEngineClient();
    final controller = AppController(engine);
    await controller.initialize();

    const live = EngineSnapshot(
      phase: ConnectionPhase.connected,
      transport: 'HTTP/3',
      addressFamily: 'IPv4',
    );
    engine.emitSnapshot(live);
    await tester.pump();
    expect(controller.snapshot, live);

    engine.eventControllers.single.addError(
      PlatformException(
        code: 'ENGINE_EVENT_UNAVAILABLE',
        message: 'test stream failure',
      ),
    );
    await tester.pump();
    expect(controller.snapshotStreamDegraded, isTrue);

    engine.emitHeartbeat();
    await tester.pump();

    expect(controller.snapshotStreamDegraded, isFalse);
    expect(controller.snapshot, live);
    expect(controller.snapshot.transport, 'HTTP/3');
    controller.dispose();
  });

  testWidgets('reconnect waits for the previous subscription to cancel', (
    tester,
  ) async {
    SharedPreferences.setMockInitialValues(<String, Object>{});
    final engine = EventEngineClient();
    final controller = AppController(engine);
    await controller.initialize();
    expect(engine.eventControllers, hasLength(1));

    engine.emitSnapshot(const EngineSnapshot(phase: ConnectionPhase.connected));
    await tester.pump();

    engine.eventControllers.single.addError(
      PlatformException(
        code: 'ENGINE_EVENT_UNAVAILABLE',
        message: 'first failure',
      ),
    );
    await tester.pump();
    engine.eventControllers.single.addError(
      PlatformException(
        code: 'ENGINE_EVENT_UNAVAILABLE',
        message: 'second failure',
      ),
    );
    await tester.pump();
    expect(engine.eventControllers, hasLength(1));
    expect(controller.snapshotStreamDegraded, isTrue);

    final hold = Completer<void>();
    engine.delayCancel = hold;
    await tester.pump(const Duration(seconds: 1));
    await tester.pump();
    expect(engine.eventControllers, hasLength(1));

    hold.complete();
    await tester.pump();
    expect(engine.eventControllers, hasLength(2));
    controller.dispose();
    await tester.pump();
  });

  test(
    'every registered catalog has the English key set and non-empty values',
    () {
      expect(AppStrings.debugCatalogsAreComplete, isTrue);
    },
  );

  test('tunnel output copy is platform-specific in every catalog', () {
    for (final catalog in kCatalogs.entries) {
      expect(
        catalog.value['tunnel_output'],
        'VPN (TUN)',
        reason: '${catalog.key} Windows tunnel label',
      );
      expect(
        catalog.value['vpn_mode'],
        'VPN',
        reason: '${catalog.key} Android tunnel label',
      );
    }

    final strings = AppStrings(LocalePreference.english);
    expect(strings.tunnelOutputLabel(TargetPlatform.windows), 'VPN (TUN)');
    expect(strings.tunnelOutputLabel(TargetPlatform.android), 'VPN');
  });

  test('compact navigation labels keep reviewed short translations', () {
    expect(AppStrings(LocalePreference.arabic).get('nav_profiles'), 'الحسابات');
    expect(AppStrings(LocalePreference.german).get('nav_settings'), 'Optionen');
    expect(AppStrings(LocalePreference.french).get('nav_settings'), 'Réglages');
    expect(
      AppStrings(LocalePreference.indonesian).get('nav_settings'),
      'Setelan',
    );
    expect(AppStrings(LocalePreference.italian).get('nav_settings'), 'Opzioni');
    expect(AppStrings(LocalePreference.japanese).get('nav_profiles'), 'アカウント');
    expect(AppStrings(LocalePreference.dutch).get('nav_settings'), 'Opties');
    expect(AppStrings(LocalePreference.polish).get('nav_settings'), 'Opcje');
    expect(
      AppStrings(LocalePreference.portuguese).get('nav_settings'),
      'Ajustes',
    );
    expect(AppStrings(LocalePreference.russian).get('nav_settings'), 'Опции');
    expect(AppStrings(LocalePreference.ukrainian).get('nav_settings'), 'Опції');
  });

  test(
    'interpolated catalogs keep {count}, {current}, {total}, and {updated}',
    () {
      expect(AppStrings.debugPlaceholdersArePreserved, isTrue);
    },
  );

  test('non-English catalogs translate geo and diagnostics UI copy', () {
    // geo_enable is the short "Direct" toggle; Direct is also Dutch/French.
    final keys = <String>{
      ...kEnCatalog.keys.where(
        (key) => key.startsWith('geo_') && key != 'geo_enable',
      ),
      'diagnostics_page_subtitle',
      'diag_refresh_timeline',
      'diag_operation_failed',
      'diag_event_stream_degraded',
      'diag_event_stream_degraded_body',
      'diag_export_included_body',
      'diag_export_excluded_body',
      'diag_export_local_only',
      'diag_run_title',
      'diag_run_subtitle',
      'diag_deep_title',
      'diag_deep_connected',
      'diag_deep_disconnected',
      'diag_start',
      'diag_session',
      'diag_progress_semantics',
      'diag_waiting_check',
      'diag_check_results',
      'diag_check_results_empty',
      'diag_timeline',
      'diag_timeline_subtitle',
      'diag_logs_subtitle',
      'diag_timeline_empty',
      'diag_timeline_truncated',
      'diag_copy_support',
      'diag_support_copied',
      'diag_finding_passed',
      'diag_finding_attention',
      'diag_finding_failed',
      'diag_finding_skipped',
      'diag_finding_cancelled',
      'diag_finding_running',
      'diag_finding_pending',
    };
    expect(AppStrings.debugUntranslatedKeys(keys), isEmpty);
  });

  test(
    'diagnostics helpers resolve English, Chinese, Japanese, and German',
    () {
      expect(
        diagnosticCheckLabel(
          AppStrings(LocalePreference.english),
          'transport.h3_connect',
        ),
        'HTTP/3 connection',
      );
      expect(
        diagnosticCheckLabel(
          AppStrings(LocalePreference.simplifiedChinese),
          'transport.h3_connect',
        ),
        'HTTP/3 连接',
      );
      expect(
        diagnosticFailureTitle(
          AppStrings(LocalePreference.english),
          'H3_HANDSHAKE_TIMEOUT',
        ),
        'H3 handshake timeout',
      );
      expect(
        diagnosticCheckLabel(
          AppStrings(LocalePreference.japanese),
          'transport.h3_connect',
        ),
        isNot(equals('HTTP/3 connection')),
      );
      expect(
        AppStrings(LocalePreference.german).get('diag_start'),
        isNot(equals('Start diagnostics')),
      );
    },
  );

  test(
    'language picker lists System then English-name A–Z with Chinese grouped',
    () {
      expect(LocalePreference.pickerOrder, <LocalePreference>[
        LocalePreference.system,
        LocalePreference.arabic,
        LocalePreference.simplifiedChinese,
        LocalePreference.traditionalChineseHongKong,
        LocalePreference.traditionalChineseTaiwan,
        LocalePreference.dutch,
        LocalePreference.english,
        LocalePreference.french,
        LocalePreference.german,
        LocalePreference.indonesian,
        LocalePreference.italian,
        LocalePreference.japanese,
        LocalePreference.korean,
        LocalePreference.persian,
        LocalePreference.polish,
        LocalePreference.portuguese,
        LocalePreference.russian,
        LocalePreference.spanish,
        LocalePreference.thai,
        LocalePreference.turkish,
        LocalePreference.ukrainian,
        LocalePreference.vietnamese,
      ]);
      expect(
        LocalePreference.pickerOrder.toSet(),
        LocalePreference.values.toSet(),
      );
    },
  );

  test('system locale maps CJK variants and falls back to English', () {
    expect(
      AppStrings.resolveCatalogId(
        LocalePreference.system,
        const Locale('zh', 'HK'),
      ),
      'zh_HK',
    );
    expect(
      AppStrings.resolveCatalogId(
        LocalePreference.system,
        const Locale.fromSubtags(
          languageCode: 'zh',
          scriptCode: 'Hant',
          countryCode: 'MO',
        ),
      ),
      'zh_HK',
    );
    expect(
      AppStrings.resolveCatalogId(
        LocalePreference.system,
        const Locale('zh', 'TW'),
      ),
      'zh_TW',
    );
    expect(
      AppStrings.resolveCatalogId(
        LocalePreference.system,
        const Locale.fromSubtags(languageCode: 'zh', scriptCode: 'Hant'),
      ),
      'zh_TW',
    );
    expect(
      AppStrings.resolveCatalogId(
        LocalePreference.system,
        const Locale('zh', 'CN'),
      ),
      'zh_CN',
    );
    expect(
      AppStrings.resolveCatalogId(LocalePreference.system, const Locale('ja')),
      'ja',
    );
    expect(
      AppStrings.resolveCatalogId(LocalePreference.system, const Locale('de')),
      'de',
    );
    expect(
      AppStrings.resolveCatalogId(LocalePreference.system, const Locale('ar')),
      'ar',
    );
    expect(
      AppStrings.resolveCatalogId(LocalePreference.system, const Locale('id')),
      'id',
    );
    expect(
      AppStrings.resolveCatalogId(LocalePreference.system, const Locale('vi')),
      'vi',
    );
    expect(
      AppStrings.resolveCatalogId(LocalePreference.system, const Locale('uk')),
      'uk',
    );
    expect(
      AppStrings.resolveCatalogId(LocalePreference.persian, const Locale('en')),
      'fa',
    );
  });

  test('Hong Kong and Taiwan catalogs keep distinct regional wording', () {
    final hk = AppStrings(LocalePreference.traditionalChineseHongKong);
    final tw = AppStrings(LocalePreference.traditionalChineseTaiwan);
    expect(hk.get('upload'), '上載');
    expect(tw.get('upload'), '上傳');
    expect(hk.get('close_to_tray'), contains('系統列'));
    expect(tw.get('close_to_tray'), contains('系統匣'));
    expect(hk.get('geo_direct_help'), contains('網絡'));
    expect(tw.get('geo_direct_help'), contains('網路'));
    expect(hk.get('diag_family'), '地址族');
    expect(tw.get('diag_family'), '位址族');
  });

  testWidgets('Persian and Arabic locales select RTL directionality', (
    tester,
  ) async {
    for (final locale in const <Locale>[Locale('fa'), Locale('ar')]) {
      await tester.pumpWidget(
        MaterialApp(
          locale: locale,
          supportedLocales: const <Locale>[
            Locale('en'),
            Locale('fa'),
            Locale('ar'),
          ],
          localizationsDelegates: const <LocalizationsDelegate<dynamic>>[
            GlobalMaterialLocalizations.delegate,
            GlobalWidgetsLocalizations.delegate,
            GlobalCupertinoLocalizations.delegate,
          ],
          home: const Scaffold(body: SizedBox.shrink()),
        ),
      );
      expect(
        Directionality.of(tester.element(find.byType(Scaffold))),
        TextDirection.rtl,
        reason: '$locale should be RTL',
      );
    }
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
    'failed shared endpoint save restores the authoritative network',
    () async {
      SharedPreferences.setMockInitialValues(<String, Object>{});
      final authoritative = UsqueProfile.defaultProfile().copyWith(
        sni: 'before.example.com',
      );
      final engine = FakeEngineClient()
        ..legacyProfilesImported = true
        ..storedProfiles = <UsqueProfile>[authoritative];
      final controller = AppController(engine);
      await controller.initialize();
      addTearDown(controller.dispose);

      engine.failProfileUpsert = true;
      controller.updateNetwork(
        controller.activeProfile.copyWith(sni: 'unsaved.example.com'),
      );
      expect(controller.activeProfile.sni, 'unsaved.example.com');

      await controller.flushProfileWrites();

      expect(
        controller.lastError,
        contains('Profile changes could not be saved'),
      );
      expect(controller.sharedNetwork.sni, 'before.example.com');
      expect(controller.activeProfile.sni, 'before.example.com');
      expect(engine.storedProfiles.single.sni, 'before.example.com');
    },
  );

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

  test(
    'Zero Trust creation hydrates registered IPs with shared port and SNI',
    () async {
      SharedPreferences.setMockInitialValues(<String, Object>{});
      final engine = FakeEngineClient();
      final controller = AppController(engine);
      await controller.initialize();
      addTearDown(controller.dispose);
      controller.updateNetwork(
        controller.activeProfile.copyWith(
          endpointPort: 8443,
          sni: 'shared.example.com',
        ),
      );
      await controller.flushProfileWrites();

      expect(
        await controller.createProfileWithIdentity(
          'Work',
          method: IdentityProvisioningMethod.zeroTrust,
          teamName: 'example-team',
          callbackUri:
              'com.cloudflare.warp://example-team.cloudflareaccess.com/auth?token=test',
        ),
        isTrue,
      );
      final work = controller.profiles.last;
      expect(
        controller.identityStatus(work.id).provider,
        IdentityProvider.zeroTrust,
      );
      controller.setActiveProfile(work.id);
      expect(controller.activeProfile.endpointIpv4, '162.159.197.2');
      expect(controller.activeProfile.endpointIpv6, '2606:4700:102::2');
      expect(controller.activeProfile.endpointPort, 8443);
      expect(controller.activeProfile.sni, 'shared.example.com');
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

  testWidgets('Zero Trust locks endpoint IPs but saves port and SNI', (
    tester,
  ) async {
    SharedPreferences.setMockInitialValues(<String, Object>{
      'onboarding_complete': true,
    });
    final registered = UsqueProfile.defaultProfile().copyWith(
      endpointIpv4: '162.159.197.2',
      endpointIpv6: '2606:4700:102::2',
    );
    final engine = FakeEngineClient()
      ..legacyProfilesImported = true
      ..storedProfiles = <UsqueProfile>[registered]
      ..storedIdentityStatuses = <String, ProfileIdentityStatus>{
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
      findsNothing,
    );
    final endpointFields = tester
        .widgetList<TextField>(find.byType(TextField))
        .take(4)
        .toList(growable: false);
    expect(endpointFields[0].readOnly, isTrue);
    expect(endpointFields[1].readOnly, isTrue);
    expect(endpointFields[2].readOnly, isFalse);
    expect(endpointFields[3].readOnly, isFalse);
    await tester.enterText(find.widgetWithText(TextFormField, 'Port'), '8443');
    await tester.enterText(
      find.widgetWithText(TextFormField, 'SNI'),
      'shared.example.com',
    );
    final save = find.widgetWithText(FilledButton, 'Save');
    await tester.ensureVisible(save);
    await tester.tap(save);
    await tester.pumpAndSettle();

    expect(controller.activeProfile.endpointIpv4, '162.159.197.2');
    expect(controller.activeProfile.endpointIpv6, '2606:4700:102::2');
    expect(controller.activeProfile.endpointPort, 8443);
    expect(controller.activeProfile.sni, 'shared.example.com');
    expect(engine.storedProfiles.single.endpointIpv4, '162.159.197.2');
    expect(engine.storedProfiles.single.endpointIpv6, '2606:4700:102::2');
    expect(engine.storedProfiles.single.endpointPort, 8443);
    expect(engine.storedProfiles.single.sni, 'shared.example.com');
  });

  test('Zero Trust keeps registered IPs while sharing port and SNI', () async {
    SharedPreferences.setMockInitialValues(<String, Object>{
      'onboarding_complete': true,
    });
    final consumer = UsqueProfile.defaultProfile().copyWith(
      sni: 'consumer.example.com',
    );
    final zeroTrust = consumer.copyWith(
      id: 'aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee',
      name: 'Work',
      endpointIpv4: '162.159.197.2',
      endpointIpv6: '2606:4700:102::2',
    );
    final engine = FakeEngineClient()
      ..legacyProfilesImported = true
      ..storedProfiles = <UsqueProfile>[consumer, zeroTrust]
      ..storedActiveProfileId = zeroTrust.id
      ..storedIdentityStatuses = <String, ProfileIdentityStatus>{
        zeroTrust.id: const ProfileIdentityStatus(
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

    expect(controller.activeProfile.sni, 'consumer.example.com');
    controller.updateNetwork(
      controller.activeProfile.copyWith(
        endpointIpv4: '192.0.2.10',
        endpointIpv6: '2001:db8::10',
        endpointPort: 8443,
        sni: 'shared.example.com',
      ),
    );
    controller.setActiveProfile(consumer.id);

    expect(controller.activeProfile.endpointIpv4, consumer.endpointIpv4);
    expect(controller.activeProfile.endpointIpv6, consumer.endpointIpv6);
    expect(controller.activeProfile.endpointPort, 8443);
    expect(controller.activeProfile.sni, 'shared.example.com');
    controller.setActiveProfile(zeroTrust.id);
    expect(controller.activeProfile.endpointIpv4, '162.159.197.2');
    expect(controller.activeProfile.endpointIpv6, '2606:4700:102::2');
    expect(controller.activeProfile.endpointPort, 8443);
    expect(controller.activeProfile.sni, 'shared.example.com');
  });

  test(
    'editing a non-active Zero Trust account cannot replace shared IPs',
    () async {
      SharedPreferences.setMockInitialValues(<String, Object>{
        'onboarding_complete': true,
      });
      final consumer = UsqueProfile.defaultProfile();
      final zeroTrust = consumer.copyWith(
        id: 'aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee',
        name: 'Work',
        endpointIpv4: '162.159.197.2',
        endpointIpv6: '2606:4700:102::2',
      );
      final engine = FakeEngineClient()
        ..legacyProfilesImported = true
        ..storedProfiles = <UsqueProfile>[consumer, zeroTrust]
        ..storedActiveProfileId = consumer.id
        ..storedIdentityStatuses = <String, ProfileIdentityStatus>{
          zeroTrust.id: const ProfileIdentityStatus(
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

      controller.updateNetwork(
        zeroTrust.copyWith(
          endpointIpv4: '192.0.2.10',
          endpointIpv6: '2001:db8::10',
          endpointPort: 8443,
          sni: 'shared.example.com',
        ),
      );
      await controller.flushProfileWrites();

      expect(controller.activeProfile.id, consumer.id);
      expect(controller.activeProfile.endpointIpv4, consumer.endpointIpv4);
      expect(controller.activeProfile.endpointIpv6, consumer.endpointIpv6);
      expect(controller.activeProfile.endpointPort, 8443);
      expect(controller.activeProfile.sni, 'shared.example.com');
      controller.setActiveProfile(zeroTrust.id);
      expect(controller.activeProfile.endpointIpv4, '162.159.197.2');
      expect(controller.activeProfile.endpointIpv6, '2606:4700:102::2');
    },
  );

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

    expect(await controller.clearAllData(), isTrue);

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

  test('clear all data commits even when update-cache cleanup warns', () async {
    SharedPreferences.setMockInitialValues(<String, Object>{
      'onboarding_complete': true,
    });
    final engine = FakeEngineClient()..provisioned = true;
    final downloader = RecordingUpdateDownloader(engine)
      ..discardError = const FormatException('update cache is locked');
    final controller = AppController(engine, updateDownloader: downloader);
    await controller.initialize();
    controller.downloadedUpdatePath = 'test-update.msi';

    expect(await controller.clearAllData(), isTrue);

    expect(controller.onboardingComplete, isFalse);
    expect(controller.lastError, contains('update cache is locked'));
    expect(downloader.discardedPath, 'test-update.msi');
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

  testWidgets('typing a WARP license key enables Finish setup', (tester) async {
    SharedPreferences.setMockInitialValues(<String, Object>{});
    await tester.pumpWidget(UsqueBootstrap(engine: FakeEngineClient()));
    await tester.pumpAndSettle();

    await tester.tap(find.text('Continue'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Continue'));
    await tester.pumpAndSettle();
    await tester.tap(find.byType(CheckboxListTile));
    await tester.pump();
    await tester.tap(find.text('Continue'));
    await tester.pumpAndSettle();

    await tester.tap(find.text('Use a WARP License Key'));
    await tester.pumpAndSettle();

    FilledButton finishButton() {
      return tester.widget<FilledButton>(
        find.widgetWithText(FilledButton, 'Finish setup'),
      );
    }

    expect(finishButton().onPressed, isNull);

    await tester.enterText(find.byType(TextField), 'test-license-key');
    await tester.pump();

    expect(finishButton().onPressed, isNotNull);
  });

  test('connect maps generic failures off the preparing phase', () async {
    SharedPreferences.setMockInitialValues(<String, Object>{
      'onboarding_complete': true,
      'update_checks_enabled': false,
    });
    final engine = FakeEngineClient();
    engine.connectError = const FormatException('Invalid listener port');
    final controller = AppController(engine);
    await controller.initialize();
    await Future<void>.delayed(Duration.zero);
    await controller.connectOrDisconnect();
    expect(controller.snapshot.phase, ConnectionPhase.error);
    expect(controller.lastError, contains('Invalid listener port'));
    expect(controller.busy, isFalse);
    controller.dispose();
  });

  test('retry maps generic failures off a transitional phase', () async {
    SharedPreferences.setMockInitialValues(<String, Object>{
      'onboarding_complete': true,
      'update_checks_enabled': false,
    });
    final engine = FakeEngineClient();
    engine.retryError = const FormatException('Invalid protobuf field');
    final controller = AppController(engine);
    await controller.initialize();
    await Future<void>.delayed(Duration.zero);
    controller.snapshot = const EngineSnapshot(
      phase: ConnectionPhase.preparing,
    );
    await controller.retry();
    expect(controller.snapshot.phase, ConnectionPhase.error);
    expect(controller.lastError, contains('Invalid protobuf field'));
    expect(controller.busy, isFalse);
    controller.dispose();
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

  testWidgets('generic connect failure leaves home on error, not preparing', (
    tester,
  ) async {
    SharedPreferences.setMockInitialValues(<String, Object>{
      'onboarding_complete': true,
    });
    final engine = FakeEngineClient();
    engine.connectError = const FormatException('Invalid listener port');
    await tester.pumpWidget(UsqueBootstrap(engine: engine));
    await tester.pumpAndSettle();

    await tester.tap(find.text('Connect'));
    await tester.pumpAndSettle();

    expect(find.text('Preparing secure tunnel…'), findsNothing);
    expect(find.text('Connection error'), findsWidgets);
    expect(find.text('Retry'), findsOneWidget);
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
    expect(find.text('Usque Engine status'), findsOneWidget);
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
    'mobile navigation has four destinations and arrow keys cycle them',
    (tester) async {
      tester.view.devicePixelRatio = 1;
      tester.view.physicalSize = const Size(600, 900);
      addTearDown(tester.view.resetDevicePixelRatio);
      addTearDown(tester.view.resetPhysicalSize);
      SharedPreferences.setMockInitialValues(<String, Object>{
        'onboarding_complete': true,
        'update_checks_enabled': false,
      });
      final controller = AppController(FakeEngineClient());
      await controller.initialize();
      addTearDown(controller.dispose);

      await tester.pumpWidget(
        MaterialApp(
          theme: UsqueTheme.light(),
          home: ShellScreen(controller: controller),
        ),
      );
      await tester.pumpAndSettle();

      final barFinder = find.byType(NavigationBar);
      expect(barFinder, findsOneWidget);
      final bar = tester.widget<NavigationBar>(barFinder);
      expect(bar.destinations, hasLength(4));
      expect(
        bar.destinations.cast<NavigationDestination>().map(
          (destination) => destination.label,
        ),
        <String>['Home', 'Profiles', 'Proxy', 'Settings'],
      );

      final homeLabel = find.descendant(
        of: barFinder,
        matching: find.text('Home'),
      );
      Focus.of(tester.element(homeLabel)).requestFocus();
      await tester.pump();

      Future<void> moveRight(AppSection expected) async {
        await tester.sendKeyDownEvent(LogicalKeyboardKey.arrowRight);
        await tester.sendKeyUpEvent(LogicalKeyboardKey.arrowRight);
        await tester.pumpAndSettle();
        expect(controller.section, expected);
        expect(
          tester.takeException(),
          isNull,
          reason: 'after moving to ${expected.name}',
        );
      }

      await moveRight(AppSection.profiles);
      await moveRight(AppSection.proxy);
      await moveRight(AppSection.settings);
      await moveRight(AppSection.home);
    },
  );

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
        engine.emitSnapshot(
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
      'update_checks_enabled': false,
    });
    final controller = AppController(FakeEngineClient());
    await controller.initialize();
    addTearDown(controller.dispose);

    await tester.pumpWidget(
      MaterialApp(
        theme: UsqueTheme.light(),
        home: ShellScreen(controller: controller),
      ),
    );
    await tester.pumpAndSettle();

    final railFinder = find.byType(NavigationRail);
    final rail = tester.widget<NavigationRail>(railFinder);
    expect(rail.destinations, hasLength(4));
    expect(
      rail.destinations.map(
        (destination) => (destination.label as Tooltip).message,
      ),
      <String>['Home', 'Profiles', 'Proxy', 'Settings'],
    );
    final homeLabel = find.descendant(
      of: railFinder,
      matching: find.text('Home'),
    );
    expect(homeLabel, findsOneWidget);
    Focus.of(tester.element(homeLabel)).requestFocus();
    await tester.pump();

    Future<void> moveDown(AppSection expected) async {
      await tester.sendKeyDownEvent(LogicalKeyboardKey.arrowDown);
      await tester.sendKeyUpEvent(LogicalKeyboardKey.arrowDown);
      await tester.pumpAndSettle();
      expect(controller.section, expected);
      expect(tester.takeException(), isNull);
    }

    await moveDown(AppSection.profiles);
    await moveDown(AppSection.proxy);
    await moveDown(AppSection.settings);
    await moveDown(AppSection.home);
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
      final railWidget = tester.widget<NavigationRail>(rail);
      expect(railWidget.extended, isTrue);
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
    final railWidget = tester.widget<NavigationRail>(rail);
    expect(railWidget.extended, isFalse);
    expect(railWidget.labelType, NavigationRailLabelType.all);
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

  testWidgets('Android rail theme target is at least 48dp', (tester) async {
    debugDefaultTargetPlatformOverride = TargetPlatform.android;
    try {
      tester.view.devicePixelRatio = 1;
      tester.view.physicalSize = const Size(900, 800);
      addTearDown(tester.view.resetDevicePixelRatio);
      addTearDown(tester.view.resetPhysicalSize);
      SharedPreferences.setMockInitialValues(<String, Object>{
        'onboarding_complete': true,
      });

      await tester.pumpWidget(UsqueBootstrap(engine: FakeEngineClient()));
      await tester.pumpAndSettle();

      final themeButton = find.descendant(
        of: find.byType(NavigationRail),
        matching: find.byTooltip('Theme · System'),
      );
      expect(themeButton, findsOneWidget);
      expect(
        tester.getSize(themeButton).shortestSide,
        greaterThanOrEqualTo(48),
      );
      expect(
        tester.getSemantics(themeButton).rect.size.shortestSide,
        greaterThanOrEqualTo(48),
      );
    } finally {
      debugDefaultTargetPlatformOverride = null;
    }
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

      final themePicker = find.byWidgetPredicate(
        (widget) => widget is DropdownButton<ThemePreference>,
      );
      expect(themePicker, findsOneWidget);
      expect(tester.getSize(themePicker).height, greaterThanOrEqualTo(48));
      expect(
        tester.getSemantics(themePicker).rect.height,
        greaterThanOrEqualTo(48),
      );
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

  testWidgets(
    'verified update UI shows progress, retry, and install confirmation',
    (tester) async {
      debugDefaultTargetPlatformOverride = TargetPlatform.windows;
      try {
        tester.view.devicePixelRatio = 1;
        tester.view.physicalSize = const Size(1280, 1400);
        addTearDown(tester.view.resetDevicePixelRatio);
        addTearDown(tester.view.resetPhysicalSize);
        SharedPreferences.setMockInitialValues(<String, Object>{
          'update_checks_enabled': false,
        });
        final controller = AppController(FakeEngineClient());
        await controller.initialize();
        addTearDown(controller.dispose);
        controller.updateResult = const UpdateCheckResult(
          available: true,
          version: 'v0.2.3',
          releaseUrl:
              'https://github.com/GeorgeXie2333/usque-app/releases/tag/v0.2.3',
          package: UpdatePackage(
            name: 'usque-v0.2.3-windows-x64-v2.msi',
            downloadUrl:
                'https://github.com/GeorgeXie2333/usque-app/releases/download/v0.2.3/usque-v0.2.3-windows-x64-v2.msi',
            size: 20 * 1024 * 1024,
            sha256:
                'a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5',
            platform: 'windows',
            variant: 'x64-v2',
          ),
        );
        controller.updatePhase = UpdateOperationPhase.downloading;
        controller.updateDownloadedBytes = 5 * 1024 * 1024;
        controller.updateTotalBytes = 20 * 1024 * 1024;

        Widget app() => MaterialApp(
          theme: UsqueTheme.light(),
          home: SettingsScreen(controller: controller),
        );

        await tester.pumpWidget(app());
        expect(find.text('v0.2.3  •  x64-v2  •  20.0 MiB'), findsOneWidget);
        expect(find.byType(LinearProgressIndicator), findsOneWidget);
        expect(find.text('5.0 MiB / 20.0 MiB'), findsOneWidget);
        expect(find.text('Cancel'), findsOneWidget);

        controller.updatePhase = UpdateOperationPhase.failed;
        controller.updateError = 'network interrupted';
        await tester.pumpWidget(app());
        expect(find.text('Retry'), findsOneWidget);
        expect(find.text('network interrupted'), findsOneWidget);

        controller.updatePhase = UpdateOperationPhase.ready;
        controller.updateError = null;
        controller.downloadedUpdatePath = r'C:\update\usque.msi';
        await tester.pumpWidget(app());
        final install = find.text('Restart and update');
        expect(install, findsOneWidget);
        await tester.ensureVisible(install);
        await tester.tap(install);
        await tester.pumpAndSettle();
        expect(find.text('Install this update?'), findsOneWidget);
        expect(
          find.textContaining('VPN and proxy connections will disconnect'),
          findsOneWidget,
        );
        await tester.tap(find.text('Cancel').last);
        await tester.pumpAndSettle();

        Future<void> pumpAccessibleViewport(Size size) async {
          tester.view.physicalSize = size;
          await tester.pumpWidget(const SizedBox.shrink());
          await tester.pump();
          await tester.pumpWidget(
            MaterialApp(
              theme: UsqueTheme.dark(),
              builder: (context, child) => MediaQuery(
                data: MediaQuery.of(context).copyWith(
                  textScaler: const TextScaler.linear(2),
                  disableAnimations: true,
                ),
                child: child!,
              ),
              home: SettingsScreen(controller: controller),
            ),
          );
          await tester.ensureVisible(find.text('Updates'));
          await tester.pump();
          expect(tester.takeException(), isNull);
        }

        await pumpAccessibleViewport(const Size(375, 667));
        await pumpAccessibleViewport(const Size(667, 375));
      } finally {
        debugDefaultTargetPlatformOverride = null;
      }
    },
  );

  testWidgets(
    'direct countries opens its own settings page and leaves Advanced',
    (tester) async {
      tester.view.devicePixelRatio = 1;
      tester.view.physicalSize = const Size(1280, 1100);
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
          home: SettingsScreen(controller: controller),
        ),
      );
      await tester.pumpAndSettle();

      final directCountries = find.text('Countries routed directly');
      expect(directCountries, findsOneWidget);
      await tester.ensureVisible(directCountries);
      await tester.tap(directCountries);
      await tester.pumpAndSettle();

      expect(find.byType(GeoDirectSettingsScreen), findsOneWidget);
      expect(find.text('Search countries'), findsOneWidget);
      expect(
        find.text(
          "Matched domains are visible to your current network's DNS; "
          'apps using encrypted DNS are routed by IP only.',
        ),
        findsOneWidget,
      );
      expect(find.textContaining('Android VPN'), findsNothing);
      expect(find.byType(AdvancedSettingsScreen), findsNothing);

      await tester.tap(find.text('Back'));
      await tester.pumpAndSettle();
      final advanced = find.text('Advanced network settings');
      await tester.ensureVisible(advanced);
      await tester.tap(advanced);
      await tester.pumpAndSettle();

      expect(find.byType(AdvancedSettingsScreen), findsOneWidget);
      expect(find.text('Countries routed directly'), findsNothing);
    },
  );

  testWidgets('Settings opens Diagnostics as a subpage on mobile and desktop', (
    tester,
  ) async {
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetDevicePixelRatio);
    addTearDown(tester.view.resetPhysicalSize);

    for (final size in <Size>[const Size(430, 900), const Size(1280, 1100)]) {
      tester.view.physicalSize = size;
      SharedPreferences.setMockInitialValues(<String, Object>{
        'onboarding_complete': true,
        'update_checks_enabled': false,
      });
      final controller = AppController(FakeEngineClient());
      await controller.initialize();

      await tester.pumpWidget(
        MaterialApp(
          key: ValueKey<double>(size.width),
          theme: UsqueTheme.light(),
          home: ShellScreen(controller: controller),
        ),
      );
      await tester.pumpAndSettle();

      final navigation = size.width < 760
          ? find.byType(NavigationBar)
          : find.byType(NavigationRail);
      await tester.tap(
        find.descendant(of: navigation, matching: find.text('Settings')),
      );
      await tester.pumpAndSettle();
      expect(controller.section, AppSection.settings);

      final diagnosticsCard = find.widgetWithText(Panel, 'Diagnostics');
      expect(diagnosticsCard, findsOneWidget);
      await tester.ensureVisible(diagnosticsCard);
      await tester.pumpAndSettle();
      expect(tester.getSize(diagnosticsCard).height, greaterThanOrEqualTo(48));
      expect(
        tester.getSemantics(diagnosticsCard).rect.height,
        greaterThanOrEqualTo(48),
      );
      expect(
        find.descendant(
          of: diagnosticsCard,
          matching: find.byIcon(LucideIcons.chevronRightDir),
        ),
        findsOneWidget,
      );
      await tester.tap(diagnosticsCard);
      await tester.pumpAndSettle();

      expect(find.byType(DiagnosticsScreen), findsOneWidget);
      expect(find.text('Diagnostics & app information'), findsOneWidget);
      expect(find.widgetWithText(TextButton, 'Back'), findsOneWidget);
      expect(find.byIcon(LucideIcons.arrowLeftDir), findsOneWidget);
      expect(tester.takeException(), isNull);

      await tester.tap(find.widgetWithText(TextButton, 'Back'));
      await tester.pumpAndSettle();
      expect(find.byType(DiagnosticsScreen), findsNothing);
      expect(find.byType(SettingsScreen), findsOneWidget);
      expect(controller.section, AppSection.settings);

      await tester.pumpWidget(const SizedBox.shrink());
      await tester.pump();
      controller.dispose();
    }
  });

  testWidgets('pushed Diagnostics listens to live AppController state', (
    tester,
  ) async {
    tester.view.devicePixelRatio = 1;
    tester.view.physicalSize = const Size(800, 900);
    addTearDown(tester.view.resetDevicePixelRatio);
    addTearDown(tester.view.resetPhysicalSize);
    SharedPreferences.setMockInitialValues(<String, Object>{
      'onboarding_complete': true,
      'update_checks_enabled': false,
    });
    final engine = EventEngineClient();
    final controller = AppController(engine);
    await controller.initialize();
    addTearDown(controller.dispose);

    await tester.pumpWidget(
      MaterialApp(
        theme: UsqueTheme.light(),
        home: SettingsScreen(controller: controller),
      ),
    );
    await tester.pumpAndSettle();
    final diagnosticsCard = find.widgetWithText(Panel, 'Diagnostics');
    await tester.ensureVisible(diagnosticsCard);
    await tester.pumpAndSettle();
    await tester.tap(diagnosticsCard);
    await tester.pumpAndSettle();
    expect(find.text('Disconnected'), findsWidgets);

    controller.snapshot = const EngineSnapshot(phase: ConnectionPhase.degraded);
    controller.selectSection(AppSection.home);
    await tester.pump();
    expect(find.text('Connected with limited connectivity'), findsWidgets);
    expect(tester.takeException(), isNull);
  });

  testWidgets('clearing all data from Diagnostics returns to onboarding', (
    tester,
  ) async {
    tester.view.devicePixelRatio = 1;
    tester.view.physicalSize = const Size(430, 900);
    addTearDown(tester.view.resetDevicePixelRatio);
    addTearDown(tester.view.resetPhysicalSize);
    SharedPreferences.setMockInitialValues(<String, Object>{
      'onboarding_complete': true,
      'update_checks_enabled': false,
    });

    await tester.pumpWidget(UsqueBootstrap(engine: FakeEngineClient()));
    await tester.pumpAndSettle();
    await tester.tap(
      find.descendant(
        of: find.byType(NavigationBar),
        matching: find.text('Settings'),
      ),
    );
    await tester.pumpAndSettle();
    final diagnosticsCard = find.widgetWithText(Panel, 'Diagnostics');
    await tester.ensureVisible(diagnosticsCard);
    await tester.pumpAndSettle();
    await tester.tap(diagnosticsCard);
    await tester.pumpAndSettle();

    final clearButton = find.widgetWithText(OutlinedButton, 'Clear all data');
    await tester.ensureVisible(clearButton);
    await tester.pumpAndSettle();
    await tester.tap(clearButton);
    await tester.pumpAndSettle();
    await tester.tap(find.widgetWithText(FilledButton, 'Clear all data'));
    await tester.pumpAndSettle();

    expect(find.byType(DiagnosticsScreen), findsNothing);
    expect(find.byType(OnboardingScreen), findsOneWidget);
    expect(tester.takeException(), isNull);
  });

  testWidgets('failed data clearing stays on Diagnostics and shows the error', (
    tester,
  ) async {
    tester.view.devicePixelRatio = 1;
    tester.view.physicalSize = const Size(430, 900);
    addTearDown(tester.view.resetDevicePixelRatio);
    addTearDown(tester.view.resetPhysicalSize);
    SharedPreferences.setMockInitialValues(<String, Object>{
      'onboarding_complete': true,
      'update_checks_enabled': false,
    });
    final engine = FakeEngineClient()
      ..clearAllDataError = const FormatException('clear failed');

    await tester.pumpWidget(UsqueBootstrap(engine: engine));
    await tester.pumpAndSettle();
    await tester.tap(
      find.descendant(
        of: find.byType(NavigationBar),
        matching: find.text('Settings'),
      ),
    );
    await tester.pumpAndSettle();
    final diagnosticsCard = find.widgetWithText(Panel, 'Diagnostics');
    await tester.ensureVisible(diagnosticsCard);
    await tester.pumpAndSettle();
    await tester.tap(diagnosticsCard);
    await tester.pumpAndSettle();

    final clearButton = find.widgetWithText(OutlinedButton, 'Clear all data');
    await tester.ensureVisible(clearButton);
    await tester.pumpAndSettle();
    await tester.tap(clearButton);
    await tester.pumpAndSettle();
    await tester.tap(find.widgetWithText(FilledButton, 'Clear all data'));
    await tester.pumpAndSettle();

    expect(find.byType(DiagnosticsScreen), findsOneWidget);
    expect(find.textContaining('clear failed'), findsOneWidget);
    expect(find.byType(OnboardingScreen), findsNothing);
    expect(tester.takeException(), isNull);
  });

  testWidgets('direct countries page enables and saves a ready country', (
    tester,
  ) async {
    tester.view.devicePixelRatio = 1;
    tester.view.physicalSize = const Size(360, 800);
    addTearDown(tester.view.resetDevicePixelRatio);
    addTearDown(tester.view.resetPhysicalSize);
    SharedPreferences.setMockInitialValues(<String, Object>{
      'onboarding_complete': true,
    });
    final engine = FakeEngineClient()
      ..storedGeoRules = const GeoRulesList(
        entries: <GeoRulesEntry>[
          GeoRulesEntry(countryCode: 'CN', hasGeoip: true, hasGeosite: true),
        ],
        hasGlobalGeosite: true,
      );
    final controller = AppController(engine);
    await controller.initialize();
    addTearDown(controller.dispose);

    await tester.pumpWidget(
      MaterialApp(
        theme: UsqueTheme.light(),
        home: GeoDirectSettingsScreen(controller: controller),
      ),
    );
    await tester.pumpAndSettle();

    await tester.enterText(find.byType(TextField), 'CN');
    await tester.pump();
    final countryTile = find.widgetWithText(ListTile, 'CN  China');
    final countrySwitch = find.descendant(
      of: countryTile,
      matching: find.byType(Switch),
    );
    expect(countrySwitch, findsOneWidget);
    await tester.tap(countrySwitch);
    await tester.tap(find.widgetWithText(FilledButton, 'Save'));
    await tester.pump();

    expect(controller.activeProfile.geoDirectCountries, const <String>['CN']);
    expect(tester.takeException(), isNull);
  });

  testWidgets(
    'direct countries can initialize data and disable an unavailable country',
    (tester) async {
      tester.view.devicePixelRatio = 1;
      tester.view.physicalSize = const Size(1280, 900);
      addTearDown(tester.view.resetDevicePixelRatio);
      addTearDown(tester.view.resetPhysicalSize);
      SharedPreferences.setMockInitialValues(<String, Object>{
        'onboarding_complete': true,
      });
      final engine = FakeEngineClient();
      final controller = AppController(engine);
      await controller.initialize();
      controller.updateNetwork(
        controller.activeProfile.copyWith(
          geoDirectCountries: const <String>['CN'],
        ),
      );
      addTearDown(controller.dispose);

      await tester.pumpWidget(
        MaterialApp(
          theme: UsqueTheme.light(),
          home: GeoDirectSettingsScreen(controller: controller),
        ),
      );
      await tester.pumpAndSettle();

      await tester.tap(
        find.widgetWithText(FilledButton, 'Update geographic data'),
      );
      await tester.pumpAndSettle();
      expect(engine.calls, contains('updateAllGeoRules'));

      await tester.enterText(find.byType(TextField), 'CN');
      await tester.pump();
      final countryTile = find.widgetWithText(ListTile, 'CN  China');
      final countrySwitch = tester.widget<Switch>(
        find.descendant(of: countryTile, matching: find.byType(Switch)),
      );
      expect(countrySwitch.value, isTrue);
      expect(countrySwitch.onChanged, isNotNull);

      await tester.tap(
        find.descendant(of: countryTile, matching: find.byType(Switch)),
      );
      await tester.tap(find.widgetWithText(FilledButton, 'Save'));
      await tester.pump();

      expect(controller.activeProfile.geoDirectCountries, isEmpty);
    },
  );

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

      final Offset engineOrigin = tester.getTopLeft(
        find.text('Usque Engine status'),
      );
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

  testWidgets(
    'error and degraded home expose Retry and the Diagnostics shortcut',
    (tester) async {
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
      await tester.pumpAndSettle();
      expect(find.text('Retry'), findsOneWidget);
      final diagnosticsButton = find.widgetWithText(
        OutlinedButton,
        'Diagnostics',
      );
      expect(diagnosticsButton, findsOneWidget);
      expect(
        tester.getSemantics(diagnosticsButton).rect.height,
        greaterThanOrEqualTo(48),
      );
      await tester.tap(diagnosticsButton);
      await tester.pumpAndSettle();
      expect(find.byType(DiagnosticsScreen), findsOneWidget);
      await tester.tap(find.widgetWithText(TextButton, 'Back'));
      await tester.pumpAndSettle();

      await tester.tap(find.text('Retry'));
      await tester.pumpAndSettle();
      expect(engine.calls, contains('retry'));
      expect(find.widgetWithText(OutlinedButton, 'Diagnostics'), findsNothing);

      controller.snapshot = const EngineSnapshot(
        phase: ConnectionPhase.degraded,
      );
      controller.selectSection(AppSection.home);
      await tester.pump();
      expect(
        find.widgetWithText(OutlinedButton, 'Diagnostics'),
        findsOneWidget,
      );
      expect(tester.takeException(), isNull);
    },
  );

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
        await Scrollable.ensureVisible(
          tester.element(tile),
          alignment: 0.5,
          duration: Duration.zero,
        );
        await tester.pumpAndSettle();
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

      await toggle('VPN (TUN)');
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
      expect(find.text('VPN'), findsOneWidget);
      expect(find.text('VPN (TUN)'), findsNothing);
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
    expect(find.widgetWithText(SwitchListTile, 'VPN (TUN)'), findsNothing);
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

  testWidgets(
    'narrow profile cards give the name its own row above identity tags',
    (tester) async {
      tester.view.devicePixelRatio = 1;
      tester.view.physicalSize = const Size(430, 900);
      addTearDown(tester.view.resetDevicePixelRatio);
      addTearDown(tester.view.resetPhysicalSize);
      SharedPreferences.setMockInitialValues(<String, Object>{
        'onboarding_complete': true,
      });
      const nameLabel = 'Work laptop';
      final engine = FakeEngineClient()
        ..legacyProfilesImported = true
        ..storedProfiles = <UsqueProfile>[
          UsqueProfile.defaultProfile().copyWith(name: nameLabel),
        ]
        ..storedIdentityStatuses = <String, ProfileIdentityStatus>{
          UsqueProfile.defaultProfileId: const ProfileIdentityStatus(
            state: ProfileIdentityState.ready,
            licenseState: LicenseState.free,
            accountType: 'Free',
          ),
        };

      await tester.pumpWidget(UsqueBootstrap(engine: engine));
      await tester.pumpAndSettle();
      await tester.tap(find.text('Profiles').last);
      await tester.pumpAndSettle();

      expect(tester.takeException(), isNull);
      final name = find.text(nameLabel);
      final identity = find.text('WARP Free');
      expect(name, findsOneWidget);
      expect(identity, findsOneWidget);
      expect(tester.getSize(name).width, greaterThan(160));
      expect(
        tester.getRect(identity).top,
        greaterThan(tester.getRect(name).bottom - 1),
      );
    },
  );
}
