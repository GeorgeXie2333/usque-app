import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/widgets/vpn_gate_server_row.dart';

import 'vpn_gate_server_row_test.dart' show observationServer;
import 'vpngate_test.dart' show GateEngine, host;

class _PagedGateEngine extends GateEngine {
  final requests =
      <({int offset, int limit, bool favoritesOnly, bool statusOnly})>[];
  String refreshStage = 'complete';
  Completer<VpnGateDirectory>? pendingStatus;

  @override
  Future<void> refreshVpnGate({bool cancel = false}) async {
    await super.refreshVpnGate(cancel: cancel);
    refreshStage = cancel ? 'cancelled' : 'primary_raw';
  }

  @override
  Future<VpnGateDirectory> listVpnGate({
    String? countryCode,
    bool unknownCountry = false,
    int offset = 0,
    int limit = 50,
    bool favoritesOnly = false,
    bool statusOnly = false,
  }) async {
    requests.add((
      offset: offset,
      limit: limit,
      favoritesOnly: favoritesOnly,
      statusOnly: statusOnly,
    ));
    if (statusOnly) {
      return pendingStatus?.future ??
          VpnGateDirectory(refreshStage: refreshStage);
    }
    final all = await super.listVpnGate(
      countryCode: countryCode,
      unknownCountry: unknownCountry,
      favoritesOnly: favoritesOnly,
      statusOnly: statusOnly,
    );
    return VpnGateDirectory(
      servers: all.servers.skip(offset).take(limit).toList(),
      countries: all.countries,
      total: all.total,
      fetchedAt: all.fetchedAt,
      refreshStage: refreshStage,
      favoriteCount: all.favoriteCount,
    );
  }
}

