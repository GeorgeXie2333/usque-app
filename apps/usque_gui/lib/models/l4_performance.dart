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
    this.receive,
  );

  /// Missing optional observations remain absent; buffers are not process RSS.
  final Map<String, int> counters;
  final String? udpBufferSource, tunMtuSource;
  final L4WaitSnapshot? commandWait, tunWriteWait;
  final Map<String, L4QueueSnapshot> queues;
  final L4ReceiveSnapshot? receive;
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
      L4ReceiveSnapshot.from(map['receive']),
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
      mapEquals(queues, other.queues) &&
      receive == other.receive;
  @override
  int get hashCode => Object.hash(
    Object.hashAll(counters.values),
    udpBufferSource,
    tunMtuSource,
    commandWait,
    tunWriteWait,
    Object.hashAll(queues.values),
    receive,
  );
}

const l4ReceiveCounterFields = <int, String>{
  1: 'requested_buffer_bytes',
  4: 'socket_drops_reported',
  5: 'overflow_reports',
  6: 'ancillary_errors',
  7: 'recv_syscalls',
  8: 'received_datagrams',
  9: 'empty_recv_syscalls',
  13: 'history_dropped',
  14: 'buffer_target_bytes',
};
const l4ReceiveStringFields = <int, String>{
  2: 'buffer_request_status',
  3: 'overflow_monitoring',
  10: 'receive_backend',
  11: 'send_backend',
};
const l4ReceiveIntervalFields = <int, String>{
  1: 'elapsed_ms',
  2: 'interval_ms',
  4: 'h3_read_bytes',
  5: 'tcp_accepted_bytes',
  6: 'tun_ingress_bytes',
  7: 'received_datagrams',
  8: 'recv_syscalls',
  9: 'socket_drops',
  10: 'socket_drops_reported',
};
const l4ReceiveStringValues = <String, Set<String>>{
  'buffer_request_status': {
    'not_requested',
    'accepted',
    'rejected',
    'already_sufficient',
  },
  'overflow_monitoring': {'enabled', 'unavailable', 'unavailable_backend'},
  'receive_backend': {'portable', 'recvmmsg'},
  'send_backend': {'portable', 'sendmmsg'},
};

@immutable
class L4ReceiveInterval {
  const L4ReceiveInterval._(this.counters, this.pathReset);
  final Map<String, int> counters;
  final bool pathReset;
  static L4ReceiveInterval? from(Object? value) {
    if (value is! Map || value['path_reset'] is! bool) return null;
    final counters = <String, int>{};
    for (final key in l4ReceiveIntervalFields.values) {
      final count = _count(value[key]);
      if (count != null) counters[key] = count;
    }
    for (final key in [
      'elapsed_ms',
      'interval_ms',
      'h3_read_bytes',
      'tcp_accepted_bytes',
      'tun_ingress_bytes',
    ]) {
      if (!counters.containsKey(key)) return null;
    }
    return L4ReceiveInterval._(
      Map.unmodifiable(counters),
      value['path_reset'] as bool,
    );
  }

  @override
  bool operator ==(Object other) =>
      other is L4ReceiveInterval &&
      pathReset == other.pathReset &&
      mapEquals(counters, other.counters);
  @override
  int get hashCode => Object.hash(pathReset, Object.hashAll(counters.values));
}

@immutable
class L4ReceiveSnapshot {
  const L4ReceiveSnapshot._(this.counters, this.states, this.history);
  final Map<String, int> counters;
  final Map<String, String> states;
  final List<L4ReceiveInterval> history;
  static L4ReceiveSnapshot? from(Object? value) {
    if (value is! Map) return null;
    final rawHistory = value['history'] ?? <Object?>[];
    if (rawHistory is! List || rawHistory.length > 120) return null;
    final history = <L4ReceiveInterval>[];
    for (final raw in rawHistory) {
      final item = L4ReceiveInterval.from(raw);
      if (item == null) return null;
      history.add(item);
    }
    final counters = <String, int>{};
    for (final key in l4ReceiveCounterFields.values) {
      final count = _count(value[key]);
      if (count != null) counters[key] = count;
    }
    final states = <String, String>{};
    for (final entry in l4ReceiveStringValues.entries) {
      if (entry.value.contains(value[entry.key])) {
        states[entry.key] = value[entry.key] as String;
      }
    }
    return L4ReceiveSnapshot._(
      Map.unmodifiable(counters),
      Map.unmodifiable(states),
      List.unmodifiable(history),
    );
  }

  @override
  bool operator ==(Object other) =>
      other is L4ReceiveSnapshot &&
      mapEquals(counters, other.counters) &&
      mapEquals(states, other.states) &&
      listEquals(history, other.history);
  @override
  int get hashCode => Object.hash(
    Object.hashAll(counters.values),
    Object.hashAll(states.values),
    Object.hashAll(history),
  );
}

@immutable
class UdpSocketReceiveSnapshot {
  const UdpSocketReceiveSnapshot({
    this.receiveBufferBytes,
    this.sendBufferBytes,
    this.observation,
  });
  final int? receiveBufferBytes, sendBufferBytes;
  final L4ReceiveSnapshot? observation;
  static UdpSocketReceiveSnapshot? from(Object? value) {
    if (value is! Map) return null;
    return UdpSocketReceiveSnapshot(
      receiveBufferBytes: _count(value['receive_buffer_bytes']),
      sendBufferBytes: _count(value['send_buffer_bytes']),
      observation: L4ReceiveSnapshot.from(value['observation']),
    );
  }

  @override
  bool operator ==(Object other) =>
      other is UdpSocketReceiveSnapshot &&
      receiveBufferBytes == other.receiveBufferBytes &&
      sendBufferBytes == other.sendBufferBytes &&
      observation == other.observation;
  @override
  int get hashCode =>
      Object.hash(receiveBufferBytes, sendBufferBytes, observation);
}
