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

  Future<void> provisionIdentity(UsqueProfile profile, {String? warpSecret});

  Future<ProfileCatalog> createProfileWithIdentity(
    UsqueProfile profile, {
    required IdentityProvisioningMethod method,
    String? warpSecret,
  });

  Future<EngineSnapshot> connect(UsqueProfile profile);

  Future<EngineSnapshot> disconnect();

  Future<EngineSnapshot> snapshot();

  Future<EngineSnapshot> pauseCaptivePortal({int seconds = 600});

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
            ?.whereType<Map>()
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
    );
  }

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
    String? warpSecret,
  }) async {
    await _invoke<void>('provisionIdentity', <String, Object?>{
      'profile_id': profile.id,
      'warp_secret': warpSecret,
      'terms_accepted': true,
      'locale': PlatformDispatcher.instance.locale.toLanguageTag(),
    });
  }

  @override
  Future<ProfileCatalog> createProfileWithIdentity(
    UsqueProfile profile, {
    required IdentityProvisioningMethod method,
    String? warpSecret,
  }) async {
    final result = await _invoke<Map<Object?, Object?>>(
      'createProfileWithIdentity',
      <String, Object?>{
        'profile': profile.toMap(),
        'method': method.name,
        'warp_secret': warpSecret,
        'terms_accepted': true,
        'locale': PlatformDispatcher.instance.locale.toLanguageTag(),
      },
    );
    final map = result ?? const <Object?, Object?>{};
    final profiles =
        (map['profiles'] as List?)
            ?.whereType<Map>()
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
    );
  }

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
  Future<EngineSnapshot> snapshot() async {
    final result = await _invoke<Map<Object?, Object?>>('snapshot');
    return EngineSnapshot.fromMap(result ?? const <Object?, Object?>{});
  }

  @override
  Future<EngineSnapshot> pauseCaptivePortal({int seconds = 600}) async {
    final result = await _invoke<Map<Object?, Object?>>(
      'pauseCaptivePortal',
      <String, Object>{'seconds': seconds},
    );
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
