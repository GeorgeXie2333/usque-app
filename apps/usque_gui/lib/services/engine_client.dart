import 'dart:async';
import 'dart:ui';

import 'package:flutter/services.dart';

import '../models/app_models.dart';

class EngineException implements Exception {
  const EngineException(this.code, this.message);

  final String code;
  final String message;

  @override
  String toString() => message;
}

abstract interface class EngineClient {
  bool get supportsSnapshotEvents;

  Stream<EngineSnapshot> get snapshotEvents;

  Future<ProfileCatalog> importLegacyProfiles(
    List<UsqueProfile> profiles,
    String activeProfileId,
  );

  Future<void> upsertProfile(UsqueProfile profile);

  Future<void> deleteProfile(String profileId);

  Future<void> setActiveProfile(String profileId);

  Future<void> provisionIdentity(
    UsqueProfile profile, {
    required IdentityProvisioningMethod method,
    String? licenseKey,
    String? teamName,
    String? callbackUri,
  });

  Future<ProfileCatalog> createProfileWithIdentity(
    UsqueProfile profile, {
    required IdentityProvisioningMethod method,
    String? licenseKey,
    String? teamName,
    String? callbackUri,
  });

  Future<void> reconfigureActiveProfile(UsqueProfile profile);

  Future<void> updateProxyAuth(
    String profileId, {
    required String username,
    required String password,
    bool confirmed = true,
  });

  Future<void> copyLicenseKey(String profileId);

  Future<void> updateLicenseKey(String profileId, String licenseKey);

  Future<void> unbindLicenseKey(String profileId);

  Future<String?> exportWarpSecret(String profileId);

  Future<String?> consumeLaunchTarget();

  Future<String?> beginZeroTrustLogin(String teamName);

  Future<String?> consumeZeroTrustCallback();

  Future<void> cancelZeroTrustLogin();

  Future<PlatformPreferences> platformPreferences();

  Future<void> setStartOnBoot(bool enabled);

  Future<void> setCloseToTray(bool enabled);

  Future<void> setWarpProtocolAssociation(bool enabled);

  Future<void> requestAddQuickSettingsTile();

  Future<PerAppProxySettings> perAppProxy();

  Future<PerAppProxySettings> setPerAppProxy(PerAppProxySettings settings);

  Future<List<InstalledAppInfo>> listInstalledApps();

  Future<Uint8List?> getAppIcon(String packageName);

  Future<EngineSnapshot> connect(UsqueProfile profile);

  Future<EngineSnapshot> disconnect();

  Future<EngineSnapshot> retry();

  Future<EngineSnapshot> snapshot();

  Future<void> openAlwaysOnVpnSettings();

  Future<String?> exportDiagnostics();

  Future<UpdateCheckResult> checkForUpdates({bool manual = true});

  Future<void> clearAllData({required bool confirmed});

  void dispose();
}

class MethodChannelEngineClient implements EngineClient {
  static const MethodChannel _channel = MethodChannel(
    'io.github.georgexie2333.usque/engine',
  );
  static const EventChannel _events = EventChannel(
    'io.github.georgexie2333.usque/engine_events',
  );

  @override
  bool get supportsSnapshotEvents => true;

  @override
  Stream<EngineSnapshot> get snapshotEvents {
    return _events.receiveBroadcastStream().map((Object? value) {
      if (value is! Map) {
        throw const EngineException(
          'ENGINE_EVENT_INVALID',
          'The Android VPN process sent an invalid status event.',
        );
      }
      return EngineSnapshot.fromMap(Map<Object?, Object?>.from(value));
    });
  }

  @override
  Future<ProfileCatalog> importLegacyProfiles(
    List<UsqueProfile> profiles,
    String activeProfileId,
  ) async {
    final result = await _invoke<Map<Object?, Object?>>(
      'importLegacyProfiles',
      <String, Object>{
        'profiles': profiles.map((profile) => profile.toMap()).toList(),
        'active_profile_id': activeProfileId,
      },
    );
    final map = result ?? const <Object?, Object?>{};
    final decodedProfiles =
        (map['profiles'] as List?)
            ?.whereType<Map<Object?, Object?>>()
            .map(
              (profile) =>
                  UsqueProfile.fromMap(Map<String, Object?>.from(profile)),
            )
            .toList(growable: false) ??
        const <UsqueProfile>[];
    final active = map['active_profile_id'] as String?;
    if (decodedProfiles.isEmpty || active == null) {
      throw const EngineException(
        'CONFIGURATION_INVALID',
        'The Rust profile store returned an invalid catalog.',
      );
    }
    return ProfileCatalog(
      profiles: decodedProfiles,
      activeProfileId: active,
      identityStates: _identityStatesFromMap(map),
      identityStatuses: _identityStatusesFromMap(map),
    );
  }

  @override
  Future<String?> consumeLaunchTarget() =>
      _invoke<String>('consumeLaunchTarget');

