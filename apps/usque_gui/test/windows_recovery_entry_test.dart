import 'dart:async';

import 'package:flutter_test/flutter_test.dart';
import 'package:shared_preferences/shared_preferences.dart';
import 'package:usque/app.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/state/app_controller.dart';

import 'app_test.dart' show FakeEngineClient;

const exhausted = EngineSnapshot(
  phase: ConnectionPhase.error,
  errorCode: 'WINDOWS_RECOVERY_EXHAUSTED',
  errorRetryable: true,
);

void main() {
  setUp(() {
    SharedPreferences.setMockInitialValues(<String, Object>{
      'onboarding_complete': true,
      'update_checks_enabled': false,
    });
  });

  testWidgets('primary button connects after an exhausted recovery snapshot', (
    tester,
  ) async {
    final engine = FakeEngineClient()..current = exhausted;
    await tester.pumpWidget(UsqueBootstrap(engine: engine));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Connect'));
    await tester.pumpAndSettle();
    expect(find.text('Connected'), findsOneWidget);
    expect(engine.current.phase, ConnectionPhase.connected);
    expect(engine.calls.where((call) => call == 'connect').length, 1);
  });

  for (final retry in <bool>[false, true]) {
    test(
      '${retry ? 'Retry' : 'Connect'} follows recovery to a connected state',
      () async {
        final engine = FakeEngineClient()..current = exhausted;
        final controller = AppController(engine);
        addTearDown(controller.dispose);
        await controller.initialize();
        await Future<void>.delayed(Duration.zero);
        engine.pendingConnect = Completer<EngineSnapshot>();
        final request = retry
            ? controller.retry()
            : controller.connectOrDisconnect();
        await Future<void>.delayed(Duration.zero);
        engine.current = const EngineSnapshot(
          phase: ConnectionPhase.reconnecting,
        );
        engine.pendingConnect!.complete(engine.current);
        await request;
        expect(controller.snapshot.phase, ConnectionPhase.reconnecting);
        expect(controller.snapshot.errorCode, isNull);
        engine.current = const EngineSnapshot(
          phase: ConnectionPhase.connected,
          transport: 'HTTP/3',
        );
        await controller.refreshSnapshot();
        expect(controller.snapshot.phase, ConnectionPhase.connected);
        expect(controller.busy, isFalse);
      },
    );
  }

  test(
    'cancel during recovery cannot apply a late connected response',
    () async {
      final engine = FakeEngineClient()..current = exhausted;
      final controller = AppController(engine);
      addTearDown(controller.dispose);
      await controller.initialize();
      await Future<void>.delayed(Duration.zero);
      engine.pendingConnect = Completer<EngineSnapshot>();
      final request = controller.connectOrDisconnect();
      await Future<void>.delayed(Duration.zero);
      await controller.connectOrDisconnect();
      engine.pendingConnect!.complete(
        const EngineSnapshot(phase: ConnectionPhase.connected),
      );
      await request;
      expect(controller.snapshot.phase, ConnectionPhase.disconnected);
      expect(engine.calls.where((call) => call == 'disconnect').length, 1);
    },
  );
}
