import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:usque/core/usque_theme.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/screens/diagnostics_screen.dart';
import 'package:usque/screens/network_quality_screen.dart';
import 'package:usque/state/app_controller.dart';
import 'package:usque/widgets/common.dart';
import 'package:usque/widgets/diagnostics/diagnostic_results.dart';
import 'package:usque/widgets/network_quality/quality_layout.dart';

import 'quality_test_support.dart';

Widget _host(Widget home) => MaterialApp(
  theme: UsqueTheme.light(),
  builder: (context, child) => MediaQuery(
    data: MediaQuery.of(context).copyWith(disableAnimations: true),
    child: child!,
  ),
  home: Scaffold(body: home),
);

QualitySection _section(WidgetTester tester, AppController app, String key) =>
    tester.widget<QualitySection>(
      find.widgetWithText(QualitySection, app.strings.get(key)),
    );

void main() {
  testWidgets('a new sample repaints the trends, not the folded internals', (
    tester,
  ) async {
    await tester.binding.setSurfaceSize(const Size(1280, 3000));
    addTearDown(() => tester.binding.setSurfaceSize(null));
    final app = qualityApp(QualityEngineStub(), state: 'degraded');
    addTearDown(app.dispose);
    await tester.pumpWidget(_host(NetworkQualityScreen(controller: app)));
    await tester.pumpAndSettle();
    final rtt = _section(tester, app, 'nq_rtt');
    final pmtu = _section(tester, app, 'nq_pmtu');
    final dns = _section(tester, app, 'nq_direct_dns');

    // Same path MTU and DNS state, a different round trip.
    final latest = app.quality.latest!;
    app.snapshot = EngineSnapshot(
      phase: ConnectionPhase.connected,
      transport: 'HTTP/3',
      addressFamily: 'IPv4',
      networkQuality: qualityFixture(
        latest.sampledAt!.add(const Duration(seconds: 1)),
        state: 'degraded',
        rtt: 90,
      ),
    );
    await tester.pump();

    expect(identical(_section(tester, app, 'nq_rtt'), rtt), isFalse);
    expect(identical(_section(tester, app, 'nq_pmtu'), pmtu), isTrue);
    expect(identical(_section(tester, app, 'nq_direct_dns'), dns), isTrue);
    expect(tester.takeException(), isNull);
    await tester.pumpWidget(const SizedBox.shrink());
  });

  testWidgets('connection samples do not repaint diagnostic results', (
    tester,
  ) async {
    await tester.binding.setSurfaceSize(const Size(1280, 1600));
    addTearDown(() => tester.binding.setSurfaceSize(null));
    final app = qualityApp(QualityEngineStub(), state: 'disconnected');
    addTearDown(app.dispose);
    await tester.pumpWidget(_host(DiagnosticsScreen(controller: app)));
    await tester.pumpAndSettle();
    ContentSection results() => tester.widget<ContentSection>(
      find
          .descendant(
            of: find.byType(DiagnosticResultsPanel),
            matching: find.byType(ContentSection),
          )
          .first,
    );
    final before = results();
    app.snapshot = const EngineSnapshot(
      phase: ConnectionPhase.connected,
      downloadBytesPerSecond: 4096,
      uploadBytesPerSecond: 1024,
    );
    await tester.pump();
    expect(identical(results(), before), isTrue);
    expect(tester.takeException(), isNull);
    await tester.pumpWidget(const SizedBox.shrink());
  });
}
