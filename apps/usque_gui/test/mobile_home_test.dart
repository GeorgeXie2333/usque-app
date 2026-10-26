import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/state/app_controller.dart';
import 'package:usque/state/network_quality_controller.dart';
import 'package:usque/widgets/common.dart';
import 'package:usque/widgets/connection_ring.dart';
import 'package:usque/widgets/live_duration.dart';
import 'package:usque/widgets/traffic_chart.dart';

import 'quality_test_support.dart';
import 'ui_workflow_test.dart' show workflowHost;

class _Fixture {
  _Fixture() {
    quality = NetworkQualityController(engine, now: () => now, autoTick: false);
    app = AppController(engine, qualityController: quality)
      ..localePreference = LocalePreference.english
      ..engineCapabilities = const EngineCapabilities(networkQuality: true);
  }
  final engine = QualityEngineStub();
  DateTime now = DateTime.utc(2026, 9, 2, 12);
  final since = DateTime.now().subtract(const Duration(minutes: 4));
  late final NetworkQualityController quality;
  late final AppController app;

  void sample(int second) {
    now = DateTime.utc(2026, 9, 2, 12).add(Duration(seconds: second));
    app.snapshot = EngineSnapshot(
      phase: ConnectionPhase.connected,
      connectedAt: since,
      transport: 'HTTP/3',
      addressFamily: 'IPv4',
      killSwitchState: 'active',
      downloadBytesPerSecond: 2048,
      uploadBytesPerSecond: 1024,
      downloadedBytes: second * 2048,
      uploadedBytes: second * 1024,
      networkQuality: qualityFixture(now),
      exit: const ExitInfo(
        country: 'Singapore',
        ipv4: '198.51.100.10',
        ipv6: '2001:db8:1234:5678:abcd:ef01:2345:6789',
      ),
    );
    engine.current = app.snapshot;
  }
}

Future<void> _show(
  WidgetTester tester,
  _Fixture fixture, {
  Size size = const Size(375, 812),
}) async {
  tester.view.devicePixelRatio = 1;
  tester.view.physicalSize = size;
  addTearDown(tester.view.resetDevicePixelRatio);
  addTearDown(tester.view.resetPhysicalSize);
  addTearDown(fixture.app.dispose);
  await tester.pumpWidget(workflowHost(fixture.app));
  await tester.pumpAndSettle();
}

TrafficChart _chart(WidgetTester tester) => tester.widget<TrafficChart>(
  find.byKey(const ValueKey('home-traffic-chart')),
);

List<int?> _trace(WidgetTester tester, String direction) =>
    direction == 'download' ? _chart(tester).download : _chart(tester).upload;

