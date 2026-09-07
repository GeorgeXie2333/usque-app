import 'dart:convert';
import 'dart:typed_data';

import 'package:flutter/foundation.dart';

import '../models/app_models.dart';
import '../models/diagnostics_models.dart';
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
    final directDns = ControlPayloadWriter()
      ..enumeration(1, _directDnsModeWireValue(profile.directDns.mode))
      ..string(2, profile.directDns.serverName)
      ..string(3, profile.directDns.dohPath);
    for (final bootstrapIp in profile.directDns.bootstrapIps) {
      directDns.string(4, bootstrapIp);
    }
    directDns.unsigned(5, profile.directDns.port);
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
    writer.message(17, directDns.takeBytes());
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
      DiagnosticSession? diagnosticSession;
      ConnectionTimeline? connectionTimeline;
      NetworkQualitySnapshot? networkQuality;
      EngineCapabilities? capabilities;
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
          case 19:
            diagnosticSession = _decodeDiagnosticSession(reader.message(field));
          case 20:
            connectionTimeline = _decodeConnectionTimeline(
              reader.message(field),
            );
          case 21:
            networkQuality = _decodeNetworkQuality(reader.message(field));
          case 15:
            capabilities = _decodeCapabilities(reader.message(field));
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
        throw EngineException(
          error.code,
          error.message,
          retryable: error.retryable,
        );
      }
      return ControlResponse(
        snapshot,
        update,
        profileCatalog,
        geoRulesList: geoRulesList,
        geoRulesUpdate: geoRulesUpdate,
        diagnosticSession: diagnosticSession,
        connectionTimeline: connectionTimeline,
        networkQuality: networkQuality,
        capabilities: capabilities,
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
      DiagnosticSession? diagnosticSession;
      NetworkQualitySnapshot? networkQuality;
      EngineCapabilities? capabilities;
      var diagnosticsChanged = false;
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
          case 18:
          case 21:
          case 22:
            final event = envelope.message(field);
            diagnosticsChanged = true;
            while (!event.isDone) {
              final eventField = event.field();
              if (eventField.number == 1) {
                diagnosticSession = _decodeDiagnosticSession(
                  event.message(eventField),
                );
              } else {
                event.skip(eventField);
              }
            }
          case 19:
          case 20:
            diagnosticsChanged = true;
            envelope.skip(field);
          case 14:
            final changed = envelope.message(field);
            while (!changed.isDone) {
              final changedField = changed.field();
              if (changedField.number == 1) {
                capabilities = _decodeCapabilities(
                  changed.message(changedField),
                );
              } else {
                changed.skip(changedField);
              }
            }
          case 23:
            final updated = envelope.message(field);
            while (!updated.isDone) {
              final updatedField = updated.field();
              if (updatedField.number == 1) {
                networkQuality = _decodeNetworkQuality(
                  updated.message(updatedField),
                );
              } else {
                updated.skip(updatedField);
              }
            }
          default:
            envelope.skip(field);
        }
      }
      return EngineSnapshotEvent(
        snapshot: snapshot,
        geoProgress: geoProgress,
        diagnosticSession: diagnosticSession,
        diagnosticsChanged: diagnosticsChanged,
        networkQuality: networkQuality,
        capabilities: capabilities,
      );
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
    this.diagnosticSession,
    this.connectionTimeline,
    this.networkQuality,
    this.capabilities,
  });

  final EngineSnapshot? snapshot;
  final UpdateCheckResult? update;
  final ProfileCatalog? profileCatalog;
  final GeoRulesList? geoRulesList;
  final List<GeoRulesUpdateResult>? geoRulesUpdate;
  final DiagnosticSession? diagnosticSession;
  final ConnectionTimeline? connectionTimeline;
  final NetworkQualitySnapshot? networkQuality;
  final EngineCapabilities? capabilities;
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
Uint8List debugEncodeProfilePayload(UsqueProfile profile) {
  return const ControlCodec().encodeProfile(profile);
}

@visibleForTesting
EngineSnapshot debugDecodeStatusFrame(Uint8List frame, String requestId) {
  return const ControlCodec().decodeResponse(frame, requestId).snapshot ??
      const EngineSnapshot();
}

@visibleForTesting
NetworkQualitySnapshot? debugDecodeNetworkQualityFrame(
  Uint8List frame,
  String requestId,
) {
  return const ControlCodec().decodeResponse(frame, requestId).networkQuality;
}

@visibleForTesting
ConnectionTimeline? debugDecodeConnectionTimelineFrame(
  Uint8List frame,
  String requestId,
) {
  return const ControlCodec()
      .decodeResponse(frame, requestId)
      .connectionTimeline;
}

@visibleForTesting
EngineSnapshotEvent debugDecodeEventFrame(Uint8List frame) {
  return const ControlCodec().decodeEvent(frame);
}

@visibleForTesting
EngineCapabilities? debugDecodeCapabilitiesFrame(
  Uint8List frame,
  String requestId,
) {
  return const ControlCodec().decodeResponse(frame, requestId).capabilities;
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
  var directDns = defaults.directDns;

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
      case 17:
        directDns = _decodeDirectDnsSettings(reader.message(field));
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
    directDns: directDns,
  );
}

