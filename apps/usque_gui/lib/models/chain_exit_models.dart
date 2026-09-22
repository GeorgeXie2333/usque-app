import 'package:flutter/foundation.dart';

enum ChainSource {
  openvpnCustom('openvpn_custom', 'OpenVPN (Custom)'),
  wireguardCustom('wireguard_custom', 'WireGuard (Custom)'),
  vpnGate('vpn_gate', 'VPN Gate');

  const ChainSource(this.wire, this.label);
  final String wire;
  final String label;
  static ChainSource parse(Object? value) => ChainSource.values.firstWhere(
    (source) => source.wire == value,
    orElse: () => throw const FormatException('Unknown chain source'),
  );
}

@immutable
class ChainExitSettings {
  const ChainExitSettings({
    this.enabled = false,
    this.source = ChainSource.openvpnCustom,
    this.profileId,
    this.revision,
  });
  final bool enabled;
  final ChainSource source;
  final String? profileId;
  final String? revision;
  factory ChainExitSettings.fromMap(Map<Object?, Object?> map) =>
      ChainExitSettings(
        enabled: map['enabled'] == true,
        source: ChainSource.parse(map['source'] ?? 'openvpn_custom'),
        profileId: _reference(map['profile_id']),
        revision: _reference(map['revision']),
      );
  static String? _reference(Object? value) =>
      value is String && value.isNotEmpty ? value : null;
  Map<String, Object?> toMap() => {
    'enabled': enabled,
    'source': source.wire,
    'profile_id': profileId,
    'revision': revision,
  };
  ChainExitSettings copyWith({
    bool? enabled,
    String? profileId,
    String? revision,
    bool clearSelection = false,
  }) => ChainExitSettings(
    enabled: enabled ?? this.enabled,
    source: source,
    profileId: clearSelection ? null : profileId ?? this.profileId,
    revision: clearSelection ? null : revision ?? this.revision,
  );
  @override
  bool operator ==(Object other) =>
      other is ChainExitSettings &&
      enabled == other.enabled &&
      source == other.source &&
      profileId == other.profileId &&
      revision == other.revision;
  @override
  int get hashCode => Object.hash(enabled, source, profileId, revision);
}

@immutable
class ChainEndpoint {
  const ChainEndpoint(this.host, this.port, {this.ipv6});
  final String host;
  final int port;
  final bool? ipv6;
  String get label => '${host.contains(':') ? '[$host]' : host}:$port';
  factory ChainEndpoint.fromMap(Map<Object?, Object?> map) =>
      ChainEndpoint(map['host'] as String, map['port'] as int);
  @override
  bool operator ==(Object other) =>
      other is ChainEndpoint &&
      host == other.host &&
      port == other.port &&
      ipv6 == other.ipv6;
  @override
  int get hashCode => Object.hash(host, port, ipv6);
}

@immutable
class ChainProfileSummary {
  const ChainProfileSummary({
    required this.id,
    required this.revision,
    required this.editRevision,
    required this.name,
    required this.protocol,
    required this.host,
    required this.port,
    this.addresses = const [],
    this.dns = const [],
    this.allowedIps = const [],
    this.mtu,
    this.requiresAuth = false,
    this.requiresKeyPassword = false,
    this.addressFamily = 'IPv4/IPv6',
    this.candidates = const [],
    this.remoteRandom = false,
  });
  final String id, revision, editRevision, name, protocol, host;
  final String addressFamily;
  final int port;
  final List<String> addresses, dns, allowedIps;
  final int? mtu;
  final bool requiresAuth, requiresKeyPassword;
  final List<ChainEndpoint> candidates;
  final bool remoteRandom;
  ChainSource get source => protocol == 'wireguard'
      ? ChainSource.wireguardCustom
      : ChainSource.openvpnCustom;
  bool get requiresUdp => protocol != 'openvpn_tcp';
  String get transportLabel => protocol == 'openvpn_tcp' ? 'TCP' : 'UDP';
  factory ChainProfileSummary.fromMap(Map<Object?, Object?> map) {
    final endpoint = map['endpoint'] as Map? ?? const {};
    return ChainProfileSummary(
      id: map['id'] as String,
      revision: map['revision'] as String,
      editRevision:
          map['edit_revision'] as String? ?? map['revision'] as String,
      name: map['name'] as String,
      protocol: map['protocol'] as String,
      host: endpoint['host'] as String,
      port: endpoint['port'] as int,
      addresses: (map['addresses'] as List? ?? const []).cast<String>(),
      dns: (map['dns_servers'] as List? ?? const []).cast<String>(),
      allowedIps: (map['allowed_ips'] as List? ?? const []).cast<String>(),
      mtu: map['mtu'] as int?,
      candidates: (map['candidates'] as List? ?? const [])
          .whereType<Map<Object?, Object?>>()
          .map((candidate) {
            final endpoint = candidate['endpoint'] as Map;
            return ChainEndpoint(
              endpoint['host'] as String,
              endpoint['port'] as int,
              ipv6: candidate['ipv6'] as bool?,
            );
          })
          .toList(growable: false),
      remoteRandom: map['remote_random'] == true,
      requiresAuth: map['requires_auth'] == true,
      requiresKeyPassword: map['requires_key_password'] == true,
      addressFamily: map['address_family'] as String? ?? 'IPv4/IPv6',
    );
  }
  @override
  bool operator ==(Object other) =>
      other is ChainProfileSummary &&
      id == other.id &&
      revision == other.revision &&
      editRevision == other.editRevision &&
      name == other.name;
  @override
  int get hashCode => Object.hash(id, revision, editRevision, name);
}

