import 'dart:convert';
import 'dart:typed_data';

import 'package:flutter/foundation.dart';

import '../models/app_models.dart';
import 'engine_client.dart';

/// Maximum framed IPC message size (4 MiB), shared with the Rust engine.
const int kMaximumFrameBytes = 4 * 1024 * 1024;

/// Protobuf encode/decode and domain mapping for desktop engine control IPC.
///
/// Wire formats (field numbers, framing, enums) must stay byte-compatible with
/// the Rust `usque.v1` control service.
class ControlCodec {
  const ControlCodec();

  /// Builds a length-prefixed control request frame.
  Uint8List buildRequestFrame({
    required String requestId,
    required int payloadField,
    required Uint8List payload,
  }) {
    final envelope = ControlPayloadWriter()
      ..string(1, requestId)
      ..message(payloadField, payload);
    return frame(envelope.takeBytes());
  }

  /// Length-prefixes a protobuf payload for named-pipe / unix-socket exchange.
  Uint8List frame(Uint8List payload) {
    if (payload.length > kMaximumFrameBytes) {
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

  Uint8List encodeProfile(UsqueProfile profile) {
    final endpoint = ControlPayloadWriter()
      ..string(1, profile.endpointIpv4)
      ..string(2, profile.endpointIpv6)
      ..unsigned(3, profile.endpointPort)
      ..string(4, profile.sni);
    final proxy = ControlPayloadWriter()
      ..string(1, '${profile.proxy.socksIpv4}:${profile.proxy.socksPort}')
      ..string(1, '[${profile.proxy.socksIpv6}]:${profile.proxy.socksPort}')
      ..string(2, '${profile.proxy.httpIpv4}:${profile.proxy.httpPort}')
      ..string(2, '[${profile.proxy.httpIpv6}]:${profile.proxy.httpPort}')
      ..boolean(3, profile.proxy.systemProxy)
      ..unsigned(4, 60)
      ..enumeration(5, profile.proxy.dnsMode.index + 1)
      ..string(6, profile.proxy.dnsIpv4)
      ..string(6, profile.proxy.dnsIpv6)
      ..string(7, profile.proxy.authUsername);
    final frontends = ControlPayloadWriter()
      ..boolean(1, profile.frontends.tunnel)
      ..boolean(2, profile.frontends.socks5)
      ..boolean(3, profile.frontends.http);
    final writer = ControlPayloadWriter()
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
      ..enumeration(14, profile.dnsMode.index + 1)
      ..message(15, frontends.takeBytes());
    for (final country in profile.geoDirectCountries) {
      writer.string(16, country);
    }
    return writer.takeBytes();
  }

  ControlResponse decodeResponse(Uint8List frame, String expectedRequestId) {
    try {
      if (frame.length < 4) {
        throw const EngineException(
          'ENGINE_IPC_TRUNCATED',
          'The local Engine response header was truncated.',
        );
      }
      final length = ByteData.sublistView(frame).getUint32(0, Endian.big);
      if (length > kMaximumFrameBytes || length != frame.length - 4) {
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
      GeoRulesList? geoRulesList;
      List<GeoRulesUpdateResult>? geoRulesUpdate;
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
          case 17:
            geoRulesList = _decodeGeoRulesList(reader.message(field));
          case 18:
            geoRulesUpdate = _decodeGeoRulesUpdate(reader.message(field));
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
      return ControlResponse(
        snapshot,
        update,
        profileCatalog,
        geoRulesList: geoRulesList,
        geoRulesUpdate: geoRulesUpdate,
      );
    } on FormatException catch (error) {
      throw _invalidIpcResponse(error);
    }
  }

  EngineSnapshot? decodeEventSnapshot(Uint8List frame) {
    return decodeEvent(frame).snapshot;
  }

  EngineSnapshotEvent decodeEvent(Uint8List frame) {
    try {
      if (frame.length < 4) {
        throw const EngineException(
          'ENGINE_EVENT_TRUNCATED',
          'The local Engine event header was truncated.',
        );
      }
      final length = ByteData.sublistView(frame).getUint32(0, Endian.big);
      if (length > kMaximumFrameBytes || length != frame.length - 4) {
        throw const EngineException(
          'ENGINE_EVENT_INVALID',
          'The local Engine event length was invalid.',
        );
      }

      final envelope = _ProtoReader(Uint8List.sublistView(frame, 4));
      EngineSnapshot? snapshot;
      GeoRulesProgress? geoProgress;
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
          case 17:
            geoProgress = _decodeGeoProgress(envelope.message(field));
          default:
            envelope.skip(field);
        }
      }
      return EngineSnapshotEvent(snapshot: snapshot, geoProgress: geoProgress);
    } on FormatException catch (error) {
      throw _invalidIpcResponse(error);
    }
  }

  ProfileCatalog requireProfileCatalog(ControlResponse response) {
    final catalog = response.profileCatalog;
    if (catalog == null) {
      throw const EngineException(
        'ENGINE_IPC_INVALID_RESPONSE',
        'The local Engine returned no profile catalog.',
      );
    }
    return catalog;
  }
}

/// Decoded control response payload (after structured error handling).
class ControlResponse {
  const ControlResponse(
    this.snapshot,
    this.update,
    this.profileCatalog, {
    this.geoRulesList,
    this.geoRulesUpdate,
  });