DirectDnsSettings _decodeDirectDnsSettings(_ProtoReader reader) {
  var mode = DirectDnsMode.physicalSystem;
  var serverName = '';
  var dohPath = '';
  final bootstrapIps = <String>[];
  var port = 0;
  while (!reader.isDone) {
    final field = reader.field();
    switch (field.number) {
      case 1:
        mode = _decodeDirectDnsMode(reader.varint(field));
      case 2:
        serverName = reader.string(field);
      case 3:
        dohPath = reader.string(field);
      case 4:
        bootstrapIps.add(reader.string(field));
      case 5:
        port = reader.varint(field);
      default:
        reader.skip(field);
    }
  }
  return DirectDnsSettings(
    mode: mode,
    serverName: serverName,
    dohPath: dohPath,
    bootstrapIps: List<String>.unmodifiable(bootstrapIps),
    port: port,
  );
}

int _directDnsModeWireValue(DirectDnsMode mode) => switch (mode) {
  DirectDnsMode.unknown => 0,
  DirectDnsMode.physicalSystem => 1,
  DirectDnsMode.doh => 2,
  DirectDnsMode.dot => 3,
};

DirectDnsMode _decodeDirectDnsMode(int value) => switch (value) {
  1 => DirectDnsMode.physicalSystem,
  2 => DirectDnsMode.doh,
  3 => DirectDnsMode.dot,
  _ => DirectDnsMode.unknown,
};

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

DiagnosticSession _decodeDiagnosticSession(_ProtoReader reader) {
  var sessionId = '';
  var state = DiagnosticSessionState.failed;
  var startedAt = DateTime.fromMillisecondsSinceEpoch(0, isUtc: true);
  DateTime? completedAt;
  var mode = DiagnosticMode.standard;
  String? currentCheck;
  var progressPercent = 0;
  final findings = <DiagnosticFinding>[];
  var summary = const DiagnosticSummary();
  while (!reader.isDone) {
    final field = reader.field();
    switch (field.number) {
      case 1:
        sessionId = reader.string(field);
      case 2:
        state = _decodeIndexedEnum(
          DiagnosticSessionState.values,
          reader.varint(field),
          'diagnostic session state',
        );
      case 3:
        final milliseconds = reader.varint(field);
        if (milliseconds > 0) {
          startedAt = DateTime.fromMillisecondsSinceEpoch(
            milliseconds,
            isUtc: true,
          );
        }
      case 4:
        final milliseconds = reader.varint(field);
        if (milliseconds > 0) {
          completedAt = DateTime.fromMillisecondsSinceEpoch(
            milliseconds,
            isUtc: true,
          );
        }
      case 5:
        mode = _decodeIndexedEnum(
          DiagnosticMode.values,
          reader.varint(field),
          'diagnostic mode',
        );
      case 6:
        currentCheck = _emptyToNull(reader.string(field));
      case 7:
        progressPercent = reader.varint(field).clamp(0, 100);
      case 8:
        findings.add(_decodeDiagnosticFinding(reader.message(field)));
      case 9:
        summary = _decodeDiagnosticSummary(reader.message(field));
      default:
        reader.skip(field);
    }
  }
  return DiagnosticSession(
    sessionId: sessionId,
    state: state,
    startedAt: startedAt,
    completedAt: completedAt,
    mode: mode,
    currentCheck: currentCheck,
    progressPercent: progressPercent,
    findings: List<DiagnosticFinding>.unmodifiable(findings),
    summary: summary,
  );
}

DiagnosticFinding _decodeDiagnosticFinding(_ProtoReader reader) {
  var checkId = '';
  var category = DiagnosticCategory.localComponent;
  var status = DiagnosticCheckStatus.pending;
  TransportFailureInfo? failure;
  var severity = DiagnosticSeverity.info;
  var summaryKey = '';
  var remediationKey = '';
  final evidence = <String>[];
  DateTime? startedAt;
  int? durationMilliseconds;
  String? dependencyReason;
  while (!reader.isDone) {
    final field = reader.field();
    switch (field.number) {
      case 1:
        checkId = reader.string(field);
      case 2:
        category = _decodeIndexedEnum(
          DiagnosticCategory.values,
          reader.varint(field),
          'diagnostic category',
        );
      case 3:
        status = _decodeIndexedEnum(
          DiagnosticCheckStatus.values,
          reader.varint(field),
          'diagnostic check status',
        );
      case 4:
        failure = _decodeTransportFailure(reader.message(field));
      case 5:
        severity = _decodeIndexedEnum(
          DiagnosticSeverity.values,
          reader.varint(field),
          'diagnostic severity',
        );
      case 6:
        summaryKey = reader.string(field);
      case 7:
        remediationKey = reader.string(field);
      case 8:
        evidence.add(reader.string(field));
      case 9:
        final milliseconds = reader.varint(field);
        if (milliseconds > 0) {
          startedAt = DateTime.fromMillisecondsSinceEpoch(
            milliseconds,
            isUtc: true,
          );
        }
      case 10:
        final value = reader.varint(field);
        durationMilliseconds = value == 0 ? null : value;
      case 11:
        dependencyReason = _emptyToNull(reader.string(field));
      default:
        reader.skip(field);
    }
  }
  return DiagnosticFinding(
    checkId: checkId,
    category: category,
    status: status,
    failure: failure,
    severity: severity,
    summaryKey: summaryKey,
    remediationKey: remediationKey,
    sanitizedEvidence: List<String>.unmodifiable(evidence),
    startedAt: startedAt,
    durationMilliseconds: durationMilliseconds,
    dependencyReason: dependencyReason,
  );
}

