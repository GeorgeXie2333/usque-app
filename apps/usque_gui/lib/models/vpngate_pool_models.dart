part of 'vpngate_models.dart';

class VpnGatePoolMetadata {
  const VpnGatePoolMetadata({
    this.firstSeenAt,
    this.lastSeenAt,
    this.present = false,
    this.tcpStatus = 'unknown',
    this.checkedAt,
    this.connectMs,
    this.inPool = true,
  });
  final DateTime? firstSeenAt, lastSeenAt, checkedAt;
  final bool present, inPool;
  final String tcpStatus;
  final int? connectMs;
  factory VpnGatePoolMetadata.fromMap(Map<Object?, Object?> map) =>
      VpnGatePoolMetadata(
        firstSeenAt: DateTime.tryParse(map['first_seen_at'] as String? ?? ''),
        lastSeenAt: DateTime.tryParse(map['last_seen_at'] as String? ?? ''),
        present: map['present_in_latest_source'] == true,
        tcpStatus: map['tcp_status'] as String? ?? 'unknown',
        checkedAt: DateTime.tryParse(map['tcp_checked_at'] as String? ?? ''),
        connectMs: (map['tcp_connect_ms'] as num?)?.toInt(),
        inPool: map['in_pool'] == true,
      );
  @override
  bool operator ==(Object other) =>
      other is VpnGatePoolMetadata &&
      firstSeenAt == other.firstSeenAt &&
      lastSeenAt == other.lastSeenAt &&
      present == other.present &&
      tcpStatus == other.tcpStatus &&
      checkedAt == other.checkedAt &&
      connectMs == other.connectMs &&
      inPool == other.inPool;
  @override
  int get hashCode => Object.hash(
    firstSeenAt,
    lastSeenAt,
    present,
    tcpStatus,
    checkedAt,
    connectMs,
    inPool,
  );
}

class VpnGateFavoriteMetadata {
  const VpnGateFavoriteMetadata({
    required this.configSha256,
    required this.savedAt,
    this.latestConfigSha256,
  });
  final String configSha256;
  final DateTime savedAt;
  final String? latestConfigSha256;
  factory VpnGateFavoriteMetadata.fromMap(Map<Object?, Object?> map) =>
      VpnGateFavoriteMetadata(
        configSha256: map['config_sha256'] as String? ?? '',
        savedAt: DateTime.fromMillisecondsSinceEpoch(
          (map['saved_at_unix_ms'] as num?)?.toInt() ?? 0,
        ),
        latestConfigSha256: _text(map['latest_config_sha256']),
      );
  @override
  bool operator ==(Object other) =>
      other is VpnGateFavoriteMetadata &&
      configSha256 == other.configSha256 &&
      savedAt == other.savedAt &&
      latestConfigSha256 == other.latestConfigSha256;
  @override
  int get hashCode => Object.hash(configSha256, savedAt, latestConfigSha256);
}

class VpnGateNodeRequest {
  const VpnGateNodeRequest({
    required this.operationId,
    required this.action,
    this.serverId = '',
    this.configSha256 = '',
    this.expectedFavoriteHash = '',
  });
  final String operationId,
      action,
      serverId,
      configSha256,
      expectedFavoriteHash;
  Map<String, Object?> toMap() => {
    'operation_id': operationId,
    'action': action,
    'server_id': serverId,
    'config_sha256': configSha256,
    'expected_favorite_hash': expectedFavoriteHash,
  };
}

class VpnGateNodeProgress {
  const VpnGateNodeProgress({
    this.operationId = '',
    this.serverId = '',
    this.configSha256 = '',
    this.stage = '',
    this.error,
  });
  final String operationId, serverId, configSha256, stage;
  final String? error;
  bool get running => stage == 'preparing';
  factory VpnGateNodeProgress.fromMap(Map<Object?, Object?> map) =>
      VpnGateNodeProgress(
        operationId: map['operation_id'] as String? ?? '',
        serverId: map['server_id'] as String? ?? '',
        configSha256: map['config_sha256'] as String? ?? '',
        stage: map['stage'] as String? ?? '',
        error: _text(map['error']),
      );
}
