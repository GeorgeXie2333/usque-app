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
        OnboardingPermissionsClient,
        EndpointSubscriptionClient {
  @override
  Future<Map<String, Object?>> scenarioCommand(
    Map<String, Object?> command,
  ) async {
    throw const EngineException('SCENARIOS_UNSUPPORTED', 'Scenes unavailable.');
  }

  PreviewEngine({bool identityReady = true}) {
    _profiles = [
      UsqueProfile.defaultProfile().copyWith(
        name: 'Preview account',
        autoConnect: false,
      ),
    ];
    if (identityReady) _identities[_activeId] = _readyIdentity;
  }

  static const _readyIdentity = ProfileIdentityStatus(
    state: ProfileIdentityState.ready,
    licenseState: LicenseState.free,
    accountType: 'Preview',
  );
  static const _capabilities = EngineCapabilities(
    vpn: true,
    systemProxy: true,
    automaticEndpoints: true,
    endpointPresets: true,
    networkSettingsApplication: true,
    applicationQuicBlocking: true,
    accountMetadataMutations: true,
    sharedProxyAuthApplication: true,
    customBypass: true,
    networkQuality: true,
    encryptedDirectDns: true,
    encryptedWarpDns: true,
    endpointSubscriptions: true,
    endpointSubscriptionDomainEndpoints: true,
    endpointSubscriptionProbe: true,
    endpointSubscriptionConnectedTunProbe: true,
    h3CongestionControlAlgorithms: CongestionControlAlgorithm.values,
    tlsFingerprints: TlsFingerprint.values,
  );

  Timer? _probeTimer;
  String? _activeProbeId;
  String? _activeProbeSubId;
  String? _activeSubscriptionId;
  int _subRevision = 1;
  final List<Map<String, Object?>> _subscriptions = _initialPreviewSubs();

  static List<Map<String, Object?>> _initialPreviewSubs() => [
    <String, Object?>{
      'id': '11111111-1111-4111-8111-111111111111',
      'revision': 'rev-free-1',
      'name': 'Cloudflare Edge (Preview)',
      'url': 'https://masque.pages.dev/endpoint/3rd/warp-ipv4.txt',
      'account_tier': 'free',
      'auto_refresh': 'daily',
      'last_refreshed_at_unix_ms': 1795800000000,
      'skipped_line_count': 0,
      'selected_endpoint_id': 'v4:162.159.193.10:2408',
      'endpoints': <Map<String, Object?>>[
        <String, Object?>{
          'id': 'v4:162.159.193.10:2408',
          'host': <String, Object?>{'kind': 'ipv4', 'value': '162.159.193.10'},
          'port': 2408,
          'note': 'Tokyo Edge #1',
          'pinned': true,
        },
        <String, Object?>{
          'id': 'v4:162.159.192.1:500',
          'host': <String, Object?>{'kind': 'ipv4', 'value': '162.159.192.1'},
          'port': 500,
          'note': 'Osaka Anycast',
          'pinned': false,
        },
        <String, Object?>{
          'id': 'v6:2606:4700:103::2:443',
          'host': <String, Object?>{
            'kind': 'ipv6',
            'value': '2606:4700:103::2',
          },
          'port': 443,
          'note': 'IPv6 Anycast',
          'pinned': false,
        },
        <String, Object?>{
          'id': 'host:engage.cloudflareclient.com:2408',
          'host': <String, Object?>{
            'kind': 'domain',
            'value': 'engage.cloudflareclient.com',
          },
          'port': 2408,
          'note': 'Domain Endpoint',
          'pinned': false,
        },
      ],
    },
    <String, Object?>{
      'id': '22222222-2222-4222-8222-222222222222',
      'revision': 'rev-plus-1',
      'name': 'WARP+ High Speed (Preview)',
      'url': 'https://masque.pages.dev/endpoint/3rd/warp-plus.txt',
      'account_tier': 'plus',
      'auto_refresh': 'weekly',
      'last_refreshed_at_unix_ms': 1795800000000,
      'skipped_line_count': 0,
      'selected_endpoint_id': 'v4:162.159.195.2:2408',
      'endpoints': <Map<String, Object?>>[
        <String, Object?>{
          'id': 'v4:162.159.195.2:2408',
          'host': <String, Object?>{'kind': 'ipv4', 'value': '162.159.195.2'},
          'port': 2408,
          'note': 'Plus Singapore',
          'pinned': false,
        },
      ],
    },
    <String, Object?>{
      'id': '33333333-3333-4333-8333-333333333333',
      'revision': 'rev-zt-1',
      'name': 'Zero Trust Custom (Preview)',
      'url': 'https://masque.pages.dev/endpoint/3rd/zt-ipv4.txt',
      'account_tier': 'zero_trust',
      'auto_refresh': 'off',
      'last_refreshed_at_unix_ms': 1795800000000,
      'skipped_line_count': 0,
      'selected_endpoint_id': 'v4:162.159.198.10:2408',
      'endpoints': <Map<String, Object?>>[
        <String, Object?>{
          'id': 'v4:162.159.198.10:2408',
          'host': <String, Object?>{'kind': 'ipv4', 'value': '162.159.198.10'},
          'port': 2408,
          'note': 'ZT Gateway',
          'pinned': false,
        },
      ],
    },
  ];

  Map<String, Object?> _subscriptionListPayload({
    String? applyStatus,
    Map<String, Object?>? subscription,
    String? probeId,
  }) {
    final activeSub = _subscriptions
        .where((s) => s['id'] == _activeSubscriptionId)
        .firstOrNull;
    final activeTier = switch (_identities[_activeId]?.provider) {
      IdentityProvider.zeroTrust => 'zero_trust',
      _ => switch (_identities[_activeId]?.licenseState) {
        LicenseState.warpPlus => 'plus',
        LicenseState.free => 'free',
        _ => null,
      },
    };
    return <String, Object?>{
      'revision': 'preview-$_subRevision-$_activeId-$_activeSubscriptionId',
      'unchanged': false,
      'subscriptions': [
        for (final s in _subscriptions)
          Map<String, Object?>.from(s)
            ..['endpoints'] = [
              for (final ep in (s['endpoints'] as List))
                Map<String, Object?>.from(ep as Map),
            ],
      ],
      'active_account_id': _activeId,
      'active_account_tier': activeTier,
      'active_subscription_id': _activeSubscriptionId,
      'active_endpoint_id': activeSub?['selected_endpoint_id'],
      'refreshing_subscription_ids': const <String>[],
      if (_activeProbeId != null && _activeProbeSubId != null)
        'active_probe': <String, Object?>{
          'probe_id': _activeProbeId,
          'subscription_id': _activeProbeSubId,
        },
      'apply_status': ?applyStatus,
      'subscription': ?subscription,
      'probe_id': ?probeId,
    };
  }

  @override
  Future<Map<String, Object?>> endpointSubscriptionCommand(
    Map<String, Object?> request,
  ) async {
    if (_disposed) _unavailable();
    final action = request['action'] as String? ?? 'list';
    switch (action) {
      case 'list':
        return _subscriptionListPayload();
      case 'add':
        final id = 'sub-${++_subRevision}';
        final created = <String, Object?>{
          'id': id,
          'revision': 'rev-$_subRevision',
          'name': request['name'] as String? ?? 'Custom Subscription',
          'url': request['url'] as String? ?? 'https://example.com/list.txt',
          'account_tier': request['account_tier'] as String? ?? 'free',
          'auto_refresh': request['auto_refresh'] as String? ?? 'off',
          'last_refreshed_at_unix_ms': DateTime.now().millisecondsSinceEpoch,
          'skipped_line_count': 0,
          'selected_endpoint_id': 'v4:162.159.193.20:2408',
          'endpoints': <Map<String, Object?>>[
            <String, Object?>{
              'id': 'v4:162.159.193.20:2408',
              'host': <String, Object?>{
                'kind': 'ipv4',
                'value': '162.159.193.20',
              },
              'port': 2408,
              'note': 'Demo Added Edge',
              'pinned': false,
            },
          ],
        };
        _subscriptions.add(created);
        return _subscriptionListPayload(subscription: created);
      case 'update':
        final id = request['id'];
        final index = _subscriptions.indexWhere((s) => s['id'] == id);
        if (index < 0) {
          throw const EngineException('SUBSCRIPTION_NOT_FOUND', 'Not found.');
        }
        _subRevision++;
        _subscriptions[index] = <String, Object?>{
          ..._subscriptions[index],
          'revision': 'rev-$_subRevision',
          'name': request['name'] ?? _subscriptions[index]['name'],
          'url': request['url'] ?? _subscriptions[index]['url'],
          'account_tier':
              request['account_tier'] ?? _subscriptions[index]['account_tier'],
          'auto_refresh':
              request['auto_refresh'] ?? _subscriptions[index]['auto_refresh'],
        };
        return _subscriptionListPayload();
      case 'delete':
        final id = request['id'];
        _subscriptions.removeWhere((s) => s['id'] == id);
        if (_activeSubscriptionId == id) {
          _activeSubscriptionId = null;
          await upsertProfile(
            _active.copyWith(clearEndpointSubscriptionId: true),
          );
        }
        _subRevision++;
        return _subscriptionListPayload();
      case 'refresh':
        final id = request['id'];
        final index = _subscriptions.indexWhere((s) => s['id'] == id);
        if (index < 0) {
          throw const EngineException('SUBSCRIPTION_NOT_FOUND', 'Not found.');
        }
        _subRevision++;
        _subscriptions[index] = <String, Object?>{
          ..._subscriptions[index],
          'revision': 'rev-$_subRevision',
          'last_refreshed_at_unix_ms': DateTime.now().millisecondsSinceEpoch,
        };
        return _subscriptionListPayload(subscription: _subscriptions[index]);
      case 'select_endpoint':
        final subId = request['subscription_id'] as String?;
        final epId = request['endpoint_id'] as String?;
        final index = _subscriptions.indexWhere((s) => s['id'] == subId);
        if (index < 0 || epId == null) {
          throw const EngineException('SUBSCRIPTION_NOT_FOUND', 'Not found.');
        }
        _subRevision++;
        _activeSubscriptionId = subId;
        _subscriptions[index] = <String, Object?>{
          ..._subscriptions[index],
          'revision': 'rev-$_subRevision',
          'selected_endpoint_id': epId,
        };
        await upsertProfile(
          _active.copyWith(
            endpointSelection: EndpointSelection.custom,
            clearEndpointPresetRegion: true,
            endpointSubscriptionId: subId,
          ),
        );
        return _subscriptionListPayload(
          applyStatus: _current.isConnected ? 'applied' : 'deferred',
        );
      case 'deactivate':
        _subRevision++;
        _activeSubscriptionId = null;
        await upsertProfile(
          _active.copyWith(clearEndpointSubscriptionId: true),
        );
        return _subscriptionListPayload(
          applyStatus: _current.isConnected ? 'applied' : 'deferred',
        );
      case 'set_pinned':
        final subId = request['subscription_id'] as String?;
        final epId = request['endpoint_id'] as String?;
        final pinned = request['pinned'] == true;
        final index = _subscriptions.indexWhere((s) => s['id'] == subId);
        if (index >= 0) {
          final eps = [
            for (final ep in (_subscriptions[index]['endpoints'] as List))
              Map<String, Object?>.from(ep as Map)
                ..['pinned'] = (ep['id'] == epId ? pinned : ep['pinned']),
          ];
          _subscriptions[index] = <String, Object?>{
            ..._subscriptions[index],
            'revision': 'rev-${++_subRevision}',
            'endpoints': eps,
          };
        }
        return _subscriptionListPayload();
      case 'start_probe':
        _probeTimer?.cancel();
        final subId = request['subscription_id'] as String? ?? '';
        final probeId = request['probe_id'] as String? ?? 'preview-probe';
        final sub = _subscriptions.where((s) => s['id'] == subId).firstOrNull;
        final rawIds = request['endpoint_ids'] as List?;
        final allEps = (sub?['endpoints'] as List? ?? const [])
            .whereType<Map<Object?, Object?>>()
            .map((e) => e['id'] as String)
            .toList(growable: false);
        final targets = rawIds == null
            ? allEps
            : rawIds.whereType<String>().toList(growable: false);
        _activeProbeId = probeId;
        _activeProbeSubId = subId;
        final results = <Map<String, Object?>>[];
        var nextIndex = 0;
        _probeTimer = Timer.periodic(const Duration(milliseconds: 90), (t) {
          if (_disposed || _activeProbeId != probeId) {
            t.cancel();
            return;
          }
          if (nextIndex < targets.length) {
            final epId = targets[nextIndex];
            results.add(<String, Object?>{
              'endpoint_id': epId,
              'status': 'reachable',
              'rtt_ms': 28 + (nextIndex * 14) % 90,
              'protocol': epId.startsWith('host:') ? 'http2' : 'http3',
            });
            nextIndex++;
          }
          final done = nextIndex >= targets.length;
          if (done) {
            t.cancel();
            _activeProbeId = null;
            _activeProbeSubId = null;
          }
          _events.add(
            EngineSnapshotEvent(
              endpointSubscriptionProbeProgress: <String, Object?>{
                'probe_id': probeId,
                'subscription_id': subId,
                'state': done ? 'completed' : 'running',
                'completed': results.length,
                'total': targets.length,
                'results': List<Map<String, Object?>>.from(results),
              },
            ),
          );
        });
        return _subscriptionListPayload(probeId: probeId);
      case 'cancel_probe':
        final probeId = request['probe_id'] as String?;
        if (_activeProbeId == probeId && probeId != null) {
          _probeTimer?.cancel();
          _probeTimer = null;
          final subId = _activeProbeSubId ?? '';
          _activeProbeId = null;
          _activeProbeSubId = null;
          _events.add(
            EngineSnapshotEvent(
              endpointSubscriptionProbeProgress: <String, Object?>{
                'probe_id': probeId,
                'subscription_id': subId,
                'state': 'cancelled',
                'completed': 0,
                'total': 0,
                'results': const <Object>[],
              },
            ),
          );
        }
        return _subscriptionListPayload();
      default:
        return _subscriptionListPayload();
    }
  }

  late List<UsqueProfile> _profiles;
  String _activeId = UsqueProfile.defaultProfileId;
  final _identities = <String, ProfileIdentityStatus>{};
  final _events = StreamController<EngineSnapshotEvent>.broadcast();
  Timer? _ticker;
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
    // Idle previews have no changing traffic. Avoid waking the controller and
    // rebuilding the preview toolbar every second while disconnected.
    if (_current.isConnected) {
      _ticker ??= Timer.periodic(const Duration(seconds: 1), (_) => _sample());
    } else {
      _ticker?.cancel();
      _ticker = null;
    }
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
  Future<void> setHideFromRecents(bool enabled) async => _unavailable();
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
    _probeTimer?.cancel();
    _probeTimer = null;
    _activeProbeId = null;
    _activeProbeSubId = null;
    _activeSubscriptionId = null;
    _subscriptions
      ..clear()
      ..addAll(_initialPreviewSubs());
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
    _probeTimer?.cancel();
    _probeTimer = null;
    _ticker?.cancel();
    unawaited(_events.close());
  }
}