DiagnosticSummary _decodeDiagnosticSummary(_ProtoReader reader) {
  var passed = 0;
  var warnings = 0;
  var failed = 0;
  var skipped = 0;
  var cancelled = 0;
  while (!reader.isDone) {
    final field = reader.field();
    switch (field.number) {
      case 1:
        passed = reader.varint(field);
      case 2:
        warnings = reader.varint(field);
      case 3:
        failed = reader.varint(field);
      case 4:
        skipped = reader.varint(field);
      case 5:
        cancelled = reader.varint(field);
      default:
        reader.skip(field);
    }
  }
  return DiagnosticSummary(
    passed: passed,
    warnings: warnings,
    failed: failed,
    skipped: skipped,
    cancelled: cancelled,
  );
}

TransportFailureInfo _decodeTransportFailure(_ProtoReader reader) {
  var code = 'INTERNAL';
  var stage = 'diagnostics';
  String? transport;
  String? addressFamily;
  var retryable = false;
  var fallbackAllowed = false;
  var severity = DiagnosticSeverity.error;
  var remediationKey = '';
  String? sanitizedDetail;
  while (!reader.isDone) {
    final field = reader.field();
    switch (field.number) {
      case 1:
        code = reader.string(field);
      case 2:
        stage = reader.string(field);
      case 3:
        transport = _emptyToNull(reader.string(field));
      case 4:
        addressFamily = _emptyToNull(reader.string(field));
      case 5:
        retryable = reader.varint(field) != 0;
      case 6:
        fallbackAllowed = reader.varint(field) != 0;
      case 7:
        severity = _decodeIndexedEnum(
          DiagnosticSeverity.values,
          reader.varint(field),
          'failure severity',
        );
      case 8:
        remediationKey = reader.string(field);
      case 9:
        sanitizedDetail = _emptyToNull(reader.string(field));
      default:
        reader.skip(field);
    }
  }
  return TransportFailureInfo(
    code: code,
    stage: stage,
    transport: transport,
    addressFamily: addressFamily,
    retryable: retryable,
    fallbackAllowed: fallbackAllowed,
    severity: severity,
    remediationKey: remediationKey,
    sanitizedDetail: sanitizedDetail,
  );
}

ConnectionTimeline _decodeConnectionTimeline(_ProtoReader reader) {
  final events = <ConnectionTimelineEvent>[];
  var metrics = const ConnectionMetrics();
  var droppedEventCount = 0;
  while (!reader.isDone) {
    final field = reader.field();
    switch (field.number) {
      case 1:
        events.add(_decodeConnectionTimelineEvent(reader.message(field)));
      case 2:
        metrics = _decodeConnectionMetrics(reader.message(field));
      case 3:
        droppedEventCount = reader.varint(field);
      default:
        reader.skip(field);
    }
  }
  return ConnectionTimeline(
    events: List<ConnectionTimelineEvent>.unmodifiable(events),
    metrics: metrics,
    droppedEventCount: droppedEventCount,
  );
}

ConnectionTimelineEvent _decodeConnectionTimelineEvent(_ProtoReader reader) {
  var sequence = 0;
  DateTime? timestamp;
  var elapsedMilliseconds = 0;
  var eventType = ConnectionTimelineEventType.unknown;
  String? stage;
  String? transport;
  String? addressFamily;
  int? durationMilliseconds;
  TransportFailureInfo? failure;
  while (!reader.isDone) {
    final field = reader.field();
    switch (field.number) {
      case 1:
        sequence = reader.varint(field);
      case 2:
        final milliseconds = reader.varint(field);
        if (milliseconds > 0) {
          timestamp = DateTime.fromMillisecondsSinceEpoch(
            milliseconds,
            isUtc: true,
          );
        }
      case 3:
        elapsedMilliseconds = reader.varint(field);
      case 4:
        eventType = _decodeConnectionTimelineEventType(reader.varint(field));
      case 5:
        stage = _emptyToNull(reader.string(field));
      case 6:
        transport = _emptyToNull(reader.string(field));
      case 7:
        addressFamily = _emptyToNull(reader.string(field));
      case 8:
        final value = reader.varint(field);
        durationMilliseconds = value == 0 ? null : value;
      case 9:
        failure = _decodeTransportFailure(reader.message(field));
      default:
        reader.skip(field);
    }
  }
  return ConnectionTimelineEvent(
    sequence: sequence,
    timestamp: timestamp,
    elapsedMilliseconds: elapsedMilliseconds,
    eventType: eventType,
    stage: stage,
    transport: transport,
    addressFamily: addressFamily,
    durationMilliseconds: durationMilliseconds,
    failure: failure,
  );
}

