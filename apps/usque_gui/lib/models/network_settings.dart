import 'package:flutter/foundation.dart';

import 'app_models.dart';

enum NetworkSettingsApplyStatus {
  notRequired,
  applying,
  applied,
  deferred,
  failed,
  unknown;

  static NetworkSettingsApplyStatus parse(Object? value) => switch (value) {
    'not_required' => notRequired,
    'applying' => applying,
    'applied' => applied,
    'deferred' => deferred,
    'failed' => failed,
    _ => unknown,
  };
}

class NetworkSettingsState {
  const NetworkSettingsState({
    required this.sourceEpoch,
    required this.sequence,
    this.operationId,
    this.sessionId,
    this.storedProfile,
    this.appliedProfile,
    this.status = NetworkSettingsApplyStatus.unknown,
    this.deferredFields = const [],
    this.errorCode,
    this.persisted,
  });

  factory NetworkSettingsState.fromMap(Map<Object?, Object?> map) {
    final epoch = map['source_epoch'];
    final sequence = map['sequence'];
    if (epoch is! String || epoch.isEmpty || sequence is! int || sequence < 0) {
      throw const FormatException('Invalid network settings state identity');
    }
    UsqueProfile? profile(String key) => map[key] is Map
        ? UsqueProfile.fromMap(Map<String, Object?>.from(map[key] as Map))
        : null;
    return NetworkSettingsState(
      sourceEpoch: epoch,
      sequence: sequence,
      operationId: map['operation_id'] as String?,
      sessionId: map['session_id'] as String?,
      storedProfile: profile('stored_profile'),
      appliedProfile: profile('applied_profile'),
      status: NetworkSettingsApplyStatus.parse(map['apply_status']),
      deferredFields:
          (map['deferred_fields'] as List?)?.cast<String>() ?? const [],
      errorCode: map['error_code'] as String?,
      persisted: map['persisted'] as bool?,
    );
  }

  final String sourceEpoch;
  final int sequence;
  final String? operationId;
  final String? sessionId;
  final UsqueProfile? storedProfile;
  final UsqueProfile? appliedProfile;
  final NetworkSettingsApplyStatus status;
  final List<String> deferredFields;
  final String? errorCode;
  final bool? persisted;
}

/// Computes an edit mask only. Runtime policy belongs to Rust.
List<String> networkSettingsChangedFields(
  UsqueProfile before,
  UsqueProfile after,
) {
  final a = _fields(before);
  final b = _fields(after);
  return a.keys
      .where((key) {
        final left = a[key];
        final right = b[key];
        return left is List && right is List
            ? !listEquals(left, right)
            : left != right;
      })
      .toList(growable: false);
}

Map<String, Object?> _fields(UsqueProfile p) => {
  'frontends.tunnel': p.frontends.tunnel,
  'frontends.socks5': p.frontends.socks5,
  'frontends.http': p.frontends.http,
  'transport': p.transport,
  'data_plane': p.dataPlane,
  'congestion_control': p.congestionControl,
  'endpoint.ipv4': p.endpointIpv4,
  'endpoint.ipv6': p.endpointIpv6,
  'endpoint.port': p.endpointPort,
  'endpoint.sni': p.sni,
  'ip_policy': p.ipPolicy,
  'mtu': p.mtu,
  'dns_mode': p.dnsMode,
  'dns_servers': [p.dnsIpv4, p.dnsIpv6],
  'allow_lan': p.allowLan,
  'split_exclusions': p.bypassCidrs,
  'kill_switch': p.killSwitch,
  'auto_connect': p.autoConnect,
  'geo_direct_countries': p.geoDirectCountries,
  'direct_dns': p.directDns,
  'proxy.socks5_listeners': [
    p.proxy.socksIpv4,
    p.proxy.socksIpv6,
    p.proxy.socksPort,
  ],
  'proxy.http_listeners': [
    p.proxy.httpIpv4,
    p.proxy.httpIpv6,
    p.proxy.httpPort,
  ],
  'proxy.system_proxy': p.proxy.systemProxy,
  'proxy.dns_mode': p.proxy.dnsMode,
  'proxy.dns_servers': [p.proxy.dnsIpv4, p.proxy.dnsIpv6],
};