  @override
  Future<String?> beginZeroTrustLogin(String teamName) => _invoke<String>(
    'beginZeroTrustLogin',
    <String, Object>{'team_name': teamName},
  );

  @override
  Future<String?> consumeZeroTrustCallback() =>
      _invoke<String>('consumeZeroTrustCallback');

  @override
  Future<void> cancelZeroTrustLogin() => _invoke<void>('cancelZeroTrustLogin');

  @override
  Future<PlatformPreferences> platformPreferences() async {
    final value = await _invoke<Map<Object?, Object?>>('platformPreferences');
    return PlatformPreferences.fromMap(value ?? const <Object?, Object?>{});
  }

  @override
  Future<void> setStartOnBoot(bool enabled) =>
      _invoke<void>('setStartOnBoot', <String, Object>{'enabled': enabled});

  @override
  Future<void> setCloseToTray(bool enabled) async {}

  @override
  Future<void> setWarpProtocolAssociation(bool enabled) async {}

  @override
  Future<void> requestAddQuickSettingsTile() =>
      _invoke<void>('requestAddQuickSettingsTile');

  @override
  Future<PerAppProxySettings> perAppProxy() async {
    final value = await _invoke<Map<Object?, Object?>>('perAppProxy');
    return PerAppProxySettings.fromMap(value ?? const <Object?, Object?>{});
  }

  @override
  Future<PerAppProxySettings> setPerAppProxy(PerAppProxySettings settings) async {
    final value = await _invoke<Map<Object?, Object?>>(
      'setPerAppProxy',
      settings.toMap(),
    );
    return PerAppProxySettings.fromMap(value ?? settings.toMap());
  }

  @override
  Future<List<InstalledAppInfo>> listInstalledApps() async {
    final value = await _invoke<List<Object?>>('listInstalledApps');
    return (value ?? const <Object?>[])
        .whereType<Map<Object?, Object?>>()
        .map(InstalledAppInfo.fromMap)
        .where((app) => app.packageName.isNotEmpty)
        .toList(growable: false);
  }

  @override
  Future<Uint8List?> getAppIcon(String packageName) =>
      _invoke<Uint8List>('getAppIcon', <String, Object>{
        'package_name': packageName,
      });

  @override
  Future<void> openAlwaysOnVpnSettings() =>
      _invoke<void>('openAlwaysOnVpnSettings');

  @override
  Future<void> upsertProfile(UsqueProfile profile) =>
      _invoke<void>('upsertProfile', profile.toMap());

  @override
  Future<void> deleteProfile(String profileId) =>
      _invoke<void>('deleteProfile', <String, Object>{'profile_id': profileId});

  @override
  Future<void> setActiveProfile(String profileId) => _invoke<void>(
    'setActiveProfile',
    <String, Object>{'profile_id': profileId},
  );

  @override
  Future<void> provisionIdentity(
    UsqueProfile profile, {
    required IdentityProvisioningMethod method,
    String? licenseKey,
    String? teamName,
    String? callbackUri,
  }) async {
    await _invoke<void>('provisionIdentity', <String, Object?>{
      'profile_id': profile.id,
      'method': method.name,
      'license_key': licenseKey,
      'team_name': teamName,
      'callback_uri': callbackUri,
      'terms_accepted': true,
      'locale': PlatformDispatcher.instance.locale.toLanguageTag(),
    });
  }

  @override
  Future<ProfileCatalog> createProfileWithIdentity(
    UsqueProfile profile, {
    required IdentityProvisioningMethod method,
    String? licenseKey,
    String? teamName,
    String? callbackUri,
  }) async {
    final result = await _invoke<Map<Object?, Object?>>(
      'createProfileWithIdentity',
      <String, Object?>{
        'profile': profile.toMap(),
        'method': method.name,
        'license_key': licenseKey,
        'team_name': teamName,
        'callback_uri': callbackUri,
        'terms_accepted': true,
        'locale': PlatformDispatcher.instance.locale.toLanguageTag(),
      },
    );
    final map = result ?? const <Object?, Object?>{};
    final profiles =
        (map['profiles'] as List?)
            ?.whereType<Map<Object?, Object?>>()
            .map(
              (value) => UsqueProfile.fromMap(Map<String, Object?>.from(value)),
            )
            .toList(growable: false) ??
        const <UsqueProfile>[];
    final active = map['active_profile_id'] as String?;
    if (profiles.isEmpty || active == null) {
      throw const EngineException(
        'CONFIGURATION_INVALID',
        'The native profile store returned an invalid catalog.',
      );
    }
    return ProfileCatalog(
      profiles: profiles,
      activeProfileId: active,
      identityStates: _identityStatesFromMap(map),
      identityStatuses: _identityStatusesFromMap(map),
    );
  }

  @override
  Future<void> reconfigureActiveProfile(UsqueProfile profile) {
    return _invoke<void>('reconfigureActiveProfile', profile.toMap());
  }

