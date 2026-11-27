import 'package:flutter/foundation.dart';

/// Account tier a subscription belongs to. Strictly single-select per
/// subscription and matched 1:1 against the active account.
enum EndpointSubscriptionTier {
  free('free', 'Free'),
  plus('plus', 'WARP+'),
  zeroTrust('zero_trust', 'ZT');

  const EndpointSubscriptionTier(this.wireName, this.shortBadge);

  final String wireName;
  final String shortBadge;

  static EndpointSubscriptionTier? parse(Object? value) => switch (value) {
    'free' => EndpointSubscriptionTier.free,
    'plus' => EndpointSubscriptionTier.plus,
    'zero_trust' => EndpointSubscriptionTier.zeroTrust,
    _ => null,
  };
}

enum SubscriptionRefreshInterval {
  off('off'),
  daily('daily'),
  weekly('weekly');

  const SubscriptionRefreshInterval(this.wireName);

  final String wireName;

  static SubscriptionRefreshInterval parse(Object? value) => switch (value) {
    'daily' => SubscriptionRefreshInterval.daily,
    'weekly' => SubscriptionRefreshInterval.weekly,
    _ => SubscriptionRefreshInterval.off,
  };
}

enum SubscriptionEndpointHostKind {
  ipv4('ipv4'),
  ipv6('ipv6'),
  domain('domain');

  const SubscriptionEndpointHostKind(this.wireName);

  final String wireName;

  static SubscriptionEndpointHostKind? parse(Object? value) => switch (value) {
    'ipv4' => SubscriptionEndpointHostKind.ipv4,
    'ipv6' => SubscriptionEndpointHostKind.ipv6,
    'domain' => SubscriptionEndpointHostKind.domain,
    _ => null,
  };
}

enum EndpointProbeStatus {
  reachable('reachable'),
  pinMismatch('pin_mismatch'),
  timedOut('timed_out'),
  cancelled('cancelled'),
  networkChanged('network_changed'),
  failed('failed');

  const EndpointProbeStatus(this.wireName);

  final String wireName;

  static EndpointProbeStatus parse(Object? value) => switch (value) {
    'reachable' => EndpointProbeStatus.reachable,
    'pin_mismatch' => EndpointProbeStatus.pinMismatch,
    'timed_out' => EndpointProbeStatus.timedOut,
    'cancelled' => EndpointProbeStatus.cancelled,
    'network_changed' => EndpointProbeStatus.networkChanged,
    _ => EndpointProbeStatus.failed,
  };
}

enum EndpointProbeProtocol {
  http3('http3', 'H3'),
  http2('http2', 'H2');

  const EndpointProbeProtocol(this.wireName, this.label);

  final String wireName;
  final String label;

  static EndpointProbeProtocol? parse(Object? value) => switch (value) {
    'http3' => EndpointProbeProtocol.http3,
    'http2' => EndpointProbeProtocol.http2,
    _ => null,
  };
}

enum EndpointProbeState {
  running('running'),
  completed('completed'),
  cancelled('cancelled'),
  failed('failed');

  const EndpointProbeState(this.wireName);

  final String wireName;

  static EndpointProbeState parse(Object? value) => switch (value) {
    'running' => EndpointProbeState.running,
    'completed' => EndpointProbeState.completed,
    'cancelled' => EndpointProbeState.cancelled,
    _ => EndpointProbeState.failed,
  };
}

@immutable
class SubscriptionEndpoint {
  const SubscriptionEndpoint({
    required this.id,
    required this.kind,
    required this.host,
    this.port,
    this.note = '',
    this.pinned = false,
  });

  factory SubscriptionEndpoint.fromMap(Map<Object?, Object?> map) {
    final hostMap = map['host'];
    if (hostMap is! Map) {
      throw const FormatException('Missing endpoint host');
    }
    final kind = SubscriptionEndpointHostKind.parse(hostMap['kind']);
    final host = hostMap['value'];
    final id = map['id'];
    if (kind == null ||
        host is! String ||
        host.isEmpty ||
        id is! String ||
        id.isEmpty) {
      throw const FormatException('Invalid subscription endpoint');
    }
    final rawPort = map['port'];
    final port = rawPort == null ? null : (rawPort as num).toInt();
    if (port != null && (port < 1 || port > 65535)) {
      throw const FormatException('Invalid subscription endpoint port');
    }
    return SubscriptionEndpoint(
      id: id,
      kind: kind,
      host: host,
      port: port,
      note: (map['note'] as String?) ?? '',
      pinned: map['pinned'] == true,
    );
  }

