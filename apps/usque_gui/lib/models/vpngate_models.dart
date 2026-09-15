import 'package:flutter/foundation.dart';
part 'vpngate_pool_models.dart';

class VpnGateSettings {
  const VpnGateSettings({
    this.enabled = false,
    this.serverId = '',
    this.configSha256 = '',
  });
  final bool enabled;
  final String serverId, configSha256;
  bool get hasSelection => serverId.isNotEmpty && configSha256.isNotEmpty;
  VpnGateSettings copyWith({bool? enabled, VpnGateServer? server}) =>
      VpnGateSettings(
        enabled: enabled ?? this.enabled,
        serverId: server?.id ?? serverId,
        configSha256: server?.configSha256 ?? configSha256,
      );
  factory VpnGateSettings.fromMap(Map<Object?, Object?> map) {
    final selection = map['selection'] is Map ? map['selection'] as Map : map;
    return VpnGateSettings(
      enabled: map['enabled'] == true,
      serverId: selection['server_id'] as String? ?? '',
      configSha256: selection['config_sha256'] as String? ?? '',
    );
  }
  Map<String, Object?> toMap() => {
    'enabled': enabled,
    'selection': hasSelection
        ? {'server_id': serverId, 'config_sha256': configSha256}
        : null,
  };
  @override
  bool operator ==(Object other) =>
      other is VpnGateSettings &&
      enabled == other.enabled &&
      serverId == other.serverId &&
      configSha256 == other.configSha256;
  @override
  int get hashCode => Object.hash(enabled, serverId, configSha256);
}

class VpnGateServer {
  const VpnGateServer({
    required this.id,
    required this.ip,
    required this.hostname,
    required this.configSha256,
    this.countryCode,
    this.countryName,
    this.score,
    this.pingMs,
    this.speedBps,
    this.sessions,
    this.unsupportedReason,
    this.pool,
    this.favorite,
  });
  final String id, ip, hostname, configSha256;
  final String? countryCode, countryName, unsupportedReason;
  final int? score, pingMs, speedBps, sessions;
  final VpnGatePoolMetadata? pool;
  final VpnGateFavoriteMetadata? favorite;
  factory VpnGateServer.fromMap(Map<Object?, Object?> map) => VpnGateServer(
    id: map['id'] as String? ?? '',
    ip: map['ip'] as String? ?? '',
    hostname: map['hostname'] as String? ?? '',
    configSha256: map['config_sha256'] as String? ?? '',
    countryCode: _text(map['country_code']),
    countryName: _text(map['country_name']),
    score: (map['score'] as num?)?.toInt(),
    pingMs: (map['ping_ms'] as num?)?.toInt(),
    speedBps: (map['speed_bps'] as num?)?.toInt(),
    sessions: (map['num_vpn_sessions'] as num?)?.toInt(),
    unsupportedReason: _text(map['unsupported_reason']),
    pool: map['pool'] is Map
        ? VpnGatePoolMetadata.fromMap(map['pool'] as Map)
        : null,
    favorite: map['favorite'] is Map
        ? VpnGateFavoriteMetadata.fromMap(map['favorite'] as Map)
        : null,
  );
  bool matches(VpnGateSettings settings) =>
      id == settings.serverId && configSha256 == settings.configSha256;
  @override
  bool operator ==(Object other) =>
      other is VpnGateServer &&
      id == other.id &&
      ip == other.ip &&
      hostname == other.hostname &&
      configSha256 == other.configSha256 &&
      countryCode == other.countryCode &&
      countryName == other.countryName &&
      score == other.score &&
      pingMs == other.pingMs &&
      speedBps == other.speedBps &&
      sessions == other.sessions &&
      unsupportedReason == other.unsupportedReason &&
      pool == other.pool &&
      favorite == other.favorite;
  @override
  int get hashCode => Object.hash(
    id,
    ip,
    hostname,
    configSha256,
    countryCode,
    countryName,
    score,
    pingMs,
    speedBps,
    sessions,
    unsupportedReason,
    pool,
    favorite,
  );
}

class VpnGateCountry {
  const VpnGateCountry({this.code, this.name, required this.count});
  final String? code, name;
  final int count;
  factory VpnGateCountry.fromMap(Map<Object?, Object?> map) => VpnGateCountry(
    code: _text(map['country_code']),
    name: _text(map['country_name']),
    count: (map['server_count'] as num?)?.toInt() ?? 0,
  );
}

