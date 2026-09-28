import 'package:flutter/foundation.dart';

const queueBackpressureFields = <String>[
  'waits',
  'active',
  'completed',
  'cancelled',
  'closed',
  'errors',
  'total_us',
  'max_us',
];
const h2PerformanceFields = <String>[
  'data_frames',
  'data_bytes',
  'assembly_copy_bytes',
  'batches',
  'packets',
  'packet_bytes',
];
const h3PerformanceFields = <String>[
  'application_batches',
  'application_packets',
  'application_bytes',
  'encode_pool_exhausted',
  'datagram_queue_full',
  'pmtu_deferred',
  'wire_queue_full',
  'quantum_limited',
  'quic_no_progress_with_backlog',
  'udp_would_block',
  'udp_partial_sends',
  'udp_message_too_large',
];

/// Fixed allowlists keep imported diagnostics bounded. Absent groups stay null.
@immutable
class PerformanceCounters {
  const PerformanceCounters(this.values, {this.buckets = const <int>[]});
  final Map<String, int> values;
  final List<int> buckets;

  static PerformanceCounters? from(
    Object? raw,
    List<String> fields, {
    int bucketLimit = 0,
  }) {
    if (raw is! Map) return null;
    return PerformanceCounters(
      Map.unmodifiable({for (final name in fields) name: _count(raw[name])}),
      buckets: boundedBuckets(raw['buckets'], bucketLimit),
    );
  }

  @override
  bool operator ==(Object other) =>
      other is PerformanceCounters &&
      mapEquals(values, other.values) &&
      listEquals(buckets, other.buckets);
  @override
  int get hashCode => Object.hash(
    Object.hashAll(values.entries.map((e) => Object.hash(e.key, e.value))),
    Object.hashAll(buckets),
  );
}

int _count(Object? value) => value is int && value >= 0 ? value : 0;
List<int> boundedBuckets(Object? raw, int limit) => List.unmodifiable(
  raw is List ? raw.take(limit).map(_count) : const <int>[],
);

@immutable
class TransportPerformanceSnapshot {
  const TransportPerformanceSnapshot({
    this.h2,
    this.h3,
    this.incomingCopyBytes = 0,
    this.sendTimeouts = 0,
    this.h2BatchSizes = const <int>[],
    this.h3BatchSizes = const <int>[],
  });
  final PerformanceCounters? h2;
  final PerformanceCounters? h3;
  final int incomingCopyBytes;
  final int sendTimeouts;
  final List<int> h2BatchSizes;
  final List<int> h3BatchSizes;

  static TransportPerformanceSnapshot? from(Object? raw) {
    if (raw is! Map) return null;
    return TransportPerformanceSnapshot(
      h2: PerformanceCounters.from(raw['h2'], h2PerformanceFields),
      h3: PerformanceCounters.from(raw['h3'], h3PerformanceFields),
      incomingCopyBytes: _count(raw['incoming_copy_bytes']),
      sendTimeouts: _count(raw['send_timeouts']),
      h2BatchSizes: boundedBuckets(raw['h2_batch_sizes'], 7),
      h3BatchSizes: boundedBuckets(raw['h3_batch_sizes'], 7),
    );
  }
}
