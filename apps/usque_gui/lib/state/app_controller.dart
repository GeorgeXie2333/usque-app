import 'dart:async';
import 'dart:convert';
import 'dart:math';

import 'package:flutter/foundation.dart';
import 'package:shared_preferences/shared_preferences.dart';

import '../core/app_strings.dart';
import '../core/user_facing_errors.dart';
import '../models/app_models.dart';
import '../services/engine_client.dart';
import '../services/update_downloader.dart';
import 'diagnostics_controller.dart';
import 'network_quality_controller.dart';
import 'network_settings_controller.dart';

class AppController extends ChangeNotifier {
  Future<ChainProfileResult> chainProfile(Map<String, Object?> request) {
    final engine = _engine;
    if (engine is ChainProfileClient) {
      return (engine as ChainProfileClient).chainProfile(request);
    }
    throw const EngineException(
      'CHAIN_PROFILE_UNAVAILABLE',
      'Chain profiles are unavailable.',
    );
  }

  Future<String?> pickChainConfiguration() {
    final engine = _engine;
    if (engine is ChainProfileClient) {
      return (engine as ChainProfileClient).pickChainConfiguration();
    }
    throw const EngineException(
      'CHAIN_PROFILE_UNAVAILABLE',
      'File import is unavailable.',
    );
  }

  AppController(
    EngineClient engine, {
    UpdateDownloader? updateDownloader,
    NetworkQualityController? qualityController,
  }) : _engine = engine,
       _updateDownloader = updateDownloader ?? UpdateDownloader(engine),
       diagnostics = DiagnosticsController(engine),
       quality = qualityController ?? NetworkQualityController(engine),
       networkSettings = NetworkSettingsController(engine) {
    diagnostics.resolveStrings = () => strings;
    networkSettings.addListener(_acceptNetworkSettings);
  }

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
  String newVpnGateOperationId() => _newUuidV4();
  int get connectionIntent => _connectionIntent;
  Future<VpnGateDirectory> listVpnGate({
    String? countryCode,
    bool unknownCountry = false,
    int offset = 0,
    int limit = 50,
    bool favoritesOnly = false,
    bool statusOnly = false,
  }) {
    final engine = _engine;
    if (engine is VpnGateClient) {
      return (engine as VpnGateClient).listVpnGate(
        countryCode: countryCode,
        unknownCountry: unknownCountry,
        offset: offset,
        limit: limit,
        favoritesOnly: favoritesOnly,
        statusOnly: statusOnly,
      );
    }
    return Future.error(
      const EngineException(
        'VPN_GATE_UNAVAILABLE',
        'The catalogue service is unavailable.',
      ),
    );
  }

  Future<void> refreshVpnGate({bool cancel = false}) {
    final engine = _engine;
    if (engine is VpnGateClient) {
      return (engine as VpnGateClient).refreshVpnGate(cancel: cancel);
    }
    return Future.error(
      const EngineException(
        'VPN_GATE_UNAVAILABLE',
        'The catalogue service is unavailable.',
      ),
    );
  }

  Future<void> vpnGateNode(VpnGateNodeRequest request) {
    final engine = _engine;
    if (engine is VpnGateClient &&
        (engineCapabilities?.vpnGatePoolFavorites ?? false)) {
      return (engine as VpnGateClient).vpnGateNode(request);
    }
    return Future.error(
      const EngineException(
        'VPN_GATE_UNAVAILABLE',
        'The node preparation service is unavailable.',
      ),
    );
  }

  final UpdateDownloader _updateDownloader;
  final DiagnosticsController diagnostics;
  final NetworkQualityController quality;
  final NetworkSettingsController networkSettings;
  String? get networkSettingsMessage {
    if (networkSettings.unconfirmed) return strings.get('settings_unknown');
    if (networkSettings.saveError != null) {
      return strings.get(
        networkSettings.saveError == 'NETWORK_SETTINGS_UNSUPPORTED'
            ? 'settings_unsupported'
            : 'settings_save_failed',
      );
    }
    final state = networkSettings.state;
    if (state == null || state.operationId == null) return null;
    return strings.get(switch (state.status) {
      NetworkSettingsApplyStatus.notRequired =>
        state.deferredFields.isEmpty ? 'settings_saved' : 'settings_deferred',
      NetworkSettingsApplyStatus.applying => 'settings_applying',
      NetworkSettingsApplyStatus.applied => 'settings_applied',
      NetworkSettingsApplyStatus.deferred => 'settings_deferred',
      NetworkSettingsApplyStatus.failed => 'settings_failed',
      NetworkSettingsApplyStatus.unknown => 'settings_unknown',
    });
  }

  bool get networkSettingsCanReconnect =>
      networkSettings.state?.persisted == true &&
      networkSettings.state?.status == NetworkSettingsApplyStatus.failed;
  SharedPreferences? _preferences;
  Timer? _snapshotTimer;
  Future<void>? _snapshotRefresh;
  int _snapshotRevision = 0;
  Timer? _snapshotReconnectTimer;
  StreamSubscription<EngineSnapshotEvent>? _snapshotSubscription;
  int _snapshotReconnectAttempt = 0;
  int _snapshotSubscriptionGeneration = 0;
  bool _snapshotStreamEstablished = false;
  bool _startupUpdateCheckStarted = false;
  bool _disposed = false;
  int _connectionIntent = 0;
  int _updateOperationGeneration = 0;
  UpdateDownloadCancellation? _updateCancellation;

