import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:usque/widgets/live_duration.dart';

Widget _host({
  required DateTime? since,
  required DateTime Function() now,
  bool tickerEnabled = true,
  GlobalKey<NavigatorState>? navigatorKey,
}) => MaterialApp(
  navigatorKey: navigatorKey,
  home: Scaffold(
    body: TickerMode(
      enabled: tickerEnabled,
      child: LiveDuration(since: since, now: now),
    ),
  ),
);

void main() {
  testWidgets('ticks before the first lifecycle notification', (tester) async {
    tester.binding.resetInternalState();
    expect(tester.binding.lifecycleState, isNull);
    final since = DateTime(2026, 10, 10);
    var now = since.add(const Duration(seconds: 2));
    await tester.pumpWidget(_host(since: since, now: () => now));
    expect(find.text('00:00:02'), findsOneWidget);

    now = now.add(const Duration(seconds: 1));
    await tester.pump(const Duration(seconds: 1));
    expect(find.text('00:00:03'), findsOneWidget);
  });

  testWidgets('muted sections stop ticking and catch up when shown', (
    tester,
  ) async {
    final since = DateTime(2026, 10, 10);
    var now = since.add(const Duration(seconds: 2));
    var clockReads = 0;
    DateTime clock() {
      clockReads++;
      return now;
    }

    await tester.pumpWidget(
      _host(since: since, now: clock, tickerEnabled: false),
    );
    var lastReads = clockReads;
    now = now.add(const Duration(minutes: 2));
    await tester.pump(const Duration(minutes: 2));
    expect(clockReads, lastReads);
    expect(find.text('00:00:02'), findsOneWidget);

    await tester.pumpWidget(_host(since: since, now: clock));
    expect(find.text('00:02:02'), findsOneWidget);
    now = now.add(const Duration(seconds: 1));
    await tester.pump(const Duration(seconds: 1));
    expect(find.text('00:02:03'), findsOneWidget);

    await tester.pumpWidget(
      _host(since: since, now: clock, tickerEnabled: false),
    );
    lastReads = clockReads;
    now = now.add(const Duration(minutes: 3));
    await tester.pump(const Duration(minutes: 3));
    expect(clockReads, lastReads);

    await tester.pumpWidget(_host(since: since, now: clock));
    expect(find.text('00:05:03'), findsOneWidget);
  });

  for (final platform in [TargetPlatform.android, TargetPlatform.windows]) {
    testWidgets(
      'hidden app catches up immediately and inactive stays visible on $platform',
      (tester) async {
        final since = DateTime(2026, 10, 10);
        var now = since;
        tester.binding.handleAppLifecycleStateChanged(AppLifecycleState.hidden);
        await tester.pumpWidget(_host(since: since, now: () => now));

        now = now.add(const Duration(minutes: 10));
        await tester.pump(const Duration(minutes: 10));
        tester.binding.handleAppLifecycleStateChanged(
          AppLifecycleState.inactive,
        );
        await tester.pump();
        expect(find.text('00:10:00'), findsOneWidget);
        now = now.add(const Duration(seconds: 1));
        await tester.pump(const Duration(seconds: 1));
        expect(find.text('00:10:01'), findsOneWidget);

        for (final state in [
          AppLifecycleState.hidden,
          AppLifecycleState.paused,
          AppLifecycleState.detached,
        ]) {
          tester.binding.handleAppLifecycleStateChanged(state);
          now = now.add(const Duration(minutes: 2));
          await tester.pump(const Duration(minutes: 2));
        }
        tester.binding.handleAppLifecycleStateChanged(
          AppLifecycleState.resumed,
        );
        await tester.pump();
        expect(find.text('00:16:01'), findsOneWidget);
        now = now.add(const Duration(seconds: 1));
        await tester.pump(const Duration(seconds: 1));
        expect(find.text('00:16:02'), findsOneWidget);
      },
      variant: TargetPlatformVariant({platform}),
    );
  }

  testWidgets('lifecycle changes do not resume a muted section', (
    tester,
  ) async {
    final since = DateTime(2026, 10, 10);
    var now = since;
    var clockReads = 0;
    DateTime clock() {
      clockReads++;
      return now;
    }

    await tester.pumpWidget(
      _host(since: since, now: clock, tickerEnabled: false),
    );
    tester.binding.handleAppLifecycleStateChanged(AppLifecycleState.hidden);
    tester.binding.handleAppLifecycleStateChanged(AppLifecycleState.resumed);
    final lastReads = clockReads;
    now = now.add(const Duration(minutes: 10));
    await tester.pump(const Duration(minutes: 10));
    expect(clockReads, lastReads);

    // Even a new connected interval must not restart a hidden section's timer.
    await tester.pumpWidget(
      _host(
        since: since.add(const Duration(minutes: 1)),
        now: clock,
        tickerEnabled: false,
      ),
    );
    final readsAfterUpdate = clockReads;
    now = now.add(const Duration(minutes: 1));
    await tester.pump(const Duration(minutes: 1));
    expect(clockReads, readsAfterUpdate);

    await tester.pumpWidget(
      _host(since: since.add(const Duration(minutes: 1)), now: clock),
    );
    expect(find.text('00:10:00'), findsOneWidget);
  });

  testWidgets('an opaque route pauses the covered readout until returning', (
    tester,
  ) async {
    final navigatorKey = GlobalKey<NavigatorState>();
    final since = DateTime(2026, 10, 10);
    var now = since;
    var clockReads = 0;
    DateTime clock() {
      clockReads++;
      return now;
    }

    await tester.pumpWidget(
      _host(since: since, now: clock, navigatorKey: navigatorKey),
    );
    navigatorKey.currentState!.push<void>(
      MaterialPageRoute<void>(
        builder: (_) => const Scaffold(body: Text('Other page')),
      ),
    );
    await tester.pumpAndSettle();
    final lastReads = clockReads;
    now = now.add(const Duration(minutes: 5));
    await tester.pump(const Duration(minutes: 5));
    expect(clockReads, lastReads);

    navigatorKey.currentState!.pop();
    await tester.pumpAndSettle();
    expect(find.text('00:05:00'), findsOneWidget);
  });

  testWidgets('updated interval and injected clock replace the old readout', (
    tester,
  ) async {
    final start = DateTime(2026, 10, 10);
    var now = start.add(const Duration(seconds: 4));
    var clockReads = 0;
    DateTime clock() {
      clockReads++;
      return now;
    }

    await tester.pumpWidget(_host(since: null, now: clock));
    expect(find.text('—'), findsOneWidget);
    expect(clockReads, 0);

    await tester.pumpWidget(_host(since: start, now: clock));
    expect(find.text('00:00:04'), findsOneWidget);
    await tester.pumpWidget(
      _host(since: start.add(const Duration(seconds: 2)), now: clock),
    );
    expect(find.text('00:00:02'), findsOneWidget);

    var replacementNow = start.add(const Duration(seconds: 22));
    await tester.pumpWidget(
      _host(
        since: start.add(const Duration(seconds: 2)),
        now: () => replacementNow,
      ),
    );
    expect(find.text('00:00:20'), findsOneWidget);
    replacementNow = replacementNow.add(const Duration(seconds: 1));
    await tester.pump(const Duration(seconds: 1));
    expect(find.text('00:00:21'), findsOneWidget);

    await tester.pumpWidget(_host(since: null, now: clock));
    final lastReads = clockReads;
    now = now.add(const Duration(minutes: 5));
    await tester.pump(const Duration(minutes: 5));
    expect(find.text('—'), findsOneWidget);
    expect(clockReads, lastReads);
  });

  testWidgets('dispose removes the timer and lifecycle observer', (
    tester,
  ) async {
    final since = DateTime(2026, 10, 10);
    var clockReads = 0;
    await tester.pumpWidget(
      _host(
        since: since,
        now: () {
          clockReads++;
          return since;
        },
      ),
    );
    await tester.pumpWidget(const SizedBox());
    final lastReads = clockReads;
    tester.binding.handleAppLifecycleStateChanged(AppLifecycleState.hidden);
    tester.binding.handleAppLifecycleStateChanged(AppLifecycleState.resumed);
    await tester.pump(const Duration(minutes: 5));
    expect(clockReads, lastReads);
    expect(tester.takeException(), isNull);
  });
}
