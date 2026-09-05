import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/state/app_controller.dart';
import 'package:usque/state/network_quality_controller.dart';
import 'package:usque/widgets/common.dart';
import 'package:usque/widgets/connection_ring.dart';
import 'package:usque/widgets/live_duration.dart';
import 'package:usque/widgets/sparkline.dart';

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

Sparkline _trace(WidgetTester tester, String direction) =>
    tester.widget<Sparkline>(find.byKey(ValueKey('home-$direction-trace')));

void main() {
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
      final protocol = find.widgetWithText(SelectableText, 'HTTP/3');
      expect(protocol, findsOneWidget);
      expect(tester.getSize(protocol).height, lessThan(100));

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
      await tester.ensureVisible(protocol);
      await tester.pumpAndSettle();
      await tester.longPress(protocol);
      await tester.pumpAndSettle();
      final editable = tester.state<EditableTextState>(
        find.descendant(of: protocol, matching: find.byType(EditableText)),
      );
      expect(editable.widget.controller.selection.isCollapsed, isFalse);
      editable.selectAll(SelectionChangedCause.toolbar);
      editable.copySelection(SelectionChangedCause.toolbar);
      await tester.pump();
      expect(copied, 'HTTP/3');
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
        expect(protocol, findsNothing);
        await tester.tap(details);
        await tester.pumpAndSettle();
        expect(expansionState(), isTrue);
        expect(protocol, findsOneWidget);
      }
      fixture.engine.current = const EngineSnapshot();
      await fixture.app.refreshSnapshot();
      await tester.pumpAndSettle();
      expect(protocol, findsNothing);
      fixture.sample(8);
      await fixture.app.refreshSnapshot();
      await tester.pumpAndSettle();
      expect(protocol, findsOneWidget);
      expect(expansionState(), isTrue);
      fixture.app.selectSection(AppSection.settings);
      await tester.pumpAndSettle();
      fixture.app.selectSection(AppSection.home);
      await tester.pumpAndSettle();
      expect(expansionState(), isTrue);
      expect(protocol, findsOneWidget);
      // Recreate the compact subtree while retaining the route's PageStorage.
      tester.view.physicalSize = const Size(1280, 900);
      await tester.pumpAndSettle();
      tester.view.physicalSize = const Size(424, 924);
      await tester.pumpAndSettle();
      expect(expansionState(), isTrue);
      expect(protocol, findsOneWidget);
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
                final protocol = find.widgetWithText(SelectableText, 'HTTP/3');
                await tester.ensureVisible(protocol);
                await tester.pumpAndSettle();
                expect(protocol.hitTestable(), findsOneWidget);
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

  testWidgets('mobile home has three cards and a centered connection control', (
    tester,
  ) async {
    final fixture = _Fixture()
      ..sample(0)
      ..sample(1)
      ..sample(2);
    await _show(tester, fixture);
    expect(find.byType(Panel), findsNWidgets(3));
    final ring = tester.getRect(find.byType(ConnectionRing));
    expect(ring.center.dx, closeTo(375 / 2, 0.1));
    expect(find.text('Singapore'), findsOneWidget);
    expect(find.text('Protocol · HTTP/3 · IPv4'), findsOneWidget);
    expect(find.text('198.51.100.10'), findsNothing);
    expect(
      tester.widget<LiveDuration>(find.byType(LiveDuration)).since,
      fixture.since,
    );
    final protectionPanel = find.ancestor(
      of: find.text('Kill Switch'),
      matching: find.byType(Panel),
    );
    expect(tester.getRect(protectionPanel).contains(ring.center), isTrue);
    for (final key in ['home-network-quality', 'home-diagnostics']) {
      final button = find.byKey(ValueKey(key));
      expect(tester.widget(button), isA<OutlinedButton>());
      expect(tester.getSize(button).height, greaterThanOrEqualTo(48));
    }
    expect(tester.takeException(), isNull);
  });

  testWidgets(
    'traffic curves use timestamped samples including unchanged rates',
    (tester) async {
      final fixture = _Fixture()
        ..sample(0)
        ..sample(1)
        ..sample(2);
      await _show(tester, fixture);
      expect(_trace(tester, 'download').samples.whereType<int>(), [2048, 2048]);
      expect(_trace(tester, 'upload').samples.whereType<int>(), [1024, 1024]);
      final length = fixture.quality.history.length;
      await tester.pumpWidget(workflowHost(fixture.app, dark: true));
      await tester.pumpAndSettle();
      expect(fixture.quality.history.length, length);
      expect(fixture.engine.qualityRequests, 0);
      fixture.sample(3);
      await tester.pump();
      expect(_trace(tester, 'download').samples.whereType<int>(), [
        2048,
        2048,
        2048,
      ]);
      fixture.now = fixture.now.add(const Duration(seconds: 4));
      fixture.quality.markStreamUnavailable(true);
      await tester.pump();
      expect(find.text('Samples delayed'), findsOneWidget);
      expect(_trace(tester, 'download').samples.last, isNull);
      expect(_trace(tester, 'download').samples.whereType<int>().length, 3);
      fixture.app.snapshot = const EngineSnapshot();
      await tester.pumpWidget(workflowHost(fixture.app));
      await tester.pumpAndSettle();
      expect(_trace(tester, 'download').samples, isEmpty);
      expect(_trace(tester, 'upload').samples, isEmpty);
      expect(find.text('Singapore'), findsNothing);
      expect(find.text('Outputs enabled after connecting'), findsOneWidget);
      expect(find.text('Starts after connecting'), findsOneWidget);
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
    expect(find.text('Waiting for samples'), findsOneWidget);
    expect(_trace(tester, 'download').samples.whereType<int>(), isEmpty);
    fixture.app.engineCapabilities = const EngineCapabilities();
    await tester.pumpAndSettle();
    expect(find.text('History unavailable'), findsOneWidget);
    expect(_trace(tester, 'download').samples.whereType<int>(), isEmpty);
    fixture.app.engineCapabilities = const EngineCapabilities(
      networkQuality: true,
    );
    fixture.sample(0);
    fixture.sample(1);
    fixture.quality.togglePaused();
    await tester.pumpAndSettle();
    final samples = _trace(tester, 'download').samples;
    fixture.sample(2);
    await tester.pumpAndSettle();
    expect(_trace(tester, 'download').samples, samples);
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
      final diagnostics = find.byKey(const ValueKey('home-diagnostics'));
      await tester.ensureVisible(diagnostics);
      await tester.pumpAndSettle();
      expect(diagnostics.hitTestable(), findsOneWidget);
      expect(tester.widget<OutlinedButton>(diagnostics).onPressed, isNotNull);
      expect(_trace(tester, 'download').samples, isEmpty);
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
      final diagnostics = find.byKey(const ValueKey('home-diagnostics'));
      await tester.ensureVisible(diagnostics);
      await tester.pumpAndSettle();
      expect(diagnostics.hitTestable(), findsOneWidget);
      expect(
        tester.getRect(diagnostics).bottom,
        lessThanOrEqualTo(tester.getTopLeft(find.byType(NavigationBar)).dy),
      );
      expect(
        tester.getRect(find.byType(NavigationBar)).bottom,
        closeTo(568 - 24, 0.1),
      );
      expect(tester.takeException(), isNull);
    },
  );
}
