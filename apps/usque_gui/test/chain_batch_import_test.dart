import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/screens/chain_proxy_screen.dart';
import 'package:usque/services/engine_client.dart';

import 'chain_proxy_test.dart' show ChainEngine, hostChain;
import 'ui_workflow_test.dart' show workflowHost;

class BatchEngine extends ChainEngine {
  final requests = <Map<String, Object?>>[];
  Completer<void>? checking, saving;
  int? interruptAt;
  int savesAttempted = 0;
  String? failName;

  @override
  Future<ChainProfileResult> chainProfile(Map<String, Object?> request) async {
    requests.add(Map.of(request));
    final action = request['action'];
    if (action == 'list') return ChainProfileResult(profiles: library);
    if (action == 'preview') await checking?.future;
    if (action == 'import') {
      savesAttempted++;
      await saving?.future;
      if (savesAttempted == interruptAt) throw StateError('transport lost');
      if (request['name'] == failName) {
        return const ChainProfileResult(error: {'reason': 'profile_limit'});
      }
    }
    final configuration = request['configuration'] as String;
    if (configuration == 'invalid') {
      return const ChainProfileResult(
        error: {'reason': 'invalid_key', 'field': 'PrivateKey', 'line': 2},
      );
    }
    final source = ChainSource.parse(request['source']);
    final profile = ChainProfileSummary(
      id: 'saved-$savesAttempted',
      revision: 'revision',
      editRevision: 'revision',
      name: request['name'] as String,
      source: source,
      protocol: source.isWireguard ? 'wireguard' : 'openvpn_tcp',
      host: '$configuration.example',
      port: 1194,
      requiresAuth: configuration == 'auth',
      requiresKeyPassword: configuration == 'key',
    );
    if (action == 'import') library = [...library, profile];
    return ChainProfileResult(profiles: library, preview: profile);
  }
}

List<ChainConfigurationFile> files(List<String> configurations) => [
  for (final text in configurations)
    ChainConfigurationFile(name: '$text.conf', configuration: text),
];

Finder get save => find.byKey(const ValueKey('chain-batch-save'));
Finder item(int index) => find.byKey(ValueKey('chain-batch-item-$index'));
Finder field(int index, String label) => find.descendant(
  of: item(index),
  matching: find.byWidgetPredicate(
    (w) => w is TextField && w.decoration?.labelText == label,
  ),
);

Future<void> open(WidgetTester tester) async {
  await tester.tap(find.text('Import file'));
  await tester.pumpAndSettle();
}

Future<void> expand(WidgetTester tester, int index) async {
  await tester.ensureVisible(item(index));
  await tester.tap(
    find.descendant(of: item(index), matching: find.byType(ListTile)).first,
  );
  await tester.pumpAndSettle();
}

