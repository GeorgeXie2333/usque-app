import 'dart:async';

import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:shared_preferences/shared_preferences.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/services/control_codec.dart';
import 'package:usque/services/desktop_engine_client.dart';
import 'package:usque/services/desktop_engine_transport.dart';
import 'package:usque/services/engine_client.dart';
import 'package:usque/services/platform_shell_bridge.dart';
import 'package:usque/state/app_controller.dart';

import 'app_test.dart' show FakeEngineClient;

class _CleanupEngine extends FakeEngineClient {
  final attempts = <Completer<EngineSnapshot>>[];

  @override
  Future<EngineSnapshot> disconnect() {
    final pending = Completer<EngineSnapshot>();
    attempts.add(pending);
    return pending.future;
  }
}

class _QueuedWritesEngine extends _CleanupEngine {
  final renames = <Completer<void>>[];
  final operations = <String>[];

  @override
  Future<void> renameProfile(String profileId, String name) async {
    operations.add('rename:$name');
    final pending = Completer<void>();
    renames.add(pending);
    await pending.future;
    await super.renameProfile(profileId, name);
  }

  @override
  Future<EngineSnapshot> disconnect() {
    operations.add('disconnect');
    return super.disconnect();
  }
}

const _channel = MethodChannel('io.github.georgexie2333.usque/engine');

Future<Object?> _requestQuit(WidgetTester tester) {
  final reply = Completer<Object?>();
  tester.binding.channelBuffers.push(
    _channel.name,
    const StandardMethodCodec().encodeMethodCall(
      const MethodCall('trayCommand', 'disconnectAndExit'),
    ),
    (bytes) {
      try {
        reply.complete(const StandardMethodCodec().decodeEnvelope(bytes!));
      } on Object catch (error, stack) {
        reply.completeError(error, stack);
      }
    },
  );
  return reply.future;
}

