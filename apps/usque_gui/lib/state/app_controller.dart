import 'dart:async';
import 'dart:convert';
import 'dart:math';

import 'package:flutter/foundation.dart';
import 'package:shared_preferences/shared_preferences.dart';

import '../core/app_strings.dart';
import '../models/app_models.dart';
import '../services/engine_client.dart';

class AppController extends ChangeNotifier {
  AppController(this._engine);

  static const int _profileSchemaVersion = 1;
  static const int _maximumProfilePayloadBytes = 1024 * 1024;
  static const String _profilesKey = 'profiles_v1';
  static const String _corruptProfilesBackupKey = 'profiles_v1_corrupt_backup';
  static const List<Duration> _snapshotReconnectDelays = <Duration>[
    Duration(seconds: 1),
    Duration(seconds: 2),
    Duration(seconds: 4),
    Duration(seconds: 8),
    Duration(seconds: 15),
    Duration(seconds: 30),
  ];

  final EngineClient _engine;
  SharedPreferences? _preferences;
  Timer? _snapshotTimer;
  Timer? _snapshotReconnectTimer;
  StreamSubscription<EngineSnapshot>? _snapshotSubscription;
  Future<void> _profileWriteTail = Future<void>.value();
  int _snapshotReconnectAttempt = 0;
  int _snapshotSubscriptionGeneration = 0;
  bool _disposed = false;

  bool initialized = false;
  bool onboardingComplete = false;
  bool busy = false;
  bool updateChecksEnabled = true;
  bool startOnBoot = false;
  bool closeToTray = true;
  bool warpProtocolAssociation = false;
  PerAppProxySettings perAppProxy = const PerAppProxySettings();
  int zeroTrustCallbackTicket = 0;
  ThemePreference themePreference = ThemePreference.system;
  LocalePreference localePreference = LocalePreference.system;
  AppSection section = AppSection.home;
  EngineSnapshot snapshot = const EngineSnapshot();
  String? lastError;
  String? lastNotice;
  bool snapshotStreamDegraded = false;
  bool _userDisconnectedThisSession = false;
  UpdateCheckResult? updateResult;
  List<UsqueProfile> profiles = <UsqueProfile>[UsqueProfile.defaultProfile()];
  String activeProfileId = UsqueProfile.defaultProfileId;
  Map<String, ProfileIdentityState> profileIdentityStates =
      <String, ProfileIdentityState>{};
  Map<String, ProfileIdentityStatus> profileIdentityStatuses =
      <String, ProfileIdentityStatus>{};

  AppStrings get strings => AppStrings(localePreference);

  UsqueProfile get activeProfile {
    return profiles.firstWhere(
      (profile) => profile.id == activeProfileId,
      orElse: UsqueProfile.defaultProfile,
    );
  }

  Future<void> initialize() async {
    _preferences = await SharedPreferences.getInstance();
    onboardingComplete = _preferences?.getBool('onboarding_complete') ?? false;
    updateChecksEnabled =
        _preferences?.getBool('update_checks_enabled') ?? true;
    themePreference = _enumByName(
      ThemePreference.values,
      _preferences?.getString('theme'),
      ThemePreference.system,
    );
    localePreference = _enumByName(
      LocalePreference.values,
      _preferences?.getString('locale'),
      LocalePreference.system,
    );
    await _loadProfiles();
    if (_disposed) {
      return;
    }
    try {
      final launchTarget = await _engine.consumeLaunchTarget();
      if (launchTarget == 'profiles') {
        section = AppSection.profiles;
      }
    } on Object {
      // A launcher shortcut is optional and must not block initialization.
    }
    try {
      final platformPreferences = await _engine.platformPreferences();
      startOnBoot = platformPreferences.startOnBoot;
      closeToTray = platformPreferences.closeToTray;
      warpProtocolAssociation = platformPreferences.warpProtocolAssociation;
    } on Object {
      // Native shell preferences are optional in unsupported test hosts.
    }
    try {
      perAppProxy = await _engine.perAppProxy();
    } on Object {
      perAppProxy = const PerAppProxySettings();
    }
    if (_engine.supportsSnapshotEvents) {
      _subscribeToSnapshotEvents();
    }
    initialized = true;
    _notifyListeners();
    unawaited(refreshSnapshot(silent: true));
    if (updateChecksEnabled) {
      unawaited(_checkForUpdates(manual: false, silent: true));
    }
    if (_shouldAutoConnectOnStart()) {
      await connectOrDisconnect();
    }
  }