  bool initialized = false;
  bool onboardingComplete = false;
  bool busy = false;
  int _activeOperations = 0;
  bool updateChecksEnabled = true;
  bool startOnBoot = false;
  bool closeToTray = true;
  PerAppProxySettings perAppProxy = const PerAppProxySettings();
  int zeroTrustCallbackTicket = 0;
  ThemePreference themePreference = ThemePreference.system;
  LocalePreference localePreference = LocalePreference.system;
  AppSection section = AppSection.home;
  EngineSnapshot _snapshot = const EngineSnapshot();
  EngineSnapshot get snapshot => _snapshot;
  set snapshot(EngineSnapshot value) {
    _snapshotRevision++;
    _snapshot = value;
    if (value.phase == ConnectionPhase.error &&
        (value.errorCode != null || value.warning?.isNotEmpty == true)) {
      lastError = userFacingFailure(
        strings,
        code: value.errorCode,
        details: value.warning,
      );
    }
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
    final newlySupported =
        !networkSettings.supported &&
        (value?.networkSettingsApplication ?? false);
    networkSettings.supported = value?.networkSettingsApplication ?? false;
    if (newlySupported) unawaited(networkSettings.refresh());
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
  NetworkSettingsState? _acceptedSettings;
  bool _profilesLoaded = false;
  String? _profileLoadError;
  bool _initialStatusLoaded = false;
  bool _startupAutoConnectChecked = false;
  Timer? _bootstrapRetryTimer;
  Future<void>? _bootstrapWork;
  int _bootstrapGeneration = 0;
  int _dataGeneration = 0;
  bool _clearing = false;
  final Map<String, int> _identityReconnectIntents = {};
  final Set<String> _managedAccountIds = {};
  final List<void Function()> _pendingAccountViews = [];

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

  void _acceptNetworkSettings() {
    final state = networkSettings.state;
    if (identical(state, _acceptedSettings)) {
      _notifyListeners();
      return;
    }
    _acceptedSettings = state;
    final stored = state?.sharedNetwork ?? state?.storedProfile;
    if (stored != null) {
      final managed =
          state?.sharedNetwork == null &&
          (_managedAccountIds.contains(stored.id) ||
              identityStatus(stored.id).provider == IdentityProvider.zeroTrust);
      sharedNetwork = managed
          ? stored.copyWith(
              endpointIpv4: sharedNetwork.endpointIpv4,
              endpointIpv6: sharedNetwork.endpointIpv6,
            )
          : stored;
    }
    _notifyListeners();
  }

  Future<void> initialize() async {
    final dataGeneration = _dataGeneration;
    _bootstrapGeneration++;
    _preferences = await SharedPreferences.getInstance();
    if (_disposed || dataGeneration != _dataGeneration) return;
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
    if (_disposed || dataGeneration != _dataGeneration) {
      return;
    }
    try {
      final launchTarget = await _engine.consumeLaunchTarget();
      if (_disposed || dataGeneration != _dataGeneration) return;
      if (launchTarget == 'profiles') {
        section = AppSection.profiles;
      }
    } on Object {
      // A launcher shortcut is optional and must not block initialization.
    }
    try {
      final platformPreferences = await _engine.platformPreferences();
      if (_disposed || dataGeneration != _dataGeneration) return;
      startOnBoot = platformPreferences.startOnBoot;
      closeToTray = platformPreferences.closeToTray;
    } on Object {
      // Native shell preferences are optional in unsupported test hosts.
    }
    try {
      final perApp = await _engine.perAppProxy();
      if (_disposed || dataGeneration != _dataGeneration) return;
      perAppProxy = perApp;
    } on Object {
      perAppProxy = const PerAppProxySettings();
    }
    if (_engine.supportsSnapshotEvents) {
      unawaited(_subscribeToSnapshotEvents());
    }
    if (_disposed || dataGeneration != _dataGeneration) return;
    initialized = true;
    _notifyListeners();
    unawaited(diagnostics.restore(silent: true));
    unawaited(_updateDownloader.cleanupStale());
    if (updateChecksEnabled && !_startupUpdateCheckStarted) {
      _startupUpdateCheckStarted = true;
      unawaited(_checkForUpdates(manual: false, silent: true));
    }
    // Rendering can proceed after the deadline; a late bootstrap still checks
    // the user's current intent before it may auto-connect.
    await _finishBootstrap().timeout(
      const Duration(seconds: 12),
      onTimeout: () {},
    );
  }

  Future<void> _finishBootstrap() {
    final pending = _bootstrapWork;
    if (pending != null) return pending;
    late final Future<void> work;
    work = _bootstrapOnce().whenComplete(() {
      if (identical(_bootstrapWork, work)) _bootstrapWork = null;
    });
    _bootstrapWork = work;
    return work;
  }

  Future<void> _bootstrapOnce() async {
    final generation = _bootstrapGeneration;
    final intent = _connectionIntent;
    try {
      if (!_profilesLoaded) await _loadProfiles();
      await Future.wait<void>([
        _refreshCapabilities(),
        refreshSnapshot(silent: true),
      ]);
      if (_disposed || generation != _bootstrapGeneration) return;
      final needsCapabilities =
          activeProfile.dataPlane == DataPlaneMode.l4Proxy ||
          activeProfile.vpnGate.enabled;
      if (!_profilesLoaded ||
          !_initialStatusLoaded ||
          (needsCapabilities && engineCapabilities == null)) {
        _scheduleBootstrapRetry();
        return;
      }
      _bootstrapRetryTimer?.cancel();
      _bootstrapRetryTimer = null;
      if (!_startupAutoConnectChecked) {
        _startupAutoConnectChecked = true;
        if (intent == _connectionIntent && _shouldAutoConnectOnStart()) {
          await connectOrDisconnect();
        }
      }
    } on Object {
      if (!_disposed && generation == _bootstrapGeneration) {
        _scheduleBootstrapRetry();
      }
    }
  }

  void _scheduleBootstrapRetry() {
    _bootstrapRetryTimer?.cancel();
    _bootstrapRetryTimer = Timer(const Duration(seconds: 2), () {
      _bootstrapRetryTimer = null;
      if (!_disposed) unawaited(_finishBootstrap());
    });
  }

  Future<void> _ensureConnectionInputs() async {
    if (!_profilesLoaded) await _loadProfiles();
    if (!_profilesLoaded) {
      throw const EngineException(
        'ENGINE_UNAVAILABLE',
        'The saved accounts are not available.',
      );
    }
    if (engineCapabilities == null) await _refreshCapabilities();
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
    final generation = _bootstrapGeneration;
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
        lastError = strings.get('accounts_reset');
      }
    }