void main() {
  setUp(() {
    SharedPreferences.setMockInitialValues({
      'onboarding_complete': true,
      'update_checks_enabled': false,
    });
  });

  testWidgets(
    'Quit waits for cleanup even with a disconnected cached snapshot',
    (tester) async {
      final engine = _CleanupEngine();
      final app = AppController(engine);
      await app.initialize();
      final bridge = PlatformShellBridge(app);
      addTearDown(() {
        bridge.dispose();
        app.dispose();
      });
      expect(app.snapshot.phase, ConnectionPhase.disconnected);
      var completed = false;
      final quitting = _requestQuit(tester).then((value) {
        completed = true;
        return value;
      });
      await tester.pump();
      expect(engine.attempts, hasLength(1));
      expect(completed, isFalse);
      expect(app.connectionActionEnabled, isFalse);
      expect(app.nativeNetworkActionsAllowed, isFalse);
      expect(app.networkShortcutsLocked, isTrue);
      expect(app.busy, isTrue);
      await app.connectOrDisconnect();
      await app.retry();
      expect(engine.calls, isNot(contains('connect')));
      expect(engine.calls, isNot(contains('retry')));
      engine.attempts.single.complete(
        const EngineSnapshot(phase: ConnectionPhase.disconnected),
      );
      expect(await quitting, {'cleanup_complete': true});
    },
    variant: TargetPlatformVariant.only(TargetPlatform.macOS),
  );

  testWidgets(
    'Quit drains queued account saves before requesting cleanup',
    (tester) async {
      final engine = _QueuedWritesEngine();
      final app = AppController(engine);
      await app.initialize();
      addTearDown(app.dispose);
      app.renameProfile(app.activeProfileId, 'first');
      app.renameProfile(app.activeProfileId, 'second');
      final quitting = app.disconnectForExit();
      await tester.pump();
      expect(engine.renames, hasLength(1));
      engine.renames.first.complete();
      await tester.pump();
      expect(engine.renames, hasLength(2));
      engine.renames.last.complete();
      await tester.pump();
      expect(engine.attempts, hasLength(1));
      engine.attempts.single.complete(
        const EngineSnapshot(phase: ConnectionPhase.disconnected),
      );
      await quitting;
      expect(engine.operations, [
        'rename:first',
        'rename:second',
        'disconnect',
      ]);
      expect(engine.storedProfiles.single.name, 'second');
    },
    variant: TargetPlatformVariant.only(TargetPlatform.macOS),
  );

  testWidgets(
    'failed cleanup denies Quit and the next Quit retries the Engine',
    (tester) async {
      final engine = _CleanupEngine();
      final app = AppController(engine);
      await app.initialize();
      final bridge = PlatformShellBridge(app);
      addTearDown(() {
        bridge.dispose();
        app.dispose();
      });
      final first = _requestQuit(tester);
      final denied = expectLater(first, throwsA(isA<PlatformException>()));
      await tester.pump();
      engine.attempts.single.completeError(
        const EngineException(
          'DISCONNECT_CLEANUP_FAILED',
          'Permission denied.',
        ),
      );
      await denied;
      expect(app.connectionActionEnabled, isTrue);
      final second = _requestQuit(tester);
      await tester.pump();
      expect(engine.attempts, hasLength(2));
      engine.attempts.last.complete(
        const EngineSnapshot(phase: ConnectionPhase.disconnected),
      );
      expect(await second, {'cleanup_complete': true});
    },
    variant: TargetPlatformVariant.only(TargetPlatform.macOS),
  );

  testWidgets(
    'an accepted Disconnect without completed state cannot acknowledge Quit',
    (tester) async {
      final engine = _CleanupEngine();
      final app = AppController(engine);
      await app.initialize();
      final quitting = app.disconnectForExit();
      final denied = expectLater(
        quitting,
        throwsA(
          isA<EngineException>().having(
            (error) => error.code,
            'code',
            'DISCONNECT_CLEANUP_FAILED',
          ),
        ),
      );
      await tester.pump();
      engine.attempts.single.complete(
        const EngineSnapshot(phase: ConnectionPhase.disconnecting),
      );
      await denied;
      app.dispose();
    },
    variant: TargetPlatformVariant.only(TargetPlatform.macOS),
  );

  testWidgets('an empty RPC reply cannot manufacture a cleanup confirmation', (
    tester,
  ) async {
    final client = DesktopEngineClient.forTest(
      transport: DesktopEngineTransport.forTest(
        requestIdFactory: () => 'quit',
        exchange: (_) async => const ControlCodec().frame(
          (ControlPayloadWriter()..string(1, 'quit')).takeBytes(),
        ),
      ),
    );
    addTearDown(client.dispose);
    await expectLater(
      client.disconnect(),
      throwsA(
        isA<EngineException>().having(
          (error) => error.code,
          'code',
          'DISCONNECT_CLEANUP_UNCONFIRMED',
        ),
      ),
    );
  });

  testWidgets(
    'menu connection actions respect busy state and retain startup cancellation',
    (tester) async {
      final engine = _CleanupEngine();
      final app = AppController(engine);
      await app.initialize();
      app.busy = true;
      expect(app.connectionActionEnabled, isFalse);
      await app.connectOrDisconnect();
      expect(engine.attempts, isEmpty);
      expect(engine.calls, isNot(contains('connect')));

      app.snapshot = const EngineSnapshot(phase: ConnectionPhase.preparing);
      expect(app.connectionActionEnabled, isTrue);
      final cancelling = app.connectOrDisconnect();
      await tester.pump();
      expect(engine.attempts, hasLength(1));
      expect(app.connectionActionEnabled, isFalse);
      engine.attempts.single.complete(
        const EngineSnapshot(phase: ConnectionPhase.disconnected),
      );
      await cancelling;
      app.dispose();
    },
    variant: TargetPlatformVariant.only(TargetPlatform.macOS),
  );
}