  bool _shouldAutoConnectOnStart() {
    return onboardingComplete &&
        !_userDisconnectedThisSession &&
        activeProfile.autoConnect &&
        identityState(activeProfile.id) == ProfileIdentityState.ready &&
        !snapshot.isConnected &&
        !snapshot.isTransitional;
  }

  Future<void> _loadProfiles() async {
    final preferences = _preferences;
    final raw = preferences?.getString(_profilesKey);
    var legacyProfiles = <UsqueProfile>[UsqueProfile.defaultProfile()];
    var legacyActiveProfileId = legacyProfiles.first.id;
    if (preferences != null && raw != null) {
      try {
        if (utf8.encode(raw).length > _maximumProfilePayloadBytes) {
          throw const FormatException('Profile data exceeds the safety limit');
        }
        final decoded = jsonDecode(raw);
        if (decoded is! Map<String, dynamic> ||
            decoded['schema_version'] != _profileSchemaVersion ||
            decoded['profiles'] is! List) {
          throw const FormatException('Unsupported profile schema');
        }
        final decodedProfiles = (decoded['profiles'] as List<dynamic>)
            .map((value) {
              if (value is! Map) {
                throw const FormatException('Invalid profile entry');
              }
              return UsqueProfile.fromMap(Map<String, Object?>.from(value));
            })
            .toList(growable: false);
        if (decodedProfiles.isEmpty || decodedProfiles.length > 128) {
          throw const FormatException('Invalid profile count');
        }
        final ids = decodedProfiles.map((profile) => profile.id).toSet();
        if (ids.length != decodedProfiles.length) {
          throw const FormatException('Duplicate profile ID');
        }
        final active = decoded['active_profile_id'];
        if (active is! String || !ids.contains(active)) {
          throw const FormatException('Active profile is missing');
        }
        legacyProfiles = decodedProfiles;
        legacyActiveProfileId = active;
      } on Object {
        await preferences.setString(_corruptProfilesBackupKey, raw);
        await preferences.remove(_profilesKey);
        lastError =
            'Saved profiles were invalid and have been reset. A local backup was retained.';
      }
    }

    profiles = legacyProfiles;
    activeProfileId = legacyActiveProfileId;
    try {
      final catalog = await _engine.importLegacyProfiles(
        legacyProfiles,
        legacyActiveProfileId,
      );
      profiles = catalog.profiles;
      activeProfileId = catalog.activeProfileId;
      profileIdentityStates = catalog.identityStates;
      profileIdentityStatuses = catalog.identityStatuses;
      await preferences?.remove(_profilesKey);
    } on EngineException catch (error) {
      lastError ??= error.message;
    }
  }

  T _enumByName<T extends Enum>(List<T> values, String? name, T fallback) {
    for (final value in values) {
      if (value.name == name) {
        return value;
      }
    }
    return fallback;
  }

  void selectSection(AppSection value) {
    section = value;
    _notifyListeners();
  }

  Future<bool> finishOnboarding({
    IdentityProvisioningMethod method = IdentityProvisioningMethod.register,
    String? licenseKey,
  }) async {
    return _run(() async {
      await _engine.provisionIdentity(
        activeProfile,
        method: method,
        licenseKey: licenseKey,
      );
      profileIdentityStates = <String, ProfileIdentityState>{
        ...profileIdentityStates,
        activeProfile.id: ProfileIdentityState.ready,
      };
      profileIdentityStatuses = <String, ProfileIdentityStatus>{
        ...profileIdentityStatuses,
        activeProfile.id: ProfileIdentityStatus(
          state: ProfileIdentityState.ready,
          licenseState: method == IdentityProvisioningMethod.registerWithLicense
              ? LicenseState.warpPlus
              : LicenseState.free,
          accountType: method == IdentityProvisioningMethod.registerWithLicense
              ? 'WARP+'
              : 'Free',
        ),
      };
      onboardingComplete = true;
      await _preferences?.setBool('onboarding_complete', true);
    });
  }