  final EngineSnapshot? snapshot;
  final UpdateCheckResult? update;
  final ProfileCatalog? profileCatalog;
  final GeoRulesList? geoRulesList;
  final List<GeoRulesUpdateResult>? geoRulesUpdate;
}

/// Minimal protobuf field writer for control request payloads.
class ControlPayloadWriter {
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

@visibleForTesting
Uint8List debugEncodeGetStatusFrame(String requestId) {
  return const ControlCodec().buildRequestFrame(
    requestId: requestId,
    payloadField: 10,
    payload: Uint8List(0),
  );
}

@visibleForTesting
EngineSnapshot debugDecodeStatusFrame(Uint8List frame, String requestId) {
  return const ControlCodec().decodeResponse(frame, requestId).snapshot ??
      const EngineSnapshot();
}

@visibleForTesting
EngineSnapshot? debugDecodeEventSnapshot(Uint8List frame) {
  return const ControlCodec().decodeEventSnapshot(frame);
}

@visibleForTesting
ProfileCatalog debugDecodeProfileCatalogFrame(
  Uint8List frame,
  String requestId,
) {
  return const ControlCodec().requireProfileCatalog(
    const ControlCodec().decodeResponse(frame, requestId),
  );
}

ProfileCatalog _decodeProfileCatalog(_ProtoReader reader) {
  final profiles = <UsqueProfile>[];
  final identityStates = <String, ProfileIdentityState>{};
  final identityStatuses = <String, ProfileIdentityStatus>{};
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
        var licenseState = LicenseState.unknown;
        var accountType = '';
        var cleanupPending = false;
        var provider = IdentityProvider.consumer;
        var organization = '';
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
            case 3:
              final value = status.varint(statusField);
              if (value >= 1 && value <= LicenseState.values.length) {
                licenseState = LicenseState.values[value - 1];
              }
            case 4:
              accountType = status.string(statusField);
            case 5:
              cleanupPending = status.varint(statusField) != 0;
            case 6:
              final value = status.varint(statusField);
              if (value >= 1 && value <= IdentityProvider.values.length) {
                provider = IdentityProvider.values[value - 1];
              }
            case 7:
              organization = status.string(statusField);
            default:
              status.skip(statusField);
          }
        }
        if (profileId != null && state != null) {
          identityStates[profileId] = state;
          identityStatuses[profileId] = ProfileIdentityStatus(
            state: state,
            licenseState: licenseState,
            accountType: accountType,
            cleanupPending: cleanupPending,
            provider: provider,
            organization: organization,
          );
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
    identityStatuses: Map<String, ProfileIdentityStatus>.unmodifiable(
      identityStatuses,
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
  var frontends = defaults.frontends;
  var frontendsSeen = false;
  final geoDirectCountries = <String>[];

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
          'Endpoint family policy',
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
      case 15:
        final source = reader.message(field);
        var tunnel = false;
        var socks5 = false;
        var http = false;
        while (!source.isDone) {
          final frontendField = source.field();
          switch (frontendField.number) {
            case 1:
              tunnel = source.varint(frontendField) != 0;
            case 2:
              socks5 = source.varint(frontendField) != 0;
            case 3:
              http = source.varint(frontendField) != 0;
            default:
              source.skip(frontendField);
          }
        }
        frontends = FrontendSettings(
          tunnel: tunnel,
          socks5: socks5,
          http: http,
        );
        frontendsSeen = true;
      case 16:
        geoDirectCountries.add(reader.string(field));
      default:
        reader.skip(field);
    }
  }
  if (!frontendsSeen) {
    frontends = FrontendSettings(
      tunnel: mode == OperatingMode.vpn,
      socks5: mode == OperatingMode.socks5,
      http: mode == OperatingMode.httpProxy,
    );
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
    geoDirectCountries: List<String>.unmodifiable(geoDirectCountries),
    proxy: proxy,
    frontends: frontends,
  );
}

