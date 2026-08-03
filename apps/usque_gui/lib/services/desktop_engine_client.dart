import 'dart:async';
import 'dart:convert';
import 'dart:io';
import 'dart:typed_data';

import 'package:flutter/services.dart';
import 'package:flutter/foundation.dart';

import '../models/app_models.dart';
import 'engine_client.dart';

const int _maximumFrameBytes = 4 * 1024 * 1024;
const String _pipePrefix =
    r'\\.\pipe\io.github.georgexie2333.usque.engine.v1-ui-';

class DesktopEngineClient implements EngineClient {
  static const MethodChannel _nativeTransport = MethodChannel(
    'io.github.georgexie2333.usque/engine',
  );
  static const EventChannel _nativeEvents = EventChannel(
    'io.github.georgexie2333.usque/engine_events',
  );

  Process? _process;
  Future<void>? _starting;
  Future<void> _requestTail = Future<void>.value();
  int _requestSequence = 0;
  bool _disposed = false;
  late final String _endpoint = Platform.isWindows
      ? '$_pipePrefix$pid-${DateTime.now().microsecondsSinceEpoch}'
      : _macSocketPath();

  @override
  bool get supportsSnapshotEvents => Platform.isWindows;

  @override
  Stream<EngineSnapshot> get snapshotEvents {
    if (!Platform.isWindows) {
      return const Stream<EngineSnapshot>.empty();
    }
    return _nativeEvents
        .receiveBroadcastStream(<String, Object>{
          'pipe_name': '$_endpoint.events',
        })
        .map<EngineSnapshot?>((Object? value) {
          if (value is! Uint8List) {
            throw const EngineException(
              'ENGINE_EVENT_INVALID',
              'The local Engine returned an invalid event frame.',
            );
          }
          return _decodeEventSnapshot(value);
        })
        .where((EngineSnapshot? snapshot) => snapshot != null)
        .cast<EngineSnapshot>();
  }

  @override
  Future<ProfileCatalog> importLegacyProfiles(
    List<UsqueProfile> profiles,
    String activeProfileId,
  ) {
    return _serialized(() async {
      final payload = _ProtoWriter();
      for (final profile in profiles) {
        payload.message(1, _encodeProfile(profile));
      }
      payload.string(2, activeProfileId);
      final response = await _request(25, payload.takeBytes());
      final catalog = response.profileCatalog;
      if (catalog == null) {
        throw const EngineException(
          'ENGINE_IPC_INVALID_RESPONSE',
          'The local Engine returned no profile catalog.',
        );
      }
      return catalog;
    });
  }

  @override
  Future<void> upsertProfile(UsqueProfile profile) =>
      _serialized(() => _upsertProfile(profile));

  @override
  Future<void> deleteProfile(String profileId) {
    return _serialized(() async {
      final payload = _ProtoWriter()..string(1, profileId);
      await _request(16, payload.takeBytes());
    });
  }

  @override
  Future<void> setActiveProfile(String profileId) {
    return _serialized(() async {
      final payload = _ProtoWriter()..string(1, profileId);
      await _request(17, payload.takeBytes());
    });
  }

  @override
  Future<void> provisionIdentity(UsqueProfile profile, {String? warpSecret}) {
    return _serialized(() async {
      await _upsertProfile(profile);
      final secret = Uint8List.fromList(utf8.encode(warpSecret ?? ''));
      try {
        final payload = _ProtoWriter()
          ..string(1, profile.id)
          ..bytes(2, secret)
          ..boolean(3, true)
          ..string(4, Platform.localeName);
        await _request(23, payload.takeBytes());
      } finally {
        secret.fillRange(0, secret.length, 0);
      }
    });
  }

  @override
  Future<ProfileCatalog> createProfileWithIdentity(
    UsqueProfile profile, {
    required IdentityProvisioningMethod method,
    String? warpSecret,
  }) {
    return _serialized(() async {
      final secret = Uint8List.fromList(utf8.encode(warpSecret ?? ''));
      try {
        final identity = _ProtoWriter()
          ..enumeration(1, method.index + 1)
          ..bytes(2, secret)
          ..boolean(3, true)
          ..string(4, Platform.localeName);
        final payload = _ProtoWriter()
          ..message(1, _encodeProfile(profile))
          ..message(2, identity.takeBytes());
        final response = await _request(26, payload.takeBytes());
        final catalog = response.profileCatalog;
        if (catalog == null) {
          throw const EngineException(
            'ENGINE_IPC_INVALID_RESPONSE',
            'The local Engine returned no profile catalog.',
          );
        }
        return catalog;
      } finally {
        secret.fillRange(0, secret.length, 0);
      }
    });
  }

