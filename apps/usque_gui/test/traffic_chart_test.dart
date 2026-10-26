import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:usque/core/usque_theme.dart';
import 'package:usque/widgets/traffic_chart.dart';

void main() {
  test('the scale rounds a peak up to 1, 2 or 5 times a power of ten', () {
    expect(trafficScaleCeiling(0), 1);
    expect(trafficScaleCeiling(1), 1);
    expect(trafficScaleCeiling(7), 10);
    expect(trafficScaleCeiling(10), 10);
    expect(trafficScaleCeiling(11), 20);
    expect(trafficScaleCeiling(180000), 200000);
    expect(trafficScaleCeiling(260000), 500000);
    expect(trafficScaleCeiling(750000), 1000000);
    expect(trafficScaleCeiling(1000001), 2000000);
  });

  test('scale labels drop the decimal of round values', () {
    expect(trafficScaleLabel(500), '500 B/s');
    expect(trafficScaleLabel(200000), '200 KB/s');
    expect(trafficScaleLabel(1000000), '1 MB/s');
    expect(trafficScaleLabel(1500000), '1.5 MB/s');
    expect(trafficScaleLabel(2000000000), '2 GB/s');
  });

  for (final (name, download, upload) in [
    ('empty', const <int?>[], const <int?>[]),
    ('gaps only', const <int?>[null, null], const <int?>[null]),
    ('one sample', const <int?>[null, 1200], const <int?>[null, 300]),
    (
      'gapped series',
      const <int?>[10, 400, null, 0, 0, 900],
      const <int?>[5, 5],
    ),
  ]) {
    testWidgets('paints $name history without errors', (tester) async {
      await tester.pumpWidget(
        MaterialApp(
          theme: UsqueTheme.light(),
          home: Scaffold(
            body: SizedBox(
              width: 320,
              child: TrafficChart(
                download: download,
                upload: upload,
                downloadColor: Colors.blue,
                uploadColor: Colors.purple,
                semanticLabel: name,
              ),
            ),
          ),
        ),
      );
      final chart = tester.widget<TrafficChart>(find.byType(TrafficChart));
      expect(
        chart.isEmpty,
        [...download, ...upload].every((sample) => sample == null),
      );
      expect(tester.getSize(find.byType(TrafficChart)).height, 96);
      expect(find.bySemanticsLabel(name), findsOneWidget);
      expect(tester.takeException(), isNull);
    });
  }
}