    profiles = legacyProfiles;
    activeProfileId = legacyActiveProfileId;
    try {
      final catalog = await _engine.importLegacyProfiles(
        legacyProfiles,
        legacyActiveProfileId,
      );
      if (_disposed || generation != _bootstrapGeneration) return;
      _profilesLoaded = true;
      if (lastError == _profileLoadError) lastError = null;
      _profileLoadError = null;
      profiles = catalog.profiles;
      activeProfileId = catalog.activeProfileId;
      profileIdentityStates = catalog.identityStates;
      profileIdentityStatuses = catalog.identityStatuses;
      _captureSharedNetwork();
      sharedNetwork = catalog.sharedNetwork ?? sharedNetwork;
      _rememberManagedAccounts();
      await preferences?.remove(_profilesKey);
    } on Object catch (error) {
      if (!_disposed &&
          generation == _bootstrapGeneration &&
          lastError == null) {
        lastError = _profileLoadError = userFacingError(strings, error);
      }
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
    final generation = _dataGeneration;
    try {
      final value = await _engine.getCapabilities();
      if (!_disposed &&
          generation == _dataGeneration &&
          !_clearing &&
          value != null) {
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
    String? teamName,
    String? callbackUri,
  }) async {
    final generation = _dataGeneration;
    return _run(() async {
      await _engine.provisionIdentity(
        activeProfile,
        method: method,
        licenseKey: licenseKey,
        teamName: teamName,
        callbackUri: callbackUri,
      );
      await _refreshProfileCatalog();
      if (_disposed || generation != _dataGeneration || _clearing) return;
      onboardingComplete = true;
      await _preferences?.setBool('onboarding_complete', true);
    });
  }

  void _requireDataPlaneCapability(UsqueProfile profile) {
    if (profile.vpnGate.enabled && !(engineCapabilities?.vpnGateTcp ?? false)) {
      throw EngineException('VPN_GATE_UNSUPPORTED', strings.vpnGateUnsupported);
    }
    if (profile.dataPlane == DataPlaneMode.l4Proxy &&
        !(engineCapabilities?.l4Available ?? false)) {
      throw EngineException('L4_UNSUPPORTED', strings.get('l4_unsupported'));
    }
  }

  Future<void> connectOrDisconnect() async {
    if (snapshot.phase == ConnectionPhase.disconnecting) return;
    final intent = ++_connectionIntent;
    if (snapshot.isConnected || snapshot.isTransitional) {
      snapshot = EngineSnapshot(
        phase: ConnectionPhase.disconnecting,
        vpnGate: snapshot.vpnGate,
        chainExit: snapshot.chainExit,
        killSwitchState: snapshot.killSwitchState,
        platformLockdown: snapshot.platformLockdown,
        alwaysOn: snapshot.alwaysOn,
      );
      await _run(() async {
        _userDisconnectedThisSession = true;
        final next = await _engine.disconnect();
        if (intent != _connectionIntent) return;
        snapshot = next;
        if (snapshot.phase == ConnectionPhase.disconnected &&
            !snapshotStreamDegraded) {
          _stopPolling();
        } else if (!_engine.supportsSnapshotEvents || snapshotStreamDegraded) {
          _startPolling(force: snapshotStreamDegraded);
        }
      }, connectionIntent: intent);
      return;
    }

    snapshot = const EngineSnapshot(phase: ConnectionPhase.preparing);
    _notifyListeners();
    final success = await _run(() async {
      await _ensureConnectionInputs();
      if (intent != _connectionIntent) return;
      if (identityState(activeProfile.id) != ProfileIdentityState.ready) {
        throw const EngineException(
          'IDENTITY_SETUP_REQUIRED',
          'This profile needs a valid Consumer WARP identity before it can connect.',
        );
      }
      await flushProfileWrites();
      if (intent != _connectionIntent) return;
      _requireDataPlaneCapability(activeProfile);
      final next = await _engine.connect(activeProfile);
      if (intent == _connectionIntent) snapshot = next;
    }, connectionIntent: intent);
    if (success && (snapshot.isConnected || snapshot.isTransitional)) {
      if (!_engine.supportsSnapshotEvents || snapshotStreamDegraded) {
        _startPolling(force: snapshotStreamDegraded);
      }
    }
  }

  Future<void> retry() async {
    final intent = ++_connectionIntent;
    final success = await _run(() async {
      await _ensureConnectionInputs();
      await flushProfileWrites();
      if (intent != _connectionIntent) return;
      _requireDataPlaneCapability(activeProfile);
      final next = await _engine.retry();
      if (intent == _connectionIntent) snapshot = next;
    }, connectionIntent: intent);
    if (success && (snapshot.isConnected || snapshot.isTransitional)) {
      if (!_engine.supportsSnapshotEvents || snapshotStreamDegraded) {
        _startPolling(force: snapshotStreamDegraded);
      }
    }
  }

  Future<void> disconnectForExit() async {
    _connectionIntent++;
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

  Future<void> refreshSnapshot({bool silent = false}) {
    if (_clearing) return Future<void>.value();
    final pending = _snapshotRefresh;
    if (pending != null) return pending;
    late final Future<void> work;
    work = _refreshSnapshotOnce(silent: silent).whenComplete(() {
      if (identical(_snapshotRefresh, work)) _snapshotRefresh = null;
    });
    _snapshotRefresh = work;
    return work;
  }

  Future<void> _refreshSnapshotOnce({required bool silent}) async {
    final revision = _snapshotRevision;
    try {
      final next = await _engine.snapshot();
      if (_disposed || revision != _snapshotRevision) {
        return;
      }
      _initialStatusLoaded = true;
      snapshot = next;
      if (!snapshot.isConnected && !snapshotStreamDegraded) {
        _stopPolling();
      }
      _notifyListeners();
    } on EngineException catch (error) {
      if (!silent && !_disposed && revision == _snapshotRevision) {
        lastError = userFacingError(strings, error);
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
    if (_clearing || _disposed) return false;
    final success = await networkSettings.enqueue(
      () => _run(() async {
        try {
          await _engine.updateProxyAuth(
            activeProfileId,
            username: username,
            password: password,
            confirmed: true,
          );
        } finally {
          // Read back even when persistence succeeded but runtime application failed.
          try {
            await _refreshProfileCatalog();
          } on Object {
            // Keep the original credential result if catalogue readback fails.
          }
          await networkSettings.refresh();
          await refreshSnapshot();
        }
      }, affectsConnection: false),
    );
    if (success) {
      lastNotice = username.isEmpty
          ? strings.get('proxy_auth_cleared')
          : strings.get('proxy_auth_saved');
      _notifyListeners();
    }
    return success;
  }

  Future<bool> updateLicenseKey(String profileId, String licenseKey) => _run(
    () => _mutateIdentity(
      profileId,
      () => _engine.updateLicenseKey(profileId, licenseKey),
    ),
    affectsConnection: false,
    connectionIntent: _connectionIntent,
  );

  Future<bool> unbindLicenseKey(String profileId) => _run(
    () => _mutateIdentity(profileId, () => _engine.unbindLicenseKey(profileId)),
    affectsConnection: false,
    connectionIntent: _connectionIntent,
  );

  Future<void> _mutateIdentity(
    String profileId,
    Future<void> Function() mutation,
  ) async {
    final intent = _connectionIntent;
    final reconnect = profileId == activeProfileId && snapshot.isConnected;
    var committed = false;
    var refreshed = false;
    if (reconnect) _identityReconnectIntents[profileId] = intent;
    try {
      if (reconnect) {
        final next = await _engine.disconnect();
        if (intent == _connectionIntent) snapshot = next;
        _notifyListeners();
      }
      try {
        await mutation();
        committed = true;
        await _refreshProfileCatalog();
        refreshed = true;
      } finally {
        if (reconnect &&
            (!committed || refreshed) &&
            !_disposed &&
            intent == _connectionIntent &&
            profileId == activeProfileId) {
          _requireDataPlaneCapability(activeProfile);
          final next = await _engine.connect(activeProfile);
          if (intent == _connectionIntent && profileId == activeProfileId) {
            snapshot = next;
          }
          _notifyListeners();
        }
      }
    } finally {
      if (_identityReconnectIntents[profileId] == intent) {
        _identityReconnectIntents.remove(profileId);
      }
    }
  }

  void cancelIdentityFlow(String profileId) {
    final intent = _identityReconnectIntents.remove(profileId);
    if (intent == null || intent != _connectionIntent) return;
    final cancelled = ++_connectionIntent;
    _userDisconnectedThisSession = true;
    unawaited(
      _engine
          .disconnect()
          .then((next) {
            if (!_disposed && cancelled == _connectionIntent) {
              snapshot = next;
              _notifyListeners();
            }
          })
          .catchError((Object _) {
            /* Snapshot recovery reports any unconfirmed stop. */
          }),
    );
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
    if (_clearing) return;
    final generation = _dataGeneration;
    final catalog = await _engine.importLegacyProfiles(
      const <UsqueProfile>[],
      '',
    );
    if (_disposed || generation != _dataGeneration || _clearing) return;
    _profilesLoaded = true;
    profiles = catalog.profiles;
    activeProfileId = catalog.activeProfileId;
    profileIdentityStates = catalog.identityStates;
    profileIdentityStatuses = catalog.identityStatuses;
    _rememberManagedAccounts();
    for (final apply in _pendingAccountViews) {
      apply();
    }
  }

  void _rememberManagedAccounts() {
    _managedAccountIds.addAll(
      profileIdentityStatuses.entries
          .where((entry) => entry.value.provider == IdentityProvider.zeroTrust)
          .map((entry) => entry.key),
    );
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
        updateOperationActive ||
        _clearing) {
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
    downloadedUpdatePath = null;
    await _updateDownloader.runExclusive(() async {
      if (_disposed || generation != _updateOperationGeneration) {
        if (identical(_updateCancellation, cancellation)) {
          _updateCancellation = null;
        }
        return;
      }
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
        if (_disposed || generation != _updateOperationGeneration) {
          await _updateDownloader.discard(path);
          return;
        }
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
        try {
          await _updateDownloader.discard(path);
        } on Object {
          // Keep the primary failure; cleanup cannot leave the UI busy.
        }
        if (!_disposed && generation == _updateOperationGeneration) {
          updateError = userFacingError(strings, error);
          updatePhase = UpdateOperationPhase.failed;
          _notifyListeners();
        }
      } finally {
        if (identical(_updateCancellation, cancellation)) {
          _updateCancellation = null;
        }
      }
    });
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
    final generation = _updateOperationGeneration;
    updatePhase = UpdateOperationPhase.installing;
    updateError = null;
    _notifyListeners();
    final success = await _run(() async {
      await flushProfileWrites();
      if (_disposed || generation != _updateOperationGeneration) return;
      if (snapshot.phase != ConnectionPhase.disconnected) {
        final disconnected = await _engine.disconnect();
        if (_disposed || generation != _updateOperationGeneration) return;
        if (disconnected.phase != ConnectionPhase.disconnected) {
          throw const EngineException(
            'UPDATE_DISCONNECT_FAILED',
            'Usque could not disconnect safely before installing the update.',
          );
        }
        snapshot = disconnected;
        _notifyListeners();
      }
      if (_disposed || generation != _updateOperationGeneration) return;
      await _engine.installUpdatePackage(
        path: path,
        version: version,
        package: package,
      );
    }, affectsConnection: false);
    if (!success && !_disposed && generation == _updateOperationGeneration) {
      updateError = lastError;
      try {
        await _updateDownloader.discard(path);
      } on Object {
        // The install failure remains visible even if deleting the file fails.
      }
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
    updateError = strings.get('operation_failed');
    _notifyListeners();
  }

  Future<void> refreshGeoRules() async {
    final generation = _dataGeneration;
    try {
      final next = await _engine.listGeoRules();
      if (_disposed || generation != _dataGeneration) return;
      geoRules = next;
      _notifyListeners();
    } on EngineException catch (error) {
      if (_disposed || generation != _dataGeneration) return;
      lastError = userFacingError(strings, error);
      _notifyListeners();
    }
  }

  Future<void> downloadGeoRules(String countryCode) async {
    final generation = _dataGeneration;
    await _run(() async {
      lastNotice = null;
      _geoOperationActive = true;
      geoProgress = GeoRulesProgress(currentFile: countryCode, total: 1);
      _notifyListeners();
      try {
        final results = await _engine.downloadGeoRules(countryCode);
        if (_disposed || generation != _dataGeneration) return;
        _recordGeoUpdateResults(results);
        final next = await _engine.listGeoRules();
        if (_disposed || generation != _dataGeneration) return;
        geoRules = next;
      } finally {
        if (generation == _dataGeneration) {
          _geoOperationActive = false;
          geoProgress = null;
        }
      }
    }, affectsConnection: false);
  }

  Future<void> updateAllGeoRules() async {
    final generation = _dataGeneration;
    await _run(() async {
      lastNotice = null;
      _geoOperationActive = true;
      geoProgress = const GeoRulesProgress(total: 1);
      _notifyListeners();
      try {
        final results = await _engine.updateAllGeoRules();
        if (_disposed || generation != _dataGeneration) return;
        _recordGeoUpdateResults(results);
        final next = await _engine.listGeoRules();
        if (_disposed || generation != _dataGeneration) return;
        geoRules = next;
      } finally {
        if (generation == _dataGeneration) {
          _geoOperationActive = false;
          geoProgress = null;
        }
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
        .length;
    if (updated > 0 || current > 0) {
      lastNotice = strings
          .get('geo_update_complete')
          .replaceAll('{updated}', '$updated')
          .replaceAll('{current}', '$current');
    }
    if (failures > 0) {
      lastError = strings
          .get('geo_update_failed')
          .replaceAll('{current}', '$failures');
    }
  }

  Future<bool> clearAllData() async {
    if (_clearing || _disposed) return false;
    _clearing = true;
    busy = true;
    _dataGeneration++;
    _connectionIntent++;
    _perAppSaveToken = null;
    _identityReconnectIntents.clear();
    _activeOperations = 0;
    _bootstrapGeneration++;
    _bootstrapWork = null;
    _bootstrapRetryTimer?.cancel();
    _bootstrapRetryTimer = null;
    _snapshotRevision++;
    _snapshotRefresh = null;
    _snapshotSubscriptionGeneration++;
    _snapshotReconnectTimer?.cancel();
    _snapshotReconnectTimer = null;
    _stopPolling();
    final subscription = _snapshotSubscription;
    _snapshotSubscription = null;
    diagnostics.suspendForReset();
    networkSettings.suspendForReset();
    _updateOperationGeneration++;
    _updateCancellation?.cancel();
    String? cleanupWarning;
    var success = false;
    try {
      try {
        await subscription?.cancel();
      } on Object catch (error) {
        cleanupWarning = userFacingError(strings, error);
      }
      await flushProfileWrites();
      success = await _run(
        () async {
          await _engine.clearAllData(confirmed: true);
          final preferences =
              _preferences ?? await SharedPreferences.getInstance();
          if (!await preferences.clear()) {
            throw const EngineException(
              'CLEAR_ALL_FAILED',
              'Local preferences could not be cleared.',
            );
          }
          onboardingComplete = false;
          updateChecksEnabled = true;
          themePreference = ThemePreference.system;
          localePreference = LocalePreference.system;
          section = AppSection.home;
          snapshot = const EngineSnapshot();
          sharedNetwork = UsqueProfile.defaultProfile();
          _acceptedSettings = null;
          _managedAccountIds.clear();
          _pendingAccountViews.clear();
          profiles = <UsqueProfile>[UsqueProfile.defaultProfile()];
          activeProfileId = UsqueProfile.defaultProfileId;
          profileIdentityStates = <String, ProfileIdentityState>{};
          profileIdentityStatuses = <String, ProfileIdentityStatus>{};
          networkSettings.reset();
          diagnostics.reset();
          quality.reset();
          geoRules = const GeoRulesList();
          geoProgress = null;
          _geoOperationActive = false;
          _profilesLoaded = false;
          _profileLoadError = null;
          _initialStatusLoaded = false;
          _startupAutoConnectChecked = false;
          _startupUpdateCheckStarted = false;
          try {
            await _updateDownloader.discard(downloadedUpdatePath);
          } on Object catch (error) {
            cleanupWarning = userFacingError(strings, error);
          }
          updateResult = null;
          updatePhase = UpdateOperationPhase.idle;
          updateDownloadedBytes = 0;
          updateTotalBytes = 0;
          updateError = null;
          downloadedUpdatePath = null;
          _updateCancellation = null;
          perAppProxy = const PerAppProxySettings();
        },
        affectsConnection: false,
        allowDuringClear: true,
      );
    } finally {
      _clearing = false;
      busy = _activeOperations > 0;
      diagnostics.resumeAfterReset();
      networkSettings.resumeAfterReset();
      if (!_disposed) {
        if (_engine.supportsSnapshotEvents) await _subscribeToSnapshotEvents();
        if (!success) {
          try {
            await _refreshProfileCatalog();
          } on Object {
            /* Keep the clear failure. */
          }
          await networkSettings.refresh();
          await diagnostics.restore(silent: true);
          await refreshSnapshot(silent: true);
        }
      }
    }
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
    if (updateOperationActive || _clearing) return;
    final generation = ++_updateOperationGeneration;
    updatePhase = UpdateOperationPhase.checking;
    updateError = null;
    _notifyListeners();
    if (silent) {
      try {
        final result = await _engine.checkForUpdates(manual: manual);
        if (_disposed || generation != _updateOperationGeneration) {
          return;
        }
        await _applyUpdateResult(result, generation);
        if (_disposed || generation != _updateOperationGeneration) return;
        if (result.available) {
          lastNotice =
              '${strings.get('update_available')} ${result.version ?? ''}'
                  .trim();
        }
        _notifyListeners();
      } on Object {
        // Automatic checks are optional and must not affect tunnel state.
        if (!_disposed &&
            generation == _updateOperationGeneration &&
            updatePhase == UpdateOperationPhase.checking) {
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
    if (success &&
        checked != null &&
        generation == _updateOperationGeneration) {
      await _applyUpdateResult(checked!, generation);
      if (_disposed || generation != _updateOperationGeneration) return;
      lastNotice = checked!.available
          ? '${strings.get('update_available')} ${checked!.version ?? ''}'
                .trim()
          : strings.get('already_latest');
      _notifyListeners();
    } else if (!_disposed &&
        generation == _updateOperationGeneration &&
        updatePhase == UpdateOperationPhase.checking) {
      updatePhase = updateResult?.available == true
          ? UpdateOperationPhase.available
          : UpdateOperationPhase.idle;
      _notifyListeners();
    }
  }

  Future<void> _applyUpdateResult(
    UpdateCheckResult result,
    int generation,
  ) async {
    String? cleanupError;
    final previousName = updateResult?.package?.name;
    final nextName = result.package?.name;
    final packageChanged = previousName != nextName;
    if (packageChanged && downloadedUpdatePath != null) {
      _updateCancellation?.cancel();
      try {
        await _updateDownloader.discard(downloadedUpdatePath);
      } on Object catch (error) {
        cleanupError = userFacingError(strings, error);
      }
      if (_disposed || generation != _updateOperationGeneration) return;
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
    updateError = cleanupError;
  }

  Future<bool> _run(
    Future<void> Function() operation, {
    bool affectsConnection = true,
    int? connectionIntent,
    bool allowDuringClear = false,
  }) async {
    if (_clearing && !allowDuringClear) return false;
    final generation = _dataGeneration;
    _activeOperations += 1;
    busy = true;
    lastError = null;
    _notifyListeners();
    try {
      await operation();
      return !_disposed && generation == _dataGeneration;
    } catch (error) {
      if (_disposed || generation != _dataGeneration) return false;
      if (connectionIntent != null && connectionIntent != _connectionIntent) {
        return false;
      }
      final message = userFacingError(strings, error);
      if (affectsConnection && snapshot.phase != ConnectionPhase.disconnected) {
        snapshot = EngineSnapshot(
          phase: ConnectionPhase.error,
          sessionCongestionControl: snapshot.sessionCongestionControl,
          dataPlane: snapshot.dataPlane,
          l4: snapshot.l4,
          vpnGate: snapshot.vpnGate,
          chainExit: snapshot.chainExit,
          warning: message,
          errorCode: error is EngineException ? error.code : null,
          errorRetryable: error is EngineException ? error.retryable : null,
        );
      }
      // Preserve exception-specific context after the snapshot setter maps its
      // structured code, including timeouts and localized adapter cleanup.
      lastError = message;
      return false;
    } finally {
      if (generation == _dataGeneration) _activeOperations -= 1;
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

  Object? _perAppSaveToken;

  /// The result confirms persisted policy. Runtime application is asynchronous.
  Future<({bool saved, String? error})> setPerAppProxy(
    PerAppProxySettings value,
  ) async {
    if (_clearing || _disposed || _perAppSaveToken != null) {
      return (saved: false, error: strings.get('operation_failed'));
    }
    final generation = _dataGeneration;
    final token = Object();
    _perAppSaveToken = token;
    try {
      final saved = await _engine.setPerAppProxy(value);
      if (_disposed || generation != _dataGeneration) {
        return (saved: false, error: strings.get('operation_failed'));
      }
      perAppProxy = saved;
      _notifyListeners();
      return (saved: true, error: null);
    } on Object catch (error) {
      return (saved: false, error: userFacingError(strings, error));
    } finally {
      if (identical(_perAppSaveToken, token)) _perAppSaveToken = null;
    }
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
      lastError = userFacingError(strings, error);
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
      lastError = userFacingError(strings, error);
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
    if (_clearing) return;
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
    _queueProfileMutation(
      () => _engine.upsertProfile(added),
      optimistic: () {
        if (!profiles.any((p) => p.id == added.id)) {
          profiles = [...profiles, added];
        }
      },
    );
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
    final generation = _dataGeneration;
    ProfileCatalog? catalog;
    final success = await _run(() async {
      catalog = await _engine.createProfileWithIdentity(
        profile,
        method: method,
        licenseKey: licenseKey,
        teamName: teamName,
        callbackUri: callbackUri,
      );
      if (_disposed || generation != _dataGeneration || _clearing) return;
      _profilesLoaded = true;
      profiles = catalog!.profiles;
      activeProfileId = catalog!.activeProfileId;
      profileIdentityStates = catalog!.identityStates;
      profileIdentityStatuses = catalog!.identityStatuses;
      _rememberManagedAccounts();
      for (final apply in _pendingAccountViews) {
        apply();
      }
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
    return _run(
      () => _mutateIdentity(
        profile.id,
        () => _engine.provisionIdentity(
          profile,
          method: method,
          licenseKey: licenseKey,
          teamName: teamName,
          callbackUri: callbackUri,
        ),
      ),
      affectsConnection: false,
      connectionIntent: _connectionIntent,
    );
  }

  Future<String> beginZeroTrustLogin(String teamName) async {
    final team = teamName.trim().toLowerCase();
    final nativeUrl = await _engine.beginZeroTrustLogin(team);
    return nativeUrl ?? 'https://$team.cloudflareaccess.com/warp';
  }

  Future<String?> consumeZeroTrustCallback() =>
      _engine.consumeZeroTrustCallback();

  Future<void> cancelZeroTrustLogin() async {
    try {
      await _engine.cancelZeroTrustLogin();
    } on Object catch (error) {
      lastError = userFacingError(strings, error);
      _notifyListeners();
    }
  }

  void updateProfile(UsqueProfile updated) {
    updateNetwork(updated);
  }

  void renameProfile(String id, String name) {
    if (_clearing) return;
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
    _queueProfileMutation(
      () => _engine.renameProfile(id, name),
      optimistic: () {
        profiles = profiles
            .map(
              (profile) =>
                  profile.id == id ? profile.copyWith(name: name) : profile,
            )
            .toList(growable: false);
      },
    );
  }

  void updateNetwork(UsqueProfile updated, {List<String>? changedFields}) {
    unawaited(saveNetwork(updated, changedFields: changedFields));
  }

  Future<bool> saveNetwork(
    UsqueProfile updated, {
    List<String>? changedFields,
  }) {
    if (updated.id != activeProfileId) return Future.value(false);
    if (updated.vpnGate.enabled && !(engineCapabilities?.vpnGateTcp ?? false)) {
      lastError = strings.vpnGateUnsupported;
      _notifyListeners();
      return Future.value(false);
    }
    if (updated.dataPlane == DataPlaneMode.l4Proxy &&
        !(engineCapabilities?.l4Available ?? false)) {
      lastError = strings.get('l4_unsupported');
      _notifyListeners();
      return Future.value(false);
    }
    return networkSettings.save(
      updated,
      changedFields ?? networkSettingsChangedFields(activeProfile, updated),
    );
  }

  void setActiveProfile(String id) {
    if (_clearing) return;
    if (profiles.any((profile) => profile.id == id)) {
      _connectionIntent++;
      activeProfileId = id;
      _notifyListeners();
      _queueProfileMutation(
        () => _engine.setActiveProfile(id),
        optimistic: () {
          if (profiles.any((p) => p.id == id)) activeProfileId = id;
        },
      );
    }
  }

  bool deleteProfile(String id) {
    if (_clearing) return false;
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
    _queueProfileMutation(
      () => _engine.deleteProfile(id),
      optimistic: () {
        profiles = profiles.where((p) => p.id != id).toList();
        profileIdentityStates = {...profileIdentityStates}..remove(id);
        profileIdentityStatuses = {...profileIdentityStatuses}..remove(id);
        if (activeProfileId == id && profiles.isNotEmpty) {
          activeProfileId = profiles.first.id;
        }
      },
    );
    return true;
  }

  Future<bool> _queueProfileMutation(
    Future<void> Function() mutation, {
    required void Function() optimistic,
  }) {
    final generation = _dataGeneration;
    _pendingAccountViews.add(optimistic);
    return networkSettings.enqueue(() async {
      if (_clearing || generation != _dataGeneration) {
        _pendingAccountViews.remove(optimistic);
        return false;
      }
      var succeeded = false;
      try {
        await mutation();
        succeeded = true;
      } on Object catch (error) {
        lastError = userFacingError(strings, error);
      }
      _pendingAccountViews.remove(optimistic);
      try {
        await _refreshProfileCatalog();
      } on Object {
        // Keep the current view if authoritative readback is unavailable.
        // Subsequent mutations retain their original immutable arguments.
      }
      _notifyListeners();
      return succeeded;
    });
  }

  /// Waits for already queued non-secret profile writes. Installers and tests
  /// can use this before terminating the UI process.
  Future<void> flushProfileWrites() => networkSettings.flushed;

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
    if (_disposed ||
        _clearing ||
        generation != _snapshotSubscriptionGeneration) {
      return;
    }
    _snapshotSubscription = _engine.snapshotEvents.listen(
      (EngineSnapshotEvent event) => _handleSnapshotEvent(event, generation),
      onError: (Object error, StackTrace stackTrace) =>
          _handleSnapshotEventError(error, stackTrace, generation),
      onDone: () => _handleSnapshotEventDone(generation),
      cancelOnError: false,
    );
    unawaited(networkSettings.refresh());
  }

  void _handleSnapshotEvent(EngineSnapshotEvent event, int generation) {
    if (_disposed ||
        _clearing ||
        generation != _snapshotSubscriptionGeneration) {
      return;
    }
    if (event.snapshot != null || event.networkQuality != null) {
      _snapshotRevision++;
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
        nextQuality != null &&
        (nextQuality != networkQuality ||
            nextQuality.sampledAt != networkQuality?.sampledAt);
    if (nextQuality != null && event.snapshot == null) {
      networkQuality = nextQuality;
    }
    final handledCapabilities =
        event.capabilities != null && event.capabilities != engineCapabilities;
    if (handledCapabilities) {
      engineCapabilities = event.capabilities;
    }
    if (event.networkSettings != null) {
      networkSettings.accept(event.networkSettings!);
    }
    final next = event.snapshot;
    if (next != null) _initialStatusLoaded = true;
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
        ? userFacingFailure(
            strings,
            code: next.errorCode,
            details: next.warning,
          )
        : null;
    final errorChanged = nextError != null && nextError != lastError;
    // Presentation equality intentionally ignores quality timestamps. The
    // observation stream must still ingest an unchanged, newly sampled frame,
    // even when its quality-only companion arrived before this full snapshot.
    final snapshotChanged =
        next != snapshot ||
        next.networkQuality?.sampledAt != snapshot.networkQuality?.sampledAt;
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
    if (_disposed ||
        _clearing ||
        generation != _snapshotSubscriptionGeneration) {
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
    _bootstrapGeneration++;
    _bootstrapRetryTimer?.cancel();
    _connectionIntent++;
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
    networkSettings.dispose();
    unawaited(networkSettings.flushed.whenComplete(_engine.dispose));
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
