import 'dart:math' as math;

import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';

import '../core/usque_theme.dart';

/// Download and upload history drawn over one shared scale, so the two
/// directions compare honestly. The top guide names the scale; missing
/// samples stay gaps and an empty window draws only its baseline.
class TrafficChart extends StatelessWidget {
  const TrafficChart({
    required this.download,
    required this.upload,
    required this.downloadColor,
    required this.uploadColor,
    this.height = 96,
    this.semanticLabel,
    super.key,
  });

  final List<int?> download;
  final List<int?> upload;
  final Color downloadColor;
  final Color uploadColor;

  /// Minimum height; a stretched parent can make the chart taller.
  final double height;
  final String? semanticLabel;

  bool get isEmpty =>
      download.every((sample) => sample == null) &&
      upload.every((sample) => sample == null);

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final tokens = UsqueTokens.of(context);
    return Semantics(
      label: semanticLabel,
      image: true,
      child: ConstrainedBox(
        constraints: BoxConstraints(minHeight: height),
        child: CustomPaint(
          size: Size(double.infinity, height),
          painter: _TrafficChartPainter(
            download: download,
            upload: upload,
            downloadColor: downloadColor,
            uploadColor: uploadColor,
            guide: tokens.hairline,
            background: theme.colorScheme.surface,
            labelStyle: UsqueTheme.readout(
              context,
              size: 11,
              color: theme.colorScheme.onSurfaceVariant,
            ),
            textDirection: Directionality.of(context),
          ),
        ),
      ),
    );
  }
}

/// Rounds [peak] up to 1, 2 or 5 times a power of ten so the top guide reads
/// as a plain value.
@visibleForTesting
int trafficScaleCeiling(int peak) {
  if (peak <= 1) return 1;
  var magnitude = 1;
  while (magnitude * 10 <= peak) {
    magnitude *= 10;
  }
  for (final step in const [1, 2, 5, 10]) {
    if (step * magnitude >= peak) return step * magnitude;
  }
  return 10 * magnitude;
}

/// A scale label without the trailing ".0" of a round value.
@visibleForTesting
String trafficScaleLabel(int bytesPerSecond) {
  const units = ['B/s', 'KB/s', 'MB/s', 'GB/s'];
  var value = bytesPerSecond.toDouble();
  var unit = 0;
  while (value >= 1000 && unit < units.length - 1) {
    value /= 1000;
    unit++;
  }
  final text = value == value.roundToDouble()
      ? value.toStringAsFixed(0)
      : value.toStringAsFixed(1);
  return '$text ${units[unit]}';
}

class _TrafficChartPainter extends CustomPainter {
  _TrafficChartPainter({
    required this.download,
    required this.upload,
    required this.downloadColor,
    required this.uploadColor,
    required this.guide,
    required this.background,
    required this.labelStyle,
    required this.textDirection,
  });

  final List<int?> download;
  final List<int?> upload;
  final Color downloadColor;
  final Color uploadColor;
  final Color guide;
  final Color background;
  final TextStyle labelStyle;
  final TextDirection textDirection;

  /// Room above the plot for the scale label.
  static const double _labelBand = 18;

  @override
  void paint(Canvas canvas, Size size) {
    final guidePaint = Paint()
      ..strokeWidth = 1
      ..color = guide;
    var peak = 0;
    for (final sample in [...download, ...upload]) {
      if (sample != null && sample > peak) peak = sample;
    }
    final empty =
        download.every((sample) => sample == null) &&
        upload.every((sample) => sample == null);
    final plot = Rect.fromLTRB(0, _labelBand, size.width, size.height - 1);
    final baseline = plot.bottom.roundToDouble() + 0.5;
    final top = plot.top.roundToDouble() + 0.5;
    final middle = ((plot.top + plot.bottom) / 2).roundToDouble() + 0.5;

    if (empty) {
      // Only the baseline: the card's note explains the empty window.
      canvas.drawLine(
        Offset(0, baseline),
        Offset(size.width, baseline),
        guidePaint,
      );
      return;
    }

    final scale = trafficScaleCeiling(math.max(peak, 1));
    _dashed(canvas, top, size.width, guidePaint);
    _dashed(canvas, middle, size.width, guidePaint);
    canvas.drawLine(
      Offset(0, baseline),
      Offset(size.width, baseline),
      guidePaint,
    );
    final label = TextPainter(
      text: TextSpan(text: trafficScaleLabel(scale), style: labelStyle),
      textDirection: textDirection,
      maxLines: 1,
    )..layout(maxWidth: size.width);
    label.paint(
      canvas,
      Offset(
        textDirection == TextDirection.rtl ? size.width - label.width : 0,
        math.max(0, top - label.height - 3),
      ),
    );

    // The larger download area sits behind the upload line.
    _series(canvas, plot, download, scale, downloadColor);
    _series(canvas, plot, upload, scale, uploadColor);
  }