  Future<void> connectOrDisconnect() async {
    if (snapshot.isConnected || snapshot.isTransitional) {
      await _run(() async {
        _userDisconnectedThisSession = true;
        snapshot = await _engine.disconnect();
        if (snapshot.phase == ConnectionPhase.disconnected &&
            !snapshotStreamDegraded) {
          _stopPolling();
        } else if (!_engine.supportsSnapshotEvents || snapshotStreamDegraded) {
          _startPolling(force: snapshotStreamDegraded);
        }
      });
      return;
    }

    snapshot = const EngineSnapshot(phase: ConnectionPhase.preparing);
    _notifyListeners();
    final success = await _run(() async {
      if (identityState(activeProfile.id) != ProfileIdentityState.ready) {
        throw const EngineException(
          'IDENTITY_SETUP_REQUIRED',
          'This profile needs a valid Consumer WARP identity before it can connect.',
        );
      }
      snapshot = await _engine.connect(activeProfile);
    });
    if (success && (snapshot.isConnected || snapshot.isTransitional)) {
      if (!_engine.supportsSnapshotEvents || snapshotStreamDegraded) {
        _startPolling(force: snapshotStreamDegraded);
      }
    }
  }

  Future<void> retry() async {
    final success = await _run(() async {
      snapshot = await _engine.retry();
    });
    if (success && (snapshot.isConnected || snapshot.isTransitional)) {
      if (!_engine.supportsSnapshotEvents || snapshotStreamDegraded) {
        _startPolling(force: snapshotStreamDegraded);
      }
    }
  }

  Future<void> disconnectForExit() async {
    if (snapshot.phase != ConnectionPhase.disconnected) {
      try {
        snapshot = await _engine.disconnect();
        _notifyListeners();
      } on Object {
        // The native disconnect path is fail-fast; exit must not leave the UI
        // alive indefinitely if the cleanup acknowledgement is unavailable.
      }
    }
  }

  Future<void> refreshSnapshot({bool silent = false}) async {
    try {
      final next = await _engine.snapshot();
      if (_disposed) {
        return;
      }
      snapshot = next;
      if (!snapshot.isConnected && !snapshotStreamDegraded) {
        _stopPolling();
      }
      _notifyListeners();
    } on EngineException catch (error) {
      if (!silent && !_disposed) {
        lastError = error.message;
        _notifyListeners();
      }
    }
  }

  Future<void> exportDiagnostics() async {
    String? destination;
    final success = await _run(() async {
      destination = await _engine.exportDiagnostics();
    }, affectsConnection: false);
    if (success && destination != null) {
      lastNotice = '${strings.get('diagnostics_saved')} $destination';
      _notifyListeners();
    }
  }

  Future<void> copyLicenseKey(String profileId) async {
    final success = await _run(
      () => _engine.copyLicenseKey(profileId),
      affectsConnection: false,
    );
    if (success) {
      lastNotice = strings.get('license_copied');
      _notifyListeners();
    }
  }

  Future<bool> updateProxyAuth({
    required String username,
    required String password,
  }) async {
    final profile = activeProfile;
    final success = await _run(() async {
      await _engine.updateProxyAuth(
        profile.id,
        username: username,
        password: password,
        confirmed: true,
      );
      final next = profile.copyWith(
        proxy: profile.proxy.copyWith(authUsername: username),
      );
      if (profile.id == activeProfileId && snapshot.isConnected) {
        await _engine.reconfigureActiveProfile(next);
      } else {
        await _engine.upsertProfile(next);
      }
      profiles = profiles
          .map((item) => item.id == next.id ? next : item)
          .toList(growable: false);
    });
    if (success) {
      lastNotice = username.isEmpty
          ? strings.get('proxy_auth_cleared')
          : strings.get('proxy_auth_saved');
      _notifyListeners();
    }
    return success;
  }

  Future<bool> updateLicenseKey(String profileId, String licenseKey) async {
    final success = await _run(() async {
      final reconnect = profileId == activeProfileId && snapshot.isConnected;
      if (reconnect) {
        snapshot = await _engine.disconnect();
        _notifyListeners();
      }
      try {
        await _engine.updateLicenseKey(profileId, licenseKey);
        await _refreshProfileCatalog();
      } finally {
        if (reconnect) {
          snapshot = await _engine.connect(activeProfile);
          _notifyListeners();
        }
      }
    });
    return success;
  }

  Future<bool> unbindLicenseKey(String profileId) async {
    final success = await _run(() async {
      final reconnect = profileId == activeProfileId && snapshot.isConnected;
      if (reconnect) {
        snapshot = await _engine.disconnect();
        _notifyListeners();
      }
      try {
        await _engine.unbindLicenseKey(profileId);
        await _refreshProfileCatalog();
      } finally {
        if (reconnect) {
          snapshot = await _engine.connect(activeProfile);
          _notifyListeners();
        }
      }
    });
    return success;
  }