class ChainProfileResult {
  const ChainProfileResult({
    this.profiles = const [],
    this.preview,
    this.error,
  });
  final List<ChainProfileSummary> profiles;
  final ChainProfileSummary? preview;
  final Map<String, Object?>? error;
  factory ChainProfileResult.fromMap(Map<Object?, Object?> map) =>
      ChainProfileResult(
        profiles: (map['profiles'] as List? ?? const [])
            .whereType<Map<Object?, Object?>>()
            .map(ChainProfileSummary.fromMap)
            .toList(),
        preview: map['preview'] is Map
            ? ChainProfileSummary.fromMap(map['preview'] as Map)
            : null,
        error: map['error'] is Map
            ? Map<String, Object?>.from(map['error'] as Map)
            : null,
      );
}

@immutable
class ChainExitStatus {
  const ChainExitStatus({
    this.stage = 'disabled',
    this.generation = 0,
    this.currentProfile,
    this.failure,
    this.dnsUnavailable = false,
    this.attemptingEndpoint,
    this.activeEndpoint,
    this.attemptCount = 0,
    this.candidateCount = 0,
    this.attemptFailures = const [],
  });
  final String stage;
  final int generation;
  final ChainProfileSummary? currentProfile;
  final String? failure;
  final bool dnsUnavailable;
  final ChainEndpoint? attemptingEndpoint;
  final String? activeEndpoint;
  final int attemptCount, candidateCount;
  final List<String> attemptFailures;
  factory ChainExitStatus.fromMap(Map<Object?, Object?> map) => ChainExitStatus(
    stage: map['stage'] as String? ?? 'disabled',
    generation: map['generation'] as int? ?? 0,
    currentProfile: map['current_profile'] is Map
        ? ChainProfileSummary.fromMap(map['current_profile'] as Map)
        : null,
    failure: map['failure'] as String?,
    dnsUnavailable: map['dns_unavailable'] == true,
    attemptingEndpoint: map['attempting_endpoint'] is Map
        ? ChainEndpoint.fromMap(map['attempting_endpoint'] as Map)
        : null,
    activeEndpoint: map['active_endpoint'] as String?,
    attemptCount: map['attempt_count'] as int? ?? 0,
    candidateCount: map['candidate_count'] as int? ?? 0,
    attemptFailures: (map['attempt_failures'] as List? ?? const [])
        .cast<String>(),
  );
  @override
  bool operator ==(Object other) =>
      other is ChainExitStatus &&
      stage == other.stage &&
      generation == other.generation &&
      currentProfile == other.currentProfile &&
      failure == other.failure &&
      dnsUnavailable == other.dnsUnavailable &&
      attemptingEndpoint == other.attemptingEndpoint &&
      activeEndpoint == other.activeEndpoint &&
      attemptCount == other.attemptCount &&
      candidateCount == other.candidateCount &&
      listEquals(attemptFailures, other.attemptFailures);
  @override
  int get hashCode => Object.hash(
    stage,
    generation,
    currentProfile,
    failure,
    dnsUnavailable,
    attemptingEndpoint,
    activeEndpoint,
    attemptCount,
    candidateCount,
    Object.hashAll(attemptFailures),
  );
}