class VpnGateNetwork {
  const VpnGateNetwork({
    this.ipv4,
    this.ipv6,
    this.dnsServers = const [],
    this.mtu = 0,
  });
  final String? ipv4, ipv6;
  final List<String> dnsServers;
  final int mtu;
  factory VpnGateNetwork.fromMap(Map<Object?, Object?> map) => VpnGateNetwork(
    ipv4: _text(map['ipv4']),
    ipv6: _text(map['ipv6']),
    dnsServers: List<String>.unmodifiable(
      (map['dns_servers'] as List?)?.cast<String>() ?? const [],
    ),
    mtu: (map['mtu'] as num?)?.toInt() ?? 0,
  );
  @override
  bool operator ==(Object other) =>
      other is VpnGateNetwork &&
      ipv4 == other.ipv4 &&
      ipv6 == other.ipv6 &&
      mtu == other.mtu &&
      listEquals(dnsServers, other.dnsServers);
  @override
  int get hashCode => Object.hash(ipv4, ipv6, mtu, Object.hashAll(dnsServers));
}

class VpnGateStatus {
  const VpnGateStatus({
    this.stage = 'disabled',
    this.generation = 0,
    this.server,
    this.network,
    this.failure,
    this.warpStage,
  });
  final String stage;
  final int generation;
  final VpnGateServer? server;
  final VpnGateNetwork? network;
  final String? failure, warpStage;
  bool get connected => stage == 'connected';
  factory VpnGateStatus.fromMap(Map<Object?, Object?> map) => VpnGateStatus(
    stage: map['stage'] as String? ?? 'disabled',
    generation: (map['generation'] as num?)?.toInt() ?? 0,
    server: map['current_server'] is Map
        ? VpnGateServer.fromMap(map['current_server'] as Map)
        : null,
    network: map['network'] is Map
        ? VpnGateNetwork.fromMap(map['network'] as Map)
        : null,
    failure: _text(map['failure']),
    warpStage: _text(map['warp_stage']),
  );
  @override
  bool operator ==(Object other) =>
      other is VpnGateStatus &&
      stage == other.stage &&
      generation == other.generation &&
      server == other.server &&
      network == other.network &&
      failure == other.failure &&
      warpStage == other.warpStage;
  @override
  int get hashCode =>
      Object.hash(stage, generation, server, network, failure, warpStage);
}

class VpnGateDirectory {
  const VpnGateDirectory({
    this.servers = const [],
    this.countries = const [],
    this.total = 0,
    this.sourceCount = 0,
    this.fetchedAt,
    this.sourceUrl,
    this.refreshStage = 'idle',
    this.failures = const [],
    this.cached = false,
    this.status = const VpnGateStatus(),
    this.savedServer,
    this.favoriteCount = 0,
    this.sourceFetchedAt,
    this.nodeProgress = const VpnGateNodeProgress(),
  });
  final List<VpnGateServer> servers;
  final List<VpnGateCountry> countries;
  final int total, sourceCount;
  final DateTime? fetchedAt;
  final String? sourceUrl;
  final String refreshStage;
  final List<String> failures;
  final bool cached;
  final VpnGateStatus status;
  final VpnGateServer? savedServer;
  final int favoriteCount;
  final DateTime? sourceFetchedAt;
  final VpnGateNodeProgress nodeProgress;
  bool get refreshing =>
      !const ['idle', 'complete', 'failed', 'cancelled'].contains(refreshStage);
  factory VpnGateDirectory.fromMap(Map<Object?, Object?> map) =>
      VpnGateDirectory(
        favoriteCount: (map['favorite_count'] as num?)?.toInt() ?? 0,
        sourceFetchedAt: DateTime.tryParse(
          map['source_fetched_at'] as String? ?? '',
        ),
        nodeProgress: map['node_progress'] is Map
            ? VpnGateNodeProgress.fromMap(map['node_progress'] as Map)
            : const VpnGateNodeProgress(),
        servers: List<VpnGateServer>.unmodifiable(
          (map['servers'] as List? ?? const []).map(
            (e) => VpnGateServer.fromMap(e as Map),
          ),
        ),
        countries: List<VpnGateCountry>.unmodifiable(
          (map['countries'] as List? ?? const []).map(
            (e) => VpnGateCountry.fromMap(e as Map),
          ),
        ),
        total: (map['total'] as num?)?.toInt() ?? 0,
        sourceCount: (map['source_server_count'] as num?)?.toInt() ?? 0,
        fetchedAt: map['fetched_at_unix_ms'] is num
            ? DateTime.fromMillisecondsSinceEpoch(
                (map['fetched_at_unix_ms'] as num).toInt(),
                isUtc: true,
              )
            : null,
        sourceUrl: _text(map['source_url']),
        refreshStage: map['refresh_stage'] as String? ?? 'idle',
        failures: List<String>.unmodifiable(
          (map['refresh_failures'] as List?)?.cast<String>() ?? const [],
        ),
        cached: map['cached'] == true,
        status: map['status'] is Map
            ? VpnGateStatus.fromMap(map['status'] as Map)
            : const VpnGateStatus(),
        savedServer: map['saved_server'] is Map
            ? VpnGateServer.fromMap(map['saved_server'] as Map)
            : null,
      );
}

String? _text(Object? value) =>
    value is String && value.isNotEmpty ? value : null;