  final String id;
  final SubscriptionEndpointHostKind kind;
  final String host;
  final int? port;
  final String note;
  final bool pinned;

  int effectivePort(int defaultPort) => port ?? defaultPort;

  String addressLabel(int defaultPort) {
    final p = effectivePort(defaultPort);
    return kind == SubscriptionEndpointHostKind.ipv6
        ? '[$host]:$p'
        : '$host:$p';
  }

  String primaryLabel(int defaultPort) =>
      note.isNotEmpty ? note : addressLabel(defaultPort);
}

@immutable
class EndpointSubscription {
  const EndpointSubscription({
    required this.id,
    required this.revision,
    required this.name,
    required this.url,
    required this.accountTier,
    this.autoRefresh = SubscriptionRefreshInterval.off,
    this.lastRefreshedAtUnixMs,
    this.nextRefreshAtUnixMs,
    this.lastRefreshError,
    this.skippedLineCount = 0,
    this.endpoints = const <SubscriptionEndpoint>[],
    this.selectedEndpointId,
    this.retainedMissingEndpoint,
  });

  factory EndpointSubscription.fromMap(Map<Object?, Object?> map) {
    final id = map['id'];
    final revision = map['revision'];
    final name = map['name'];
    final url = map['url'];
    final tier = EndpointSubscriptionTier.parse(map['account_tier']);
    if (id is! String ||
        revision is! String ||
        name is! String ||
        url is! String ||
        tier == null) {
      throw const FormatException('Invalid endpoint subscription');
    }
    final rawEndpoints = map['endpoints'] as List? ?? const <Object>[];
    final endpoints = List<SubscriptionEndpoint>.unmodifiable(
      rawEndpoints.whereType<Map<Object?, Object?>>().map(
        SubscriptionEndpoint.fromMap,
      ),
    );
    final retainedRaw = map['retained_missing_endpoint'];
    return EndpointSubscription(
      id: id,
      revision: revision,
      name: name,
      url: url,
      accountTier: tier,
      autoRefresh: SubscriptionRefreshInterval.parse(map['auto_refresh']),
      lastRefreshedAtUnixMs: (map['last_refreshed_at_unix_ms'] as num?)
          ?.toInt(),
      nextRefreshAtUnixMs: (map['next_refresh_at_unix_ms'] as num?)?.toInt(),
      lastRefreshError: map['last_refresh_error'] as String?,
      skippedLineCount: (map['skipped_line_count'] as num?)?.toInt() ?? 0,
      endpoints: endpoints,
      selectedEndpointId: map['selected_endpoint_id'] as String?,
      retainedMissingEndpoint: retainedRaw is Map
          ? SubscriptionEndpoint.fromMap(retainedRaw)
          : null,
    );
  }

  final String id;
  final String revision;
  final String name;
  final String url;
  final EndpointSubscriptionTier accountTier;
  final SubscriptionRefreshInterval autoRefresh;
  final int? lastRefreshedAtUnixMs;
  final int? nextRefreshAtUnixMs;
  final String? lastRefreshError;
  final int skippedLineCount;
  final List<SubscriptionEndpoint> endpoints;
  final String? selectedEndpointId;
  final SubscriptionEndpoint? retainedMissingEndpoint;

  ({SubscriptionEndpoint endpoint, bool removedUpstream})? get activeEndpoint {
    final selectedId = selectedEndpointId;
    if (selectedId != null) {
      for (final endpoint in endpoints) {
        if (endpoint.id == selectedId) {
          return (endpoint: endpoint, removedUpstream: false);
        }
      }
      if (retainedMissingEndpoint case final retained?
          when retained.id == selectedId) {
        return (endpoint: retained, removedUpstream: true);
      }
    }
    if (endpoints.isNotEmpty) {
      return (endpoint: endpoints.first, removedUpstream: false);
    }
    if (retainedMissingEndpoint case final retained?) {
      return (endpoint: retained, removedUpstream: true);
    }
    return null;
  }
}

@immutable
class EndpointSubscriptionCatalog {
  const EndpointSubscriptionCatalog({
    required this.revision,
    this.unchanged = false,
    this.subscriptions = const <EndpointSubscription>[],
    this.activeAccountId,
    this.activeAccountTier,
    this.activeSubscriptionId,
    this.activeEndpointId,
    this.refreshingSubscriptionIds = const <String>{},
    this.activeProbeId,
    this.activeProbeSubscriptionId,
  });