ConnectionTimelineEventType _decodeConnectionTimelineEventType(int wireValue) {
  return switch (wireValue) {
    1 => ConnectionTimelineEventType.attemptStarted,
    2 => ConnectionTimelineEventType.endpointResolved,
    3 => ConnectionTimelineEventType.socketConnected,
    4 => ConnectionTimelineEventType.tlsReady,
    5 => ConnectionTimelineEventType.quicReady,
    6 => ConnectionTimelineEventType.masqueAccepted,
    7 => ConnectionTimelineEventType.peerSettingsReceived,
    8 => ConnectionTimelineEventType.addressAssigned,
    9 => ConnectionTimelineEventType.tunnelReady,
    10 => ConnectionTimelineEventType.firstPacketSent,
    11 => ConnectionTimelineEventType.firstPacketReceived,
    12 => ConnectionTimelineEventType.fallbackStarted,
    13 => ConnectionTimelineEventType.reconnectScheduled,
    14 => ConnectionTimelineEventType.networkChanged,
    15 => ConnectionTimelineEventType.recoveryProbeStarted,
    16 => ConnectionTimelineEventType.recoveryProbeSucceeded,
    17 => ConnectionTimelineEventType.recoveryProbeFailed,
    18 => ConnectionTimelineEventType.pathPromoted,
    19 => ConnectionTimelineEventType.queueSaturated,
    20 => ConnectionTimelineEventType.disconnected,
    21 => ConnectionTimelineEventType.failed,
    22 => ConnectionTimelineEventType.migrationStarted,
    23 => ConnectionTimelineEventType.migrationPathValidated,
    24 => ConnectionTimelineEventType.migrationPromoted,
    25 => ConnectionTimelineEventType.migrationFailed,
    26 => ConnectionTimelineEventType.pmtuChanged,
    27 => ConnectionTimelineEventType.pmtuRevalidationStarted,
    28 => ConnectionTimelineEventType.pmtuRevalidationFailed,
    29 => ConnectionTimelineEventType.directDnsDegraded,
    30 => ConnectionTimelineEventType.directDnsRecovered,
    _ => ConnectionTimelineEventType.unknown,
  };
}

ConnectionMetrics _decodeConnectionMetrics(_ProtoReader reader) {
  int? lastConnectDuration;
  int? h3Duration;
  int? h2Duration;
  var rtt = 0;
  var rttKnown = false;
  var reconnectCount = 0;
  var fallbackCount = 0;
  var networkChangeCount = 0;
  var highWatermark = 0;
  var dropCount = 0;
  String? lastFailureCode;
  String? lastReconnectCode;
  while (!reader.isDone) {
    final field = reader.field();
    switch (field.number) {
      case 1:
        final value = reader.varint(field);
        lastConnectDuration = value == 0 ? null : value;
      case 2:
        final value = reader.varint(field);
        h3Duration = value == 0 ? null : value;
      case 3:
        final value = reader.varint(field);
        h2Duration = value == 0 ? null : value;
      case 4:
        rtt = reader.varint(field);
      case 5:
        rttKnown = reader.varint(field) != 0;
      case 6:
        reconnectCount = reader.varint(field);
      case 7:
        fallbackCount = reader.varint(field);
      case 8:
        networkChangeCount = reader.varint(field);
      case 9:
        highWatermark = reader.varint(field);
      case 10:
        dropCount = reader.varint(field);
      case 11:
        lastFailureCode = _emptyToNull(reader.string(field));
      case 12:
        lastReconnectCode = _emptyToNull(reader.string(field));
      default:
        reader.skip(field);
    }
  }
  return ConnectionMetrics(
    lastConnectDurationMilliseconds: lastConnectDuration,
    lastH3HandshakeDurationMilliseconds: h3Duration,
    lastH2HandshakeDurationMilliseconds: h2Duration,
    currentSmoothedRttMilliseconds: rttKnown ? rtt : null,
    reconnectCount: reconnectCount,
    fallbackCount: fallbackCount,
    networkChangeCount: networkChangeCount,
    sendQueueHighWatermark: highWatermark,
    sendQueueDropCount: dropCount,
    lastFailureCode: lastFailureCode,
    lastReconnectCode: lastReconnectCode,
  );
}

EngineCapabilities _decodeCapabilities(_ProtoReader reader) {
  var networkQuality = false;
  var encryptedDirectDns = false;
  var quicMigration = false;
  var automaticPmtu = false;
  while (!reader.isDone) {
    final field = reader.field();
    switch (field.number) {
      case 20:
        networkQuality = reader.varint(field) != 0;
      case 21:
        encryptedDirectDns = reader.varint(field) != 0;
      case 22:
        quicMigration = reader.varint(field) != 0;
      case 23:
        automaticPmtu = reader.varint(field) != 0;
      default:
        reader.skip(field);
    }
  }
  return EngineCapabilities(
    networkQuality: networkQuality,
    encryptedDirectDns: encryptedDirectDns,
    quicMigration: quicMigration,
    automaticPmtu: automaticPmtu,
  );
}

