import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:flutter/foundation.dart';

import '../models/app_models.dart';
import '../models/diagnostics_models.dart';
import 'control_codec.dart';
import 'desktop_engine_transport.dart';
import 'engine_client.dart';

export 'control_codec.dart'
    show
        debugDecodeConnectionTimelineFrame,
        debugDecodeEventSnapshot,
        debugDecodeEventFrame,
        debugDecodeCapabilitiesFrame,
        debugDecodeNetworkQualityFrame,
        debugDecodeProfileCatalogFrame,
        debugDecodeStatusFrame,
        debugEncodeGetStatusFrame,
        debugEncodeProfilePayload;

/// Desktop [EngineClient] that coordinates request serialization, codec, and
/// transport. Public API, MethodChannel names, named pipes, and protobuf wire
/// data are unchanged from the pre-split client.
class DesktopEngineClient
    implements
        EngineClient,
        StartupProfileClient,
        VpnGateClient,
        ChainProfileClient,
        WarpWireguardClient,
        ProtonVpnClient,
        ProtonHumanVerificationClient,
        InitialIdentityClient,
        LinuxServiceClient,
        AdsRulesClient {
  @override
  Future<LinuxServiceState> getLinuxServiceState() async =>
      LinuxServiceState.fromMap(
        await _transport.invokePlatformMethod<Map<Object?, Object?>>(
              'getLinuxServiceState',
            ) ??
            const {},
      );

  @override
  Future<void> maintainLinuxService({required bool install}) async {
    if (!Platform.isLinux) {
      throw const EngineException(
        'LINUX_SERVICE_UNAVAILABLE',
        'Linux is required.',
      );
    }
    final state = await getLinuxServiceState();
    final bundle = Platform.environment['APPDIR'];
    if (install &&
            (!state.canInstall || bundle == null || !bundle.startsWith('/')) ||
        !install && !state.canRepair) {
      throw const EngineException(
        'LINUX_SERVICE_UNAVAILABLE',
        'Reinstall the application package.',
      );
    }
    final helper = install && !state.installed
        ? '$bundle/usr/lib/usque/usque-update'
        : '/usr/lib/usque/usque-update';
    final result = await Process.run('/usr/bin/pkexec', [
      helper,
      if (install) ...[
        '--install-service',
        '--bundle',
        bundle!,
      ] else
        '--repair-service',
    ], runInShell: false);
    if (result.exitCode != 0) {
      if (result.stderr.toString().contains(
        'USQUE_LINUX_DEPENDENCIES_MISSING',
      )) {
        throw const EngineException(
          'LINUX_SERVICE_DEPENDENCIES_MISSING',
          'Install the required host dependencies and retry.',
          retryable: true,
        );
      }
      throw const EngineException(
        'LINUX_SERVICE_MAINTENANCE_FAILED',
        'Service maintenance was cancelled or failed. The operation can be retried.',
        retryable: true,
      );
    }
  }

  @override
  Future<InitialIdentityState> initializeIdentity(
    UsqueProfile profile, {
    required String operationId,
    required IdentityProvisioningMethod method,
    bool resumeOnly = false,
    String? licenseKey,
    String? teamName,
    String? callbackUri,
  }) => _serialized(() async {
    final license = Uint8List.fromList(utf8.encode(licenseKey ?? ''));
    final callback = Uint8List.fromList(utf8.encode(callbackUri ?? ''));
    try {
      final provisioning = ControlPayloadWriter()
        ..enumeration(1, _identityProvisioningWireValue(method))
        ..boolean(3, true)
        ..string(4, Platform.localeName)
        ..bytes(6, license);
      if (method == IdentityProvisioningMethod.zeroTrust) {
        provisioning.message(
          7,
          (ControlPayloadWriter()
                ..string(1, teamName ?? '')
                ..bytes(2, callback))
              .takeBytes(),
        );
      }
      final payload = ControlPayloadWriter()
        ..string(1, operationId)
        ..string(2, profile.id)
        ..message(3, provisioning.takeBytes())
        ..boolean(4, resumeOnly);
      return _requireInitialIdentityState(
        await _request(49, payload.takeBytes()),
      );
    } finally {
      license.fillRange(0, license.length, 0);
      callback.fillRange(0, callback.length, 0);
    }
  });

  @override
  Future<InitialIdentityState> getInitialIdentityState(String profileId) =>
      _serialized(() async {
        final payload = ControlPayloadWriter()..string(1, profileId);
        return _requireInitialIdentityState(
          await _request(50, payload.takeBytes()),
        );
      });

  InitialIdentityState _requireInitialIdentityState(ControlResponse response) =>
      response.initialIdentityState ??
      (throw const EngineException(
        'INITIAL_IDENTITY_UNSUPPORTED',
        'Initial setup unavailable.',
      ));

  @override
  Future<Map<Object?, Object?>> warpWireguard(Map<String, Object?> request) =>
      _serialized(() async {
        final payload = ControlPayloadWriter()..string(1, jsonEncode(request));
        return (await _request(48, payload.takeBytes())).warpWireguard ??
            const {};
      });
  @override
  Future<ProtonVpnResult> protonVpn(
    Map<String, Object?> command, {
    Uint8List? sensitiveInput,
  }) => _serialized(() async {
    if (sensitiveInput != null && sensitiveInput.length > 40 * 1024) {
      throw const EngineException(
        'PROTON_INPUT_TOO_LARGE',
        'Invalid authentication input.',
      );
    }
    final payload = ControlPayloadWriter()
      ..string(1, encodeProtonVpnCommand(command))
      ..bytes(2, sensitiveInput ?? Uint8List(0));
    final encoded = payload.takeBytes();
    try {
      return (await _request(52, encoded)).protonVpn ??
          (throw const EngineException(
            'PROTON_UNAVAILABLE',
            'Proton VPN is unavailable.',
          ));
    } finally {
      encoded.fillRange(0, encoded.length, 0);
    }
  });

  @override
  Future<Uint8List?> completeProtonHumanVerification(Uint8List handoff) =>
      _transport.invokePlatformMethod<Uint8List>(
        'completeProtonHumanVerification',
        {'handoff': handoff},
      );

  @override
  Future<void> cancelProtonHumanVerification() =>
      _transport.invokePlatformMethod<void>('cancelProtonHumanVerification');
  @override
  Future<ChainProfileResult> chainProfile(Map<String, Object?> request) =>
      _serialized(() async {
        final payload = ControlPayloadWriter()
          ..string(1, request['action'] as String? ?? 'list')
          ..string(2, request['source'] as String? ?? '')
          ..string(3, request['name'] as String? ?? '')
          ..string(4, request['profile_id'] as String? ?? '')
          ..string(5, request['revision'] as String? ?? '')
          ..string(6, request['configuration'] as String? ?? '')
          ..string(7, request['username'] as String? ?? '')
          ..string(8, request['password'] as String? ?? '')
          ..string(9, request['private_key_password'] as String? ?? '');
        if (request['proxy'] case final Map<String, Object?> proxy) {
          final nested = ControlPayloadWriter()
            ..string(1, proxy['host'] as String)
            ..unsigned(2, proxy['port'] as int)
            ..string(3, proxy['auth_mode'] as String)
            ..string(5, proxy['dns_transport'] as String? ?? 'auto');
          for (final server in proxy['dns_servers'] as List? ?? const []) {
            nested.string(4, server as String);
          }
          payload.message(10, nested.takeBytes());
        }
        return (await _request(47, payload.takeBytes())).chainProfiles ??
            (throw const EngineException(
              'CHAIN_PROFILE_UNAVAILABLE',
              'Chain profiles are unavailable.',
            ));
      });
  @override
  Future<List<ChainConfigurationFile>> pickChainConfigurations() =>
      pickChainConfigurationFiles();
  @override
  Future<VpnGateDirectory> listVpnGate({
    String? countryCode,
    bool unknownCountry = false,
    int offset = 0,
    int limit = 50,
    bool favoritesOnly = false,
    bool statusOnly = false,
  }) => _serialized(() async {
    final request = ControlPayloadWriter()
      ..string(1, countryCode ?? '')
      ..boolean(2, unknownCountry)
      ..unsigned(3, offset)
      ..unsigned(4, limit)
      ..boolean(6, favoritesOnly)
      ..boolean(7, statusOnly);
    final response = await _request(43, request.takeBytes());
    return response.vpnGateDirectory ??
        (throw const EngineException(
          'VPN_GATE_UNAVAILABLE',
          'The catalogue service is unavailable.',
        ));
  });
  @override
  Future<void> refreshVpnGate({bool cancel = false}) => _serialized(() async {
    await _request(
      44,
      (ControlPayloadWriter()..boolean(1, cancel)).takeBytes(),
    );
  });
  @override
  Future<void> vpnGateNode(VpnGateNodeRequest request) => _serialized(() async {
    await _request(
      45,
      (ControlPayloadWriter()
            ..string(1, request.operationId)
            ..string(2, request.action)
            ..string(3, request.serverId)
            ..string(4, request.configSha256)
            ..string(5, request.expectedFavoriteHash))
          .takeBytes(),
    );
  });
  @override
  Future<NetworkSettingsState> saveNetworkSettings(
    String operationId,
    String accountId,
    UsqueProfile values,
    List<String> changedFields,
  ) async {
    final payload = ControlPayloadWriter()
      ..string(1, operationId)
      ..string(2, accountId)
      ..message(3, _codec.encodeProfile(values));
    for (final field in changedFields) {
      payload.string(4, field);
    }
    return _requireSettings(await _request(41, payload.takeBytes()));
  }

  @override
  Future<NetworkSettingsState> getNetworkSettingsState() async =>
      _requireSettings(await _request(42, Uint8List(0)));

  NetworkSettingsState _requireSettings(ControlResponse response) =>
      response.networkSettings ??
      (throw const EngineException(
        'NETWORK_SETTINGS_UNSUPPORTED',
        'Restart or update the Engine to save network settings.',
      ));
  DesktopEngineClient()
    : _transport = DesktopEngineTransport(),
      _codec = const ControlCodec(),
      _requestTimeoutOverride = null;

  /// Test-only constructor with an injected transport (and optional codec).
  @visibleForTesting
  DesktopEngineClient.forTest({
    required this._transport,
    this._codec = const ControlCodec(),
    Duration Function(int payloadField)? requestTimeout,
  }) : _requestTimeoutOverride = requestTimeout;

  final DesktopEngineTransport _transport;
  final ControlCodec _codec;
  final Duration Function(int payloadField)? _requestTimeoutOverride;
  Future<void> _requestTail = Future<void>.value();
  int _connectionIntent = 0;
  bool _continuousConnection = false;

  @override
  bool get supportsSnapshotEvents => _transport.supportsSnapshotEvents;

  @override
  Stream<EngineSnapshotEvent> get snapshotEvents {
    return _transport.rawEventFrames.transform(
      StreamTransformer<Uint8List, EngineSnapshotEvent>.fromHandlers(
        handleData: (Uint8List value, EventSink<EngineSnapshotEvent> sink) {
          try {
            sink.add(_codec.decodeEvent(value));
          } on Object catch (error) {
            debugPrint('Usque: ignored invalid engine event frame ($error).');
          }
        },
      ),
    );
  }

  @override
  Future<ProfileCatalog> readStartupProfiles() async =>
      _codec.requireProfileCatalog(
        await _request(
          11,
          (ControlPayloadWriter()..boolean(1, true)).takeBytes(),
        ),
      );

  @override
  Future<ProfileCatalog> importLegacyProfiles(
    List<UsqueProfile> profiles,
    String activeProfileId,
  ) {
    return _serialized(() async {
      final payload = ControlPayloadWriter();
      for (final profile in profiles) {
        payload.message(1, _codec.encodeProfile(profile));
      }
      payload.string(2, activeProfileId);
      final response = await _request(25, payload.takeBytes());
      return _codec.requireProfileCatalog(response);
    });
  }

  @override
  Future<void> upsertProfile(UsqueProfile profile) =>
      _serialized(() => _upsertProfile(profile));

  @override
  Future<void> renameProfile(String profileId, String name) =>
      _serialized(() async {
        final capabilities = (await _request(24, Uint8List(0))).capabilities;
        if (!(capabilities?.accountMetadataMutations ?? false)) {
          throw const EngineException(
            'ACCOUNT_METADATA_UNSUPPORTED',
            'Update the Engine to rename an account safely.',
          );
        }
        await _request(
          46,
          (ControlPayloadWriter()
                ..string(1, profileId)
                ..string(2, name))
              .takeBytes(),
        );
      });

  @override
  Future<void> deleteProfile(String profileId) {
    return _serialized(() async {
      final payload = ControlPayloadWriter()..string(1, profileId);
      await _request(16, payload.takeBytes());
    });
  }

  @override
  Future<void> setActiveProfile(String profileId) {
    return _serialized(() async {
      final payload = ControlPayloadWriter()..string(1, profileId);
      await _request(17, payload.takeBytes());
    });
  }

  @override
  Future<void> provisionIdentity(
    UsqueProfile profile, {
    required IdentityProvisioningMethod method,
    String? licenseKey,
    String? teamName,
    String? callbackUri,
  }) {
    return _serialized(() async {
      final license = Uint8List.fromList(utf8.encode(licenseKey ?? ''));
      final callback = Uint8List.fromList(utf8.encode(callbackUri ?? ''));
      try {
        final payload = ControlPayloadWriter()
          ..string(1, profile.id)
          ..boolean(3, true)
          ..string(4, Platform.localeName)
          ..bytes(6, license)
          ..enumeration(7, _identityProvisioningWireValue(method));
        if (method == IdentityProvisioningMethod.zeroTrust) {
          final enrollment = ControlPayloadWriter()
            ..string(1, teamName ?? '')
            ..bytes(2, callback);
          payload.message(8, enrollment.takeBytes());
        }
        await _request(23, payload.takeBytes());
      } finally {
        license.fillRange(0, license.length, 0);
        callback.fillRange(0, callback.length, 0);
      }
    });
  }

  @override
  Future<ProfileCatalog> createProfileWithIdentity(
    UsqueProfile profile, {
    required IdentityProvisioningMethod method,
    String? licenseKey,
    String? teamName,
    String? callbackUri,
  }) {
    return _serialized(() async {
      final license = Uint8List.fromList(utf8.encode(licenseKey ?? ''));
      final callback = Uint8List.fromList(utf8.encode(callbackUri ?? ''));
      try {
        final identity = ControlPayloadWriter()
          ..enumeration(1, _identityProvisioningWireValue(method))
          ..boolean(3, true)
          ..string(4, Platform.localeName)
          ..bytes(6, license);
        if (method == IdentityProvisioningMethod.zeroTrust) {
          final enrollment = ControlPayloadWriter()
            ..string(1, teamName ?? '')
            ..bytes(2, callback);
          identity.message(7, enrollment.takeBytes());
        }
        final payload = ControlPayloadWriter()
          ..message(1, _codec.encodeProfile(profile))
          ..message(2, identity.takeBytes());
        final response = await _request(26, payload.takeBytes());
        return _codec.requireProfileCatalog(response);
      } finally {
        license.fillRange(0, license.length, 0);
        callback.fillRange(0, callback.length, 0);
      }
    });
  }

  @override
  Future<void> reconfigureActiveProfile(UsqueProfile profile) {
    return _serialized(() async {
      final payload = ControlPayloadWriter()
        ..message(1, _codec.encodeProfile(profile));
      await _request(27, payload.takeBytes());
    });
  }

  @override
  Future<void> updateProxyAuth(
    String profileId, {
    required String username,
    required String password,
    bool confirmed = true,
  }) {
    return _serialized(() async {
      final capabilities = (await _request(24, Uint8List(0))).capabilities;
      if (capabilities?.sharedProxyAuthApplication != true) {
        throw const EngineException(
          'PROXY_AUTH_UNSUPPORTED',
          'Update the Engine before saving shared credentials.',
        );
      }
      final secret = Uint8List.fromList(utf8.encode(password));
      try {
        final payload = ControlPayloadWriter()
          ..string(1, profileId)
          ..string(2, username)
          ..bytes(3, secret)
          ..boolean(4, confirmed);
        await _request(32, payload.takeBytes());
      } finally {
        secret.fillRange(0, secret.length, 0);
      }
    });
  }

  @override
  Future<void> copyLicenseKey(String profileId) {
    return _serialized(() async {
      final payload = ControlPayloadWriter()..string(1, profileId);
      final response = await _request(28, payload.takeBytes());
      final sensitive = response.sensitiveClipboard;
      if (Platform.isLinux && sensitive == null) {
        throw const EngineException(
          'ENGINE_IPC_INVALID_RESPONSE',
          'The Engine returned no clipboard value.',
        );
      }
      if (sensitive != null) {
        try {
          await _transport.invokePlatformMethod<void>('copySensitiveText', {
            'value': sensitive.utf8,
            'clear_after_seconds': sensitive.clearAfterSeconds,
          });
        } finally {
          sensitive.utf8.fillRange(0, sensitive.utf8.length, 0);
        }
      }
    });
  }

  @override
  Future<void> updateLicenseKey(String profileId, String licenseKey) {
    return _serialized(() async {
      final license = Uint8List.fromList(utf8.encode(licenseKey));
      try {
        final payload = ControlPayloadWriter()
          ..string(1, profileId)
          ..bytes(2, license);
        await _request(29, payload.takeBytes());
      } finally {
        license.fillRange(0, license.length, 0);
      }
    });
  }

  @override
  Future<void> unbindLicenseKey(String profileId) {
    return _serialized(() async {
      final payload = ControlPayloadWriter()..string(1, profileId);
      await _request(30, payload.takeBytes());
    });
  }

  @override
  Future<String?> exportWarpSecret(String profileId) async {
    final destination = await _transport.selectWarpSecretDestination();
    if (destination == null || destination.isEmpty) return null;
    final payload = ControlPayloadWriter()
      ..string(1, profileId)
      ..string(2, destination)
      ..boolean(3, true);
    await _serialized(() => _request(31, payload.takeBytes()));
    return destination;
  }

  @override
  Future<String?> consumeLaunchTarget() async => null;

  @override
  Future<String?> beginZeroTrustLogin(String teamName) async {
    if (!(Platform.isWindows || Platform.isLinux)) return null;
    return _transport.invokePlatformMethod<String>(
      'beginZeroTrustLogin',
      <String, Object>{'team_name': teamName},
    );
  }

  @override
  Future<String?> consumeZeroTrustCallback() async {
    if (!(Platform.isWindows || Platform.isLinux)) return null;
    return _transport.invokePlatformMethod<String>('consumeZeroTrustCallback');
  }

  @override
  Future<void> cancelZeroTrustLogin() async {
    if (!(Platform.isWindows || Platform.isLinux)) return;
    await _transport.invokePlatformMethod<void>('cancelZeroTrustLogin');
  }

  @override
  Future<PlatformPreferences> platformPreferences() async {
    final value = await _transport.invokePlatformMethod<Map<Object?, Object?>>(
      'platformPreferences',
    );
    return PlatformPreferences.fromMap(value ?? const <Object?, Object?>{});
  }

  @override
  Future<void> setStartOnBoot(bool enabled) =>
      _transport.invokePlatformMethod<void>('setStartOnBoot', <String, Object?>{
        'enabled': enabled,
      });

  @override
  Future<void> setCloseToTray(bool enabled) =>
      _transport.invokePlatformMethod<void>('setCloseToTray', <String, Object?>{
        'enabled': enabled,
      });

  @override
  Future<void> requestAddQuickSettingsTile() async {}

  @override
  Future<PerAppProxySettings> perAppProxy() async =>
      const PerAppProxySettings();

  @override
  Future<PerAppProxySettings> setPerAppProxy(
    PerAppProxySettings settings,
  ) async => const PerAppProxySettings();

  @override
  Future<List<InstalledAppInfo>> listInstalledApps() async =>
      const <InstalledAppInfo>[];

  @override
  Future<Uint8List?> getAppIcon(String packageName) async => null;

  @override
  Future<void> openAlwaysOnVpnSettings() async {}

  @override
  Future<EngineSnapshot> connect(UsqueProfile profile) {
    final intent = ++_connectionIntent;
    return _serialized(() async {
      final payload = ControlPayloadWriter()..string(1, profile.id);
      if (_continuousConnection) payload.boolean(2, true);
      final response = await _connectionRequest(
        12,
        payload.takeBytes(),
        connectionIntent: intent,
      );
      return response.snapshot ?? const EngineSnapshot();
    });
  }

  @override
  Future<EngineSnapshot> disconnect() async {
    _connectionIntent++;
    // Disconnect is a priority safety operation. Do not queue it behind
    // profile persistence, status reads, or other non-critical requests.
    final response = await _request(13, Uint8List(0));
    return response.snapshot ?? const EngineSnapshot();
  }

  @override
  Future<EngineSnapshot> retry() {
    final intent = ++_connectionIntent;
    return _serialized(() async {
      final payload = ControlPayloadWriter();
      if (_continuousConnection) payload.boolean(1, true);
      final response = await _connectionRequest(
        14,
        payload.takeBytes(),
        connectionIntent: intent,
      );
      return response.snapshot ?? const EngineSnapshot();
    });
  }

  @override
  Future<EngineSnapshot> snapshot() {
    return _serialized(() async {
      final response = await _request(10, Uint8List(0));
      return response.snapshot ?? const EngineSnapshot();
    });
  }

  @override
  Future<NetworkQualitySnapshot?> getNetworkQuality() {
    return _serialized(() async {
      final response = await _request(40, Uint8List(0));
      return response.networkQuality;
    });
  }

  @override
  Future<EngineCapabilities?> getCapabilities() {
    return _serialized(() async {
      final response = await _request(24, Uint8List(0));
      return response.capabilities;
    });
  }

  @override
  Future<DiagnosticSession> startDiagnostics(DiagnosticMode mode) {
    return _serialized(() async {
      final payload = ControlPayloadWriter()
        ..enumeration(1, mode == DiagnosticMode.standard ? 1 : 2);
      final response = await _request(36, payload.takeBytes());
      final session = response.diagnosticSession;
      if (session == null || session.sessionId.isEmpty) {
        throw const EngineException(
          'ENGINE_IPC_INVALID_RESPONSE',
          'The local Engine returned no diagnostic session.',
        );
      }
      return session;
    });
  }

  @override
  Future<DiagnosticSession> cancelDiagnostics(String sessionId) {
    return _serialized(() async {
      final payload = ControlPayloadWriter()..string(1, sessionId);
      final response = await _request(37, payload.takeBytes());
      final session = response.diagnosticSession;
      if (session == null || session.sessionId.isEmpty) {
        throw const EngineException(
          'ENGINE_IPC_INVALID_RESPONSE',
          'The local Engine returned no diagnostic session.',
        );
      }
      return session;
    });
  }

  @override
  Future<DiagnosticSession?> getDiagnostics() {
    return _serialized(() async {
      final response = await _request(38, Uint8List(0));
      final session = response.diagnosticSession;
      return session == null || session.sessionId.isEmpty ? null : session;
    });
  }

  @override
  Future<ConnectionTimeline> getConnectionTimeline() {
    return _serialized(() async {
      final response = await _request(39, Uint8List(0));
      return response.connectionTimeline ?? const ConnectionTimeline();
    });
  }

  @override
  Future<String?> exportDiagnostics({String? diagnosticSessionId}) async {
    final destination = await _transport.selectDiagnosticsDestination();
    if (destination == null || destination.isEmpty) {
      return null;
    }
    final payload = ControlPayloadWriter()
      ..string(1, destination)
      ..string(2, diagnosticSessionId ?? '');
    await _serialized(() => _request(21, payload.takeBytes()));
    return destination;
  }

  @override
  Future<UpdateCheckResult> checkForUpdates({bool manual = true}) async {
    // Update discovery is read-only and uses its own framed exchange. Keeping it
    // outside the mutation queue prevents a slow GitHub request from delaying
    // startup connection, while the transport still enforces its request bound.
    final payload = ControlPayloadWriter()..boolean(1, manual);
    final response = await _request(20, payload.takeBytes());
    return response.update ?? const UpdateCheckResult.current();
  }

  @override
  Future<String> getUpdateCacheDirectory() async {
    if (Platform.isLinux) {
      final configured = Platform.environment['XDG_CACHE_HOME'];
      if (configured != null && configured.startsWith('/')) {
        return '$configured/usque/updates';
      }
      final home = Platform.environment['HOME'];
      if (home != null && home.startsWith('/')) {
        return '$home/.cache/usque/updates';
      }
      throw const EngineException(
        'UPDATE_STORAGE_UNAVAILABLE',
        'HOME is unavailable.',
      );
    }
    if (!Platform.isWindows) {
      throw const EngineException(
        'UPDATE_PLATFORM_UNSUPPORTED',
        'In-app package installation is available only on Windows and Android.',
      );
    }
    final localAppData = Platform.environment['LOCALAPPDATA'];
    if (localAppData == null || localAppData.isEmpty) {
      throw const EngineException(
        'UPDATE_STORAGE_UNAVAILABLE',
        'Windows did not provide the current user application-data directory.',
      );
    }
    return '$localAppData${Platform.pathSeparator}Usque'
        '${Platform.pathSeparator}updates';
  }

  @override
  Future<void> verifyUpdatePackage({
    required String path,
    required String version,
    required UpdatePackage package,
  }) async {
    if (Platform.isLinux) {
      await _runLinuxUpdate(path, package, install: false);
      return;
    }
    await _runUpdateHelper(<String>[
      '--verify-only',
      ..._updatePackageArguments(path, version, package),
    ], failureCode: 'UPDATE_PACKAGE_INVALID');
  }

  @override
  Future<void> installUpdatePackage({
    required String path,
    required String version,
    required UpdatePackage package,
  }) async {
    if (Platform.isLinux) {
      await _runLinuxUpdate(path, package, install: true);
      await _transport.invokePlatformMethod<void>('exitApplication');
      return;
    }
    await _runUpdateHelper(<String>[
      '--install',
      '--parent-pid',
      '$pid',
      ..._updatePackageArguments(path, version, package),
    ], failureCode: 'UPDATE_INSTALL_LAUNCH_FAILED');
    await _transport.invokePlatformMethod<void>('exitApplication');
  }

  @override
  Future<GeoRulesList> listGeoRules() {
    return _serialized(() async {
      final response = await _request(33, Uint8List(0));
      return response.geoRulesList ?? const GeoRulesList();
    });
  }

  List<String> _updatePackageArguments(
    String path,
    String version,
    UpdatePackage package,
  ) => <String>[
    '--package',
    path,
    '--version',
    version,
    '--expected-name',
    package.name,
    '--expected-size',
    '${package.size}',
    '--expected-sha256',
    package.sha256,
    '--variant',
    package.variant,
  ];

  Future<void> _runLinuxUpdate(
    String path,
    UpdatePackage package, {
    required bool install,
  }) async {
    if (package.platform != 'linux' ||
        !const {'AppImage', 'deb', 'rpm'}.contains(package.installFormat) ||
        package.signature.length != 64 ||
        package.signatureManifest.isEmpty ||
        package.signatureManifest.length > 64 * 1024) {
      throw const EngineException(
        'UPDATE_PACKAGE_INVALID',
        'Signed Linux update metadata is missing.',
      );
    }
    final helper = File(
      '${File(Platform.resolvedExecutable).parent.path}/usque-update',
    );
    if (!await helper.exists()) {
      throw const EngineException(
        'UPDATE_HELPER_UNAVAILABLE',
        'The Linux update helper is missing.',
      );
    }
    // Metadata is not trusted here. The maintenance helper independently checks
    // the fixed public key, package digest, architecture, format and rollback.
    final cache = await Directory(
      await getUpdateCacheDirectory(),
    ).create(recursive: true);
    final temporary = await cache.createTemp('verification-');
    try {
      final manifest = File('${temporary.path}/manifest.json');
      final signature = File('${temporary.path}/manifest.sig');
      await manifest.writeAsBytes(package.signatureManifest, flush: true);
      await signature.writeAsBytes(package.signature, flush: true);
      final arguments = <String>[
        install ? '--install' : '--verify-only',
        '--package',
        path,
        '--manifest',
        manifest.path,
        '--signature',
        signature.path,
      ];
      if (install && package.installFormat == 'AppImage') {
        final appimage = Platform.environment['APPIMAGE'];
        if (appimage == null || !appimage.startsWith('/')) {
          throw const EngineException(
            'UPDATE_STORAGE_UNAVAILABLE',
            'The original AppImage path is unavailable.',
          );
        }
        arguments.addAll(['--appimage', appimage]);
      }
      final result = await Process.run(
        helper.path,
        arguments,
        runInShell: false,
      );
      if (result.exitCode != 0) {
        throw EngineException(
          install ? 'UPDATE_INSTALL_LAUNCH_FAILED' : 'UPDATE_PACKAGE_INVALID',
          'The Linux update helper did not complete (exit ${result.exitCode}). Retry after resolving the installation or authorization issue.',
        );
      }
    } finally {
      await temporary.delete(recursive: true);
    }
  }

  Future<void> _runUpdateHelper(
    List<String> arguments, {
    required String failureCode,
  }) async {
    if (!Platform.isWindows) {
      throw const EngineException(
        'UPDATE_PLATFORM_UNSUPPORTED',
        'In-app package installation is available only on Windows and Android.',
      );
    }
    final helper = File(
      '${File(Platform.resolvedExecutable).parent.path}'
      '${Platform.pathSeparator}usque-update.exe',
    );
    if (!helper.isAbsolute || !await helper.exists()) {
      throw const EngineException(
        'UPDATE_HELPER_UNAVAILABLE',
        'The signed Windows update helper is missing from this installation.',
      );
    }
    final result = await Process.run(helper.path, arguments, runInShell: false);
    if (result.exitCode == 0) return;
    final detail = '${result.stderr}'.trim();
    throw EngineException(
      failureCode,
      detail.isEmpty
          ? 'The Windows update helper rejected the operation.'
          : (detail.length <= 512 ? detail : detail.substring(0, 512)),
    );
  }

  @override
  Future<List<GeoRulesUpdateResult>> downloadGeoRules(String countryCode) {
    return _serialized(() async {
      final payload = ControlPayloadWriter()..string(1, countryCode);
      final response = await _request(34, payload.takeBytes());
      return response.geoRulesUpdate ?? const <GeoRulesUpdateResult>[];
    });
  }

  @override
  Future<List<GeoRulesUpdateResult>> updateAdsRules() {
    return _serialized(() async {
      final response = await _request(51, Uint8List(0));
      return response.geoRulesUpdate ?? const <GeoRulesUpdateResult>[];
    });
  }

  @override
  Future<List<GeoRulesUpdateResult>> updateAllGeoRules() {
    return _serialized(() async {
      final response = await _request(35, Uint8List(0));
      return response.geoRulesUpdate ?? const <GeoRulesUpdateResult>[];
    });
  }

  @override
  Future<void> clearAllData({required bool confirmed}) {
    return _serialized(() async {
      final payload = ControlPayloadWriter()..boolean(1, confirmed);
      await _request(22, payload.takeBytes());
      await _transport.resetEventStream();
    });
  }

  Future<void> _upsertProfile(UsqueProfile profile) async {
    final request = ControlPayloadWriter()
      ..message(1, _codec.encodeProfile(profile));
    await _request(15, request.takeBytes());
  }

  Future<ControlResponse> _connectionRequest(
    int payloadField,
    Uint8List payload, {
    required int connectionIntent,
  }) async {
    try {
      return await _request(
        payloadField,
        payload,
        connectionIntent: connectionIntent,
      );
    } on EngineException catch (error) {
      if (!_continuousConnection || error.code != 'ENGINE_REQUEST_TIMEOUT') {
        rethrow;
      }
      final observed = await _request(
        10,
        Uint8List(0),
        connectionIntent: connectionIntent,
      );
      final snapshot = observed.snapshot;
      if (snapshot != null &&
          (snapshot.isTransitional || snapshot.isConnected)) {
        return observed;
      }
      rethrow;
    }
  }

  Future<ControlResponse> _request(
    int payloadField,
    Uint8List payload, {
    int? connectionIntent,
  }) async {
    _checkConnectionIntent(connectionIntent);
    if (_transport.isDisposed) {
      throw const EngineException(
        'ENGINE_CLOSED',
        'The Usque Engine client has already closed.',
      );
    }
    await _transport.ensureStarted();
    // Dispose may race startup; refuse work that would talk to a closed client.
    if (_transport.isDisposed) {
      throw const EngineException(
        'ENGINE_CLOSED',
        'The Usque Engine client has already closed.',
      );
    }
    final requestId = _transport.allocateRequestId();
    final frame = _codec.buildRequestFrame(
      requestId: requestId,
      payloadField: payloadField,
      payload: payload,
    );

    Object? lastError;
    for (var attempt = 0; attempt < 20; attempt++) {
      _checkConnectionIntent(connectionIntent);
      Uint8List responseFrame;
      try {
        responseFrame = await _transport
            .exchangeFrame(frame, timeout: _timeoutFor(payloadField))
            .timeout(_timeoutFor(payloadField));
      } on TimeoutException {
        // Once a frame has reached the Engine it may still be executing.
        // Retrying a mutating request could start a second registration or
        // connection, so timeouts are never replayed.
        throw const EngineException(
          'ENGINE_REQUEST_TIMEOUT',
          'The Usque Engine did not finish the operation before its safety deadline.',
        );
      } on Object catch (error) {
        if (error is EngineException &&
            error.code == 'ENGINE_REQUEST_TIMEOUT') {
          rethrow;
        }
        lastError = error;
        // Production: stop retrying once the sidecar process handle is gone.
        // Test transports have no live process, so errors surface immediately.
        const safeReads = {10, 11, 24, 33, 38, 39, 40, 42, 43};
        if (!safeReads.contains(payloadField) || !_transport.hasLiveProcess) {
          break;
        }
        await Future<void>.delayed(const Duration(milliseconds: 50));
        continue;
      }
      // A valid response is authoritative. In particular, structured engine
      // errors must reach the UI unchanged instead of being retried and later
      // mislabeled as an IPC outage.
      try {
        final response = _codec.decodeResponse(responseFrame, requestId);
        if (response.capabilities != null) {
          _continuousConnection = response.capabilities!.continuousConnection;
        }
        return response;
      } on FormatException catch (error) {
        throw EngineException(
          'ENGINE_IPC_INVALID_RESPONSE',
          'The local Engine response could not be decoded: ${error.message}',
        );
      } finally {
        if (payloadField == 28) {
          responseFrame.fillRange(0, responseFrame.length, 0);
        }
      }
    }
    throw EngineException(
      'ENGINE_IPC_UNAVAILABLE',
      'Could not connect to the local Usque Engine: $lastError',
    );
  }

  Duration _timeoutFor(int payloadField) {
    final override = _requestTimeoutOverride;
    if (override != null) {
      return override(payloadField);
    }
    if (_continuousConnection && (payloadField == 12 || payloadField == 14)) {
      return const Duration(seconds: 5);
    }
    return Platform.isLinux
        ? linuxRequestTimeoutForPayload(payloadField)
        : requestTimeoutForPayload(payloadField);
  }

  void _checkConnectionIntent(int? expected) {
    if (expected != null && expected != _connectionIntent) {
      throw const EngineException(
        'ENGINE_REQUEST_CANCELLED',
        'The connection request was cancelled.',
      );
    }
  }

  Future<T> _serialized<T>(Future<T> Function() operation) {
    final completer = Completer<T>();
    _requestTail = _requestTail.then((_) async {
      try {
        completer.complete(await operation());
      } on Object catch (error, stackTrace) {
        completer.completeError(error, stackTrace);
      }
    });
    return completer.future;
  }

  @override
  void dispose() {
    _transport.dispose();
  }
}

