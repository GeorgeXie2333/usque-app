import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:flutter/foundation.dart';

import '../models/app_models.dart';
import 'control_codec.dart';
import 'desktop_engine_transport.dart';
import 'engine_client.dart';

export 'control_codec.dart'
    show
        debugDecodeEventSnapshot,
        debugDecodeProfileCatalogFrame,
        debugDecodeStatusFrame,
        debugEncodeGetStatusFrame;

/// Desktop [EngineClient] that coordinates request serialization, codec, and
/// transport. Public API, MethodChannel names, named pipes, and protobuf wire
/// data are unchanged from the pre-split client.
class DesktopEngineClient implements EngineClient {
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
      await _upsertProfile(profile);
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
      await _request(28, payload.takeBytes());
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
    if (!Platform.isWindows) return null;
    return _transport.invokePlatformMethod<String>(
      'beginZeroTrustLogin',
      <String, Object>{'team_name': teamName},
    );
  }

  @override
  Future<String?> consumeZeroTrustCallback() async {
    if (!Platform.isWindows) return null;
    return _transport.invokePlatformMethod<String>('consumeZeroTrustCallback');
  }

  @override
  Future<void> cancelZeroTrustLogin() async {
    if (!Platform.isWindows) return;
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
  Future<void> setWarpProtocolAssociation(bool enabled) async {
    if (!Platform.isWindows) return;
    await _transport.invokePlatformMethod<void>(
      'setWarpProtocolAssociation',
      <String, Object?>{'enabled': enabled},
    );
  }

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
    return _serialized(() async {
      await _upsertProfile(profile);
      final payload = ControlPayloadWriter()..string(1, profile.id);
      final response = await _request(12, payload.takeBytes());
      return response.snapshot ?? const EngineSnapshot();
    });
  }

  @override
  Future<EngineSnapshot> disconnect() async {
    // Disconnect is a priority safety operation. Do not queue it behind
    // profile persistence, status reads, or other non-critical requests.
    final response = await _request(13, Uint8List(0));
    return response.snapshot ?? const EngineSnapshot();
  }

  @override
  Future<EngineSnapshot> retry() {
    return _serialized(() async {
      final response = await _request(14, Uint8List(0));
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
  Future<String?> exportDiagnostics() async {
    final destination = await _transport.selectDiagnosticsDestination();
    if (destination == null || destination.isEmpty) {
      return null;
    }
    final payload = ControlPayloadWriter()..string(1, destination);
    await _serialized(() => _request(21, payload.takeBytes()));
    return destination;
  }

  @override
  Future<UpdateCheckResult> checkForUpdates({bool manual = true}) {
    return _serialized(() async {
      final payload = ControlPayloadWriter()..boolean(1, manual);
      final response = await _request(20, payload.takeBytes());
      return response.update ?? const UpdateCheckResult.current();
    });
  }

  @override
  Future<GeoRulesList> listGeoRules() {
    return _serialized(() async {
      final response = await _request(33, Uint8List(0));
      return response.geoRulesList ?? const GeoRulesList();
    });
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
    });
  }

  Future<void> _upsertProfile(UsqueProfile profile) async {
    final request = ControlPayloadWriter()
      ..message(1, _codec.encodeProfile(profile));
    await _request(15, request.takeBytes());
  }

  Future<ControlResponse> _request(int payloadField, Uint8List payload) async {
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
      Uint8List responseFrame;
      try {
        responseFrame = await _transport
            .exchangeFrame(frame)
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
        lastError = error;
        // Production: stop retrying once the sidecar process handle is gone.
        // Test transports have no live process, so errors surface immediately.
        if (!_transport.hasLiveProcess) {
          break;
        }
        await Future<void>.delayed(const Duration(milliseconds: 50));
        continue;
      }
      // A valid response is authoritative. In particular, structured engine
      // errors must reach the UI unchanged instead of being retried and later
      // mislabeled as an IPC outage.
      try {
        return _codec.decodeResponse(responseFrame, requestId);
      } on FormatException catch (error) {
        throw EngineException(
          'ENGINE_IPC_INVALID_RESPONSE',
          'The local Engine response could not be decoded: ${error.message}',
        );
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
    return requestTimeoutForPayload(payloadField);
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
Duration requestTimeoutForPayload(int payloadField) {
  switch (payloadField) {
    case 12:
    case 14:
      return const Duration(seconds: 55);
    case 23:
    case 26:
    case 29:
    case 30:
      return const Duration(seconds: 60);
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