NetworkQualitySnapshot _decodeNetworkQuality(_ProtoReader reader) {
  DateTime? sampledAt;
  String? connectionInstanceId;
  var level = NetworkQualityLevel.unknown;
  var metrics = const NetworkConnectionMetrics();
  final queues = <NetworkQueueQuality>[];
  final samples = <NetworkQualitySample>[];
  var sampleCount = 0;
  var pmtu = const PmtuQualityInfo();
  var migration = const MigrationQualityInfo();
  var directDns = const DirectDnsQualityInfo();
  while (!reader.isDone) {
    final field = reader.field();
    switch (field.number) {
      case 1:
        final milliseconds = reader.varint(field);
        sampledAt = milliseconds <= 0 || milliseconds > 8640000000000000
            ? null
            : DateTime.fromMillisecondsSinceEpoch(milliseconds, isUtc: true);
      case 2:
        final id = reader.string(field);
        connectionInstanceId = id.length <= 64 ? _emptyToNull(id) : null;
      case 3:
        level = _decodeNetworkQualityLevel(reader.varint(field));
      case 4:
        metrics = _decodeNetworkConnectionMetrics(reader.message(field));
      case 5:
        if (queues.length < 8) {
          queues.add(_decodeNetworkQueueQuality(reader.message(field)));
        } else {
          reader.skip(field);
        }
      case 6:
        pmtu = _decodePmtuQuality(reader.message(field));
      case 7:
        migration = _decodeMigrationQuality(reader.message(field));
      case 8:
        directDns = _decodeDirectDnsQuality(reader.message(field));
      case 9:
        if (sampleCount++ < 16) {
          final sample = _decodeNetworkQualitySample(reader.message(field));
          if (sample != null) samples.add(sample);
        } else {
          reader.skip(field);
        }
      default:
        reader.skip(field);
    }
  }
  return NetworkQualitySnapshot(
    sampledAt: sampledAt,
    connectionInstanceId: connectionInstanceId,
    level: level,
    metrics: metrics,
    queues: List<NetworkQueueQuality>.unmodifiable(queues),
    pmtu: pmtu,
    migration: migration,
    directDns: directDns,
    samples: List.unmodifiable(samples),
  );
}

NetworkQualitySample? _decodeNetworkQualitySample(_ProtoReader reader) {
  const keys = <int, String>{
    1: 'sequence',
    2: 'sampled_at_unix_ms',
    3: 'monotonic_millis',
    4: 'downloaded_bytes',
    5: 'uploaded_bytes',
    6: 'rtt_ms',
    7: 'loss_basis_points',
  };
  // Proto3 omits the zero monotonic origin; optional metrics retain presence.
  final values = <Object?, Object?>{'monotonic_millis': 0};
  while (!reader.isDone) {
    final field = reader.field();
    final key = keys[field.number];
    if (key == null) {
      reader.skip(field);
    } else {
      values[key] = reader.varint(field);
    }
  }
  return NetworkQualitySample.fromMap(values);
}