GeoRulesList _decodeGeoRulesList(_ProtoReader reader) {
  final entries = <GeoRulesEntry>[];
  var lastSuccessfulUpdateUnixMilliseconds = 0;
  var hasGlobalGeosite = false;
  var globalGeositeUpdatedUnixMilliseconds = 0;
  while (!reader.isDone) {
    final field = reader.field();
    switch (field.number) {
      case 1:
        entries.add(_decodeGeoRulesEntry(reader.message(field)));
      case 2:
        lastSuccessfulUpdateUnixMilliseconds = reader.varint(field);
      case 3:
        hasGlobalGeosite = reader.varint(field) != 0;
      case 4:
        globalGeositeUpdatedUnixMilliseconds = reader.varint(field);
      default:
        reader.skip(field);
    }
  }
  return GeoRulesList(
    entries: List<GeoRulesEntry>.unmodifiable(entries),
    lastSuccessfulUpdateUnixMilliseconds: lastSuccessfulUpdateUnixMilliseconds,
    hasGlobalGeosite: hasGlobalGeosite,
    globalGeositeUpdatedUnixMilliseconds: globalGeositeUpdatedUnixMilliseconds,
  );
}

GeoRulesEntry _decodeGeoRulesEntry(_ProtoReader reader) {
  var countryCode = '';
  var hasGeoip = false;
  var hasGeosite = false;
  var lastUpdatedUnixMilliseconds = 0;
  while (!reader.isDone) {
    final field = reader.field();
    switch (field.number) {
      case 1:
        countryCode = reader.string(field);
      case 2:
        hasGeoip = reader.varint(field) != 0;
      case 3:
        hasGeosite = reader.varint(field) != 0;
      case 4:
        lastUpdatedUnixMilliseconds = reader.varint(field);
      default:
        reader.skip(field);
    }
  }
  return GeoRulesEntry(
    countryCode: countryCode,
    hasGeoip: hasGeoip,
    hasGeosite: hasGeosite,
    lastUpdatedUnixMilliseconds: lastUpdatedUnixMilliseconds,
  );
}

List<GeoRulesUpdateResult> _decodeGeoRulesUpdate(_ProtoReader reader) {
  final results = <GeoRulesUpdateResult>[];
  while (!reader.isDone) {
    final field = reader.field();
    if (field.number != 1) {
      reader.skip(field);
      continue;
    }
    final item = reader.message(field);
    var countryCode = '';
    var status = GeoRulesUpdateStatus.updated;
    var reason = '';
    var artifactKind = '';
    var artifactScope = '';
    while (!item.isDone) {
      final itemField = item.field();
      switch (itemField.number) {
        case 1:
          countryCode = item.string(itemField);
        case 2:
          status = switch (item.varint(itemField)) {
            1 => GeoRulesUpdateStatus.upToDate,
            3 => GeoRulesUpdateStatus.failed,
            _ => GeoRulesUpdateStatus.updated,
          };
        case 3:
          reason = item.string(itemField);
        case 4:
          artifactKind = item.string(itemField);
        case 5:
          artifactScope = item.string(itemField);
        default:
          item.skip(itemField);
      }
    }
    results.add(
      GeoRulesUpdateResult(
        countryCode: countryCode,
        status: status,
        reason: reason,
        artifactKind: artifactKind,
        artifactScope: artifactScope,
      ),
    );
  }
  return results;
}