/// Production request deadlines by control payload field number.
@visibleForTesting
Duration linuxRequestTimeoutForPayload(int payloadField) {
  final base = requestTimeoutForPayload(payloadField);
  // A Linux Secret Service prompt is bounded at 120s. Account mutations and
  // first-time encrypted chain storage can read, create and verify a key.
  // Preserve the existing network budget in addition to those prompt waits.
  const keyringOperations = {
    12,
    13,
    14,
    15,
    16,
    17,
    18,
    22,
    23,
    25,
    26,
    27,
    28,
    29,
    30,
    31,
    32,
    41,
    47,
    48,
    49,
  };
  if (keyringOperations.contains(payloadField)) {
    return base + const Duration(seconds: 360);
  }
  // Native Agent availability has its own five-second bound.
  if (payloadField == 24) return const Duration(seconds: 10);
  return base;
}

@visibleForTesting
Duration requestTimeoutForPayload(int payloadField) {
  switch (payloadField) {
    case 26:
    case 49:
      return identityRegistrationTimeout;
    case 50:
      return const Duration(seconds: 5);
    case 12:
    case 14:
      // Covers the maximum automatic endpoint cycle, chain startup and native
      // reporting margin. Native code applies the profile's narrower budget.
      return const Duration(seconds: 715);
    case 27:
      // A legacy cold reconfigure may connect the target and then roll back.
      return const Duration(seconds: 1415);
    case 23:
    case 29:
    case 30:
      // Replacing credentials can also revoke the retired identity before
      // returning. Keep that cleanup outside the registration HTTP budget.
      return identityRegistrationTimeout + const Duration(seconds: 60);
    case 20:
      return const Duration(seconds: 20);
    case 21:
      return const Duration(seconds: 15);
    case 22:
      return const Duration(seconds: 30);
    case 34:
      return const Duration(seconds: 90);
    case 35:
      return const Duration(seconds: 180);
    case 51:
      // Checksum and rules downloads each try up to three 30-second hosts;
      // allow another 15 seconds for validation, cache commit and IPC reporting.
      return const Duration(seconds: 195);
    default:
      return const Duration(seconds: 5);
  }
}

int _identityProvisioningWireValue(IdentityProvisioningMethod method) {
  return switch (method) {
    IdentityProvisioningMethod.register => 1,
    IdentityProvisioningMethod.registerWithLicense => 3,
    IdentityProvisioningMethod.zeroTrust => 4,
  };
}