  Future<void> exportWarpSecret(String profileId) async {
    String? destination;
    final success = await _run(() async {
      destination = await _engine.exportWarpSecret(profileId);
    }, affectsConnection: false);
    if (success && destination != null) {
      lastNotice = '${strings.get('warp_secret_saved')} $destination';
      _notifyListeners();
    }
  }

  Future<void> _refreshProfileCatalog() async {
    final catalog = await _engine.importLegacyProfiles(
      const <UsqueProfile>[],
      '',
    );
    profiles = catalog.profiles;
    activeProfileId = catalog.activeProfileId;
    profileIdentityStates = catalog.identityStates;
    profileIdentityStatuses = catalog.identityStatuses;
  }

  Future<void> checkForUpdates() async {
    await _checkForUpdates(manual: true, silent: false);
  }

  Future<void> clearAllData() async {
    await flushProfileWrites();
    final success = await _run(() async {
      await _engine.clearAllData(confirmed: true);
      await _preferences?.clear();
      onboardingComplete = false;
      updateChecksEnabled = true;
      themePreference = ThemePreference.system;
      localePreference = LocalePreference.system;
      section = AppSection.home;
      snapshot = const EngineSnapshot();
      profiles = <UsqueProfile>[UsqueProfile.defaultProfile()];
      activeProfileId = UsqueProfile.defaultProfileId;
      profileIdentityStates = <String, ProfileIdentityState>{};
      profileIdentityStatuses = <String, ProfileIdentityStatus>{};
      updateResult = null;
      perAppProxy = const PerAppProxySettings();
    }, affectsConnection: false);
    if (success) {
      lastNotice = strings.get('clear_all_data_complete');
      _notifyListeners();
    }
  }

  Future<void> _checkForUpdates({
    required bool manual,
    required bool silent,
  }) async {
    if (silent) {
      try {
        final result = await _engine.checkForUpdates(manual: manual);
        if (_disposed) {
          return;
        }
        updateResult = result;
        if (result.available) {
          lastNotice =
              '${strings.get('update_available')} ${result.version ?? ''}'
                  .trim();
          _notifyListeners();
        }
      } on Object {
        // Automatic checks are optional and must not affect tunnel state.
      }
      return;
    }

    UpdateCheckResult? checked;
    final success = await _run(() async {
      checked = await _engine.checkForUpdates(manual: manual);
    }, affectsConnection: false);
    if (success && checked != null) {
      updateResult = checked;
      lastNotice = checked!.available
          ? '${strings.get('update_available')} ${checked!.version ?? ''}'
                .trim()
          : strings.get('already_latest');
      _notifyListeners();
    }
  }

  Future<bool> _run(
    Future<void> Function() operation, {
    bool affectsConnection = true,
  }) async {
    busy = true;
    lastError = null;
    _notifyListeners();
    try {
      await operation();
      return true;
    } on EngineException catch (error) {
      lastError = error.message;
      if (affectsConnection && snapshot.phase != ConnectionPhase.disconnected) {
        snapshot = EngineSnapshot(
          phase: ConnectionPhase.error,
          warning: error.message,
        );
      }
      return false;
    } catch (error) {
      lastError = error.toString();
      return false;
    } finally {
      busy = false;
      _notifyListeners();
    }
  }

  void clearError() {
    lastError = null;
    _notifyListeners();
  }

  void clearNotice() {
    lastNotice = null;
    _notifyListeners();
  }

  Future<void> setTheme(ThemePreference value) async {
    themePreference = value;
    _notifyListeners();
    await _preferences?.setString('theme', value.name);
  }

  Future<void> setLocale(LocalePreference value) async {
    localePreference = value;
    _notifyListeners();
    await _preferences?.setString('locale', value.name);
  }

  Future<void> setUpdateChecks(bool value) async {
    updateChecksEnabled = value;
    _notifyListeners();
    await _preferences?.setBool('update_checks_enabled', value);
  }

  Future<void> setPerAppProxy(PerAppProxySettings value) async {
    final previous = perAppProxy;
    perAppProxy = value;
    _notifyListeners();
    try {
      perAppProxy = await _engine.setPerAppProxy(value);
      if (snapshot.isConnected) {
        await refreshSnapshot(silent: true);
      }
    } on Object catch (error) {
      perAppProxy = previous;
      lastError = error is EngineException ? error.message : error.toString();
    }
    _notifyListeners();
  }