void main() {
  testWidgets(
    'Android builds only nearby nodes from a full directory page',
    (tester) async {
      final engine = GateEngine()
        ..nodes = List.generate(50, (i) => observationServer(id: 'node-$i'));
      await host(tester, engine, size: const Size(390, 844));

      expect(
        find.byType(VpnGateServerRow, skipOffstage: false).evaluate().length,
        lessThan(10),
      );
      expect(find.byKey(const ValueKey('vpn-gate-node-node-49')), findsNothing);
      await tester.scrollUntilVisible(
        find.byKey(const ValueKey('vpn-gate-node-node-49')),
        600,
        scrollable: find.byType(Scrollable).first,
        maxScrolls: 60,
      );
      await tester.pumpAndSettle();
      expect(
        find.byType(VpnGateServerRow, skipOffstage: false).evaluate().length,
        lessThan(10),
      );
      expect(find.byKey(const ValueKey('vpn-gate-node-node-0')), findsNothing);
      expect(tester.takeException(), isNull);
      await tester.pumpWidget(const SizedBox());
    },
    variant: TargetPlatformVariant({TargetPlatform.android}),
  );

  testWidgets('unrelated controller notifications do not rebuild node rows', (
    tester,
  ) async {
    final engine = GateEngine()..nodes = [observationServer()];
    final app = await host(tester, engine);
    engine.current = const EngineSnapshot(phase: ConnectionPhase.connected);
    await app.refreshSnapshot(silent: true);
    await tester.pump();
    final row = tester.widget<VpnGateServerRow>(
      find.byType(VpnGateServerRow, skipOffstage: false),
    );
    for (var sample = 1; sample <= 3; sample++) {
      engine.current = EngineSnapshot(
        phase: ConnectionPhase.connected,
        downloadBytesPerSecond: sample * 1000,
        downloadedBytes: sample * 10000,
      );
      await app.refreshSnapshot(silent: true);
      await tester.pump();
      expect(
        tester.widget(find.byType(VpnGateServerRow, skipOffstage: false)),
        same(row),
      );
    }

    engine.current = const EngineSnapshot(phase: ConnectionPhase.connectingH3);
    await app.refreshSnapshot(silent: true);
    await tester.pump();
    expect(
      tester.widget(find.byType(VpnGateServerRow, skipOffstage: false)),
      isNot(same(row)),
    );
    await tester.pumpWidget(const SizedBox());
  });

  testWidgets(
    'evicted rows retain selection and details without retaining their widgets',
    (tester) async {
      final engine = GateEngine()
        ..nodes = List.generate(50, (i) => observationServer(id: 'node-$i'));
      final app = await host(tester, engine, size: const Size(390, 844));
      await tester.tap(find.byKey(const ValueKey('vpn-gate-toggle')));
      await tester.pumpAndSettle();
      final first = find.byKey(const ValueKey('vpn-gate-node-node-0'));
      final last = find.byKey(const ValueKey('vpn-gate-node-node-49'));
      final details = find.byKey(const ValueKey('vpn-gate-details-node-0'));
      final expanded = find.byKey(
        const ValueKey('vpn-gate-observations-node-0'),
      );
      final scrollable = find.byType(Scrollable).first;
      await tester.scrollUntilVisible(first, 250, scrollable: scrollable);
      await tester.tap(first);
      await tester.pumpAndSettle();
      await tester.ensureVisible(details);
      await tester.tap(details);
      await tester.pumpAndSettle();
      expect(expanded, findsOneWidget);
      final original = tester.element(first);

      await tester.scrollUntilVisible(
        last,
        600,
        scrollable: scrollable,
        maxScrolls: 60,
      );
      await tester.pumpAndSettle();
      expect(first, findsNothing);
      expect(original.mounted, isFalse);
      await tester.scrollUntilVisible(
        first,
        -600,
        scrollable: scrollable,
        maxScrolls: 60,
      );
      await tester.pumpAndSettle();
      expect(tester.widget<ListTile>(first).selected, isTrue);
      expect(expanded, findsOneWidget);
      // Scrolling and expansion never apply the local draft.
      expect(app.activeProfile.vpnGate.hasSelection, isFalse);
      expect(engine.saves, 0);
      expect(tester.takeException(), isNull);
      await tester.pumpWidget(const SizedBox());
    },
    variant: TargetPlatformVariant({TargetPlatform.android}),
  );

  testWidgets(
    'paging and empty favorites remain reachable with a lazy directory',
    (tester) async {
      final engine = _PagedGateEngine()
        ..nodes = List.generate(60, (i) => observationServer(id: 'node-$i'));
      final app = await host(tester, engine, size: const Size(390, 844));
      final scrollable = find.byType(Scrollable).first;
      await tester.scrollUntilVisible(
        find.byKey(const ValueKey('vpn-gate-node-node-49')),
        600,
        scrollable: scrollable,
        maxScrolls: 60,
      );
      final next = find.byTooltip(app.strings.get('gate_next'));
      await tester.ensureVisible(next);
      await tester.pumpAndSettle();
      await tester.tap(next);
      await tester.pumpAndSettle();
      expect(engine.requests.last, (
        offset: 50,
        limit: 50,
        favoritesOnly: false,
        statusOnly: false,
      ));
      await tester.scrollUntilVisible(
        find.byKey(const ValueKey('vpn-gate-node-node-50')),
        -400,
        scrollable: scrollable,
        maxScrolls: 60,
      );
      expect(find.byKey(const ValueKey('vpn-gate-node-node-49')), findsNothing);

      final favorites = find.byKey(const ValueKey('vpn-gate-favorites'));
      await tester.scrollUntilVisible(favorites, -400, scrollable: scrollable);
      await tester.pumpAndSettle();
      await tester.tap(favorites);
      await tester.pumpAndSettle();
      expect(engine.requests.last, (
        offset: 0,
        limit: 50,
        favoritesOnly: true,
        statusOnly: false,
      ));
      expect(find.byType(VpnGateServerRow, skipOffstage: false), findsNothing);
      expect(
        find.text(app.strings.get('gate_favorites_empty')),
        findsOneWidget,
      );
      expect(tester.takeException(), isNull);
      await tester.pumpWidget(const SizedBox());
    },
    variant: TargetPlatformVariant({TargetPlatform.android}),
  );

  testWidgets(
    '3000-node refresh polls status without repeatedly loading the directory',
    (tester) async {
      final engine = _PagedGateEngine()
        ..nodes = List.generate(3000, (i) => observationServer(id: 'node-$i'));
      final app = await host(tester, engine, size: const Size(390, 844));
      expect(engine.requests.where((r) => !r.statusOnly).length, 1);
      expect(
        find.byType(VpnGateServerRow, skipOffstage: false).evaluate().length,
        lessThan(10),
      );
      final first = find.byKey(const ValueKey('vpn-gate-node-node-0'));
      await tester.tap(find.text(app.strings.get('gate_refresh')));
      await tester.pump();
      await tester.scrollUntilVisible(
        first,
        250,
        scrollable: find.byType(Scrollable).first,
      );
      await tester.pump();
      final initialStatusRequests = engine.requests
          .where((r) => r.statusOnly)
          .length;
      final row = tester
          .widgetList<VpnGateServerRow>(
            find.byType(VpnGateServerRow, skipOffstage: false),
          )
          .first;

      for (var tick = 0; tick < 5; tick++) {
        await tester.pump(const Duration(seconds: 1));
        expect(engine.requests.where((r) => !r.statusOnly).length, 1);
        expect(
          tester
              .widgetList<VpnGateServerRow>(
                find.byType(VpnGateServerRow, skipOffstage: false),
              )
              .first,
          same(row),
        );
      }
      expect(
        engine.requests.where((r) => r.statusOnly).length,
        initialStatusRequests + 5,
      );
      expect(
        find.byType(LinearProgressIndicator, skipOffstage: false),
        findsOneWidget,
      );
      engine.refreshStage = 'complete';
      engine.nodes[0] = observationServer(id: 'updated');
      await tester.pump(const Duration(seconds: 1));
      await tester.pumpAndSettle();
      expect(engine.requests.where((r) => !r.statusOnly).length, 2);
      expect(
        find.byKey(const ValueKey('vpn-gate-node-updated')),
        findsOneWidget,
      );
      expect(
        find.byType(LinearProgressIndicator, skipOffstage: false),
        findsNothing,
      );
      final completedRequests = engine.requests.length;
      await tester.pump(const Duration(seconds: 3));
      expect(engine.requests.length, completedRequests);
      expect(tester.takeException(), isNull);
      await tester.pumpWidget(const SizedBox());
    },
    variant: TargetPlatformVariant({TargetPlatform.android}),
  );

  testWidgets(
    'refresh status polls stay single flight and ignore a superseded filter',
    (tester) async {
      final engine = _PagedGateEngine()..nodes = [observationServer()];
      final app = await host(tester, engine);
      await tester.tap(find.text(app.strings.get('gate_refresh')));
      await tester.pump();
      final pending = engine.pendingStatus = Completer<VpnGateDirectory>();
      await tester.pump(const Duration(seconds: 1));
      await tester.pump(const Duration(seconds: 3));
      expect(engine.requests.where((r) => r.statusOnly).length, 1);

      await tester.tap(find.byKey(const ValueKey('vpn-gate-favorites')));
      await tester.pump();
      expect(engine.requests.where((r) => !r.statusOnly).length, 2);
      expect(engine.requests.last.favoritesOnly, isTrue);
      pending.complete(const VpnGateDirectory(refreshStage: 'complete'));
      await tester.pump();
      expect(engine.requests.where((r) => !r.statusOnly).length, 2);

      engine.pendingStatus = null;
      engine.refreshStage = 'complete';
      await tester.pump(const Duration(seconds: 1));
      await tester.pumpAndSettle();
      expect(engine.requests.where((r) => !r.statusOnly).length, 3);
      expect(engine.requests.last.favoritesOnly, isTrue);
      expect(
        find.text(app.strings.get('gate_favorites_empty')),
        findsOneWidget,
      );
      expect(
        find.byType(LinearProgressIndicator, skipOffstage: false),
        findsNothing,
      );
      expect(tester.takeException(), isNull);
      await tester.pumpWidget(const SizedBox());
    },
    variant: TargetPlatformVariant({TargetPlatform.android}),
  );
}