  @override
  Future<void> updateProxyAuth(
    String profileId, {
    required String username,
    required String password,
    bool confirmed = true,
  }) {
    return _invoke<void>('updateProxyAuth', <String, Object>{
      'profile_id': profileId,
      'username': username,
      'password': password,
      'confirmed': confirmed,
    });
  }

  @override
  Future<void> copyLicenseKey(String profileId) => _invoke<void>(
    'copyLicenseKey',
    <String, Object>{'profile_id': profileId},
  );

  @override
  Future<void> updateLicenseKey(String profileId, String licenseKey) =>
      _invoke<void>('updateLicenseKey', <String, Object>{
        'profile_id': profileId,
        'license_key': licenseKey,
      });

  @override
  Future<void> unbindLicenseKey(String profileId) => _invoke<void>(
    'unbindLicenseKey',
    <String, Object>{'profile_id': profileId},
  );

  @override
  Future<String?> exportWarpSecret(String profileId) => _invoke<String>(
    'exportWarpSecret',
    <String, Object>{'profile_id': profileId},
  );

  @override
  Future<EngineSnapshot> connect(UsqueProfile profile) async {
    final result = await _invoke<Map<Object?, Object?>>(
      'connect',
      profile.toMap(),
    );
    return EngineSnapshot.fromMap(result ?? const <Object?, Object?>{});
  }

  @override
  Future<EngineSnapshot> disconnect() async {
    final result = await _invoke<Map<Object?, Object?>>('disconnect');
    return EngineSnapshot.fromMap(result ?? const <Object?, Object?>{});
  }

  @override
  Future<EngineSnapshot> retry() async {
    final result = await _invoke<Map<Object?, Object?>>('retry');
    return EngineSnapshot.fromMap(result ?? const <Object?, Object?>{});
  }

  @override
  Future<EngineSnapshot> snapshot() async {
    final result = await _invoke<Map<Object?, Object?>>('snapshot');
    return EngineSnapshot.fromMap(result ?? const <Object?, Object?>{});
  }

  @override
  Future<String?> exportDiagnostics() => _invoke<String>('exportDiagnostics');

  @override
  Future<UpdateCheckResult> checkForUpdates({bool manual = true}) async {
    final result = await _invoke<Map<Object?, Object?>>(
      'checkForUpdates',
      <String, Object>{'manual': manual},
    );
    return UpdateCheckResult.fromMap(result ?? const <Object?, Object?>{});
  }

  @override
  Future<void> clearAllData({required bool confirmed}) =>
      _invoke<void>('clearAllData', <String, Object>{'confirmed': confirmed});

  @override
  void dispose() {}

  Future<T?> _invoke<T>(String method, [Object? arguments]) async {
    try {
      return await _channel.invokeMethod<T>(method, arguments);
    } on MissingPluginException {
      throw const EngineException(
        'ENGINE_UNAVAILABLE',
        'The native Usque Engine is not available in this build yet.',
      );
    } on PlatformException catch (error) {
      throw EngineException(
        error.code,
        error.message ?? 'The native engine rejected this operation.',
      );
    }
  }
}

Map<String, ProfileIdentityState> _identityStatesFromMap(
  Map<Object?, Object?> map,
) {
  final states = <String, ProfileIdentityState>{};
  for (final value in (map['identity_statuses'] as List?) ?? const <Object>[]) {
    if (value is! Map) continue;
    final id = value['profile_id'];
    final raw = value['state'];
    if (id is! String || raw is! String) continue;
    states[id] = ProfileIdentityState.values.firstWhere(
      (state) => state.name == raw,
      orElse: () => ProfileIdentityState.invalid,
    );
  }
  return Map<String, ProfileIdentityState>.unmodifiable(states);
}

Map<String, ProfileIdentityStatus> _identityStatusesFromMap(
  Map<Object?, Object?> map,
) {
  final statuses = <String, ProfileIdentityStatus>{};
  for (final value in (map['identity_statuses'] as List?) ?? const <Object>[]) {
    if (value is! Map) continue;
    final id = value['profile_id'];
    final rawState = value['state'];
    if (id is! String || rawState is! String) continue;
    final state = ProfileIdentityState.values.firstWhere(
      (item) => item.name == rawState,
      orElse: () => ProfileIdentityState.invalid,
    );
    final rawLicense = value['license_state'];
    final licenseState = LicenseState.values.firstWhere(
      (item) => item.name == rawLicense,
      orElse: () => LicenseState.unknown,
    );
    statuses[id] = ProfileIdentityStatus(
      state: state,
      licenseState: licenseState,
      accountType: value['account_type'] as String? ?? '',
      cleanupPending: value['cleanup_pending'] as bool? ?? false,
      provider: IdentityProvider.values.firstWhere(
        (item) => item.name == value['provider'],
        orElse: () => IdentityProvider.consumer,
      ),
      organization: value['organization'] as String? ?? '',
    );
  }
  return Map<String, ProfileIdentityStatus>.unmodifiable(statuses);
}