  @override
  Future<EngineSnapshot> connect(UsqueProfile profile) {
    return _serialized(() async {
      await _upsertProfile(profile);
      final payload = _ProtoWriter()..string(1, profile.id);
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
  Future<EngineSnapshot> snapshot() {
    return _serialized(() async {
      final response = await _request(10, Uint8List(0));
      return response.snapshot ?? const EngineSnapshot();
    });
  }

  @override
  Future<EngineSnapshot> pauseCaptivePortal({int seconds = 600}) {
    return _serialized(() async {
      final payload = _ProtoWriter()..unsigned(1, seconds);
      final response = await _request(19, payload.takeBytes());
      return response.snapshot ?? const EngineSnapshot();
    });
  }

  @override
  Future<String?> exportDiagnostics() async {
    final destination = await _nativeTransport.invokeMethod<String>(
      'selectDiagnosticsDestination',
    );
    if (destination == null || destination.isEmpty) {
      return null;
    }
    final payload = _ProtoWriter()..string(1, destination);
    await _serialized(() => _request(21, payload.takeBytes()));
    return destination;
  }

  @override
  Future<UpdateCheckResult> checkForUpdates({bool manual = true}) {
    return _serialized(() async {
      final payload = _ProtoWriter()..boolean(1, manual);
      final response = await _request(20, payload.takeBytes());
      return response.update ?? const UpdateCheckResult.current();
    });
  }

  @override
  Future<void> clearAllData({required bool confirmed}) {
    return _serialized(() async {
      final payload = _ProtoWriter()..boolean(1, confirmed);
      await _request(22, payload.takeBytes());
    });
  }

  Future<void> _upsertProfile(UsqueProfile profile) async {
    final request = _ProtoWriter()..message(1, _encodeProfile(profile));
    await _request(15, request.takeBytes());
  }

  Future<_ControlResponse> _request(int payloadField, Uint8List payload) async {
    if (_disposed) {
      throw const EngineException(
        'ENGINE_CLOSED',
        'The Usque Engine client has already closed.',
      );
    }
    await _ensureStarted();
    final requestId = '$pid-${++_requestSequence}';
    final envelope = _ProtoWriter()
      ..string(1, requestId)
      ..message(payloadField, payload);
    final frame = _frame(envelope.takeBytes());

    Object? lastError;
    for (var attempt = 0; attempt < 20; attempt++) {
      Uint8List responseFrame;
      try {
        responseFrame = await _exchange(
          frame,
        ).timeout(_requestTimeout(payloadField));
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
        if (_process == null) {
          break;
        }
        await Future<void>.delayed(const Duration(milliseconds: 50));
        continue;
      }
      // A valid response is authoritative. In particular, structured engine
      // errors must reach the UI unchanged instead of being retried and later
      // mislabeled as an IPC outage.
      return _decodeResponse(responseFrame, requestId);
    }
    throw EngineException(
      'ENGINE_IPC_UNAVAILABLE',
      'Could not connect to the local Usque Engine: $lastError',
    );
  }

  Future<Uint8List> _exchange(Uint8List request) async {
    if (Platform.isWindows) {
      try {
        final response = await _nativeTransport.invokeMethod<Uint8List>(
          'exchangeFrame',
          <String, Object>{'pipe_name': _endpoint, 'request': request},
        );
        if (response == null) {
          throw const EngineException(
            'ENGINE_IPC_INVALID_RESPONSE',
            'The native Named Pipe bridge returned no response.',
          );
        }
        return response;
      } on PlatformException catch (error) {
        throw EngineException(
          error.code,
          error.message ?? 'The native Named Pipe bridge failed.',
        );
      }
    }

    final socket = await Socket.connect(
      InternetAddress(_endpoint, type: InternetAddressType.unix),
      0,
      timeout: const Duration(seconds: 1),
    );
    try {
      socket.add(request);
      await socket.flush();
      return await _readFrame(socket);
    } finally {
      await socket.close();
    }
  }

  Future<void> _ensureStarted() async {
    if (_process != null) {
      return;
    }
    final existing = _starting;
    if (existing != null) {
      return existing;
    }
    final start = _startEngine();
    _starting = start;
    try {
      await start;
    } finally {
      _starting = null;
    }
  }

  Future<void> _startEngine() async {
    final executable = _engineExecutable();
    if (!await File(executable).exists()) {
      throw EngineException(
        'ENGINE_BINARY_MISSING',
        'The Rust Engine sidecar is missing at $executable.',
      );
    }
    final configPath = _configPath();
    await Directory(File(configPath).parent.path).create(recursive: true);
    if (Platform.isMacOS) {
      await Directory(File(_endpoint).parent.path).create(recursive: true);
    }
    final arguments = <String>[
      '--config',
      configPath,
      if (Platform.isWindows) ...<String>[
        '--pipe',
        _endpoint,
        '--parent-pid',
        '$pid',
      ],
      if (Platform.isMacOS) ...<String>['--socket', _endpoint],
    ];
    final process = await Process.start(
      executable,
      arguments,
      mode: ProcessStartMode.normal,
    );
    _process = process;
    unawaited(process.stdout.drain<void>());
    unawaited(process.stderr.drain<void>());
    unawaited(
      process.exitCode.then((_) {
        if (identical(_process, process)) {
          _process = null;
        }
      }),
    );
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
    _disposed = true;
    final process = _process;
    _process = null;
    process?.kill();
  }
}

Duration _requestTimeout(int payloadField) {
  switch (payloadField) {
    case 12:
      return const Duration(seconds: 55);
    case 23:
    case 26:
      return const Duration(seconds: 60);
    case 20:
      return const Duration(seconds: 20);
    case 21:
      return const Duration(seconds: 15);
    case 22:
      return const Duration(seconds: 30);
    default:
      return const Duration(seconds: 5);
  }
}

String _engineExecutable() {
  final override = Platform.environment['USQUE_ENGINE_PATH'];
  if (kDebugMode && override != null && override.trim().isNotEmpty) {
    return override;
  }
  final appExecutable = File(Platform.resolvedExecutable);
  if (Platform.isWindows) {
    return '${appExecutable.parent.path}${Platform.pathSeparator}'
        'usque-engine.exe';
  }
  final contents = appExecutable.parent.parent;
  return '${contents.path}${Platform.pathSeparator}Resources'
      '${Platform.pathSeparator}usque-engine';
}

String _configPath() {
  if (Platform.isWindows) {
    final localAppData = Platform.environment['LOCALAPPDATA'];
    if (localAppData == null || localAppData.isEmpty) {
      throw const EngineException(
        'APP_DATA_UNAVAILABLE',
        'LOCALAPPDATA is unavailable.',
      );
    }
    return '$localAppData${Platform.pathSeparator}Usque'
        '${Platform.pathSeparator}config.json';
  }
  final home = Platform.environment['HOME'];
  if (home == null || home.isEmpty) {
    throw const EngineException('APP_DATA_UNAVAILABLE', 'HOME is unavailable.');
  }
  return '$home/Library/Application Support/Usque/config.json';
}

String _macSocketPath() {
  final home = Platform.environment['HOME'];
  if (home == null || home.isEmpty) {
    throw const EngineException('APP_DATA_UNAVAILABLE', 'HOME is unavailable.');
  }
  return '$home/Library/Caches/Usque/engine-$pid.sock';
}

Uint8List _encodeProfile(UsqueProfile profile) {
  final endpoint = _ProtoWriter()
    ..string(1, profile.endpointIpv4)
    ..string(2, profile.endpointIpv6)
    ..unsigned(3, profile.endpointPort)
    ..string(4, profile.sni);
  final proxy = _ProtoWriter()
    ..string(1, '${profile.proxy.socksIpv4}:${profile.proxy.socksPort}')
    ..string(1, '[${profile.proxy.socksIpv6}]:${profile.proxy.socksPort}')
    ..string(2, '${profile.proxy.httpIpv4}:${profile.proxy.httpPort}')
    ..string(2, '[${profile.proxy.httpIpv6}]:${profile.proxy.httpPort}')
    ..boolean(3, profile.proxy.systemProxy)
    ..unsigned(4, 60)
    ..enumeration(5, profile.proxy.dnsMode.index + 1);
  final writer = _ProtoWriter()
    ..string(1, profile.id)
    ..string(2, profile.name)
    ..enumeration(3, profile.mode.index + 1)
    ..enumeration(4, profile.transport.index + 1)
    ..message(5, endpoint.takeBytes())
    ..enumeration(6, profile.ipPolicy.index + 1)
    ..unsigned(7, profile.mtu)
    ..string(8, profile.dnsIpv4)
    ..string(8, profile.dnsIpv6)
    ..boolean(9, profile.allowLan);
  for (final cidr in profile.bypassCidrs) {
    writer.string(10, cidr);
  }
  writer
    ..boolean(11, profile.killSwitch)
    ..boolean(12, profile.autoConnect)
    ..message(13, proxy.takeBytes())
    ..enumeration(14, profile.dnsMode.index + 1);
  return writer.takeBytes();
}

Uint8List _frame(Uint8List payload) {
  if (payload.length > _maximumFrameBytes) {
    throw const EngineException(
      'ENGINE_IPC_FRAME_TOO_LARGE',
      'The local Engine request exceeded 4 MiB.',
    );
  }
  final output = Uint8List(payload.length + 4);
  ByteData.sublistView(output).setUint32(0, payload.length, Endian.big);
  output.setRange(4, output.length, payload);
  return output;
}

@visibleForTesting
Uint8List debugEncodeGetStatusFrame(String requestId) {
  final envelope = _ProtoWriter()
    ..string(1, requestId)
    ..message(10, Uint8List(0));
  return _frame(envelope.takeBytes());
}

@visibleForTesting
EngineSnapshot debugDecodeStatusFrame(Uint8List frame, String requestId) {
  return _decodeResponse(frame, requestId).snapshot ?? const EngineSnapshot();
}

@visibleForTesting
EngineSnapshot? debugDecodeEventSnapshot(Uint8List frame) {
  return _decodeEventSnapshot(frame);
}

@visibleForTesting
ProfileCatalog debugDecodeProfileCatalogFrame(
  Uint8List frame,
  String requestId,
) {
  final catalog = _decodeResponse(frame, requestId).profileCatalog;
  if (catalog == null) {
    throw const EngineException(
      'ENGINE_IPC_INVALID_RESPONSE',
      'The local Engine returned no profile catalog.',
    );
  }
  return catalog;
}

Future<Uint8List> _readFrame(Stream<List<int>> stream) async {
  final iterator = StreamIterator<List<int>>(stream);
  final buffer = BytesBuilder(copy: false);
  int? expected;
  while (await iterator.moveNext()) {
    buffer.add(iterator.current);
    final bytes = buffer.toBytes();
    if (expected == null && bytes.length >= 4) {
      expected = ByteData.sublistView(bytes).getUint32(0, Endian.big);
      if (expected > _maximumFrameBytes) {
        throw const EngineException(
          'ENGINE_IPC_FRAME_TOO_LARGE',
          'The local Engine response exceeded 4 MiB.',
        );
      }
    }
    if (expected != null && bytes.length >= expected + 4) {
      if (bytes.length != expected + 4) {
        throw const EngineException(
          'ENGINE_IPC_INVALID_RESPONSE',
          'The local Engine returned trailing frame bytes.',
        );
      }
      return bytes;
    }
  }
  throw const EngineException(
    'ENGINE_IPC_TRUNCATED',
    'The local Engine closed an incomplete response.',
  );
}

_ControlResponse _decodeResponse(Uint8List frame, String expectedRequestId) {
  if (frame.length < 4) {
    throw const EngineException(
      'ENGINE_IPC_TRUNCATED',
      'The local Engine response header was truncated.',
    );
  }
  final length = ByteData.sublistView(frame).getUint32(0, Endian.big);
  if (length > _maximumFrameBytes || length != frame.length - 4) {
    throw const EngineException(
      'ENGINE_IPC_INVALID_RESPONSE',
      'The local Engine response length was invalid.',
    );
  }
  final reader = _ProtoReader(Uint8List.sublistView(frame, 4));
  String? requestId;
  _StructuredEngineError? error;
  EngineSnapshot? snapshot;
  UpdateCheckResult? update;
  ProfileCatalog? profileCatalog;
  while (!reader.isDone) {
    final field = reader.field();
    switch (field.number) {
      case 1:
        requestId = reader.string(field);
      case 2:
        error = _decodeError(reader.message(field));
      case 11:
        snapshot = _decodeSnapshot(reader.message(field));
      case 14:
        update = _decodeUpdate(reader.message(field));
      case 12:
        profileCatalog = _decodeProfileCatalog(reader.message(field));
      default:
        reader.skip(field);
    }
  }
  if (requestId != expectedRequestId) {
    throw const EngineException(
      'ENGINE_IPC_REQUEST_MISMATCH',
      'The local Engine response did not match its request.',
    );
  }
  if (error != null) {
    throw EngineException(error.code, error.message);
  }
  return _ControlResponse(snapshot, update, profileCatalog);
}

EngineSnapshot? _decodeEventSnapshot(Uint8List frame) {
  if (frame.length < 4) {
    throw const EngineException(
      'ENGINE_EVENT_TRUNCATED',
      'The local Engine event header was truncated.',
    );
  }
  final length = ByteData.sublistView(frame).getUint32(0, Endian.big);
  if (length > _maximumFrameBytes || length != frame.length - 4) {
    throw const EngineException(
      'ENGINE_EVENT_INVALID',
      'The local Engine event length was invalid.',
    );
  }

  final envelope = _ProtoReader(Uint8List.sublistView(frame, 4));
  EngineSnapshot? snapshot;
  while (!envelope.isDone) {
    final field = envelope.field();
    switch (field.number) {
      case 1:
        envelope.varint(field);
      case 10:
        final stateChanged = envelope.message(field);
        while (!stateChanged.isDone) {
          final stateField = stateChanged.field();
          if (stateField.number == 1) {
            snapshot = _decodeSnapshot(stateChanged.message(stateField));
          } else {
            stateChanged.skip(stateField);
          }
        }
      default:
        envelope.skip(field);
    }
  }
  return snapshot;
}

ProfileCatalog _decodeProfileCatalog(_ProtoReader reader) {
  final profiles = <UsqueProfile>[];
  final identityStates = <String, ProfileIdentityState>{};
  String? activeProfileId;
  while (!reader.isDone) {
    final field = reader.field();
    switch (field.number) {
      case 1:
        profiles.add(_decodeProfile(reader.message(field)));
      case 2:
        activeProfileId = _emptyToNull(reader.string(field));
      case 3:
        final status = reader.message(field);
        String? profileId;
        ProfileIdentityState? state;
        while (!status.isDone) {
          final statusField = status.field();
          switch (statusField.number) {
            case 1:
              profileId = _emptyToNull(status.string(statusField));
            case 2:
              final value = status.varint(statusField);
              if (value >= 1 && value <= ProfileIdentityState.values.length) {
                state = ProfileIdentityState.values[value - 1];
              }
            default:
              status.skip(statusField);
          }
        }
        if (profileId != null && state != null) {
          identityStates[profileId] = state;
        }
      default:
        reader.skip(field);
    }
  }
  if (profiles.isEmpty ||
      activeProfileId == null ||
      !profiles.any((profile) => profile.id == activeProfileId)) {
    throw const EngineException(
      'ENGINE_IPC_INVALID_RESPONSE',
      'The local Engine returned an invalid profile catalog.',
    );
  }
  return ProfileCatalog(
    profiles: List<UsqueProfile>.unmodifiable(profiles),
    activeProfileId: activeProfileId,
    identityStates: Map<String, ProfileIdentityState>.unmodifiable(
      identityStates,
    ),
  );
}

UsqueProfile _decodeProfile(_ProtoReader reader) {
  final defaults = UsqueProfile.defaultProfile();
  var id = defaults.id;
  var name = defaults.name;
  var mode = defaults.mode;
  var transport = defaults.transport;
  var ipPolicy = defaults.ipPolicy;
  var endpointIpv4 = defaults.endpointIpv4;
  var endpointIpv6 = defaults.endpointIpv6;
  var endpointPort = defaults.endpointPort;
  var sni = defaults.sni;
  var mtu = defaults.mtu;
  final dnsServers = <String>[];
  var allowLan = defaults.allowLan;
  final bypassCidrs = <String>[];
  var killSwitch = defaults.killSwitch;
  var autoConnect = defaults.autoConnect;
  var dnsMode = defaults.dnsMode;
  var proxy = defaults.proxy;

  while (!reader.isDone) {
    final field = reader.field();
    switch (field.number) {
      case 1:
        id = reader.string(field);
      case 2:
        name = reader.string(field);
      case 3:
        mode = _decodeIndexedEnum(
          OperatingMode.values,
          reader.varint(field),
          'operating mode',
        );
      case 4:
        transport = _decodeIndexedEnum(
          TransportPolicy.values,
          reader.varint(field),
          'transport policy',
        );
      case 5:
        final endpoint = reader.message(field);
        while (!endpoint.isDone) {
          final endpointField = endpoint.field();
          switch (endpointField.number) {
            case 1:
              endpointIpv4 = endpoint.string(endpointField);
            case 2:
              endpointIpv6 = endpoint.string(endpointField);
            case 3:
              endpointPort = endpoint.varint(endpointField);
            case 4:
              sni = endpoint.string(endpointField);
            default:
              endpoint.skip(endpointField);
          }
        }
      case 6:
        ipPolicy = _decodeIndexedEnum(
          IpPolicy.values,
          reader.varint(field),
          'IP policy',
        );
      case 7:
        mtu = reader.varint(field);
      case 8:
        dnsServers.add(reader.string(field));
      case 9:
        allowLan = reader.varint(field) != 0;
      case 10:
        bypassCidrs.add(reader.string(field));
      case 11:
        killSwitch = reader.varint(field) != 0;
      case 12:
        autoConnect = reader.varint(field) != 0;
      case 13:
        proxy = _decodeProxySettings(reader.message(field), proxy);
      case 14:
        dnsMode = _decodeIndexedEnum(
          DnsMode.values,
          reader.varint(field),
          'DNS mode',
        );
      default:
        reader.skip(field);
    }
  }
  return UsqueProfile(
    id: id,
    name: name,
    mode: mode,
    transport: transport,
    ipPolicy: ipPolicy,
    endpointIpv4: endpointIpv4,
    endpointIpv6: endpointIpv6,
    endpointPort: endpointPort,
    sni: sni,
    mtu: mtu,
    dnsIpv4:
        dnsServers.where((value) => value.contains('.')).firstOrNull ??
        defaults.dnsIpv4,
    dnsIpv6:
        dnsServers.where((value) => value.contains(':')).firstOrNull ??
        defaults.dnsIpv6,
    dnsMode: dnsMode,
    killSwitch: killSwitch,
    allowLan: allowLan,
    autoConnect: autoConnect,
    bypassCidrs: List<String>.unmodifiable(bypassCidrs),
    proxy: proxy,
  );
}

ProxySettings _decodeProxySettings(
  _ProtoReader reader,
  ProxySettings defaults,
) {
  final socksListeners = <String>[];
  final httpListeners = <String>[];
  var systemProxy = defaults.systemProxy;
  var dnsMode = defaults.dnsMode;
  while (!reader.isDone) {
    final field = reader.field();
    switch (field.number) {
      case 1:
        socksListeners.add(reader.string(field));
      case 2:
        httpListeners.add(reader.string(field));
      case 3:
        systemProxy = reader.varint(field) != 0;
      case 5:
        dnsMode = _decodeIndexedEnum(
          ProxyDnsMode.values,
          reader.varint(field),
          'proxy DNS mode',
        );
      default:
        reader.skip(field);
    }
  }
  final socks = _decodeDualStackListeners(
    socksListeners,
    defaults.socksIpv4,
    defaults.socksIpv6,
    defaults.socksPort,
  );
  final http = _decodeDualStackListeners(
    httpListeners,
    defaults.httpIpv4,
    defaults.httpIpv6,
    defaults.httpPort,
  );
  return ProxySettings(
    socksIpv4: socks.ipv4,
    socksIpv6: socks.ipv6,
    socksPort: socks.port,
    httpIpv4: http.ipv4,
    httpIpv6: http.ipv6,
    httpPort: http.port,
    dnsMode: dnsMode,
    systemProxy: systemProxy,
  );
}

({String ipv4, String ipv6, int port}) _decodeDualStackListeners(
  List<String> listeners,
  String defaultIpv4,
  String defaultIpv6,
  int defaultPort,
) {
  var ipv4 = defaultIpv4;
  var ipv6 = defaultIpv6;
  var port = defaultPort;
  for (final listener in listeners) {
    final decoded = _splitSocketAddress(listener);
    port = decoded.port;
    if (decoded.host.contains(':')) {
      ipv6 = decoded.host;
    } else {
      ipv4 = decoded.host;
    }
  }
  return (ipv4: ipv4, ipv6: ipv6, port: port);
}

({String host, int port}) _splitSocketAddress(String value) {
  if (value.startsWith('[')) {
    final closing = value.indexOf(']');
    if (closing <= 1 || closing + 2 >= value.length) {
      throw const EngineException(
        'ENGINE_IPC_INVALID_RESPONSE',
        'The local Engine returned an invalid IPv6 listener.',
      );
    }
    return (
      host: value.substring(1, closing),
      port: int.parse(value.substring(closing + 2)),
    );
  }
  final separator = value.lastIndexOf(':');
  if (separator <= 0) {
    throw const EngineException(
      'ENGINE_IPC_INVALID_RESPONSE',
      'The local Engine returned an invalid listener.',
    );
  }
  return (
    host: value.substring(0, separator),
    port: int.parse(value.substring(separator + 1)),
  );
}

T _decodeIndexedEnum<T>(List<T> values, int wireValue, String label) {
  final index = wireValue - 1;
  if (index < 0 || index >= values.length) {
    throw EngineException(
      'ENGINE_IPC_INVALID_RESPONSE',
      'The local Engine returned an unknown $label.',
    );
  }
  return values[index];
}

UpdateCheckResult _decodeUpdate(_ProtoReader reader) {
  var available = false;
  String? version;
  String? releaseUrl;
  while (!reader.isDone) {
    final field = reader.field();
    switch (field.number) {
      case 1:
        available = reader.varint(field) != 0;
      case 2:
        version = _emptyToNull(reader.string(field));
      case 3:
        releaseUrl = _emptyToNull(reader.string(field));
      default:
        reader.skip(field);
    }
  }
  return UpdateCheckResult(
    available: available,
    version: version,
    releaseUrl: releaseUrl,
  );
}

_StructuredEngineError _decodeError(_ProtoReader reader) {
  var code = 'ENGINE_ERROR';
  var message = 'The local Engine rejected this operation.';
  while (!reader.isDone) {
    final field = reader.field();
    switch (field.number) {
      case 1:
        code = reader.string(field);
      case 2:
        message = reader.string(field);
      default:
        reader.skip(field);
    }
  }
  return _StructuredEngineError(code, message);
}

EngineSnapshot _decodeSnapshot(_ProtoReader reader) {
  var phase = ConnectionPhase.error;
  String? transport;
  String? family;
  var connectedSeconds = 0;
  var uploaded = 0;
  var downloaded = 0;
  var uploadRate = 0;
  var downloadRate = 0;
  ExitInfo exit = const ExitInfo();
  String? warning;
  var captivePauseRemainingSeconds = 0;
  while (!reader.isDone) {
    final field = reader.field();
    switch (field.number) {
      case 1:
        phase = _decodePhase(reader.varint(field));
      case 2:
        transport = _emptyToNull(reader.string(field));
      case 3:
        family = _emptyToNull(reader.string(field));
      case 6:
        final statistics = reader.message(field);
        while (!statistics.isDone) {
          final statistic = statistics.field();
          switch (statistic.number) {
            case 1:
              connectedSeconds = statistics.varint(statistic);
            case 2:
              uploaded = statistics.varint(statistic);
            case 3:
              downloaded = statistics.varint(statistic);
            case 4:
              uploadRate = statistics.varint(statistic);
            case 5:
              downloadRate = statistics.varint(statistic);
            default:
              statistics.skip(statistic);
          }
        }
      case 7:
        exit = _decodeExit(reader.message(field));
      case 8:
        warning = _decodeError(reader.message(field)).message;
      case 14:
        captivePauseRemainingSeconds = reader.varint(field);
      default:
        reader.skip(field);
    }
  }
  return EngineSnapshot(
    phase: phase,
    transport: transport,
    addressFamily: family,
    connectedAt: connectedSeconds == 0
        ? null
        : DateTime.now().subtract(Duration(seconds: connectedSeconds)),
    downloadBytesPerSecond: downloadRate,
    uploadBytesPerSecond: uploadRate,
    downloadedBytes: downloaded,
    uploadedBytes: uploaded,
    exit: exit,
    warning: warning,
    captivePauseRemainingSeconds: captivePauseRemainingSeconds,
  );
}

ExitInfo _decodeExit(_ProtoReader reader) {
  String? ipv4;
  String? ipv6;
  _Geo? ipv4Location;
  _Geo? ipv6Location;
  while (!reader.isDone) {
    final field = reader.field();
    switch (field.number) {
      case 1:
        ipv4 = _emptyToNull(reader.string(field));
      case 2:
        ipv6 = _emptyToNull(reader.string(field));
      case 3:
        ipv4Location = _decodeGeo(reader.message(field));
      case 4:
        ipv6Location = _decodeGeo(reader.message(field));
      default:
        reader.skip(field);
    }
  }
  final location = ipv4Location ?? ipv6Location;
  return ExitInfo(
    city: location?.city,
    country: location?.country,
    countryCode: location?.countryCode,
    flagSvg: location?.flagSvg,
    ipv4: ipv4,
    ipv6: ipv6,
  );
}

_Geo _decodeGeo(_ProtoReader reader) {
  String? countryCode;
  String? country;
  String? city;
  String? flagSvg;
  while (!reader.isDone) {
    final field = reader.field();
    switch (field.number) {
      case 2:
        countryCode = _emptyToNull(reader.string(field));
      case 3:
        country = _emptyToNull(reader.string(field));
      case 5:
        city = _emptyToNull(reader.string(field));
      case 7:
        flagSvg = _emptyToNull(reader.string(field));
      default:
        reader.skip(field);
    }
  }
  return _Geo(countryCode, country, city, flagSvg);
}

ConnectionPhase _decodePhase(int value) {
  return switch (value) {
    1 => ConnectionPhase.disconnected,
    2 => ConnectionPhase.preparing,
    3 => ConnectionPhase.connectingH3,
    4 => ConnectionPhase.connectingH2,
    5 => ConnectionPhase.connected,
    6 => ConnectionPhase.degraded,
    7 => ConnectionPhase.reconnecting,
    8 => ConnectionPhase.disconnecting,
    9 => ConnectionPhase.captivePortalPaused,
    10 => ConnectionPhase.error,
    _ => ConnectionPhase.error,
  };
}

String? _emptyToNull(String value) => value.isEmpty ? null : value;

class _ControlResponse {
  const _ControlResponse(this.snapshot, this.update, this.profileCatalog);

  final EngineSnapshot? snapshot;
  final UpdateCheckResult? update;
  final ProfileCatalog? profileCatalog;
}

class _StructuredEngineError {
  const _StructuredEngineError(this.code, this.message);

  final String code;
  final String message;
}

class _Geo {
  const _Geo(this.countryCode, this.country, this.city, this.flagSvg);

  final String? countryCode;
  final String? country;
  final String? city;
  final String? flagSvg;
}

class _ProtoWriter {
  final BytesBuilder _bytes = BytesBuilder(copy: false);

  void unsigned(int number, int value) {
    if (value == 0) {
      return;
    }
    _tag(number, 0);
    _varint(value);
  }

  void enumeration(int number, int value) => unsigned(number, value);

  void boolean(int number, bool value) {
    if (value) {
      unsigned(number, 1);
    }
  }

  void string(int number, String value) {
    if (value.isNotEmpty) {
      bytes(number, Uint8List.fromList(utf8.encode(value)));
    }
  }

  void message(int number, Uint8List value) {
    _tag(number, 2);
    _varint(value.length);
    _bytes.add(value);
  }

  void bytes(int number, Uint8List value) {
    if (value.isNotEmpty) {
      message(number, value);
    }
  }

  Uint8List takeBytes() => _bytes.takeBytes();

  void _tag(int number, int wireType) => _varint((number << 3) | wireType);

  void _varint(int value) {
    if (value < 0) {
      throw const FormatException('Negative protobuf varint');
    }
    do {
      var byte = value & 0x7f;
      value >>= 7;
      if (value != 0) {
        byte |= 0x80;
      }
      _bytes.addByte(byte);
    } while (value != 0);
  }
}

class _ProtoField {
  const _ProtoField(this.number, this.wireType);

  final int number;
  final int wireType;
}

class _ProtoReader {
  _ProtoReader(this._bytes);

  final Uint8List _bytes;
  int _offset = 0;

  bool get isDone => _offset == _bytes.length;

  _ProtoField field() {
    final tag = _varint();
    final number = tag >> 3;
    final wireType = tag & 7;
    if (number == 0 || !<int>{0, 1, 2, 5}.contains(wireType)) {
      throw const FormatException('Invalid protobuf field');
    }
    return _ProtoField(number, wireType);
  }

  int varint(_ProtoField field) {
    _expect(field, 0);
    return _varint();
  }

  String string(_ProtoField field) {
    return utf8.decode(_lengthDelimited(field), allowMalformed: false);
  }

  _ProtoReader message(_ProtoField field) {
    return _ProtoReader(_lengthDelimited(field));
  }

  void skip(_ProtoField field) {
    switch (field.wireType) {
      case 0:
        _varint();
      case 1:
        _advance(8);
      case 2:
        _advance(_varint());
      case 5:
        _advance(4);
      default:
        throw const FormatException('Unsupported protobuf wire type');
    }
  }

  Uint8List _lengthDelimited(_ProtoField field) {
    _expect(field, 2);
    final length = _varint();
    final start = _offset;
    _advance(length);
    return Uint8List.sublistView(_bytes, start, _offset);
  }

  int _varint() {
    var value = 0;
    for (var shift = 0; shift < 70; shift += 7) {
      if (_offset >= _bytes.length) {
        throw const FormatException('Truncated protobuf varint');
      }
      final byte = _bytes[_offset++];
      if (shift == 63 && byte > 1) {
        throw const FormatException('Oversized protobuf varint');
      }
      value |= (byte & 0x7f) << shift;
      if (byte & 0x80 == 0) {
        return value;
      }
    }
    throw const FormatException('Oversized protobuf varint');
  }

  void _advance(int count) {
    if (count < 0 || count > _bytes.length - _offset) {
      throw const FormatException('Truncated protobuf field');
    }
    _offset += count;
  }

  void _expect(_ProtoField field, int wireType) {
    if (field.wireType != wireType) {
      throw const FormatException('Unexpected protobuf wire type');
    }
  }
}