  void _series(
    Canvas canvas,
    Rect plot,
    List<int?> samples,
    int scale,
    Color color,
  ) {
    if (samples.isEmpty) return;
    final step = samples.length > 1
        ? plot.width / (samples.length - 1)
        : plot.width;
    final segments = <List<Offset>>[];
    var segment = <Offset>[];
    for (var i = 0; i < samples.length; i++) {
      final sample = samples[i];
      if (sample == null) {
        if (segment.isNotEmpty) segments.add(segment);
        segment = <Offset>[];
        continue;
      }
      final y = plot.bottom - (sample / scale).clamp(0.0, 1.0) * plot.height;
      segment.add(Offset(i * step, y));
    }
    if (segment.isNotEmpty) segments.add(segment);

    final stroke = Paint()
      ..style = PaintingStyle.stroke
      ..strokeWidth = 2
      ..strokeJoin = StrokeJoin.round
      ..strokeCap = StrokeCap.round
      ..color = color;
    final fill = Paint()
      ..shader = LinearGradient(
        begin: Alignment.topCenter,
        end: Alignment.bottomCenter,
        colors: <Color>[
          color.withValues(alpha: 0.20),
          color.withValues(alpha: 0),
        ],
      ).createShader(plot);
    for (final points in segments) {
      if (points.length == 1) {
        canvas.drawCircle(points.single, 2, Paint()..color = color);
        continue;
      }
      final line = _monotone(points);
      final area = Path.from(line)
        ..lineTo(points.last.dx, plot.bottom)
        ..lineTo(points.first.dx, plot.bottom)
        ..close();
      canvas
        ..drawPath(area, fill)
        ..drawPath(line, stroke);
    }

    // Mark the latest reading when it is current.
    if (samples.last != null && segments.isNotEmpty) {
      final last = segments.last.last;
      canvas
        ..drawCircle(last, 4.5, Paint()..color = background)
        ..drawCircle(last, 3, Paint()..color = color);
    }
  }

  /// Monotone cubic (Fritsch–Carlson) through [points]: smooth, and never
  /// overshoots a sample, so a curve cannot dip below zero or invent a peak.
  static Path _monotone(List<Offset> points) {
    final path = Path()..moveTo(points.first.dx, points.first.dy);
    final n = points.length;
    if (n == 2) return path..lineTo(points.last.dx, points.last.dy);
    final secants = List<double>.generate(n - 1, (i) {
      final dx = points[i + 1].dx - points[i].dx;
      return dx == 0 ? 0 : (points[i + 1].dy - points[i].dy) / dx;
    });
    final tangents = List<double>.filled(n, 0);
    tangents[0] = secants.first;
    tangents[n - 1] = secants.last;
    for (var i = 1; i < n - 1; i++) {
      tangents[i] = secants[i - 1] * secants[i] <= 0
          ? 0
          : (secants[i - 1] + secants[i]) / 2;
    }
    for (var i = 0; i < n - 1; i++) {
      if (secants[i] == 0) {
        tangents[i] = 0;
        tangents[i + 1] = 0;
        continue;
      }
      final a = tangents[i] / secants[i];
      final b = tangents[i + 1] / secants[i];
      final s = a * a + b * b;
      if (s > 9) {
        final tau = 3 / math.sqrt(s);
        tangents[i] = tau * a * secants[i];
        tangents[i + 1] = tau * b * secants[i];
      }
    }
    for (var i = 0; i < n - 1; i++) {
      final p0 = points[i];
      final p1 = points[i + 1];
      final h = (p1.dx - p0.dx) / 3;
      path.cubicTo(
        p0.dx + h,
        p0.dy + tangents[i] * h,
        p1.dx - h,
        p1.dy - tangents[i + 1] * h,
        p1.dx,
        p1.dy,
      );
    }
    return path;
  }

  static void _dashed(Canvas canvas, double y, double width, Paint paint) {
    const dash = 3.0;
    const gap = 4.0;
    for (var x = 0.0; x < width; x += dash + gap) {
      canvas.drawLine(
        Offset(x, y),
        Offset(math.min(x + dash, width), y),
        paint,
      );
    }
  }

  @override
  bool shouldRepaint(covariant _TrafficChartPainter old) =>
      !listEquals(old.download, download) ||
      !listEquals(old.upload, upload) ||
      old.downloadColor != downloadColor ||
      old.uploadColor != uploadColor ||
      old.guide != guide ||
      old.background != background ||
      old.labelStyle != labelStyle ||
      old.textDirection != textDirection;
}