  factory EndpointSubscriptionCatalog.fromMap(
    Map<Object?, Object?> map, {
    EndpointSubscriptionCatalog? previous,
  }) {
    final revision = map['revision'] as String? ?? '';
    final unchanged = map['unchanged'] == true && previous != null;
    final rawSubs = map['subscriptions'];
    final List<EndpointSubscription> subscriptions;
    if (unchanged && rawSubs == null) {
      subscriptions = previous.subscriptions;
    } else if (rawSubs is List) {
      subscriptions = List<EndpointSubscription>.unmodifiable(
        rawSubs.whereType<Map<Object?, Object?>>().map(
          EndpointSubscription.fromMap,
        ),
      );
    } else {
      subscriptions = const <EndpointSubscription>[];
    }
    final probe = map['active_probe'];
    final probeMap = probe is Map ? probe : null;
    return EndpointSubscriptionCatalog(
      revision: revision,
      unchanged: unchanged,
      subscriptions: subscriptions,
      activeAccountId: map['active_account_id'] as String?,
      activeAccountTier: EndpointSubscriptionTier.parse(
        map['active_account_tier'],
      ),
      activeSubscriptionId: map['active_subscription_id'] as String?,
      activeEndpointId: map['active_endpoint_id'] as String?,
      refreshingSubscriptionIds: Set<String>.unmodifiable(
        (map['refreshing_subscription_ids'] as List? ?? const <Object>[])
            .whereType<String>(),
      ),
      activeProbeId: probeMap?['probe_id'] as String?,
      activeProbeSubscriptionId: probeMap?['subscription_id'] as String?,
    );
  }

  final String revision;
  final bool unchanged;
  final List<EndpointSubscription> subscriptions;
  final String? activeAccountId;
  final EndpointSubscriptionTier? activeAccountTier;
  final String? activeSubscriptionId;
  final String? activeEndpointId;
  final Set<String> refreshingSubscriptionIds;
  final String? activeProbeId;
  final String? activeProbeSubscriptionId;

  EndpointSubscription? byId(String? id) {
    if (id == null) return null;
    for (final sub in subscriptions) {
      if (sub.id == id) return sub;
    }
    return null;
  }
}

@immutable
class EndpointProbeResult {
  const EndpointProbeResult({
    required this.endpointId,
    required this.status,
    this.rttMs,
    this.protocol,
  });

  factory EndpointProbeResult.fromMap(Map<Object?, Object?> map) {
    final endpointId = map['endpoint_id'];
    if (endpointId is! String || endpointId.isEmpty) {
      throw const FormatException('Invalid probe result');
    }
    return EndpointProbeResult(
      endpointId: endpointId,
      status: EndpointProbeStatus.parse(map['status']),
      rttMs: (map['rtt_ms'] as num?)?.toInt(),
      protocol: EndpointProbeProtocol.parse(map['protocol']),
    );
  }

  final String endpointId;
  final EndpointProbeStatus status;
  final int? rttMs;
  final EndpointProbeProtocol? protocol;
}

@immutable
class EndpointProbeProgress {
  const EndpointProbeProgress({
    required this.probeId,
    required this.subscriptionId,
    required this.state,
    required this.completed,
    required this.total,
    this.errorCode,
    this.results = const <EndpointProbeResult>[],
  });

  factory EndpointProbeProgress.fromMap(Map<Object?, Object?> map) {
    final probeId = map['probe_id'];
    final subscriptionId = map['subscription_id'];
    if (probeId is! String ||
        probeId.isEmpty ||
        subscriptionId is! String ||
        subscriptionId.isEmpty) {
      throw const FormatException('Invalid probe progress');
    }
    final rawResults = map['results'] as List? ?? const <Object>[];
    return EndpointProbeProgress(
      probeId: probeId,
      subscriptionId: subscriptionId,
      state: EndpointProbeState.parse(map['state']),
      completed: (map['completed'] as num?)?.toInt() ?? 0,
      total: (map['total'] as num?)?.toInt() ?? 0,
      errorCode: map['error_code'] as String?,
      results: List<EndpointProbeResult>.unmodifiable(
        rawResults.whereType<Map<Object?, Object?>>().map(
          EndpointProbeResult.fromMap,
        ),
      ),
    );
  }

  final String probeId;
  final String subscriptionId;
  final EndpointProbeState state;
  final int completed;
  final int total;
  final String? errorCode;
  final List<EndpointProbeResult> results;
}
