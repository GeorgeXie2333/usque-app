import 'dart:async';
import 'dart:convert';
import 'dart:math';

import 'package:flutter/foundation.dart';
import 'package:shared_preferences/shared_preferences.dart';

import '../core/app_strings.dart';
import '../models/app_models.dart';
import '../services/engine_client.dart';
import '../services/update_downloader.dart';
import 'diagnostics_controller.dart';
import 'network_quality_controller.dart';

class AppController extends ChangeNotifier {
  AppController(
    EngineClient engine, {
    UpdateDownloader? updateDownloader,
    NetworkQualityController? qualityController,
  }) : _engine = engine,
       _updateDownloader = updateDownloader ?? UpdateDownloader(engine),
       diagnostics = DiagnosticsController(engine),
       quality = qualityController ?? NetworkQualityController(engine);

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
  final UpdateDownloader _updateDownloader;
  final DiagnosticsController diagnostics;
  final NetworkQualityController quality;
  SharedPreferences? _preferences;
  Timer? _snapshotTimer;
  Timer? _snapshotReconnectTimer;
  StreamSubscription<EngineSnapshotEvent>? _snapshotSubscription;
  Future<void> _profileWriteTail = Future<void>.value();
  int _snapshotReconnectAttempt = 0;
  int _snapshotSubscriptionGeneration = 0;
  bool _snapshotStreamEstablished = false;
  bool _startupUpdateCheckStarted = false;
  bool _disposed = false;
  int _updateOperationGeneration = 0;
  UpdateDownloadCancellation? _updateCancellation;

  bool initialized = false;
  bool onboardingComplete = false;
  bool busy = false;
  int _activeOperations = 0;
  bool updateChecksEnabled = true;
  bool startOnBoot = false;
  bool closeToTray = true;
  bool warpProtocolAssociation = false;
  PerAppProxySettings perAppProxy = const PerAppProxySettings();
  int zeroTrustCallbackTicket = 0;
  ThemePreference themePreference = ThemePreference.system;
  LocalePreference localePreference = LocalePreference.system;
  AppSection section = AppSection.home;
  EngineSnapshot _snapshot = const EngineSnapshot();
  EngineSnapshot get snapshot => _snapshot;
  set snapshot(EngineSnapshot value) {
    _snapshot = value;
    quality.updateConnection(value);
  }

  NetworkQualitySnapshot? get networkQuality => quality.latest;
  set networkQuality(NetworkQualitySnapshot? value) {
    if (value != null) quality.accept(value);
  }

  EngineCapabilities? _engineCapabilities;
  EngineCapabilities? get engineCapabilities => _engineCapabilities;
  set engineCapabilities(EngineCapabilities? value) {
    _engineCapabilities = value;
    quality.setEnabled(value?.networkQuality ?? false);
  }

  List<AppSection> get availableSections => const <AppSection>[
    AppSection.home,
    AppSection.profiles,
    AppSection.proxy,
    AppSection.settings,
  ];
  String? lastError;
  String? lastNotice;
  bool snapshotStreamDegraded = false;
  bool _userDisconnectedThisSession = false;
  UpdateCheckResult? updateResult;
  UpdateOperationPhase updatePhase = UpdateOperationPhase.idle;
  int updateDownloadedBytes = 0;
  int updateTotalBytes = 0;
  String? updateError;
  String? downloadedUpdatePath;
  GeoRulesList geoRules = const GeoRulesList();
  GeoRulesProgress? geoProgress;
  bool _geoOperationActive = false;
  List<UsqueProfile> profiles = <UsqueProfile>[UsqueProfile.defaultProfile()];
  String activeProfileId = UsqueProfile.defaultProfileId;
  Map<String, ProfileIdentityState> profileIdentityStates =
      <String, ProfileIdentityState>{};
  Map<String, ProfileIdentityStatus> profileIdentityStatuses =
      <String, ProfileIdentityStatus>{};

  AppStrings get strings => AppStrings(localePreference);