NetworkConnectionMetrics _decodeNetworkConnectionMetrics(_ProtoReader reader) {
  var latestRtt = 0;
  var latestRttKnown = false;
  var latestAvailability = MetricAvailability.unknown;
  var smoothedRtt = 0;
  var smoothedRttKnown = false;
  var minimumRtt = 0;
  var minimumRttKnown = false;
  var rttVariance = 0;
  var rttVarianceKnown = false;
  var intervalLoss = 0;
  var intervalLossKnown = false;
  var congestionWindow = 0;
  var congestionWindowKnown = false;
  var bytesInFlight = 0;
  var bytesInFlightKnown = false;
  var sendRate = 0;
  var sendRateKnown = false;
  var packetsLost = 0;
  var bytesLost = 0;
  var tunSinkDrops = 0;
  var quicDatagramDrops = 0;
  var queueOldestAge = 0;
  var queueOldestAgeKnown = false;
  var currentPmtu = 0;
  var currentPmtuKnown = false;
  var migrationAttempts = 0;
  var migrationSuccesses = 0;
  var migrationFailures = 0;
  var lastMigrationDuration = 0;
  var lastMigrationDurationKnown = false;
  var udpSendSyscalls = 0;
  var udpRecvSyscalls = 0;
  var udpDatagramsSent = 0;
  var udpDatagramsReceived = 0;
  var poolHits = 0;
  var poolMisses = 0;
  var h2StallCount = 0;
  var h2StallTotal = 0;
  var h2StallMax = 0;
  var h2StreamWindow = 0;
  var h2ConnectionWindow = 0;
  var dnsSuccesses = 0;
  var dnsFailures = 0;
  var dnsTimeouts = 0;
  var dnsLastRtt = 0;
  var dnsLastRttKnown = false;
  var pmtuChanges = 0;
  var pmtuRevalidationFailures = 0;
  var pmtuSendTooLarge = 0;
  var smoothedAvailability = MetricAvailability.unknown;
  var minimumAvailability = MetricAvailability.unknown;
  var varianceAvailability = MetricAvailability.unknown;
  var lossAvailability = MetricAvailability.unknown;
  var congestionAvailability = MetricAvailability.unknown;
  var bytesInFlightAvailability = MetricAvailability.unknown;
  var sendRateAvailability = MetricAvailability.unknown;
  while (!reader.isDone) {
    final field = reader.field();
    switch (field.number) {
      case 4:
        smoothedRtt = reader.varint(field);
      case 5:
        smoothedRttKnown = reader.varint(field) != 0;
      case 13:
        minimumRtt = reader.varint(field);
      case 14:
        minimumRttKnown = reader.varint(field) != 0;
      case 15:
        rttVariance = reader.varint(field);
      case 16:
        rttVarianceKnown = reader.varint(field) != 0;
      case 17:
        intervalLoss = reader.varint(field);
      case 18:
        intervalLossKnown = reader.varint(field) != 0;
      case 19:
        congestionWindow = reader.varint(field);
      case 20:
        congestionWindowKnown = reader.varint(field) != 0;
      case 21:
        bytesInFlight = reader.varint(field);
      case 22:
        bytesInFlightKnown = reader.varint(field) != 0;
      case 23:
        sendRate = reader.varint(field);
      case 24:
        sendRateKnown = reader.varint(field) != 0;
      case 25:
        packetsLost = reader.varint(field);
      case 26:
        bytesLost = reader.varint(field);
      case 27:
        tunSinkDrops = reader.varint(field);
      case 28:
        quicDatagramDrops = reader.varint(field);
      case 29:
        queueOldestAge = reader.varint(field);
      case 30:
        queueOldestAgeKnown = reader.varint(field) != 0;
      case 31:
        currentPmtu = reader.varint(field);
      case 32:
        currentPmtuKnown = reader.varint(field) != 0;
      case 33:
        migrationAttempts = reader.varint(field);
      case 34:
        migrationSuccesses = reader.varint(field);
      case 35:
        migrationFailures = reader.varint(field);
      case 36:
        lastMigrationDuration = reader.varint(field);
      case 37:
        lastMigrationDurationKnown = reader.varint(field) != 0;
      case 38:
        udpSendSyscalls = reader.varint(field);
      case 39:
        udpRecvSyscalls = reader.varint(field);
      case 40:
        udpDatagramsSent = reader.varint(field);
      case 41:
        udpDatagramsReceived = reader.varint(field);
      case 42:
        poolHits = reader.varint(field);
      case 43:
        poolMisses = reader.varint(field);
      case 44:
        h2StallCount = reader.varint(field);
      case 45:
        h2StallTotal = reader.varint(field);
      case 46:
        h2StallMax = reader.varint(field);
      case 47:
        h2StreamWindow = reader.varint(field);
      case 48:
        h2ConnectionWindow = reader.varint(field);
      case 49:
        dnsSuccesses = reader.varint(field);
      case 50:
        dnsFailures = reader.varint(field);
      case 51:
        dnsTimeouts = reader.varint(field);
      case 52:
        dnsLastRtt = reader.varint(field);
      case 53:
        dnsLastRttKnown = reader.varint(field) != 0;
      case 54:
        pmtuChanges = reader.varint(field);
      case 55:
        pmtuRevalidationFailures = reader.varint(field);
      case 56:
        smoothedAvailability = _decodeMetricAvailability(reader.varint(field));
      case 57:
        minimumAvailability = _decodeMetricAvailability(reader.varint(field));
      case 58:
        varianceAvailability = _decodeMetricAvailability(reader.varint(field));
      case 59:
        lossAvailability = _decodeMetricAvailability(reader.varint(field));
      case 60:
        congestionAvailability = _decodeMetricAvailability(
          reader.varint(field),
        );
      case 61:
        bytesInFlightAvailability = _decodeMetricAvailability(
          reader.varint(field),
        );
      case 62:
        sendRateAvailability = _decodeMetricAvailability(reader.varint(field));
      case 63:
        pmtuSendTooLarge = reader.varint(field);
      case 64:
        latestRtt = reader.varint(field);
      case 65:
        latestRttKnown = reader.varint(field) != 0;
      case 66:
        latestAvailability = _decodeMetricAvailability(reader.varint(field));
      default:
        reader.skip(field);
    }
  }
  return NetworkConnectionMetrics(
    latestRttMilliseconds: latestRttKnown ? latestRtt : null,
    latestRttAvailability: latestAvailability,
    smoothedRttMilliseconds: smoothedRttKnown ? smoothedRtt : null,
    minimumRttMilliseconds: minimumRttKnown ? minimumRtt : null,
    rttVarianceMilliseconds: rttVarianceKnown ? rttVariance : null,
    intervalLossBasisPoints: intervalLossKnown ? intervalLoss : null,
    congestionWindowBytes: congestionWindowKnown ? congestionWindow : null,
    bytesInFlight: bytesInFlightKnown ? bytesInFlight : null,
    sendRateBitsPerSecond: sendRateKnown ? sendRate : null,
    packetsLost: packetsLost,
    bytesLost: bytesLost,
    tunSinkDropCount: tunSinkDrops,
    quicDatagramDropCount: quicDatagramDrops,
    queueOldestAgeMilliseconds: queueOldestAgeKnown ? queueOldestAge : null,
    currentPmtuBytes: currentPmtuKnown ? currentPmtu : null,
    migrationAttemptCount: migrationAttempts,
    migrationSuccessCount: migrationSuccesses,
    migrationFailureCount: migrationFailures,
    lastMigrationDurationMilliseconds: lastMigrationDurationKnown
        ? lastMigrationDuration
        : null,
    udpSendSyscallCount: udpSendSyscalls,
    udpRecvSyscallCount: udpRecvSyscalls,
    udpDatagramSentCount: udpDatagramsSent,
    udpDatagramReceivedCount: udpDatagramsReceived,
    packetBufferPoolHitCount: poolHits,
    packetBufferPoolMissCount: poolMisses,
    h2FlowControlStallCount: h2StallCount,
    h2FlowControlStallTotalMilliseconds: h2StallTotal,
    h2FlowControlStallMaxMilliseconds: h2StallMax,
    h2StreamReceiveWindowBytes: h2StreamWindow,
    h2ConnectionReceiveWindowBytes: h2ConnectionWindow,
    directDnsSuccessCount: dnsSuccesses,
    directDnsFailureCount: dnsFailures,
    directDnsTimeoutCount: dnsTimeouts,
    directDnsLastRttMilliseconds: dnsLastRttKnown ? dnsLastRtt : null,
    pmtuChangeCount: pmtuChanges,
    pmtuRevalidationFailureCount: pmtuRevalidationFailures,
    pmtuSendTooLargeCount: pmtuSendTooLarge,
    smoothedRttAvailability: smoothedAvailability,
    minimumRttAvailability: minimumAvailability,
    rttVarianceAvailability: varianceAvailability,
    intervalLossAvailability: lossAvailability,
    congestionWindowAvailability: congestionAvailability,
    bytesInFlightAvailability: bytesInFlightAvailability,
    sendRateAvailability: sendRateAvailability,
  );
}

