import 'package:flutter/material.dart';

/// A short history of one traffic rate, drawn as a filled trace.
///
/// The trace is scaled to the tallest sample in the window, so it shows the
/// shape of recent activity rather than an absolute magnitude; the number next
/// to it carries the magnitude.
class Sparkline extends StatelessWidget {
  const Sparkline({
    required this.samples,
    required this.color,
    this.height = 32,
    this.semanticLabel,
    super.key,
  });

  final List<int?> samples;
  final String? semanticLabel;
  final Color color;
  final double height;

  @override
  Widget build(BuildContext context) {
    return Semantics(
      label: semanticLabel,
      image: semanticLabel != null,
      child: SizedBox(
        height: height,
        width: double.infinity,
        child: RepaintBoundary(
          child: CustomPaint(
            painter: _SparklinePainter(samples: samples, color: color),
          ),
        ),
      ),
    );
  }
}

class _SparklinePainter extends CustomPainter {
  _SparklinePainter({required this.samples, required this.color});

  final List<int?> samples;
  final Color color;

  @override
  void paint(Canvas canvas, Size size) {
    final Paint baseline = Paint()
      ..strokeWidth = 1
      ..color = color.withValues(alpha: 0.18);
    if (samples.every((sample) => sample == null)) {
      _paintEmpty(canvas, size, baseline);
      return;
    }
    canvas.drawLine(
      Offset(0, size.height - 0.5),
      Offset(size.width, size.height - 0.5),
      baseline,
    );

    if (samples.length < 2) {
      return;
    }

    var peak = 0;
    for (final sample in samples) {
      if (sample != null && sample > peak) peak = sample;
    }
    if (peak <= 0) peak = 1;

    final double step = size.width / (samples.length - 1);
    final segments = <List<Offset>>[];
    var segment = <Offset>[];
    for (int i = 0; i < samples.length; i += 1) {
      final sample = samples[i];
      if (sample == null) {
        if (segment.isNotEmpty) segments.add(segment);
        segment = <Offset>[];
        continue;
      }
      final double x = i * step;
      final double y = size.height - 1 - (sample / peak) * (size.height - 2);
      segment.add(Offset(x, y));
    }
    if (segment.isNotEmpty) segments.add(segment);

    for (final points in segments) {
      final line = Path()..moveTo(points.first.dx, points.first.dy);
      for (final point in points.skip(1)) {
        line.lineTo(point.dx, point.dy);
      }
      if (points.length == 1) {
        canvas.drawCircle(points.first, 1.8, Paint()..color = color);
        continue;
      }

      final Path area = Path.from(line)
        ..lineTo(points.last.dx, size.height)
        ..lineTo(points.first.dx, size.height)
        ..close();
      canvas.drawPath(
        area,
        Paint()
          ..shader = LinearGradient(
            begin: Alignment.topCenter,
            end: Alignment.bottomCenter,
            colors: <Color>[
              color.withValues(alpha: 0.22),
              color.withValues(alpha: 0),
            ],
          ).createShader(Rect.fromLTWH(0, 0, size.width, size.height)),
      );
      canvas.drawPath(
        line,
        Paint()
          ..style = PaintingStyle.stroke
          ..strokeWidth = 1.6
          ..strokeJoin = StrokeJoin.round
          ..strokeCap = StrokeCap.round
          ..color = color,
      );
    }
  }

  /// An empty window draws a dashed frame so it reads as "no data yet" rather
  /// than a missing chart.
  void _paintEmpty(Canvas canvas, Size size, Paint baseline) {
    final Paint guide = Paint()
      ..strokeWidth = 1
      ..color = color.withValues(alpha: 0.09);
    final int guides = size.height >= 160
        ? 3
        : size.height >= 64
        ? 2
        : 1;
    for (int i = 1; i <= guides; i += 1) {
      final double y = (size.height * i / (guides + 1)).roundToDouble() + 0.5;
      _dashedLine(canvas, y, size.width, guide);
    }
    _dashedLine(canvas, size.height - 0.5, size.width, baseline);
  }

  void _dashedLine(Canvas canvas, double y, double width, Paint paint) {
    const double dash = 4;
    const double gap = 4;
    for (double x = 0; x < width; x += dash + gap) {
      canvas.drawLine(
        Offset(x, y),
        Offset((x + dash).clamp(0, width).toDouble(), y),
        paint,
      );
    }
  }

  @override
  bool shouldRepaint(covariant _SparklinePainter oldDelegate) {
    return oldDelegate.color != color ||
        !identical(oldDelegate.samples, samples);
  }
}