  double? get updateProgress => updateTotalBytes <= 0
      ? null
      : (updateDownloadedBytes / updateTotalBytes).clamp(0.0, 1.0).toDouble();

  bool get updateOperationActive => switch (updatePhase) {
    UpdateOperationPhase.checking ||
    UpdateOperationPhase.downloading ||
    UpdateOperationPhase.verifying ||
    UpdateOperationPhase.installing => true,
    _ => false,
  };

  UsqueProfile sharedNetwork = UsqueProfile.defaultProfile();

  UsqueProfile get activeProfile {
    final account = profiles.firstWhere(
      (profile) => profile.id == activeProfileId,
      orElse: UsqueProfile.defaultProfile,
    );
    return _hydrateAccount(account);
  }

  UsqueProfile _hydrateAccount(UsqueProfile account) {
    final zeroTrust =
        identityStatus(account.id).provider == IdentityProvider.zeroTrust;
    return sharedNetwork.copyWith(
      id: account.id,
      name: account.name,
      endpointIpv4: zeroTrust
          ? account.endpointIpv4
          : sharedNetwork.endpointIpv4,
      endpointIpv6: zeroTrust
          ? account.endpointIpv6
          : sharedNetwork.endpointIpv6,
    );
  }

  void _captureSharedNetwork() {
    if (profiles.isEmpty) {
      sharedNetwork = UsqueProfile.defaultProfile();
      return;
    }
    final source = profiles.firstWhere(
      (profile) =>
          identityStatus(profile.id).provider != IdentityProvider.zeroTrust,
      orElse: () => profiles.first,
    );
    final zeroTrust =
        identityStatus(source.id).provider == IdentityProvider.zeroTrust;
    sharedNetwork = zeroTrust
        ? source.copyWith(
            endpointIpv4: UsqueProfile.defaultEndpointIpv4,
            endpointIpv6: UsqueProfile.defaultEndpointIpv6,
          )
        : source;
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
      unawaited(_subscribeToSnapshotEvents());
    }
    initialized = true;
    unawaited(_refreshCapabilities());
    _notifyListeners();
    unawaited(diagnostics.restore(silent: true));
    unawaited(refreshSnapshot(silent: true));
    unawaited(_updateDownloader.cleanupStale());
    if (updateChecksEnabled && !_startupUpdateCheckStarted) {
      _startupUpdateCheckStarted = true;
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
      _captureSharedNetwork();
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
    if (!availableSections.contains(value)) return;
    section = value;
    _notifyListeners();
  }

  Future<void> _refreshCapabilities() async {
    try {
      final value = await _engine.getCapabilities();
      if (!_disposed && value != null) {
        engineCapabilities = value;
        _notifyListeners();
      }
    } on Object {
      // Older engines have no optional quality surface.
    }
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
      await _refreshProfileCatalog();
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
    _captureSharedNetwork();
  }

  Future<void> checkForUpdates() async {
    if (updateOperationActive) return;
    await _checkForUpdates(manual: true, silent: false);
  }