NetworkQueueQuality _decodeNetworkQueueQuality(_ProtoReader reader) {
  var kind = NetworkQueueKind.unknown;
  var availability = MetricAvailability.unknown;
  var currentItems = 0;
  var capacityItems = 0;
  var currentBytes = 0;
  var capacityBytes = 0;
  var highWaterItems = 0;
  var highWaterBytes = 0;
  var dropItems = 0;
  var dropBytes = 0;
  var oldestAge = 0;
  var oldestAgeKnown = false;
  var enqueueCount = 0;
  var dequeueCount = 0;
  var closed = false;
  var cancelled = false;
  while (!reader.isDone) {
    final field = reader.field();
    switch (field.number) {
      case 1:
        kind = _decodeNetworkQueueKind(reader.varint(field));
      case 2:
        currentItems = reader.varint(field);
      case 3:
        capacityItems = reader.varint(field);
      case 4:
        currentBytes = reader.varint(field);
      case 5:
        capacityBytes = reader.varint(field);
      case 6:
        highWaterItems = reader.varint(field);
      case 7:
        highWaterBytes = reader.varint(field);
      case 8:
        dropItems = reader.varint(field);
      case 9:
        dropBytes = reader.varint(field);
      case 10:
        oldestAge = reader.varint(field);
      case 11:
        oldestAgeKnown = reader.varint(field) != 0;
      case 12:
        availability = _decodeMetricAvailability(reader.varint(field));
      case 13:
        enqueueCount = reader.varint(field);
      case 14:
        dequeueCount = reader.varint(field);
      case 15:
        closed = reader.varint(field) != 0;
      case 16:
        cancelled = reader.varint(field) != 0;
      default:
        reader.skip(field);
    }
  }
  return NetworkQueueQuality(
    kind: kind,
    availability: availability,
    currentItems: currentItems,
    capacityItems: capacityItems,
    currentBytes: currentBytes,
    capacityBytes: capacityBytes,
    highWaterItems: highWaterItems,
    highWaterBytes: highWaterBytes,
    dropItems: dropItems,
    dropBytes: dropBytes,
    oldestAgeMilliseconds: oldestAgeKnown ? oldestAge : null,
    enqueueCount: enqueueCount,
    dequeueCount: dequeueCount,
    closed: closed,
    cancelled: cancelled,
  );
}

PmtuQualityInfo _decodePmtuQuality(_ProtoReader reader) {
  var availability = MetricAvailability.unknown;
  var outerPmtu = 0;
  var effectivePayload = 0;
  var effectiveAvailability = MetricAvailability.unknown;
  var phaseCode = '';
  var changeCount = 0;
  var revalidationFailures = 0;
  var sendTooLarge = 0;
  while (!reader.isDone) {
    final field = reader.field();
    switch (field.number) {
      case 1:
        availability = _decodeMetricAvailability(reader.varint(field));
      case 2:
        outerPmtu = reader.varint(field);
      case 3:
        effectivePayload = reader.varint(field);
      case 4:
        phaseCode = reader.string(field);
      case 5:
        changeCount = reader.varint(field);
      case 6:
        revalidationFailures = reader.varint(field);
      case 7:
        effectiveAvailability = _decodeMetricAvailability(reader.varint(field));
      case 8:
        sendTooLarge = reader.varint(field);
      default:
        reader.skip(field);
    }
  }
  return PmtuQualityInfo(
    availability: availability,
    outerPmtuBytes: _availabilityHasValue(availability) ? outerPmtu : null,
    effectiveConnectIpPayloadBytes: _availabilityHasValue(effectiveAvailability)
        ? effectivePayload
        : null,
    effectivePayloadAvailability: effectiveAvailability,
    phaseCode: phaseCode,
    changeCount: changeCount,
    revalidationFailureCount: revalidationFailures,
    sendTooLargeCount: sendTooLarge,
  );
}

MigrationQualityInfo _decodeMigrationQuality(_ProtoReader reader) {
  var phaseCode = '';
  var attempts = 0;
  var successes = 0;
  var failures = 0;
  var lastDuration = 0;
  var lastDurationKnown = false;
  var lastReasonCode = '';
  while (!reader.isDone) {
    final field = reader.field();
    switch (field.number) {
      case 1:
        phaseCode = reader.string(field);
      case 2:
        attempts = reader.varint(field);
      case 3:
        successes = reader.varint(field);
      case 4:
        failures = reader.varint(field);
      case 5:
        lastDuration = reader.varint(field);
      case 6:
        lastDurationKnown = reader.varint(field) != 0;
      case 7:
        lastReasonCode = reader.string(field);
      default:
        reader.skip(field);
    }
  }
  return MigrationQualityInfo(
    phaseCode: phaseCode,
    attemptCount: attempts,
    successCount: successes,
    failureCount: failures,
    lastDurationMilliseconds: lastDurationKnown ? lastDuration : null,
    lastReasonCode: lastReasonCode,
  );
}