  Future<List<InstalledAppInfo>> listInstalledApps() =>
      _engine.listInstalledApps();

  Future<Uint8List?> getAppIcon(String packageName) =>
      _engine.getAppIcon(packageName);

  Future<void> setStartOnBoot(bool value) async {
    final previous = startOnBoot;
    startOnBoot = value;
    _notifyListeners();
    try {
      await _engine.setStartOnBoot(value);
    } on Object catch (error) {
      startOnBoot = previous;
      lastError = error is EngineException ? error.message : error.toString();
      _notifyListeners();
    }
  }

  Future<void> setCloseToTray(bool value) async {
    final previous = closeToTray;
    closeToTray = value;
    _notifyListeners();
    try {
      await _engine.setCloseToTray(value);
    } on Object catch (error) {
      closeToTray = previous;
      lastError = error is EngineException ? error.message : error.toString();
      _notifyListeners();
    }
  }

  Future<void> setWarpProtocolAssociation(bool value) async {
    final previous = warpProtocolAssociation;
    warpProtocolAssociation = value;
    _notifyListeners();
    try {
      await _engine.setWarpProtocolAssociation(value);
    } on Object catch (error) {
      warpProtocolAssociation = previous;
      lastError = error is EngineException ? error.message : error.toString();
      _notifyListeners();
    }
  }

  void noteZeroTrustCallbackArrived() {
    zeroTrustCallbackTicket += 1;
    _notifyListeners();
  }

  Future<void> requestAddQuickSettingsTile() =>
      _run(_engine.requestAddQuickSettingsTile, affectsConnection: false);

  Future<void> openAlwaysOnVpnSettings() =>
      _run(_engine.openAlwaysOnVpnSettings, affectsConnection: false);

  void addProfile(String name) {
    final normalized = name.trim();
    if (normalized.isEmpty || normalized.runes.length > 64) {
      return;
    }
    final id = _newUuidV4();
    final added = UsqueProfile.defaultProfile().copyWith(
      id: id,
      name: normalized,
    );
    profiles = <UsqueProfile>[...profiles, added];
    profileIdentityStates = <String, ProfileIdentityState>{
      ...profileIdentityStates,
      added.id: ProfileIdentityState.missing,
    };
    profileIdentityStatuses = <String, ProfileIdentityStatus>{
      ...profileIdentityStatuses,
      added.id: const ProfileIdentityStatus(
        state: ProfileIdentityState.missing,
      ),
    };
    _notifyListeners();
    _queueProfileMutation(() => _engine.upsertProfile(added));
  }

  ProfileIdentityState identityState(String profileId) =>
      profileIdentityStates[profileId] ?? ProfileIdentityState.missing;

  ProfileIdentityStatus identityStatus(String profileId) =>
      profileIdentityStatuses[profileId] ??
      ProfileIdentityStatus(state: identityState(profileId));

  Future<bool> createProfileWithIdentity(
    String name, {
    required IdentityProvisioningMethod method,
    String? licenseKey,
    String? teamName,
    String? callbackUri,
  }) async {
    final normalized = name.trim();
    if (normalized.isEmpty || normalized.runes.length > 64) return false;
    final profile = UsqueProfile.defaultProfile().copyWith(
      id: _newUuidV4(),
      name: normalized,
    );
    ProfileCatalog? catalog;
    final success = await _run(() async {
      catalog = await _engine.createProfileWithIdentity(
        profile,
        method: method,
        licenseKey: licenseKey,
        teamName: teamName,
        callbackUri: callbackUri,
      );
      profiles = catalog!.profiles;
      activeProfileId = catalog!.activeProfileId;
      profileIdentityStates = catalog!.identityStates;
      profileIdentityStatuses = catalog!.identityStatuses;
    }, affectsConnection: false);
    return success;
  }