void main() {
  testWidgets(
    'batch automatically checks each file and saves only valid entries without selecting',
    (tester) async {
      final engine = BatchEngine()
        ..pickedFiles = [
          ...files(['one', 'invalid', 'two']),
          const ChainConfigurationFile(
            name: 'unreadable.conf',
            errorCode: 'CHAIN_FILE_READ_FAILED',
          ),
          const ChainConfigurationFile(
            name: 'wrong.ovpn',
            configuration: 'client\ndev tun',
          ),
        ];
      final app = await hostChain(tester, engine);
      await tester.tap(find.byKey(const ValueKey('chain-proxy-toggle')));
      await tester.pumpAndSettle();
      final settings = app.activeProfile.chainExit;
      final phase = app.snapshot.phase;
      await open(tester);
      expect(
        engine.requests.where((r) => r['action'] == 'preview'),
        hasLength(3),
      );
      expect(
        find.text('Ready: 2 · Incomplete: 0 · Failed: 3 · Saved: 0'),
        findsOneWidget,
      );
      expect(find.textContaining('PrivateKey, line 2'), findsOneWidget);
      expect(find.textContaining('looks like an OpenVPN'), findsOneWidget);
      await expand(tester, 0);
      expect(
        tester.widget<TextField>(field(0, 'Name')).controller!.text,
        'one.example',
      );
      await tester.enterText(field(0, 'Name'), 'Office');
      await tester.pumpAndSettle();
      await tester.ensureVisible(save);
      await tester.tap(save);
      await tester.pumpAndSettle();
      expect(engine.library.map((p) => p.name), ['Office', 'two.example']);
      expect(
        find.text('Ready: 0 · Incomplete: 0 · Failed: 3 · Saved: 2'),
        findsOneWidget,
      );
      expect(tester.widget<FilledButton>(save).onPressed, isNull);
      await tester.tap(find.text('Close'));
      await tester.pumpAndSettle();
      expect(app.activeProfile.chainExit, settings);
      expect(app.snapshot.phase, phase);
      expect(engine.saves, 0);
      expect(
        tester
            .widget<SwitchListTile>(
              find.byKey(const ValueKey('chain-proxy-toggle')),
            )
            .value,
        isTrue,
      );
    },
  );

  testWidgets('all invalid files cannot be saved', (tester) async {
    final engine = BatchEngine()..pickedFiles = files(['invalid', 'invalid']);
    await hostChain(tester, engine);
    await open(tester);
    expect(tester.widget<FilledButton>(save).onPressed, isNull);
    expect(engine.savesAttempted, 0);
  });

  testWidgets(
    'credentials are per item and incomplete items can be completed after a partial save',
    (tester) async {
      final engine = BatchEngine()..pickedFiles = files(['one', 'auth', 'key']);
      await hostChain(tester, engine, source: ChainSource.openvpnCustom);
      await open(tester);
      expect(
        find.text('Ready: 1 · Incomplete: 2 · Failed: 0 · Saved: 0'),
        findsOneWidget,
      );
      await tester.tap(save);
      await tester.pumpAndSettle();
      expect(engine.library, hasLength(1));
      await expand(tester, 1);
      await tester.enterText(field(1, 'Username'), 'alice');
      await tester.enterText(field(1, 'Password'), 'fixture-password');
      expect(
        tester.widget<TextField>(field(1, 'Password')).obscureText,
        isTrue,
      );
      await tester.pumpAndSettle();
      await tester.ensureVisible(save);
      await tester.tap(save);
      await tester.pumpAndSettle();
      expect(engine.library, hasLength(2));
      final imported = engine.requests
          .where((r) => r['action'] == 'import')
          .toList();
      expect(imported[0]['username'], '');
      expect(imported[1]['username'], 'alice');
      expect(imported[1]['password'], 'fixture-password');
      expect(
        find.text('Ready: 0 · Incomplete: 1 · Failed: 0 · Saved: 2'),
        findsOneWidget,
      );
      expect(tester.widget<FilledButton>(save).onPressed, isNull);
    },
  );

  testWidgets(
    'cancel during validation ignores late replies and stops remaining previews',
    (tester) async {
      final gate = Completer<void>();
      final engine = BatchEngine()
        ..pickedFiles = files(['one', 'two'])
        ..checking = gate;
      await hostChain(tester, engine);
      await tester.tap(find.text('Import file'));
      await tester.pump();
      await tester.pump(const Duration(milliseconds: 300));
      expect(tester.widget<FilledButton>(save).onPressed, isNull);
      await tester.tap(find.text('Cancel'));
      await tester.pumpAndSettle();
      gate.complete();
      await tester.pumpAndSettle();
      expect(
        engine.requests.where((r) => r['action'] == 'preview'),
        hasLength(1),
      );
      expect(engine.savesAttempted, 0);
      expect(find.byType(AlertDialog), findsNothing);
      expect(tester.takeException(), isNull);
    },
  );

  testWidgets(
    'save blocks duplicate submissions and dismissal; interruption stops the queue',
    (tester) async {
      final gate = Completer<void>();
      final engine = BatchEngine()
        ..pickedFiles = files(['one', 'two', 'three'])
        ..saving = gate
        ..interruptAt = 2;
      await hostChain(tester, engine);
      await open(tester);
      final submit = tester.widget<FilledButton>(save).onPressed!;
      submit();
      submit();
      await tester.pump();
      expect(engine.savesAttempted, 1);
      expect(tester.widget<FilledButton>(save).onPressed, isNull);
      await tester.sendKeyEvent(LogicalKeyboardKey.escape);
      await tester.pump();
      expect(find.byType(AlertDialog), findsOneWidget);
      gate.complete();
      await tester.pumpAndSettle();
      expect(engine.savesAttempted, 2);
      expect(engine.library, hasLength(1));
      expect(engine.requests.last['action'], 'list');
      expect(find.textContaining('Saving was interrupted.'), findsWidgets);
      expect(tester.widget<FilledButton>(save).onPressed, isNull);
    },
  );

  testWidgets(
    'definite per-item failure does not roll back successful imports',
    (tester) async {
      final engine = BatchEngine()
        ..pickedFiles = files(['one', 'two', 'three'])
        ..failName = 'two.example';
      await hostChain(tester, engine);
      await open(tester);
      await tester.tap(save);
      await tester.pumpAndSettle();
      expect(engine.library.map((p) => p.name), [
        'one.example',
        'three.example',
      ]);
      expect(
        find.text('The configuration library is full (128 profiles).'),
        findsOneWidget,
      );
      expect(
        find.text('Ready: 0 · Incomplete: 0 · Failed: 1 · Saved: 2'),
        findsOneWidget,
      );
      await expand(tester, 1);
      await tester.enterText(field(1, 'Name'), 'renamed');
      await tester.pumpAndSettle();
      await tester.ensureVisible(save);
      await tester.tap(save);
      await tester.pumpAndSettle();
      expect(engine.library.map((p) => p.name), [
        'one.example',
        'three.example',
        'renamed',
      ]);
      expect(engine.savesAttempted, 4);
    },
  );

  testWidgets(
    'phone at 200 percent text supports keyboard expansion without overflow',
    (tester) async {
      final engine = BatchEngine()..pickedFiles = files(['one', 'two']);
      final app = await hostChain(tester, engine, width: 390);
      await tester.pumpWidget(
        workflowHost(app, scale: 2, home: ChainProxyScreen(controller: app)),
      );
      await tester.pumpAndSettle();
      await tester.scrollUntilVisible(
        find.text('Import file'),
        200,
        scrollable: find
            .descendant(
              of: find.byType(CustomScrollView),
              matching: find.byType(Scrollable),
            )
            .first,
      );
      await tester.pumpAndSettle();
      for (
        var i = 0;
        i < 5 && find.text('Import file').hitTestable().evaluate().isEmpty;
        i++
      ) {
        await tester.drag(find.byType(CustomScrollView), const Offset(0, -200));
        await tester.pumpAndSettle();
      }
      await open(tester);
      final tile = find
          .descendant(of: item(0), matching: find.byType(ListTile))
          .first;
      await tester.ensureVisible(tile);
      Focus.of(tester.element(find.text('one.conf'))).requestFocus();
      await tester.pump();
      await tester.sendKeyEvent(LogicalKeyboardKey.enter);
      await tester.pumpAndSettle();
      expect(field(0, 'Name'), findsOneWidget);
      expect(tester.takeException(), isNull);
    },
  );
}