DirectDnsQualityInfo _decodeDirectDnsQuality(_ProtoReader reader) {
  var mode = DirectDnsMode.unknown;
  var phaseCode = '';
  var successes = 0;
  var failures = 0;
  var timeouts = 0;
  var lastRtt = 0;
  var lastRttKnown = false;
  var lastReasonCode = '';
  while (!reader.isDone) {
    final field = reader.field();
    switch (field.number) {
      case 1:
        mode = _decodeDirectDnsMode(reader.varint(field));
      case 2:
        phaseCode = reader.string(field);
      case 3:
        successes = reader.varint(field);
      case 4:
        failures = reader.varint(field);
      case 5:
        timeouts = reader.varint(field);
      case 6:
        lastRtt = reader.varint(field);
      case 7:
        lastRttKnown = reader.varint(field) != 0;
      case 8:
        lastReasonCode = reader.string(field);
      default:
        reader.skip(field);
    }
  }
  return DirectDnsQualityInfo(
    mode: mode,
    phaseCode: phaseCode,
    successCount: successes,
    failureCount: failures,
    timeoutCount: timeouts,
    lastRttMilliseconds: lastRttKnown ? lastRtt : null,
    lastReasonCode: lastReasonCode,
  );
}

MetricAvailability _decodeMetricAvailability(int value) => switch (value) {
  1 => MetricAvailability.available,
  2 => MetricAvailability.unsupported,
  3 => MetricAvailability.notReady,
  4 => MetricAvailability.stale,
  _ => MetricAvailability.unknown,
};

bool _availabilityHasValue(MetricAvailability availability) =>
    availability == MetricAvailability.available ||
    availability == MetricAvailability.stale;

NetworkQualityLevel _decodeNetworkQualityLevel(int value) => switch (value) {
  1 => NetworkQualityLevel.good,
  2 => NetworkQualityLevel.fair,
  3 => NetworkQualityLevel.poor,
  4 => NetworkQualityLevel.limitedData,
  5 => NetworkQualityLevel.disconnected,
  _ => NetworkQualityLevel.unknown,
};

NetworkQueueKind _decodeNetworkQueueKind(int value) => switch (value) {
  1 => NetworkQueueKind.tunToTransport,
  2 => NetworkQueueKind.proxyToTransport,
  3 => NetworkQueueKind.transportOutgoing,
  4 => NetworkQueueKind.h3DatagramSend,
  5 => NetworkQueueKind.h3WireSend,
  6 => NetworkQueueKind.transportToTun,
  7 => NetworkQueueKind.transportToProxy,
  8 => NetworkQueueKind.directDns,
  _ => NetworkQueueKind.unknown,
};

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
  UpdatePackage? package;
  while (!reader.isDone) {
    final field = reader.field();
    switch (field.number) {
      case 1:
        available = reader.varint(field) != 0;
      case 2:
        version = _emptyToNull(reader.string(field));
      case 3:
        releaseUrl = _emptyToNull(reader.string(field));
      case 4:
        package = _decodeUpdatePackage(reader.message(field));
      default:
        reader.skip(field);
    }
  }
  return UpdateCheckResult(
    available: available,
    version: version,
    releaseUrl: releaseUrl,
    package: package,
  );
}

UpdatePackage _decodeUpdatePackage(_ProtoReader reader) {
  var name = '';
  var downloadUrl = '';
  var size = 0;
  var sha256 = '';
  var platform = '';
  var variant = '';
  while (!reader.isDone) {
    final field = reader.field();
    switch (field.number) {
      case 1:
        name = reader.string(field);
      case 2:
        downloadUrl = reader.string(field);
      case 3:
        size = reader.varint(field);
      case 4:
        sha256 = reader.string(field);
      case 5:
        platform = reader.string(field);
      case 6:
        variant = reader.string(field);
      default:
        reader.skip(field);
    }
  }
  return UpdatePackage(
    name: name,
    downloadUrl: downloadUrl,
    size: size,
    sha256: sha256,
    platform: platform,
    variant: variant,
  );
}

_StructuredEngineError _decodeError(_ProtoReader reader) {
  var code = 'ENGINE_ERROR';
  var message = 'The local Engine rejected this operation.';
  var retryable = false;
  while (!reader.isDone) {
    final field = reader.field();
    switch (field.number) {
      case 1:
        code = reader.string(field);
      case 2:
        message = reader.string(field);
      case 3:
        retryable = reader.varint(field) != 0;
      default:
        reader.skip(field);
    }
  }
  return _StructuredEngineError(code, message, retryable);
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
  String? errorCode;
  bool? errorRetryable;
  var reconnectCount = 0;
  String? killSwitchState;
  var platformLockdown = false;
  final activeListeners = <String>[];
  final frontends = <FrontendRuntimeStatus>[];
  TransportFailureInfo? failure;
  NetworkQualitySnapshot? networkQuality;
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
        final error = _decodeError(reader.message(field));
        warning = error.message;
        errorCode = error.code;
        errorRetryable = error.retryable;
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
      case 16:
        failure = _decodeTransportFailure(reader.message(field));
      case 17:
        networkQuality = _decodeNetworkQuality(reader.message(field));
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
    errorCode: failure?.code ?? errorCode,
    errorRetryable: failure?.retryable ?? errorRetryable,
    failure: failure,
    networkQuality: networkQuality,
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
  const _StructuredEngineError(this.code, this.message, this.retryable);

  final String code;
  final String message;
  final bool retryable;
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
