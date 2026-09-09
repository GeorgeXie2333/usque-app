import 'package:flutter/foundation.dart';

/// First 32 entries match protobuf fields 1..32; the extra counter is field 39.
/// No arbitrary native JSON is retained.
const l4PerformanceScalarFields = <String>[
  'h3_read_calls',
  'h3_read_bytes',
  'h3_empty_reads',
  'receive_pool_allocations',
  'receive_pool_hits',
  'receive_pool_evictions',
  'receive_pool_idle_bytes',
  'receive_pool_idle_high_watermark',
  'receive_pool_live_bytes',
  'receive_pool_live_high_watermark',
  'adapter_copied_bytes',
  'tcp_accepted_bytes',
  'tcp_write_calls',
  'tcp_partial_writes',
  'actor_wakeups',
  'actor_polls',
  'actor_no_progress_polls',
  'budget_wakeups',
  'tun_ingress_packets',
  'tun_ingress_bytes',
  'tun_egress_packets',
  'tun_egress_bytes',
  'tun_write_calls',
  'tun_write_would_block',
  'udp_receive_buffer_bytes',
  'udp_send_buffer_bytes',
  'udp_buffer_source',
  'tun_mtu',
  'tun_mtu_source',
  'tcp_preferred_sockets',
  'tcp_fallback_sockets',
  'tcp_buffer_bytes',
  'actor_no_progress_wakeups',
];
const l4PerformanceQueueFields = <String>[
  'tun_ingress_queue',
  'tun_egress_queue',
  'stack_ingress_queue',
  'stack_egress_queue',
];

int? _count(Object? value) => value is int && value >= 0 ? value : null;

@immutable
class L4WaitSnapshot {
  const L4WaitSnapshot(this.samples, this.sumUs, this.maxUs, this.buckets);
  final int samples;
  final int sumUs;
  final int maxUs;
  final List<int> buckets;
  bool get hasSamples => samples > 0;
  static L4WaitSnapshot? from(Object? value) {
    if (value is! Map) return null;
    final buckets = value['buckets'];
    if (buckets is! List ||
        buckets.length != 32 ||
        buckets.any((n) => _count(n) == null)) {
      return null;
    }
    final samples = _count(value['samples']);
    final sum = _count(value['sum_us']);
    final max = _count(value['max_us']);
    if (samples == null || sum == null || max == null) return null;
    return L4WaitSnapshot(
      samples,
      sum,
      max,
      List<int>.unmodifiable(buckets.cast<int>()),
    );
  }

  @override
  bool operator ==(Object other) =>
      other is L4WaitSnapshot &&
      samples == other.samples &&
      sumUs == other.sumUs &&
      maxUs == other.maxUs &&
      listEquals(buckets, other.buckets);
  @override
  int get hashCode =>
      Object.hash(samples, sumUs, maxUs, Object.hashAll(buckets));
}

@immutable
class L4QueueSnapshot {
  const L4QueueSnapshot(
    this.packets,
    this.bytes,
    this.highWaterPackets,
    this.highWaterBytes,
    this.wait,
  );
  final int packets, bytes, highWaterPackets, highWaterBytes;
  final L4WaitSnapshot? wait;
  static L4QueueSnapshot? from(Object? value) {
    if (value is! Map) return null;
    final numbers = [
      'packets',
      'bytes',
      'high_water_packets',
      'high_water_bytes',
    ].map((k) => _count(value[k])).toList();
    if (numbers.any((n) => n == null)) return null;
    return L4QueueSnapshot(
      numbers[0]!,
      numbers[1]!,
      numbers[2]!,
      numbers[3]!,
      L4WaitSnapshot.from(value['wait']),
    );
  }

  @override
  bool operator ==(Object other) =>
      other is L4QueueSnapshot &&
      packets == other.packets &&
      bytes == other.bytes &&
      highWaterPackets == other.highWaterPackets &&
      highWaterBytes == other.highWaterBytes &&
      wait == other.wait;
  @override
  int get hashCode =>
      Object.hash(packets, bytes, highWaterPackets, highWaterBytes, wait);
}

@immutable
class L4PerformanceSnapshot {
  const L4PerformanceSnapshot._(
    this.counters,
    this.udpBufferSource,
    this.tunMtuSource,
    this.commandWait,
    this.tunWriteWait,
    this.queues,
  );

  /// Missing optional observations remain absent; buffers are not process RSS.
  final Map<String, int> counters;
  final String? udpBufferSource, tunMtuSource;
  final L4WaitSnapshot? commandWait, tunWriteWait;
  final Map<String, L4QueueSnapshot> queues;
  factory L4PerformanceSnapshot.fromMap(Map<Object?, Object?> map) {
    final counters = <String, int>{};
    for (final key in l4PerformanceScalarFields) {
      final value = _count(map[key]);
      if (value != null) counters[key] = value;
    }
    final queues = <String, L4QueueSnapshot>{};
    for (final key in l4PerformanceQueueFields) {
      final value = L4QueueSnapshot.from(map[key]);
      if (value != null) queues[key] = value;
    }
    return L4PerformanceSnapshot._(
      Map.unmodifiable(counters),
      map['udp_buffer_source'] == 'getsockopt_raw' ? 'getsockopt_raw' : null,
      map['tun_mtu_source'] == 'applied_profile' ? 'applied_profile' : null,
      L4WaitSnapshot.from(map['command_wait']),
      L4WaitSnapshot.from(map['tun_write_wait']),
      Map.unmodifiable(queues),
    );
  }
  @override
  bool operator ==(Object other) =>
      other is L4PerformanceSnapshot &&
      mapEquals(counters, other.counters) &&
      udpBufferSource == other.udpBufferSource &&
      tunMtuSource == other.tunMtuSource &&
      commandWait == other.commandWait &&
      tunWriteWait == other.tunWriteWait &&
      mapEquals(queues, other.queues);
  @override
  int get hashCode => Object.hash(
    Object.hashAll(counters.values),
    udpBufferSource,
    tunMtuSource,
    commandWait,
    tunWriteWait,
    Object.hashAll(queues.values),
  );
}