void main() {
  testWidgets(
    'desktop traffic caption follows paused and unavailable history',
    (tester) async {
      final fixture = _Fixture()
        ..sample(0)
        ..sample(1)
        ..sample(2);
      await _show(tester, fixture, size: const Size(1280, 900));
      TrafficChart chart() => _chart(tester);
      void expectCaption(String key) =>
          expect(find.text(fixture.app.strings.get(key)), findsOneWidget);

      expectCaption('home_traffic_window');
      final frozen = List<int?>.of(chart().download);
      fixture.quality.togglePaused();
      await tester.pump();
      expectCaption('nq_paused');
      expect(find.text('Last 60 seconds'), findsNothing);

      // A later engine event must not relabel the frozen window as live.
      fixture.sample(123);
      await tester.pump();
      expectCaption('nq_paused');
      expect(chart().download, frozen);
      expect(chart().semanticLabel, contains('Charts paused'));

      fixture.quality.togglePaused();
      await tester.pump();
      expectCaption('home_traffic_waiting');
      fixture.sample(124);
      fixture.sample(125);
      await tester.pump();
      expectCaption('home_traffic_window');

      fixture.now = fixture.now.add(const Duration(seconds: 4));
      fixture.quality.markStreamUnavailable(true);
      await tester.pump();
      expectCaption('home_traffic_stale');
      expect(chart().semanticLabel, contains('Traffic updates delayed'));

      fixture.app.engineCapabilities = const EngineCapabilities();
      await tester.pump();
      expectCaption('home_traffic_unavailable');
      fixture.app.snapshot = const EngineSnapshot();
      await tester.pump();
      expectCaption('home_traffic_idle');
      expect(fixture.engine.qualityRequests, 0);
    },
    variant: TargetPlatformVariant.only(TargetPlatform.windows),
  );

  testWidgets(
    'desktop traffic chart keeps its height and shows a dash while disconnected',
    (tester) async {
      final fixture = _Fixture()
        ..sample(0)
        ..sample(1);
      await _show(tester, fixture, size: const Size(1280, 900));
      final chart = find.byKey(const ValueKey('home-traffic-chart'));
      String? rate(String direction) => tester
          .widget<Text>(find.byKey(ValueKey('home-desktop-$direction-rate')))
          .data;
      final height = tester.getSize(chart).height;
      expect(height, greaterThanOrEqualTo(96));
      expect(rate('download'), '2.0 KB/s');
      expect(rate('upload'), '1.0 KB/s');

      fixture.app.snapshot = const EngineSnapshot();
      await tester.pump();
      for (final direction in ['download', 'upload']) {
        expect(rate(direction), '—');
      }
      expect(tester.getSize(chart).height, height);
      expect(_chart(tester).download, isEmpty);
      expect(_chart(tester).upload, isEmpty);
    },
    variant: TargetPlatformVariant.only(TargetPlatform.windows),
  );

  testWidgets(
    'desktop traffic card takes the spare page height down to its minimum',
    (tester) async {
      final fixture = _Fixture()..sample(1);
      await _show(tester, fixture, size: const Size(1200, 900));
      final chart = find.byKey(const ValueKey('home-traffic-chart'));
      final card = find.byKey(const ValueKey('home-traffic'));
      ScrollPosition page() => tester
          .state<ScrollableState>(
            find
                .descendant(
                  of: find.byType(PageFrame),
                  matching: find.byType(Scrollable),
                )
                .first,
          )
          .position;

      final open = tester.getSize(chart).height;
      expect(open, greaterThan(96));
      expect(page().maxScrollExtent, 0);
      expect(
        tester.getRect(card).bottom,
        moreOrLessEquals(tester.getRect(find.byType(PageFrame)).bottom - 34),
      );

      await tester.tap(
        find.text(fixture.app.strings.get('connection_details')),
      );
      await tester.pumpAndSettle();
      // Details grow the session card; the chart gives way, never the
      // page margin, until it reaches its minimum.
      final expanded = tester.getSize(chart).height;
      expect(expanded, lessThanOrEqualTo(open));
      expect(expanded, greaterThanOrEqualTo(96));

      tester.view.physicalSize = const Size(1200, 600);
      await tester.pumpAndSettle();
      expect(tester.getSize(chart).height, 96);
      expect(page().maxScrollExtent, greaterThan(0));
      expect(tester.takeException(), isNull);
    },
    variant: TargetPlatformVariant.only(TargetPlatform.windows),
  );

  testWidgets(
    'connected Android home details keep selectable values bounded',
    (tester) async {
      final fixture = _Fixture()..sample(1);
      fixture.app.localePreference = LocalePreference.simplifiedChinese;
      await _show(tester, fixture, size: const Size(424, 924));
      final details = find.text(fixture.app.strings.get('connection_details'));
      await tester.ensureVisible(details);
      await tester.pumpAndSettle();
      await tester.tap(details);
      await tester.pumpAndSettle();
      expect(tester.takeException(), isNull);
      expect(find.byType(ErrorWidget), findsNothing);
      final address = find.widgetWithText(SelectableText, '198.51.100.10');
      expect(address, findsOneWidget);
      expect(tester.getSize(address).height, lessThan(100));
      expect(find.widgetWithText(SelectableText, 'HTTP/3'), findsNothing);

      final tile = find.byKey(const PageStorageKey('home-connection-details'));
      Object? expansionState() {
        final context = tester.element(tile);
        return PageStorage.of(context).readState(context);
      }

      expect(expansionState(), isTrue);
      String? copied;
      tester.binding.defaultBinaryMessenger.setMockMethodCallHandler(
        SystemChannels.platform,
        (call) async {
          if (call.method == 'Clipboard.setData') {
            copied = (call.arguments as Map)['text'] as String;
          }
          return null;
        },
      );
      addTearDown(
        () => tester.binding.defaultBinaryMessenger.setMockMethodCallHandler(
          SystemChannels.platform,
          null,
        ),
      );
      await tester.ensureVisible(address);
      await tester.pumpAndSettle();
      await tester.longPress(address);
      await tester.pumpAndSettle();
      final editable = tester.state<EditableTextState>(
        find.descendant(of: address, matching: find.byType(EditableText)),
      );
      expect(editable.widget.controller.selection.isCollapsed, isFalse);
      editable.selectAll(SelectionChangedCause.toolbar);
      editable.copySelection(SelectionChangedCause.toolbar);
      await tester.pump();
      expect(copied, '198.51.100.10');
      FocusManager.instance.primaryFocus?.unfocus();
      await tester.pumpAndSettle();
      expect(expansionState(), isTrue);

      for (var iteration = 0; iteration < 3; iteration++) {
        fixture.sample(iteration + 2);
        await fixture.app.refreshSnapshot();
        await tester.pump(const Duration(seconds: 1));
        await tester.ensureVisible(details);
        await tester.pumpAndSettle();
        await tester.tap(details);
        await tester.pumpAndSettle();
        expect(expansionState(), isFalse);
        expect(address, findsNothing);
        await tester.tap(details);
        await tester.pumpAndSettle();
        expect(expansionState(), isTrue);
        expect(address, findsOneWidget);
      }
      fixture.engine.current = const EngineSnapshot();
      await fixture.app.refreshSnapshot();
      await tester.pumpAndSettle();
      expect(address, findsNothing);
      fixture.sample(8);
      await fixture.app.refreshSnapshot();
      await tester.pumpAndSettle();
      expect(address, findsOneWidget);
      expect(expansionState(), isTrue);
      fixture.app.selectSection(AppSection.settings);
      await tester.pumpAndSettle();
      fixture.app.selectSection(AppSection.home);
      await tester.pumpAndSettle();
      expect(expansionState(), isTrue);
      expect(address, findsOneWidget);
      // Recreate the compact subtree while retaining the route's PageStorage.
      tester.view.physicalSize = const Size(1280, 900);
      await tester.pumpAndSettle();
      tester.view.physicalSize = const Size(424, 924);
      await tester.pumpAndSettle();
      expect(expansionState(), isTrue);
      expect(address, findsOneWidget);
      expect(find.byType(ErrorWidget), findsNothing);
      expect(tester.takeException(), isNull);
      expect(fixture.engine.qualityRequests, 0);
    },
    variant: TargetPlatformVariant.only(TargetPlatform.android),
  );

  testWidgets(
    'expanded Android details fit small screens, landscape and large text',
    (tester) async {
      tester.view.devicePixelRatio = 1;
      addTearDown(tester.view.resetDevicePixelRatio);
      addTearDown(tester.view.resetPhysicalSize);
      for (final size in const [
        Size(320, 568),
        Size(375, 812),
        Size(424, 924),
        Size(812, 375),
      ]) {
        tester.view.physicalSize = size;
        for (final zh in [false, true]) {
          for (final scale in [1.0, 2.0]) {
            for (final reducedMotion in [false, true]) {
              final fixture = _Fixture()..sample(1);
              fixture.app.localePreference = zh
                  ? LocalePreference.simplifiedChinese
                  : LocalePreference.english;
              try {
                await tester.pumpWidget(
                  workflowHost(
                    fixture.app,
                    dark: !zh,
                    scale: scale,
                    reducedMotion: reducedMotion,
                  ),
                );
                await tester.pumpAndSettle();
                final details = find.text(
                  fixture.app.strings.get('connection_details'),
                );
                await tester.ensureVisible(details);
                await tester.pumpAndSettle();
                await tester.tap(details);
                await tester.pump(const Duration(milliseconds: 100));
                expect(tester.takeException(), isNull);
                await tester.pumpAndSettle();
                final address = find.widgetWithText(
                  SelectableText,
                  '198.51.100.10',
                );
                await tester.ensureVisible(address);
                await tester.pumpAndSettle();
                expect(address.hitTestable(), findsOneWidget);
                for (final value in tester.widgetList<MonoValue>(
                  find.byType(MonoValue),
                )) {
                  final rect = tester.getRect(find.byWidget(value));
                  expect(rect.height, lessThan(300));
                  expect(rect.left, greaterThanOrEqualTo(0));
                  expect(rect.right, lessThanOrEqualTo(size.width));
                }
                expect(find.byType(ErrorWidget), findsNothing);
                expect(
                  tester.takeException(),
                  isNull,
                  reason: '$size zh=$zh scale=$scale reduced=$reducedMotion',
                );
                await tester.pumpWidget(const SizedBox.shrink());
              } finally {
                fixture.app.dispose();
              }
            }
          }
        }
      }
    },
    variant: TargetPlatformVariant.only(TargetPlatform.android),
  );

  testWidgets(
    'mobile home centres the control above its traffic and status cards',
    (tester) async {
      final fixture = _Fixture()
        ..sample(0)
        ..sample(1)
        ..sample(2);
      await _show(tester, fixture);
      expect(find.byType(StatusPill), findsNothing);
      final ring = tester.getRect(find.byType(ConnectionRing));
      expect(ring.center.dx, closeTo(375 / 2, 0.1));
      final traffic = tester.getRect(
        find.byKey(const ValueKey('home-traffic')),
      );
      final status = tester.getRect(find.byKey(const ValueKey('home-status')));
      expect(traffic.top, greaterThan(ring.bottom));
      expect(status.top, greaterThan(traffic.bottom));
      expect(find.byType(Panel), findsNWidgets(2));
      expect(find.text('Singapore'), findsOneWidget);
      expect(
        tester.getRect(find.text('Singapore')).top,
        greaterThan(ring.bottom),
      );
      expect(
        find.descendant(
          of: find.byKey(const ValueKey('home-protocol')),
          matching: find.text('HTTP/3 · IPv4'),
        ),
        findsOneWidget,
      );
      expect(find.text('198.51.100.10'), findsNothing);
      expect(
        tester.widget<LiveDuration>(find.byType(LiveDuration)).since,
        fixture.since,
      );
      expect(
        find.descendant(
          of: find.byKey(const ValueKey('home-status')),
          matching: find.text('Kill Switch'),
        ),
        findsOneWidget,
      );
      // The account opens Accounts from the page heading.
      expect(find.byKey(const ValueKey('home-account-chip')), findsOneWidget);
      for (final key in ['home-network-quality', 'home-diagnostics']) {
        expect(find.byKey(ValueKey(key)), findsNothing);
      }
      expect(find.text('Connection details'), findsOneWidget);
      expect(tester.takeException(), isNull);
      fixture.app.selectSection(AppSection.settings);
      await tester.pumpAndSettle();
      expect(find.text('Network quality'), findsOneWidget);
      expect(find.text('Diagnostics'), findsOneWidget);
      expect(tester.takeException(), isNull);
    },
  );

  testWidgets(
    'traffic curves use timestamped samples including unchanged rates',
    (tester) async {
      final fixture = _Fixture()
        ..sample(0)
        ..sample(1)
        ..sample(2);
      await _show(tester, fixture);
      expect(_trace(tester, 'download').whereType<int>(), [2048, 2048]);
      expect(_trace(tester, 'upload').whereType<int>(), [1024, 1024]);
      final length = fixture.quality.history.length;
      await tester.pumpWidget(workflowHost(fixture.app, dark: true));
      await tester.pumpAndSettle();
      expect(fixture.quality.history.length, length);
      expect(fixture.engine.qualityRequests, 0);
      fixture.sample(3);
      await tester.pump();
      expect(_trace(tester, 'download').whereType<int>(), [2048, 2048, 2048]);
      fixture.now = fixture.now.add(const Duration(seconds: 4));
      fixture.quality.markStreamUnavailable(true);
      await tester.pump();
      expect(find.text('Traffic updates delayed'), findsOneWidget);
      expect(_trace(tester, 'download').last, isNull);
      expect(_trace(tester, 'download').whereType<int>().length, 3);
      fixture.app.snapshot = const EngineSnapshot();
      await tester.pumpWidget(workflowHost(fixture.app));
      await tester.pumpAndSettle();
      expect(_trace(tester, 'download'), isEmpty);
      expect(_trace(tester, 'upload'), isEmpty);
      expect(find.text('Singapore'), findsNothing);
      expect(
        find.text('VPN and proxies for the next connection'),
        findsOneWidget,
      );
      expect(find.text('Traffic appears after connecting'), findsOneWidget);
      expect(fixture.engine.qualityRequests, 0);
    },
  );

  testWidgets('empty, unsupported and paused history never invent curves', (
    tester,
  ) async {
    final fixture = _Fixture();
    fixture.app.snapshot = const EngineSnapshot(
      phase: ConnectionPhase.connected,
    );
    await _show(tester, fixture);
    expect(find.text('Waiting for traffic data'), findsOneWidget);
    expect(_trace(tester, 'download').whereType<int>(), isEmpty);
    fixture.app.engineCapabilities = const EngineCapabilities();
    await tester.pumpAndSettle();
    expect(find.text('Traffic history unavailable'), findsOneWidget);
    expect(_trace(tester, 'download').whereType<int>(), isEmpty);
    fixture.app.engineCapabilities = const EngineCapabilities(
      networkQuality: true,
    );
    fixture.sample(0);
    fixture.sample(1);
    fixture.quality.togglePaused();
    await tester.pumpAndSettle();
    final samples = _trace(tester, 'download');
    fixture.sample(2);
    await tester.pumpAndSettle();
    expect(_trace(tester, 'download'), samples);
    expect(find.text(fixture.app.strings.get('nq_paused')), findsOneWidget);
    expect(fixture.engine.qualityRequests, 0);
  });

  testWidgets(
    'blocked recovery retains its explanation without enabling retry',
    (tester) async {
      final fixture = _Fixture();
      fixture.app.snapshot = const EngineSnapshot(
        phase: ConnectionPhase.error,
        errorCode: 'WINDOWS_RECOVERY_BLOCKED',
      );
      fixture.app.lastError = 'Recovery requires attention.';
      await _show(tester, fixture);
      expect(find.text('Recovery requires attention.'), findsOneWidget);
      expect(find.text('Retry'), findsNothing);
      expect(
        tester.widget<ConnectionRing>(find.byType(ConnectionRing)).onPressed,
        isNull,
      );
      expect(find.byKey(const ValueKey('home-diagnostics')), findsNothing);
      expect(find.byKey(const ValueKey('home-network-quality')), findsNothing);
      expect(find.text('Connection details'), findsOneWidget);
      expect(_trace(tester, 'download'), isEmpty);
    },
  );

  testWidgets(
    'small phones retain safe-area clearance and scroll large error text',
    (tester) async {
      final fixture = _Fixture();
      fixture.app.snapshot = const EngineSnapshot(
        phase: ConnectionPhase.error,
        errorCode: 'TEST_FAILED',
      );
      fixture.app.lastError =
          'The connection is unavailable. Review the network and retry.';
      fixture.app.profiles = [
        fixture.app.profiles.single.copyWith(
          name: 'A long account name for a small travel phone',
        ),
      ];
      tester.view.padding = const FakeViewPadding(top: 24, bottom: 24);
      tester.view.viewPadding = const FakeViewPadding(top: 24, bottom: 24);
      addTearDown(tester.view.resetPadding);
      addTearDown(tester.view.resetViewPadding);
      await _show(tester, fixture, size: const Size(320, 568));
      await tester.pumpWidget(workflowHost(fixture.app, scale: 2, dark: true));
      await tester.pumpAndSettle();
      expect(tester.takeException(), isNull);
      final details = find.text('Connection details');
      await tester.ensureVisible(details);
      await tester.pumpAndSettle();
      expect(details.hitTestable(), findsOneWidget);
      expect(
        tester.getRect(details).bottom,
        lessThanOrEqualTo(tester.getTopLeft(find.byType(NavigationBar)).dy),
      );
      expect(
        tester.getRect(find.byType(NavigationBar)).bottom,
        closeTo(568 - 24, 0.1),
      );
      expect(tester.takeException(), isNull);
    },
  );

  testWidgets('disconnected phone rates show a dash instead of zero', (
    tester,
  ) async {
    final fixture = _Fixture();
    await _show(tester, fixture);
    for (final direction in ['download', 'upload']) {
      expect(
        tester.widget<Text>(find.byKey(ValueKey('home-$direction-rate'))).data,
        '—',
      );
    }
    expect(find.text('0 B/s'), findsNothing);
  });

  testWidgets('a stream outage without history is delayed, not waiting', (
    tester,
  ) async {
    final fixture = _Fixture();
    fixture.app.snapshot = const EngineSnapshot(
      phase: ConnectionPhase.connected,
      downloadBytesPerSecond: 2048,
    );
    await _show(tester, fixture);
    final strings = fixture.app.strings;
    expect(find.text(strings.get('home_traffic_waiting')), findsOneWidget);
    fixture.quality.markStreamUnavailable(true);
    await tester.pumpAndSettle();
    expect(find.text(strings.get('home_traffic_waiting')), findsNothing);
    expect(find.text(strings.get('home_traffic_stale')), findsOneWidget);
    expect(_trace(tester, 'download').whereType<int>(), isEmpty);
  });

  testWidgets('home error banner names only connection failures', (
    tester,
  ) async {
    final fixture = _Fixture();
    fixture.app.lastError = 'Saved accounts were reset.';
    await _show(tester, fixture);
    final strings = fixture.app.strings;
    expect(find.text('Saved accounts were reset.'), findsOneWidget);
    expect(find.text(strings.get('error_generic')), findsOneWidget);
    expect(find.text(strings.get('error')), findsNothing);

    fixture.app.snapshot = const EngineSnapshot(
      phase: ConnectionPhase.error,
      errorCode: 'TEST_FAILED',
    );
    fixture.app.lastError = 'The connection is unavailable.';
    fixture.app.selectSection(AppSection.home);
    await tester.pumpAndSettle();
    expect(find.text('The connection is unavailable.'), findsOneWidget);
    // The status heading names the failure; the banner adds only the message.
    expect(find.text(strings.get('error')), findsOneWidget);
    expect(find.text(strings.get('error_generic')), findsNothing);
  });
}