  Future<bool> provisionProfileIdentity(
    UsqueProfile profile, {
    required IdentityProvisioningMethod method,
    String? licenseKey,
    String? teamName,
    String? callbackUri,
  }) async {
    final success = await _run(() async {
      final reconnect = profile.id == activeProfileId && snapshot.isConnected;
      var mutationCommitted = false;
      var refreshedZeroTrustProfile = false;
      if (reconnect) {
        snapshot = await _engine.disconnect();
        _notifyListeners();
      }
      try {
        await _engine.provisionIdentity(
          profile,
          method: method,
          licenseKey: licenseKey,
          teamName: teamName,
          callbackUri: callbackUri,
        );
        mutationCommitted = true;
        if (method == IdentityProvisioningMethod.zeroTrust) {
          await _refreshProfileCatalog();
          refreshedZeroTrustProfile = true;
          return;
        }
        profileIdentityStates = <String, ProfileIdentityState>{
          ...profileIdentityStates,
          profile.id: ProfileIdentityState.ready,
        };
        profileIdentityStatuses = <String, ProfileIdentityStatus>{
          ...profileIdentityStatuses,
          profile.id: ProfileIdentityStatus(
            state: ProfileIdentityState.ready,
            licenseState:
                method == IdentityProvisioningMethod.registerWithLicense
                ? LicenseState.warpPlus
                : LicenseState.free,
            accountType:
                method == IdentityProvisioningMethod.registerWithLicense
                ? 'WARP+'
                : 'Free',
            provider: IdentityProvider.consumer,
          ),
        };
      } finally {
        final safeToReconnect =
            !mutationCommitted ||
            method != IdentityProvisioningMethod.zeroTrust ||
            refreshedZeroTrustProfile;
        if (reconnect && safeToReconnect) {
          snapshot = await _engine.connect(activeProfile);
          _notifyListeners();
        }
      }
    }, affectsConnection: false);
    return success;
  }

  Future<String> beginZeroTrustLogin(String teamName) async {
    final team = teamName.trim().toLowerCase();
    final nativeUrl = await _engine.beginZeroTrustLogin(team);
    return nativeUrl ?? 'https://$team.cloudflareaccess.com/warp';
  }

  Future<String?> consumeZeroTrustCallback() =>
      _engine.consumeZeroTrustCallback();

  Future<void> cancelZeroTrustLogin() => _engine.cancelZeroTrustLogin();

  void updateProfile(UsqueProfile updated) {
    if (!profiles.any((profile) => profile.id == updated.id)) {
      return;
    }
    final normalized = updated.frontends.http
        ? updated
        : updated.copyWith(proxy: updated.proxy.copyWith(systemProxy: false));
    profiles = profiles
        .map((profile) => profile.id == normalized.id ? normalized : profile)
        .toList(growable: false);
    _notifyListeners();
    _queueProfileMutation(() {
      if (normalized.id == activeProfileId && snapshot.isConnected) {
        return _engine.reconfigureActiveProfile(normalized);
      }
      return _engine.upsertProfile(normalized);
    });
  }

  void setActiveProfile(String id) {
    if (profiles.any((profile) => profile.id == id)) {
      activeProfileId = id;
      _notifyListeners();
      _queueProfileMutation(() => _engine.setActiveProfile(id));
    }
  }

  bool deleteProfile(String id) {
    if (profiles.length == 1) {
      return false;
    }
    profiles = profiles.where((profile) => profile.id != id).toList();
    profileIdentityStates = Map<String, ProfileIdentityState>.from(
      profileIdentityStates,
    )..remove(id);
    profileIdentityStatuses = Map<String, ProfileIdentityStatus>.from(
      profileIdentityStatuses,
    )..remove(id);
    if (activeProfileId == id) {
      activeProfileId = profiles.first.id;
    }
    _notifyListeners();
    _queueProfileMutation(() => _engine.deleteProfile(id));
    return true;
  }

  void _queueProfileMutation(Future<void> Function() mutation) {
    _profileWriteTail = _profileWriteTail.then((_) async {
      try {
        await mutation();
      } on Object catch (error) {
        lastError = 'Profile changes could not be saved: $error';
        try {
          final catalog = await _engine.importLegacyProfiles(
            const <UsqueProfile>[],
            '',
          );
          profiles = catalog.profiles;
          activeProfileId = catalog.activeProfileId;
          profileIdentityStates = catalog.identityStates;
          profileIdentityStatuses = catalog.identityStatuses;
        } on Object {
          // Keep the optimistic in-memory state when the authoritative store
          // cannot be reloaded; the original mutation error remains visible.
        }
        _notifyListeners();
      }
    });
  }

  /// Waits for already queued non-secret profile writes. Installers and tests
  /// can use this before terminating the UI process.
  Future<void> flushProfileWrites() => _profileWriteTail;

  void _notifyListeners() {
    if (!_disposed) {
      notifyListeners();
    }
  }

