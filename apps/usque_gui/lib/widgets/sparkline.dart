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
    super.key,
  });

  final List<int> samples;
  final Color color;
  final double height;

  @override
  Widget build(BuildContext context) {
    return SizedBox(
      height: height,
      width: double.infinity,
      child: RepaintBoundary(
        child: CustomPaint(
          painter: _SparklinePainter(samples: samples, color: color),
        ),
      ),
    );
  }
}

class _SparklinePainter extends CustomPainter {
  _SparklinePainter({required this.samples, required this.color});

  final List<int> samples;
  final Color color;

  @override
  void paint(Canvas canvas, Size size) {
    final Paint baseline = Paint()
      ..strokeWidth = 1
      ..color = color.withValues(alpha: 0.18);
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
      if (sample > peak) peak = sample;
    }
    if (peak <= 0) {
      return;
    }

    final double step = size.width / (samples.length - 1);
    final Path line = Path();
    for (int i = 0; i < samples.length; i += 1) {
      final double x = i * step;
      final double y = size.height - (samples[i] / peak) * (size.height - 2);
      if (i == 0) {
        line.moveTo(x, y);
      } else {
        line.lineTo(x, y);
      }
    }

    final Path area = Path.from(line)
      ..lineTo(size.width, size.height)
      ..lineTo(0, size.height)
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

  @override
  bool shouldRepaint(covariant _SparklinePainter oldDelegate) {
    return oldDelegate.color != color ||
        !identical(oldDelegate.samples, samples);
  }
}
