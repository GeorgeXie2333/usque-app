import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:usque/core/usque_theme.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/screens/vpn_gate_screen.dart';
import 'package:usque/state/app_controller.dart';
import 'package:usque/widgets/zero_trust_enrollment_editor.dart';

import 'app_test.dart' show FakeEngineClient;
import 'vpngate_test.dart' show GateEngine;

Widget shell(Widget child) => MaterialApp(
  theme: UsqueTheme.light(),
  home: Scaffold(body: SingleChildScrollView(child: child)),
);

class HeldRefreshEngine extends GateEngine {
  final refreshReply = Completer<void>();
  @override
  Future<void> refreshVpnGate({bool cancel = false}) async {
    await super.refreshVpnGate(cancel: cancel);
    if (!cancel) await refreshReply.future;
  }
}

void main() {
  for (final change in ['dispose', 'disable', 'team', 'manual', 'new paste']) {
    testWidgets('clipboard reply is retired after $change', (tester) async {
      final replies = <Completer<Object?>>[];
      final messenger =
          TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger;
      messenger.setMockMethodCallHandler(SystemChannels.platform, (call) {
        if (call.method == 'Clipboard.getData') {
          final reply = Completer<Object?>();
          replies.add(reply);
          return reply.future;
        }
        return Future<Object?>.value();
      });
      addTearDown(
        () => messenger.setMockMethodCallHandler(SystemChannels.platform, null),
      );
      final app = AppController(FakeEngineClient())
        ..localePreference = LocalePreference.english;
      addTearDown(app.dispose);
      Widget editor(bool enabled) => shell(
        ZeroTrustEnrollmentEditor(
          controller: app,
          enabled: enabled,
          initialTeam: 'example-team',
        ),
      );
      await tester.pumpWidget(editor(true));
      await tester.pumpAndSettle();
      final paste = find.text(app.strings.get('zero_trust_paste_clipboard'));
      await tester.ensureVisible(paste);
      await tester.tap(paste);
      await tester.pump();
      switch (change) {
        case 'dispose':
          await tester.pumpWidget(const SizedBox());
        case 'disable':
          await tester.pumpWidget(editor(false));
        case 'team':
          await tester.enterText(find.byType(TextField).first, 'another-team');
        case 'manual':
          await tester.enterText(find.byType(TextField).last, 'manual-value');
        case 'new paste':
          await tester.tap(paste);
          await tester.pump();
          replies.last.complete({'text': 'new-value'});
          await tester.pump();
      }
      replies.first.complete({'text': 'stale-value'});
      await tester.pumpAndSettle();
      expect(tester.takeException(), isNull);
      if (change != 'dispose') {
        final value = tester
            .widget<TextField>(find.byType(TextField).last)
            .controller!
            .text;
        expect(
          value,
          change == 'manual'
              ? 'manual-value'
              : change == 'new paste'
              ? 'new-value'
              : '',
        );
      }
      await tester.pumpWidget(const SizedBox());
    });
  }

  testWidgets('old completed directory query cannot release a newer refresh', (
    tester,
  ) async {
    final engine = GateEngine()..pending = Completer<VpnGateDirectory>();
    final app = AppController(engine)
      ..localePreference = LocalePreference.english;
    addTearDown(app.dispose);
    await tester.pumpWidget(
      MaterialApp(
        theme: UsqueTheme.light(),
        home: VpnGateScreen(controller: app),
      ),
    );
    tester
        .widget<TextButton>(
          find.widgetWithText(TextButton, app.strings.get('gate_refresh')),
        )
        .onPressed!();
    await tester.pump();
    expect(engine.refreshes, 1);
    engine.pending!.complete(
      VpnGateDirectory(fetchedAt: DateTime.now(), refreshStage: 'complete'),
    );
    await tester.pump();
    await tester.pumpWidget(const SizedBox());
    await tester.pump();
    expect(engine.cancellations, 1);
  });

  testWidgets(
    'leaving while refresh dispatch is pending stops it after acknowledgement',
    (tester) async {
      final engine = HeldRefreshEngine();
      final app = AppController(engine)
        ..localePreference = LocalePreference.english;
      addTearDown(app.dispose);
      await tester.pumpWidget(
        MaterialApp(
          theme: UsqueTheme.light(),
          home: VpnGateScreen(controller: app),
        ),
      );
      await tester.pumpAndSettle();
      tester
          .widget<TextButton>(
            find.widgetWithText(TextButton, app.strings.get('gate_refresh')),
          )
          .onPressed!();
      await tester.pump();
      await tester.pumpWidget(const SizedBox());
      expect(engine.cancellations, 0);
      engine.refreshReply.complete();
      await tester.pump();
      expect(engine.cancellations, 1);
      expect(tester.takeException(), isNull);
    },
  );
}
