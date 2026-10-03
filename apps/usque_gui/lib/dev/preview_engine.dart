import 'dart:async';
import 'dart:typed_data';

import '../models/app_models.dart';
import '../models/diagnostics_models.dart';
import '../services/engine_client.dart';

/// In-memory UI scenarios. This client has no native transport or I/O backend.
class PreviewEngine
    implements
        EngineClient,
        InitialIdentityClient,
        OnboardingPermissionsClient {
  PreviewEngine({bool identityReady = true}) {
    _profiles = [
      UsqueProfile.defaultProfile().copyWith(
        name: 'Preview account',
        autoConnect: false,
      ),
    ];
    if (identityReady) _identities[_activeId] = _readyIdentity;
    _ticker = Timer.periodic(const Duration(seconds: 1), (_) => _sample());
  }

  static const _readyIdentity = ProfileIdentityStatus(
    state: ProfileIdentityState.ready,
    licenseState: LicenseState.free,
    accountType: 'Preview',
  );
  static const _capabilities = EngineCapabilities(
    automaticEndpoints: true,
    networkSettingsApplication: true,
    applicationQuicBlocking: true,
    accountMetadataMutations: true,
    sharedProxyAuthApplication: true,
    customBypass: true,
    networkQuality: true,
    encryptedDirectDns: true,
    h3CongestionControlAlgorithms: CongestionControlAlgorithm.values,
  );

  late List<UsqueProfile> _profiles;
  String _activeId = UsqueProfile.defaultProfileId;
  final _identities = <String, ProfileIdentityStatus>{};
  final _events = StreamController<EngineSnapshotEvent>.broadcast();
  late final Timer _ticker;
  EngineSnapshot _current = const EngineSnapshot();
  NetworkSettingsState? _settings;
  DiagnosticSession? _diagnostics;
  int _settingsSequence = 0;
  int _intent = 0;
  int _sampleSequence = 0;
  int _downloaded = 0;
  int _uploaded = 0;
  DateTime? _connectedAt;
  bool _disposed = false;

  UsqueProfile get _active =>
      _profiles.firstWhere((profile) => profile.id == _activeId);

  ProfileCatalog get _catalog => ProfileCatalog(
    profiles: List.unmodifiable(_profiles),
    activeProfileId: _activeId,
    identityStates: {
      for (final profile in _profiles)
        profile.id:
            _identities[profile.id]?.state ?? ProfileIdentityState.missing,
    },
    identityStatuses: Map.unmodifiable(_identities),
    sharedNetwork: _active,
  );

  Never _unavailable() => throw const EngineException(
    'PREVIEW_UNAVAILABLE',
    'This operation is unavailable in the UI preview.',
  );

  void showPhase(ConnectionPhase phase) {
    if (_disposed) return;
    _intent++;
    if (phase == ConnectionPhase.connected && !_current.isConnected) {
      _connectedAt = DateTime.now().toUtc();
      _downloaded = 0;
      _uploaded = 0;
    }
    _publish(phase);
  }

  void _sample() {
    if (_disposed) return;
    if (_current.isConnected) {
      _downloaded += 180000 + (_sampleSequence % 5) * 45000;
      _uploaded += 18000 + (_sampleSequence % 3) * 6000;
    }
    _publish(_current.phase);
  }

  void _publish(ConnectionPhase phase) {
    if (_disposed) return;
    final connected = phase == ConnectionPhase.connected;
    final at = DateTime.now().toUtc();
    final quality = NetworkQualitySnapshot(
      sampledAt: at,
      connectionInstanceId:
          'preview-${_connectedAt?.millisecondsSinceEpoch ?? 0}',
      level: connected
          ? NetworkQualityLevel.good
          : NetworkQualityLevel.disconnected,
      metrics: NetworkConnectionMetrics(
        latestRttMilliseconds: connected ? 24 : null,
        latestRttAvailability: connected
            ? MetricAvailability.available
            : MetricAvailability.notReady,
      ),
      samples: connected
          ? [
              NetworkQualitySample(
                sequence: ++_sampleSequence,
                sampledAt: at,
                monotonicMillis: _sampleSequence * 1000,
                downloadedBytes: _downloaded,
                uploadedBytes: _uploaded,
                rttMilliseconds: 24,
                lossBasisPoints: 0,
              ),
            ]
          : const [],
    );
    _current = EngineSnapshot(
      phase: phase,
      transport: connected ? 'HTTP/3' : null,
      addressFamily: connected ? 'IPv6' : null,
      connectedAt: connected ? _connectedAt : null,
      downloadBytesPerSecond: connected ? 240000 : 0,
      uploadBytesPerSecond: connected ? 24000 : 0,
      downloadedBytes: connected ? _downloaded : 0,
      uploadedBytes: connected ? _uploaded : 0,
      killSwitchState: 'inactive',
      exit: connected
          ? const ExitInfo(
              city: 'Tokyo',
              country: 'Japan',
              countryCode: 'JP',
              ipv4: '198.51.100.7',
              ipv6: '2001:db8::7',
            )
          : const ExitInfo(),
      errorCode: phase == ConnectionPhase.error ? 'CONNECTION_FAILED' : null,
      errorRetryable: phase == ConnectionPhase.error ? true : null,
      networkQuality: quality,
    );
    _events.add(EngineSnapshotEvent(snapshot: _current));
  }

  @override
  bool get supportsSnapshotEvents => true;
  @override
  Stream<EngineSnapshotEvent> get snapshotEvents => _events.stream;
  @override
  Future<EngineSnapshot> snapshot() async => _current;
  @override
  Future<NetworkQualitySnapshot?> getNetworkQuality() async =>
      _current.networkQuality;
  @override
  Future<EngineCapabilities?> getCapabilities() async => _capabilities;

  @override
  Future<EngineSnapshot> connect(UsqueProfile profile) async {
    if (_disposed) _unavailable();
    showPhase(ConnectionPhase.connectingH3);
    final intent = _intent;
    await Future<void>.delayed(const Duration(milliseconds: 400));
    if (!_disposed && intent == _intent) showPhase(ConnectionPhase.connected);
    return _current;
  }

  @override
  Future<EngineSnapshot> disconnect() async {
    showPhase(ConnectionPhase.disconnected);
    return _current;
  }

  @override
  Future<EngineSnapshot> retry() => connect(_active);

  @override
  Future<ProfileCatalog> importLegacyProfiles(
    List<UsqueProfile> profiles,
    String activeProfileId,
  ) async => _catalog;

  @override
  Future<void> upsertProfile(UsqueProfile profile) async {
    _settings = null;
    final exists = _profiles.any((value) => value.id == profile.id);
    _profiles = [
      for (final value in _profiles)
        profile.copyWith(
          id: value.id,
          name: value.id == profile.id ? profile.name : value.name,
        ),
      if (!exists) profile,
    ];
  }

  @override
  Future<void> renameProfile(String profileId, String name) async {
    _settings = null;
    _profiles = [
      for (final value in _profiles)
        value.id == profileId ? value.copyWith(name: name) : value,
    ];
  }

  @override
  Future<void> deleteProfile(String profileId) async {
    if (_profiles.length == 1) _unavailable();
    _settings = null;
    _profiles.removeWhere((profile) => profile.id == profileId);
    _identities.remove(profileId);
    if (_activeId == profileId) _activeId = _profiles.first.id;
  }

  @override
  Future<void> setActiveProfile(String profileId) async {
    if (!_profiles.any((profile) => profile.id == profileId)) _unavailable();
    _settings = null;
    _activeId = profileId;
  }

  @override
  Future<void> provisionIdentity(
    UsqueProfile profile, {
    required IdentityProvisioningMethod method,
    String? licenseKey,
    String? teamName,
    String? callbackUri,
  }) async {
    _identities[profile.id] = _readyIdentity;
  }

  @override
  Future<ProfileCatalog> createProfileWithIdentity(
    UsqueProfile profile, {
    required IdentityProvisioningMethod method,
    String? licenseKey,
    String? teamName,
    String? callbackUri,
  }) async {
    await upsertProfile(profile);
    _identities[profile.id] = _readyIdentity;
    return _catalog;
  }

  @override
  Future<InitialIdentityState> getInitialIdentityState(
    String profileId,
  ) async => InitialIdentityState(
    profileId: profileId,
    phase: _identities.containsKey(profileId)
        ? InitialIdentityPhase.completed
        : InitialIdentityPhase.idle,
  );

  @override
  Future<InitialIdentityState> initializeIdentity(
    UsqueProfile profile, {
    required String operationId,
    required IdentityProvisioningMethod method,
    bool resumeOnly = false,
    String? licenseKey,
    String? teamName,
    String? callbackUri,
  }) async {
    if (!resumeOnly) _identities[profile.id] = _readyIdentity;
    return getInitialIdentityState(profile.id);
  }

  @override
  Future<OnboardingPermissionState> getOnboardingPermissions() async =>
      const OnboardingPermissionState(
        vpnGranted: true,
        notification: OnboardingNotificationPermission.notRequired,
      );
  @override
  Future<OnboardingPermissionState> prepareOnboardingPermissions() =>
      getOnboardingPermissions();

  @override
  Future<NetworkSettingsState> getNetworkSettingsState() async =>
      _settings ??
      NetworkSettingsState(
        sourceEpoch: 'ui-preview',
        sequence: _settingsSequence,
        storedProfile: _active,
        sharedNetwork: _active,
      );

  @override
  Future<NetworkSettingsState> saveNetworkSettings(
    String operationId,
    String accountId,
    UsqueProfile values,
    List<String> changedFields,
  ) async {
    await upsertProfile(values);
    _settings = NetworkSettingsState(
      sourceEpoch: 'ui-preview',
      sequence: ++_settingsSequence,
      operationId: operationId,
      storedProfile: _profiles.firstWhere((profile) => profile.id == accountId),
      sharedNetwork: _active,
      status: NetworkSettingsApplyStatus.deferred,
      deferredFields: changedFields,
      persisted: true,
    );
    return _settings!;
  }

  @override
  Future<void> reconfigureActiveProfile(UsqueProfile profile) =>
      upsertProfile(profile);
  @override
  Future<void> updateProxyAuth(
    String profileId, {
    required String username,
    required String password,
    bool confirmed = true,
  }) async {
    if (!confirmed || username.isNotEmpty && password.isEmpty) _unavailable();
    await upsertProfile(
      _active.copyWith(proxy: _active.proxy.copyWith(authUsername: username)),
    );
  }

  @override
  Future<void> copyLicenseKey(String profileId) async => _unavailable();
  @override
  Future<void> updateLicenseKey(String profileId, String licenseKey) async {}
  @override
  Future<void> unbindLicenseKey(String profileId) async {}
  @override
  Future<String?> exportWarpSecret(String profileId) async => _unavailable();
  @override
  Future<String?> consumeLaunchTarget() async => null;
  @override
  Future<String?> beginZeroTrustLogin(String teamName) async => _unavailable();
  @override
  Future<String?> consumeZeroTrustCallback() async => null;
  @override
  Future<void> cancelZeroTrustLogin() async {}
  @override
  Future<PlatformPreferences> platformPreferences() async =>
      const PlatformPreferences(closeToTray: false);
  @override
  Future<void> setStartOnBoot(bool enabled) async => _unavailable();
  @override
  Future<void> setCloseToTray(bool enabled) async => _unavailable();
  @override
  Future<void> requestAddQuickSettingsTile() async => _unavailable();
  @override
  Future<PerAppProxySettings> perAppProxy() async =>
      const PerAppProxySettings();
  @override
  Future<PerAppProxySettings> setPerAppProxy(
    PerAppProxySettings settings,
  ) async => settings;
  @override
  Future<List<InstalledAppInfo>> listInstalledApps() async => const [];
  @override
  Future<Uint8List?> getAppIcon(String packageName) async => null;
  @override
  Future<void> openAlwaysOnVpnSettings() async => _unavailable();

  @override
  Future<DiagnosticSession> startDiagnostics(DiagnosticMode mode) async {
    final at = DateTime.now().toUtc();
    return _diagnostics = DiagnosticSession(
      sessionId: 'ui-preview',
      state: DiagnosticSessionState.completed,
      startedAt: at,
      completedAt: at,
      mode: mode,
      progressPercent: 100,
    );
  }

  @override
  Future<DiagnosticSession> cancelDiagnostics(String sessionId) async =>
      _diagnostics ?? await startDiagnostics(DiagnosticMode.standard);
  @override
  Future<DiagnosticSession?> getDiagnostics() async => _diagnostics;
  @override
  Future<ConnectionTimeline> getConnectionTimeline() async =>
      const ConnectionTimeline();
  @override
  Future<String?> exportDiagnostics({String? diagnosticSessionId}) async =>
      _unavailable();
  @override
  Future<UpdateCheckResult> checkForUpdates({bool manual = true}) async =>
      const UpdateCheckResult.current();
  @override
  Future<String> getUpdateCacheDirectory() async => _unavailable();
  @override
  Future<void> verifyUpdatePackage({
    required String path,
    required String version,
    required UpdatePackage package,
  }) async => _unavailable();
  @override
  Future<void> installUpdatePackage({
    required String path,
    required String version,
    required UpdatePackage package,
  }) async => _unavailable();
  @override
  Future<GeoRulesList> listGeoRules() async => const GeoRulesList();
  @override
  Future<List<GeoRulesUpdateResult>> downloadGeoRules(
    String countryCode,
  ) async => _unavailable();
  @override
  Future<List<GeoRulesUpdateResult>> updateAllGeoRules() async =>
      _unavailable();
  @override
  Future<void> clearAllData({required bool confirmed}) async {
    if (!confirmed) _unavailable();
    _identities.clear();
    _settings = null;
    _diagnostics = null;
    showPhase(ConnectionPhase.disconnected);
    _profiles = [
      UsqueProfile.defaultProfile().copyWith(
        name: 'Preview account',
        autoConnect: false,
      ),
    ];
    _activeId = UsqueProfile.defaultProfileId;
  }

  @override
  void dispose() {
    if (_disposed) return;
    _disposed = true;
    _intent++;
    _ticker.cancel();
    unawaited(_events.close());
  }
}
