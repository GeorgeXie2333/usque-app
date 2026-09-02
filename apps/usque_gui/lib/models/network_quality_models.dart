import 'dart:math' as math;

import 'app_models.dart';

class NetworkQualityPoint {
  const NetworkQualityPoint({
    required this.at,
    this.rttMilliseconds,
    this.lossBasisPoints,
    this.downloadBytesPerSecond,
    this.uploadBytesPerSecond,
  });

  final DateTime at;
  final int? rttMilliseconds;
  final int? lossBasisPoints;
  final int? downloadBytesPerSecond;
  final int? uploadBytesPerSecond;
}

int? availableMetric(int? value, MetricAvailability availability) =>
    availability == MetricAvailability.available && value != null && value >= 0
    ? value
    : null;

double? queuePressure(NetworkQueueQuality queue) {
  if (queue.availability != MetricAvailability.available) return null;
  final values = <double>[
    if (queue.capacityItems > 0) queue.currentItems / queue.capacityItems,
    if (queue.capacityBytes > 0) queue.currentBytes / queue.capacityBytes,
  ];
  return values.isEmpty ? null : values.reduce(math.max).clamp(0.0, 1.0);
}

String qualityByteRate(int? bytes) {
  if (bytes == null || bytes < 0) return '—';
  const units = <String>[
    'B/s',
    'KiB/s',
    'MiB/s',
    'GiB/s',
    'TiB/s',
    'PiB/s',
    'EiB/s',
  ];
  var value = bytes.toDouble();
  var unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit++;
  }
  return '${value.toStringAsFixed(unit == 0 || value >= 100 ? 0 : 1)} ${units[unit]}';
}

String qualityBytes(int? bytes) =>
    qualityByteRate(bytes).replaceFirst('/s', '');