GeoRulesProgress _decodeGeoProgress(_ProtoReader reader) {
  var currentFile = '';
  var completed = 0;
  var total = 0;
  while (!reader.isDone) {
    final field = reader.field();
    switch (field.number) {
      case 1:
        currentFile = reader.string(field);
      case 2:
        completed = reader.varint(field);
      case 3:
        total = reader.varint(field);
      default:
        reader.skip(field);
    }
  }
  return GeoRulesProgress(
    currentFile: currentFile,
    completed: completed,
    total: total,
  );
}

ProxySettings _decodeProxySettings(
  _ProtoReader reader,
  ProxySettings defaults,
) {
  final socksListeners = <String>[];
  final httpListeners = <String>[];
  final dnsServers = <String>[];
  var systemProxy = defaults.systemProxy;
  var dnsMode = defaults.dnsMode;
  var authUsername = defaults.authUsername;
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
      case 6:
        dnsServers.add(reader.string(field));
      case 7:
        authUsername = reader.string(field);
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
    dnsIpv4:
        dnsServers.where((value) => value.contains('.')).firstOrNull ??
        defaults.dnsIpv4,
    dnsIpv6:
        dnsServers.where((value) => value.contains(':')).firstOrNull ??
        defaults.dnsIpv6,
    systemProxy: systemProxy,
    authUsername: authUsername,
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
      port: _parseListenerPort(value.substring(closing + 2)),
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
    port: _parseListenerPort(value.substring(separator + 1)),
  );
}

int _parseListenerPort(String value) {
  final port = int.tryParse(value);
  if (port == null) {
    throw const EngineException(
      'ENGINE_IPC_INVALID_RESPONSE',
      'The local Engine returned an invalid listener port.',
    );
  }
  return port;
}

EngineException _invalidIpcResponse(FormatException error) {
  return EngineException(
    'ENGINE_IPC_INVALID_RESPONSE',
    'The local Engine response could not be decoded: ${error.message}',
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
  var reconnectCount = 0;
  String? killSwitchState;
  var platformLockdown = false;
  final activeListeners = <String>[];
  final frontends = <FrontendRuntimeStatus>[];
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
      case 9:
        killSwitchState = _decodeKillSwitchState(reader.varint(field));
      case 10:
        platformLockdown = reader.varint(field) == 3;
      case 11:
        reconnectCount = reader.varint(field);
      case 12:
        activeListeners.add(reader.string(field));
      case 15:
        frontends.add(_decodeFrontendStatus(reader.message(field)));
      default:
        // Includes reserved field 14 (legacy captive-portal countdown).
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
    reconnectCount: reconnectCount,
    killSwitchState: killSwitchState,
    platformLockdown: platformLockdown,
    activeListeners: List<String>.unmodifiable(activeListeners),
    frontends: List<FrontendRuntimeStatus>.unmodifiable(frontends),
  );
}

String? _decodeKillSwitchState(int value) {
  return switch (value) {
    1 => 'notApplicable',
    2 => 'inactive',
    3 => 'active',
    5 => 'error',
    _ => null,
  };
}

FrontendRuntimeStatus _decodeFrontendStatus(_ProtoReader reader) {
  var kind = FrontendKind.tunnel;
  var phase = FrontendPhase.error;
  final listeners = <String>[];
  String? errorCode;
  while (!reader.isDone) {
    final field = reader.field();
    switch (field.number) {
      case 1:
        final value = reader.varint(field);
        if (value >= 1 && value <= FrontendKind.values.length) {
          kind = FrontendKind.values[value - 1];
        }
      case 2:
        final value = reader.varint(field);
        if (value >= 1 && value <= FrontendPhase.values.length) {
          phase = FrontendPhase.values[value - 1];
        }
      case 3:
        listeners.add(reader.string(field));
      case 4:
        errorCode = _decodeError(reader.message(field)).code;
      default:
        reader.skip(field);
    }
  }
  return FrontendRuntimeStatus(
    kind: kind,
    phase: phase,
    listeners: List<String>.unmodifiable(listeners),
    errorCode: errorCode,
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
    // 9 was captivePortalPaused (removed).
    10 => ConnectionPhase.error,
    _ => ConnectionPhase.error,
  };
}

String? _emptyToNull(String value) => value.isEmpty ? null : value;

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