  Future<void> downloadUpdate() async {
    final result = updateResult;
    final package = result?.package;
    final version = result?.version;
    if (result == null ||
        !result.available ||
        package == null ||
        version == null ||
        updateOperationActive) {
      return;
    }
    final generation = ++_updateOperationGeneration;
    final cancellation = UpdateDownloadCancellation();
    _updateCancellation = cancellation;
    updatePhase = UpdateOperationPhase.downloading;
    updateDownloadedBytes = 0;
    updateTotalBytes = package.size;
    updateError = null;
    lastError = null;
    _notifyListeners();
    String? path;
    try {
      path = await _updateDownloader.download(
        package,
        cancellation: cancellation,
        onProgress: (downloaded, total) {
          if (_disposed || generation != _updateOperationGeneration) return;
          updateDownloadedBytes = downloaded;
          updateTotalBytes = total;
          _notifyListeners();
        },
      );
      if (_disposed || generation != _updateOperationGeneration) {
        await _updateDownloader.discard(path);
        return;
      }
      updatePhase = UpdateOperationPhase.verifying;
      _notifyListeners();
      await _engine.verifyUpdatePackage(
        path: path,
        version: version,
        package: package,
      );
      if (_disposed || generation != _updateOperationGeneration) {
        await _updateDownloader.discard(path);
        return;
      }
      path = await _updateDownloader.publish(path, package);
      downloadedUpdatePath = path;
      updatePhase = UpdateOperationPhase.ready;
      updateDownloadedBytes = package.size;
      updateTotalBytes = package.size;
      _notifyListeners();
    } on UpdateDownloadCancelled {
      if (!_disposed && generation == _updateOperationGeneration) {
        updatePhase = UpdateOperationPhase.available;
        updateDownloadedBytes = 0;
        updateTotalBytes = package.size;
        _notifyListeners();
      }
    } on Object catch (error) {
      await _updateDownloader.discard(path);
      if (!_disposed && generation == _updateOperationGeneration) {
        updateError = error is EngineException
            ? error.message
            : error.toString();
        updatePhase = UpdateOperationPhase.failed;
        _notifyListeners();
      }
    } finally {
      if (identical(_updateCancellation, cancellation)) {
        _updateCancellation = null;
      }
    }
  }

  void cancelUpdateDownload() {
    if (updatePhase != UpdateOperationPhase.downloading) return;
    _updateCancellation?.cancel();
  }

  Future<void> installDownloadedUpdate() async {
    final result = updateResult;
    final package = result?.package;
    final version = result?.version;
    final path = downloadedUpdatePath;
    if (result == null ||
        !result.available ||
        package == null ||
        version == null ||
        path == null ||
        updatePhase != UpdateOperationPhase.ready) {
      return;
    }
    updatePhase = UpdateOperationPhase.installing;
    updateError = null;
    _notifyListeners();
    final success = await _run(() async {
      await flushProfileWrites();
      if (snapshot.phase != ConnectionPhase.disconnected) {
        final disconnected = await _engine.disconnect();
        if (disconnected.phase != ConnectionPhase.disconnected) {
          throw const EngineException(
            'UPDATE_DISCONNECT_FAILED',
            'Usque could not disconnect safely before installing the update.',
          );
        }
        snapshot = disconnected;
        _notifyListeners();
      }
      await _engine.installUpdatePackage(
        path: path,
        version: version,
        package: package,
      );
    }, affectsConnection: false);
    if (!success && !_disposed) {
      updateError = lastError;
      await _updateDownloader.discard(path);
      downloadedUpdatePath = null;
      updateDownloadedBytes = 0;
      updatePhase = UpdateOperationPhase.available;
      _notifyListeners();
    }
  }

  void noteUpdateInstallFinished({required bool success, String? message}) {
    if (success) return;
    downloadedUpdatePath = null;
    updateDownloadedBytes = 0;
    updatePhase = updateResult?.available == true
        ? UpdateOperationPhase.available
        : UpdateOperationPhase.idle;
    updateError = message;
    _notifyListeners();
  }

  Future<void> refreshGeoRules() async {
    try {
      geoRules = await _engine.listGeoRules();
      _notifyListeners();
    } on EngineException catch (error) {
      lastError = error.message;
      _notifyListeners();
    }
  }

  Future<void> downloadGeoRules(String countryCode) async {
    await _run(() async {
      lastNotice = null;
      _geoOperationActive = true;
      geoProgress = GeoRulesProgress(currentFile: countryCode, total: 1);
      _notifyListeners();
      try {
        final results = await _engine.downloadGeoRules(countryCode);
        _recordGeoUpdateResults(results);
        geoRules = await _engine.listGeoRules();
      } finally {
        _geoOperationActive = false;
        geoProgress = null;
      }
    }, affectsConnection: false);
  }

  Future<void> updateAllGeoRules() async {
    await _run(() async {
      lastNotice = null;
      _geoOperationActive = true;
      geoProgress = const GeoRulesProgress(total: 1);
      _notifyListeners();
      try {
        final results = await _engine.updateAllGeoRules();
        _recordGeoUpdateResults(results);
        geoRules = await _engine.listGeoRules();
      } finally {
        _geoOperationActive = false;
        geoProgress = null;
      }
    }, affectsConnection: false);
  }