  void _startPolling({bool force = false}) {
    if (_engine.supportsSnapshotEvents && !force) {
      return;
    }
    if (_snapshotTimer != null) {
      return;
    }
    _snapshotTimer = Timer.periodic(
      const Duration(seconds: 1),
      (_) => unawaited(refreshSnapshot(silent: true)),
    );
  }

  void _stopPolling() {
    _snapshotTimer?.cancel();
    _snapshotTimer = null;
  }

  void _subscribeToSnapshotEvents() {
    if (_disposed || !_engine.supportsSnapshotEvents) {
      return;
    }
    _snapshotReconnectTimer?.cancel();
    _snapshotReconnectTimer = null;
    final previous = _snapshotSubscription;
    _snapshotSubscription = null;
    if (previous != null) {
      unawaited(previous.cancel());
    }
    final generation = ++_snapshotSubscriptionGeneration;
    _snapshotSubscription = _engine.snapshotEvents.listen(
      (EngineSnapshot next) => _handleSnapshotEvent(next, generation),
      onError: (Object error, StackTrace stackTrace) =>
          _handleSnapshotEventError(error, stackTrace, generation),
      onDone: () => _handleSnapshotEventDone(generation),
      cancelOnError: true,
    );
  }

  void _handleSnapshotEvent(EngineSnapshot next, int generation) {
    if (_disposed || generation != _snapshotSubscriptionGeneration) {
      return;
    }
    final wasDegraded = snapshotStreamDegraded;
    _snapshotReconnectAttempt = 0;
    _snapshotReconnectTimer?.cancel();
    _snapshotReconnectTimer = null;
    snapshotStreamDegraded = false;
    _stopPolling();
    final nextError =
        next.phase == ConnectionPhase.error &&
            (next.warning?.trim().isNotEmpty ?? false)
        ? <String?>[
            next.errorCode?.trim(),
            next.warning?.trim(),
          ].whereType<String>().where((part) => part.isNotEmpty).join(': ')
        : null;
    final errorChanged = nextError != null && nextError != lastError;
    final snapshotChanged = next != snapshot;
    if (!snapshotChanged && !errorChanged && !wasDegraded) {
      return;
    }
    snapshot = next;
    if (errorChanged) {
      lastError = nextError;
    }
    _notifyListeners();
  }

  void _handleSnapshotEventError(
    Object error,
    StackTrace stackTrace,
    int generation,
  ) {
    _markSnapshotStreamUnavailable(generation);
  }

  void _handleSnapshotEventDone(int generation) {
    _markSnapshotStreamUnavailable(generation);
  }

  void _markSnapshotStreamUnavailable(int generation) {
    if (_disposed || generation != _snapshotSubscriptionGeneration) {
      return;
    }
    _snapshotSubscription = null;
    snapshotStreamDegraded = true;
    _startPolling(force: true);
    if (_snapshotReconnectTimer == null) {
      final delay =
          _snapshotReconnectDelays[_snapshotReconnectAttempt.clamp(
            0,
            _snapshotReconnectDelays.length - 1,
          )];
      if (_snapshotReconnectAttempt < _snapshotReconnectDelays.length - 1) {
        _snapshotReconnectAttempt += 1;
      }
      _snapshotReconnectTimer = Timer(delay, () {
        _snapshotReconnectTimer = null;
        _subscribeToSnapshotEvents();
      });
    }
    _notifyListeners();
  }

  @override
  void dispose() {
    _disposed = true;
    _stopPolling();
    _snapshotReconnectTimer?.cancel();
    _snapshotReconnectTimer = null;
    _snapshotSubscriptionGeneration += 1;
    unawaited(_snapshotSubscription?.cancel());
    _snapshotSubscription = null;
    unawaited(_profileWriteTail.whenComplete(_engine.dispose));
    super.dispose();
  }
}

String _newUuidV4() {
  final random = Random.secure();
  final bytes = List<int>.generate(16, (_) => random.nextInt(256));
  bytes[6] = (bytes[6] & 0x0f) | 0x40;
  bytes[8] = (bytes[8] & 0x3f) | 0x80;
  final hex = bytes
      .map((value) => value.toRadixString(16).padLeft(2, '0'))
      .join();
  return '${hex.substring(0, 8)}-${hex.substring(8, 12)}-'
      '${hex.substring(12, 16)}-${hex.substring(16, 20)}-'
      '${hex.substring(20)}';
}