  void _recordGeoUpdateResults(List<GeoRulesUpdateResult> results) {
    final updated = results
        .where((result) => result.status == GeoRulesUpdateStatus.updated)
        .length;
    final current = results
        .where((result) => result.status == GeoRulesUpdateStatus.upToDate)
        .length;
    final failures = results
        .where((result) => result.status == GeoRulesUpdateStatus.failed)
        .map((result) {
          final scope = result.artifactScope == 'global'
              ? 'global'
              : result.countryCode;
          final artifact = result.artifactKind.isEmpty
              ? scope
              : '$scope ${result.artifactKind}';
          return result.reason.isEmpty
              ? artifact
              : '$artifact: ${result.reason}';
        })
        .join('; ');
    if (updated > 0 || current > 0) {
      lastNotice = strings
          .get('geo_update_complete')
          .replaceAll('{updated}', '$updated')
          .replaceAll('{current}', '$current');
    }
    if (failures.isNotEmpty) {
      lastError = strings
          .get('geo_update_failed')
          .replaceAll('{current}', failures);
    }
  }

  Future<bool> clearAllData() async {
    await flushProfileWrites();
    String? cleanupWarning;
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
      _updateCancellation?.cancel();
      _updateOperationGeneration += 1;
      try {
        await _updateDownloader.discard(downloadedUpdatePath);
      } on Object catch (error) {
        cleanupWarning = error is EngineException
            ? error.message
            : error.toString();
      }
      updateResult = null;
      updatePhase = UpdateOperationPhase.idle;
      updateDownloadedBytes = 0;
      updateTotalBytes = 0;
      updateError = null;
      downloadedUpdatePath = null;
      perAppProxy = const PerAppProxySettings();
    }, affectsConnection: false);
    if (success) {
      lastNotice = strings.get('clear_all_data_complete');
      lastError = cleanupWarning;
      _notifyListeners();
    }
    return success;
  }

  Future<void> _checkForUpdates({
    required bool manual,
    required bool silent,
  }) async {
    if (updateOperationActive) return;
    updatePhase = UpdateOperationPhase.checking;
    updateError = null;
    _notifyListeners();
    if (silent) {
      try {
        final result = await _engine.checkForUpdates(manual: manual);
        if (_disposed) {
          return;
        }
        await _applyUpdateResult(result);
        if (result.available) {
          lastNotice =
              '${strings.get('update_available')} ${result.version ?? ''}'
                  .trim();
        }
        _notifyListeners();
      } on Object {
        // Automatic checks are optional and must not affect tunnel state.
        if (!_disposed && updatePhase == UpdateOperationPhase.checking) {
          updatePhase = updateResult?.available == true
              ? UpdateOperationPhase.available
              : UpdateOperationPhase.idle;
          _notifyListeners();
        }
      }
      return;
    }

    UpdateCheckResult? checked;
    final success = await _run(() async {
      checked = await _engine.checkForUpdates(manual: manual);
    }, affectsConnection: false);
    if (success && checked != null) {
      await _applyUpdateResult(checked!);
      lastNotice = checked!.available
          ? '${strings.get('update_available')} ${checked!.version ?? ''}'
                .trim()
          : strings.get('already_latest');
      _notifyListeners();
    } else if (!_disposed && updatePhase == UpdateOperationPhase.checking) {
      updatePhase = updateResult?.available == true
          ? UpdateOperationPhase.available
          : UpdateOperationPhase.idle;
      _notifyListeners();
    }
  }

  Future<void> _applyUpdateResult(UpdateCheckResult result) async {
    final previousName = updateResult?.package?.name;
    final nextName = result.package?.name;
    final packageChanged = previousName != nextName;
    if (packageChanged && downloadedUpdatePath != null) {
      _updateOperationGeneration += 1;
      _updateCancellation?.cancel();
      await _updateDownloader.discard(downloadedUpdatePath);
      downloadedUpdatePath = null;
      updateDownloadedBytes = 0;
      updateTotalBytes = 0;
    }
    updateResult = result;
    if (!result.available) {
      updatePhase = UpdateOperationPhase.idle;
    } else if (downloadedUpdatePath != null && !packageChanged) {
      updatePhase = UpdateOperationPhase.ready;
    } else {
      updatePhase = UpdateOperationPhase.available;
      updateTotalBytes = result.package?.size ?? 0;
    }
    updateError = null;
  }

  Future<bool> _run(
    Future<void> Function() operation, {
    bool affectsConnection = true,
  }) async {
    _activeOperations += 1;
    busy = true;
    lastError = null;
    _notifyListeners();
    try {
      await operation();
      return true;
    } catch (error) {
      lastError = error is EngineException ? error.message : error.toString();
      if (affectsConnection && snapshot.phase != ConnectionPhase.disconnected) {
        snapshot = EngineSnapshot(
          phase: ConnectionPhase.error,
          warning: lastError,
        );
      }
      return false;
    } finally {
      _activeOperations -= 1;
      busy = _activeOperations > 0;
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
    final added = sharedNetwork.copyWith(id: id, name: normalized);
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
    final profile = sharedNetwork.copyWith(id: _newUuidV4(), name: normalized);
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
      _captureSharedNetwork();
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
      var refreshedCatalog = false;
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
        await _refreshProfileCatalog();
        refreshedCatalog = true;
      } finally {
        final safeToReconnect = !mutationCommitted || refreshedCatalog;
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
    updateNetwork(updated);
  }

  void renameProfile(String id, String name) {
    if (!profiles.any((profile) => profile.id == id)) {
      return;
    }
    profiles = profiles
        .map(
          (profile) =>
              profile.id == id ? profile.copyWith(name: name) : profile,
        )
        .toList(growable: false);
    _notifyListeners();
    final outgoing = _hydrateAccount(
      profiles.firstWhere((profile) => profile.id == id),
    );
    _queueProfileMutation(() => _engine.upsertProfile(outgoing));
  }

  void updateNetwork(UsqueProfile updated) {
    unawaited(saveNetwork(updated));
  }

  Future<bool> saveNetwork(UsqueProfile updated) {
    if (!profiles.any((profile) => profile.id == updated.id) &&
        updated.id != activeProfileId) {
      return Future<bool>.value(false);
    }
    final normalized = updated.frontends.http
        ? updated
        : updated.copyWith(proxy: updated.proxy.copyWith(systemProxy: false));
    final zeroTrust =
        identityStatus(updated.id).provider == IdentityProvider.zeroTrust;
    sharedNetwork = sharedNetwork.copyWith(
      frontends: normalized.frontends,
      transport: normalized.transport,
      ipPolicy: normalized.ipPolicy,
      endpointIpv4: zeroTrust
          ? sharedNetwork.endpointIpv4
          : normalized.endpointIpv4,
      endpointIpv6: zeroTrust
          ? sharedNetwork.endpointIpv6
          : normalized.endpointIpv6,
      endpointPort: normalized.endpointPort,
      sni: normalized.sni,
      mtu: normalized.mtu,
      dnsIpv4: normalized.dnsIpv4,
      dnsIpv6: normalized.dnsIpv6,
      dnsMode: normalized.dnsMode,
      killSwitch: normalized.killSwitch,
      allowLan: normalized.allowLan,
      autoConnect: normalized.autoConnect,
      bypassCidrs: normalized.bypassCidrs,
      geoDirectCountries: normalized.geoDirectCountries,
      directDns: normalized.directDns,
      proxy: normalized.proxy,
    );
    _notifyListeners();
    final outgoing = activeProfile;
    return _queueProfileMutation(() {
      if (outgoing.id == activeProfileId && snapshot.isConnected) {
        return _engine.reconfigureActiveProfile(outgoing);
      }
      return _engine.upsertProfile(outgoing);
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

  Future<bool> _queueProfileMutation(Future<void> Function() mutation) {
    final outcome = Completer<bool>();
    _profileWriteTail = _profileWriteTail.then((_) async {
      var succeeded = false;
      try {
        await mutation();
        succeeded = true;
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
          _captureSharedNetwork();
        } on Object {
          // Keep the optimistic in-memory state when the authoritative store
          // cannot be reloaded; the original mutation error remains visible.
        }
        _notifyListeners();
      }
      outcome.complete(succeeded);
    });
    return outcome.future;
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

  Future<void> _subscribeToSnapshotEvents() async {
    if (_disposed || !_engine.supportsSnapshotEvents) {
      return;
    }
    _snapshotReconnectTimer?.cancel();
    _snapshotReconnectTimer = null;
    final previous = _snapshotSubscription;
    _snapshotSubscription = null;
    final generation = ++_snapshotSubscriptionGeneration;
    if (previous != null) {
      await previous.cancel();
    }
    if (_disposed || generation != _snapshotSubscriptionGeneration) {
      return;
    }
    _snapshotSubscription = _engine.snapshotEvents.listen(
      (EngineSnapshotEvent event) => _handleSnapshotEvent(event, generation),
      onError: (Object error, StackTrace stackTrace) =>
          _handleSnapshotEventError(error, stackTrace, generation),
      onDone: () => _handleSnapshotEventDone(generation),
      cancelOnError: false,
    );
  }

  void _handleSnapshotEvent(EngineSnapshotEvent event, int generation) {
    if (_disposed || generation != _snapshotSubscriptionGeneration) {
      return;
    }
    _snapshotStreamEstablished = true;
    final wasDegraded = snapshotStreamDegraded;
    _snapshotReconnectAttempt = 0;
    _snapshotReconnectTimer?.cancel();
    _snapshotReconnectTimer = null;
    snapshotStreamDegraded = false;
    quality.markStreamUnavailable(false);
    _stopPolling();
    diagnostics.handleEngineEvent(event);
    if (wasDegraded) {
      unawaited(diagnostics.restore(silent: true));
    }
    final handledGeoProgress = event.geoProgress != null && _geoOperationActive;
    if (handledGeoProgress) {
      final progress = event.geoProgress!;
      geoProgress = progress.total > 0 && progress.completed >= progress.total
          ? null
          : progress;
      _notifyListeners();
    }
    final nextQuality = event.networkQuality ?? event.snapshot?.networkQuality;
    final handledNetworkQuality =
        nextQuality != null && nextQuality != networkQuality;
    if (nextQuality != null) {
      networkQuality = nextQuality;
    }
    final handledCapabilities =
        event.capabilities != null && event.capabilities != engineCapabilities;
    if (handledCapabilities) {
      engineCapabilities = event.capabilities;
    }
    final next = event.snapshot;
    if (next == null) {
      if ((wasDegraded || handledNetworkQuality || handledCapabilities) &&
          !handledGeoProgress) {
        _notifyListeners();
      }
      return;
    }
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
    if (!snapshotChanged &&
        !errorChanged &&
        !wasDegraded &&
        !handledNetworkQuality &&
        !handledCapabilities) {
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
    final established = _snapshotStreamEstablished;
    if (established) {
      snapshotStreamDegraded = true;
      quality.markStreamUnavailable(true);
      diagnostics.markEventStreamUnavailable();
    }
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
        unawaited(_subscribeToSnapshotEvents());
      });
    }
    if (established) {
      _notifyListeners();
    }
  }

  @override
  void dispose() {
    _disposed = true;
    _updateOperationGeneration += 1;
    _updateCancellation?.cancel();
    _updateCancellation = null;
    _stopPolling();
    _snapshotReconnectTimer?.cancel();
    _snapshotReconnectTimer = null;
    _snapshotSubscriptionGeneration += 1;
    unawaited(_snapshotSubscription?.cancel());
    _snapshotSubscription = null;
    diagnostics.dispose();
    quality.dispose();
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
